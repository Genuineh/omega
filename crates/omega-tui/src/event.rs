use std::cell::RefCell;
use std::sync::{mpsc, Arc, Mutex};

use arboard::Clipboard;
use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use omega_keymap::{InteractionMode, KeyAction, KeyContext, KeyResolution, KeymapManager};
use omega_session::{AgentSession, RuntimeUiEnvelope};
use tracing::info;

use crate::app::{App, MsgKind, Panel, ResponseActivation};
use crate::overlay::{ConfirmChoice, ConfirmIntent, OverlayState};

thread_local! {
    static PERSISTENT_CLIPBOARD: RefCell<Option<SystemClipboard>> = const { RefCell::new(None) };
}

trait ClipboardBackend {
    fn set_text(&mut self, text: &str) -> Result<(), String>;
}

struct SystemClipboard {
    inner: Clipboard,
}

impl SystemClipboard {
    fn new() -> Result<Self, String> {
        Clipboard::new()
            .map(|inner| Self { inner })
            .map_err(|error| error.to_string())
    }
}

impl ClipboardBackend for SystemClipboard {
    fn set_text(&mut self, text: &str) -> Result<(), String> {
        self.inner
            .set_text(text.to_string())
            .map_err(|error| error.to_string())
    }
}

pub fn handle_event(
    event: Event,
    app: &Arc<Mutex<App>>,
    session: &AgentSession,
    tx: &mpsc::Sender<RuntimeUiEnvelope>,
    keymap: &KeymapManager,
) -> anyhow::Result<bool> {
    match event {
        Event::Key(key) if key.kind == KeyEventKind::Press => {
            handle_key_event(key, app, session, tx, keymap)
        }
        Event::Mouse(mouse) => {
            handle_mouse_event(mouse, app);
            Ok(false)
        }
        _ => Ok(false),
    }
}

fn handle_key_event(
    key: KeyEvent,
    app: &Arc<Mutex<App>>,
    session: &AgentSession,
    tx: &mpsc::Sender<RuntimeUiEnvelope>,
    keymap: &KeymapManager,
) -> anyhow::Result<bool> {
    if app.lock().unwrap().overlay_active() {
        return handle_overlay_key_event(key, app, session);
    }

    {
        let mut app_guard = app.lock().unwrap();
        if app_guard.interaction_mode == InteractionMode::Normal && is_copy_shortcut(key) {
            if let Some(count) = copy_selected_text(&mut app_guard).map_err(anyhow::Error::msg)? {
                app_guard.set_status_notice(format!("Copied {} chars.", count));
                return Ok(false);
            }
        }
    }

    {
        let mut app_guard = app.lock().unwrap();
        if app_guard.interaction_mode == InteractionMode::Normal
            && app_guard.focused_panel == Panel::SidebarRail
        {
            match key.code {
                KeyCode::Left => {
                    app_guard.cycle_sidebar_rail_previous();
                    return Ok(false);
                }
                KeyCode::Right => {
                    app_guard.cycle_sidebar_rail_next();
                    return Ok(false);
                }
                KeyCode::Enter => {
                    app_guard.activate_sidebar_selection();
                    return Ok(false);
                }
                KeyCode::Char('x') => {
                    app_guard.toggle_selected_sidebar_section();
                    return Ok(false);
                }
                _ => {}
            }
        }
    }

    {
        let mut app_guard = app.lock().unwrap();
        if app_guard.interaction_mode == InteractionMode::Normal
            && app_guard.focused_panel == Panel::Response
        {
            match key.code {
                KeyCode::Enter | KeyCode::Char('x') => {
                    let notice = match app_guard.activate_selected_response_item() {
                        Some(ResponseActivation::ThinkingCollapsed) => "Thinking collapsed.",
                        Some(ResponseActivation::ThinkingExpanded) => "Thinking expanded.",
                        Some(ResponseActivation::ToolDetailOpened(tool_name)) => {
                            app_guard
                                .set_status_notice(format!("Opened {tool_name} detail overlay."));
                            return Ok(false);
                        }
                        None if app_guard.show_thinking => {
                            "Select a thinking block or tool summary before activating it."
                        }
                        None => "Thinking visibility is disabled in .omega/tui.toml.",
                    };
                    app_guard.set_status_notice(notice);
                    return Ok(false);
                }
                _ => {}
            }
        }
    }

    {
        let mut app_guard = app.lock().unwrap();
        if app_guard.interaction_mode == InteractionMode::Normal
            && app_guard.focused_panel == Panel::Diagnostics
        {
            match key.code {
                KeyCode::Enter | KeyCode::Char('x') => {
                    let notice = match app_guard.activate_selected_diagnostics_item() {
                        Some(step_label) => {
                            app_guard.set_status_notice(format!(
                                "Opened diagnostics overlay for {step_label}."
                            ));
                            return Ok(false);
                        }
                        None => "Select a diagnostics entry before activating it.",
                    };
                    app_guard.set_status_notice(notice);
                    return Ok(false);
                }
                _ => {}
            }
        }
    }

    let resolution = {
        let app_guard = app.lock().unwrap();
        let context = KeyContext {
            mode: app_guard.interaction_mode,
            focus: app_guard.key_focus(),
            input_capable: app_guard.input_capable(),
            leader_pending: app_guard.is_leader_pending(),
        };
        keymap.resolve_with_pending(&context, app_guard.pending_key_events(), key)
    };

    match resolution {
        KeyResolution::PendingLeader => {
            let mut app_guard = app.lock().unwrap();
            app_guard.begin_leader_pending(key);
            Ok(false)
        }
        KeyResolution::PendingSequence => {
            let mut app_guard = app.lock().unwrap();
            app_guard.extend_pending_sequence(key);
            Ok(false)
        }
        KeyResolution::Matched(action) => {
            {
                let mut app_guard = app.lock().unwrap();
                app_guard.clear_leader_pending();
                app_guard.clear_status_notice();
            }
            execute_action(action, app, session, tx)
        }
        KeyResolution::InvalidInContext(action) => {
            let mut app_guard = app.lock().unwrap();
            app_guard.clear_leader_pending();
            if action == KeyAction::EnterInsertMode {
                app_guard.set_status_notice("Insert mode is unavailable in the current context.");
            } else {
                app_guard.set_status_notice(format!(
                    "Action '{}' is unavailable in the current mode or focus.",
                    action.as_str()
                ));
            }
            Ok(false)
        }
        KeyResolution::NoMatch => handle_unmatched_key(key, app),
    }
}

