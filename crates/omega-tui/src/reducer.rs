use omega_session::{
    ActivityTarget, OverlayRequest, OverlayTarget, ResponseSectionDelta, RuntimeUiEffect,
    RuntimeUiEnvelope, RuntimeUiMessage, StatusValue, UiContent, UiMessageKind, UiSource, UiTarget,
    WorkflowRunRole,
};

use crate::app::{
    App, MsgKind, Panel, SessionRoutingSummary, SessionStatusSummary, WorkflowSummary,
};

pub struct TuiUpdateReducer;

impl TuiUpdateReducer {
    pub fn apply(app: &mut App, envelope: RuntimeUiEnvelope) {
        match envelope {
            RuntimeUiEnvelope::Message { turn_id, message } if app.is_current_turn(turn_id) => {
                Self::apply_message(app, message);
            }
            RuntimeUiEnvelope::Effect { turn_id, effect } if app.is_current_turn(turn_id) => {
                Self::apply_effect(app, turn_id, effect);
            }
            _ => {}
        }
    }

    fn apply_message(app: &mut App, message: RuntimeUiMessage) {
        match message.target {
            UiTarget::Response => Self::apply_response_message(app, message),
            UiTarget::Activity(ActivityTarget::Log) => Self::apply_log_message(app, message),
            UiTarget::Todo => {
                let UiContent::Text(text) = message.content;
                app.set_todo_snapshot(app.active_turn_id, &text);
            }
            UiTarget::StatusBar(slot) => {
                let UiContent::Text(text) = message.content;
                app.set_status_slot(slot, StatusValue::Label(text));
            }
            UiTarget::Overlay(target) => Self::show_overlay_message(app, target, message),
        }
    }

    fn apply_response_message(app: &mut App, message: RuntimeUiMessage) {
        let text = message.content.as_text();
        match (&message.source, message.kind) {
            (UiSource::WorkflowStep { .. }, UiMessageKind::Narrative | UiMessageKind::Result) => {}
            (UiSource::Assistant, UiMessageKind::Result | UiMessageKind::Narrative) => {}
            (_, UiMessageKind::Error) => app.push_msg(MsgKind::Error, text),
            _ => app.push_msg(MsgKind::Agent, text),
        }
    }

    fn apply_log_message(app: &mut App, message: RuntimeUiMessage) {
        let text = message.content.as_text();
        match (&message.source, message.kind) {
            (UiSource::Tool { .. }, UiMessageKind::Log) => {
                app.add_log(format!("[tool] {}", text));
            }
            (
                UiSource::WorkflowStep {
                    workflow_id,
                    workflow_role,
                    step_id,
                    step_label,
                    index,
                    total,
                },
                UiMessageKind::Summary,
            ) => {
                app.add_log(format!(
                    "[{}:{} {}/{}] {} ({})",
                    workflow_role.as_str(),
                    workflow_id,
                    index,
                    total,
                    step_label,
                    step_id
                ));
            }
            (UiSource::SessionRouting, UiMessageKind::Summary | UiMessageKind::Warning) => {
                app.add_log(format!("[route] {}", text));
            }
            _ => app.add_log(text.to_string()),
        }
    }

    fn apply_effect(app: &mut App, turn_id: u64, effect: RuntimeUiEffect) {
        match effect {
            RuntimeUiEffect::SetStatusSlot { slot, value } => app.set_status_slot(slot, value),
            RuntimeUiEffect::ClearStatusSlot { slot } => app.clear_status_slot(slot),
            RuntimeUiEffect::ReplacePanel { target, content } => {
                Self::replace_panel(app, turn_id, target, content)
            }
            RuntimeUiEffect::ShowOverlay(request) => Self::show_overlay_request(app, request),
            RuntimeUiEffect::HideOverlay { target } => app.hide_overlay_target(target),
            RuntimeUiEffect::FocusHint { target } => Self::apply_focus_hint(app, target),
            RuntimeUiEffect::BeginResponseSection { section } => {
                app.begin_response_section(section)
            }
            RuntimeUiEffect::AppendResponseSection { id, delta } => {
                Self::append_response_section(app, &id, delta)
            }
            RuntimeUiEffect::CompleteResponseSection { id, state } => {
                app.complete_response_section(&id, state)
            }
            RuntimeUiEffect::BeginToolRun { tool_run } => app.begin_tool_run(tool_run),
            RuntimeUiEffect::UpdateToolRun { tool_run } => app.update_tool_run(tool_run),
            RuntimeUiEffect::CompleteToolRun { id, status } => app.complete_tool_run(&id, status),
            RuntimeUiEffect::UpsertStepDiagnostics { diagnostics } => {
                app.upsert_step_diagnostics(*diagnostics)
            }
            RuntimeUiEffect::UpsertStepSubflow { subflow } => app.upsert_step_subflow(subflow),
        }
    }

