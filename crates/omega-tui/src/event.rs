use std::sync::{mpsc, Arc, Mutex};

use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEvent, MouseEventKind,
};
use omega_keymap::{InteractionMode, KeyAction, KeyContext, KeyResolution, KeymapManager};
use omega_session::{AgentSession, SessionUpdate};
use tracing::info;

use crate::app::{App, MsgKind};

pub fn handle_event(
    event: Event,
    app: &Arc<Mutex<App>>,
    session: &AgentSession,
    tx: &mpsc::Sender<SessionUpdate>,
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
    tx: &mpsc::Sender<SessionUpdate>,
    keymap: &KeymapManager,
) -> anyhow::Result<bool> {
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

fn handle_unmatched_key(key: KeyEvent, app: &Arc<Mutex<App>>) -> anyhow::Result<bool> {
    let mut app_guard = app.lock().unwrap();
    if app_guard.is_leader_pending() {
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
    tx: &mpsc::Sender<SessionUpdate>,
) -> anyhow::Result<bool> {
    match action {
        KeyAction::Quit => {
            info!("user exit via keymap");
            Ok(true)
        }
        KeyAction::InterruptTurn => {
            let mut app_guard = app.lock().unwrap();
            if app_guard.is_running {
                app_guard.interrupt_turn();
                let turn_id = app_guard.active_turn_id;
                app_guard.push_msg(MsgKind::Error, "⚠ Interrupted");
                drop(app_guard);
                info!("user interrupted running task via keymap");
                session.interrupt(turn_id)?;
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
        KeyAction::ToggleSidebar
        | KeyAction::PanelSearch
        | KeyAction::HistoryPrevious
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
    tx: &mpsc::Sender<SessionUpdate>,
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
    match mouse.kind {
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

    #[test]
    fn submit_while_running_shows_wait_message() {
        let client: DynLlmClient = Arc::new(IdleClient);
        let root = std::env::temp_dir().join("omega-event-test");
        let _ = std::fs::create_dir_all(&root);
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let session = AgentSession::new(omega_session::AgentSessionConfig {
            client,
            system: "system".to_string(),
            cwd: root,
            runtime_handle: runtime.handle().clone(),
        })
        .unwrap();
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
        let session = AgentSession::new(omega_session::AgentSessionConfig {
            client,
            system: "system".to_string(),
            cwd: root,
            runtime_handle: runtime.handle().clone(),
        })
        .unwrap();
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
        let session = AgentSession::new(omega_session::AgentSessionConfig {
            client,
            system: "system".to_string(),
            cwd: root,
            runtime_handle: runtime.handle().clone(),
        })
        .unwrap();
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
        let session = AgentSession::new(omega_session::AgentSessionConfig {
            client,
            system: "system".to_string(),
            cwd: root,
            runtime_handle: runtime.handle().clone(),
        })
        .unwrap();
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
        let session = AgentSession::new(omega_session::AgentSessionConfig {
            client,
            system: "system".to_string(),
            cwd: root,
            runtime_handle: runtime.handle().clone(),
        })
        .unwrap();
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
        let session = AgentSession::new(omega_session::AgentSessionConfig {
            client,
            system: "system".to_string(),
            cwd: root,
            runtime_handle: runtime.handle().clone(),
        })
        .unwrap();
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
        let session = AgentSession::new(omega_session::AgentSessionConfig {
            client,
            system: "system".to_string(),
            cwd: root,
            runtime_handle: runtime.handle().clone(),
        })
        .unwrap();
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
}
