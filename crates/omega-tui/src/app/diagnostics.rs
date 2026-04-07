use omega_observability::strip_ansi;
use omega_session::{
    CacheDiagnostics, ContextDiagnostics, DocumentHealthStatus, HealthScore, StepContextWrite,
    StepContextWriteKind, StepDiagnostics, StepInputStatus, StepOutputAttemptKind,
    StepOutputRecoveryDecision, StepOutputStatus, ToolCapabilityDiagnostics,
};

use super::{App, DiagnosticsLine, Panel};

impl App {
    pub fn upsert_step_diagnostics(&mut self, diagnostics: StepDiagnostics) {
        let sanitized = sanitize_step_diagnostics(diagnostics);
        if let Some(existing) = self
            .step_diagnostics
            .iter_mut()
            .find(|existing| existing.id == sanitized.id)
        {
            *existing = sanitized;
        } else {
            self.step_diagnostics.push(sanitized);
        }
        self.step_diagnostics.sort_by(|left, right| {
            (
                left.workflow_role.as_str(),
                left.workflow_id.as_str(),
                left.index,
                left.step_id.as_str(),
            )
                .cmp(&(
                    right.workflow_role.as_str(),
                    right.workflow_id.as_str(),
                    right.index,
                    right.step_id.as_str(),
                ))
        });
        self.rebuild_diagnostics_lines();
    }

    pub fn diagnostics_panel_title(&self) -> String {
        let invalid = self
            .step_diagnostics
            .iter()
            .filter(|diagnostics| diagnostics.output.status == StepOutputStatus::Invalid)
            .count();
        let mut title = if invalid > 0 {
            format!(" Contract Diagnostics (!{}) ", invalid)
        } else {
            " Contract Diagnostics ".to_string()
        };
        if self.focused_panel == Panel::Diagnostics {
            title.push('◆');
            title.push(' ');
        }
        title
    }

    pub fn activate_selected_diagnostics_item(&mut self) -> Option<String> {
        let selected = self.diagnostics_state.selected()?;
        let width = (self.diagnostics_rect.width as usize)
            .saturating_sub(2)
            .max(1);
        let line = self
            .wrapped_panel_lines(Panel::Diagnostics, width)
            .get(selected)
            .cloned()?;
        let diagnostic_id = self
            .diagnostics_lines
            .get(line.source_line_index)
            .and_then(|line| line.diagnostic_id.clone())?;
        self.open_step_diagnostics_detail(&diagnostic_id)
    }

    pub(super) fn clear_step_diagnostics(&mut self) {
        self.step_diagnostics.clear();
        self.diagnostics_lines.clear();
        self.diagnostics_state.select(None);
        self.diagnostics_displayed_count = 0;
        self.diagnostics_pinned = false;
    }

    fn rebuild_diagnostics_lines(&mut self) {
        self.diagnostics_lines = self
            .step_diagnostics
            .iter()
            .flat_map(build_diagnostics_lines)
            .collect();
    }

    fn open_step_diagnostics_detail(&mut self, id: &str) -> Option<String> {
        let diagnostics = self
            .step_diagnostics
            .iter()
            .find(|diagnostics| diagnostics.id == id)?;
        let title = format!(
            " Contract Diagnostics {}:{} {} ",
            diagnostics.workflow_role.as_str(),
            diagnostics.workflow_id,
            diagnostics.step_label
        );
        let lines = build_step_diagnostics_detail_lines(diagnostics);
        let label = diagnostics.step_label.clone();
        self.open_detail_overlay(title, lines);
        Some(label)
    }
}

