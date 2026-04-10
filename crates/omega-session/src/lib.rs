use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use omega_command::{
    CommandHint, CommandHintProvider, CommandHintResolution, OmegaCommandDescriptor,
    OmegaCommandInvocation, OmegaCommandRegistry, OmegaCommandSource, OmegaCommandSubcommand,
};
use omega_context::{
    ArchiveTrigger, DocType, DocumentMutationMode, DocumentOp, FileRecord, FileStatus,
    GovernanceEventSignal, OmegaContextFacade, SearchMode, SearchQuery, TurnData,
    TurnRetentionSignals,
};
pub use omega_context::{
    ContextBudgetDiagnostics, ContextDiagnostics, ContextDocumentDiagnostics,
    ContextMemoryDiagnostics, ContextStoreDiagnostics, ContextSupervisionSnapshot,
    DocumentActivitySummary, DocumentHealthStatus, DocumentHitItem, DocumentHitSummary,
    DocumentOperatorUsage, DocumentStoreVersion, DocumentSupervisionSnapshot,
    DocumentSupervisionTotals, HealthScore, MemoryHitItem, MemoryHitSummary,
    MemoryQueryDiagnostics, MemoryQueryHitItem, MemorySupervisionSnapshot,
    MemorySupervisionTotals, ObservationRecallDiagnostics, ObservationRecallHitItem,
    ObservationFreshness, ResponseDocumentKnowledge, ResponseMemoryKnowledge,
    StepKnowledgeSummary, SupervisionReadiness,
};
use omega_core::{Agent, CoreSharedTodoManager, DynLlmClient, Message, TodoManager};
use omega_hooks::HookHost;
use omega_project::{
    OmegaProjectHandle, ProjectDetailSnapshot, ProjectDetectionKind, ProjectRegistry,
    ProjectResolutionInput, ProjectSessionStatus, ProjectSessionUpdate,
};
use omega_skills::SkillLoader;
use omega_workflow::{SceneCatalog, WorkflowCatalog, WorkflowPromptCatalog};
use tokio::runtime::Handle;
use tokio::sync::watch;
use tracing::{error, info};
use uuid::Uuid;

const SUMMARY_CHAR_LIMIT: usize = 2_000;
const CONTEXT_SAFETY_MARGIN_TOKENS: u32 = 2_000;
const TOKEN_ESTIMATE_DIVISOR: usize = 4;
const REPAIR_PASS_MAX_ITERATIONS: u32 = 1;

mod hook_adapter;
mod output;
mod routing;
mod runner;
mod runtime_message;
mod runtime_ui;
mod session_state;
mod skill_catalog;
#[cfg(any(test, feature = "test-support"))]
mod test_support;
mod tool_catalog;
mod ui_emit;

use crate::output::{parse_feature_execute_output, parse_feature_plan_output};

pub use omega_workflow::{
    StepSkillRequest, StepToolRequest, DEEP_RESEARCH_SCENE_ID, DEEP_RESEARCH_WORKFLOW_ID,
    EXECUTE_STEP_ID, EXPLORE_STEP_ID, FEATURE_SCENE_ID, FEATURE_WORKFLOW_ID, PLAN_STEP_ID,
    REPORT_STEP_ID, RESEARCH_SCENE_ID, RESEARCH_WORKFLOW_ID, SCENE_RECOGNITION_STEP_ID,
    SELECT_WORKFLOW_STEP_ID,
};
pub use runtime_message::{
    ConversationMessage, LegacyRuntimeUiBridge, RuntimeContentKind, RuntimeMessage,
    RuntimeMessageBridge, RuntimeMessageEnvelope, RuntimePriority, RuntimeSource,
    SessionRoutingStatus, SharedRuntimeMessageBridge, StateMessage, WorkflowStepStatus,
};
pub use runtime_ui::{
    ActivityTarget, CacheDiagnostics, ExecuteProgressDiagnostics, OverlayRequest, OverlayTarget,
    ResponseSection, ResponseSectionDelta, ResponseSectionKind, ResponseSectionMetadata,
    ResponseSectionState, RuntimeUiBridge, RuntimeUiEffect, RuntimeUiEnvelope, RuntimeUiMessage,
    RuntimeUiSink, SectionOrigin, SessionRuntimeContext, StatusSlot, StatusValue, StepContextWrite,
    StepContextWriteKind, StepDiagnostics, StepInputDiagnostics, StepInputStatus,
    StepOutputAttemptKind, StepOutputContractMode, StepOutputDiagnostics,
    StepOutputRecoveryDecision, StepOutputStatus, StepSubflowRef, StepSubflowState,
    StepSubflowStatus, StepSummarySource, TokenCountSource, ToolCapabilityDiagnostics, ToolRun,
    ToolRunDetail, ToolRunStatus, SkillLoadSummary,
    UiContent, UiMessageKind, UiPriority, UiSource, UiTarget, WorkflowRunRole,
};
pub use skill_catalog::{ResolvedSkillSet, SessionSkillCatalog};
#[cfg(any(test, feature = "test-support"))]
pub use test_support::RuntimeEnvelopeRecorder;
pub use tool_catalog::{ResolvedToolSet, SessionToolCatalog};

#[cfg(test)]
pub(crate) use omega_context::render_output_contract;
#[cfg(test)]
pub(crate) use output::{parse_json_values, validate_schema_file};
#[cfg(test)]
pub(crate) use routing::{
    latest_user_turn_prefers_research_scene, latest_user_turn_requires_feature_scene,
};
#[cfg(test)]
pub(crate) use runner::resolve_structured_input;
pub(crate) use session_state::SessionContext;
#[cfg(test)]
pub(crate) use ui_emit::{preview_tool_invocation, ProviderMarkupSanitizer};

#[cfg(test)]
pub(crate) use output::validate_structured_output;

pub struct AgentSessionConfig {
    pub client: DynLlmClient,
    pub system: String,
    pub cwd: std::path::PathBuf,
    pub runtime_handle: Handle,
    pub scene_catalog: SceneCatalog,
    pub workflow_catalog: WorkflowCatalog,
    pub prompt_catalog: WorkflowPromptCatalog,
    pub context_window: u32,
    pub max_output_tokens: u32,
    pub bash_allowed_commands: Vec<String>,
    pub batch_max_requests: usize,
}

struct ProjectRuntimeState {
    registry: Arc<ProjectRegistry>,
    active_handle: Arc<OmegaProjectHandle>,
}

#[derive(Clone)]
struct SessionRuntimeBindings {
    hook_host: Arc<HookHost>,
    skill_catalog: Arc<SessionSkillCatalog>,
    tool_catalog: Arc<SessionToolCatalog>,
}

struct AgentSlot {
    turn_id: u64,
    agent: Option<Agent>,
}

pub struct AgentSession {
    agent_slot: Arc<Mutex<AgentSlot>>,
    turn_checkpoint: Arc<Mutex<Vec<Message>>>,
    active_turn_tx: watch::Sender<u64>,
    session_context: Arc<Mutex<SessionContext>>,
    project_state: Arc<Mutex<ProjectRuntimeState>>,
    client: DynLlmClient,
    base_system: String,
    cwd: Arc<Mutex<std::path::PathBuf>>,
    session_id: String,
    todo_manager: CoreSharedTodoManager,
    runtime_bindings: Arc<Mutex<SessionRuntimeBindings>>,
    runtime_handle: Handle,
    scene_catalog: SceneCatalog,
    workflow_catalog: WorkflowCatalog,
    prompt_catalog: WorkflowPromptCatalog,
    context_window: u32,
    max_output_tokens: u32,
    bash_allowed_commands: Vec<String>,
    batch_max_requests: usize,
}

