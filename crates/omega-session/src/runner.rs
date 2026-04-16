use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use omega_compression::{
    LedgerSessionContextCompressor, SessionContextCompressor, SessionContextLoadGoal,
    SessionContextLoadRequest, session_context_budget_tokens,
};
use omega_context::{
    ContextCacheDiagnostics, ContextDiagnostics, ContextExecuteItem, ContextRouting,
    ContextSession, ContextStep, ContextStepSummary, ContextSupervisionSnapshot,
    ContextTokenCountSource, ContextTokenCounter, ContextWorkflowRole, DocumentHitItem,
    DocumentHitSummary, DocumentSupervisionSnapshot, DocumentSupervisionTotals, HealthScore,
    MemoryHitItem, MemoryHitSummary, MemorySupervisionSnapshot, MemorySupervisionTotals,
    ObservationRecallHitItem, OmegaContextFacade, OutputRepairContextRequest,
    OutputRepairFailure, ResponseDocumentKnowledge, ResponseMemoryKnowledge, SearchMode,
    SearchResult, StepContextRequest, StepKnowledgeSummary, SupervisionReadiness,
};
use omega_core::{
    Agent, ChatRequest, CoreSharedTodoManager, CoreToolExecutionContext, CoreToolResult,
    DynLlmClient, Message, TodoItem, TodoStatus,
};
use omega_hooks::{HookAdvanceOutcome, HookHost};
use omega_project::{ProjectRegistry, ProjectResolutionInput, SessionContextRecordKind};
use omega_workflow::{
    DataFormat, OutputRecoveryMode, SceneCatalog, StepInputContract, StepLoopContract,
    StepOutputContract, WorkflowCatalog, WorkflowPromptCatalog, WorkflowPrompts, WorkflowStep,
    DEEP_RESEARCH_SCENE_ID, DEEP_RESEARCH_WORKFLOW_ID, EXECUTE_STEP_ID, FEATURE_SCENE_ID,
    FEATURE_WORKFLOW_ID, PLAN_STEP_ID, RESEARCH_SCENE_ID, RESEARCH_WORKFLOW_ID,
    ROOT_WORKFLOW_ID, SCENE_RECOGNITION_STEP_ID, SELECT_SKILLS_STEP_ID,
    SELECT_WORKFLOW_STEP_ID,
};
use serde_json::Value;
use tokio::runtime::Handle;
use tokio::sync::watch;
use tracing::{debug, error, info};

