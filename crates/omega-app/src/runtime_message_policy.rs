use omega_session::{
    ConversationMessage, RuntimeContentKind, RuntimeMessage, RuntimeSource, StateMessage,
    StepSubflowState, StepSubflowStatus,
};
use omega_tui::{RuntimeMessagePolicy, TuiSurface};

pub struct DefaultRuntimeMessagePolicy;

impl RuntimeMessagePolicy for DefaultRuntimeMessagePolicy {
    fn apply(&self, surface: &mut dyn TuiSurface, message: RuntimeMessage) {
        match message {
            RuntimeMessage::Conversation(message) => apply_conversation(surface, message),
            RuntimeMessage::State(message) => apply_state(surface, message),
        }
    }
}

fn apply_conversation(surface: &mut dyn TuiSurface, message: ConversationMessage) {
    match message {
        ConversationMessage::BeginSection { section } => surface.begin_section(section),
        ConversationMessage::AppendSection { id, delta } => surface.append_section(&id, delta),
        ConversationMessage::CompleteSection { id, state } => {
            surface.complete_section(&id, state)
        }
        ConversationMessage::BeginToolRun { tool_run } => surface.begin_tool_run(tool_run),
        ConversationMessage::UpdateToolRun { tool_run } => surface.update_tool_run(tool_run),
        ConversationMessage::CompleteToolRun { id, status } => {
            surface.complete_tool_run(&id, status)
        }
        ConversationMessage::Text {
            source,
            kind,
            text,
            ..
        } => match (&source, kind) {
            (RuntimeSource::WorkflowStep { .. }, RuntimeContentKind::Narrative)
            | (RuntimeSource::WorkflowStep { .. }, RuntimeContentKind::Result)
            | (RuntimeSource::Assistant, RuntimeContentKind::Narrative)
            | (RuntimeSource::Assistant, RuntimeContentKind::Result) => {}
            (_, RuntimeContentKind::Error) => surface.push_error_message(&text),
            _ => surface.push_agent_message(&text),
        },
    }
}

fn apply_state(surface: &mut dyn TuiSurface, message: StateMessage) {
    match message {
        StateMessage::WorkflowStep(step) => surface.set_workflow_step(step),
        StateMessage::ClearWorkflowStep => surface.clear_workflow_step(),
        StateMessage::AgentStatus { label: Some(label) } => surface.set_agent_status(&label),
        StateMessage::AgentStatus { label: None } => surface.clear_agent_status(),
        StateMessage::SessionRouting(routing) => surface.set_session_routing(routing),
        StateMessage::TodoSnapshot { rendered } => surface.set_todo_snapshot(&rendered),
        StateMessage::ShowOverlay { request } => surface.show_overlay(request),
        StateMessage::Diagnostics { diagnostics } => surface.upsert_diagnostics(*diagnostics),
        StateMessage::StepSubflow { subflow } => {
            surface.add_activity_line(format_step_subflow_line(&subflow));
            surface.upsert_step_subflow(subflow);
        }
        StateMessage::Activity {
            source,
            kind,
            text,
            ..
        } => surface.add_activity_line(format_activity_line(&source, kind, text)),
        StateMessage::TurnFinished => surface.mark_turn_finished(),
    }
}

fn format_step_subflow_line(subflow: &StepSubflowStatus) -> String {
    let mut line = format!(
        "[item] {}:{} {} {}/{} {}",
        subflow.workflow_role.as_str(),
        subflow.workflow_id,
        subflow.subflow_id,
        subflow.item_index,
        subflow.item_total,
        match subflow.status {
            StepSubflowState::Queued => "queued",
            StepSubflowState::Running => "running",
            StepSubflowState::Complete => "done",
            StepSubflowState::Failed => "failed",
        }
    );

    if let Some(item_id) = subflow.item_id.as_deref() {
        line.push_str(&format!(" #{item_id}"));
    }
    if subflow.repeat_count_for_item > 0 {
        line.push_str(&format!(" r{}", subflow.repeat_count_for_item));
    }

    line
}

