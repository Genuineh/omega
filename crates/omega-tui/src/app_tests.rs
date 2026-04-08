use omega_session::{
    ActivityTarget, CacheDiagnostics, ContextBudgetDiagnostics, ContextDiagnostics,
    ContextDocumentDiagnostics, ContextMemoryDiagnostics, ContextStoreDiagnostics,
    ExecuteProgressDiagnostics, HealthScore, OverlayRequest, ResponseSection, ResponseSectionDelta,
    ResponseSectionKind, ResponseSectionMetadata, RuntimeUiEffect, RuntimeUiMessage,
    SectionOrigin,
    StepContextWrite, StepContextWriteKind, StepDiagnostics, StepInputDiagnostics, StepInputStatus,
    StepOutputAttemptKind, StepOutputContractMode, StepOutputDiagnostics,
    StepOutputRecoveryDecision, StepOutputStatus, StepSubflowRef, StepSubflowState,
    StepSubflowStatus, StepSummarySource, TokenCountSource, ToolCapabilityDiagnostics,
    ToolRunDetail, UiContent,
    UiMessageKind, UiSource, UiTarget, WorkflowRunRole,
};
use ratatui::layout::Rect;

use super::*;

fn workflow_metadata(
    scene_id: Option<&str>,
    workflow_id: &str,
    workflow_role: WorkflowRunRole,
    step_id: Option<&str>,
    step_label: Option<&str>,
    subflow_ref: Option<StepSubflowRef>,
) -> ResponseSectionMetadata {
    ResponseSectionMetadata {
        scene_id: scene_id.map(ToOwned::to_owned),
        origin: SectionOrigin::Workflow {
            workflow_id: workflow_id.to_string(),
            workflow_role,
        },
        step_id: step_id.map(ToOwned::to_owned),
        step_label: step_label.map(ToOwned::to_owned),
        subflow_ref,
    }
}

fn sample_step_diagnostics() -> StepDiagnostics {
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
                retention_candidates_accepted: 3,
                retention_candidates_dropped: 1,
                dropped_candidates_by_profile: std::collections::BTreeMap::from([(
                    "ephemeral_debug".to_string(),
                    1,
                )]),
                memory_query_count: 4,
                memory_query_hit_mix: std::collections::BTreeMap::from([
                    ("project_facts".to_string(), 3),
                    ("open_threads".to_string(), 1),
                ]),
                observation_count: 2,
                observation_fresh_count: 1,
                observation_stale_count: 0,
                observation_superseded_count: 1,
                observation_corrected_count: 0,
                observation_correction_activity: 1,
                current_query: Some(omega_session::MemoryQueryDiagnostics {
                    raw_query: "memory query".to_string(),
                    planned_queries: vec!["memory query".to_string()],
                    rewrite_reason: None,
                    rewrite_queries: Vec::new(),
                    recovery_path: Some("deterministic_bundle".to_string()),
                    query: "memory query".to_string(),
                    result_count: 2,
                    hit_mix: std::collections::BTreeMap::from([
                        ("project_facts".to_string(), 1),
                        ("open_threads".to_string(), 1),
                    ]),
                    top_hits: vec![omega_session::MemoryQueryHitItem {
                        profile: "project_facts".to_string(),
                        title: "Project fact: planner wired".to_string(),
                        preview: "Planner now wires archived memory query.".to_string(),
                    }],
                }),
                current_observations: Some(omega_session::ObservationRecallDiagnostics {
                    raw_query: "memory query".to_string(),
                    planned_queries: vec!["memory query".to_string()],
                    rewrite_reason: None,
                    rewrite_queries: Vec::new(),
                    recovery_path: Some("deterministic_bundle".to_string()),
                    query: "memory query".to_string(),
                    result_count: 1,
                    freshness_mix: std::collections::BTreeMap::from([(
                        "fresh".to_string(),
                        1,
                    )]),
                    top_hits: vec![omega_session::ObservationRecallHitItem {
                        id: "obs-1".to_string(),
                        title: "Open thread: task-memory-query".to_string(),
                        summary: "Query surface still needs planner wiring.".to_string(),
                        freshness: omega_session::ObservationFreshness::Fresh,
                    }],
                }),
            },
            document: ContextDocumentDiagnostics {
                total_files_indexed: 12,
                total_chunks: 48,
                total_embeddings: 48,
                index_staleness_seconds: 4,
                governance_health: Some(HealthScore::NeedsAttention),
                health_status: omega_session::DocumentHealthStatus::NeedsAttention,
                last_health_check: Some(2),
                active_version: None,
                pending_version: None,
                last_promotion_error: None,
                recent_activity: Vec::new(),
                operator_usage: Vec::new(),
            },
            store: ContextStoreDiagnostics {
                lance_db_size_bytes: 4096,
                tantivy_index_size_bytes: 2048,
                todo_items_count: 3,
                turn_archive_count: 2,
                turn_archive_size_bytes: 8192,
            },
        }),
        cache: Some(CacheDiagnostics {
            token_count_source: TokenCountSource::ProviderCountTokens,
            request_input_tokens: 321,
            budget_input_tokens: 1024,
            cache_breakpoints: vec!["tools".to_string(), "system:stable".to_string()],
            cache_creation_input_tokens: Some(40),
            cache_read_input_tokens: Some(60),
            uncached_input_tokens: Some(120),
            cache_hit_ratio_percent: Some(33),
        }),
        execute_progress: Some(ExecuteProgressDiagnostics {
            todo_total: 2,
            todo_completed: 1,
            todo_open: 1,
            current_item_id: Some("task-2".to_string()),
            current_item_index: Some(2),
            current_item_total: Some(2),
            repeat_count: 1,
            no_progress_streak: 0,
            max_step_repeats: 8,
            max_item_repeats: Some(3),
            completion_source: Some("structured_output".to_string()),
        }),
        input: StepInputDiagnostics {
            status: StepInputStatus::Ready,
            summary_sources: vec![StepSummarySource {
                workflow_id: "feature".to_string(),
                step_id: "explore".to_string(),
                title: "Explore".to_string(),
                preview: "Explored the workspace state and narrowed the active files.".to_string(),
            }],
            expected_structured_sources: vec!["explore".to_string()],
            resolved_structured_sources: vec!["explore".to_string()],
            missing_structured_sources: vec![],
            structured_input_preview: Some("{\"explore\":{\"objective\":\"Ship\"}}".to_string()),
            todo_state_preview: None,
            error: None,
        },
        output: StepOutputDiagnostics {
            contract_mode: StepOutputContractMode::Required,
            format: Some("json".to_string()),
            schema_path: Some(".omega/schema/step/plan.json".to_string()),
            status: StepOutputStatus::Valid,
            attempt_kind: StepOutputAttemptKind::Repair,
            extracted_json_preview: Some("{\"tasks\":[{\"id\":\"task-1\"}]}".to_string()),
            previous_response_preview: Some("{\"tasks\":[]}".to_string()),
            attempts: 2,
            retry_count: 1,
            max_retries: 2,
            validation_error: Some("missing validation_targets".to_string()),
            recovery_decision: Some(StepOutputRecoveryDecision::Regenerate),
        },
        session_writes: vec![StepContextWrite {
            path: "step_outputs.plan".to_string(),
            kind: StepContextWriteKind::Added,
            before_preview: None,
            after_preview: Some("{\"tasks\":[{\"id\":\"task-1\"}]}".to_string()),
        }],
        tool_capabilities: Some(ToolCapabilityDiagnostics {
            tool_invocations: std::collections::BTreeMap::from([
                ("grep_search".to_string(), 1),
                ("ask_user_question".to_string(), 1),
            ]),
            family_invocations: std::collections::BTreeMap::from([
                ("workspace_inspection".to_string(), 1),
                ("interaction".to_string(), 1),
            ]),
            tool_failure_count_by_kind: std::collections::BTreeMap::from([(
                "policy".to_string(),
                1,
            )]),
            bash_fallback_count: 0,
            question_block_count: 1,
            tool_switch_after_failure: 1,
            same_intent_retry_count: 0,
        }),
    }
}

#[test]
fn input_editing_uses_character_indices() {
    let mut app = App::new();
    app.insert_char('你');
    app.insert_char('好');
    app.move_cursor_left();
    app.insert_char('们');

    assert_eq!(app.input_buffer, "你们好");
    assert_eq!(app.cursor_pos, 2);
}

#[test]
fn add_log_strips_ansi_sequences() {
    let mut app = App::new();
    app.add_log("\u{1b}[32mhello\u{1b}[0m".to_string());

    assert_eq!(app.log_lines, vec!["hello"]);
}

#[test]
fn upserting_step_diagnostics_builds_sidebar_lines() {
    let mut app = App::new();

    app.upsert_step_diagnostics(sample_step_diagnostics());

    assert_eq!(app.step_diagnostics.len(), 1);
    assert!(!app.diagnostics_lines.is_empty());
    assert!(app
        .diagnostics_lines
        .iter()
        .any(|line| line.text.contains("child:feature 2/4 Plan")));
    assert!(app
        .diagnostics_lines
        .iter()
        .any(|line| line.text.contains("attempt=repair · next=regenerate")));
    assert!(app
        .diagnostics_lines
        .iter()
        .any(|line| line.text.contains("budget=31%")));
    assert!(app
        .diagnostics_lines
        .iter()
        .any(|line| line.text.contains("headroom=703")));
    assert!(app.diagnostics_lines.iter().any(|line| line
        .text
        .contains("context memory turns=2 compact=1 summaries=1")));
    assert!(app.diagnostics_lines.iter().any(|line| line
        .text
        .contains("context docs files=12 chunks=48 health=needs_attention")));
    assert!(app
        .diagnostics_lines
        .iter()
        .any(|line| line.text.contains("current=task-2")));
    assert_eq!(app.rail_badge(SidebarSection::Diagnostics), "D 1");
}