fn handle_overlay_key_event(
    key: KeyEvent,
    app: &Arc<Mutex<App>>,
    session: &AgentSession,
) -> anyhow::Result<bool> {
    let mut app_guard = app.lock().unwrap();
    let Some(overlay) = app_guard.overlay.as_mut() else {
        return Ok(false);
    };

    match overlay {
        OverlayState::Search(overlay) => match key.code {
            KeyCode::Esc => {
                app_guard.close_overlay();
                app_guard.set_status_notice("Search popup closed.");
            }
            KeyCode::Left => {
                overlay.cursor_pos = overlay.cursor_pos.saturating_sub(1);
            }
            KeyCode::Right => {
                let count = overlay.query.chars().count();
                if overlay.cursor_pos < count {
                    overlay.cursor_pos += 1;
                }
            }
            KeyCode::Home => {
                overlay.cursor_pos = 0;
            }
            KeyCode::End => {
                overlay.cursor_pos = overlay.query.chars().count();
            }
            KeyCode::Backspace => {
                delete_char_before(&mut overlay.query, &mut overlay.cursor_pos);
            }
            KeyCode::Delete => {
                delete_char_at(&mut overlay.query, overlay.cursor_pos);
            }
            KeyCode::Enter => {
                let query = overlay.query.clone();
                let target_panel = overlay.target_panel;
                let count = if query.trim().is_empty() {
                    0
                } else {
                    let lowered = query.to_ascii_lowercase();
                    app_guard
                        .panel_lines(target_panel)
                        .into_iter()
                        .map(|line| line.to_ascii_lowercase().matches(&lowered).count())
                        .sum()
                };
                app_guard.set_status_notice(format!(
                    "Search popup captured '{}' with {count} match(es). Jump navigation lands in Task 15B-11.",
                    query
                ));
            }
            KeyCode::Char(c)
                if key.modifiers == KeyModifiers::NONE || key.modifiers == KeyModifiers::SHIFT =>
            {
                insert_char(&mut overlay.query, &mut overlay.cursor_pos, c);
            }
            _ => {}
        },
        OverlayState::Confirm(overlay) => match key.code {
            KeyCode::Esc => {
                app_guard.close_overlay();
                app_guard.set_status_notice("Interrupt request cancelled.");
            }
            KeyCode::Left => {
                overlay.selected = ConfirmChoice::Cancel;
            }
            KeyCode::Right | KeyCode::Tab => {
                overlay.selected = match overlay.selected {
                    ConfirmChoice::Cancel => ConfirmChoice::Confirm,
                    ConfirmChoice::Confirm => ConfirmChoice::Cancel,
                };
            }
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                overlay.selected = ConfirmChoice::Confirm;
            }
            KeyCode::Char('n') | KeyCode::Char('N') => {
                overlay.selected = ConfirmChoice::Cancel;
            }
            KeyCode::Enter => {
                let selected = overlay.selected;
                let intent = overlay.intent.clone();
                if selected == ConfirmChoice::Confirm {
                    drop(app_guard);
                    return confirm_overlay_intent(intent, app, session);
                }

                app_guard.close_overlay();
                app_guard.set_status_notice("Interrupt request cancelled.");
            }
            _ => {}
        },
        OverlayState::Detail(overlay) => match key.code {
            KeyCode::Esc => {
                app_guard.close_overlay();
            }
            KeyCode::Up => {
                overlay.scroll = overlay.scroll.saturating_sub(1);
            }
            KeyCode::Down => {
                overlay.scroll = overlay.scroll.saturating_add(1);
            }
            _ => {}
        },
        OverlayState::Picker(overlay) => match key.code {
            KeyCode::Esc => {
                app_guard.close_overlay();
            }
            KeyCode::Up => {
                overlay.selected = overlay.selected.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Tab => {
                if !overlay.items.is_empty() {
                    overlay.selected = (overlay.selected + 1).min(overlay.items.len() - 1);
                }
            }
            KeyCode::Enter => {
                let selection = overlay.items.get(overlay.selected).cloned();
                app_guard.close_overlay();
                if let Some(selection) = selection {
                    app_guard.set_status_notice(format!("Picker selected '{selection}'."));
                }
            }
            _ => {}
        },
        OverlayState::InputPrompt(overlay) => match key.code {
            KeyCode::Esc => {
                app_guard.close_overlay();
            }
            KeyCode::Left => {
                overlay.cursor_pos = overlay.cursor_pos.saturating_sub(1);
            }
            KeyCode::Right => {
                let count = overlay.value.chars().count();
                if overlay.cursor_pos < count {
                    overlay.cursor_pos += 1;
                }
            }
            KeyCode::Home => {
                overlay.cursor_pos = 0;
            }
            KeyCode::End => {
                overlay.cursor_pos = overlay.value.chars().count();
            }
            KeyCode::Backspace => {
                delete_char_before(&mut overlay.value, &mut overlay.cursor_pos);
            }
            KeyCode::Delete => {
                delete_char_at(&mut overlay.value, overlay.cursor_pos);
            }
            KeyCode::Enter => {
                let value = overlay.value.clone();
                app_guard.close_overlay();
                app_guard.set_status_notice(format!("Input prompt submitted '{value}'."));
            }
            KeyCode::Char(c)
                if key.modifiers == KeyModifiers::NONE || key.modifiers == KeyModifiers::SHIFT =>
            {
                insert_char(&mut overlay.value, &mut overlay.cursor_pos, c);
            }
            _ => {}
        },
    }

    Ok(false)
}