fn format_activity_line(source: &RuntimeSource, kind: RuntimeContentKind, text: String) -> String {
    match (source, kind) {
        (RuntimeSource::Tool { .. }, RuntimeContentKind::Log) => format!("[tool] {text}"),
        (
            RuntimeSource::WorkflowStep {
                workflow_id,
                workflow_role,
                step_id,
                step_label,
                index,
                total,
            },
            RuntimeContentKind::Summary,
        ) => format!(
            "[{}:{} {}/{}] {} ({})",
            workflow_role.as_str(),
            workflow_id,
            index,
            total,
            step_label,
            step_id
        ),
        (RuntimeSource::SessionRouting, RuntimeContentKind::Summary)
        | (RuntimeSource::SessionRouting, RuntimeContentKind::Warning) => {
            format!("[route] {text}")
        }
        _ => text,
    }
}

#[cfg(test)]
mod tests {
    use omega_session::{
        ContextBudgetDiagnostics, ContextDiagnostics, ContextDocumentDiagnostics,
        ContextMemoryDiagnostics, ContextStoreDiagnostics, ConversationMessage,
        HealthScore, OverlayRequest, OverlayTarget, ResponseSection, ResponseSectionDelta,
        ResponseSectionKind, ResponseSectionMetadata, ResponseSectionState,
        RuntimeMessageEnvelope, RuntimeSource, SessionRoutingStatus, StateMessage,
        StepContextWrite, StepContextWriteKind, StepDiagnostics, StepInputDiagnostics,
        StepInputStatus, StepOutputAttemptKind, StepOutputContractMode,
        StepOutputDiagnostics, StepOutputRecoveryDecision, StepOutputStatus,
        StepSubflowState, StepSubflowStatus, StepSummarySource, ToolRun, ToolRunDetail,
        ToolRunStatus, UiContent, WorkflowRunRole, WorkflowStepStatus,
    };
    use omega_tui::{apply_runtime_message_with_policy, TuiSurface};

    use super::DefaultRuntimeMessagePolicy;

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum SurfaceOp {
        BeginSection(String),
        AppendSection(String, String),
        CompleteSection(String, ResponseSectionState),
        BeginToolRun(String),
        UpdateToolRun(String),
        CompleteToolRun(String, ToolRunStatus),
        WorkflowStep(String, String),
        ClearWorkflowStep,
        AgentStatus(String),
        SessionRouting(String),
        TodoSnapshot(String),
        Diagnostics(String),
        StepSubflow(String, String),
        Activity(String),
        ShowOverlay(OverlayTarget, String),
        AgentText(String),
        ErrorText(String),
        TurnFinished,
    }

    #[derive(Default)]
    struct RecordingSurface {
        ops: Vec<SurfaceOp>,
    }

    impl TuiSurface for RecordingSurface {
        fn begin_section(&mut self, section: ResponseSection) {
            self.ops.push(SurfaceOp::BeginSection(section.id));
        }

        fn append_section(&mut self, id: &str, delta: ResponseSectionDelta) {
            let ResponseSectionDelta::Text(text) = delta;
            self.ops.push(SurfaceOp::AppendSection(id.to_string(), text));
        }

        fn complete_section(&mut self, id: &str, state: ResponseSectionState) {
            self.ops
                .push(SurfaceOp::CompleteSection(id.to_string(), state));
        }

        fn begin_tool_run(&mut self, tool_run: ToolRun) {
            self.ops.push(SurfaceOp::BeginToolRun(tool_run.id));
        }

        fn update_tool_run(&mut self, tool_run: ToolRun) {
            self.ops.push(SurfaceOp::UpdateToolRun(tool_run.id));
        }

        fn complete_tool_run(&mut self, id: &str, status: ToolRunStatus) {
            self.ops
                .push(SurfaceOp::CompleteToolRun(id.to_string(), status));
        }

        fn set_workflow_step(&mut self, step: WorkflowStepStatus) {
            self.ops
                .push(SurfaceOp::WorkflowStep(step.workflow_id, step.step_id));
        }

        fn clear_workflow_step(&mut self) {
            self.ops.push(SurfaceOp::ClearWorkflowStep);
        }

        fn set_agent_status(&mut self, label: &str) {
            self.ops.push(SurfaceOp::AgentStatus(label.to_string()));
        }

        fn clear_agent_status(&mut self) {}

