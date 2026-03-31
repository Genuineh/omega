use std::collections::BTreeMap;

use omega_context::{ContextDiagnostics, HealthScore};
use omega_core::{ChatEvent, CoreToolResult};
use omega_workflow::{WorkflowStep, WorkflowStepState};
use serde_json::Value;

use crate::runtime_message::{
    ConversationMessage, RuntimeContentKind, RuntimeMessageBridge, RuntimeMessageEnvelope,
    RuntimePriority, RuntimeSource, SessionRoutingStatus, StateMessage, WorkflowStepStatus,
};
use crate::runtime_ui::{
    OverlayRequest, OverlayTarget, ResponseSection, ResponseSectionDelta, ResponseSectionKind,
    ResponseSectionMetadata, ResponseSectionState, StepSubflowRef, StepSubflowStatus, ToolRun,
    ToolRunDetail, ToolRunStatus, UiContent, WorkflowRunRole,
};
use crate::session_state::SessionContext;
use crate::{preview_json_value, preview_text};

pub(crate) fn preview_tool_invocation(tool_name: &str, input: &serde_json::Value) -> String {
    match tool_name {
        "bash" => {
            let command_preview = input
                .get("command")
                .and_then(|value| value.as_str())
                .map(|command| format!("$ {}", preview_text(command, 80)));
            let description = input
                .get("description")
                .and_then(|value| value.as_str())
                .map(|value| preview_text(value.trim(), 40))
                .filter(|value| !value.is_empty());
            let workdir = input
                .get("workdir")
                .and_then(|value| value.as_str())
                .map(str::trim)
                .filter(|value| !value.is_empty() && *value != ".")
                .map(|value| preview_text(value, 30));

            match (description, workdir, command_preview) {
                (Some(description), Some(workdir), Some(command)) => {
                    format!("{} @ {}: {}", description, workdir, command)
                }
                (Some(description), None, Some(command)) => format!("{}: {}", description, command),
                (None, Some(workdir), Some(command)) => format!("{} @ {}", command, workdir),
                (_, _, Some(command)) => command,
                _ => format!("{} {}", tool_name, preview_json_value(input, 80)),
            }
        }
        _ => input
            .get("path")
            .and_then(|value| value.as_str())
            .map(|path| preview_text(path, 80))
            .unwrap_or_else(|| preview_json_value(input, 80)),
    }
}

