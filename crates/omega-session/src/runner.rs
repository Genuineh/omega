use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use omega_core::{Agent, CoreSharedTodoManager, TodoItem, TodoStatus};
use omega_hooks::{HookAdvanceOutcome, HookHost};
use omega_workflow::{
    OutputRecoveryMode, SceneCatalog, StepInputContract, StepLoopContract,
    StepOutputContract, WorkflowCatalog, WorkflowPromptCatalog, WorkflowPrompts, WorkflowStep,
    EXECUTE_STEP_ID, FEATURE_SCENE_ID, FEATURE_WORKFLOW_ID, PLAN_STEP_ID,
    RESEARCH_WORKFLOW_ID, SCENE_RECOGNITION_STEP_ID, SELECT_WORKFLOW_STEP_ID,
};
use serde_json::Value;
use tokio::runtime::Handle;
use tokio::sync::watch;
use tracing::{debug, error, info};

use crate::output::{
    build_output_validation_feedback, parse_feature_execute_output, parse_feature_plan_output,
    parse_structured_output_candidates, validate_schema_file, validate_workflow_step_output,
};
use crate::hook_adapter::{ExecuteLoopItemContext, StepHookRuntime};
use crate::prompt_builder::{
    build_output_repair_system_prompt, build_step_system_prompt, render_output_contract,
    render_routing_context, render_structured_input, render_visible_tools,
};
use crate::routing::{
    find_catalog_match, latest_user_turn_prefers_research_scene,
    latest_user_turn_requires_feature_scene, parse_structured_id, parse_structured_id_from_value,
};
use crate::session_state::{SessionContext, StepSummary};
use crate::ui_emit::{
    send_routing_log, send_session_status, send_step_text, send_system_log_text,
    send_step_subflow_status, send_todo_snapshot, send_warning_text, send_workflow_step,
    StepResponseStreamer, ToolRunTracker,
};
use crate::{
    ExecuteProgressDiagnostics, ResolvedSkillSet, ResolvedToolSet, RuntimeMessageBridge,
    SessionSkillCatalog, SessionToolCatalog, SharedRuntimeMessageBridge, StepContextWrite,
    StepContextWriteKind, StepDiagnostics, StepInputDiagnostics, StepInputStatus,
    StepOutputAttemptKind, StepOutputContractMode, StepOutputDiagnostics,
    StepOutputRecoveryDecision, StepOutputStatus, StepSubflowState, StepSubflowStatus,
    StepSummarySource, WorkflowRunRole,
    CONTEXT_SAFETY_MARGIN_TOKENS, REPAIR_PASS_MAX_ITERATIONS, SUMMARY_CHAR_LIMIT,
    TOKEN_ESTIMATE_DIVISOR,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StepExecutionInput {
    pub(crate) base_system: String,
    pub(crate) cwd: PathBuf,
    pub(crate) resolved_tools: ResolvedToolSet,
    pub(crate) resolved_skills: ResolvedSkillSet,
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
    attempts: u32,
    retry_count: u32,
    max_retries: u32,
    validation_error: Option<&'a str>,
    previous_response_preview: Option<&'a str>,
    recovery_decision: Option<StepOutputRecoveryDecision>,
    session_writes: Vec<StepContextWrite>,
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

    pub(crate) fn extracted_json_preview(&self) -> Option<String> {
        self.extracted_json
            .as_ref()
            .map(|value| crate::preview_json_value(value, 600))
    }
}

#[allow(dead_code)]
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
    skill_catalog: &'a Arc<SessionSkillCatalog>,
    tool_catalog: &'a Arc<SessionToolCatalog>,
    base_system: &'a str,
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
}

