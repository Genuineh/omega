use std::sync::{mpsc, Arc, Mutex};

use crossterm::event;
use omega_keymap::KeymapManager;
use omega_session::{AgentSession, RuntimeUiEnvelope};
use omega_theme::OmegaTheme;
use tracing::info;

use crate::app::App;
use crate::event::handle_event;
use crate::render::render;
use crate::terminal::TerminalGuard;

pub struct TuiLaunchConfig {
    pub model_name: String,
    pub session: AgentSession,
    pub keymap: KeymapManager,
    pub theme: OmegaTheme,
    pub show_thinking: bool,
    pub keymap_source: String,
    pub startup_warnings: Vec<String>,
    pub trace_rx: mpsc::Receiver<String>,
}

pub fn run(config: TuiLaunchConfig) -> anyhow::Result<()> {
    let TuiLaunchConfig {
        model_name,
        session,
        keymap,
        theme,
        show_thinking,
        keymap_source,
        startup_warnings,
        trace_rx,
    } = config;

    info!("omega starting with multi-panel TUI");
    info!(model = %model_name, "tui config loaded");

    let app = Arc::new(Mutex::new(App::new()));
    {
        let mut app_guard = app.lock().unwrap();
        app_guard.set_show_thinking(show_thinking);
        app_guard.set_keymap_source(keymap_source);
        for warning in &startup_warnings {
            app_guard.add_log(warning.clone());
        }
        if !startup_warnings.is_empty() {
            app_guard.set_status_notice(startup_warnings.join(" | "));
        }
    }
    let (tx, rx) = mpsc::channel::<RuntimeUiEnvelope>();
    let mut terminal = TerminalGuard::enter()?;
    let trace_rx = trace_rx;

    loop {
        for _ in 0..20 {
            if let Ok(update) = rx.try_recv() {
                app.lock().unwrap().apply_runtime_envelope(update);
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
            app_guard.expire_leader_pending(keymap.leader_timeout());
            app_guard.spinner_tick = app_guard.spinner_tick.wrapping_add(1);
        }

        {
            let mut app_guard = app.lock().unwrap();
            terminal
                .terminal_mut()
                .draw(|frame| render(frame, &mut app_guard, &model_name, &theme))?;
        }

        if event::poll(std::time::Duration::from_millis(50))?
            && handle_event(event::read()?, &app, &session, &tx, &keymap)?
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
    use crate::app::App;
    use omega_session::{
        ActivityTarget, ResponseSection, ResponseSectionDelta, ResponseSectionKind,
        ResponseSectionMetadata, ResponseSectionState, RuntimeUiEffect, RuntimeUiEnvelope,
        RuntimeUiMessage, StatusSlot, StatusValue, UiContent, UiMessageKind, UiSource, UiTarget,
        WorkflowRunRole,
    };

    #[test]
    fn interrupt_turn_invalidates_old_updates() {
        let mut app = App::new();
        let turn_id = app.begin_turn();
        app.interrupt_turn();

        app.apply_runtime_envelope(RuntimeUiEnvelope::message(
            turn_id,
            RuntimeUiMessage {
                target: UiTarget::Response,
                source: UiSource::Assistant,
                kind: UiMessageKind::Result,
                content: UiContent::Text("stale".to_string()),
                priority: None,
            },
        ));
        app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
            turn_id,
            RuntimeUiEffect::SetStatusSlot {
                slot: StatusSlot::Agent,
                value: StatusValue::Label("Idle".to_string()),
            },
        ));

        assert!(app.output_msgs.is_empty());
        assert!(!app.is_running);
    }

    #[test]
    fn current_turn_updates_are_applied() {
        let mut app = App::new();
        let turn_id = app.begin_turn();

        app.apply_runtime_envelope(RuntimeUiEnvelope::message(
            turn_id,
            RuntimeUiMessage {
                target: UiTarget::Activity(ActivityTarget::Log),
                source: UiSource::Tool {
                    tool_name: "bash".to_string(),
                },
                kind: UiMessageKind::Log,
                content: UiContent::Text("$ echo hi".to_string()),
                priority: None,
            },
        ));
        app.apply_runtime_envelope(RuntimeUiEnvelope::message(
            turn_id,
            RuntimeUiMessage {
                target: UiTarget::Activity(ActivityTarget::Log),
                source: UiSource::Tool {
                    tool_name: "bash".to_string(),
                },
                kind: UiMessageKind::Log,
                content: UiContent::Text("hi".to_string()),
                priority: None,
            },
        ));
        app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
            turn_id,
            RuntimeUiEffect::ReplacePanel {
                target: UiTarget::Todo,
                content: UiContent::Text("[>] #1: Code".to_string()),
            },
        ));
        app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
            turn_id,
            RuntimeUiEffect::SetStatusSlot {
                slot: StatusSlot::Workflow,
                value: StatusValue::WorkflowStep {
                    workflow_id: "feature".to_string(),
                    workflow_role: WorkflowRunRole::Child,
                    step_id: "execute".to_string(),
                    step_label: "Execute".to_string(),
                    index: 3,
                    total: 4,
                },
            },
        ));
        app.apply_runtime_envelope(RuntimeUiEnvelope::message(
            turn_id,
            RuntimeUiMessage {
                target: UiTarget::Activity(ActivityTarget::Log),
                source: UiSource::WorkflowStep {
                    workflow_id: "feature".to_string(),
                    workflow_role: WorkflowRunRole::Child,
                    step_id: "execute".to_string(),
                    step_label: "Execute".to_string(),
                    index: 3,
                    total: 4,
                },
                kind: UiMessageKind::Summary,
                content: UiContent::Text("Execute".to_string()),
                priority: None,
            },
        ));
        app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
            turn_id,
            RuntimeUiEffect::BeginResponseSection {
                section: ResponseSection {
                    id: "turn-1:child:feature:plan".to_string(),
                    parent_id: None,
                    kind: ResponseSectionKind::Step,
                    title: "Plan".to_string(),
                    state: ResponseSectionState::Streaming,
                    metadata: ResponseSectionMetadata {
                        scene_id: Some("feature".to_string()),
                        workflow_id: "feature".to_string(),
                        workflow_role: WorkflowRunRole::Child,
                        step_id: Some("plan".to_string()),
                        step_label: Some("Plan".to_string()),
                    },
                },
            },
        ));
        app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
            turn_id,
            RuntimeUiEffect::AppendResponseSection {
                id: "turn-1:child:feature:plan".to_string(),
                delta: ResponseSectionDelta::Text("draft patch".to_string()),
            },
        ));
        app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
            turn_id,
            RuntimeUiEffect::CompleteResponseSection {
                id: "turn-1:child:feature:plan".to_string(),
                state: ResponseSectionState::Complete,
            },
        ));
        app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
            turn_id,
            RuntimeUiEffect::BeginResponseSection {
                section: ResponseSection {
                    id: "turn-1:child:feature:report".to_string(),
                    parent_id: None,
                    kind: ResponseSectionKind::FinalAnswer,
                    title: "Final Answer".to_string(),
                    state: ResponseSectionState::Streaming,
                    metadata: ResponseSectionMetadata {
                        scene_id: Some("feature".to_string()),
                        workflow_id: "feature".to_string(),
                        workflow_role: WorkflowRunRole::Child,
                        step_id: Some("report".to_string()),
                        step_label: Some("Report".to_string()),
                    },
                },
            },
        ));
        app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
            turn_id,
            RuntimeUiEffect::AppendResponseSection {
                id: "turn-1:child:feature:report".to_string(),
                delta: ResponseSectionDelta::Text("hello".to_string()),
            },
        ));
        app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
            turn_id,
            RuntimeUiEffect::CompleteResponseSection {
                id: "turn-1:child:feature:report".to_string(),
                state: ResponseSectionState::Complete,
            },
        ));
        app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
            turn_id,
            RuntimeUiEffect::ClearStatusSlot {
                slot: StatusSlot::Workflow,
            },
        ));
        app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
            turn_id,
            RuntimeUiEffect::SetStatusSlot {
                slot: StatusSlot::Agent,
                value: StatusValue::Label("Idle".to_string()),
            },
        ));

        assert_eq!(
            app.response_lines(),
            vec![
                "step  child:feature  Plan  [done]".to_string(),
                "  scene feature".to_string(),
                "  draft patch".to_string(),
                "final  child:feature  Final Answer  [done]".to_string(),
                "  scene feature".to_string(),
                "  hello".to_string(),
            ]
        );
        assert_eq!(app.todo_lines, vec!["[>] #1: Code"]);
        assert_eq!(
            app.log_lines,
            vec![
                "[tool] $ echo hi",
                "[tool] hi",
                "[child:feature 3/4] Execute (execute)",
            ]
        );
        assert!(app.workflow_summary.is_none());
        assert!(!app.is_running);
    }
}
