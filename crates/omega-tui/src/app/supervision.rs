use omega_session::{ContextSupervisionSnapshot, DocumentHealthStatus, HealthScore};

use super::{App, Panel};

pub(crate) fn knowledge_placeholder_lines() -> Vec<String> {
    vec!["No knowledge supervision snapshot yet.".to_string()]
}

pub(crate) fn memory_placeholder_lines() -> Vec<String> {
    vec!["No memory supervision snapshot yet.".to_string()]
}

impl App {
    pub fn set_context_supervision(&mut self, snapshot: ContextSupervisionSnapshot) {
        self.context_supervision = Some(snapshot);
        self.rebuild_knowledge_supervision_lines();
        self.rebuild_memory_supervision_lines();
    }

    pub(super) fn clear_context_supervision(&mut self) {
        self.context_supervision = None;
        self.document_lines = knowledge_placeholder_lines();
        self.memory_lines = memory_placeholder_lines();
        self.document_state.select(None);
        self.memory_state.select(None);
        self.document_displayed_count = 0;
        self.memory_displayed_count = 0;
        self.document_pinned = false;
        self.memory_pinned = false;
    }

    pub fn knowledge_panel_title(&self) -> String {
        let mut title = "Knowledge".to_string();
        if self.focused_panel == Panel::Document {
            title.push('◆');
            title.push(' ');
        }
        title
    }

    pub fn memory_panel_title(&self) -> String {
        let mut title = "Memory Supervision".to_string();
        if self.focused_panel == Panel::Memory {
            title.push('◆');
            title.push(' ');
        }
        title
    }

    pub fn open_knowledge_supervision_detail(&mut self) -> bool {
        let Some(snapshot) = self.context_supervision.as_ref() else {
            return false;
        };
        self.open_detail_overlay(" Knowledge ", build_knowledge_detail_lines(snapshot));
        true
    }

    pub fn open_memory_supervision_detail(&mut self) -> bool {
        let Some(snapshot) = self.context_supervision.as_ref() else {
            return false;
        };
        self.open_detail_overlay(" Memory Supervision ", build_memory_lines(snapshot));
        true
    }

    fn rebuild_knowledge_supervision_lines(&mut self) {
        self.document_lines = self
            .context_supervision
            .as_ref()
            .map(build_knowledge_lines)
            .unwrap_or_else(knowledge_placeholder_lines);
    }

    fn rebuild_memory_supervision_lines(&mut self) {
        self.memory_lines = self
            .context_supervision
            .as_ref()
            .map(build_memory_lines)
            .unwrap_or_else(memory_placeholder_lines);
    }
}

fn build_knowledge_lines(snapshot: &ContextSupervisionSnapshot) -> Vec<String> {
    let document = &snapshot.document;
    let memory = &snapshot.memory;

    let doc_hits = document
        .current_hits
        .as_ref()
        .map(|hits| hits.result_count as u64)
        .unwrap_or(0);
    let mem_hits = memory
        .current_query
        .as_ref()
        .map(|query| query.result_count as u64)
        .unwrap_or(0);
    let obs_hits = memory
        .current_observations
        .as_ref()
        .map(|observations| observations.result_count as u64)
        .unwrap_or(0);
    let doc_queries: u64 = document
        .operator_usage
        .iter()
        .map(|usage| usage.count as u64)
        .sum();
    let mem_queries = memory.totals.memory_query_count as u64;
    let max_hits = doc_hits.max(mem_hits).max(obs_hits).max(1);

    let mut lines = vec![
        format!(
            "status: doc {} ({}) · mem {} ({})",
            if document.enabled { "on" } else { "off" },
            document.readiness.as_str(),
            if memory.enabled { "on" } else { "off" },
            memory.readiness.as_str()
        ),
        format!(
            "stores: files={} chunks={} turns={} summaries={}",
            document.totals.total_files_indexed,
            document.totals.total_chunks,
            memory.totals.total_turns_archived,
            memory.totals.current_summary_count
        ),
        format!(
            "health: doc={} govern={} obs={}",
            document.health_status.as_str(),
            governance_label(document.health_status, document.totals.governance_health),
            memory.totals.observation_count
        ),
        "hits:".to_string(),
        format!("  doc {} {}", compact_bar(doc_hits, max_hits, 6), doc_hits),
        format!("  mem {} {}", compact_bar(mem_hits, max_hits, 6), mem_hits),
        format!("  obs {} {}", compact_bar(obs_hits, max_hits, 6), obs_hits),
        format!(
            "queries: doc={} mem={} corrections={}",
            doc_queries, mem_queries, memory.totals.observation_correction_activity
        ),
    ];

    if let Some(hits) = document.current_hits.as_ref() {
        if let Some(hit) = hits.top_hits.first() {
            lines.push(format!("doc lead: {}", hit.path));
        }
    }
    if let Some(query) = memory.current_query.as_ref() {
        if let Some(hit) = query.top_hits.first() {
            lines.push(format!("mem lead: [{}] {}", hit.profile, hit.title));
        }
    }
    if let Some(observations) = memory.current_observations.as_ref() {
        if let Some(hit) = observations.top_hits.first() {
            lines.push(format!(
                "obs lead: [{}] {}",
                hit.freshness.as_str(),
                hit.title
            ));
        }
    }

    lines
}