impl AgentSession {
    pub fn new(config: AgentSessionConfig) -> anyhow::Result<Self> {
        let registry = Arc::new(ProjectRegistry::new());
        let active_project = registry.resolve(ProjectResolutionInput {
            current_file_path: None,
            cwd: config.cwd.clone(),
            explicit_root: None,
        })?;
        let resolved_cwd = active_project.root();
        let todo_manager = Arc::new(Mutex::new(TodoManager::new()));
        let (runtime_bindings, dispatcher) = load_runtime_bindings(
            &active_project,
            resolved_cwd.clone(),
            todo_manager.clone(),
            config.bash_allowed_commands.clone(),
            config.batch_max_requests,
        )?;
        let initial_system = runtime_bindings.skill_catalog.build_system_prompt(
            &config.system,
            "",
            &[],
            &StepSkillRequest::MatchTask,
        );
        let mut agent = Agent::new(config.client.clone(), initial_system, dispatcher)?;
        agent.set_max_tokens(config.max_output_tokens);
        let checkpoint = agent.messages().to_vec();
        let (active_turn_tx, _active_turn_rx) = watch::channel(0u64);
        let session_id = Uuid::new_v4().simple().to_string();
        active_project.upsert_session(ProjectSessionUpdate {
            session_id: session_id.clone(),
            title: Some(session_title(&session_id)),
            status: ProjectSessionStatus::Active,
            turn_count: 0,
            last_user_turn_preview: None,
        })?;

        if config
            .workflow_catalog
            .workflow(&config.scene_catalog.root_workflow_id)
            .is_none()
        {
            return Err(anyhow::anyhow!(
                "missing root workflow '{}' in workflow catalog",
                config.scene_catalog.root_workflow_id
            ));
        }
        if config
            .scene_catalog
            .scene(&config.scene_catalog.default_scene_id)
            .is_none()
        {
            return Err(anyhow::anyhow!(
                "missing default scene '{}' in scene catalog",
                config.scene_catalog.default_scene_id
            ));
        }

        Ok(Self {
            agent_slot: Arc::new(Mutex::new(AgentSlot {
                turn_id: 0,
                agent: Some(agent),
            })),
            turn_checkpoint: Arc::new(Mutex::new(checkpoint)),
            active_turn_tx,
            session_context: Arc::new(Mutex::new(SessionContext::new(
                config.scene_catalog.root_workflow_id.clone(),
            ))),
            project_state: Arc::new(Mutex::new(ProjectRuntimeState {
                registry,
                active_handle: active_project,
            })),
            client: config.client,
            base_system: config.system,
            cwd: Arc::new(Mutex::new(resolved_cwd)),
            session_id,
            todo_manager,
            runtime_bindings: Arc::new(Mutex::new(runtime_bindings)),
            runtime_handle: config.runtime_handle,
            scene_catalog: config.scene_catalog,
            workflow_catalog: config.workflow_catalog,
            prompt_catalog: config.prompt_catalog,
            context_window: config.context_window,
            max_output_tokens: config.max_output_tokens,
            bash_allowed_commands: config.bash_allowed_commands,
            batch_max_requests: config.batch_max_requests,
        })
    }

    pub fn is_ready(&self) -> bool {
        self.agent_slot.lock().unwrap().agent.is_some()
    }

    pub fn command_hint(&self, input: &str) -> Option<String> {
        if !input.trim_start().starts_with('/') {
            return None;
        }

        let registry = command_registry(&self.active_project_handle());
        Some(render_command_hint(registry.resolve_hint(input)))
    }

    pub fn project_detail_snapshot(&self) -> anyhow::Result<ProjectDetailSnapshot> {
        self.active_project_handle().detail_snapshot()
    }

    pub fn project_status_value(&self) -> anyhow::Result<StatusValue> {
        Ok(StatusValue::ProjectSelection {
            snapshot: Box::new(self.project_detail_snapshot()?),
        })
    }

    pub fn checkpoint_current_messages(&self) {
        let current_messages = self
            .agent_slot
            .lock()
            .unwrap()
            .agent
            .as_ref()
            .map(|agent| agent.messages().to_vec())
            .unwrap_or_default();
        *self.turn_checkpoint.lock().unwrap() = current_messages;
    }

    fn active_project_handle(&self) -> Arc<OmegaProjectHandle> {
        active_project_handle(&self.project_state)
    }

    fn current_cwd(&self) -> std::path::PathBuf {
        self.cwd.lock().unwrap().clone()
    }

    fn current_skill_catalog(&self) -> Arc<SessionSkillCatalog> {
        self.runtime_bindings.lock().unwrap().skill_catalog.clone()
    }

    fn current_hook_host(&self) -> Arc<HookHost> {
        self.runtime_bindings.lock().unwrap().hook_host.clone()
    }

    fn current_tool_catalog(&self) -> Arc<SessionToolCatalog> {
        self.runtime_bindings.lock().unwrap().tool_catalog.clone()
    }

    #[cfg(test)]
    pub(crate) fn debug_runtime_bindings_snapshot(&self) -> RuntimeBindingsDebugSnapshot {
        let bindings = self.runtime_bindings.lock().unwrap().clone();
        RuntimeBindingsDebugSnapshot {
            cwd: self.current_cwd(),
            skill_descriptions: bindings.skill_catalog.descriptions(),
            available_tool_ids: bindings.tool_catalog.available_tool_names(),
            hook_ids: bindings
                .hook_host
                .catalog()
                .hook_ids()
                .into_iter()
                .map(ToString::to_string)
                .collect(),
        }
    }

    pub fn interrupt(&self, replacement_turn_id: u64) -> anyhow::Result<()> {
        let checkpoint = self.turn_checkpoint.lock().unwrap().clone();
        let skill_catalog = self.current_skill_catalog();
        let project_handle = self.active_project_handle();
        let system = skill_catalog.build_system_prompt(
            &self.base_system,
            "",
            &[],
            &StepSkillRequest::MatchTask,
        );
        let dispatcher = omega_core::create_default_tools_with_context_and_todo_manager_and_tool_limits(
            self.current_cwd(),
            project_handle.context_facade(),
            self.todo_manager.clone(),
            self.bash_allowed_commands.clone(),
            self.batch_max_requests,
        );
        let mut replacement = Agent::new(self.client.clone(), system, dispatcher)?;
        replacement.set_max_tokens(self.max_output_tokens);
        replacement.set_messages(checkpoint);

        let mut slot = self.agent_slot.lock().unwrap();
        slot.turn_id = replacement_turn_id;
        slot.agent = Some(replacement);
        self.active_turn_tx.send_replace(replacement_turn_id);
        Ok(())
    }

    pub fn spawn_turn(
        &self,
        input: String,
        turn_id: u64,
        tx: mpsc::Sender<RuntimeMessageEnvelope>,
    ) -> anyhow::Result<()> {
        self.spawn_turn_with_bridge(input, turn_id, Arc::new(tx))
    }