#[test]
fn upserting_skill_load_summary_builds_skills_sidebar_lines() {
    let mut app = App::new();

    app.upsert_skill_load_summary(
        "turn-42:root:root:load-skills".to_string(),
        omega_session::SkillLoadSummary {
            source_step_id: Some("select-skills".to_string()),
            recognized_skill_ids: vec!["docs-specs".to_string(), "plan".to_string()],
            loaded_skill_ids: vec!["docs-specs".to_string()],
            ignored_skill_ids: vec!["plan".to_string()],
            selection_reason: Some("spec task".to_string()),
        },
    );

    assert_eq!(app.rail_badge(SidebarSection::Skills), "S 1/2");
    assert!(app
        .skill_lines
        .iter()
        .any(|line| line.contains("recognized ids: docs-specs, plan")));
    assert!(app
        .skill_lines
        .iter()
        .any(|line| line.contains("loaded ids: docs-specs")));
    assert!(app
        .skill_lines
        .iter()
        .any(|line| line.contains("ignored ids: plan")));
}

#[test]
fn finishing_turn_builds_delivery_sidebar_lines_and_badge() {
    let mut app = App::new();
    let turn_id = app.begin_turn();
    app.remember_delivery_model_name("gpt-5.4");
    app.upsert_step_diagnostics(sample_step_diagnostics());
    app.upsert_skill_load_summary(
        "turn-1:root:root:load-skills".to_string(),
        omega_session::SkillLoadSummary {
            source_step_id: Some("select-skills".to_string()),
            recognized_skill_ids: vec!["docs-specs".to_string(), "plan".to_string()],
            loaded_skill_ids: vec!["docs-specs".to_string()],
            ignored_skill_ids: vec!["plan".to_string()],
            selection_reason: Some("spec task".to_string()),
        },
    );
    app.upsert_step_knowledge_summary(
        "turn-1:child:feature:plan".to_string(),
        omega_session::StepKnowledgeSummary {
            document: Some(omega_session::ResponseDocumentKnowledge {
                raw_query: "delivery summary".to_string(),
                planned_queries: vec!["delivery summary".to_string()],
                rewrite_reason: None,
                rewrite_queries: Vec::new(),
                recovery_path: Some("deterministic_bundle".to_string()),
                readiness: omega_session::SupervisionReadiness::Ready,
                query: "delivery summary".to_string(),
                mode: "keyword".to_string(),
                degraded_from: None,
                reason: None,
                result_count: 1,
                top_hits: Vec::new(),
            }),
            memory: Some(omega_session::ResponseMemoryKnowledge {
                raw_query: Some("delivery memory".to_string()),
                planned_queries: vec!["delivery memory".to_string()],
                rewrite_reason: None,
                rewrite_queries: Vec::new(),
                recovery_path: Some("deterministic_bundle".to_string()),
                memory_query: Some("delivery memory".to_string()),
                observation_query: Some("delivery observation".to_string()),
                selected_summary_count: 0,
                top_selected_summaries: Vec::new(),
                memory_hit_count: 0,
                observation_hit_count: 0,
                top_memory_hits: Vec::new(),
                top_observations: Vec::new(),
            }),
        },
    );
    app.begin_tool_run(omega_session::ToolRun {
        id: "tool-1".to_string(),
        parent_section_id: "turn-1:child:feature:plan".to_string(),
        tool_name: "write_file".to_string(),
        status: omega_session::ToolRunStatus::Running,
        invocation_preview: "crates/omega-tui/src/app.rs".to_string(),
        result_preview: Some("updated file".to_string()),
        detail: ToolRunDetail {
            title: " Tool: write_file ".to_string(),
            lines: vec!["path: crates/omega-tui/src/app.rs".to_string()],
        },
    });
    app.complete_tool_run("tool-1", omega_session::ToolRunStatus::Complete);

    app.set_status_slot(StatusSlot::Agent, StatusValue::Label("Idle".to_string()));

    assert_eq!(turn_id, 1);
    assert_eq!(app.rail_badge(SidebarSection::Delivery), "V 2/1");
    assert!(app
        .delivery_lines
        .iter()
        .any(|line| line.contains("model: gpt-5.4")));
    assert!(app
        .delivery_lines
        .iter()
        .any(|line| line.contains("files: 1 changed")));
    assert!(app
        .response_lines()
        .iter()
        .any(|line| line.contains("Task Delivery Summary")));
}

#[test]
fn activating_delivery_summary_opens_detail_overlay() {
    let mut app = App::new();
    app.begin_turn();
    app.remember_delivery_model_name("gpt-5.4");
    app.upsert_step_diagnostics(sample_step_diagnostics());
    app.begin_tool_run(omega_session::ToolRun {
        id: "tool-1".to_string(),
        parent_section_id: "turn-1:child:feature:plan".to_string(),
        tool_name: "write_file".to_string(),
        status: omega_session::ToolRunStatus::Running,
        invocation_preview: "crates/omega-tui/src/app.rs".to_string(),
        result_preview: Some("updated file".to_string()),
        detail: ToolRunDetail {
            title: " Tool: write_file ".to_string(),
            lines: vec!["path: crates/omega-tui/src/app.rs".to_string()],
        },
    });
    app.complete_tool_run("tool-1", omega_session::ToolRunStatus::Complete);
    app.set_status_slot(StatusSlot::Agent, StatusValue::Label("Idle".to_string()));

    let delivery_line = app
        .response_display_lines()
        .iter()
        .position(|line| line.text.contains("delivery  complete"))
        .expect("delivery response lane should exist");

    let activation = app.activate_response_item_at_line(delivery_line);

    assert_eq!(activation, Some(ResponseActivation::DeliveryDetailOpened));
    match app.overlay.as_ref() {
        Some(OverlayState::Detail(detail)) => {
            assert_eq!(detail.title, " Task Delivery ");
            assert!(detail
                .lines
                .iter()
                .any(|line| line.contains("model: gpt-5.4")));
            assert!(detail
                .lines
                .iter()
                .any(|line| line.contains("crates/omega-tui/src/app.rs [update]")));
        }
        other => panic!("expected delivery detail overlay, got {other:?}"),
    }
}

#[test]
fn activating_diagnostics_item_opens_detail_overlay() {
    let mut app = App::new();
    app.upsert_step_diagnostics(sample_step_diagnostics());
    app.focused_panel = Panel::Diagnostics;
    app.diagnostics_rect = Rect::new(0, 0, 80, 8);
    app.diagnostics_state.select(Some(0));

    let opened = app.activate_selected_diagnostics_item();

    assert_eq!(opened, Some("Plan".to_string()));
    match app.overlay.as_ref() {
        Some(OverlayState::Detail(detail)) => {
            assert!(detail.title.contains("Plan"));
            assert!(detail
                .lines
                .iter()
                .any(|line| line.contains("context_budget_percent: 31")));
            assert!(detail
                .lines
                .iter()
                .any(|line| line.contains("context_headroom_tokens: 703")));
            assert!(detail
                .lines
                .iter()
                .any(|line| line
                    .contains("context_memory: turns_archived=2 compactions_triggered=1")));
            assert!(detail
                .lines
                .iter()
                .any(|line| line.contains("context_document: files=12 chunks=48 embeddings=48 staleness_seconds=4 health=needs_attention governance_health=needs_attention")));
            assert!(detail
                .lines
                .iter()
                .any(|line| line.contains("context_store: todo_items=3 turn_archive_count=2 tantivy_index_size_bytes=2048 lance_db_size_bytes=4096")));
            assert!(detail
                .lines
                .iter()
                .any(|line| line.contains("execute_progress: todos=1/2 open=1")));
            assert!(detail
                .lines
                .iter()
                .any(|line| line.contains("current_item: task-2 (2/2)")));
            assert!(detail
                .lines
                .iter()
                .any(|line| line.contains("completion_source: structured_output")));
            assert!(detail
                .lines
                .iter()
                .any(|line| line.contains("step_outputs.plan")));
            assert!(detail
                .lines
                .iter()
                .any(|line| line.contains("recovery_decision: regenerate")));
            assert!(detail
                .lines
                .iter()
                .any(|line| line.contains("validation_error: missing validation_targets")));
            assert!(detail
                .lines
                .iter()
                .any(|line| line.contains("previous_response_preview:")));
            assert!(detail
                .lines
                .iter()
                .any(|line| line.contains("extracted_json_preview:")));
            assert!(detail.lines.iter().any(|line| line.contains("(added)")));
            assert!(detail
                .lines
                .iter()
                .any(|line| line.contains("after  {\"tasks\"")));
        }
        other => panic!("expected detail overlay, got {other:?}"),
    }
}

#[test]
fn todo_snapshot_replaces_lines() {
    let mut app = App::new();
    app.set_todo_snapshot(1, "[ ] #1: Plan\n[>] #2: Code\n\n(0/2 completed)");

    assert_eq!(app.todo_lines, vec!["[ ] #1: Plan", "[>] #2: Code"]);
    assert_eq!(
        app.todo_summary,
        Some(TodoSummary {
            completed: 0,
            total: 2,
        })
    );
}

