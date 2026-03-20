use std::sync::{mpsc, Arc, Mutex};

use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use omega_keymap::{InteractionMode, KeyAction, KeyContext, KeyResolution, KeymapManager};
use omega_session::{AgentSession, RuntimeUiEnvelope};
use tracing::info;

use crate::app::{App, MsgKind, Panel};
use crate::overlay::{ConfirmChoice, ConfirmIntent, OverlayState};

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
                Panel::SidebarRail => app_guard.focus_sidebar_rail(),
                Panel::Todo if app_guard.todo_visible() => app_guard.focused_panel = Panel::Todo,
                Panel::Logs if app_guard.logs_visible() => app_guard.focused_panel = Panel::Logs,
                _ => app_guard.focused_panel = Panel::Response,
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

#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
    use omega_client::{ChatRequest, ChatResponse, ClientError};
    use omega_core::{DynLlmClient, LlmClient};
    use omega_keymap::{InteractionMode, KeymapManager};
    use omega_workflow::WorkflowDefinition;

    use crate::app::Panel;

    use super::*;

    struct IdleClient;

    #[async_trait]
    impl LlmClient for IdleClient {
        async fn chat(&self, _request: ChatRequest) -> Result<ChatResponse, ClientError> {
            panic!("chat should not run in wait-message test");
        }

        fn provider_name(&self) -> &'static str {
            "idle"
        }
    }

    fn press_key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent {
            code,
            modifiers,
            kind: KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        }
    }

    fn test_session(
        client: DynLlmClient,
        root: std::path::PathBuf,
        runtime: &tokio::runtime::Runtime,
    ) -> AgentSession {
        AgentSession::new(omega_session::AgentSessionConfig {
            client,
            system: "system".to_string(),
            cwd: root,
            runtime_handle: runtime.handle().clone(),
            workflow_definition: WorkflowDefinition::default_linear(),
            workflow_prompts: omega_workflow::WorkflowPrompts::builtin_defaults(),
        })
        .unwrap()
    }

    #[test]
    fn submit_while_running_shows_wait_message() {
        let client: DynLlmClient = Arc::new(IdleClient);
        let root = std::env::temp_dir().join("omega-event-test");
        let _ = std::fs::create_dir_all(&root);
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let session = test_session(client, root, &runtime);
        let app = Arc::new(Mutex::new(App::new()));
        let (tx, _rx) = mpsc::channel();
        {
            let mut app_guard = app.lock().unwrap();
            app_guard.is_running = true;
            app_guard.input_buffer = "pending".to_string();
        }

        let should_quit = handle_submit(&app, &session, &tx).unwrap();
        let app_guard = app.lock().unwrap();

        assert!(!should_quit);
        assert_eq!(app_guard.output_msgs.len(), 1);
        assert_eq!(app_guard.output_msgs[0].kind, MsgKind::Error);
        assert!(app_guard.output_msgs[0]
            .text
            .contains("Previous turn still finishing"));
    }

    #[test]
    fn tab_keeps_focus_on_response_when_sidebar_is_hidden() {
        let client: DynLlmClient = Arc::new(IdleClient);
        let root = std::env::temp_dir().join("omega-event-tab-hidden-test");
        let _ = std::fs::create_dir_all(&root);
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let session = test_session(client, root, &runtime);
        let app = Arc::new(Mutex::new(App::new()));
        let (tx, _rx) = mpsc::channel();
        let keymap = KeymapManager::default();

        let should_quit = handle_key_event(
            press_key(KeyCode::Char(' '), KeyModifiers::NONE),
            &app,
            &session,
            &tx,
            &keymap,
        )
        .unwrap();
        assert!(!should_quit);
        let should_quit = handle_key_event(
            press_key(KeyCode::Tab, KeyModifiers::NONE),
            &app,
            &session,
            &tx,
            &keymap,
        )
        .unwrap();

        assert!(!should_quit);
        assert_eq!(app.lock().unwrap().focused_panel, Panel::Response);
    }

    #[test]
    fn raw_tab_does_not_change_focus_in_normal_mode() {
        let client: DynLlmClient = Arc::new(IdleClient);
        let root = std::env::temp_dir().join("omega-event-raw-tab-test");
        let _ = std::fs::create_dir_all(&root);
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let session = test_session(client, root, &runtime);
        let app = Arc::new(Mutex::new(App::new()));
        let (tx, _rx) = mpsc::channel();
        let keymap = KeymapManager::default();

        {
            let mut app_guard = app.lock().unwrap();
            app_guard.todo_rect = ratatui::layout::Rect::new(60, 1, 20, 8);
            app_guard.logs_rect = ratatui::layout::Rect::new(60, 9, 20, 8);
        }

        handle_key_event(
            press_key(KeyCode::Tab, KeyModifiers::NONE),
            &app,
            &session,
            &tx,
            &keymap,
        )
        .unwrap();

        assert_eq!(app.lock().unwrap().focused_panel, Panel::Response);
    }

    #[test]
    fn leader_jk_toggles_into_insert_mode_and_allows_typing() {
        let client: DynLlmClient = Arc::new(IdleClient);
        let root = std::env::temp_dir().join("omega-event-insert-mode-test");
        let _ = std::fs::create_dir_all(&root);
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let session = test_session(client, root, &runtime);
        let app = Arc::new(Mutex::new(App::new()));
        let (tx, _rx) = mpsc::channel();
        let keymap = KeymapManager::default();

        handle_key_event(
            press_key(KeyCode::Char(' '), KeyModifiers::NONE),
            &app,
            &session,
            &tx,
            &keymap,
        )
        .unwrap();
        handle_key_event(
            press_key(KeyCode::Char('j'), KeyModifiers::NONE),
            &app,
            &session,
            &tx,
            &keymap,
        )
        .unwrap();
        handle_key_event(
            press_key(KeyCode::Char('k'), KeyModifiers::NONE),
            &app,
            &session,
            &tx,
            &keymap,
        )
        .unwrap();
        handle_key_event(
            press_key(KeyCode::Char('h'), KeyModifiers::NONE),
            &app,
            &session,
            &tx,
            &keymap,
        )
        .unwrap();

        let app_guard = app.lock().unwrap();
        assert_eq!(app_guard.interaction_mode, InteractionMode::Insert);
        assert_eq!(app_guard.input_buffer, "h");
    }

    #[test]
    fn plain_text_is_ignored_in_normal_mode() {
        let client: DynLlmClient = Arc::new(IdleClient);
        let root = std::env::temp_dir().join("omega-event-normal-mode-test");
        let _ = std::fs::create_dir_all(&root);
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let session = test_session(client, root, &runtime);
        let app = Arc::new(Mutex::new(App::new()));
        let (tx, _rx) = mpsc::channel();
        let keymap = KeymapManager::default();

        handle_key_event(
            press_key(KeyCode::Char('h'), KeyModifiers::NONE),
            &app,
            &session,
            &tx,
            &keymap,
        )
        .unwrap();

        assert!(app.lock().unwrap().input_buffer.is_empty());
    }

    #[test]
    fn leader_jk_rejects_insert_mode_when_input_is_disabled() {
        let client: DynLlmClient = Arc::new(IdleClient);
        let root = std::env::temp_dir().join("omega-event-insert-disabled-test");
        let _ = std::fs::create_dir_all(&root);
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let session = test_session(client, root, &runtime);
        let app = Arc::new(Mutex::new(App::new()));
        let (tx, _rx) = mpsc::channel();
        let keymap = KeymapManager::default();
        app.lock().unwrap().input_enabled = false;

        handle_key_event(
            press_key(KeyCode::Char(' '), KeyModifiers::NONE),
            &app,
            &session,
            &tx,
            &keymap,
        )
        .unwrap();
        handle_key_event(
            press_key(KeyCode::Char('j'), KeyModifiers::NONE),
            &app,
            &session,
            &tx,
            &keymap,
        )
        .unwrap();
        handle_key_event(
            press_key(KeyCode::Char('k'), KeyModifiers::NONE),
            &app,
            &session,
            &tx,
            &keymap,
        )
        .unwrap();

        let app_guard = app.lock().unwrap();
        assert_eq!(app_guard.interaction_mode, InteractionMode::Normal);
        assert!(app_guard
            .status_notice
            .as_deref()
            .is_some_and(|notice| notice.contains("Insert mode")));
    }

    #[test]
    fn leader_jk_toggles_back_to_normal_mode() {
        let client: DynLlmClient = Arc::new(IdleClient);
        let root = std::env::temp_dir().join("omega-event-toggle-normal-test");
        let _ = std::fs::create_dir_all(&root);
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let session = test_session(client, root, &runtime);
        let app = Arc::new(Mutex::new(App::new()));
        let (tx, _rx) = mpsc::channel();
        let keymap = KeymapManager::default();
        app.lock().unwrap().interaction_mode = InteractionMode::Insert;

        handle_key_event(
            press_key(KeyCode::Char(' '), KeyModifiers::NONE),
            &app,
            &session,
            &tx,
            &keymap,
        )
        .unwrap();
        handle_key_event(
            press_key(KeyCode::Char('j'), KeyModifiers::NONE),
            &app,
            &session,
            &tx,
            &keymap,
        )
        .unwrap();
        handle_key_event(
            press_key(KeyCode::Char('k'), KeyModifiers::NONE),
            &app,
            &session,
            &tx,
            &keymap,
        )
        .unwrap();

        let app_guard = app.lock().unwrap();
        assert_eq!(app_guard.interaction_mode, InteractionMode::Normal);
        assert!(app_guard
            .status_notice
            .as_deref()
            .is_some_and(|notice| notice.contains("Mode: Normal")));
    }

    #[test]
    fn panel_search_overlay_captures_text_without_touching_main_input() {
        let client: DynLlmClient = Arc::new(IdleClient);
        let root = std::env::temp_dir().join("omega-event-overlay-search-test");
        let _ = std::fs::create_dir_all(&root);
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let session = test_session(client, root, &runtime);
        let app = Arc::new(Mutex::new(App::new()));
        let (tx, _rx) = mpsc::channel();
        let keymap = KeymapManager::default();

        handle_key_event(
            press_key(KeyCode::Char(' '), KeyModifiers::NONE),
            &app,
            &session,
            &tx,
            &keymap,
        )
        .unwrap();
        handle_key_event(
            press_key(KeyCode::Char('/'), KeyModifiers::NONE),
            &app,
            &session,
            &tx,
            &keymap,
        )
        .unwrap();
        handle_key_event(
            press_key(KeyCode::Char('a'), KeyModifiers::NONE),
            &app,
            &session,
            &tx,
            &keymap,
        )
        .unwrap();

        let app_guard = app.lock().unwrap();
        let query = match app_guard.overlay.as_ref() {
            Some(OverlayState::Search(overlay)) => overlay.query.as_str(),
            _ => "",
        };
        assert_eq!(query, "a");
        assert!(app_guard.input_buffer.is_empty());
    }

    #[test]
    fn overlay_esc_restores_previous_focus() {
        let client: DynLlmClient = Arc::new(IdleClient);
        let root = std::env::temp_dir().join("omega-event-overlay-escape-test");
        let _ = std::fs::create_dir_all(&root);
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let session = test_session(client, root, &runtime);
        let app = Arc::new(Mutex::new(App::new()));
        let (tx, _rx) = mpsc::channel();
        let keymap = KeymapManager::default();
        {
            let mut app_guard = app.lock().unwrap();
            app_guard.focused_panel = Panel::Logs;
            app_guard.logs_rect = ratatui::layout::Rect::new(60, 8, 20, 8);
        }

        handle_key_event(
            press_key(KeyCode::Char(' '), KeyModifiers::NONE),
            &app,
            &session,
            &tx,
            &keymap,
        )
        .unwrap();
        handle_key_event(
            press_key(KeyCode::Char('/'), KeyModifiers::NONE),
            &app,
            &session,
            &tx,
            &keymap,
        )
        .unwrap();
        handle_key_event(
            press_key(KeyCode::Esc, KeyModifiers::NONE),
            &app,
            &session,
            &tx,
            &keymap,
        )
        .unwrap();

        let app_guard = app.lock().unwrap();
        assert!(!app_guard.overlay_active());
        assert_eq!(app_guard.focused_panel, Panel::Logs);
    }

    #[test]
    fn overlay_blocks_background_tab_focus_changes() {
        let client: DynLlmClient = Arc::new(IdleClient);
        let root = std::env::temp_dir().join("omega-event-overlay-block-focus-test");
        let _ = std::fs::create_dir_all(&root);
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let session = test_session(client, root, &runtime);
        let app = Arc::new(Mutex::new(App::new()));
        let (tx, _rx) = mpsc::channel();
        let keymap = KeymapManager::default();
        {
            let mut app_guard = app.lock().unwrap();
            app_guard.todo_rect = ratatui::layout::Rect::new(60, 1, 20, 8);
            app_guard.logs_rect = ratatui::layout::Rect::new(60, 9, 20, 8);
        }

        handle_key_event(
            press_key(KeyCode::Char(' '), KeyModifiers::NONE),
            &app,
            &session,
            &tx,
            &keymap,
        )
        .unwrap();
        handle_key_event(
            press_key(KeyCode::Char('/'), KeyModifiers::NONE),
            &app,
            &session,
            &tx,
            &keymap,
        )
        .unwrap();
        handle_key_event(
            press_key(KeyCode::Tab, KeyModifiers::NONE),
            &app,
            &session,
            &tx,
            &keymap,
        )
        .unwrap();

        let app_guard = app.lock().unwrap();
        assert_eq!(app_guard.focused_panel, Panel::Response);
        assert!(app_guard.overlay_active());
    }

    #[test]
    fn leader_b_toggles_sidebar_shell() {
        let client: DynLlmClient = Arc::new(IdleClient);
        let root = std::env::temp_dir().join("omega-event-toggle-sidebar-test");
        let _ = std::fs::create_dir_all(&root);
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let session = test_session(client, root, &runtime);
        let app = Arc::new(Mutex::new(App::new()));
        let (tx, _rx) = mpsc::channel();
        let keymap = KeymapManager::default();

        handle_key_event(
            press_key(KeyCode::Char(' '), KeyModifiers::NONE),
            &app,
            &session,
            &tx,
            &keymap,
        )
        .unwrap();
        handle_key_event(
            press_key(KeyCode::Char('b'), KeyModifiers::NONE),
            &app,
            &session,
            &tx,
            &keymap,
        )
        .unwrap();

        let app_guard = app.lock().unwrap();
        assert!(app_guard.sidebar.shell_collapsed);
        assert_eq!(app_guard.focused_panel, Panel::Response);
    }

    #[test]
    fn sidebar_rail_cycles_and_toggles_selected_section() {
        let client: DynLlmClient = Arc::new(IdleClient);
        let root = std::env::temp_dir().join("omega-event-sidebar-rail-test");
        let _ = std::fs::create_dir_all(&root);
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let session = test_session(client, root, &runtime);
        let app = Arc::new(Mutex::new(App::new()));
        let (tx, _rx) = mpsc::channel();
        let keymap = KeymapManager::default();
        {
            let mut app_guard = app.lock().unwrap();
            app_guard.sidebar_rect = ratatui::layout::Rect::new(60, 1, 20, 18);
            app_guard.sidebar_rail_rect = ratatui::layout::Rect::new(61, 2, 18, 3);
            app_guard.todo_rect = ratatui::layout::Rect::new(61, 5, 18, 6);
            app_guard.logs_rect = ratatui::layout::Rect::new(61, 11, 18, 7);
            app_guard.focused_panel = Panel::SidebarRail;
        }

        handle_key_event(
            press_key(KeyCode::Right, KeyModifiers::NONE),
            &app,
            &session,
            &tx,
            &keymap,
        )
        .unwrap();
        handle_key_event(
            press_key(KeyCode::Char('x'), KeyModifiers::NONE),
            &app,
            &session,
            &tx,
            &keymap,
        )
        .unwrap();

        let app_guard = app.lock().unwrap();
        assert_eq!(
            app_guard.sidebar.rail_selection,
            crate::sidebar::SidebarSection::Logs
        );
        assert!(!app_guard.sidebar.logs_expanded);
        assert_eq!(app_guard.focused_panel, Panel::SidebarRail);
    }
}
