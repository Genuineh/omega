use std::collections::BTreeMap;
use std::sync::{mpsc, Arc};

use omega_context::{ContextDiagnostics, ContextSupervisionSnapshot, StepKnowledgeSummary};
use omega_project::ProjectDetailSnapshot;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeUiEnvelope {
    Message {
        turn_id: u64,
        message: RuntimeUiMessage,
    },
    Effect {
        turn_id: u64,
        effect: RuntimeUiEffect,
    },
}

impl RuntimeUiEnvelope {
    pub fn message(turn_id: u64, message: RuntimeUiMessage) -> Self {
        Self::Message { turn_id, message }
    }

    pub fn effect(turn_id: u64, effect: RuntimeUiEffect) -> Self {
        Self::Effect { turn_id, effect }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeUiMessage {
    pub target: UiTarget,
    pub source: UiSource,
    pub kind: UiMessageKind,
    pub content: UiContent,
    pub priority: Option<UiPriority>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeUiEffect {
    SetStatusSlot {
        slot: StatusSlot,
        value: StatusValue,
    },
    ClearStatusSlot {
        slot: StatusSlot,
    },
    ReplacePanel {
        target: UiTarget,
        content: UiContent,
    },
    OpenDiffPreview {
        diff: String,
    },
    RequestInput {
        prompt: String,
    },
    OpenWebResultView {
        title: String,
        content: String,
    },
    RequestToolApproval {
        message: String,
    },
    ShowOverlay(OverlayRequest),
    HideOverlay {
        target: OverlayTarget,
    },
    FocusHint {
        target: UiTarget,
    },
    BeginResponseSection {
        section: ResponseSection,
    },
    AppendResponseSection {
        id: String,
        delta: ResponseSectionDelta,
    },
    CompleteResponseSection {
        id: String,
        state: ResponseSectionState,
    },
    BeginToolRun {
        tool_run: ToolRun,
    },
    UpdateToolRun {
        tool_run: ToolRun,
    },
    CompleteToolRun {
        id: String,
        status: ToolRunStatus,
    },
    UpsertStepSubflow {
        subflow: StepSubflowStatus,
    },
    UpsertStepDiagnostics {
        diagnostics: Box<StepDiagnostics>,
    },
    UpsertContextSupervision {
        snapshot: Box<ContextSupervisionSnapshot>,
    },
    UpsertSkillLoadSummary {
        section_id: String,
        summary: Box<SkillLoadSummary>,
    },
    UpsertStepKnowledgeSummary {
        section_id: String,
        summary: Box<StepKnowledgeSummary>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiTarget {
    Response,
    Activity(ActivityTarget),
    Todo,
    StatusBar(StatusSlot),
    Overlay(OverlayTarget),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityTarget {
    Log,
    Skills,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillLoadSummary {
    pub source_step_id: Option<String>,
    pub recognized_skill_ids: Vec<String>,
    pub loaded_skill_ids: Vec<String>,
    pub ignored_skill_ids: Vec<String>,
    pub selection_reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusSlot {
    Workflow,
    Agent,
    Session,
    Project,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkflowRunRole {
    Root,
    Child,
}

impl WorkflowRunRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Root => "root",
            Self::Child => "child",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatusValue {
    Label(String),
    WorkflowStep {
        workflow_id: String,
        workflow_role: WorkflowRunRole,
        step_id: String,
        step_label: String,
        index: usize,
        total: usize,
    },
    SessionRouting {
        root_workflow_id: String,
        active_workflow_id: String,
        active_workflow_role: WorkflowRunRole,
        recognized_scene_id: Option<String>,
        selected_workflow_id: Option<String>,
    },
    ProjectSelection {
        snapshot: Box<ProjectDetailSnapshot>,
    },
    Hidden,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResponseSection {
    pub id: String,
    pub parent_id: Option<String>,
    pub kind: ResponseSectionKind,
    pub title: String,
    pub state: ResponseSectionState,
    pub metadata: ResponseSectionMetadata,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResponseSectionKind {
    Routing,
    Step,
    FinalAnswer,
    Thinking,
    Command,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SectionOrigin {
    Workflow {
        workflow_id: String,
        workflow_role: WorkflowRunRole,
    },
    Command {
        command_name: String,
        source: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResponseSectionMetadata {
    pub scene_id: Option<String>,
    pub origin: SectionOrigin,
    pub step_id: Option<String>,
    pub step_label: Option<String>,
    pub subflow_ref: Option<StepSubflowRef>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepSubflowRef {
    pub parent_workflow_id: String,
    pub parent_step_id: String,
    pub parent_step_label: String,
    pub subflow_id: String,
    pub item_id: Option<String>,
    pub item_label: Option<String>,
    pub item_index: usize,
    pub item_total: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepSubflowState {
    Queued,
    Running,
    Complete,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepSubflowStatus {
    pub workflow_id: String,
    pub workflow_role: WorkflowRunRole,
    pub step_id: String,
    pub step_label: String,
    pub subflow_id: String,
    pub item_id: Option<String>,
    pub item_label: Option<String>,
    pub item_index: usize,
    pub item_total: usize,
    pub status: StepSubflowState,
    pub repeat_count_for_item: u32,
    pub no_progress_streak_for_item: u32,
    pub completion_source: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResponseSectionState {
    Streaming,
    Complete,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResponseSectionDelta {
    Text(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolRun {
    pub id: String,
    pub parent_section_id: String,
    pub tool_name: String,
    pub status: ToolRunStatus,
    pub invocation_preview: String,
    pub result_preview: Option<String>,
    pub detail: ToolRunDetail,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolRunStatus {
    Running,
    Complete,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolRunDetail {
    pub title: String,
    pub lines: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepDiagnostics {
    pub id: String,
    pub workflow_id: String,
    pub workflow_role: WorkflowRunRole,
    pub step_id: String,
    pub step_label: String,
    pub index: usize,
    pub total: usize,
    pub context: Option<ContextDiagnostics>,
    pub cache: Option<CacheDiagnostics>,
    pub execute_progress: Option<ExecuteProgressDiagnostics>,
    pub input: StepInputDiagnostics,
    pub output: StepOutputDiagnostics,
    pub session_writes: Vec<StepContextWrite>,
    pub tool_capabilities: Option<ToolCapabilityDiagnostics>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ToolCapabilityDiagnostics {
    pub tool_invocations: BTreeMap<String, u32>,
    pub family_invocations: BTreeMap<String, u32>,
    pub tool_failure_count_by_kind: BTreeMap<String, u32>,
    pub bash_fallback_count: u32,
    pub question_block_count: u32,
    pub tool_switch_after_failure: u32,
    pub same_intent_retry_count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheDiagnostics {
    pub token_count_source: TokenCountSource,
    pub request_input_tokens: u32,
    pub budget_input_tokens: u32,
    pub cache_breakpoints: Vec<String>,
    pub cache_creation_input_tokens: Option<u32>,
    pub cache_read_input_tokens: Option<u32>,
    pub uncached_input_tokens: Option<u32>,
    pub cache_hit_ratio_percent: Option<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenCountSource {
    ProviderCountTokens,
    Estimated,
}

impl TokenCountSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ProviderCountTokens => "provider_count_tokens",
            Self::Estimated => "estimated",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecuteProgressDiagnostics {
    pub todo_total: usize,
    pub todo_completed: usize,
    pub todo_open: usize,
    pub current_item_id: Option<String>,
    pub current_item_index: Option<usize>,
    pub current_item_total: Option<usize>,
    pub repeat_count: u32,
    pub no_progress_streak: u32,
    pub max_step_repeats: u32,
    pub max_item_repeats: Option<u32>,
    pub completion_source: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepInputDiagnostics {
    pub status: StepInputStatus,
    pub summary_sources: Vec<StepSummarySource>,
    pub expected_structured_sources: Vec<String>,
    pub resolved_structured_sources: Vec<String>,
    pub missing_structured_sources: Vec<String>,
    pub structured_input_preview: Option<String>,
    pub todo_state_preview: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepInputStatus {
    None,
    Ready,
    OptionalEmpty,
    MissingRequired,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepSummarySource {
    pub workflow_id: String,
    pub step_id: String,
    pub title: String,
    pub preview: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepOutputDiagnostics {
    pub contract_mode: StepOutputContractMode,
    pub format: Option<String>,
    pub schema_path: Option<String>,
    pub status: StepOutputStatus,
    pub attempt_kind: StepOutputAttemptKind,
    pub extracted_json_preview: Option<String>,
    pub previous_response_preview: Option<String>,
    pub attempts: u32,
    pub retry_count: u32,
    pub max_retries: u32,
    pub validation_error: Option<String>,
    pub recovery_decision: Option<StepOutputRecoveryDecision>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepOutputAttemptKind {
    Primary,
    Repair,
    Regenerate,
}

impl StepOutputAttemptKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Primary => "primary",
            Self::Repair => "repair",
            Self::Regenerate => "regenerate",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepOutputRecoveryDecision {
    Repair,
    Regenerate,
    FallbackTextRouting,
    Abort,
}

impl StepOutputRecoveryDecision {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Repair => "repair",
            Self::Regenerate => "regenerate",
            Self::FallbackTextRouting => "fallback_text_routing",
            Self::Abort => "abort",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepOutputContractMode {
    None,
    Required,
    Optional,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepOutputStatus {
    None,
    Pending,
    Valid,
    Invalid,
    Skipped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepContextWriteKind {
    Added,
    Updated,
    Cleared,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepContextWrite {
    pub path: String,
    pub kind: StepContextWriteKind,
    pub before_preview: Option<String>,
    pub after_preview: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayTarget {
    Search,
    Confirm,
    Detail,
    Picker,
    InputPrompt,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverlayRequest {
    pub target: OverlayTarget,
    pub content: UiContent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiSource {
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
pub enum UiMessageKind {
    Narrative,
    Result,
    Log,
    Warning,
    Error,
    Summary,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiContent {
    Text(String),
}

impl UiContent {
    pub fn as_text(&self) -> &str {
        match self {
            Self::Text(text) => text,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiPriority {
    Normal,
    Low,
    High,
}

pub trait RuntimeUiBridge: Send + Sync {
    fn send(&self, envelope: RuntimeUiEnvelope);
}

impl RuntimeUiBridge for mpsc::Sender<RuntimeUiEnvelope> {
    fn send(&self, envelope: RuntimeUiEnvelope) {
        let _ = mpsc::Sender::send(self, envelope);
    }
}

pub trait RuntimeUiSink {
    fn try_recv(&self) -> Option<RuntimeUiEnvelope>;
}

impl RuntimeUiSink for mpsc::Receiver<RuntimeUiEnvelope> {
    fn try_recv(&self) -> Option<RuntimeUiEnvelope> {
        self.try_recv().ok()
    }
}

pub struct SessionRuntimeContext {
    pub ui_bridge: Arc<dyn RuntimeUiBridge>,
}