#[test]
fn empty_todo_snapshot_uses_actionable_copy() {
    let mut app = App::new();
    app.set_todo_snapshot(2, "No todos.");

    assert_eq!(app.todo_status, TodoPanelStatus::Empty);
    assert_eq!(app.todo_lines, todo_empty_lines());
    assert_eq!(
        app.todo_summary,
        Some(TodoSummary {
            completed: 0,
            total: 0,
        })
    );
}

#[test]
fn running_turn_marks_todo_as_stale_until_snapshot_arrives() {
    let mut app = App::new();
    let first_turn = app.begin_turn();
    app.set_todo_snapshot(first_turn, "[>] #1: Code\n\n(0/1 completed)");
    app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
        first_turn,
        RuntimeUiEffect::SetStatusSlot {
            slot: StatusSlot::Agent,
            value: StatusValue::Label("Idle".to_string()),
        },
    ));

    app.begin_turn();

    assert!(app.todo_refresh_pending());
    assert!(app.todo_panel_title().contains("stale"));
}

#[test]
fn current_turn_workflow_updates_replace_summary_and_clear_on_finish() {
    let mut app = App::new();
    let turn_id = app.begin_turn();

    app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::SetStatusSlot {
            slot: StatusSlot::Workflow,
            value: StatusValue::WorkflowStep {
                workflow_id: "feature".to_string(),
                workflow_role: WorkflowRunRole::Child,
                step_id: "plan".to_string(),
                step_label: "Plan".to_string(),
                index: 2,
                total: 4,
            },
        },
    ));
    app.apply_runtime_envelope(RuntimeUiEnvelope::message(
        turn_id,
        RuntimeUiMessage {
            target: UiTarget::Activity(ActivityTarget::Log),
            source: UiSource::WorkflowStep {
                workflow_id: "feature".to_string(),
                workflow_role: WorkflowRunRole::Child,
                step_id: "plan".to_string(),
                step_label: "Plan".to_string(),
                index: 2,
                total: 4,
            },
            kind: UiMessageKind::Summary,
            content: UiContent::Text("Plan".to_string()),
            priority: None,
        },
    ));

    assert_eq!(
        app.workflow_summary,
        Some(WorkflowSummary {
            workflow_id: "feature".to_string(),
            workflow_role: WorkflowRunRole::Child,
            id: "plan".to_string(),
            label: "Plan".to_string(),
            index: 2,
            total: 4,
        })
    );
    assert_eq!(app.log_lines, vec!["[child:feature 2/4] Plan (plan)"]);

    app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::ClearStatusSlot {
            slot: StatusSlot::Workflow,
        },
    ));

    assert!(app.workflow_summary.is_none());
}

#[test]
fn tool_preview_routes_to_logs_instead_of_response() {
    let mut app = App::new();
    let turn_id = app.begin_turn();

    app.apply_runtime_envelope(RuntimeUiEnvelope::message(
        turn_id,
        RuntimeUiMessage {
            target: UiTarget::Activity(ActivityTarget::Log),
            source: UiSource::Tool {
                tool_name: "bash".to_string(),
            },
            kind: UiMessageKind::Log,
            content: UiContent::Text("$ echo hi".to_string()),
            priority: None,
        },
    ));
    app.apply_runtime_envelope(RuntimeUiEnvelope::message(
        turn_id,
        RuntimeUiMessage {
            target: UiTarget::Activity(ActivityTarget::Log),
            source: UiSource::Tool {
                tool_name: "bash".to_string(),
            },
            kind: UiMessageKind::Log,
            content: UiContent::Text("hi".to_string()),
            priority: None,
        },
    ));

    assert!(app.output_msgs.is_empty());
    assert_eq!(app.log_lines, vec!["[tool] $ echo hi", "[tool] hi"]);
}

#[test]
fn step_text_routes_to_response_with_step_label() {
    let mut app = App::new();
    let turn_id = app.begin_turn();

    app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::BeginResponseSection {
            section: ResponseSection {
                id: "turn-1:child:feature:plan".to_string(),
                parent_id: None,
                kind: ResponseSectionKind::Step,
                title: "Plan".to_string(),
                state: ResponseSectionState::Streaming,
                metadata: workflow_metadata(
                    Some("feature"),
                    "feature",
                    WorkflowRunRole::Child,
                    Some("plan"),
                    Some("Plan"),
                    None,
                ),
            },
        },
    ));
    app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::AppendResponseSection {
            id: "turn-1:child:feature:plan".to_string(),
            delta: ResponseSectionDelta::Text("Line one\nLine two".to_string()),
        },
    ));
    app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::CompleteResponseSection {
            id: "turn-1:child:feature:plan".to_string(),
            state: ResponseSectionState::Complete,
        },
    ));

    assert_eq!(
        app.response_lines(),
        vec![
            "step  child:feature  Plan  [done]".to_string(),
            "  scene feature".to_string(),
            "  Line one".to_string(),
            "  Line two".to_string(),
        ]
    );
    assert!(app.log_lines.is_empty());
}

#[test]
fn command_sections_render_in_response_panel() {
    let mut app = App::new();
    let turn_id = app.begin_turn();

    app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::BeginResponseSection {
            section: ResponseSection {
                id: "turn-1:command".to_string(),
                parent_id: None,
                kind: ResponseSectionKind::Command,
                title: "/document health".to_string(),
                state: ResponseSectionState::Streaming,
                metadata: ResponseSectionMetadata {
                    scene_id: None,
                    origin: SectionOrigin::Command {
                        command_name: "/document health".to_string(),
                        source: "builtin".to_string(),
                    },
                    step_id: None,
                    step_label: None,
                    subflow_ref: None,
                },
            },
        },
    ));
    app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::AppendResponseSection {
            id: "turn-1:command".to_string(),
            delta: ResponseSectionDelta::Text(
                "Overall health: good\nTotal docs: 12".to_string(),
            ),
        },
    ));
    app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::CompleteResponseSection {
            id: "turn-1:command".to_string(),
            state: ResponseSectionState::Complete,
        },
    ));

    assert_eq!(
        app.response_lines(),
        vec![
            "command  builtin  /document health  [done]  [collapse]".to_string(),
            "  » Overall health: good".to_string(),
            "  » Total docs: 12".to_string(),
        ]
    );
    assert!(app.log_lines.is_empty());
}

#[test]
fn command_sections_can_be_collapsed_from_response_panel() {
    let mut app = App::new();
    let turn_id = app.begin_turn();

    app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::BeginResponseSection {
            section: ResponseSection {
                id: "turn-1:command".to_string(),
                parent_id: None,
                kind: ResponseSectionKind::Command,
                title: "/document init".to_string(),
                state: ResponseSectionState::Streaming,
                metadata: ResponseSectionMetadata {
                    scene_id: None,
                    origin: SectionOrigin::Command {
                        command_name: "/document init".to_string(),
                        source: "builtin".to_string(),
                    },
                    step_id: None,
                    step_label: None,
                    subflow_ref: None,
                },
            },
        },
    ));
    app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::AppendResponseSection {
            id: "turn-1:command".to_string(),
            delta: ResponseSectionDelta::Text(
                "Running /document init...\nIndexed 12 files\nVector ignored: 4".to_string(),
            ),
        },
    ));

    let header_index = app
        .response_display_lines()
        .iter()
        .position(|line| line.text == "command  builtin  /document init  [streaming]  [collapse]")
        .unwrap();
    app.response_state.select(Some(header_index));
    assert_eq!(
        app.activate_selected_response_item(),
        Some(ResponseActivation::CommandCollapsed)
    );

    assert_eq!(
        app.response_lines(),
        vec![
            "command  builtin  /document init  [streaming]  [expand]".to_string(),
            "  » ▸ 3 lines · Running /document init...".to_string(),
        ]
    );
}

#[test]
fn routing_and_final_answer_sections_form_response_timeline() {
    let mut app = App::new();
    let turn_id = app.begin_turn();

    app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::BeginResponseSection {
            section: ResponseSection {
                id: "turn-7:root:root:scene-recognition".to_string(),
                parent_id: None,
                kind: ResponseSectionKind::Routing,
                title: "Scene Recognition".to_string(),
                state: ResponseSectionState::Streaming,
                metadata: workflow_metadata(
                    None,
                    "root",
                    WorkflowRunRole::Root,
                    Some("scene-recognition"),
                    Some("Scene Recognition"),
                    None,
                ),
            },
        },
    ));
    app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::AppendResponseSection {
            id: "turn-7:root:root:scene-recognition".to_string(),
            delta: ResponseSectionDelta::Text("chat".to_string()),
        },
    ));
    app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::CompleteResponseSection {
            id: "turn-7:root:root:scene-recognition".to_string(),
            state: ResponseSectionState::Complete,
        },
    ));
    app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::BeginResponseSection {
            section: ResponseSection {
                id: "turn-7:child:chat:chat".to_string(),
                parent_id: None,
                kind: ResponseSectionKind::FinalAnswer,
                title: "Final Answer".to_string(),
                state: ResponseSectionState::Streaming,
                metadata: workflow_metadata(
                    Some("chat"),
                    "chat",
                    WorkflowRunRole::Child,
                    Some("chat"),
                    Some("Chat"),
                    None,
                ),
            },
        },
    ));
    app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::AppendResponseSection {
            id: "turn-7:child:chat:chat".to_string(),
            delta: ResponseSectionDelta::Text("hello".to_string()),
        },
    ));
    app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::CompleteResponseSection {
            id: "turn-7:child:chat:chat".to_string(),
            state: ResponseSectionState::Complete,
        },
    ));

    assert_eq!(
        app.response_lines(),
        vec![
            "route  root:root  Scene Recognition  [done]".to_string(),
            "  result chat".to_string(),
            "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━".to_string(),
            "final  child:chat  Final Answer  [done]".to_string(),
            "  scene chat".to_string(),
            "  │ hello".to_string(),
        ]
    );
}