fn sanitize_step_diagnostics(mut diagnostics: StepDiagnostics) -> StepDiagnostics {
    if let Some(progress) = diagnostics.execute_progress.as_mut() {
        progress.current_item_id = progress
            .current_item_id
            .as_ref()
            .map(|text| strip_ansi(text));
        progress.completion_source = progress
            .completion_source
            .as_ref()
            .map(|text| strip_ansi(text));
    }
    diagnostics.input.structured_input_preview = diagnostics
        .input
        .structured_input_preview
        .map(|text| strip_ansi(&text));
    diagnostics.input.todo_state_preview = diagnostics
        .input
        .todo_state_preview
        .map(|text| strip_ansi(&text));
    diagnostics.input.error = diagnostics.input.error.map(|text| strip_ansi(&text));
    diagnostics.output.extracted_json_preview = diagnostics
        .output
        .extracted_json_preview
        .map(|text| strip_ansi(&text));
    diagnostics.output.previous_response_preview = diagnostics
        .output
        .previous_response_preview
        .map(|text| strip_ansi(&text));
    diagnostics.output.validation_error = diagnostics
        .output
        .validation_error
        .map(|text| strip_ansi(&text));
    diagnostics.session_writes = diagnostics
        .session_writes
        .into_iter()
        .map(|write| StepContextWrite {
            path: strip_ansi(&write.path),
            kind: write.kind,
            before_preview: write.before_preview.map(|text| strip_ansi(&text)),
            after_preview: write.after_preview.map(|text| strip_ansi(&text)),
        })
        .collect();
    diagnostics.cache = diagnostics.cache.map(sanitize_cache_diagnostics);
    diagnostics
}

fn sanitize_cache_diagnostics(mut diagnostics: CacheDiagnostics) -> CacheDiagnostics {
    diagnostics.cache_breakpoints = diagnostics
        .cache_breakpoints
        .into_iter()
        .map(|value| strip_ansi(&value))
        .collect();
    diagnostics
}

fn context_budget_percent(cache: &CacheDiagnostics) -> Option<u8> {
    if cache.budget_input_tokens == 0 {
        return None;
    }

    Some(
        ((cache.request_input_tokens.saturating_mul(100)) / cache.budget_input_tokens).min(100)
            as u8,
    )
}

fn context_headroom_tokens(cache: &CacheDiagnostics) -> u32 {
    cache
        .budget_input_tokens
        .saturating_sub(cache.request_input_tokens)
}