fn build_knowledge_detail_lines(snapshot: &ContextSupervisionSnapshot) -> Vec<String> {
    let mut lines = vec!["Document lane".to_string()];
    lines.extend(build_document_lines(snapshot));
    lines.push(String::new());
    lines.push("Memory lane".to_string());
    lines.extend(build_memory_lines(snapshot));
    lines
}

fn build_document_lines(snapshot: &ContextSupervisionSnapshot) -> Vec<String> {
    let document = &snapshot.document;
    let governance_status =
        governance_label(document.health_status, document.totals.governance_health);
    let has_document_query_activity = document.operator_usage.iter().any(|usage| {
        matches!(
            usage.operator.as_str(),
            "context_recall_planner" | "search_codebase"
        )
    });
    let mut lines = vec![
        format!(
            "status: {} ({})",
            if document.enabled {
                "enabled"
            } else {
                "disabled"
            },
            document.readiness.as_str()
        ),
        format!(
            "totals: files={} chunks={} embeddings={}",
            document.totals.total_files_indexed,
            document.totals.total_chunks,
            document.totals.total_embeddings
        ),
        format!(
            "store: lance={} tantivy={}",
            format_bytes(document.totals.lance_db_size_bytes),
            format_bytes(document.totals.tantivy_index_size_bytes)
        ),
        format!(
            "freshness: staleness={}s health={} governance={}",
            document.totals.index_staleness_seconds,
            document.health_status.as_str(),
            governance_status
        ),
    ];

    if let Some(version) = document.active_version.as_ref() {
        lines.push(format!(
            "active: {} rev={} tantivy={} lance={} path={}",
            version.version_id,
            version.manifest_revision,
            version.tantivy_revision,
            version
                .lance_revision
                .map(|revision| revision.to_string())
                .unwrap_or_else(|| "none".to_string()),
            version.storage_path,
        ));
    } else {
        lines.push("active: no promoted store version yet".to_string());
        if document.totals.lance_db_size_bytes > 0 || document.totals.tantivy_index_size_bytes > 0 {
            lines.push(
                "store note: disk bytes exist, but no promoted store version is active yet"
                    .to_string(),
            );
        }
    }

    if let Some(version) = document.pending_version.as_ref() {
        lines.push(format!(
            "pending: {} rev={} path={}",
            version.version_id, version.manifest_revision, version.storage_path
        ));
    }
    if let Some(error) = document.last_promotion_error.as_ref() {
        lines.push(format!("promotion: {error}"));
    }
    if let Some(last_health_check) = document.totals.last_health_check {
        lines.push(format!("health check: last_run={}s", last_health_check));
    } else if matches!(document.health_status, DocumentHealthStatus::NeverChecked) {
        lines.push("health check: never run".to_string());
    }

    if !document.operator_usage.is_empty() {
        lines.push("usage:".to_string());
        for usage in document.operator_usage.iter().take(3) {
            lines.push(format!(
                "- {} via {} count={} last={}",
                usage.operator,
                usage.source,
                usage.count,
                usage
                    .last_used_at
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "unknown".to_string())
            ));
        }
    }

    if !document.recent_activity.is_empty() {
        lines.push("activity:".to_string());
        for activity in document.recent_activity.iter().take(3) {
            lines.push(format!("- {} @{}", activity.label, activity.at));
            lines.push(format!("  {}", activity.detail));
        }
    }

    match document.current_hits.as_ref() {
        Some(hits) => {
            lines.push(String::new());
            lines.push(format!(
                "query: {}",
                if hits.query.is_empty() {
                    "(empty query)"
                } else {
                    hits.query.as_str()
                }
            ));
            lines.push(format!(
                "results: {} via {}{}",
                hits.result_count,
                hits.mode,
                hits.degraded_from
                    .as_ref()
                    .map(|mode| format!(" (degraded from {mode})"))
                    .unwrap_or_default()
            ));
            if hits.top_hits.is_empty() {
                lines.push("hits: no matches returned".to_string());
            } else {
                lines.push("hits:".to_string());
                for hit in &hits.top_hits {
                    lines.push(format!("- {}", hit.path));
                    lines.push(format!("  {}", hit.preview));
                }
            }
        }
        None => {
            if has_document_query_activity {
                if document.active_version.is_none() {
                    lines.push(
                        "hits: recent document recall attempts ran before any promoted store version was available"
                            .to_string(),
                    );
                } else {
                    lines.push(
                        "hits: recent document queries returned no captured result snapshot"
                            .to_string(),
                    );
                }
            } else {
                lines.push("hits: no document query has populated supervision yet".to_string());
            }
        }
    }

    lines
}