fn confirm_overlay_intent(
    intent: ConfirmIntent,
    app: &Arc<Mutex<App>>,
    session: &AgentSession,
) -> anyhow::Result<bool> {
    match intent {
        ConfirmIntent::InterruptTurn { turn_id } => {
            let mut app_guard = app.lock().unwrap();
            if app_guard.is_running && app_guard.is_current_turn(turn_id) {
                app_guard.interrupt_turn();
                app_guard.push_msg(MsgKind::Error, "⚠ Interrupted");
                app_guard.close_overlay();
                app_guard.set_status_notice("Running turn interrupted.");
                drop(app_guard);
                info!(turn_id, "user interrupted running task via overlay confirm");
                session.interrupt(turn_id)?;
            } else {
                app_guard.close_overlay();
                app_guard.set_status_notice("Running turn already finished.");
            }
            Ok(false)
        }
    }
}

fn insert_char(buffer: &mut String, cursor_pos: &mut usize, c: char) {
    let byte_pos = cursor_byte_pos(buffer, *cursor_pos);
    buffer.insert(byte_pos, c);
    *cursor_pos += 1;
}

fn delete_char_before(buffer: &mut String, cursor_pos: &mut usize) {
    if *cursor_pos > 0 {
        *cursor_pos -= 1;
        let byte_pos = cursor_byte_pos(buffer, *cursor_pos);
        buffer.remove(byte_pos);
    }
}