fn build_diagnostics_lines(diagnostics: &StepDiagnostics) -> Vec<DiagnosticsLine> {
    let header = format!(
        "{}:{} {}/{} {}",
        diagnostics.workflow_role.as_str(),
        diagnostics.workflow_id,
        diagnostics.index,
        diagnostics.total,
        diagnostics.step_label
    );
    let input = format!(
        "  input {} · summaries={} · structured={}{}",
        diagnostics_input_status_label(diagnostics.input.status),
        diagnostics.input.summary_sources.len(),
        diagnostics.input.resolved_structured_sources.len(),
        if diagnostics.input.todo_state_preview.is_some() {
            " · todo"
        } else {
            ""
        }
    );
    let output = format!(
        "  output {} · attempt={}{} · retries={}/{} · writes={}",
        diagnostics_output_status_label(diagnostics.output.status),
        diagnostics_output_attempt_kind_label(diagnostics.output.attempt_kind),
        diagnostics
            .output
            .recovery_decision
            .map(|decision| format!(
                " · next={}",
                diagnostics_output_recovery_decision_label(decision)
            ))
            .unwrap_or_default(),
        diagnostics.output.retry_count,
        diagnostics.output.max_retries,
        diagnostics.session_writes.len()
    );
    let mut lines = vec![
        DiagnosticsLine {
            text: header,
            diagnostic_id: Some(diagnostics.id.clone()),
        },
        DiagnosticsLine {
            text: input,
            diagnostic_id: Some(diagnostics.id.clone()),
        },
        DiagnosticsLine {
            text: output,
            diagnostic_id: Some(diagnostics.id.clone()),
        },
    ];
    if let Some(tool_capabilities) = diagnostics.tool_capabilities.as_ref() {
        lines.push(DiagnosticsLine {
            text: format!(
                "  tools invocations={} failures={} bash_fallback={} questions={} switch_after_failure={} retry_same_tool={}",
                tool_capabilities.tool_invocations.values().copied().sum::<u32>(),
                tool_capabilities
                    .tool_failure_count_by_kind
                    .values()
                    .copied()
                    .sum::<u32>(),
                tool_capabilities.bash_fallback_count,
                tool_capabilities.question_block_count,
                tool_capabilities.tool_switch_after_failure,
                tool_capabilities.same_intent_retry_count,
            ),
            diagnostic_id: Some(diagnostics.id.clone()),
        });
    }
    if let Some(cache) = diagnostics.cache.as_ref() {
        let mut cache_line = format!(
            "  cache {} · budget={}{} · anchors={}",
            cache.token_count_source.as_str(),
            context_budget_percent(cache)
                .map(|percent| format!("{percent}%"))
                .unwrap_or_else(|| "n/a".to_string()),
            if cache.budget_input_tokens > 0 {
                format!(" · headroom={}", context_headroom_tokens(cache))
            } else {
                String::new()
            },
            cache.cache_breakpoints.len()
        );
        if let Some(hit_ratio) = cache.cache_hit_ratio_percent {
            cache_line.push_str(&format!(" · hit={}%", hit_ratio));
        }
        cache_line = cache_line.replace("%,", "%");
        lines.push(DiagnosticsLine {
            text: cache_line,
            diagnostic_id: Some(diagnostics.id.clone()),
        });
    }
    if let Some(context) = diagnostics.context.as_ref() {
        lines.push(DiagnosticsLine {
            text: format!(
                "  context memory turns={} compact={} summaries={}",
                context.memory.total_turns_archived,
                context.memory.compactions_triggered,
                context.memory.current_summary_count
            ),
            diagnostic_id: Some(diagnostics.id.clone()),
        });
        lines.push(DiagnosticsLine {
            text: format!(
                "  context docs files={} chunks={} health={}",
                context.document.total_files_indexed,
                context.document.total_chunks,
                context_health_label(context)
            ),
            diagnostic_id: Some(diagnostics.id.clone()),
        });
        lines.push(DiagnosticsLine {
            text: format!(
                "  context store todo={} tantivy={} lance={}",
                context.store.todo_items_count,
                context.store.tantivy_index_size_bytes,
                context.store.lance_db_size_bytes
            ),
            diagnostic_id: Some(diagnostics.id.clone()),
        });
    }
    if let Some(progress) = diagnostics.execute_progress.as_ref() {
        let mut execute = format!(
            "  execute todos={}/{} open={} repeats={}",
            progress.todo_completed, progress.todo_total, progress.todo_open, progress.repeat_count
        );
        if let Some(item_id) = progress.current_item_id.as_deref() {
            execute.push_str(&format!(" · current={item_id}"));
        }
        if let Some(source) = progress.completion_source.as_deref() {
            execute.push_str(&format!(" · source={source}"));
        }
        lines.push(DiagnosticsLine {
            text: execute,
            diagnostic_id: Some(diagnostics.id.clone()),
        });
    }
    if let Some(error) = diagnostics
        .input
        .error
        .as_deref()
        .or(diagnostics.output.validation_error.as_deref())
    {
        lines.push(DiagnosticsLine {
            text: format!("  error {}", truncate_preview(error, 96)),
            diagnostic_id: Some(diagnostics.id.clone()),
        });
    }
    lines
}

