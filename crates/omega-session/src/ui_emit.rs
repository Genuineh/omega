use std::collections::BTreeMap;

use omega_context::{ContextDiagnostics, DocumentHealthStatus, HealthScore};
use omega_core::{
    ChatEvent, CoreToolExecutionContext, CoreToolManifestMetadata, CoreToolResult,
};
use omega_workflow::{WorkflowStep, WorkflowStepState};
use serde_json::Value;

use crate::runtime_message::{
    ConversationMessage, RuntimeContentKind, RuntimeMessageBridge, RuntimeMessageEnvelope,
    RuntimePriority, RuntimeSource, SessionRoutingStatus, StateMessage, WorkflowStepStatus,
};
use crate::runtime_ui::{
    OverlayRequest, OverlayTarget, ResponseSection, ResponseSectionDelta, ResponseSectionKind,
    ResponseSectionMetadata, ResponseSectionState, SectionOrigin, StepSubflowRef, StepSubflowStatus,
    ToolCapabilityDiagnostics, ToolRun, ToolRunDetail, ToolRunStatus, UiContent,
    WorkflowRunRole,
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
    manifest: Option<&CoreToolManifestMetadata>,
    execution_context: Option<&CoreToolExecutionContext>,
) -> Vec<String> {
    let mut lines = vec![
        format!("tool: {}", tool_name),
        format!("invoke: {}", preview_tool_invocation(tool_name, input)),
    ];

    if let Some(manifest) = manifest {
        lines.push(format!("family: {}", manifest.family.as_str()));
        lines.push(format!("stability: {}", manifest.stability.as_str()));
        lines.push(format!("summary: {}", manifest.prompt.summary));
        if let Some(permissions) = manifest.permissions.as_ref() {
            lines.push(format!(
                "permission: {} [{}{}]",
                permissions.permission_class,
                permissions.default_policy_mode,
                if permissions.requires_approval {
                    ", approval required"
                } else {
                    ""
                }
            ));
        }
        if let Some(storage) = manifest.storage.as_ref() {
            let effects = storage_effect_labels(storage);
            if !effects.is_empty() {
                lines.push(format!("storage: {}", effects.join(", ")));
            }
        }
        if let Some(context) = manifest.context.as_ref() {
            lines.push(format!(
                "context: workspace_root={} step_metadata={} memory_scope={} network={}",
                context.needs_workspace_root,
                context.needs_step_metadata,
                context.memory_scope.as_str(),
                context.network_context,
            ));
        }
        if let Some(observability) = manifest.observability.as_ref() {
            lines.push(format!(
                "metrics: {}, {}, {}",
                observability.invocation_metric,
                observability.success_metric,
                observability.failure_metric
            ));
        }
    }

    if let Some(execution_context) = execution_context {
        lines.push(format!(
            "execution: {}:{}:{} turn={} workspace={}",
            execution_context.workflow_role,
            execution_context.workflow_id,
            execution_context.step_id,
            execution_context.turn_id,
            execution_context.workspace_root,
        ));
        if let Some(item_id) = execution_context.current_item_id.as_deref() {
            lines.push(format!(
                "item: {} ({}/{})",
                item_id,
                execution_context.current_item_index.unwrap_or_default(),
                execution_context.current_item_total.unwrap_or_default(),
            ));
        }
    }

    if let Some(tool_result) = tool_result {
        if let Some(error_kind) = tool_result.error_kind {
            lines.push(format!("error_kind: {}", error_kind.as_str()));
        }
        if let Some(remediation) = tool_result.remediation.as_ref() {
            lines.push(format!("remediation.kind: {}", remediation.kind.as_str()));
            lines.push(format!("remediation.suggestion: {}", remediation.suggestion));
            if !remediation.alternative_tools.is_empty() {
                lines.push(format!(
                    "remediation.alternatives: {}",
                    remediation.alternative_tools.join(", ")
                ));
            }
            lines.push(format!("remediation.recoverable: {}", remediation.recoverable));
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

fn storage_effect_labels(storage: &omega_core::CoreToolStorageProfile) -> Vec<&'static str> {
    let mut labels = Vec::new();
    if storage.writes_session_journal {
        labels.push("session_journal");
    }
    if storage.produces_artifact {
        labels.push("artifact");
    }
    if storage.writes_memory {
        labels.push("memory");
    }
    if storage.writes_todo {
        labels.push("todo");
    }
    if storage.replayable {
        labels.push("replayable");
    }
    labels
}

fn maybe_emit_tool_capability_effects(
    tx: &dyn RuntimeMessageBridge,
    turn_id: u64,
    manifest: Option<&CoreToolManifestMetadata>,
    execution_context: &CoreToolExecutionContext,
    tool_result: &CoreToolResult,
) {
    let Some(manifest) = manifest else {
        return;
    };

    if !tool_result.is_error() {
        if let Some(storage) = manifest.storage.as_ref() {
            let effects = storage_effect_labels(storage);
            if !effects.is_empty() {
                send_system_log_text(
                    tx,
                    turn_id,
                    &format!(
                        "tool.storage tool={} step={}:{} effects={}",
                        manifest.id,
                        execution_context.workflow_id,
                        execution_context.step_id,
                        effects.join(","),
                    ),
                );
            }
        }

        if manifest.family == omega_core::CoreToolFamily::Editing {
            if let Some(diff) = extract_diff_preview(tool_result) {
                tx.send(RuntimeMessageEnvelope::state(
                    turn_id,
                    StateMessage::OpenDiffPreview { diff },
                ));
            }
        }

        if manifest.id == "ask_user_question" {
            if let Some(prompt) = build_input_prompt(tool_result) {
                tx.send(RuntimeMessageEnvelope::state(
                    turn_id,
                    StateMessage::RequestInput { prompt },
                ));
            }
        }

        if manifest.family == omega_core::CoreToolFamily::WebResearch {
            if let Some((title, content)) = build_web_result_view(manifest, tool_result) {
                tx.send(RuntimeMessageEnvelope::state(
                    turn_id,
                    StateMessage::OpenWebResultView { title, content },
                ));
            }
        }
    }

    if tool_result.error_kind == Some(omega_core::CoreToolErrorKind::Policy)
        && manifest
            .permissions
            .as_ref()
            .is_some_and(|permissions| permissions.requires_approval)
    {
        tx.send(RuntimeMessageEnvelope::state(
            turn_id,
            StateMessage::RequestToolApproval {
                message: build_tool_approval_message(manifest, execution_context, tool_result),
            },
        ));
    }
}

fn build_input_prompt(tool_result: &CoreToolResult) -> Option<String> {
    let question = tool_result.metadata.get("question")?.as_str()?.trim();
    if question.is_empty() {
        return None;
    }

    let mut lines = vec![format!("question: {question}")];
    if let Some(context) = tool_result.metadata.get("context").and_then(|value| value.as_str()) {
        let context = context.trim();
        if !context.is_empty() {
            lines.push(format!("context: {context}"));
        }
    }
    if let Some(options) = tool_result.metadata.get("options").and_then(|value| value.as_array()) {
        let options = options
            .iter()
            .filter_map(|value| value.as_str())
            .collect::<Vec<_>>();
        if !options.is_empty() {
            lines.push(format!("options: {}", options.join(", ")));
        }
    }
    lines.push(format!(
        "allow_freeform: {}",
        tool_result
            .metadata
            .get("allow_freeform")
            .and_then(|value| value.as_bool())
            .unwrap_or(true)
    ));
    Some(lines.join("\n"))
}

fn build_web_result_view(
    manifest: &CoreToolManifestMetadata,
    tool_result: &CoreToolResult,
) -> Option<(String, String)> {
    let content = tool_result.output.trim();
    if content.is_empty() {
        return None;
    }

    let title = match manifest.id.as_str() {
        "web_search" => {
            let query = tool_result
                .metadata
                .get("query")
                .and_then(|value| value.as_str())
                .unwrap_or("web search");
            format!(" Web Search: {query} ")
        }
        "web_fetch" => {
            let url = tool_result
                .metadata
                .get("url")
                .and_then(|value| value.as_str())
                .unwrap_or("web fetch");
            format!(" Web Fetch: {url} ")
        }
        _ => " Web Result ".to_string(),
    };

    Some((title, content.to_string()))
}

fn extract_diff_preview(tool_result: &CoreToolResult) -> Option<String> {
    if tool_result
        .metadata
        .get("diff_available")
        .and_then(|value| value.as_bool())
        != Some(true)
    {
        return None;
    }

    if let Some((_, diff)) = tool_result.output.split_once("\n\n") {
        let diff = diff.trim();
        if !diff.is_empty() {
            return Some(diff.to_string());
        }
    }

    let output = tool_result.output.trim();
    (!output.is_empty()).then(|| output.to_string())
}

fn build_tool_approval_message(
    manifest: &CoreToolManifestMetadata,
    execution_context: &CoreToolExecutionContext,
    tool_result: &CoreToolResult,
) -> String {
    let mut lines = vec![format!(
        "Tool '{}' requires approval in {}:{}.",
        manifest.display_name, execution_context.workflow_id, execution_context.step_id
    )];
    if let Some(permissions) = manifest.permissions.as_ref() {
        lines.push(format!(
            "permission_class: {} ({})",
            permissions.permission_class, permissions.default_policy_mode
        ));
        if let Some(remediation) = permissions.denial_remediation.as_deref() {
            lines.push(format!("guidance: {remediation}"));
        }
    }
    if let Some(remediation) = tool_result.remediation.as_ref() {
        lines.push(format!("next_step: {}", remediation.suggestion));
        if !remediation.alternative_tools.is_empty() {
            lines.push(format!(
                "alternatives: {}",
                remediation.alternative_tools.join(", ")
            ));
        }
    }
    lines.join("\n")
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
        "document.{action} ok={ok} files={file_count} warnings={warning_count} issues={issues} health={} governance={} indexed_files={} todo={}",
        document_health_label(context),
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
    let vector_ignored = metadata_u64(metadata, &["scan", "vector_ignored_files"]).unwrap_or(0);

    send_system_log_text(
        tx,
        turn_id,
        &format!(
            "context.index files={} chunks={} deleted={} vector_ignored={} stale={}s tantivy={} lance={}",
            files_indexed,
            chunks_indexed,
            deleted_marked,
            vector_ignored,
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
    let health_status = document_health_label(context).to_string();
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
        "Document health\nstatus: {}\nscore: {}\nindexed_files: {}\nindexed_chunks: {}\nindex_staleness_seconds: {}\nstore_tantivy_bytes: {}\nstore_lance_bytes: {}\ntodo_items_count: {}\nturn_archive_count: {}\nlast_health_check: {}\nactive_version: {}\npending_version: {}\npromotion_error: {}\nstructure_violations: {}\nnaming_violations: {}\nbroken_crossrefs: {}\nstale_docs: {}\nmissing_frontmatter: {}\norphaned_docs: {}\n\n{}",
        health_status,
        health_score,
        context.document.total_files_indexed,
        context.document.total_chunks,
        context.document.index_staleness_seconds,
        context.store.tantivy_index_size_bytes,
        context.store.lance_db_size_bytes,
        context.store.todo_items_count,
        context.store.turn_archive_count,
        context
            .document
            .last_health_check
            .map(|value| value.to_string())
            .unwrap_or_else(|| "never".to_string()),
        format_store_version(context.document.active_version.as_ref()),
        format_store_version(context.document.pending_version.as_ref()),
        context
            .document
            .last_promotion_error
            .as_deref()
            .unwrap_or("none"),
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

fn document_health_label(context: &ContextDiagnostics) -> &'static str {
    match context.document.health_status {
        DocumentHealthStatus::NeverChecked => "never_checked",
        DocumentHealthStatus::Good => "good",
        DocumentHealthStatus::NeedsAttention => "needs_attention",
        DocumentHealthStatus::Critical => "critical",
        DocumentHealthStatus::Failed => "failed",
    }
}

fn format_store_version(version: Option<&omega_context::DocumentStoreVersion>) -> String {
    version
        .map(|version| {
            format!(
                "{} rev={} path={}",
                version.version_id, version.manifest_revision, version.storage_path
            )
        })
        .unwrap_or_else(|| "none".to_string())
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
            origin: SectionOrigin::Workflow {
                workflow_id: workflow_id.to_string(),
                workflow_role: role,
            },
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
    manifests: BTreeMap<String, CoreToolManifestMetadata>,
    execution_context: CoreToolExecutionContext,
    tool_runs: BTreeMap<String, ToolRun>,
    tool_metrics: ToolCapabilityDiagnostics,
    last_failed_tool_name: Option<String>,
}

impl<'a> ToolRunTracker<'a> {
    pub(crate) fn new(
        tx: &'a dyn RuntimeMessageBridge,
        turn_id: u64,
        parent_section_id: String,
        manifests: BTreeMap<String, CoreToolManifestMetadata>,
        execution_context: CoreToolExecutionContext,
    ) -> Self {
        Self {
            tx,
            turn_id,
            parent_section_id,
            manifests,
            execution_context,
            tool_runs: BTreeMap::new(),
            tool_metrics: ToolCapabilityDiagnostics::default(),
            last_failed_tool_name: None,
        }
    }

    pub(crate) fn tool_metrics(&self) -> ToolCapabilityDiagnostics {
        self.tool_metrics.clone()
    }

    pub(crate) fn observe_chat_event(&mut self, event: &ChatEvent) {
        let ChatEvent::ToolUse { id, name, input } = event else {
            return;
        };
        let manifest = self.manifests.get(name);

        let tool_run = ToolRun {
            id: id.clone(),
            parent_section_id: self.parent_section_id.clone(),
            tool_name: name.clone(),
            status: ToolRunStatus::Running,
            invocation_preview: preview_tool_invocation(name, input),
            result_preview: None,
            detail: ToolRunDetail {
                title: format!(
                    " Tool: {} ",
                    manifest
                        .map(|manifest| manifest.display_name.as_str())
                        .unwrap_or(name)
                ),
                lines: build_tool_run_detail_lines(
                    name,
                    input,
                    None,
                    manifest,
                    Some(&self.execution_context),
                ),
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
        let manifest = self.manifests.get(tool_name).cloned();
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
                    title: format!(
                        " Tool: {} ",
                        manifest
                            .as_ref()
                            .map(|manifest| manifest.display_name.as_str())
                            .unwrap_or(tool_name)
                    ),
                    lines: build_tool_run_detail_lines(
                        tool_name,
                        tool_input,
                        None,
                        manifest.as_ref(),
                        Some(&self.execution_context),
                    ),
                },
            });

        tool_run.status = status;
        tool_run.result_preview = tool_result_preview(tool_result, 100);
        tool_run.detail = ToolRunDetail {
            title: format!(
                " Tool: {} ",
                manifest
                    .as_ref()
                    .map(|manifest| manifest.display_name.as_str())
                    .unwrap_or(tool_name)
            ),
            lines: build_tool_run_detail_lines(
                tool_name,
                tool_input,
                Some(tool_result),
                manifest.as_ref(),
                Some(&self.execution_context),
            ),
        };

        self.record_tool_capability_metrics(tool_name, manifest.as_ref(), tool_result);

        send_update_tool_run(self.tx, self.turn_id, tool_run.clone());
        send_complete_tool_run(self.tx, self.turn_id, &tool_run.id, status);
        maybe_emit_tool_capability_effects(
            self.tx,
            self.turn_id,
            manifest.as_ref(),
            &self.execution_context,
            tool_result,
        );
        self.tool_runs.insert(tool_run.id.clone(), tool_run);
    }

    fn record_tool_capability_metrics(
        &mut self,
        tool_name: &str,
        manifest: Option<&CoreToolManifestMetadata>,
        tool_result: &CoreToolResult,
    ) {
        increment_metric(&mut self.tool_metrics.tool_invocations, tool_name);

        if let Some(manifest) = manifest {
            increment_metric(
                &mut self.tool_metrics.family_invocations,
                manifest.family.as_str(),
            );
            if manifest.family == omega_core::CoreToolFamily::EscapeHatch {
                self.tool_metrics.bash_fallback_count += 1;
            }
        }

        if tool_name == "ask_user_question" {
            self.tool_metrics.question_block_count += 1;
        }

        if let Some(previous_failed_tool) = self.last_failed_tool_name.take() {
            if previous_failed_tool == tool_name {
                self.tool_metrics.same_intent_retry_count += 1;
            } else {
                self.tool_metrics.tool_switch_after_failure += 1;
            }
        }

        if let Some(error_kind) = tool_result.error_kind {
            increment_metric(
                &mut self.tool_metrics.tool_failure_count_by_kind,
                error_kind.as_str(),
            );
            self.last_failed_tool_name = Some(tool_name.to_string());
        }
    }
}

fn increment_metric(metrics: &mut BTreeMap<String, u32>, key: &str) {
    *metrics.entry(key.to_string()).or_insert(0) += 1;
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
    use std::collections::BTreeMap;
    use std::sync::mpsc;

    use omega_context::{
        ContextBudgetDiagnostics, ContextDiagnostics, ContextDocumentDiagnostics,
        ContextMemoryDiagnostics, ContextStoreDiagnostics,
    };
    use omega_core::{
        CoreMemoryScopeLevel, CoreToolContextProfile, CoreToolExecutionContext, CoreToolFamily,
        CoreToolManifestMetadata,
        CoreToolObservabilityProfile, CoreToolPermissionProfile, CoreToolRemediation,
        CoreToolRemediationKind, CoreToolResult, CoreToolStorageProfile, CoreToolUiProfile,
        CoreToolPromptProfile, CoreToolStability,
    };
    use serde_json::json;

    use super::{
        build_tool_run_detail_lines, maybe_emit_context_observability,
        maybe_emit_tool_capability_effects,
    };
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
                retention_candidates_accepted: 3,
                retention_candidates_dropped: 1,
                dropped_candidates_by_profile: std::collections::BTreeMap::from([
                    ("ephemeral_debug".to_string(), 1),
                ]),
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
                current_query: Some(omega_context::MemoryQueryDiagnostics {
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
                    top_hits: vec![omega_context::MemoryQueryHitItem {
                        profile: "project_facts".to_string(),
                        title: "Project fact: planner wired".to_string(),
                        preview: "Planner now wires archived memory query.".to_string(),
                    }],
                }),
                current_observations: Some(omega_context::ObservationRecallDiagnostics {
                    raw_query: "memory query".to_string(),
                    planned_queries: vec!["memory query".to_string()],
                    rewrite_reason: None,
                    rewrite_queries: Vec::new(),
                    recovery_path: Some("deterministic_bundle".to_string()),
                    query: "memory query".to_string(),
                    result_count: 1,
                    freshness_mix: std::collections::BTreeMap::from([
                        ("fresh".to_string(), 1),
                    ]),
                    top_hits: vec![omega_context::ObservationRecallHitItem {
                        id: "obs-1".to_string(),
                        title: "Open thread: task-memory-query".to_string(),
                        summary: "Query surface still needs planner wiring.".to_string(),
                        freshness: omega_context::ObservationFreshness::Fresh,
                    }],
                }),
            },
            document: ContextDocumentDiagnostics {
                total_files_indexed: 12,
                total_chunks: 48,
                total_embeddings: 48,
                index_staleness_seconds: 4,
                governance_health: Some(omega_context::HealthScore::NeedsAttention),
                health_status: omega_context::DocumentHealthStatus::NeedsAttention,
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
        }
    }

    fn sample_execution_context() -> CoreToolExecutionContext {
        CoreToolExecutionContext {
            workspace_root: "/tmp/project".to_string(),
            workflow_id: "feature".to_string(),
            workflow_role: "child".to_string(),
            step_id: "execute".to_string(),
            step_label: "Execute".to_string(),
            turn_id: 7,
            current_item_id: Some("task-1".to_string()),
            current_item_index: Some(1),
            current_item_total: Some(3),
        }
    }

    fn file_edit_manifest() -> CoreToolManifestMetadata {
        CoreToolManifestMetadata::legacy(
            "apply_patch",
            "Apply a targeted text patch to an existing file.",
            json!({"type": "object"}),
        )
        .with_family(CoreToolFamily::Editing)
        .with_stability(CoreToolStability::Stable)
        .with_prompt_profile(CoreToolPromptProfile {
            summary: "Apply a targeted text patch to an existing file.".to_string(),
            when_to_use: vec!["you know the exact edit window".to_string()],
            when_not_to_use: vec!["the file does not exist yet".to_string()],
            prefer_over: vec!["edit_file".to_string()],
            fallback_to: vec!["edit_file".to_string()],
            examples: vec![],
            anti_patterns: vec![],
        })
        .with_ui(CoreToolUiProfile {
            invocation_preview: true,
            result_preview: true,
            detail_overlay: true,
            action_affordances: vec!["open_diff_preview".to_string()],
        })
        .with_context(CoreToolContextProfile {
            needs_workspace_root: true,
            needs_step_metadata: true,
            needs_selection: false,
            memory_scope: CoreMemoryScopeLevel::Project,
            network_context: false,
        })
        .with_permissions(CoreToolPermissionProfile {
            permission_class: "workspace_write".to_string(),
            default_policy_mode: "step_visible_then_runtime_approval".to_string(),
            requires_approval: true,
            denial_remediation: Some("ask for confirmation before retrying".to_string()),
        })
        .with_storage(CoreToolStorageProfile {
            writes_session_journal: true,
            produces_artifact: true,
            writes_memory: false,
            writes_todo: false,
            replayable: false,
        })
        .with_observability(CoreToolObservabilityProfile {
            invocation_metric: "tool.file_edit.apply_patch.invoke".to_string(),
            success_metric: "tool.file_edit.apply_patch.success".to_string(),
            failure_metric: "tool.file_edit.apply_patch.failure".to_string(),
        })
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
                "deleted_marked": 0,
                "vector_ignored_files": 2
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
                if text.contains("context.index files=12 chunks=48 deleted=0 vector_ignored=2")
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
                "deleted_marked": 0,
                "vector_ignored_files": 1
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

    #[test]
    fn tool_run_detail_lines_include_structured_remediation() {
        let lines = build_tool_run_detail_lines(
            "bash",
            &json!({"command": "exit 1"}),
            Some(
                &CoreToolResult::error("command failed", omega_core::CoreToolErrorKind::Execution)
                    .with_remediation(CoreToolRemediation {
                        kind: CoreToolRemediationKind::RetryOrFallback,
                        suggestion: "Retry with a narrower command or switch tools.".to_string(),
                        alternative_tools: vec!["read_file".to_string()],
                        recoverable: true,
                    }),
            ),
            Some(&file_edit_manifest()),
            Some(&sample_execution_context()),
        );

        assert!(lines.iter().any(|line| line == "family: editing"));
        assert!(lines.iter().any(|line| line.contains("permission: workspace_write")));
        assert!(lines.iter().any(|line| line == "storage: session_journal, artifact"));
        assert!(lines.iter().any(|line| line.contains("execution: child:feature:execute")));
        assert!(lines.iter().any(|line| line == "error_kind: execution"));
        assert!(lines.iter().any(|line| line == "remediation.kind: retry_or_fallback"));
        assert!(lines.iter().any(|line| line.contains("Retry with a narrower command")));
        assert!(lines.iter().any(|line| line == "remediation.alternatives: read_file"));
        assert!(lines.iter().any(|line| line == "remediation.recoverable: true"));
    }

    #[test]
    fn capability_effects_emit_diff_preview_and_approval_surface() {
        let (tx, rx) = mpsc::channel();
        maybe_emit_tool_capability_effects(
            &tx,
            7,
            Some(&file_edit_manifest()),
            &sample_execution_context(),
            &CoreToolResult::success(
                "Applied patch to src/lib.rs\n\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1 +1 @@\n-old\n+new",
            )
            .with_metadata(json!({"diff_available": true})),
        );
        maybe_emit_tool_capability_effects(
            &tx,
            8,
            Some(&file_edit_manifest()),
            &sample_execution_context(),
            &CoreToolResult::error("not allowed", omega_core::CoreToolErrorKind::Policy)
                .with_remediation(CoreToolRemediation {
                    kind: CoreToolRemediationKind::UseAllowedAlternative,
                    suggestion: "Switch to a visible read-only tool.".to_string(),
                    alternative_tools: vec!["read_file".to_string()],
                    recoverable: true,
                }),
        );

        let envelopes = drain_envelopes(&rx);
        assert!(envelopes.iter().any(|envelope| matches!(
            &envelope.message,
            RuntimeMessage::State(StateMessage::OpenDiffPreview { diff })
                if diff.contains("--- a/src/lib.rs")
        )));
        assert!(envelopes.iter().any(|envelope| matches!(
            &envelope.message,
            RuntimeMessage::State(StateMessage::RequestToolApproval { message })
                if message.contains("requires approval")
                    && message.contains("workspace_write")
                    && message.contains("read_file")
        )));
        assert!(envelopes.iter().any(|envelope| matches!(
            &envelope.message,
            RuntimeMessage::State(StateMessage::Activity { text, .. })
                if text.contains("tool.storage tool=apply_patch")
        )));
    }

    #[test]
    fn capability_effects_emit_input_prompt_and_web_overlay() {
        let (tx, rx) = mpsc::channel();
        let interaction_manifest = CoreToolManifestMetadata::legacy(
            "ask_user_question",
            "Request structured user input.",
            json!({"type": "object"}),
        )
        .with_family(CoreToolFamily::Interaction)
        .with_stability(CoreToolStability::Preview);
        let web_manifest = CoreToolManifestMetadata::legacy(
            "web_search",
            "Search the public web.",
            json!({"type": "object"}),
        )
        .with_family(CoreToolFamily::WebResearch)
        .with_stability(CoreToolStability::Preview);

        maybe_emit_tool_capability_effects(
            &tx,
            10,
            Some(&interaction_manifest),
            &sample_execution_context(),
            &CoreToolResult::success("Question requested").with_metadata(json!({
                "question": "Use the fast path?",
                "options": ["yes", "no"],
                "allow_freeform": false,
            })),
        );
        maybe_emit_tool_capability_effects(
            &tx,
            11,
            Some(&web_manifest),
            &sample_execution_context(),
            &CoreToolResult::success("Web results for 'omega':\n1. Example")
                .with_metadata(json!({"query": "omega", "result_count": 1})),
        );

        let envelopes = drain_envelopes(&rx);
        assert!(envelopes.iter().any(|envelope| matches!(
            &envelope.message,
            RuntimeMessage::State(StateMessage::RequestInput { prompt })
                if prompt.contains("question: Use the fast path?") && prompt.contains("options: yes, no")
        )));
        assert!(envelopes.iter().any(|envelope| matches!(
            &envelope.message,
            RuntimeMessage::State(StateMessage::OpenWebResultView { title, content })
                if title.contains("Web Search") && content.contains("1. Example")
        )));
    }

    #[test]
    fn tool_run_tracker_accumulates_capability_metrics() {
        let manifests = BTreeMap::from([
            (
                "bash".to_string(),
                CoreToolManifestMetadata::legacy("bash", "Run shell commands.", json!({}))
                    .with_family(CoreToolFamily::EscapeHatch)
                    .with_stability(CoreToolStability::Stable),
            ),
            (
                "ask_user_question".to_string(),
                CoreToolManifestMetadata::legacy(
                    "ask_user_question",
                    "Request structured user input.",
                    json!({}),
                )
                .with_family(CoreToolFamily::Interaction)
                .with_stability(CoreToolStability::Preview),
            ),
        ]);
        let (tx, _rx) = mpsc::channel();
        let mut tracker = super::ToolRunTracker::new(
            &tx,
            7,
            "section-1".to_string(),
            manifests,
            sample_execution_context(),
        );

        tracker.complete_tool_run(
            "tool-1",
            "bash",
            &json!({"command": "false"}),
            &CoreToolResult::error("command failed", omega_core::CoreToolErrorKind::Execution),
        );
        tracker.complete_tool_run(
            "tool-2",
            "bash",
            &json!({"command": "pwd"}),
            &CoreToolResult::success("/tmp/project"),
        );
        tracker.complete_tool_run(
            "tool-3",
            "ask_user_question",
            &json!({"question": "Proceed?"}),
            &CoreToolResult::success("Question requested").with_metadata(json!({
                "question": "Proceed?",
                "allow_freeform": true,
            })),
        );

        let metrics = tracker.tool_metrics();
        assert_eq!(metrics.tool_invocations.get("bash"), Some(&2));
        assert_eq!(metrics.family_invocations.get("escape_hatch"), Some(&2));
        assert_eq!(metrics.tool_failure_count_by_kind.get("execution"), Some(&1));
        assert_eq!(metrics.bash_fallback_count, 2);
        assert_eq!(metrics.same_intent_retry_count, 1);
        assert_eq!(metrics.question_block_count, 1);
    }
}