#[test]
fn thinking_sections_stream_then_collapse_on_complete() {
    let mut app = App::new();
    let turn_id = app.begin_turn();

    app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::BeginResponseSection {
            section: ResponseSection {
                id: "turn-9:child:chat:chat".to_string(),
                parent_id: None,
                kind: ResponseSectionKind::FinalAnswer,
                title: "Final Answer".to_string(),
                state: ResponseSectionState::Streaming,
                metadata: workflow_metadata(
                    Some("chat"),
                    "chat",
                    WorkflowRunRole::Child,
                    Some("chat"),
                    Some("Chat"),
                    None,
                ),
            },
        },
    ));
    app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::BeginResponseSection {
            section: ResponseSection {
                id: "turn-9:child:chat:chat:thinking".to_string(),
                parent_id: Some("turn-9:child:chat:chat".to_string()),
                kind: ResponseSectionKind::Thinking,
                title: "Thinking".to_string(),
                state: ResponseSectionState::Streaming,
                metadata: workflow_metadata(
                    Some("chat"),
                    "chat",
                    WorkflowRunRole::Child,
                    Some("chat"),
                    Some("Chat"),
                    None,
                ),
            },
        },
    ));
    app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::AppendResponseSection {
            id: "turn-9:child:chat:chat:thinking".to_string(),
            delta: ResponseSectionDelta::Text("outline answer\ncheck tone".to_string()),
        },
    ));

    assert_eq!(
        app.response_lines(),
        vec![
            "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━".to_string(),
            "final  child:chat  Final Answer  [streaming]".to_string(),
            "  scene chat".to_string(),
            "  │ …".to_string(),
            "  reasoning  child:chat  Reasoning live  [streaming]".to_string(),
            "    ⠋ outline answer".to_string(),
            "    ⠋ check tone".to_string(),
        ]
    );

    app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::CompleteResponseSection {
            id: "turn-9:child:chat:chat:thinking".to_string(),
            state: ResponseSectionState::Complete,
        },
    ));

    assert_eq!(
        app.response_lines(),
        vec![
            "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━".to_string(),
            "final  child:chat  Final Answer  [streaming]".to_string(),
            "  scene chat".to_string(),
            "  │ …".to_string(),
            "  reasoning  child:chat  Reasoning  [done]".to_string(),
            "    ▸ reasoning · 2 lines · outline answer".to_string(),
        ]
    );

    let thinking_index = app
        .response_display_lines()
        .iter()
        .position(|line| line.text == "  reasoning  child:chat  Reasoning  [done]")
        .unwrap();
    app.response_state.select(Some(thinking_index));

    assert_eq!(app.toggle_selected_thinking_section(), Some(false));
    assert_eq!(
        app.response_lines(),
        vec![
            "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━".to_string(),
            "final  child:chat  Final Answer  [streaming]".to_string(),
            "  scene chat".to_string(),
            "  │ …".to_string(),
            "  reasoning  child:chat  Reasoning  [done]".to_string(),
            "    │ outline answer".to_string(),
            "    │ check tone".to_string(),
        ]
    );
}

#[test]
fn thinking_sections_limit_streaming_body_to_recent_lines() {
    let mut app = App::new();
    let turn_id = app.begin_turn();

    app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::BeginResponseSection {
            section: ResponseSection {
                id: "turn-91:child:chat:chat".to_string(),
                parent_id: None,
                kind: ResponseSectionKind::FinalAnswer,
                title: "Final Answer".to_string(),
                state: ResponseSectionState::Streaming,
                metadata: workflow_metadata(
                    Some("chat"),
                    "chat",
                    WorkflowRunRole::Child,
                    Some("chat"),
                    Some("Chat"),
                    None,
                ),
            },
        },
    ));
    app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::BeginResponseSection {
            section: ResponseSection {
                id: "turn-91:child:chat:chat:thinking".to_string(),
                parent_id: Some("turn-91:child:chat:chat".to_string()),
                kind: ResponseSectionKind::Thinking,
                title: "Thinking".to_string(),
                state: ResponseSectionState::Streaming,
                metadata: workflow_metadata(
                    Some("chat"),
                    "chat",
                    WorkflowRunRole::Child,
                    Some("chat"),
                    Some("Chat"),
                    None,
                ),
            },
        },
    ));
    app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::AppendResponseSection {
            id: "turn-91:child:chat:chat:thinking".to_string(),
            delta: ResponseSectionDelta::Text(
                "first thought\nsecond thought\nthird thought".to_string(),
            ),
        },
    ));

    assert_eq!(
        app.response_lines(),
        vec![
            "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━".to_string(),
            "final  child:chat  Final Answer  [streaming]".to_string(),
            "  scene chat".to_string(),
            "  │ …".to_string(),
            "  reasoning  child:chat  Reasoning live  [streaming]".to_string(),
            "    ⠋ second thought".to_string(),
            "    ⠋ third thought".to_string(),
        ]
    );
}

#[test]
fn failed_thinking_sections_surface_failure_summary() {
    let mut app = App::new();
    let turn_id = app.begin_turn();

    app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::BeginResponseSection {
            section: ResponseSection {
                id: "turn-10:child:chat:chat:thinking".to_string(),
                parent_id: Some("turn-10:child:chat:chat".to_string()),
                kind: ResponseSectionKind::Thinking,
                title: "Thinking".to_string(),
                state: ResponseSectionState::Streaming,
                metadata: workflow_metadata(
                    Some("chat"),
                    "chat",
                    WorkflowRunRole::Child,
                    Some("chat"),
                    Some("Chat"),
                    None,
                ),
            },
        },
    ));
    app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::AppendResponseSection {
            id: "turn-10:child:chat:chat:thinking".to_string(),
            delta: ResponseSectionDelta::Text("tool result mismatched".to_string()),
        },
    ));
    app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::CompleteResponseSection {
            id: "turn-10:child:chat:chat:thinking".to_string(),
            state: ResponseSectionState::Failed,
        },
    ));

    assert_eq!(
        app.response_lines(),
        vec![
            "  reasoning  child:chat  Reasoning failed  [failed]".to_string(),
            "    ▸ reasoning failed · 1 line · tool result mismatched".to_string(),
        ]
    );
}

#[test]
fn interrupt_turn_stops_streaming_reasoning_and_running_tool_styles() {
    let mut app = App::new();
    let turn_id = app.begin_turn();

    app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::BeginResponseSection {
            section: ResponseSection {
                id: "turn-50:child:feature:explore".to_string(),
                parent_id: None,
                kind: ResponseSectionKind::Step,
                title: "Explore".to_string(),
                state: ResponseSectionState::Streaming,
                metadata: workflow_metadata(
                    Some("feature"),
                    "feature",
                    WorkflowRunRole::Child,
                    Some("explore"),
                    Some("Explore"),
                    None,
                ),
            },
        },
    ));
    app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::BeginResponseSection {
            section: ResponseSection {
                id: "turn-50:child:feature:explore:thinking".to_string(),
                parent_id: Some("turn-50:child:feature:explore".to_string()),
                kind: ResponseSectionKind::Thinking,
                title: "Thinking".to_string(),
                state: ResponseSectionState::Streaming,
                metadata: workflow_metadata(
                    Some("feature"),
                    "feature",
                    WorkflowRunRole::Child,
                    Some("explore"),
                    Some("Explore"),
                    None,
                ),
            },
        },
    ));
    app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::AppendResponseSection {
            id: "turn-50:child:feature:explore:thinking".to_string(),
            delta: ResponseSectionDelta::Text("让我再确认一下是否有其他调用点。".to_string()),
        },
    ));
    app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::BeginToolRun {
            tool_run: omega_session::ToolRun {
                id: "tool-1".to_string(),
                parent_section_id: "turn-50:child:feature:explore".to_string(),
                tool_name: "read_file".to_string(),
                status: omega_session::ToolRunStatus::Running,
                invocation_preview: "read_file(Cargo.toml)".to_string(),
                result_preview: None,
                detail: ToolRunDetail {
                    title: " Tool: Read File ".to_string(),
                    lines: vec!["tool: read_file".to_string()],
                },
            },
        },
    ));

    app.interrupt_turn();

    assert!(app.response_lines().iter().any(|line| line.contains("Reasoning failed  [failed]")));
    assert!(app.response_lines().iter().any(|line| line.contains("reasoning failed")));
    assert!(!app.response_lines().iter().any(|line| line.contains("Reasoning live  [streaming]")));
    assert!(app
        .tool_runs
        .iter()
        .any(|tool_run| tool_run.id == "tool-1" && tool_run.status == ToolRunStatus::Failed));
    assert!(!app.is_running);
}

#[test]
fn thinking_sections_can_be_hidden_by_config() {
    let mut app = App::new();
    app.set_show_thinking(false);
    let turn_id = app.begin_turn();

    app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::BeginResponseSection {
            section: ResponseSection {
                id: "turn-12:child:chat:chat:thinking".to_string(),
                parent_id: Some("turn-12:child:chat:chat".to_string()),
                kind: ResponseSectionKind::Thinking,
                title: "Thinking".to_string(),
                state: ResponseSectionState::Streaming,
                metadata: workflow_metadata(
                    Some("chat"),
                    "chat",
                    WorkflowRunRole::Child,
                    Some("chat"),
                    Some("Chat"),
                    None,
                ),
            },
        },
    ));
    app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::AppendResponseSection {
            id: "turn-12:child:chat:chat:thinking".to_string(),
            delta: ResponseSectionDelta::Text("hidden reasoning".to_string()),
        },
    ));

    assert!(app.response_lines().is_empty());
}

