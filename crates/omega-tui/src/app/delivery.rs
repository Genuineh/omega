use std::collections::{BTreeMap, BTreeSet};

use omega_session::{ResponseSection, ResponseSectionKind, ResponseSectionMetadata, ResponseSectionState, SectionOrigin, ToolRunStatus, WorkflowRunRole};

use super::{App, MsgKind, Panel};

pub(crate) fn delivery_placeholder_lines() -> Vec<String> {
    vec!["Tracking current turn delivery. No delivery signals yet.".to_string()]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeliveryStatus {
    Running,
    Complete,
    Failed,
    Interrupted,
}

impl DeliveryStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Complete => "complete",
            Self::Failed => "failed",
            Self::Interrupted => "interrupted",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeliveryChangedFile {
    pub path: String,
    pub kind: DeliveryFileChangeKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DeliveryFileChangeKind {
    Create,
    Update,
    Delete,
}

impl DeliveryFileChangeKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Update => "update",
            Self::Delete => "delete",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeliverySummary {
    pub turn_id: u64,
    pub status: DeliveryStatus,
    pub primary_model: Option<String>,
    pub llm_request_count: u32,
    pub input_tokens: u32,
    pub cache_creation_input_tokens: u32,
    pub cache_read_input_tokens: u32,
    pub token_stats_partial: bool,
    pub tool_call_count: usize,
    pub unique_tool_count: usize,
    pub failed_tool_count: usize,
    pub tool_counts: BTreeMap<String, u32>,
    pub recognized_skill_ids: Vec<String>,
    pub loaded_skill_ids: Vec<String>,
    pub ignored_skill_ids: Vec<String>,
    pub document_search_count: usize,
    pub memory_search_count: usize,
    pub observation_search_count: usize,
    pub document_queries: Vec<String>,
    pub memory_queries: Vec<String>,
    pub changed_files: Vec<DeliveryChangedFile>,
}

impl DeliverySummary {
    fn total_tokens(&self) -> u32 {
        self.input_tokens
            .saturating_add(self.cache_creation_input_tokens)
            .saturating_add(self.cache_read_input_tokens)
    }

    pub(super) fn summary_line(&self) -> String {
        format!(
            "{} · {} · {} tok{} · {} llm · {} tools · {} skills · {} files",
            self.status.as_str(),
            self.primary_model
                .as_deref()
                .unwrap_or("model unknown"),
            self.total_tokens(),
            if self.token_stats_partial { " partial" } else { "" },
            self.llm_request_count,
            self.tool_call_count,
            self.loaded_skill_ids.len(),
            self.changed_files.len(),
        )
    }

    fn compact_badge(&self) -> String {
        format!(
            "{} tok · {} llm · {} tools · {} files",
            self.total_tokens(),
            self.llm_request_count,
            self.tool_call_count,
            self.changed_files.len(),
        )
    }
}

impl App {
    pub fn remember_delivery_model_name(&mut self, model_name: &str) {
        if model_name.trim().is_empty() {
            return;
        }
        self.delivery_model_name = Some(model_name.to_string());
    }

    pub fn delivery_panel_title(&self) -> String {
        let mut title = "Delivery".to_string();
        if self.focused_panel == Panel::Delivery {
            title.push('◆');
            title.push(' ');
        }
        title
    }

    pub fn open_latest_delivery_detail(&mut self) -> bool {
        let turn_id = self.delivery_panel_turn_id().unwrap_or(self.active_turn_id);
        self.open_delivery_detail_for_turn(turn_id)
    }

    pub fn open_delivery_detail_for_turn(&mut self, turn_id: u64) -> bool {
        let Some(summary) = self.delivery_summary_for_turn(turn_id) else {
            return false;
        };
        self.open_detail_overlay(" Task Delivery ", build_delivery_detail_lines(&summary));
        true
    }

    pub fn delivery_badge_text(&self) -> Option<String> {
        self.delivery_panel_summary().map(|summary| summary.compact_badge())
    }

    pub(super) fn refresh_delivery_panel(&mut self) {
        self.delivery_lines = self
            .delivery_panel_summary()
            .map(|summary| build_delivery_panel_lines(&summary))
            .unwrap_or_else(delivery_placeholder_lines);
    }

    pub(super) fn finalize_current_delivery_summary(&mut self, interrupted: bool) {
        if self.active_turn_id == 0 {
            return;
        }

        let status = if interrupted {
            DeliveryStatus::Interrupted
        } else if self.current_turn_failed() {
            DeliveryStatus::Failed
        } else {
            DeliveryStatus::Complete
        };

        let summary = self.build_delivery_summary(self.active_turn_id, status);
        self.delivery_summaries
            .insert(self.active_turn_id, summary.clone());
        self.latest_delivery_turn_id = Some(self.active_turn_id);
        self.refresh_delivery_panel();
        self.upsert_delivery_response_section(&summary);
    }

    fn delivery_panel_turn_id(&self) -> Option<u64> {
        if self.is_running && self.active_turn_id > 0 {
            Some(self.active_turn_id)
        } else {
            self.latest_delivery_turn_id
        }
    }

    pub(super) fn delivery_panel_summary(&self) -> Option<DeliverySummary> {
        let turn_id = self.delivery_panel_turn_id()?;
        self.delivery_summary_for_turn(turn_id)
    }

    pub(super) fn delivery_summary_for_turn(&self, turn_id: u64) -> Option<DeliverySummary> {
        if self.is_running && self.active_turn_id == turn_id {
            Some(self.build_delivery_summary(turn_id, DeliveryStatus::Running))
        } else {
            self.delivery_summaries.get(&turn_id).cloned()
        }
    }

    fn build_delivery_summary(&self, turn_id: u64, status: DeliveryStatus) -> DeliverySummary {
        let prefix = turn_prefix(turn_id);
        let current_tools = self
            .tool_runs
            .iter()
            .filter(|tool_run| tool_run.parent_section_id.starts_with(&prefix))
            .collect::<Vec<_>>();

        let mut tool_counts = BTreeMap::new();
        let mut changed_files = BTreeMap::<String, DeliveryFileChangeKind>::new();
        let mut failed_tool_count = 0usize;
        for tool_run in &current_tools {
            *tool_counts.entry(tool_run.tool_name.clone()).or_insert(0) += 1;
            if tool_run.status == ToolRunStatus::Failed {
                failed_tool_count += 1;
            }
            if let Some(kind) = tool_change_kind(&tool_run.tool_name) {
                let path = tool_run.invocation_preview.trim();
                if !path.is_empty() {
                    changed_files
                        .entry(path.to_string())
                        .and_modify(|existing| {
                            if kind == DeliveryFileChangeKind::Create {
                                *existing = kind;
                            }
                        })
                        .or_insert(kind);
                }
            }
        }

        let mut recognized = BTreeSet::new();
        let mut loaded = BTreeSet::new();
        let mut ignored = BTreeSet::new();
        for (section_id, summary) in &self.skill_load_summaries {
            if !section_id.starts_with(&prefix) {
                continue;
            }
            recognized.extend(summary.recognized_skill_ids.iter().cloned());
            loaded.extend(summary.loaded_skill_ids.iter().cloned());
            ignored.extend(summary.ignored_skill_ids.iter().cloned());
        }

        let mut document_queries = Vec::new();
        let mut memory_queries = Vec::new();
        let mut document_search_count = 0usize;
        let mut memory_search_count = 0usize;
        let mut observation_search_count = 0usize;
        for (section_id, summary) in &self.step_knowledge_summaries {
            if !section_id.starts_with(&prefix) {
                continue;
            }
            if let Some(document) = summary.document.as_ref() {
                document_search_count += 1;
                if !document.query.trim().is_empty() {
                    document_queries.push(document.query.clone());
                }
            }
            if let Some(memory) = summary.memory.as_ref() {
                memory_search_count += 1;
                if let Some(query) = memory.memory_query.as_ref() {
                    if !query.trim().is_empty() {
                        memory_queries.push(query.clone());
                    }
                }
                if memory.observation_query.is_some() {
                    observation_search_count += 1;
                }
            }
        }

        let llm_request_count = self
            .step_diagnostics
            .iter()
            .map(|diagnostics| diagnostics.output.attempts.max(1))
            .sum::<u32>();
        let input_tokens = self
            .step_diagnostics
            .iter()
            .filter_map(|diagnostics| diagnostics.cache.as_ref())
            .map(|cache| cache.request_input_tokens)
            .sum::<u32>();
        let cache_creation_input_tokens = self
            .step_diagnostics
            .iter()
            .filter_map(|diagnostics| diagnostics.cache.as_ref())
            .filter_map(|cache| cache.cache_creation_input_tokens)
            .sum::<u32>();
        let cache_read_input_tokens = self
            .step_diagnostics
            .iter()
            .filter_map(|diagnostics| diagnostics.cache.as_ref())
            .filter_map(|cache| cache.cache_read_input_tokens)
            .sum::<u32>();
        let token_stats_partial = self.step_diagnostics.iter().any(|diagnostics| diagnostics.cache.is_none());

        DeliverySummary {
            turn_id,
            status,
            primary_model: self.delivery_model_name.clone(),
            llm_request_count,
            input_tokens,
            cache_creation_input_tokens,
            cache_read_input_tokens,
            token_stats_partial,
            tool_call_count: current_tools.len(),
            unique_tool_count: tool_counts.len(),
            failed_tool_count,
            tool_counts,
            recognized_skill_ids: recognized.into_iter().collect(),
            loaded_skill_ids: loaded.into_iter().collect(),
            ignored_skill_ids: ignored.into_iter().collect(),
            document_search_count,
            memory_search_count,
            observation_search_count,
            document_queries: dedupe(document_queries),
            memory_queries: dedupe(memory_queries),
            changed_files: changed_files
                .into_iter()
                .map(|(path, kind)| DeliveryChangedFile { path, kind })
                .collect(),
        }
    }

    fn current_turn_failed(&self) -> bool {
        let prefix = turn_prefix(self.active_turn_id);
        self.output_msgs.iter().any(|message| {
            message
                .id
                .as_deref()
                .is_some_and(|id| id.starts_with(&prefix))
                && message.state == Some(ResponseSectionState::Failed)
                && message.kind != MsgKind::Thinking
        })
    }

    fn upsert_delivery_response_section(&mut self, summary: &DeliverySummary) {
        let section_id = delivery_section_id(summary.turn_id);
        let body = build_delivery_response_body(summary);
        self.begin_response_section(ResponseSection {
            id: section_id.clone(),
            parent_id: None,
            kind: ResponseSectionKind::Step,
            title: "Task Delivery Summary".to_string(),
            state: ResponseSectionState::Complete,
            metadata: delivery_response_metadata(summary),
        });
        self.append_response_section(&section_id, &body);
        self.complete_response_section(&section_id, ResponseSectionState::Complete);
    }
}

pub(super) fn delivery_section_id(turn_id: u64) -> String {
    format!("turn-{turn_id}:delivery-summary")
}

fn delivery_response_metadata(summary: &DeliverySummary) -> ResponseSectionMetadata {
    ResponseSectionMetadata {
        scene_id: Some("delivery".to_string()),
        origin: SectionOrigin::Workflow {
            workflow_id: format!("delivery-{}", summary.turn_id),
            workflow_role: WorkflowRunRole::Child,
        },
        step_id: Some("task-delivery-summary".to_string()),
        step_label: Some("Task Delivery Summary".to_string()),
        subflow_ref: None,
    }
}

pub(super) fn turn_prefix(turn_id: u64) -> String {
    format!("turn-{turn_id}:")
}

pub(super) fn extract_turn_id_from_section_id(section_id: &str) -> Option<u64> {
    let suffix = section_id.strip_prefix("turn-")?;
    let turn = suffix.split(':').next()?;
    turn.parse().ok()
}

fn tool_change_kind(tool_name: &str) -> Option<DeliveryFileChangeKind> {
    match tool_name {
        "create_file" => Some(DeliveryFileChangeKind::Create),
        "apply_patch" | "edit_file" | "write_file" => Some(DeliveryFileChangeKind::Update),
        _ => None,
    }
}

fn build_delivery_panel_lines(summary: &DeliverySummary) -> Vec<String> {
    let mut lines = vec![format!("status: {}", summary.status.as_str())];
    lines.push(format!(
        "model: {}",
        summary.primary_model.as_deref().unwrap_or("unknown")
    ));
    lines.push(format!(
        "tokens: {} total{}",
        summary.total_tokens(),
        if summary.token_stats_partial { " (partial)" } else { "" }
    ));
    lines.push(format!(
        "llm: {} requests",
        summary.llm_request_count
    ));
    lines.push(format!(
        "tools: {} calls · {} unique · {} failed",
        summary.tool_call_count, summary.unique_tool_count, summary.failed_tool_count
    ));
    lines.push(format!(
        "skills: {} loaded / {} recognized / {} ignored",
        summary.loaded_skill_ids.len(),
        summary.recognized_skill_ids.len(),
        summary.ignored_skill_ids.len(),
    ));
    lines.push(format!(
        "knowledge: document {} · memory {} · observations {}",
        summary.document_search_count,
        summary.memory_search_count,
        summary.observation_search_count,
    ));
    if summary.changed_files.is_empty() {
        lines.push("files: none changed".to_string());
    } else {
        lines.push(format!("files: {} changed", summary.changed_files.len()));
        lines.push(format!(
            "paths: {}",
            summary
                .changed_files
                .iter()
                .map(|file| file.path.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    lines
}

fn build_delivery_response_body(summary: &DeliverySummary) -> String {
    vec![
        summary.summary_line(),
        format!(
            "document searches: {} · memory searches: {} · observations: {}",
            summary.document_search_count,
            summary.memory_search_count,
            summary.observation_search_count
        ),
        if summary.changed_files.is_empty() {
            "changed files: none".to_string()
        } else {
            format!(
                "changed files: {}",
                summary
                    .changed_files
                    .iter()
                    .map(|file| file.path.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        },
    ]
    .join("\n")
}

pub(super) fn build_delivery_detail_lines(summary: &DeliverySummary) -> Vec<String> {
    let mut lines = vec![
        format!("turn: {}", summary.turn_id),
        format!("status: {}", summary.status.as_str()),
        format!(
            "model: {}",
            summary.primary_model.as_deref().unwrap_or("unknown")
        ),
        format!(
            "tokens: total={} request_input={} cache_create={} cache_read={}{}",
            summary.total_tokens(),
            summary.input_tokens,
            summary.cache_creation_input_tokens,
            summary.cache_read_input_tokens,
            if summary.token_stats_partial { " (partial)" } else { "" }
        ),
        format!("llm requests: {}", summary.llm_request_count),
        String::new(),
        format!(
            "tools: {} calls · {} unique · {} failed",
            summary.tool_call_count, summary.unique_tool_count, summary.failed_tool_count
        ),
    ];

    if summary.tool_counts.is_empty() {
        lines.push("tool counts: none".to_string());
    } else {
        for (tool_name, count) in &summary.tool_counts {
            lines.push(format!("- {tool_name}: {count}"));
        }
    }

    lines.push(String::new());
    lines.push(format!(
        "skills: loaded={} recognized={} ignored={}",
        summary.loaded_skill_ids.len(),
        summary.recognized_skill_ids.len(),
        summary.ignored_skill_ids.len()
    ));
    lines.push(format!(
        "recognized ids: {}",
        join_or_none(&summary.recognized_skill_ids)
    ));
    lines.push(format!(
        "loaded ids: {}",
        join_or_none(&summary.loaded_skill_ids)
    ));
    lines.push(format!(
        "ignored ids: {}",
        join_or_none(&summary.ignored_skill_ids)
    ));

    lines.push(String::new());
    lines.push(format!(
        "knowledge: document={} memory={} observations={}",
        summary.document_search_count, summary.memory_search_count, summary.observation_search_count
    ));
    lines.push(format!(
        "document queries: {}",
        join_or_none(&summary.document_queries)
    ));
    lines.push(format!(
        "memory queries: {}",
        join_or_none(&summary.memory_queries)
    ));

    lines.push(String::new());
    if summary.changed_files.is_empty() {
        lines.push("changed files: none".to_string());
    } else {
        lines.push("changed files:".to_string());
        for file in &summary.changed_files {
            lines.push(format!("- {} [{}]", file.path, file.kind.as_str()));
        }
    }

    lines
}

fn join_or_none(values: &[String]) -> String {
    if values.is_empty() {
        "none".to_string()
    } else {
        values.join(", ")
    }
}

fn dedupe(values: Vec<String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    values
        .into_iter()
        .filter(|value| seen.insert(value.clone()))
        .collect()
}