        fn set_session_routing(&mut self, routing: SessionRoutingStatus) {
            self.ops
                .push(SurfaceOp::SessionRouting(routing.active_workflow_id));
        }

        fn clear_session_routing(&mut self) {}

        fn set_todo_snapshot(&mut self, text: &str) {
            self.ops.push(SurfaceOp::TodoSnapshot(text.to_string()));
        }

        fn upsert_diagnostics(&mut self, diagnostics: StepDiagnostics) {
            self.ops.push(SurfaceOp::Diagnostics(diagnostics.id));
        }

        fn upsert_step_subflow(&mut self, subflow: StepSubflowStatus) {
            self.ops
                .push(SurfaceOp::StepSubflow(subflow.step_id, subflow.subflow_id));
        }

        fn add_activity_line(&mut self, line: String) {
            self.ops.push(SurfaceOp::Activity(line));
        }

        fn show_overlay(&mut self, request: OverlayRequest) {
            let preview = match request.content {
                UiContent::Text(text) => text,
            };
            self.ops.push(SurfaceOp::ShowOverlay(request.target, preview));
        }

        fn push_agent_message(&mut self, text: &str) {
            self.ops.push(SurfaceOp::AgentText(text.to_string()));
        }

        fn push_error_message(&mut self, text: &str) {
            self.ops.push(SurfaceOp::ErrorText(text.to_string()));
        }

        fn mark_turn_finished(&mut self) {
            self.ops.push(SurfaceOp::TurnFinished);
        }
    }

    fn sample_diagnostics() -> StepDiagnostics {
        StepDiagnostics {
            id: "child:feature:plan".to_string(),
            workflow_id: "feature".to_string(),
            workflow_role: WorkflowRunRole::Child,
            step_id: "plan".to_string(),
            step_label: "Plan".to_string(),
            index: 2,
            total: 4,
            context: Some(ContextDiagnostics {
                budget: ContextBudgetDiagnostics {
                    budget_input_tokens: 1024,
                    request_input_tokens: 321,
                    headroom_tokens: 703,
                    usage_percent: 31,
                    selected_summary_count: 1,
                    available_summary_count: 2,
                },
                cache: None,
                memory: ContextMemoryDiagnostics {
                    total_turns_archived: 2,
                    compactions_triggered: 1,
                    last_compaction_at: Some(1),
                    current_summary_tokens: 144,
                    current_summary_count: 1,
                    compression_ratio_avg_percent: 50,
                },
                document: ContextDocumentDiagnostics {
                    total_files_indexed: 12,
                    total_chunks: 48,
                    total_embeddings: 48,
                    index_staleness_seconds: 4,
                    governance_health: Some(HealthScore::NeedsAttention),
                    last_health_check: Some(2),
                },
                store: ContextStoreDiagnostics {
                    lance_db_size_bytes: 4096,
                    tantivy_index_size_bytes: 2048,
                    todo_items_count: 3,
                    turn_archive_count: 2,
                },
            }),
            cache: None,
            execute_progress: None,
            input: StepInputDiagnostics {
                status: StepInputStatus::Ready,
                summary_sources: vec![StepSummarySource {
                    workflow_id: "feature".to_string(),
                    step_id: "explore".to_string(),
                    title: "Explore".to_string(),
                }],
                expected_structured_sources: vec!["explore".to_string()],
                resolved_structured_sources: vec!["explore".to_string()],
                missing_structured_sources: vec![],
                structured_input_preview: Some("{\"explore\":{}}".to_string()),
                todo_state_preview: None,
                error: None,
            },
            output: StepOutputDiagnostics {
                contract_mode: StepOutputContractMode::Required,
                format: Some("json".to_string()),
                schema_path: Some(".omega/schema/step/plan.json".to_string()),
                status: StepOutputStatus::Valid,
                attempt_kind: StepOutputAttemptKind::Repair,
                extracted_json_preview: Some("{\"tasks\":[]}".to_string()),
                previous_response_preview: Some("{}".to_string()),
                attempts: 2,
                retry_count: 1,
                max_retries: 2,
                validation_error: Some("missing tasks".to_string()),
                recovery_decision: Some(StepOutputRecoveryDecision::Regenerate),
            },
            session_writes: vec![StepContextWrite {
                path: "step_outputs.plan".to_string(),
                kind: StepContextWriteKind::Added,
                before_preview: None,
                after_preview: Some("{\"tasks\":[]}".to_string()),
            }],
        }
    }