fn delete_char_at(buffer: &mut String, cursor_pos: usize) {
    if cursor_pos < buffer.chars().count() {
        let byte_pos = cursor_byte_pos(buffer, cursor_pos);
        buffer.remove(byte_pos);
    }
}

fn cursor_byte_pos(buffer: &str, cursor_pos: usize) -> usize {
    buffer
        .char_indices()
        .nth(cursor_pos)
        .map(|(index, _)| index)
        .unwrap_or(buffer.len())
}

fn handle_unmatched_key(key: KeyEvent, app: &Arc<Mutex<App>>) -> anyhow::Result<bool> {
    let mut app_guard = app.lock().unwrap();
    if app_guard.is_leader_pending() {
        if app_guard.interaction_mode == InteractionMode::Normal {
            match key.code {
                KeyCode::Char('/') => {
                    app_guard.clear_leader_pending();
                    app_guard.open_search_overlay();
                    return Ok(false);
                }
                KeyCode::Char('b') => {
                    app_guard.clear_leader_pending();
                    app_guard.toggle_sidebar_shell();
                    let notice = if app_guard.sidebar.shell_collapsed {
                        "Sidebar collapsed."
                    } else {
                        "Sidebar expanded."
                    };
                    app_guard.set_status_notice(notice);
                    return Ok(false);
                }
                _ => {}
            }
        }

        app_guard.clear_leader_pending();
        app_guard.set_status_notice("No mapping for that leader sequence.");
        return Ok(false);
    }

    if app_guard.interaction_mode != InteractionMode::Insert {
        return Ok(false);
    }

    if key.modifiers == KeyModifiers::NONE || key.modifiers == KeyModifiers::SHIFT {
        if let KeyCode::Char(c) = key.code {
            app_guard.insert_char(c);
        }
    }

    Ok(false)
}