#[test]
fn tool_run_effects_render_inside_step_block() {
    let mut app = App::new();
    let turn_id = app.begin_turn();

    app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::BeginResponseSection {
            section: ResponseSection {
                id: "turn-12:child:feature:execute".to_string(),
                parent_id: None,
                kind: ResponseSectionKind::Step,
                title: "Execute".to_string(),
                state: ResponseSectionState::Streaming,
                metadata: workflow_metadata(
                    Some("feature"),
                    "feature",
                    WorkflowRunRole::Child,
                    Some("execute"),
                    Some("Execute"),
                    None,
                ),
            },
        },
    ));
    app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::BeginToolRun {
            tool_run: ToolRun {
                id: "tool-1".to_string(),
                parent_section_id: "turn-12:child:feature:execute".to_string(),
                tool_name: "bash".to_string(),
                status: ToolRunStatus::Running,
                invocation_preview: "$ echo hi".to_string(),
                result_preview: None,
                detail: ToolRunDetail {
                    title: " Tool: bash ".to_string(),
                    lines: vec!["tool: bash".to_string(), "invoke: $ echo hi".to_string()],
                },
            },
        },
    ));
    app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::UpdateToolRun {
            tool_run: ToolRun {
                id: "tool-1".to_string(),
                parent_section_id: "turn-12:child:feature:execute".to_string(),
                tool_name: "bash".to_string(),
                status: ToolRunStatus::Complete,
                invocation_preview: "$ echo hi".to_string(),
                result_preview: Some("hi".to_string()),
                detail: ToolRunDetail {
                    title: " Tool: bash ".to_string(),
                    lines: vec![
                        "tool: bash".to_string(),
                        "invoke: $ echo hi".to_string(),
                        "result:".to_string(),
                        "hi".to_string(),
                    ],
                },
            },
        },
    ));
    app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::CompleteToolRun {
            id: "tool-1".to_string(),
            status: ToolRunStatus::Complete,
        },
    ));

    assert_eq!(
        app.response_lines(),
        vec![
            "step  child:feature  Execute  [streaming]".to_string(),
            "  scene feature".to_string(),
            "  tools  1 total".to_string(),
            "    bash  [done]  $ echo hi -> hi".to_string(),
        ]
    );
}

#[test]
fn activating_tool_summary_opens_detail_overlay() {
    let mut app = App::new();
    let turn_id = app.begin_turn();

    app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::BeginResponseSection {
            section: ResponseSection {
                id: "turn-13:child:feature:execute".to_string(),
                parent_id: None,
                kind: ResponseSectionKind::Step,
                title: "Execute".to_string(),
                state: ResponseSectionState::Streaming,
                metadata: workflow_metadata(
                    Some("feature"),
                    "feature",
                    WorkflowRunRole::Child,
                    Some("execute"),
                    Some("Execute"),
                    None,
                ),
            },
        },
    ));
    app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::BeginToolRun {
            tool_run: ToolRun {
                id: "tool-2".to_string(),
                parent_section_id: "turn-13:child:feature:execute".to_string(),
                tool_name: "read_file".to_string(),
                status: ToolRunStatus::Complete,
                invocation_preview: "src/main.rs".to_string(),
                result_preview: Some("12 lines".to_string()),
                detail: ToolRunDetail {
                    title: " Tool: read_file ".to_string(),
                    lines: vec![
                        "tool: read_file".to_string(),
                        "invoke: src/main.rs".to_string(),
                        "result:".to_string(),
                        "12 lines".to_string(),
                    ],
                },
            },
        },
    ));

    let selected_index = app
        .response_display_lines()
        .iter()
        .position(|line| line.text == "    read_file  [done]  src/main.rs -> 12 lines")
        .unwrap();
    app.response_state.select(Some(selected_index));

    assert_eq!(
        app.activate_selected_response_item(),
        Some(ResponseActivation::ToolDetailOpened(
            "read_file".to_string(),
        ))
    );

    match app.overlay.as_ref() {
        Some(OverlayState::Detail(detail)) => {
            assert_eq!(detail.title, " Tool: read_file ");
            assert_eq!(
                detail.lines,
                vec![
                    "tool: read_file".to_string(),
                    "invoke: src/main.rs".to_string(),
                    "result:".to_string(),
                    "12 lines".to_string(),
                ]
            );
        }
        other => panic!("expected detail overlay, got {other:?}"),
    }
}

#[test]
fn response_lines_include_knowledge_lane_for_section_summary() {
    let mut app = App::new();
    let turn_id = app.begin_turn();

    app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::BeginResponseSection {
            section: ResponseSection {
                id: "turn-13k:child:feature:execute".to_string(),
                parent_id: None,
                kind: ResponseSectionKind::Step,
                title: "Execute".to_string(),
                state: ResponseSectionState::Streaming,
                metadata: workflow_metadata(
                    Some("feature"),
                    "feature",
                    WorkflowRunRole::Child,
                    Some("execute"),
                    Some("Execute"),
                    None,
                ),
            },
        },
    ));
    app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::UpsertStepKnowledgeSummary {
            section_id: "turn-13k:child:feature:execute".to_string(),
            summary: Box::new(omega_session::StepKnowledgeSummary {
                document: Some(omega_session::ResponseDocumentKnowledge {
                    raw_query: "roadmap".to_string(),
                    planned_queries: vec!["roadmap".to_string()],
                    rewrite_reason: None,
                    rewrite_queries: Vec::new(),
                    recovery_path: Some("deterministic_bundle".to_string()),
                    readiness: omega_session::SupervisionReadiness::Ready,
                    query: "roadmap".to_string(),
                    mode: "hybrid".to_string(),
                    degraded_from: None,
                    reason: None,
                    result_count: 2,
                    top_hits: vec![omega_session::DocumentHitItem {
                        path: "docs/TODO.md".to_string(),
                        preview: "Current priorities and follow-up work".to_string(),
                    }],
                }),
                memory: Some(omega_session::ResponseMemoryKnowledge {
                    raw_query: Some("knowledge ui".to_string()),
                    planned_queries: vec!["knowledge ui".to_string()],
                    rewrite_reason: None,
                    rewrite_queries: Vec::new(),
                    recovery_path: Some("deterministic_bundle".to_string()),
                    memory_query: Some("knowledge ui".to_string()),
                    observation_query: None,
                    selected_summary_count: 2,
                    top_selected_summaries: vec![omega_session::MemoryHitItem {
                        workflow_id: "feature".to_string(),
                        step_id: "plan".to_string(),
                        title: "Knowledge UI plan".to_string(),
                        preview: "Response lane and overlay follow-up".to_string(),
                    }],
                    memory_hit_count: 2,
                    observation_hit_count: 0,
                    top_memory_hits: vec![omega_session::MemoryQueryHitItem {
                        profile: "project_facts".to_string(),
                        title: "Knowledge UI plan".to_string(),
                        preview: "Response lane and overlay follow-up".to_string(),
                    }],
                    top_observations: Vec::new(),
                }),
            }),
        },
    ));

    assert_eq!(
        app.response_lines(),
        vec![
            "step  child:feature  Execute  [streaming]".to_string(),
            "  scene feature".to_string(),
            "  knowledge".to_string(),
            "    document  [ready]  2 hits  ·  roadmap  ·  docs/TODO.md".to_string(),
            "    memory  2 selected  ·  2 archived  ·  0 observations  ·  knowledge ui"
                .to_string(),
        ]
    );
}

#[test]
fn response_lines_include_skill_lane_and_activation_opens_detail_overlay() {
    let mut app = App::new();
    let turn_id = app.begin_turn();

    app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::BeginResponseSection {
            section: ResponseSection {
                id: "turn-42:root:root:load-skills".to_string(),
                parent_id: None,
                kind: ResponseSectionKind::Step,
                title: "Load Skills".to_string(),
                state: ResponseSectionState::Streaming,
                metadata: workflow_metadata(
                    Some("feature"),
                    "root",
                    WorkflowRunRole::Root,
                    Some("load-skills"),
                    Some("Load Skills"),
                    None,
                ),
            },
        },
    ));
    app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::AppendResponseSection {
            id: "turn-42:root:root:load-skills".to_string(),
            delta: ResponseSectionDelta::Text(
                "recognized: docs-specs, plan\nloaded: docs-specs\nignored: plan".to_string(),
            ),
        },
    ));
    app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::UpsertSkillLoadSummary {
            section_id: "turn-42:root:root:load-skills".to_string(),
            summary: Box::new(omega_session::SkillLoadSummary {
                source_step_id: Some("select-skills".to_string()),
                recognized_skill_ids: vec!["docs-specs".to_string(), "plan".to_string()],
                loaded_skill_ids: vec!["docs-specs".to_string()],
                ignored_skill_ids: vec!["plan".to_string()],
                selection_reason: Some("spec task".to_string()),
            }),
        },
    ));
    app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::CompleteResponseSection {
            id: "turn-42:root:root:load-skills".to_string(),
            state: ResponseSectionState::Complete,
        },
    ));

    let response_lines = app.response_display_lines();
    let skill_line = response_lines
        .iter()
        .position(|line| line.text.contains("skills  recognized=2 loaded=1 ignored=1"))
        .expect("skill lane line should exist");

    let activation = app.activate_response_item_at_line(skill_line);

    assert_eq!(activation, Some(super::ResponseActivation::SkillLoadDetailOpened));
    match app.overlay.as_ref() {
        Some(OverlayState::Detail(detail)) => {
            assert!(detail.title.contains("Routed Skills"));
            assert!(detail
                .lines
                .iter()
                .any(|line| line.contains("recognized ids: docs-specs, plan")));
            assert!(detail
                .lines
                .iter()
                .any(|line| line.contains("ignored ids: plan")));
        }
        other => panic!("expected detail overlay, got {other:?}"),
    }
}