    #[test]
    fn runtime_message_pipeline_matrix_covers_current_flow() {
        let policy = DefaultRuntimeMessagePolicy;
        let mut surface = RecordingSurface::default();

        let stale_applied = apply_runtime_message_with_policy(
            42,
            RuntimeMessageEnvelope::state(
                41,
                StateMessage::Activity {
                    source: RuntimeSource::System,
                    kind: omega_session::RuntimeContentKind::Log,
                    text: "stale".to_string(),
                    priority: None,
                },
            ),
            &policy,
            &mut surface,
        );

        let section = ResponseSection {
            id: "turn-42:child:feature:plan".to_string(),
            parent_id: None,
            kind: ResponseSectionKind::Step,
            title: "Plan".to_string(),
            state: ResponseSectionState::Streaming,
            metadata: ResponseSectionMetadata {
                scene_id: Some("feature".to_string()),
                workflow_id: "feature".to_string(),
                workflow_role: WorkflowRunRole::Child,
                step_id: Some("plan".to_string()),
                step_label: Some("Plan".to_string()),
                subflow_ref: None,
            },
        };

        let tool_run = ToolRun {
            id: "tool-1".to_string(),
            parent_section_id: section.id.clone(),
            tool_name: "bash".to_string(),
            status: ToolRunStatus::Running,
            invocation_preview: "$ echo hi".to_string(),
            result_preview: None,
            detail: ToolRunDetail {
                title: " Tool: bash ".to_string(),
                lines: vec!["tool: bash".to_string()],
            },
        };

        let events = vec![
            RuntimeMessageEnvelope::state(
                42,
                StateMessage::WorkflowStep(WorkflowStepStatus {
                    workflow_id: "feature".to_string(),
                    workflow_role: WorkflowRunRole::Child,
                    step_id: "plan".to_string(),
                    step_label: "Plan".to_string(),
                    index: 2,
                    total: 4,
                }),
            ),
            RuntimeMessageEnvelope::state(
                42,
                StateMessage::Activity {
                    source: RuntimeSource::SessionRouting,
                    kind: omega_session::RuntimeContentKind::Summary,
                    text: "Delegating to child workflow 'feature'.".to_string(),
                    priority: None,
                },
            ),
            RuntimeMessageEnvelope::conversation(
                42,
                ConversationMessage::BeginSection {
                    section: section.clone(),
                },
            ),
            RuntimeMessageEnvelope::conversation(
                42,
                ConversationMessage::AppendSection {
                    id: section.id.clone(),
                    delta: ResponseSectionDelta::Text("draft patch".to_string()),
                },
            ),
            RuntimeMessageEnvelope::conversation(
                42,
                ConversationMessage::BeginToolRun {
                    tool_run: tool_run.clone(),
                },
            ),
            RuntimeMessageEnvelope::conversation(
                42,
                ConversationMessage::UpdateToolRun { tool_run },
            ),
            RuntimeMessageEnvelope::conversation(
                42,
                ConversationMessage::CompleteToolRun {
                    id: "tool-1".to_string(),
                    status: ToolRunStatus::Complete,
                },
            ),
            RuntimeMessageEnvelope::state(
                42,
                StateMessage::TodoSnapshot {
                    rendered: "[>] #1: Code".to_string(),
                },
            ),
            RuntimeMessageEnvelope::state(
                42,
                StateMessage::Diagnostics {
                    diagnostics: Box::new(sample_diagnostics()),
                },
            ),
            RuntimeMessageEnvelope::state(
                42,
                StateMessage::StepSubflow {
                    subflow: StepSubflowStatus {
                        workflow_id: "feature".to_string(),
                        workflow_role: WorkflowRunRole::Child,
                        step_id: "execute".to_string(),
                        step_label: "Execute".to_string(),
                        subflow_id: "execute-2".to_string(),
                        item_id: Some("risk-2".to_string()),
                        item_label: Some("Validate risk".to_string()),
                        item_index: 2,
                        item_total: 5,
                        status: StepSubflowState::Running,
                        repeat_count_for_item: 1,
                        no_progress_streak_for_item: 0,
                        completion_source: None,
                    },
                },
            ),
            RuntimeMessageEnvelope::conversation(
                42,
                ConversationMessage::CompleteSection {
                    id: section.id,
                    state: ResponseSectionState::Complete,
                },
            ),
            RuntimeMessageEnvelope::state(
                42,
                StateMessage::TurnFinished,
            ),
        ];

        for event in events {
            let _ = apply_runtime_message_with_policy(42, event, &policy, &mut surface);
        }

        assert!(!stale_applied);
        assert_eq!(
            surface.ops,
            vec![
                SurfaceOp::WorkflowStep("feature".to_string(), "plan".to_string()),
                SurfaceOp::Activity(
                    "[route] Delegating to child workflow 'feature'.".to_string(),
                ),
                SurfaceOp::BeginSection("turn-42:child:feature:plan".to_string()),
                SurfaceOp::AppendSection(
                    "turn-42:child:feature:plan".to_string(),
                    "draft patch".to_string(),
                ),
                SurfaceOp::BeginToolRun("tool-1".to_string()),
                SurfaceOp::UpdateToolRun("tool-1".to_string()),
                SurfaceOp::CompleteToolRun("tool-1".to_string(), ToolRunStatus::Complete),
                SurfaceOp::TodoSnapshot("[>] #1: Code".to_string()),
                SurfaceOp::Diagnostics("child:feature:plan".to_string()),
                SurfaceOp::Activity("[item] child:feature execute-2 2/5 running #risk-2 r1".to_string()),
                SurfaceOp::StepSubflow("execute".to_string(), "execute-2".to_string()),
                SurfaceOp::CompleteSection(
                    "turn-42:child:feature:plan".to_string(),
                    ResponseSectionState::Complete,
                ),
                SurfaceOp::TurnFinished,
            ]
        );
    }

