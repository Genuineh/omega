use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, mpsc};
use std::thread;

use omega_core::{
    Agent, ChatEvent, CoreSharedTodoManager, DynLlmClient, Message, TodoItem, TodoManager,
    TodoStatus,
};
use omega_skills::SkillLoader;
use omega_workflow::{
    ANALYSIS_STEP_ID, DataFormat, EXECUTE_STEP_ID, FEATURE_WORKFLOW_ID, PLAN_STEP_ID,
    REPORT_STEP_ID, SCENE_RECOGNITION_STEP_ID, SELECT_WORKFLOW_STEP_ID, SceneCatalog,
    StepInputContract, StepOutputContract, WorkflowCatalog, WorkflowPromptCatalog, WorkflowPrompts,
    WorkflowStep, WorkflowStepState,
};
use serde::Deserialize;
use serde_json::Value;
use tokio::runtime::Handle;
use tracing::{debug, error, info};

const SUMMARY_CHAR_LIMIT: usize = 2_000;
const CONTEXT_SAFETY_MARGIN_TOKENS: u32 = 2_000;
const TOKEN_ESTIMATE_DIVISOR: usize = 4;

mod runtime_ui;
mod skill_catalog;
mod tool_catalog;

pub use omega_workflow::{StepSkillRequest, StepToolRequest};
pub use runtime_ui::{
    ActivityTarget, OverlayRequest, OverlayTarget, ResponseSection, ResponseSectionDelta,
    ResponseSectionKind, ResponseSectionMetadata, ResponseSectionState, RuntimeUiBridge,
    RuntimeUiEffect, RuntimeUiEnvelope, RuntimeUiMessage, RuntimeUiSink, SessionRuntimeContext,
    StatusSlot, StatusValue, StepContextWrite, StepContextWriteKind, StepDiagnostics,
    StepInputDiagnostics, StepInputStatus, StepOutputContractMode, StepOutputDiagnostics,
    StepOutputStatus, StepSummarySource, ToolRun, ToolRunDetail, ToolRunStatus, UiContent,
    UiMessageKind, UiPriority, UiSource, UiTarget, WorkflowRunRole,
};
pub use skill_catalog::{ResolvedSkillSet, SessionSkillCatalog};
pub use tool_catalog::{ResolvedToolSet, SessionToolCatalog};

pub struct AgentSessionConfig {
    pub client: DynLlmClient,
    pub system: String,
    pub cwd: PathBuf,
    pub runtime_handle: Handle,
    pub scene_catalog: SceneCatalog,
    pub workflow_catalog: WorkflowCatalog,
    pub prompt_catalog: WorkflowPromptCatalog,
    pub context_window: u32,
    pub max_output_tokens: u32,
    pub bash_allowed_commands: Vec<String>,
}

struct AgentSlot {
    turn_id: u64,
    agent: Option<Agent>,
}

pub struct AgentSession {
    agent_slot: Arc<Mutex<AgentSlot>>,
    turn_checkpoint: Arc<Mutex<Vec<Message>>>,
    session_context: Arc<Mutex<SessionContext>>,
    client: DynLlmClient,
    base_system: String,
    cwd: PathBuf,
    todo_manager: CoreSharedTodoManager,
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct SessionContext {
    latest_user_turn: String,
    routing: RoutingContext,
    step_summaries: Vec<StepSummary>,
    step_outputs: BTreeMap<String, Value>,
}

impl SessionContext {
    fn new(root_workflow_id: impl Into<String>) -> Self {
        Self {
            latest_user_turn: String::new(),
            routing: RoutingContext::for_workflow(root_workflow_id.into(), WorkflowRunRole::Root),
            step_summaries: Vec::new(),
            step_outputs: BTreeMap::new(),
        }
    }