fn tool_result_preview(tool_result: &CoreToolResult, limit: usize) -> Option<String> {
    tool_result.preview.clone().or_else(|| {
        let preview = preview_text(&tool_result.output, limit);
        (!preview.is_empty()).then_some(preview)
    })
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

fn build_tool_run_detail_lines(
    tool_name: &str,
    input: &serde_json::Value,
    tool_result: Option<&CoreToolResult>,
) -> Vec<String> {
    let mut lines = vec![
        format!("tool: {}", tool_name),
        format!("invoke: {}", preview_tool_invocation(tool_name, input)),
    ];

    if let Some(tool_result) = tool_result {
        if let Some(error_kind) = tool_result.error_kind {
            lines.push(format!("error_kind: {}", error_kind.as_str()));
        }
        if tool_result.truncated {
            lines.push("truncated: true".to_string());
        }
        if tool_result.has_metadata() {
            lines.push("metadata:".to_string());
            lines.extend(preview_lines(
                &serde_json::to_string_pretty(&tool_result.metadata)
                    .unwrap_or_else(|_| "{}".to_string()),
                12,
                160,
            ));
        }
        lines.push("result:".to_string());
        lines.extend(preview_lines(&tool_result.output, 12, 160));
    }

    lines
}

pub(crate) fn send_begin_tool_run(tx: &dyn RuntimeMessageBridge, turn_id: u64, tool_run: ToolRun) {
    tx.send(RuntimeMessageEnvelope::conversation(
        turn_id,
        ConversationMessage::BeginToolRun { tool_run },
    ));
}

pub(crate) fn send_update_tool_run(tx: &dyn RuntimeMessageBridge, turn_id: u64, tool_run: ToolRun) {
    tx.send(RuntimeMessageEnvelope::conversation(
        turn_id,
        ConversationMessage::UpdateToolRun { tool_run },
    ));
}

pub(crate) fn send_complete_tool_run(
    tx: &dyn RuntimeMessageBridge,
    turn_id: u64,
    id: &str,
    status: ToolRunStatus,
) {
    tx.send(RuntimeMessageEnvelope::conversation(
        turn_id,
        ConversationMessage::CompleteToolRun {
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

pub(crate) fn send_workflow_step(
    tx: &dyn RuntimeMessageBridge,
    turn_id: u64,
    step: Option<WorkflowStepState>,
    workflow_id: &str,
    role: WorkflowRunRole,
) {
    let Some(step) = step else {
        return;
    };

    tx.send(RuntimeMessageEnvelope::state(
        turn_id,
        StateMessage::WorkflowStep(WorkflowStepStatus {
            workflow_id: workflow_id.to_string(),
            workflow_role: role,
            step_id: step.id.clone(),
            step_label: step.label.clone(),
            index: step.index,
            total: step.total,
        }),
    ));
    tx.send(RuntimeMessageEnvelope::state(
        turn_id,
        StateMessage::Activity {
            source: RuntimeSource::WorkflowStep {
                workflow_id: workflow_id.to_string(),
                workflow_role: role,
                step_id: step.id.clone(),
                step_label: step.label.clone(),
                index: step.index,
                total: step.total,
            },
            kind: RuntimeContentKind::Summary,
            text: step.label.clone(),
            priority: None,
        },
    ));
}

pub(crate) fn send_step_subflow_status(
    tx: &dyn RuntimeMessageBridge,
    turn_id: u64,
    subflow: StepSubflowStatus,
) {
    tx.send(RuntimeMessageEnvelope::state(
        turn_id,
        StateMessage::StepSubflow { subflow },
    ));
}

pub(crate) fn send_step_text(
    tx: &dyn RuntimeMessageBridge,
    turn_id: u64,
    workflow_id: &str,
    role: WorkflowRunRole,
    step: &WorkflowStep,
    text: &str,
) {
    tx.send(RuntimeMessageEnvelope::conversation(
        turn_id,
        ConversationMessage::Text {
            source: RuntimeSource::WorkflowStep {
                workflow_id: workflow_id.to_string(),
                workflow_role: role,
                step_id: step.id.clone(),
                step_label: step.label.clone(),
                index: 0,
                total: 0,
            },
            kind: RuntimeContentKind::Narrative,
            text: text.to_string(),
            priority: None,
        },
    ));
}

pub(crate) fn send_assistant_text(tx: &dyn RuntimeMessageBridge, turn_id: u64, text: &str) {
    tx.send(RuntimeMessageEnvelope::conversation(
        turn_id,
        ConversationMessage::Text {
            source: RuntimeSource::Assistant,
            kind: RuntimeContentKind::Result,
            text: text.to_string(),
            priority: None,
        },
    ));
}

pub(crate) fn send_error_text(tx: &dyn RuntimeMessageBridge, turn_id: u64, text: &str) {
    tx.send(RuntimeMessageEnvelope::conversation(
        turn_id,
        ConversationMessage::Text {
            source: RuntimeSource::System,
            kind: RuntimeContentKind::Error,
            text: text.to_string(),
            priority: Some(RuntimePriority::High),
        },
    ));
}

pub(crate) fn send_warning_text(tx: &dyn RuntimeMessageBridge, turn_id: u64, text: &str) {
    tx.send(RuntimeMessageEnvelope::state(
        turn_id,
        StateMessage::Activity {
            source: RuntimeSource::System,
            kind: RuntimeContentKind::Warning,
            text: text.to_string(),
            priority: Some(RuntimePriority::Normal),
        },
    ));
}

pub(crate) fn send_system_log_text(tx: &dyn RuntimeMessageBridge, turn_id: u64, text: &str) {
    tx.send(RuntimeMessageEnvelope::state(
        turn_id,
        StateMessage::Activity {
            source: RuntimeSource::System,
            kind: RuntimeContentKind::Log,
            text: text.to_string(),
            priority: None,
        },
    ));
}

pub(crate) fn send_tool_call_preview(
    tx: &dyn RuntimeMessageBridge,
    turn_id: u64,
    tool_name: &str,
    command: Option<String>,
    preview: String,
) {
    let source = RuntimeSource::Tool {
        tool_name: tool_name.to_string(),
    };

    if let Some(command) = command {
        tx.send(RuntimeMessageEnvelope::state(
            turn_id,
            StateMessage::Activity {
                source: source.clone(),
                kind: RuntimeContentKind::Log,
                text: format!("$ {command}"),
                priority: None,
            },
        ));
    }

    tx.send(RuntimeMessageEnvelope::state(
        turn_id,
        StateMessage::Activity {
            source,
            kind: RuntimeContentKind::Log,
            text: preview,
            priority: None,
        },
    ));
}

pub(crate) fn send_todo_snapshot(tx: &dyn RuntimeMessageBridge, turn_id: u64, rendered: &str) {
    tx.send(RuntimeMessageEnvelope::state(
        turn_id,
        StateMessage::TodoSnapshot {
            rendered: rendered.to_string(),
        },
    ));
}

pub(crate) fn send_show_overlay(
    tx: &dyn RuntimeMessageBridge,
    turn_id: u64,
    request: OverlayRequest,
) {
    tx.send(RuntimeMessageEnvelope::state(
        turn_id,
        StateMessage::ShowOverlay { request },
    ));
}

pub(crate) fn maybe_emit_context_observability(
    tx: &dyn RuntimeMessageBridge,
    turn_id: u64,
    tool_name: &str,
    tool_input: &Value,
    tool_result: &CoreToolResult,
    context: &ContextDiagnostics,
) {
    if tool_result.is_error() {
        return;
    }

    emit_index_scan_activity(tx, turn_id, &tool_result.metadata, context);

    match tool_name {
        "search_codebase" => {
            emit_search_observability(tx, turn_id, tool_input, tool_result, context)
        }
        "manage_document" => emit_document_observability(tx, turn_id, tool_result, context),
        _ => {}
    }
}

pub(crate) fn send_begin_response_section(
    tx: &dyn RuntimeMessageBridge,
    turn_id: u64,
    section: ResponseSection,
) {
    tx.send(RuntimeMessageEnvelope::conversation(
        turn_id,
        ConversationMessage::BeginSection { section },
    ));
}

pub(crate) fn send_append_response_section(
    tx: &dyn RuntimeMessageBridge,
    turn_id: u64,
    id: &str,
    delta: ResponseSectionDelta,
) {
    tx.send(RuntimeMessageEnvelope::conversation(
        turn_id,
        ConversationMessage::AppendSection {
            id: id.to_string(),
            delta,
        },
    ));
}

pub(crate) fn send_complete_response_section(
    tx: &dyn RuntimeMessageBridge,
    turn_id: u64,
    id: &str,
    state: ResponseSectionState,
) {
    tx.send(RuntimeMessageEnvelope::conversation(
        turn_id,
        ConversationMessage::CompleteSection {
            id: id.to_string(),
            state,
        },
    ));
}

pub(crate) fn send_turn_finished(tx: &dyn RuntimeMessageBridge, turn_id: u64) {
    tx.send(RuntimeMessageEnvelope::state(
        turn_id,
        StateMessage::TurnFinished,
    ));
}

pub(crate) fn send_session_status(
    tx: &dyn RuntimeMessageBridge,
    turn_id: u64,
    root_workflow_id: &str,
    session_context: &SessionContext,
) {
    tx.send(RuntimeMessageEnvelope::state(
        turn_id,
        StateMessage::SessionRouting(SessionRoutingStatus {
            root_workflow_id: root_workflow_id.to_string(),
            active_workflow_id: session_context.routing.active_workflow_id.clone(),
            active_workflow_role: session_context.routing.active_workflow_role,
            recognized_scene_id: session_context.routing.recognized_scene_id.clone(),
            selected_workflow_id: session_context.routing.selected_workflow_id.clone(),
        }),
    ));
}

pub(crate) fn send_routing_log(tx: &dyn RuntimeMessageBridge, turn_id: u64, text: String) {
    tx.send(RuntimeMessageEnvelope::state(
        turn_id,
        StateMessage::Activity {
            source: RuntimeSource::SessionRouting,
            kind: RuntimeContentKind::Summary,
            text,
            priority: None,
        },
    ));
}

fn emit_search_observability(
    tx: &dyn RuntimeMessageBridge,
    turn_id: u64,
    tool_input: &Value,
    tool_result: &CoreToolResult,
    context: &ContextDiagnostics,
) {
    let query = metadata_string(&tool_result.metadata, &["query"])
        .or_else(|| json_string(tool_input, &["query"]))
        .unwrap_or_else(|| "(unknown)".to_string());
    let mode =
        metadata_string(&tool_result.metadata, &["mode"]).unwrap_or_else(|| "keyword".to_string());
    let result_count = metadata_u64(&tool_result.metadata, &["result_count"]).unwrap_or(0);

    send_system_log_text(
        tx,
        turn_id,
        &format!(
            "context.search query={query:?} mode={mode} results={result_count} files={} chunks={} stale={}s",
            context.document.total_files_indexed,
            context.document.total_chunks,
            context.document.index_staleness_seconds,
        ),
    );
    send_show_overlay(
        tx,
        turn_id,
        OverlayRequest {
            target: OverlayTarget::Search,
            content: UiContent::Text(build_search_overlay_text(
                &query,
                &mode,
                result_count,
                &tool_result.output,
                context,
            )),
        },
    );
}

fn emit_document_observability(
    tx: &dyn RuntimeMessageBridge,
    turn_id: u64,
    tool_result: &CoreToolResult,
    context: &ContextDiagnostics,
) {
    let action = metadata_string(&tool_result.metadata, &["action"])
        .unwrap_or_else(|| "unknown".to_string());
    let ok = metadata_bool(&tool_result.metadata, &["ok"]).unwrap_or(true);
    let file_count = metadata_u64(&tool_result.metadata, &["file_count"]).unwrap_or(0);
    let warning_count = metadata_u64(&tool_result.metadata, &["warning_count"]).unwrap_or(0);
    let issues = document_issue_count(&tool_result.metadata);
    let summary = format!(
        "document.{action} ok={ok} files={file_count} warnings={warning_count} issues={issues} health={} indexed_files={} todo={}",
        health_score_label(context.document.governance_health),
        context.document.total_files_indexed,
        context.store.todo_items_count,
    );

    if warning_count > 0 || issues > 0 || !ok {
        send_warning_text(tx, turn_id, &summary);
    } else {
        send_system_log_text(tx, turn_id, &summary);
    }

    if action == "health_check" {
        send_show_overlay(
            tx,
            turn_id,
            OverlayRequest {
                target: OverlayTarget::Detail,
                content: UiContent::Text(build_document_health_overlay_text(tool_result, context)),
            },
        );
    }
}

fn emit_index_scan_activity(
    tx: &dyn RuntimeMessageBridge,
    turn_id: u64,
    metadata: &Value,
    context: &ContextDiagnostics,
) {
    let files_indexed = metadata_u64(metadata, &["scan", "files_indexed"])
        .unwrap_or(context.document.total_files_indexed);
    if files_indexed == 0 {
        return;
    }
    let chunks_indexed = metadata_u64(metadata, &["scan", "chunks_indexed"])
        .unwrap_or(context.document.total_chunks);
    let deleted_marked = metadata_u64(metadata, &["scan", "deleted_marked"]).unwrap_or(0);

    send_system_log_text(
        tx,
        turn_id,
        &format!(
            "context.index files={} chunks={} deleted={} stale={}s tantivy={} lance={}",
            files_indexed,
            chunks_indexed,
            deleted_marked,
            context.document.index_staleness_seconds,
            context.store.tantivy_index_size_bytes,
            context.store.lance_db_size_bytes,
        ),
    );
}

fn build_search_overlay_text(
    query: &str,
    mode: &str,
    result_count: u64,
    output: &str,
    context: &ContextDiagnostics,
) -> String {
    format!(
        "Search results\nquery: {query}\nmode: {mode}\nresults: {result_count}\nindexed_files: {}\nindexed_chunks: {}\nindex_staleness_seconds: {}\nstore_tantivy_bytes: {}\nstore_lance_bytes: {}\n\n{output}",
        context.document.total_files_indexed,
        context.document.total_chunks,
        context.document.index_staleness_seconds,
        context.store.tantivy_index_size_bytes,
        context.store.lance_db_size_bytes,
    )
}

fn build_document_health_overlay_text(
    tool_result: &CoreToolResult,
    context: &ContextDiagnostics,
) -> String {
    let health_score = health_score_label(context.document.governance_health).to_string();
    let structure_violations =
        metadata_u64(&tool_result.metadata, &["health", "structure_violations"]).unwrap_or(0);
    let naming_violations =
        metadata_u64(&tool_result.metadata, &["health", "naming_violations"]).unwrap_or(0);
    let broken_crossrefs =
        metadata_u64(&tool_result.metadata, &["health", "broken_crossrefs"]).unwrap_or(0);
    let stale_docs = metadata_u64(&tool_result.metadata, &["health", "stale_docs"]).unwrap_or(0);
    let missing_frontmatter =
        metadata_u64(&tool_result.metadata, &["health", "missing_frontmatter"]).unwrap_or(0);
    let orphaned_docs =
        metadata_u64(&tool_result.metadata, &["health", "orphaned_docs"]).unwrap_or(0);

    format!(
        "Document health\nscore: {}\nindexed_files: {}\nindexed_chunks: {}\nindex_staleness_seconds: {}\nstore_tantivy_bytes: {}\nstore_lance_bytes: {}\ntodo_items_count: {}\nturn_archive_count: {}\nstructure_violations: {}\nnaming_violations: {}\nbroken_crossrefs: {}\nstale_docs: {}\nmissing_frontmatter: {}\norphaned_docs: {}\n\n{}",
        health_score,
        context.document.total_files_indexed,
        context.document.total_chunks,
        context.document.index_staleness_seconds,
        context.store.tantivy_index_size_bytes,
        context.store.lance_db_size_bytes,
        context.store.todo_items_count,
        context.store.turn_archive_count,
        structure_violations,
        naming_violations,
        broken_crossrefs,
        stale_docs,
        missing_frontmatter,
        orphaned_docs,
        tool_result.output,
    )
}

fn health_score_label(score: Option<HealthScore>) -> &'static str {
    match score {
        Some(HealthScore::Good) => "good",
        Some(HealthScore::NeedsAttention) => "needs_attention",
        Some(HealthScore::Critical) => "critical",
        None => "unknown",
    }
}

fn document_issue_count(metadata: &Value) -> u64 {
    [
        ["health", "structure_violations"],
        ["health", "naming_violations"],
        ["health", "broken_crossrefs"],
        ["health", "stale_docs"],
        ["health", "missing_frontmatter"],
        ["health", "orphaned_docs"],
    ]
    .into_iter()
    .filter_map(|path| metadata_u64(metadata, &path))
    .sum()
}

fn json_value_at_path<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    let mut current = value;
    for segment in path {
        current = current.get(*segment)?;
    }
    Some(current)
}

fn metadata_string(metadata: &Value, path: &[&str]) -> Option<String> {
    json_value_at_path(metadata, path)?
        .as_str()
        .map(ToOwned::to_owned)
}

fn json_string(value: &Value, path: &[&str]) -> Option<String> {
    json_value_at_path(value, path)?
        .as_str()
        .map(ToOwned::to_owned)
}

fn metadata_u64(metadata: &Value, path: &[&str]) -> Option<u64> {
    json_value_at_path(metadata, path)?.as_u64()
}

fn metadata_bool(metadata: &Value, path: &[&str]) -> Option<bool> {
    json_value_at_path(metadata, path)?.as_bool()
}

pub(crate) struct StepResponseStreamer<'a> {
    tx: &'a dyn RuntimeMessageBridge,
    turn_id: u64,
    primary_section_id: String,
    thinking_section_id: String,
    primary_section: ResponseSection,
    thinking_section: ResponseSection,
    thinking_started: bool,
    stream_primary_text: bool,
    primary_sanitizer: ProviderMarkupSanitizer,
    thinking_sanitizer: ProviderMarkupSanitizer,
}

impl<'a> StepResponseStreamer<'a> {
    pub(crate) fn new(
        tx: &'a dyn RuntimeMessageBridge,
        turn_id: u64,
        workflow_id: &str,
        role: WorkflowRunRole,
        step: &WorkflowStep,
        is_final_step: bool,
        scene_id: Option<&str>,
        current_item: Option<&crate::hook_adapter::ExecuteLoopItemContext>,
        stream_primary_text: bool,
    ) -> Self {
        let primary_step_id = current_item
            .map(|item| item.child_step_id.as_str())
            .unwrap_or(step.id.as_str());
        let base_id = format!(
            "turn-{turn_id}:{}:{}:{}",
            role.as_str(),
            workflow_id,
            primary_step_id
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
            subflow_ref: current_item.map(|item| StepSubflowRef {
                parent_workflow_id: workflow_id.to_string(),
                parent_step_id: step.id.clone(),
                parent_step_label: step.label.clone(),
                subflow_id: item.child_step_id.clone(),
                item_id: Some(item.item_id.clone()),
                item_label: item.item_label.clone(),
                item_index: item.item_index,
                item_total: item.item_total,
            }),
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
            stream_primary_text,
            primary_sanitizer: ProviderMarkupSanitizer::default(),
            thinking_sanitizer: ProviderMarkupSanitizer::default(),
        }
    }

    pub(crate) fn begin(&self) {
        send_begin_response_section(self.tx, self.turn_id, self.primary_section.clone());
    }

    pub(crate) fn primary_section_id(&self) -> &str {
        &self.primary_section_id
    }

    pub(crate) fn push_chat_event(&mut self, event: &ChatEvent) {
        match event {
            ChatEvent::TextDelta { text } if !text.is_empty() => {
                let sanitized = self.primary_sanitizer.push(text);
                if self.stream_primary_text {
                    self.append_primary_text(&sanitized);
                }
            }
            ChatEvent::ThinkingDelta { thinking, .. } if !thinking.is_empty() => {
                let sanitized = self.thinking_sanitizer.push(thinking);
                self.append_thinking_text(&sanitized);
            }
            _ => {}
        }
    }

    pub(crate) fn append_final_text(&mut self, text: &str) {
        self.append_primary_text(text);
    }

    pub(crate) fn complete(&mut self) {
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

    pub(crate) fn fail(&mut self) {
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
        if self.stream_primary_text {
            self.append_primary_text(&remaining_primary);
        }

        let remaining_thinking = self.thinking_sanitizer.finish();
        self.append_thinking_text(&remaining_thinking);
    }
}

pub(crate) struct ToolRunTracker<'a> {
    tx: &'a dyn RuntimeMessageBridge,
    turn_id: u64,
    parent_section_id: String,
    tool_runs: BTreeMap<String, ToolRun>,
}

impl<'a> ToolRunTracker<'a> {
    pub(crate) fn new(
        tx: &'a dyn RuntimeMessageBridge,
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

    pub(crate) fn observe_chat_event(&mut self, event: &ChatEvent) {
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

    pub(crate) fn complete_tool_run(
        &mut self,
        tool_use_id: &str,
        tool_name: &str,
        tool_input: &serde_json::Value,
        tool_result: &CoreToolResult,
    ) {
        let status = if tool_result.is_error() {
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
        tool_run.result_preview = tool_result_preview(tool_result, 100);
        tool_run.detail = ToolRunDetail {
            title: format!(" Tool: {} ", tool_name),
            lines: build_tool_run_detail_lines(tool_name, tool_input, Some(tool_result)),
        };

        send_update_tool_run(self.tx, self.turn_id, tool_run.clone());
        send_complete_tool_run(self.tx, self.turn_id, &tool_run.id, status);
        self.tool_runs.insert(tool_run.id.clone(), tool_run);
    }
}

#[derive(Default)]
pub(crate) struct ProviderMarkupSanitizer {
    carry: String,
    stripping_until: Option<&'static str>,
}

impl ProviderMarkupSanitizer {
    pub(crate) fn push(&mut self, chunk: &str) -> String {
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

    pub(crate) fn finish(&mut self) -> String {
        if self.stripping_until.is_some() {
            self.carry.clear();
            self.stripping_until = None;
            String::new()
        } else {
            std::mem::take(&mut self.carry)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use omega_context::{
        ContextBudgetDiagnostics, ContextDiagnostics, ContextDocumentDiagnostics,
        ContextMemoryDiagnostics, ContextStoreDiagnostics,
    };
    use omega_core::CoreToolResult;
    use serde_json::json;

    use super::maybe_emit_context_observability;
    use crate::runtime_message::{RuntimeMessage, RuntimeMessageEnvelope, StateMessage};
    use crate::{OverlayTarget, UiContent};

    fn drain_envelopes(rx: &mpsc::Receiver<RuntimeMessageEnvelope>) -> Vec<RuntimeMessageEnvelope> {
        let mut envelopes = Vec::new();
        while let Ok(envelope) = rx.try_recv() {
            envelopes.push(envelope);
        }
        envelopes
    }

    fn sample_context_diagnostics() -> ContextDiagnostics {
        ContextDiagnostics {
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
                governance_health: Some(omega_context::HealthScore::NeedsAttention),
                last_health_check: Some(2),
            },
            store: ContextStoreDiagnostics {
                lance_db_size_bytes: 4096,
                tantivy_index_size_bytes: 2048,
                todo_items_count: 3,
                turn_archive_count: 2,
            },
        }
    }

    #[test]
    fn search_observability_emits_index_log_and_search_overlay() {
        let (tx, rx) = mpsc::channel();
        let tool_result = CoreToolResult::success(
            r#"[{"path":"docs/specs/omega-context-management.md","score":0.9}]"#,
        )
        .with_preview("1 result")
        .with_metadata(json!({
            "query": "context management",
            "mode": "hybrid",
            "result_count": 1,
            "scan": {
                "files_indexed": 12,
                "chunks_indexed": 48,
                "deleted_marked": 0
            }
        }));

        maybe_emit_context_observability(
            &tx,
            7,
            "search_codebase",
            &json!({ "query": "context management" }),
            &tool_result,
            &sample_context_diagnostics(),
        );

        let envelopes = drain_envelopes(&rx);
        assert!(envelopes.iter().any(|envelope| matches!(
            &envelope.message,
            RuntimeMessage::State(StateMessage::Activity { text, .. })
                if text.contains("context.index files=12 chunks=48 deleted=0")
                    && text.contains("tantivy=2048")
        )));
        assert!(envelopes.iter().any(|envelope| matches!(
            &envelope.message,
            RuntimeMessage::State(StateMessage::Activity { text, .. })
                if text.contains("context.search") && text.contains("hybrid")
        )));
        assert!(envelopes.iter().any(|envelope| matches!(
            &envelope.message,
            RuntimeMessage::State(StateMessage::ShowOverlay { request })
                if request.target == OverlayTarget::Search
                    && matches!(&request.content, UiContent::Text(text) if text.contains("query: context management") && text.contains("results: 1") && text.contains("store_lance_bytes: 4096"))
        )));
    }

    #[test]
    fn document_health_observability_emits_warning_and_detail_overlay() {
        let (tx, rx) = mpsc::channel();
        let tool_result = CoreToolResult::success(
            json!({
                "health": {
                    "overall_health": "needs_attention",
                    "broken_crossrefs": ["docs/README.md -> missing.md"]
                }
            })
            .to_string(),
        )
        .with_preview("health check")
        .with_metadata(json!({
            "action": "health_check",
            "ok": true,
            "file_count": 3,
            "warning_count": 1,
            "health": {
                "overall_health": "needs_attention",
                "structure_violations": 0,
                "naming_violations": 0,
                "broken_crossrefs": 1,
                "stale_docs": 0,
                "missing_frontmatter": 0,
                "orphaned_docs": 0
            },
            "scan": {
                "files_indexed": 3,
                "chunks_indexed": 9,
                "deleted_marked": 0
            }
        }));

        maybe_emit_context_observability(
            &tx,
            9,
            "manage_document",
            &json!({}),
            &tool_result,
            &sample_context_diagnostics(),
        );

        let envelopes = drain_envelopes(&rx);
        assert!(envelopes.iter().any(|envelope| matches!(
            &envelope.message,
            RuntimeMessage::State(StateMessage::Activity { text, .. })
                if text.contains("document.health") && text.contains("issues=1")
        )));
        assert!(envelopes.iter().any(|envelope| matches!(
            &envelope.message,
            RuntimeMessage::State(StateMessage::ShowOverlay { request })
                if request.target == OverlayTarget::Detail
                    && matches!(&request.content, UiContent::Text(text) if text.contains("Document health") && text.contains("broken_crossrefs: 1") && text.contains("score: needs_attention") && text.contains("todo_items_count: 3"))
        )));
    }
}