fn build_step_diagnostics_detail_lines(diagnostics: &StepDiagnostics) -> Vec<String> {
    let mut lines = vec![
        format!(
            "step: {}:{} {} {}/{}",
            diagnostics.workflow_role.as_str(),
            diagnostics.workflow_id,
            diagnostics.step_label,
            diagnostics.index,
            diagnostics.total
        ),
        format!("step_id: {}", diagnostics.step_id),
        format!(
            "input: {}",
            diagnostics_input_status_label(diagnostics.input.status)
        ),
    ];

    if diagnostics.input.summary_sources.is_empty() {
        lines.push("summary_sources: none".to_string());
    } else {
        lines.push(format!(
            "summary_sources: {}",
            diagnostics
                .input
                .summary_sources
                .iter()
                .map(|source| format!(
                    "{}:{} ({})",
                    source.workflow_id, source.step_id, source.title
                ))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    if diagnostics.input.expected_structured_sources.is_empty() {
        lines.push("structured_sources: none".to_string());
    } else {
        lines.push(format!(
            "structured_expected: {}",
            diagnostics.input.expected_structured_sources.join(", ")
        ));
        lines.push(format!(
            "structured_resolved: {}",
            if diagnostics.input.resolved_structured_sources.is_empty() {
                "none".to_string()
            } else {
                diagnostics.input.resolved_structured_sources.join(", ")
            }
        ));
        if !diagnostics.input.missing_structured_sources.is_empty() {
            lines.push(format!(
                "structured_missing: {}",
                diagnostics.input.missing_structured_sources.join(", ")
            ));
        }
    }

    if let Some(preview) = diagnostics.input.structured_input_preview.as_deref() {
        lines.push("structured_input_preview:".to_string());
        lines.extend(preview.lines().map(|line| format!("  {line}")));
    }
    if let Some(preview) = diagnostics.input.todo_state_preview.as_deref() {
        lines.push("todo_state_preview:".to_string());
        lines.extend(preview.lines().map(|line| format!("  {line}")));
    }
    if let Some(error) = diagnostics.input.error.as_deref() {
        lines.push(format!("input_error: {error}"));
    }

    if let Some(cache) = diagnostics.cache.as_ref() {
        lines.push(format!(
            "cache: {} request_input_tokens={}/{}",
            cache.token_count_source.as_str(),
            cache.request_input_tokens,
            cache.budget_input_tokens
        ));
        if let Some(percent) = context_budget_percent(cache) {
            lines.push(format!("context_budget_percent: {percent}"));
        }
        lines.push(format!(
            "context_headroom_tokens: {}",
            context_headroom_tokens(cache)
        ));
        lines.push(format!(
            "cache_breakpoints: {}",
            if cache.cache_breakpoints.is_empty() {
                "none".to_string()
            } else {
                cache.cache_breakpoints.join(", ")
            }
        ));
        if let Some(cache_creation) = cache.cache_creation_input_tokens {
            lines.push(format!("cache_creation_input_tokens: {cache_creation}"));
        }
        if let Some(cache_read) = cache.cache_read_input_tokens {
            lines.push(format!("cache_read_input_tokens: {cache_read}"));
        }
        if let Some(uncached_input) = cache.uncached_input_tokens {
            lines.push(format!("uncached_input_tokens: {uncached_input}"));
        }
        if let Some(hit_ratio) = cache.cache_hit_ratio_percent {
            lines.push(format!("cache_hit_ratio_percent: {hit_ratio}"));
        }
    }

    if let Some(context) = diagnostics.context.as_ref() {
        lines.push(format!(
            "context_budget: request_tokens={} budget_tokens={} headroom_tokens={} usage_percent={} selected_summaries={}/{}",
            context.budget.request_input_tokens,
            context.budget.budget_input_tokens,
            context.budget.headroom_tokens,
            context.budget.usage_percent,
            context.budget.selected_summary_count,
            context.budget.available_summary_count,
        ));
        lines.push(format!(
            "context_memory: turns_archived={} compactions_triggered={} current_summary_tokens={} current_summary_count={} compression_ratio_avg_percent={}",
            context.memory.total_turns_archived,
            context.memory.compactions_triggered,
            context.memory.current_summary_tokens,
            context.memory.current_summary_count,
            context.memory.compression_ratio_avg_percent,
        ));
        lines.push(format!(
            "context_document: files={} chunks={} embeddings={} staleness_seconds={} health={} governance_health={} active_version={} pending_version={} promotion_error={}",
            context.document.total_files_indexed,
            context.document.total_chunks,
            context.document.total_embeddings,
            context.document.index_staleness_seconds,
            document_health_label(context),
            context_health_label(context),
            format_store_version(context.document.active_version.as_ref()),
            format_store_version(context.document.pending_version.as_ref()),
            context
                .document
                .last_promotion_error
                .as_deref()
                .unwrap_or("none"),
        ));
        lines.push(format!(
            "context_store: todo_items={} turn_archive_count={} tantivy_index_size_bytes={} lance_db_size_bytes={}",
            context.store.todo_items_count,
            context.store.turn_archive_count,
            context.store.tantivy_index_size_bytes,
            context.store.lance_db_size_bytes,
        ));
    }

    if let Some(progress) = diagnostics.execute_progress.as_ref() {
        lines.push(format!(
            "execute_progress: todos={}/{} open={} repeat_count={} no_progress_streak={} max_step_repeats={}",
            progress.todo_completed,
            progress.todo_total,
            progress.todo_open,
            progress.repeat_count,
            progress.no_progress_streak,
            progress.max_step_repeats
        ));
        if let Some(item_id) = progress.current_item_id.as_deref() {
            lines.push(format!(
                "current_item: {} ({}/{})",
                item_id,
                progress.current_item_index.unwrap_or_default(),
                progress.current_item_total.unwrap_or_default()
            ));
        }
        if let Some(max_item_repeats) = progress.max_item_repeats {
            lines.push(format!("max_item_repeats: {max_item_repeats}"));
        }
        if let Some(source) = progress.completion_source.as_deref() {
            lines.push(format!("completion_source: {source}"));
        }
    }

    if let Some(tool_capabilities) = diagnostics.tool_capabilities.as_ref() {
        lines.push(tool_capability_summary_line(tool_capabilities));
        lines.push(format!(
            "tool_invocations: {}",
            format_counter_map(&tool_capabilities.tool_invocations)
        ));
        lines.push(format!(
            "tool_families: {}",
            format_counter_map(&tool_capabilities.family_invocations)
        ));
        lines.push(format!(
            "tool_failures: {}",
            format_counter_map(&tool_capabilities.tool_failure_count_by_kind)
        ));
    }

    lines.push(format!(
        "output: {}",
        diagnostics_output_status_label(diagnostics.output.status)
    ));
    let attempt = format!(
        " · attempt={}",
        diagnostics_output_attempt_kind_label(diagnostics.output.attempt_kind)
    );
    let attempts = if diagnostics.output.max_retries > 0 {
        format!(
            " · attempts={} · retries={}/{}",
            diagnostics.output.attempts,
            diagnostics.output.retry_count,
            diagnostics.output.max_retries
        )
    } else if diagnostics.output.attempts > 0 {
        format!(" · attempts={}", diagnostics.output.attempts)
    } else {
        String::new()
    };

    lines.push(format!(
        "output_contract: {}{}{}{}",
        diagnostics_output_contract_label(
            diagnostics.output.status,
            diagnostics.output.format.as_deref()
        ),
        attempt,
        diagnostics
            .output
            .schema_path
            .as_deref()
            .map(|path| format!(" · schema={path}"))
            .unwrap_or_default(),
        attempts
    ));
    if let Some(recovery_decision) = diagnostics.output.recovery_decision {
        lines.push(format!(
            "recovery_decision: {}",
            diagnostics_output_recovery_decision_label(recovery_decision)
        ));
    }
    if let Some(preview) = diagnostics.output.previous_response_preview.as_deref() {
        lines.push("previous_response_preview:".to_string());
        lines.extend(preview.lines().map(|line| format!("  {line}")));
    }
    if let Some(preview) = diagnostics.output.extracted_json_preview.as_deref() {
        lines.push("extracted_json_preview:".to_string());
        lines.extend(preview.lines().map(|line| format!("  {line}")));
    }
    if let Some(error) = diagnostics.output.validation_error.as_deref() {
        lines.push(format!("validation_error: {error}"));
    }

    if diagnostics.session_writes.is_empty() {
        lines.push("session_writes: none".to_string());
    } else {
        lines.push("session_writes:".to_string());
        for write in &diagnostics.session_writes {
            lines.push(format!(
                "  {} ({})",
                write.path,
                diagnostics_write_kind_label(write.kind)
            ));
            if let Some(preview) = write.before_preview.as_deref() {
                lines.push(format!("    before {}", truncate_preview(preview, 140)));
            }
            if let Some(preview) = write.after_preview.as_deref() {
                lines.push(format!("    after  {}", truncate_preview(preview, 140)));
            }
        }
    }

    lines
}

fn diagnostics_input_status_label(status: StepInputStatus) -> &'static str {
    match status {
        StepInputStatus::None => "none",
        StepInputStatus::Ready => "ready",
        StepInputStatus::OptionalEmpty => "optional-empty",
        StepInputStatus::MissingRequired => "missing-required",
    }
}

fn diagnostics_output_status_label(status: StepOutputStatus) -> &'static str {
    match status {
        StepOutputStatus::None => "none",
        StepOutputStatus::Pending => "pending",
        StepOutputStatus::Valid => "valid",
        StepOutputStatus::Invalid => "invalid",
        StepOutputStatus::Skipped => "skipped",
    }
}

fn diagnostics_output_attempt_kind_label(kind: StepOutputAttemptKind) -> &'static str {
    match kind {
        StepOutputAttemptKind::Primary => "primary",
        StepOutputAttemptKind::Repair => "repair",
        StepOutputAttemptKind::Regenerate => "regenerate",
    }
}