    fn begin_turn(
        &mut self,
        latest_user_turn: impl Into<String>,
        root_workflow_id: impl Into<String>,
    ) {
        self.latest_user_turn = latest_user_turn.into();
        self.routing = RoutingContext::for_workflow(root_workflow_id.into(), WorkflowRunRole::Root);
        self.step_outputs.clear();
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RoutingContext {
    recognized_scene_id: Option<String>,
    selected_workflow_id: Option<String>,
    active_workflow_id: String,
    active_workflow_role: WorkflowRunRole,
}

impl RoutingContext {
    fn for_workflow(active_workflow_id: String, active_workflow_role: WorkflowRunRole) -> Self {
        Self {
            recognized_scene_id: None,
            selected_workflow_id: None,
            active_workflow_id,
            active_workflow_role,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StepSummary {
    workflow_id: String,
    step_id: String,
    title: String,
    summary: String,
    estimated_tokens: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StepExecutionInput {
    base_system: String,
    resolved_tools: ResolvedToolSet,
    resolved_skills: ResolvedSkillSet,
    session_context: SessionContext,
    structured_input: Option<Value>,
    todo_snapshot: Option<String>,
    step: WorkflowStep,
    step_prompt: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StepExecutionResult {
    final_text: String,
    structured_output: Option<Value>,
    summary: StepSummary,
    session_writes: Vec<StepContextWrite>,
    transition: StepTransition,
}

#[derive(Debug, Clone, Copy)]
struct StepDiagnosticContext<'a> {
    workflow_id: &'a str,
    workflow_role: WorkflowRunRole,
    step: &'a WorkflowStep,
    index: usize,
    total: usize,
}

#[derive(Debug, Clone)]
struct OutputDiagnosticState<'a> {
    status: StepOutputStatus,
    structured_output: Option<&'a Value>,
    attempts: u32,
    retry_count: u32,
    max_retries: u32,
    error: Option<&'a str>,
    session_writes: Vec<StepContextWrite>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
enum StepTransition {
    Continue,
    StartWorkflow { workflow_id: String },
    FinishTurn,
    Error { message: String },
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
struct FeatureAnalysisOutput {
    objective: String,
    constraints: Vec<String>,
    risks: Vec<String>,
    affected_paths: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
struct FeaturePlanOutput {
    goal: String,
    tasks: Vec<FeaturePlanTask>,
    validation_targets: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
struct FeaturePlanTask {
    id: String,
    title: String,
    description: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
struct FeatureExecuteOutput {
    completed_tasks: Vec<String>,
    open_tasks: Vec<String>,
    validation_results: Vec<FeatureValidationResult>,
    changed_paths: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
struct FeatureValidationResult {
    target: String,
    status: String,
    #[serde(default)]
    details: Option<String>,
}

impl AgentSession {
    pub fn new(config: AgentSessionConfig) -> anyhow::Result<Self> {
        let skill_loader = SkillLoader::from_repo_root(&config.cwd)?;
        let skill_catalog = Arc::new(SessionSkillCatalog::new(skill_loader));
        let todo_manager = Arc::new(Mutex::new(TodoManager::new()));
        let dispatcher = omega_core::create_default_tools_with_todo_manager_and_bash_allowlist(
            config.cwd.clone(),
            todo_manager.clone(),
            config.bash_allowed_commands.clone(),
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
            session_context: Arc::new(Mutex::new(SessionContext::new(
                config.scene_catalog.root_workflow_id.clone(),
            ))),
            client: config.client,
            base_system: config.system,
            cwd: config.cwd,
            todo_manager,
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
        Ok(())
    }

    pub fn spawn_turn(
        &self,
        input: String,
        turn_id: u64,
        tx: mpsc::Sender<RuntimeUiEnvelope>,
    ) -> anyhow::Result<()> {
        self.agent_slot.lock().unwrap().turn_id = turn_id;

        let agent_slot = self.agent_slot.clone();
        let mut agent = match self.agent_slot.lock().unwrap().agent.take() {
            Some(agent) => agent,
            None => return Err(anyhow::anyhow!("agent turn already in progress")),
        };

        let tx_callback = tx.clone();
        let tx_result = tx;
        let handle = self.runtime_handle.clone();
        let base_system = self.base_system.clone();
        let cwd = self.cwd.clone();
        let todo_manager = self.todo_manager.clone();
        let skill_catalog = self.skill_catalog.clone();
        let tool_catalog = self.tool_catalog.clone();
        let scene_catalog = self.scene_catalog.clone();
        let workflow_catalog = self.workflow_catalog.clone();
        let prompt_catalog = self.prompt_catalog.clone();
        let session_context = self.session_context.clone();
        let context_window = self.context_window;
        let max_output_tokens = self.max_output_tokens;
        thread::spawn(move || {
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
            let runner = WorkflowTurnRunner {
                handle: &handle,
                skill_catalog: &skill_catalog,
                tool_catalog: &tool_catalog,
                base_system: &base_system,
                input: &input,
                cwd: &cwd,
                todo_manager: &todo_manager,
                scene_catalog: &scene_catalog,
                workflow_catalog: &workflow_catalog,
                prompt_catalog: &prompt_catalog,
                context_window,
                max_output_tokens,
                turn_id,
                tx_callback: &tx_callback,
                tx_result: &tx_result,
            };
            let result = runner.run(&mut agent, &mut turn_context);

            {
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
            }

            match result {
                Ok(text) if !text.is_empty() => {
                    send_assistant_text(&tx_result, turn_id, &text);
                }
                Ok(_) => {}
                Err(e) => {
                    error!(error = %e, "agent loop error");
                    send_error_text(&tx_result, turn_id, &format!("Error: {e}"));
                }
            }

            let mut slot = agent_slot.lock().unwrap();
            if slot.turn_id == turn_id {
                slot.agent = Some(agent);
            }

            send_turn_finished(&tx_result, turn_id);
        });

        Ok(())
    }
}

struct WorkflowTurnRunner<'a> {
    handle: &'a Handle,
    skill_catalog: &'a Arc<SessionSkillCatalog>,
    tool_catalog: &'a Arc<SessionToolCatalog>,
    base_system: &'a str,
    input: &'a str,
    cwd: &'a PathBuf,
    todo_manager: &'a CoreSharedTodoManager,
    scene_catalog: &'a SceneCatalog,
    workflow_catalog: &'a WorkflowCatalog,
    prompt_catalog: &'a WorkflowPromptCatalog,
    context_window: u32,
    max_output_tokens: u32,
    turn_id: u64,
    tx_callback: &'a mpsc::Sender<RuntimeUiEnvelope>,
    tx_result: &'a mpsc::Sender<RuntimeUiEnvelope>,
}

struct StepResponseStreamer<'a> {
    tx: &'a mpsc::Sender<RuntimeUiEnvelope>,
    turn_id: u64,
    primary_section_id: String,
    thinking_section_id: String,
    primary_section: ResponseSection,
    thinking_section: ResponseSection,
    thinking_started: bool,
    primary_sanitizer: ProviderMarkupSanitizer,
    thinking_sanitizer: ProviderMarkupSanitizer,
}

impl<'a> StepResponseStreamer<'a> {
    fn new(
        tx: &'a mpsc::Sender<RuntimeUiEnvelope>,
        turn_id: u64,
        workflow_id: &str,
        role: WorkflowRunRole,
        step: &WorkflowStep,
        is_final_step: bool,
        scene_id: Option<&str>,
    ) -> Self {
        let base_id = format!(
            "turn-{turn_id}:{}:{}:{}",
            role.as_str(),
            workflow_id,
            step.id
        );
        let primary_kind = if role == WorkflowRunRole::Root {
            ResponseSectionKind::Routing
        } else if is_final_step {
            ResponseSectionKind::FinalAnswer
        } else {
            ResponseSectionKind::Step
        };
        let primary_title = if primary_kind == ResponseSectionKind::FinalAnswer {
            "Final Answer".to_string()
        } else {
            step.label.clone()
        };
        let metadata = ResponseSectionMetadata {
            scene_id: scene_id.map(ToOwned::to_owned),
            workflow_id: workflow_id.to_string(),
            workflow_role: role,
            step_id: Some(step.id.clone()),
            step_label: Some(step.label.clone()),
        };

        Self {
            tx,
            turn_id,
            primary_section_id: base_id.clone(),
            thinking_section_id: format!("{base_id}:thinking"),
            primary_section: ResponseSection {
                id: base_id,
                parent_id: None,
                kind: primary_kind,
                title: primary_title,
                state: ResponseSectionState::Streaming,
                metadata: metadata.clone(),
            },
            thinking_section: ResponseSection {
                id: String::new(),
                parent_id: None,
                kind: ResponseSectionKind::Thinking,
                title: "Thinking".to_string(),
                state: ResponseSectionState::Streaming,
                metadata,
            },
            thinking_started: false,
            primary_sanitizer: ProviderMarkupSanitizer::default(),
            thinking_sanitizer: ProviderMarkupSanitizer::default(),
        }
    }

    fn begin(&self) {
        send_begin_response_section(self.tx, self.turn_id, self.primary_section.clone());
    }

    fn primary_section_id(&self) -> &str {
        &self.primary_section_id
    }

    fn push_chat_event(&mut self, event: &ChatEvent) {
        match event {
            ChatEvent::TextDelta { text } if !text.is_empty() => {
                let sanitized = self.primary_sanitizer.push(text);
                self.append_primary_text(&sanitized);
            }
            ChatEvent::ThinkingDelta { thinking, .. } if !thinking.is_empty() => {
                let sanitized = self.thinking_sanitizer.push(thinking);
                self.append_thinking_text(&sanitized);
            }
            _ => {}
        }
    }

    fn complete(&mut self) {
        self.flush_pending_sanitized_text();
        if self.thinking_started {
            send_complete_response_section(
                self.tx,
                self.turn_id,
                &self.thinking_section_id,
                ResponseSectionState::Complete,
            );
        }
        send_complete_response_section(
            self.tx,
            self.turn_id,
            &self.primary_section_id,
            ResponseSectionState::Complete,
        );
    }

    fn fail(&mut self) {
        self.flush_pending_sanitized_text();
        if self.thinking_started {
            send_complete_response_section(
                self.tx,
                self.turn_id,
                &self.thinking_section_id,
                ResponseSectionState::Failed,
            );
        }
        send_complete_response_section(
            self.tx,
            self.turn_id,
            &self.primary_section_id,
            ResponseSectionState::Failed,
        );
    }

    fn append_primary_text(&self, text: &str) {
        if text.is_empty() {
            return;
        }
        send_append_response_section(
            self.tx,
            self.turn_id,
            &self.primary_section_id,
            ResponseSectionDelta::Text(text.to_string()),
        );
    }

    fn append_thinking_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        if !self.thinking_started {
            self.thinking_started = true;
            self.thinking_section.id = self.thinking_section_id.clone();
            self.thinking_section.parent_id = Some(self.primary_section_id.clone());
            send_begin_response_section(self.tx, self.turn_id, self.thinking_section.clone());
        }
        send_append_response_section(
            self.tx,
            self.turn_id,
            &self.thinking_section_id,
            ResponseSectionDelta::Text(text.to_string()),
        );
    }

    fn flush_pending_sanitized_text(&mut self) {
        let remaining_primary = self.primary_sanitizer.finish();
        self.append_primary_text(&remaining_primary);

        let remaining_thinking = self.thinking_sanitizer.finish();
        self.append_thinking_text(&remaining_thinking);
    }
}

struct ToolRunTracker<'a> {
    tx: &'a mpsc::Sender<RuntimeUiEnvelope>,
    turn_id: u64,
    parent_section_id: String,
    tool_runs: BTreeMap<String, ToolRun>,
}

impl<'a> ToolRunTracker<'a> {
    fn new(
        tx: &'a mpsc::Sender<RuntimeUiEnvelope>,
        turn_id: u64,
        parent_section_id: String,
    ) -> Self {
        Self {
            tx,
            turn_id,
            parent_section_id,
            tool_runs: BTreeMap::new(),
        }
    }

    fn observe_chat_event(&mut self, event: &ChatEvent) {
        let ChatEvent::ToolUse { id, name, input } = event else {
            return;
        };

        let tool_run = ToolRun {
            id: id.clone(),
            parent_section_id: self.parent_section_id.clone(),
            tool_name: name.clone(),
            status: ToolRunStatus::Running,
            invocation_preview: preview_tool_invocation(name, input),
            result_preview: None,
            detail: ToolRunDetail {
                title: format!(" Tool: {} ", name),
                lines: build_tool_run_detail_lines(name, input, None),
            },
        };
        self.tool_runs.insert(id.clone(), tool_run.clone());
        send_begin_tool_run(self.tx, self.turn_id, tool_run);
    }

    fn complete_tool_run(
        &mut self,
        tool_use_id: &str,
        tool_name: &str,
        tool_input: &serde_json::Value,
        output: &str,
    ) {
        let status = if output.starts_with("Error:") {
            ToolRunStatus::Failed
        } else {
            ToolRunStatus::Complete
        };

        let mut tool_run = self
            .tool_runs
            .remove(tool_use_id)
            .unwrap_or_else(|| ToolRun {
                id: tool_use_id.to_string(),
                parent_section_id: self.parent_section_id.clone(),
                tool_name: tool_name.to_string(),
                status: ToolRunStatus::Running,
                invocation_preview: preview_tool_invocation(tool_name, tool_input),
                result_preview: None,
                detail: ToolRunDetail {
                    title: format!(" Tool: {} ", tool_name),
                    lines: build_tool_run_detail_lines(tool_name, tool_input, None),
                },
            });

        tool_run.status = status;
        tool_run.result_preview = Some(preview_text(output, 100));
        tool_run.detail = ToolRunDetail {
            title: format!(" Tool: {} ", tool_name),
            lines: build_tool_run_detail_lines(tool_name, tool_input, Some(output)),
        };

        send_update_tool_run(self.tx, self.turn_id, tool_run.clone());
        send_complete_tool_run(self.tx, self.turn_id, &tool_run.id, status);
        self.tool_runs.insert(tool_run.id.clone(), tool_run);
    }
}

#[derive(Default)]
struct ProviderMarkupSanitizer {
    carry: String,
    stripping_until: Option<&'static str>,
}

impl ProviderMarkupSanitizer {
    fn push(&mut self, chunk: &str) -> String {
        let mut input = std::mem::take(&mut self.carry);
        input.push_str(chunk);
        let mut output = String::new();

        loop {
            if let Some(closing_marker) = self.stripping_until {
                if let Some(position) = input.find(closing_marker) {
                    input = input[position + closing_marker.len()..].to_string();
                    self.stripping_until = None;
                    continue;
                }

                let keep = suffix_prefix_len(&input, &[closing_marker]);
                self.carry = tail_fragment(&input, keep);
                return output;
            }

            if let Some((position, closing_marker)) = find_markup_opening(&input) {
                output.push_str(&input[..position]);
                let remainder = &input[position..];
                if let Some(tag_end) = remainder.find('>') {
                    input = remainder[tag_end + 1..].to_string();
                    self.stripping_until = Some(closing_marker);
                    continue;
                }

                self.carry = remainder.to_string();
                return output;
            }

            let keep = suffix_prefix_len(&input, &["<minimax:tool_call", "<invoke"]);
            if keep == 0 {
                output.push_str(&input);
                self.carry.clear();
            } else {
                output.push_str(&input[..input.len() - keep]);
                self.carry = tail_fragment(&input, keep);
            }
            return output;
        }
    }

    fn finish(&mut self) -> String {
        if self.stripping_until.is_some() {
            self.carry.clear();
            self.stripping_until = None;
            String::new()
        } else {
            std::mem::take(&mut self.carry)
        }
    }
}

impl WorkflowTurnRunner<'_> {
    fn run(
        &self,
        agent: &mut Agent,
        session_context: &mut SessionContext,
    ) -> anyhow::Result<String> {
        self.update_active_workflow(
            session_context,
            self.scene_catalog.root_workflow_id.clone(),
            WorkflowRunRole::Root,
        );
        self.send_routing_log(format!(
            "Routing turn through root workflow '{}'.",
            self.scene_catalog.root_workflow_id
        ));

        self.run_workflow(
            agent,
            &self.scene_catalog.root_workflow_id,
            WorkflowRunRole::Root,
            session_context,
        )?;

        let selected_workflow_id = self.ensure_selected_workflow(session_context);
        self.send_routing_log(format!(
            "Delegating to child workflow '{}'.",
            selected_workflow_id
        ));

        self.run_workflow(
            agent,
            &selected_workflow_id,
            WorkflowRunRole::Child,
            session_context,
        )
    }

    fn run_workflow(
        &self,
        agent: &mut Agent,
        workflow_id: &str,
        role: WorkflowRunRole,
        session_context: &mut SessionContext,
    ) -> anyhow::Result<String> {
        self.update_active_workflow(session_context, workflow_id.to_string(), role);
        let (definition, prompts) = self.resolve_workflow_bundle(workflow_id)?;
        let mut last_text = String::new();
        let mut run = definition.start_run();

        loop {
            let Some(step_state) = run.current_step() else {
                break;
            };
            let Some(step) = run.current_step_definition().cloned() else {
                break;
            };
            let is_final_step = step_state.index == step_state.total;
            let diagnostic_context = StepDiagnosticContext {
                workflow_id,
                workflow_role: role,
                step: &step,
                index: step_state.index,
                total: step_state.total,
            };

            send_workflow_step(
                self.tx_result,
                self.turn_id,
                Some(step_state),
                workflow_id,
                role,
            );

            let step_prompt = prompts.prompt_for(&step.id).unwrap_or_default();
            let step_input =
                match self.build_step_execution_input(session_context, &step, step_prompt) {
                    Ok(step_input) => {
                        self.send_step_input_diagnostics(&diagnostic_context, &step_input);
                        step_input
                    }
                    Err(error) => {
                        self.send_step_input_error_diagnostics(
                            &diagnostic_context,
                            session_context,
                            &error.to_string(),
                        );
                        return Err(error);
                    }
                };
            agent.set_system(build_step_system_prompt(&step_input));

            let checkpoint = if role == WorkflowRunRole::Root {
                Some(agent.messages().to_vec())
            } else {
                None
            };
            let mut response_streamer = StepResponseStreamer::new(
                self.tx_result,
                self.turn_id,
                workflow_id,
                role,
                &step,
                is_final_step,
                session_context.routing.recognized_scene_id.as_deref(),
            );
            response_streamer.begin();
            let step_attempt_checkpoint = agent.messages().to_vec();
            let max_validation_retries = max_output_validation_retries(&step.output_contract);
            let mut validation_attempt = 0u32;
            let mut last_validation_error = None::<String>;
            let (stage_text, structured_output, validation_attempts) = loop {
                let stage_text = match self.execute_step(
                    agent,
                    &step,
                    &step_input.resolved_tools,
                    &mut response_streamer,
                ) {
                    Ok(stage_text) => stage_text,
                    Err(error) => {
                        response_streamer.fail();
                        return Err(error);
                    }
                };

                match self.validate_step_output(&step, &stage_text) {
                    Ok(structured_output) => {
                        response_streamer.complete();
                        break (
                            stage_text,
                            structured_output,
                            completed_output_attempts(&step.output_contract, validation_attempt),
                        );
                    }
                    Err(validation_error) => {
                        let validation_error_text = validation_error.to_string();
                        let attempts = validation_attempt + 1;
                        let can_retry = validation_attempt < max_validation_retries;
                        let retry_count = if can_retry {
                            validation_attempt + 1
                        } else {
                            validation_attempt
                        };
                        self.send_step_output_diagnostics(
                            &diagnostic_context,
                            &step_input,
                            OutputDiagnosticState {
                                status: StepOutputStatus::Invalid,
                                structured_output: None,
                                attempts,
                                retry_count,
                                max_retries: max_validation_retries,
                                error: Some(&validation_error_text),
                                session_writes: Vec::new(),
                            },
                        );
                        last_validation_error = Some(validation_error_text.clone());

                        if !can_retry {
                            if allows_root_routing_text_fallback(role, &step) {
                                send_warning_text(
                                    self.tx_result,
                                    self.turn_id,
                                    &format!(
                                        "Step '{}' failed structured output validation after {} attempt(s); falling back to text routing.",
                                        step.id, attempts
                                    ),
                                );
                                response_streamer.complete();
                                break (stage_text, None, attempts);
                            }

                            response_streamer.fail();
                            return Err(anyhow::anyhow!(
                                "step '{}' failed output validation after {} attempt(s): {}",
                                step.id,
                                attempts,
                                validation_error
                            ));
                        }

                        validation_attempt += 1;
                        send_warning_text(
                            self.tx_result,
                            self.turn_id,
                            &format!(
                                "Step '{}' produced invalid structured output. Retrying ({}/{}): {}",
                                step.id,
                                validation_attempt,
                                max_validation_retries,
                                validation_error
                            ),
                        );
                        agent.set_messages(step_attempt_checkpoint.clone());
                        let validation_feedback =
                            build_output_validation_feedback(&step, &validation_error.to_string());
                        agent.add_user_message(&validation_feedback);
                    }
                }
            };
            if let Some(checkpoint) = checkpoint {
                agent.set_messages(checkpoint);
            }

            let step_result = self.finalize_step(
                &step,
                is_final_step,
                stage_text,
                structured_output,
                session_context,
            )?;

            self.send_step_output_diagnostics(
                &diagnostic_context,
                &step_input,
                OutputDiagnosticState {
                    status: final_output_status(
                        &step.output_contract,
                        step_result.structured_output.as_ref(),
                    ),
                    structured_output: step_result.structured_output.as_ref(),
                    attempts: validation_attempts,
                    retry_count: validation_attempt,
                    max_retries: max_validation_retries,
                    error: last_validation_error.as_deref(),
                    session_writes: step_result.session_writes.clone(),
                },
            );

            if !step_result.summary.summary.is_empty() {
                session_context
                    .step_summaries
                    .push(step_result.summary.clone());
                info!(
                    workflow_id,
                    workflow_role = %role.as_str(),
                    step_id = %step.id,
                    summary_tokens = step_result.summary.estimated_tokens,
                    summary_preview = %preview_text(&step_result.summary.summary, 160),
                    total_step_summaries = session_context.step_summaries.len(),
                    "session context stored step summary"
                );
            }
            log_session_context_snapshot(
                workflow_id,
                role,
                &step.id,
                session_context,
                &step_result.session_writes,
            );

            if !step_result.final_text.is_empty() {
                if role == WorkflowRunRole::Child && !is_final_step {
                    send_step_text(
                        self.tx_result,
                        self.turn_id,
                        workflow_id,
                        role,
                        &step,
                        &step_result.final_text,
                    );
                }
                last_text = step_result.final_text.clone();
            }

            match step_result.transition {
                StepTransition::Continue => {
                    if run.advance().is_none() {
                        break;
                    }
                }
                StepTransition::StartWorkflow { .. } | StepTransition::FinishTurn => break,
                StepTransition::Error { message } => return Err(anyhow::anyhow!(message)),
            }
        }

        Ok(last_text)
    }

    fn execute_step(
        &self,
        agent: &mut Agent,
        step: &WorkflowStep,
        resolved_tools: &ResolvedToolSet,
        response_streamer: &mut StepResponseStreamer<'_>,
    ) -> anyhow::Result<String> {
        let tool_name_refs = resolved_tools.tool_name_refs();
        agent.set_visible_tools(Some(&tool_name_refs));
        agent.set_max_iterations(step.max_iterations);

        let tool_runs = Arc::new(Mutex::new(ToolRunTracker::new(
            self.tx_callback,
            self.turn_id,
            response_streamer.primary_section_id().to_string(),
        )));

        self.handle.block_on(agent.run_loop_with_events(
            {
                let tx_callback = self.tx_callback.clone();
                let turn_id = self.turn_id;
                let tool_runs = tool_runs.clone();
                move |tool_use_id, name, tool_input, output| {
                    let command = if name == "bash" {
                        tool_input
                            .get("command")
                            .and_then(|value| value.as_str())
                            .map(ToOwned::to_owned)
                    } else {
                        None
                    };

                    send_tool_call_preview(
                        &tx_callback,
                        turn_id,
                        name,
                        command,
                        preview_text(output, 100),
                    );

                    tool_runs.lock().unwrap().complete_tool_run(
                        tool_use_id,
                        name,
                        tool_input,
                        output,
                    );

                    if name == "todo" && !output.starts_with("Error:") {
                        send_todo_snapshot(&tx_callback, turn_id, output);
                    }
                }
            },
            {
                let tool_runs = tool_runs.clone();
                move |event| {
                    tool_runs.lock().unwrap().observe_chat_event(event);
                    response_streamer.push_chat_event(event);
                }
            },
        ))
    }

    fn resolve_workflow_bundle(
        &self,
        workflow_id: &str,
    ) -> anyhow::Result<(&omega_workflow::WorkflowDefinition, &WorkflowPrompts)> {
        let definition = self
            .workflow_catalog
            .workflow(workflow_id)
            .ok_or_else(|| anyhow::anyhow!("missing workflow '{}' in catalog", workflow_id))?;
        let prompts = self
            .prompt_catalog
            .prompts_for_workflow(workflow_id)
            .ok_or_else(|| anyhow::anyhow!("missing workflow prompt set for '{}'", workflow_id))?;
        Ok((definition, prompts))
    }

    fn build_step_execution_input(
        &self,
        session_context: &SessionContext,
        step: &WorkflowStep,
        step_prompt: &str,
    ) -> anyhow::Result<StepExecutionInput> {
        let resolved_tools = self.tool_catalog.resolve_for_step(&step.tool_request);
        let resolved_skills = self
            .skill_catalog
            .resolve_for_step(self.input, &step.skill_request);
        let step_summaries = self.select_step_summaries(
            session_context,
            step,
            step_prompt,
            &resolved_skills,
            &resolved_tools,
        );
        let structured_input = resolve_structured_input(session_context, step)?;
        let todo_snapshot = self.todo_snapshot_for_step(session_context, step);

        Ok(StepExecutionInput {
            base_system: self.base_system.to_string(),
            resolved_tools,
            resolved_skills,
            session_context: SessionContext {
                latest_user_turn: session_context.latest_user_turn.clone(),
                routing: session_context.routing.clone(),
                step_summaries,
                step_outputs: session_context.step_outputs.clone(),
            },
            structured_input,
            todo_snapshot,
            step: step.clone(),
            step_prompt: step_prompt.to_string(),
        })
    }

    fn select_step_summaries(
        &self,
        session_context: &SessionContext,
        step: &WorkflowStep,
        step_prompt: &str,
        resolved_skills: &ResolvedSkillSet,
        resolved_tools: &ResolvedToolSet,
    ) -> Vec<StepSummary> {
        let summary_budget = self
            .context_window
            .saturating_sub(self.max_output_tokens)
            .saturating_sub(CONTEXT_SAFETY_MARGIN_TOKENS);
        let fixed_tokens = estimate_tokens(&resolved_skills.build_system_prompt(self.base_system))
            .saturating_add(estimate_tokens(&step.label))
            .saturating_add(estimate_tokens(step_prompt))
            .saturating_add(estimate_tokens(&session_context.latest_user_turn))
            .saturating_add(estimate_tokens(&render_routing_context(
                &session_context.routing,
            )))
            .saturating_add(estimate_tokens(&render_visible_tools(
                resolved_tools.tool_names(),
            )))
            .saturating_add(estimate_tokens(&render_output_contract(
                &step.output_contract,
            )));

        let fixed_tokens = match resolve_structured_input(session_context, step) {
            Ok(Some(structured_input)) => fixed_tokens
                .saturating_add(estimate_tokens(&render_structured_input(&structured_input))),
            Ok(None) | Err(_) => fixed_tokens,
        };

        let fixed_tokens = match self.todo_snapshot_for_step(session_context, step) {
            Some(todo_snapshot) => fixed_tokens.saturating_add(estimate_tokens(&todo_snapshot)),
            None => fixed_tokens,
        };

        let mut remaining = summary_budget.saturating_sub(fixed_tokens);
        let mut selected = Vec::new();
        for summary in session_context.step_summaries.iter().rev() {
            if selected.is_empty() {
                remaining = remaining.saturating_sub(summary.estimated_tokens);
                selected.push(summary.clone());
                continue;
            }

            if summary.estimated_tokens <= remaining {
                remaining = remaining.saturating_sub(summary.estimated_tokens);
                selected.push(summary.clone());
            }
        }

        selected.reverse();
        debug!(
            step_id = %step.id,
            workflow_id = %session_context.routing.active_workflow_id,
            workflow_role = %session_context.routing.active_workflow_role.as_str(),
            summary_budget_tokens = summary_budget,
            fixed_tokens,
            selected_summary_count = selected.len(),
            selected_summary_tokens = selected.iter().map(|summary| summary.estimated_tokens).sum::<u32>(),
            total_available_summaries = session_context.step_summaries.len(),
            "step context summary budget resolved"
        );
        selected
    }

    fn todo_snapshot_for_step(
        &self,
        session_context: &SessionContext,
        step: &WorkflowStep,
    ) -> Option<String> {
        if !matches!(step.id.as_str(), EXECUTE_STEP_ID | REPORT_STEP_ID) {
            return None;
        }
        if !session_context.step_outputs.contains_key(PLAN_STEP_ID) {
            return None;
        }

        let manager = self.todo_manager.lock().ok()?;
        (!manager.items().is_empty()).then(|| manager.render())
    }

    fn send_step_diagnostics_effect(&self, diagnostics: StepDiagnostics) {
        let _ = self.tx_result.send(RuntimeUiEnvelope::effect(
            self.turn_id,
            RuntimeUiEffect::UpsertStepDiagnostics {
                diagnostics: Box::new(diagnostics),
            },
        ));
    }

    fn send_step_input_diagnostics(
        &self,
        context: &StepDiagnosticContext<'_>,
        step_input: &StepExecutionInput,
    ) {
        let output = build_step_output_diagnostics(
            &context.step.output_contract,
            pending_output_status_for_contract(&context.step.output_contract),
            None,
            0,
            0,
            max_output_validation_retries(&context.step.output_contract),
            None,
        );
        self.send_step_diagnostics_effect(build_step_diagnostics(
            context,
            build_step_input_diagnostics(step_input),
            output,
            Vec::new(),
        ));
        debug!(
            workflow_id = %context.workflow_id,
            workflow_role = %context.workflow_role.as_str(),
            step_id = %context.step.id,
            summary_sources = step_input.session_context.step_summaries.len(),
            structured_sources = step_input
                .structured_input
                .as_ref()
                .and_then(|value| value.as_object())
                .map(|value| value.len())
                .unwrap_or(0),
            structured_input_preview = step_input
                .structured_input
                .as_ref()
                .map(|value| preview_json_value(value, 160))
                .unwrap_or_default(),
            todo_injected = step_input.todo_snapshot.is_some(),
            "step input diagnostics updated"
        );
    }

    fn send_step_input_error_diagnostics(
        &self,
        context: &StepDiagnosticContext<'_>,
        session_context: &SessionContext,
        error: &str,
    ) {
        let output = build_step_output_diagnostics(
            &context.step.output_contract,
            pending_output_status_for_contract(&context.step.output_contract),
            None,
            0,
            0,
            max_output_validation_retries(&context.step.output_contract),
            None,
        );
        self.send_step_diagnostics_effect(build_step_diagnostics(
            context,
            build_failed_step_input_diagnostics(session_context, context.step, error),
            output,
            Vec::new(),
        ));
        error!(
            workflow_id = %context.workflow_id,
            workflow_role = %context.workflow_role.as_str(),
            step_id = %context.step.id,
            reason = %error,
            "step input diagnostics failed"
        );
    }

    fn send_step_output_diagnostics(
        &self,
        context: &StepDiagnosticContext<'_>,
        step_input: &StepExecutionInput,
        output_state: OutputDiagnosticState<'_>,
    ) {
        let diagnostics = build_step_diagnostics(
            context,
            build_step_input_diagnostics(step_input),
            build_step_output_diagnostics(
                &context.step.output_contract,
                output_state.status,
                output_state.structured_output,
                output_state.attempts,
                output_state.retry_count,
                output_state.max_retries,
                output_state.error,
            ),
            output_state.session_writes,
        );
        let extracted_preview = diagnostics
            .output
            .extracted_preview
            .clone()
            .unwrap_or_default();
        let session_write_count = diagnostics.session_writes.len();
        let session_writes = format_step_context_writes(&diagnostics.session_writes);
        self.send_step_diagnostics_effect(diagnostics);
        debug!(
            workflow_id = %context.workflow_id,
            workflow_role = %context.workflow_role.as_str(),
            step_id = %context.step.id,
            validation_status = ?output_state.status,
            attempts = output_state.attempts,
            retry_count = output_state.retry_count,
            error = output_state.error.unwrap_or(""),
            extracted_preview,
            session_write_count,
            session_writes = %session_writes,
            "step output diagnostics updated"
        );
    }

    fn finalize_step(
        &self,
        step: &WorkflowStep,
        is_final_step: bool,
        final_text: String,
        structured_output: Option<Value>,
        session_context: &mut SessionContext,
    ) -> anyhow::Result<StepExecutionResult> {
        let workflow_id = session_context.routing.active_workflow_id.clone();
        let role = session_context.routing.active_workflow_role;
        let mut session_writes = Vec::new();
        let mut transition = if role == WorkflowRunRole::Child && is_final_step {
            StepTransition::FinishTurn
        } else {
            StepTransition::Continue
        };

        if let Some(output) = structured_output.as_ref() {
            let previous_output = session_context.step_outputs.get(&step.id);
            if let Some(write) = build_context_write(
                format!("step_outputs.{}", step.id),
                previous_output.map(|value| preview_json_value(value, 160)),
                Some(preview_json_value(output, 160)),
            ) {
                session_writes.push(write);
            }
            session_context
                .step_outputs
                .insert(step.id.clone(), output.clone());
            info!(
                workflow_id = %workflow_id,
                workflow_role = %role.as_str(),
                step_id = %step.id,
                write_path = %format!("step_outputs.{}", step.id),
                preview = %preview_json_value(output, 160),
                total_step_outputs = session_context.step_outputs.len(),
                "session context stored structured step output"
            );
            session_writes.extend(self.sync_todo_state_from_step(&workflow_id, step, output)?);
        }

        let summary_text = match (role, step.id.as_str()) {
            (WorkflowRunRole::Root, SCENE_RECOGNITION_STEP_ID) => {
                let scene_id =
                    self.resolve_scene_from_output(structured_output.as_ref(), &final_text);
                session_context.routing.recognized_scene_id = Some(scene_id.clone());
                session_context.routing.selected_workflow_id = None;
                self.send_session_status(session_context);
                self.send_routing_log(format!(
                    "Recognized scene '{}' via workflow '{}'.",
                    scene_id, workflow_id
                ));
                format!("Recognized scene: {scene_id}.")
            }
            (WorkflowRunRole::Root, SELECT_WORKFLOW_STEP_ID) => {
                let scene_id = session_context
                    .routing
                    .recognized_scene_id
                    .clone()
                    .unwrap_or_else(|| self.scene_catalog.default_scene_id.clone());
                let selected_workflow_id = self.resolve_workflow_from_output(
                    structured_output.as_ref(),
                    &final_text,
                    &scene_id,
                );
                session_context.routing.selected_workflow_id = Some(selected_workflow_id.clone());
                self.send_session_status(session_context);
                self.send_routing_log(format!(
                    "Selected workflow '{}' for scene '{}'.",
                    selected_workflow_id, scene_id
                ));
                transition = StepTransition::StartWorkflow {
                    workflow_id: selected_workflow_id.clone(),
                };
                format!("Selected workflow: {selected_workflow_id}.")
            }
            _ => summarize_step_text(&final_text),
        };

        Ok(StepExecutionResult {
            final_text,
            structured_output,
            summary: StepSummary {
                workflow_id: workflow_id.to_string(),
                step_id: step.id.clone(),
                title: step.label.clone(),
                estimated_tokens: estimate_tokens(&summary_text),
                summary: summary_text,
            },
            session_writes,
            transition,
        })
    }

    fn sync_todo_state_from_step(
        &self,
        workflow_id: &str,
        step: &WorkflowStep,
        structured_output: &Value,
    ) -> anyhow::Result<Vec<StepContextWrite>> {
        if workflow_id != FEATURE_WORKFLOW_ID {
            return Ok(Vec::new());
        }

        match step.id.as_str() {
            PLAN_STEP_ID => self.sync_todo_manager_from_plan_output(structured_output),
            EXECUTE_STEP_ID => self.sync_todo_manager_from_execute_output(structured_output),
            _ => Ok(Vec::new()),
        }
    }

    fn sync_todo_manager_from_plan_output(
        &self,
        structured_output: &Value,
    ) -> anyhow::Result<Vec<StepContextWrite>> {
        let plan = parse_feature_plan_output(structured_output.clone())?;
        let items = plan
            .tasks
            .iter()
            .enumerate()
            .map(|(index, task)| TodoItem {
                id: Some(task.id.clone()),
                text: format!("{}: {}", task.title.trim(), task.description.trim()),
                status: if index == 0 {
                    TodoStatus::InProgress
                } else {
                    TodoStatus::Pending
                },
                active_form: (index == 0).then(|| format!("working on {}", task.title.trim())),
            })
            .collect::<Vec<_>>();
        let mut manager = self
            .todo_manager
            .lock()
            .map_err(|_| anyhow::anyhow!("todo manager lock poisoned"))?;
        let had_items = !manager.items().is_empty();
        let before_rendered = manager.render();
        let rendered = manager.update(items)?;
        let writes = build_text_context_write(
            "todo.rendered",
            had_items.then_some(before_rendered.as_str()),
            (!manager.items().is_empty()).then_some(rendered.as_str()),
        )
        .into_iter()
        .collect::<Vec<_>>();
        drop(manager);
        send_todo_snapshot(self.tx_result, self.turn_id, &rendered);
        Ok(writes)
    }

    fn sync_todo_manager_from_execute_output(
        &self,
        structured_output: &Value,
    ) -> anyhow::Result<Vec<StepContextWrite>> {
        let execute = parse_feature_execute_output(structured_output.clone())?;
        let mut manager = self
            .todo_manager
            .lock()
            .map_err(|_| anyhow::anyhow!("todo manager lock poisoned"))?;
        if manager.items().is_empty() {
            return Ok(Vec::new());
        }
        let had_items = !manager.items().is_empty();
        let before_rendered = manager.render();

        let completed = execute
            .completed_tasks
            .iter()
            .map(String::as_str)
            .collect::<std::collections::BTreeSet<_>>();
        let open = execute
            .open_tasks
            .iter()
            .map(String::as_str)
            .collect::<std::collections::BTreeSet<_>>();
        let mut promoted_open = false;
        let updated_items = manager
            .items()
            .iter()
            .cloned()
            .map(|mut item| {
                let item_id = item.id.as_deref().unwrap_or_default();
                if completed.contains(item_id) {
                    item.status = TodoStatus::Completed;
                    item.active_form = None;
                } else if open.contains(item_id) {
                    item.status = if !promoted_open {
                        promoted_open = true;
                        TodoStatus::InProgress
                    } else {
                        TodoStatus::Pending
                    };
                    item.active_form = (item.status == TodoStatus::InProgress)
                        .then(|| format!("working on {}", item.text));
                }
                item
            })
            .collect::<Vec<_>>();

        let rendered = manager.update(updated_items)?;
        let writes = build_text_context_write(
            "todo.rendered",
            had_items.then_some(before_rendered.as_str()),
            (!manager.items().is_empty()).then_some(rendered.as_str()),
        )
        .into_iter()
        .collect::<Vec<_>>();
        drop(manager);
        send_todo_snapshot(self.tx_result, self.turn_id, &rendered);
        Ok(writes)
    }

    fn validate_step_output(
        &self,
        step: &WorkflowStep,
        final_text: &str,
    ) -> anyhow::Result<Option<Value>> {
        match &step.output_contract {
            StepOutputContract::None => Ok(None),
            StepOutputContract::Required {
                format,
                schema_path,
                ..
            } => {
                let value = parse_structured_output(*format, final_text).ok_or_else(|| {
                    anyhow::anyhow!(
                        "expected {} output but response was not valid {}",
                        format.as_str(),
                        format.as_str()
                    )
                })?;
                if let Some(schema_path) = schema_path {
                    validate_schema_file(self.cwd, schema_path, &value)?;
                }
                validate_feature_step_output(step, &value)?;
                Ok(Some(value))
            }
            StepOutputContract::Optional {
                format,
                schema_path,
            } => {
                let Some(value) = parse_structured_output(*format, final_text) else {
                    return Ok(None);
                };
                if let Some(schema_path) = schema_path {
                    if validate_schema_file(self.cwd, schema_path, &value).is_err() {
                        return Ok(None);
                    }
                }
                if validate_feature_step_output(step, &value).is_err() {
                    return Ok(None);
                }
                Ok(Some(value))
            }
        }
    }

    fn resolve_scene_from_output(
        &self,
        structured_output: Option<&Value>,
        stage_text: &str,
    ) -> String {
        if let Some(scene_id) =
            parse_structured_id_from_value(structured_output, &["recognized_scene_id", "scene_id"])
                .or_else(|| parse_structured_id(stage_text, &["recognized_scene_id", "scene_id"]))
        {
            if self.scene_catalog.scene(&scene_id).is_some() {
                return scene_id;
            }
        }

        match find_catalog_match(
            stage_text,
            self.scene_catalog
                .scenes
                .iter()
                .map(|scene| scene.id.as_str()),
        ) {
            Some(scene_id) => scene_id,
            None => {
                let fallback = self.scene_catalog.default_scene_id.clone();
                send_warning_text(
                    self.tx_result,
                    self.turn_id,
                    &format!(
                        "Scene recognition did not resolve a configured scene; defaulting to '{}'.",
                        fallback
                    ),
                );
                fallback
            }
        }
    }

    fn resolve_workflow_from_output(
        &self,
        structured_output: Option<&Value>,
        stage_text: &str,
        scene_id: &str,
    ) -> String {
        let mapped_workflow = self
            .scene_catalog
            .scene(scene_id)
            .map(|scene| scene.workflow_id.clone())
            .unwrap_or_else(|| FEATURE_WORKFLOW_ID.to_string());

        if let Some(workflow_id) = parse_structured_id_from_value(
            structured_output,
            &["selected_workflow_id", "workflow_id"],
        )
        .or_else(|| parse_structured_id(stage_text, &["selected_workflow_id", "workflow_id"]))
        {
            if workflow_id != self.scene_catalog.root_workflow_id
                && self.workflow_catalog.workflow(&workflow_id).is_some()
            {
                return workflow_id;
            }
        }

        match find_catalog_match(stage_text, self.workflow_catalog.workflow_ids()) {
            Some(workflow_id)
                if workflow_id != self.scene_catalog.root_workflow_id
                    && self.workflow_catalog.workflow(&workflow_id).is_some() =>
            {
                workflow_id
            }
            _ => mapped_workflow,
        }
    }

    fn ensure_selected_workflow(&self, session_context: &mut SessionContext) -> String {
        if session_context.routing.recognized_scene_id.is_none() {
            session_context.routing.recognized_scene_id =
                Some(self.scene_catalog.default_scene_id.clone());
            self.send_session_status(session_context);
        }

        if session_context.routing.selected_workflow_id.is_none() {
            let scene_id = session_context
                .routing
                .recognized_scene_id
                .clone()
                .unwrap_or_else(|| self.scene_catalog.default_scene_id.clone());
            let workflow_id = self
                .scene_catalog
                .scene(&scene_id)
                .map(|scene| scene.workflow_id.clone())
                .unwrap_or_else(|| FEATURE_WORKFLOW_ID.to_string());
            session_context.routing.selected_workflow_id = Some(workflow_id.clone());
            self.send_session_status(session_context);
            self.send_routing_log(format!(
                "Workflow selection fell back to '{}' for scene '{}'.",
                workflow_id, scene_id
            ));
        }

        let selected_workflow_id = session_context
            .routing
            .selected_workflow_id
            .clone()
            .unwrap_or_else(|| FEATURE_WORKFLOW_ID.to_string());
        if selected_workflow_id == self.scene_catalog.root_workflow_id {
            let fallback = self
                .scene_catalog
                .scene(
                    session_context
                        .routing
                        .recognized_scene_id
                        .as_deref()
                        .unwrap_or(&self.scene_catalog.default_scene_id),
                )
                .map(|scene| scene.workflow_id.clone())
                .unwrap_or_else(|| FEATURE_WORKFLOW_ID.to_string());
            session_context.routing.selected_workflow_id = Some(fallback.clone());
            self.send_session_status(session_context);
            self.send_routing_log(format!(
                "Ignoring root workflow as child target; using '{}' instead.",
                fallback
            ));
            return fallback;
        }

        selected_workflow_id
    }

    fn update_active_workflow(
        &self,
        session_context: &mut SessionContext,
        workflow_id: String,
        role: WorkflowRunRole,
    ) {
        session_context.routing.active_workflow_id = workflow_id;
        session_context.routing.active_workflow_role = role;
        self.send_session_status(session_context);
    }

    fn send_session_status(&self, session_context: &SessionContext) {
        let _ = self.tx_result.send(RuntimeUiEnvelope::effect(
            self.turn_id,
            RuntimeUiEffect::SetStatusSlot {
                slot: StatusSlot::Session,
                value: StatusValue::SessionRouting {
                    root_workflow_id: self.scene_catalog.root_workflow_id.clone(),
                    active_workflow_id: session_context.routing.active_workflow_id.clone(),
                    active_workflow_role: session_context.routing.active_workflow_role,
                    recognized_scene_id: session_context.routing.recognized_scene_id.clone(),
                    selected_workflow_id: session_context.routing.selected_workflow_id.clone(),
                },
            },
        ));
    }

    fn send_routing_log(&self, text: String) {
        let _ = self.tx_result.send(RuntimeUiEnvelope::message(
            self.turn_id,
            RuntimeUiMessage {
                target: UiTarget::Activity(ActivityTarget::Log),
                source: UiSource::SessionRouting,
                kind: UiMessageKind::Summary,
                content: UiContent::Text(text),
                priority: None,
            },
        ));
    }
}

fn allows_root_routing_text_fallback(role: WorkflowRunRole, step: &WorkflowStep) -> bool {
    role == WorkflowRunRole::Root
        && matches!(
            step.id.as_str(),
            SCENE_RECOGNITION_STEP_ID | SELECT_WORKFLOW_STEP_ID
        )
}
fn render_routing_context(routing: &RoutingContext) -> String {
    let mut lines = vec![
        format!("Workflow role: {}", routing.active_workflow_role.as_str()),
        format!("Active workflow: {}", routing.active_workflow_id),
    ];
    if let Some(scene_id) = routing.recognized_scene_id.as_deref() {
        lines.push(format!("Recognized scene: {scene_id}"));
    }
    if let Some(selected_workflow_id) = routing.selected_workflow_id.as_deref() {
        lines.push(format!("Selected workflow: {selected_workflow_id}"));
    }
    lines.join("\n")
}

fn render_visible_tools(tool_names: &[String]) -> String {
    if tool_names.is_empty() {
        "Visible tools: none".to_string()
    } else {
        format!("Visible tools: {}", tool_names.join(", "))
    }
}

fn render_step_summaries(step_summaries: &[StepSummary]) -> String {
    step_summaries
        .iter()
        .map(|summary| {
            format!(
                "- [{}:{}] {}\n{}",
                summary.workflow_id, summary.step_id, summary.title, summary.summary
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn render_session_context(session_context: &SessionContext) -> String {
    let mut sections = Vec::new();
    if !session_context.latest_user_turn.trim().is_empty() {
        sections.push(format!(
            "<latest_user_turn>\n{}\n</latest_user_turn>",
            session_context.latest_user_turn.trim_end()
        ));
    }

    let routing_context = render_routing_context(&session_context.routing);
    if !routing_context.trim().is_empty() {
        sections.push(format!(
            "<workflow_runtime>\n{}\n</workflow_runtime>",
            routing_context.trim_end()
        ));
    }

    if !session_context.step_summaries.is_empty() {
        sections.push(format!(
            "<step_summaries>\n{}\n</step_summaries>",
            render_step_summaries(&session_context.step_summaries)
        ));
    }

    sections.join("\n\n")
}

fn render_structured_input(structured_input: &Value) -> String {
    serde_json::to_string_pretty(structured_input).unwrap_or_else(|_| structured_input.to_string())
}

fn render_output_contract(output_contract: &StepOutputContract) -> String {
    match output_contract {
        StepOutputContract::None => String::new(),
        StepOutputContract::Required {
            format,
            schema_path,
            max_retries,
        } => {
            let mut lines = vec![
                "mode: required".to_string(),
                format!("format: {}", format.as_str()),
                format!("max_retries: {}", max_retries),
            ];
            lines.extend(render_output_format_rules(*format));
            if let Some(schema_path) = schema_path {
                lines.push(format!("schema_path: {}", schema_path.display()));
            }
            lines.join("\n")
        }
        StepOutputContract::Optional {
            format,
            schema_path,
        } => {
            let mut lines = vec![
                "mode: optional".to_string(),
                format!("format: {}", format.as_str()),
            ];
            lines.extend(render_output_format_rules(*format));
            if let Some(schema_path) = schema_path {
                lines.push(format!("schema_path: {}", schema_path.display()));
            }
            lines.join("\n")
        }
    }
}

fn render_output_format_rules(format: DataFormat) -> Vec<String> {
    match format {
        DataFormat::Json => vec![
            "response_rules: return exactly one valid JSON value".to_string(),
            "response_rules: do not add prose before or after the JSON".to_string(),
            "response_rules: do not wrap the JSON in markdown fences".to_string(),
        ],
    }
}

fn resolve_structured_input(
    session_context: &SessionContext,
    step: &WorkflowStep,
) -> anyhow::Result<Option<Value>> {
    match &step.input_contract {
        StepInputContract::None => Ok(None),
        StepInputContract::Required { sources } => {
            let missing = sources
                .iter()
                .filter(|source| !session_context.step_outputs.contains_key(source.as_str()))
                .cloned()
                .collect::<Vec<_>>();
            if !missing.is_empty() {
                return Err(anyhow::anyhow!(
                    "step '{}' requires structured input from missing source(s): {}",
                    step.id,
                    missing.join(", ")
                ));
            }
            Ok(Some(build_structured_input_payload(
                &session_context.step_outputs,
                sources,
            )))
        }
        StepInputContract::Optional { sources } => {
            let payload = build_structured_input_payload(&session_context.step_outputs, sources);
            if payload.as_object().is_some_and(|object| !object.is_empty()) {
                Ok(Some(payload))
            } else {
                Ok(None)
            }
        }
    }
}

fn build_structured_input_payload(
    step_outputs: &BTreeMap<String, Value>,
    sources: &[String],
) -> Value {
    let mut payload = serde_json::Map::new();
    for source in sources {
        if let Some(value) = step_outputs.get(source) {
            payload.insert(source.clone(), value.clone());
        }
    }
    Value::Object(payload)
}

fn max_output_validation_retries(output_contract: &StepOutputContract) -> u32 {
    match output_contract {
        StepOutputContract::Required { max_retries, .. } => *max_retries,
        StepOutputContract::None | StepOutputContract::Optional { .. } => 0,
    }
}

fn pending_output_status_for_contract(output_contract: &StepOutputContract) -> StepOutputStatus {
    match output_contract {
        StepOutputContract::None => StepOutputStatus::None,
        StepOutputContract::Required { .. } | StepOutputContract::Optional { .. } => {
            StepOutputStatus::Pending
        }
    }
}

fn final_output_status(
    output_contract: &StepOutputContract,
    structured_output: Option<&Value>,
) -> StepOutputStatus {
    match output_contract {
        StepOutputContract::None => StepOutputStatus::None,
        StepOutputContract::Required { .. } | StepOutputContract::Optional { .. }
            if structured_output.is_some() =>
        {
            StepOutputStatus::Valid
        }
        StepOutputContract::Optional { .. } => StepOutputStatus::Skipped,
        StepOutputContract::Required { .. } => StepOutputStatus::Invalid,
    }
}

fn completed_output_attempts(output_contract: &StepOutputContract, retry_count: u32) -> u32 {
    match output_contract {
        StepOutputContract::None => 0,
        StepOutputContract::Required { .. } | StepOutputContract::Optional { .. } => {
            retry_count + 1
        }
    }
}

fn build_step_diagnostics(
    context: &StepDiagnosticContext<'_>,
    input: StepInputDiagnostics,
    output: StepOutputDiagnostics,
    session_writes: Vec<StepContextWrite>,
) -> StepDiagnostics {
    StepDiagnostics {
        id: format!(
            "{}:{}:{}",
            context.workflow_role.as_str(),
            context.workflow_id,
            context.step.id
        ),
        workflow_id: context.workflow_id.to_string(),
        workflow_role: context.workflow_role,
        step_id: context.step.id.clone(),
        step_label: context.step.label.clone(),
        index: context.index,
        total: context.total,
        input,
        output,
        session_writes,
    }
}

fn build_step_input_diagnostics(step_input: &StepExecutionInput) -> StepInputDiagnostics {
    let (expected_structured_sources, missing_structured_sources) =
        expected_and_missing_sources(&step_input.step, &step_input.session_context.step_outputs);
    let resolved_structured_sources = step_input
        .structured_input
        .as_ref()
        .and_then(|value| value.as_object())
        .map(|value| value.keys().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    let status = match &step_input.step.input_contract {
        StepInputContract::None => StepInputStatus::None,
        StepInputContract::Required { .. } => StepInputStatus::Ready,
        StepInputContract::Optional { .. } if resolved_structured_sources.is_empty() => {
            StepInputStatus::OptionalEmpty
        }
        StepInputContract::Optional { .. } => StepInputStatus::Ready,
    };

    StepInputDiagnostics {
        status,
        summary_sources: step_input
            .session_context
            .step_summaries
            .iter()
            .map(|summary| StepSummarySource {
                workflow_id: summary.workflow_id.clone(),
                step_id: summary.step_id.clone(),
                title: summary.title.clone(),
            })
            .collect(),
        expected_structured_sources,
        resolved_structured_sources,
        missing_structured_sources,
        structured_input_preview: step_input
            .structured_input
            .as_ref()
            .map(|value| preview_json_value(value, 160)),
        todo_state_preview: step_input
            .todo_snapshot
            .as_deref()
            .map(|text| preview_text(text, 160)),
        error: None,
    }
}

fn build_failed_step_input_diagnostics(
    session_context: &SessionContext,
    step: &WorkflowStep,
    error: &str,
) -> StepInputDiagnostics {
    let (expected_structured_sources, missing_structured_sources) =
        expected_and_missing_sources(step, &session_context.step_outputs);
    StepInputDiagnostics {
        status: StepInputStatus::MissingRequired,
        summary_sources: Vec::new(),
        expected_structured_sources,
        resolved_structured_sources: Vec::new(),
        missing_structured_sources,
        structured_input_preview: None,
        todo_state_preview: None,
        error: Some(error.to_string()),
    }
}

fn expected_and_missing_sources(
    step: &WorkflowStep,
    step_outputs: &BTreeMap<String, Value>,
) -> (Vec<String>, Vec<String>) {
    let expected_structured_sources = match &step.input_contract {
        StepInputContract::None => Vec::new(),
        StepInputContract::Required { sources } | StepInputContract::Optional { sources } => {
            sources.clone()
        }
    };
    let missing_structured_sources = expected_structured_sources
        .iter()
        .filter(|source| !step_outputs.contains_key(source.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    (expected_structured_sources, missing_structured_sources)
}

fn build_step_output_diagnostics(
    output_contract: &StepOutputContract,
    status: StepOutputStatus,
    structured_output: Option<&Value>,
    attempts: u32,
    retry_count: u32,
    max_retries: u32,
    error: Option<&str>,
) -> StepOutputDiagnostics {
    let (contract_mode, format, schema_path) = match output_contract {
        StepOutputContract::None => (StepOutputContractMode::None, None, None),
        StepOutputContract::Required {
            format,
            schema_path,
            ..
        } => (
            StepOutputContractMode::Required,
            Some(format.as_str().to_string()),
            schema_path.as_ref().map(|path| path.display().to_string()),
        ),
        StepOutputContract::Optional {
            format,
            schema_path,
        } => (
            StepOutputContractMode::Optional,
            Some(format.as_str().to_string()),
            schema_path.as_ref().map(|path| path.display().to_string()),
        ),
    };

    StepOutputDiagnostics {
        contract_mode,
        format,
        schema_path,
        status,
        extracted_preview: structured_output.map(|value| preview_json_value(value, 160)),
        attempts,
        retry_count,
        max_retries,
        error: error.map(ToOwned::to_owned),
    }
}

fn build_text_context_write(
    path: impl Into<String>,
    before: Option<&str>,
    after: Option<&str>,
) -> Option<StepContextWrite> {
    build_context_write(
        path,
        before.map(|value| preview_text(value, 160)),
        after.map(|value| preview_text(value, 160)),
    )
}

fn build_context_write(
    path: impl Into<String>,
    before_preview: Option<String>,
    after_preview: Option<String>,
) -> Option<StepContextWrite> {
    let before_preview = normalize_context_preview(before_preview);
    let after_preview = normalize_context_preview(after_preview);

    if before_preview == after_preview {
        return None;
    }

    let kind = match (before_preview.as_ref(), after_preview.as_ref()) {
        (None, Some(_)) => StepContextWriteKind::Added,
        (Some(_), None) => StepContextWriteKind::Cleared,
        (Some(_), Some(_)) => StepContextWriteKind::Updated,
        (None, None) => return None,
    };

    Some(StepContextWrite {
        path: path.into(),
        kind,
        before_preview,
        after_preview,
    })
}

fn normalize_context_preview(preview: Option<String>) -> Option<String> {
    preview.filter(|value| !value.trim().is_empty())
}

fn format_step_context_writes(writes: &[StepContextWrite]) -> String {
    if writes.is_empty() {
        return "none".to_string();
    }

    writes
        .iter()
        .map(|write| {
            let change = match write.kind {
                StepContextWriteKind::Added => "added",
                StepContextWriteKind::Updated => "updated",
                StepContextWriteKind::Cleared => "cleared",
            };
            format!("{} ({})", write.path, change)
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn log_session_context_snapshot(
    workflow_id: &str,
    workflow_role: WorkflowRunRole,
    step_id: &str,
    session_context: &SessionContext,
    session_writes: &[StepContextWrite],
) {
    let step_output_keys = session_context
        .step_outputs
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    info!(
        workflow_id,
        workflow_role = %workflow_role.as_str(),
        step_id,
        recognized_scene_id = %session_context.routing.recognized_scene_id.as_deref().unwrap_or("-"),
        selected_workflow_id = %session_context.routing.selected_workflow_id.as_deref().unwrap_or("-"),
        active_workflow_id = %session_context.routing.active_workflow_id,
        total_step_summaries = session_context.step_summaries.len(),
        step_output_keys = ?step_output_keys,
        session_writes = %format_step_context_writes(session_writes),
        "session context snapshot updated"
    );
}

#[cfg(test)]
fn validate_structured_output(
    output_contract: &StepOutputContract,
    final_text: &str,
) -> anyhow::Result<Option<Value>> {
    match output_contract {
        StepOutputContract::None => Ok(None),
        StepOutputContract::Required { format, .. } => parse_structured_output(*format, final_text)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "expected {} output but response was not valid {}",
                    format.as_str(),
                    format.as_str()
                )
            })
            .map(Some),
        StepOutputContract::Optional { format, .. } => {
            Ok(parse_structured_output(*format, final_text))
        }
    }
}

fn parse_structured_output(format: DataFormat, final_text: &str) -> Option<Value> {
    match format {
        DataFormat::Json => parse_json_value(final_text),
    }
}

fn validate_schema_file(
    root: &std::path::Path,
    schema_path: &std::path::Path,
    value: &Value,
) -> anyhow::Result<()> {
    let path = if schema_path.is_absolute() {
        schema_path.to_path_buf()
    } else {
        root.join(schema_path)
    };
    let raw = std::fs::read_to_string(&path)
        .map_err(|error| anyhow::anyhow!("failed to read schema {}: {error}", path.display()))?;
    let schema = serde_json::from_str::<Value>(&raw)
        .map_err(|error| anyhow::anyhow!("failed to parse schema {}: {error}", path.display()))?;
    validate_schema_value(&schema, value, "$", &path)
}

fn validate_schema_value(
    schema: &Value,
    value: &Value,
    location: &str,
    schema_path: &std::path::Path,
) -> anyhow::Result<()> {
    match schema.get("type").and_then(|value| value.as_str()) {
        Some("object") => {
            let object = value.as_object().ok_or_else(|| {
                anyhow::anyhow!(
                    "schema {} expected object at {}",
                    schema_path.display(),
                    location
                )
            })?;
            if let Some(required) = schema.get("required").and_then(|value| value.as_array()) {
                for key in required.iter().filter_map(|value| value.as_str()) {
                    if !object.contains_key(key) {
                        anyhow::bail!(
                            "schema {} missing required key {}{}",
                            schema_path.display(),
                            if location == "$" { "" } else { "." },
                            if location == "$" {
                                key.to_string()
                            } else {
                                format!("{location}.{key}")
                            }
                        );
                    }
                }
            }
            if let Some(properties) = schema.get("properties").and_then(|value| value.as_object()) {
                for (key, property_schema) in properties {
                    if let Some(property_value) = object.get(key) {
                        let property_location = if location == "$" {
                            format!("$.{key}")
                        } else {
                            format!("{location}.{key}")
                        };
                        validate_schema_value(
                            property_schema,
                            property_value,
                            &property_location,
                            schema_path,
                        )?;
                    }
                }
            }
            Ok(())
        }
        Some("array") => {
            let items = value.as_array().ok_or_else(|| {
                anyhow::anyhow!(
                    "schema {} expected array at {}",
                    schema_path.display(),
                    location
                )
            })?;
            if let Some(item_schema) = schema.get("items") {
                for (index, item) in items.iter().enumerate() {
                    validate_schema_value(
                        item_schema,
                        item,
                        &format!("{}[{}]", location, index),
                        schema_path,
                    )?;
                }
            }
            Ok(())
        }
        Some("string") if value.is_string() => Ok(()),
        Some("string") => anyhow::bail!(
            "schema {} expected string at {}",
            schema_path.display(),
            location
        ),
        Some("number") if value.is_number() => Ok(()),
        Some("number") => anyhow::bail!(
            "schema {} expected number at {}",
            schema_path.display(),
            location
        ),
        Some("boolean") if value.is_boolean() => Ok(()),
        Some("boolean") => anyhow::bail!(
            "schema {} expected boolean at {}",
            schema_path.display(),
            location
        ),
        _ => Ok(()),
    }
}

fn validate_feature_step_output(step: &WorkflowStep, value: &Value) -> anyhow::Result<()> {
    match step.id.as_str() {
        ANALYSIS_STEP_ID => {
            parse_feature_analysis_output(value.clone())?;
            Ok(())
        }
        PLAN_STEP_ID => {
            parse_feature_plan_output(value.clone())?;
            Ok(())
        }
        EXECUTE_STEP_ID => {
            parse_feature_execute_output(value.clone())?;
            Ok(())
        }
        _ => Ok(()),
    }
}

fn parse_feature_analysis_output(value: Value) -> anyhow::Result<FeatureAnalysisOutput> {
    let output = serde_json::from_value::<FeatureAnalysisOutput>(value)?;
    if output.objective.trim().is_empty() {
        anyhow::bail!("analysis output objective must be non-empty");
    }
    for constraint in &output.constraints {
        if constraint.trim().is_empty() {
            anyhow::bail!("analysis constraints must be non-empty strings");
        }
    }
    for risk in &output.risks {
        if risk.trim().is_empty() {
            anyhow::bail!("analysis risks must be non-empty strings");
        }
    }
    for path in &output.affected_paths {
        if path.trim().is_empty() {
            anyhow::bail!("analysis affected_paths must be non-empty strings");
        }
    }
    Ok(output)
}

fn parse_feature_plan_output(value: Value) -> anyhow::Result<FeaturePlanOutput> {
    let output = serde_json::from_value::<FeaturePlanOutput>(value)?;
    if output.goal.trim().is_empty() {
        anyhow::bail!("plan output goal must be non-empty");
    }
    if output.tasks.is_empty() {
        anyhow::bail!("plan output must include at least one task");
    }
    let mut seen_ids = std::collections::BTreeSet::new();
    for task in &output.tasks {
        if task.id.trim().is_empty() {
            anyhow::bail!("plan task id must be non-empty");
        }
        if !seen_ids.insert(task.id.trim().to_string()) {
            anyhow::bail!("plan task ids must be unique");
        }
        if task.title.trim().is_empty() || task.description.trim().is_empty() {
            anyhow::bail!("plan task title and description must be non-empty");
        }
    }
    Ok(output)
}

fn parse_feature_execute_output(value: Value) -> anyhow::Result<FeatureExecuteOutput> {
    let output = serde_json::from_value::<FeatureExecuteOutput>(value)?;
    let completed = output
        .completed_tasks
        .iter()
        .map(|id| id.trim())
        .collect::<std::collections::BTreeSet<_>>();
    for task_id in &output.open_tasks {
        let task_id = task_id.trim();
        if task_id.is_empty() {
            anyhow::bail!("execute output open_tasks must not contain empty ids");
        }
        if completed.contains(task_id) {
            anyhow::bail!("execute output task ids cannot be both completed and open");
        }
    }
    for result in &output.validation_results {
        if result.target.trim().is_empty() || result.status.trim().is_empty() {
            anyhow::bail!("execute validation_results entries must include target and status");
        }
        if result
            .details
            .as_deref()
            .is_some_and(|details| details.trim().is_empty())
        {
            anyhow::bail!("execute validation result details must be non-empty when present");
        }
    }
    for path in &output.changed_paths {
        if path.trim().is_empty() {
            anyhow::bail!("execute changed_paths must be non-empty strings");
        }
    }
    Ok(output)
}

fn build_output_validation_feedback(step: &WorkflowStep, validation_error: &str) -> String {
    let contract = render_output_contract(&step.output_contract);
    if contract.is_empty() {
        format!(
            "Your previous response for step '{}' failed validation: {}. Re-run the step and satisfy the expected structured output.",
            step.id, validation_error
        )
    } else {
        format!(
            "Your previous response for step '{}' failed validation: {}. Re-run the step and respond with valid structured output matching this contract:\n\n{}",
            step.id, validation_error, contract
        )
    }
}

fn estimate_tokens(text: &str) -> u32 {
    let chars = text.chars().count();
    chars.div_ceil(TOKEN_ESTIMATE_DIVISOR) as u32
}

fn truncate_chars(text: &str, limit: usize) -> String {
    text.chars().take(limit).collect()
}

fn summarize_step_text(text: &str) -> String {
    truncate_chars(text.trim(), SUMMARY_CHAR_LIMIT)
}

fn parse_json_value(text: &str) -> Option<serde_json::Value> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Ok(value) = serde_json::from_str(trimmed) {
        return Some(value);
    }

    for prefix in ["```json", "```JSON", "```"] {
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            let body = rest.trim();
            if let Some(body) = body.strip_suffix("```") {
                if let Ok(value) = serde_json::from_str(body.trim()) {
                    return Some(value);
                }
            }
        }
    }

    extract_top_level_json(trimmed).and_then(|candidate| serde_json::from_str(candidate).ok())
}

fn extract_top_level_json(text: &str) -> Option<&str> {
    let start_indices = text
        .char_indices()
        .filter_map(|(index, character)| matches!(character, '{' | '[').then_some(index));

    for start in start_indices {
        let candidate = &text[start..];
        let Some(end) = find_top_level_json_end(candidate) else {
            continue;
        };
        let candidate = &candidate[..end];
        if serde_json::from_str::<Value>(candidate).is_ok() {
            return Some(candidate);
        }
    }

    None
}

fn find_top_level_json_end(text: &str) -> Option<usize> {
    let mut depth = 0u32;
    let mut in_string = false;
    let mut escaped = false;

    for (index, character) in text.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                in_string = false;
            }
            continue;
        }

        match character {
            '"' => in_string = true,
            '{' | '[' => depth += 1,
            '}' | ']' => {
                if depth == 0 {
                    return None;
                }
                depth -= 1;
                if depth == 0 {
                    return Some(index + character.len_utf8());
                }
            }
            _ => {}
        }
    }

    None
}

fn parse_structured_id(text: &str, field_names: &[&str]) -> Option<String> {
    let value = parse_json_value(text)?;
    parse_structured_id_from_value(Some(&value), field_names)
}

fn parse_structured_id_from_value(value: Option<&Value>, field_names: &[&str]) -> Option<String> {
    let object = value?.as_object()?;
    field_names.iter().find_map(|field_name| {
        object
            .get(*field_name)
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
    })
}

fn find_catalog_match<'a>(
    text: &str,
    candidates: impl IntoIterator<Item = &'a str>,
) -> Option<String> {
    let candidates = candidates.into_iter().collect::<Vec<_>>();
    let normalized = text.to_ascii_lowercase();
    normalized
        .split(|character: char| {
            !character.is_ascii_alphanumeric() && character != '-' && character != '_'
        })
        .find_map(|token| {
            if token.is_empty() {
                return None;
            }
            candidates.iter().find_map(|candidate| {
                token
                    .eq_ignore_ascii_case(candidate)
                    .then(|| (*candidate).to_string())
            })
        })
}

fn preview_text(text: &str, limit: usize) -> String {
    let mut chars = text.chars();
    let preview: String = chars.by_ref().take(limit).collect();
    if chars.next().is_some() {
        format!("{}...", preview)
    } else {
        text.to_string()
    }
}

fn preview_tool_invocation(tool_name: &str, input: &serde_json::Value) -> String {
    match tool_name {
        "bash" => input
            .get("command")
            .and_then(|value| value.as_str())
            .map(|command| format!("$ {}", preview_text(command, 80)))
            .unwrap_or_else(|| format!("{} {}", tool_name, preview_json_value(input, 80))),
        _ => input
            .get("path")
            .and_then(|value| value.as_str())
            .map(|path| preview_text(path, 80))
            .unwrap_or_else(|| preview_json_value(input, 80)),
    }
}

fn preview_json_value(value: &serde_json::Value, limit: usize) -> String {
    preview_text(
        &serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string()),
        limit,
    )
}

fn build_tool_run_detail_lines(
    tool_name: &str,
    input: &serde_json::Value,
    output: Option<&str>,
) -> Vec<String> {
    let mut lines = vec![
        format!("tool: {}", tool_name),
        format!("invoke: {}", preview_tool_invocation(tool_name, input)),
    ];

    if let Some(output) = output {
        lines.push("result:".to_string());
        lines.extend(preview_lines(output, 12, 160));
    }

    lines
}

fn preview_lines(text: &str, max_lines: usize, max_chars: usize) -> Vec<String> {
    let mut lines = text
        .lines()
        .take(max_lines)
        .map(|line| preview_text(line, max_chars))
        .collect::<Vec<_>>();
    if text.lines().count() > max_lines {
        lines.push("...".to_string());
    }
    if lines.is_empty() {
        lines.push(preview_text(text, max_chars));
    }
    lines
}

fn send_begin_tool_run(tx: &mpsc::Sender<RuntimeUiEnvelope>, turn_id: u64, tool_run: ToolRun) {
    let _ = tx.send(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::BeginToolRun { tool_run },
    ));
}

fn send_update_tool_run(tx: &mpsc::Sender<RuntimeUiEnvelope>, turn_id: u64, tool_run: ToolRun) {
    let _ = tx.send(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::UpdateToolRun { tool_run },
    ));
}

fn send_complete_tool_run(
    tx: &mpsc::Sender<RuntimeUiEnvelope>,
    turn_id: u64,
    id: &str,
    status: ToolRunStatus,
) {
    let _ = tx.send(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::CompleteToolRun {
            id: id.to_string(),
            status,
        },
    ));
}

fn find_markup_opening(text: &str) -> Option<(usize, &'static str)> {
    [
        ("<minimax:tool_call", "</minimax:tool_call>"),
        ("<invoke", "</invoke>"),
    ]
    .into_iter()
    .filter_map(|(opening, closing)| text.find(opening).map(|position| (position, closing)))
    .min_by_key(|(position, _)| *position)
}

fn suffix_prefix_len(text: &str, patterns: &[&str]) -> usize {
    let mut best = 0usize;
    for pattern in patterns {
        let max_len = pattern.len().min(text.len());
        for len in 1..=max_len {
            if text.ends_with(&pattern[..len]) {
                best = best.max(len);
            }
        }
    }
    best
}

fn tail_fragment(text: &str, keep: usize) -> String {
    if keep == 0 {
        String::new()
    } else {
        text[text.len() - keep..].to_string()
    }
}

fn send_workflow_step(
    tx: &mpsc::Sender<RuntimeUiEnvelope>,
    turn_id: u64,
    step: Option<WorkflowStepState>,
    workflow_id: &str,
    role: WorkflowRunRole,
) {
    let Some(step) = step else {
        return;
    };

    let _ = tx.send(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::SetStatusSlot {
            slot: StatusSlot::Workflow,
            value: StatusValue::WorkflowStep {
                workflow_id: workflow_id.to_string(),
                workflow_role: role,
                step_id: step.id.clone(),
                step_label: step.label.clone(),
                index: step.index,
                total: step.total,
            },
        },
    ));
    let _ = tx.send(RuntimeUiEnvelope::message(
        turn_id,
        RuntimeUiMessage {
            target: UiTarget::Activity(ActivityTarget::Log),
            source: UiSource::WorkflowStep {
                workflow_id: workflow_id.to_string(),
                workflow_role: role,
                step_id: step.id.clone(),
                step_label: step.label.clone(),
                index: step.index,
                total: step.total,
            },
            kind: UiMessageKind::Summary,
            content: UiContent::Text(step.label.clone()),
            priority: None,
        },
    ));
}

fn send_step_text(
    tx: &mpsc::Sender<RuntimeUiEnvelope>,
    turn_id: u64,
    workflow_id: &str,
    role: WorkflowRunRole,
    step: &WorkflowStep,
    text: &str,
) {
    let _ = tx.send(RuntimeUiEnvelope::message(
        turn_id,
        RuntimeUiMessage {
            target: UiTarget::Response,
            source: UiSource::WorkflowStep {
                workflow_id: workflow_id.to_string(),
                workflow_role: role,
                step_id: step.id.clone(),
                step_label: step.label.clone(),
                index: 0,
                total: 0,
            },
            kind: UiMessageKind::Narrative,
            content: UiContent::Text(text.to_string()),
            priority: None,
        },
    ));
}

fn send_assistant_text(tx: &mpsc::Sender<RuntimeUiEnvelope>, turn_id: u64, text: &str) {
    let _ = tx.send(RuntimeUiEnvelope::message(
        turn_id,
        RuntimeUiMessage {
            target: UiTarget::Response,
            source: UiSource::Assistant,
            kind: UiMessageKind::Result,
            content: UiContent::Text(text.to_string()),
            priority: None,
        },
    ));
}

fn send_error_text(tx: &mpsc::Sender<RuntimeUiEnvelope>, turn_id: u64, text: &str) {
    let _ = tx.send(RuntimeUiEnvelope::message(
        turn_id,
        RuntimeUiMessage {
            target: UiTarget::Response,
            source: UiSource::System,
            kind: UiMessageKind::Error,
            content: UiContent::Text(text.to_string()),
            priority: Some(UiPriority::High),
        },
    ));
}

fn send_warning_text(tx: &mpsc::Sender<RuntimeUiEnvelope>, turn_id: u64, text: &str) {
    let _ = tx.send(RuntimeUiEnvelope::message(
        turn_id,
        RuntimeUiMessage {
            target: UiTarget::Activity(ActivityTarget::Log),
            source: UiSource::System,
            kind: UiMessageKind::Warning,
            content: UiContent::Text(text.to_string()),
            priority: Some(UiPriority::Normal),
        },
    ));
}

fn send_tool_call_preview(
    tx: &mpsc::Sender<RuntimeUiEnvelope>,
    turn_id: u64,
    tool_name: &str,
    command: Option<String>,
    preview: String,
) {
    let source = UiSource::Tool {
        tool_name: tool_name.to_string(),
    };

    if let Some(command) = command {
        let _ = tx.send(RuntimeUiEnvelope::message(
            turn_id,
            RuntimeUiMessage {
                target: UiTarget::Activity(ActivityTarget::Log),
                source: source.clone(),
                kind: UiMessageKind::Log,
                content: UiContent::Text(format!("$ {command}")),
                priority: None,
            },
        ));
    }

    let _ = tx.send(RuntimeUiEnvelope::message(
        turn_id,
        RuntimeUiMessage {
            target: UiTarget::Activity(ActivityTarget::Log),
            source,
            kind: UiMessageKind::Log,
            content: UiContent::Text(preview),
            priority: None,
        },
    ));
}

fn send_todo_snapshot(tx: &mpsc::Sender<RuntimeUiEnvelope>, turn_id: u64, rendered: &str) {
    let _ = tx.send(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::ReplacePanel {
            target: UiTarget::Todo,
            content: UiContent::Text(rendered.to_string()),
        },
    ));
}

fn send_begin_response_section(
    tx: &mpsc::Sender<RuntimeUiEnvelope>,
    turn_id: u64,
    section: ResponseSection,
) {
    let _ = tx.send(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::BeginResponseSection { section },
    ));
}

fn send_append_response_section(
    tx: &mpsc::Sender<RuntimeUiEnvelope>,
    turn_id: u64,
    id: &str,
    delta: ResponseSectionDelta,
) {
    let _ = tx.send(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::AppendResponseSection {
            id: id.to_string(),
            delta,
        },
    ));
}

fn send_complete_response_section(
    tx: &mpsc::Sender<RuntimeUiEnvelope>,
    turn_id: u64,
    id: &str,
    state: ResponseSectionState,
) {
    let _ = tx.send(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::CompleteResponseSection {
            id: id.to_string(),
            state,
        },
    ));
}