    pub fn spawn_turn_ui_compat(
        &self,
        input: String,
        turn_id: u64,
        tx: mpsc::Sender<RuntimeUiEnvelope>,
    ) -> anyhow::Result<()> {
        self.spawn_turn_with_bridge(input, turn_id, Arc::new(LegacyRuntimeUiBridge::new(tx)))
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn spawn_turn_with_test_bridge(
        &self,
        input: String,
        turn_id: u64,
        tx: SharedRuntimeMessageBridge,
    ) -> anyhow::Result<()> {
        self.spawn_turn_with_bridge(input, turn_id, tx)
    }

    fn spawn_turn_with_bridge(
        &self,
        input: String,
        turn_id: u64,
        tx: SharedRuntimeMessageBridge,
    ) -> anyhow::Result<()> {
        let agent_slot = self.agent_slot.clone();
        let mut slot = self.agent_slot.lock().unwrap();
        slot.turn_id = turn_id;
        let mut agent = match slot.agent.take() {
            Some(agent) => agent,
            None => return Err(anyhow::anyhow!("agent turn already in progress")),
        };
        drop(slot);
        self.active_turn_tx.send_replace(turn_id);
        let project_handle = self.active_project_handle();
        project_handle.upsert_session(ProjectSessionUpdate {
            session_id: self.session_id.clone(),
            title: Some(session_title(&self.session_id)),
            status: ProjectSessionStatus::Active,
            turn_count: turn_id,
            last_user_turn_preview: Some(preview_text(&input, 160)),
        })?;

        let tx_callback = tx.clone();
        let tx_result = tx;
        let handle = self.runtime_handle.clone();
        let cancel_turn_rx = self.active_turn_tx.subscribe();
        let base_system = self.base_system.clone();
        let cwd = self.current_cwd();
        let todo_manager = self.todo_manager.clone();
        let client = self.client.clone();
        let context_facade = project_handle.context_facade();
        let hook_host = self.current_hook_host();
        let skill_catalog = self.current_skill_catalog();
        let tool_catalog = self.current_tool_catalog();
        let scene_catalog = self.scene_catalog.clone();
        let workflow_catalog = self.workflow_catalog.clone();
        let prompt_catalog = self.prompt_catalog.clone();
        let session_context = self.session_context.clone();
        let context_window = self.context_window;
        let max_output_tokens = self.max_output_tokens;
        let session_id = self.session_id.clone();
        thread::spawn(move || {
            let thread_turn_rx = cancel_turn_rx.clone();
            agent.add_user_message(&input);
            let mut turn_context = {
                let mut shared = session_context.lock().unwrap();
                shared.begin_turn(input.clone(), scene_catalog.root_workflow_id.clone());
                shared.clone()
            };
            info!(
                turn_id,
                root_workflow_id = %scene_catalog.root_workflow_id,
                latest_user_turn = %preview_text(&input, 160),
                carried_step_summaries = turn_context.step_summaries.len(),
                "session turn started"
            );
            let runner = runner::WorkflowTurnRunner::new(
                &handle,
                &client,
                &context_facade,
                &skill_catalog,
                &tool_catalog,
                &base_system,
                &input,
                &cwd,
                &todo_manager,
                &hook_host,
                &scene_catalog,
                &workflow_catalog,
                &prompt_catalog,
                context_window,
                max_output_tokens,
                turn_id,
                cancel_turn_rx,
                tx_callback.clone(),
                tx_result.clone(),
            );
            let result = runner.run(&mut agent, &mut turn_context);
            let turn_still_active = *thread_turn_rx.borrow() == turn_id;
            let archive_data = turn_still_active.then(|| build_turn_archive(turn_id, &turn_context));
            let session_title = session_title_from_context(&turn_context);
            let latest_user_turn_preview = preview_text(&turn_context.latest_user_turn, 160);

            if turn_still_active {
                let mut shared = session_context.lock().unwrap();
                *shared = turn_context;
                info!(
                    turn_id,
                    recognized_scene_id = %shared.routing.recognized_scene_id.as_deref().unwrap_or("-"),
                    selected_workflow_id = %shared.routing.selected_workflow_id.as_deref().unwrap_or("-"),
                    active_workflow_id = %shared.routing.active_workflow_id,
                    active_workflow_role = %shared.routing.active_workflow_role.as_str(),
                    stored_step_summaries = shared.step_summaries.len(),
                    "session turn context committed"
                );
            } else {
                info!(turn_id, "discarding canceled turn result");
            }

            if let Some(archive_data) = archive_data.as_ref() {
                if let Err(error) = context_facade.memory.archive_turn(archive_data) {
                    error!(turn_id, error = %error, "failed to archive turn memory");
                } else if let Ok(snapshot) = context_facade.memory.diagnostics_snapshot() {
                    context_facade.diagnostics.record_memory_snapshot(&snapshot);
                }
            }

            match result {
                Ok(text) if turn_still_active && !text.is_empty() => {
                    ui_emit::send_assistant_text(&*tx_result, turn_id, &text);
                }
                Ok(_) => {}
                Err(e) => {
                    if turn_still_active {
                        error!(error = %e, "agent loop error");
                        ui_emit::send_error_text(&*tx_result, turn_id, &format!("Error: {e}"));
                    } else {
                        info!(turn_id, error = %e, "canceled turn stopped before completion");
                    }
                }
            }

            let mut slot = agent_slot.lock().unwrap();
            if slot.turn_id == turn_id {
                slot.agent = Some(agent);
            }

            if turn_still_active {
                if let Ok(snapshot) = project_handle
                    .upsert_session(ProjectSessionUpdate {
                        session_id,
                        title: Some(session_title),
                        status: ProjectSessionStatus::Idle,
                        turn_count: turn_id,
                        last_user_turn_preview: Some(latest_user_turn_preview),
                    })
                    .and_then(|_| project_handle.detail_snapshot())
                {
                    ui_emit::send_project_status(&*tx_result, turn_id, snapshot);
                }
                ui_emit::send_turn_finished(&*tx_result, turn_id);
            }
        });

        Ok(())
    }

    pub fn spawn_command(
        &self,
        input: String,
        turn_id: u64,
        tx: mpsc::Sender<RuntimeMessageEnvelope>,
    ) -> anyhow::Result<()> {
        self.spawn_command_with_bridge(input, turn_id, Arc::new(tx))
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn spawn_command_with_test_bridge(
        &self,
        input: String,
        turn_id: u64,
        tx: SharedRuntimeMessageBridge,
    ) -> anyhow::Result<()> {
        self.spawn_command_with_bridge(input, turn_id, tx)
    }

    fn spawn_command_with_bridge(
        &self,
        input: String,
        turn_id: u64,
        tx: SharedRuntimeMessageBridge,
    ) -> anyhow::Result<()> {
        self.active_turn_tx.send_replace(turn_id);
        let project_state = self.project_state.clone();
        let session_id = self.session_id.clone();
        self.active_project_handle().upsert_session(ProjectSessionUpdate {
            session_id: self.session_id.clone(),
            title: Some(session_title(&self.session_id)),
            status: ProjectSessionStatus::Active,
            turn_count: turn_id,
            last_user_turn_preview: Some(preview_text(&input, 160)),
        })?;
        let session_context = self.session_context.clone();
        let scene_catalog = self.scene_catalog.clone();
        let cwd = self.cwd.clone();
        let agent_slot = self.agent_slot.clone();
        let client = self.client.clone();
        let base_system = self.base_system.clone();
        let todo_manager = self.todo_manager.clone();
        let runtime_bindings = self.runtime_bindings.clone();
        let bash_allowed_commands = self.bash_allowed_commands.clone();
        let max_output_tokens = self.max_output_tokens;
        let batch_max_requests = self.batch_max_requests;

        thread::spawn(move || {
            let mut turn_context = {
                let mut shared = session_context.lock().unwrap();
                shared.begin_turn(input.clone(), scene_catalog.root_workflow_id.clone());
                shared.clone()
            };
            let registry = command_registry(&active_project_handle(&project_state));
            let parsed = registry.parse(&input);
            let title = command_title_from_input(&input);
            let source = parsed
                .as_ref()
                .map(|invocation| invocation.source)
                .unwrap_or(OmegaCommandSource::Builtin);
            let section_id = begin_command_output(&*tx, turn_id, &title, source);
            let mut progress = |text: &str| append_command_output(&*tx, turn_id, &section_id, text);

            let output = match parsed {
                Ok(invocation) => execute_command(
                    &project_state,
                    &session_id,
                    &cwd,
                    invocation,
                    &mut turn_context,
                    &mut progress,
                ),
                Err(error) => Err(anyhow::anyhow!(error)),
            };

            let archive_data = build_turn_archive(turn_id, &turn_context);
            {
                let mut shared = session_context.lock().unwrap();
                *shared = turn_context;
            }
            let context_facade = active_project_handle(&project_state).context_facade();
            if let Err(error) = context_facade.memory.archive_turn(&archive_data) {
                error!(turn_id, error = %error, "failed to archive command turn memory");
            } else if let Ok(snapshot) = context_facade.memory.diagnostics_snapshot() {
                context_facade.diagnostics.record_memory_snapshot(&snapshot);
            }

            match output {
                Ok(output) => emit_command_output(&*tx, turn_id, &section_id, output),
                Err(error) => emit_command_output(
                    &*tx,
                    turn_id,
                    &section_id,
                    CommandExecutionOutput {
                        body: format!("Error: {error}"),
                        state: ResponseSectionState::Failed,
                        activity: format!("{} failed", command_title_from_input(&input)),
                        knowledge_summary: None,
                    },
                ),
            }

            let (session_title, latest_user_turn_preview) = {
                let shared = session_context.lock().unwrap();
                (
                    session_title_from_context(&shared),
                    preview_text(&shared.latest_user_turn, 160),
                )
            };

            if let Ok(snapshot) = active_project_handle(&project_state)
                .upsert_session(ProjectSessionUpdate {
                    session_id: session_id.clone(),
                    title: Some(session_title),
                    status: ProjectSessionStatus::Idle,
                    turn_count: turn_id,
                    last_user_turn_preview: Some(latest_user_turn_preview),
                })
                .and_then(|_| active_project_handle(&project_state).detail_snapshot())
            {
                ui_emit::send_project_status(&*tx, turn_id, snapshot);
            }

            if let Err(error) = rebind_agent_to_current_project(
                &agent_slot,
                &project_state,
                &cwd,
                &client,
                &runtime_bindings,
                &base_system,
                &todo_manager,
                &bash_allowed_commands,
                batch_max_requests,
                max_output_tokens,
            ) {
                error!(turn_id, error = %error, "failed to rebind agent after project command");
            }

            ui_emit::send_turn_finished(&*tx, turn_id);
        });

        Ok(())
    }
}

#[cfg(test)]
pub(crate) struct RuntimeBindingsDebugSnapshot {
    pub(crate) cwd: std::path::PathBuf,
    pub(crate) skill_descriptions: Vec<String>,
    pub(crate) available_tool_ids: Vec<String>,
    pub(crate) hook_ids: Vec<String>,
}

#[derive(Debug)]
struct CommandExecutionOutput {
    body: String,
    state: ResponseSectionState,
    activity: String,
    knowledge_summary: Option<StepKnowledgeSummary>,
}

fn command_registry(project_handle: &Arc<OmegaProjectHandle>) -> OmegaCommandRegistry {
    let facade = project_handle.context_facade();
    OmegaCommandRegistry::new(vec![
        OmegaCommandDescriptor::new(
            "document",
            vec!["doc".to_string()],
            None,
            vec![
                OmegaCommandSubcommand::new("init", "Initialize document indexes", None),
                OmegaCommandSubcommand::new("sync", "Refresh indexed workspace documents", None),
                OmegaCommandSubcommand::new("health", "Check repository document health", None),
                OmegaCommandSubcommand::new(
                    "query",
                    "Search indexed project documents",
                    Some("<text>"),
                ),
                OmegaCommandSubcommand::new(
                    "create",
                    "Create a managed document from a template",
                    Some("<path> <doc_type> <title...>"),
                ),
                OmegaCommandSubcommand::new(
                    "archive",
                    "Archive a managed document",
                    Some("<path> [reason] [replaced_by]"),
                ),
                OmegaCommandSubcommand::new(
                    "list",
                    "List tracked documents",
                    Some("[doc_type] [status]"),
                ),
            ],
            "Manage workspace document indexing and query operations.",
            OmegaCommandSource::Builtin,
            Arc::new(move || facade.document_backend_enabled),
        ),
        OmegaCommandDescriptor::new(
            "project",
            vec!["proj".to_string()],
            None,
            vec![
                OmegaCommandSubcommand::new("list", "List resolved projects", None),
                OmegaCommandSubcommand::new("switch", "Switch active project", Some("<path>")),
                OmegaCommandSubcommand::new("info", "Show current project summary", None),
                OmegaCommandSubcommand::new("sessions", "Show project sessions", None),
                OmegaCommandSubcommand::new("knowledge", "Show project knowledge summary", None),
                OmegaCommandSubcommand::new(
                    "delete",
                    "Delete inactive project state",
                    Some("<project-id|path>"),
                ),
            ],
            "Manage project selection, sessions, and knowledge ownership.",
            OmegaCommandSource::Builtin,
            Arc::new(|| true),
        ),
    ])
}

fn execute_command(
    project_state: &Arc<Mutex<ProjectRuntimeState>>,
    session_id: &str,
    cwd: &Arc<Mutex<std::path::PathBuf>>,
    invocation: OmegaCommandInvocation,
    turn_context: &mut SessionContext,
    progress: &mut dyn FnMut(&str),
) -> anyhow::Result<CommandExecutionOutput> {
    match invocation.name.as_str() {
        "document" => {
            let handle = active_project_handle(project_state);
            execute_document_command(&handle.context_facade(), invocation, turn_context, progress)
        }
        "project" => execute_project_command(
            project_state,
            session_id,
            cwd,
            invocation,
            turn_context,
            progress,
        ),
        _ => Err(anyhow::anyhow!("unsupported command '/{}'", invocation.name)),
    }
}

fn execute_project_command(
    project_state: &Arc<Mutex<ProjectRuntimeState>>,
    session_id: &str,
    cwd: &Arc<Mutex<std::path::PathBuf>>,
    invocation: OmegaCommandInvocation,
    turn_context: &mut SessionContext,
    progress: &mut dyn FnMut(&str),
) -> anyhow::Result<CommandExecutionOutput> {
    let Some(subcommand) = invocation.subcommand.as_deref() else {
        return Err(anyhow::anyhow!(
            "missing subcommand for '/project'; expected list, switch, info, sessions, knowledge, or delete"
        ));
    };

    match subcommand {
        "list" => {
            let state = project_state.lock().unwrap();
            let records = state.registry.list();
            let active_id = state.active_handle.project_id();
            Ok(CommandExecutionOutput {
                body: render_project_list(&records, &active_id),
                state: ResponseSectionState::Complete,
                activity: format!("/project list returned {} projects", records.len()),
                knowledge_summary: None,
            })
        }
        "info" => {
            let snapshot = active_project_handle(project_state).detail_snapshot()?;
            Ok(CommandExecutionOutput {
                body: render_project_info(&snapshot),
                state: ResponseSectionState::Complete,
                activity: "/project info completed".to_string(),
                knowledge_summary: None,
            })
        }
        "sessions" => {
            let snapshot = active_project_handle(project_state).detail_snapshot()?;
            Ok(CommandExecutionOutput {
                body: render_project_sessions(&snapshot),
                state: ResponseSectionState::Complete,
                activity: format!("/project sessions returned {} sessions", snapshot.sessions.len()),
                knowledge_summary: None,
            })
        }
        "knowledge" => {
            let snapshot = active_project_handle(project_state).detail_snapshot()?;
            Ok(CommandExecutionOutput {
                body: render_project_knowledge(&snapshot),
                state: ResponseSectionState::Complete,
                activity: "/project knowledge completed".to_string(),
                knowledge_summary: None,
            })
        }
        "switch" => {
            let target = invocation.args.join(" ");
            if target.trim().is_empty() {
                return Err(anyhow::anyhow!("usage: /project switch <path>"));
            }
            progress("Phase: resolve target project root");
            let explicit_root = std::path::PathBuf::from(target.trim());
            let (registry, current_handle) = {
                let state = project_state.lock().unwrap();
                (Arc::clone(&state.registry), Arc::clone(&state.active_handle))
            };
            let next_handle = registry.resolve(ProjectResolutionInput {
                current_file_path: None,
                cwd: explicit_root.clone(),
                explicit_root: Some(explicit_root),
            })?;
            if next_handle.project_id() == current_handle.project_id() {
                return Ok(CommandExecutionOutput {
                    body: format!(
                        "Project unchanged: {} ({})",
                        next_handle.display_name(),
                        next_handle.root().display()
                    ),
                    state: ResponseSectionState::Complete,
                    activity: "/project switch left active project unchanged".to_string(),
                    knowledge_summary: None,
                });
            }

            current_handle.upsert_session(ProjectSessionUpdate {
                session_id: session_id.to_string(),
                title: Some(session_title_from_context(turn_context)),
                status: ProjectSessionStatus::Idle,
                turn_count: 0,
                last_user_turn_preview: Some(preview_text(&turn_context.latest_user_turn, 160)),
            })?;
            next_handle.upsert_session(ProjectSessionUpdate {
                session_id: session_id.to_string(),
                title: Some(session_title_from_context(turn_context)),
                status: ProjectSessionStatus::Active,
                turn_count: 0,
                last_user_turn_preview: Some(preview_text(&turn_context.latest_user_turn, 160)),
            })?;
            {
                let mut state = project_state.lock().unwrap();
                state.active_handle = Arc::clone(&next_handle);
            }
            *cwd.lock().unwrap() = next_handle.root();
            progress("Phase: switched active project handle");
            let snapshot = next_handle.detail_snapshot()?;
            Ok(CommandExecutionOutput {
                body: format!(
                    "Active project switched to {}\nRoot: {}\nProject ID: {}\nSessions: {}",
                    snapshot.record.display_name,
                    snapshot.record.root.display(),
                    snapshot.record.project_id,
                    snapshot.sessions.len(),
                ),
                state: ResponseSectionState::Complete,
                activity: format!("/project switch -> {}", snapshot.record.display_name),
                knowledge_summary: None,
            })
        }
        "delete" => {
            let target = invocation.args.join(" ");
            if target.trim().is_empty() {
                return Err(anyhow::anyhow!("usage: /project delete <project-id|path>"));
            }
            let (registry, active_handle) = {
                let state = project_state.lock().unwrap();
                (Arc::clone(&state.registry), Arc::clone(&state.active_handle))
            };
            let target_handle = resolve_known_project_target(&registry, target.trim())?;
            if target_handle.project_id() == active_handle.project_id() {
                return Err(anyhow::anyhow!(
                    "refusing to delete the active project; switch to another project first"
                ));
            }
            target_handle.delete_local_state()?;
            registry.forget(&target_handle.project_id());
            Ok(CommandExecutionOutput {
                body: format!(
                    "Deleted project state for {}\nRoot: {}",
                    target_handle.display_name(),
                    target_handle.root().display(),
                ),
                state: ResponseSectionState::Complete,
                activity: format!("/project delete removed {}", target_handle.project_id()),
                knowledge_summary: None,
            })
        }
        other => Err(anyhow::anyhow!(
            "unsupported '/project' subcommand '{other}'"
        )),
    }
}

fn execute_document_command(
    context_facade: &Arc<OmegaContextFacade>,
    invocation: OmegaCommandInvocation,
    turn_context: &mut SessionContext,
    progress: &mut dyn FnMut(&str),
) -> anyhow::Result<CommandExecutionOutput> {
    if !context_facade.document_backend_enabled {
        return Err(anyhow::anyhow!(
            "document backend disabled; rebuild with feature 'document-backend' enabled"
        ));
    }

    let Some(subcommand) = invocation.subcommand.as_deref() else {
        return Err(anyhow::anyhow!(
            "missing subcommand for '/document'; expected init, sync, health, query, create, archive, or list"
        ));
    };

    match subcommand {
        "init" | "sync" => {
            record_governance_event(turn_context, format!("document.{subcommand}"));
            context_facade.diagnostics.record_document_usage(
                "/document",
                "builtin_command",
                &format!("subcommand={subcommand}"),
            );
            progress("Phase: load storeignore rules and scan workspace");
            let scan = context_facade.query.scan_workspace()?;
            progress("Phase: scan complete, preparing command summary");
            context_facade.diagnostics.record_document_scan(&scan);
            let ignored_summary = if scan.vector_ignored_paths.is_empty() {
                String::new()
            } else {
                format!(
                    "\nIgnored paths:\n- {}",
                    scan.vector_ignored_paths.join("\n- ")
                )
            };
            let indexed_summary = if scan.indexed_paths.is_empty() {
                String::new()
            } else {
                format!("\nIndexed files:\n- {}", scan.indexed_paths.join("\n- "))
            };
            let embedded_summary = if scan.embedded_paths.is_empty() {
                String::new()
            } else {
                format!(
                    "\nEmbedded files:\n- {}",
                    scan.embedded_paths.join("\n- ")
                )
            };
            Ok(CommandExecutionOutput {
                body: format!(
                    "Phase: scan workspace\nIndexed {} files\nChunks indexed: {}\nDeleted marked: {}\nStoreignored skipped: {}\nManifest: {}\nKeyword index: {}\nActive version: {}\nPending version: {}\nArchived version path: {}{}{}{}",
                    scan.files_indexed,
                    scan.chunks_indexed,
                    scan.deleted_marked,
                    scan.vector_ignored_files,
                    scan.manifest_path,
                    scan.keyword_index_path,
                    format_store_version(scan.active_version.as_ref()),
                    format_store_version(scan.pending_version.as_ref()),
                    scan.archived_version_path.as_deref().unwrap_or("none"),
                    ignored_summary,
                    indexed_summary,
                    embedded_summary,
                ),
                state: ResponseSectionState::Complete,
                activity: format!("/document {subcommand} indexed {} files", scan.files_indexed),
                knowledge_summary: None,
            })
        }
        "health" => {
            record_governance_event(turn_context, "document.health_check");
            context_facade.diagnostics.record_document_usage(
                "/document",
                "builtin_command",
                "subcommand=health",
            );
            let health = context_facade.governance.check_document_health()?;
            context_facade.diagnostics.record_document_health(&health);
            let snapshot = context_facade.diagnostics.context_diagnostics();
            Ok(CommandExecutionOutput {
                body: format!(
                    "Overall health: {}\nHealth status: {}\nTotal docs: {}\nStructure violations: {}\nNaming violations: {}\nBroken crossrefs: {}\nMissing frontmatter: {}\nStale docs: {}\nLast health check: {}\nActive version: {}\nPending version: {}\nPromotion error: {}",
                    health_score_label(health.overall_health),
                    snapshot.document.health_status.as_str(),
                    health.total_docs,
                    health.structure_violations.len(),
                    health.naming_violations.len(),
                    health.broken_crossrefs.len(),
                    health.missing_frontmatter.len(),
                    health.stale_docs.len(),
                    snapshot
                        .document
                        .last_health_check
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "never".to_string()),
                    format_store_version(snapshot.document.active_version.as_ref()),
                    format_store_version(snapshot.document.pending_version.as_ref()),
                    snapshot
                        .document
                        .last_promotion_error
                        .as_deref()
                        .unwrap_or("none"),
                ),
                state: ResponseSectionState::Complete,
                activity: "/document health completed".to_string(),
                knowledge_summary: None,
            })
        }
        "query" => {
            let query_text = invocation.args.join(" ");
            if query_text.trim().is_empty() {
                return Err(anyhow::anyhow!("missing search text for '/document query'"));
            }
            context_facade.diagnostics.record_document_usage(
                "/document",
                "builtin_command",
                &format!("subcommand=query query={query_text}"),
            );
            let scan = context_facade.query.scan_workspace()?;
            context_facade.diagnostics.record_document_scan(&scan);
            let results = context_facade.query.search(SearchQuery {
                text: Some(query_text.clone()),
                mode: SearchMode::Hybrid,
                filters: Vec::new(),
                sort: None,
                max_results: 10,
            })?;
            Ok(CommandExecutionOutput {
                body: render_document_query_results(&query_text, &results),
                state: ResponseSectionState::Complete,
                activity: format!("/document query returned {} results", results.len()),
                knowledge_summary: Some(build_document_query_knowledge_summary(
                    &context_facade.diagnostics.context_diagnostics(),
                    &query_text,
                    &results,
                )),
            })
        }
        "create" => {
            if let Some(path) = invocation.args.first() {
                record_governance_event(turn_context, format!("document.create {path}"));
            }
            if invocation.args.len() < 3 {
                return Err(anyhow::anyhow!(
                    "usage: /document create <path> <doc_type> <title...>"
                ));
            }
            context_facade.diagnostics.record_document_usage(
                "/document",
                "builtin_command",
                "subcommand=create",
            );

            let path = invocation.args[0].clone();
            let doc_type = parse_doc_type(&invocation.args[1]).ok_or_else(|| {
                anyhow::anyhow!("unknown doc_type '{}' for '/document create'", invocation.args[1])
            })?;
            let title = invocation.args[2..].join(" ");
            let result = context_facade.governance.manage_document(DocumentOp::Create {
                mode: DocumentMutationMode::Apply,
                path: path.clone(),
                doc_type,
                title: title.clone(),
                content: document_template(doc_type, &title),
            })?;

            Ok(CommandExecutionOutput {
                body: render_document_operation_result(&result),
                state: state_from_document_result(&result),
                activity: format!("/document create wrote {}", path),
                knowledge_summary: None,
            })
        }
        "archive" => {
            if let Some(path) = invocation.args.first() {
                record_governance_event(turn_context, format!("document.archive {path}"));
            }
            if invocation.args.is_empty() {
                return Err(anyhow::anyhow!(
                    "usage: /document archive <path> [reason] [replaced_by]"
                ));
            }
            context_facade.diagnostics.record_document_usage(
                "/document",
                "builtin_command",
                "subcommand=archive",
            );

            let path = invocation.args[0].clone();
            let reason = invocation
                .args
                .get(1)
                .and_then(|value| parse_archive_trigger(value))
                .unwrap_or(ArchiveTrigger::HistoryOnly);
            let replaced_by = invocation.args.get(2).cloned();
            let result = context_facade.governance.manage_document(DocumentOp::Archive {
                mode: DocumentMutationMode::Apply,
                path: path.clone(),
                reason,
                replaced_by,
            })?;

            Ok(CommandExecutionOutput {
                body: render_document_operation_result(&result),
                state: state_from_document_result(&result),
                activity: format!("/document archive updated {}", path),
                knowledge_summary: None,
            })
        }
        "list" => {
            context_facade.diagnostics.record_document_usage(
                "/document",
                "builtin_command",
                "subcommand=list",
            );
            let doc_type = invocation.args.first().and_then(|value| parse_doc_type(value));
            let status = invocation.args.get(1).and_then(|value| parse_file_status(value));
            let result = context_facade
                .governance
                .manage_document(DocumentOp::List {
                    doc_type,
                    status: status.clone(),
                })?;

            Ok(CommandExecutionOutput {
                body: render_document_list_result(&result.files, doc_type, status),
                state: state_from_document_result(&result),
                activity: format!("/document list returned {} files", result.files.len()),
                knowledge_summary: None,
            })
        }
        other => Err(anyhow::anyhow!(
            "unsupported '/document' subcommand '{other}'"
        )),
    }
}

fn render_document_query_results(
    query_text: &str,
    results: &[omega_context::SearchResult],
) -> String {
    if results.is_empty() {
        return format!("Query: {query_text}\nNo indexed documents matched.");
    }

    let mut body = format!("Query: {query_text}\nResults: {}", results.len());
    for result in results.iter().take(5) {
        body.push_str(&format!(
            "\n- {} ({}, score {:.2})\n  {}",
            result.path,
            search_mode_label(result.mode_used),
            result.score,
            preview_text(&result.preview, 140),
        ));
    }
    body
}

fn render_document_operation_result(result: &omega_context::DocumentOpResult) -> String {
    let mut body = result.message.clone();

    if let Some(mode) = result.mode {
        body.push_str(&format!("\nMode: {}", mutation_mode_label(mode)));
    }

    if !result.files.is_empty() {
        body.push_str(&format!("\nFiles: {}", result.files.len()));
        for file in result.files.iter().take(5) {
            body.push_str(&format!(
                "\n- {} ({}, {})",
                file.path,
                doc_type_label(file.doc_type),
                file_status_label(&file.status),
            ));
        }
    }

    if !result.warnings.is_empty() {
        body.push_str("\nWarnings:");
        for warning in &result.warnings {
            body.push_str(&format!("\n- {warning}"));
        }
    }

    body
}

fn render_document_list_result(
    files: &[FileRecord],
    doc_type: Option<DocType>,
    status: Option<FileStatus>,
) -> String {
    let mut body = format!(
        "Tracked documents: {}\nFilter: doc_type={} status={}",
        files.len(),
        doc_type_label(doc_type),
        status_label(status.as_ref()),
    );

    if files.is_empty() {
        body.push_str("\nNo tracked documents matched.");
        return body;
    }

    for file in files.iter().take(10) {
        body.push_str(&format!(
            "\n- {} ({}, {})",
            file.path,
            doc_type_label(file.doc_type),
            file_status_label(&file.status),
        ));
    }
    body
}

fn render_command_hint(resolution: CommandHintResolution) -> String {
    match resolution {
        CommandHintResolution::TopLevel(commands) => {
            let summary = commands
                .into_iter()
                .map(|command| format_hint_item(&format!("/{}", command.name), &command))
                .collect::<Vec<_>>()
                .join("  •  ");
            format!(" Slash: {summary}")
        }
        CommandHintResolution::Command {
            command,
            subcommands,
        } => {
            let list = subcommands
                .into_iter()
                .take(4)
                .map(|subcommand| format_hint_item(&subcommand.name, &subcommand))
                .collect::<Vec<_>>()
                .join("  •  ");
            if list.is_empty() {
                format_hint_line(&format!("/{}", command.name), &command)
            } else {
                format!(
                    " {}  •  subcommands: {}",
                    format_hint_line(&format!("/{}", command.name), &command),
                    list,
                )
            }
        }
        CommandHintResolution::Ready {
            command,
            subcommand,
            args,
        } => {
            if let Some(subcommand) = subcommand {
                let pending = if args.is_empty() {
                    String::new()
                } else {
                    format!("  •  args: {}", args.join(" "))
                };
                format!(
                    " Ready: /{} {}{}",
                    command.name,
                    format_hint_item(&subcommand.name, &subcommand),
                    pending,
                )
            } else {
                format!(" Ready: {}", format_hint_line(&format!("/{}", command.name), &command))
            }
        }
        CommandHintResolution::Disabled { command, .. } => {
            format!(
                " Slash unavailable: /{} — {}",
                command.name, command.description
            )
        }
        CommandHintResolution::NoMatch { input } => {
            format!(" Slash: no command matches '/{}'", input)
        }
    }
}

fn format_hint_line(label: &str, hint: &CommandHint) -> String {
    let mut line = format!("{} — {}", label, hint.description);
    if let Some(argument_hint) = hint.argument_hint.as_deref() {
        line.push_str(&format!(" {}", argument_hint));
    }
    line
}

fn format_hint_item(label: &str, hint: &CommandHint) -> String {
    match hint.argument_hint.as_deref() {
        Some(argument_hint) => format!("{} {}", label, argument_hint),
        None => format!("{}", label),
    }
}

fn document_template(doc_type: DocType, title: &str) -> String {
    match doc_type {
        DocType::Readme | DocType::Todo | DocType::Changelog => {
            format!("# {title}\n")
        }
        _ => format!("---\nstatus: draft\ntitle: {title}\n---\n\n# {title}\n"),
    }
}

fn parse_doc_type(value: &str) -> Option<DocType> {
    match value.to_ascii_lowercase().as_str() {
        "spec" | "specs" => Some(DocType::Spec),
        "prd" | "prds" => Some(DocType::Prd),
        "guide" | "guides" => Some(DocType::Guide),
        "adr" | "adrs" | "decision" | "decisions" => Some(DocType::Adr),
        "todo" => Some(DocType::Todo),
        "archive" | "archived" => Some(DocType::Archive),
        "readme" => Some(DocType::Readme),
        "changelog" => Some(DocType::Changelog),
        _ => None,
    }
}

fn parse_file_status(value: &str) -> Option<FileStatus> {
    match value.to_ascii_lowercase().as_str() {
        "active" => Some(FileStatus::Active),
        "archived" | "archive" => Some(FileStatus::Archived),
        "deleted" => Some(FileStatus::Deleted),
        _ => None,
    }
}

fn parse_archive_trigger(value: &str) -> Option<ArchiveTrigger> {
    match value.to_ascii_lowercase().as_str() {
        "superseded" => Some(ArchiveTrigger::Superseded),
        "completed" | "completed_and_inactive" | "inactive" => {
            Some(ArchiveTrigger::CompletedAndInactive)
        }
        "outdated" | "structurally_outdated" => Some(ArchiveTrigger::StructurallyOutdated),
        "history" | "history_only" => Some(ArchiveTrigger::HistoryOnly),
        _ => None,
    }
}

fn state_from_document_result(result: &omega_context::DocumentOpResult) -> ResponseSectionState {
    if result.ok {
        ResponseSectionState::Complete
    } else {
        ResponseSectionState::Failed
    }
}

fn mutation_mode_label(mode: DocumentMutationMode) -> &'static str {
    match mode {
        DocumentMutationMode::Check => "check",
        DocumentMutationMode::Plan => "plan",
        DocumentMutationMode::Apply => "apply",
    }
}