fn tool_capability_summary_line(tool_capabilities: &ToolCapabilityDiagnostics) -> String {
    format!(
        "tool_capabilities: bash_fallback_count={} question_block_count={} tool_switch_after_failure={} same_intent_retry_count={}",
        tool_capabilities.bash_fallback_count,
        tool_capabilities.question_block_count,
        tool_capabilities.tool_switch_after_failure,
        tool_capabilities.same_intent_retry_count,
    )
}

fn format_counter_map(counters: &std::collections::BTreeMap<String, u32>) -> String {
    if counters.is_empty() {
        return "none".to_string();
    }

    counters
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn diagnostics_output_recovery_decision_label(
    decision: StepOutputRecoveryDecision,
) -> &'static str {
    match decision {
        StepOutputRecoveryDecision::Repair => "repair",
        StepOutputRecoveryDecision::Regenerate => "regenerate",
        StepOutputRecoveryDecision::FallbackTextRouting => "fallback-text-routing",
        StepOutputRecoveryDecision::Abort => "abort",
    }
}

fn context_health_label(context: &ContextDiagnostics) -> &'static str {
    match context.document.governance_health {
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

fn format_store_version(version: Option<&omega_session::DocumentStoreVersion>) -> String {
    version
        .map(|version| {
            format!(
                "{}@{}",
                version.version_id, version.manifest_revision
            )
        })
        .unwrap_or_else(|| "none".to_string())
}

fn diagnostics_output_contract_label(status: StepOutputStatus, format: Option<&str>) -> String {
    match format {
        Some(format) => format!(
            "format={format} · status={}",
            diagnostics_output_status_label(status)
        ),
        None => format!("status={}", diagnostics_output_status_label(status)),
    }
}

fn diagnostics_write_kind_label(kind: StepContextWriteKind) -> &'static str {
    match kind {
        StepContextWriteKind::Added => "added",
        StepContextWriteKind::Updated => "updated",
        StepContextWriteKind::Cleared => "cleared",
    }
}

fn truncate_preview(text: &str, max_chars: usize) -> String {
    let mut chars = text.chars();
    let preview: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{preview}...")
    } else {
        preview
    }
}