#[test]
fn activating_knowledge_summary_opens_detail_overlay() {
    let mut app = App::new();
    let turn_id = app.begin_turn();

    app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::BeginResponseSection {
            section: ResponseSection {
                id: "turn-13m:child:feature:execute".to_string(),
                parent_id: None,
                kind: ResponseSectionKind::Step,
                title: "Execute".to_string(),
                state: ResponseSectionState::Streaming,
                metadata: workflow_metadata(
                    Some("feature"),
                    "feature",
                    WorkflowRunRole::Child,
                    Some("execute"),
                    Some("Execute"),
                    None,
                ),
            },
        },
    ));
    app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::UpsertStepKnowledgeSummary {
            section_id: "turn-13m:child:feature:execute".to_string(),
            summary: Box::new(omega_session::StepKnowledgeSummary {
                document: Some(omega_session::ResponseDocumentKnowledge {
                    raw_query: "roadmap".to_string(),
                    planned_queries: vec!["roadmap".to_string(), "omega roadmap".to_string()],
                    rewrite_reason: None,
                    rewrite_queries: Vec::new(),
                    recovery_path: Some("deterministic_bundle".to_string()),
                    readiness: omega_session::SupervisionReadiness::Uninitialized,
                    query: "roadmap".to_string(),
                    mode: "hybrid".to_string(),
                    degraded_from: None,
                    reason: Some("no promoted store version".to_string()),
                    result_count: 0,
                    top_hits: Vec::new(),
                }),
                memory: Some(omega_session::ResponseMemoryKnowledge {
                    raw_query: Some("knowledge ui overlay".to_string()),
                    planned_queries: vec![
                        "knowledge ui".to_string(),
                        "response overlay".to_string(),
                    ],
                    rewrite_reason: None,
                    rewrite_queries: Vec::new(),
                    recovery_path: Some("deterministic_bundle".to_string()),
                    memory_query: Some("knowledge ui".to_string()),
                    observation_query: Some("knowledge ui observation".to_string()),
                    selected_summary_count: 1,
                    top_selected_summaries: vec![omega_session::MemoryHitItem {
                        workflow_id: "feature".to_string(),
                        step_id: "report".to_string(),
                        title: "Knowledge summary lane".to_string(),
                        preview: "Need response-facing drill-down".to_string(),
                    }],
                    memory_hit_count: 1,
                    observation_hit_count: 1,
                    top_memory_hits: vec![omega_session::MemoryQueryHitItem {
                        profile: "project_facts".to_string(),
                        title: "Knowledge summary lane".to_string(),
                        preview: "Need response-facing drill-down".to_string(),
                    }],
                    top_observations: vec![omega_session::ObservationRecallHitItem {
                        id: "obs-1".to_string(),
                        title: "Knowledge overlay feedback".to_string(),
                        summary: "Overlay should expose readable previews".to_string(),
                        freshness: omega_session::ObservationFreshness::Fresh,
                    }],
                }),
            }),
        },
    ));

    let document_line = app
        .response_display_lines()
        .iter()
        .position(|line| line.text.contains("document  [uninitialized]"))
        .unwrap();
    app.response_state.select(Some(document_line));
    assert_eq!(
        app.activate_selected_response_item(),
        Some(ResponseActivation::DocumentKnowledgeDetailOpened)
    );
    match app.overlay.as_ref() {
        Some(OverlayState::Detail(detail)) => {
            assert_eq!(detail.title, " Document Knowledge ");
            assert!(detail.lines.iter().any(|line| line == "reason: no promoted store version"));
        }
        other => panic!("expected document knowledge detail overlay, got {other:?}"),
    }

    let memory_line = app
        .response_display_lines()
        .iter()
        .position(|line| line.text.contains("memory  1 selected"))
        .unwrap();
    app.response_state.select(Some(memory_line));
    assert_eq!(
        app.activate_selected_response_item(),
        Some(ResponseActivation::MemoryKnowledgeDetailOpened)
    );
    match app.overlay.as_ref() {
        Some(OverlayState::Detail(detail)) => {
            assert_eq!(detail.title, " Memory Knowledge ");
            assert!(detail.lines.iter().any(|line| line == "planned queries: knowledge ui | response overlay"));
            assert!(detail.lines.iter().any(|line| line == "memory query: knowledge ui"));
            assert!(detail
                .lines
                .iter()
                .any(|line| line == "archived memory hits:"));
            assert!(detail.lines.iter().any(|line| line == "observations:"));
        }
        other => panic!("expected memory knowledge detail overlay, got {other:?}"),
    }
}

#[test]
fn tool_lane_defaults_to_collapsed_for_six_or_more_tools_and_can_toggle() {
    let mut app = App::new();
    let turn_id = app.begin_turn();

    app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::BeginResponseSection {
            section: ResponseSection {
                id: "turn-13b:child:feature:execute".to_string(),
                parent_id: None,
                kind: ResponseSectionKind::Step,
                title: "Execute".to_string(),
                state: ResponseSectionState::Streaming,
                metadata: workflow_metadata(
                    Some("feature"),
                    "feature",
                    WorkflowRunRole::Child,
                    Some("execute"),
                    Some("Execute"),
                    None,
                ),
            },
        },
    ));

    for index in 1..=6 {
        app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
            turn_id,
            RuntimeUiEffect::BeginToolRun {
                tool_run: omega_session::ToolRun {
                    id: format!("tool-collapse-{index}"),
                    parent_section_id: "turn-13b:child:feature:execute".to_string(),
                    tool_name: format!("tool_{index}"),
                    status: omega_session::ToolRunStatus::Complete,
                    invocation_preview: format!("arg-{index}"),
                    result_preview: Some(format!("ok-{index}")),
                    detail: ToolRunDetail {
                        title: format!(" Tool: tool_{index} "),
                        lines: vec![format!("tool: tool_{index}")],
                    },
                },
            },
        ));
    }

    assert_eq!(
        app.response_lines(),
        vec![
            "step  child:feature  Execute  [streaming]".to_string(),
            "  scene feature".to_string(),
            "  tools  6 total  [expand]".to_string(),
        ]
    );

    let header_index = app
        .response_display_lines()
        .iter()
        .position(|line| line.text == "  tools  6 total  [expand]")
        .unwrap();
    app.response_state.select(Some(header_index));
    assert_eq!(
        app.activate_selected_response_item(),
        Some(ResponseActivation::ToolLaneExpanded)
    );

    let expanded_lines = app.response_lines();
    assert_eq!(expanded_lines[2], "  tools  6 total  [collapse]");
    assert!(expanded_lines.iter().any(|line| line == "    tool_1  [done]  arg-1 -> ok-1"));
    assert!(expanded_lines.iter().any(|line| line == "    tool_6  [done]  arg-6 -> ok-6"));

    let header_index = app
        .response_display_lines()
        .iter()
        .position(|line| line.text == "  tools  6 total  [collapse]")
        .unwrap();
    app.response_state.select(Some(header_index));
    assert_eq!(
        app.activate_selected_response_item(),
        Some(ResponseActivation::ToolLaneCollapsed)
    );

    assert_eq!(
        app.response_lines(),
        vec![
            "step  child:feature  Execute  [streaming]".to_string(),
            "  scene feature".to_string(),
            "  tools  6 total  [expand]".to_string(),
        ]
    );
}