fn build_memory_lines(snapshot: &ContextSupervisionSnapshot) -> Vec<String> {
    let memory = &snapshot.memory;
    let mut lines = vec![
        format!(
            "status: {} ({})",
            if memory.enabled {
                "enabled"
            } else {
                "disabled"
            },
            memory.readiness.as_str()
        ),
        format!(
            "totals: archived_turns={} compactions={}",
            memory.totals.total_turns_archived, memory.totals.compactions_triggered
        ),
        format!(
            "selection: summaries={} tokens={}",
            memory.totals.current_summary_count, memory.totals.current_summary_tokens
        ),
        format!(
            "archive: count={} size={}",
            memory.totals.turn_archive_count,
            format_bytes(memory.totals.turn_archive_size_bytes)
        ),
        format!(
            "retention: accepted={} dropped={}",
            memory.totals.retention_candidates_accepted, memory.totals.retention_candidates_dropped
        ),
        format!(
            "queries: count={} mix={}",
            memory.totals.memory_query_count,
            format_kv_counts(&memory.totals.memory_query_hit_mix)
        ),
        format!(
            "observations: total={} fresh={} stale={} superseded={} corrected={} corrections={}",
            memory.totals.observation_count,
            memory.totals.observation_fresh_count,
            memory.totals.observation_stale_count,
            memory.totals.observation_superseded_count,
            memory.totals.observation_corrected_count,
            memory.totals.observation_correction_activity
        ),
    ];

    if !memory.totals.dropped_candidates_by_profile.is_empty() {
        lines.push(format!(
            "dropped by profile: {}",
            format_kv_counts(&memory.totals.dropped_candidates_by_profile)
        ));
    }

    match memory.current_hits.as_ref() {
        Some(hits) => {
            lines.push(String::new());
            lines.push(format!(
                "selected summaries: {} ({})",
                hits.selected_count, hits.total_tokens
            ));
            for hit in &hits.top_hits {
                lines.push(format!(
                    "- {} [{}:{}]",
                    hit.title, hit.workflow_id, hit.step_id
                ));
                lines.push(format!("  {}", hit.preview));
            }
        }
        None => lines.push("selected summaries: none for the current step".to_string()),
    }

    match memory.current_query.as_ref() {
        Some(query) => {
            lines.push(String::new());
            lines.push(format!(
                "archived memory query: {}",
                if query.query.is_empty() {
                    "(empty query)"
                } else {
                    query.query.as_str()
                }
            ));
            if !query.planned_queries.is_empty() {
                lines.push(format!(
                    "planned memory queries: {}",
                    query.planned_queries.join(" | ")
                ));
            }
            if let Some(reason) = query.rewrite_reason.as_deref() {
                lines.push(format!("memory rewrite reason: {reason}"));
            }
            if !query.rewrite_queries.is_empty() {
                lines.push(format!(
                    "memory rewrite queries: {}",
                    query.rewrite_queries.join(" | ")
                ));
            }
            lines.push(format!(
                "archived memory hits: {} ({})",
                query.result_count,
                format_kv_counts(&query.hit_mix)
            ));
            for hit in &query.top_hits {
                lines.push(format!("- [{}] {}", hit.profile, hit.title));
                lines.push(format!("  {}", hit.preview));
            }
        }
        None => {
            lines.push("memory query: no archived recall has populated supervision yet".to_string())
        }
    }

    match memory.current_observations.as_ref() {
        Some(observations) => {
            lines.push(String::new());
            lines.push(format!(
                "observations query: {}",
                if observations.query.is_empty() {
                    "(empty query)"
                } else {
                    observations.query.as_str()
                }
            ));
            if !observations.planned_queries.is_empty() {
                lines.push(format!(
                    "planned observation queries: {}",
                    observations.planned_queries.join(" | ")
                ));
            }
            if let Some(reason) = observations.rewrite_reason.as_deref() {
                lines.push(format!("observation rewrite reason: {reason}"));
            }
            if !observations.rewrite_queries.is_empty() {
                lines.push(format!(
                    "observation rewrite queries: {}",
                    observations.rewrite_queries.join(" | ")
                ));
            }
            lines.push(format!(
                "observation hits: {} ({})",
                observations.result_count,
                format_kv_counts(&observations.freshness_mix)
            ));
            for hit in &observations.top_hits {
                lines.push(format!("- [{}] {}", hit.freshness.as_str(), hit.title));
                lines.push(format!("  {}", hit.summary));
            }
        }
        None => lines.push(
            "observations: no project observation recall has populated supervision yet".to_string(),
        ),
    }

    lines
}