fn doc_type_label(doc_type: Option<DocType>) -> &'static str {
    match doc_type {
        Some(DocType::Spec) => "spec",
        Some(DocType::Prd) => "prd",
        Some(DocType::Guide) => "guide",
        Some(DocType::Adr) => "adr",
        Some(DocType::Todo) => "todo",
        Some(DocType::Archive) => "archive",
        Some(DocType::Readme) => "readme",
        Some(DocType::Changelog) => "changelog",
        None => "any",
    }
}

fn status_label(status: Option<&FileStatus>) -> &'static str {
    match status {
        Some(FileStatus::Active) => "active",
        Some(FileStatus::Deleted) => "deleted",
        Some(FileStatus::Archived) => "archived",
        Some(FileStatus::Moved { .. }) => "moved",
        None => "any",
    }
}

fn file_status_label(status: &FileStatus) -> &'static str {
    status_label(Some(status))
}

fn health_score_label(score: HealthScore) -> &'static str {
    match score {
        HealthScore::Good => "good",
        HealthScore::NeedsAttention => "needs_attention",
        HealthScore::Critical => "critical",
    }
}

fn format_store_version(version: Option<&DocumentStoreVersion>) -> String {
    version
        .map(|version| {
            format!(
                "{} rev={} path={}",
                version.version_id, version.manifest_revision, version.storage_path
            )
        })
        .unwrap_or_else(|| "none".to_string())
}

