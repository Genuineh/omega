use std::sync::{mpsc, Arc, Mutex};

use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEvent, MouseEventKind,
};
use tracing::info;

use crate::agent_session::{AgentSession, LogUpdate};
use crate::app::{App, MsgKind, Panel};

pub fn handle_event(
    event: Event,
    app: &Arc<Mutex<App>>,
    session: &AgentSession,
    tx: &mpsc::Sender<LogUpdate>,
) -> anyhow::Result<bool> {
    match event {
        Event::Key(key) if key.kind == KeyEventKind::Press => {
            handle_key_event(key, app, session, tx)
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
    tx: &mpsc::Sender<LogUpdate>,
) -> anyhow::Result<bool> {
    match key.code {
        KeyCode::Char(c) if key.modifiers == KeyModifiers::CONTROL => match c {
            'q' => {
                info!("user exit via Ctrl+Q");
                Ok(true)
            }
            'c' => {
                let mut app_guard = app.lock().unwrap();
                if app_guard.is_running {
                    app_guard.interrupt_turn();
                    let turn_id = app_guard.active_turn_id;
                    app_guard.push_msg(MsgKind::Error, "⚠ Interrupted");
                    drop(app_guard);
                    info!("user interrupted running task via Ctrl+C");
                    session.interrupt(turn_id)?;
                }
                Ok(false)
            }
            _ => Ok(false),
        },
        KeyCode::Tab => {
            let mut app_guard = app.lock().unwrap();
            app_guard.focused_panel = match app_guard.focused_panel {
                Panel::Response => Panel::Logs,
                Panel::Logs => Panel::Response,
            };
            Ok(false)
        }
        KeyCode::Up => {
            let mut app_guard = app.lock().unwrap();
            let panel = app_guard.focused_panel;
            app_guard.scroll_panel_up(panel, 3);
            Ok(false)
        }
        KeyCode::Down => {
            let mut app_guard = app.lock().unwrap();
            let panel = app_guard.focused_panel;
            app_guard.scroll_panel_down(panel, 3);
            Ok(false)
        }
        KeyCode::Left => {
            app.lock().unwrap().move_cursor_left();
            Ok(false)
        }
        KeyCode::Right => {
            app.lock().unwrap().move_cursor_right();
            Ok(false)
        }
        KeyCode::Home => {
            app.lock().unwrap().move_cursor_home();
            Ok(false)
        }
        KeyCode::End => {
            app.lock().unwrap().move_cursor_end();
            Ok(false)
        }
        KeyCode::Delete => {
            app.lock().unwrap().delete_char_at();
            Ok(false)
        }
        KeyCode::Char(c) => {
            app.lock().unwrap().insert_char(c);
            Ok(false)
        }
        KeyCode::Backspace => {
            app.lock().unwrap().delete_char_before();
            Ok(false)
        }
        KeyCode::Enter => handle_submit(app, session, tx),
        _ => Ok(false),
    }
}

fn handle_submit(
    app: &Arc<Mutex<App>>,
    session: &AgentSession,
    tx: &mpsc::Sender<LogUpdate>,
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
            let panel = app_guard.panel_at(mouse.column);
            app_guard.scroll_panel_up(panel, 3);
        }
        MouseEventKind::ScrollDown => {
            let panel = app_guard.panel_at(mouse.column);
            app_guard.scroll_panel_down(panel, 3);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use async_trait::async_trait;
    use omega_client::{ChatRequest, ChatResponse, ClientError};
    use omega_core::{DynLlmClient, LlmClient};

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

    #[test]
    fn submit_while_running_shows_wait_message() {
        let client: DynLlmClient = Arc::new(IdleClient);
        let root = PathBuf::from(std::env::temp_dir().join("omega-event-test"));
        let _ = std::fs::create_dir_all(&root);
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let session =
            AgentSession::new(client, "system".to_string(), root, runtime.handle().clone())
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
}
