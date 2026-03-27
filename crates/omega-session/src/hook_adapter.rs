use std::sync::{Arc, Mutex};

use anyhow::Result;
use omega_core::{CoreSharedTodoManager, CoreToolResult};
use omega_hooks::{
    HookAdvanceOutcome, HookDiagnosticLevel, HookDispatchInput, HookDispatchSummary,
    HookEventKind, HookHost, HookSession, HookSessionContextSnapshot, HookStepKey,
    HookStepSummarySnapshot, HookTodoSnapshot, HookToolCallSnapshot, HookToolResultSnapshot,
    HookWorkflowRole,
};
use omega_workflow::WorkflowStep;
use serde_json::Value;

use crate::{SharedRuntimeMessageBridge, WorkflowRunRole};
use crate::session_state::SessionContext;
use crate::ui_emit::{send_error_text, send_system_log_text, send_warning_text};
use crate::ResolvedToolSet;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExecuteLoopItemContext {
    pub(crate) child_step_id: String,
    pub(crate) item_id: String,
    pub(crate) item_label: Option<String>,
    pub(crate) item_index: usize,
    pub(crate) item_total: usize,
}

#[derive(Clone)]
pub(crate) struct StepHookRuntime {
    host: Arc<HookHost>,
    session: Arc<Mutex<HookSession>>,
    todo_manager: CoreSharedTodoManager,
    tx_result: SharedRuntimeMessageBridge,
    turn_id: u64,
    workflow_id: String,
    workflow_role: WorkflowRunRole,
    step: WorkflowStep,
    step_index: usize,
    step_total: usize,
    visible_tools: Vec<String>,
    structured_input: Option<Value>,
    session_context: SessionContext,
    current_item: Option<ExecuteLoopItemContext>,
}