fn search_mode_label(mode: SearchMode) -> &'static str {
    match mode {
        SearchMode::Keyword => "keyword",
        SearchMode::Semantic => "semantic",
        SearchMode::Hybrid => "hybrid",
    }
}

fn append_command_output(
    tx: &dyn RuntimeMessageBridge,
    turn_id: u64,
    section_id: &str,
    text: &str,
) {
    tx.send(RuntimeMessageEnvelope::conversation(
        turn_id,
        ConversationMessage::AppendSection {
            id: section_id.to_string(),
            delta: ResponseSectionDelta::Text(format!("\n{text}")),
        },
    ));
}

fn emit_command_output(
    tx: &dyn RuntimeMessageBridge,
    turn_id: u64,
    section_id: &str,
    output: CommandExecutionOutput,
) {
    let CommandExecutionOutput {
        body,
        state,
        activity,
        knowledge_summary,
    } = output;

    append_command_output(tx, turn_id, section_id, &body);
    if let Some(summary) = knowledge_summary {
        tx.send(RuntimeMessageEnvelope::state(
            turn_id,
            StateMessage::StepKnowledgeSummary {
                section_id: section_id.to_string(),
                summary: Box::new(summary),
            },
        ));
    }
    tx.send(RuntimeMessageEnvelope::conversation(
        turn_id,
        ConversationMessage::CompleteSection {
            id: section_id.to_string(),
            state,
        },
    ));
    tx.send(RuntimeMessageEnvelope::state(
        turn_id,
        StateMessage::Activity {
            source: RuntimeSource::System,
            kind: if state == ResponseSectionState::Failed {
                RuntimeContentKind::Error
            } else {
                RuntimeContentKind::Result
            },
            text: activity,
            priority: None,
        },
    ));
}