    #[test]
    fn policy_keeps_legacy_fallback_text_behavior() {
        let policy = DefaultRuntimeMessagePolicy;
        let mut surface = RecordingSurface::default();

        let inputs = vec![
            RuntimeMessageEnvelope::conversation(
                7,
                ConversationMessage::Text {
                    source: RuntimeSource::Assistant,
                    kind: omega_session::RuntimeContentKind::Result,
                    text: "assistant final".to_string(),
                    priority: None,
                },
            ),
            RuntimeMessageEnvelope::conversation(
                7,
                ConversationMessage::Text {
                    source: RuntimeSource::System,
                    kind: omega_session::RuntimeContentKind::Error,
                    text: "boom".to_string(),
                    priority: None,
                },
            ),
            RuntimeMessageEnvelope::state(
                7,
                StateMessage::Activity {
                    source: RuntimeSource::Tool {
                        tool_name: "bash".to_string(),
                    },
                    kind: omega_session::RuntimeContentKind::Log,
                    text: "$ echo hi".to_string(),
                    priority: None,
                },
            ),
        ];

        for event in inputs {
            let _ = apply_runtime_message_with_policy(7, event, &policy, &mut surface);
        }

        assert_eq!(
            surface.ops,
            vec![
                SurfaceOp::ErrorText("boom".to_string()),
                SurfaceOp::Activity("[tool] $ echo hi".to_string()),
            ]
        );
    }

    #[test]
    fn policy_routes_show_overlay_state_messages_to_surface() {
        let policy = DefaultRuntimeMessagePolicy;
        let mut surface = RecordingSurface::default();

        let _ = apply_runtime_message_with_policy(
            7,
            RuntimeMessageEnvelope::state(
                7,
                StateMessage::ShowOverlay {
                    request: OverlayRequest {
                        target: OverlayTarget::Search,
                        content: UiContent::Text("results".to_string()),
                    },
                },
            ),
            &policy,
            &mut surface,
        );

        assert_eq!(
            surface.ops,
            vec![SurfaceOp::ShowOverlay(
                OverlayTarget::Search,
                "results".to_string(),
            )]
        );
    }
}