use crate::hook_adapter::{ExecuteLoopItemContext, StepHookRuntime};
use crate::output::{
    build_output_validation_feedback, parse_feature_execute_output, parse_feature_plan_output,
    parse_structured_output_candidates, validate_schema_file, validate_workflow_step_output,
};
use crate::routing::{
    find_catalog_match, latest_user_turn_prefers_deep_research_scene,
    latest_user_turn_prefers_research_scene,
    latest_user_turn_requires_feature_scene, parse_structured_id, parse_structured_id_from_value,
};
use crate::session_state::{SessionContext, SkillRoutingContext, StepSummary};
use crate::skill_catalog::normalize_skill_ids;
use crate::ui_emit::{
    maybe_emit_context_observability, send_routing_log, send_session_status,
    send_step_subflow_status, send_step_text, send_system_log_text, send_todo_snapshot,
    send_warning_text, send_workflow_step, StepResponseStreamer, ToolRunTracker,
};
use crate::{
    CacheDiagnostics, ExecuteProgressDiagnostics, ResolvedSkillSet, ResolvedToolSet,
    RuntimeMessageBridge, SessionSkillCatalog, SessionToolCatalog, SharedRuntimeMessageBridge,
    StepContextWrite, StepContextWriteKind, StepDiagnostics, StepInputDiagnostics, StepInputStatus,
    StepOutputAttemptKind, StepOutputContractMode, StepOutputDiagnostics,
    StepOutputRecoveryDecision, StepOutputStatus, StepSubflowState, StepSubflowStatus,
    StepSummarySource, TokenCountSource, ToolCapabilityDiagnostics, WorkflowRunRole,
    CONTEXT_SAFETY_MARGIN_TOKENS,
    REPAIR_PASS_MAX_ITERATIONS, SUMMARY_CHAR_LIMIT, TOKEN_ESTIMATE_DIVISOR,
};

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct StepExecutionInput {
    pub(crate) base_system: String,
    pub(crate) cwd: PathBuf,
    pub(crate) resolved_tools: ResolvedToolSet,
    pub(crate) resolved_skills: ResolvedSkillSet,
    pub(crate) system_blocks: Vec<omega_core::SystemBlock>,
    pub(crate) document_hits: Option<DocumentHitSummary>,
    pub(crate) context_diagnostics: ContextDiagnostics,
    pub(crate) cache_diagnostics: CacheDiagnostics,
    pub(crate) session_context: SessionContext,
    pub(crate) structured_input: Option<Value>,
    pub(crate) todo_snapshot: Option<String>,
    pub(crate) current_execute_item: Option<ExecuteLoopItemContext>,
    pub(crate) step: WorkflowStep,
    pub(crate) step_prompt: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StepExecutionResult {
    final_text: String,
    structured_output: Option<Value>,
    summary: StepSummary,
    session_writes: Vec<StepContextWrite>,
    transition: StepTransition,
}

#[derive(Debug, Clone)]
struct StepRunOutput {
    stage_text: String,
    usage: Option<omega_core::Usage>,
    tool_capabilities: Option<ToolCapabilityDiagnostics>,
    tool_runs: Vec<crate::ToolRun>,
    model_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TurnDeliveryChangedFile {
    pub(crate) path: String,
    pub(crate) kind: TurnDeliveryFileChangeKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TurnDeliveryFileChangeKind {
    Create,
    Update,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct TurnDeliverySummary {
    pub(crate) primary_model: Option<String>,
    pub(crate) llm_request_count: u32,
    pub(crate) input_tokens: u32,
    pub(crate) output_tokens: u32,
    pub(crate) cache_creation_input_tokens: u32,
    pub(crate) cache_read_input_tokens: u32,
    pub(crate) tool_call_count: usize,
    pub(crate) failed_tool_count: usize,
    pub(crate) tool_counts: BTreeMap<String, u32>,
    pub(crate) recognized_skill_ids: Vec<String>,
    pub(crate) loaded_skill_ids: Vec<String>,
    pub(crate) ignored_skill_ids: Vec<String>,
    pub(crate) changed_files: Vec<TurnDeliveryChangedFile>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TurnRunOutput {
    pub(crate) final_text: String,
    pub(crate) delivery_summary: TurnDeliverySummary,
}

#[derive(Debug, Default)]
struct TurnDeliveryAccumulator {
    primary_model: Option<String>,
    llm_request_count: u32,
    input_tokens: u32,
    output_tokens: u32,
    cache_creation_input_tokens: u32,
    cache_read_input_tokens: u32,
    tool_counts: BTreeMap<String, u32>,
    failed_tool_count: usize,
    changed_files: BTreeMap<String, TurnDeliveryFileChangeKind>,
}

impl TurnDeliveryAccumulator {
    fn observe_step_run(&mut self, step_run: &StepRunOutput) {
        self.llm_request_count += 1;
        if self.primary_model.is_none() {
            self.primary_model = step_run.model_name.clone();
        }
        if let Some(usage) = step_run.usage.as_ref() {
            self.input_tokens = self.input_tokens.saturating_add(usage.input_tokens);
            self.output_tokens = self.output_tokens.saturating_add(usage.output_tokens);
            self.cache_creation_input_tokens = self
                .cache_creation_input_tokens
                .saturating_add(usage.cache_creation_input_tokens.unwrap_or(0));
            self.cache_read_input_tokens = self
                .cache_read_input_tokens
                .saturating_add(usage.cache_read_input_tokens.unwrap_or(0));
        }
        for tool_run in &step_run.tool_runs {
            *self.tool_counts.entry(tool_run.tool_name.clone()).or_insert(0) += 1;
            if tool_run.status == crate::ToolRunStatus::Failed {
                self.failed_tool_count += 1;
            }
            if let Some(kind) = delivery_change_kind(&tool_run.tool_name) {
                let path = tool_run.invocation_preview.trim();
                if !path.is_empty() {
                    self.changed_files
                        .entry(path.to_string())
                        .and_modify(|existing| {
                            if kind == TurnDeliveryFileChangeKind::Create {
                                *existing = kind;
                            }
                        })
                        .or_insert(kind);
                }
            }
        }
    }

    fn finish(self, session_context: &SessionContext) -> TurnDeliverySummary {
        TurnDeliverySummary {
            primary_model: self.primary_model,
            llm_request_count: self.llm_request_count,
            input_tokens: self.input_tokens,
            output_tokens: self.output_tokens,
            cache_creation_input_tokens: self.cache_creation_input_tokens,
            cache_read_input_tokens: self.cache_read_input_tokens,
            tool_call_count: self.tool_counts.values().copied().map(|count| count as usize).sum(),
            failed_tool_count: self.failed_tool_count,
            tool_counts: self.tool_counts,
            recognized_skill_ids: session_context.skill_routing.selected_skill_ids.clone(),
            loaded_skill_ids: session_context.skill_routing.loaded_skill_ids.clone(),
            ignored_skill_ids: session_context.skill_routing.ignored_skill_ids.clone(),
            changed_files: self
                .changed_files
                .into_iter()
                .map(|(path, kind)| TurnDeliveryChangedFile { path, kind })
                .collect(),
        }
    }
}

fn delivery_change_kind(tool_name: &str) -> Option<TurnDeliveryFileChangeKind> {
    match tool_name {
        "create_file" => Some(TurnDeliveryFileChangeKind::Create),
        "apply_patch" | "edit_file" | "write_file" => Some(TurnDeliveryFileChangeKind::Update),
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RecallRewriteDecision {
    reason: String,
    queries: Vec<String>,
    recovery_path: String,
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
struct ExecuteLoopProgressState {
    current_item: Option<ExecuteLoopItemContext>,
    repeat_count: u32,
    completion_source: Option<String>,
}

#[derive(Debug, Clone)]
struct OutputDiagnosticState<'a> {
    status: StepOutputStatus,
    attempt_kind: StepOutputAttemptKind,
    structured_output: Option<&'a Value>,
    usage: Option<&'a omega_core::Usage>,
    attempts: u32,
    retry_count: u32,
    max_retries: u32,
    validation_error: Option<&'a str>,
    previous_response_preview: Option<&'a str>,
    recovery_decision: Option<StepOutputRecoveryDecision>,
    session_writes: Vec<StepContextWrite>,
}

#[derive(Debug, Clone, Default)]
struct ContextSupervisionState {
    document_hits: Option<DocumentHitSummary>,
    memory_hits: Option<MemoryHitSummary>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum SlotPriority {
    Medium,
    Low,
}

#[derive(Debug, Clone)]
struct SummaryCandidate {
    summary: StepSummary,
    original_index: usize,
    priority: SlotPriority,
    score: u32,
    compacted: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OutputValidationErrorKind {
    ExtractFailed,
    SchemaInvalid,
    SemanticInvalid,
}

impl OutputValidationErrorKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::ExtractFailed => "extract_failed",
            Self::SchemaInvalid => "schema_invalid",
            Self::SemanticInvalid => "semantic_invalid",
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct OutputValidationFailure {
    pub(crate) error_kind: OutputValidationErrorKind,
    pub(crate) message: String,
    pub(crate) previous_response_preview: String,
    pub(crate) extracted_json: Option<Value>,
}

impl OutputValidationFailure {
    fn new(
        error_kind: OutputValidationErrorKind,
        message: impl Into<String>,
        previous_response: &str,
        extracted_json: Option<Value>,
    ) -> Self {
        Self {
            error_kind,
            message: message.into(),
            previous_response_preview: crate::preview_text(previous_response.trim(), 600),
            extracted_json,
        }
    }

    fn extracted_json_preview(&self) -> Option<String> {
        self.extracted_json
            .as_ref()
            .map(|value| crate::preview_json_value(value, 300))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum StepTransition {
    Continue,
    Repeat,
    RepeatItem,
    StartWorkflow { workflow_id: String },
    FinishTurn,
    Error { message: String },
}

pub(crate) struct WorkflowTurnRunner<'a> {
    handle: &'a Handle,
    client: &'a DynLlmClient,
    context_facade: &'a Arc<OmegaContextFacade>,
    skill_catalog: &'a Arc<SessionSkillCatalog>,
    tool_catalog: &'a Arc<SessionToolCatalog>,
    base_system: &'a str,
    session_id: &'a str,
    input: &'a str,
    cwd: &'a PathBuf,
    todo_manager: &'a CoreSharedTodoManager,
    hook_host: &'a Arc<HookHost>,
    scene_catalog: &'a SceneCatalog,
    workflow_catalog: &'a WorkflowCatalog,
    prompt_catalog: &'a WorkflowPromptCatalog,
    context_window: u32,
    max_output_tokens: u32,
    turn_id: u64,
    cancel_turn_rx: watch::Receiver<u64>,
    tx_callback: SharedRuntimeMessageBridge,
    tx_result: SharedRuntimeMessageBridge,
    supervision_state: Arc<Mutex<ContextSupervisionState>>,
}

const CONTEXT_COMPACTION_THRESHOLD_PERCENT: u32 = 70;
const MAX_UNCOMPACTED_SUMMARIES: usize = 5;
const COMPACTED_SUMMARY_CHAR_LIMIT: usize = 480;
const AGGRESSIVE_COMPACTED_SUMMARY_CHAR_LIMIT: usize = 240;

impl<'a> WorkflowTurnRunner<'a> {
    pub(crate) fn new(
        handle: &'a Handle,
        client: &'a DynLlmClient,
        context_facade: &'a Arc<OmegaContextFacade>,
        skill_catalog: &'a Arc<SessionSkillCatalog>,
        tool_catalog: &'a Arc<SessionToolCatalog>,
        base_system: &'a str,
        session_id: &'a str,
        input: &'a str,
        cwd: &'a PathBuf,
        todo_manager: &'a CoreSharedTodoManager,
        hook_host: &'a Arc<HookHost>,
        scene_catalog: &'a SceneCatalog,
        workflow_catalog: &'a WorkflowCatalog,
        prompt_catalog: &'a WorkflowPromptCatalog,
        context_window: u32,
        max_output_tokens: u32,
        turn_id: u64,
        cancel_turn_rx: watch::Receiver<u64>,
        tx_callback: SharedRuntimeMessageBridge,
        tx_result: SharedRuntimeMessageBridge,
    ) -> Self {
        Self {
            handle,
            client,
            context_facade,
            skill_catalog,
            tool_catalog,
            base_system,
            session_id,
            input,
            cwd,
            todo_manager,
            hook_host,
            scene_catalog,
            workflow_catalog,
            prompt_catalog,
            context_window,
            max_output_tokens,
            turn_id,
            cancel_turn_rx,
            tx_callback,
            tx_result,
            supervision_state: Arc::new(Mutex::new(ContextSupervisionState::default())),
        }
    }

    pub(crate) fn run(
        &self,
        agent: &mut Agent,
        session_context: &mut SessionContext,
    ) -> anyhow::Result<TurnRunOutput> {
        self.ensure_turn_active()?;
        let hook_session = Arc::new(Mutex::new(self.hook_host.start_session()));
        let mut delivery_accumulator = TurnDeliveryAccumulator::default();
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
            hook_session.clone(),
            &mut delivery_accumulator,
        )?;

        self.ensure_turn_active()?;
        self.apply_routed_skill_load_action(session_context);
        self.ensure_turn_active()?;

        let selected_workflow_id = self.ensure_selected_workflow(session_context);
        self.send_routing_log(format!(
            "Delegating to child workflow '{}'.",
            selected_workflow_id
        ));

        let final_text = self.run_workflow(
            agent,
            &selected_workflow_id,
            WorkflowRunRole::Child,
            session_context,
            hook_session,
            &mut delivery_accumulator,
        )?;

		Ok(TurnRunOutput {
			final_text,
			delivery_summary: delivery_accumulator.finish(session_context),
		})
    }

    fn run_workflow(
        &self,
        agent: &mut Agent,
        workflow_id: &str,
        role: WorkflowRunRole,
        session_context: &mut SessionContext,
        hook_session: Arc<Mutex<omega_hooks::HookSession>>,
        delivery_accumulator: &mut TurnDeliveryAccumulator,
    ) -> anyhow::Result<String> {
        self.update_active_workflow(session_context, workflow_id.to_string(), role);
        let (definition, prompts) = self.resolve_workflow_bundle(workflow_id)?;
        let mut last_text = String::new();
        let mut step_repeat_counts: BTreeMap<String, u32> = BTreeMap::new();
        let mut item_repeat_counts: BTreeMap<String, u32> = BTreeMap::new();
        let mut run = definition.start_run();

        loop {
            self.ensure_turn_active()?;
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
            let current_execute_item = self.resolve_execute_loop_item(&step)?;
            let repeat_count = *step_repeat_counts.get(&step.id).unwrap_or(&0);
            let current_item_repeat_count = current_execute_item
                .as_ref()
                .and_then(|item| item_repeat_counts.get(&self.execute_item_repeat_key(&step, item)))
                .copied()
                .unwrap_or(0);

            if let Some(current_item) = current_execute_item.as_ref() {
                self.emit_step_subflow_status(
                    workflow_id,
                    role,
                    &step,
                    current_item,
                    StepSubflowState::Running,
                    current_item_repeat_count,
                    None,
                );
            }

            send_workflow_step(
                &*self.tx_result,
                self.turn_id,
                Some(step_state.clone()),
                workflow_id,
                role,
            );

            let step_prompt = prompts.prompt_for(&step.id).unwrap_or_default();
            let step_input = match self.build_step_execution_input(
                agent.messages(),
                session_context,
                &step,
                step_prompt,
                current_execute_item.clone(),
            ) {
                Ok(step_input) => {
                    self.send_step_input_diagnostics(
                        &diagnostic_context,
                        &step_input,
                        ExecuteLoopProgressState {
                            current_item: current_execute_item.clone(),
                            repeat_count: current_item_repeat_count,
                            completion_source: None,
                        },
                    );
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
            let hook_runtime = StepHookRuntime::new(
                self.hook_host.clone(),
                hook_session.clone(),
                self.todo_manager.clone(),
                self.tx_result.clone(),
                self.turn_id,
                workflow_id,
                role,
                &step,
                step_state.index,
                step_state.total,
                &step_input.resolved_tools,
                step_input.structured_input.as_ref(),
                session_context,
                current_execute_item.clone(),
            );
            hook_runtime.before_step()?;
            let base_system_blocks = step_input.system_blocks.clone();
            agent.set_system_blocks(base_system_blocks.clone());

            let checkpoint = if role == WorkflowRunRole::Root {
                Some(agent.messages().to_vec())
            } else {
                None
            };
            let mut response_streamer = StepResponseStreamer::new(
                &*self.tx_result,
                self.turn_id,
                workflow_id,
                role,
                &step,
                is_final_step,
                session_context.routing.recognized_scene_id.as_deref(),
                current_execute_item.as_ref(),
                step.id != PLAN_STEP_ID,
            );
            response_streamer.begin();
            let step_attempt_checkpoint = agent.messages().to_vec();
            let max_validation_retries = max_output_validation_retries(&step);
            let mut validation_attempt = 0u32;
            let mut last_validation_error = None::<String>;
            let mut last_validation_failure = None::<OutputValidationFailure>;
            let mut current_attempt_kind = StepOutputAttemptKind::Primary;
            let mut attempt_tools = step_input.resolved_tools.clone();
            let mut attempt_max_iterations = step.max_iterations;
            let mut last_usage: Option<omega_core::Usage>;
            let mut last_tool_capabilities = None;
            let (stage_text, structured_output, validation_attempts) = loop {
                let step_run = match self.execute_step(
                    agent,
                    &attempt_tools,
                    attempt_max_iterations,
                    workflow_id,
                    role,
                    &step,
                    current_execute_item.as_ref(),
                    &mut response_streamer,
                    Some(hook_runtime.clone()),
                ) {
                    Ok(stage_text) => stage_text,
                    Err(error) => {
                        if let Some(current_item) = current_execute_item.as_ref() {
                            self.emit_step_subflow_status(
                                workflow_id,
                                role,
                                &step,
                                current_item,
                                StepSubflowState::Failed,
                                current_item_repeat_count,
                                None,
                            );
                        }
                        let _ = hook_runtime.step_failed(&error.to_string());
                        response_streamer.fail();
                        return Err(error);
                    }
                };
                last_usage = step_run.usage.clone();
                last_tool_capabilities = step_run.tool_capabilities.clone();
                delivery_accumulator.observe_step_run(&step_run);
                let stage_text = step_run.stage_text;

                match self.validate_step_output(
                    workflow_id,
                    &step,
                    current_execute_item.as_ref(),
                    &stage_text,
                ) {
                    Ok(structured_output) => {
                        if step.id == PLAN_STEP_ID {
                            response_streamer.append_final_text(&canonical_step_summary_text(
                                &step,
                                &stage_text,
                                structured_output.as_ref(),
                            ));
                        }
                        response_streamer.complete();
                        break (
                            stage_text,
                            structured_output,
                            completed_output_attempts(&step, validation_attempt),
                        );
                    }
                    Err(validation_failure) => {
                        let validation_error_text = validation_failure.message.clone();
                        let attempts = validation_attempt + 1;
                        let can_retry = validation_attempt < max_validation_retries;
                        let retry_count = if can_retry {
                            validation_attempt + 1
                        } else {
                            validation_attempt
                        };
                        self.send_step_output_diagnostics(
                            &diagnostic_context,
                            response_streamer.primary_section_id(),
                            &step_input,
                            OutputDiagnosticState {
                                status: StepOutputStatus::Invalid,
                                attempt_kind: current_attempt_kind,
                                structured_output: validation_failure.extracted_json.as_ref(),
                                usage: last_usage.as_ref(),
                                attempts,
                                retry_count,
                                max_retries: max_validation_retries,
                                validation_error: Some(&validation_error_text),
                                previous_response_preview: Some(
                                    &validation_failure.previous_response_preview,
                                ),
                                recovery_decision: recovery_decision_for_failure(
                                    can_retry,
                                    allows_root_routing_text_fallback(role, &step),
                                    next_retry_attempt_kind(&step, validation_attempt + 1),
                                ),
                                session_writes: Vec::new(),
                            },
                            ExecuteLoopProgressState {
                                current_item: current_execute_item.clone(),
                                repeat_count: current_item_repeat_count,
                                completion_source: None,
                            },
                            last_tool_capabilities.clone(),
                        );
                        last_validation_error = Some(validation_error_text.clone());
                        last_validation_failure = Some(validation_failure.clone());

                        emit_output_recovery_activity(
                            &*self.tx_result,
                            self.turn_id,
                            &step,
                            current_attempt_kind,
                            recovery_decision_for_failure(
                                can_retry,
                                allows_root_routing_text_fallback(role, &step),
                                next_retry_attempt_kind(&step, validation_attempt + 1),
                            ),
                            &validation_failure,
                        );

                        if !can_retry {
                            if allows_root_routing_text_fallback(role, &step) {
                                send_warning_text(
                                    &*self.tx_result,
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
                            if let Some(current_item) = current_execute_item.as_ref() {
                                self.emit_step_subflow_status(
                                    workflow_id,
                                    role,
                                    &step,
                                    current_item,
                                    StepSubflowState::Failed,
                                    current_item_repeat_count,
                                    None,
                                );
                            }
                            let _ = hook_runtime.step_failed(&format!(
                                "step '{}' failed output validation after {} attempt(s): {}",
                                step.id, attempts, validation_failure.message
                            ));
                            return Err(anyhow::anyhow!(
                                "step '{}' failed output validation after {} attempt(s): {}",
                                step.id,
                                attempts,
                                validation_failure.message
                            ));
                        }

                        validation_attempt += 1;
                        let attempt_kind = next_retry_attempt_kind(&step, validation_attempt);
                        send_warning_text(
                            &*self.tx_result,
                            self.turn_id,
                            &retry_warning_message(
                                &step,
                                attempt_kind,
                                validation_attempt,
                                max_validation_retries,
                                &validation_failure.message,
                            ),
                        );
                        agent.set_messages(step_attempt_checkpoint.clone());

                        match attempt_kind {
                            StepOutputAttemptKind::Repair => {
                                attempt_tools = ResolvedToolSet::new(Vec::new());
                                attempt_max_iterations = REPAIR_PASS_MAX_ITERATIONS;
                                let repair_blocks = self
                                    .context_facade
                                    .assembler
                                    .assemble_output_repair_context(
                                        self.build_output_repair_context_request(
                                            &step_input,
                                            &validation_failure,
                                        ),
                                    )?;
                                agent.set_system_blocks(repair_blocks);
                            }
                            StepOutputAttemptKind::Regenerate => {
                                attempt_tools = step_input.resolved_tools.clone();
                                attempt_max_iterations = step.max_iterations;
                                agent.set_system_blocks(base_system_blocks.clone());
                                let validation_feedback = build_output_validation_feedback(
                                    self.cwd,
                                    &step,
                                    &validation_failure.message,
                                );
                                let validation_feedback = if let Some(current_item) =
                                    step_input.current_execute_item.as_ref()
                                {
                                    format!(
                                        "{}\n\nCurrent execute item: '{}' ({}/{}). Only '{}' may be newly added to completed_tasks in this attempt. Keep future items open until their own execute slice runs.",
                                        validation_feedback,
                                        current_item.item_id,
                                        current_item.item_index,
                                        current_item.item_total,
                                        current_item.item_id,
                                    )
                                } else {
                                    validation_feedback
                                };
                                agent.add_user_message(&validation_feedback);
                            }
                            StepOutputAttemptKind::Primary => {}
                        }
                        current_attempt_kind = attempt_kind;
                    }
                }
            };
            if let Some(checkpoint) = checkpoint {
                agent.set_messages(checkpoint);
            }

            let mut step_result = self.finalize_step(
                &step,
                is_final_step,
                stage_text,
                structured_output,
                session_context,
            )?;

            let output_status = final_output_status(
                &step.output_contract,
                step_result.structured_output.as_ref(),
            );

            let progress_completion_source = self.execute_completion_source(
                &step,
                current_execute_item.as_ref(),
                step_result.structured_output.as_ref(),
            )?;

            step_result.transition = match self.apply_before_advance_gate(
                &step,
                step_result.transition.clone(),
                &step_result.final_text,
                step_result.structured_output.as_ref(),
                &hook_runtime,
                repeat_count,
                current_execute_item.as_ref(),
                current_item_repeat_count,
            ) {
                Ok(transition) => transition,
                Err(error) => {
                    self.send_step_output_diagnostics(
                        &diagnostic_context,
                        response_streamer.primary_section_id(),
                        &step_input,
                        OutputDiagnosticState {
                            status: output_status,
                            attempt_kind: current_attempt_kind,
                            structured_output: step_result.structured_output.as_ref(),
                            usage: last_usage.as_ref(),
                            attempts: validation_attempts,
                            retry_count: validation_attempt,
                            max_retries: max_validation_retries,
                            validation_error: matches!(output_status, StepOutputStatus::Invalid)
                                .then_some(last_validation_error.as_deref())
                                .flatten(),
                            previous_response_preview: matches!(
                                output_status,
                                StepOutputStatus::Invalid
                            )
                            .then_some(
                                last_validation_failure
                                    .as_ref()
                                    .map(|failure| failure.previous_response_preview.as_str()),
                            )
                            .flatten(),
                            recovery_decision: None,
                            session_writes: step_result.session_writes.clone(),
                        },
                        ExecuteLoopProgressState {
                            current_item: current_execute_item.clone(),
                            repeat_count: current_item_repeat_count,
                            completion_source: progress_completion_source.clone(),
                        },
                        last_tool_capabilities.clone(),
                    );
                    if let Some(current_item) = current_execute_item.as_ref() {
                        self.emit_step_subflow_status(
                            workflow_id,
                            role,
                            &step,
                            current_item,
                            StepSubflowState::Failed,
                            current_item_repeat_count,
                            None,
                        );
                    }
                    let _ = hook_runtime.step_failed(&error.to_string());
                    return Err(error);
                }
            };

            step_result.transition = self.apply_execute_loop_progression(
                &step,
                step_result.transition,
                current_execute_item.as_ref(),
                progress_completion_source.clone(),
            )?;

            let emitted_repeat_count = match step_result.transition {
                StepTransition::Repeat => current_item_repeat_count + 1,
                _ => current_item_repeat_count,
            };

            self.send_step_output_diagnostics(
                &diagnostic_context,
                response_streamer.primary_section_id(),
                &step_input,
                OutputDiagnosticState {
                    status: output_status,
                    attempt_kind: current_attempt_kind,
                    structured_output: step_result.structured_output.as_ref(),
                    usage: last_usage.as_ref(),
                    attempts: validation_attempts,
                    retry_count: validation_attempt,
                    max_retries: max_validation_retries,
                    validation_error: matches!(output_status, StepOutputStatus::Invalid)
                        .then_some(last_validation_error.as_deref())
                        .flatten(),
                    previous_response_preview: matches!(output_status, StepOutputStatus::Invalid)
                        .then_some(
                            last_validation_failure
                                .as_ref()
                                .map(|failure| failure.previous_response_preview.as_str()),
                        )
                        .flatten(),
                    recovery_decision: None,
                    session_writes: step_result.session_writes.clone(),
                },
                ExecuteLoopProgressState {
                    current_item: current_execute_item.clone(),
                    repeat_count: emitted_repeat_count,
                    completion_source: progress_completion_source.clone(),
                },
                last_tool_capabilities.clone(),
            );

            if let Some(current_item) = current_execute_item.as_ref() {
                let subflow_state = match step_result.transition {
                    StepTransition::Repeat => StepSubflowState::Running,
                    StepTransition::RepeatItem
                    | StepTransition::Continue
                    | StepTransition::FinishTurn => {
                        if progress_completion_source.is_some() {
                            StepSubflowState::Complete
                        } else {
                            StepSubflowState::Running
                        }
                    }
                    StepTransition::Error { .. } => StepSubflowState::Failed,
                    StepTransition::StartWorkflow { .. } => StepSubflowState::Complete,
                };
                self.emit_step_subflow_status(
                    workflow_id,
                    role,
                    &step,
                    current_item,
                    subflow_state,
                    emitted_repeat_count,
                    progress_completion_source.clone(),
                );
            }

            match step_result.transition {
                StepTransition::Repeat => {
                    step_repeat_counts.insert(step.id.clone(), repeat_count + 1);
                    if let Some(current_item) = current_execute_item.as_ref() {
                        item_repeat_counts.insert(
                            self.execute_item_repeat_key(&step, current_item),
                            current_item_repeat_count + 1,
                        );
                    }
                }
                StepTransition::RepeatItem => {
                    if let Some(current_item) = current_execute_item.as_ref() {
                        item_repeat_counts
                            .remove(&self.execute_item_repeat_key(&step, current_item));
                    }
                }
                _ => {
                    step_repeat_counts.remove(&step.id);
                    if let Some(current_item) = current_execute_item.as_ref() {
                        item_repeat_counts
                            .remove(&self.execute_item_repeat_key(&step, current_item));
                    }
                }
            }

            if !step_result.summary.summary.is_empty() {
                session_context
                    .step_summaries
                    .push(step_result.summary.clone());
                info!(
                    workflow_id,
                    workflow_role = %role.as_str(),
                    step_id = %step.id,
                    summary_tokens = step_result.summary.estimated_tokens,
                    summary_preview = %crate::preview_text(&step_result.summary.summary, 160),
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

            if !matches!(
                step_result.transition,
                StepTransition::Repeat | StepTransition::RepeatItem
            ) {
                hook_runtime.after_step(
                    &step_result.final_text,
                    step_result.structured_output.as_ref(),
                )?;
            }

            if !step_result.final_text.is_empty() {
                if role == WorkflowRunRole::Child && !is_final_step {
                    send_step_text(
                        &*self.tx_result,
                        self.turn_id,
                        workflow_id,
                        role,
                        &step,
                        &step_result.final_text,
                    );
                }
            }

            if !step_result.final_text.is_empty() {
                last_text = step_result.final_text.clone();
            }

            match step_result.transition {
                StepTransition::Continue => {
                    if run.advance().is_none() {
                        break;
                    }
                }
                StepTransition::Repeat | StepTransition::RepeatItem => {}
                StepTransition::StartWorkflow { .. } | StepTransition::FinishTurn => break,
                StepTransition::Error { message } => return Err(anyhow::anyhow!(message)),
            }
        }

        Ok(last_text)
    }

    fn execute_step(
        &self,
        agent: &mut Agent,
        resolved_tools: &ResolvedToolSet,
        max_iterations: u32,
        workflow_id: &str,
        role: WorkflowRunRole,
        step: &WorkflowStep,
        current_execute_item: Option<&ExecuteLoopItemContext>,
        response_streamer: &mut StepResponseStreamer<'_>,
        hook_runtime: Option<StepHookRuntime>,
    ) -> anyhow::Result<StepRunOutput> {
        let tool_name_refs = resolved_tools.tool_name_refs();
        agent.set_visible_tools(Some(&tool_name_refs));
        agent.set_max_iterations(max_iterations);

        let tool_manifests = resolved_tools
            .tool_manifests()
            .iter()
            .cloned()
            .map(|manifest| (manifest.id.clone(), manifest))
            .collect::<BTreeMap<_, _>>();
        let execution_context = CoreToolExecutionContext {
            workspace_root: self.cwd.display().to_string(),
            workflow_id: workflow_id.to_string(),
            workflow_role: role.as_str().to_string(),
            step_id: step.id.clone(),
            step_label: step.label.clone(),
            turn_id: self.turn_id,
            current_item_id: current_execute_item.map(|item| item.item_id.clone()),
            current_item_index: current_execute_item.map(|item| item.item_index),
            current_item_total: current_execute_item.map(|item| item.item_total),
        };

        let tool_runs = Arc::new(Mutex::new(ToolRunTracker::new(
            &*self.tx_callback,
            self.turn_id,
            response_streamer.primary_section_id().to_string(),
            tool_manifests,
            execution_context,
        )));
        let hook_error = Arc::new(Mutex::new(None::<String>));
        let usage = Arc::new(Mutex::new(None::<omega_core::Usage>));
        let model_name = Arc::new(Mutex::new(None::<String>));
        let mut cancel_turn_rx = self.cancel_turn_rx.clone();
        let context_facade = Arc::clone(self.context_facade);
        let supervision_state = Arc::clone(&self.supervision_state);

        let stage_text = self
            .handle
            .block_on(agent.run_loop_with_events_until_turn_change(
                {
                    let tx_callback = self.tx_callback.clone();
                    let turn_id = self.turn_id;
                    let tool_runs = tool_runs.clone();
                    let hook_runtime = hook_runtime.clone();
                    let hook_error = hook_error.clone();
                    let context_facade = context_facade.clone();
                    let supervision_state = supervision_state.clone();
                    move |tool_use_id, name, tool_input, tool_result| {
                        let command = if name == "bash" {
                            tool_input
                                .get("command")
                                .and_then(|value| value.as_str())
                                .map(ToOwned::to_owned)
                        } else {
                            None
                        };

                        crate::ui_emit::send_tool_call_preview(
                            &tx_callback,
                            turn_id,
                            name,
                            command,
                            crate::preview_text(
                                &tool_result
                                    .preview
                                    .clone()
                                    .unwrap_or_else(|| tool_result.output.clone()),
                                100,
                            ),
                        );

                        tool_runs.lock().unwrap().complete_tool_run(
                            tool_use_id,
                            name,
                            tool_input,
                            tool_result,
                        );

                        if (name == "todo" || name == "todo_write") && !tool_result.is_error() {
                            send_todo_snapshot(&tx_callback, turn_id, &tool_result.output);
                        }

                        let context_diagnostics = context_facade.diagnostics.context_diagnostics();
                        update_document_supervision_hits(
                            &supervision_state,
                            name,
                            tool_result,
                        );
                        send_context_supervision_snapshot(
                            &tx_callback,
                            turn_id,
                            &context_diagnostics,
                            &supervision_state,
                        );
                        maybe_emit_context_observability(
                            &tx_callback,
                            turn_id,
                            name,
                            tool_input,
                            tool_result,
                            &context_diagnostics,
                        );

                        if let Some(hook_runtime) = &hook_runtime {
                            if let Err(error) = hook_runtime.after_tool_call(
                                tool_use_id,
                                name,
                                tool_input,
                                tool_result,
                            ) {
                                *hook_error.lock().unwrap() = Some(error.to_string());
                            }
                        }
                    }
                },
                {
                    let tool_runs = tool_runs.clone();
                    let usage = usage.clone();
                    let model_name = model_name.clone();
                    move |event| {
                        tool_runs.lock().unwrap().observe_chat_event(event);
                        if let omega_core::ChatEvent::MessageStart {
                            model: Some(step_model_name),
                            ..
                        } = event
                        {
                            *model_name.lock().unwrap() = Some(step_model_name.clone());
                        }
                        if let omega_core::ChatEvent::MessageComplete {
                            usage: Some(usage_update),
                            ..
                        } = event
                        {
                            *usage.lock().unwrap() = Some(usage_update.clone());
                        }
                        response_streamer.push_chat_event(event);
                    }
                },
                Some(&mut cancel_turn_rx),
                Some(self.turn_id),
            ))?;

        if let Some(error) = hook_error.lock().unwrap().take() {
            return Err(anyhow::anyhow!(error));
        }

        let usage = usage.lock().unwrap().clone();
        let model_name = model_name.lock().unwrap().clone();
        let tracker = tool_runs.lock().unwrap();
        let tool_capabilities = Some(tracker.tool_metrics());
        let tool_runs = tracker.tool_runs();
        Ok(StepRunOutput {
            stage_text,
            usage,
            tool_capabilities,
            tool_runs,
            model_name,
        })
    }

    fn ensure_turn_active(&self) -> anyhow::Result<()> {
        if *self.cancel_turn_rx.borrow() != self.turn_id {
            anyhow::bail!("agent turn canceled")
        }

        Ok(())
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
        agent_messages: &[omega_core::Message],
        session_context: &SessionContext,
        step: &WorkflowStep,
        step_prompt: &str,
        current_execute_item: Option<ExecuteLoopItemContext>,
    ) -> anyhow::Result<StepExecutionInput> {
        let resolved_tools = self.tool_catalog.resolve_for_step(&step.tool_request);
        let resolved_skills = self
            .skill_catalog
            .resolve_for_step(
                self.input,
                &session_context.skill_routing.loaded_skill_ids,
                &step.skill_request,
            );
        let structured_input = resolve_structured_input(session_context, step)?;
        let todo_snapshot = self.todo_snapshot_for_step(session_context, step);
        let mut step_request = self.build_step_context_request(
            agent_messages,
            session_context,
            step,
            step_prompt,
            &resolved_skills,
            &resolved_tools,
            structured_input.as_ref(),
            todo_snapshot.as_deref(),
            current_execute_item.as_ref(),
        );
        let token_counter = SessionTokenCounter {
            handle: self.handle,
            client: self.client,
        };
        let mut assembled = self
            .context_facade
            .assemble_step_context(step_request.clone(), &token_counter)?;
        let initial_context = self.context_facade.diagnostics.context_diagnostics();
        if let Some(rewrite) = self.maybe_rewrite_recall_queries(
            step,
            &step_request,
            &assembled,
            &initial_context,
        )? {
            step_request.recall_rewrite_reason = Some(rewrite.reason);
            step_request.recall_rewrite_queries = rewrite.queries;
            step_request.recall_recovery_path = Some(rewrite.recovery_path);
            assembled = self
                .context_facade
                .assemble_step_context(step_request, &token_counter)?;
        }
        let final_context_diagnostics = self.context_facade.diagnostics.context_diagnostics();
        self.context_facade.diagnostics.record_context_assembly(
            &assembled.cache_diagnostics,
            &assembled.selected_step_summaries,
            session_context.step_summaries.len(),
        );
        self.supervision_state.lock().unwrap().document_hits = assembled.document_summary.clone();
        self.log_context_assembly(
            step,
            session_context,
            &assembled.cache_diagnostics,
            &assembled.selected_step_summaries,
        );
        let step_input = StepExecutionInput {
            base_system: self.base_system.to_string(),
            cwd: self.cwd.to_path_buf(),
            resolved_tools,
            resolved_skills,
            system_blocks: assembled.system_blocks,
            document_hits: assembled.document_summary,
            context_diagnostics: final_context_diagnostics,
            cache_diagnostics: cache_diagnostics_from_context(&assembled.cache_diagnostics),
            session_context: SessionContext {
                latest_user_turn: session_context.latest_user_turn.clone(),
                routing: session_context.routing.clone(),
                skill_routing: session_context.skill_routing.clone(),
                step_summaries: assembled
                    .selected_step_summaries
                    .iter()
                    .cloned()
                    .map(step_summary_from_context)
                    .collect(),
                step_outputs: session_context.step_outputs.clone(),
                governance_events: session_context.governance_events.clone(),
                selected_task: session_context.selected_task.clone(),
            },
            structured_input,
            todo_snapshot,
            current_execute_item,
            step: step.clone(),
            step_prompt: step_prompt.to_string(),
        };

        Ok(step_input)
    }

    fn prompt_assembly_ledger_context(&self) -> Option<String> {
        let project_handle = ProjectRegistry::new()
            .resolve(ProjectResolutionInput {
                current_file_path: None,
                cwd: self.cwd.clone(),
                explicit_root: None,
            })
            .ok()?;
        let projection = LedgerSessionContextCompressor::with_budget(
            project_handle,
            session_context_budget_tokens(),
        )
        .load(SessionContextLoadRequest {
            session_id: self.session_id.to_string(),
            max_tokens: session_context_budget_tokens(),
            goal: SessionContextLoadGoal::PromptAssembly,
            query: None,
        })
        .ok()?;

        let checkpoints = projection
            .recent_records
            .iter()
            .chain(projection.checkpoint_records.iter())
            .filter_map(|record| match &record.record {
                SessionContextRecordKind::CompressionCheckpoint { summary, .. } => {
                    Some(summary.as_str())
                }
                SessionContextRecordKind::WorkingSetSnapshot { .. }
                | SessionContextRecordKind::ReplayEntry { .. } => None,
            })
            .take(2)
            .collect::<Vec<_>>();

        if checkpoints.is_empty() && !projection.truncated_history {
            return None;
        }

        let mut lines = vec![format!(
            "source: session.context.jsonl (estimated_tokens={})",
            projection.estimated_tokens
        )];
        lines.push(if projection.truncated_history {
            "history: recent records plus checkpoint summaries".to_string()
        } else {
            "history: recent records only".to_string()
        });
        for summary in checkpoints {
            lines.push(format!("checkpoint: {summary}"));
        }
        Some(lines.join("\n"))
    }

    fn build_step_context_request(
        &self,
        agent_messages: &[omega_core::Message],
        session_context: &SessionContext,
        step: &WorkflowStep,
        step_prompt: &str,
        resolved_skills: &ResolvedSkillSet,
        resolved_tools: &ResolvedToolSet,
        structured_input: Option<&Value>,
        todo_snapshot: Option<&str>,
        current_execute_item: Option<&ExecuteLoopItemContext>,
    ) -> StepContextRequest {
        StepContextRequest {
            skill_system_prompt: resolved_skills.build_system_prompt(self.base_system),
            cwd: self.cwd.to_path_buf(),
            session: ContextSession {
                session_id: self.session_id.to_string(),
                latest_user_turn: session_context.latest_user_turn.clone(),
                routing: context_routing_from_session(&session_context.routing),
                step_summaries: session_context
                    .step_summaries
                    .iter()
                    .cloned()
                    .map(step_summary_to_context)
                    .collect(),
                selected_task: session_context.selected_task.clone(),
            },
            step: ContextStep {
                id: step.id.clone(),
                label: step.label.clone(),
                prompt_path: step.prompt_path.clone(),
                input_sources: step_input_sources(step),
                output_contract: step.output_contract.clone(),
            },
            step_prompt: step_prompt.to_string(),
            document_hits: Vec::new(),
            memory_hits: Vec::new(),
            observation_hits: Vec::new(),
            session_history_hits: Vec::new(),
            structured_input: structured_input.cloned(),
            todo_snapshot: todo_snapshot.map(ToOwned::to_owned),
            current_execute_item: current_execute_item
                .cloned()
                .map(context_execute_item_from_session),
            visible_tool_names: resolved_tools.tool_names().to_vec(),
            tool_manifests: resolved_tools.tool_manifests().to_vec(),
            tool_definitions: resolved_tools.tool_definitions().to_vec(),
            messages: agent_messages.to_vec(),
            session_ledger_context: self.prompt_assembly_ledger_context(),
            recall_rewrite_reason: None,
            recall_rewrite_queries: Vec::new(),
            recall_recovery_path: None,
            context_window: self.context_window,
            max_output_tokens: self.max_output_tokens,
            safety_margin_tokens: CONTEXT_SAFETY_MARGIN_TOKENS,
            report_step_id: crate::REPORT_STEP_ID.to_string(),
            execute_step_id: EXECUTE_STEP_ID.to_string(),
            plan_step_id: PLAN_STEP_ID.to_string(),
            scene_recognition_step_id: SCENE_RECOGNITION_STEP_ID.to_string(),
            select_workflow_step_id: SELECT_WORKFLOW_STEP_ID.to_string(),
            root_workflow_id: ROOT_WORKFLOW_ID.to_string(),
        }
    }

    fn build_output_repair_context_request(
        &self,
        step_input: &StepExecutionInput,
        failure: &OutputValidationFailure,
    ) -> OutputRepairContextRequest {
        OutputRepairContextRequest {
            step_request: StepContextRequest {
                skill_system_prompt: step_input
                    .resolved_skills
                    .build_system_prompt(&step_input.base_system),
                cwd: step_input.cwd.clone(),
                session: ContextSession {
                    session_id: self.session_id.to_string(),
                    latest_user_turn: step_input.session_context.latest_user_turn.clone(),
                    routing: context_routing_from_session(&step_input.session_context.routing),
                    step_summaries: step_input
                        .session_context
                        .step_summaries
                        .iter()
                        .cloned()
                        .map(step_summary_to_context)
                        .collect(),
                    selected_task: step_input.session_context.selected_task.clone(),
                },
                step: ContextStep {
                    id: step_input.step.id.clone(),
                    label: step_input.step.label.clone(),
                    prompt_path: step_input.step.prompt_path.clone(),
                    input_sources: step_input_sources(&step_input.step),
                    output_contract: step_input.step.output_contract.clone(),
                },
                step_prompt: step_input.step_prompt.clone(),
                document_hits: Vec::new(),
                memory_hits: Vec::new(),
                observation_hits: Vec::new(),
                session_history_hits: Vec::new(),
                structured_input: step_input.structured_input.clone(),
                todo_snapshot: step_input.todo_snapshot.clone(),
                current_execute_item: step_input
                    .current_execute_item
                    .as_ref()
                    .cloned()
                    .map(context_execute_item_from_session),
                visible_tool_names: step_input.resolved_tools.tool_names().to_vec(),
                tool_manifests: step_input.resolved_tools.tool_manifests().to_vec(),
                tool_definitions: step_input.resolved_tools.tool_definitions().to_vec(),
                messages: Vec::new(),
                session_ledger_context: self.prompt_assembly_ledger_context(),
                recall_rewrite_reason: None,
                recall_rewrite_queries: Vec::new(),
                recall_recovery_path: None,
                context_window: self.context_window,
                max_output_tokens: self.max_output_tokens,
                safety_margin_tokens: CONTEXT_SAFETY_MARGIN_TOKENS,
                report_step_id: crate::REPORT_STEP_ID.to_string(),
                execute_step_id: EXECUTE_STEP_ID.to_string(),
                plan_step_id: PLAN_STEP_ID.to_string(),
                scene_recognition_step_id: SCENE_RECOGNITION_STEP_ID.to_string(),
                select_workflow_step_id: SELECT_WORKFLOW_STEP_ID.to_string(),
                root_workflow_id: ROOT_WORKFLOW_ID.to_string(),
            },
            failure: OutputRepairFailure {
                error_kind: failure.error_kind.as_str().to_string(),
                message: failure.message.clone(),
                previous_response_preview: failure.previous_response_preview.clone(),
                extracted_json_preview: failure.extracted_json_preview(),
            },
        }
    }

    fn maybe_rewrite_recall_queries(
        &self,
        step: &WorkflowStep,
        request: &StepContextRequest,
        assembled: &omega_context::AssembledContext,
        context: &ContextDiagnostics,
    ) -> anyhow::Result<Option<RecallRewriteDecision>> {
        let Some(reason) = recall_rewrite_reason(step, request, assembled, context) else {
            return Ok(None);
        };

        let prompt = build_recall_rewrite_request(step, request, assembled, context, &reason);
        let response = self
            .handle
            .block_on(self.client.chat(prompt))
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        let queries = parse_recall_rewrite_queries(&response.text_content());
        if queries.is_empty() {
            return Ok(None);
        }

        Ok(Some(RecallRewriteDecision {
            reason,
            queries,
            recovery_path: "rewritten_after_initial_empty_hit".to_string(),
        }))
    }

    fn log_context_assembly(
        &self,
        step: &WorkflowStep,
        session_context: &SessionContext,
        cache_diagnostics: &ContextCacheDiagnostics,
        selected_summaries: &[ContextStepSummary],
    ) {
        debug!(
            step_id = %step.id,
            workflow_id = %session_context.routing.active_workflow_id,
            workflow_role = %session_context.routing.active_workflow_role.as_str(),
            available_input_budget_tokens = cache_diagnostics.budget_input_tokens,
            request_input_tokens = cache_diagnostics.request_input_tokens,
            token_count_source = %cache_diagnostics.token_count_source.as_str(),
            selected_summary_count = selected_summaries.len(),
            selected_summary_tokens = selected_summaries.iter().map(|summary| summary.estimated_tokens).sum::<u32>(),
            total_available_summaries = session_context.step_summaries.len(),
            "step context summary budget resolved"
        );

        if let Some(summary) = build_context_compaction_log(
            session_context.step_summaries.len(),
            selected_summaries,
            cache_diagnostics,
        ) {
            send_system_log_text(&*self.tx_result, self.turn_id, &summary);
        }
    }

    fn todo_snapshot_for_step(
        &self,
        session_context: &SessionContext,
        step: &WorkflowStep,
    ) -> Option<String> {
        if !matches!(step.id.as_str(), EXECUTE_STEP_ID | crate::REPORT_STEP_ID) {
            return None;
        }
        if !session_context.step_outputs.contains_key(PLAN_STEP_ID) {
            return None;
        }

        let manager = self.todo_manager.lock().ok()?;
        (!manager.items().is_empty()).then(|| manager.render())
    }

    fn send_step_diagnostics_effect(&self, section_id: Option<&str>, diagnostics: StepDiagnostics) {
        update_memory_supervision_hits(&self.supervision_state, &diagnostics);
        let context = diagnostics.context.clone();
        self.tx_result.send(crate::RuntimeMessageEnvelope::state(
            self.turn_id,
            crate::StateMessage::Diagnostics {
                diagnostics: Box::new(diagnostics),
            },
        ));
        if let Some(context) = context.as_ref() {
            send_context_supervision_snapshot(
                &*self.tx_result,
                self.turn_id,
                context,
                &self.supervision_state,
            );
            if let Some(section_id) = section_id {
                send_step_knowledge_summary(
                    &*self.tx_result,
                    self.turn_id,
                    section_id,
                    context,
                    &self.supervision_state,
                );
            }
        }
    }

    fn send_step_input_diagnostics(
        &self,
        context: &StepDiagnosticContext<'_>,
        step_input: &StepExecutionInput,
        progress_state: ExecuteLoopProgressState,
    ) {
        let output = build_step_output_diagnostics(
            &context.step.output_contract,
            &OutputDiagnosticState {
                status: pending_output_status_for_contract(&context.step.output_contract),
                attempt_kind: StepOutputAttemptKind::Primary,
                structured_output: None,
                usage: None,
                attempts: 0,
                retry_count: 0,
                max_retries: max_output_validation_retries(context.step),
                validation_error: None,
                previous_response_preview: None,
                recovery_decision: None,
                session_writes: Vec::new(),
            },
        );
        self.send_step_diagnostics_effect(None, build_step_diagnostics(
            context,
            Some(step_input.context_diagnostics.clone()),
            Some(step_input.cache_diagnostics.clone()),
            self.build_execute_progress_diagnostics(context.step, &progress_state),
            build_step_input_diagnostics(step_input),
            output,
            Vec::new(),
            None,
        ));
    }

    fn send_step_input_error_diagnostics(
        &self,
        context: &StepDiagnosticContext<'_>,
        session_context: &SessionContext,
        error: &str,
    ) {
        let output = build_step_output_diagnostics(
            &context.step.output_contract,
            &OutputDiagnosticState {
                status: pending_output_status_for_contract(&context.step.output_contract),
                attempt_kind: StepOutputAttemptKind::Primary,
                structured_output: None,
                usage: None,
                attempts: 0,
                retry_count: 0,
                max_retries: max_output_validation_retries(context.step),
                validation_error: None,
                previous_response_preview: None,
                recovery_decision: None,
                session_writes: Vec::new(),
            },
        );
        self.send_step_diagnostics_effect(None, build_step_diagnostics(
            context,
            Some(self.context_facade.diagnostics.context_diagnostics()),
            None,
            self.build_execute_progress_diagnostics(
                context.step,
                &ExecuteLoopProgressState {
                    current_item: None,
                    repeat_count: 0,
                    completion_source: None,
                },
            ),
            build_failed_step_input_diagnostics(session_context, context.step, error),
            output,
            Vec::new(),
            None,
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
        section_id: &str,
        step_input: &StepExecutionInput,
        output_state: OutputDiagnosticState<'_>,
        progress_state: ExecuteLoopProgressState,
        tool_capabilities: Option<ToolCapabilityDiagnostics>,
    ) {
        let diagnostics = build_step_diagnostics(
            context,
            Some(self.context_facade.diagnostics.context_diagnostics()),
            Some(cache_diagnostics_for_output(
                &step_input.cache_diagnostics,
                output_state.usage,
            )),
            self.build_execute_progress_diagnostics(context.step, &progress_state),
            build_step_input_diagnostics(step_input),
            build_step_output_diagnostics(&context.step.output_contract, &output_state),
            output_state.session_writes,
            tool_capabilities,
        );
        self.send_step_diagnostics_effect(Some(section_id), diagnostics);
    }

    fn build_execute_progress_diagnostics(
        &self,
        step: &WorkflowStep,
        progress_state: &ExecuteLoopProgressState,
    ) -> Option<ExecuteProgressDiagnostics> {
        if step.id != EXECUTE_STEP_ID {
            return None;
        }

        let manager = self.todo_manager.lock().ok()?;
        let todo_total = manager.items().len();
        let todo_completed = manager
            .items()
            .iter()
            .filter(|item| item.status == TodoStatus::Completed)
            .count();
        let todo_open = manager
            .items()
            .iter()
            .filter(|item| item.status != TodoStatus::Completed)
            .count();
        let max_item_repeats = match &step.loop_contract {
            Some(StepLoopContract::TodoItems {
                max_item_repeats, ..
            }) => Some(*max_item_repeats),
            None => None,
        };

        Some(ExecuteProgressDiagnostics {
            todo_total,
            todo_completed,
            todo_open,
            current_item_id: progress_state
                .current_item
                .as_ref()
                .map(|item| item.item_id.clone()),
            current_item_index: progress_state
                .current_item
                .as_ref()
                .map(|item| item.item_index),
            current_item_total: progress_state
                .current_item
                .as_ref()
                .map(|item| item.item_total),
            repeat_count: progress_state.repeat_count,
            no_progress_streak: manager.rounds_without_update() as u32,
            max_step_repeats: step.max_step_repeats,
            max_item_repeats,
            completion_source: progress_state.completion_source.clone(),
        })
    }

    fn resolve_execute_loop_item(
        &self,
        step: &WorkflowStep,
    ) -> anyhow::Result<Option<ExecuteLoopItemContext>> {
        let Some(StepLoopContract::TodoItems {
            child_step_prefix, ..
        }) = &step.loop_contract
        else {
            return Ok(None);
        };

        if step.id != EXECUTE_STEP_ID {
            return Ok(None);
        }

        let manager = self
            .todo_manager
            .lock()
            .map_err(|_| anyhow::anyhow!("todo manager lock poisoned"))?;
        let items = manager.items();
        let total = items.len();
        let Some((index, item)) = items
            .iter()
            .enumerate()
            .find(|(_, item)| item.status == TodoStatus::InProgress)
            .or_else(|| {
                items
                    .iter()
                    .enumerate()
                    .find(|(_, item)| item.status != TodoStatus::Completed)
            })
        else {
            return Ok(None);
        };

        let item_id = item.id.clone().ok_or_else(|| {
            anyhow::anyhow!("todo item missing id while resolving execute loop item")
        })?;

        Ok(Some(ExecuteLoopItemContext {
            child_step_id: format!("{}-{}", child_step_prefix, index + 1),
            item_id,
            item_label: Some(item.text.clone()),
            item_index: index + 1,
            item_total: total,
        }))
    }

    fn emit_step_subflow_status(
        &self,
        workflow_id: &str,
        role: WorkflowRunRole,
        step: &WorkflowStep,
        current_item: &ExecuteLoopItemContext,
        status: StepSubflowState,
        repeat_count_for_item: u32,
        completion_source: Option<String>,
    ) {
        let no_progress_streak_for_item = self
            .todo_manager
            .lock()
            .map(|manager| manager.rounds_without_update() as u32)
            .unwrap_or(0);

        send_step_subflow_status(
            &*self.tx_result,
            self.turn_id,
            StepSubflowStatus {
                workflow_id: workflow_id.to_string(),
                workflow_role: role,
                step_id: step.id.clone(),
                step_label: step.label.clone(),
                subflow_id: current_item.child_step_id.clone(),
                item_id: Some(current_item.item_id.clone()),
                item_label: current_item.item_label.clone(),
                item_index: current_item.item_index,
                item_total: current_item.item_total,
                status,
                repeat_count_for_item,
                no_progress_streak_for_item,
                completion_source,
            },
        );
    }

    fn execute_item_repeat_key(
        &self,
        step: &WorkflowStep,
        item: &ExecuteLoopItemContext,
    ) -> String {
        format!("{}:{}", step.id, item.item_id)
    }

    fn execute_completion_source(
        &self,
        step: &WorkflowStep,
        current_item: Option<&ExecuteLoopItemContext>,
        structured_output: Option<&Value>,
    ) -> anyhow::Result<Option<String>> {
        if step.id != EXECUTE_STEP_ID {
            return Ok(None);
        }

        if let Some(current_item) = current_item {
            if execute_output_marks_item_complete(structured_output, &current_item.item_id)? {
                return Ok(Some("structured_output".to_string()));
            }
        }

        let manager = self
            .todo_manager
            .lock()
            .map_err(|_| anyhow::anyhow!("todo manager lock poisoned"))?;
        if !manager.has_open_items() {
            return Ok(Some("todo_state".to_string()));
        }

        Ok(None)
    }

    fn apply_execute_loop_progression(
        &self,
        step: &WorkflowStep,
        transition: StepTransition,
        current_item: Option<&ExecuteLoopItemContext>,
        completion_source: Option<String>,
    ) -> anyhow::Result<StepTransition> {
        if !matches!(
            transition,
            StepTransition::Continue
                | StepTransition::StartWorkflow { .. }
                | StepTransition::FinishTurn
        ) {
            return Ok(transition);
        }

        let Some(current_item) = current_item else {
            return Ok(transition);
        };
        if step.id != EXECUTE_STEP_ID || completion_source.as_deref() != Some("structured_output") {
            return Ok(transition);
        }

        let next_item = self.resolve_execute_loop_item(step)?;
        if let Some(next_item) = next_item {
            if next_item.item_id != current_item.item_id {
                send_system_log_text(
                    &*self.tx_result,
                    self.turn_id,
                    &format!(
                        "Step '{}' completed item '{}' and continuing with '{}'.",
                        step.id, current_item.item_id, next_item.child_step_id
                    ),
                );
                return Ok(StepTransition::RepeatItem);
            }
        }

        Ok(transition)
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
        let mut structured_output = structured_output;
        let mut transition = if role == WorkflowRunRole::Child && is_final_step {
            StepTransition::FinishTurn
        } else {
            StepTransition::Continue
        };

        let summary_text = match (role, step.id.as_str()) {
            (WorkflowRunRole::Root, SCENE_RECOGNITION_STEP_ID) => {
                let scene_id = self.resolve_scene_from_output(
                    structured_output.as_ref(),
                    &final_text,
                    &session_context.latest_user_turn,
                );
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
                let scene_id = self.resolve_scene_from_output(
                    structured_output.as_ref(),
                    &final_text,
                    &session_context.latest_user_turn,
                );
                session_context.routing.recognized_scene_id = Some(scene_id.clone());
                let selected_workflow_id = self.resolve_workflow_from_output(
                    structured_output.as_ref(),
                    &final_text,
                    &scene_id,
                    &session_context.latest_user_turn,
                );
                session_context.routing.selected_workflow_id = Some(selected_workflow_id.clone());
                if structured_output.is_none() {
                    structured_output = Some(serde_json::json!({
                        "recognized_scene_id": scene_id,
                        "selected_workflow_id": selected_workflow_id,
                    }));
                }
                self.send_session_status(session_context);
                self.send_routing_log(format!(
                    "Recognized scene '{}' and selected workflow '{}'.",
                    scene_id, selected_workflow_id
                ));
                format!("Recognized scene: {scene_id}. Selected workflow: {selected_workflow_id}.")
            }
            (WorkflowRunRole::Root, SELECT_SKILLS_STEP_ID) => {
                let recognized_skill_ids = normalize_skill_ids(&self.resolve_selected_skill_ids_from_output(
                    structured_output.as_ref(),
                    &final_text,
                ));
                let selection_reason = selection_reason_from_output(structured_output.as_ref());
                session_context.skill_routing = SkillRoutingContext {
                    selected_skill_ids: recognized_skill_ids.clone(),
                    loaded_skill_ids: Vec::new(),
                    ignored_skill_ids: Vec::new(),
                    selection_reason,
                    source_step_id: Some(SELECT_SKILLS_STEP_ID.to_string()),
                };
                let selected_workflow_id = session_context
                    .routing
                    .selected_workflow_id
                    .clone()
                    .unwrap_or_else(|| self.ensure_selected_workflow(session_context));
                self.send_routing_log(format!(
                    "Recognized routed skills [{}] before load-skills action for workflow '{}'.",
                    if recognized_skill_ids.is_empty() {
                        "none".to_string()
                    } else {
                        recognized_skill_ids.join(", ")
                    },
                    selected_workflow_id
                ));
                transition = StepTransition::StartWorkflow {
                    workflow_id: selected_workflow_id,
                };
                if recognized_skill_ids.is_empty() {
                    "Recognized routed skills: none.".to_string()
                } else {
                    format!("Recognized routed skills: {}.", recognized_skill_ids.join(", "))
                }
            }
            _ => canonical_step_summary_text(step, &final_text, structured_output.as_ref()),
        };

        if let Some(output) = structured_output.as_ref() {
            let previous_output = session_context.step_outputs.get(&step.id);
            if let Some(write) = build_context_write(
                format!("step_outputs.{}", step.id),
                previous_output.map(|value| crate::preview_json_value(value, 160)),
                Some(crate::preview_json_value(output, 160)),
            ) {
                session_writes.push(write);
            }
            session_context
                .step_outputs
                .insert(step.id.clone(), output.clone());
            session_writes.extend(self.sync_todo_state_from_step(&workflow_id, step, output)?);
        }

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

    fn apply_before_advance_gate(
        &self,
        step: &WorkflowStep,
        base_transition: StepTransition,
        final_text: &str,
        structured_output: Option<&Value>,
        hook_runtime: &StepHookRuntime,
        repeat_count: u32,
        current_item: Option<&ExecuteLoopItemContext>,
        current_item_repeat_count: u32,
    ) -> anyhow::Result<StepTransition> {
        if !matches!(
            base_transition,
            StepTransition::Continue
                | StepTransition::StartWorkflow { .. }
                | StepTransition::FinishTurn
        ) {
            return Ok(base_transition);
        }

        match hook_runtime.before_advance(final_text, structured_output)? {
            HookAdvanceOutcome::Allow => Ok(base_transition),
            HookAdvanceOutcome::Deny { reasons } => {
                let reason_text = reasons
                    .iter()
                    .map(|denial| format!("{}: {}", denial.hook_id, denial.reason))
                    .collect::<Vec<_>>()
                    .join("; ");

                if repeat_count >= step.max_step_repeats {
                    return Err(anyhow::anyhow!(
                        "step '{}' exhausted max_step_repeats={} after advance denial: {}",
                        step.id,
                        step.max_step_repeats,
                        reason_text
                    ));
                }

                if let (
                    Some(ExecuteLoopItemContext { item_id, .. }),
                    Some(StepLoopContract::TodoItems {
                        max_item_repeats, ..
                    }),
                ) = (current_item, &step.loop_contract)
                {
                    if current_item_repeat_count >= *max_item_repeats {
                        return Err(anyhow::anyhow!(
                            "step '{}' exhausted max_item_repeats={} for todo item '{}' after advance denial: {}",
                            step.id,
                            max_item_repeats,
                            item_id,
                            reason_text
                        ));
                    }
                }

                send_warning_text(
                    &*self.tx_result,
                    self.turn_id,
                    &format!(
                        "Step '{}' advance denied; repeating ({}/{}): {}",
                        step.id,
                        repeat_count + 1,
                        step.max_step_repeats,
                        reason_text
                    ),
                );
                send_system_log_text(
                    &*self.tx_result,
                    self.turn_id,
                    &format!(
                        "step '{}' advance_gate=deny repeat_count={} max_step_repeats={} reasons={}",
                        step.id,
                        repeat_count + 1,
                        step.max_step_repeats,
                        reason_text
                    ),
                );

                Ok(StepTransition::Repeat)
            }
        }
    }

    fn sync_todo_state_from_step(
        &self,
        workflow_id: &str,
        step: &WorkflowStep,
        structured_output: &Value,
    ) -> anyhow::Result<Vec<StepContextWrite>> {
        match step.id.as_str() {
            PLAN_STEP_ID
                if matches!(workflow_id, FEATURE_WORKFLOW_ID | DEEP_RESEARCH_WORKFLOW_ID) => {
                self.sync_todo_manager_from_plan_output(structured_output)
            }
            EXECUTE_STEP_ID
                if matches!(
                    workflow_id,
                    FEATURE_WORKFLOW_ID | RESEARCH_WORKFLOW_ID | DEEP_RESEARCH_WORKFLOW_ID
                ) =>
            {
                self.sync_todo_manager_from_execute_output(structured_output)
            }
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
        send_todo_snapshot(&*self.tx_result, self.turn_id, &rendered);
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

        let completed = normalize_execute_task_refs(
            manager.items(),
            &execute.completed_tasks,
            "completed_tasks",
        )?
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>();
        let open = normalize_execute_task_refs(manager.items(), &execute.open_tasks, "open_tasks")?
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>();
        let current_in_progress_id = manager
            .items()
            .iter()
            .find(|item| item.status == TodoStatus::InProgress)
            .and_then(|item| item.id.clone());
        let preserve_current_in_progress = current_in_progress_id
            .as_deref()
            .is_some_and(|item_id| !completed.contains(item_id));
        let mut promoted_open = false;
        let updated_items = manager
            .items()
            .iter()
            .cloned()
            .map(|mut item| {
                let item_id = item.id.as_deref().unwrap_or_default();
                if item.status == TodoStatus::Completed || completed.contains(item_id) {
                    item.status = TodoStatus::Completed;
                    item.active_form = None;
                } else if preserve_current_in_progress
                    && current_in_progress_id.as_deref() == Some(item_id)
                {
                    item.status = TodoStatus::InProgress;
                    item.active_form = Some(format!("working on {}", item.text));
                } else if open.contains(item_id) {
                    item.status = if !preserve_current_in_progress && !promoted_open {
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

        let todo_changed = manager.items() != updated_items.as_slice();
        let rendered = if todo_changed {
            manager.update(updated_items)?
        } else {
            manager.increment_rounds();
            manager.render()
        };
        let writes = build_text_context_write(
            "todo.rendered",
            had_items.then_some(before_rendered.as_str()),
            (!manager.items().is_empty()).then_some(rendered.as_str()),
        )
        .into_iter()
        .collect::<Vec<_>>();
        drop(manager);
        send_todo_snapshot(&*self.tx_result, self.turn_id, &rendered);
        Ok(writes)
    }

    fn validate_step_output(
        &self,
        workflow_id: &str,
        step: &WorkflowStep,
        current_item: Option<&ExecuteLoopItemContext>,
        final_text: &str,
    ) -> Result<Option<Value>, OutputValidationFailure> {
        if step.id == crate::REPORT_STEP_ID {
            validate_report_step_output(final_text).map_err(|error| {
                OutputValidationFailure::new(
                    OutputValidationErrorKind::SemanticInvalid,
                    error.to_string(),
                    final_text,
                    None,
                )
            })?;
        }

        match &step.output_contract {
            StepOutputContract::None => Ok(None),
            StepOutputContract::Required {
                format,
                schema_path,
                ..
            } => {
                let candidates = parse_structured_output_candidates(*format, final_text);
                if step.id == PLAN_STEP_ID {
                    validate_plan_step_output(final_text, &candidates).map_err(|error| {
                        OutputValidationFailure::new(
                            OutputValidationErrorKind::SemanticInvalid,
                            error.to_string(),
                            final_text,
                            candidates.first().cloned(),
                        )
                    })?;
                } else if step.id == EXECUTE_STEP_ID {
                    validate_execute_step_output(final_text, &candidates).map_err(|error| {
                        OutputValidationFailure::new(
                            OutputValidationErrorKind::SemanticInvalid,
                            error.to_string(),
                            final_text,
                            candidates.first().cloned(),
                        )
                    })?;
                }
                if candidates.is_empty() {
                    return Err(OutputValidationFailure::new(
                        OutputValidationErrorKind::ExtractFailed,
                        format!(
                            "expected {} output but response was not valid {}",
                            format.as_str(),
                            format.as_str()
                        ),
                        final_text,
                        None,
                    ));
                }

                let mut first_failure = None;
                for value in candidates {
                    if let Some(schema_path) = schema_path {
                        if let Err(error) = validate_schema_file(self.cwd, schema_path, &value) {
                            if first_failure.is_none() {
                                first_failure = Some(OutputValidationFailure::new(
                                    OutputValidationErrorKind::SchemaInvalid,
                                    error.to_string(),
                                    final_text,
                                    Some(value.clone()),
                                ));
                            }
                            continue;
                        }
                    }

                    if let Err(error) = validate_workflow_step_output(self.cwd, workflow_id, step, &value) {
                        if first_failure.is_none() {
                            first_failure = Some(OutputValidationFailure::new(
                                OutputValidationErrorKind::SemanticInvalid,
                                error.to_string(),
                                final_text,
                                Some(value.clone()),
                            ));
                        }
                        continue;
                    }

                    if let Err(error) = self.validate_itemized_execute_output(
                        workflow_id,
                        step,
                        current_item,
                        &value,
                    ) {
                        if let Some(repaired) = self.try_repair_itemized_execute_output(
                            workflow_id,
                            step,
                            current_item,
                            &value,
                        ) {
                            if self
                                .validate_itemized_execute_output(
                                    workflow_id,
                                    step,
                                    current_item,
                                    &repaired,
                                )
                                .is_ok()
                            {
                                return Ok(Some(repaired));
                            }
                        }
                        if first_failure.is_none() {
                            first_failure = Some(OutputValidationFailure::new(
                                OutputValidationErrorKind::SemanticInvalid,
                                error.to_string(),
                                final_text,
                                Some(value.clone()),
                            ));
                        }
                        continue;
                    }

                    return Ok(Some(value));
                }

                Err(first_failure.unwrap_or_else(|| {
                    OutputValidationFailure::new(
                        OutputValidationErrorKind::ExtractFailed,
                        format!(
                            "expected {} output but response was not valid {}",
                            format.as_str(),
                            format.as_str()
                        ),
                        final_text,
                        None,
                    )
                }))
            }
            StepOutputContract::Optional {
                format,
                schema_path,
                ..
            } => {
                let mut first_failure = None;
                for value in parse_structured_output_candidates(*format, final_text) {
                    if let Some(schema_path) = schema_path {
                        if let Err(error) = validate_schema_file(self.cwd, schema_path, &value) {
                            if first_failure.is_none() {
                                first_failure = Some(OutputValidationFailure::new(
                                    OutputValidationErrorKind::SchemaInvalid,
                                    error.to_string(),
                                    final_text,
                                    Some(value.clone()),
                                ));
                            }
                            continue;
                        }
                    }

                    if let Err(error) = validate_workflow_step_output(self.cwd, workflow_id, step, &value) {
                        if first_failure.is_none() {
                            first_failure = Some(OutputValidationFailure::new(
                                OutputValidationErrorKind::SemanticInvalid,
                                error.to_string(),
                                final_text,
                                Some(value.clone()),
                            ));
                        }
                        continue;
                    }

                    if let Err(error) = self.validate_itemized_execute_output(
                        workflow_id,
                        step,
                        current_item,
                        &value,
                    ) {
                        if let Some(repaired) = self.try_repair_itemized_execute_output(
                            workflow_id,
                            step,
                            current_item,
                            &value,
                        ) {
                            if self
                                .validate_itemized_execute_output(
                                    workflow_id,
                                    step,
                                    current_item,
                                    &repaired,
                                )
                                .is_ok()
                            {
                                return Ok(Some(repaired));
                            }
                        }
                        if first_failure.is_none() {
                            first_failure = Some(OutputValidationFailure::new(
                                OutputValidationErrorKind::SemanticInvalid,
                                error.to_string(),
                                final_text,
                                Some(value.clone()),
                            ));
                        }
                        continue;
                    }

                    return Ok(Some(value));
                }

                if let Some(first_failure) = first_failure {
                    Err(first_failure)
                } else if step_requires_structured_execute_output(workflow_id, step, current_item) {
                    Err(OutputValidationFailure::new(
                        OutputValidationErrorKind::ExtractFailed,
                        "hook-managed itemized execute steps must emit JSON completed_tasks/open_tasks for the current todo item"
                            .to_string(),
                        final_text,
                        None,
                    ))
                } else {
                    Ok(None)
                }
            }
        }
    }

    fn step_uses_json_output(step: &WorkflowStep) -> bool {
        matches!(
            step.output_contract,
            StepOutputContract::Required {
                format: DataFormat::Json,
                ..
            } | StepOutputContract::Optional {
                format: DataFormat::Json,
                ..
            }
        )
    }

    fn canonical_step_summary_text(
        step: &WorkflowStep,
        final_text: &str,
        structured_output: Option<&Value>,
    ) -> String {
        if step_uses_json_output(step) {
            if let Some(output) = structured_output {
                return summarize_step_text(&crate::preview_json_value(output, SUMMARY_CHAR_LIMIT));
            }
        }

        summarize_step_text(final_text)
    }

    fn step_requires_structured_execute_output(
        workflow_id: &str,
        step: &WorkflowStep,
        current_item: Option<&ExecuteLoopItemContext>,
    ) -> bool {
        matches!(workflow_id, FEATURE_WORKFLOW_ID | RESEARCH_WORKFLOW_ID)
            && step.id == EXECUTE_STEP_ID
            && current_item.is_some()
            && matches!(step.loop_contract, Some(StepLoopContract::TodoItems { .. }))
            && step
                .hooks
                .iter()
                .any(|hook_id| hook_id == "todo_managed_execute")
    }
    fn validate_itemized_execute_output(
        &self,
        workflow_id: &str,
        step: &WorkflowStep,
        current_item: Option<&ExecuteLoopItemContext>,
        value: &Value,
    ) -> anyhow::Result<()> {
        if !matches!(
            workflow_id,
            FEATURE_WORKFLOW_ID | RESEARCH_WORKFLOW_ID | DEEP_RESEARCH_WORKFLOW_ID
        )
            || step.id != EXECUTE_STEP_ID
            || !matches!(step.loop_contract, Some(StepLoopContract::TodoItems { .. }))
        {
            return Ok(());
        }

        let Some(current_item) = current_item else {
            return Ok(());
        };

        let execute = parse_feature_execute_output(value.clone())?;
        let manager = self
            .todo_manager
            .lock()
            .map_err(|_| anyhow::anyhow!("todo manager lock poisoned"))?;
        let completed = normalize_execute_task_refs(
            manager.items(),
            &execute.completed_tasks,
            "completed_tasks",
        )?
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>();
        let open = normalize_execute_task_refs(manager.items(), &execute.open_tasks, "open_tasks")?
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>();
        let already_completed = manager
            .items()
            .iter()
            .filter(|item| item.status == TodoStatus::Completed)
            .filter_map(|item| item.id.as_deref())
            .collect::<std::collections::BTreeSet<_>>();
        let allowed_completed = manager
            .items()
            .iter()
            .filter(|item| item.status == TodoStatus::Completed)
            .filter_map(|item| item.id.as_deref())
            .chain(std::iter::once(current_item.item_id.as_str()))
            .collect::<std::collections::BTreeSet<_>>();

        if let Some(reopened_open_id) = open
            .iter()
            .map(String::as_str)
            .find(|task_id| already_completed.contains(task_id))
        {
            anyhow::bail!(
                "itemized execute output reopened previously completed todo item '{}' in open_tasks while current todo item is '{}'",
                reopened_open_id,
                current_item.item_id
            );
        }

        for task_id in completed.iter().map(String::as_str) {
            if !allowed_completed.contains(task_id) {
                anyhow::bail!(
                    "itemized execute output cannot complete future todo item '{}' while current item is '{}'",
                    task_id,
                    current_item.item_id
                );
            }
        }

        let current_item_completed = completed.contains(current_item.item_id.as_str());
        if !current_item_completed {
            let current_item_open = open.contains(current_item.item_id.as_str());
            if !current_item_open {
                anyhow::bail!(
                    "itemized execute output must keep current todo item '{}' in open_tasks until it is completed",
                    current_item.item_id
                );
            }

            if let Some(repeated_completed_id) = completed
                .iter()
                .map(String::as_str)
                .find(|task_id| already_completed.contains(task_id))
            {
                anyhow::bail!(
                    "itemized execute output repeated previously completed todo item '{}' while current todo item '{}' remains open",
                    repeated_completed_id,
                    current_item.item_id
                );
            }
        }

        Ok(())
    }

    /// Attempt to auto-repair an itemized execute output by stripping future
    /// todo items from `completed_tasks` and moving them back to `open_tasks`.
    /// Returns `Some(repaired_value)` when a repair was applied, `None` when
    /// the step is not an itemized execute or no repair is possible.
    fn try_repair_itemized_execute_output(
        &self,
        workflow_id: &str,
        step: &WorkflowStep,
        current_item: Option<&ExecuteLoopItemContext>,
        value: &Value,
    ) -> Option<Value> {
        if !matches!(
            workflow_id,
            FEATURE_WORKFLOW_ID | RESEARCH_WORKFLOW_ID | DEEP_RESEARCH_WORKFLOW_ID
        )
            || step.id != EXECUTE_STEP_ID
            || !matches!(step.loop_contract, Some(StepLoopContract::TodoItems { .. }))
        {
            return None;
        }

        let current_item = current_item?;
        let execute = parse_feature_execute_output(value.clone()).ok()?;
        let current_item_completed = execute
            .completed_tasks
            .iter()
            .any(|task_id| task_id.trim() == current_item.item_id);

        if !current_item_completed {
            return None;
        }

        let manager = self.todo_manager.lock().ok()?;
        let allowed_completed: std::collections::BTreeSet<&str> = manager
            .items()
            .iter()
            .filter(|item| item.status == TodoStatus::Completed)
            .filter_map(|item| item.id.as_deref())
            .chain(std::iter::once(current_item.item_id.as_str()))
            .collect();
        let already_completed: std::collections::BTreeSet<&str> = manager
            .items()
            .iter()
            .filter(|item| item.status == TodoStatus::Completed)
            .filter_map(|item| item.id.as_deref())
            .collect();

        let mut repaired_completed = Vec::new();
        let mut stripped_future_completed = Vec::new();
        for task_id in &execute.completed_tasks {
            if allowed_completed.contains(task_id.trim()) {
                repaired_completed.push(task_id.clone());
            } else {
                stripped_future_completed.push(task_id.clone());
            }
        }

        let mut removed_reopened_open = Vec::new();
        let mut repaired_open = Vec::new();
        for id in &execute.open_tasks {
            if already_completed.contains(id.trim()) {
                removed_reopened_open.push(id.clone());
            } else {
                repaired_open.push(id.clone());
            }
        }

        if stripped_future_completed.is_empty() && removed_reopened_open.is_empty() {
            return None;
        }

        for id in &stripped_future_completed {
            if !repaired_open.iter().any(|oid| oid.trim() == id.trim()) {
                repaired_open.push(id.clone());
            }
        }

        let mut obj = value.as_object()?.clone();
        obj.insert(
            "completed_tasks".to_string(),
            serde_json::json!(repaired_completed),
        );
        obj.insert("open_tasks".to_string(), serde_json::json!(repaired_open));

        info!(
            step_id = %step.id,
            current_item = %current_item.item_id,
            stripped_future_completed = stripped_future_completed.len(),
            removed_reopened_open = removed_reopened_open.len(),
            "auto-repaired itemized execute output: removed invalid future/open todo references"
        );

        Some(Value::Object(obj))
    }

    fn resolve_scene_from_output(
        &self,
        structured_output: Option<&Value>,
        stage_text: &str,
        latest_user_turn: &str,
    ) -> String {
        if let Some(scene_id) =
            parse_structured_id_from_value(structured_output, &["recognized_scene_id", "scene_id"])
                .or_else(|| parse_structured_id(stage_text, &["recognized_scene_id", "scene_id"]))
        {
            if self.scene_catalog.scene(&scene_id).is_some() {
                return self.maybe_align_scene_for_request_intent(scene_id, latest_user_turn);
            }
        }

        if let Some(workflow_id) = parse_structured_id_from_value(
            structured_output,
            &["selected_workflow_id", "workflow_id"],
        )
        .or_else(|| parse_structured_id(stage_text, &["selected_workflow_id", "workflow_id"]))
        {
            if let Some(scene_id) = self
                .scene_catalog
                .scenes
                .iter()
                .find(|scene| scene.workflow_id == workflow_id)
                .map(|scene| scene.id.clone())
            {
                return self.maybe_align_scene_for_request_intent(scene_id, latest_user_turn);
            }
        }

        match find_catalog_match(
            stage_text,
            self.scene_catalog
                .scenes
                .iter()
                .map(|scene| scene.id.as_str()),
        ) {
            Some(scene_id) => self.maybe_align_scene_for_request_intent(scene_id, latest_user_turn),
            None => {
                let fallback = self.scene_catalog.default_scene_id.clone();
                send_warning_text(
                    &*self.tx_result,
                    self.turn_id,
                    &format!(
                        "Scene recognition did not resolve a configured scene; defaulting to '{}'.",
                        fallback
                    ),
                );
                self.maybe_align_scene_for_request_intent(fallback, latest_user_turn)
            }
        }
    }

    fn resolve_workflow_from_output(
        &self,
        structured_output: Option<&Value>,
        stage_text: &str,
        scene_id: &str,
        latest_user_turn: &str,
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
                return self.maybe_align_workflow_for_request_intent(
                    workflow_id,
                    &mapped_workflow,
                    latest_user_turn,
                );
            }
        }

        match find_catalog_match(stage_text, self.workflow_catalog.workflow_ids()) {
            Some(workflow_id)
                if workflow_id != self.scene_catalog.root_workflow_id
                    && self.workflow_catalog.workflow(&workflow_id).is_some() =>
            {
                self.maybe_align_workflow_for_request_intent(
                    workflow_id,
                    &mapped_workflow,
                    latest_user_turn,
                )
            }
            _ => self.maybe_align_workflow_for_request_intent(
                mapped_workflow.clone(),
                &mapped_workflow,
                latest_user_turn,
            ),
        }
    }

    fn resolve_selected_skill_ids_from_output(
        &self,
        structured_output: Option<&Value>,
        stage_text: &str,
    ) -> Vec<String> {
        if let Some(structured_output) = structured_output {
            let skill_ids = parse_selected_skill_ids(structured_output);
            if !skill_ids.is_empty() || structured_output.get("selected_skill_ids").is_some() {
                return skill_ids;
            }
        }

        parse_structured_output_candidates(DataFormat::Json, stage_text)
            .into_iter()
            .find_map(|value| {
                let skill_ids = parse_selected_skill_ids(&value);
                (!skill_ids.is_empty() || value.get("selected_skill_ids").is_some())
                    .then_some(skill_ids)
            })
            .unwrap_or_default()
    }

    fn apply_routed_skill_load_action(&self, session_context: &mut SessionContext) {
        if session_context.skill_routing.selected_skill_ids.is_empty() {
            session_context.skill_routing.loaded_skill_ids.clear();
            session_context.skill_routing.ignored_skill_ids.clear();
            return;
        }

        let load_result = self
            .skill_catalog
            .load_routed_skills(&session_context.skill_routing.selected_skill_ids);
        session_context.skill_routing.selected_skill_ids = load_result.recognized_skill_ids;
        session_context.skill_routing.loaded_skill_ids = load_result.loaded_skill_ids;
        session_context.skill_routing.ignored_skill_ids = load_result.ignored_skill_ids;

        let section_id = format!(
            "turn-{}:{}:{}:load-skills",
            self.turn_id,
            WorkflowRunRole::Root.as_str(),
            self.scene_catalog.root_workflow_id,
        );
        self.emit_skill_load_response_section(&section_id, session_context);
        self.tx_result.send(crate::RuntimeMessageEnvelope::state(
            self.turn_id,
            crate::StateMessage::SkillLoadSummary {
                section_id: section_id.clone(),
                summary: Box::new(crate::SkillLoadSummary {
                    source_step_id: session_context.skill_routing.source_step_id.clone(),
                    recognized_skill_ids: session_context.skill_routing.selected_skill_ids.clone(),
                    loaded_skill_ids: session_context.skill_routing.loaded_skill_ids.clone(),
                    ignored_skill_ids: session_context.skill_routing.ignored_skill_ids.clone(),
                    selection_reason: session_context.skill_routing.selection_reason.clone(),
                }),
            },
        ));

        let loaded = if session_context.skill_routing.loaded_skill_ids.is_empty() {
            "none".to_string()
        } else {
            session_context.skill_routing.loaded_skill_ids.join(", ")
        };
        if session_context.skill_routing.ignored_skill_ids.is_empty() {
            self.send_routing_log(format!(
                "Loaded routed skills [{}] before child workflow start.",
                loaded
            ));
        } else {
            self.send_routing_log(format!(
                "Loaded routed skills [{}] before child workflow start; ignored [{}].",
                loaded,
                session_context.skill_routing.ignored_skill_ids.join(", ")
            ));
        }
    }

    fn emit_skill_load_response_section(
        &self,
        section_id: &str,
        session_context: &SessionContext,
    ) {
        let section = crate::ResponseSection {
            id: section_id.to_string(),
            parent_id: None,
            kind: crate::ResponseSectionKind::Step,
            title: "Load Skills".to_string(),
            state: crate::ResponseSectionState::Streaming,
            metadata: crate::ResponseSectionMetadata {
                scene_id: session_context.routing.recognized_scene_id.clone(),
                origin: crate::SectionOrigin::Workflow {
                    workflow_id: self.scene_catalog.root_workflow_id.clone(),
                    workflow_role: WorkflowRunRole::Root,
                },
                step_id: Some("load-skills".to_string()),
                step_label: Some("Load Skills".to_string()),
                subflow_ref: None,
            },
        };
        crate::ui_emit::send_begin_response_section(&*self.tx_result, self.turn_id, section);
        crate::ui_emit::send_append_response_section(
            &*self.tx_result,
            self.turn_id,
            section_id,
            crate::ResponseSectionDelta::Text(format_skill_load_response_text(session_context)),
        );
        crate::ui_emit::send_complete_response_section(
            &*self.tx_result,
            self.turn_id,
            section_id,
            crate::ResponseSectionState::Complete,
        );
    }

    fn maybe_align_scene_for_request_intent(
        &self,
        scene_id: String,
        latest_user_turn: &str,
    ) -> String {
        if latest_user_turn_requires_feature_scene(latest_user_turn) {
            let promoted_scene_id = self
                .scene_catalog
                .scene(FEATURE_SCENE_ID)
                .map(|scene| scene.id.clone())
                .unwrap_or_else(|| self.scene_catalog.default_scene_id.clone());
            if promoted_scene_id != scene_id {
                send_warning_text(
                    &*self.tx_result,
                    self.turn_id,
                    &format!(
                        "Scene recognition returned '{}' for an implementation-oriented request; promoting to '{}'.",
                        scene_id, promoted_scene_id
                    ),
                );
                return promoted_scene_id;
            }
        }

        if latest_user_turn_prefers_deep_research_scene(latest_user_turn) {
            if let Some(promoted_scene_id) = self
                .scene_catalog
                .scene(DEEP_RESEARCH_SCENE_ID)
                .map(|scene| scene.id.clone())
            {
                if promoted_scene_id != scene_id {
                    send_warning_text(
                        &*self.tx_result,
                        self.turn_id,
                        &format!(
                            "Scene recognition returned '{}' for a deep-research-oriented request; promoting to '{}'.",
                            scene_id, promoted_scene_id
                        ),
                    );
                    return promoted_scene_id;
                }
            }
        }

        if scene_id != DEEP_RESEARCH_SCENE_ID
            && latest_user_turn_prefers_research_scene(latest_user_turn)
        {
            if let Some(promoted_scene_id) = self
                .scene_catalog
                .scene(RESEARCH_SCENE_ID)
                .map(|scene| scene.id.clone())
            {
                if promoted_scene_id != scene_id {
                    send_warning_text(
                        &*self.tx_result,
                        self.turn_id,
                        &format!(
                            "Scene recognition returned '{}' for a research-oriented request; promoting to '{}'.",
                            scene_id, promoted_scene_id
                        ),
                    );
                    return promoted_scene_id;
                }
            }
        }

        scene_id
    }

    fn maybe_align_workflow_for_request_intent(
        &self,
        workflow_id: String,
        mapped_workflow: &str,
        latest_user_turn: &str,
    ) -> String {
        if workflow_id != mapped_workflow
            && latest_user_turn_requires_feature_scene(latest_user_turn)
        {
            send_warning_text(
                &*self.tx_result,
                self.turn_id,
                &format!(
                    "Workflow selection returned '{}' for an implementation-oriented request; promoting to '{}'.",
                    workflow_id, mapped_workflow
                ),
            );
            return mapped_workflow.to_string();
        }

        if workflow_id != mapped_workflow
            && mapped_workflow == DEEP_RESEARCH_WORKFLOW_ID
            && latest_user_turn_prefers_deep_research_scene(latest_user_turn)
        {
            send_warning_text(
                &*self.tx_result,
                self.turn_id,
                &format!(
                    "Workflow selection returned '{}' for a deep-research-oriented request; promoting to '{}'.",
                    workflow_id, mapped_workflow
                ),
            );
            return mapped_workflow.to_string();
        }

        if workflow_id != mapped_workflow
            && mapped_workflow == RESEARCH_WORKFLOW_ID
            && latest_user_turn_prefers_research_scene(latest_user_turn)
        {
            send_warning_text(
                &*self.tx_result,
                self.turn_id,
                &format!(
                    "Workflow selection returned '{}' for a research-oriented request; promoting to '{}'.",
                    workflow_id, mapped_workflow
                ),
            );
            return mapped_workflow.to_string();
        }

        workflow_id
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
        send_session_status(
            &*self.tx_result,
            self.turn_id,
            &self.scene_catalog.root_workflow_id,
            session_context,
        );
    }

    fn send_routing_log(&self, text: String) {
        send_routing_log(&*self.tx_result, self.turn_id, text);
    }
}

struct SessionTokenCounter<'a> {
    handle: &'a Handle,
    client: &'a DynLlmClient,
}

impl ContextTokenCounter for SessionTokenCounter<'_> {
    fn count_request_tokens(&self, request: ChatRequest) -> anyhow::Result<u32> {
        self.handle
            .block_on(self.client.count_tokens(request))
            .map_err(|error| anyhow::anyhow!(error.to_string()))
    }
}

fn context_routing_from_session(routing: &crate::session_state::RoutingContext) -> ContextRouting {
    ContextRouting {
        recognized_scene_id: routing.recognized_scene_id.clone(),
        selected_workflow_id: routing.selected_workflow_id.clone(),
        active_workflow_id: routing.active_workflow_id.clone(),
        active_workflow_role: match routing.active_workflow_role {
            WorkflowRunRole::Root => ContextWorkflowRole::Root,
            WorkflowRunRole::Child => ContextWorkflowRole::Child,
        },
    }
}

fn step_summary_to_context(summary: StepSummary) -> ContextStepSummary {
    ContextStepSummary {
        workflow_id: summary.workflow_id,
        step_id: summary.step_id,
        title: summary.title,
        summary: summary.summary,
        estimated_tokens: summary.estimated_tokens,
    }
}

fn step_summary_from_context(summary: ContextStepSummary) -> StepSummary {
    StepSummary {
        workflow_id: summary.workflow_id,
        step_id: summary.step_id,
        title: summary.title,
        summary: summary.summary,
        estimated_tokens: summary.estimated_tokens,
    }
}

fn context_execute_item_from_session(item: ExecuteLoopItemContext) -> ContextExecuteItem {
    ContextExecuteItem {
        item_id: item.item_id,
        item_index: item.item_index,
        item_total: item.item_total,
        item_label: item.item_label,
    }
}

fn step_input_sources(step: &WorkflowStep) -> Vec<String> {
    match &step.input_contract {
        StepInputContract::Required { sources } | StepInputContract::Optional { sources } => {
            sources.clone()
        }
        StepInputContract::None => Vec::new(),
    }
}

fn parse_selected_skill_ids(value: &Value) -> Vec<String> {
    value
        .get("selected_skill_ids")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect()
}

fn selection_reason_from_output(output: Option<&Value>) -> Option<String> {
    output
        .and_then(|value| value.get("selection_reason"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn format_skill_load_response_text(session_context: &SessionContext) -> String {
    let recognized = if session_context.skill_routing.selected_skill_ids.is_empty() {
        "none".to_string()
    } else {
        session_context.skill_routing.selected_skill_ids.join(", ")
    };
    let loaded = if session_context.skill_routing.loaded_skill_ids.is_empty() {
        "none".to_string()
    } else {
        session_context.skill_routing.loaded_skill_ids.join(", ")
    };
    let ignored = if session_context.skill_routing.ignored_skill_ids.is_empty() {
        "none".to_string()
    } else {
        session_context.skill_routing.ignored_skill_ids.join(", ")
    };

    let mut lines = vec![
        format!("recognized: {recognized}"),
        format!("loaded: {loaded}"),
        format!("ignored: {ignored}"),
    ];
    if let Some(reason) = session_context.skill_routing.selection_reason.as_deref() {
        lines.push(format!("reason: {reason}"));
    }
    lines.join("\n")
}

fn cache_diagnostics_from_context(diagnostics: &ContextCacheDiagnostics) -> CacheDiagnostics {
    CacheDiagnostics {
        token_count_source: match diagnostics.token_count_source {
            ContextTokenCountSource::ProviderCountTokens => TokenCountSource::ProviderCountTokens,
            ContextTokenCountSource::Estimated => TokenCountSource::Estimated,
        },
        request_input_tokens: diagnostics.request_input_tokens,
        budget_input_tokens: diagnostics.budget_input_tokens,
        cache_breakpoints: diagnostics.cache_breakpoints.clone(),
        cache_creation_input_tokens: diagnostics.cache_creation_input_tokens,
        cache_read_input_tokens: diagnostics.cache_read_input_tokens,
        uncached_input_tokens: diagnostics.uncached_input_tokens,
        cache_hit_ratio_percent: diagnostics.cache_hit_ratio_percent,
    }
}

fn recall_rewrite_reason(
    step: &WorkflowStep,
    request: &StepContextRequest,
    assembled: &omega_context::AssembledContext,
    context: &ContextDiagnostics,
) -> Option<String> {
    if !request.recall_rewrite_queries.is_empty() {
        return None;
    }

    let raw_query = request.session.latest_user_turn.trim();
    if raw_query.is_empty() {
        return None;
    }

    let memory_empty = context
        .memory
        .current_query
        .as_ref()
        .map(|query| query.result_count == 0)
        .unwrap_or(true);
    let observation_empty = context
        .memory
        .current_observations
        .as_ref()
        .map(|query| query.result_count == 0)
        .unwrap_or(true);
    let document_empty = assembled
        .document_summary
        .as_ref()
        .map(|summary| summary.result_count == 0)
        .unwrap_or(true);
    let long_query = raw_query.chars().count() > 120;
    let weak_anchor = recall_query_lacks_anchor(raw_query);

    if step_depends_on_recall(step)
        && (long_query || weak_anchor)
        && memory_empty
        && observation_empty
        && document_empty
    {
        return Some("initial recall returned no hits for a recall-dependent step".to_string());
    }

    None
}

fn step_depends_on_recall(step: &WorkflowStep) -> bool {
    matches!(step.id.as_str(), crate::REPORT_STEP_ID)
        || step.label.to_lowercase().contains("report")
        || step.label.to_lowercase().contains("document")
}

fn recall_query_lacks_anchor(query: &str) -> bool {
    let lowered = query.to_lowercase();
    !lowered.contains("/")
        && !lowered.contains("::")
        && !lowered.contains("omega-")
        && !lowered.contains(".rs")
        && !lowered.contains(".md")
        && query.split_whitespace().count() >= 10
}

fn build_recall_rewrite_request(
    step: &WorkflowStep,
    request: &StepContextRequest,
    assembled: &omega_context::AssembledContext,
    context: &ContextDiagnostics,
    reason: &str,
) -> ChatRequest {
    let mut prompt = vec![
        format!("reason: {reason}"),
        format!("step_id: {}", step.id),
        format!("step_label: {}", step.label),
        format!("raw_query: {}", request.session.latest_user_turn.trim()),
        format!(
            "initial_hits: document={} memory={} observations={}",
            assembled
                .document_summary
                .as_ref()
                .map(|summary| summary.result_count)
                .unwrap_or_default(),
            context
                .memory
                .current_query
                .as_ref()
                .map(|query| query.result_count)
                .unwrap_or_default(),
            context
                .memory
                .current_observations
                .as_ref()
                .map(|query| query.result_count)
                .unwrap_or_default()
        ),
    ];
    if let Some(structured_input) = request.structured_input.as_ref() {
        prompt.push(format!(
            "structured_input: {}",
            serde_json::to_string_pretty(structured_input)
                .unwrap_or_else(|_| structured_input.to_string())
        ));
    }
    if let Some(todo_snapshot) = request.todo_snapshot.as_deref() {
        if !todo_snapshot.trim().is_empty() {
            prompt.push(format!("todo_snapshot: {todo_snapshot}"));
        }
    }

    ChatRequest::new(vec![Message::user(prompt.join("\n"))])
        .with_system_blocks(vec![omega_core::SystemBlock::text(
            "Return ONLY a JSON object {\"queries\": [\"...\"]}. Produce 1-3 short repository recall queries. Keep deterministic anchors like file paths, crate names, task labels, or concepts. Do not explain.",
        )])
        .with_max_tokens(256)
}

fn parse_recall_rewrite_queries(text: &str) -> Vec<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    let value = serde_json::from_str::<Value>(trimmed).ok().or_else(|| {
        parse_structured_output_candidates(DataFormat::Json, trimmed)
            .into_iter()
            .next()
    });
    let Some(value) = value else {
        return Vec::new();
    };
    let queries = value
        .get("queries")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    dedupe_recall_queries(
        &queries
            .into_iter()
            .filter_map(|value| value.as_str().map(str::trim).map(ToOwned::to_owned))
            .collect::<Vec<_>>(),
    )
}

fn dedupe_recall_queries(values: &[String]) -> Vec<String> {
    let mut seen = std::collections::BTreeSet::new();
    let mut deduped = Vec::new();
    for value in values {
        let trimmed = value.trim();
        if trimmed.is_empty() || !seen.insert(trimmed.to_string()) {
            continue;
        }
        deduped.push(trimmed.to_string());
    }
    deduped.truncate(3);
    deduped
}

fn allows_root_routing_text_fallback(role: WorkflowRunRole, step: &WorkflowStep) -> bool {
    role == WorkflowRunRole::Root
        && matches!(
            step.id.as_str(),
            SCENE_RECOGNITION_STEP_ID | SELECT_WORKFLOW_STEP_ID | SELECT_SKILLS_STEP_ID
        )
}

pub(crate) fn resolve_structured_input(
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

fn max_output_validation_retries(step: &WorkflowStep) -> u32 {
    if step.id == crate::REPORT_STEP_ID {
        return 1;
    }

    match &step.output_contract {
        StepOutputContract::Required { max_retries, .. } => *max_retries,
        StepOutputContract::Optional { max_retries, .. } => *max_retries,
        StepOutputContract::None => 0,
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

fn completed_output_attempts(step: &WorkflowStep, retry_count: u32) -> u32 {
    if step.id == crate::REPORT_STEP_ID {
        return retry_count + 1;
    }

    match &step.output_contract {
        StepOutputContract::None => 0,
        StepOutputContract::Required { .. } | StepOutputContract::Optional { .. } => {
            retry_count + 1
        }
    }
}

fn next_retry_attempt_kind(step: &WorkflowStep, retry_count: u32) -> StepOutputAttemptKind {
    if step.id == crate::REPORT_STEP_ID {
        return StepOutputAttemptKind::Regenerate;
    }

    match &step.output_contract {
        StepOutputContract::Required {
            recovery_mode: OutputRecoveryMode::RepairThenRegenerate,
            ..
        }
        | StepOutputContract::Optional {
            recovery_mode: OutputRecoveryMode::RepairThenRegenerate,
            ..
        } if retry_count == 1 => StepOutputAttemptKind::Repair,
        StepOutputContract::Required { .. } | StepOutputContract::Optional { .. } => {
            StepOutputAttemptKind::Regenerate
        }
        StepOutputContract::None => StepOutputAttemptKind::Primary,
    }
}

fn validate_report_step_output(final_text: &str) -> anyhow::Result<()> {
    let trimmed = final_text.trim();
    if trimmed.is_empty() {
        anyhow::bail!("report step must produce a non-empty user-facing response");
    }

    if serde_json::from_str::<Value>(trimmed)
        .ok()
        .is_some_and(|value| matches!(value, Value::Object(_) | Value::Array(_)))
    {
        anyhow::bail!(
            "report step must produce user-facing prose, not a raw JSON object or array"
        );
    }

    Ok(())
}

fn validate_plan_step_output(final_text: &str, candidates: &[Value]) -> anyhow::Result<()> {
    let trimmed = final_text.trim();
    if trimmed.is_empty() {
        anyhow::bail!(
            "plan step must return only a single JSON object with goal, tasks, and validation_targets"
        );
    }

    match serde_json::from_str::<Value>(trimmed) {
        Ok(Value::Object(_)) => Ok(()),
        Ok(_) => anyhow::bail!(
            "plan step must return only a single JSON object with goal, tasks, and validation_targets"
        ),
        Err(_) if has_single_plan_shaped_candidate(candidates) => Ok(()),
        Err(_) if candidates.is_empty() => anyhow::bail!(
            "plan step must return only a single JSON object with goal, tasks, and validation_targets; do not include headings, markdown, code fences, or report prose"
        ),
        Err(_) => anyhow::bail!(
            "plan step must resolve to a single JSON object with goal, tasks, and validation_targets; avoid multiple JSON blocks or non-plan wrappers"
        ),
    }
}

fn validate_execute_step_output(final_text: &str, candidates: &[Value]) -> anyhow::Result<()> {
    let trimmed = final_text.trim();
    if trimmed.is_empty() {
        anyhow::bail!(
            "execute step must return only a single JSON object with completed_tasks, open_tasks, validation_results, and changed_paths"
        );
    }

    match serde_json::from_str::<Value>(trimmed) {
        Ok(Value::Object(_)) => Ok(()),
        Ok(_) => anyhow::bail!(
            "execute step must return only a single JSON object with completed_tasks, open_tasks, validation_results, and changed_paths"
        ),
        Err(_) if has_single_execute_shaped_candidate(candidates) => Ok(()),
        Err(_) if candidates.is_empty() => anyhow::bail!(
            "execute step must return only a single JSON object with completed_tasks, open_tasks, validation_results, and changed_paths; do not include headings, markdown, code fences, or report prose"
        ),
        Err(_) => anyhow::bail!(
            "execute step must resolve to a single JSON object with completed_tasks, open_tasks, validation_results, and changed_paths; avoid multiple JSON blocks or non-execute wrappers"
        ),
    }
}

/// Returns true when exactly one candidate is a plan-shaped JSON object
/// (has `goal`, `tasks`, and `validation_targets` keys).  This handles
/// two scenarios: (1) a single JSON object wrapped in prose, and
/// (2) multiple JSON blocks where only one is the actual plan (the rest
/// are echoed explore output or other non-plan JSON).
fn has_single_plan_shaped_candidate(candidates: &[Value]) -> bool {
    let plan_count = candidates
        .iter()
        .filter(|value| is_plan_shaped(value))
        .count();
    plan_count == 1
}

fn has_single_execute_shaped_candidate(candidates: &[Value]) -> bool {
    let execute_count = candidates
        .iter()
        .filter(|value| is_execute_shaped(value))
        .count();
    execute_count == 1
}

fn is_plan_shaped(value: &Value) -> bool {
    if let Value::Object(map) = value {
        map.contains_key("goal")
            && map.contains_key("tasks")
            && map.contains_key("validation_targets")
    } else {
        false
    }
}

fn is_execute_shaped(value: &Value) -> bool {
    if let Value::Object(map) = value {
        map.contains_key("completed_tasks")
            && map.contains_key("open_tasks")
            && map.contains_key("validation_results")
            && map.contains_key("changed_paths")
    } else {
        false
    }
}

fn normalize_execute_task_refs(
    items: &[TodoItem],
    task_refs: &[String],
    field_name: &str,
) -> anyhow::Result<Vec<String>> {
    task_refs
        .iter()
        .map(|task_ref| {
            resolve_todo_reference(items, task_ref).ok_or_else(|| {
                anyhow::anyhow!(
                    "execute output {field_name} contains unknown todo item '{}'",
                    task_ref.trim()
                )
            })
        })
        .collect()
}

fn resolve_todo_reference(items: &[TodoItem], task_ref: &str) -> Option<String> {
    let task_ref = task_ref.trim();
    if task_ref.is_empty() {
        return None;
    }

    if let Some(item_id) = items.iter().find_map(|item| {
        let item_id = item.id.as_deref()?;
        (item_id == task_ref).then_some(item_id)
    }) {
        return Some(item_id.to_string());
    }

    if let Some(item_id) = items.iter().find_map(|item| {
        let item_id = item.id.as_deref()?;
        let active_form = item.active_form.as_deref().map(str::trim);
        (item.text.trim() == task_ref || active_form == Some(task_ref)).then_some(item_id)
    }) {
        return Some(item_id.to_string());
    }

    let title_matches = items
        .iter()
        .filter_map(|item| {
            let item_id = item.id.as_deref()?;
            let title = todo_item_title(item.text.as_str());
            let active_title = item
                .active_form
                .as_deref()
                .and_then(|active_form| active_form.strip_prefix("working on "))
                .map(str::trim);
            (title == task_ref || active_title == Some(task_ref)).then_some(item_id.to_string())
        })
        .collect::<std::collections::BTreeSet<_>>();

    (title_matches.len() == 1)
        .then(|| title_matches.into_iter().next())
        .flatten()
}

fn todo_item_title(item_text: &str) -> &str {
    item_text
        .split_once(':')
        .map(|(title, _)| title.trim())
        .unwrap_or_else(|| item_text.trim())
}

fn recovery_decision_for_failure(
    can_retry: bool,
    allows_text_fallback: bool,
    next_attempt_kind: StepOutputAttemptKind,
) -> Option<StepOutputRecoveryDecision> {
    if can_retry {
        return Some(match next_attempt_kind {
            StepOutputAttemptKind::Repair => StepOutputRecoveryDecision::Repair,
            StepOutputAttemptKind::Regenerate => StepOutputRecoveryDecision::Regenerate,
            StepOutputAttemptKind::Primary => StepOutputRecoveryDecision::Regenerate,
        });
    }

    Some(if allows_text_fallback {
        StepOutputRecoveryDecision::FallbackTextRouting
    } else {
        StepOutputRecoveryDecision::Abort
    })
}

fn retry_warning_message(
    step: &WorkflowStep,
    attempt_kind: StepOutputAttemptKind,
    retry_count: u32,
    max_retries: u32,
    validation_error: &str,
) -> String {
    match attempt_kind {
        StepOutputAttemptKind::Repair => format!(
            "Step '{}' produced invalid structured output. Running repair pass ({}/{}): {}",
            step.id, retry_count, max_retries, validation_error
        ),
        StepOutputAttemptKind::Regenerate => format!(
            "Step '{}' produced invalid structured output. Regenerating step ({}/{}): {}",
            step.id, retry_count, max_retries, validation_error
        ),
        StepOutputAttemptKind::Primary => format!(
            "Step '{}' produced invalid structured output. Retrying ({}/{}): {}",
            step.id, retry_count, max_retries, validation_error
        ),
    }
}

fn emit_output_recovery_activity(
    tx: &dyn RuntimeMessageBridge,
    turn_id: u64,
    step: &WorkflowStep,
    attempt_kind: StepOutputAttemptKind,
    recovery_decision: Option<StepOutputRecoveryDecision>,
    failure: &OutputValidationFailure,
) {
    let mut summary = format!(
        "Step '{}' structured output attempt={} error_kind={}",
        step.id,
        attempt_kind.as_str(),
        failure.error_kind.as_str()
    );
    if let Some(recovery_decision) = recovery_decision {
        summary.push_str(&format!(" next={}", recovery_decision.as_str()));
    }
    summary.push_str(&format!(" validation_error={}", failure.message));
    send_system_log_text(tx, turn_id, &summary);
    send_system_log_text(
        tx,
        turn_id,
        &format!(
            "previous_response_preview: {}",
            failure.previous_response_preview
        ),
    );
    if let Some(extracted_json_preview) = failure.extracted_json_preview() {
        send_system_log_text(
            tx,
            turn_id,
            &format!("extracted_json_preview: {extracted_json_preview}"),
        );
    }
}

fn build_context_compaction_log(
    total_available_summaries: usize,
    selected_summaries: &[ContextStepSummary],
    cache_diagnostics: &ContextCacheDiagnostics,
) -> Option<String> {
    if total_available_summaries <= selected_summaries.len() {
        return None;
    }

    Some(format!(
        "context.compact selected={}/{} summary_tokens={} budget={} request_tokens={} token_source={}",
        selected_summaries.len(),
        total_available_summaries,
        selected_summaries
            .iter()
            .map(|summary| summary.estimated_tokens)
            .sum::<u32>(),
        cache_diagnostics.budget_input_tokens,
        cache_diagnostics.request_input_tokens,
        cache_diagnostics.token_count_source.as_str(),
    ))
}

fn build_step_diagnostics(
    context: &StepDiagnosticContext<'_>,
    snapshot: Option<ContextDiagnostics>,
    cache: Option<CacheDiagnostics>,
    execute_progress: Option<ExecuteProgressDiagnostics>,
    input: StepInputDiagnostics,
    output: StepOutputDiagnostics,
    session_writes: Vec<StepContextWrite>,
    tool_capabilities: Option<ToolCapabilityDiagnostics>,
) -> StepDiagnostics {
    let diagnostic_id = match (
        &context.step.loop_contract,
        execute_progress
            .as_ref()
            .and_then(|progress| progress.current_item_index),
    ) {
        (
            Some(StepLoopContract::TodoItems {
                child_step_prefix, ..
            }),
            Some(item_index),
        ) => format!(
            "{}:{}:{}-{}",
            context.workflow_role.as_str(),
            context.workflow_id,
            child_step_prefix,
            item_index
        ),
        _ => format!(
            "{}:{}:{}",
            context.workflow_role.as_str(),
            context.workflow_id,
            context.step.id
        ),
    };

    StepDiagnostics {
        id: diagnostic_id,
        workflow_id: context.workflow_id.to_string(),
        workflow_role: context.workflow_role,
        step_id: context.step.id.clone(),
        step_label: context.step.label.clone(),
        index: context.index,
        total: context.total,
        context: snapshot,
        cache,
        execute_progress,
        input,
        output,
        session_writes,
        tool_capabilities,
    }
}

fn cache_diagnostics_for_output(
    base: &CacheDiagnostics,
    usage: Option<&omega_core::Usage>,
) -> CacheDiagnostics {
    let mut diagnostics = base.clone();
    if let Some(usage) = usage {
        diagnostics.cache_creation_input_tokens = usage.cache_creation_input_tokens;
        diagnostics.cache_read_input_tokens = usage.cache_read_input_tokens;
        diagnostics.uncached_input_tokens = Some(usage.input_tokens);
        diagnostics.cache_hit_ratio_percent = cache_hit_ratio_percent(
            usage.input_tokens,
            usage.cache_read_input_tokens.unwrap_or(0),
        );
    }
    diagnostics
}

fn cache_hit_ratio_percent(uncached_input_tokens: u32, cache_read_input_tokens: u32) -> Option<u8> {
    let total = uncached_input_tokens.saturating_add(cache_read_input_tokens);
    if total == 0 {
        return None;
    }
    Some(((cache_read_input_tokens.saturating_mul(100)) / total) as u8)
}

fn execute_output_marks_item_complete(
    structured_output: Option<&Value>,
    item_id: &str,
) -> anyhow::Result<bool> {
    let Some(structured_output) = structured_output else {
        return Ok(false);
    };
    let execute = parse_feature_execute_output(structured_output.clone())?;
    Ok(execute
        .completed_tasks
        .iter()
        .any(|task_id| task_id == item_id))
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
                preview: crate::preview_text(&summary.summary, 120),
            })
            .collect(),
        expected_structured_sources,
        resolved_structured_sources,
        missing_structured_sources,
        structured_input_preview: step_input
            .structured_input
            .as_ref()
            .map(|value| crate::preview_json_value(value, 160)),
        todo_state_preview: step_input
            .todo_snapshot
            .as_deref()
            .map(|text| crate::preview_text(text, 160)),
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

fn update_document_supervision_hits(
    supervision_state: &Arc<Mutex<ContextSupervisionState>>,
    tool_name: &str,
    tool_result: &CoreToolResult,
) {
    if tool_name != "search_codebase" || tool_result.is_error() {
        return;
    }

    let summary = build_document_hit_summary(tool_result);
    supervision_state.lock().unwrap().document_hits = Some(summary);
}

fn update_memory_supervision_hits(
    supervision_state: &Arc<Mutex<ContextSupervisionState>>,
    diagnostics: &StepDiagnostics,
) {
    let summary = (!diagnostics.input.summary_sources.is_empty()).then(|| MemoryHitSummary {
        selected_count: diagnostics.input.summary_sources.len() as u32,
        total_tokens: diagnostics
            .context
            .as_ref()
            .map(|context| context.memory.current_summary_tokens)
            .unwrap_or_default(),
        top_hits: diagnostics
            .input
            .summary_sources
            .iter()
            .take(5)
            .map(|source| MemoryHitItem {
                workflow_id: source.workflow_id.clone(),
                step_id: source.step_id.clone(),
                title: source.title.clone(),
                preview: source.preview.clone(),
            })
            .collect(),
    });
    supervision_state.lock().unwrap().memory_hits = summary;
}

fn send_context_supervision_snapshot(
    tx: &dyn RuntimeMessageBridge,
    turn_id: u64,
    context: &ContextDiagnostics,
    supervision_state: &Arc<Mutex<ContextSupervisionState>>,
) {
    let snapshot = build_context_supervision_snapshot(context, &supervision_state.lock().unwrap());
    tx.send(crate::RuntimeMessageEnvelope::state(
        turn_id,
        crate::StateMessage::ContextSupervision {
            snapshot: Box::new(snapshot),
        },
    ));
}

fn send_step_knowledge_summary(
    tx: &dyn RuntimeMessageBridge,
    turn_id: u64,
    section_id: &str,
    context: &ContextDiagnostics,
    supervision_state: &Arc<Mutex<ContextSupervisionState>>,
) {
    let summary = build_step_knowledge_summary(context, &supervision_state.lock().unwrap());
    if let Some(summary) = summary {
        tx.send(crate::RuntimeMessageEnvelope::state(
            turn_id,
            crate::StateMessage::StepKnowledgeSummary {
                section_id: section_id.to_string(),
                summary: Box::new(summary),
            },
        ));
    }
}

fn build_step_knowledge_summary(
    context: &ContextDiagnostics,
    supervision_state: &ContextSupervisionState,
) -> Option<StepKnowledgeSummary> {
    let document = build_response_document_knowledge(context, supervision_state.document_hits.clone());
    let memory = build_response_memory_knowledge(context, supervision_state.memory_hits.clone());
    if document.is_none() && memory.is_none() {
        None
    } else {
        Some(StepKnowledgeSummary { document, memory })
    }
}

fn build_response_document_knowledge(
    context: &ContextDiagnostics,
    document_hits: Option<DocumentHitSummary>,
) -> Option<ResponseDocumentKnowledge> {
    let query_attempted = context
        .document
        .operator_usage
        .iter()
        .any(|usage| usage.operator == "search_codebase");
    let readiness = document_supervision_readiness_with_backend_enabled(context, cfg!(feature = "document-backend"));
    let hits = document_hits.as_ref();

    if hits.is_none() && !query_attempted {
        return None;
    }

    let no_promoted_store = context.document.active_version.is_none();
    let result_count = hits.map(|summary| summary.result_count).unwrap_or_default();
    let reason = if result_count > 0 {
        None
    } else if no_promoted_store {
        Some("no promoted store version".to_string())
    } else if query_attempted {
        Some("no matches returned".to_string())
    } else {
        Some("document recall attempted without a hit snapshot".to_string())
    };

    Some(ResponseDocumentKnowledge {
        raw_query: hits.map(|summary| summary.raw_query.clone()).unwrap_or_default(),
        planned_queries: hits
            .map(|summary| summary.planned_queries.clone())
            .unwrap_or_default(),
        rewrite_reason: hits.and_then(|summary| summary.rewrite_reason.clone()),
        rewrite_queries: hits
            .map(|summary| summary.rewrite_queries.clone())
            .unwrap_or_default(),
        recovery_path: hits.and_then(|summary| summary.recovery_path.clone()),
        readiness,
        query: hits.map(|summary| summary.query.clone()).unwrap_or_default(),
        mode: hits
            .map(|summary| summary.mode.clone())
            .unwrap_or_else(|| "unknown".to_string()),
        degraded_from: hits.and_then(|summary| summary.degraded_from.clone()),
        reason,
        result_count,
        top_hits: hits
            .map(|summary| summary.top_hits.iter().take(3).cloned().collect())
            .unwrap_or_default(),
    })
}

fn build_response_memory_knowledge(
    context: &ContextDiagnostics,
    memory_hits: Option<MemoryHitSummary>,
) -> Option<ResponseMemoryKnowledge> {
    let current_query = context.memory.current_query.as_ref();
    let current_observations = context.memory.current_observations.as_ref();
    let hits = memory_hits.as_ref();

    if current_query.is_none() && current_observations.is_none() && hits.is_none() {
        return None;
    }

    Some(ResponseMemoryKnowledge {
        raw_query: current_query
            .map(|query| query.raw_query.clone())
            .or_else(|| current_observations.map(|query| query.raw_query.clone())),
        planned_queries: current_query
            .map(|query| query.planned_queries.clone())
            .or_else(|| current_observations.map(|query| query.planned_queries.clone()))
            .unwrap_or_default(),
        rewrite_reason: current_query
            .and_then(|query| query.rewrite_reason.clone())
            .or_else(|| current_observations.and_then(|query| query.rewrite_reason.clone())),
        rewrite_queries: current_query
            .map(|query| query.rewrite_queries.clone())
            .or_else(|| current_observations.map(|query| query.rewrite_queries.clone()))
            .unwrap_or_default(),
        recovery_path: current_query
            .and_then(|query| query.recovery_path.clone())
            .or_else(|| current_observations.and_then(|query| query.recovery_path.clone())),
        memory_query: current_query.map(|query| query.query.clone()),
        observation_query: current_observations.map(|query| query.query.clone()),
        selected_summary_count: hits.map(|summary| summary.selected_count).unwrap_or_default(),
        top_selected_summaries: hits
            .map(|summary| summary.top_hits.iter().take(3).cloned().collect())
            .unwrap_or_default(),
        memory_hit_count: current_query.map(|query| query.result_count).unwrap_or_default(),
        observation_hit_count: current_observations
            .map(|query| query.result_count)
            .unwrap_or_default(),
        top_memory_hits: current_query
            .map(|summary| summary.top_hits.iter().take(3).cloned().collect())
            .unwrap_or_default(),
        top_observations: current_observations
            .map(|summary| {
                summary
                    .top_hits
                    .iter()
                    .take(3)
                    .cloned()
                    .collect::<Vec<ObservationRecallHitItem>>()
            })
            .unwrap_or_default(),
    })
}

fn build_context_supervision_snapshot(
    context: &ContextDiagnostics,
    supervision_state: &ContextSupervisionState,
) -> ContextSupervisionSnapshot {
    ContextSupervisionSnapshot {
        document: DocumentSupervisionSnapshot {
            enabled: cfg!(feature = "document-backend"),
            readiness: document_supervision_readiness(context),
            health_status: context.document.health_status,
            totals: DocumentSupervisionTotals {
                total_files_indexed: context.document.total_files_indexed,
                total_chunks: context.document.total_chunks,
                total_embeddings: context.document.total_embeddings,
                index_staleness_seconds: context.document.index_staleness_seconds,
                governance_health: context.document.governance_health,
                last_health_check: context.document.last_health_check,
                lance_db_size_bytes: context.store.lance_db_size_bytes,
                tantivy_index_size_bytes: context.store.tantivy_index_size_bytes,
            },
            active_version: context.document.active_version.clone(),
            pending_version: context.document.pending_version.clone(),
            last_promotion_error: context.document.last_promotion_error.clone(),
            recent_activity: context.document.recent_activity.clone(),
            operator_usage: context.document.operator_usage.clone(),
            current_hits: supervision_state.document_hits.clone(),
        },
        memory: MemorySupervisionSnapshot {
            enabled: true,
            readiness: memory_supervision_readiness(context),
            totals: MemorySupervisionTotals {
                total_turns_archived: context.memory.total_turns_archived,
                compactions_triggered: context.memory.compactions_triggered,
                current_summary_tokens: context.memory.current_summary_tokens,
                current_summary_count: context.memory.current_summary_count,
                turn_archive_count: context.store.turn_archive_count,
                turn_archive_size_bytes: context.store.turn_archive_size_bytes,
                retention_candidates_accepted: context.memory.retention_candidates_accepted,
                retention_candidates_dropped: context.memory.retention_candidates_dropped,
                dropped_candidates_by_profile: context.memory.dropped_candidates_by_profile.clone(),
                memory_query_count: context.memory.memory_query_count,
                memory_query_hit_mix: context.memory.memory_query_hit_mix.clone(),
                observation_count: context.memory.observation_count,
                observation_fresh_count: context.memory.observation_fresh_count,
                observation_stale_count: context.memory.observation_stale_count,
                observation_superseded_count: context.memory.observation_superseded_count,
                observation_corrected_count: context.memory.observation_corrected_count,
                observation_correction_activity: context.memory.observation_correction_activity,
            },
            current_hits: supervision_state.memory_hits.clone(),
            current_query: context.memory.current_query.clone(),
            current_observations: context.memory.current_observations.clone(),
        },
    }
}

fn document_supervision_readiness(context: &ContextDiagnostics) -> SupervisionReadiness {
    document_supervision_readiness_with_backend_enabled(context, cfg!(feature = "document-backend"))
}

fn document_supervision_readiness_with_backend_enabled(
    context: &ContextDiagnostics,
    document_backend_enabled: bool,
) -> SupervisionReadiness {
    if !document_backend_enabled {
        return SupervisionReadiness::Disabled;
    }
    if context.document.last_promotion_error.is_some() {
        return SupervisionReadiness::Failed;
    }
    if context.document.pending_version.is_some() {
        return SupervisionReadiness::Degraded;
    }
    if context.document.active_version.is_none() {
        if context.document.total_files_indexed == 0 && context.document.total_chunks == 0 {
            return SupervisionReadiness::Uninitialized;
        }
        return SupervisionReadiness::Degraded;
    }
    if matches!(context.document.governance_health, Some(HealthScore::Critical)) {
        return SupervisionReadiness::Degraded;
    }
    SupervisionReadiness::Ready
}

fn memory_supervision_readiness(context: &ContextDiagnostics) -> SupervisionReadiness {
    if context.memory.total_turns_archived == 0 && context.memory.current_summary_count == 0 {
        SupervisionReadiness::Idle
    } else {
        SupervisionReadiness::Ready
    }
}

fn build_document_hit_summary(tool_result: &CoreToolResult) -> DocumentHitSummary {
    let query = tool_result
        .metadata
        .get("query")
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .trim()
        .to_string();
    let results = serde_json::from_str::<Vec<SearchResult>>(&tool_result.output).unwrap_or_default();
    let mode = results
        .first()
        .map(|result| search_mode_label(result.mode_used).to_string())
        .or_else(|| {
            tool_result
                .metadata
                .get("mode")
                .and_then(|value| value.as_str())
                .map(ToOwned::to_owned)
        })
        .unwrap_or_else(|| "unknown".to_string());
    let degraded_from = results
        .iter()
        .find_map(|result| result.degraded_from.map(search_mode_label))
        .map(ToOwned::to_owned);
    let result_count = tool_result
        .metadata
        .get("result_count")
        .and_then(|value| value.as_u64())
        .unwrap_or(results.len() as u64) as u32;

    DocumentHitSummary {
        raw_query: query.clone(),
        planned_queries: (!query.is_empty()).then_some(vec![query.clone()]).unwrap_or_default(),
        rewrite_reason: None,
        rewrite_queries: Vec::new(),
        recovery_path: None,
        query,
        mode,
        degraded_from,
        result_count,
        top_hits: results
            .into_iter()
            .take(5)
            .map(|result| DocumentHitItem {
                path: result.path,
                preview: crate::preview_text(&result.preview, 140),
            })
            .collect(),
    }
}

fn search_mode_label(mode: SearchMode) -> &'static str {
    match mode {
        SearchMode::Keyword => "keyword",
        SearchMode::Semantic => "semantic",
        SearchMode::Hybrid => "hybrid",
    }
}

fn build_step_output_diagnostics(
    output_contract: &StepOutputContract,
    output_state: &OutputDiagnosticState<'_>,
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
            ..
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
        status: output_state.status,
        attempt_kind: output_state.attempt_kind,
        extracted_json_preview: output_state
            .structured_output
            .map(|value| crate::preview_json_value(value, 160)),
        previous_response_preview: output_state
            .previous_response_preview
            .map(ToOwned::to_owned),
        attempts: output_state.attempts,
        retry_count: output_state.retry_count,
        max_retries: output_state.max_retries,
        validation_error: output_state.validation_error.map(ToOwned::to_owned),
        recovery_decision: output_state.recovery_decision,
    }
}

fn build_text_context_write(
    path: impl Into<String>,
    before: Option<&str>,
    after: Option<&str>,
) -> Option<StepContextWrite> {
    build_context_write(
        path,
        before.map(|value| crate::preview_text(value, 160)),
        after.map(|value| crate::preview_text(value, 160)),
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

fn estimate_tokens(text: &str) -> u32 {
    let chars = text.chars().count();
    chars.div_ceil(TOKEN_ESTIMATE_DIVISOR) as u32
}

fn should_trigger_context_compaction(
    base_input_tokens: u32,
    available_input_budget: u32,
    summary_count: usize,
) -> bool {
    let threshold_tokens =
        available_input_budget.saturating_mul(CONTEXT_COMPACTION_THRESHOLD_PERCENT) / 100;
    base_input_tokens >= threshold_tokens || summary_count > MAX_UNCOMPACTED_SUMMARIES
}

fn rank_summary_candidates(
    step_summaries: &[StepSummary],
    step: &WorkflowStep,
    active_workflow_id: &str,
    has_execute_item: bool,
    compaction_triggered: bool,
) -> Vec<SummaryCandidate> {
    let total = step_summaries.len();
    let mut candidates = step_summaries
        .iter()
        .enumerate()
        .map(|(index, summary)| {
            let priority = classify_summary_priority(summary, step, active_workflow_id);
            let score = summary_relevance_score(
                summary,
                step,
                active_workflow_id,
                has_execute_item,
                total.saturating_sub(index),
            );
            let compacted_summary =
                maybe_compact_summary(summary, priority, compaction_triggered, index, total);
            SummaryCandidate {
                compacted: compacted_summary.summary != summary.summary,
                summary: compacted_summary,
                original_index: index,
                priority,
                score,
            }
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        slot_priority_rank(right.priority)
            .cmp(&slot_priority_rank(left.priority))
            .then_with(|| right.score.cmp(&left.score))
            .then_with(|| right.original_index.cmp(&left.original_index))
            .then_with(|| left.compacted.cmp(&right.compacted))
    });
    candidates
}

fn slot_priority_rank(priority: SlotPriority) -> u8 {
    match priority {
        SlotPriority::Medium => 2,
        SlotPriority::Low => 1,
    }
}

fn classify_summary_priority(
    summary: &StepSummary,
    step: &WorkflowStep,
    active_workflow_id: &str,
) -> SlotPriority {
    if is_input_source_summary(summary, step)
        || (step.id == EXECUTE_STEP_ID
            && matches!(summary.step_id.as_str(), PLAN_STEP_ID | EXECUTE_STEP_ID))
        || (step.id == crate::REPORT_STEP_ID
            && matches!(summary.step_id.as_str(), PLAN_STEP_ID | EXECUTE_STEP_ID))
    {
        SlotPriority::Medium
    } else if summary.workflow_id == active_workflow_id {
        SlotPriority::Medium
    } else if is_root_routing_summary(summary) {
        SlotPriority::Low
    } else {
        SlotPriority::Low
    }
}

fn is_input_source_summary(summary: &StepSummary, step: &WorkflowStep) -> bool {
    match &step.input_contract {
        StepInputContract::Required { sources } | StepInputContract::Optional { sources } => {
            sources.iter().any(|source| source == &summary.step_id)
        }
        StepInputContract::None => false,
    }
}

fn is_root_routing_summary(summary: &StepSummary) -> bool {
    summary.workflow_id == ROOT_WORKFLOW_ID
        && matches!(
            summary.step_id.as_str(),
            SCENE_RECOGNITION_STEP_ID | SELECT_WORKFLOW_STEP_ID
        )
}

fn summary_relevance_score(
    summary: &StepSummary,
    step: &WorkflowStep,
    active_workflow_id: &str,
    has_execute_item: bool,
    recency_score: usize,
) -> u32 {
    let mut score = recency_score as u32;
    if summary.workflow_id == active_workflow_id {
        score += 20;
    }
    if is_input_source_summary(summary, step) {
        score += 80;
    }
    if step.id == EXECUTE_STEP_ID {
        match summary.step_id.as_str() {
            PLAN_STEP_ID => score += 70,
            EXECUTE_STEP_ID => score += 55,
            _ => {}
        }
    }
    if step.id == crate::REPORT_STEP_ID {
        match summary.step_id.as_str() {
            EXECUTE_STEP_ID => score += 80,
            PLAN_STEP_ID => score += 50,
            _ => {}
        }
    }
    if has_execute_item && matches!(summary.step_id.as_str(), PLAN_STEP_ID | EXECUTE_STEP_ID) {
        score += 15;
    }
    if is_root_routing_summary(summary) {
        score = score.saturating_sub(40);
    }
    score
}

fn step_uses_json_output(step: &WorkflowStep) -> bool {
    matches!(
        step.output_contract,
        StepOutputContract::Required {
            format: DataFormat::Json,
            ..
        } | StepOutputContract::Optional {
            format: DataFormat::Json,
            ..
        }
    )
}

fn canonical_step_summary_text(
    step: &WorkflowStep,
    final_text: &str,
    structured_output: Option<&Value>,
) -> String {
    if step_uses_json_output(step) {
        if let Some(output) = structured_output {
            return summarize_step_text(&crate::preview_json_value(output, SUMMARY_CHAR_LIMIT));
        }
    }

    summarize_step_text(final_text)
}

fn step_requires_structured_execute_output(
    workflow_id: &str,
    step: &WorkflowStep,
    current_item: Option<&ExecuteLoopItemContext>,
) -> bool {
    matches!(
        workflow_id,
        FEATURE_WORKFLOW_ID | RESEARCH_WORKFLOW_ID | DEEP_RESEARCH_WORKFLOW_ID
    )
        && step.id == EXECUTE_STEP_ID
        && current_item.is_some()
        && matches!(step.loop_contract, Some(StepLoopContract::TodoItems { .. }))
        && step
            .hooks
            .iter()
            .any(|hook_id| hook_id == "todo_managed_execute")
}
fn maybe_compact_summary(
    summary: &StepSummary,
    priority: SlotPriority,
    compaction_triggered: bool,
    index: usize,
    total: usize,
) -> StepSummary {
    let target_limit = match (priority, compaction_triggered) {
        (SlotPriority::Medium, true) if index + 2 < total => Some(COMPACTED_SUMMARY_CHAR_LIMIT),
        (SlotPriority::Low, true) => Some(AGGRESSIVE_COMPACTED_SUMMARY_CHAR_LIMIT),
        _ => None,
    };
    let Some(target_limit) = target_limit else {
        return summary.clone();
    };
    let compacted = compact_summary_text(&summary.summary, target_limit);
    if compacted == summary.summary {
        return summary.clone();
    }
    StepSummary {
        workflow_id: summary.workflow_id.clone(),
        step_id: summary.step_id.clone(),
        title: summary.title.clone(),
        estimated_tokens: estimate_tokens(&compacted),
        summary: compacted,
    }
}

fn compact_summary_text(text: &str, limit: usize) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= limit {
        return trimmed.to_string();
    }
    if limit <= 16 {
        return truncate_chars(trimmed, limit);
    }

    let head_len = limit.saturating_mul(2) / 3;
    let tail_len = limit.saturating_sub(head_len).saturating_sub(3);
    let head = truncate_chars(trimmed, head_len);
    let tail = tail_chars(trimmed, tail_len);
    format!("{}...{}", head.trim_end(), tail.trim_start())
}

fn tail_chars(text: &str, limit: usize) -> String {
    if limit == 0 {
        return String::new();
    }
    let chars = text.chars().collect::<Vec<_>>();
    if chars.len() <= limit {
        return text.to_string();
    }
    chars[chars.len() - limit..].iter().collect()
}

fn truncate_chars(text: &str, limit: usize) -> String {
    text.chars().take(limit).collect()
}

fn summarize_step_text(text: &str) -> String {
    truncate_chars(text.trim(), SUMMARY_CHAR_LIMIT)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    use omega_client::test_support::{IdleLlmClient, ScriptedLlmClientBuilder};
    use omega_client::{ChatResponse, ContentBlock, STOP_REASON_END_TURN};
    use omega_context::{ContextCacheDiagnostics, ContextTokenCountSource, OmegaContextFacade};
    use omega_core::{DynLlmClient, TodoManager};
    use omega_hooks::HookHost;
    use omega_test_support::persistent_test_root;
    use omega_workflow::{
        DataFormat, OutputRecoveryMode, StepInputContract, StepLoopMode, StepOutputContract,
        StepSkillRequest, StepToolRequest, WorkflowStep, LoadedWorkflowCatalog,
        RESEARCH_WORKFLOW_ID, ROOT_WORKFLOW_ID,
    };
    use tokio::sync::watch;

    use super::{
        build_context_compaction_log, classify_summary_priority, compact_summary_text,
        document_supervision_readiness_with_backend_enabled, maybe_compact_summary,
        rank_summary_candidates, should_trigger_context_compaction, step_summary_to_context,
        validate_execute_step_output, SlotPriority, WorkflowTurnRunner,
    };
    use crate::{
        output::parse_structured_output_candidates,
        session_state::{SessionContext, StepSummary}, RuntimeContentKind,
        RuntimeEnvelopeRecorder, RuntimeMessage, RuntimeSource, SessionSkillCatalog,
        SessionToolCatalog, StateMessage, EXECUTE_STEP_ID, PLAN_STEP_ID,
    };
    use omega_context::{
        ContextDiagnostics, ContextDocumentDiagnostics, ContextMemoryDiagnostics,
        ContextStoreDiagnostics, SupervisionReadiness,
    };
    use omega_core::{TodoItem, TodoStatus};
    use serde_json::json;

    fn workflow_step(step_id: &str, sources: Vec<&str>) -> WorkflowStep {
        WorkflowStep {
            id: step_id.to_string(),
            label: step_id.to_string(),
            prompt_path: PathBuf::from(format!(".omega/prompt/{step_id}.md")),
            loop_mode: StepLoopMode::AgentLoop,
            loop_contract: None,
            max_iterations: 8,
            max_step_repeats: 2,
            hooks: Vec::new(),
            tool_request: StepToolRequest::Inherit,
            skill_request: StepSkillRequest::MatchTask,
            input_contract: if sources.is_empty() {
                StepInputContract::None
            } else {
                StepInputContract::Optional {
                    sources: sources.into_iter().map(ToOwned::to_owned).collect(),
                }
            },
            output_contract: StepOutputContract::Required {
                format: DataFormat::Json,
                schema_path: None,
                max_retries: 1,
                recovery_mode: OutputRecoveryMode::RepairThenRegenerate,
            },
            enabled: true,
        }
    }

    fn summary(workflow_id: &str, step_id: &str, text: &str) -> StepSummary {
        StepSummary {
            workflow_id: workflow_id.to_string(),
            step_id: step_id.to_string(),
            title: step_id.to_string(),
            estimated_tokens: super::estimate_tokens(text),
            summary: text.to_string(),
        }
    }

    #[test]
    fn rank_summary_candidates_prefers_step_inputs_over_newer_routing_history() {
        let step = workflow_step(EXECUTE_STEP_ID, vec!["explore", "plan", "execute"]);
        let summaries = vec![
            summary(
                omega_workflow::FEATURE_WORKFLOW_ID,
                "explore",
                "Explore found the root cause in the session runner.",
            ),
            summary(
                omega_workflow::FEATURE_WORKFLOW_ID,
                PLAN_STEP_ID,
                "Plan agreed to update slot budgeting and summary selection.",
            ),
            summary(
                omega_workflow::ROOT_WORKFLOW_ID,
                omega_workflow::SELECT_WORKFLOW_STEP_ID,
                "Selected workflow: feature.",
            ),
        ];

        let ranked = rank_summary_candidates(
            &summaries,
            &step,
            omega_workflow::FEATURE_WORKFLOW_ID,
            true,
            false,
        );

        assert_eq!(ranked[0].summary.step_id, PLAN_STEP_ID);
        assert!(
            ranked
                .iter()
                .position(|candidate| candidate.summary.step_id == PLAN_STEP_ID)
                .unwrap()
                < ranked
                    .iter()
                    .position(|candidate| {
                        candidate.summary.step_id == omega_workflow::SELECT_WORKFLOW_STEP_ID
                    })
                    .unwrap()
        );
    }

    #[test]
    fn maybe_compact_summary_only_shrinks_low_priority_history_when_triggered() {
        let low = summary(
            omega_workflow::ROOT_WORKFLOW_ID,
            omega_workflow::SCENE_RECOGNITION_STEP_ID,
            &format!(
                "Recognized scene with extra detail. {} tail-marker",
                "x".repeat(500)
            ),
        );
        let compacted = maybe_compact_summary(&low, SlotPriority::Low, true, 0, 8);
        assert!(compacted.summary.len() < low.summary.len());
        assert!(compacted.summary.contains("Recognized scene"));
        assert!(compacted.summary.contains("tail-marker"));
        let medium = summary(
            omega_workflow::FEATURE_WORKFLOW_ID,
            PLAN_STEP_ID,
            &format!("Plan summary {}", "y".repeat(500)),
        );
        let preserved = maybe_compact_summary(&medium, SlotPriority::Medium, false, 0, 8);
        assert_eq!(preserved.summary, medium.summary);
    }


    #[test]
    fn validate_execute_step_output_accepts_single_execute_object_wrapped_in_prose() {
        let response = concat!(
            "修复完成，结果如下：\n",
            "{\"completed_tasks\":[\"task-1\"],\"open_tasks\":[\"task-2\"],\"validation_results\":[{\"target\":\"cargo test -p omega-session\",\"status\":\"passed\"}],\"changed_paths\":[\"crates/omega-session/src/runner.rs\"]}\n",
            "后续继续观察。"
        );
        let candidates = parse_structured_output_candidates(DataFormat::Json, response);

        let result = validate_execute_step_output(response, &candidates);

        assert!(result.is_ok());
    }

    #[test]
    fn validate_execute_step_output_rejects_non_object_wrappers_without_execute_candidate() {
        let response = "执行摘要\n[\"task-1\", \"task-2\"]";
        let candidates = parse_structured_output_candidates(DataFormat::Json, response);

        let error = validate_execute_step_output(response, &candidates).unwrap_err();

        assert!(error
            .to_string()
            .contains("execute step must resolve to a single JSON object"));
    }

    #[test]
    fn compact_summary_text_preserves_head_and_tail_markers() {
        let text = format!("head-marker {} tail-marker", "body ".repeat(60));
        let compacted = compact_summary_text(&text, 80);
        assert!(compacted.chars().count() <= 83);
        assert!(compacted.contains("head-marker"));
        assert!(compacted.contains("tail-marker"));
        assert!(compacted.contains("..."));
    }

    #[test]
    fn compaction_trigger_follows_budget_or_history_threshold() {
        assert!(should_trigger_context_compaction(710, 1_000, 2));
        assert!(should_trigger_context_compaction(120, 1_000, 6));
        assert!(!should_trigger_context_compaction(500, 1_000, 3));
    }

    #[test]
    fn classify_summary_priority_marks_execute_inputs_as_medium() {
        let step = workflow_step(EXECUTE_STEP_ID, vec!["explore", "plan", "execute"]);
        let plan_summary = summary(omega_workflow::FEATURE_WORKFLOW_ID, PLAN_STEP_ID, "plan");
        let routing_summary = summary(
            omega_workflow::ROOT_WORKFLOW_ID,
            omega_workflow::SELECT_WORKFLOW_STEP_ID,
            "selected workflow",
        );

        assert_eq!(
            classify_summary_priority(&plan_summary, &step, omega_workflow::FEATURE_WORKFLOW_ID,),
            SlotPriority::Medium
        );
        assert_eq!(
            classify_summary_priority(&routing_summary, &step, omega_workflow::FEATURE_WORKFLOW_ID,),
            SlotPriority::Low
        );
    }

    #[test]
    fn build_context_compaction_log_reports_dropped_history() {
        let selected = vec![step_summary_to_context(summary(
            omega_workflow::FEATURE_WORKFLOW_ID,
            PLAN_STEP_ID,
            "plan summary",
        ))];
        let cache = ContextCacheDiagnostics {
            token_count_source: ContextTokenCountSource::Estimated,
            request_input_tokens: 8_200,
            budget_input_tokens: 6_000,
            cache_breakpoints: Vec::new(),
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
            uncached_input_tokens: None,
            cache_hit_ratio_percent: None,
        };

        let summary = build_context_compaction_log(4, &selected, &cache)
            .expect("expected compaction summary when history is dropped");

        assert!(summary.contains("context.compact"));
        assert!(summary.contains("selected=1/4"));
        assert!(summary.contains("token_source=estimated"));
    }

    #[test]
    fn ensure_selected_workflow_replaces_root_child_target_with_scene_workflow() {
        let client: DynLlmClient = Arc::new(IdleLlmClient::new(
            "chat should not run in ensure_selected_workflow test",
        ));
        let cwd = persistent_test_root("session-runner-selected-workflow-root-guard");
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let loaded_catalog = LoadedWorkflowCatalog::load(&cwd);
        let context_facade = Arc::new(OmegaContextFacade::local(cwd.clone()));
        let skill_catalog = Arc::new(SessionSkillCatalog::default());
        let tool_catalog = Arc::new(SessionToolCatalog::new(Vec::new()));
        let todo_manager = Arc::new(Mutex::new(TodoManager::new()));
        let hook_host = Arc::new(HookHost::load(&cwd).unwrap());
        let (_cancel_tx, cancel_rx) = watch::channel(0u64);
        let recorder = RuntimeEnvelopeRecorder::new();
        let bridge = recorder.runtime_bridge();
        let runner = WorkflowTurnRunner::new(
            runtime.handle(),
            &client,
            &context_facade,
            &skill_catalog,
            &tool_catalog,
            "system",
            "session-test",
            "inspect routing",
            &cwd,
            &todo_manager,
            &hook_host,
            &loaded_catalog.scene_catalog,
            &loaded_catalog.workflow_catalog,
            &loaded_catalog.prompt_catalog,
            200_000,
            32_000,
            91,
            cancel_rx,
            bridge.clone(),
            bridge,
        );
        let mut session_context = SessionContext::new(ROOT_WORKFLOW_ID);
        session_context.routing.recognized_scene_id = Some("research".to_string());
        session_context.routing.selected_workflow_id = Some(ROOT_WORKFLOW_ID.to_string());

        let selected = runner.ensure_selected_workflow(&mut session_context);

        assert_eq!(selected, RESEARCH_WORKFLOW_ID);
        assert_eq!(
            session_context.routing.selected_workflow_id.as_deref(),
            Some(RESEARCH_WORKFLOW_ID)
        );
        let messages = recorder.runtime_messages();
        assert!(messages.iter().any(|envelope| {
            matches!(
                &envelope.message,
                RuntimeMessage::State(StateMessage::SessionRouting(status))
                    if status.recognized_scene_id.as_deref() == Some("research")
                        && status.selected_workflow_id.as_deref() == Some(RESEARCH_WORKFLOW_ID)
            )
        }));
        assert!(messages.iter().any(|envelope| {
            matches!(
                &envelope.message,
                RuntimeMessage::State(StateMessage::Activity {
                    source: RuntimeSource::SessionRouting,
                    kind: RuntimeContentKind::Summary,
                    text,
                    ..
                }) if text.contains("Ignoring root workflow as child target")
            )
        }));
    }

    #[test]
    fn apply_routed_skill_load_action_skips_when_no_selected_skills() {
        let client: DynLlmClient = Arc::new(IdleLlmClient::new(
            "chat should not run in routed skill load skip test",
        ));
        let cwd = persistent_test_root("session-runner-skip-routed-skill-load");
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let loaded_catalog = LoadedWorkflowCatalog::load(&cwd);
        let context_facade = Arc::new(OmegaContextFacade::local(cwd.clone()));
        let skill_catalog = Arc::new(SessionSkillCatalog::default());
        let tool_catalog = Arc::new(SessionToolCatalog::new(Vec::new()));
        let todo_manager = Arc::new(Mutex::new(TodoManager::new()));
        let hook_host = Arc::new(HookHost::load(&cwd).unwrap());
        let (_cancel_tx, cancel_rx) = watch::channel(0u64);
        let recorder = RuntimeEnvelopeRecorder::new();
        let bridge = recorder.runtime_bridge();
        let runner = WorkflowTurnRunner::new(
            runtime.handle(),
            &client,
            &context_facade,
            &skill_catalog,
            &tool_catalog,
            "system",
            "session-test",
            "inspect routing",
            &cwd,
            &todo_manager,
            &hook_host,
            &loaded_catalog.scene_catalog,
            &loaded_catalog.workflow_catalog,
            &loaded_catalog.prompt_catalog,
            200_000,
            32_000,
            92,
            cancel_rx,
            bridge.clone(),
            bridge,
        );
        let mut session_context = SessionContext::new(ROOT_WORKFLOW_ID);
        session_context.skill_routing.loaded_skill_ids = vec!["stale".to_string()];
        session_context.skill_routing.ignored_skill_ids = vec!["missing".to_string()];

        runner.apply_routed_skill_load_action(&mut session_context);

        assert!(session_context.skill_routing.selected_skill_ids.is_empty());
        assert!(session_context.skill_routing.loaded_skill_ids.is_empty());
        assert!(session_context.skill_routing.ignored_skill_ids.is_empty());
        let messages = recorder.runtime_messages();
        assert!(!messages.iter().any(|envelope| {
            matches!(
                &envelope.message,
                RuntimeMessage::State(StateMessage::Activity {
                    source: RuntimeSource::SessionRouting,
                    kind: RuntimeContentKind::Summary,
                    text,
                    ..
                }) if text.contains("Loaded routed skills")
            )
        }));
    }

    #[test]
    fn build_step_execution_input_rewrites_recall_queries_after_initial_empty_hits() {
        let client = Arc::new(
            ScriptedLlmClientBuilder::default()
                .push_response(ChatResponse {
                    id: "msg-recall-rewrite".to_string(),
                    model: Some("test-model".to_string()),
                    content: vec![ContentBlock::text(
                        r#"{"queries":["omega-context recall planner","memory query planner"]}"#,
                    )],
                    stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                    usage: None,
                })
                .build(),
        );
        let client_dyn: DynLlmClient = client.clone();
        let cwd = persistent_test_root("session-runner-recall-rewrite");
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let loaded_catalog = LoadedWorkflowCatalog::load(&cwd);
        let context_facade = Arc::new(OmegaContextFacade::local(cwd.clone()));
        let skill_catalog = Arc::new(SessionSkillCatalog::default());
        let tool_catalog = Arc::new(SessionToolCatalog::new(Vec::new()));
        let todo_manager = Arc::new(Mutex::new(TodoManager::new()));
        let hook_host = Arc::new(HookHost::load(&cwd).unwrap());
        let (_cancel_tx, cancel_rx) = watch::channel(0u64);
        let recorder = RuntimeEnvelopeRecorder::new();
        let bridge = recorder.runtime_bridge();
        let runner = WorkflowTurnRunner::new(
            runtime.handle(),
            &client_dyn,
            &context_facade,
            &skill_catalog,
            &tool_catalog,
            "system",
            "session-test",
            "write the final report",
            &cwd,
            &todo_manager,
            &hook_host,
            &loaded_catalog.scene_catalog,
            &loaded_catalog.workflow_catalog,
            &loaded_catalog.prompt_catalog,
            200_000,
            32_000,
            92,
            cancel_rx,
            bridge.clone(),
            bridge,
        );
        let mut session_context = SessionContext::new(ROOT_WORKFLOW_ID);
        session_context.latest_user_turn = "Please produce a very detailed final report about how the memory and document recall system should improve over time without assuming any exact file anchor or crate name yet".to_string();
        session_context.routing.recognized_scene_id = Some("research".to_string());
        session_context.routing.selected_workflow_id = Some(RESEARCH_WORKFLOW_ID.to_string());
        session_context.routing.active_workflow_id = RESEARCH_WORKFLOW_ID.to_string();
        session_context.routing.active_workflow_role = crate::WorkflowRunRole::Child;

        let step_input = runner
            .build_step_execution_input(
                &[omega_core::Message::user("report please")],
                &session_context,
                &workflow_step(crate::REPORT_STEP_ID, vec![PLAN_STEP_ID]),
                "Write the report",
                None,
            )
            .unwrap();

        let query = step_input
            .context_diagnostics
            .memory
            .current_query
            .expect("expected memory query diagnostics after rewrite");
        assert_eq!(
            query.recovery_path.as_deref(),
            Some("rewritten_after_initial_empty_hit")
        );
        assert!(query
            .rewrite_queries
            .iter()
            .any(|value| value == "omega-context recall planner"));
        assert_eq!(client.recorded_requests().len(), 1);
    }

    #[test]
    fn sync_execute_output_does_not_reopen_previously_completed_items() {
        let client: DynLlmClient = Arc::new(IdleLlmClient::new(
            "execute reopen regression test should not call LLM",
        ));
        let cwd = persistent_test_root("session-runner-execute-does-not-reopen-completed");
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let loaded_catalog = LoadedWorkflowCatalog::load(&cwd);
        let context_facade = Arc::new(OmegaContextFacade::local(cwd.clone()));
        let skill_catalog = Arc::new(SessionSkillCatalog::default());
        let tool_catalog = Arc::new(SessionToolCatalog::new(Vec::new()));
        let todo_manager = Arc::new(Mutex::new(TodoManager::new()));
        {
            let mut manager = todo_manager.lock().unwrap();
            manager
                .update(vec![
                    TodoItem {
                        id: Some("task-1".to_string()),
                        text: "Inspect diagnostics path".to_string(),
                        status: TodoStatus::Completed,
                        active_form: None,
                    },
                    TodoItem {
                        id: Some("task-2".to_string()),
                        text: "Trace tool callback path".to_string(),
                        status: TodoStatus::InProgress,
                        active_form: Some("working on Trace tool callback path".to_string()),
                    },
                    TodoItem {
                        id: Some("task-3".to_string()),
                        text: "Compare archive paths".to_string(),
                        status: TodoStatus::Pending,
                        active_form: None,
                    },
                ])
                .unwrap();
        }
        let hook_host = Arc::new(HookHost::load(&cwd).unwrap());
        let (_cancel_tx, cancel_rx) = watch::channel(0u64);
        let recorder = RuntimeEnvelopeRecorder::new();
        let bridge = recorder.runtime_bridge();
        let runner = WorkflowTurnRunner::new(
            runtime.handle(),
            &client,
            &context_facade,
            &skill_catalog,
            &tool_catalog,
            "system",
            "session-test",
            "continue execute",
            &cwd,
            &todo_manager,
            &hook_host,
            &loaded_catalog.scene_catalog,
            &loaded_catalog.workflow_catalog,
            &loaded_catalog.prompt_catalog,
            200_000,
            32_000,
            93,
            cancel_rx,
            bridge.clone(),
            bridge,
        );

        runner
            .sync_todo_manager_from_execute_output(&json!({
                "completed_tasks": ["task-2"],
                "open_tasks": ["task-1", "task-3"],
                "validation_results": [],
                "changed_paths": []
            }))
            .unwrap();

        let manager = todo_manager.lock().unwrap();
        let statuses = manager
            .items()
            .iter()
            .map(|item| {
                (
                    item.id.clone().unwrap_or_default(),
                    item.status.clone(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            statuses,
            vec![
                ("task-1".to_string(), TodoStatus::Completed),
                ("task-2".to_string(), TodoStatus::Completed),
                ("task-3".to_string(), TodoStatus::InProgress),
            ]
        );
    }

    #[test]
    fn document_supervision_marks_missing_active_store_as_uninitialized() {
        let readiness = document_supervision_readiness_with_backend_enabled(&ContextDiagnostics {
            document: ContextDocumentDiagnostics {
                total_files_indexed: 0,
                total_chunks: 0,
                total_embeddings: 0,
                active_version: None,
                pending_version: None,
                last_promotion_error: None,
                ..ContextDocumentDiagnostics::default()
            },
            memory: ContextMemoryDiagnostics::default(),
            store: ContextStoreDiagnostics::default(),
            ..ContextDiagnostics::default()
        }, true);

        assert_eq!(readiness, SupervisionReadiness::Uninitialized);
    }
}