impl StepHookRuntime {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        host: Arc<HookHost>,
        session: Arc<Mutex<HookSession>>,
        todo_manager: CoreSharedTodoManager,
        tx_result: SharedRuntimeMessageBridge,
        turn_id: u64,
        workflow_id: &str,
        workflow_role: WorkflowRunRole,
        step: &WorkflowStep,
        step_index: usize,
        step_total: usize,
        resolved_tools: &ResolvedToolSet,
        structured_input: Option<&Value>,
        session_context: &SessionContext,
        current_item: Option<ExecuteLoopItemContext>,
    ) -> Self {
        Self {
            host,
            session,
            todo_manager,
            tx_result,
            turn_id,
            workflow_id: workflow_id.to_string(),
            workflow_role,
            step: step.clone(),
            step_index,
            step_total,
            visible_tools: resolved_tools.tool_names().to_vec(),
            structured_input: structured_input.cloned(),
            session_context: session_context.clone(),
            current_item,
        }
    }

    pub(crate) fn before_step(&self) -> Result<()> {
        if self.step.hooks.is_empty() {
            return Ok(());
        }

        let step_key = self.step_key();
        let mut session = self.session.lock().unwrap();
        if !session.activate_step(step_key.clone()) {
            return Ok(());
        }

        let result = self.dispatch_locked(
            &mut session,
            HookEventKind::BeforeStep,
            None,
            None,
            None,
            None,
        );
        if result.is_err() {
            session.deactivate_step(&step_key);
        }
        result?;
        Ok(())
    }

    pub(crate) fn after_tool_call(
        &self,
        tool_use_id: &str,
        tool_name: &str,
        tool_input: &Value,
        tool_result: &CoreToolResult,
    ) -> Result<()> {
        if self.step.hooks.is_empty() || !self.session.lock().unwrap().is_step_active(&self.step_key()) {
            return Ok(());
        }

        let tool_call = HookToolCallSnapshot {
            tool_use_id: Some(tool_use_id.to_string()),
            tool_name: tool_name.to_string(),
            input: tool_input.clone(),
            result: Some(HookToolResultSnapshot {
                output_preview: tool_result
                    .preview
                    .clone()
                    .unwrap_or_else(|| crate::preview_text(&tool_result.output, 160)),
                error_kind: tool_result.error_kind.map(|kind| kind.as_str().to_string()),
                truncated: tool_result.truncated,
            }),
        };

        let mut session = self.session.lock().unwrap();
        self.dispatch_locked(
            &mut session,
            HookEventKind::AfterToolCall,
            None,
            None,
            None,
            Some(tool_call),
        )?;
        Ok(())
    }

    pub(crate) fn after_step(&self, final_text: &str, structured_output: Option<&Value>) -> Result<()> {
        if self.step.hooks.is_empty() || !self.session.lock().unwrap().is_step_active(&self.step_key()) {
            return Ok(());
        }

        let mut session = self.session.lock().unwrap();
        self.dispatch_locked(
            &mut session,
            HookEventKind::AfterStep,
            Some(final_text),
            None,
            structured_output,
            None,
        )?;
        Ok(())
    }

    pub(crate) fn before_advance(
        &self,
        final_text: &str,
        structured_output: Option<&Value>,
    ) -> Result<HookAdvanceOutcome> {
        if self.step.hooks.is_empty() || !self.session.lock().unwrap().is_step_active(&self.step_key()) {
            return Ok(HookAdvanceOutcome::Allow);
        }

        let mut session = self.session.lock().unwrap();
        let summary = self.dispatch_locked(
            &mut session,
            HookEventKind::BeforeAdvance,
            Some(final_text),
            None,
            structured_output,
            None,
        )?;
        Ok(summary.advance)
    }

    pub(crate) fn step_failed(&self, error: &str) -> Result<()> {
        if self.step.hooks.is_empty() || !self.session.lock().unwrap().is_step_active(&self.step_key()) {
            return Ok(());
        }

        let mut session = self.session.lock().unwrap();
        self.dispatch_locked(
            &mut session,
            HookEventKind::StepFailed,
            None,
            Some(error),
            None,
            None,
        )
        .map_err(|dispatch_error| anyhow::anyhow!("{dispatch_error}"))?;

            send_error_text(
            &*self.tx_result,
            self.turn_id,
            &format!("Hook-managed step failed: {error}"),
        );
        Ok(())
    }

    fn dispatch_locked(
        &self,
        session: &mut HookSession,
        event: HookEventKind,
        final_text: Option<&str>,
        error_text: Option<&str>,
        structured_output: Option<&Value>,
        tool_call: Option<HookToolCallSnapshot>,
    ) -> Result<HookDispatchSummary> {
        let summary = self.host.dispatch(
            session,
            &self.step_key(),
            &self.step.hooks,
            HookDispatchInput {
                event,
                workflow_id: self.workflow_id.clone(),
                workflow_role: to_hook_role(self.workflow_role),
                step_id: self.step.id.clone(),
                step_label: self.step.label.clone(),
                step_index: self.step_index,
                step_total: self.step_total,
                current_item_id: self.current_item.as_ref().map(|item| item.item_id.clone()),
                item_index: self.current_item.as_ref().map(|item| item.item_index),
                item_total: self.current_item.as_ref().map(|item| item.item_total),
                visible_tools: self.visible_tools.clone(),
                structured_input: self.structured_input.clone(),
                structured_output: structured_output.cloned(),
                final_text: final_text.map(ToOwned::to_owned),
                error: matches!(event, HookEventKind::StepFailed)
                    .then(|| error_text.unwrap_or("step failed").to_string()),
                tool_call,
                todo: todo_snapshot(&self.todo_manager)?,
                session_context: session_context_snapshot(&self.session_context),
                storage: Default::default(),
            },
        )?;

        for diagnostic in &summary.diagnostics {
            let text = format!(
                "Hook {} [{}] {}",
                diagnostic.hook_id,
                diagnostic.level.as_str(),
                diagnostic.message
            );
            match diagnostic.level {
                HookDiagnosticLevel::Info => {
                    send_system_log_text(&*self.tx_result, self.turn_id, &text);
                }
                HookDiagnosticLevel::Warning => {
                    send_warning_text(&*self.tx_result, self.turn_id, &text);
                }
                HookDiagnosticLevel::Error => {
                    send_error_text(&*self.tx_result, self.turn_id, &text);
                }
            }
        }

        Ok(summary)
    }

    fn step_key(&self) -> HookStepKey {
        HookStepKey {
            workflow_id: self.workflow_id.clone(),
            workflow_role: to_hook_role(self.workflow_role),
            step_id: self.step.id.clone(),
        }
    }
}

fn todo_snapshot(todo_manager: &CoreSharedTodoManager) -> Result<HookTodoSnapshot> {
    let manager = todo_manager
        .lock()
        .map_err(|_| anyhow::anyhow!("todo manager lock poisoned"))?;
    Ok(HookTodoSnapshot {
        rendered: (!manager.items().is_empty()).then(|| manager.render()),
        has_open_items: manager.has_open_items(),
        rounds_without_update: manager.rounds_without_update(),
    })
}

fn session_context_snapshot(session_context: &SessionContext) -> HookSessionContextSnapshot {
    HookSessionContextSnapshot {
        latest_user_turn: session_context.latest_user_turn.clone(),
        recognized_scene_id: session_context.routing.recognized_scene_id.clone(),
        selected_workflow_id: session_context.routing.selected_workflow_id.clone(),
        active_workflow_id: session_context.routing.active_workflow_id.clone(),
        active_workflow_role: to_hook_role(session_context.routing.active_workflow_role),
        step_summaries: session_context
            .step_summaries
            .iter()
            .map(|summary| HookStepSummarySnapshot {
                workflow_id: summary.workflow_id.clone(),
                step_id: summary.step_id.clone(),
                title: summary.title.clone(),
                summary: summary.summary.clone(),
                estimated_tokens: summary.estimated_tokens,
            })
            .collect(),
        step_outputs: session_context.step_outputs.clone(),
    }
}

fn to_hook_role(role: WorkflowRunRole) -> HookWorkflowRole {
    match role {
        WorkflowRunRole::Root => HookWorkflowRole::Root,
        WorkflowRunRole::Child => HookWorkflowRole::Child,
    }
}