use std::sync::{mpsc, Arc, Mutex};
use std::thread;

use omega_core::{Agent, CoreSharedTodoManager, DynLlmClient, Message, TodoManager};
use omega_hooks::HookHost;
use omega_skills::SkillLoader;
use omega_workflow::{SceneCatalog, WorkflowCatalog, WorkflowPromptCatalog};
use tokio::runtime::Handle;
use tokio::sync::watch;
use tracing::{error, info};

const SUMMARY_CHAR_LIMIT: usize = 2_000;
const CONTEXT_SAFETY_MARGIN_TOKENS: u32 = 2_000;
const TOKEN_ESTIMATE_DIVISOR: usize = 4;
const REPAIR_PASS_MAX_ITERATIONS: u32 = 1;

mod output;
mod prompt_builder;
mod routing;
mod runtime_message;
mod runner;
mod runtime_ui;
mod hook_adapter;
mod session_state;
mod skill_catalog;
mod tool_catalog;
mod ui_emit;

pub use omega_workflow::{
    StepSkillRequest, StepToolRequest, EXECUTE_STEP_ID, EXPLORE_STEP_ID, FEATURE_SCENE_ID,
    FEATURE_WORKFLOW_ID, PLAN_STEP_ID, REPORT_STEP_ID, RESEARCH_SCENE_ID, RESEARCH_WORKFLOW_ID,
    SCENE_RECOGNITION_STEP_ID, SELECT_WORKFLOW_STEP_ID,
};
pub use runtime_ui::{
    ActivityTarget, OverlayRequest, OverlayTarget, ResponseSection, ResponseSectionDelta,
    ResponseSectionKind, ResponseSectionMetadata, ResponseSectionState, StepSubflowRef,
    StepSubflowState, StepSubflowStatus,
    ExecuteProgressDiagnostics, RuntimeUiBridge, RuntimeUiEffect, RuntimeUiEnvelope,
    RuntimeUiMessage, RuntimeUiSink, SessionRuntimeContext, StatusSlot, StatusValue,
    StepContextWrite, StepContextWriteKind, StepDiagnostics, StepInputDiagnostics,
    StepInputStatus, StepOutputAttemptKind, StepOutputContractMode, StepOutputDiagnostics,
    StepOutputRecoveryDecision, StepOutputStatus, StepSummarySource, ToolRun,
    ToolRunDetail, ToolRunStatus, UiContent, UiMessageKind, UiPriority, UiSource, UiTarget,
    WorkflowRunRole,
};
pub use runtime_message::{
    ConversationMessage, LegacyRuntimeUiBridge, RuntimeContentKind, RuntimeMessage,
    RuntimeMessageBridge, RuntimeMessageEnvelope, RuntimePriority, RuntimeSource,
    SessionRoutingStatus, SharedRuntimeMessageBridge, StateMessage, WorkflowStepStatus,
};
pub use skill_catalog::{ResolvedSkillSet, SessionSkillCatalog};
pub use tool_catalog::{ResolvedToolSet, SessionToolCatalog};

#[cfg(test)]
pub(crate) use output::{parse_json_values, validate_schema_file};
#[cfg(test)]
pub(crate) use prompt_builder::render_output_contract;
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

struct AgentSlot {
    turn_id: u64,
    agent: Option<Agent>,
}

pub struct AgentSession {
    agent_slot: Arc<Mutex<AgentSlot>>,
    turn_checkpoint: Arc<Mutex<Vec<Message>>>,
    active_turn_tx: watch::Sender<u64>,
    session_context: Arc<Mutex<SessionContext>>,
    client: DynLlmClient,
    base_system: String,
    cwd: std::path::PathBuf,
    todo_manager: CoreSharedTodoManager,
    hook_host: Arc<HookHost>,
    skill_catalog: Arc<SessionSkillCatalog>,
    tool_catalog: Arc<SessionToolCatalog>,
    runtime_handle: Handle,
    scene_catalog: SceneCatalog,
    workflow_catalog: WorkflowCatalog,
    prompt_catalog: WorkflowPromptCatalog,
    context_window: u32,
    max_output_tokens: u32,
    bash_allowed_commands: Vec<String>,
}

impl AgentSession {
    pub fn new(config: AgentSessionConfig) -> anyhow::Result<Self> {
        let skill_loader = SkillLoader::from_repo_root(&config.cwd)?;
        let skill_catalog = Arc::new(SessionSkillCatalog::new(skill_loader));
        let todo_manager = Arc::new(Mutex::new(TodoManager::new()));
        let hook_host = Arc::new(HookHost::load(&config.cwd)?);
        let dispatcher = omega_core::create_default_tools_with_todo_manager_and_tool_limits(
            config.cwd.clone(),
            todo_manager.clone(),
            config.bash_allowed_commands.clone(),
            config.batch_max_requests,
        );
        let tool_catalog = Arc::new(SessionToolCatalog::new(
            dispatcher
                .tool_names()
                .into_iter()
                .map(ToOwned::to_owned)
                .collect(),
        ));
        let initial_system =
            skill_catalog.build_system_prompt(&config.system, "", &StepSkillRequest::MatchTask);
        let mut agent = Agent::new(config.client.clone(), initial_system, dispatcher)?;
        agent.set_max_tokens(config.max_output_tokens);
        let checkpoint = agent.messages().to_vec();
        let (active_turn_tx, _active_turn_rx) = watch::channel(0u64);

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
            client: config.client,
            base_system: config.system,
            cwd: config.cwd,
            todo_manager,
            hook_host,
            skill_catalog,
            tool_catalog,
            runtime_handle: config.runtime_handle,
            scene_catalog: config.scene_catalog,
            workflow_catalog: config.workflow_catalog,
            prompt_catalog: config.prompt_catalog,
            context_window: config.context_window,
            max_output_tokens: config.max_output_tokens,
            bash_allowed_commands: config.bash_allowed_commands,
        })
    }

    pub fn is_ready(&self) -> bool {
        self.agent_slot.lock().unwrap().agent.is_some()
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

    pub fn interrupt(&self, replacement_turn_id: u64) -> anyhow::Result<()> {
        let checkpoint = self.turn_checkpoint.lock().unwrap().clone();
        let system = self.skill_catalog.build_system_prompt(
            &self.base_system,
            "",
            &StepSkillRequest::MatchTask,
        );
        let dispatcher = omega_core::create_default_tools_with_todo_manager_and_bash_allowlist(
            self.cwd.clone(),
            self.todo_manager.clone(),
            self.bash_allowed_commands.clone(),
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

        let tx_callback = tx.clone();
        let tx_result = tx;
        let handle = self.runtime_handle.clone();
        let cancel_turn_rx = self.active_turn_tx.subscribe();
        let base_system = self.base_system.clone();
        let cwd = self.cwd.clone();
        let todo_manager = self.todo_manager.clone();
        let hook_host = self.hook_host.clone();
        let skill_catalog = self.skill_catalog.clone();
        let tool_catalog = self.tool_catalog.clone();
        let scene_catalog = self.scene_catalog.clone();
        let workflow_catalog = self.workflow_catalog.clone();
        let prompt_catalog = self.prompt_catalog.clone();
        let session_context = self.session_context.clone();
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
            info!(
                turn_id,
                root_workflow_id = %scene_catalog.root_workflow_id,
                latest_user_turn = %preview_text(&input, 160),
                carried_step_summaries = turn_context.step_summaries.len(),
                "session turn started"
            );
            let runner = runner::WorkflowTurnRunner::new(
                &handle,
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
                ui_emit::send_turn_finished(&*tx_result, turn_id);
            }
        });

        Ok(())
    }
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
