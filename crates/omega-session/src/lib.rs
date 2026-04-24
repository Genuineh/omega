use anyhow::Context;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use omega_command::{
    CommandHint, CommandHintProvider, CommandHintResolution, OmegaCommandDescriptor,
    OmegaCommandInvocation, OmegaCommandRegistry, OmegaCommandSource, OmegaCommandSubcommand,
};
use omega_compression::{
    LedgerSessionContextCompressor, SessionCompactionRequest, SessionContextCompressor,
    SessionContextLoadGoal, SessionContextLoadRequest, session_context_budget_tokens,
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
use omega_core::{
    Agent, CoreSharedTodoManager, DynLlmClient, Message, TodoItem, TodoManager, TodoStatus,
};
use omega_document::{
    DocType as StructuredDocType,
    DocumentMutationMode as StructuredDocumentMutationMode,
    DocumentOp as StructuredDocumentOp, OmegaDocument, StructuredDocumentRecord,
    StructuredDocumentSection, StructuredDocsSnapshot,
};
use omega_hooks::HookHost;
use omega_plan::{
    NewPlannedTask, PlannedTask, PlannedTaskStatus, PlannedTaskUpdate, ProjectPlanAccess,
    ProjectPlanStore, SelectedProjectTaskContext, TaskActor, TaskArtifactKind,
    TaskArtifactLink, TaskDependencyOperation, TaskLinkSurface, TaskListFilter,
    TaskLogEntry, TaskLogKind, TaskOrderPlacement, TaskPriority,
};
use omega_project::{
    OmegaProjectHandle, ProjectDetailSnapshot, ProjectDetectionKind, ProjectRegistry,
    ProjectResolutionInput, ProjectSessionSnapshot, ProjectSessionStatus,
    SessionContextRecord, SessionContextRecordKind,
    ProjectSessionStepSummary, ProjectSessionTodoItem, ProjectSessionTodoStatus,
    ProjectSessionTurnSummary, ProjectSessionUpdate, ProjectSkillRoutingSnapshot,
    ProjectSessionRoutingSnapshot, SessionReplayEntry, SessionReplayEntryKind,
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
const SESSION_PICKER_FLAG: &str = "--picker";
const SESSION_PICKER_ID: &str = "session-operator";
const PLAN_LIST_PICKER_ID: &str = "plan-list";
const PLAN_LINKS_PICKER_ID_PREFIX: &str = "plan-links:";

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
    ActivityTarget, CacheDiagnostics, DocumentNavigatorBody, DocumentNavigatorBodyKind,
    DocumentNavigatorEntry, DocumentNavigatorEntryKind, DocumentNavigatorGroup,
    DocumentNavigatorRequest, ExecuteProgressDiagnostics, OperatorPickerAction,
    OperatorPickerIntent, OperatorPickerItem, OperatorPickerOverlayBehavior,
    OperatorPickerRequest, OperatorPickerShortcut, OverlayRequest, OverlayTarget,
    ResponseSection, ResponseSectionDelta, ResponseSectionKind, ResponseSectionMetadata,
    ResponseSectionState, RuntimeUiBridge, RuntimeUiEffect, RuntimeUiEnvelope,
    RuntimeUiMessage, RuntimeUiSink, SectionOrigin, SessionRuntimeContext,
    SessionRestoreSnapshot, SkillLoadSummary, StatusSlot, StatusValue, StepContextWrite,
    StepContextWriteKind, StepDiagnostics, StepInputDiagnostics, StepInputStatus,
    StepOutputAttemptKind, StepOutputContractMode, StepOutputDiagnostics,
    StepOutputRecoveryDecision, StepOutputStatus, StepSubflowRef, StepSubflowState,
    StepSubflowStatus, StepSummarySource, TokenCountSource, ToolCapabilityDiagnostics,
    ToolRun, ToolRunDetail, ToolRunStatus, UiContent, UiMessageKind, UiPriority,
    UiSource, UiTarget, WorkflowRunRole,
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

struct SessionRuntimeState {
    session_id: Option<String>,
    selected_task_id: Option<String>,
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
    session_runtime: Arc<Mutex<SessionRuntimeState>>,
    plan_store: Arc<Mutex<Option<Arc<ProjectPlanStore>>>>,
    todo_manager: CoreSharedTodoManager,
    runtime_bindings: Arc<Mutex<SessionRuntimeBindings>>,
    runtime_handle: Handle,
    scene_catalog: SceneCatalog,
    workflow_catalog: WorkflowCatalog,
    prompt_catalog: WorkflowPromptCatalog,
    startup_restore_snapshot: Option<SessionRestoreSnapshot>,
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
        let plan_store = Arc::new(ProjectPlanStore::open_or_scaffold(active_project.root())?);
        let todo_manager = Arc::new(Mutex::new(TodoManager::new()));
        let resolved_cwd = config.cwd.clone();
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
        let session_context = SessionContext::new(config.scene_catalog.root_workflow_id.clone());

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
            session_context: Arc::new(Mutex::new(session_context)),
            project_state: Arc::new(Mutex::new(ProjectRuntimeState {
                registry,
                active_handle: active_project,
            })),
            client: config.client,
            base_system: config.system,
            cwd: Arc::new(Mutex::new(resolved_cwd)),
            session_runtime: Arc::new(Mutex::new(SessionRuntimeState {
                session_id: None,
                selected_task_id: None,
            })),
            plan_store: Arc::new(Mutex::new(Some(plan_store))),
            todo_manager,
            runtime_bindings: Arc::new(Mutex::new(runtime_bindings)),
            runtime_handle: config.runtime_handle,
            scene_catalog: config.scene_catalog,
            workflow_catalog: config.workflow_catalog,
            prompt_catalog: config.prompt_catalog,
            startup_restore_snapshot: None,
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

    pub fn startup_restore_snapshot(&self) -> Option<SessionRestoreSnapshot> {
        self.startup_restore_snapshot.clone()
    }

    pub fn has_bound_session(&self) -> bool {
        self.session_runtime.lock().unwrap().session_id.is_some()
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

    fn current_session_id(&self) -> Option<String> {
        self.session_runtime.lock().unwrap().session_id.clone()
    }

    pub fn current_selected_task_id(&self) -> Option<String> {
        self.session_runtime.lock().unwrap().selected_task_id.clone()
    }

    pub fn set_selected_task(&self, task_id: Option<String>) -> anyhow::Result<()> {
        set_selected_task_state(
            &self.plan_store,
            &self.session_runtime,
            &mut self.session_context.lock().unwrap(),
            task_id,
        )?;
        Ok(())
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
        let session_runtime = self.session_runtime.clone();
        let session_id = match self.current_session_id() {
            Some(session_id) => session_id,
            None => {
                let session_id = Uuid::new_v4().simple().to_string();
                project_handle.upsert_session(ProjectSessionUpdate {
                    session_id: session_id.clone(),
                    title: Some(session_title(&session_id)),
                    status: ProjectSessionStatus::Active,
                    turn_count: 0,
                    last_user_turn_preview: None,
                    archived_turn_count: Some(0),
                })?;
                self.session_runtime.lock().unwrap().session_id = Some(session_id.clone());
                session_id
            }
        };
        project_handle.upsert_session(ProjectSessionUpdate {
            session_id: session_id.clone(),
            title: Some(session_title(&session_id)),
            status: ProjectSessionStatus::Active,
            turn_count: turn_id,
            last_user_turn_preview: Some(preview_text(&input, 160)),
            archived_turn_count: None,
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
        let plan_store = self.plan_store.clone();
        let context_window = self.context_window;
        let max_output_tokens = self.max_output_tokens;
        thread::spawn(move || {
            let thread_turn_rx = cancel_turn_rx.clone();
            agent.add_user_message(&input);
            let mut turn_context = {
                let mut shared = session_context.lock().unwrap();
                shared.begin_turn(input.clone(), scene_catalog.root_workflow_id.clone());
                shared.clone()
            };
            let selected_task_id = session_runtime.lock().unwrap().selected_task_id.clone();
            let selected_task_id_for_turn = selected_task_id.clone();
            let restored_plan_store = plan_store.lock().unwrap().clone();
            let restore_outcome = match restore_selected_task_state(
                &session_runtime,
                &mut turn_context,
                selected_task_id,
                &restored_plan_store,
            ) {
                Ok(outcome) => outcome,
                Err(error) => {
                    error!(turn_id, error = %error, "failed to restore selected project task into turn context");
                    SelectedTaskRestoreOutcome::NoSelection
                }
            };
            if let SelectedTaskRestoreOutcome::ClearedDangling(task_id) = restore_outcome {
                ui_emit::send_warning_text(
                    &*tx_result,
                    turn_id,
                    &dangling_selected_task_warning(&task_id),
                );
            }
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
                &session_id,
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
            let archive_data =
                turn_still_active.then(|| build_turn_archive(turn_id, &session_id, &turn_context));
            let session_title = session_title_from_context(&turn_context);
            let latest_user_turn_preview = preview_text(&turn_context.latest_user_turn, 160);
            let replay_entries = turn_still_active.then(|| match &result {
                Ok(output) => build_turn_replay_entries(
                    &session_id,
                    &turn_context.latest_user_turn,
                    &output.final_text,
                ),
                Err(error) => build_turn_replay_entries(
                    &session_id,
                    &turn_context.latest_user_turn,
                    &format!("Error: {error}"),
                ),
            });

            let persisted_turn_context = if turn_still_active {
                Some(turn_context.clone())
            } else {
                None
            };

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

            let archived_turn_count = if turn_still_active {
                match persist_session_artifacts(
                    &project_handle,
                    &session_id,
                    &cwd,
                    persisted_turn_context
                        .as_ref()
                        .expect("active turn must retain a persisted session snapshot"),
                    &todo_manager,
                    selected_task_id_for_turn.clone(),
                    Some(turn_id),
                    replay_entries.as_deref().unwrap_or(&[]),
                ) {
                    Ok(count) => Some(count),
                    Err(error) => {
                        error!(turn_id, error = %error, "failed to persist session snapshot");
                        None
                    }
                }
            } else {
                None
            };

            match &result {
                Ok(output) if turn_still_active && !output.final_text.is_empty() => {
                    ui_emit::send_assistant_text(&*tx_result, turn_id, &output.final_text);
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

            if let Some(task_id) = selected_task_id_for_turn.as_deref() {
                if let Some(store) = restored_plan_store.as_ref() {
                    let log_result = if turn_still_active {
                        match &result {
                            Ok(output) => store.append_log(
                                task_id,
                                omega_plan::TaskLogKind::DeliveryAttached,
                                TaskActor::Assistant,
                                build_task_delivery_log_summary(
                                    &input,
                                    &output.final_text,
                                    &output.delivery_summary,
                                ),
                                Some(session_id.clone()),
                                Some(turn_id),
                                Some(format!("turn-{turn_id}")),
                            ),
                            Err(error) => store.append_log(
                                task_id,
                                omega_plan::TaskLogKind::PartialDelivery,
                                TaskActor::Assistant,
                                build_task_turn_log_summary(&input, None, Some(&error.to_string())),
                                Some(session_id.clone()),
                                Some(turn_id),
                                None,
                            ),
                        }
                    } else {
                        store.append_log(
                            task_id,
                            omega_plan::TaskLogKind::PartialDelivery,
                            TaskActor::System,
                            format!(
                                "Turn canceled before completion. Prompt: {}",
                                preview_text(&input, 120)
                            ),
                            Some(session_id.clone()),
                            Some(turn_id),
                            None,
                        )
                    };
                    if let Err(error) = log_result {
                        error!(turn_id, task_id, error = %error, "failed to append task-bound turn log");
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
                        status: ProjectSessionStatus::Active,
                        turn_count: turn_id,
                        last_user_turn_preview: Some(latest_user_turn_preview),
                        archived_turn_count,
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
        if self.try_dispatch_plan_send(&input, turn_id, tx.clone())? {
            return Ok(());
        }
        self.active_turn_tx.send_replace(turn_id);
        let project_state = self.project_state.clone();
        let session_id = self.current_session_id();
        let session_context = self.session_context.clone();
        let scene_catalog = self.scene_catalog.clone();
        let cwd = self.cwd.clone();
        let agent_slot = self.agent_slot.clone();
        let turn_checkpoint = self.turn_checkpoint.clone();
        let session_runtime = self.session_runtime.clone();
        let plan_store = self.plan_store.clone();
        let client = self.client.clone();
        let base_system = self.base_system.clone();
        let todo_manager = self.todo_manager.clone();
        let runtime_bindings = self.runtime_bindings.clone();
        let bash_allowed_commands = self.bash_allowed_commands.clone();
        let max_output_tokens = self.max_output_tokens;
        let batch_max_requests = self.batch_max_requests;

        thread::spawn(move || {
            let (previous_session_context, mut turn_context) = {
                let mut shared = session_context.lock().unwrap();
                let previous = shared.clone();
                shared.begin_turn(input.clone(), scene_catalog.root_workflow_id.clone());
                (previous, shared.clone())
            };
            let registry = command_registry(&active_project_handle(&project_state));
            let parsed = registry.parse(&input);
            let title = command_title_from_input(&input);
            let source = parsed
                .as_ref()
                .map(|invocation| invocation.source)
                .unwrap_or(OmegaCommandSource::Builtin);
            let overlay_only = parsed
                .as_ref()
                .map(command_prefers_overlay_only)
                .unwrap_or(false);
            let section_id = if overlay_only {
                None
            } else {
                Some(begin_command_output(&*tx, turn_id, &title, source))
            };
            let mut progress = |text: &str| {
                if let Some(section_id) = section_id.as_deref() {
                    append_command_output(&*tx, turn_id, section_id, text);
                }
            };

            let output = match parsed {
                Ok(invocation) => execute_command(
                    &project_state,
                    &session_runtime,
                    &plan_store,
                    &session_context,
                    &todo_manager,
                    &turn_checkpoint,
                    &agent_slot,
                    &client,
                    &runtime_bindings,
                    &base_system,
                    &bash_allowed_commands,
                    batch_max_requests,
                    max_output_tokens,
                    turn_id,
                    &*tx,
                    session_id.as_deref(),
                    &scene_catalog.root_workflow_id,
                    &cwd,
                    invocation,
                    &previous_session_context,
                    &mut turn_context,
                    &mut progress,
                ),
                Err(error) => Err(anyhow::anyhow!(error)),
            };

            let active_session_id = session_runtime.lock().unwrap().session_id.clone();
            let replay_entries = active_session_id
                .as_deref()
                .map(|active_session_id| match &output {
                    Ok(output) => build_command_replay_entries(
                        active_session_id,
                        &input,
                        &output.body,
                        output.state,
                    ),
                    Err(error) => build_command_replay_entries(
                        active_session_id,
                        &input,
                        &format!("Error: {error}"),
                        ResponseSectionState::Failed,
                    ),
                })
                .unwrap_or_default();

            let archive_data = active_session_id
                .as_deref()
                .map(|active_session_id| build_turn_archive(turn_id, active_session_id, &turn_context));
            {
                let mut shared = session_context.lock().unwrap();
                *shared = turn_context;
            }
            let context_facade = active_project_handle(&project_state).context_facade();
            if let Some(archive_data) = archive_data.as_ref() {
                if let Err(error) = context_facade.memory.archive_turn(archive_data) {
                    error!(turn_id, error = %error, "failed to archive command turn memory");
                } else if let Ok(snapshot) = context_facade.memory.diagnostics_snapshot() {
                    context_facade.diagnostics.record_memory_snapshot(&snapshot);
                }
            }

            let archived_turn_count = active_session_id.as_deref().and_then(|active_session_id| {
                match persist_session_artifacts(
                    &active_project_handle(&project_state),
                    active_session_id,
                    &cwd.lock().unwrap().clone(),
                    &session_context.lock().unwrap().clone(),
                    &todo_manager,
                    session_runtime.lock().unwrap().selected_task_id.clone(),
                    Some(turn_id),
                    &replay_entries,
                ) {
                    Ok(count) => Some(count),
                    Err(error) => {
                        error!(turn_id, error = %error, "failed to persist command session snapshot");
                        None
                    }
                }
            });

            let replacement_messages = match &output {
                Ok(output) => output.agent_messages.clone(),
                Err(_) => None,
            };

            match output {
                Ok(output) => emit_command_output(
                    &*tx,
                    turn_id,
                    &title,
                    source,
                    section_id.as_deref(),
                    output,
                ),
                Err(error) => emit_command_output(
                    &*tx,
                    turn_id,
                    &title,
                    source,
                    section_id.as_deref(),
                    CommandExecutionOutput {
                        body: format!("Error: {error}"),
                        state: ResponseSectionState::Failed,
                        activity: format!("{} failed", command_title_from_input(&input)),
                        knowledge_summary: None,
                        agent_messages: None,
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

            let project_snapshot = if let Some(active_session_id) = active_session_id.as_deref() {
                active_project_handle(&project_state)
                    .upsert_session(ProjectSessionUpdate {
                        session_id: active_session_id.to_string(),
                        title: Some(session_title),
                        status: ProjectSessionStatus::Active,
                        turn_count: turn_id,
                        last_user_turn_preview: Some(latest_user_turn_preview),
                        archived_turn_count,
                    })
                    .and_then(|_| active_project_handle(&project_state).detail_snapshot())
            } else {
                active_project_handle(&project_state).detail_snapshot()
            };

            if let Ok(snapshot) = project_snapshot {
                ui_emit::send_project_status(&*tx, turn_id, snapshot);
            }

            let rebind_result = if let Some(messages) = replacement_messages {
                rebind_agent_to_current_project_with_messages(
                    &agent_slot,
                    &turn_checkpoint,
                    &project_state,
                    &cwd,
                    &client,
                    &runtime_bindings,
                    &base_system,
                    &todo_manager,
                    &bash_allowed_commands,
                    batch_max_requests,
                    max_output_tokens,
                    messages,
                )
            } else {
                rebind_agent_to_current_project(
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
                )
            };

            if let Err(error) = rebind_result {
                error!(turn_id, error = %error, "failed to rebind agent after project command");
            }

            ui_emit::send_turn_finished(&*tx, turn_id);
        });

        Ok(())
    }

    fn try_dispatch_plan_send(
        &self,
        input: &str,
        turn_id: u64,
        tx: SharedRuntimeMessageBridge,
    ) -> anyhow::Result<bool> {
        let registry = command_registry(&self.active_project_handle());
        let Ok(invocation) = registry.parse(input) else {
            return Ok(false);
        };
        if invocation.name != "plan" || invocation.subcommand.as_deref() != Some("send") {
            return Ok(false);
        }

        let current_selected_task_id = self.current_selected_task_id();
        let Ok((task_id, prompt)) = parse_plan_send_args(&invocation.args, current_selected_task_id) else {
            return Ok(false);
        };
        self.set_selected_task(Some(task_id))?;
        self.spawn_turn_with_bridge(prompt, turn_id, tx)?;
        Ok(true)
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
    agent_messages: Option<Vec<Message>>,
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
                OmegaCommandSubcommand::new(
                    "render",
                    "Render docs-data records into presentation markdown",
                    Some("[--check|--plan|--apply] [doc-id...]"),
                ),
                OmegaCommandSubcommand::new(
                    "validate",
                    "Validate docs-data projection against current markdown",
                    Some("[doc-id...]"),
                ),
                OmegaCommandSubcommand::new(
                    "extract",
                    "Extract markdown sources into docs-data records",
                    Some("<source...> [--doc-type <...>] [--check|--plan|--apply]"),
                ),
            ],
            "Manage workspace document indexing and query operations.",
            OmegaCommandSource::Builtin,
            Arc::new(move || facade.document_backend_enabled),
        ),
        OmegaCommandDescriptor::new(
            "plan",
            vec![],
            None,
            vec![
                OmegaCommandSubcommand::new(
                    "list",
                    "List project plan tasks",
                    Some("[current|history|all] [--status <...>] [--priority <...>]"),
                ),
                OmegaCommandSubcommand::new("show", "Show a task from the project plan", Some("<task-id>")),
                OmegaCommandSubcommand::new(
                    "create",
                    "Create a task in the project plan",
                    Some("[--priority <p0|p1|p2|p3>] <title...>"),
                ),
                OmegaCommandSubcommand::new(
                    "update",
                    "Update task fields in the project plan",
                    Some("<task-id> [--title <...>] [--summary <...>] [--requirement <...>] [--status <...>] [--accept <...>] [--tag <...>]"),
                ),
                OmegaCommandSubcommand::new(
                    "prioritize",
                    "Change task priority and relative ordering",
                    Some("<task-id> <p0|p1|p2|p3> [--before <task-id>] [--after <task-id>]"),
                ),
                OmegaCommandSubcommand::new(
                    "depends",
                    "Add or remove task dependencies",
                    Some("<add|remove> <task-id> <other-id>"),
                ),
                OmegaCommandSubcommand::new(
                    "log",
                    "Append a note to a task log",
                    Some("<task-id> <note...>"),
                ),
                OmegaCommandSubcommand::new(
                    "link",
                    "Link a design or implementation artifact",
                    Some("<task-id> <design|implementation> <path> [label...]"),
                ),
                OmegaCommandSubcommand::new(
                    "links",
                    "Open the task link navigator overlay",
                    Some("<task-id>"),
                ),
                OmegaCommandSubcommand::new(
                    "view-file",
                    "Open a task-linked file in the detail overlay",
                    Some("<path>"),
                ),
                OmegaCommandSubcommand::new(
                    "open-link",
                    "Open a task navigator item in the detail overlay",
                    Some("<task-id> <path-or-log-entry>"),
                ),
                OmegaCommandSubcommand::new("select", "Bind or clear the current session task", Some("<task-id|none>")),
                OmegaCommandSubcommand::new(
                    "send",
                    "Dispatch a normal task-bound turn",
                    Some("[<task-id>] <prompt...>"),
                ),
                OmegaCommandSubcommand::new(
                    "sync-todo",
                    "Project current open work into docs/TODO.md",
                    None,
                ),
                OmegaCommandSubcommand::new(
                    "migrate-todo",
                    "Import open docs/TODO.md tracks into the project plan",
                    None,
                ),
                OmegaCommandSubcommand::new(
                    "load",
                    "Preview or import plan tasks from docs files or directories",
                    Some("<source> [--kind <auto|todo|task-headings>] [--apply]"),
                ),
            ],
            "Manage the project-scoped long-term task board.",
            OmegaCommandSource::Builtin,
            Arc::new(|| true),
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
        OmegaCommandDescriptor::new(
            "session",
            vec!["sess".to_string()],
            None,
            vec![
                OmegaCommandSubcommand::new("list", "List project sessions", Some("[status]")),
                OmegaCommandSubcommand::new("info", "Show session details", Some("<session-id>")),
                OmegaCommandSubcommand::new("new", "Start a new session", Some("[title...]")),
                OmegaCommandSubcommand::new("resume", "Resume a previous session", Some("[session-id]")),
                OmegaCommandSubcommand::new("switch", "Alias for resume", Some("[session-id]")),
                OmegaCommandSubcommand::new("archive", "Archive a session", Some("<session-id>")),
                OmegaCommandSubcommand::new("delete", "Delete a session", Some("<session-id>")),
            ],
            "Manage sessions within the current project.",
            OmegaCommandSource::Builtin,
            Arc::new(|| true),
        ),
    ])
}

fn execute_command(
    project_state: &Arc<Mutex<ProjectRuntimeState>>,
    session_runtime: &Arc<Mutex<SessionRuntimeState>>,
    plan_store: &Arc<Mutex<Option<Arc<ProjectPlanStore>>>>,
    session_context: &Arc<Mutex<SessionContext>>,
    todo_manager: &CoreSharedTodoManager,
    turn_checkpoint: &Arc<Mutex<Vec<Message>>>,
    agent_slot: &Arc<Mutex<AgentSlot>>,
    client: &DynLlmClient,
    runtime_bindings: &Arc<Mutex<SessionRuntimeBindings>>,
    base_system: &str,
    bash_allowed_commands: &[String],
    batch_max_requests: usize,
    max_output_tokens: u32,
    turn_id: u64,
    tx: &dyn RuntimeMessageBridge,
    session_id: Option<&str>,
    root_workflow_id: &str,
    cwd: &Arc<Mutex<std::path::PathBuf>>,
    invocation: OmegaCommandInvocation,
    previous_session_context: &SessionContext,
    turn_context: &mut SessionContext,
    progress: &mut dyn FnMut(&str),
) -> anyhow::Result<CommandExecutionOutput> {
    match invocation.name.as_str() {
        "document" => {
            let handle = active_project_handle(project_state);
            let project_root = handle.root();
            execute_document_command(
                &project_root,
                &handle.context_facade(),
                turn_id,
                tx,
                invocation,
                turn_context,
                progress,
            )
        }
        "project" => execute_project_command(
            project_state,
            session_id,
            session_runtime,
            plan_store,
            cwd,
            invocation,
            turn_context,
            progress,
        ),
        "plan" => execute_plan_command(
            project_state,
            plan_store,
            session_runtime,
            turn_id,
            tx,
            invocation,
            turn_context,
        ),
        "session" => execute_session_command(
            project_state,
            session_runtime,
            plan_store,
            session_context,
            todo_manager,
            turn_checkpoint,
            agent_slot,
            client,
            runtime_bindings,
            base_system,
            bash_allowed_commands,
            batch_max_requests,
            max_output_tokens,
            turn_id,
            tx,
            session_id,
            root_workflow_id,
            cwd,
            invocation,
            previous_session_context,
            turn_context,
            progress,
        ),
        _ => Err(anyhow::anyhow!("unsupported command '/{}'", invocation.name)),
    }
}

fn command_prefers_overlay_only(invocation: &OmegaCommandInvocation) -> bool {
    if invocation.name != "session" {
        return false;
    }

    let picker_mode = invocation.args.iter().any(|arg| arg == SESSION_PICKER_FLAG);
    let non_hidden_args = invocation
        .args
        .iter()
        .filter(|arg| arg.as_str() != SESSION_PICKER_FLAG)
        .count();

    match invocation.subcommand.as_deref() {
        None | Some("list") | Some("info") => true,
        Some("resume") | Some("switch") => picker_mode || non_hidden_args == 0,
        Some("new") | Some("archive") | Some("delete") => picker_mode,
        _ => false,
    }
}

fn execute_project_command(
    project_state: &Arc<Mutex<ProjectRuntimeState>>,
    session_id: Option<&str>,
    session_runtime: &Arc<Mutex<SessionRuntimeState>>,
    plan_store: &Arc<Mutex<Option<Arc<ProjectPlanStore>>>>,
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
                agent_messages: None,
            })
        }
        "info" => {
            let snapshot = active_project_handle(project_state).detail_snapshot()?;
            Ok(CommandExecutionOutput {
                body: render_project_info(&snapshot),
                state: ResponseSectionState::Complete,
                activity: "/project info completed".to_string(),
                knowledge_summary: None,
                agent_messages: None,
            })
        }
        "sessions" => {
            let snapshot = active_project_handle(project_state).detail_snapshot()?;
            Ok(CommandExecutionOutput {
                body: render_project_sessions(&snapshot),
                state: ResponseSectionState::Complete,
                activity: format!("/project sessions returned {} sessions", snapshot.sessions.len()),
                knowledge_summary: None,
                agent_messages: None,
            })
        }
        "knowledge" => {
            let snapshot = active_project_handle(project_state).detail_snapshot()?;
            Ok(CommandExecutionOutput {
                body: render_project_knowledge(&snapshot),
                state: ResponseSectionState::Complete,
                activity: "/project knowledge completed".to_string(),
                knowledge_summary: None,
                agent_messages: None,
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
                    agent_messages: None,
                });
            }

            if let Some(session_id) = session_id {
                current_handle.upsert_session(ProjectSessionUpdate {
                    session_id: session_id.to_string(),
                    title: Some(session_title_from_context(turn_context)),
                    status: ProjectSessionStatus::Idle,
                    turn_count: 0,
                    last_user_turn_preview: Some(preview_text(&turn_context.latest_user_turn, 160)),
                    archived_turn_count: None,
                })?;
                next_handle.upsert_session(ProjectSessionUpdate {
                    session_id: session_id.to_string(),
                    title: Some(session_title_from_context(turn_context)),
                    status: ProjectSessionStatus::Active,
                    turn_count: 0,
                    last_user_turn_preview: Some(preview_text(&turn_context.latest_user_turn, 160)),
                    archived_turn_count: None,
                })?;
            }
            {
                let mut state = project_state.lock().unwrap();
                state.active_handle = Arc::clone(&next_handle);
            }
            *plan_store.lock().unwrap() = Some(Arc::new(ProjectPlanStore::open_or_scaffold(
                next_handle.root(),
            )?));
            session_runtime.lock().unwrap().selected_task_id = None;
            turn_context.selected_task = None;
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
                agent_messages: None,
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
                agent_messages: None,
            })
        }
        other => Err(anyhow::anyhow!(
            "unsupported '/project' subcommand '{other}'"
        )),
    }
}

fn execute_plan_command(
    project_state: &Arc<Mutex<ProjectRuntimeState>>,
    plan_store: &Arc<Mutex<Option<Arc<ProjectPlanStore>>>>,
    session_runtime: &Arc<Mutex<SessionRuntimeState>>,
    turn_id: u64,
    tx: &dyn RuntimeMessageBridge,
    invocation: OmegaCommandInvocation,
    turn_context: &mut SessionContext,
) -> anyhow::Result<CommandExecutionOutput> {
    let Some(subcommand) = invocation.subcommand.as_deref() else {
        return Err(anyhow::anyhow!(
            "missing subcommand for '/plan'; expected list, show, create, update, prioritize, depends, log, link, links, view-file, select, send, sync-todo, migrate-todo, or load"
        ));
    };

    let store = current_plan_store(plan_store)?;
    match subcommand {
        "list" => {
            let (view, filter) = parse_plan_list_args(&invocation.args)?;
            let mut tasks = store.list_tasks(filter)?;
            tasks.retain(|task| match view.as_str() {
                "all" => true,
                "history" => matches!(task.status, PlannedTaskStatus::Done | PlannedTaskStatus::Archived),
                _ => !matches!(task.status, PlannedTaskStatus::Done | PlannedTaskStatus::Archived),
            });
            let selected_task_id = session_runtime.lock().unwrap().selected_task_id.clone();
            let request = build_plan_list_picker_request(
                &store,
                &tasks,
                selected_task_id.as_deref(),
                &view,
            )?;
            tx.send(RuntimeMessageEnvelope::state(
                turn_id,
                StateMessage::ShowOverlay {
                    request: OverlayRequest {
                        target: OverlayTarget::Picker,
                        content: UiContent::OperatorPicker(request),
                    },
                },
            ));

            Ok(CommandExecutionOutput {
                body: render_plan_list(
                    &tasks,
                    selected_task_id.as_deref(),
                    &view,
                ),
                state: ResponseSectionState::Complete,
                activity: format!("/plan list returned {} tasks", tasks.len()),
                knowledge_summary: None,
                agent_messages: None,
            })
        }
        "show" => {
            let task_id = invocation
                .args
                .first()
                .ok_or_else(|| anyhow::anyhow!("usage: /plan show <task-id>"))?;
            let task = store
                .get_task(task_id)?
                .ok_or_else(|| anyhow::anyhow!("unknown task '{task_id}'"))?;
            Ok(CommandExecutionOutput {
                body: render_plan_task_detail(&store, &task)?,
                state: ResponseSectionState::Complete,
                activity: format!("/plan show loaded {}", task.id),
                knowledge_summary: None,
                agent_messages: None,
            })
        }
        "links" => {
            let task_id = invocation
                .args
                .first()
                .ok_or_else(|| anyhow::anyhow!("usage: /plan links <task-id>"))?;
            let task = store
                .get_task(task_id)?
                .ok_or_else(|| anyhow::anyhow!("unknown task '{task_id}'"))?;
            let project_root = active_project_handle(project_state).root();
            let request = build_plan_links_picker_request(&project_root, &store, &task)?;
            tx.send(RuntimeMessageEnvelope::state(
                turn_id,
                StateMessage::ShowOverlay {
                    request: OverlayRequest {
                        target: OverlayTarget::Picker,
                        content: UiContent::OperatorPicker(request),
                    },
                },
            ));

            Ok(CommandExecutionOutput {
                body: String::new(),
                state: ResponseSectionState::Complete,
                activity: format!("/plan links loaded {}", task.id),
                knowledge_summary: None,
                agent_messages: None,
            })
        }
        "view-file" => {
            let relative_path = parse_plan_view_file_args(&invocation.args)?;
            let project_root = active_project_handle(project_state).root();
            let request = build_plan_view_file_navigator_request(&project_root, &relative_path)?;
            tx.send(RuntimeMessageEnvelope::state(
                turn_id,
                StateMessage::ShowOverlay {
                    request: OverlayRequest {
                        target: OverlayTarget::Detail,
                        content: UiContent::DocumentNavigator(request),
                    },
                },
            ));

            Ok(CommandExecutionOutput {
                body: String::new(),
                state: ResponseSectionState::Complete,
                activity: format!("/plan view-file opened {}", relative_path),
                knowledge_summary: None,
                agent_messages: None,
            })
        }
        "create" => {
            let task = store.create_task(parse_plan_create_args(&invocation.args)?)?;
            Ok(CommandExecutionOutput {
                body: render_plan_task_detail(&store, &task)?,
                state: ResponseSectionState::Complete,
                activity: format!("/plan create wrote {}", task.id),
                knowledge_summary: None,
                agent_messages: None,
            })
        }
        "update" => {
            let (task_id, update) = parse_plan_update_args(&invocation.args)?;
            let task = store.update_task(&task_id, update)?;
            Ok(CommandExecutionOutput {
                body: render_plan_task_detail(&store, &task)?,
                state: ResponseSectionState::Complete,
                activity: format!("/plan update wrote {}", task.id),
                knowledge_summary: None,
                agent_messages: None,
            })
        }
        "prioritize" => {
            let (task_id, priority, placement) = parse_plan_prioritize_args(&invocation.args)?;
            let task = store.reprioritize_task(&task_id, priority, placement)?;
            Ok(CommandExecutionOutput {
                body: render_plan_task_detail(&store, &task)?,
                state: ResponseSectionState::Complete,
                activity: format!("/plan prioritize reordered {}", task.id),
                knowledge_summary: None,
                agent_messages: None,
            })
        }
        "depends" => {
            let (operation, task_id, other_id) = parse_plan_dependency_args(&invocation.args)?;
            let task = store.mutate_dependency(&task_id, &other_id, operation)?;
            Ok(CommandExecutionOutput {
                body: render_plan_task_detail(&store, &task)?,
                state: ResponseSectionState::Complete,
                activity: format!("/plan depends updated {}", task.id),
                knowledge_summary: None,
                agent_messages: None,
            })
        }
        "log" => {
            let (task_id, note) = parse_plan_log_args(&invocation.args)?;
            store.append_note(
                &task_id,
                TaskActor::User,
                note,
                session_runtime.lock().unwrap().session_id.clone(),
                None,
            )?;
            let task = store
                .get_task(&task_id)?
                .ok_or_else(|| anyhow::anyhow!("unknown task '{task_id}'"))?;
            Ok(CommandExecutionOutput {
                body: render_plan_task_detail(&store, &task)?,
                state: ResponseSectionState::Complete,
                activity: format!("/plan log appended note to {}", task.id),
                knowledge_summary: None,
                agent_messages: None,
            })
        }
        "link" => {
            let (task_id, surface, path, label) = parse_plan_link_args(&invocation.args)?;
            let task = store.add_artifact_link(
                &task_id,
                surface,
                TaskArtifactLink {
                    kind: infer_task_artifact_kind(&path, surface),
                    path,
                    label,
                },
            )?;
            Ok(CommandExecutionOutput {
                body: render_plan_task_detail(&store, &task)?,
                state: ResponseSectionState::Complete,
                activity: format!("/plan link updated {}", task.id),
                knowledge_summary: None,
                agent_messages: None,
            })
        }
        "open-link" => {
            let (task_id, target_id) = parse_plan_open_link_args(&invocation.args)?;
            let task = store
                .get_task(&task_id)?
                .ok_or_else(|| anyhow::anyhow!("unknown task '{task_id}'"))?;
            let project_root = active_project_handle(project_state).root();
            let request = build_plan_links_navigator_request(
                &project_root,
                &store,
                &task,
                &target_id,
            )?;
            tx.send(RuntimeMessageEnvelope::state(
                turn_id,
                StateMessage::ShowOverlay {
                    request: OverlayRequest {
                        target: OverlayTarget::Detail,
                        content: UiContent::DocumentNavigator(request),
                    },
                },
            ));

            Ok(CommandExecutionOutput {
                body: String::new(),
                state: ResponseSectionState::Complete,
                activity: format!("/plan open-link opened {} for {}", target_id, task_id),
                knowledge_summary: None,
                agent_messages: None,
            })
        }
        "select" => {
            let raw = invocation
                .args
                .first()
                .ok_or_else(|| anyhow::anyhow!("usage: /plan select <task-id|none>"))?;
            if raw.eq_ignore_ascii_case("none") {
                set_selected_task_state(plan_store, session_runtime, turn_context, None)?;
                return Ok(CommandExecutionOutput {
                    body: "Cleared selected project task for this session.".to_string(),
                    state: ResponseSectionState::Complete,
                    activity: "/plan select cleared task binding".to_string(),
                    knowledge_summary: None,
                    agent_messages: None,
                });
            }
            let selected = set_selected_task_state(
                plan_store,
                session_runtime,
                turn_context,
                Some(raw.to_string()),
            )?
            .ok_or_else(|| anyhow::anyhow!("unknown task '{raw}'"))?;
            Ok(CommandExecutionOutput {
                body: format!(
                    "Selected task {}\nTitle: {}\nRequirement: {}",
                    selected.task_id, selected.title, selected.requirement
                ),
                state: ResponseSectionState::Complete,
                activity: format!("/plan select bound {}", selected.task_id),
                knowledge_summary: None,
                agent_messages: None,
            })
        }
        "send" => {
            let current_selected_task_id = session_runtime.lock().unwrap().selected_task_id.clone();
            let (task_id, prompt) = parse_plan_send_args(&invocation.args, current_selected_task_id)?;
            Ok(CommandExecutionOutput {
                body: format!(
                    "Dispatching task-bound turn for {}\nPrompt: {}",
                    task_id, prompt
                ),
                state: ResponseSectionState::Complete,
                activity: format!("/plan send prepared {}", task_id),
                knowledge_summary: None,
                agent_messages: None,
            })
        }
        "sync-todo" => {
            let project_root = active_project_handle(project_state).root();
            let mut tasks = store.list_tasks(TaskListFilter::default())?;
            tasks.retain(|task| {
                !matches!(task.status, PlannedTaskStatus::Done | PlannedTaskStatus::Archived)
            });
            sync_plan_todo_projection(&project_root, &tasks)?;
            Ok(CommandExecutionOutput {
                body: format!(
                    "Synced {} open project-plan tasks into docs/TODO.md",
                    tasks.len()
                ),
                state: ResponseSectionState::Complete,
                activity: format!("/plan sync-todo projected {} tasks", tasks.len()),
                knowledge_summary: None,
                agent_messages: None,
            })
        }
        "migrate-todo" => {
            let project_root = active_project_handle(project_state).root();
            let result = migrate_todo_into_plan(&store, &project_root)?;
            Ok(CommandExecutionOutput {
                body: render_plan_todo_migration_result(&result),
                state: ResponseSectionState::Complete,
                activity: format!(
                    "/plan migrate-todo created {} tasks and skipped {} existing imports",
                    result.created_task_ids.len(),
                    result.skipped_source_keys.len(),
                ),
                knowledge_summary: None,
                agent_messages: None,
            })
        }
        "load" => {
            let project_root = active_project_handle(project_state).root();
            let (source, kind, apply) = parse_plan_load_args(&invocation.args)?;
            let result = load_plan_documents(&store, &project_root, &source, kind, apply)?;
            if !apply && !result.candidates.is_empty() {
                let request = build_plan_load_picker_request(&result);
                tx.send(RuntimeMessageEnvelope::state(
                    turn_id,
                    StateMessage::ShowOverlay {
                        request: OverlayRequest {
                            target: OverlayTarget::Picker,
                            content: UiContent::OperatorPicker(request),
                        },
                    },
                ));
            }
            Ok(CommandExecutionOutput {
                body: render_plan_load_result(&result),
                state: ResponseSectionState::Complete,
                activity: format!(
                    "/plan load source={} matched {} files apply={}",
                    source,
                    result.matched_files.len(),
                    apply,
                ),
                knowledge_summary: None,
                agent_messages: None,
            })
        }
        other => Err(anyhow::anyhow!("unsupported '/plan' subcommand '{other}'")),
    }
}

fn execute_document_command(
    project_root: &std::path::Path,
    context_facade: &Arc<OmegaContextFacade>,
    turn_id: u64,
    tx: &dyn RuntimeMessageBridge,
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
            "missing subcommand for '/document'; expected init, sync, health, query, create, archive, list, render, validate, or extract"
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
                agent_messages: None,
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
                agent_messages: None,
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
            if !results.is_empty() {
                let request = build_document_query_navigator_request(project_root, &query_text, &results);
                tx.send(RuntimeMessageEnvelope::state(
                    turn_id,
                    StateMessage::ShowOverlay {
                        request: OverlayRequest {
                            target: OverlayTarget::Detail,
                            content: UiContent::DocumentNavigator(request),
                        },
                    },
                ));
            }
            Ok(CommandExecutionOutput {
                body: render_document_query_results(&query_text, &results),
                state: ResponseSectionState::Complete,
                activity: format!("/document query returned {} results", results.len()),
                knowledge_summary: Some(build_document_query_knowledge_summary(
                    &context_facade.diagnostics.context_diagnostics(),
                    &query_text,
                    &results,
                )),
                agent_messages: None,
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
                agent_messages: None,
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
                agent_messages: None,
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
                agent_messages: None,
            })
        }
        "render" => {
            context_facade.diagnostics.record_document_usage(
                "/document",
                "builtin_command",
                "subcommand=render",
            );
            let (mode, doc_ids) = split_document_mode_args(&invocation.args, DocumentMutationMode::Apply);
            let result = context_facade
                .governance
                .manage_document(DocumentOp::RenderProjection {
                    mode,
                    doc_ids: doc_ids.clone(),
                })?;
            Ok(CommandExecutionOutput {
                body: render_document_operation_result(&result),
                state: state_from_document_result(&result),
                activity: format!(
                    "/document render prepared {} document(s)",
                    result
                        .render_state
                        .as_ref()
                        .map(|state| state.rendered_doc_ids.len())
                        .unwrap_or_else(|| result.records.len())
                ),
                knowledge_summary: None,
                agent_messages: None,
            })
        }
        "validate" => {
            context_facade.diagnostics.record_document_usage(
                "/document",
                "builtin_command",
                "subcommand=validate",
            );
            let result = context_facade
                .governance
                .manage_document(DocumentOp::ValidateProjection {
                    doc_ids: invocation.args.clone(),
                })?;
            Ok(CommandExecutionOutput {
                body: render_document_operation_result(&result),
                state: state_from_document_result(&result),
                activity: "/document validate completed".to_string(),
                knowledge_summary: None,
                agent_messages: None,
            })
        }
        "extract" => {
            context_facade.diagnostics.record_document_usage(
                "/document",
                "builtin_command",
                "subcommand=extract",
            );
            let (mode, doc_type, sources) = parse_document_extract_args(&invocation.args)?;
            let result = context_facade
                .governance
                .manage_document(DocumentOp::ExtractSource {
                    mode,
                    sources: sources.clone(),
                    doc_type,
                })?;
            Ok(CommandExecutionOutput {
                body: render_document_operation_result(&result),
                state: state_from_document_result(&result),
                activity: format!("/document extract processed {} source(s)", sources.len()),
                knowledge_summary: None,
                agent_messages: None,
            })
        }
        other => Err(anyhow::anyhow!(
            "unsupported '/document' subcommand '{other}'"
        )),
    }
}

#[allow(clippy::too_many_arguments)]
fn execute_session_command(
    project_state: &Arc<Mutex<ProjectRuntimeState>>,
    session_runtime: &Arc<Mutex<SessionRuntimeState>>,
    plan_store: &Arc<Mutex<Option<Arc<ProjectPlanStore>>>>,
    _session_context: &Arc<Mutex<SessionContext>>,
    todo_manager: &CoreSharedTodoManager,
    turn_checkpoint: &Arc<Mutex<Vec<Message>>>,
    agent_slot: &Arc<Mutex<AgentSlot>>,
    client: &DynLlmClient,
    runtime_bindings: &Arc<Mutex<SessionRuntimeBindings>>,
    base_system: &str,
    bash_allowed_commands: &[String],
    batch_max_requests: usize,
    max_output_tokens: u32,
    turn_id: u64,
    tx: &dyn RuntimeMessageBridge,
    session_id: Option<&str>,
    root_workflow_id: &str,
    cwd: &Arc<Mutex<std::path::PathBuf>>,
    mut invocation: OmegaCommandInvocation,
    previous_session_context: &SessionContext,
    turn_context: &mut SessionContext,
    progress: &mut dyn FnMut(&str),
) -> anyhow::Result<CommandExecutionOutput> {
    let project_handle = active_project_handle(project_state);
    let picker_mode = take_hidden_command_flag(&mut invocation.args, SESSION_PICKER_FLAG);
    let active_session_id = session_runtime.lock().unwrap().session_id.clone();

    let Some(subcommand) = invocation.subcommand.as_deref() else {
        let sessions = project_handle.list_sessions()?;
        let request = build_session_picker_request(&sessions, active_session_id.as_deref(), false);
        tx.send(RuntimeMessageEnvelope::state(
            turn_id,
            StateMessage::ShowOverlay {
                request: OverlayRequest {
                    target: OverlayTarget::Picker,
                    content: UiContent::OperatorPicker(request),
                },
            },
        ));
        return Ok(CommandExecutionOutput {
            body: String::new(),
            state: ResponseSectionState::Complete,
            activity: format!("/session opened {} sessions", sessions.len()),
            knowledge_summary: None,
            agent_messages: None,
        });
    };

    match subcommand {
        "list" => {
            let status_filter = invocation.args.first().map(|value| value.trim().to_lowercase());
            let sessions = project_handle.list_sessions()?;
            let filtered = sessions
                .into_iter()
                .filter(|session| match status_filter.as_deref() {
                    Some("active") => session.status == ProjectSessionStatus::Active,
                    Some("idle") => session.status == ProjectSessionStatus::Idle,
                    Some("archived") => session.status == ProjectSessionStatus::Archived,
                    Some(_) | None => true,
                })
                .collect::<Vec<_>>();
            if matches!(status_filter.as_deref(), Some(value) if value != "active" && value != "idle" && value != "archived") {
                return Err(anyhow::anyhow!("status filter must be active, idle, or archived"));
            }
            let request = build_session_picker_request(&filtered, active_session_id.as_deref(), false);
            tx.send(RuntimeMessageEnvelope::state(
                turn_id,
                StateMessage::ShowOverlay {
                    request: OverlayRequest {
                        target: OverlayTarget::Picker,
                        content: UiContent::OperatorPicker(request),
                    },
                },
            ));
            Ok(CommandExecutionOutput {
                body: String::new(),
                state: ResponseSectionState::Complete,
                activity: format!("/session list returned {} sessions", filtered.len()),
                knowledge_summary: None,
                agent_messages: None,
            })
        }
        "info" => {
            let target_session_id = invocation
                .args
                .first()
                .cloned()
                .or_else(|| active_session_id.clone())
                .ok_or_else(|| anyhow::anyhow!(
                    "no current session is bound; use /session list, /session new, or /session resume"
                ))?;
            let session = project_handle.load_session(&target_session_id)?;
            let snapshot = project_handle.load_session_snapshot(&target_session_id)?;
            let ledger_info = load_session_ledger_info(&project_handle, &target_session_id)?;
            tx.send(RuntimeMessageEnvelope::state(
                turn_id,
                StateMessage::ShowOverlay {
                    request: OverlayRequest {
                        target: OverlayTarget::Detail,
                        content: UiContent::Text(render_session_info(
                            &session,
                            snapshot.as_ref(),
                            &ledger_info,
                        )),
                    },
                },
            ));
            Ok(CommandExecutionOutput {
                body: String::new(),
                state: ResponseSectionState::Complete,
                activity: format!("/session info loaded {}", target_session_id),
                knowledge_summary: None,
                agent_messages: None,
            })
        }
        "new" => {
            if let Some(session_id) = session_id {
                progress("Phase: persist current session state");
                let current_cwd = cwd.lock().unwrap().clone();
                let archived_turn_count = persist_session_artifacts(
                    &project_handle,
                    session_id,
                    &current_cwd,
                    previous_session_context,
                    todo_manager,
                    session_runtime.lock().unwrap().selected_task_id.clone(),
                    None,
                    &[],
                )?;
                project_handle.upsert_session(ProjectSessionUpdate {
                    session_id: session_id.to_string(),
                    title: Some(session_title_from_context(previous_session_context)),
                    status: ProjectSessionStatus::Idle,
                    turn_count: 0,
                    last_user_turn_preview: Some(preview_text(
                        &previous_session_context.latest_user_turn,
                        160,
                    )),
                    archived_turn_count: Some(archived_turn_count),
                })?;
            }

            let new_session_id = Uuid::new_v4().simple().to_string();
            let title = if invocation.args.is_empty() {
                session_title(&new_session_id)
            } else {
                invocation.args.join(" ")
            };
            {
                let mut runtime = session_runtime.lock().unwrap();
                runtime.session_id = Some(new_session_id.clone());
                runtime.selected_task_id = None;
            }
            *turn_context = SessionContext::new(root_workflow_id.to_string());
            reset_todo_manager(todo_manager)?;
            *cwd.lock().unwrap() = project_handle.root();
            rebind_agent_to_current_project_with_messages(
                agent_slot,
                turn_checkpoint,
                project_state,
                cwd,
                client,
                runtime_bindings,
                base_system,
                todo_manager,
                bash_allowed_commands,
                batch_max_requests,
                max_output_tokens,
                Vec::new(),
            )?;
            project_handle.upsert_session(ProjectSessionUpdate {
                session_id: new_session_id.clone(),
                title: Some(title.clone()),
                status: ProjectSessionStatus::Active,
                turn_count: 0,
                last_user_turn_preview: None,
                archived_turn_count: Some(0),
            })?;
            tx.send(RuntimeMessageEnvelope::state(
                turn_id,
                StateMessage::SessionRestored {
                    snapshot: Box::new(SessionRestoreSnapshot {
                        session_id: new_session_id.clone(),
                        title: title.clone(),
                        visible_history: Vec::new(),
                        turn_count: 0,
                        archived_turn_count: 0,
                        latest_user_turn_preview: None,
                        recent_context_record_count: 0,
                        checkpoint_summary_count: 0,
                        search_hit_count: 0,
                        truncated_history: false,
                        todo_rendered: todo_manager.lock().unwrap().render(),
                        root_workflow_id: root_workflow_id.to_string(),
                        active_workflow_id: turn_context.routing.active_workflow_id.clone(),
                        active_workflow_role: turn_context.routing.active_workflow_role,
                        recognized_scene_id: None,
                        selected_workflow_id: None,
                        project_snapshot: Box::new(project_handle.detail_snapshot()?),
                    }),
                },
            ));
            Ok(CommandExecutionOutput {
                body: if picker_mode {
                    String::new()
                } else {
                    format!("Started new session {}\nTitle: {}", new_session_id, title)
                },
                state: ResponseSectionState::Complete,
                activity: format!("/session new -> {}", new_session_id),
                knowledge_summary: None,
                agent_messages: Some(Vec::new()),
            })
        }
        "resume" | "switch" => {
            if invocation.args.is_empty() {
                let sessions = project_handle.list_sessions()?;
                let request = build_session_picker_request(&sessions, active_session_id.as_deref(), true);
                tx.send(RuntimeMessageEnvelope::state(
                    turn_id,
                    StateMessage::ShowOverlay {
                        request: OverlayRequest {
                            target: OverlayTarget::Picker,
                            content: UiContent::OperatorPicker(request),
                        },
                    },
                ));
                return Ok(CommandExecutionOutput {
                    body: String::new(),
                    state: ResponseSectionState::Complete,
                    activity: format!("/session {subcommand} opened {} sessions", sessions.len()),
                    knowledge_summary: None,
                    agent_messages: None,
                });
            }
            let target_session_id = invocation
                .args
                .first()
                .ok_or_else(|| anyhow::anyhow!("usage: /session resume <session-id>"))?
                .to_string();
            progress("Phase: load target session snapshot");
            let target_session = project_handle.load_session(&target_session_id)?;
            let compressor = LedgerSessionContextCompressor::new(Arc::clone(&project_handle));
            let resume_projection = compressor.load(SessionContextLoadRequest {
                session_id: target_session_id.clone(),
                max_tokens: session_context_budget_tokens(),
                goal: SessionContextLoadGoal::ResumeContext,
                query: None,
            })?;
            let snapshot = resume_projection
                .reconstructed_working_set
                .ok_or_else(|| anyhow::anyhow!("session exists but is not resume-ready"))?;
            let replay_projection = compressor.load(SessionContextLoadRequest {
                session_id: target_session_id.clone(),
                max_tokens: session_context_budget_tokens(),
                goal: SessionContextLoadGoal::UiHydration,
                query: None,
            })?;

            if session_id.is_some() && Some(target_session_id.as_str()) != session_id {
                let current_cwd = cwd.lock().unwrap().clone();
                let archived_turn_count = persist_session_artifacts(
                    &project_handle,
                    session_id.expect("checked current bound session"),
                    &current_cwd,
                    previous_session_context,
                    todo_manager,
                    session_runtime.lock().unwrap().selected_task_id.clone(),
                    None,
                    &[],
                )?;
                project_handle.upsert_session(ProjectSessionUpdate {
                    session_id: session_id
                        .expect("checked current bound session")
                        .to_string(),
                    title: Some(session_title_from_context(previous_session_context)),
                    status: ProjectSessionStatus::Idle,
                    turn_count: 0,
                    last_user_turn_preview: Some(preview_text(
                        &previous_session_context.latest_user_turn,
                        160,
                    )),
                    archived_turn_count: Some(archived_turn_count),
                })?;
            }

            progress("Phase: rebuild session runtime state");
            *turn_context = session_context_from_snapshot(&snapshot);
            restore_todo_manager(todo_manager, &snapshot.todo_items)?;
            *cwd.lock().unwrap() = normalized_restored_cwd(project_handle.root(), snapshot.last_known_cwd.clone());
            let restore_outcome = restore_selected_task_state(
                session_runtime,
                turn_context,
                snapshot.selected_task_id.clone(),
                &plan_store.lock().unwrap().clone(),
            )?;
            if let SelectedTaskRestoreOutcome::ClearedDangling(task_id) = restore_outcome {
                ui_emit::send_warning_text(tx, turn_id, &dangling_selected_task_warning(&task_id));
            }
            session_runtime.lock().unwrap().session_id = Some(target_session_id.clone());
            rebind_agent_to_current_project_with_messages(
                agent_slot,
                turn_checkpoint,
                project_state,
                cwd,
                client,
                runtime_bindings,
                base_system,
                todo_manager,
                bash_allowed_commands,
                batch_max_requests,
                max_output_tokens,
                Vec::new(),
            )?;
            project_handle.upsert_session(ProjectSessionUpdate {
                session_id: target_session_id.clone(),
                title: Some(target_session.title.clone()),
                status: ProjectSessionStatus::Active,
                turn_count: target_session.turn_count,
                last_user_turn_preview: target_session.last_user_turn_preview.clone(),
                archived_turn_count: Some(target_session.archived_turn_count),
            })?;
            tx.send(RuntimeMessageEnvelope::state(
                turn_id,
                StateMessage::SessionRestored {
                    snapshot: Box::new(build_session_restore_snapshot(
                        &target_session,
                        replay_projection.recent_records,
                        resume_projection.recent_records.len(),
                        resume_projection.checkpoint_records.len(),
                        resume_projection.matched_records.len(),
                        replay_projection.truncated_history,
                        root_workflow_id,
                        &turn_context.routing,
                        &todo_manager.lock().unwrap().render(),
                        project_handle.detail_snapshot()?,
                    )),
                },
            ));
            Ok(CommandExecutionOutput {
                body: if picker_mode {
                    String::new()
                } else {
                    format!(
                        "Resumed session {}\nTitle: {}\nContext strategy: recent records={}, compression summaries={}, search hits={}\nHistory folded: {}\nResume ready: {}",
                        target_session.session_id,
                        target_session.title,
                        resume_projection.recent_records.len(),
                        resume_projection.checkpoint_records.len(),
                        resume_projection.matched_records.len(),
                        replay_projection.truncated_history,
                        target_session.resume_ready,
                    )
                },
                state: ResponseSectionState::Complete,
                activity: format!("/session resume -> {}", target_session.session_id),
                knowledge_summary: None,
                agent_messages: Some(Vec::new()),
            })
        }
        "archive" => {
            let target_session_id = invocation
                .args
                .first()
                .cloned()
                .or_else(|| active_session_id.clone())
                .ok_or_else(|| anyhow::anyhow!(
                    "no current session is bound; use /session list, /session new, or /session resume"
                ))?;
            if active_session_id.as_deref() == Some(target_session_id.as_str()) {
                return Err(anyhow::anyhow!(
                    "refusing to archive the active session; create or resume another session first"
                ));
            }
            let session = project_handle.load_session(&target_session_id)?;
            project_handle.upsert_session(ProjectSessionUpdate {
                session_id: session.session_id.clone(),
                title: Some(session.title.clone()),
                status: ProjectSessionStatus::Archived,
                turn_count: session.turn_count,
                last_user_turn_preview: session.last_user_turn_preview.clone(),
                archived_turn_count: Some(session.archived_turn_count),
            })?;
            if picker_mode {
                let sessions = project_handle.list_sessions()?;
                let request = build_session_picker_request(&sessions, active_session_id.as_deref(), false);
                tx.send(RuntimeMessageEnvelope::state(
                    turn_id,
                    StateMessage::ShowOverlay {
                        request: OverlayRequest {
                            target: OverlayTarget::Picker,
                            content: UiContent::OperatorPicker(request),
                        },
                    },
                ));
            }
            Ok(CommandExecutionOutput {
                body: if picker_mode {
                    String::new()
                } else {
                    format!("Archived session {} ({})", session.session_id, session.title)
                },
                state: ResponseSectionState::Complete,
                activity: format!("/session archive -> {}", session.session_id),
                knowledge_summary: None,
                agent_messages: None,
            })
        }
        "delete" => {
            let target_session_id = invocation
                .args
                .first()
                .cloned()
                .or_else(|| active_session_id.clone())
                .ok_or_else(|| anyhow::anyhow!(
                    "no current session is bound; use /session list, /session new, or /session resume"
                ))?;
            if active_session_id.as_deref() == Some(target_session_id.as_str()) {
                return Err(anyhow::anyhow!(
                    "refusing to delete the active session; create or resume another session first"
                ));
            }
            project_handle.delete_session_artifacts(&target_session_id)?;
            if picker_mode {
                let sessions = project_handle.list_sessions()?;
                let request = build_session_picker_request(&sessions, active_session_id.as_deref(), false);
                tx.send(RuntimeMessageEnvelope::state(
                    turn_id,
                    StateMessage::ShowOverlay {
                        request: OverlayRequest {
                            target: OverlayTarget::Picker,
                            content: UiContent::OperatorPicker(request),
                        },
                    },
                ));
            }
            Ok(CommandExecutionOutput {
                body: if picker_mode {
                    String::new()
                } else {
                    format!("Deleted session artifacts for {}", target_session_id)
                },
                state: ResponseSectionState::Complete,
                activity: format!("/session delete -> {}", target_session_id),
                knowledge_summary: None,
                agent_messages: None,
            })
        }
        other => Err(anyhow::anyhow!("unsupported '/session' subcommand '{other}'")),
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

    if let Some(manifest) = result.manifest.as_ref() {
        body.push_str(&format!(
            "\nStructured manifest: schema v{} | record sets {} | generated root {}",
            manifest.schema_version,
            manifest.record_sets.len(),
            manifest.generated_root,
        ));
    }

    if !result.records.is_empty() {
        body.push_str(&format!("\nStructured records: {}", result.records.len()));
        for record in result.records.iter().take(5) {
            body.push_str(&format!(
                "\n- {} -> {}",
                record.doc_id,
                record.render.presentation_path,
            ));
        }
    }

    if !result.relations.is_empty() {
        body.push_str(&format!("\nStructured relations: {}", result.relations.len()));
        for relation in result.relations.iter().take(5) {
            body.push_str(&format!(
                "\n- {} [{}] {}",
                relation.source,
                relation.kind,
                relation.target,
            ));
        }
    }

    if let Some(render_state) = result.render_state.as_ref() {
        body.push_str(&format!(
            "\nRender state: {} doc(s), {} path(s)",
            render_state.rendered_doc_ids.len(),
            render_state.generated_paths.len(),
        ));
    }

    if let Some(validation) = result.validation.as_ref() {
        body.push_str(&format!(
            "\nValidation: {} | missing {} | mismatched {} | broken relations {}",
            if validation.ok { "ok" } else { "failed" },
            validation.missing_files.len(),
            validation.mismatched_files.len(),
            validation.broken_relations.len(),
        ));
    }

    if let Some(extraction) = result.extraction.as_ref() {
        body.push_str(&format!(
            "\nExtraction: {} doc(s) from {} source(s)",
            extraction.extracted_doc_ids.len(),
            extraction.extracted_paths.len(),
        ));
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
        "whitepaper" | "whitepapers" | "paper" => Some(DocType::Whitepaper),
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
        Some(DocType::Whitepaper) => "whitepaper",
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

fn parse_document_mode_flag(value: &str) -> Option<DocumentMutationMode> {
    match value.to_ascii_lowercase().as_str() {
        "--check" | "check" => Some(DocumentMutationMode::Check),
        "--plan" | "plan" => Some(DocumentMutationMode::Plan),
        "--apply" | "apply" => Some(DocumentMutationMode::Apply),
        _ => None,
    }
}

fn split_document_mode_args(
    args: &[String],
    default_mode: DocumentMutationMode,
) -> (DocumentMutationMode, Vec<String>) {
    let mut mode = default_mode;
    let mut values = Vec::new();
    for arg in args {
        if let Some(parsed_mode) = parse_document_mode_flag(arg) {
            mode = parsed_mode;
        } else {
            values.push(arg.clone());
        }
    }
    (mode, values)
}

fn parse_document_extract_args(
    args: &[String],
) -> anyhow::Result<(DocumentMutationMode, Option<DocType>, Vec<String>)> {
    let mut mode = DocumentMutationMode::Apply;
    let mut doc_type = None;
    let mut sources = Vec::new();
    let mut index = 0usize;

    while index < args.len() {
        let arg = &args[index];
        if let Some(parsed_mode) = parse_document_mode_flag(arg) {
            mode = parsed_mode;
            index += 1;
            continue;
        }
        if arg == "--doc-type" {
            let value = args
                .get(index + 1)
                .ok_or_else(|| anyhow::anyhow!("missing value after '--doc-type'"))?;
            doc_type = parse_doc_type(value);
            if doc_type.is_none() {
                return Err(anyhow::anyhow!("unknown doc_type '{}' for '/document extract'", value));
            }
            index += 2;
            continue;
        }
        if let Some(value) = arg.strip_prefix("--doc-type=") {
            doc_type = parse_doc_type(value);
            if doc_type.is_none() {
                return Err(anyhow::anyhow!("unknown doc_type '{}' for '/document extract'", value));
            }
            index += 1;
            continue;
        }
        sources.push(arg.clone());
        index += 1;
    }

    if sources.is_empty() {
        return Err(anyhow::anyhow!(
            "usage: /document extract <source...> [--doc-type <...>] [--check|--plan|--apply]"
        ));
    }

    Ok((mode, doc_type, sources))
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

fn open_command_section(
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
    section_id
}

fn emit_command_output(
    tx: &dyn RuntimeMessageBridge,
    turn_id: u64,
    title: &str,
    source: OmegaCommandSource,
    section_id: Option<&str>,
    output: CommandExecutionOutput,
) {
    let CommandExecutionOutput {
        body,
        state,
        activity,
        knowledge_summary,
        agent_messages: _,
    } = output;

    let section_id = if let Some(section_id) = section_id {
        Some(section_id.to_string())
    } else if state == ResponseSectionState::Failed || !body.trim().is_empty() {
        Some(open_command_section(tx, turn_id, title, source))
    } else {
        None
    };

    if let Some(section_id) = section_id.as_deref() {
        if !body.trim().is_empty() {
            append_command_output(tx, turn_id, section_id, &body);
        }
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
    }

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
    let section_id = open_command_section(tx, turn_id, title, source);
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

fn build_turn_archive(turn_id: u64, session_id: &str, session_context: &SessionContext) -> TurnData {
    TurnData {
        session_id: session_id.to_string(),
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

fn rebind_agent_to_current_project_with_messages(
    agent_slot: &Arc<Mutex<AgentSlot>>,
    turn_checkpoint: &Arc<Mutex<Vec<Message>>>,
    project_state: &Arc<Mutex<ProjectRuntimeState>>,
    cwd: &Arc<Mutex<std::path::PathBuf>>,
    client: &DynLlmClient,
    runtime_bindings: &Arc<Mutex<SessionRuntimeBindings>>,
    base_system: &str,
    todo_manager: &CoreSharedTodoManager,
    bash_allowed_commands: &[String],
    batch_max_requests: usize,
    max_output_tokens: u32,
    messages: Vec<Message>,
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
    let mut replacement = Agent::new(client.clone(), system, dispatcher)?;
    replacement.set_max_tokens(max_output_tokens);
    replacement.set_messages(messages.clone());
    let mut slot = agent_slot.lock().unwrap();
    slot.agent = Some(replacement);
    *runtime_bindings.lock().unwrap() = next_bindings;
    *turn_checkpoint.lock().unwrap() = messages;
    Ok(())
}

fn persist_session_artifacts(
    project_handle: &Arc<OmegaProjectHandle>,
    session_id: &str,
    cwd: &std::path::Path,
    session_context: &SessionContext,
    todo_manager: &CoreSharedTodoManager,
    selected_task_id: Option<String>,
    last_completed_turn_id: Option<u64>,
    replay_entries: &[SessionReplayEntry],
) -> anyhow::Result<u64> {
    let previous = project_handle.load_session_snapshot(session_id)?;
    let mut recent_turn_summaries = previous
        .as_ref()
        .map(|snapshot| snapshot.recent_turn_summaries.clone())
        .unwrap_or_default();
    if let Some(turn_id) = last_completed_turn_id {
        recent_turn_summaries.retain(|summary| summary.turn_id != turn_id);
        recent_turn_summaries.insert(
            0,
            ProjectSessionTurnSummary {
                turn_id,
                workflow_id: session_context.routing.active_workflow_id.clone(),
                user_intent: session_context.latest_user_turn.clone(),
                summary_count: session_context.step_summaries.len(),
            },
        );
        recent_turn_summaries.truncate(12);
    }

    let snapshot = ProjectSessionSnapshot {
        schema_version: 1,
        project_id: project_handle.project_id(),
        session_id: session_id.to_string(),
        saved_at: current_unix_timestamp(),
        last_completed_turn_id: last_completed_turn_id.or_else(|| {
            previous
                .as_ref()
                .and_then(|snapshot| snapshot.last_completed_turn_id)
        }),
        latest_user_turn: if session_context.latest_user_turn.trim().is_empty() {
            None
        } else {
            Some(session_context.latest_user_turn.clone())
        },
        recent_turn_summaries,
        routing: ProjectSessionRoutingSnapshot {
            recognized_scene_id: session_context.routing.recognized_scene_id.clone(),
            selected_workflow_id: session_context.routing.selected_workflow_id.clone(),
            active_workflow_id: session_context.routing.active_workflow_id.clone(),
            active_workflow_role: session_context.routing.active_workflow_role.as_str().to_string(),
        },
        skill_routing: ProjectSkillRoutingSnapshot {
            selected_skill_ids: session_context.skill_routing.selected_skill_ids.clone(),
            loaded_skill_ids: session_context.skill_routing.loaded_skill_ids.clone(),
            ignored_skill_ids: session_context.skill_routing.ignored_skill_ids.clone(),
            selection_reason: session_context.skill_routing.selection_reason.clone(),
            source_step_id: session_context.skill_routing.source_step_id.clone(),
        },
        step_summaries: session_context
            .step_summaries
            .iter()
            .map(|summary| ProjectSessionStepSummary {
                workflow_id: summary.workflow_id.clone(),
                step_id: summary.step_id.clone(),
                title: summary.title.clone(),
                summary: summary.summary.clone(),
                estimated_tokens: summary.estimated_tokens,
            })
            .collect(),
        step_outputs: session_context.step_outputs.clone(),
        governance_events: session_context.governance_events.clone(),
        todo_items: todo_manager
            .lock()
            .unwrap()
            .items()
            .iter()
            .filter_map(|item| {
                Some(ProjectSessionTodoItem {
                    id: item.id.clone()?,
                    text: item.text.clone(),
                    status: project_todo_status(item.status.clone()),
                    active_form: item.active_form.clone(),
                })
            })
            .collect(),
        structured_input: None,
        selected_task_id,
        last_known_cwd: Some(cwd.to_path_buf()),
    };
    project_handle.save_session_snapshot(&snapshot)?;
    project_handle.append_replay_entries(session_id, replay_entries)?;
    let checkpoint_records = LedgerSessionContextCompressor::with_budget(
        Arc::clone(project_handle),
        session_context_budget_tokens(),
    )
    .compact(SessionCompactionRequest {
        session_id: session_id.to_string(),
        max_tokens: session_context_budget_tokens(),
    })?
    .checkpoint_records;
    if !checkpoint_records.is_empty() {
        project_handle.append_context_records(session_id, &checkpoint_records)?;
    }
    Ok(snapshot.recent_turn_summaries.len() as u64)
}

fn project_todo_status(status: TodoStatus) -> ProjectSessionTodoStatus {
    match status {
        TodoStatus::Pending => ProjectSessionTodoStatus::Pending,
        TodoStatus::InProgress => ProjectSessionTodoStatus::InProgress,
        TodoStatus::Completed => ProjectSessionTodoStatus::Completed,
    }
}

fn todo_status_from_snapshot(status: ProjectSessionTodoStatus) -> TodoStatus {
    match status {
        ProjectSessionTodoStatus::Pending => TodoStatus::Pending,
        ProjectSessionTodoStatus::InProgress => TodoStatus::InProgress,
        ProjectSessionTodoStatus::Completed => TodoStatus::Completed,
    }
}

fn reset_todo_manager(todo_manager: &CoreSharedTodoManager) -> anyhow::Result<()> {
    todo_manager.lock().unwrap().update(Vec::new())?;
    Ok(())
}

fn restore_todo_manager(
    todo_manager: &CoreSharedTodoManager,
    items: &[ProjectSessionTodoItem],
) -> anyhow::Result<()> {
    let restored = items
        .iter()
        .map(|item| TodoItem {
            id: Some(item.id.clone()),
            text: item.text.clone(),
            status: todo_status_from_snapshot(item.status),
            active_form: item.active_form.clone(),
        })
        .collect::<Vec<_>>();
    todo_manager.lock().unwrap().update(restored)?;
    Ok(())
}

fn session_context_from_snapshot(snapshot: &ProjectSessionSnapshot) -> SessionContext {
    SessionContext {
        latest_user_turn: snapshot.latest_user_turn.clone().unwrap_or_default(),
        routing: session_state::RoutingContext {
            recognized_scene_id: snapshot.routing.recognized_scene_id.clone(),
            selected_workflow_id: snapshot.routing.selected_workflow_id.clone(),
            active_workflow_id: snapshot.routing.active_workflow_id.clone(),
            active_workflow_role: workflow_role_from_str(&snapshot.routing.active_workflow_role),
        },
        skill_routing: session_state::SkillRoutingContext {
            selected_skill_ids: snapshot.skill_routing.selected_skill_ids.clone(),
            loaded_skill_ids: snapshot.skill_routing.loaded_skill_ids.clone(),
            ignored_skill_ids: snapshot.skill_routing.ignored_skill_ids.clone(),
            selection_reason: snapshot.skill_routing.selection_reason.clone(),
            source_step_id: snapshot.skill_routing.source_step_id.clone(),
        },
        step_summaries: snapshot
            .step_summaries
            .iter()
            .map(|summary| session_state::StepSummary {
                workflow_id: summary.workflow_id.clone(),
                step_id: summary.step_id.clone(),
                title: summary.title.clone(),
                summary: summary.summary.clone(),
                estimated_tokens: summary.estimated_tokens,
            })
            .collect(),
        step_outputs: snapshot.step_outputs.clone(),
        governance_events: snapshot.governance_events.clone(),
        selected_task: None,
    }
}

fn current_plan_store(
    plan_store: &Arc<Mutex<Option<Arc<ProjectPlanStore>>>>,
) -> anyhow::Result<Arc<ProjectPlanStore>> {
    plan_store
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(|| anyhow::anyhow!("project plan store unavailable"))
}

fn set_selected_task_state(
    plan_store: &Arc<Mutex<Option<Arc<ProjectPlanStore>>>>,
    session_runtime: &Arc<Mutex<SessionRuntimeState>>,
    turn_context: &mut SessionContext,
    task_id: Option<String>,
) -> anyhow::Result<Option<SelectedProjectTaskContext>> {
    let selected = if let Some(task_id) = task_id {
        let context = current_plan_store(plan_store)?
            .resolve_task_context(&task_id)?
            .ok_or_else(|| anyhow::anyhow!("unknown task '{task_id}'"))?;
        session_runtime.lock().unwrap().selected_task_id = Some(task_id);
        turn_context.selected_task = Some(context.clone());
        Some(context)
    } else {
        session_runtime.lock().unwrap().selected_task_id = None;
        turn_context.selected_task = None;
        None
    };
    Ok(selected)
}

fn restore_selected_task_state(
    session_runtime: &Arc<Mutex<SessionRuntimeState>>,
    turn_context: &mut SessionContext,
    selected_task_id: Option<String>,
    plan_store: &Option<Arc<ProjectPlanStore>>,
) -> anyhow::Result<SelectedTaskRestoreOutcome> {
    if let Some(task_id) = selected_task_id {
        if let Some(store) = plan_store.as_ref() {
            if let Some(context) = store.resolve_task_context(&task_id)? {
                session_runtime.lock().unwrap().selected_task_id = Some(task_id);
                turn_context.selected_task = Some(context);
                return Ok(SelectedTaskRestoreOutcome::Restored);
            }
        }

		session_runtime.lock().unwrap().selected_task_id = None;
		turn_context.selected_task = None;
		return Ok(SelectedTaskRestoreOutcome::ClearedDangling(task_id));
    }

    session_runtime.lock().unwrap().selected_task_id = None;
    turn_context.selected_task = None;
    Ok(SelectedTaskRestoreOutcome::NoSelection)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SelectedTaskRestoreOutcome {
	NoSelection,
	Restored,
	ClearedDangling(String),
}

fn dangling_selected_task_warning(task_id: &str) -> String {
	format!(
		"Selected project task {task_id} no longer exists in the active project plan. The session task binding was cleared."
	)
}

fn parse_plan_create_args(args: &[String]) -> anyhow::Result<NewPlannedTask> {
    if args.is_empty() {
        return Err(anyhow::anyhow!(
            "usage: /plan create [--priority <p0|p1|p2|p3>] <title...>"
        ));
    }

    let mut priority = TaskPriority::P2;
    let mut title_start = 0usize;
    if args.first().is_some_and(|arg| arg == "--priority") {
        let raw = args.get(1).ok_or_else(|| {
            anyhow::anyhow!("missing value after '--priority'; expected p0, p1, p2, or p3")
        })?;
        priority = TaskPriority::parse_cli(raw)
            .ok_or_else(|| anyhow::anyhow!("unknown priority '{raw}'"))?;
        title_start = 2;
    }
    let title = args[title_start..].join(" ").trim().to_string();
    if title.is_empty() {
        return Err(anyhow::anyhow!(
            "usage: /plan create [--priority <p0|p1|p2|p3>] <title...>"
        ));
    }
    Ok(NewPlannedTask::simple(title, priority))
}

fn parse_plan_list_args(args: &[String]) -> anyhow::Result<(String, TaskListFilter)> {
    let mut view = "current".to_string();
    let mut status = None;
    let mut priority = None;
    let mut index = 0usize;
    while index < args.len() {
        match args[index].as_str() {
            "current" | "history" | "all" if index == 0 => {
                view = args[index].to_ascii_lowercase();
                index += 1;
            }
            "--status" => {
                let raw = args.get(index + 1).ok_or_else(|| {
                    anyhow::anyhow!("missing value after '--status'")
                })?;
                status = Some(
                    PlannedTaskStatus::parse_cli(raw)
                        .ok_or_else(|| anyhow::anyhow!("unknown status '{raw}'"))?,
                );
                index += 2;
            }
            "--priority" => {
                let raw = args.get(index + 1).ok_or_else(|| {
                    anyhow::anyhow!("missing value after '--priority'")
                })?;
                priority = Some(
                    TaskPriority::parse_cli(raw)
                        .ok_or_else(|| anyhow::anyhow!("unknown priority '{raw}'"))?,
                );
                index += 2;
            }
            other => {
                return Err(anyhow::anyhow!(
                    "unexpected /plan list argument '{other}'"
                ));
            }
        }
    }
    Ok((view, TaskListFilter { status, priority }))
}

fn parse_plan_update_args(args: &[String]) -> anyhow::Result<(String, PlannedTaskUpdate)> {
    let task_id = args
        .first()
        .ok_or_else(|| anyhow::anyhow!("usage: /plan update <task-id> [--title <...>] [--summary <...>] [--requirement <...>] [--status <...>] [--accept <...>] [--tag <...>]") )?
        .clone();
    let mut update = PlannedTaskUpdate::default();
    let mut acceptance = Vec::new();
    let mut tags = Vec::new();
    let mut index = 1usize;
    while index < args.len() {
        match args[index].as_str() {
            "--title" => {
                let (value, next_index) = collect_plan_flag_value(args, index + 1, "--title")?;
                update.title = Some(value);
                index = next_index;
            }
            "--summary" => {
                let (value, next_index) = collect_plan_flag_value(args, index + 1, "--summary")?;
                update.summary = Some(value);
                index = next_index;
            }
            "--requirement" => {
                let (value, next_index) = collect_plan_flag_value(args, index + 1, "--requirement")?;
                update.requirement = Some(value);
                index = next_index;
            }
            "--status" => {
                let raw = args.get(index + 1).ok_or_else(|| {
                    anyhow::anyhow!("missing value after '--status'")
                })?;
                update.status = Some(
                    PlannedTaskStatus::parse_cli(raw)
                        .ok_or_else(|| anyhow::anyhow!("unknown status '{raw}'"))?,
                );
                index += 2;
            }
            "--accept" => {
                let (value, next_index) = collect_plan_flag_value(args, index + 1, "--accept")?;
                acceptance.push(value);
                index = next_index;
            }
            "--tag" => {
                let raw = args.get(index + 1).ok_or_else(|| {
                    anyhow::anyhow!("missing value after '--tag'")
                })?;
                tags.push(raw.clone());
                index += 2;
            }
            other => {
                return Err(anyhow::anyhow!(
                    "unexpected /plan update argument '{other}'"
                ));
            }
        }
    }
    if !acceptance.is_empty() {
        update.acceptance = Some(acceptance);
    }
    if !tags.is_empty() {
        update.tags = Some(tags);
    }
    Ok((task_id, update))
}

fn parse_plan_prioritize_args(
    args: &[String],
) -> anyhow::Result<(String, TaskPriority, TaskOrderPlacement)> {
    if args.len() < 2 {
        return Err(anyhow::anyhow!(
            "usage: /plan prioritize <task-id> <p0|p1|p2|p3> [--before <task-id>] [--after <task-id>]"
        ));
    }
    let task_id = args[0].clone();
    let priority = TaskPriority::parse_cli(&args[1])
        .ok_or_else(|| anyhow::anyhow!("unknown priority '{}'", args[1]))?;
    let mut placement = TaskOrderPlacement::default();
    let mut index = 2usize;
    while index < args.len() {
        match args[index].as_str() {
            "--before" => {
                placement.before_task_id = Some(
                    args.get(index + 1)
                        .ok_or_else(|| anyhow::anyhow!("missing value after '--before'"))?
                        .clone(),
                );
                index += 2;
            }
            "--after" => {
                placement.after_task_id = Some(
                    args.get(index + 1)
                        .ok_or_else(|| anyhow::anyhow!("missing value after '--after'"))?
                        .clone(),
                );
                index += 2;
            }
            other => {
                return Err(anyhow::anyhow!(
                    "unexpected /plan prioritize argument '{other}'"
                ));
            }
        }
    }
    Ok((task_id, priority, placement))
}

fn parse_plan_dependency_args(
    args: &[String],
) -> anyhow::Result<(TaskDependencyOperation, String, String)> {
    if args.len() != 3 {
        return Err(anyhow::anyhow!(
            "usage: /plan depends <add|remove> <task-id> <other-id>"
        ));
    }
    let operation = match args[0].as_str() {
        "add" => TaskDependencyOperation::Add,
        "remove" => TaskDependencyOperation::Remove,
        other => return Err(anyhow::anyhow!("unknown dependency operation '{other}'")),
    };
    Ok((operation, args[1].clone(), args[2].clone()))
}

fn parse_plan_log_args(args: &[String]) -> anyhow::Result<(String, String)> {
    if args.len() < 2 {
        return Err(anyhow::anyhow!("usage: /plan log <task-id> <note...>"));
    }
    Ok((args[0].clone(), args[1..].join(" ").trim().to_string()))
}

fn parse_plan_link_args(
    args: &[String],
) -> anyhow::Result<(String, TaskLinkSurface, String, Option<String>)> {
    if args.len() < 3 {
        return Err(anyhow::anyhow!(
            "usage: /plan link <task-id> <design|implementation> <path> [label...]"
        ));
    }
    let surface = TaskLinkSurface::parse_cli(&args[1])
        .ok_or_else(|| anyhow::anyhow!("unknown link surface '{}'", args[1]))?;
    let label = if args.len() > 3 {
        Some(args[3..].join(" ").trim().to_string())
    } else {
        None
    };
    Ok((args[0].clone(), surface, args[2].clone(), label))
}

fn parse_plan_view_file_args(args: &[String]) -> anyhow::Result<String> {
    if args.is_empty() {
        return Err(anyhow::anyhow!("usage: /plan view-file <path>"));
    }
    let path = args.join(" ");
    if path.trim().is_empty() {
        return Err(anyhow::anyhow!("usage: /plan view-file <path>"));
    }
    Ok(path)
}

fn parse_plan_open_link_args(args: &[String]) -> anyhow::Result<(String, String)> {
    if args.len() < 2 {
        return Err(anyhow::anyhow!(
            "usage: /plan open-link <task-id> <path-or-log-entry>"
        ));
    }
    Ok((args[0].clone(), args[1..].join(" ")))
}

fn parse_plan_send_args(
    args: &[String],
    current_selected_task_id: Option<String>,
) -> anyhow::Result<(String, String)> {
    if args.is_empty() {
        return Err(anyhow::anyhow!(
            "usage: /plan send [<task-id>] <prompt...>"
        ));
    }
    let (task_id, prompt_args) = if args[0].starts_with("TASK-") {
        (
            args[0].clone(),
            args.get(1..)
                .ok_or_else(|| anyhow::anyhow!("usage: /plan send [<task-id>] <prompt...>"))?,
        )
    } else {
        (
            current_selected_task_id.ok_or_else(|| {
                anyhow::anyhow!("No task selected. Use /plan select <id> first.")
            })?,
            args,
        )
    };
    let prompt = prompt_args.join(" ").trim().to_string();
    if prompt.is_empty() {
        return Err(anyhow::anyhow!(
            "usage: /plan send [<task-id>] <prompt...>"
        ));
    }
    Ok((task_id, prompt))
}

fn collect_plan_flag_value(
    args: &[String],
    start_index: usize,
    flag: &str,
) -> anyhow::Result<(String, usize)> {
    if start_index >= args.len() {
        return Err(anyhow::anyhow!("missing value after '{flag}'"));
    }
    let mut index = start_index;
    let mut values = Vec::new();
    while index < args.len() && !args[index].starts_with("--") {
        values.push(args[index].clone());
        index += 1;
    }
    if values.is_empty() {
        return Err(anyhow::anyhow!("missing value after '{flag}'"));
    }
    Ok((values.join(" ").trim().to_string(), index))
}

fn infer_task_artifact_kind(path: &str, surface: TaskLinkSurface) -> TaskArtifactKind {
    if path.starts_with("docs/prds/") {
        TaskArtifactKind::Prd
    } else if path.starts_with("docs/specs/") {
        TaskArtifactKind::Spec
    } else if path.starts_with("docs/guide/") {
        TaskArtifactKind::Guide
    } else if path.starts_with("docs/decisions/") {
        TaskArtifactKind::Adr
    } else if matches!(surface, TaskLinkSurface::Implementation)
        && (path.contains("/tests/") || path.ends_with("_tests.rs") || path.ends_with(".test.ts"))
    {
        TaskArtifactKind::Test
    } else {
        TaskArtifactKind::Code
    }
}

fn is_doc_artifact_kind(kind: TaskArtifactKind) -> bool {
    matches!(
        kind,
        TaskArtifactKind::Prd
            | TaskArtifactKind::Spec
            | TaskArtifactKind::Guide
            | TaskArtifactKind::Adr
    )
}

fn task_artifact_kind_label(kind: TaskArtifactKind) -> &'static str {
    match kind {
        TaskArtifactKind::Prd => "prd",
        TaskArtifactKind::Spec => "spec",
        TaskArtifactKind::Guide => "guide",
        TaskArtifactKind::Adr => "adr",
        TaskArtifactKind::Code => "code",
        TaskArtifactKind::Test => "test",
        TaskArtifactKind::Delivery => "delivery",
    }
}

fn task_log_kind_label(kind: TaskLogKind) -> &'static str {
    match kind {
        TaskLogKind::Created => "created",
        TaskLogKind::NoteAdded => "note",
        TaskLogKind::DeliveryAttached => "delivery",
        TaskLogKind::PartialDelivery => "partial-delivery",
    }
}

fn plan_link_display_name(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(path)
        .to_string()
}

fn preview_plan_link_file(project_root: &std::path::Path, relative_path: &str) -> Option<String> {
    let resolved = resolve_project_relative_file(project_root, relative_path).ok()?;
    let content = std::fs::read_to_string(&resolved).ok()?;
    let stripped = strip_basic_markdown(&content);
    let preview = stripped
        .lines()
        .take(8)
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string();
    (!preview.is_empty()).then_some(preview)
}

fn resolve_project_relative_file(
    project_root: &std::path::Path,
    relative_path: &str,
) -> anyhow::Result<std::path::PathBuf> {
    let relative_path = relative_path.trim();
    if relative_path.is_empty() {
        anyhow::bail!("path cannot be empty");
    }
    let relative = std::path::Path::new(relative_path);
    if relative.is_absolute() {
        anyhow::bail!("/plan view-file path must be project-root relative, not absolute");
    }
    let root = std::fs::canonicalize(project_root)
        .with_context(|| format!("failed to canonicalize {}", project_root.display()))?;
    let unresolved = root.join(relative);
    let resolved = std::fs::canonicalize(&unresolved)
        .with_context(|| format!("failed to resolve path {}", unresolved.display()))?;
    if resolved.strip_prefix(&root).is_err() {
        anyhow::bail!("/plan view-file path must stay within the active project root");
    }
    if !resolved.is_file() {
        anyhow::bail!("/plan view-file path must reference a file");
    }
    Ok(resolved)
}

fn render_plan_view_file_content(
    project_root: &std::path::Path,
    relative_path: &str,
) -> anyhow::Result<String> {
    let resolved = resolve_project_relative_file(project_root, relative_path)?;
    let content = std::fs::read_to_string(&resolved)
        .with_context(|| format!("failed to read {}", resolved.display()))?;
    let stripped = strip_basic_markdown(&content);
    Ok(format!("File: {}\n\n{}", relative_path, stripped.trim()))
}

fn render_plan_log_entry_detail(task_id: &str, entry: &TaskLogEntry) -> String {
    format!(
        "Task: {}\nLog entry: #{}\nKind: {}\nActor: {:?}\nSummary: {}\nSession: {}\nTurn: {}\nDelivery: {}",
        task_id,
        entry.seq,
        task_log_kind_label(entry.kind),
        entry.actor,
        entry.summary,
        entry.related_session_id.as_deref().unwrap_or("none"),
        entry
            .related_turn_id
            .map(|value| value.to_string())
            .as_deref()
            .unwrap_or("none"),
        entry.related_delivery_id.as_deref().unwrap_or("none"),
    )
}

fn strip_basic_markdown(content: &str) -> String {
    let mut lines = Vec::new();
    let mut in_frontmatter = false;
    let mut saw_first_line = false;

    for raw_line in content.lines() {
        let trimmed_end = raw_line.trim_end();
        if !saw_first_line {
            saw_first_line = true;
            if trimmed_end.trim() == "---" {
                in_frontmatter = true;
                continue;
            }
        }
        if in_frontmatter {
            if trimmed_end.trim() == "---" {
                in_frontmatter = false;
            }
            continue;
        }

        let trimmed_start = trimmed_end.trim_start();
        if trimmed_start.starts_with("```") {
            continue;
        }
        let normalized = strip_markdown_heading(trimmed_start).unwrap_or(trimmed_end);
        lines.push(normalized.to_string());
    }

    lines.join("\n")
}

fn strip_markdown_heading(line: &str) -> Option<&str> {
    let hashes = line.chars().take_while(|character| *character == '#').count();
    if hashes == 0 {
        return None;
    }
    line.get(hashes..)
        .and_then(|remainder| remainder.strip_prefix(' '))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TodoMigrationDraft {
    source_key: String,
    title: String,
    status: PlannedTaskStatus,
    priority: TaskPriority,
    description: String,
    blocked_by: Vec<String>,
    related_paths: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct TodoMigrationResult {
    created_task_ids: Vec<String>,
    skipped_source_keys: Vec<String>,
}

fn migrate_todo_into_plan(
    store: &ProjectPlanStore,
    project_root: &std::path::Path,
) -> anyhow::Result<TodoMigrationResult> {
    let todo_path = project_root.join("docs/TODO.md");
    let todo = std::fs::read_to_string(&todo_path)
        .with_context(|| format!("failed to read {}", todo_path.display()))?;
    let drafts = parse_open_todo_migration_drafts(&todo)?;
    if drafts.is_empty() {
        return Ok(TodoMigrationResult::default());
    }

    let existing_tasks = store.list_tasks(TaskListFilter::default())?;
    let mut source_key_to_task_id = existing_tasks
        .iter()
        .flat_map(|task| {
            task.tags.iter().filter_map(|tag| {
                tag.strip_prefix("todo-import:")
                    .map(|source_key| (source_key.to_string(), task.id.clone()))
            })
        })
        .collect::<std::collections::BTreeMap<_, _>>();

    let mut result = TodoMigrationResult::default();
    for draft in &drafts {
        let source_key = todo_migration_key(&draft.source_key);
        let source_tag = todo_migration_tag(&draft.source_key);
        if source_key_to_task_id.contains_key(source_key.as_str()) {
            result.skipped_source_keys.push(draft.source_key.clone());
            continue;
        }

        let task = store.create_task(NewPlannedTask {
            title: draft.title.clone(),
            kind: omega_plan::PlannedTaskKind::Task,
            status: draft.status,
            priority: draft.priority,
            summary: draft.description.clone(),
            requirement: draft.description.clone(),
            acceptance: Vec::new(),
            parent_id: None,
            depends_on: Vec::new(),
            tags: vec![source_tag.clone()],
			doc_scope: Vec::new(),
        })?;
        store.append_note(
            &task.id,
            TaskActor::Command,
            format!("Imported from docs/TODO.md track {}", draft.source_key),
            None,
            None,
        )?;
        source_key_to_task_id.insert(source_key, task.id.clone());
        result.created_task_ids.push(task.id);
    }

    for draft in &drafts {
		let Some(task_id) = source_key_to_task_id.get(&todo_migration_key(&draft.source_key)).cloned() else {
            continue;
        };
        for blocked_key in &draft.blocked_by {
			let Some(dependency_id) = source_key_to_task_id.get(&todo_migration_key(blocked_key)).cloned() else {
                continue;
            };
            if dependency_id != task_id {
                store.mutate_dependency(&task_id, &dependency_id, TaskDependencyOperation::Add)?;
            }
        }
        for path in &draft.related_paths {
            store.add_artifact_link(
                &task_id,
                TaskLinkSurface::Design,
                TaskArtifactLink {
                    kind: infer_task_artifact_kind(path, TaskLinkSurface::Design),
                    path: path.clone(),
                    label: None,
                },
            )?;
        }
    }

    Ok(result)
}

fn parse_open_todo_migration_drafts(todo: &str) -> anyhow::Result<Vec<TodoMigrationDraft>> {
    let mut drafts = Vec::new();
    let mut in_active_tasks = false;
    let mut current_heading = None::<String>;
    let mut current_lines = Vec::new();

    for line in todo.lines() {
        if line.trim() == "## Active Tasks" {
            in_active_tasks = true;
            continue;
        }
        if !in_active_tasks {
            continue;
        }
        if line.starts_with("## ") {
            break;
        }
        if let Some(heading) = line.strip_prefix("### ") {
            if let Some(previous_heading) = current_heading.replace(heading.trim().to_string()) {
                if let Some(draft) = build_todo_migration_draft(&previous_heading, &current_lines)? {
                    drafts.push(draft);
                }
                current_lines.clear();
            }
            continue;
        }
        if current_heading.is_some() {
            current_lines.push(line.to_string());
        }
    }

    if let Some(heading) = current_heading {
        if let Some(draft) = build_todo_migration_draft(&heading, &current_lines)? {
            drafts.push(draft);
        }
    }

    Ok(drafts)
}

fn build_todo_migration_draft(
    heading: &str,
    lines: &[String],
) -> anyhow::Result<Option<TodoMigrationDraft>> {
    let source_key = heading
        .split(':')
        .next()
        .map(str::trim)
        .unwrap_or(heading)
        .to_string();
    let title = heading
        .split_once(':')
        .map(|(_, tail)| tail.trim())
        .filter(|tail| !tail.is_empty())
        .unwrap_or(heading)
        .to_string();
    let status = parse_todo_migration_status(extract_todo_field(lines, "Status").as_deref())?;
    if matches!(status, PlannedTaskStatus::Done | PlannedTaskStatus::Archived) {
        return Ok(None);
    }
    let priority = parse_todo_migration_priority(extract_todo_field(lines, "Priority").as_deref())?;
    let description = extract_todo_field(lines, "Description")
        .or_else(|| extract_todo_field(lines, "Planning Note"))
        .unwrap_or_else(|| title.clone());
    let blocked_by = extract_todo_field(lines, "Blocked by")
        .map(|value| split_todo_reference_list(&value))
        .unwrap_or_default();
    let related_paths = extract_todo_field(lines, "Related")
        .map(|value| extract_backtick_paths(&value))
        .unwrap_or_default();
    Ok(Some(TodoMigrationDraft {
        source_key,
        title,
        status,
        priority,
        description,
        blocked_by,
        related_paths,
    }))
}

fn extract_todo_field(lines: &[String], field: &str) -> Option<String> {
    let prefix = format!("- **{field}**:");
    lines.iter().find_map(|line| {
        line.trim()
            .strip_prefix(&prefix)
            .map(|value| value.trim().to_string())
    })
}

fn parse_todo_migration_status(value: Option<&str>) -> anyhow::Result<PlannedTaskStatus> {
    match value.unwrap_or("Pending").trim().to_ascii_lowercase().as_str() {
        "pending" => Ok(PlannedTaskStatus::Backlog),
        "in progress" => Ok(PlannedTaskStatus::InProgress),
        "blocked" => Ok(PlannedTaskStatus::Blocked),
        "review" => Ok(PlannedTaskStatus::Ready),
        "completed" => Ok(PlannedTaskStatus::Done),
        "replaced" => Ok(PlannedTaskStatus::Archived),
        other => Err(anyhow::anyhow!(
            "unsupported task import status '{other}'; expected Pending, In Progress, Blocked, Review, Completed, or Replaced"
        )),
    }
}

fn parse_todo_migration_priority(value: Option<&str>) -> anyhow::Result<TaskPriority> {
    match value.unwrap_or("Medium").trim().to_ascii_lowercase().as_str() {
        "high" => Ok(TaskPriority::P0),
        "medium" => Ok(TaskPriority::P1),
        "low" => Ok(TaskPriority::P2),
        other => Err(anyhow::anyhow!("unsupported docs/TODO priority '{other}'")),
    }
}

fn split_todo_reference_list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(|item| item.trim().trim_matches('`').to_string())
        .filter(|item| !item.is_empty())
        .collect()
}

fn extract_backtick_paths(value: &str) -> Vec<String> {
    let mut paths = Vec::new();
    let mut remainder = value;
    while let Some(start) = remainder.find('`') {
        let after_start = &remainder[start + 1..];
        let Some(end) = after_start.find('`') else {
            break;
        };
        let candidate = after_start[..end].trim();
        if candidate.contains('/') {
            paths.push(candidate.to_string());
        }
        remainder = &after_start[end + 1..];
    }
    paths
}

fn todo_migration_tag(source_key: &str) -> String {
    format!("todo-import:{}", todo_migration_key(source_key))
}

fn todo_migration_key(source_key: &str) -> String {
    source_key
        .trim()
        .to_ascii_lowercase()
        .replace(' ', "-")
        .replace(':', "")
}

fn render_plan_todo_migration_result(result: &TodoMigrationResult) -> String {
    let mut lines = vec![format!(
        "Imported {} docs/TODO.md open task(s).",
        result.created_task_ids.len()
    )];
    if result.created_task_ids.is_empty() {
        lines.push("Created task ids: none".to_string());
    } else {
        lines.push(format!("Created task ids: {}", result.created_task_ids.join(", ")));
    }
    if result.skipped_source_keys.is_empty() {
        lines.push("Skipped existing imports: none".to_string());
    } else {
        lines.push(format!(
            "Skipped existing imports: {}",
            result.skipped_source_keys.join(", ")
        ));
    }
    lines.join("\n")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlanLoadKind {
    Auto,
    Todo,
    TaskHeadings,
}

impl PlanLoadKind {
    fn parse_cli(value: &str) -> anyhow::Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "auto" => Ok(Self::Auto),
            "todo" => Ok(Self::Todo),
            "task-headings" => Ok(Self::TaskHeadings),
            other => Err(anyhow::anyhow!(
                "unsupported /plan load kind '{other}'; expected auto, todo, or task-headings"
            )),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Todo => "todo",
            Self::TaskHeadings => "task-headings",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlanLoadAdapter {
    Todo,
    TaskHeadings,
}

impl PlanLoadAdapter {
    fn as_str(self) -> &'static str {
        match self {
            Self::Todo => "todo",
            Self::TaskHeadings => "task-headings",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlanLoadDisposition {
    Create,
    Update,
}

impl PlanLoadDisposition {
    fn as_str(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Update => "update",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PlanLoadDraft {
    adapter: PlanLoadAdapter,
    source_path: String,
    source_key: String,
    source_tag: String,
    title: String,
    status: PlannedTaskStatus,
    priority: TaskPriority,
    description: String,
    blocked_by: Vec<String>,
    related_paths: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PlanLoadCandidateSummary {
    disposition: PlanLoadDisposition,
    adapter: PlanLoadAdapter,
    source_path: String,
    source_key: String,
    title: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PlanLoadResult {
    source: String,
    kind: PlanLoadKind,
    apply: bool,
    matched_files: Vec<String>,
    skipped_files: Vec<String>,
    candidates: Vec<PlanLoadCandidateSummary>,
    created_task_ids: Vec<String>,
    updated_task_ids: Vec<String>,
    warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct PlanLoadParseResult {
    drafts: Vec<PlanLoadDraft>,
    warnings: Vec<String>,
}

fn parse_plan_load_args(args: &[String]) -> anyhow::Result<(String, PlanLoadKind, bool)> {
    let mut source = None::<String>;
    let mut kind = PlanLoadKind::Auto;
    let mut apply = false;
    let mut index = 0usize;
    while index < args.len() {
        match args[index].as_str() {
            "--kind" => {
                let value = args.get(index + 1).ok_or_else(|| {
                    anyhow::anyhow!(
                        "missing value after '--kind'; expected auto, todo, or task-headings"
                    )
                })?;
                kind = PlanLoadKind::parse_cli(value)?;
                index += 2;
            }
            "--apply" => {
                apply = true;
                index += 1;
            }
            flag if flag.starts_with("--") => {
                return Err(anyhow::anyhow!("unsupported flag '{flag}' for '/plan load'"));
            }
            value => {
                if source.replace(value.to_string()).is_some() {
                    return Err(anyhow::anyhow!(
                        "usage: /plan load <source> [--kind <auto|todo|task-headings>] [--apply]"
                    ));
                }
                index += 1;
            }
        }
    }

    let source = source.ok_or_else(|| {
        anyhow::anyhow!(
            "usage: /plan load <source> [--kind <auto|todo|task-headings>] [--apply]"
        )
    })?;
    Ok((source, kind, apply))
}

fn load_plan_documents(
    store: &ProjectPlanStore,
    project_root: &std::path::Path,
    source: &str,
    kind: PlanLoadKind,
    apply: bool,
) -> anyhow::Result<PlanLoadResult> {
    let source_files = collect_plan_load_source_files(project_root, source)?;
    let existing_sources = collect_existing_plan_import_sources(store)?;

    let mut matched_files = Vec::new();
    let mut skipped_files = Vec::new();
    let mut candidates = Vec::new();
    let mut drafts = Vec::new();
    let mut warnings = Vec::new();

    for source_file in source_files {
        let relative_path = normalize_plan_load_relative_path(
            source_file
                .strip_prefix(project_root)
                .unwrap_or(source_file.as_path()),
        );
        let content = std::fs::read_to_string(&source_file)
            .with_context(|| format!("failed to read {}", source_file.display()))?;
        let parse_result = parse_plan_load_drafts_for_file(&relative_path, &content, kind)?;
        warnings.extend(parse_result.warnings.iter().cloned());
        if parse_result.drafts.is_empty() {
            skipped_files.push(relative_path);
            continue;
        }
        matched_files.push(relative_path.clone());
        for draft in parse_result.drafts {
            let disposition = if existing_sources.contains_key(draft.source_tag.as_str()) {
                PlanLoadDisposition::Update
            } else {
                PlanLoadDisposition::Create
            };
            candidates.push(PlanLoadCandidateSummary {
                disposition,
                adapter: draft.adapter,
                source_path: draft.source_path.clone(),
                source_key: draft.source_key.clone(),
                title: draft.title.clone(),
            });
            drafts.push(draft);
        }
    }

    let mut result = PlanLoadResult {
        source: source.to_string(),
        kind,
        apply,
        matched_files,
        skipped_files,
        candidates,
        created_task_ids: Vec::new(),
        updated_task_ids: Vec::new(),
        warnings,
    };

    if apply {
        apply_plan_load_drafts(store, &drafts, &mut result)?;
    }

    if result.candidates.is_empty() {
        result.warnings.push(format!(
            "No importable task blocks were found under source '{}'.",
            source
        ));
    }

    dedupe_plan_load_result(&mut result);
    Ok(result)
}

fn collect_plan_load_source_files(
    project_root: &std::path::Path,
    source: &str,
) -> anyhow::Result<Vec<std::path::PathBuf>> {
    let source_path = std::path::Path::new(source);
    if source_path.is_absolute() {
        anyhow::bail!("/plan load source must be project-root relative, not absolute");
    }
    let root = std::fs::canonicalize(project_root)
        .with_context(|| format!("failed to canonicalize {}", project_root.display()))?;
    let candidate = root.join(source_path);
    let candidate = std::fs::canonicalize(&candidate)
        .with_context(|| format!("failed to resolve source {}", candidate.display()))?;
    if candidate.strip_prefix(&root).is_err() {
        anyhow::bail!("/plan load source must stay within the active project root");
    }
    let relative_candidate =
        normalize_plan_load_relative_path(candidate.strip_prefix(&root).unwrap());
    if should_skip_plan_load_path(&relative_candidate) {
        anyhow::bail!("/plan load source points at an ignored path");
    }

    let mut files = Vec::new();
    if candidate.is_file() {
        if candidate.extension().and_then(|ext| ext.to_str()) == Some("md") {
            files.push(candidate);
        }
        return Ok(files);
    }

    collect_plan_load_markdown_files(&root, &candidate, &mut files)?;
    files.sort();
    Ok(files)
}

fn collect_plan_load_markdown_files(
    project_root: &std::path::Path,
    directory: &std::path::Path,
    files: &mut Vec<std::path::PathBuf>,
) -> anyhow::Result<()> {
    for entry in std::fs::read_dir(directory)
        .with_context(|| format!("failed to read directory {}", directory.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        let relative = normalize_plan_load_relative_path(path.strip_prefix(project_root).unwrap_or(&path));
        if should_skip_plan_load_path(&relative) {
            continue;
        }
        if path.is_dir() {
            collect_plan_load_markdown_files(project_root, &path, files)?;
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("md") {
            files.push(path);
        }
    }
    Ok(())
}

fn should_skip_plan_load_path(relative_path: &str) -> bool {
    relative_path == "docs/archive"
        || relative_path.starts_with("docs/archive/")
        || relative_path == "target"
        || relative_path.starts_with("target/")
        || relative_path == ".omega-state"
        || relative_path.starts_with(".omega-state/")
}

fn parse_plan_load_drafts_for_file(
    relative_path: &str,
    content: &str,
    kind: PlanLoadKind,
) -> anyhow::Result<PlanLoadParseResult> {
    match kind {
        PlanLoadKind::Todo => parse_plan_load_todo_drafts(relative_path, content),
        PlanLoadKind::TaskHeadings => parse_plan_load_task_heading_drafts(relative_path, content),
        PlanLoadKind::Auto => {
            if relative_path == "docs/TODO.md" {
                let todo_drafts = parse_plan_load_todo_drafts(relative_path, content)?;
                if !todo_drafts.drafts.is_empty() {
                    return Ok(todo_drafts);
                }
            }
            let heading_drafts = parse_plan_load_task_heading_drafts(relative_path, content)?;
            if !heading_drafts.drafts.is_empty() {
                return Ok(heading_drafts);
            }
            let mut todo_drafts = parse_plan_load_todo_drafts(relative_path, content)?;
            todo_drafts.warnings.extend(heading_drafts.warnings);
            Ok(todo_drafts)
        }
    }
}

fn parse_plan_load_todo_drafts(
    relative_path: &str,
    content: &str,
) -> anyhow::Result<PlanLoadParseResult> {
    parse_open_todo_migration_drafts(content).map(|drafts| PlanLoadParseResult {
        drafts: drafts
            .into_iter()
            .map(|draft| plan_load_draft_from_todo(relative_path, draft, PlanLoadAdapter::Todo))
            .collect(),
        warnings: Vec::new(),
    })
}

fn plan_load_draft_from_todo(
    relative_path: &str,
    draft: TodoMigrationDraft,
    adapter: PlanLoadAdapter,
) -> PlanLoadDraft {
    PlanLoadDraft {
        adapter,
        source_tag: plan_load_source_tag(relative_path, &draft.source_key, adapter),
        source_path: relative_path.to_string(),
        source_key: draft.source_key,
        title: draft.title,
        status: draft.status,
        priority: draft.priority,
        description: draft.description,
        blocked_by: draft.blocked_by,
        related_paths: draft.related_paths,
    }
}

fn parse_plan_load_task_heading_drafts(
    relative_path: &str,
    content: &str,
) -> anyhow::Result<PlanLoadParseResult> {
    let mut drafts = Vec::new();
    let mut warnings = Vec::new();
    let mut current_heading = None::<String>;
    let mut current_lines = Vec::new();

    for line in content.lines() {
        if let Some(heading) = parse_task_heading_line(line) {
            if let Some(previous_heading) = current_heading.replace(heading) {
                finish_task_heading_import_block(
                    relative_path,
                    &previous_heading,
                    &current_lines,
                    &mut drafts,
                    &mut warnings,
                );
                current_lines.clear();
            }
            continue;
        }
        if line.trim_start().starts_with('#') {
            if let Some(previous_heading) = current_heading.take() {
                finish_task_heading_import_block(
                    relative_path,
                    &previous_heading,
                    &current_lines,
                    &mut drafts,
                    &mut warnings,
                );
                current_lines.clear();
            }
            continue;
        }
        if current_heading.is_some() {
            current_lines.push(line.to_string());
        }
    }

    if let Some(heading) = current_heading {
        finish_task_heading_import_block(
            relative_path,
            &heading,
            &current_lines,
            &mut drafts,
            &mut warnings,
        );
    }

    Ok(PlanLoadParseResult { drafts, warnings })
}

fn finish_task_heading_import_block(
    relative_path: &str,
    heading: &str,
    lines: &[String],
    drafts: &mut Vec<PlanLoadDraft>,
    warnings: &mut Vec<String>,
) {
    match build_markdown_task_import_draft(heading, lines, true) {
        Ok(Some(draft)) => drafts.push(plan_load_draft_from_todo(
            relative_path,
            draft,
            PlanLoadAdapter::TaskHeadings,
        )),
        Ok(None) => {}
        Err(error) => warnings.push(format!(
            "Skipped {} heading '{}' during /plan load: {}",
            relative_path, heading, error
        )),
    }
}

fn parse_task_heading_line(line: &str) -> Option<String> {
    let trimmed = line.trim();
    let heading = trimmed
        .strip_prefix("### ")
        .or_else(|| trimmed.strip_prefix("#### "))?;
    let heading = heading.trim();
    heading.starts_with("Task ").then(|| heading.to_string())
}

fn build_markdown_task_import_draft(
    heading: &str,
    lines: &[String],
    require_structured_metadata: bool,
) -> anyhow::Result<Option<TodoMigrationDraft>> {
    let has_structured_metadata = ["Status", "Priority", "Description", "Planning Note", "Blocked by", "Related"]
        .iter()
        .any(|field| extract_todo_field(lines, field).is_some());
    if require_structured_metadata && !has_structured_metadata {
        return Ok(None);
    }
    build_todo_migration_draft(heading, lines)
}

fn collect_existing_plan_import_sources(
    store: &ProjectPlanStore,
) -> anyhow::Result<std::collections::BTreeMap<String, String>> {
    Ok(store
        .list_tasks(TaskListFilter::default())?
        .into_iter()
        .flat_map(|task| {
            task.tags.into_iter().filter_map(move |tag| {
                (tag.starts_with("todo-import:") || tag.starts_with("doc-load:"))
                    .then(|| (tag, task.id.clone()))
            })
        })
        .collect())
}

fn apply_plan_load_drafts(
    store: &ProjectPlanStore,
    drafts: &[PlanLoadDraft],
    result: &mut PlanLoadResult,
) -> anyhow::Result<()> {
    let mut imported_sources = collect_existing_plan_import_sources(store)?;
    let mut source_keys_to_task_ids = std::collections::BTreeMap::<String, Vec<String>>::new();

    for draft in drafts {
        let task_id = if let Some(existing_task_id) = imported_sources.get(draft.source_tag.as_str()).cloned() {
            let existing_task = store
                .get_task(&existing_task_id)?
                .ok_or_else(|| anyhow::anyhow!("missing previously imported task '{existing_task_id}'"))?;
            let mut tags = existing_task.tags;
            if !tags.iter().any(|tag| tag == &draft.source_tag) {
                tags.push(draft.source_tag.clone());
                tags.sort();
            }
            let updated = store.update_task(
                &existing_task_id,
                PlannedTaskUpdate {
                    title: Some(draft.title.clone()),
                    summary: Some(draft.description.clone()),
                    requirement: Some(draft.description.clone()),
                    status: Some(draft.status),
                    acceptance: Some(Vec::new()),
                    doc_scope: None,
                    tags: Some(tags),
                },
            )?;
            result.updated_task_ids.push(updated.id.clone());
            updated.id
        } else {
            let created = store.create_task(NewPlannedTask {
                title: draft.title.clone(),
                kind: omega_plan::PlannedTaskKind::Task,
                status: draft.status,
                priority: draft.priority,
                summary: draft.description.clone(),
                requirement: draft.description.clone(),
                acceptance: Vec::new(),
                parent_id: None,
                depends_on: Vec::new(),
                tags: vec![draft.source_tag.clone()],
				doc_scope: Vec::new(),
            })?;
            result.created_task_ids.push(created.id.clone());
            created.id
        };

        imported_sources.insert(draft.source_tag.clone(), task_id.clone());
        source_keys_to_task_ids
            .entry(todo_migration_key(&draft.source_key))
            .or_default()
            .push(task_id.clone());

        store.add_artifact_link(
            &task_id,
            TaskLinkSurface::Design,
            TaskArtifactLink {
                kind: infer_task_artifact_kind(&draft.source_path, TaskLinkSurface::Design),
                path: draft.source_path.clone(),
                label: Some("source".to_string()),
            },
        )?;
        for path in &draft.related_paths {
            store.add_artifact_link(
                &task_id,
                TaskLinkSurface::Design,
                TaskArtifactLink {
                    kind: infer_task_artifact_kind(path, TaskLinkSurface::Design),
                    path: path.clone(),
                    label: None,
                },
            )?;
        }
        store.append_note(
            &task_id,
            TaskActor::Command,
            format!(
                "Loaded from {} via /plan load ({})",
                draft.source_path,
                draft.adapter.as_str(),
            ),
            None,
            None,
        )?;
    }

    for draft in drafts {
        let Some(task_id) = imported_sources.get(draft.source_tag.as_str()).cloned() else {
            continue;
        };
        for blocked_key in &draft.blocked_by {
            let normalized_key = todo_migration_key(blocked_key);
            let Some(dependency_ids) = source_keys_to_task_ids.get(&normalized_key) else {
                result.warnings.push(format!(
                    "{} references unresolved dependency '{}'.",
                    draft.source_path, blocked_key
                ));
                continue;
            };
            if dependency_ids.len() > 1 {
                result.warnings.push(format!(
                    "{} references ambiguous dependency '{}'.",
                    draft.source_path, blocked_key
                ));
                continue;
            }
            let dependency_id = &dependency_ids[0];
            if dependency_id != &task_id {
                store.mutate_dependency(&task_id, dependency_id, TaskDependencyOperation::Add)?;
            }
        }
    }

    Ok(())
}

fn plan_load_source_tag(relative_path: &str, source_key: &str, adapter: PlanLoadAdapter) -> String {
    if matches!(adapter, PlanLoadAdapter::Todo) && relative_path == "docs/TODO.md" {
        todo_migration_tag(source_key)
    } else {
        format!(
            "doc-load:{}#{}",
            normalize_plan_load_tag_path(relative_path),
            todo_migration_key(source_key),
        )
    }
}

fn normalize_plan_load_relative_path(path: &std::path::Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn normalize_plan_load_tag_path(path: &str) -> String {
    path.replace('\\', "/")
}

fn dedupe_plan_load_result(result: &mut PlanLoadResult) {
    dedupe_strings(&mut result.matched_files);
    dedupe_strings(&mut result.skipped_files);
    dedupe_strings(&mut result.created_task_ids);
    dedupe_strings(&mut result.updated_task_ids);
    dedupe_strings(&mut result.warnings);
}

fn dedupe_strings(values: &mut Vec<String>) {
    let mut seen = std::collections::BTreeSet::new();
    values.retain(|value| seen.insert(value.clone()));
}

fn render_plan_load_result(result: &PlanLoadResult) -> String {
    let would_create = result
        .candidates
        .iter()
        .filter(|candidate| candidate.disposition == PlanLoadDisposition::Create)
        .count();
    let would_update = result.candidates.len().saturating_sub(would_create);
    let mut lines = vec![format!(
        "Mode: {}",
        if result.apply { "apply" } else { "preview" }
    )];
    lines.push(format!("Source: {}", result.source));
    lines.push(format!("Kind: {}", result.kind.as_str()));
    lines.push(format!("Matched files: {}", join_or_none(&result.matched_files)));
    lines.push(format!("Skipped files: {}", join_or_none(&result.skipped_files)));
    lines.push(format!("Candidates: {}", result.candidates.len()));
    lines.push(format!("Would create: {}", would_create));
    lines.push(format!("Would update: {}", would_update));
    if result.apply {
        lines.push(format!("Created task ids: {}", join_or_none(&result.created_task_ids)));
        lines.push(format!("Updated task ids: {}", join_or_none(&result.updated_task_ids)));
    }
    if !result.candidates.is_empty() {
        lines.push("Candidate tasks:".to_string());
        lines.extend(result.candidates.iter().map(|candidate| {
            format!(
                "- {} [{}] {} :: {} ({})",
                candidate.disposition.as_str(),
                candidate.adapter.as_str(),
                candidate.source_path,
                candidate.source_key,
                candidate.title,
            )
        }));
    }
    if !result.warnings.is_empty() {
        lines.push("Warnings:".to_string());
        lines.extend(result.warnings.iter().map(|warning| format!("- {warning}")));
    }
    lines.join("\n")
}

fn build_plan_load_picker_request(result: &PlanLoadResult) -> OperatorPickerRequest {
    let item_count = result.candidates.len();
    let primary_action = OperatorPickerAction {
        action_id: "plan-load-detail".to_string(),
        label: "Detail".to_string(),
        shortcut: OperatorPickerShortcut::Enter,
        requires_selection: true,
        overlay_behavior: OperatorPickerOverlayBehavior::KeepOpen,
        intent: OperatorPickerIntent::OpenDetail,
    };
    let mut load_command = format!("/plan load {}", result.source);
    if result.kind != PlanLoadKind::Auto {
        load_command.push_str(&format!(" --kind {}", result.kind.as_str()));
    }
    load_command.push_str(" --apply");

    let secondary_actions = vec![OperatorPickerAction {
        action_id: "plan-load-confirm".to_string(),
        label: "Load".to_string(),
        shortcut: OperatorPickerShortcut::Ctrl('l'),
        requires_selection: true,
        overlay_behavior: OperatorPickerOverlayBehavior::KeepOpen,
        intent: OperatorPickerIntent::RequestConfirmSlashCommand {
            title_template: " Confirm plan load ".to_string(),
            message_template: format!(
                "Import {item_count} previewed task(s) from {} into the project plan?",
                result.source
            ),
            confirm_label: "Load".to_string(),
            command_template: load_command,
        },
    }];

    OperatorPickerRequest {
        picker_id: "plan-load-preview".to_string(),
        title: format!(" Plan Load Preview ({item_count}) "),
        empty_state: "No importable task blocks found for this source.".to_string(),
        filter_enabled: true,
        items: result
            .candidates
            .iter()
            .enumerate()
            .map(|(index, candidate)| build_plan_load_picker_item(index, candidate))
            .collect(),
        primary_action,
        secondary_actions,
    }
}

fn build_plan_list_picker_request(
    store: &ProjectPlanStore,
    tasks: &[PlannedTask],
    selected_task_id: Option<&str>,
    view: &str,
) -> anyhow::Result<OperatorPickerRequest> {
    let title = match view {
        "all" => format!(" Plan Tasks ({}) ", tasks.len()),
        "history" => format!(" Plan History ({}) ", tasks.len()),
        _ => format!(" Current Plan Tasks ({}) ", tasks.len()),
    };
    let empty_state = match view {
        "all" => "No project tasks recorded yet.",
        "history" => "No completed or archived project tasks are recorded yet.",
        _ => "No current project tasks are recorded yet.",
    };

    Ok(OperatorPickerRequest {
        picker_id: PLAN_LIST_PICKER_ID.to_string(),
        title,
        empty_state: empty_state.to_string(),
        filter_enabled: true,
        items: tasks
            .iter()
            .map(|task| build_plan_list_picker_item(store, task, selected_task_id))
            .collect::<anyhow::Result<Vec<_>>>()?,
        primary_action: OperatorPickerAction {
            action_id: "plan-list-links".to_string(),
            label: "Links".to_string(),
            shortcut: OperatorPickerShortcut::Enter,
            requires_selection: true,
            overlay_behavior: OperatorPickerOverlayBehavior::CloseOverlay,
            intent: OperatorPickerIntent::SubmitSlashCommand {
                command_template: "/plan links {id}".to_string(),
            },
        },
        secondary_actions: vec![OperatorPickerAction {
            action_id: "plan-list-select".to_string(),
            label: "Select".to_string(),
            shortcut: OperatorPickerShortcut::Ctrl('s'),
            requires_selection: true,
            overlay_behavior: OperatorPickerOverlayBehavior::CloseOverlay,
            intent: OperatorPickerIntent::SubmitSlashCommand {
                command_template: "/plan select {id}".to_string(),
            },
        }],
    })
}

fn build_plan_links_picker_request(
    project_root: &std::path::Path,
    store: &ProjectPlanStore,
    task: &PlannedTask,
) -> anyhow::Result<OperatorPickerRequest> {
    let context = store
        .resolve_task_context(&task.id)?
        .ok_or_else(|| anyhow::anyhow!("unknown task '{}'", task.id))?;
    let mut items = Vec::new();
    for link in &context.design_links {
        items.push(build_plan_link_picker_item(project_root, link, TaskLinkSurface::Design));
    }
    for link in &context.implementation_links {
        items.push(build_plan_link_picker_item(
            project_root,
            link,
            TaskLinkSurface::Implementation,
        ));
    }
    for entry in &context.recent_logs {
        items.push(build_plan_log_picker_item(&task.id, entry));
    }

    Ok(OperatorPickerRequest {
        picker_id: format!("{PLAN_LINKS_PICKER_ID_PREFIX}{}", task.id),
        title: format!(" {}: {} ", task.id, task.title),
        empty_state: "No linked artifacts or recent logs are available for this task.".to_string(),
        filter_enabled: true,
        items,
        primary_action: OperatorPickerAction {
            action_id: "plan-links-open".to_string(),
            label: "Open".to_string(),
            shortcut: OperatorPickerShortcut::Enter,
            requires_selection: true,
            overlay_behavior: OperatorPickerOverlayBehavior::CloseOverlay,
            intent: OperatorPickerIntent::SubmitSlashCommand {
                command_template: format!("/plan open-link {} {{id}}", task.id),
            },
        },
        secondary_actions: vec![OperatorPickerAction {
            action_id: "plan-links-back".to_string(),
            label: "Back".to_string(),
            shortcut: OperatorPickerShortcut::Ctrl('l'),
            requires_selection: false,
            overlay_behavior: OperatorPickerOverlayBehavior::CloseOverlay,
            intent: OperatorPickerIntent::SubmitSlashCommand {
                command_template: "/plan list".to_string(),
            },
        }],
    })
}

fn build_plan_links_navigator_request(
    project_root: &std::path::Path,
    store: &ProjectPlanStore,
    task: &PlannedTask,
    active_target_id: &str,
) -> anyhow::Result<DocumentNavigatorRequest> {
    let context = store
        .resolve_task_context(&task.id)?
        .ok_or_else(|| anyhow::anyhow!("unknown task '{}'", task.id))?;
    let mut entries = Vec::new();
    for link in &context.design_links {
        entries.push(build_plan_link_navigator_entry(
            project_root,
            &task.id,
            link,
            TaskLinkSurface::Design,
            DocumentNavigatorGroup::Context,
        ));
    }
    for link in &context.implementation_links {
        entries.push(build_plan_link_navigator_entry(
            project_root,
            &task.id,
            link,
            TaskLinkSurface::Implementation,
            DocumentNavigatorGroup::Context,
        ));
    }
    for entry in &context.recent_logs {
        entries.push(build_plan_log_navigator_entry(
            &task.id,
            entry,
            DocumentNavigatorGroup::Context,
        ));
    }

    if !entries.iter().any(|entry| entry.id == active_target_id) {
        anyhow::bail!("unknown linked artifact '{}' for task '{}'", active_target_id, task.id);
    }

    let snapshot = load_structured_docs_snapshot(project_root);
    let mut known_ids = entries
        .iter()
        .map(|entry| entry.id.clone())
        .collect::<std::collections::BTreeSet<_>>();
    entries.extend(build_related_doc_entries(
        project_root,
        snapshot.as_ref(),
        active_target_id,
        &mut known_ids,
    ));

    Ok(DocumentNavigatorRequest {
        navigator_id: format!("plan-links:{}", task.id),
        title: format!(" {}: {} ", task.id, task.title),
        origin_label: format!("Task {} linked artifacts", task.id),
        active_entry_id: active_target_id.to_string(),
        entries,
    })
}

fn build_plan_view_file_navigator_request(
    project_root: &std::path::Path,
    relative_path: &str,
) -> anyhow::Result<DocumentNavigatorRequest> {
    resolve_project_relative_file(project_root, relative_path)?;
    let label = plan_link_display_name(relative_path);
    let subtitle = Some(relative_path.to_string());
    let is_doc = relative_path.ends_with(".md");
    let mut entries = vec![DocumentNavigatorEntry {
        id: relative_path.to_string(),
        label: label.clone(),
        subtitle: subtitle.clone(),
        preview: preview_plan_link_file(project_root, relative_path),
        group: DocumentNavigatorGroup::Context,
        kind: if is_doc {
            DocumentNavigatorEntryKind::Document
        } else {
            DocumentNavigatorEntryKind::File
        },
        disabled_reason: None,
        body: build_file_navigator_body(
            project_root,
            relative_path,
            label.clone(),
            subtitle.clone(),
            vec!["Project File".to_string()],
            if is_doc {
                DocumentNavigatorBodyKind::Markdown
            } else {
                DocumentNavigatorBodyKind::File
            },
        ),
    }];

    let snapshot = load_structured_docs_snapshot(project_root);
    let mut known_ids = std::collections::BTreeSet::from([relative_path.to_string()]);
    entries.extend(build_related_doc_entries(
        project_root,
        snapshot.as_ref(),
        relative_path,
        &mut known_ids,
    ));

    Ok(DocumentNavigatorRequest {
        navigator_id: format!("plan-view-file:{}", relative_path),
        title: format!(" File: {} ", label),
        origin_label: format!("file: {}", relative_path),
        active_entry_id: relative_path.to_string(),
        entries,
    })
}

fn build_document_query_navigator_request(
    project_root: &std::path::Path,
    query_text: &str,
    results: &[omega_context::SearchResult],
) -> DocumentNavigatorRequest {
    let mut entries = results
        .iter()
        .map(|result| build_document_query_navigator_entry(project_root, query_text, result))
        .collect::<Vec<_>>();
    let active_entry_id = results
        .first()
        .map(|result| result.path.clone())
        .unwrap_or_default();
    let snapshot = load_structured_docs_snapshot(project_root);
    let mut known_ids = entries
        .iter()
        .map(|entry| entry.id.clone())
        .collect::<std::collections::BTreeSet<_>>();
    entries.extend(build_related_doc_entries(
        project_root,
        snapshot.as_ref(),
        &active_entry_id,
        &mut known_ids,
    ));

    DocumentNavigatorRequest {
        navigator_id: format!("document-query:{}", query_text.trim()),
        title: format!(" Document Query ({}) ", results.len()),
        origin_label: format!("query: {}", query_text.trim()),
        active_entry_id,
        entries,
    }
}

fn build_plan_link_navigator_entry(
    project_root: &std::path::Path,
    task_id: &str,
    link: &TaskArtifactLink,
    surface: TaskLinkSurface,
    group: DocumentNavigatorGroup,
) -> DocumentNavigatorEntry {
    let label = link
        .label
        .clone()
        .unwrap_or_else(|| plan_link_display_name(&link.path));
    let surface_label = match surface {
        TaskLinkSurface::Design => "Design",
        TaskLinkSurface::Implementation => "Implementation",
    };
    let is_doc = is_doc_artifact_kind(link.kind);
    let subtitle = Some(format!("{} · {}", surface_label, link.path));
    let body_kind = if is_doc {
        DocumentNavigatorBodyKind::Markdown
    } else {
        DocumentNavigatorBodyKind::File
    };
    DocumentNavigatorEntry {
        id: link.path.clone(),
        label: label.clone(),
        subtitle: subtitle.clone(),
        preview: preview_plan_link_file(project_root, &link.path),
        group,
        kind: if is_doc {
            DocumentNavigatorEntryKind::Document
        } else {
            DocumentNavigatorEntryKind::File
        },
        disabled_reason: None,
        body: build_file_navigator_body(
            project_root,
            &link.path,
            label,
            subtitle,
            vec![task_id.to_string(), surface_label.to_string()],
            body_kind,
        ),
    }
}

fn build_plan_log_navigator_entry(
    task_id: &str,
    entry: &TaskLogEntry,
    group: DocumentNavigatorGroup,
) -> DocumentNavigatorEntry {
    let label = preview_text(&entry.summary, 80);
    let subtitle = Some(format!("#{} {}", entry.seq, task_log_kind_label(entry.kind)));
    let preview = render_plan_log_entry_detail(task_id, entry);
    DocumentNavigatorEntry {
        id: format!("log-entry:{}", entry.seq),
        label: label.clone(),
        subtitle: subtitle.clone(),
        preview: Some(preview.clone()),
        group,
        kind: DocumentNavigatorEntryKind::Log,
        disabled_reason: None,
        body: DocumentNavigatorBody {
            title: format!("{} · log #{}", task_id, entry.seq),
            subtitle,
            breadcrumbs: vec![task_id.to_string(), "Recent Log".to_string()],
            kind: DocumentNavigatorBodyKind::Log,
            lines: preview.lines().map(str::to_string).collect(),
        },
    }
}

fn build_document_query_navigator_entry(
    project_root: &std::path::Path,
    query_text: &str,
    result: &omega_context::SearchResult,
) -> DocumentNavigatorEntry {
    let is_doc = result.doc_type.is_some() || result.path.ends_with(".md");
    let label = plan_link_display_name(&result.path);
    let subtitle = Some(format!(
        "{} · {} · score {:.2}",
        result.path,
        search_mode_label(result.mode_used),
        result.score,
    ));
    let preview = Some(preview_text(&result.preview, 160));
    DocumentNavigatorEntry {
        id: result.path.clone(),
        label: label.clone(),
        subtitle: subtitle.clone(),
        preview,
        group: DocumentNavigatorGroup::Context,
        kind: if is_doc {
            DocumentNavigatorEntryKind::Document
        } else {
            DocumentNavigatorEntryKind::SearchResult
        },
        disabled_reason: None,
        body: build_search_result_navigator_body(
            project_root,
            query_text,
            result,
            label,
            subtitle,
            if is_doc {
                DocumentNavigatorBodyKind::Markdown
            } else {
                DocumentNavigatorBodyKind::SearchPreview
            },
        ),
    }
}

fn build_file_navigator_body(
    project_root: &std::path::Path,
    relative_path: &str,
    title: String,
    subtitle: Option<String>,
    breadcrumbs: Vec<String>,
    kind: DocumentNavigatorBodyKind,
) -> DocumentNavigatorBody {
    let lines = match render_plan_view_file_content(project_root, relative_path) {
        Ok(content) => content.lines().map(str::to_string).collect(),
        Err(error) => vec![
            format!("File: {}", relative_path),
            String::new(),
            format!("Unable to load file: {}", error),
        ],
    };

    DocumentNavigatorBody {
        title,
        subtitle,
        breadcrumbs,
        kind,
        lines,
    }
}

fn build_search_result_navigator_body(
    project_root: &std::path::Path,
    query_text: &str,
    result: &omega_context::SearchResult,
    title: String,
    subtitle: Option<String>,
    kind: DocumentNavigatorBodyKind,
) -> DocumentNavigatorBody {
    match render_plan_view_file_content(project_root, &result.path) {
        Ok(content) => DocumentNavigatorBody {
            title,
            subtitle,
            breadcrumbs: vec![
                format!("query: {}", query_text.trim()),
                search_mode_label(result.mode_used).to_string(),
            ],
            kind,
            lines: content.lines().map(str::to_string).collect(),
        },
        Err(_) => DocumentNavigatorBody {
            title,
            subtitle,
            breadcrumbs: vec![
                format!("query: {}", query_text.trim()),
                search_mode_label(result.mode_used).to_string(),
            ],
            kind: DocumentNavigatorBodyKind::SearchPreview,
            lines: vec![
                format!("Path: {}", result.path),
                format!("Score: {:.2}", result.score),
                format!("Mode: {}", search_mode_label(result.mode_used)),
                String::new(),
                preview_text(&result.preview, 400),
            ],
        },
    }
}

fn load_structured_docs_snapshot(project_root: &std::path::Path) -> Option<StructuredDocsSnapshot> {
    OmegaDocument::new(project_root.to_path_buf())
        .structured_docs_snapshot()
        .ok()
}

fn build_related_doc_entries(
    project_root: &std::path::Path,
    snapshot: Option<&StructuredDocsSnapshot>,
    active_path: &str,
    known_ids: &mut std::collections::BTreeSet<String>,
) -> Vec<DocumentNavigatorEntry> {
    let Some(snapshot) = snapshot else {
        return Vec::new();
    };
    let Some(source_record) = find_structured_doc_record(snapshot, active_path) else {
        return Vec::new();
    };

    let mut entries = Vec::new();
    for relation in &source_record.relations {
        let Some(target_record) = find_structured_doc_record(snapshot, &relation.target) else {
            continue;
        };
        if !known_ids.insert(target_record.source_path.clone()) {
            continue;
        }
        entries.push(DocumentNavigatorEntry {
            id: target_record.source_path.clone(),
            label: target_record.title.clone(),
            subtitle: Some(format!("{} · {}", relation.kind, target_record.source_path)),
            preview: preview_plan_link_file(project_root, &target_record.source_path),
            group: DocumentNavigatorGroup::Related,
            kind: DocumentNavigatorEntryKind::Document,
            disabled_reason: None,
            body: build_file_navigator_body(
                project_root,
                &target_record.source_path,
                target_record.title.clone(),
                Some(target_record.source_path.clone()),
                vec![source_record.title.clone(), format!("Related via {}", relation.kind)],
                DocumentNavigatorBodyKind::Markdown,
            ),
        });
    }
    entries
}

fn find_structured_doc_record<'a>(
    snapshot: &'a StructuredDocsSnapshot,
    target: &str,
) -> Option<&'a StructuredDocumentRecord> {
    snapshot
        .records
        .iter()
        .find(|record| record.source_path == target || record.doc_id == target)
}

fn build_plan_link_picker_item(
    project_root: &std::path::Path,
    link: &TaskArtifactLink,
    surface: TaskLinkSurface,
) -> OperatorPickerItem {
    let mut badges = Vec::new();
    if is_doc_artifact_kind(link.kind) {
        badges.push("doc".to_string());
    } else {
        badges.push(match surface {
            TaskLinkSurface::Design => "design".to_string(),
            TaskLinkSurface::Implementation => "implementation".to_string(),
        });
    }
    badges.push(task_artifact_kind_label(link.kind).to_string());

    OperatorPickerItem {
        id: link.path.clone(),
        title: link
            .label
            .clone()
            .unwrap_or_else(|| plan_link_display_name(&link.path)),
        subtitle: Some(link.path.clone()),
        badges,
        preview: preview_plan_link_file(project_root, &link.path),
        disabled_reason: None,
    }
}

fn build_plan_log_picker_item(task_id: &str, entry: &TaskLogEntry) -> OperatorPickerItem {
    OperatorPickerItem {
        id: format!("log-entry:{}", entry.seq),
        title: preview_text(&entry.summary, 80),
        subtitle: Some(format!("#{} {}", entry.seq, task_log_kind_label(entry.kind))),
        badges: vec!["log".to_string()],
        preview: Some(render_plan_log_entry_detail(task_id, entry)),
        disabled_reason: None,
    }
}

fn build_plan_list_picker_item(
    store: &ProjectPlanStore,
    task: &PlannedTask,
    selected_task_id: Option<&str>,
) -> anyhow::Result<OperatorPickerItem> {
    let mut badges = vec![task.priority.as_str().to_string(), task.status.as_str().to_string()];
    if Some(task.id.as_str()) == selected_task_id {
        badges.insert(0, "selected".to_string());
    }

    let subtitle = if task.summary.trim().is_empty() {
        Some(task.id.clone())
    } else {
        Some(format!("{} :: {}", task.id, task.summary.trim()))
    };

    Ok(OperatorPickerItem {
        id: task.id.clone(),
        title: task.title.clone(),
        subtitle,
        badges,
        preview: Some(render_plan_task_detail(store, task)?),
        disabled_reason: None,
    })
}

fn build_plan_load_picker_item(
    index: usize,
    candidate: &PlanLoadCandidateSummary,
) -> OperatorPickerItem {
    OperatorPickerItem {
        id: format!("{}:{}", candidate.source_path, index),
        title: candidate.title.clone(),
        subtitle: Some(format!("{} :: {}", candidate.source_path, candidate.source_key)),
        badges: vec![
            candidate.disposition.as_str().to_string(),
            candidate.adapter.as_str().to_string(),
        ],
        preview: Some(format!(
            "title: {}\nsource: {}\nsource key: {}\nadapter: {}\ndisposition: {}",
            candidate.title,
            candidate.source_path,
            candidate.source_key,
            candidate.adapter.as_str(),
            candidate.disposition.as_str(),
        )),
        disabled_reason: None,
    }
}

fn join_or_none(values: &[String]) -> String {
    if values.is_empty() {
        "none".to_string()
    } else {
        values.join(", ")
    }
}

fn sync_plan_todo_projection(project_root: &std::path::Path, tasks: &[PlannedTask]) -> anyhow::Result<()> {
    const START: &str = "<!-- omega-plan-sync:start -->";
    const END: &str = "<!-- omega-plan-sync:end -->";
    let generated = format!(
        "{START}\n### Project Plan Projection (generated by /plan sync-todo)\n- Open planned tasks: {}\n{}\n{END}",
        tasks.len(),
        if tasks.is_empty() {
            "- No open planned tasks.".to_string()
        } else {
            tasks
                .iter()
                .map(|task| format!("- {} [{} {}] {}", task.id, task.priority.as_str(), planned_task_status_label(task.status), task.title))
                .collect::<Vec<_>>()
                .join("\n")
        }
    );

    let document = OmegaDocument::new(project_root.to_path_buf());
    let mut todo_record = match document
        .structured_docs_snapshot()?
        .records
        .into_iter()
        .find(|record| record.doc_id == "todo:docs-todo")
    {
        Some(record) => record,
        None => {
            document.manage_document(StructuredDocumentOp::ExtractSource {
                mode: StructuredDocumentMutationMode::Apply,
                sources: vec!["docs/TODO.md".to_string()],
                doc_type: Some(StructuredDocType::Todo),
            })?;
            document
                .structured_docs_snapshot()?
                .records
                .into_iter()
                .find(|record| record.doc_id == "todo:docs-todo")
                .ok_or_else(|| anyhow::anyhow!("missing structured todo record 'todo:docs-todo'"))?
        }
    };

    upsert_plan_projection_notes_block(&mut todo_record.sections, &generated);

    document.manage_document(StructuredDocumentOp::UpsertRecord {
        mode: StructuredDocumentMutationMode::Apply,
        record: todo_record,
    })?;
    document.manage_document(StructuredDocumentOp::RenderProjection {
        mode: StructuredDocumentMutationMode::Apply,
        doc_ids: vec!["todo:docs-todo".to_string()],
    })?;
    let validation = document.manage_document(StructuredDocumentOp::ValidateProjection {
        doc_ids: vec!["todo:docs-todo".to_string()],
    })?;
    if let Some(report) = validation.validation {
        if !report.ok {
            anyhow::bail!(
                "failed to validate docs/TODO.md projection after /plan sync-todo: {} mismatch(es), {} missing file(s), {} broken relation(s)",
                report.mismatched_files.len() + report.version_mismatches.len(),
                report.missing_files.len(),
                report.broken_relations.len()
            );
        }
    }
    Ok(())
}

fn upsert_plan_projection_notes_block(sections: &mut Vec<StructuredDocumentSection>, generated: &str) {
    const START: &str = "<!-- omega-plan-sync:start -->";
    const END: &str = "<!-- omega-plan-sync:end -->";

    let notes = sections.iter_mut().find(|section| section.heading == "Notes");
    let notes = match notes {
        Some(section) => section,
        None => {
            sections.push(StructuredDocumentSection {
                section_id: "notes".to_string(),
                heading: "Notes".to_string(),
                body_markdown: String::new(),
            });
            sections.last_mut().expect("notes section inserted")
        }
    };

    let existing = notes.body_markdown.trim();
    notes.body_markdown = if let (Some(start), Some(end)) = (existing.find(START), existing.find(END)) {
        let end = end + END.len();
        format!("{}{}{}", &existing[..start], generated, &existing[end..]).trim().to_string()
    } else if existing.is_empty() {
        generated.to_string()
    } else {
        format!("{}\n\n{}", generated, existing)
    };
}

fn render_plan_list(tasks: &[PlannedTask], selected_task_id: Option<&str>, view: &str) -> String {
    if tasks.is_empty() {
        return format!("Plan view: {view}\nNo tasks.");
    }

    let mut body = format!("Plan view: {view}\nTasks: {}", tasks.len());
    for task in tasks {
        let selected = if selected_task_id == Some(task.id.as_str()) {
            " selected"
        } else {
            ""
        };
        body.push_str(&format!(
            "\n- {} [{} {}]{}\n  title: {}",
            task.id,
            task.priority.as_str(),
            planned_task_status_label(task.status),
            selected,
            task.title,
        ));
    }
    body
}

fn render_plan_task_detail(store: &ProjectPlanStore, task: &PlannedTask) -> anyhow::Result<String> {
    let logs = store.load_logs(&task.id, Some(8))?;
    let dependencies = if task.depends_on.is_empty() {
        "none".to_string()
    } else {
        task.depends_on.join(", ")
    };
    let acceptance = if task.acceptance.is_empty() {
        "none".to_string()
    } else {
        task.acceptance
            .iter()
            .map(|item| format!("- {item}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let log_body = if logs.is_empty() {
        "none".to_string()
    } else {
        logs.into_iter()
            .map(|entry| format!("- #{} {:?}: {}", entry.seq, entry.kind, entry.summary))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let design_links = if task.design_links.is_empty() {
        "none".to_string()
    } else {
        task.design_links
            .iter()
            .map(|link| format!("- {}", link.path))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let implementation_links = if task.implementation_links.is_empty() {
        "none".to_string()
    } else {
        task.implementation_links
            .iter()
            .map(|link| format!("- {}", link.path))
            .collect::<Vec<_>>()
            .join("\n")
    };
    Ok(format!(
        "Task: {}\nTitle: {}\nPriority: {}\nStatus: {}\nRequirement: {}\nDependencies: {}\nAcceptance:\n{}\nDesign links:\n{}\nImplementation links:\n{}\nRecent logs:\n{}",
        task.id,
        task.title,
        task.priority.as_str(),
        planned_task_status_label(task.status),
        task.requirement,
        dependencies,
        acceptance,
        design_links,
        implementation_links,
        log_body,
    ))
}

fn planned_task_status_label(status: PlannedTaskStatus) -> &'static str {
    match status {
        PlannedTaskStatus::Backlog => "backlog",
        PlannedTaskStatus::Ready => "ready",
        PlannedTaskStatus::InProgress => "in_progress",
        PlannedTaskStatus::Blocked => "blocked",
        PlannedTaskStatus::Done => "done",
        PlannedTaskStatus::Archived => "archived",
    }
}

fn build_task_turn_log_summary(
    input: &str,
    response_text: Option<&str>,
    error_text: Option<&str>,
) -> String {
    let mut lines = vec![format!("Prompt: {}", preview_text(input, 120))];
    if let Some(response_text) = response_text {
        if !response_text.trim().is_empty() {
            lines.push(format!("Response: {}", preview_text(response_text, 160)));
        }
    }
    if let Some(error_text) = error_text {
        lines.push(format!("Error: {}", preview_text(error_text, 160)));
    }
    lines.join(" | ")
}

fn build_task_delivery_log_summary(
    input: &str,
    response_text: &str,
    delivery_summary: &runner::TurnDeliverySummary,
) -> String {
    let changed_files = if delivery_summary.changed_files.is_empty() {
        "none".to_string()
    } else {
        delivery_summary
            .changed_files
            .iter()
            .map(|file| file.path.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    };
    let loaded_skills = if delivery_summary.loaded_skill_ids.is_empty() {
        "none".to_string()
    } else {
        delivery_summary.loaded_skill_ids.join(", ")
    };
    format!(
        "Prompt: {} | Response: {} | delivery: model={} llm_requests={} tools={} failed_tools={} tokens={} in/{} out files={} skills={} changed_paths={}",
        preview_text(input, 120),
        preview_text(response_text, 160),
        delivery_summary.primary_model.as_deref().unwrap_or("unknown"),
        delivery_summary.llm_request_count,
        delivery_summary.tool_call_count,
        delivery_summary.failed_tool_count,
        delivery_summary.input_tokens,
        delivery_summary.output_tokens,
        delivery_summary.changed_files.len(),
        loaded_skills,
        changed_files,
    )
}

fn workflow_role_from_str(value: &str) -> WorkflowRunRole {
    match value {
        "child" => WorkflowRunRole::Child,
        _ => WorkflowRunRole::Root,
    }
}

fn normalized_restored_cwd(
    project_root: std::path::PathBuf,
    snapshot_cwd: Option<std::path::PathBuf>,
) -> std::path::PathBuf {
    match snapshot_cwd {
        Some(path) if path.starts_with(&project_root) => path,
        _ => project_root,
    }
}

fn build_turn_replay_entries(
    session_id: &str,
    user_input: &str,
    assistant_output: &str,
) -> Vec<SessionReplayEntry> {
    let mut entries = vec![SessionReplayEntry {
        session_id: session_id.to_string(),
        recorded_at: current_unix_timestamp(),
        kind: SessionReplayEntryKind::UserTurn,
        title: Some("User".to_string()),
        body: user_input.to_string(),
        state: None,
    }];
    if !assistant_output.trim().is_empty() {
        entries.push(SessionReplayEntry {
            session_id: session_id.to_string(),
            recorded_at: current_unix_timestamp(),
            kind: SessionReplayEntryKind::AssistantResponse,
            title: Some("Assistant".to_string()),
            body: assistant_output.to_string(),
            state: None,
        });
    }
    entries
}

fn build_command_replay_entries(
    session_id: &str,
    input: &str,
    body: &str,
    state: ResponseSectionState,
) -> Vec<SessionReplayEntry> {
    vec![SessionReplayEntry {
        session_id: session_id.to_string(),
        recorded_at: current_unix_timestamp(),
        kind: SessionReplayEntryKind::CommandSection,
        title: Some(command_title_from_input(input)),
        body: body.to_string(),
        state: Some(match state {
            ResponseSectionState::Failed => "failed",
            _ => "complete",
        }
        .to_string()),
    }]
}

fn build_session_restore_snapshot(
    session: &omega_project::ProjectSessionRef,
    visible_history: Vec<SessionContextRecord>,
    recent_context_record_count: usize,
    checkpoint_summary_count: usize,
    search_hit_count: usize,
    truncated_history: bool,
    root_workflow_id: &str,
    routing: &session_state::RoutingContext,
    todo_rendered: &str,
    project_snapshot: ProjectDetailSnapshot,
) -> SessionRestoreSnapshot {
    let active_workflow_id = routing
        .selected_workflow_id
        .clone()
        .unwrap_or_else(|| routing.active_workflow_id.clone());
    let active_workflow_role = if active_workflow_id != root_workflow_id {
        WorkflowRunRole::Child
    } else {
        routing.active_workflow_role
    };

    SessionRestoreSnapshot {
        session_id: session.session_id.clone(),
        title: session.title.clone(),
        visible_history,
        turn_count: session.turn_count,
        archived_turn_count: session.archived_turn_count,
        latest_user_turn_preview: session.last_user_turn_preview.clone(),
        recent_context_record_count,
        checkpoint_summary_count,
        search_hit_count,
        truncated_history,
        todo_rendered: todo_rendered.to_string(),
        root_workflow_id: root_workflow_id.to_string(),
        active_workflow_id,
        active_workflow_role,
        recognized_scene_id: routing.recognized_scene_id.clone(),
        selected_workflow_id: routing.selected_workflow_id.clone(),
        project_snapshot: Box::new(project_snapshot),
    }
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
        "Project: {}\nProject ID: {}\nRoot: {}\nDetection: {}\nActive session: {}\nSessions: {}\nPlan current/history: {}/{}\nPlan blocked: {}\nSelected task: {}\nDocument readiness: {}\nMemory readiness: {}",
        snapshot.record.display_name,
        snapshot.record.project_id,
        snapshot.record.root.display(),
        detection_kind_label(snapshot.record.detection_kind),
        snapshot.record.active_session_id.as_deref().unwrap_or("none"),
        snapshot.sessions.len(),
        snapshot.plan.current_task_count,
        snapshot.plan.history_task_count,
        snapshot.plan.blocked_task_count,
        snapshot.plan.selected_task_id.as_deref().unwrap_or("none"),
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

fn take_hidden_command_flag(args: &mut Vec<String>, flag: &str) -> bool {
    let original_len = args.len();
    args.retain(|arg| arg != flag);
    args.len() != original_len
}

fn build_session_picker_request(
    sessions: &[omega_project::ProjectSessionRef],
    active_session_id: Option<&str>,
    prioritize_resume_ready: bool,
) -> OperatorPickerRequest {
    let mut sessions = sessions.to_vec();
    sessions.sort_by(|left, right| {
        session_picker_sort_key(left, active_session_id, prioritize_resume_ready)
            .cmp(&session_picker_sort_key(right, active_session_id, prioritize_resume_ready))
    });

    let detail_action = OperatorPickerAction {
        action_id: "session-detail".to_string(),
        label: "Detail".to_string(),
        shortcut: if prioritize_resume_ready {
            OperatorPickerShortcut::Ctrl('o')
        } else {
            OperatorPickerShortcut::Enter
        },
        requires_selection: true,
        overlay_behavior: OperatorPickerOverlayBehavior::KeepOpen,
        intent: OperatorPickerIntent::SubmitSlashCommand {
            command_template: format!("/session info {{id}} {SESSION_PICKER_FLAG}"),
        },
    };
    let resume_action = OperatorPickerAction {
        action_id: "session-resume".to_string(),
        label: "Resume".to_string(),
        shortcut: if prioritize_resume_ready {
            OperatorPickerShortcut::Enter
        } else {
            OperatorPickerShortcut::Ctrl('r')
        },
        requires_selection: true,
        overlay_behavior: OperatorPickerOverlayBehavior::CloseOverlay,
        intent: OperatorPickerIntent::SubmitSlashCommand {
            command_template: format!("/session resume {{id}} {SESSION_PICKER_FLAG}"),
        },
    };
    let primary_action = if prioritize_resume_ready {
        resume_action.clone()
    } else {
        detail_action.clone()
    };
    let mut secondary_actions = if prioritize_resume_ready {
        vec![detail_action, resume_action]
    } else {
        vec![resume_action]
    };
    secondary_actions.extend([
        OperatorPickerAction {
            action_id: "session-archive".to_string(),
            label: "Archive".to_string(),
            shortcut: OperatorPickerShortcut::Ctrl('a'),
            requires_selection: true,
            overlay_behavior: OperatorPickerOverlayBehavior::KeepOpen,
            intent: OperatorPickerIntent::SubmitSlashCommand {
                command_template: format!("/session archive {{id}} {SESSION_PICKER_FLAG}"),
            },
        },
        OperatorPickerAction {
            action_id: "session-delete".to_string(),
            label: "Delete".to_string(),
            shortcut: OperatorPickerShortcut::Ctrl('d'),
            requires_selection: true,
            overlay_behavior: OperatorPickerOverlayBehavior::KeepOpen,
            intent: OperatorPickerIntent::RequestConfirmSlashCommand {
                title_template: " Confirm session delete ".to_string(),
                message_template: "Delete session {title} ({id}) and remove its saved artifacts?"
                    .to_string(),
                confirm_label: "Delete".to_string(),
                command_template: format!("/session delete {{id}} {SESSION_PICKER_FLAG}"),
            },
        },
        OperatorPickerAction {
            action_id: "session-new".to_string(),
            label: "New".to_string(),
            shortcut: OperatorPickerShortcut::Ctrl('n'),
            requires_selection: false,
            overlay_behavior: OperatorPickerOverlayBehavior::CloseOverlay,
            intent: OperatorPickerIntent::SubmitSlashCommand {
                command_template: format!("/session new {SESSION_PICKER_FLAG}"),
            },
        },
    ]);

    OperatorPickerRequest {
        picker_id: SESSION_PICKER_ID.to_string(),
        title: if prioritize_resume_ready {
            " Resume Session ".to_string()
        } else {
            " Sessions ".to_string()
        },
        empty_state: if prioritize_resume_ready {
            "No saved sessions are available to resume.".to_string()
        } else {
            "No sessions recorded for the current project.".to_string()
        },
        filter_enabled: true,
        items: sessions
            .iter()
            .map(|session| build_session_picker_item(session, active_session_id))
            .collect(),
        primary_action,
        secondary_actions,
    }
}

fn build_session_picker_item(
    session: &omega_project::ProjectSessionRef,
    active_session_id: Option<&str>,
) -> OperatorPickerItem {
    let mut badges = Vec::new();
    if Some(session.session_id.as_str()) == active_session_id {
        badges.push("current".to_string());
    } else {
        badges.push(project_session_status_label(session.status).to_string());
    }
    if session.resume_ready {
        badges.push("resume-ready".to_string());
    }
    badges.push(format!("turns:{}", session.turn_count));
    badges.push(format!("archived:{}", session.archived_turn_count));

    OperatorPickerItem {
        id: session.session_id.clone(),
        title: session.title.clone(),
        subtitle: session.last_user_turn_preview.clone(),
        badges,
        preview: Some(format!(
            "id: {}\nstatus: {}\nturns: {}\nresume ready: {}\narchived turns: {}\nlatest user preview: {}",
            session.session_id,
            project_session_status_label(session.status),
            session.turn_count,
            session.resume_ready,
            session.archived_turn_count,
            session
                .last_user_turn_preview
                .as_deref()
                .unwrap_or("none"),
        )),
        disabled_reason: None,
    }
}

fn session_picker_sort_key(
    session: &omega_project::ProjectSessionRef,
    active_session_id: Option<&str>,
    prioritize_resume_ready: bool,
) -> (u8, u8, u8, std::cmp::Reverse<u64>, String) {
    let current_rank = if Some(session.session_id.as_str()) == active_session_id {
        0
    } else {
        1
    };
    let resume_rank = if prioritize_resume_ready && session.resume_ready {
        0
    } else {
        1
    };
    let status_rank = match session.status {
        ProjectSessionStatus::Active => 0,
        ProjectSessionStatus::Idle => 1,
        ProjectSessionStatus::Archived => 2,
    };
    (
        resume_rank,
        current_rank,
        status_rank,
        std::cmp::Reverse(session.last_active_at),
        session.title.to_ascii_lowercase(),
    )
}

struct SessionLedgerInfo {
    total_record_count: usize,
    replay_entry_count: usize,
    working_set_snapshot_count: usize,
    checkpoint_count: usize,
    latest_checkpoint_summary: Option<String>,
}

fn load_session_ledger_info(
    project_handle: &Arc<OmegaProjectHandle>,
    session_id: &str,
) -> anyhow::Result<SessionLedgerInfo> {
    let records = project_handle.load_context_records(session_id)?;
    let replay_entry_count = records
        .iter()
        .filter(|record| matches!(record.record, SessionContextRecordKind::ReplayEntry { .. }))
        .count();
    let working_set_snapshot_count = records
        .iter()
        .filter(|record| {
            matches!(
                record.record,
                SessionContextRecordKind::WorkingSetSnapshot { .. }
            )
        })
        .count();
    let checkpoint_count = records
        .iter()
        .filter(|record| {
            matches!(
                record.record,
                SessionContextRecordKind::CompressionCheckpoint { .. }
            )
        })
        .count();
    let latest_checkpoint_summary = records.iter().rev().find_map(|record| match &record.record {
        SessionContextRecordKind::CompressionCheckpoint { summary, .. } => {
            Some(preview_text(summary, 120))
        }
        SessionContextRecordKind::WorkingSetSnapshot { .. }
        | SessionContextRecordKind::ReplayEntry { .. } => None,
    });

    Ok(SessionLedgerInfo {
        total_record_count: records.len(),
        replay_entry_count,
        working_set_snapshot_count,
        checkpoint_count,
        latest_checkpoint_summary,
    })
}

fn render_session_info(
    session: &omega_project::ProjectSessionRef,
    snapshot: Option<&ProjectSessionSnapshot>,
    ledger_info: &SessionLedgerInfo,
) -> String {
    format!(
        "Session: {}\nSession ID: {}\nStatus: {}\nTurns: {}\nResume ready: {}\nArchived turns: {}\nCanonical ledger: {}\nLedger records: {}\nReplay entries: {}\nWorking snapshots: {}\nCheckpoint summaries: {}\nLatest checkpoint summary: {}\nLatest user preview: {}\nSnapshot workflow: {}\nSnapshot skills: {}",
        session.title,
        session.session_id,
        project_session_status_label(session.status),
        session.turn_count,
        session.resume_ready,
        session.archived_turn_count,
        if ledger_info.total_record_count > 0 {
            "present"
        } else {
            "empty"
        },
        ledger_info.total_record_count,
        ledger_info.replay_entry_count,
        ledger_info.working_set_snapshot_count,
        ledger_info.checkpoint_count,
        ledger_info
            .latest_checkpoint_summary
            .as_deref()
            .unwrap_or("none"),
        session.last_user_turn_preview.as_deref().unwrap_or("none"),
        snapshot
            .map(|snapshot| snapshot.routing.active_workflow_id.as_str())
            .unwrap_or("none"),
        snapshot
            .map(|snapshot| snapshot.skill_routing.loaded_skill_ids.join(", "))
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "none".to_string()),
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