fn build_document_query_knowledge_summary(
    context: &ContextDiagnostics,
    query_text: &str,
    results: &[omega_context::SearchResult],
) -> StepKnowledgeSummary {
    let reason = if results.is_empty() {
        if context.document.active_version.is_none() {
            Some("no promoted store version".to_string())
        } else {
            Some("no matches returned".to_string())
        }
    } else {
        None
    };

    StepKnowledgeSummary {
        document: Some(ResponseDocumentKnowledge {
            raw_query: query_text.to_string(),
            planned_queries: vec![query_text.to_string()],
            rewrite_reason: None,
            rewrite_queries: Vec::new(),
            recovery_path: Some("command_query".to_string()),
            readiness: command_document_query_readiness(context),
            query: query_text.to_string(),
            mode: results
                .first()
                .map(|result| search_mode_label(result.mode_used).to_string())
                .unwrap_or_else(|| "hybrid".to_string()),
            degraded_from: results
                .iter()
                .find_map(|result| result.degraded_from.map(search_mode_label))
                .map(ToOwned::to_owned),
            reason,
            result_count: results.len() as u32,
            top_hits: results
                .iter()
                .take(3)
                .map(|result| DocumentHitItem {
                    path: result.path.clone(),
                    preview: preview_text(&result.preview, 140),
                })
                .collect(),
        }),
        memory: None,
    }
}

