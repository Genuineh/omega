use omega_session::{
    ContextSupervisionSnapshot, OverlayRequest, ResponseSection, ResponseSectionDelta,
    ResponseSectionState, RuntimeUiEffect, RuntimeUiEnvelope, SessionRoutingStatus,
    SkillLoadSummary, StatusSlot, StatusValue, StepDiagnostics, StepKnowledgeSummary,
    StepSubflowStatus, ToolRun,
    ToolRunStatus, WorkflowStepStatus,
};

use crate::app::{App, MsgKind};
use crate::reducer::TuiUpdateReducer;

pub trait TuiSurface {
    fn begin_section(&mut self, section: ResponseSection);
    fn append_section(&mut self, id: &str, delta: ResponseSectionDelta);
    fn complete_section(&mut self, id: &str, state: ResponseSectionState);
    fn begin_tool_run(&mut self, tool_run: ToolRun);
    fn update_tool_run(&mut self, tool_run: ToolRun);
    fn complete_tool_run(&mut self, id: &str, status: ToolRunStatus);
    fn set_workflow_step(&mut self, step: WorkflowStepStatus);
    fn clear_workflow_step(&mut self);
    fn set_agent_status(&mut self, label: &str);
    fn clear_agent_status(&mut self);
    fn set_session_routing(&mut self, routing: SessionRoutingStatus);
    fn clear_session_routing(&mut self);
    fn set_todo_snapshot(&mut self, text: &str);
    fn upsert_diagnostics(&mut self, diagnostics: StepDiagnostics);
    fn set_context_supervision(&mut self, snapshot: ContextSupervisionSnapshot);
    fn upsert_skill_load_summary(&mut self, section_id: String, summary: SkillLoadSummary);
    fn upsert_step_knowledge_summary(
        &mut self,
        section_id: String,
        summary: StepKnowledgeSummary,
    );
    fn upsert_step_subflow(&mut self, subflow: StepSubflowStatus);
    fn add_activity_line(&mut self, line: String);
    fn show_overlay(&mut self, request: OverlayRequest);
    fn push_agent_message(&mut self, text: &str);
    fn push_error_message(&mut self, text: &str);
    fn mark_turn_finished(&mut self);
}

pub struct TuiEngine<'a> {
    app: &'a mut App,
}

impl<'a> TuiEngine<'a> {
    pub fn new(app: &'a mut App) -> Self {
        Self { app }
    }
}

impl TuiSurface for TuiEngine<'_> {
    fn begin_section(&mut self, section: ResponseSection) {
        self.app.begin_response_section(section);
    }

    fn append_section(&mut self, id: &str, delta: ResponseSectionDelta) {
        match delta {
            ResponseSectionDelta::Text(text) => self.app.append_response_section(id, &text),
        }
    }

    fn complete_section(&mut self, id: &str, state: ResponseSectionState) {
        self.app.complete_response_section(id, state);
    }

    fn begin_tool_run(&mut self, tool_run: ToolRun) {
        self.app.begin_tool_run(tool_run);
    }

    fn update_tool_run(&mut self, tool_run: ToolRun) {
        self.app.update_tool_run(tool_run);
    }

    fn complete_tool_run(&mut self, id: &str, status: ToolRunStatus) {
        self.app.complete_tool_run(id, status);
    }

    fn set_workflow_step(&mut self, step: WorkflowStepStatus) {
        self.app.set_status_slot(
            StatusSlot::Workflow,
            StatusValue::WorkflowStep {
                workflow_id: step.workflow_id,
                workflow_role: step.workflow_role,
                step_id: step.step_id,
                step_label: step.step_label,
                index: step.index,
                total: step.total,
            },
        );
    }

    fn clear_workflow_step(&mut self) {
        self.app.clear_status_slot(StatusSlot::Workflow);
    }

    fn set_agent_status(&mut self, label: &str) {
        self.app
            .set_status_slot(StatusSlot::Agent, StatusValue::Label(label.to_string()));
    }

    fn clear_agent_status(&mut self) {
        self.app.clear_status_slot(StatusSlot::Agent);
    }

    fn set_session_routing(&mut self, routing: SessionRoutingStatus) {
        self.app.set_status_slot(
            StatusSlot::Session,
            StatusValue::SessionRouting {
                root_workflow_id: routing.root_workflow_id,
                active_workflow_id: routing.active_workflow_id,
                active_workflow_role: routing.active_workflow_role,
                recognized_scene_id: routing.recognized_scene_id,
                selected_workflow_id: routing.selected_workflow_id,
            },
        );
    }

    fn clear_session_routing(&mut self) {
        self.app.clear_status_slot(StatusSlot::Session);
    }

    fn set_todo_snapshot(&mut self, text: &str) {
        self.app.set_todo_snapshot(self.app.active_turn_id, text);
    }

    fn upsert_diagnostics(&mut self, diagnostics: StepDiagnostics) {
        self.app.upsert_step_diagnostics(diagnostics);
    }

    fn set_context_supervision(&mut self, snapshot: ContextSupervisionSnapshot) {
        self.app.set_context_supervision(snapshot);
    }

    fn upsert_skill_load_summary(&mut self, section_id: String, summary: SkillLoadSummary) {
        self.app.upsert_skill_load_summary(section_id, summary);
    }

    fn upsert_step_knowledge_summary(
        &mut self,
        section_id: String,
        summary: StepKnowledgeSummary,
    ) {
        self.app.upsert_step_knowledge_summary(section_id, summary);
    }

    fn upsert_step_subflow(&mut self, subflow: StepSubflowStatus) {
        self.app.upsert_step_subflow(subflow);
    }

    fn add_activity_line(&mut self, line: String) {
        self.app.add_log(line);
    }

    fn show_overlay(&mut self, request: OverlayRequest) {
        TuiUpdateReducer::apply(
            self.app,
            RuntimeUiEnvelope::effect(
                self.app.active_turn_id,
                RuntimeUiEffect::ShowOverlay(request),
            ),
        );
    }

    fn push_agent_message(&mut self, text: &str) {
        self.app.push_msg(MsgKind::Agent, text);
    }

    fn push_error_message(&mut self, text: &str) {
        self.app.push_msg(MsgKind::Error, text);
    }

    fn mark_turn_finished(&mut self) {
        self.app.clear_status_slot(StatusSlot::Workflow);
        self.app
            .set_status_slot(StatusSlot::Agent, StatusValue::Label("Idle".to_string()));
    }
}
