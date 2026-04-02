use omega_session::{ContextSupervisionSnapshot, HealthScore};

use super::{App, Panel};

pub(crate) fn document_placeholder_lines() -> Vec<String> {
    vec!["No document supervision snapshot yet.".to_string()]
}

pub(crate) fn memory_placeholder_lines() -> Vec<String> {
    vec!["No memory supervision snapshot yet.".to_string()]
}

impl App {
    pub fn set_context_supervision(&mut self, snapshot: ContextSupervisionSnapshot) {
        self.context_supervision = Some(snapshot);
        self.rebuild_document_supervision_lines();
        self.rebuild_memory_supervision_lines();
    }

    pub(super) fn clear_context_supervision(&mut self) {
        self.context_supervision = None;
        self.document_lines = document_placeholder_lines();
        self.memory_lines = memory_placeholder_lines();
        self.document_state.select(None);
        self.memory_state.select(None);
        self.document_displayed_count = 0;
        self.memory_displayed_count = 0;
        self.document_pinned = false;
        self.memory_pinned = false;
    }

    pub fn document_panel_title(&self) -> String {
        let mut title = " Document Supervision ".to_string();
        if self.focused_panel == Panel::Document {
            title.push('◆');
            title.push(' ');
        }
        title
    }

    pub fn memory_panel_title(&self) -> String {
        let mut title = " Memory Supervision ".to_string();
        if self.focused_panel == Panel::Memory {
            title.push('◆');
            title.push(' ');
        }
        title
    }

    pub fn open_document_supervision_detail(&mut self) -> bool {
        let Some(snapshot) = self.context_supervision.as_ref() else {
            return false;
        };
        self.open_detail_overlay(
            " Document Supervision ",
            build_document_lines(snapshot),
        );
        true
    }

    pub fn open_memory_supervision_detail(&mut self) -> bool {
        let Some(snapshot) = self.context_supervision.as_ref() else {
            return false;
        };
        self.open_detail_overlay(" Memory Supervision ", build_memory_lines(snapshot));
        true
    }

    fn rebuild_document_supervision_lines(&mut self) {
        self.document_lines = self
            .context_supervision
            .as_ref()
            .map(build_document_lines)
            .unwrap_or_else(document_placeholder_lines);
    }

    fn rebuild_memory_supervision_lines(&mut self) {
        self.memory_lines = self
            .context_supervision
            .as_ref()
            .map(build_memory_lines)
            .unwrap_or_else(memory_placeholder_lines);
    }
}

fn build_document_lines(snapshot: &ContextSupervisionSnapshot) -> Vec<String> {
    let document = &snapshot.document;
    let mut lines = vec![
        format!(
            "status: {} ({})",
            if document.enabled { "enabled" } else { "disabled" },
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
            "freshness: staleness={}s health={}",
            document.totals.index_staleness_seconds,
            health_label(document.totals.governance_health)
        ),
    ];

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
        None => lines.push("hits: no document query has populated supervision yet".to_string()),
    }

    lines
}

fn build_memory_lines(snapshot: &ContextSupervisionSnapshot) -> Vec<String> {
    let memory = &snapshot.memory;
    let mut lines = vec![
        format!(
            "status: {} ({})",
            if memory.enabled { "enabled" } else { "disabled" },
            memory.readiness.as_str()
        ),
        format!(
            "totals: archived_turns={} compactions={}",
            memory.totals.total_turns_archived,
            memory.totals.compactions_triggered
        ),
        format!(
            "selection: summaries={} tokens={}",
            memory.totals.current_summary_count,
            memory.totals.current_summary_tokens
        ),
        format!(
            "archive: count={} size={}",
            memory.totals.turn_archive_count,
            format_bytes(memory.totals.turn_archive_size_bytes)
        ),
    ];

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

    lines
}

fn health_label(health: Option<HealthScore>) -> &'static str {
    match health {
        Some(HealthScore::Good) => "good",
        Some(HealthScore::NeedsAttention) => "needs_attention",
        Some(HealthScore::Critical) => "critical",
        None => "unknown",
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