use std::sync::{mpsc, Arc};

use crate::runtime_ui::{
    ResponseSection, ResponseSectionDelta, ResponseSectionState, RuntimeUiEffect,
    RuntimeUiEnvelope, RuntimeUiMessage, StatusSlot, StatusValue, StepDiagnostics, ToolRun,
    ToolRunStatus, UiContent, UiMessageKind, UiPriority, UiSource, UiTarget, WorkflowRunRole,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeMessageEnvelope {
    pub turn_id: u64,
    pub message: RuntimeMessage,
}

impl RuntimeMessageEnvelope {
    pub fn conversation(turn_id: u64, message: ConversationMessage) -> Self {
        Self {
            turn_id,
            message: RuntimeMessage::Conversation(message),
        }
    }

    pub fn state(turn_id: u64, message: StateMessage) -> Self {
        Self {
            turn_id,
            message: RuntimeMessage::State(message),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeMessage {
    Conversation(ConversationMessage),
    State(StateMessage),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConversationMessage {
    BeginSection { section: ResponseSection },
    AppendSection { id: String, delta: ResponseSectionDelta },
    CompleteSection { id: String, state: ResponseSectionState },
    BeginToolRun { tool_run: ToolRun },
    UpdateToolRun { tool_run: ToolRun },
    CompleteToolRun { id: String, status: ToolRunStatus },
    Text {
        source: RuntimeSource,
        kind: RuntimeContentKind,
        text: String,
        priority: Option<RuntimePriority>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StateMessage {
    WorkflowStep(WorkflowStepStatus),
    ClearWorkflowStep,
    AgentStatus { label: Option<String> },
    SessionRouting(SessionRoutingStatus),
    TodoSnapshot { rendered: String },
    Diagnostics { diagnostics: Box<StepDiagnostics> },
    Activity {
        source: RuntimeSource,
        kind: RuntimeContentKind,
        text: String,
        priority: Option<RuntimePriority>,
    },
    TurnFinished,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowStepStatus {
    pub workflow_id: String,
    pub workflow_role: WorkflowRunRole,
    pub step_id: String,
    pub step_label: String,
    pub index: usize,
    pub total: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRoutingStatus {
    pub root_workflow_id: String,
    pub active_workflow_id: String,
    pub active_workflow_role: WorkflowRunRole,
    pub recognized_scene_id: Option<String>,
    pub selected_workflow_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeSource {
    User,
    Assistant,
    WorkflowStep {
        workflow_id: String,
        workflow_role: WorkflowRunRole,
        step_id: String,
        step_label: String,
        index: usize,
        total: usize,
    },
    SessionRouting,
    Tool {
        tool_name: String,
    },
    SkillLoader,
    Subagent {
        agent_id: String,
    },
    BackgroundTask {
        task_id: String,
    },
    MessageBus,
    Team,
    Worktree,
    System,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeContentKind {
    Narrative,
    Result,
    Log,
    Warning,
    Error,
    Summary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimePriority {
    Normal,
    Low,
    High,
}

pub trait RuntimeMessageBridge: Send + Sync {
    fn send(&self, envelope: RuntimeMessageEnvelope);
}

impl RuntimeMessageBridge for mpsc::Sender<RuntimeMessageEnvelope> {
    fn send(&self, envelope: RuntimeMessageEnvelope) {
        let _ = mpsc::Sender::send(self, envelope);
    }
}

impl<T> RuntimeMessageBridge for Arc<T>
where
    T: RuntimeMessageBridge + ?Sized,
{
    fn send(&self, envelope: RuntimeMessageEnvelope) {
        (**self).send(envelope);
    }
}

impl<T> RuntimeMessageBridge for &T
where
    T: RuntimeMessageBridge + ?Sized,
{
    fn send(&self, envelope: RuntimeMessageEnvelope) {
        (**self).send(envelope);
    }
}

pub type SharedRuntimeMessageBridge = Arc<dyn RuntimeMessageBridge>;

#[derive(Clone)]
pub struct LegacyRuntimeUiBridge {
    tx: mpsc::Sender<RuntimeUiEnvelope>,
}

impl LegacyRuntimeUiBridge {
    pub fn new(tx: mpsc::Sender<RuntimeUiEnvelope>) -> Self {
        Self { tx }
    }
}

impl RuntimeMessageBridge for LegacyRuntimeUiBridge {
    fn send(&self, envelope: RuntimeMessageEnvelope) {
        for ui_envelope in legacy_runtime_ui_envelopes(envelope) {
            let _ = self.tx.send(ui_envelope);
        }
    }
}

pub fn legacy_runtime_ui_envelopes(envelope: RuntimeMessageEnvelope) -> Vec<RuntimeUiEnvelope> {
    match envelope.message {
        RuntimeMessage::Conversation(message) => legacy_ui_envelopes_from_conversation(
            envelope.turn_id,
            message,
        ),
        RuntimeMessage::State(message) => legacy_ui_envelopes_from_state(envelope.turn_id, message),
    }
}

fn legacy_ui_envelopes_from_conversation(
    turn_id: u64,
    message: ConversationMessage,
) -> Vec<RuntimeUiEnvelope> {
    match message {
        ConversationMessage::BeginSection { section } => vec![RuntimeUiEnvelope::effect(
            turn_id,
            RuntimeUiEffect::BeginResponseSection { section },
        )],
        ConversationMessage::AppendSection { id, delta } => vec![RuntimeUiEnvelope::effect(
            turn_id,
            RuntimeUiEffect::AppendResponseSection { id, delta },
        )],
        ConversationMessage::CompleteSection { id, state } => vec![RuntimeUiEnvelope::effect(
            turn_id,
            RuntimeUiEffect::CompleteResponseSection { id, state },
        )],
        ConversationMessage::BeginToolRun { tool_run } => vec![RuntimeUiEnvelope::effect(
            turn_id,
            RuntimeUiEffect::BeginToolRun { tool_run },
        )],
        ConversationMessage::UpdateToolRun { tool_run } => vec![RuntimeUiEnvelope::effect(
            turn_id,
            RuntimeUiEffect::UpdateToolRun { tool_run },
        )],
        ConversationMessage::CompleteToolRun { id, status } => vec![RuntimeUiEnvelope::effect(
            turn_id,
            RuntimeUiEffect::CompleteToolRun { id, status },
        )],
        ConversationMessage::Text {
            source,
            kind,
            text,
            priority,
        } => vec![RuntimeUiEnvelope::message(
            turn_id,
            RuntimeUiMessage {
                target: UiTarget::Response,
                source: legacy_ui_source(source),
                kind: legacy_ui_kind(kind),
                content: UiContent::Text(text),
                priority: legacy_ui_priority(priority),
            },
        )],
    }
}

fn legacy_ui_envelopes_from_state(turn_id: u64, message: StateMessage) -> Vec<RuntimeUiEnvelope> {
    match message {
        StateMessage::WorkflowStep(step) => vec![RuntimeUiEnvelope::effect(
            turn_id,
            RuntimeUiEffect::SetStatusSlot {
                slot: StatusSlot::Workflow,
                value: StatusValue::WorkflowStep {
                    workflow_id: step.workflow_id,
                    workflow_role: step.workflow_role,
                    step_id: step.step_id,
                    step_label: step.step_label,
                    index: step.index,
                    total: step.total,
                },
            },
        )],
        StateMessage::ClearWorkflowStep => vec![RuntimeUiEnvelope::effect(
            turn_id,
            RuntimeUiEffect::ClearStatusSlot {
                slot: StatusSlot::Workflow,
            },
        )],
        StateMessage::AgentStatus { label } => {
            let effect = match label {
                Some(label) => RuntimeUiEffect::SetStatusSlot {
                    slot: StatusSlot::Agent,
                    value: StatusValue::Label(label),
                },
                None => RuntimeUiEffect::ClearStatusSlot {
                    slot: StatusSlot::Agent,
                },
            };
            vec![RuntimeUiEnvelope::effect(turn_id, effect)]
        }
        StateMessage::SessionRouting(routing) => vec![RuntimeUiEnvelope::effect(
            turn_id,
            RuntimeUiEffect::SetStatusSlot {
                slot: StatusSlot::Session,
                value: StatusValue::SessionRouting {
                    root_workflow_id: routing.root_workflow_id,
                    active_workflow_id: routing.active_workflow_id,
                    active_workflow_role: routing.active_workflow_role,
                    recognized_scene_id: routing.recognized_scene_id,
                    selected_workflow_id: routing.selected_workflow_id,
                },
            },
        )],
        StateMessage::TodoSnapshot { rendered } => vec![RuntimeUiEnvelope::effect(
            turn_id,
            RuntimeUiEffect::ReplacePanel {
                target: UiTarget::Todo,
                content: UiContent::Text(rendered),
            },
        )],
        StateMessage::Diagnostics { diagnostics } => vec![RuntimeUiEnvelope::effect(
            turn_id,
            RuntimeUiEffect::UpsertStepDiagnostics { diagnostics },
        )],
        StateMessage::Activity {
            source,
            kind,
            text,
            priority,
        } => vec![RuntimeUiEnvelope::message(
            turn_id,
            RuntimeUiMessage {
                target: UiTarget::Activity(crate::ActivityTarget::Log),
                source: legacy_ui_source(source),
                kind: legacy_ui_kind(kind),
                content: UiContent::Text(text),
                priority: legacy_ui_priority(priority),
            },
        )],
        StateMessage::TurnFinished => vec![
            RuntimeUiEnvelope::effect(
                turn_id,
                RuntimeUiEffect::ClearStatusSlot {
                    slot: StatusSlot::Workflow,
                },
            ),
            RuntimeUiEnvelope::effect(
                turn_id,
                RuntimeUiEffect::SetStatusSlot {
                    slot: StatusSlot::Agent,
                    value: StatusValue::Label("Idle".to_string()),
                },
            ),
        ],
    }
}

fn legacy_ui_source(source: RuntimeSource) -> UiSource {
    match source {
        RuntimeSource::User => UiSource::User,
        RuntimeSource::Assistant => UiSource::Assistant,
        RuntimeSource::WorkflowStep {
            workflow_id,
            workflow_role,
            step_id,
            step_label,
            index,
            total,
        } => UiSource::WorkflowStep {
            workflow_id,
            workflow_role,
            step_id,
            step_label,
            index,
            total,
        },
        RuntimeSource::SessionRouting => UiSource::SessionRouting,
        RuntimeSource::Tool { tool_name } => UiSource::Tool { tool_name },
        RuntimeSource::SkillLoader => UiSource::SkillLoader,
        RuntimeSource::Subagent { agent_id } => UiSource::Subagent { agent_id },
        RuntimeSource::BackgroundTask { task_id } => UiSource::BackgroundTask { task_id },
        RuntimeSource::MessageBus => UiSource::MessageBus,
        RuntimeSource::Team => UiSource::Team,
        RuntimeSource::Worktree => UiSource::Worktree,
        RuntimeSource::System => UiSource::System,
    }
}

fn legacy_ui_kind(kind: RuntimeContentKind) -> UiMessageKind {
    match kind {
        RuntimeContentKind::Narrative => UiMessageKind::Narrative,
        RuntimeContentKind::Result => UiMessageKind::Result,
        RuntimeContentKind::Log => UiMessageKind::Log,
        RuntimeContentKind::Warning => UiMessageKind::Warning,
        RuntimeContentKind::Error => UiMessageKind::Error,
        RuntimeContentKind::Summary => UiMessageKind::Summary,
    }
}

fn legacy_ui_priority(priority: Option<RuntimePriority>) -> Option<UiPriority> {
    priority.map(|priority| match priority {
        RuntimePriority::Normal => UiPriority::Normal,
        RuntimePriority::Low => UiPriority::Low,
        RuntimePriority::High => UiPriority::High,
    })
}