fn command_document_query_readiness(context: &ContextDiagnostics) -> SupervisionReadiness {
    if let Some(error) = context.document.last_promotion_error.as_ref() {
        if !error.trim().is_empty() {
            return SupervisionReadiness::Failed;
        }
    }

    if context.document.pending_version.is_some() {
        return SupervisionReadiness::Degraded;
    }

    if context.document.active_version.is_none()
        && context.document.total_files_indexed == 0
        && context.document.total_chunks == 0
    {
        return SupervisionReadiness::Uninitialized;
    }

    if context.document.active_version.is_none() {
        return SupervisionReadiness::Degraded;
    }

    if matches!(context.document.governance_health, Some(HealthScore::Critical)) {
        return SupervisionReadiness::Degraded;
    }

    SupervisionReadiness::Ready
}

fn begin_command_output(
    tx: &dyn RuntimeMessageBridge,
    turn_id: u64,
    title: &str,
    source: OmegaCommandSource,
) -> String {
    let section_id = format!("turn-{turn_id}:command");
    tx.send(RuntimeMessageEnvelope::conversation(
        turn_id,
        ConversationMessage::BeginSection {
            section: ResponseSection {
                id: section_id.clone(),
                parent_id: None,
                kind: ResponseSectionKind::Command,
                title: title.to_string(),
                state: ResponseSectionState::Streaming,
                metadata: ResponseSectionMetadata {
                    scene_id: None,
                    origin: SectionOrigin::Command {
                        command_name: title.to_string(),
                        source: source.as_str().to_string(),
                    },
                    step_id: None,
                    step_label: None,
                    subflow_ref: None,
                },
            },
        },
    ));
    tx.send(RuntimeMessageEnvelope::conversation(
        turn_id,
        ConversationMessage::AppendSection {
            id: section_id.clone(),
            delta: ResponseSectionDelta::Text(format!("Running {title}...")),
        },
    ));
    section_id
}

fn command_title_from_input(input: &str) -> String {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return "/command".to_string();
    }
    trimmed
        .split_whitespace()
        .take(2)
        .collect::<Vec<_>>()
        .join(" ")
}

fn build_turn_archive(turn_id: u64, session_context: &SessionContext) -> TurnData {
    TurnData {
        turn_id,
        workflow_id: session_context.routing.active_workflow_id.clone(),
        user_intent: session_context.latest_user_turn.clone(),
        summaries: session_context
            .step_summaries
            .iter()
            .map(|summary| omega_context::ContextStepSummary {
                workflow_id: summary.workflow_id.clone(),
                step_id: summary.step_id.clone(),
                title: summary.title.clone(),
                summary: summary.summary.clone(),
                estimated_tokens: summary.estimated_tokens,
            })
            .collect(),
        signals: build_turn_retention_signals(session_context),
    }
}

fn active_project_handle(project_state: &Arc<Mutex<ProjectRuntimeState>>) -> Arc<OmegaProjectHandle> {
    Arc::clone(&project_state.lock().unwrap().active_handle)
}

fn resolve_known_project_target(
    registry: &Arc<ProjectRegistry>,
    target: &str,
) -> anyhow::Result<Arc<OmegaProjectHandle>> {
    if let Some(record) = registry
        .list()
        .into_iter()
        .find(|record| record.project_id == target)
    {
        return registry.resolve(ProjectResolutionInput {
            current_file_path: None,
            cwd: record.root.clone(),
            explicit_root: Some(record.root),
        });
    }

    registry.resolve(ProjectResolutionInput {
        current_file_path: None,
        cwd: std::path::PathBuf::from(target),
        explicit_root: Some(std::path::PathBuf::from(target)),
    })
}

fn rebind_agent_to_current_project(
    agent_slot: &Arc<Mutex<AgentSlot>>,
    project_state: &Arc<Mutex<ProjectRuntimeState>>,
    cwd: &Arc<Mutex<std::path::PathBuf>>,
    client: &DynLlmClient,
    runtime_bindings: &Arc<Mutex<SessionRuntimeBindings>>,
    base_system: &str,
    todo_manager: &CoreSharedTodoManager,
    bash_allowed_commands: &[String],
    batch_max_requests: usize,
    max_output_tokens: u32,
) -> anyhow::Result<()> {
    let project_handle = active_project_handle(project_state);
    let current_cwd = cwd.lock().unwrap().clone();
    let (next_bindings, dispatcher) = load_runtime_bindings(
        &project_handle,
        current_cwd,
        todo_manager.clone(),
        bash_allowed_commands.to_vec(),
        batch_max_requests,
    )?;
    let system = next_bindings
        .skill_catalog
        .build_system_prompt(base_system, "", &[], &StepSkillRequest::MatchTask);
    let messages = {
        let slot = agent_slot.lock().unwrap();
        slot.agent
            .as_ref()
            .map(|agent| agent.messages().to_vec())
            .unwrap_or_default()
    };
    let mut replacement = Agent::new(client.clone(), system, dispatcher)?;
    replacement.set_max_tokens(max_output_tokens);
    replacement.set_messages(messages);
    let mut slot = agent_slot.lock().unwrap();
    slot.agent = Some(replacement);
    *runtime_bindings.lock().unwrap() = next_bindings;
    Ok(())
}

