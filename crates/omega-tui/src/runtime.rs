use std::path::PathBuf;
use std::sync::{mpsc, Arc, Mutex};

use crossterm::event;
use omega_core::DynLlmClient;
use tokio::runtime::Handle;
use tracing::info;

use crate::agent_session::{AgentSession, LogUpdate};
use crate::app::App;
use crate::event::handle_event;
use crate::render::render;
use crate::terminal::TerminalGuard;

pub struct TuiLaunchConfig {
    pub client: DynLlmClient,
    pub cwd: PathBuf,
    pub model_name: String,
    pub runtime_handle: Handle,
    pub system: String,
    pub trace_rx: mpsc::Receiver<String>,
}

pub fn run(config: TuiLaunchConfig) -> anyhow::Result<()> {
    let TuiLaunchConfig {
        client,
        cwd,
        model_name,
        runtime_handle,
        system,
        trace_rx,
    } = config;

    info!("omega starting with multi-panel TUI");
    info!(model = %model_name, cwd = %cwd.display(), "tui config loaded");

    let session = AgentSession::new(client, system, cwd, runtime_handle)?;
    let app = Arc::new(Mutex::new(App::new()));
    let (tx, rx) = mpsc::channel::<LogUpdate>();
    let mut terminal = TerminalGuard::enter()?;
    let trace_rx = trace_rx;

    loop {
        for _ in 0..20 {
            if let Ok(update) = rx.try_recv() {
                app.lock().unwrap().apply_log_update(update);
            } else {
                break;
            }
        }

        for _ in 0..20 {
            if let Ok(line) = trace_rx.try_recv() {
                app.lock().unwrap().add_log(line);
            } else {
                break;
            }
        }

        {
            let mut app_guard = app.lock().unwrap();
            app_guard.spinner_tick = app_guard.spinner_tick.wrapping_add(1);
        }

        {
            let mut app_guard = app.lock().unwrap();
            terminal
                .terminal_mut()
                .draw(|frame| render(frame, &mut app_guard, &model_name))?;
        }

        if event::poll(std::time::Duration::from_millis(50))?
            && handle_event(event::read()?, &app, &session, &tx)?
        {
            break;
        }
    }

    terminal.restore()?;
    info!("omega exiting");
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::agent_session::LogUpdate;
    use crate::app::App;

    #[test]
    fn interrupt_turn_invalidates_old_updates() {
        let mut app = App::new();
        let turn_id = app.begin_turn();
        app.interrupt_turn();

        app.apply_log_update(LogUpdate::Output {
            turn_id,
            text: "stale".to_string(),
        });
        app.apply_log_update(LogUpdate::Done { turn_id });

        assert!(app.output_msgs.is_empty());
        assert!(!app.is_running);
    }

    #[test]
    fn current_turn_updates_are_applied() {
        let mut app = App::new();
        let turn_id = app.begin_turn();

        app.apply_log_update(LogUpdate::ToolLog {
            turn_id,
            log: "$ echo hi".to_string(),
        });
        app.apply_log_update(LogUpdate::Output {
            turn_id,
            text: "hello".to_string(),
        });
        app.apply_log_update(LogUpdate::Done { turn_id });

        assert_eq!(app.output_msgs.len(), 2);
        assert!(!app.is_running);
    }
}