    fn append_response_section(app: &mut App, id: &str, delta: ResponseSectionDelta) {
        match delta {
            ResponseSectionDelta::Text(text) => app.append_response_section(id, &text),
        }
    }

    fn replace_panel(app: &mut App, turn_id: u64, target: UiTarget, content: UiContent) {
        match (target, content) {
            (UiTarget::Todo, UiContent::Text(text)) => app.set_todo_snapshot(turn_id, &text),
            (UiTarget::StatusBar(slot), UiContent::Text(text)) => {
                app.set_status_slot(slot, StatusValue::Label(text));
            }
            (UiTarget::Overlay(target), content) => {
                Self::show_overlay_request(app, OverlayRequest { target, content });
            }
            _ => {}
        }
    }

    fn show_overlay_message(app: &mut App, target: OverlayTarget, message: RuntimeUiMessage) {
        Self::show_overlay_request(
            app,
            OverlayRequest {
                target,
                content: message.content,
            },
        );
    }

    fn show_overlay_request(app: &mut App, request: OverlayRequest) {
        match request {
            OverlayRequest {
                target: OverlayTarget::Search,
                ..
            } => app.open_search_overlay(),
            OverlayRequest {
                target: OverlayTarget::Detail,
                content: UiContent::Text(text),
            } => app.open_detail_overlay(
                " Runtime Detail ",
                text.lines().map(str::to_string).collect(),
            ),
            OverlayRequest {
                target: OverlayTarget::Picker,
                content: UiContent::Text(text),
            } => app.open_picker_overlay(
                " Runtime Picker ",
                text.lines().map(str::to_string).collect(),
            ),
            OverlayRequest {
                target: OverlayTarget::InputPrompt,
                content: UiContent::Text(text),
            } => app.open_input_prompt_overlay(" Runtime Input ", text),
            OverlayRequest {
                target: OverlayTarget::Confirm,
                ..
            } => {}
        }
    }

    fn apply_focus_hint(app: &mut App, target: UiTarget) {
        match target {
            UiTarget::Response => app.focused_panel = Panel::Response,
            UiTarget::Activity(ActivityTarget::Log) => {
                if app.logs_visible() {
                    app.focused_panel = Panel::Logs;
                }
            }
            UiTarget::Todo => {
                if app.todo_visible() {
                    app.focused_panel = Panel::Todo;
                }
            }
            UiTarget::Overlay(_) => {}
            UiTarget::StatusBar(_) => {}
        }
    }
}

pub fn workflow_summary_from_status(value: StatusValue) -> Option<WorkflowSummary> {
    match value {
        StatusValue::WorkflowStep {
            workflow_id,
            workflow_role,
            step_id,
            step_label,
            index,
            total,
        } => Some(WorkflowSummary {
            workflow_id,
            workflow_role,
            id: step_id,
            label: step_label,
            index,
            total,
        }),
        StatusValue::Label(label) => Some(WorkflowSummary {
            workflow_id: "workflow".to_string(),
            workflow_role: WorkflowRunRole::Child,
            id: "workflow".to_string(),
            label,
            index: 0,
            total: 0,
        }),
        StatusValue::Hidden => None,
        StatusValue::SessionRouting { .. } => None,
    }
}

pub fn session_status_from_status(value: StatusValue) -> Option<SessionStatusSummary> {
    match value {
        StatusValue::Label(label) => Some(SessionStatusSummary::Label(label)),
        StatusValue::SessionRouting {
            root_workflow_id,
            active_workflow_id,
            active_workflow_role,
            recognized_scene_id,
            selected_workflow_id,
        } => Some(SessionStatusSummary::Routing(SessionRoutingSummary {
            root_workflow_id,
            active_workflow_id,
            active_workflow_role,
            recognized_scene_id,
            selected_workflow_id,
        })),
        StatusValue::Hidden | StatusValue::WorkflowStep { .. } => None,
    }
}