fn execute_action(
    action: KeyAction,
    app: &Arc<Mutex<App>>,
    session: &AgentSession,
    tx: &mpsc::Sender<RuntimeUiEnvelope>,
) -> anyhow::Result<bool> {
    match action {
        KeyAction::Quit => {
            info!("user exit via keymap");
            Ok(true)
        }
        KeyAction::InterruptTurn => {
            let mut app_guard = app.lock().unwrap();
            if app_guard.is_running {
                let turn_id = app_guard.active_turn_id;
                app_guard.open_interrupt_confirm_overlay(turn_id);
            } else {
                app_guard.set_status_notice("No running turn to interrupt.");
            }
            Ok(false)
        }
        KeyAction::EnterNormalMode => {
            let mut app_guard = app.lock().unwrap();
            app_guard.enter_normal_mode();
            app_guard.set_status_notice("Mode: Normal");
            Ok(false)
        }
        KeyAction::EnterInsertMode => {
            let mut app_guard = app.lock().unwrap();
            if app_guard.enter_insert_mode() {
                app_guard.set_status_notice("Mode: Insert");
            } else {
                app_guard.set_status_notice("Insert mode is unavailable in the current context.");
            }
            Ok(false)
        }
        KeyAction::ToggleInteractionMode => {
            let mut app_guard = app.lock().unwrap();
            if app_guard.interaction_mode == InteractionMode::Insert {
                app_guard.enter_normal_mode();
                app_guard.set_status_notice("Mode: Normal");
            } else if app_guard.enter_insert_mode() {
                app_guard.set_status_notice("Mode: Insert");
            } else {
                app_guard.set_status_notice("Insert mode is unavailable in the current context.");
            }
            Ok(false)
        }
        KeyAction::FocusNextPanel => {
            let mut app_guard = app.lock().unwrap();
            app_guard.focused_panel = app_guard.next_focus_panel();
            Ok(false)
        }
        KeyAction::ScrollPanelUp => {
            let mut app_guard = app.lock().unwrap();
            let panel = app_guard.focused_panel;
            app_guard.scroll_panel_up(panel, 3);
            Ok(false)
        }
        KeyAction::ScrollPanelDown => {
            let mut app_guard = app.lock().unwrap();
            let panel = app_guard.focused_panel;
            app_guard.scroll_panel_down(panel, 3);
            Ok(false)
        }
        KeyAction::MoveCursorLeft => {
            app.lock().unwrap().move_cursor_left();
            Ok(false)
        }
        KeyAction::MoveCursorRight => {
            app.lock().unwrap().move_cursor_right();
            Ok(false)
        }
        KeyAction::MoveCursorHome => {
            app.lock().unwrap().move_cursor_home();
            Ok(false)
        }
        KeyAction::MoveCursorEnd => {
            app.lock().unwrap().move_cursor_end();
            Ok(false)
        }
        KeyAction::DeleteCharAt => {
            app.lock().unwrap().delete_char_at();
            Ok(false)
        }
        KeyAction::DeleteCharBefore => {
            app.lock().unwrap().delete_char_before();
            Ok(false)
        }
        KeyAction::SubmitInput => handle_submit(app, session, tx),
        KeyAction::CancelPendingSequence => {
            let mut app_guard = app.lock().unwrap();
            app_guard.clear_leader_pending();
            app_guard.set_status_notice("Leader sequence cancelled.");
            Ok(false)
        }
        KeyAction::PanelSearch => {
            let mut app_guard = app.lock().unwrap();
            app_guard.open_search_overlay();
            Ok(false)
        }
        KeyAction::ToggleSidebar => {
            let mut app_guard = app.lock().unwrap();
            app_guard.toggle_sidebar_shell();
            let notice = if app_guard.sidebar.shell_collapsed {
                "Sidebar collapsed."
            } else {
                "Sidebar expanded."
            };
            app_guard.set_status_notice(notice);
            Ok(false)
        }
        KeyAction::HistoryPrevious
        | KeyAction::HistoryNext
        | KeyAction::ResizeSidebarWider
        | KeyAction::ResizeSidebarNarrower => {
            app.lock().unwrap().set_status_notice(format!(
                "Action '{}' is reserved for a later task.",
                action.as_str()
            ));
            Ok(false)
        }
    }
}

fn handle_submit(
    app: &Arc<Mutex<App>>,
    session: &AgentSession,
    tx: &mpsc::Sender<RuntimeUiEnvelope>,
) -> anyhow::Result<bool> {
    let agent_ready = session.is_ready();
    let still_running = app.lock().unwrap().is_running;
    if !agent_ready || still_running {
        app.lock().unwrap().push_msg(
            MsgKind::Error,
            "⚠ Previous turn still finishing — please wait…",
        );
        return Ok(false);
    }

    let input = {
        let mut app_guard = app.lock().unwrap();
        app_guard.take_input()
    };

    if input == "q" || input == "exit" {
        info!("user exit");
        return Ok(true);
    }

    if input.is_empty() {
        return Ok(false);
    }

    session.checkpoint_current_messages();
    let turn_id = {
        let mut app_guard = app.lock().unwrap();
        if !app_guard.output_msgs.is_empty() {
            app_guard.push_msg(MsgKind::Separator, &"─".repeat(40));
        }
        app_guard.push_msg(MsgKind::User, &format!("> {}", input));
        app_guard.begin_turn()
    };

    if let Err(e) = session.spawn_turn(input, turn_id, tx.clone()) {
        let mut app_guard = app.lock().unwrap();
        app_guard.is_running = false;
        app_guard.push_msg(MsgKind::Error, &format!("Error: {e}"));
    }

    Ok(false)
}