fn compact_bar(value: u64, max: u64, width: usize) -> String {
    let filled = if max == 0 {
        0
    } else {
        ((value.saturating_mul(width as u64) + max - 1) / max) as usize
    }
    .min(width);

    format!(
        "{}{}",
        "█".repeat(filled),
        "░".repeat(width.saturating_sub(filled))
    )
}

fn format_kv_counts(counts: &std::collections::BTreeMap<String, impl std::fmt::Display>) -> String {
    if counts.is_empty() {
        return "none".to_string();
    }
    counts
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn health_label(health: Option<HealthScore>) -> &'static str {
    match health {
        Some(HealthScore::Good) => "good",
        Some(HealthScore::NeedsAttention) => "needs_attention",
        Some(HealthScore::Critical) => "critical",
        None => "unknown",
    }
}

fn governance_label(
    health_status: DocumentHealthStatus,
    governance_health: Option<HealthScore>,
) -> &'static str {
    if governance_health.is_none() && matches!(health_status, DocumentHealthStatus::NeverChecked) {
        "pending_health_check"
    } else {
        health_label(governance_health)
    }
}

fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.1} GiB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MiB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KiB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

#[cfg(test)]
mod tests {
    use omega_session::{
        ContextSupervisionSnapshot, DocumentHealthStatus, DocumentOperatorUsage,
        DocumentSupervisionSnapshot, DocumentSupervisionTotals, MemorySupervisionSnapshot,
        SupervisionReadiness,
    };