fn load_runtime_bindings(
    project_handle: &Arc<OmegaProjectHandle>,
    root: std::path::PathBuf,
    todo_manager: CoreSharedTodoManager,
    bash_allowed_commands: Vec<String>,
    batch_max_requests: usize,
) -> anyhow::Result<(SessionRuntimeBindings, omega_core::ToolDispatcher)> {
    let skill_loader = SkillLoader::from_repo_root(&root)?;
    let skill_catalog = Arc::new(SessionSkillCatalog::new(skill_loader));
    let hook_host = Arc::new(HookHost::load(&root)?);
    let dispatcher = omega_core::create_default_tools_with_context_and_todo_manager_and_tool_limits(
        root,
        project_handle.context_facade(),
        todo_manager,
        bash_allowed_commands,
        batch_max_requests,
    );
    let available_manifests = dispatcher.manifest_metadata();
    let default_manifests = available_manifests
        .iter()
        .filter(|manifest| manifest.id != "ask_user_question")
        .cloned()
        .collect::<Vec<_>>();
    let tool_catalog = Arc::new(SessionToolCatalog::with_available_manifests(
        default_manifests,
        available_manifests,
    ));

    Ok((
        SessionRuntimeBindings {
            hook_host,
            skill_catalog,
            tool_catalog,
        },
        dispatcher,
    ))
}

fn session_title(session_id: &str) -> String {
    format!("Session {}", &session_id[..8.min(session_id.len())])
}

fn session_title_from_context(session_context: &SessionContext) -> String {
    let preview = preview_text(&session_context.latest_user_turn, 48);
    if preview.is_empty() {
        "Untitled Session".to_string()
    } else {
        preview
    }
}

fn render_project_list(
    records: &[omega_project::ProjectRecord],
    active_project_id: &str,
) -> String {
    if records.is_empty() {
        return "No resolved projects.".to_string();
    }

    let mut body = format!("Projects: {}", records.len());
    for record in records {
        let active_marker = if record.project_id == active_project_id {
            "active"
        } else {
            "idle"
        };
        body.push_str(&format!(
            "\n- {} [{}] ({})\n  root: {}",
            record.display_name,
            active_marker,
            detection_kind_label(record.detection_kind),
            record.root.display(),
        ));
    }
    body
}

fn render_project_info(snapshot: &ProjectDetailSnapshot) -> String {
    format!(
        "Project: {}\nProject ID: {}\nRoot: {}\nDetection: {}\nActive session: {}\nSessions: {}\nDocument readiness: {}\nMemory readiness: {}",
        snapshot.record.display_name,
        snapshot.record.project_id,
        snapshot.record.root.display(),
        detection_kind_label(snapshot.record.detection_kind),
        snapshot.record.active_session_id.as_deref().unwrap_or("none"),
        snapshot.sessions.len(),
        command_document_query_readiness(&ContextDiagnostics {
            document: snapshot.knowledge.document.clone(),
            memory: snapshot.knowledge.memory.clone(),
            ..ContextDiagnostics::default()
        })
        .as_str(),
        memory_readiness_from_diagnostics(&snapshot.knowledge.memory).as_str(),
    )
}

fn render_project_sessions(snapshot: &ProjectDetailSnapshot) -> String {
    if snapshot.sessions.is_empty() {
        return format!(
            "Project: {}\nNo recorded sessions.",
            snapshot.record.display_name
        );
    }
    let mut body = format!(
        "Project: {}\nSessions: {}",
        snapshot.record.display_name,
        snapshot.sessions.len()
    );
    for session in &snapshot.sessions {
        body.push_str(&format!(
            "\n- {} [{}] turns={} last_active={}\n  preview: {}",
            session.title,
            project_session_status_label(session.status),
            session.turn_count,
            session.last_active_at,
            session
                .last_user_turn_preview
                .as_deref()
                .unwrap_or("none"),
        ));
    }
    body
}

fn render_project_knowledge(snapshot: &ProjectDetailSnapshot) -> String {
    format!(
        "Project: {}\nDocument files: {}\nDocument chunks: {}\nDocument health: {}\nMemory turns: {}\nMemory queries: {}\nObservations: {}",
        snapshot.record.display_name,
        snapshot.knowledge.document.total_files_indexed,
        snapshot.knowledge.document.total_chunks,
        snapshot.knowledge.document.health_status.as_str(),
        snapshot.knowledge.memory.total_turns_archived,
        snapshot.knowledge.memory.memory_query_count,
        snapshot.knowledge.memory.observation_count,
    )
}

fn detection_kind_label(kind: ProjectDetectionKind) -> &'static str {
    match kind {
        ProjectDetectionKind::Explicit => "explicit",
        ProjectDetectionKind::CurrentFile => "current-file",
        ProjectDetectionKind::Cwd => "cwd",
        ProjectDetectionKind::LooseDirectory => "loose-directory",
    }
}

fn project_session_status_label(status: ProjectSessionStatus) -> &'static str {
    match status {
        ProjectSessionStatus::Active => "active",
        ProjectSessionStatus::Idle => "idle",
        ProjectSessionStatus::Archived => "archived",
    }
}

fn memory_readiness_from_diagnostics(
    diagnostics: &ContextMemoryDiagnostics,
) -> SupervisionReadiness {
    if diagnostics.total_turns_archived == 0
        && diagnostics.memory_query_count == 0
        && diagnostics.observation_count == 0
    {
        SupervisionReadiness::Uninitialized
    } else {
        SupervisionReadiness::Ready
    }
}

pub(crate) fn build_turn_retention_signals(session_context: &SessionContext) -> TurnRetentionSignals {
    let mut signals = TurnRetentionSignals::default();

    if let Some(plan_value) = session_context.step_outputs.get(PLAN_STEP_ID) {
        if let Ok(plan) = parse_feature_plan_output(plan_value.clone()) {
            signals.validation_targets.extend(plan.validation_targets);
            if session_context.step_outputs.get(EXECUTE_STEP_ID).is_none() {
                signals
                    .open_tasks
                    .extend(plan.tasks.into_iter().map(|task| task.title));
            }
        }
    }

    if let Some(execute_value) = session_context.step_outputs.get(EXECUTE_STEP_ID) {
        if let Ok(execute) = parse_feature_execute_output(execute_value.clone()) {
            signals.changed_paths.extend(execute.changed_paths);
            signals.completed_tasks.extend(execute.completed_tasks);
            signals.open_tasks.extend(execute.open_tasks);
            signals.validation_targets.extend(
                execute
                    .validation_results
                    .into_iter()
                    .map(|result| result.target),
            );
        }
    }

    signals
        .developer_preferences
        .extend(extract_preference_hints(&session_context.latest_user_turn));
    signals
        .governance_events
        .extend(session_context.governance_events.clone());
    dedupe_signal_values(&mut signals.changed_paths);
    dedupe_signal_values(&mut signals.completed_tasks);
    dedupe_signal_values(&mut signals.open_tasks);
    dedupe_signal_values(&mut signals.validation_targets);
    dedupe_signal_values(&mut signals.developer_preferences);
    dedupe_governance_signal_values(&mut signals.governance_events);
    signals
}

fn extract_preference_hints(text: &str) -> Vec<String> {
    let lowered = text.to_lowercase();
    let looks_like_preference = ["prefer", "always", "never", "must", "should", "keep"]
        .iter()
        .any(|needle| lowered.contains(needle));
    if looks_like_preference {
        vec![preview_text(text, 160)]
    } else {
        Vec::new()
    }
}

fn dedupe_signal_values(values: &mut Vec<String>) {
    let mut seen = std::collections::BTreeSet::new();
    values.retain(|value| {
        let trimmed = value.trim();
        !trimmed.is_empty() && seen.insert(trimmed.to_string())
    });
}

fn dedupe_governance_signal_values(values: &mut Vec<GovernanceEventSignal>) {
    let mut seen = std::collections::BTreeSet::new();
    values.retain(|value| {
        let label = value.label.trim();
        !label.is_empty() && seen.insert((label.to_string(), value.at))
    });
}

fn record_governance_event(turn_context: &mut SessionContext, label: impl Into<String>) {
    turn_context.governance_events.push(GovernanceEventSignal {
        label: label.into(),
        at: current_unix_timestamp(),
    });
}

fn current_unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

pub(crate) fn preview_text(text: &str, limit: usize) -> String {
    let mut chars = text.chars();
    let preview: String = chars.by_ref().take(limit).collect();
    if chars.next().is_some() {
        format!("{}...", preview)
    } else {
        text.to_string()
    }
}

pub(crate) fn preview_json_value(value: &serde_json::Value, limit: usize) -> String {
    preview_text(
        &serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string()),
        limit,
    )
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
