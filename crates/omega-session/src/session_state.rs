use std::collections::BTreeMap;

use omega_context::GovernanceEventSignal;
use omega_plan::SelectedProjectTaskContext;
use serde_json::Value;

use crate::runtime_ui::WorkflowRunRole;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SessionContext {
    pub(crate) latest_user_turn: String,
    pub(crate) routing: RoutingContext,
    pub(crate) skill_routing: SkillRoutingContext,
    pub(crate) step_summaries: Vec<StepSummary>,
    pub(crate) step_outputs: BTreeMap<String, Value>,
    pub(crate) governance_events: Vec<GovernanceEventSignal>,
    pub(crate) selected_task: Option<SelectedProjectTaskContext>,
}

impl SessionContext {
    pub(crate) fn new(root_workflow_id: impl Into<String>) -> Self {
        Self {
            latest_user_turn: String::new(),
            routing: RoutingContext::for_workflow(root_workflow_id.into(), WorkflowRunRole::Root),
            skill_routing: SkillRoutingContext::default(),
            step_summaries: Vec::new(),
            step_outputs: BTreeMap::new(),
            governance_events: Vec::new(),
            selected_task: None,
        }
    }

    pub(crate) fn begin_turn(
        &mut self,
        latest_user_turn: impl Into<String>,
        root_workflow_id: impl Into<String>,
    ) {
        self.latest_user_turn = latest_user_turn.into();
        self.routing = RoutingContext::for_workflow(root_workflow_id.into(), WorkflowRunRole::Root);
        self.skill_routing = SkillRoutingContext::default();
        self.step_outputs.clear();
        self.governance_events.clear();
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct SkillRoutingContext {
    pub(crate) selected_skill_ids: Vec<String>,
    pub(crate) loaded_skill_ids: Vec<String>,
    pub(crate) ignored_skill_ids: Vec<String>,
    pub(crate) selection_reason: Option<String>,
    pub(crate) source_step_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RoutingContext {
    pub(crate) recognized_scene_id: Option<String>,
    pub(crate) selected_workflow_id: Option<String>,
    pub(crate) active_workflow_id: String,
    pub(crate) active_workflow_role: WorkflowRunRole,
}

impl RoutingContext {
    pub(crate) fn for_workflow(
        active_workflow_id: String,
        active_workflow_role: WorkflowRunRole,
    ) -> Self {
        Self {
            recognized_scene_id: None,
            selected_workflow_id: None,
            active_workflow_id,
            active_workflow_role,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StepSummary {
    pub(crate) workflow_id: String,
    pub(crate) step_id: String,
    pub(crate) title: String,
    pub(crate) summary: String,
    pub(crate) estimated_tokens: u32,
}