    use super::{build_document_lines, build_knowledge_lines};

    #[test]
    fn document_lines_explain_uninitialized_store_and_pending_health_check() {
        let lines = build_document_lines(&ContextSupervisionSnapshot {
            document: DocumentSupervisionSnapshot {
                enabled: true,
                readiness: SupervisionReadiness::Uninitialized,
                health_status: DocumentHealthStatus::NeverChecked,
                totals: DocumentSupervisionTotals {
                    lance_db_size_bytes: 5 * 1024 * 1024,
                    tantivy_index_size_bytes: 640 * 1024,
                    ..DocumentSupervisionTotals::default()
                },
                ..DocumentSupervisionSnapshot::default()
            },
            memory: MemorySupervisionSnapshot::default(),
        });

        assert!(lines
            .iter()
            .any(|line| line.contains("status: enabled (uninitialized)")));
        assert!(lines
            .iter()
            .any(|line| line.contains("governance=pending_health_check")));
        assert!(lines
            .iter()
            .any(|line| line.contains("store note: disk bytes exist")));
    }

    #[test]
    fn document_lines_distinguish_query_attempts_without_hits() {
        let lines = build_document_lines(&ContextSupervisionSnapshot {
            document: DocumentSupervisionSnapshot {
                enabled: true,
                readiness: SupervisionReadiness::Uninitialized,
                health_status: DocumentHealthStatus::NeverChecked,
                operator_usage: vec![DocumentOperatorUsage {
                    operator: "context_recall_planner".to_string(),
                    source: "planner".to_string(),
                    count: 3,
                    last_used_at: Some(42),
                }],
                ..DocumentSupervisionSnapshot::default()
            },
            memory: MemorySupervisionSnapshot::default(),
        });

        assert!(lines.iter().any(|line| {
            line.contains("recent document recall attempts ran before any promoted store version was available")
        }));
    }

    #[test]
    fn knowledge_lines_show_compact_hit_dashboard() {
        let lines = build_knowledge_lines(&ContextSupervisionSnapshot {
            document: DocumentSupervisionSnapshot {
                enabled: true,
                current_hits: Some(omega_session::DocumentHitSummary {
                    query: "policy".to_string(),
                    raw_query: "policy".to_string(),
                    planned_queries: vec![],
                    rewrite_reason: None,
                    rewrite_queries: vec![],
                    recovery_path: None,
                    result_count: 3,
                    mode: "semantic".to_string(),
                    degraded_from: None,
                    top_hits: vec![],
                }),
                ..DocumentSupervisionSnapshot::default()
            },
            memory: MemorySupervisionSnapshot {
                current_query: Some(omega_session::MemoryQueryDiagnostics {
                    raw_query: "policy".to_string(),
                    planned_queries: vec![],
                    rewrite_reason: None,
                    rewrite_queries: vec![],
                    recovery_path: None,
                    query: "policy".to_string(),
                    result_count: 2,
                    hit_mix: std::collections::BTreeMap::new(),
                    top_hits: vec![],
                }),
                current_observations: Some(omega_session::ObservationRecallDiagnostics {
                    raw_query: "policy".to_string(),
                    planned_queries: vec![],
                    rewrite_reason: None,
                    rewrite_queries: vec![],
                    recovery_path: None,
                    query: "policy".to_string(),
                    result_count: 1,
                    freshness_mix: std::collections::BTreeMap::new(),
                    top_hits: vec![],
                }),
                ..MemorySupervisionSnapshot::default()
            },
        });

        assert!(lines.iter().any(|line| line == "hits:"));
        assert!(lines.iter().any(|line| line.contains("doc ██████ 3")));
        assert!(lines.iter().any(|line| line.contains("mem ████░░ 2")));
        assert!(lines.iter().any(|line| line.contains("obs ██░░░░ 1")));
    }
}