#[test]
fn step_subflow_sections_render_as_nested_timeline() {
    let mut app = App::new();
    let turn_id = app.begin_turn();

    app.set_todo_snapshot(
        turn_id,
        "[x] #risk-1: Inspect state\n[>] #risk-2: Validate risk\n[ ] #risk-3: Ship\n\n(1/3 completed)",
    );
    app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::UpsertStepSubflow {
            subflow: StepSubflowStatus {
                workflow_id: "feature".to_string(),
                workflow_role: WorkflowRunRole::Child,
                step_id: "execute".to_string(),
                step_label: "Execute".to_string(),
                subflow_id: "execute-1".to_string(),
                item_id: Some("risk-1".to_string()),
                item_label: Some("Inspect state".to_string()),
                item_index: 1,
                item_total: 3,
                status: StepSubflowState::Complete,
                repeat_count_for_item: 0,
                no_progress_streak_for_item: 0,
                completion_source: Some("structured_output".to_string()),
            },
        },
    ));
    app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::UpsertStepSubflow {
            subflow: StepSubflowStatus {
                workflow_id: "feature".to_string(),
                workflow_role: WorkflowRunRole::Child,
                step_id: "execute".to_string(),
                step_label: "Execute".to_string(),
                subflow_id: "execute-2".to_string(),
                item_id: Some("risk-2".to_string()),
                item_label: Some("Validate risk".to_string()),
                item_index: 2,
                item_total: 3,
                status: StepSubflowState::Running,
                repeat_count_for_item: 1,
                no_progress_streak_for_item: 0,
                completion_source: None,
            },
        },
    ));

    for (section_id, item_id, item_label, item_index, text) in [
        (
            "turn-21:child:feature:execute-1",
            "risk-1",
            "Inspect state",
            1usize,
            "inspected runtime state",
        ),
        (
            "turn-21:child:feature:execute-2",
            "risk-2",
            "Validate risk",
            2usize,
            "validating current risk",
        ),
    ] {
        app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
            turn_id,
            RuntimeUiEffect::BeginResponseSection {
                section: ResponseSection {
                    id: section_id.to_string(),
                    parent_id: None,
                    kind: ResponseSectionKind::Step,
                    title: "Execute".to_string(),
                    state: ResponseSectionState::Streaming,
                    metadata: workflow_metadata(
                        Some("feature"),
                        "feature",
                        WorkflowRunRole::Child,
                        Some("execute"),
                        Some("Execute"),
                        Some(StepSubflowRef {
                            parent_workflow_id: "feature".to_string(),
                            parent_step_id: "execute".to_string(),
                            parent_step_label: "Execute".to_string(),
                            subflow_id: format!("execute-{item_index}"),
                            item_id: Some(item_id.to_string()),
                            item_label: Some(item_label.to_string()),
                            item_index,
                            item_total: 3,
                        }),
                    ),
                },
            },
        ));
        app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
            turn_id,
            RuntimeUiEffect::AppendResponseSection {
                id: section_id.to_string(),
                delta: ResponseSectionDelta::Text(text.to_string()),
            },
        ));
    }

    assert_eq!(
        app.response_lines(),
        vec![
            "step  child:feature  Execute  [streaming]".to_string(),
            "  scene feature".to_string(),
            "  items 2/3 · current execute-2 · todo #risk-2 · repeat 1".to_string(),
            "  subflow  execute-1  #risk-1  Inspect state  [done]".to_string(),
            "  subflow  execute-2  #risk-2  Validate risk  [running]  repeat 1".to_string(),
            "    validating current risk".to_string(),
            "  subflow  execute-3  #risk-3  Ship  [queued]".to_string(),
        ]
    );
    assert_eq!(app.highlighted_todo_line_index(), Some(1));
}

#[test]
fn activating_subflow_header_opens_detail_overlay() {
    let mut app = App::new();
    let turn_id = app.begin_turn();

    app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::UpsertStepSubflow {
            subflow: StepSubflowStatus {
                workflow_id: "feature".to_string(),
                workflow_role: WorkflowRunRole::Child,
                step_id: "execute".to_string(),
                step_label: "Execute".to_string(),
                subflow_id: "execute-2".to_string(),
                item_id: Some("risk-2".to_string()),
                item_label: Some("Validate risk".to_string()),
                item_index: 2,
                item_total: 3,
                status: StepSubflowState::Running,
                repeat_count_for_item: 1,
                no_progress_streak_for_item: 0,
                completion_source: None,
            },
        },
    ));
    app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::BeginResponseSection {
            section: ResponseSection {
                id: "turn-22:child:feature:execute-2".to_string(),
                parent_id: None,
                kind: ResponseSectionKind::Step,
                title: "Execute".to_string(),
                state: ResponseSectionState::Streaming,
                metadata: workflow_metadata(
                    Some("feature"),
                    "feature",
                    WorkflowRunRole::Child,
                    Some("execute"),
                    Some("Execute"),
                    Some(StepSubflowRef {
                        parent_workflow_id: "feature".to_string(),
                        parent_step_id: "execute".to_string(),
                        parent_step_label: "Execute".to_string(),
                        subflow_id: "execute-2".to_string(),
                        item_id: Some("risk-2".to_string()),
                        item_label: Some("Validate risk".to_string()),
                        item_index: 2,
                        item_total: 3,
                    }),
                ),
            },
        },
    ));

    let selected_index = app
        .response_display_lines()
        .iter()
        .position(|line| {
            line.text == "  subflow  execute-2  #risk-2  Validate risk  [running]  repeat 1"
        })
        .unwrap();
    app.response_state.select(Some(selected_index));

    assert_eq!(
        app.activate_selected_response_item(),
        Some(ResponseActivation::StepSubflowDetailOpened(
            "Validate risk".to_string(),
        ))
    );

    match app.overlay.as_ref() {
        Some(OverlayState::Detail(detail)) => {
            assert_eq!(detail.title, " Subflow: execute-2 ");
            assert!(detail.lines.iter().any(|line| line == "todo: #risk-2"));
            assert!(detail.lines.iter().any(|line| line == "status: running"));
        }
        other => panic!("expected detail overlay, got {other:?}"),
    }
}

#[test]
fn todo_snapshot_backfills_prior_subflow_statuses_without_replayed_sections() {
    let mut app = App::new();
    let turn_id = app.begin_turn();

    app.set_todo_snapshot(
        turn_id,
        "[x] #risk-1: FFI audit\n[x] #risk-2: Bash boundary\n[x] #risk-3: API fallback\n[>] #risk-4: Coverage analysis\n[ ] #risk-5: Dependency audit\n\n(3/5 completed)",
    );
    app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::UpsertStepSubflow {
            subflow: StepSubflowStatus {
                workflow_id: "research".to_string(),
                workflow_role: WorkflowRunRole::Child,
                step_id: "execute".to_string(),
                step_label: "Execute".to_string(),
                subflow_id: "execute-4".to_string(),
                item_id: Some("risk-4".to_string()),
                item_label: Some("Coverage analysis".to_string()),
                item_index: 4,
                item_total: 5,
                status: StepSubflowState::Running,
                repeat_count_for_item: 3,
                no_progress_streak_for_item: 1,
                completion_source: None,
            },
        },
    ));
    app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::BeginResponseSection {
            section: ResponseSection {
                id: "turn-30:child:research:execute-4".to_string(),
                parent_id: None,
                kind: ResponseSectionKind::Step,
                title: "Execute".to_string(),
                state: ResponseSectionState::Streaming,
                metadata: workflow_metadata(
                    Some("research"),
                    "research",
                    WorkflowRunRole::Child,
                    Some("execute"),
                    Some("Execute"),
                    Some(StepSubflowRef {
                        parent_workflow_id: "research".to_string(),
                        parent_step_id: "execute".to_string(),
                        parent_step_label: "Execute".to_string(),
                        subflow_id: "execute-4".to_string(),
                        item_id: Some("risk-4".to_string()),
                        item_label: Some("Coverage analysis".to_string()),
                        item_index: 4,
                        item_total: 5,
                    }),
                ),
            },
        },
    ));
    app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::AppendResponseSection {
            id: "turn-30:child:research:execute-4".to_string(),
            delta: ResponseSectionDelta::Text("collecting coverage gaps".to_string()),
        },
    ));

    assert_eq!(
        app.response_lines(),
        vec![
            "step  child:research  Execute  [streaming]".to_string(),
            "  scene research".to_string(),
            "  items 4/5 · current execute-4 · todo #risk-4 · repeat 3".to_string(),
            "  subflow  execute-1  #risk-1  FFI audit  [done]".to_string(),
            "  subflow  execute-2  #risk-2  Bash boundary  [done]".to_string(),
            "  subflow  execute-3  #risk-3  API fallback  [done]".to_string(),
            "  subflow  execute-4  #risk-4  Coverage analysis  [running]  repeat 3".to_string(),
            "    collecting coverage gaps".to_string(),
            "  subflow  execute-5  #risk-5  Dependency audit  [queued]".to_string(),
        ]
    );
}

#[test]
fn later_subflows_do_not_render_done_while_earlier_item_is_still_running() {
    let mut app = App::new();
    let turn_id = app.begin_turn();

    app.set_todo_snapshot(
        turn_id,
        "[>] #plan-1: Verify duplicated diagnostics path\n[x] #plan-2: Trace tool callback path\n[ ] #plan-3: Compare archive paths\n\n(1/3 completed)",
    );
    app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::UpsertStepSubflow {
            subflow: StepSubflowStatus {
                workflow_id: "feature".to_string(),
                workflow_role: WorkflowRunRole::Child,
                step_id: "execute".to_string(),
                step_label: "Execute".to_string(),
                subflow_id: "execute-2".to_string(),
                item_id: Some("plan-2".to_string()),
                item_label: Some("Trace tool callback path".to_string()),
                item_index: 2,
                item_total: 3,
                status: StepSubflowState::Complete,
                repeat_count_for_item: 0,
                no_progress_streak_for_item: 0,
                completion_source: Some("structured_output".to_string()),
            },
        },
    ));
    app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::UpsertStepSubflow {
            subflow: StepSubflowStatus {
                workflow_id: "feature".to_string(),
                workflow_role: WorkflowRunRole::Child,
                step_id: "execute".to_string(),
                step_label: "Execute".to_string(),
                subflow_id: "execute-1".to_string(),
                item_id: Some("plan-1".to_string()),
                item_label: Some("Verify duplicated diagnostics path".to_string()),
                item_index: 1,
                item_total: 3,
                status: StepSubflowState::Running,
                repeat_count_for_item: 0,
                no_progress_streak_for_item: 0,
                completion_source: None,
            },
        },
    ));
    app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::BeginResponseSection {
            section: ResponseSection {
                id: "turn-31:child:feature:execute-1".to_string(),
                parent_id: None,
                kind: ResponseSectionKind::Step,
                title: "Execute".to_string(),
                state: ResponseSectionState::Streaming,
                metadata: workflow_metadata(
                    Some("feature"),
                    "feature",
                    WorkflowRunRole::Child,
                    Some("execute"),
                    Some("Execute"),
                    Some(StepSubflowRef {
                        parent_workflow_id: "feature".to_string(),
                        parent_step_id: "execute".to_string(),
                        parent_step_label: "Execute".to_string(),
                        subflow_id: "execute-1".to_string(),
                        item_id: Some("plan-1".to_string()),
                        item_label: Some("Verify duplicated diagnostics path".to_string()),
                        item_index: 1,
                        item_total: 3,
                    }),
                ),
            },
        },
    ));

    assert_eq!(
        app.response_lines(),
        vec![
            "step  child:feature  Execute  [streaming]".to_string(),
            "  scene feature".to_string(),
            "  items 1/3 · current execute-1 · todo #plan-1".to_string(),
            "  subflow  execute-1  #plan-1  Verify duplicated diagnostics path  [running]"
                .to_string(),
            "    …".to_string(),
            "  subflow  execute-2  #plan-2  Trace tool callback path  [queued]".to_string(),
            "  subflow  execute-3  #plan-3  Compare archive paths  [queued]".to_string(),
        ]
    );
}