fn handle_mouse_event(mouse: MouseEvent, app: &Arc<Mutex<App>>) {
    let mut app_guard = app.lock().unwrap();
    if app_guard.overlay_active() {
        let inside_overlay = mouse.column >= app_guard.overlay_rect.x
            && mouse.column
                < app_guard
                    .overlay_rect
                    .x
                    .saturating_add(app_guard.overlay_rect.width)
            && mouse.row >= app_guard.overlay_rect.y
            && mouse.row
                < app_guard
                    .overlay_rect
                    .y
                    .saturating_add(app_guard.overlay_rect.height);

        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) if !inside_overlay => {
                if app_guard
                    .overlay
                    .as_ref()
                    .is_some_and(|overlay| overlay.dismiss_on_backdrop())
                {
                    app_guard.close_overlay();
                    app_guard.set_status_notice("Overlay closed.");
                }
            }
            _ => {}
        }
        return;
    }

    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            let panel = app_guard.panel_at(mouse.column, mouse.row);
            match panel {
                Panel::SidebarRail => {
                    app_guard.clear_text_selection();
                    app_guard.focus_sidebar_rail();
                }
                Panel::Diagnostics if app_guard.diagnostics_visible() => {
                    app_guard.focused_panel = Panel::Diagnostics;
                    app_guard.begin_mouse_selection(Panel::Diagnostics, mouse.column, mouse.row);
                }
                Panel::Todo if app_guard.todo_visible() => {
                    app_guard.focused_panel = Panel::Todo;
                    app_guard.begin_mouse_selection(Panel::Todo, mouse.column, mouse.row);
                }
                Panel::Logs if app_guard.logs_visible() => {
                    app_guard.focused_panel = Panel::Logs;
                    app_guard.begin_mouse_selection(Panel::Logs, mouse.column, mouse.row);
                }
                _ => {
                    app_guard.focused_panel = Panel::Response;
                    app_guard.begin_mouse_selection(Panel::Response, mouse.column, mouse.row);
                }
            }
        }
        MouseEventKind::Drag(MouseButton::Left) => {
            app_guard.update_mouse_selection(mouse.column, mouse.row);
        }
        MouseEventKind::Up(MouseButton::Left) => {
            if let Some(text) = app_guard.finish_mouse_selection(mouse.column, mouse.row) {
                app_guard.set_status_notice(format!(
                    "Selected {} chars. Press y or Ctrl+C to copy.",
                    text.chars().count()
                ));
            }
        }
        MouseEventKind::ScrollUp => {
            let panel = app_guard.panel_at(mouse.column, mouse.row);
            app_guard.scroll_panel_up(panel, 3);
        }
        MouseEventKind::ScrollDown => {
            let panel = app_guard.panel_at(mouse.column, mouse.row);
            app_guard.scroll_panel_down(panel, 3);
        }
        _ => {}
    }
}

fn copy_selected_text(app: &mut App) -> Result<Option<usize>, String> {
    PERSISTENT_CLIPBOARD.with(|clipboard| {
        let mut clipboard = clipboard.borrow_mut();
        copy_selected_text_with_backend(app, &mut clipboard, SystemClipboard::new)
    })
}

fn copy_selected_text_with_backend<B, F>(
    app: &mut App,
    backend: &mut Option<B>,
    init: F,
) -> Result<Option<usize>, String>
where
    B: ClipboardBackend,
    F: Fn() -> Result<B, String>,
{
    let Some(text) = app.selected_text() else {
        return Ok(None);
    };
    let count = text.chars().count();
    write_text_with_backend(backend, &text, init)?;
    Ok(Some(count))
}

fn is_copy_shortcut(key: KeyEvent) -> bool {
    matches!(key.code, KeyCode::Char('y') | KeyCode::Char('Y'))
        && matches!(key.modifiers, KeyModifiers::NONE | KeyModifiers::SHIFT)
        || matches!(key.code, KeyCode::Char('c')) && key.modifiers == KeyModifiers::CONTROL
}

fn write_text_with_backend<B, F>(backend: &mut Option<B>, text: &str, init: F) -> Result<(), String>
where
    B: ClipboardBackend,
    F: Fn() -> Result<B, String>,
{
    if backend.is_none() {
        *backend = Some(init()?);
    }

    if let Some(clipboard) = backend.as_mut() {
        if clipboard.set_text(text).is_ok() {
            return Ok(());
        }
    }

    *backend = Some(init()?);
    backend
        .as_mut()
        .ok_or_else(|| "clipboard backend is unavailable".to_string())?
        .set_text(text)
}

#[cfg(test)]
#[path = "event_tests.rs"]
mod tests;