fn send_turn_finished(tx: &mpsc::Sender<RuntimeUiEnvelope>, turn_id: u64) {
    let _ = tx.send(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::ClearStatusSlot {
            slot: StatusSlot::Workflow,
        },
    ));
    let _ = tx.send(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::SetStatusSlot {
            slot: StatusSlot::Agent,
            value: StatusValue::Label("Idle".to_string()),
        },
    ));
}

fn build_step_system_prompt(input: &StepExecutionInput) -> String {
    let mut sections = vec![
        input
            .resolved_skills
            .build_system_prompt(&input.base_system),
        format!("Workflow phase: {}", input.step.label),
        render_visible_tools(input.resolved_tools.tool_names()),
    ];
    let session_context = render_session_context(&input.session_context);
    if !session_context.trim().is_empty() {
        sections.push(format!(
            "<session_context>\n{}\n</session_context>",
            session_context.trim_end()
        ));
    }
    if let Some(structured_input) = input.structured_input.as_ref() {
        sections.push(format!(
            "<structured_input step_id=\"{}\">\n{}\n</structured_input>",
            input.step.id,
            render_structured_input(structured_input)
        ));
    }
    if let Some(todo_snapshot) = input.todo_snapshot.as_deref() {
        sections.push(format!(
            "<todo_state step_id=\"{}\">\n{}\n</todo_state>",
            input.step.id, todo_snapshot
        ));
    }
    let output_contract = render_output_contract(&input.step.output_contract);
    if !output_contract.is_empty() {
        sections.push(format!(
            "<output_contract step_id=\"{}\">\n{}\n</output_contract>",
            input.step.id, output_contract
        ));
    }
    if !input.step_prompt.trim().is_empty() {
        sections.push(format!(
            "<workflow_prompt step_id=\"{}\" prompt_path=\"{}\">\n{}\n</workflow_prompt>",
            input.step.id,
            input.step.prompt_path.display(),
            input.step_prompt.trim_end()
        ));
    }

    sections.join("\n\n")
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::mpsc;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use async_trait::async_trait;
    use omega_client::{
        ChatRequest, ChatResponse, ClientError, ContentBlock, STOP_REASON_END_TURN,
        STOP_REASON_TOOL_USE,
    };
    use omega_core::{DynLlmClient, LlmClient};
    use omega_workflow::{
        ANALYSIS_STEP_ID, CHAT_STEP_ID, CHAT_WORKFLOW_ID, DEFAULT_ANALYSIS_SCHEMA_PATH, DataFormat,
        EXECUTE_STEP_ID, FEATURE_WORKFLOW_ID, LoadedWorkflowCatalog, PLAN_STEP_ID, REPORT_STEP_ID,
        ROOT_WORKFLOW_ID, SCENE_RECOGNITION_STEP_ID, SELECT_WORKFLOW_STEP_ID, StepInputContract,
        StepLoopMode, StepOutputContract,
    };

    use super::{
        AgentSession, AgentSessionConfig, ProviderMarkupSanitizer, ResponseSectionDelta,
        ResponseSectionKind, ResponseSectionState, RuntimeUiEffect, RuntimeUiEnvelope,
        SessionContext, SessionSkillCatalog, SessionToolCatalog, StatusSlot, StatusValue,
        StepContextWriteKind, StepOutputStatus, StepSkillRequest, StepToolRequest, ToolRunStatus,
        UiMessageKind, UiSource, UiTarget, WorkflowRunRole, preview_text, resolve_structured_input,
        validate_schema_file, validate_structured_output,
    };

    struct IdleClient;

    struct SequencedClient {
        responses: Mutex<Vec<ChatResponse>>,
        systems: Mutex<Vec<Option<String>>>,
        max_tokens: Mutex<Vec<u32>>,
    }

    #[async_trait]
    impl LlmClient for IdleClient {
        async fn chat(&self, _request: ChatRequest) -> Result<ChatResponse, ClientError> {
            panic!("chat should not be called in AgentSession unit tests");
        }

        fn provider_name(&self) -> &'static str {
            "idle"
        }
    }

    #[async_trait]
    impl LlmClient for SequencedClient {
        async fn chat(&self, request: ChatRequest) -> Result<ChatResponse, ClientError> {
            self.systems.lock().unwrap().push(request.system.clone());
            self.max_tokens.lock().unwrap().push(request.max_tokens);
            let mut responses = self.responses.lock().unwrap();
            Ok(responses.remove(0))
        }

        fn provider_name(&self) -> &'static str {
            "sequenced"
        }
    }

    fn feature_analysis_json() -> &'static str {
        r#"{"objective":"Implement the requested change","constraints":["preserve existing behavior"],"risks":["regression risk"],"affected_paths":["crates/omega-session/src/lib.rs"]}"#
    }

    fn feature_plan_json() -> &'static str {
        r#"{"goal":"Implement the requested change safely","tasks":[{"id":"task-1","title":"Inspect code","description":"Review the relevant workflow and session logic"},{"id":"task-2","title":"Apply changes","description":"Implement the requested code and test updates"}],"validation_targets":["cargo test -p omega-workflow -p omega-session"]}"#
    }

    fn feature_execute_json() -> &'static str {
        r#"{"completed_tasks":["task-1"],"open_tasks":["task-2"],"validation_results":[{"target":"cargo test -p omega-workflow -p omega-session","status":"passed"}],"changed_paths":["crates/omega-session/src/lib.rs"]}"#
    }

    #[test]
    fn preview_text_preserves_utf8_boundaries() {
        assert_eq!(preview_text("你好世界", 3), "你好世...");
    }

    #[test]
    fn provider_markup_sanitizer_strips_known_tool_wrappers_across_chunks() {
        let mut sanitizer = ProviderMarkupSanitizer::default();

        assert_eq!(sanitizer.push("before<minimax:tool_"), "before");
        assert_eq!(
            sanitizer.push("call><invoke name=\"bash\">ignored</invoke></minimax:tool_call>after"),
            "after"
        );
        assert_eq!(sanitizer.finish(), "");
    }

    #[test]
    fn structured_contract_helpers_resolve_inputs_and_validate_required_json() {
        let mut session_context = SessionContext::new(ROOT_WORKFLOW_ID);
        session_context.step_outputs.insert(
            ANALYSIS_STEP_ID.to_string(),
            serde_json::json!({"summary": "analysis"}),
        );

        let step = omega_workflow::WorkflowStep {
            id: "plan".to_string(),
            label: "Plan".to_string(),
            prompt_path: PathBuf::from(".omega/prompt/step/plan.md"),
            loop_mode: StepLoopMode::AgentLoop,
            max_iterations: 8,
            tool_request: StepToolRequest::Block(Vec::new()),
            skill_request: StepSkillRequest::MatchTask,
            input_contract: StepInputContract::Required {
                sources: vec![ANALYSIS_STEP_ID.to_string()],
            },
            output_contract: StepOutputContract::Required {
                format: DataFormat::Json,
                schema_path: None,
                max_retries: 2,
            },
            enabled: true,
        };

        let structured_input = resolve_structured_input(&session_context, &step)
            .unwrap()
            .unwrap();
        assert_eq!(
            structured_input,
            serde_json::json!({
                ANALYSIS_STEP_ID: {"summary": "analysis"}
            })
        );

        let structured_output =
            validate_structured_output(
                &step.output_contract,
                "{\"goal\":\"ship\",\"tasks\":[{\"id\":\"task-1\",\"title\":\"Inspect\",\"description\":\"Review code\"}],\"validation_targets\":[\"cargo test\"]}",
            )
                .unwrap()
                .unwrap();
        assert_eq!(
            structured_output,
            serde_json::json!({
                "goal": "ship",
                "tasks": [{"id": "task-1", "title": "Inspect", "description": "Review code"}],
                "validation_targets": ["cargo test"]
            })
        );
        assert!(validate_structured_output(&step.output_contract, "not json").is_err());
    }

    #[test]
    fn structured_contract_helpers_extract_embedded_json_value() {
        let step = omega_workflow::WorkflowStep {
            id: SCENE_RECOGNITION_STEP_ID.to_string(),
            label: "Scene Recognition".to_string(),
            prompt_path: PathBuf::from(".omega/prompt/step/scene-recognition.md"),
            loop_mode: StepLoopMode::AgentLoop,
            max_iterations: 2,
            tool_request: StepToolRequest::Block(Vec::new()),
            skill_request: StepSkillRequest::MatchTask,
            input_contract: StepInputContract::None,
            output_contract: StepOutputContract::Required {
                format: DataFormat::Json,
                schema_path: None,
                max_retries: 1,
            },
            enabled: true,
        };

        let structured_output = validate_structured_output(
            &step.output_contract,
            "Scene: feature\n{\"recognized_scene_id\":\"feature\"}",
        )
        .unwrap()
        .unwrap();

        assert_eq!(
            structured_output,
            serde_json::json!({"recognized_scene_id": "feature"})
        );
    }

    #[test]
    fn schema_validator_rejects_missing_required_keys() {
        let root = std::env::temp_dir().join("omega-agent-session-schema-validation-test");
        let _ = std::fs::remove_dir_all(&root);
        let loaded = LoadedWorkflowCatalog::load(&root);
        assert!(loaded.warnings.is_empty());

        let error = validate_schema_file(
            &root,
            &PathBuf::from(DEFAULT_ANALYSIS_SCHEMA_PATH),
            &serde_json::json!({"objective": "Ship feature"}),
        )
        .unwrap_err();

        assert!(error.to_string().contains("missing required key"));
    }

    #[test]
    fn spawn_turn_retries_invalid_required_structured_output() {
        let client: Arc<SequencedClient> = Arc::new(SequencedClient {
            responses: Mutex::new(vec![
                ChatResponse {
                    id: "scene-1".to_string(),
                    model: Some("test-model".to_string()),
                    content: vec![ContentBlock::text("{\"recognized_scene_id\":\"feature\"}")],
                    stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                    usage: None,
                },
                ChatResponse {
                    id: "select-1".to_string(),
                    model: Some("test-model".to_string()),
                    content: vec![ContentBlock::text("{\"selected_workflow_id\":\"feature\"}")],
                    stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                    usage: None,
                },
                ChatResponse {
                    id: "analysis-1".to_string(),
                    model: Some("test-model".to_string()),
                    content: vec![ContentBlock::text("analysis")],
                    stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                    usage: None,
                },
                ChatResponse {
                    id: "analysis-2".to_string(),
                    model: Some("test-model".to_string()),
                    content: vec![ContentBlock::text(feature_analysis_json())],
                    stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                    usage: None,
                },
                ChatResponse {
                    id: "plan-1".to_string(),
                    model: Some("test-model".to_string()),
                    content: vec![ContentBlock::text(feature_plan_json())],
                    stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                    usage: None,
                },
                ChatResponse {
                    id: "execute-1".to_string(),
                    model: Some("test-model".to_string()),
                    content: vec![ContentBlock::text("execution complete")],
                    stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                    usage: None,
                },
                ChatResponse {
                    id: "report-1".to_string(),
                    model: Some("test-model".to_string()),
                    content: vec![ContentBlock::text("done")],
                    stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                    usage: None,
                },
            ]),
            systems: Mutex::new(Vec::new()),
            max_tokens: Mutex::new(Vec::new()),
        });
        let client_dyn: DynLlmClient = client;
        let root = std::env::temp_dir().join("omega-agent-session-structured-retry-test");
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::create_dir_all(&root);
        let skills_dir = root.join(".claude/skills/review");
        let _ = std::fs::create_dir_all(&skills_dir);
        let _ = std::fs::write(
            skills_dir.join("SKILL.md"),
            "---\nname: review\ndescription: Review code\n---\nFind regressions.",
        );
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let loaded_catalog = LoadedWorkflowCatalog::load(&root);
        let session = AgentSession::new(AgentSessionConfig {
            client: client_dyn,
            system: "system".to_string(),
            cwd: root,
            runtime_handle: runtime.handle().clone(),
            scene_catalog: loaded_catalog.scene_catalog,
            workflow_catalog: loaded_catalog.workflow_catalog,
            prompt_catalog: loaded_catalog.prompt_catalog,
            context_window: 200_000,
            max_output_tokens: 32_000,
            bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        })
        .unwrap();
        let (tx, rx) = mpsc::channel();

        session.spawn_turn("hello".to_string(), 21, tx).unwrap();

        let mut warnings = Vec::new();
        let mut diagnostics = Vec::new();
        loop {
            match rx.recv_timeout(Duration::from_secs(2)).unwrap() {
                RuntimeUiEnvelope::Message { turn_id, message }
                    if turn_id == 21
                        && matches!(message.source, UiSource::System)
                        && message.kind == UiMessageKind::Warning =>
                {
                    warnings.push(message.content.as_text().to_string());
                }
                RuntimeUiEnvelope::Effect {
                    turn_id,
                    effect:
                        RuntimeUiEffect::UpsertStepDiagnostics {
                            diagnostics: update,
                        },
                } => {
                    assert_eq!(turn_id, 21);
                    diagnostics.push(*update);
                }
                RuntimeUiEnvelope::Effect {
                    turn_id,
                    effect:
                        RuntimeUiEffect::SetStatusSlot {
                            slot: StatusSlot::Agent,
                            value: StatusValue::Label(label),
                        },
                } => {
                    assert_eq!(turn_id, 21);
                    assert_eq!(label, "Idle");
                    break;
                }
                _ => {}
            }
        }

        assert!(warnings.iter().any(|warning| {
            warning.contains("Step 'analysis' produced invalid structured output")
        }));
        assert!(diagnostics.iter().any(|diagnostics| {
            diagnostics.step_id == ANALYSIS_STEP_ID
                && diagnostics.output.status == StepOutputStatus::Invalid
        }));
    }

    #[test]
    fn spawn_turn_syncs_execute_output_back_into_todo_state_for_report() {
        let client: Arc<SequencedClient> = Arc::new(SequencedClient {
            responses: Mutex::new(vec![
                ChatResponse {
                    id: "scene-1".to_string(),
                    model: Some("test-model".to_string()),
                    content: vec![ContentBlock::text("{\"recognized_scene_id\":\"feature\"}")],
                    stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                    usage: None,
                },
                ChatResponse {
                    id: "select-1".to_string(),
                    model: Some("test-model".to_string()),
                    content: vec![ContentBlock::text("{\"selected_workflow_id\":\"feature\"}")],
                    stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                    usage: None,
                },
                ChatResponse {
                    id: "analysis-1".to_string(),
                    model: Some("test-model".to_string()),
                    content: vec![ContentBlock::text(feature_analysis_json())],
                    stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                    usage: None,
                },
                ChatResponse {
                    id: "plan-1".to_string(),
                    model: Some("test-model".to_string()),
                    content: vec![ContentBlock::text(feature_plan_json())],
                    stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                    usage: None,
                },
                ChatResponse {
                    id: "execute-1".to_string(),
                    model: Some("test-model".to_string()),
                    content: vec![ContentBlock::text(feature_execute_json())],
                    stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                    usage: None,
                },
                ChatResponse {
                    id: "report-1".to_string(),
                    model: Some("test-model".to_string()),
                    content: vec![ContentBlock::text("done")],
                    stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                    usage: None,
                },
            ]),
            systems: Mutex::new(Vec::new()),
            max_tokens: Mutex::new(Vec::new()),
        });
        let client_dyn: DynLlmClient = client.clone();
        let root = std::env::temp_dir().join("omega-agent-session-execute-todo-sync-test");
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::create_dir_all(&root);
        let skills_dir = root.join(".claude/skills/review");
        let _ = std::fs::create_dir_all(&skills_dir);
        let _ = std::fs::write(
            skills_dir.join("SKILL.md"),
            "---\nname: review\ndescription: Review code\n---\nFind regressions.",
        );
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let loaded_catalog = LoadedWorkflowCatalog::load(&root);
        let session = AgentSession::new(AgentSessionConfig {
            client: client_dyn,
            system: "system".to_string(),
            cwd: root,
            runtime_handle: runtime.handle().clone(),
            scene_catalog: loaded_catalog.scene_catalog,
            workflow_catalog: loaded_catalog.workflow_catalog,
            prompt_catalog: loaded_catalog.prompt_catalog,
            context_window: 200_000,
            max_output_tokens: 32_000,
            bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        })
        .unwrap();
        let (tx, rx) = mpsc::channel();

        session.spawn_turn("hello".to_string(), 41, tx).unwrap();

        let mut todo_panels = Vec::new();
        let mut diagnostics = Vec::new();
        loop {
            match rx.recv_timeout(Duration::from_secs(2)).unwrap() {
                RuntimeUiEnvelope::Effect {
                    turn_id,
                    effect:
                        RuntimeUiEffect::ReplacePanel {
                            target: UiTarget::Todo,
                            content,
                        },
                } => {
                    assert_eq!(turn_id, 41);
                    todo_panels.push(content.as_text().to_string());
                }
                RuntimeUiEnvelope::Effect {
                    turn_id,
                    effect:
                        RuntimeUiEffect::UpsertStepDiagnostics {
                            diagnostics: update,
                        },
                } => {
                    assert_eq!(turn_id, 41);
                    diagnostics.push(*update);
                }
                RuntimeUiEnvelope::Effect {
                    turn_id,
                    effect:
                        RuntimeUiEffect::SetStatusSlot {
                            slot: StatusSlot::Agent,
                            value: StatusValue::Label(label),
                        },
                } => {
                    assert_eq!(turn_id, 41);
                    assert_eq!(label, "Idle");
                    break;
                }
                _ => {}
            }
        }

        assert!(
            todo_panels
                .iter()
                .any(|panel| panel.contains("[x] #task-1"))
        );
        assert!(
            todo_panels
                .iter()
                .any(|panel| panel.contains("[>] #task-2"))
        );
        assert!(diagnostics.iter().any(|diagnostics| {
            diagnostics.step_id == PLAN_STEP_ID
                && diagnostics.output.status == StepOutputStatus::Valid
                && diagnostics.session_writes.iter().any(|write| {
                    write.path == "step_outputs.plan"
                        && write.kind == StepContextWriteKind::Added
                        && write.before_preview.is_none()
                        && write.after_preview.is_some()
                })
        }));
        assert!(diagnostics.iter().any(|diagnostics| {
            diagnostics.step_id == PLAN_STEP_ID
                && diagnostics.session_writes.iter().any(|write| {
                    write.path == "todo.rendered"
                        && write.kind == StepContextWriteKind::Added
                        && write.before_preview.is_none()
                        && write
                            .after_preview
                            .as_deref()
                            .is_some_and(|preview| preview.contains("#task-1"))
                })
        }));
        assert!(diagnostics.iter().any(|diagnostics| {
            diagnostics.step_id == EXECUTE_STEP_ID
                && diagnostics.session_writes.iter().any(|write| {
                    write.path == "todo.rendered"
                        && write.kind == StepContextWriteKind::Updated
                        && write
                            .before_preview
                            .as_deref()
                            .is_some_and(|preview| preview.contains("[>] #task-1"))
                        && write
                            .after_preview
                            .as_deref()
                            .is_some_and(|preview| preview.contains("[x] #task-1"))
                })
        }));
        assert!(diagnostics.iter().any(|diagnostics| {
            diagnostics.step_id == REPORT_STEP_ID && diagnostics.input.todo_state_preview.is_some()
        }));

        let systems = client.systems.lock().unwrap();
        assert!(
            systems
                .iter()
                .filter_map(|system| system.as_deref())
                .any(|system| system.contains("<todo_state step_id=\"report\">"))
        );
        assert!(
            systems
                .iter()
                .filter_map(|system| system.as_deref())
                .any(|system| system.contains("(1/2 completed)"))
        );
    }

    #[test]
    fn interrupt_restores_checkpoint_messages() {
        let client: DynLlmClient = Arc::new(IdleClient);
        let root = std::env::temp_dir().join("omega-agent-session-test");
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::create_dir_all(&root);
        let skills_dir = root.join(".claude/skills/review");
        let _ = std::fs::create_dir_all(&skills_dir);
        let _ = std::fs::write(
            skills_dir.join("SKILL.md"),
            "---\nname: review\ndescription: Review code\n---\nFind regressions.",
        );
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let loaded_catalog = LoadedWorkflowCatalog::load(&root);
        let session = AgentSession::new(AgentSessionConfig {
            client,
            system: "system".to_string(),
            cwd: root,
            runtime_handle: runtime.handle().clone(),
            scene_catalog: loaded_catalog.scene_catalog,
            workflow_catalog: loaded_catalog.workflow_catalog,
            prompt_catalog: loaded_catalog.prompt_catalog,
            context_window: 200_000,
            max_output_tokens: 32_000,
            bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        })
        .unwrap();

        {
            let mut slot = session.agent_slot.lock().unwrap();
            let agent = slot.agent.as_mut().unwrap();
            agent.add_user_message("checkpoint me");
        }
        session.checkpoint_current_messages();
        session.interrupt(42).unwrap();

        let slot = session.agent_slot.lock().unwrap();
        let restored = slot.agent.as_ref().unwrap().messages();
        assert_eq!(slot.turn_id, 42);
        assert_eq!(restored.len(), 1);
    }

    #[test]
    fn spawn_turn_emits_root_then_child_workflow_steps_and_uses_phase_prompts() {
        let client: Arc<SequencedClient> = Arc::new(SequencedClient {
            responses: Mutex::new(vec![
                ChatResponse {
                    id: "scene-1".to_string(),
                    model: Some("test-model".to_string()),
                    content: vec![ContentBlock::text("{\"recognized_scene_id\":\"feature\"}")],
                    stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                    usage: None,
                },
                ChatResponse {
                    id: "select-1".to_string(),
                    model: Some("test-model".to_string()),
                    content: vec![ContentBlock::text("{\"selected_workflow_id\":\"feature\"}")],
                    stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                    usage: None,
                },
                ChatResponse {
                    id: "analysis-1".to_string(),
                    model: Some("test-model".to_string()),
                    content: vec![ContentBlock::text(feature_analysis_json())],
                    stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                    usage: None,
                },
                ChatResponse {
                    id: "plan-1".to_string(),
                    model: Some("test-model".to_string()),
                    content: vec![ContentBlock::text(feature_plan_json())],
                    stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                    usage: None,
                },
                ChatResponse {
                    id: "execute-1".to_string(),
                    model: Some("test-model".to_string()),
                    content: vec![ContentBlock::tool_use(
                        "tool-1",
                        "bash",
                        serde_json::json!({"command": "echo hi"}),
                    )],
                    stop_reason: Some(STOP_REASON_TOOL_USE.to_string()),
                    usage: None,
                },
                ChatResponse {
                    id: "execute-2".to_string(),
                    model: Some("test-model".to_string()),
                    content: vec![ContentBlock::text("execution complete")],
                    stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                    usage: None,
                },
                ChatResponse {
                    id: "report-1".to_string(),
                    model: Some("test-model".to_string()),
                    content: vec![ContentBlock::text("done")],
                    stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                    usage: None,
                },
            ]),
            systems: Mutex::new(Vec::new()),
            max_tokens: Mutex::new(Vec::new()),
        });
        let client_dyn: DynLlmClient = client.clone();
        let root = std::env::temp_dir().join("omega-agent-session-workflow-test");
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::create_dir_all(&root);
        let skills_dir = root.join(".claude/skills/review");
        let _ = std::fs::create_dir_all(&skills_dir);
        let _ = std::fs::write(
            skills_dir.join("SKILL.md"),
            "---\nname: review\ndescription: Review code\n---\nFind regressions.",
        );
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let loaded_catalog = LoadedWorkflowCatalog::load(&root);
        let session = AgentSession::new(AgentSessionConfig {
            client: client_dyn,
            system: "system".to_string(),
            cwd: root,
            runtime_handle: runtime.handle().clone(),
            scene_catalog: loaded_catalog.scene_catalog,
            workflow_catalog: loaded_catalog.workflow_catalog,
            prompt_catalog: loaded_catalog.prompt_catalog,
            context_window: 200_000,
            max_output_tokens: 32_000,
            bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        })
        .unwrap();
        let (tx, rx) = mpsc::channel();

        session.spawn_turn("hello".to_string(), 7, tx).unwrap();

        let mut steps = Vec::new();
        let mut step_texts = Vec::new();
        let mut session_routes = Vec::new();
        let mut todo_panels = Vec::new();
        let mut logs = Vec::new();
        let mut saw_text = false;
        loop {
            match rx.recv_timeout(Duration::from_secs(2)).unwrap() {
                RuntimeUiEnvelope::Effect {
                    turn_id,
                    effect:
                        RuntimeUiEffect::SetStatusSlot {
                            slot: StatusSlot::Workflow,
                            value:
                                StatusValue::WorkflowStep {
                                    workflow_id,
                                    workflow_role,
                                    step_id,
                                    step_label,
                                    ..
                                },
                        },
                } => {
                    assert_eq!(turn_id, 7);
                    steps.push((workflow_id, workflow_role, step_id, step_label));
                }
                RuntimeUiEnvelope::Effect {
                    turn_id,
                    effect:
                        RuntimeUiEffect::SetStatusSlot {
                            slot: StatusSlot::Session,
                            value:
                                StatusValue::SessionRouting {
                                    root_workflow_id,
                                    active_workflow_id,
                                    active_workflow_role,
                                    recognized_scene_id,
                                    selected_workflow_id,
                                },
                        },
                } => {
                    assert_eq!(turn_id, 7);
                    session_routes.push((
                        root_workflow_id,
                        active_workflow_id,
                        active_workflow_role,
                        recognized_scene_id,
                        selected_workflow_id,
                    ));
                }
                RuntimeUiEnvelope::Message { turn_id, message } => {
                    assert_eq!(turn_id, 7);
                    match (message.source, message.kind) {
                        (
                            UiSource::WorkflowStep {
                                workflow_id,
                                workflow_role,
                                step_id,
                                step_label,
                                ..
                            },
                            UiMessageKind::Narrative,
                        ) => step_texts.push((
                            workflow_id,
                            workflow_role,
                            step_id,
                            step_label,
                            message.content.as_text().to_string(),
                        )),
                        (UiSource::Assistant, UiMessageKind::Result) => {
                            assert_eq!(message.content.as_text(), "done");
                            saw_text = true;
                        }
                        (
                            UiSource::SessionRouting,
                            UiMessageKind::Summary | UiMessageKind::Warning,
                        ) => logs.push(message.content.as_text().to_string()),
                        (UiSource::System, UiMessageKind::Summary | UiMessageKind::Warning) => {
                            logs.push(message.content.as_text().to_string())
                        }
                        _ => {}
                    }
                }
                RuntimeUiEnvelope::Effect {
                    turn_id,
                    effect:
                        RuntimeUiEffect::ReplacePanel {
                            target: UiTarget::Todo,
                            content,
                        },
                } => {
                    assert_eq!(turn_id, 7);
                    todo_panels.push(content.as_text().to_string());
                }
                RuntimeUiEnvelope::Effect {
                    turn_id,
                    effect:
                        RuntimeUiEffect::SetStatusSlot {
                            slot: StatusSlot::Agent,
                            value: StatusValue::Label(label),
                        },
                } => {
                    assert_eq!(turn_id, 7);
                    assert_eq!(label, "Idle");
                    break;
                }
                _ => {}
            }
        }

        assert_eq!(
            steps,
            vec![
                (
                    ROOT_WORKFLOW_ID.to_string(),
                    WorkflowRunRole::Root,
                    SCENE_RECOGNITION_STEP_ID.to_string(),
                    "Scene Recognition".to_string(),
                ),
                (
                    ROOT_WORKFLOW_ID.to_string(),
                    WorkflowRunRole::Root,
                    SELECT_WORKFLOW_STEP_ID.to_string(),
                    "Select Workflow".to_string(),
                ),
                (
                    FEATURE_WORKFLOW_ID.to_string(),
                    WorkflowRunRole::Child,
                    ANALYSIS_STEP_ID.to_string(),
                    "Analyze".to_string(),
                ),
                (
                    FEATURE_WORKFLOW_ID.to_string(),
                    WorkflowRunRole::Child,
                    "plan".to_string(),
                    "Plan".to_string(),
                ),
                (
                    FEATURE_WORKFLOW_ID.to_string(),
                    WorkflowRunRole::Child,
                    EXECUTE_STEP_ID.to_string(),
                    "Execute".to_string(),
                ),
                (
                    FEATURE_WORKFLOW_ID.to_string(),
                    WorkflowRunRole::Child,
                    "report".to_string(),
                    "Report".to_string(),
                ),
            ]
        );
        assert_eq!(
            step_texts,
            vec![
                (
                    FEATURE_WORKFLOW_ID.to_string(),
                    WorkflowRunRole::Child,
                    ANALYSIS_STEP_ID.to_string(),
                    "Analyze".to_string(),
                    feature_analysis_json().to_string(),
                ),
                (
                    FEATURE_WORKFLOW_ID.to_string(),
                    WorkflowRunRole::Child,
                    "plan".to_string(),
                    "Plan".to_string(),
                    feature_plan_json().to_string(),
                ),
                (
                    FEATURE_WORKFLOW_ID.to_string(),
                    WorkflowRunRole::Child,
                    EXECUTE_STEP_ID.to_string(),
                    "Execute".to_string(),
                    "execution complete".to_string(),
                ),
            ]
        );
        assert!(saw_text);
        assert!(session_routes.iter().any(|route| {
            route
                == &(
                    ROOT_WORKFLOW_ID.to_string(),
                    ROOT_WORKFLOW_ID.to_string(),
                    WorkflowRunRole::Root,
                    None,
                    None,
                )
        }));
        assert!(session_routes.iter().any(|route| {
            route
                == &(
                    ROOT_WORKFLOW_ID.to_string(),
                    ROOT_WORKFLOW_ID.to_string(),
                    WorkflowRunRole::Root,
                    Some("feature".to_string()),
                    None,
                )
        }));
        assert!(session_routes.iter().any(|route| {
            route
                == &(
                    ROOT_WORKFLOW_ID.to_string(),
                    FEATURE_WORKFLOW_ID.to_string(),
                    WorkflowRunRole::Child,
                    Some("feature".to_string()),
                    Some(FEATURE_WORKFLOW_ID.to_string()),
                )
        }));
        assert!(
            logs.iter()
                .any(|line| line.contains("Recognized scene 'feature'"))
        );
        assert!(
            logs.iter()
                .any(|line| line.contains("Selected workflow 'feature'"))
        );
        assert!(todo_panels.iter().any(|panel| panel.contains("#task-1")));
        assert!(todo_panels.iter().any(|panel| panel.contains("#task-2")));
        let systems = client.systems.lock().unwrap();
        assert_eq!(systems.len(), 7);
        assert!(
            systems[0]
                .as_deref()
                .is_some_and(|system| system.contains("Workflow role: root"))
        );
        assert!(
            systems[0]
                .as_deref()
                .is_some_and(|system| system.contains("Visible tools: none"))
        );
        assert!(
            systems[1]
                .as_deref()
                .is_some_and(|system| system.contains("Recognized scene: feature"))
        );
        assert!(
            systems[1]
                .as_deref()
                .is_some_and(|system| system.contains("Recognized scene: feature."))
        );
        assert!(
            systems[1]
                .as_deref()
                .is_some_and(|system| system.contains("Visible tools: none"))
        );
        assert!(
            systems[2]
                .as_deref()
                .is_some_and(|system| system.contains("Workflow role: child"))
        );
        assert!(
            systems[2]
                .as_deref()
                .is_some_and(|system| system.contains("Active workflow: feature"))
        );
        assert!(
            systems[2]
                .as_deref()
                .is_some_and(|system| system.contains("Selected workflow: feature."))
        );
        assert!(
            systems[2]
                .as_deref()
                .is_some_and(|system| system.contains("hello"))
        );
        assert!(
            systems
                .iter()
                .filter_map(|system| system.as_deref())
                .any(|system| system.contains("<todo_state step_id=\"execute\">"))
        );
        assert!(
            systems
                .iter()
                .filter_map(|system| system.as_deref())
                .any(|system| system.contains("#task-1"))
        );
        assert!(
            systems[6]
                .as_deref()
                .is_some_and(|system| system.contains("Workflow phase: Report"))
        );
    }

    #[test]
    fn chat_scene_routes_to_chat_workflow_without_showing_root_text() {
        let client: Arc<SequencedClient> = Arc::new(SequencedClient {
            responses: Mutex::new(vec![
                ChatResponse {
                    id: "scene-1".to_string(),
                    model: Some("test-model".to_string()),
                    content: vec![ContentBlock::text("{\"recognized_scene_id\":\"chat\"}")],
                    stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                    usage: None,
                },
                ChatResponse {
                    id: "select-1".to_string(),
                    model: Some("test-model".to_string()),
                    content: vec![ContentBlock::text("{\"selected_workflow_id\":\"chat\"}")],
                    stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                    usage: None,
                },
                ChatResponse {
                    id: "chat-1".to_string(),
                    model: Some("test-model".to_string()),
                    content: vec![ContentBlock::text("chat answer")],
                    stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                    usage: None,
                },
            ]),
            systems: Mutex::new(Vec::new()),
            max_tokens: Mutex::new(Vec::new()),
        });
        let client_dyn: DynLlmClient = client.clone();
        let root = std::env::temp_dir().join("omega-agent-session-chat-test");
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::create_dir_all(&root);
        let skills_dir = root.join(".claude/skills/review");
        let _ = std::fs::create_dir_all(&skills_dir);
        let _ = std::fs::write(
            skills_dir.join("SKILL.md"),
            "---\nname: review\ndescription: Review code\n---\nFind regressions.",
        );
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let loaded_catalog = LoadedWorkflowCatalog::load(&root);
        let session = AgentSession::new(AgentSessionConfig {
            client: client_dyn,
            system: "system".to_string(),
            cwd: root,
            runtime_handle: runtime.handle().clone(),
            scene_catalog: loaded_catalog.scene_catalog,
            workflow_catalog: loaded_catalog.workflow_catalog,
            prompt_catalog: loaded_catalog.prompt_catalog,
            context_window: 200_000,
            max_output_tokens: 24_000,
            bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        })
        .unwrap();
        let (tx, rx) = mpsc::channel();

        session.spawn_turn("just chat".to_string(), 9, tx).unwrap();

        let mut steps = Vec::new();
        let mut root_narratives = Vec::new();
        let mut assistant_results = Vec::new();
        let mut session_routes = Vec::new();
        loop {
            match rx.recv_timeout(Duration::from_secs(2)).unwrap() {
                RuntimeUiEnvelope::Effect {
                    turn_id,
                    effect:
                        RuntimeUiEffect::SetStatusSlot {
                            slot: StatusSlot::Workflow,
                            value:
                                StatusValue::WorkflowStep {
                                    workflow_id,
                                    workflow_role,
                                    step_id,
                                    ..
                                },
                        },
                } => {
                    assert_eq!(turn_id, 9);
                    steps.push((workflow_id, workflow_role, step_id));
                }
                RuntimeUiEnvelope::Effect {
                    turn_id,
                    effect:
                        RuntimeUiEffect::SetStatusSlot {
                            slot: StatusSlot::Session,
                            value:
                                StatusValue::SessionRouting {
                                    root_workflow_id,
                                    active_workflow_id,
                                    active_workflow_role,
                                    recognized_scene_id,
                                    selected_workflow_id,
                                },
                        },
                } => {
                    assert_eq!(turn_id, 9);
                    session_routes.push((
                        root_workflow_id,
                        active_workflow_id,
                        active_workflow_role,
                        recognized_scene_id,
                        selected_workflow_id,
                    ));
                }
                RuntimeUiEnvelope::Message { turn_id, message } => {
                    assert_eq!(turn_id, 9);
                    match (message.source, message.kind) {
                        (UiSource::WorkflowStep { step_id, .. }, UiMessageKind::Narrative) => {
                            root_narratives.push(step_id)
                        }
                        (UiSource::Assistant, UiMessageKind::Result) => {
                            assistant_results.push(message.content.as_text().to_string())
                        }
                        _ => {}
                    }
                }
                RuntimeUiEnvelope::Effect {
                    turn_id,
                    effect:
                        RuntimeUiEffect::SetStatusSlot {
                            slot: StatusSlot::Agent,
                            value: StatusValue::Label(label),
                        },
                } => {
                    assert_eq!(turn_id, 9);
                    assert_eq!(label, "Idle");
                    break;
                }
                _ => {}
            }
        }

        assert_eq!(
            steps,
            vec![
                (
                    ROOT_WORKFLOW_ID.to_string(),
                    WorkflowRunRole::Root,
                    SCENE_RECOGNITION_STEP_ID.to_string(),
                ),
                (
                    ROOT_WORKFLOW_ID.to_string(),
                    WorkflowRunRole::Root,
                    SELECT_WORKFLOW_STEP_ID.to_string(),
                ),
                (
                    CHAT_WORKFLOW_ID.to_string(),
                    WorkflowRunRole::Child,
                    CHAT_STEP_ID.to_string(),
                ),
            ]
        );
        assert!(root_narratives.is_empty());
        assert_eq!(assistant_results, vec!["chat answer".to_string()]);
        assert!(session_routes.iter().any(|route| {
            route
                == &(
                    ROOT_WORKFLOW_ID.to_string(),
                    CHAT_WORKFLOW_ID.to_string(),
                    WorkflowRunRole::Child,
                    Some("chat".to_string()),
                    Some(CHAT_WORKFLOW_ID.to_string()),
                )
        }));
        let systems = client.systems.lock().unwrap();
        assert_eq!(systems.len(), 3);
        assert!(
            systems[0]
                .as_deref()
                .is_some_and(|system| system.contains("Visible tools: none"))
        );
        assert!(
            systems[1]
                .as_deref()
                .is_some_and(|system| system.contains("Visible tools: none"))
        );
        assert!(
            systems[2]
                .as_deref()
                .is_some_and(|system| system.contains("Active workflow: chat"))
        );
        assert!(
            systems[2]
                .as_deref()
                .is_some_and(|system| system.contains("Selected workflow: chat."))
        );
        let max_tokens = client.max_tokens.lock().unwrap();
        assert_eq!(*max_tokens, vec![24_000, 24_000, 24_000]);
    }

    #[test]
    fn session_context_persists_step_summaries_across_turns() {
        let client: Arc<SequencedClient> = Arc::new(SequencedClient {
            responses: Mutex::new(vec![
                ChatResponse {
                    id: "scene-1".to_string(),
                    model: Some("test-model".to_string()),
                    content: vec![ContentBlock::text("{\"recognized_scene_id\":\"chat\"}")],
                    stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                    usage: None,
                },
                ChatResponse {
                    id: "select-1".to_string(),
                    model: Some("test-model".to_string()),
                    content: vec![ContentBlock::text("{\"selected_workflow_id\":\"chat\"}")],
                    stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                    usage: None,
                },
                ChatResponse {
                    id: "chat-1".to_string(),
                    model: Some("test-model".to_string()),
                    content: vec![ContentBlock::text("first answer")],
                    stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                    usage: None,
                },
                ChatResponse {
                    id: "scene-2".to_string(),
                    model: Some("test-model".to_string()),
                    content: vec![ContentBlock::text("{\"recognized_scene_id\":\"chat\"}")],
                    stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                    usage: None,
                },
                ChatResponse {
                    id: "select-2".to_string(),
                    model: Some("test-model".to_string()),
                    content: vec![ContentBlock::text("{\"selected_workflow_id\":\"chat\"}")],
                    stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                    usage: None,
                },
                ChatResponse {
                    id: "chat-2".to_string(),
                    model: Some("test-model".to_string()),
                    content: vec![ContentBlock::text("second answer")],
                    stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                    usage: None,
                },
            ]),
            systems: Mutex::new(Vec::new()),
            max_tokens: Mutex::new(Vec::new()),
        });
        let client_dyn: DynLlmClient = client.clone();
        let root = std::env::temp_dir().join("omega-agent-session-context-persistence-test");
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::create_dir_all(&root);
        let skills_dir = root.join(".claude/skills/review");
        let _ = std::fs::create_dir_all(&skills_dir);
        let _ = std::fs::write(
            skills_dir.join("SKILL.md"),
            "---\nname: review\ndescription: Review code\n---\nFind regressions.",
        );
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let loaded_catalog = LoadedWorkflowCatalog::load(&root);
        let session = AgentSession::new(AgentSessionConfig {
            client: client_dyn,
            system: "system".to_string(),
            cwd: root,
            runtime_handle: runtime.handle().clone(),
            scene_catalog: loaded_catalog.scene_catalog,
            workflow_catalog: loaded_catalog.workflow_catalog,
            prompt_catalog: loaded_catalog.prompt_catalog,
            context_window: 200_000,
            max_output_tokens: 24_000,
            bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        })
        .unwrap();

        for (turn_id, input) in [(31, "first question"), (32, "second question")] {
            let (tx, rx) = mpsc::channel();
            session.spawn_turn(input.to_string(), turn_id, tx).unwrap();

            loop {
                if let RuntimeUiEnvelope::Effect {
                    turn_id: observed_turn_id,
                    effect:
                        RuntimeUiEffect::SetStatusSlot {
                            slot: StatusSlot::Agent,
                            value: StatusValue::Label(label),
                        },
                } = rx.recv_timeout(Duration::from_secs(2)).unwrap()
                {
                    assert_eq!(observed_turn_id, turn_id);
                    assert_eq!(label, "Idle");
                    break;
                }
            }
        }

        let systems = client.systems.lock().unwrap();
        assert_eq!(systems.len(), 6);
        assert!(
            systems[3]
                .as_deref()
                .is_some_and(|system| system.contains("second question"))
        );
        assert!(
            systems[3]
                .as_deref()
                .is_some_and(|system| system.contains("first answer"))
        );
        assert!(
            systems[4]
                .as_deref()
                .is_some_and(|system| system.contains("Selected workflow: chat."))
        );
    }

    #[test]
    fn spawn_turn_emits_response_sections_for_routing_and_thinking() {
        let client: Arc<SequencedClient> = Arc::new(SequencedClient {
            responses: Mutex::new(vec![
                ChatResponse {
                    id: "scene-1".to_string(),
                    model: Some("test-model".to_string()),
                    content: vec![ContentBlock::text("{\"recognized_scene_id\":\"chat\"}")],
                    stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                    usage: None,
                },
                ChatResponse {
                    id: "select-1".to_string(),
                    model: Some("test-model".to_string()),
                    content: vec![ContentBlock::text("{\"selected_workflow_id\":\"chat\"}")],
                    stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                    usage: None,
                },
                ChatResponse {
                    id: "chat-1".to_string(),
                    model: Some("test-model".to_string()),
                    content: vec![
                        ContentBlock::Thinking {
                            thinking: "outline answer".to_string(),
                            signature: None,
                        },
                        ContentBlock::text("chat answer"),
                    ],
                    stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                    usage: None,
                },
            ]),
            systems: Mutex::new(Vec::new()),
            max_tokens: Mutex::new(Vec::new()),
        });
        let client_dyn: DynLlmClient = client;
        let root = std::env::temp_dir().join("omega-agent-session-response-section-test");
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::create_dir_all(&root);
        let skills_dir = root.join(".claude/skills/review");
        let _ = std::fs::create_dir_all(&skills_dir);
        let _ = std::fs::write(
            skills_dir.join("SKILL.md"),
            "---\nname: review\ndescription: Review code\n---\nFind regressions.",
        );
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let loaded_catalog = LoadedWorkflowCatalog::load(&root);
        let session = AgentSession::new(AgentSessionConfig {
            client: client_dyn,
            system: "system".to_string(),
            cwd: root,
            runtime_handle: runtime.handle().clone(),
            scene_catalog: loaded_catalog.scene_catalog,
            workflow_catalog: loaded_catalog.workflow_catalog,
            prompt_catalog: loaded_catalog.prompt_catalog,
            context_window: 200_000,
            max_output_tokens: 32_000,
            bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        })
        .unwrap();
        let (tx, rx) = mpsc::channel();
        session.spawn_turn("just chat".to_string(), 11, tx).unwrap();

        let mut began = Vec::new();
        let mut appended = Vec::new();
        let mut completed = Vec::new();
        loop {
            match rx.recv_timeout(Duration::from_secs(2)).unwrap() {
                RuntimeUiEnvelope::Effect {
                    turn_id,
                    effect: RuntimeUiEffect::BeginResponseSection { section },
                } => {
                    assert_eq!(turn_id, 11);
                    began.push((
                        section.id,
                        section.parent_id,
                        section.kind,
                        section.title,
                        section.metadata.workflow_id,
                        section.metadata.workflow_role,
                        section.metadata.scene_id,
                    ));
                }
                RuntimeUiEnvelope::Effect {
                    turn_id,
                    effect: RuntimeUiEffect::AppendResponseSection { id, delta },
                } => {
                    assert_eq!(turn_id, 11);
                    appended.push((id, delta));
                }
                RuntimeUiEnvelope::Effect {
                    turn_id,
                    effect: RuntimeUiEffect::CompleteResponseSection { id, state },
                } => {
                    assert_eq!(turn_id, 11);
                    completed.push((id, state));
                }
                RuntimeUiEnvelope::Effect {
                    turn_id,
                    effect:
                        RuntimeUiEffect::SetStatusSlot {
                            slot: StatusSlot::Agent,
                            value: StatusValue::Label(label),
                        },
                } => {
                    assert_eq!(turn_id, 11);
                    assert_eq!(label, "Idle");
                    break;
                }
                _ => {}
            }
        }

        assert!(began.iter().any(|entry| {
            entry
                == &(
                    "turn-11:root:root:scene-recognition".to_string(),
                    None,
                    ResponseSectionKind::Routing,
                    "Scene Recognition".to_string(),
                    ROOT_WORKFLOW_ID.to_string(),
                    WorkflowRunRole::Root,
                    None,
                )
        }));
        assert!(began.iter().any(|entry| {
            entry
                == &(
                    "turn-11:root:root:select-workflow".to_string(),
                    None,
                    ResponseSectionKind::Routing,
                    "Select Workflow".to_string(),
                    ROOT_WORKFLOW_ID.to_string(),
                    WorkflowRunRole::Root,
                    Some("chat".to_string()),
                )
        }));
        assert!(began.iter().any(|entry| {
            entry
                == &(
                    "turn-11:child:chat:chat".to_string(),
                    None,
                    ResponseSectionKind::FinalAnswer,
                    "Final Answer".to_string(),
                    CHAT_WORKFLOW_ID.to_string(),
                    WorkflowRunRole::Child,
                    Some("chat".to_string()),
                )
        }));
        assert!(began.iter().any(|entry| {
            entry
                == &(
                    "turn-11:child:chat:chat:thinking".to_string(),
                    Some("turn-11:child:chat:chat".to_string()),
                    ResponseSectionKind::Thinking,
                    "Thinking".to_string(),
                    CHAT_WORKFLOW_ID.to_string(),
                    WorkflowRunRole::Child,
                    Some("chat".to_string()),
                )
        }));
        assert!(appended.iter().any(|entry| {
            entry
                == &(
                    "turn-11:child:chat:chat:thinking".to_string(),
                    ResponseSectionDelta::Text("outline answer".to_string()),
                )
        }));
        assert!(appended.iter().any(|entry| {
            entry
                == &(
                    "turn-11:child:chat:chat".to_string(),
                    ResponseSectionDelta::Text("chat answer".to_string()),
                )
        }));
        assert!(completed.iter().any(|entry| {
            entry
                == &(
                    "turn-11:child:chat:chat:thinking".to_string(),
                    ResponseSectionState::Complete,
                )
        }));
        assert!(completed.iter().any(|entry| {
            entry
                == &(
                    "turn-11:child:chat:chat".to_string(),
                    ResponseSectionState::Complete,
                )
        }));
    }

    #[test]
    fn spawn_turn_falls_back_to_text_routing_when_root_json_validation_fails() {
        let client: Arc<SequencedClient> = Arc::new(SequencedClient {
            responses: Mutex::new(vec![
                ChatResponse {
                    id: "scene-1".to_string(),
                    model: Some("test-model".to_string()),
                    content: vec![ContentBlock::text("This request fits the chat scene.")],
                    stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                    usage: None,
                },
                ChatResponse {
                    id: "scene-2".to_string(),
                    model: Some("test-model".to_string()),
                    content: vec![ContentBlock::text("I still think this belongs to chat.")],
                    stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                    usage: None,
                },
                ChatResponse {
                    id: "select-1".to_string(),
                    model: Some("test-model".to_string()),
                    content: vec![ContentBlock::text("Use the chat workflow.")],
                    stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                    usage: None,
                },
                ChatResponse {
                    id: "select-2".to_string(),
                    model: Some("test-model".to_string()),
                    content: vec![ContentBlock::text("chat is the right workflow here.")],
                    stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                    usage: None,
                },
                ChatResponse {
                    id: "chat-1".to_string(),
                    model: Some("test-model".to_string()),
                    content: vec![ContentBlock::text("chat answer")],
                    stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                    usage: None,
                },
            ]),
            systems: Mutex::new(Vec::new()),
            max_tokens: Mutex::new(Vec::new()),
        });
        let client_dyn: DynLlmClient = client;
        let root = std::env::temp_dir().join("omega-agent-session-root-routing-fallback-test");
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::create_dir_all(&root);
        let skills_dir = root.join(".claude/skills/review");
        let _ = std::fs::create_dir_all(&skills_dir);
        let _ = std::fs::write(
            skills_dir.join("SKILL.md"),
            "---\nname: review\ndescription: Review code\n---\nFind regressions.",
        );
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let loaded_catalog = LoadedWorkflowCatalog::load(&root);
        let session = AgentSession::new(AgentSessionConfig {
            client: client_dyn,
            system: "system".to_string(),
            cwd: root,
            runtime_handle: runtime.handle().clone(),
            scene_catalog: loaded_catalog.scene_catalog,
            workflow_catalog: loaded_catalog.workflow_catalog,
            prompt_catalog: loaded_catalog.prompt_catalog,
            context_window: 200_000,
            max_output_tokens: 32_000,
            bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        })
        .unwrap();
        let (tx, rx) = mpsc::channel();

        session
            .spawn_turn("分析下这个项目的优缺点".to_string(), 73, tx)
            .unwrap();

        let mut diagnostics = Vec::new();
        let mut began = Vec::new();
        loop {
            match rx.recv_timeout(Duration::from_secs(2)).unwrap() {
                RuntimeUiEnvelope::Effect {
                    turn_id,
                    effect:
                        RuntimeUiEffect::UpsertStepDiagnostics {
                            diagnostics: update,
                        },
                } => {
                    assert_eq!(turn_id, 73);
                    diagnostics.push(*update);
                }
                RuntimeUiEnvelope::Effect {
                    turn_id,
                    effect: RuntimeUiEffect::BeginResponseSection { section },
                } => {
                    assert_eq!(turn_id, 73);
                    began.push((section.id, section.metadata.workflow_id, section.kind));
                }
                RuntimeUiEnvelope::Effect {
                    turn_id,
                    effect:
                        RuntimeUiEffect::SetStatusSlot {
                            slot: StatusSlot::Agent,
                            value: StatusValue::Label(label),
                        },
                } => {
                    assert_eq!(turn_id, 73);
                    assert_eq!(label, "Idle");
                    break;
                }
                _ => {}
            }
        }

        assert!(began.iter().any(|entry| {
            entry
                == &(
                    "turn-73:child:chat:chat".to_string(),
                    CHAT_WORKFLOW_ID.to_string(),
                    ResponseSectionKind::FinalAnswer,
                )
        }));
        assert!(diagnostics.iter().any(|diagnostics| {
            diagnostics.step_id == SCENE_RECOGNITION_STEP_ID
                && diagnostics.output.status == StepOutputStatus::Invalid
        }));
        assert!(diagnostics.iter().any(|diagnostics| {
            diagnostics.step_id == SELECT_WORKFLOW_STEP_ID
                && diagnostics.output.status == StepOutputStatus::Invalid
        }));
    }

    #[test]
    fn spawn_turn_accepts_root_json_when_model_adds_short_preface() {
        let client: Arc<SequencedClient> = Arc::new(SequencedClient {
            responses: Mutex::new(vec![
                ChatResponse {
                    id: "scene-1".to_string(),
                    model: Some("test-model".to_string()),
                    content: vec![ContentBlock::text(
                        "Best match is feature.\n{\"recognized_scene_id\":\"feature\"}",
                    )],
                    stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                    usage: None,
                },
                ChatResponse {
                    id: "select-1".to_string(),
                    model: Some("test-model".to_string()),
                    content: vec![ContentBlock::text(
                        "Use the feature workflow.\n{\"selected_workflow_id\":\"feature\"}",
                    )],
                    stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                    usage: None,
                },
                ChatResponse {
                    id: "analysis-1".to_string(),
                    model: Some("test-model".to_string()),
                    content: vec![ContentBlock::text(feature_analysis_json())],
                    stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                    usage: None,
                },
                ChatResponse {
                    id: "plan-1".to_string(),
                    model: Some("test-model".to_string()),
                    content: vec![ContentBlock::text(feature_plan_json())],
                    stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                    usage: None,
                },
                ChatResponse {
                    id: "execute-1".to_string(),
                    model: Some("test-model".to_string()),
                    content: vec![ContentBlock::text("execution complete")],
                    stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                    usage: None,
                },
                ChatResponse {
                    id: "report-1".to_string(),
                    model: Some("test-model".to_string()),
                    content: vec![ContentBlock::text("done")],
                    stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                    usage: None,
                },
            ]),
            systems: Mutex::new(Vec::new()),
            max_tokens: Mutex::new(Vec::new()),
        });
        let client_dyn: DynLlmClient = client;
        let root = std::env::temp_dir().join("omega-agent-session-root-embedded-json-test");
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::create_dir_all(&root);
        let skills_dir = root.join(".claude/skills/review");
        let _ = std::fs::create_dir_all(&skills_dir);
        let _ = std::fs::write(
            skills_dir.join("SKILL.md"),
            "---\nname: review\ndescription: Review code\n---\nFind regressions.",
        );
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let loaded_catalog = LoadedWorkflowCatalog::load(&root);
        let session = AgentSession::new(AgentSessionConfig {
            client: client_dyn,
            system: "system".to_string(),
            cwd: root,
            runtime_handle: runtime.handle().clone(),
            scene_catalog: loaded_catalog.scene_catalog,
            workflow_catalog: loaded_catalog.workflow_catalog,
            prompt_catalog: loaded_catalog.prompt_catalog,
            context_window: 200_000,
            max_output_tokens: 32_000,
            bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        })
        .unwrap();
        let (tx, rx) = mpsc::channel();

        session
            .spawn_turn("fix this bug".to_string(), 71, tx)
            .unwrap();

        let mut warnings = Vec::new();
        let mut diagnostics = Vec::new();
        loop {
            match rx.recv_timeout(Duration::from_secs(2)).unwrap() {
                RuntimeUiEnvelope::Message { turn_id, message }
                    if turn_id == 71
                        && matches!(message.source, UiSource::System)
                        && message.kind == UiMessageKind::Warning =>
                {
                    warnings.push(message.content.as_text().to_string());
                }
                RuntimeUiEnvelope::Effect {
                    turn_id,
                    effect:
                        RuntimeUiEffect::UpsertStepDiagnostics {
                            diagnostics: update,
                        },
                } => {
                    assert_eq!(turn_id, 71);
                    diagnostics.push(*update);
                }
                RuntimeUiEnvelope::Effect {
                    turn_id,
                    effect:
                        RuntimeUiEffect::SetStatusSlot {
                            slot: StatusSlot::Agent,
                            value: StatusValue::Label(label),
                        },
                } => {
                    assert_eq!(turn_id, 71);
                    assert_eq!(label, "Idle");
                    break;
                }
                _ => {}
            }
        }

        assert!(!warnings.iter().any(|warning| {
            warning.contains("scene-recognition") || warning.contains("select-workflow")
        }));
        assert!(diagnostics.iter().any(|diagnostics| {
            diagnostics.step_id == SCENE_RECOGNITION_STEP_ID
                && diagnostics.output.status == StepOutputStatus::Valid
                && diagnostics.output.retry_count == 0
        }));
        assert!(diagnostics.iter().any(|diagnostics| {
            diagnostics.step_id == SELECT_WORKFLOW_STEP_ID
                && diagnostics.output.status == StepOutputStatus::Valid
                && diagnostics.output.retry_count == 0
        }));
    }

    #[test]
    fn spawn_turn_emits_tool_runs_and_sanitizes_provider_markup() {
        let client: Arc<SequencedClient> = Arc::new(SequencedClient {
            responses: Mutex::new(vec![
                ChatResponse {
                    id: "scene-1".to_string(),
                    model: Some("test-model".to_string()),
                    content: vec![ContentBlock::text("{\"recognized_scene_id\":\"feature\"}")],
                    stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                    usage: None,
                },
                ChatResponse {
                    id: "select-1".to_string(),
                    model: Some("test-model".to_string()),
                    content: vec![ContentBlock::text("{\"selected_workflow_id\":\"feature\"}")],
                    stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                    usage: None,
                },
                ChatResponse {
                    id: "analysis-1".to_string(),
                    model: Some("test-model".to_string()),
                    content: vec![ContentBlock::text(feature_analysis_json())],
                    stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                    usage: None,
                },
                ChatResponse {
                    id: "plan-1".to_string(),
                    model: Some("test-model".to_string()),
                    content: vec![ContentBlock::text(feature_plan_json())],
                    stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                    usage: None,
                },
                ChatResponse {
                    id: "execute-1".to_string(),
                    model: Some("test-model".to_string()),
                    content: vec![
                        ContentBlock::Thinking {
                            thinking: "thinking <minimax:tool_call><invoke name=\"bash\">ignored</invoke></minimax:tool_call> done".to_string(),
                            signature: None,
                        },
                        ContentBlock::text(
                            "before <invoke name=\"bash\">ignored</invoke> after",
                        ),
                        ContentBlock::tool_use(
                            "tool-1",
                            "bash",
                            serde_json::json!({"command": "echo hi"}),
                        ),
                    ],
                    stop_reason: Some(STOP_REASON_TOOL_USE.to_string()),
                    usage: None,
                },
                ChatResponse {
                    id: "execute-2".to_string(),
                    model: Some("test-model".to_string()),
                    content: vec![ContentBlock::text("execution complete")],
                    stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                    usage: None,
                },
                ChatResponse {
                    id: "report-1".to_string(),
                    model: Some("test-model".to_string()),
                    content: vec![ContentBlock::text("done")],
                    stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                    usage: None,
                },
            ]),
            systems: Mutex::new(Vec::new()),
            max_tokens: Mutex::new(Vec::new()),
        });
        let client_dyn: DynLlmClient = client;
        let root = std::env::temp_dir().join("omega-agent-session-tool-run-test");
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::create_dir_all(&root);
        let skills_dir = root.join(".claude/skills/review");
        let _ = std::fs::create_dir_all(&skills_dir);
        let _ = std::fs::write(
            skills_dir.join("SKILL.md"),
            "---\nname: review\ndescription: Review code\n---\nFind regressions.",
        );
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let loaded_catalog = LoadedWorkflowCatalog::load(&root);
        let session = AgentSession::new(AgentSessionConfig {
            client: client_dyn,
            system: "system".to_string(),
            cwd: root,
            runtime_handle: runtime.handle().clone(),
            scene_catalog: loaded_catalog.scene_catalog,
            workflow_catalog: loaded_catalog.workflow_catalog,
            prompt_catalog: loaded_catalog.prompt_catalog,
            context_window: 200_000,
            max_output_tokens: 32_000,
            bash_allowed_commands: omega_core::default_bash_allowed_commands(),
        })
        .unwrap();
        let (tx, rx) = mpsc::channel();

        session.spawn_turn("hello".to_string(), 12, tx).unwrap();

        let mut began_runs = Vec::new();
        let mut updated_runs = Vec::new();
        let mut completed_runs = Vec::new();
        let mut append_deltas = Vec::new();
        let mut tool_logs = Vec::new();
        loop {
            match rx.recv_timeout(Duration::from_secs(2)).unwrap() {
                RuntimeUiEnvelope::Effect {
                    turn_id,
                    effect: RuntimeUiEffect::BeginToolRun { tool_run },
                } => {
                    assert_eq!(turn_id, 12);
                    began_runs.push(tool_run);
                }
                RuntimeUiEnvelope::Effect {
                    turn_id,
                    effect: RuntimeUiEffect::UpdateToolRun { tool_run },
                } => {
                    assert_eq!(turn_id, 12);
                    updated_runs.push(tool_run);
                }
                RuntimeUiEnvelope::Effect {
                    turn_id,
                    effect: RuntimeUiEffect::CompleteToolRun { id, status },
                } => {
                    assert_eq!(turn_id, 12);
                    completed_runs.push((id, status));
                }
                RuntimeUiEnvelope::Effect {
                    turn_id,
                    effect: RuntimeUiEffect::AppendResponseSection { id, delta },
                } => {
                    assert_eq!(turn_id, 12);
                    append_deltas.push((id, delta));
                }
                RuntimeUiEnvelope::Message { turn_id, message }
                    if matches!(message.source, UiSource::Tool { .. })
                        && message.kind == UiMessageKind::Log =>
                {
                    assert_eq!(turn_id, 12);
                    tool_logs.push(message.content.as_text().to_string());
                }
                RuntimeUiEnvelope::Effect {
                    turn_id,
                    effect:
                        RuntimeUiEffect::SetStatusSlot {
                            slot: StatusSlot::Agent,
                            value: StatusValue::Label(label),
                        },
                } => {
                    assert_eq!(turn_id, 12);
                    assert_eq!(label, "Idle");
                    break;
                }
                _ => {}
            }
        }

        assert_eq!(began_runs.len(), 1);
        assert_eq!(began_runs[0].id, "tool-1");
        assert_eq!(
            began_runs[0].parent_section_id,
            "turn-12:child:feature:execute"
        );
        assert_eq!(began_runs[0].tool_name, "bash");
        assert_eq!(began_runs[0].status, ToolRunStatus::Running);
        assert_eq!(began_runs[0].invocation_preview, "$ echo hi");
        assert!(began_runs[0].result_preview.is_none());

        assert_eq!(updated_runs.len(), 1);
        assert_eq!(updated_runs[0].id, "tool-1");
        assert_eq!(updated_runs[0].status, ToolRunStatus::Complete);
        assert!(
            updated_runs[0]
                .result_preview
                .as_deref()
                .is_some_and(|preview| preview.contains("hi"))
        );

        assert_eq!(
            completed_runs,
            vec![("tool-1".to_string(), ToolRunStatus::Complete)]
        );

        assert!(tool_logs.iter().any(|line| line == "$ echo hi"));
        assert!(tool_logs.iter().any(|line| line.contains("hi")));

        let sanitized_text = append_deltas
            .iter()
            .filter_map(|(id, delta)| match delta {
                ResponseSectionDelta::Text(text)
                    if id == "turn-12:child:feature:execute"
                        || id == "turn-12:child:feature:execute:thinking" =>
                {
                    Some(text.as_str())
                }
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");

        assert!(sanitized_text.contains("before "));
        assert!(sanitized_text.contains(" after"));
        assert!(sanitized_text.contains("thinking "));
        assert!(sanitized_text.contains(" done"));
        assert!(!sanitized_text.contains("<minimax:tool_call"));
        assert!(!sanitized_text.contains("<invoke"));
    }

    #[test]
    fn session_tool_catalog_matches_current_default_tool_set() {
        let dispatcher = omega_core::create_default_tools(std::env::temp_dir());
        let catalog = SessionToolCatalog::new(
            dispatcher
                .tool_names()
                .into_iter()
                .map(ToOwned::to_owned)
                .collect(),
        );

        let inherit = catalog.resolve_for_step(&StepToolRequest::Inherit);
        let blocked = catalog.resolve_for_step(&StepToolRequest::Block(vec![
            "bash".to_string(),
            "read_file".to_string(),
        ]));

        assert_eq!(
            inherit.tool_names(),
            [
                "bash",
                "edit_file",
                "load_skill",
                "read_file",
                "todo",
                "write_file"
            ]
        );
        assert_eq!(
            blocked.tool_names(),
            ["edit_file", "load_skill", "todo", "write_file"]
        );
    }

    #[test]
    fn session_skill_catalog_preserves_existing_prompt_shape() {
        let root = std::env::temp_dir().join("omega-agent-session-skill-catalog-test");
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::create_dir_all(&root);
        let review = root.join(".claude/skills/review");
        let docs = root.join(".claude/skills/docs");
        let _ = std::fs::create_dir_all(&review);
        let _ = std::fs::create_dir_all(&docs);
        let _ = std::fs::write(
            review.join("SKILL.md"),
            "---\nname: review\ndescription: Review code\n---\nFind regressions.",
        );
        let _ = std::fs::write(
            docs.join("SKILL.md"),
            "---\nname: docs-specs\ndescription: Technical specs\n---\nBe precise.",
        );

        let loader = omega_skills::SkillLoader::from_repo_root(&root).unwrap();
        let catalog = SessionSkillCatalog::new(loader);
        let prompt = catalog.build_system_prompt(
            "Base prompt",
            "Please review this patch",
            &StepSkillRequest::Append(vec!["docs-specs".to_string()]),
        );

        assert!(prompt.contains("Skills available:"));
        assert!(prompt.contains("review: Review code"));
        assert!(prompt.contains("Preloaded skills for this task:"));
        assert!(prompt.contains("<skill name=\"review\">"));
        assert!(prompt.contains("<skill name=\"docs-specs\">"));
    }
}