#[test]
fn status_bar_session_target_updates_session_slot() {
    let mut app = App::new();
    let turn_id = app.begin_turn();

    app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::SetStatusSlot {
            slot: StatusSlot::Session,
            value: StatusValue::SessionRouting {
                root_workflow_id: "root".to_string(),
                active_workflow_id: "chat".to_string(),
                active_workflow_role: WorkflowRunRole::Child,
                recognized_scene_id: Some("chat".to_string()),
                selected_workflow_id: Some("chat".to_string()),
            },
        },
    ));

    assert_eq!(
        app.session_status,
        Some(SessionStatusSummary::Routing(SessionRoutingSummary {
            root_workflow_id: "root".to_string(),
            active_workflow_id: "chat".to_string(),
            active_workflow_role: WorkflowRunRole::Child,
            recognized_scene_id: Some("chat".to_string()),
            selected_workflow_id: Some("chat".to_string()),
        }))
    );
}

#[test]
fn focus_hint_routes_to_visible_logs_panel() {
    let mut app = App::new();
    let turn_id = app.begin_turn();
    app.logs_rect = Rect::new(60, 10, 20, 8);

    app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::FocusHint {
            target: UiTarget::Activity(ActivityTarget::Log),
        },
    ));

    assert_eq!(app.focused_panel, Panel::Logs);
}

#[test]
fn detail_overlay_target_can_be_shown_and_hidden() {
    let mut app = App::new();
    let turn_id = app.begin_turn();

    app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::ShowOverlay(OverlayRequest {
            target: OverlayTarget::Detail,
            content: UiContent::Text("first\nsecond".to_string()),
        }),
    ));

    match app.overlay.as_ref() {
        Some(OverlayState::Detail(detail)) => {
            assert_eq!(
                detail.lines,
                vec!["first".to_string(), "second".to_string()]
            );
        }
        other => panic!("expected detail overlay, got {other:?}"),
    }

    app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::HideOverlay {
            target: OverlayTarget::Detail,
        },
    ));

    assert!(app.overlay.is_none());
}

#[test]
fn search_overlay_target_can_show_runtime_results_and_hide() {
    let mut app = App::new();
    let turn_id = app.begin_turn();

    app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::ShowOverlay(OverlayRequest {
            target: OverlayTarget::Search,
            content: UiContent::Text(
                "Mode: hybrid\n1. crates/omega-context/src/lib.rs".to_string(),
            ),
        }),
    ));

    match app.overlay.as_ref() {
        Some(OverlayState::SearchResults(overlay)) => {
            assert!(overlay.title.contains("Search Results"));
            assert_eq!(
                overlay.lines,
                vec![
                    "Mode: hybrid".to_string(),
                    "1. crates/omega-context/src/lib.rs".to_string(),
                ]
            );
        }
        other => panic!("expected search results overlay, got {other:?}"),
    }

    app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::HideOverlay {
            target: OverlayTarget::Search,
        },
    ));

    assert!(app.overlay.is_none());
}

#[test]
fn diff_preview_effect_opens_detail_overlay() {
    let mut app = App::new();
    let turn_id = app.begin_turn();

    app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::OpenDiffPreview {
            diff: "--- a/file\n+++ b/file\n@@ -1 +1 @@\n-old\n+new".to_string(),
        },
    ));

    match app.overlay.as_ref() {
        Some(OverlayState::Detail(detail)) => {
            assert!(detail.title.contains("Diff Preview"));
            assert!(detail.lines.iter().any(|line| line == "--- a/file"));
        }
        other => panic!("expected detail overlay, got {other:?}"),
    }
}

#[test]
fn request_tool_approval_effect_opens_confirm_overlay() {
    let mut app = App::new();
    let turn_id = app.begin_turn();

    app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::RequestToolApproval {
            message: "workspace_write approval required".to_string(),
        },
    ));

    match app.overlay.as_ref() {
        Some(OverlayState::Confirm(confirm)) => {
            assert!(confirm.title.contains("Approval Required"));
            assert_eq!(confirm.message, "workspace_write approval required");
        }
        other => panic!("expected confirm overlay, got {other:?}"),
    }
}

#[test]
fn request_input_effect_opens_input_overlay() {
    let mut app = App::new();
    let turn_id = app.begin_turn();

    app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::RequestInput {
            prompt: "question: Use the fast path?".to_string(),
        },
    ));

    match app.overlay.as_ref() {
        Some(OverlayState::InputPrompt(prompt)) => {
            assert!(prompt.title.contains("Question"));
            assert!(prompt.prompt.contains("Use the fast path?"));
        }
        other => panic!("expected input overlay, got {other:?}"),
    }
}

#[test]
fn open_web_result_view_effect_opens_search_overlay() {
    let mut app = App::new();
    let turn_id = app.begin_turn();

    app.apply_runtime_envelope(RuntimeUiEnvelope::effect(
        turn_id,
        RuntimeUiEffect::OpenWebResultView {
            title: " Web Search: omega ".to_string(),
            content: "1. Example".to_string(),
        },
    ));

    match app.overlay.as_ref() {
        Some(OverlayState::SearchResults(search)) => {
            assert!(search.title.contains("Web Search: omega"));
            assert!(search.lines.iter().any(|line| line == "1. Example"));
        }
        other => panic!("expected search overlay, got {other:?}"),
    }
}

#[test]
fn panel_hit_testing_distinguishes_todo_and_logs() {
    let mut app = App::new();
    app.todo_rect = Rect::new(60, 1, 20, 8);
    app.logs_rect = Rect::new(60, 9, 20, 10);

    assert_eq!(app.panel_at(10, 5), Panel::Response);
    assert_eq!(app.panel_at(65, 4), Panel::Todo);
    assert_eq!(app.panel_at(65, 12), Panel::Logs);
}

#[test]
fn normalize_focus_returns_to_response_when_sidebar_hides() {
    let mut app = App::new();
    app.focused_panel = Panel::Todo;
    app.todo_rect = Rect::default();
    app.logs_rect = Rect::default();

    app.normalize_focus();

    assert_eq!(app.focused_panel, Panel::Response);
    assert_eq!(app.next_focus_panel(), Panel::Response);
}

#[test]
fn normalize_mode_returns_to_normal_when_input_is_disabled() {
    let mut app = App::new();
    app.interaction_mode = InteractionMode::Insert;
    app.input_enabled = false;

    app.normalize_mode();

    assert_eq!(app.interaction_mode, InteractionMode::Normal);
    assert!(app
        .status_notice
        .as_deref()
        .is_some_and(|notice| notice.contains("Insert mode")));
}

#[test]
fn overlay_close_restores_origin_focus() {
    let mut app = App::new();
    app.focused_panel = Panel::Logs;
    app.logs_rect = Rect::new(60, 10, 20, 8);

    app.open_search_overlay();
    app.focused_panel = Panel::Response;
    app.close_overlay();

    assert_eq!(app.focused_panel, Panel::Logs);
    assert!(!app.overlay_active());
}

#[test]
fn search_match_count_uses_overlay_target_panel() {
    let mut app = App::new();
    app.log_lines = vec!["alpha beta".to_string(), "alpha".to_string()];
    app.focused_panel = Panel::Logs;
    app.open_search_overlay();

    if let Some(OverlayState::Search(overlay)) = app.overlay.as_mut() {
        overlay.query = "alpha".to_string();
        overlay.cursor_pos = 5;
    }

    assert_eq!(app.panel_search_match_count(), Some((Panel::Logs, 2)));
}

#[test]
fn wrapped_selection_copies_without_soft_newlines() {
    let mut app = App::new();
    app.logs_rect = Rect::new(0, 0, 7, 5);
    app.log_lines = vec!["abcdefg".to_string()];

    assert!(app.begin_mouse_selection(Panel::Logs, 2, 1));
    assert!(app.update_mouse_selection(3, 2));

    assert_eq!(app.selected_text().as_deref(), Some("bcdefg"));
}

#[test]
fn panel_text_point_accounts_for_scroll_offset() {
    let mut app = App::new();
    app.logs_rect = Rect::new(0, 0, 8, 5);
    app.log_lines = vec![
        "first".to_string(),
        "second".to_string(),
        "third".to_string(),
    ];
    *app.logs_state.offset_mut() = 1;

    let point = app.panel_text_point_at(Panel::Logs, 1, 1).unwrap();

    assert_eq!(point.line_index, 1);
    assert_eq!(point.column, 0);
}