impl<'a> WorkflowTurnRunner<'a> {
    pub(crate) fn new(
        handle: &'a Handle,
        skill_catalog: &'a Arc<SessionSkillCatalog>,
        tool_catalog: &'a Arc<SessionToolCatalog>,
        base_system: &'a str,
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
            skill_catalog,
            tool_catalog,
            base_system,
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
        }
    }

    pub(crate) fn run(
        &self,
        agent: &mut Agent,
        session_context: &mut SessionContext,
    ) -> anyhow::Result<String> {
        self.ensure_turn_active()?;
        let hook_session = Arc::new(Mutex::new(self.hook_host.start_session()));
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
        )?;

        self.ensure_turn_active()?;

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
            hook_session,
        )
    }

    fn run_workflow(
        &self,
        agent: &mut Agent,
        workflow_id: &str,
        role: WorkflowRunRole,
        session_context: &mut SessionContext,
        hook_session: Arc<Mutex<omega_hooks::HookSession>>,
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
            let step_input =
                match self.build_step_execution_input(
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
            let base_system_prompt = build_step_system_prompt(&step_input);
            agent.set_system(base_system_prompt.clone());

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
            );
            response_streamer.begin();
            let step_attempt_checkpoint = agent.messages().to_vec();
            let max_validation_retries = max_output_validation_retries(&step.output_contract);
            let mut validation_attempt = 0u32;
            let mut last_validation_error = None::<String>;
            let mut last_validation_failure = None::<OutputValidationFailure>;
            let mut current_attempt_kind = StepOutputAttemptKind::Primary;
            let mut attempt_tools = step_input.resolved_tools.clone();
            let mut attempt_max_iterations = step.max_iterations;
            let (stage_text, structured_output, validation_attempts) = loop {
                let stage_text = match self.execute_step(
                    agent,
                    &attempt_tools,
                    attempt_max_iterations,
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

                match self.validate_step_output(
                    workflow_id,
                    &step,
                    current_execute_item.as_ref(),
                    &stage_text,
                ) {
                    Ok(structured_output) => {
                        response_streamer.complete();
                        break (
                            stage_text,
                            structured_output,
                            completed_output_attempts(&step.output_contract, validation_attempt),
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
                            &step_input,
                            OutputDiagnosticState {
                                status: StepOutputStatus::Invalid,
                                attempt_kind: current_attempt_kind,
                                structured_output: validation_failure.extracted_json.as_ref(),
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
                                    next_retry_attempt_kind(
                                        &step.output_contract,
                                        validation_attempt + 1,
                                    ),
                                ),
                                session_writes: Vec::new(),
                            },
                            ExecuteLoopProgressState {
                                current_item: current_execute_item.clone(),
                                repeat_count: current_item_repeat_count,
                                completion_source: None,
                            },
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
                                next_retry_attempt_kind(
                                    &step.output_contract,
                                    validation_attempt + 1,
                                ),
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
                                step.id,
                                attempts,
                                validation_failure.message
                            ));
                            return Err(anyhow::anyhow!(
                                "step '{}' failed output validation after {} attempt(s): {}",
                                step.id,
                                attempts,
                                validation_failure.message
                            ));
                        }

                        validation_attempt += 1;
                        let attempt_kind =
                            next_retry_attempt_kind(&step.output_contract, validation_attempt);
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
                                agent.set_system(build_output_repair_system_prompt(
                                    &step_input,
                                    &validation_failure,
                                ));
                            }
                            StepOutputAttemptKind::Regenerate => {
                                attempt_tools = step_input.resolved_tools.clone();
                                attempt_max_iterations = step.max_iterations;
                                agent.set_system(base_system_prompt.clone());
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

            let progress_completion_source = self.execute_completion_source(
                &step,
                current_execute_item.as_ref(),
                step_result.structured_output.as_ref(),
            )?;

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
                &step_input,
                OutputDiagnosticState {
                    status: output_status,
                    attempt_kind: current_attempt_kind,
                    structured_output: step_result.structured_output.as_ref(),
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
            );

            if let Some(current_item) = current_execute_item.as_ref() {
                let subflow_state = match step_result.transition {
                    StepTransition::Repeat => StepSubflowState::Running,
                    StepTransition::RepeatItem | StepTransition::Continue | StepTransition::FinishTurn => {
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
                        item_repeat_counts.remove(&self.execute_item_repeat_key(&step, current_item));
                    }
                }
                _ => {
                    step_repeat_counts.remove(&step.id);
                    if let Some(current_item) = current_execute_item.as_ref() {
                        item_repeat_counts.remove(&self.execute_item_repeat_key(&step, current_item));
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

            if !matches!(step_result.transition, StepTransition::Repeat | StepTransition::RepeatItem) {
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
        response_streamer: &mut StepResponseStreamer<'_>,
        hook_runtime: Option<StepHookRuntime>,
    ) -> anyhow::Result<String> {
        let tool_name_refs = resolved_tools.tool_name_refs();
        agent.set_visible_tools(Some(&tool_name_refs));
        agent.set_max_iterations(max_iterations);

        let tool_runs = Arc::new(Mutex::new(ToolRunTracker::new(
            &*self.tx_callback,
            self.turn_id,
            response_streamer.primary_section_id().to_string(),
        )));
        let hook_error = Arc::new(Mutex::new(None::<String>));
        let mut cancel_turn_rx = self.cancel_turn_rx.clone();

        let stage_text = self.handle.block_on(agent.run_loop_with_events_until_turn_change(
            {
                let tx_callback = self.tx_callback.clone();
                let turn_id = self.turn_id;
                let tool_runs = tool_runs.clone();
                let hook_runtime = hook_runtime.clone();
                let hook_error = hook_error.clone();
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

                    if name == "todo" && !tool_result.is_error() {
                        send_todo_snapshot(&tx_callback, turn_id, &tool_result.output);
                    }

                    if let Some(hook_runtime) = &hook_runtime {
                        if let Err(error) =
                            hook_runtime.after_tool_call(tool_use_id, name, tool_input, tool_result)
                        {
                            *hook_error.lock().unwrap() = Some(error.to_string());
                        }
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
            Some(&mut cancel_turn_rx),
            Some(self.turn_id),
        ))?;

        if let Some(error) = hook_error.lock().unwrap().take() {
            return Err(anyhow::anyhow!(error));
        }

        Ok(stage_text)
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
        session_context: &SessionContext,
        step: &WorkflowStep,
        step_prompt: &str,
        current_execute_item: Option<ExecuteLoopItemContext>,
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
            cwd: self.cwd.to_path_buf(),
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
            current_execute_item,
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
                self.cwd,
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
        if !matches!(step.id.as_str(), EXECUTE_STEP_ID | crate::REPORT_STEP_ID) {
            return None;
        }
        if !session_context.step_outputs.contains_key(PLAN_STEP_ID) {
            return None;
        }

        let manager = self.todo_manager.lock().ok()?;
        (!manager.items().is_empty()).then(|| manager.render())
    }

    fn send_step_diagnostics_effect(&self, diagnostics: StepDiagnostics) {
        self.tx_result.send(crate::RuntimeMessageEnvelope::state(
            self.turn_id,
            crate::StateMessage::Diagnostics {
                diagnostics: Box::new(diagnostics),
            },
        ));
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
                attempts: 0,
                retry_count: 0,
                max_retries: max_output_validation_retries(&context.step.output_contract),
                validation_error: None,
                previous_response_preview: None,
                recovery_decision: None,
                session_writes: Vec::new(),
            },
        );
        self.send_step_diagnostics_effect(build_step_diagnostics(
            context,
            self.build_execute_progress_diagnostics(context.step, &progress_state),
            build_step_input_diagnostics(step_input),
            output,
            Vec::new(),
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
                attempts: 0,
                retry_count: 0,
                max_retries: max_output_validation_retries(&context.step.output_contract),
                validation_error: None,
                previous_response_preview: None,
                recovery_decision: None,
                session_writes: Vec::new(),
            },
        );
        self.send_step_diagnostics_effect(build_step_diagnostics(
            context,
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
        progress_state: ExecuteLoopProgressState,
    ) {
        let diagnostics = build_step_diagnostics(
            context,
            self.build_execute_progress_diagnostics(context.step, &progress_state),
            build_step_input_diagnostics(step_input),
            build_step_output_diagnostics(&context.step.output_contract, &output_state),
            output_state.session_writes,
        );
        self.send_step_diagnostics_effect(diagnostics);
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
            child_step_prefix,
            ..
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
                items.iter()
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
            StepTransition::Continue | StepTransition::StartWorkflow { .. } | StepTransition::FinishTurn
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
        let mut transition = if role == WorkflowRunRole::Child && is_final_step {
            StepTransition::FinishTurn
        } else {
            StepTransition::Continue
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
                let scene_id = session_context
                    .routing
                    .recognized_scene_id
                    .clone()
                    .unwrap_or_else(|| self.scene_catalog.default_scene_id.clone());
                let selected_workflow_id = self.resolve_workflow_from_output(
                    structured_output.as_ref(),
                    &final_text,
                    &scene_id,
                    &session_context.latest_user_turn,
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
            StepTransition::Continue | StepTransition::StartWorkflow { .. } | StepTransition::FinishTurn
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
            PLAN_STEP_ID if matches!(workflow_id, FEATURE_WORKFLOW_ID | RESEARCH_WORKFLOW_ID) => {
                self.sync_todo_manager_from_plan_output(structured_output)
            }
            EXECUTE_STEP_ID
                if matches!(workflow_id, FEATURE_WORKFLOW_ID | RESEARCH_WORKFLOW_ID) =>
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
        match &step.output_contract {
            StepOutputContract::None => Ok(None),
            StepOutputContract::Required {
                format,
                schema_path,
                ..
            } => {
                let candidates = parse_structured_output_candidates(*format, final_text);
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

                    if let Err(error) = validate_workflow_step_output(workflow_id, step, &value) {
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

                    if let Err(error) = validate_workflow_step_output(workflow_id, step, &value) {
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
                } else {
                    Ok(None)
                }
            }
        }
    }

    fn validate_itemized_execute_output(
        &self,
        workflow_id: &str,
        step: &WorkflowStep,
        current_item: Option<&ExecuteLoopItemContext>,
        value: &Value,
    ) -> anyhow::Result<()> {
        if !matches!(workflow_id, FEATURE_WORKFLOW_ID | RESEARCH_WORKFLOW_ID)
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
        let known_ids = manager
            .items()
            .iter()
            .filter_map(|item| item.id.as_deref())
            .collect::<std::collections::BTreeSet<_>>();
        let allowed_completed = manager
            .items()
            .iter()
            .filter(|item| item.status == TodoStatus::Completed)
            .filter_map(|item| item.id.as_deref())
            .chain(std::iter::once(current_item.item_id.as_str()))
            .collect::<std::collections::BTreeSet<_>>();

        for task_id in execute.completed_tasks.iter().map(|task_id| task_id.trim()) {
            if !known_ids.contains(task_id) {
                anyhow::bail!(
                    "execute output completed_tasks contains unknown todo item '{}'",
                    task_id
                );
            }
            if !allowed_completed.contains(task_id) {
                anyhow::bail!(
                    "itemized execute output cannot complete future todo item '{}' while current item is '{}'",
                    task_id,
                    current_item.item_id
                );
            }
        }

        for task_id in execute.open_tasks.iter().map(|task_id| task_id.trim()) {
            if !known_ids.contains(task_id) {
                anyhow::bail!(
                    "execute output open_tasks contains unknown todo item '{}'",
                    task_id
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
        if !matches!(workflow_id, FEATURE_WORKFLOW_ID | RESEARCH_WORKFLOW_ID)
            || step.id != EXECUTE_STEP_ID
            || !matches!(step.loop_contract, Some(StepLoopContract::TodoItems { .. }))
        {
            return None;
        }

        let current_item = current_item?;
        let execute = parse_feature_execute_output(value.clone()).ok()?;

        let manager = self.todo_manager.lock().ok()?;
        let allowed_completed: std::collections::BTreeSet<&str> = manager
            .items()
            .iter()
            .filter(|item| item.status == TodoStatus::Completed)
            .filter_map(|item| item.id.as_deref())
            .chain(std::iter::once(current_item.item_id.as_str()))
            .collect();

        let mut repaired_completed = Vec::new();
        let mut stripped = Vec::new();
        for task_id in &execute.completed_tasks {
            if allowed_completed.contains(task_id.trim()) {
                repaired_completed.push(task_id.clone());
            } else {
                stripped.push(task_id.clone());
            }
        }

        if stripped.is_empty() {
            return None;
        }

        let mut repaired_open = execute.open_tasks.clone();
        for id in &stripped {
            if !repaired_open.iter().any(|oid| oid.trim() == id.trim()) {
                repaired_open.push(id.clone());
            }
        }

        let mut obj = value.as_object()?.clone();
        obj.insert(
            "completed_tasks".to_string(),
            serde_json::json!(repaired_completed),
        );
        obj.insert(
            "open_tasks".to_string(),
            serde_json::json!(repaired_open),
        );

        info!(
            step_id = %step.id,
            current_item = %current_item.item_id,
            stripped_count = stripped.len(),
            "auto-repaired itemized execute output: stripped future items from completed_tasks"
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

        if latest_user_turn_prefers_research_scene(latest_user_turn) {
            if let Some(promoted_scene_id) = self
                .scene_catalog
                .scene(crate::RESEARCH_SCENE_ID)
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

fn allows_root_routing_text_fallback(role: WorkflowRunRole, step: &WorkflowStep) -> bool {
    role == WorkflowRunRole::Root
        && matches!(
            step.id.as_str(),
            SCENE_RECOGNITION_STEP_ID | SELECT_WORKFLOW_STEP_ID
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

fn max_output_validation_retries(output_contract: &StepOutputContract) -> u32 {
    match output_contract {
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

fn completed_output_attempts(output_contract: &StepOutputContract, retry_count: u32) -> u32 {
    match output_contract {
        StepOutputContract::None => 0,
        StepOutputContract::Required { .. } | StepOutputContract::Optional { .. } => {
            retry_count + 1
        }
    }
}

fn next_retry_attempt_kind(
    output_contract: &StepOutputContract,
    retry_count: u32,
) -> StepOutputAttemptKind {
    match output_contract {
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

fn build_step_diagnostics(
    context: &StepDiagnosticContext<'_>,
    execute_progress: Option<ExecuteProgressDiagnostics>,
    input: StepInputDiagnostics,
    output: StepOutputDiagnostics,
    session_writes: Vec<StepContextWrite>,
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
        execute_progress,
        input,
        output,
        session_writes,
    }
}

fn execute_output_marks_item_complete(
    structured_output: Option<&Value>,
    item_id: &str,
) -> anyhow::Result<bool> {
    let Some(structured_output) = structured_output else {
        return Ok(false);
    };
    let execute = parse_feature_execute_output(structured_output.clone())?;
    Ok(execute.completed_tasks.iter().any(|task_id| task_id == item_id))
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

fn truncate_chars(text: &str, limit: usize) -> String {
    text.chars().take(limit).collect()
}

fn summarize_step_text(text: &str) -> String {
    truncate_chars(text.trim(), SUMMARY_CHAR_LIMIT)
}
