mod document_model;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
#[cfg(feature = "document-backend")]
use anyhow::Context;
use omega_client::{
    ChatRequest, ContentBlock, Message, MessageContent, PromptCacheControl, Role, SystemBlock,
    ToolDefinition,
};
#[cfg(feature = "document-backend")]
use omega_document::OmegaDocument;
pub use document_model::{
    ArchiveTrigger, DocType, DocumentActivitySummary, DocumentHealthReport,
    DocumentHealthStatus, DocumentMutationMode, DocumentOp, DocumentOperatorUsage,
    DocumentOpResult, DocumentStoreVersion, FileRecord, FileStatus, FileType, HealthScore,
    MetadataUpdate, ScanResult, SearchFilter, SearchMode, SearchQuery, SearchResult,
    SortField, TodoOp, TodoOpResult,
};
pub use omega_memory::StepSummary as ContextStepSummary;
pub use omega_memory::{
    GovernanceEventSignal, MemoryQuery, MemoryQueryHit, ObservationFreshness, ObservationQuery,
    ProjectObservation, RetentionProfile, TurnRetentionSignals,
};
use omega_memory::{
    rank_summary_candidates, should_trigger_context_compaction, ArchivedTurnInput,
    LocalMemoryStore, MemoryStoreStats, StepContextHint,
};
use omega_tools::{
    ToolErrorKind, ToolFamily, ToolHandler, ToolManifestMetadata, ToolResult,
};
use omega_workflow::{DataFormat, StepOutputContract};
use serde::{Deserialize, Serialize};
#[cfg(feature = "document-backend")]
use serde::de::DeserializeOwned;
use serde_json::{json, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextWorkflowRole {
    Root,
    Child,
}

impl ContextWorkflowRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Root => "root",
            Self::Child => "child",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextRouting {
    pub recognized_scene_id: Option<String>,
    pub selected_workflow_id: Option<String>,
    pub active_workflow_id: String,
    pub active_workflow_role: ContextWorkflowRole,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextSession {
    pub latest_user_turn: String,
    pub routing: ContextRouting,
    pub step_summaries: Vec<ContextStepSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextStep {
    pub id: String,
    pub label: String,
    pub prompt_path: PathBuf,
    pub input_sources: Vec<String>,
    pub output_contract: StepOutputContract,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextExecuteItem {
    pub item_id: String,
    pub item_index: usize,
    pub item_total: usize,
    pub item_label: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StepContextRequest {
    pub skill_system_prompt: String,
    pub cwd: PathBuf,
    pub session: ContextSession,
    pub step: ContextStep,
    pub step_prompt: String,
    pub document_hits: Vec<SearchResult>,
    pub memory_hits: Vec<MemoryQueryHit>,
    pub observation_hits: Vec<ProjectObservation>,
    pub structured_input: Option<Value>,
    pub todo_snapshot: Option<String>,
    pub current_execute_item: Option<ContextExecuteItem>,
    pub visible_tool_names: Vec<String>,
    pub tool_manifests: Vec<ToolManifestMetadata>,
    pub tool_definitions: Vec<ToolDefinition>,
    pub messages: Vec<Message>,
    pub recall_rewrite_reason: Option<String>,
    pub recall_rewrite_queries: Vec<String>,
    pub recall_recovery_path: Option<String>,
    pub context_window: u32,
    pub max_output_tokens: u32,
    pub safety_margin_tokens: u32,
    pub report_step_id: String,
    pub execute_step_id: String,
    pub plan_step_id: String,
    pub scene_recognition_step_id: String,
    pub select_workflow_step_id: String,
    pub root_workflow_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputRepairFailure {
    pub error_kind: String,
    pub message: String,
    pub previous_response_preview: String,
    pub extracted_json_preview: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OutputRepairContextRequest {
    pub step_request: StepContextRequest,
    pub failure: OutputRepairFailure,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContextTokenCountSource {
    ProviderCountTokens,
    Estimated,
}

impl ContextTokenCountSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ProviderCountTokens => "provider_count_tokens",
            Self::Estimated => "estimated",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextCacheDiagnostics {
    pub token_count_source: ContextTokenCountSource,
    pub request_input_tokens: u32,
    pub budget_input_tokens: u32,
    pub cache_breakpoints: Vec<String>,
    pub cache_creation_input_tokens: Option<u32>,
    pub cache_read_input_tokens: Option<u32>,
    pub uncached_input_tokens: Option<u32>,
    pub cache_hit_ratio_percent: Option<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssembledContext {
    pub system_blocks: Vec<SystemBlock>,
    pub selected_step_summaries: Vec<ContextStepSummary>,
    pub cache_diagnostics: ContextCacheDiagnostics,
    pub document_summary: Option<DocumentHitSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TurnData {
    pub turn_id: u64,
    pub workflow_id: String,
    pub user_intent: String,
    pub summaries: Vec<ContextStepSummary>,
    pub signals: TurnRetentionSignals,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TurnSummary {
    pub turn_id: u64,
    pub workflow_id: String,
    pub user_intent: String,
    pub summary_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CompactionPolicy {
    pub trigger: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CompactionResult {
    pub trigger: String,
    pub changed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ContextDiagnostics {
    pub budget: ContextBudgetDiagnostics,
    pub cache: Option<ContextCacheDiagnostics>,
    pub memory: ContextMemoryDiagnostics,
    pub document: ContextDocumentDiagnostics,
    pub store: ContextStoreDiagnostics,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ContextBudgetDiagnostics {
    pub budget_input_tokens: u32,
    pub request_input_tokens: u32,
    pub headroom_tokens: u32,
    pub usage_percent: u8,
    pub selected_summary_count: u32,
    pub available_summary_count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ContextMemoryDiagnostics {
    pub total_turns_archived: u64,
    pub compactions_triggered: u64,
    pub last_compaction_at: Option<u64>,
    pub current_summary_tokens: u32,
    pub current_summary_count: u32,
    pub compression_ratio_avg_percent: u8,
    pub retention_candidates_accepted: u64,
    pub retention_candidates_dropped: u64,
    pub dropped_candidates_by_profile: BTreeMap<String, u64>,
    pub memory_query_count: u64,
    pub memory_query_hit_mix: BTreeMap<String, u64>,
    pub observation_count: u64,
    pub observation_fresh_count: u64,
    pub observation_stale_count: u64,
    pub observation_superseded_count: u64,
    pub observation_corrected_count: u64,
    pub observation_correction_activity: u64,
    pub current_query: Option<MemoryQueryDiagnostics>,
    pub current_observations: Option<ObservationRecallDiagnostics>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ContextDocumentDiagnostics {
    pub total_files_indexed: u64,
    pub total_chunks: u64,
    pub total_embeddings: u64,
    pub index_staleness_seconds: u64,
    pub governance_health: Option<HealthScore>,
    pub health_status: DocumentHealthStatus,
    pub last_health_check: Option<u64>,
    pub active_version: Option<DocumentStoreVersion>,
    pub pending_version: Option<DocumentStoreVersion>,
    pub last_promotion_error: Option<String>,
    pub recent_activity: Vec<DocumentActivitySummary>,
    pub operator_usage: Vec<DocumentOperatorUsage>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ContextStoreDiagnostics {
    pub lance_db_size_bytes: u64,
    pub tantivy_index_size_bytes: u64,
    pub todo_items_count: u32,
    pub turn_archive_count: u32,
    pub turn_archive_size_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SupervisionReadiness {
    Disabled,
    #[default]
    Idle,
    Uninitialized,
    Ready,
    Degraded,
    Failed,
}

impl SupervisionReadiness {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Idle => "idle",
            Self::Uninitialized => "uninitialized",
            Self::Ready => "ready",
            Self::Degraded => "degraded",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ContextSupervisionSnapshot {
    pub document: DocumentSupervisionSnapshot,
    pub memory: MemorySupervisionSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct StepKnowledgeSummary {
    pub document: Option<ResponseDocumentKnowledge>,
    pub memory: Option<ResponseMemoryKnowledge>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ResponseDocumentKnowledge {
    pub raw_query: String,
    pub planned_queries: Vec<String>,
    pub rewrite_reason: Option<String>,
    pub rewrite_queries: Vec<String>,
    pub recovery_path: Option<String>,
    pub readiness: SupervisionReadiness,
    pub query: String,
    pub mode: String,
    pub degraded_from: Option<String>,
    pub reason: Option<String>,
    pub result_count: u32,
    pub top_hits: Vec<DocumentHitItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ResponseMemoryKnowledge {
    pub raw_query: Option<String>,
    pub planned_queries: Vec<String>,
    pub rewrite_reason: Option<String>,
    pub rewrite_queries: Vec<String>,
    pub recovery_path: Option<String>,
    pub memory_query: Option<String>,
    pub observation_query: Option<String>,
    pub selected_summary_count: u32,
    pub top_selected_summaries: Vec<MemoryHitItem>,
    pub memory_hit_count: u32,
    pub observation_hit_count: u32,
    pub top_memory_hits: Vec<MemoryQueryHitItem>,
    pub top_observations: Vec<ObservationRecallHitItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct DocumentSupervisionSnapshot {
    pub enabled: bool,
    pub readiness: SupervisionReadiness,
    pub health_status: DocumentHealthStatus,
    pub totals: DocumentSupervisionTotals,
    pub active_version: Option<DocumentStoreVersion>,
    pub pending_version: Option<DocumentStoreVersion>,
    pub last_promotion_error: Option<String>,
    pub recent_activity: Vec<DocumentActivitySummary>,
    pub operator_usage: Vec<DocumentOperatorUsage>,
    pub current_hits: Option<DocumentHitSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct DocumentSupervisionTotals {
    pub total_files_indexed: u64,
    pub total_chunks: u64,
    pub total_embeddings: u64,
    pub index_staleness_seconds: u64,
    pub governance_health: Option<HealthScore>,
    pub last_health_check: Option<u64>,
    pub lance_db_size_bytes: u64,
    pub tantivy_index_size_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct DocumentHitSummary {
    pub raw_query: String,
    pub planned_queries: Vec<String>,
    pub rewrite_reason: Option<String>,
    pub rewrite_queries: Vec<String>,
    pub recovery_path: Option<String>,
    pub query: String,
    pub mode: String,
    pub degraded_from: Option<String>,
    pub result_count: u32,
    pub top_hits: Vec<DocumentHitItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct DocumentHitItem {
    pub path: String,
    pub preview: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct MemorySupervisionSnapshot {
    pub enabled: bool,
    pub readiness: SupervisionReadiness,
    pub totals: MemorySupervisionTotals,
    pub current_hits: Option<MemoryHitSummary>,
    pub current_query: Option<MemoryQueryDiagnostics>,
    pub current_observations: Option<ObservationRecallDiagnostics>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct MemorySupervisionTotals {
    pub total_turns_archived: u64,
    pub compactions_triggered: u64,
    pub current_summary_tokens: u32,
    pub current_summary_count: u32,
    pub turn_archive_count: u32,
    pub turn_archive_size_bytes: u64,
    pub retention_candidates_accepted: u64,
    pub retention_candidates_dropped: u64,
    pub dropped_candidates_by_profile: BTreeMap<String, u64>,
    pub memory_query_count: u64,
    pub memory_query_hit_mix: BTreeMap<String, u64>,
    pub observation_count: u64,
    pub observation_fresh_count: u64,
    pub observation_stale_count: u64,
    pub observation_superseded_count: u64,
    pub observation_corrected_count: u64,
    pub observation_correction_activity: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct MemoryQueryDiagnostics {
    pub raw_query: String,
    pub planned_queries: Vec<String>,
    pub rewrite_reason: Option<String>,
    pub rewrite_queries: Vec<String>,
    pub recovery_path: Option<String>,
    pub query: String,
    pub result_count: u32,
    pub hit_mix: BTreeMap<String, u32>,
    pub top_hits: Vec<MemoryQueryHitItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct MemoryQueryHitItem {
    pub profile: String,
    pub title: String,
    pub preview: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ObservationRecallDiagnostics {
    pub raw_query: String,
    pub planned_queries: Vec<String>,
    pub rewrite_reason: Option<String>,
    pub rewrite_queries: Vec<String>,
    pub recovery_path: Option<String>,
    pub query: String,
    pub result_count: u32,
    pub freshness_mix: BTreeMap<String, u32>,
    pub top_hits: Vec<ObservationRecallHitItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObservationRecallHitItem {
    pub id: String,
    pub title: String,
    pub summary: String,
    pub freshness: ObservationFreshness,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MemoryKnowledgeSnapshot {
    pub total_turns_archived: u64,
    pub retention_candidates_accepted: u64,
    pub retention_candidates_dropped: u64,
    pub dropped_candidates_by_profile: BTreeMap<String, u64>,
    pub memory_query_count: u64,
    pub memory_query_hit_mix: BTreeMap<String, u64>,
    pub observation_count: u64,
    pub observation_fresh_count: u64,
    pub observation_stale_count: u64,
    pub observation_superseded_count: u64,
    pub observation_corrected_count: u64,
    pub observation_correction_activity: u64,
    pub current_query: Option<MemoryQueryDiagnostics>,
    pub current_observations: Option<ObservationRecallDiagnostics>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct RecallPlan {
    pub document_hits: Vec<SearchResult>,
    pub memory_hits: Vec<MemoryQueryHit>,
    pub observations: Vec<ProjectObservation>,
    pub document_summary: Option<DocumentHitSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecallBudgetConfig {
    pub memory_hits_tokens: u32,
    pub observation_hits_tokens: u32,
    pub document_hits_tokens: u32,
}

impl Default for RecallBudgetConfig {
    fn default() -> Self {
        Self {
            memory_hits_tokens: 220,
            observation_hits_tokens: 220,
            document_hits_tokens: 240,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct RecallQueryBundle {
    raw_query: String,
    keyword_queries: Vec<String>,
    concept_queries: Vec<String>,
    path_hints: Vec<String>,
    doc_type_hints: Vec<DocType>,
    rewrite_reason: Option<String>,
    rewrite_queries: Vec<String>,
    recovery_path: Option<String>,
}

impl RecallQueryBundle {
    fn planned_queries(&self) -> Vec<String> {
        let mut values = self.keyword_queries.clone();
        values.extend(self.concept_queries.clone());
        values.extend(self.rewrite_queries.clone());
        dedupe_trimmed_strings(&values)
            .into_iter()
            .take(5)
            .collect()
    }

    fn primary_query(&self) -> String {
        self.planned_queries()
            .into_iter()
            .next()
            .unwrap_or_else(|| self.raw_query.clone())
    }
}

impl RecallBudgetConfig {
    fn load(root: &Path) -> Self {
        let path = root.join(".omega/model.toml");
        let Ok(contents) = fs::read_to_string(path) else {
            return Self::default();
        };
        let Ok(file) = toml::from_str::<RecallBudgetConfigFile>(&contents) else {
            return Self::default();
        };

        let mut config = Self::default();
        if let Some(context) = file.context {
            if let Some(recall) = context.recall {
                if let Some(value) = recall.memory_hits_tokens.filter(|value| *value > 0) {
                    config.memory_hits_tokens = value;
                }
                if let Some(value) = recall.observation_hits_tokens.filter(|value| *value > 0) {
                    config.observation_hits_tokens = value;
                }
                if let Some(value) = recall.document_hits_tokens.filter(|value| *value > 0) {
                    config.document_hits_tokens = value;
                }
            }
        }
        config
    }
}

#[derive(Debug, Deserialize)]
struct RecallBudgetConfigFile {
    #[serde(default)]
    context: Option<RecallBudgetContextConfigFile>,
}

#[derive(Debug, Deserialize)]
struct RecallBudgetContextConfigFile {
    #[serde(default)]
    recall: Option<RecallBudgetValuesFile>,
}

#[derive(Debug, Deserialize)]
struct RecallBudgetValuesFile {
    #[serde(default)]
    memory_hits_tokens: Option<u32>,
    #[serde(default)]
    observation_hits_tokens: Option<u32>,
    #[serde(default)]
    document_hits_tokens: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct MemoryHitSummary {
    pub selected_count: u32,
    pub total_tokens: u32,
    pub top_hits: Vec<MemoryHitItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct MemoryHitItem {
    pub workflow_id: String,
    pub step_id: String,
    pub title: String,
    pub preview: String,
}

pub trait ContextTokenCounter: Send + Sync {
    fn count_request_tokens(&self, request: ChatRequest) -> Result<u32>;
}

pub trait ContextAssembler: Send + Sync {
    fn assemble_step_context(
        &self,
        request: StepContextRequest,
        token_counter: &dyn ContextTokenCounter,
    ) -> Result<AssembledContext>;

    fn assemble_output_repair_context(
        &self,
        request: OutputRepairContextRequest,
    ) -> Result<Vec<SystemBlock>>;

    fn estimate_tokens(&self, text: &str) -> u32;
}

pub trait MemoryService: Send + Sync {
    fn archive_turn(&self, turn: &TurnData) -> Result<()>;
    fn compact_context(&self, policy: CompactionPolicy) -> Result<CompactionResult>;
    fn get_turn_history(&self, limit: usize) -> Result<Vec<TurnSummary>>;
    fn query(&self, query: MemoryQuery) -> Result<Vec<MemoryQueryHit>>;
    fn query_observations(&self, query: ObservationQuery) -> Result<Vec<ProjectObservation>>;
    fn diagnostics_snapshot(&self) -> Result<MemoryKnowledgeSnapshot>;
}

pub trait KnowledgeQueryService: Send + Sync {
    fn scan_workspace(&self) -> Result<ScanResult>;
    fn search(&self, query: SearchQuery) -> Result<Vec<SearchResult>>;
}

pub trait DocumentGovernanceService: Send + Sync {
    fn manage_document(&self, op: DocumentOp) -> Result<DocumentOpResult>;
    fn manage_todo(&self, op: TodoOp) -> Result<TodoOpResult>;
    fn check_document_health(&self) -> Result<DocumentHealthReport>;
}

pub trait ContextDiagnosticsProvider: Send + Sync {
    fn context_diagnostics(&self) -> ContextDiagnostics;
    fn cache_diagnostics(&self) -> Option<ContextCacheDiagnostics>;
    fn record_context_assembly(
        &self,
        cache: &ContextCacheDiagnostics,
        selected_step_summaries: &[ContextStepSummary],
        total_available_summaries: usize,
    );
    fn record_document_scan(&self, scan: &ScanResult);
    fn record_document_health(&self, health: &DocumentHealthReport);
    fn record_document_usage(&self, operator: &str, source: &str, detail: &str);
    fn record_memory_snapshot(&self, snapshot: &MemoryKnowledgeSnapshot);
}

pub struct OmegaContextFacade {
    pub assembler: Arc<dyn ContextAssembler>,
    pub memory: Arc<dyn MemoryService>,
    pub query: Arc<dyn KnowledgeQueryService>,
    pub governance: Arc<dyn DocumentGovernanceService>,
    pub diagnostics: Arc<dyn ContextDiagnosticsProvider>,
    pub document_backend_enabled: bool,
    recall_budget: RecallBudgetConfig,
}

impl OmegaContextFacade {
    pub fn local(root: PathBuf) -> Self {
        let diagnostics_root = root.clone();
        let diagnostics = Arc::new(LocalDiagnostics::new(diagnostics_root));
        let memory = Arc::new(LocalMemoryService::new(root.clone()));
        let recall_budget = RecallBudgetConfig::load(&root);
        if let Ok(snapshot) = memory.diagnostics_snapshot() {
            diagnostics.record_memory_snapshot(&snapshot);
        }
        #[cfg(feature = "document-backend")]
        let documents = Arc::new(OmegaDocument::new(root.clone()));
        Self {
            assembler: Arc::new(DefaultContextAssembler),
            memory,
            query: Arc::new(LocalKnowledgeQueryService::new(
                root.clone(),
                #[cfg(feature = "document-backend")]
                Some(Arc::clone(&documents)),
            )),
            governance: Arc::new(LocalDocumentGovernanceService::new(
                root,
                #[cfg(feature = "document-backend")]
                Some(documents),
            )),
            diagnostics,
            document_backend_enabled: cfg!(feature = "document-backend"),
            recall_budget,
        }
    }

    pub fn assemble_step_context(
        &self,
        request: StepContextRequest,
        token_counter: &dyn ContextTokenCounter,
    ) -> Result<AssembledContext> {
        let recall = self.plan_recall(&request)?;
        let mut request = request;
        request.document_hits = recall.document_hits;
        request.memory_hits = recall.memory_hits;
        request.observation_hits = recall.observations;
        let mut assembled = self.assembler.assemble_step_context(request, token_counter)?;
        assembled.document_summary = recall.document_summary;
        self.sync_memory_snapshot();
        Ok(assembled)
    }

    fn plan_recall(&self, request: &StepContextRequest) -> Result<RecallPlan> {
        let bundle = build_recall_query_bundle(request);
        let planned_queries = bundle.planned_queries();
        if planned_queries.is_empty() && bundle.raw_query.trim().is_empty() {
            self.sync_memory_snapshot();
            return Ok(RecallPlan::default());
        }

        let memory_hits = self.memory.query(MemoryQuery {
            text: Some(bundle.primary_query()),
            queries: planned_queries.clone(),
            raw_query: Some(bundle.raw_query.clone()),
            profiles: vec![
                RetentionProfile::ProjectFacts,
                RetentionProfile::DeveloperPreferences,
                RetentionProfile::OpenThreads,
            ],
            max_results: 6,
            rewrite_reason: bundle.rewrite_reason.clone(),
            rewrite_queries: bundle.rewrite_queries.clone(),
            recovery_path: bundle.recovery_path.clone(),
        })?;
        let observations = self.memory.query_observations(ObservationQuery {
            text: Some(bundle.primary_query()),
            queries: planned_queries.clone(),
            raw_query: Some(bundle.raw_query.clone()),
            max_results: 4,
            include_stale: true,
            rewrite_reason: bundle.rewrite_reason.clone(),
            rewrite_queries: bundle.rewrite_queries.clone(),
            recovery_path: bundle.recovery_path.clone(),
        })?;
        let (document_hits, document_summary) =
            self.plan_document_recall(&bundle, &memory_hits, &observations);

        Ok(RecallPlan {
            document_hits,
            memory_hits: trim_memory_hits_for_budget(
                memory_hits,
                self.recall_budget.memory_hits_tokens,
            ),
            observations: trim_observations_for_budget(
                observations,
                self.recall_budget.observation_hits_tokens,
            ),
            document_summary,
        })
    }

    fn plan_document_recall(
        &self,
        bundle: &RecallQueryBundle,
        memory_hits: &[MemoryQueryHit],
        observations: &[ProjectObservation],
    ) -> (Vec<SearchResult>, Option<DocumentHitSummary>) {
        if !self.document_backend_enabled {
            return (Vec::new(), None);
        }

        let planned_queries = bundle.planned_queries();
        if planned_queries.is_empty() {
            return (Vec::new(), None);
        }

        self.diagnostics.record_document_usage(
            "context_recall_planner",
            "planner",
            &format!(
                "queries={} raw={}",
                planned_queries.len(),
                preview_text(&bundle.raw_query, 80)
            ),
        );
        let related_paths = collect_related_paths(memory_hits, observations, &bundle.path_hints);
        let mut merged_results = Vec::new();
        for query_text in &planned_queries {
            let recent_window = query_requests_recent_window(query_text)
                .then(|| current_unix_timestamp().saturating_sub(14 * 24 * 60 * 60));
            let filters = recent_window
                .map(|timestamp| vec![SearchFilter::ModifiedAfter(timestamp)])
                .unwrap_or_else(|| document_query_filters(&bundle.doc_type_hints));
            let results = self
                .query
                .search(SearchQuery {
                    text: Some(query_text.to_string()),
                    mode: SearchMode::Hybrid,
                    filters,
                    sort: None,
                    max_results: 6,
                })
                .or_else(|_| {
                    self.query.search(SearchQuery {
                        text: Some(query_text.to_string()),
                        mode: SearchMode::Hybrid,
                        filters: document_query_filters(&bundle.doc_type_hints),
                        sort: None,
                        max_results: 6,
                    })
                })
                .unwrap_or_default();
            merged_results.extend(results);
        }

        let hits = trim_document_hits_for_budget(
            rerank_document_hits(dedupe_document_hits_by_path(merged_results), &related_paths),
            self.recall_budget.document_hits_tokens,
        );
        let summary = Some(DocumentHitSummary {
            raw_query: bundle.raw_query.clone(),
            planned_queries,
            rewrite_reason: bundle.rewrite_reason.clone(),
            rewrite_queries: bundle.rewrite_queries.clone(),
            recovery_path: bundle.recovery_path.clone(),
            query: bundle.primary_query(),
            mode: hits
                .first()
                .map(|hit| search_mode_label(hit.mode_used).to_string())
                .unwrap_or_else(|| "hybrid".to_string()),
            degraded_from: hits
                .iter()
                .find_map(|hit| hit.degraded_from.map(search_mode_label))
                .map(ToOwned::to_owned),
            result_count: hits.len() as u32,
            top_hits: hits
                .iter()
                .take(5)
                .map(|hit| DocumentHitItem {
                    path: hit.path.clone(),
                    preview: preview_text(&hit.preview, 140),
                })
                .collect(),
        });

        (hits, summary)
    }

    fn sync_memory_snapshot(&self) {
        if let Ok(snapshot) = self.memory.diagnostics_snapshot() {
            self.diagnostics.record_memory_snapshot(&snapshot);
        }
    }
}

#[cfg(feature = "document-backend")]
fn convert_to_backend<T, U>(value: T) -> Result<U>
where
    T: Serialize,
    U: DeserializeOwned,
{
    serde_json::from_value(serde_json::to_value(value)?).context("convert to omega-document type")
}

#[cfg(feature = "document-backend")]
fn convert_from_backend<T, U>(value: T) -> Result<U>
where
    T: Serialize,
    U: DeserializeOwned,
{
    serde_json::from_value(serde_json::to_value(value)?).context("convert from omega-document type")
}

pub struct DefaultContextAssembler;

impl ContextAssembler for DefaultContextAssembler {
    fn assemble_step_context(
        &self,
        request: StepContextRequest,
        token_counter: &dyn ContextTokenCounter,
    ) -> Result<AssembledContext> {
        let available_input_budget = request
            .context_window
            .saturating_sub(request.max_output_tokens)
            .saturating_sub(request.safety_margin_tokens);
        let base_request = build_preview_request(&request, &[]);
        let (base_tokens, _) = count_preview_request_tokens(token_counter, base_request);
        let compaction_triggered = should_trigger_context_compaction(
            base_tokens,
            available_input_budget,
            request.session.step_summaries.len(),
        );
        let hint = StepContextHint {
            step_id: request.step.id.clone(),
            input_sources: request.step.input_sources.clone(),
            active_workflow_id: request.session.routing.active_workflow_id.clone(),
            report_step_id: request.report_step_id.clone(),
            execute_step_id: request.execute_step_id.clone(),
            plan_step_id: request.plan_step_id.clone(),
            scene_recognition_step_id: request.scene_recognition_step_id.clone(),
            select_workflow_step_id: request.select_workflow_step_id.clone(),
            root_workflow_id: request.root_workflow_id.clone(),
            has_execute_item: request.current_execute_item.is_some(),
        };
        let ranked_candidates =
            rank_summary_candidates(&request.session.step_summaries, &hint, compaction_triggered);

        let mut selected = Vec::new();
        for candidate in ranked_candidates {
            let mut next_selected = selected.clone();
            next_selected.push(candidate);
            next_selected.sort_by_key(|candidate| candidate.original_index);
            let candidate_summaries = next_selected
                .iter()
                .map(|candidate| candidate.summary.clone())
                .collect::<Vec<_>>();
            let preview_request = build_preview_request(&request, &candidate_summaries);
            let (candidate_tokens, _) =
                count_preview_request_tokens(token_counter, preview_request);
            if candidate_tokens <= available_input_budget {
                selected = next_selected;
            }
        }

        let selected = selected
            .into_iter()
            .map(|candidate| candidate.summary)
            .collect::<Vec<_>>();
        let final_request = build_preview_request(&request, &selected);
        let (final_tokens, final_source) =
            count_preview_request_tokens(token_counter, final_request);

        Ok(AssembledContext {
            system_blocks: build_step_system_blocks(&request, &selected),
            selected_step_summaries: selected.clone(),
            cache_diagnostics: ContextCacheDiagnostics {
                token_count_source: final_source,
                request_input_tokens: final_tokens,
                budget_input_tokens: available_input_budget,
                cache_breakpoints: cache_breakpoints_for_step(&selected, &request.messages),
                cache_creation_input_tokens: None,
                cache_read_input_tokens: None,
                uncached_input_tokens: None,
                cache_hit_ratio_percent: None,
            },
            document_summary: None,
        })
    }

    fn assemble_output_repair_context(
        &self,
        request: OutputRepairContextRequest,
    ) -> Result<Vec<SystemBlock>> {
        Ok(build_output_repair_system_blocks(
            &request.step_request,
            &request.failure,
        ))
    }

    fn estimate_tokens(&self, text: &str) -> u32 {
        estimate_tokens(text)
    }
}

struct LocalMemoryService {
    store: LocalMemoryStore,
    runtime: Mutex<MemoryRuntimeState>,
}

#[derive(Debug, Default)]
struct MemoryRuntimeState {
    query_count: u64,
    query_hit_mix: BTreeMap<String, u64>,
    current_query: Option<MemoryQueryDiagnostics>,
    current_observations: Option<ObservationRecallDiagnostics>,
}

impl LocalMemoryService {
    fn new(root: PathBuf) -> Self {
        Self {
            store: LocalMemoryStore::new(root),
            runtime: Mutex::new(MemoryRuntimeState::default()),
        }
    }

    fn build_snapshot(&self, store_stats: MemoryStoreStats) -> MemoryKnowledgeSnapshot {
        let runtime = self.runtime.lock().unwrap();
        MemoryKnowledgeSnapshot {
            total_turns_archived: store_stats.total_turns_archived,
            retention_candidates_accepted: store_stats.retained_candidates_accepted,
            retention_candidates_dropped: store_stats.retained_candidates_dropped,
            dropped_candidates_by_profile: store_stats.dropped_by_profile,
            memory_query_count: runtime.query_count,
            memory_query_hit_mix: runtime.query_hit_mix.clone(),
            observation_count: store_stats.observation_totals.total,
            observation_fresh_count: store_stats.observation_totals.fresh,
            observation_stale_count: store_stats.observation_totals.stale,
            observation_superseded_count: store_stats.observation_totals.superseded,
            observation_corrected_count: store_stats.observation_totals.corrected,
            observation_correction_activity: store_stats.observation_totals.correction_activity,
            current_query: runtime.current_query.clone(),
            current_observations: runtime.current_observations.clone(),
        }
    }
}

impl MemoryService for LocalMemoryService {
    fn archive_turn(&self, turn: &TurnData) -> Result<()> {
        self.store.archive_turn(ArchivedTurnInput {
            turn_id: turn.turn_id,
            workflow_id: turn.workflow_id.clone(),
            user_intent: turn.user_intent.clone(),
            summaries: turn.summaries.clone(),
            signals: turn.signals.clone(),
        })?;
        Ok(())
    }

    fn compact_context(&self, policy: CompactionPolicy) -> Result<CompactionResult> {
        let changed = self
            .store
            .compact_archived_turns(omega_memory::MAX_UNCOMPACTED_SUMMARIES)?;
        Ok(CompactionResult {
            trigger: policy.trigger,
            changed,
        })
    }

    fn get_turn_history(&self, limit: usize) -> Result<Vec<TurnSummary>> {
        Ok(self
            .store
            .get_turn_history(limit)?
            .into_iter()
            .map(|turn| TurnSummary {
                turn_id: turn.turn_id,
                workflow_id: turn.workflow_id,
                user_intent: turn.user_intent,
                summary_count: turn.summary_count,
            })
            .collect())
    }

    fn query(&self, query: MemoryQuery) -> Result<Vec<MemoryQueryHit>> {
        let hits = self.store.query(&query)?;
        let query_text = query.text.unwrap_or_default();
        let planned_queries = dedupe_trimmed_strings(&query.queries);
        let mut hit_mix = BTreeMap::new();
        for hit in &hits {
            *hit_mix.entry(hit.profile.as_str().to_string()).or_default() += 1;
        }
        let mut runtime = self.runtime.lock().unwrap();
        runtime.query_count = runtime.query_count.saturating_add(1);
        for (profile, count) in &hit_mix {
            *runtime.query_hit_mix.entry(profile.clone()).or_default() += *count as u64;
        }
        runtime.current_query = Some(MemoryQueryDiagnostics {
            raw_query: query.raw_query.unwrap_or_else(|| query_text.clone()),
            planned_queries,
            rewrite_reason: query.rewrite_reason.clone(),
            rewrite_queries: dedupe_trimmed_strings(&query.rewrite_queries),
            recovery_path: query.recovery_path.clone(),
            query: query_text,
            result_count: hits.len() as u32,
            hit_mix,
            top_hits: hits
                .iter()
                .take(5)
                .map(|hit| MemoryQueryHitItem {
                    profile: hit.profile.as_str().to_string(),
                    title: hit.title.clone(),
                    preview: hit.preview.clone(),
                })
                .collect(),
        });
        Ok(hits)
    }

    fn query_observations(&self, query: ObservationQuery) -> Result<Vec<ProjectObservation>> {
        let observations = self.store.query_observations(&query)?;
        let query_text = query.text.unwrap_or_default();
        let mut freshness_mix = BTreeMap::new();
        for observation in &observations {
            *freshness_mix
                .entry(observation.freshness.as_str().to_string())
                .or_default() += 1;
        }
        self.runtime.lock().unwrap().current_observations = Some(ObservationRecallDiagnostics {
            raw_query: query.raw_query.unwrap_or_else(|| query_text.clone()),
            planned_queries: dedupe_trimmed_strings(&query.queries),
            rewrite_reason: query.rewrite_reason.clone(),
            rewrite_queries: dedupe_trimmed_strings(&query.rewrite_queries),
            recovery_path: query.recovery_path.clone(),
            query: query_text,
            result_count: observations.len() as u32,
            freshness_mix,
            top_hits: observations
                .iter()
                .take(5)
                .map(|observation| ObservationRecallHitItem {
                    id: observation.id.clone(),
                    title: observation.title.clone(),
                    summary: preview_text(&observation.summary, 160),
                    freshness: observation.freshness,
                })
                .collect(),
        });
        Ok(observations)
    }

    fn diagnostics_snapshot(&self) -> Result<MemoryKnowledgeSnapshot> {
        Ok(self.build_snapshot(self.store.stats()?))
    }
}

struct LocalKnowledgeQueryService {
    root: PathBuf,
    #[cfg(feature = "document-backend")]
    documents: Arc<OmegaDocument>,
}

impl LocalKnowledgeQueryService {
    fn new(
        root: PathBuf,
        #[cfg(feature = "document-backend")] documents: Option<Arc<OmegaDocument>>,
    ) -> Self {
        Self {
            root,
            #[cfg(feature = "document-backend")]
            documents: documents.expect("document backend enabled requires OmegaDocument"),
        }
    }
}

impl KnowledgeQueryService for LocalKnowledgeQueryService {
    fn scan_workspace(&self) -> Result<ScanResult> {
        #[cfg(feature = "document-backend")]
        {
            return convert_from_backend(self.documents.scan_workspace()?);
        }

        #[cfg(not(feature = "document-backend"))]
        {
            Ok(ScanResult {
                files_indexed: 0,
                chunks_indexed: 0,
                deleted_marked: 0,
                vector_ignored_files: 0,
                vector_ignored_paths: Vec::new(),
                indexed_paths: Vec::new(),
                embedded_paths: Vec::new(),
                manifest_path: self
                    .root
                    .join(".omega/store/files.jsonl")
                    .display()
                    .to_string(),
                keyword_index_path: self
                    .root
                    .join(".omega/store/tantivy")
                    .display()
                    .to_string(),
                active_version: None,
                pending_version: None,
                archived_version_path: None,
            })
        }
    }

    fn search(&self, query: SearchQuery) -> Result<Vec<SearchResult>> {
        #[cfg(feature = "document-backend")]
        {
            let query = convert_to_backend(query)?;
            let results: Vec<omega_document::SearchResult> = self.documents.search(query)?;
            return convert_from_backend(results);
        }

        #[cfg(not(feature = "document-backend"))]
        {
            let _ = query;
            anyhow::bail!(
                "document backend disabled; enable omega-context feature 'document-backend' to use search_codebase"
            )
        }
    }
}

struct LocalDocumentGovernanceService {
    root: PathBuf,
    #[cfg(feature = "document-backend")]
    documents: Arc<OmegaDocument>,
}

impl LocalDocumentGovernanceService {
    fn new(
        root: PathBuf,
        #[cfg(feature = "document-backend")] documents: Option<Arc<OmegaDocument>>,
    ) -> Self {
        Self {
            root,
            #[cfg(feature = "document-backend")]
            documents: documents.expect("document backend enabled requires OmegaDocument"),
        }
    }
}

impl DocumentGovernanceService for LocalDocumentGovernanceService {
    fn manage_document(&self, op: DocumentOp) -> Result<DocumentOpResult> {
        #[cfg(feature = "document-backend")]
        {
            let op = convert_to_backend(op)?;
            return convert_from_backend(self.documents.manage_document(op)?);
        }

        #[cfg(not(feature = "document-backend"))]
        {
            let _ = op;
            anyhow::bail!(
                "document backend disabled; enable omega-context feature 'document-backend' to use manage_document"
            )
        }
    }

    fn manage_todo(&self, op: TodoOp) -> Result<TodoOpResult> {
        #[cfg(feature = "document-backend")]
        {
            let op = convert_to_backend(op)?;
            return convert_from_backend(self.documents.manage_todo(op)?);
        }

        #[cfg(not(feature = "document-backend"))]
        {
            let _ = op;
            anyhow::bail!(
                "document backend disabled; enable omega-context feature 'document-backend' to use persistent todo storage"
            )
        }
    }

    fn check_document_health(&self) -> Result<DocumentHealthReport> {
        #[cfg(feature = "document-backend")]
        {
            return convert_from_backend(self.documents.check_document_health()?);
        }

        #[cfg(not(feature = "document-backend"))]
        {
            Ok(DocumentHealthReport {
                total_docs: 0,
                structure_violations: Vec::new(),
                naming_violations: Vec::new(),
                orphaned_docs: Vec::new(),
                broken_crossrefs: Vec::new(),
                stale_docs: Vec::new(),
                missing_frontmatter: Vec::new(),
                overall_health: HealthScore::NeedsAttention,
            })
        }
    }
}

#[derive(Debug, Default)]
struct DiagnosticsState {
    budget: ContextBudgetDiagnostics,
    cache: Option<ContextCacheDiagnostics>,
    memory: ContextMemoryDiagnostics,
    document: ContextDocumentDiagnostics,
    last_scan_at: Option<u64>,
    compression_ratio_total_percent: u64,
    compression_ratio_samples: u64,
}

struct LocalDiagnostics {
    root: PathBuf,
    state: Mutex<DiagnosticsState>,
}

impl LocalDiagnostics {
    fn new(root: PathBuf) -> Self {
        Self {
            root,
            state: Mutex::new(DiagnosticsState::default()),
        }
    }

    fn store_diagnostics(&self, turn_archive_count: u32) -> ContextStoreDiagnostics {
        ContextStoreDiagnostics {
            lance_db_size_bytes: dir_size_bytes(&self.root.join(".omega/store/lance")),
            tantivy_index_size_bytes: dir_size_bytes(&self.root.join(".omega/store/tantivy")),
            todo_items_count: count_jsonl_items(&self.root.join(".omega/store/todos.jsonl")),
            turn_archive_count,
            turn_archive_size_bytes: dir_size_bytes(&self.root.join(".omega/memory/turns")),
        }
    }
}

impl ContextDiagnosticsProvider for LocalDiagnostics {
    fn context_diagnostics(&self) -> ContextDiagnostics {
        let state = self.state.lock().unwrap();
        let document = ContextDocumentDiagnostics {
            index_staleness_seconds: state
                .last_scan_at
                .map(|timestamp| current_unix_timestamp().saturating_sub(timestamp))
                .unwrap_or(0),
            ..state.document.clone()
        };
        let store = self.store_diagnostics(state.memory.total_turns_archived as u32);
        ContextDiagnostics {
            budget: state.budget.clone(),
            cache: state.cache.clone(),
            memory: state.memory.clone(),
            document,
            store,
        }
    }

    fn cache_diagnostics(&self) -> Option<ContextCacheDiagnostics> {
        self.state.lock().unwrap().cache.clone()
    }

    fn record_context_assembly(
        &self,
        cache: &ContextCacheDiagnostics,
        selected_step_summaries: &[ContextStepSummary],
        total_available_summaries: usize,
    ) {
        let mut state = self.state.lock().unwrap();
        let selected_summary_tokens = selected_step_summaries
            .iter()
            .map(|summary| summary.estimated_tokens)
            .sum::<u32>();
        let selected_summary_count = selected_step_summaries.len() as u32;
        let available_summary_count = total_available_summaries as u32;
        let headroom_tokens = cache
            .budget_input_tokens
            .saturating_sub(cache.request_input_tokens);
        let usage_percent = if cache.budget_input_tokens == 0 {
            0
        } else {
            ((cache.request_input_tokens.saturating_mul(100)) / cache.budget_input_tokens).min(100)
                as u8
        };

        state.budget = ContextBudgetDiagnostics {
            budget_input_tokens: cache.budget_input_tokens,
            request_input_tokens: cache.request_input_tokens,
            headroom_tokens,
            usage_percent,
            selected_summary_count,
            available_summary_count,
        };
        state.cache = Some(cache.clone());
        state.memory.current_summary_tokens = selected_summary_tokens;
        state.memory.current_summary_count = selected_summary_count;

        if available_summary_count > 0 {
            let compression_ratio_percent = ((selected_summary_count.saturating_mul(100))
                / available_summary_count)
                .min(100) as u8;
            state.compression_ratio_total_percent = state
                .compression_ratio_total_percent
                .saturating_add(compression_ratio_percent as u64);
            state.compression_ratio_samples = state.compression_ratio_samples.saturating_add(1);
            state.memory.compression_ratio_avg_percent = (state.compression_ratio_total_percent
                / state.compression_ratio_samples.max(1))
                as u8;
        }

        if total_available_summaries > selected_step_summaries.len() {
            state.memory.compactions_triggered =
                state.memory.compactions_triggered.saturating_add(1);
            state.memory.last_compaction_at = Some(current_unix_timestamp());
        }
    }

    fn record_document_scan(&self, scan: &ScanResult) {
        let mut state = self.state.lock().unwrap();
        state.document.total_files_indexed = scan.files_indexed as u64;
        state.document.total_chunks = scan.chunks_indexed as u64;
        state.document.total_embeddings = scan.chunks_indexed as u64;
        state.document.active_version = scan.active_version.clone();
        state.document.pending_version = scan.pending_version.clone();
        state.document.last_promotion_error = None;
        push_document_activity(
            &mut state.document.recent_activity,
            "scan workspace",
            format!(
                "files={} chunks={} embeddings={}{}",
                scan.files_indexed,
                scan.chunks_indexed,
                scan.chunks_indexed,
                scan.archived_version_path
                    .as_ref()
                    .map(|path| format!(" archived={path}"))
                    .unwrap_or_default()
            ),
        );
        state.last_scan_at = Some(current_unix_timestamp());
    }

    fn record_document_health(&self, health: &DocumentHealthReport) {
        let mut state = self.state.lock().unwrap();
        state.document.governance_health = Some(health.overall_health);
        state.document.health_status = match health.overall_health {
            HealthScore::Good => DocumentHealthStatus::Good,
            HealthScore::NeedsAttention => DocumentHealthStatus::NeedsAttention,
            HealthScore::Critical => DocumentHealthStatus::Critical,
        };
        let health_status = state.document.health_status;
        state.document.last_health_check = Some(current_unix_timestamp());
        state.document.total_files_indexed = state
            .document
            .total_files_indexed
            .max(health.total_docs as u64);
        push_document_activity(
            &mut state.document.recent_activity,
            "health check",
            format!(
                "score={} issues={}",
                health_status.as_str(),
                health.structure_violations.len()
                    + health.naming_violations.len()
                    + health.orphaned_docs.len()
                    + health.broken_crossrefs.len()
                    + health.stale_docs.len()
                    + health.missing_frontmatter.len()
            ),
        );
    }

    fn record_document_usage(&self, operator: &str, source: &str, detail: &str) {
        let mut state = self.state.lock().unwrap();
        let now = current_unix_timestamp();
        if let Some(existing) = state
            .document
            .operator_usage
            .iter_mut()
            .find(|usage| usage.operator == operator && usage.source == source)
        {
            existing.count = existing.count.saturating_add(1);
            existing.last_used_at = Some(now);
        } else {
            state.document.operator_usage.push(DocumentOperatorUsage {
                operator: operator.to_string(),
                source: source.to_string(),
                count: 1,
                last_used_at: Some(now),
            });
        }
        state.document.operator_usage.sort_by(|left, right| {
            right
                .last_used_at
                .cmp(&left.last_used_at)
                .then_with(|| right.count.cmp(&left.count))
                .then_with(|| left.operator.cmp(&right.operator))
        });
        state.document.operator_usage.truncate(6);
        push_document_activity(
            &mut state.document.recent_activity,
            operator,
            format!("source={source} {detail}"),
        );
    }

    fn record_memory_snapshot(&self, snapshot: &MemoryKnowledgeSnapshot) {
        let mut state = self.state.lock().unwrap();
        state.memory.total_turns_archived = snapshot.total_turns_archived;
        state.memory.retention_candidates_accepted = snapshot.retention_candidates_accepted;
        state.memory.retention_candidates_dropped = snapshot.retention_candidates_dropped;
        state.memory.dropped_candidates_by_profile =
            snapshot.dropped_candidates_by_profile.clone();
        state.memory.memory_query_count = snapshot.memory_query_count;
        state.memory.memory_query_hit_mix = snapshot.memory_query_hit_mix.clone();
        state.memory.observation_count = snapshot.observation_count;
        state.memory.observation_fresh_count = snapshot.observation_fresh_count;
        state.memory.observation_stale_count = snapshot.observation_stale_count;
        state.memory.observation_superseded_count = snapshot.observation_superseded_count;
        state.memory.observation_corrected_count = snapshot.observation_corrected_count;
        state.memory.observation_correction_activity = snapshot.observation_correction_activity;
        state.memory.current_query = snapshot.current_query.clone();
        state.memory.current_observations = snapshot.current_observations.clone();
    }
}

fn push_document_activity(
    activity: &mut Vec<DocumentActivitySummary>,
    label: impl Into<String>,
    detail: impl Into<String>,
) {
    activity.insert(
        0,
        DocumentActivitySummary {
            label: label.into(),
            detail: detail.into(),
            at: current_unix_timestamp(),
        },
    );
    activity.truncate(6);
}

fn trim_memory_hits_for_budget(hits: Vec<MemoryQueryHit>, max_tokens: u32) -> Vec<MemoryQueryHit> {
    let mut selected = Vec::new();
    let mut used_tokens = 0_u32;
    for hit in hits {
        let cost = estimate_tokens(&format!("{}\n{}", hit.title, hit.preview));
        if !selected.is_empty() && used_tokens.saturating_add(cost) > max_tokens {
            break;
        }
        used_tokens = used_tokens.saturating_add(cost);
        selected.push(hit);
    }
    selected
}

fn trim_document_hits_for_budget(hits: Vec<SearchResult>, max_tokens: u32) -> Vec<SearchResult> {
    let mut selected = Vec::new();
    let mut used_tokens = 0_u32;
    for hit in hits {
        let cost = estimate_tokens(&format!("{}\n{}", hit.path, hit.preview));
        if !selected.is_empty() && used_tokens.saturating_add(cost) > max_tokens {
            break;
        }
        used_tokens = used_tokens.saturating_add(cost);
        selected.push(hit);
    }
    selected
}

fn document_query_filters(doc_type_hints: &[DocType]) -> Vec<SearchFilter> {
    if doc_type_hints.is_empty() {
        Vec::new()
    } else {
        vec![SearchFilter::DocType(doc_type_hints.to_vec())]
    }
}

fn trim_observations_for_budget(
    observations: Vec<ProjectObservation>,
    max_tokens: u32,
) -> Vec<ProjectObservation> {
    let mut selected = Vec::new();
    let mut used_tokens = 0_u32;
    for observation in observations {
        let cost = estimate_tokens(&format!(
            "{}\n{}",
            observation.title,
            observation.summary
        ));
        if !selected.is_empty() && used_tokens.saturating_add(cost) > max_tokens {
            break;
        }
        used_tokens = used_tokens.saturating_add(cost);
        selected.push(observation);
    }
    selected
}

fn rerank_document_hits(
    mut results: Vec<SearchResult>,
    related_paths: &BTreeSet<String>,
) -> Vec<SearchResult> {
    results.sort_by(|left, right| {
        document_relation_rank(right, related_paths)
            .cmp(&document_relation_rank(left, related_paths))
            .then_with(|| left.path.cmp(&right.path))
    });
    results
}

fn dedupe_document_hits_by_path(results: Vec<SearchResult>) -> Vec<SearchResult> {
    let mut deduped = BTreeMap::new();
    for result in results {
        deduped
            .entry(result.path.clone())
            .and_modify(|existing: &mut SearchResult| {
                if result.score > existing.score
                    || (result.score == existing.score && result.modified_at > existing.modified_at)
                {
                    *existing = result.clone();
                }
            })
            .or_insert(result);
    }
    deduped.into_values().collect()
}

fn document_relation_rank(result: &SearchResult, related_paths: &BTreeSet<String>) -> u8 {
    related_paths
        .iter()
        .any(|path| result.path == *path || result.path.starts_with(path))
        .then_some(1)
        .unwrap_or(0)
}

fn collect_related_paths(
    memory_hits: &[MemoryQueryHit],
    observations: &[ProjectObservation],
    path_hints: &[String],
) -> BTreeSet<String> {
    let mut paths = BTreeSet::new();
    for path_hint in path_hints {
        paths.insert(path_hint.clone());
    }
    for hit in memory_hits {
        for evidence in &hit.evidence_refs {
            if let omega_memory::RetentionEvidenceRef::ChangedPath { path } = evidence {
                paths.insert(path.clone());
            }
        }
    }
    for observation in observations {
        for evidence in &observation.evidence_refs {
            if evidence.kind == "changed_path" {
                paths.insert(evidence.locator.clone());
            }
        }
    }
    paths
}

fn build_recall_query_bundle(request: &StepContextRequest) -> RecallQueryBundle {
    let raw_query = request.session.latest_user_turn.trim().to_string();
    let mut keyword_queries = Vec::new();
    let mut concept_queries = Vec::new();
    let mut path_hints = extract_path_hints(&raw_query);
    let mut doc_type_hints = infer_doc_type_hints(&raw_query);

    if !raw_query.is_empty() {
        keyword_queries.push(raw_query.clone());
        if let Some(compressed) = compress_recall_query(&raw_query) {
            concept_queries.push(compressed);
        }
    }

    if let Some(item) = request.current_execute_item.as_ref() {
        if let Some(label) = item.item_label.as_deref() {
            keyword_queries.push(format!("{} {}", request.step.label, label));
            concept_queries.push(label.to_string());
            path_hints.extend(extract_path_hints(label));
            doc_type_hints.extend(infer_doc_type_hints(label));
        }
    } else if !request.step.label.trim().is_empty() {
        concept_queries.push(request.step.label.trim().to_string());
    }

    let structured_anchors = extract_structured_input_strings(request.structured_input.as_ref());
    if !structured_anchors.is_empty() {
        keyword_queries.push(structured_anchors.iter().take(3).cloned().collect::<Vec<_>>().join(" "));
        for anchor in &structured_anchors {
            path_hints.extend(extract_path_hints(anchor));
            doc_type_hints.extend(infer_doc_type_hints(anchor));
        }
    }

    if let Some(todo_snapshot) = request.todo_snapshot.as_deref() {
        let todo_anchors = extract_todo_labels(todo_snapshot);
        if !todo_anchors.is_empty() {
            concept_queries.push(todo_anchors.join(" "));
        }
    }

    let summary_anchors = extract_recent_summary_entities(&request.session.step_summaries);
    if !summary_anchors.is_empty() {
        concept_queries.push(summary_anchors.join(" "));
        for anchor in &summary_anchors {
            path_hints.extend(extract_path_hints(anchor));
        }
    }

    RecallQueryBundle {
        raw_query,
        keyword_queries: dedupe_trimmed_strings(&keyword_queries),
        concept_queries: dedupe_trimmed_strings(&concept_queries),
        path_hints: dedupe_trimmed_strings(&path_hints),
        doc_type_hints: dedupe_doc_types(&doc_type_hints),
        rewrite_reason: request.recall_rewrite_reason.clone(),
        rewrite_queries: dedupe_trimmed_strings(&request.recall_rewrite_queries),
        recovery_path: request.recall_recovery_path.clone(),
    }
}

fn compress_recall_query(text: &str) -> Option<String> {
    let anchors = text
        .split(|ch: char| ch.is_whitespace() || [',', ';', ':', '，', '。', '、'].contains(&ch))
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .filter(|part| {
            part.contains('/')
                || part.contains("::")
                || part.contains("omega-")
                || part.ends_with(".rs")
                || part.ends_with(".md")
                || part.chars().count() >= 4
                || part.chars().any(is_cjk_like)
        })
        .take(6)
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    if anchors.is_empty() {
        None
    } else {
        Some(anchors.join(" "))
    }
}

fn extract_structured_input_strings(value: Option<&Value>) -> Vec<String> {
    let Some(value) = value else {
        return Vec::new();
    };
    let mut values = Vec::new();
    collect_structured_input_strings(value, &mut values);
    dedupe_trimmed_strings(&values)
}

fn collect_structured_input_strings(value: &Value, values: &mut Vec<String>) {
    match value {
        Value::String(text) => {
            if !text.trim().is_empty() {
                values.push(text.trim().to_string());
            }
        }
        Value::Array(items) => {
            for item in items.iter().take(6) {
                collect_structured_input_strings(item, values);
            }
        }
        Value::Object(map) => {
            for (key, value) in map.iter().take(8) {
                values.push(key.trim().to_string());
                collect_structured_input_strings(value, values);
            }
        }
        _ => {}
    }
}

fn extract_todo_labels(todo_snapshot: &str) -> Vec<String> {
    todo_snapshot
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter_map(|line| {
            line.split_once(':')
                .map(|(_, label)| label.trim())
                .or_else(|| line.strip_prefix("[>] ").map(str::trim))
        })
        .filter(|label| !label.is_empty())
        .map(ToOwned::to_owned)
        .take(3)
        .collect()
}

fn extract_recent_summary_entities(step_summaries: &[ContextStepSummary]) -> Vec<String> {
    let mut counts = BTreeMap::new();
    for summary in step_summaries.iter().rev().take(4) {
        for anchor in extract_summary_anchors(summary) {
            *counts.entry(anchor).or_insert(0_u32) += 1;
        }
    }
    counts
        .into_iter()
        .filter(|(_, count)| *count > 1)
        .map(|(anchor, _)| anchor)
        .take(3)
        .collect()
}

fn extract_summary_anchors(summary: &ContextStepSummary) -> Vec<String> {
    let mut anchors = vec![summary.title.clone(), summary.workflow_id.clone()];
    anchors.extend(extract_path_hints(&summary.summary));
    if let Some(compressed) = compress_recall_query(&summary.summary) {
        anchors.push(compressed);
    }
    dedupe_trimmed_strings(&anchors)
}

fn extract_path_hints(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(|part| part.trim_matches(|ch: char| matches!(ch, ',' | ':' | ';' | ')' | '(' | '[' | ']')))
        .filter(|part| {
            part.contains('/')
                || part.contains("::")
                || part.starts_with("omega-")
                || part.starts_with("crates/")
                || part.ends_with(".rs")
                || part.ends_with(".md")
        })
        .map(ToOwned::to_owned)
        .collect()
}

fn infer_doc_type_hints(text: &str) -> Vec<DocType> {
    let normalized = text.to_lowercase();
    let mut hints = Vec::new();
    if normalized.contains("spec") {
        hints.push(DocType::Spec);
    }
    if normalized.contains("guide") {
        hints.push(DocType::Guide);
    }
    if normalized.contains("todo") || normalized.contains("task") {
        hints.push(DocType::Todo);
    }
    if normalized.contains("readme") {
        hints.push(DocType::Readme);
    }
    if normalized.contains("adr") || normalized.contains("decision") {
        hints.push(DocType::Adr);
    }
    dedupe_doc_types(&hints)
}

fn dedupe_trimmed_strings(values: &[String]) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut deduped = Vec::new();
    for value in values {
        let trimmed = value.trim();
        if trimmed.is_empty() || !seen.insert(trimmed.to_string()) {
            continue;
        }
        deduped.push(trimmed.to_string());
    }
    deduped
}

fn dedupe_doc_types(values: &[DocType]) -> Vec<DocType> {
    let mut deduped = Vec::new();
    for value in values {
        if !deduped.contains(value) {
            deduped.push(*value);
        }
    }
    deduped
}

fn is_cjk_like(ch: char) -> bool {
    matches!(
        ch,
        '\u{3400}'..='\u{4DBF}'
            | '\u{4E00}'..='\u{9FFF}'
            | '\u{3040}'..='\u{309F}'
            | '\u{30A0}'..='\u{30FF}'
            | '\u{AC00}'..='\u{D7AF}'
    )
}

fn search_mode_label(mode: SearchMode) -> &'static str {
    match mode {
        SearchMode::Keyword => "keyword",
        SearchMode::Semantic => "semantic",
        SearchMode::Hybrid => "hybrid",
    }
}

fn query_requests_recent_window(query_text: &str) -> bool {
    let lowered = query_text.to_lowercase();
    ["recent", "latest", "current", "today", "newest"]
        .iter()
        .any(|needle| lowered.contains(needle))
}

fn preview_text(text: &str, limit: usize) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= limit {
        return trimmed.to_string();
    }
    format!("{}...", trimmed.chars().take(limit.saturating_sub(3)).collect::<String>())
}

fn current_unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn dir_size_bytes(path: &Path) -> u64 {
    let Ok(metadata) = fs::metadata(path) else {
        return 0;
    };
    if metadata.is_file() {
        return metadata.len();
    }

    let Ok(entries) = fs::read_dir(path) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| dir_size_bytes(&entry.path()))
        .sum()
}

fn count_jsonl_items(path: &Path) -> u32 {
    fs::read_to_string(path)
        .ok()
        .map(|contents| {
            contents
                .lines()
                .filter(|line| !line.trim().is_empty())
                .count() as u32
        })
        .unwrap_or(0)
}

pub struct ContextToolRegistry {
    facade: Arc<OmegaContextFacade>,
}

impl ContextToolRegistry {
    pub fn new(facade: Arc<OmegaContextFacade>) -> Self {
        Self { facade }
    }

    pub fn register_tools(&self) -> Vec<Box<dyn ToolHandler>> {
        vec![
            Box::new(SearchCodebaseHandler {
                facade: Arc::clone(&self.facade),
            }),
            Box::new(ManageDocumentHandler {
                facade: Arc::clone(&self.facade),
            }),
        ]
    }
}

#[derive(Clone)]
struct SearchCodebaseHandler {
    facade: Arc<OmegaContextFacade>,
}

impl ToolHandler for SearchCodebaseHandler {
    fn name(&self) -> &str {
        "search_codebase"
    }

    fn description(&self) -> &str {
        "Search the project codebase using keyword, semantic, or hybrid retrieval with optional structured filters."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "Keyword query to search for." },
                "mode": {
                    "type": "string",
                    "enum": ["keyword", "semantic", "hybrid"],
                    "default": "hybrid"
                },
                "filters": {
                    "type": "object",
                    "properties": {
                        "language": { "type": "array", "items": { "type": "string" } },
                        "file_type": { "type": "array", "items": { "type": "string" } },
                        "doc_type": { "type": "array", "items": { "type": "string" } },
                        "path_glob": { "type": "string" },
                        "status": { "type": "array", "items": { "type": "string" } },
                        "tags": { "type": "array", "items": { "type": "string" } },
                        "modified_after": { "type": "integer" },
                        "modified_before": { "type": "integer" },
                        "min_tokens": { "type": "integer" },
                        "max_tokens": { "type": "integer" }
                    }
                },
                "max_results": { "type": "integer", "default": 10 }
            },
            "required": ["query"]
        })
    }

    fn execute(&self, input: Value) -> Result<String> {
        self.execute_v2(input).map(|result| result.output)
    }

    fn execute_v2(&self, input: Value) -> Result<ToolResult> {
        if !self.facade.document_backend_enabled {
            return Ok(ToolResult::error(
                "Error: Tool 'search_codebase' requires the optional document backend. Rebuild with feature 'document-backend' enabled.",
                ToolErrorKind::Execution,
            )
            .with_preview("document backend disabled"));
        }

        let input: SearchCodebaseInput = serde_json::from_value(input)
            .map_err(|error| anyhow::anyhow!("invalid search_codebase input: {error}"))?;
        self.facade
            .diagnostics
            .record_document_usage("search_codebase", "builtin_tool", &format!("query={}", input.query));
        let scan = self.facade.query.scan_workspace()?;
        self.facade.diagnostics.record_document_scan(&scan);
        let query = SearchQuery {
            text: Some(input.query.clone()),
            mode: input.mode.unwrap_or(SearchMode::Hybrid),
            filters: input.filters.into_filters()?,
            sort: input.sort,
            max_results: input.max_results.unwrap_or(10),
        };
        let results = self.facade.query.search(query)?;
        let output = serde_json::to_string_pretty(&results)?;
        Ok(ToolResult::success(output)
            .with_preview(format!("{} result(s) for '{}'", results.len(), input.query))
            .with_metadata(json!({
                "query": input.query,
                "mode": input.mode.unwrap_or(SearchMode::Hybrid),
                "result_count": results.len(),
                "scan": {
                    "files_indexed": scan.files_indexed,
                    "chunks_indexed": scan.chunks_indexed,
                    "deleted_marked": scan.deleted_marked,
                },
            })))
    }
}

#[derive(Clone)]
struct ManageDocumentHandler {
    facade: Arc<OmegaContextFacade>,
}

impl ToolHandler for ManageDocumentHandler {
    fn name(&self) -> &str {
        "manage_document"
    }

    fn description(&self) -> &str {
        "Check, plan, or apply staged document governance operations for project documentation."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["create", "archive", "update_metadata", "health_check", "list"]
                },
                "mode": {
                    "type": "string",
                    "enum": ["check", "plan", "apply"]
                },
                "path": { "type": "string" },
                "doc_type": { "type": "string" },
                "title": { "type": "string" },
                "content": { "type": "string" },
                "reason": { "type": "string", "enum": ["superseded", "completed_and_inactive", "structurally_outdated", "history_only"] },
                "replaced_by": { "type": "string" },
                "updates": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "kind": { "type": "string" },
                            "value": {}
                        },
                        "required": ["kind", "value"]
                    }
                },
                "status": { "type": "string" }
            },
            "required": ["action"]
        })
    }

    fn execute(&self, input: Value) -> Result<String> {
        self.execute_v2(input).map(|result| result.output)
    }

    fn execute_v2(&self, input: Value) -> Result<ToolResult> {
        if !self.facade.document_backend_enabled {
            return Ok(ToolResult::error(
                "Error: Tool 'manage_document' requires the optional document backend. Rebuild with feature 'document-backend' enabled.",
                ToolErrorKind::Execution,
            )
            .with_preview("document backend disabled"));
        }

        let input: ManageDocumentInput = serde_json::from_value(input)
            .map_err(|error| anyhow::anyhow!("invalid manage_document input: {error}"))?;
        let action = input.action.clone();
        self.facade.diagnostics.record_document_usage(
            "manage_document",
            "builtin_tool",
            &format!("action={action}"),
        );
        let scan = matches!(action.as_str(), "health_check" | "list")
            .then(|| self.facade.query.scan_workspace())
            .transpose()?;
        let op = input.into_op()?;
        let result = self.facade.governance.manage_document(op)?;
        if let Some(scan) = scan.as_ref() {
            self.facade.diagnostics.record_document_scan(scan);
        }
        if let Some(health) = result.health.as_ref() {
            self.facade.diagnostics.record_document_health(health);
        }
        let output = serde_json::to_string_pretty(&result)?;
        let tool_result = ToolResult::success(output)
            .with_preview(result.message.clone())
            .with_metadata(json!({
                "action": action,
                "ok": result.ok,
                "has_plan": result.plan.is_some(),
                "file_count": result.files.len(),
                "warning_count": result.warnings.len(),
                "health": result.health.as_ref().map(|health| json!({
                    "overall_health": health.overall_health,
                    "structure_violations": health.structure_violations.len(),
                    "naming_violations": health.naming_violations.len(),
                    "orphaned_docs": health.orphaned_docs.len(),
                    "broken_crossrefs": health.broken_crossrefs.len(),
                    "stale_docs": health.stale_docs.len(),
                    "missing_frontmatter": health.missing_frontmatter.len(),
                    "total_docs": health.total_docs,
                })),
                "scan": scan.as_ref().map(|scan| json!({
                    "files_indexed": scan.files_indexed,
                    "chunks_indexed": scan.chunks_indexed,
                    "deleted_marked": scan.deleted_marked,
                })),
            }));
        Ok(if result.ok {
            tool_result
        } else {
            tool_result.with_error_kind(ToolErrorKind::Validation)
        })
    }
}

#[derive(Debug, Deserialize)]
struct SearchCodebaseInput {
    query: String,
    #[serde(default)]
    mode: Option<SearchMode>,
    #[serde(default)]
    filters: SearchCodebaseFilters,
    #[serde(default)]
    sort: Option<SortField>,
    #[serde(default)]
    max_results: Option<usize>,
}

#[derive(Debug, Default, Deserialize)]
struct SearchCodebaseFilters {
    #[serde(default)]
    language: Vec<String>,
    #[serde(default)]
    file_type: Vec<String>,
    #[serde(default)]
    doc_type: Vec<String>,
    #[serde(default)]
    path_glob: Option<String>,
    #[serde(default)]
    status: Vec<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    modified_after: Option<u64>,
    #[serde(default)]
    modified_before: Option<u64>,
    #[serde(default)]
    min_tokens: Option<u32>,
    #[serde(default)]
    max_tokens: Option<u32>,
}

impl SearchCodebaseFilters {
    fn into_filters(self) -> Result<Vec<SearchFilter>> {
        let mut filters = Vec::new();
        if !self.language.is_empty() {
            filters.push(SearchFilter::Language(self.language));
        }
        if !self.file_type.is_empty() {
            filters.push(SearchFilter::FileType(
                self.file_type
                    .iter()
                    .map(|value| parse_file_type(value))
                    .collect::<Result<Vec<_>>>()?,
            ));
        }
        if !self.doc_type.is_empty() {
            filters.push(SearchFilter::DocType(
                self.doc_type
                    .iter()
                    .map(|value| parse_doc_type(value))
                    .collect::<Result<Vec<_>>>()?,
            ));
        }
        if let Some(path_glob) = self.path_glob {
            filters.push(SearchFilter::PathGlob(path_glob));
        }
        if !self.status.is_empty() {
            filters.push(SearchFilter::Status(
                self.status
                    .iter()
                    .map(|value| parse_file_status(value))
                    .collect::<Result<Vec<_>>>()?,
            ));
        }
        if !self.tags.is_empty() {
            filters.push(SearchFilter::Tag(self.tags));
        }
        if let Some(modified_after) = self.modified_after {
            filters.push(SearchFilter::ModifiedAfter(modified_after));
        }
        if let Some(modified_before) = self.modified_before {
            filters.push(SearchFilter::ModifiedBefore(modified_before));
        }
        if let Some(min_tokens) = self.min_tokens {
            filters.push(SearchFilter::MinTokens(min_tokens));
        }
        if let Some(max_tokens) = self.max_tokens {
            filters.push(SearchFilter::MaxTokens(max_tokens));
        }
        Ok(filters)
    }
}

#[derive(Debug, Deserialize)]
struct ManageDocumentInput {
    action: String,
    #[serde(default)]
    mode: Option<DocumentMutationMode>,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    doc_type: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    reason: Option<ArchiveTrigger>,
    #[serde(default)]
    replaced_by: Option<String>,
    #[serde(default)]
    updates: Vec<MetadataUpdateInput>,
    #[serde(default)]
    status: Option<String>,
}

impl ManageDocumentInput {
    fn into_op(self) -> Result<DocumentOp> {
        match self.action.as_str() {
            "create" => Ok(DocumentOp::Create {
                mode: self.mode.unwrap_or(DocumentMutationMode::Check),
                path: required_field(self.path, "path")?,
                doc_type: parse_doc_type(&required_field(self.doc_type, "doc_type")?)?,
                title: required_field(self.title, "title")?,
                content: required_field(self.content, "content")?,
            }),
            "archive" => Ok(DocumentOp::Archive {
                mode: self.mode.unwrap_or(DocumentMutationMode::Check),
                path: required_field(self.path, "path")?,
                reason: self.reason.unwrap_or(ArchiveTrigger::HistoryOnly),
                replaced_by: self.replaced_by,
            }),
            "update_metadata" => Ok(DocumentOp::UpdateMetadata {
                mode: self.mode.unwrap_or(DocumentMutationMode::Check),
                path: required_field(self.path, "path")?,
                updates: self
                    .updates
                    .into_iter()
                    .map(MetadataUpdateInput::into_update)
                    .collect::<Result<Vec<_>>>()?,
            }),
            "health_check" => Ok(DocumentOp::HealthCheck),
            "list" => Ok(DocumentOp::List {
                doc_type: self.doc_type.as_deref().map(parse_doc_type).transpose()?,
                status: self.status.as_deref().map(parse_file_status).transpose()?,
            }),
            other => anyhow::bail!("unsupported manage_document action '{other}'"),
        }
    }
}

#[derive(Debug, Deserialize)]
struct MetadataUpdateInput {
    kind: String,
    value: Value,
}

impl MetadataUpdateInput {
    fn into_update(self) -> Result<MetadataUpdate> {
        match self.kind.as_str() {
            "add_tag" => Ok(MetadataUpdate::AddTag(parse_string_value(self.value)?)),
            "remove_tag" => Ok(MetadataUpdate::RemoveTag(parse_string_value(self.value)?)),
            "set_status" => Ok(MetadataUpdate::SetStatus(parse_file_status(
                &parse_string_value(self.value)?,
            )?)),
            other => anyhow::bail!("unsupported metadata update kind '{other}'"),
        }
    }
}

fn required_field<T>(value: Option<T>, field: &str) -> Result<T> {
    value.ok_or_else(|| anyhow::anyhow!("missing required field '{field}'"))
}

fn parse_string_value(value: Value) -> Result<String> {
    value
        .as_str()
        .map(ToOwned::to_owned)
        .ok_or_else(|| anyhow::anyhow!("expected string value"))
}

fn parse_file_type(value: &str) -> Result<FileType> {
    match value {
        "source" => Ok(FileType::Source),
        "doc" => Ok(FileType::Doc),
        "config" => Ok(FileType::Config),
        "asset" => Ok(FileType::Asset),
        "test" => Ok(FileType::Test),
        "other" => Ok(FileType::Other),
        _ => anyhow::bail!("unsupported file_type '{value}'"),
    }
}

fn parse_doc_type(value: &str) -> Result<DocType> {
    match value {
        "spec" => Ok(DocType::Spec),
        "prd" => Ok(DocType::Prd),
        "guide" => Ok(DocType::Guide),
        "adr" => Ok(DocType::Adr),
        "todo" => Ok(DocType::Todo),
        "archive" => Ok(DocType::Archive),
        "readme" => Ok(DocType::Readme),
        "changelog" => Ok(DocType::Changelog),
        _ => anyhow::bail!("unsupported doc_type '{value}'"),
    }
}

fn parse_file_status(value: &str) -> Result<FileStatus> {
    match value {
        "active" => Ok(FileStatus::Active),
        "deleted" => Ok(FileStatus::Deleted),
        "archived" => Ok(FileStatus::Archived),
        _ => anyhow::bail!("unsupported status '{value}'"),
    }
}

fn build_step_system_blocks(
    request: &StepContextRequest,
    step_summaries: &[ContextStepSummary],
) -> Vec<SystemBlock> {
    let mut blocks = Vec::new();
    let stable_context = render_stable_session_context(&request.session);
    let stable_sections = [
        request.skill_system_prompt.clone(),
        format!("Workflow phase: {}", request.step.label),
        render_visible_tools(&request.step, &request.tool_manifests),
        stable_context,
    ]
    .into_iter()
    .filter(|section| !section.trim().is_empty())
    .collect::<Vec<_>>()
    .join("\n\n");
    if !stable_sections.trim().is_empty() {
        blocks.push(
            SystemBlock::text(stable_sections).with_cache_control(PromptCacheControl::ephemeral()),
        );
    }

    let summary_context = render_step_summaries_context(step_summaries);
    if !summary_context.trim().is_empty() {
        blocks.push(
            SystemBlock::text(summary_context).with_cache_control(PromptCacheControl::ephemeral()),
        );
    }

    let document_context = render_document_hits_context(&request.document_hits);
    if !document_context.trim().is_empty() {
        blocks.push(
            SystemBlock::text(document_context).with_cache_control(PromptCacheControl::ephemeral()),
        );
    }

    let memory_context = render_memory_hits_context(&request.memory_hits);
    if !memory_context.trim().is_empty() {
        blocks.push(
            SystemBlock::text(memory_context).with_cache_control(PromptCacheControl::ephemeral()),
        );
    }

    let observation_context = render_observation_hits_context(&request.observation_hits);
    if !observation_context.trim().is_empty() {
        blocks.push(
            SystemBlock::text(observation_context)
                .with_cache_control(PromptCacheControl::ephemeral()),
        );
    }

    let dynamic_sections = render_dynamic_step_sections(request);
    if !dynamic_sections.trim().is_empty() {
        blocks.push(SystemBlock::text(dynamic_sections));
    }

    blocks
}

fn build_output_repair_system_blocks(
    request: &StepContextRequest,
    failure: &OutputRepairFailure,
) -> Vec<SystemBlock> {
    let mut blocks = Vec::new();
    let stable_context = render_stable_session_context(&request.session);
    let stable_sections = [
        request.skill_system_prompt.clone(),
        format!(
            "Workflow phase: {} (structured output repair)",
            request.step.label
        ),
        "Visible tools: none".to_string(),
        stable_context,
    ]
    .into_iter()
    .filter(|section| !section.trim().is_empty())
    .collect::<Vec<_>>()
    .join("\n\n");
    if !stable_sections.trim().is_empty() {
        blocks.push(
            SystemBlock::text(stable_sections).with_cache_control(PromptCacheControl::ephemeral()),
        );
    }

    let summary_context = render_step_summaries_context(&request.session.step_summaries);
    if !summary_context.trim().is_empty() {
        blocks.push(
            SystemBlock::text(summary_context).with_cache_control(PromptCacheControl::ephemeral()),
        );
    }

    let mut dynamic_sections = render_dynamic_step_sections_without_prompt(request);
    dynamic_sections.push(render_output_repair_envelope(request, failure));
    let dynamic_sections = dynamic_sections
        .into_iter()
        .filter(|section| !section.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");
    if !dynamic_sections.trim().is_empty() {
        blocks.push(SystemBlock::text(dynamic_sections));
    }

    blocks
}

fn render_dynamic_step_sections(request: &StepContextRequest) -> String {
    render_dynamic_step_sections_without_prompt(request)
        .into_iter()
        .chain((!request.step_prompt.trim().is_empty()).then(|| {
            format!(
                "<workflow_prompt step_id=\"{}\" prompt_path=\"{}\">\n{}\n</workflow_prompt>",
                request.step.id,
                request.step.prompt_path.display(),
                request.step_prompt.trim_end()
            )
        }))
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn render_dynamic_step_sections_without_prompt(request: &StepContextRequest) -> Vec<String> {
    let mut sections = Vec::new();
    if let Some(structured_input) = request.structured_input.as_ref() {
        sections.push(format!(
            "<structured_input step_id=\"{}\">\n{}\n</structured_input>",
            request.step.id,
            render_structured_input(structured_input)
        ));
    }
    if let Some(todo_snapshot) = request.todo_snapshot.as_deref() {
        sections.push(format!(
            "<todo_state step_id=\"{}\">\n{}\n</todo_state>",
            request.step.id, todo_snapshot
        ));
    }
    if let Some(execute_item) = request.current_execute_item.as_ref() {
        sections.push(render_execute_item_context(execute_item));
    }
    let output_contract = render_output_contract(&request.cwd, &request.step.output_contract);
    if !output_contract.is_empty() {
        sections.push(format!(
            "<output_contract step_id=\"{}\">\n{}\n</output_contract>",
            request.step.id, output_contract
        ));
    }
    sections
}

fn render_output_repair_envelope(
    request: &StepContextRequest,
    failure: &OutputRepairFailure,
) -> String {
    let mut lines = vec![
        "mode: repair_structured_output".to_string(),
        format!("error_kind: {}", failure.error_kind),
        format!("validation_error: {}", failure.message),
        format!(
            "previous_response_preview: {}",
            failure.previous_response_preview
        ),
    ];
    if let Some(extracted_json_preview) = failure.extracted_json_preview.as_ref() {
        lines.push(format!(
            "extracted_json_preview: {}",
            extracted_json_preview
        ));
    }
    let required_contract = render_output_contract(&request.cwd, &request.step.output_contract);
    if !required_contract.is_empty() {
        lines.push("required_contract:".to_string());
        lines.extend(required_contract.lines().map(ToOwned::to_owned));
    }
    lines.push(
        "repair_rules: preserve the meaning of the previous answer when possible".to_string(),
    );
    lines.push("repair_rules: do not add prose before or after the JSON".to_string());
    lines.push(
        "repair_rules: respond with a single JSON object, not an array of objects".to_string(),
    );
    lines.push("repair_rules: if information is missing, infer only from the previous answer and existing structured_input".to_string());
    if let Some(execute_item) = request.current_execute_item.as_ref() {
        lines.push(format!(
            "repair_rules: for itemized execute, only '{}' may be newly added to completed_tasks in this repair pass",
            execute_item.item_id
        ));
        lines.push(format!(
            "repair_rules: keep future todo items open until their own execute slice runs; current item is '{}' ({}/{})",
            execute_item.item_id, execute_item.item_index, execute_item.item_total
        ));
    }
    format!(
        "<output_repair step_id=\"{}\">\n{}\n</output_repair>",
        request.step.id,
        lines.join("\n")
    )
}

fn render_execute_item_context(execute_item: &ContextExecuteItem) -> String {
    let mut lines = vec![
        format!("item_id: {}", execute_item.item_id),
        format!("item_index: {}", execute_item.item_index),
        format!("item_total: {}", execute_item.item_total),
    ];
    if let Some(item_label) = execute_item.item_label.as_deref() {
        lines.push(format!("item_label: {item_label}"));
    }
    lines.push(
        "rule: this execute slice is scoped to the current item only; do not mark future todo items complete yet".to_string(),
    );
    format!("<execute_item>\n{}\n</execute_item>", lines.join("\n"))
}

pub fn render_routing_context(routing: &ContextRouting) -> String {
    let mut lines = vec![
        format!("Workflow role: {}", routing.active_workflow_role.as_str()),
        format!("Active workflow: {}", routing.active_workflow_id),
    ];
    if let Some(scene_id) = routing.recognized_scene_id.as_deref() {
        lines.push(format!("Recognized scene: {scene_id}"));
    }
    if let Some(selected_workflow_id) = routing.selected_workflow_id.as_deref() {
        lines.push(format!("Selected workflow: {selected_workflow_id}"));
    }
    lines.join("\n")
}

pub fn render_visible_tools(step: &ContextStep, tool_manifests: &[ToolManifestMetadata]) -> String {
    if tool_manifests.is_empty() {
        return [
            "Visible tools: none".to_string(),
            "Tool strategy: no tool calls are available in this step; respond directly from the existing workflow context and user input.".to_string(),
        ]
        .join("\n");
    }

    let tool_names = tool_manifests
        .iter()
        .map(|manifest| manifest.id.as_str())
        .collect::<Vec<_>>();

    let mut lines = vec![format!("Visible tools: {}", tool_names.join(", ")), "Tool strategy:".to_string()];
    lines.push("Global:".to_string());
    lines.push("- Prefer the narrowest structured tool that fits the task and step goal.".to_string());
    lines.push("- Prefer read-only inspection or knowledge tools for exploration; switch to editing tools only when the step explicitly needs workspace changes.".to_string());
    lines.push("- Use escape-hatch tools only after the structured tools and their guidance have been ruled out.".to_string());

    let family_sections = render_family_tool_strategy(tool_manifests);
    if !family_sections.is_empty() {
        lines.push("Families:".to_string());
        lines.extend(family_sections);
    }

    let step_hints = render_step_tool_hints(step, tool_manifests);
    if !step_hints.is_empty() {
        lines.push("Step hints:".to_string());
        lines.extend(step_hints.into_iter().map(|hint| format!("- {hint}")));
    }

    lines.push("Tools:".to_string());
    lines.extend(tool_manifests.iter().flat_map(render_tool_prompt_section));

    lines.join("\n")
}

fn render_family_tool_strategy(tool_manifests: &[ToolManifestMetadata]) -> Vec<String> {
    ordered_tool_families()
        .into_iter()
        .filter_map(|family| {
            let family_tools = tool_manifests
                .iter()
                .filter(|manifest| manifest.family == family)
                .map(|manifest| manifest.id.as_str())
                .collect::<Vec<_>>();
            if family_tools.is_empty() {
                return None;
            }

            let (label, summary) = family_strategy_copy(family);
            Some(format!("- {} [{}]: {}", label, family_tools.join(", "), summary))
        })
        .collect()
}

fn render_step_tool_hints(step: &ContextStep, tool_manifests: &[ToolManifestMetadata]) -> Vec<String> {
    let families = tool_manifests
        .iter()
        .map(|manifest| manifest.family.as_str())
        .collect::<BTreeSet<_>>();
    let mut hints = Vec::new();

    match step.id.as_str() {
        "scene-recognition" | "select-workflow" => {
            hints.push("This is a routing step; decide from the conversation state first and avoid workspace exploration unless the step prompt explicitly asks for evidence.".to_string());
        }
        "plan" => {
            hints.push("Prefer synthesizing a credible next-step plan from existing context; only call tools to close specific evidence gaps.".to_string());
        }
        "execute" => {
            hints.push("Use tools to make concrete progress on the current execution slice, then verify the touched behavior before moving on.".to_string());
            if !families.contains(ToolFamily::Editing.as_str()) {
                hints.push("This execute slice is read-only at the tool layer; inspect and reason from existing evidence instead of attempting file mutation.".to_string());
            }
        }
        "report" => {
            hints.push("Prefer summarizing from completed work and existing evidence; only call tools for a last narrow verification pass.".to_string());
        }
        _ => {}
    }

    if families.contains(ToolFamily::EscapeHatch.as_str()) {
        hints.push("If bash is visible, keep it as a fallback after structured tools or tool-specific guidance stop fitting the task.".to_string());
    }

    hints
}

fn render_tool_prompt_section(manifest: &ToolManifestMetadata) -> Vec<String> {
    let mut lines = vec![format!(
        "- {} [{} | {}]: {}",
        manifest.id,
        family_strategy_copy(manifest.family).0,
        manifest.stability.as_str(),
        manifest.prompt.summary
    )];
    if let Some(rule) = manifest.prompt.when_to_use.first() {
        lines.push(format!("  use when: {rule}"));
    }
    if let Some(rule) = manifest.prompt.when_not_to_use.first() {
        lines.push(format!("  avoid when: {rule}"));
    }
    if !manifest.prompt.prefer_over.is_empty() {
        lines.push(format!("  prefer over: {}", manifest.prompt.prefer_over.join(", ")));
    }
    if !manifest.prompt.fallback_to.is_empty() {
        lines.push(format!("  fallback to: {}", manifest.prompt.fallback_to.join(", ")));
    }
    lines
}

fn ordered_tool_families() -> [ToolFamily; 8] {
    [
        ToolFamily::WorkspaceInspection,
        ToolFamily::KnowledgeAndGovernance,
        ToolFamily::Editing,
        ToolFamily::Planning,
        ToolFamily::Interaction,
        ToolFamily::WebResearch,
        ToolFamily::EscapeHatch,
        ToolFamily::Other,
    ]
}

fn family_strategy_copy(family: ToolFamily) -> (&'static str, &'static str) {
    match family {
        ToolFamily::WorkspaceInspection => (
            "Workspace inspection",
            "Use these for deterministic local inspection before reaching for shell commands.",
        ),
        ToolFamily::KnowledgeAndGovernance => (
            "Knowledge and governance",
            "Use these when ranked retrieval, repository guidance, or document-governance actions fit better than raw file reads.",
        ),
        ToolFamily::Editing => (
            "Editing",
            "Use these only when the step needs a real workspace change and a structured edit surface can express it safely.",
        ),
        ToolFamily::Planning => (
            "Planning",
            "Use these to externalize task state that the runtime should preserve across steps.",
        ),
        ToolFamily::Interaction => (
            "Interaction",
            "Use these when the workflow needs a structured exchange rather than direct workspace mutation.",
        ),
        ToolFamily::WebResearch => (
            "Web research",
            "Use these for network-backed lookup only when local workspace evidence is insufficient.",
        ),
        ToolFamily::EscapeHatch => (
            "Escape hatch",
            "Use these only after the structured tools and their guidance stop fitting the operation.",
        ),
        ToolFamily::Other => (
            "Other",
            "Use these only when their description clearly matches the task better than the named families above.",
        ),
    }
}

fn render_step_summaries(step_summaries: &[ContextStepSummary]) -> String {
    step_summaries
        .iter()
        .map(|summary| {
            format!(
                "- [{}:{}] {}\n{}",
                summary.workflow_id, summary.step_id, summary.title, summary.summary
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn render_stable_session_context(session: &ContextSession) -> String {
    let mut sections = Vec::new();
    if !session.latest_user_turn.trim().is_empty() {
        sections.push(format!(
            "<latest_user_turn>\n{}\n</latest_user_turn>",
            session.latest_user_turn.trim_end()
        ));
    }
    let routing_context = render_routing_context(&session.routing);
    if !routing_context.trim().is_empty() {
        sections.push(format!(
            "<workflow_runtime>\n{}\n</workflow_runtime>",
            routing_context.trim_end()
        ));
    }
    sections.join("\n\n")
}

fn render_step_summaries_context(step_summaries: &[ContextStepSummary]) -> String {
    if step_summaries.is_empty() {
        return String::new();
    }
    format!(
        "<step_summaries>\n{}\n</step_summaries>",
        render_step_summaries(step_summaries)
    )
}

fn render_document_hits_context(document_hits: &[SearchResult]) -> String {
    if document_hits.is_empty() {
        return String::new();
    }

    let lines = document_hits
        .iter()
        .map(|hit| format!("- {}\n{}", hit.path, preview_text(&hit.preview, 220)))
        .collect::<Vec<_>>()
        .join("\n\n");
    format!("<planner_document_hits>\n{}\n</planner_document_hits>", lines)
}

fn render_memory_hits_context(memory_hits: &[MemoryQueryHit]) -> String {
    if memory_hits.is_empty() {
        return String::new();
    }

    let lines = memory_hits
        .iter()
        .map(|hit| {
            format!(
                "- [{}] {}\n{}",
                hit.profile.as_str(),
                hit.title,
                hit.preview.trim_end()
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    format!("<archived_memory_hits>\n{}\n</archived_memory_hits>", lines)
}

fn render_observation_hits_context(observation_hits: &[ProjectObservation]) -> String {
    if observation_hits.is_empty() {
        return String::new();
    }

    let lines = observation_hits
        .iter()
        .map(|observation| {
            format!(
                "- [{}] {}\n{}",
                observation.freshness.as_str(),
                observation.title,
                preview_text(&observation.summary, 220)
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    format!("<project_observations>\n{}\n</project_observations>", lines)
}

pub fn render_structured_input(structured_input: &Value) -> String {
    serde_json::to_string_pretty(structured_input).unwrap_or_else(|_| structured_input.to_string())
}

pub fn render_output_contract(root: &Path, output_contract: &StepOutputContract) -> String {
    match output_contract {
        StepOutputContract::None => String::new(),
        StepOutputContract::Required {
            format,
            schema_path,
            max_retries,
            recovery_mode,
        } => {
            let mut lines = vec![
                "mode: required".to_string(),
                format!("format: {}", format.as_str()),
                format!("max_retries: {}", max_retries),
                format!("recovery_mode: {}", recovery_mode.as_str()),
            ];
            lines.extend(render_output_format_rules(*format));
            if let Some(schema_path) = schema_path {
                lines.push(format!("schema_path: {}", schema_path.display()));
                if let Some(schema_contract) = render_output_schema_contract(root, schema_path) {
                    lines.push("schema_json:".to_string());
                    lines.extend(schema_contract.lines().map(|line| format!("  {line}")));
                }
            }
            lines.join("\n")
        }
        StepOutputContract::Optional {
            format,
            schema_path,
            max_retries,
            recovery_mode,
        } => {
            let mut lines = vec![
                "mode: optional".to_string(),
                format!("format: {}", format.as_str()),
                format!("max_retries: {}", max_retries),
                format!("recovery_mode: {}", recovery_mode.as_str()),
            ];
            lines.extend(render_output_format_rules(*format));
            if let Some(schema_path) = schema_path {
                lines.push(format!("schema_path: {}", schema_path.display()));
                if let Some(schema_contract) = render_output_schema_contract(root, schema_path) {
                    lines.push("schema_json:".to_string());
                    lines.extend(schema_contract.lines().map(|line| format!("  {line}")));
                }
            }
            lines.join("\n")
        }
    }
}

fn render_output_schema_contract(root: &Path, schema_path: &Path) -> Option<String> {
    let path = if schema_path.is_absolute() {
        schema_path.to_path_buf()
    } else {
        root.join(schema_path)
    };
    std::fs::read_to_string(path).ok()
}

fn render_output_format_rules(format: DataFormat) -> Vec<String> {
    match format {
        DataFormat::Json => vec![
            "rules: respond with a single JSON object".to_string(),
            "rules: do not wrap the JSON in markdown fences".to_string(),
            "rules: do not add prose before or after the JSON".to_string(),
        ],
    }
}

fn build_preview_request(
    request: &StepContextRequest,
    step_summaries: &[ContextStepSummary],
) -> ChatRequest {
    ChatRequest::new(request.messages.clone())
        .with_system_blocks(build_step_system_blocks(request, step_summaries))
        .with_tools(request.tool_definitions.clone())
        .with_cache_last_assistant_turn(true)
        .with_max_tokens(request.max_output_tokens)
}

fn count_preview_request_tokens(
    token_counter: &dyn ContextTokenCounter,
    request: ChatRequest,
) -> (u32, ContextTokenCountSource) {
    match token_counter.count_request_tokens(request.clone()) {
        Ok(tokens) => (tokens, ContextTokenCountSource::ProviderCountTokens),
        Err(_) => (
            estimate_request_tokens(&request),
            ContextTokenCountSource::Estimated,
        ),
    }
}

fn cache_breakpoints_for_step(
    step_summaries: &[ContextStepSummary],
    messages: &[Message],
) -> Vec<String> {
    let mut breakpoints = vec!["tools".to_string(), "system:stable".to_string()];
    if !step_summaries.is_empty() {
        breakpoints.push("system:summaries".to_string());
    }
    if messages
        .iter()
        .any(|message| message.role == Role::Assistant)
    {
        breakpoints.push("messages:last_assistant_turn".to_string());
    }
    breakpoints
}

fn estimate_tokens(text: &str) -> u32 {
    text.chars().count().div_ceil(4) as u32
}

fn estimate_request_tokens(request: &ChatRequest) -> u32 {
    serde_json::to_string(request)
        .map(|body| estimate_tokens(&body))
        .unwrap_or_else(|_| {
            estimate_tokens(request.system.as_deref().unwrap_or_default())
                .saturating_add(
                    request
                        .system_blocks
                        .iter()
                        .map(|block| estimate_tokens(&block.text))
                        .sum::<u32>(),
                )
                .saturating_add(
                    request
                        .messages
                        .iter()
                        .map(|message| match &message.content {
                            MessageContent::Text(text) => estimate_tokens(text),
                            MessageContent::Blocks(blocks) => blocks
                                .iter()
                                .map(|block| match block {
                                    ContentBlock::Text { text } => estimate_tokens(text),
                                    ContentBlock::Thinking { thinking, .. } => {
                                        estimate_tokens(thinking)
                                    }
                                    ContentBlock::ToolUse { name, input, .. } => {
                                        estimate_tokens(name)
                                            .saturating_add(estimate_tokens(&input.to_string()))
                                    }
                                    ContentBlock::ToolResult { content, .. } => {
                                        estimate_tokens(&content.to_string())
                                    }
                                })
                                .sum::<u32>(),
                        })
                        .sum::<u32>(),
                )
                .saturating_add(
                    request
                        .tools
                        .iter()
                        .map(|tool| {
                            estimate_tokens(&tool.name)
                                .saturating_add(estimate_tokens(&tool.description))
                                .saturating_add(estimate_tokens(&tool.input_schema.to_string()))
                        })
                        .sum::<u32>(),
                )
        })
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use omega_client::{ChatRequest, ContentBlock, Message, ToolDefinition};
    use omega_tools::{ToolFamily, ToolManifestMetadata, ToolPromptProfile};
    use omega_workflow::{DataFormat, OutputRecoveryMode};

    use crate::{DocType, FileStatus, FileType, SearchMode, SearchResult};

    use super::{
        build_recall_query_bundle, dedupe_document_hits_by_path, render_output_contract,
        render_visible_tools, CompactionPolicy, ContextAssembler, ContextExecuteItem,
        ContextRouting, ContextSession, ContextStep, ContextTokenCounter,
        ContextWorkflowRole, DefaultContextAssembler, DocumentHealthReport, HealthScore,
        LocalMemoryService, MemoryService, OmegaContextFacade, OutputRepairContextRequest,
        OutputRepairFailure, RecallBudgetConfig, ScanResult, StepContextRequest, TurnData,
        TurnRetentionSignals,
    };

    struct FixedTokenCounter;

    impl ContextTokenCounter for FixedTokenCounter {
        fn count_request_tokens(&self, _request: ChatRequest) -> anyhow::Result<u32> {
            Ok(320)
        }
    }

    struct FailingTokenCounter;

    impl ContextTokenCounter for FailingTokenCounter {
        fn count_request_tokens(&self, _request: ChatRequest) -> anyhow::Result<u32> {
            anyhow::bail!("count_tokens unavailable")
        }
    }

    struct SummaryLengthTokenCounter;

    impl ContextTokenCounter for SummaryLengthTokenCounter {
        fn count_request_tokens(&self, request: ChatRequest) -> anyhow::Result<u32> {
            let summary_len = request
                .system_blocks
                .iter()
                .find(|block| block.text.contains("<step_summaries>"))
                .map(|block| block.text.chars().count() as u32)
                .unwrap_or(0);
            Ok(70 + summary_len.div_ceil(5))
        }
    }

    fn context_summary(
        workflow_id: &str,
        step_id: &str,
        title: &str,
        summary: &str,
    ) -> super::ContextStepSummary {
        super::ContextStepSummary {
            workflow_id: workflow_id.to_string(),
            step_id: step_id.to_string(),
            title: title.to_string(),
            summary: summary.to_string(),
            estimated_tokens: 0,
        }
    }

    fn step_request() -> StepContextRequest {
        StepContextRequest {
            skill_system_prompt: "Base prompt".to_string(),
            cwd: PathBuf::from("/tmp/project"),
            session: ContextSession {
                latest_user_turn: "fix this bug".to_string(),
                routing: ContextRouting {
                    recognized_scene_id: Some("feature".to_string()),
                    selected_workflow_id: Some("feature".to_string()),
                    active_workflow_id: "feature".to_string(),
                    active_workflow_role: ContextWorkflowRole::Child,
                },
                step_summaries: vec![super::ContextStepSummary {
                    workflow_id: "feature".to_string(),
                    step_id: "plan".to_string(),
                    title: "Plan".to_string(),
                    summary: "Plan summary".to_string(),
                    estimated_tokens: 10,
                }],
            },
            step: ContextStep {
                id: "execute".to_string(),
                label: "Execute".to_string(),
                prompt_path: PathBuf::from(".omega/prompt/step/execute.md"),
                input_sources: vec!["plan".to_string(), "execute".to_string()],
                output_contract: omega_workflow::StepOutputContract::Required {
                    format: DataFormat::Json,
                    schema_path: None,
                    max_retries: 1,
                    recovery_mode: OutputRecoveryMode::RepairThenRegenerate,
                },
            },
            step_prompt: "Do the work".to_string(),
            document_hits: Vec::new(),
            memory_hits: Vec::new(),
            observation_hits: Vec::new(),
            structured_input: Some(serde_json::json!({"goal": "fix"})),
            todo_snapshot: Some("[>] #task-1".to_string()),
            current_execute_item: Some(ContextExecuteItem {
                item_id: "task-1".to_string(),
                item_index: 1,
                item_total: 2,
                item_label: Some("Inspect".to_string()),
            }),
            visible_tool_names: vec!["bash".to_string()],
            tool_manifests: vec![ToolManifestMetadata::legacy(
                "bash",
                "Run commands",
                serde_json::json!({"type": "object"}),
            )
            .with_family(ToolFamily::EscapeHatch)
            .with_prompt_profile(ToolPromptProfile {
                summary: "Run a shell command when structured tools do not fit.".to_string(),
                when_to_use: vec!["the exact shell command or its output is the artifact you need".to_string()],
                when_not_to_use: vec!["a structured read or edit tool already covers the task".to_string()],
                prefer_over: vec![],
                fallback_to: vec!["read_file".to_string()],
                examples: vec![],
                anti_patterns: vec![],
            })],
            tool_definitions: vec![ToolDefinition {
                name: "bash".to_string(),
                description: "Run commands".to_string(),
                input_schema: serde_json::json!({"type": "object"}),
            }],
            messages: vec![Message::user("hello")],
            recall_rewrite_reason: None,
            recall_rewrite_queries: Vec::new(),
            recall_recovery_path: None,
            context_window: 200_000,
            max_output_tokens: 32_000,
            safety_margin_tokens: 2_000,
            report_step_id: "report".to_string(),
            execute_step_id: "execute".to_string(),
            plan_step_id: "plan".to_string(),
            scene_recognition_step_id: "scene-recognition".to_string(),
            select_workflow_step_id: "select-workflow".to_string(),
            root_workflow_id: "root".to_string(),
        }
    }

    fn tool_manifest(
        name: &str,
        family: ToolFamily,
        summary: &str,
        when_to_use: &[&str],
        when_not_to_use: &[&str],
        fallback_to: &[&str],
    ) -> ToolManifestMetadata {
        ToolManifestMetadata::legacy(name, format!("{name} description"), serde_json::json!({"type": "object"}))
            .with_family(family)
            .with_prompt_profile(ToolPromptProfile {
                summary: summary.to_string(),
                when_to_use: when_to_use.iter().map(|value| (*value).to_string()).collect(),
                when_not_to_use: when_not_to_use.iter().map(|value| (*value).to_string()).collect(),
                prefer_over: Vec::new(),
                fallback_to: fallback_to.iter().map(|value| (*value).to_string()).collect(),
                examples: Vec::new(),
                anti_patterns: Vec::new(),
            })
    }

    #[test]
    fn render_visible_tools_reports_when_no_tools_are_available() {
        let rendered = render_visible_tools(&step_request().step, &[]);

        assert!(rendered.contains("Visible tools: none"));
        assert!(rendered.contains("no tool calls are available in this step"));
    }

    #[test]
    fn render_visible_tools_uses_manifest_families_and_step_hints() {
        let request = step_request();
        let rendered = render_visible_tools(
            &request.step,
            &[
                tool_manifest(
                    "bash",
                    ToolFamily::EscapeHatch,
                    "Run shell commands as a fallback.",
                    &["the shell output itself is the artifact you need"],
                    &["structured tools already cover the operation"],
                    &["read_file"],
                ),
                tool_manifest(
                    "batch",
                    ToolFamily::WorkspaceInspection,
                    "Bundle read-only inspection calls.",
                    &["you already know several inspection calls to run in parallel"],
                    &["a single targeted inspection tool will answer the question"],
                    &[],
                ),
                tool_manifest(
                    "grep_search",
                    ToolFamily::WorkspaceInspection,
                    "Search file contents quickly.",
                    &["you need exact text matches in the workspace"],
                    &["you need ranked semantic retrieval"],
                    &["search_codebase"],
                ),
            ],
        );

        assert!(rendered.contains("Visible tools: bash, batch, grep_search"));
        assert!(rendered.contains("Families:"));
        assert!(rendered.contains("Workspace inspection [batch, grep_search]"));
        assert!(rendered.contains("Step hints:"));
        assert!(rendered.contains("This execute slice is read-only at the tool layer"));
        assert!(rendered.contains("- bash [Escape hatch | stable]: Run shell commands as a fallback."));
        assert!(rendered.contains("  fallback to: read_file"));
    }

    #[test]
    fn render_visible_tools_renders_tool_specific_manifest_guidance() {
        let request = step_request();
        let rendered = render_visible_tools(
            &request.step,
            &[
                tool_manifest(
                    "apply_patch",
                    ToolFamily::Editing,
                    "Apply a targeted patch to an existing file.",
                    &["you already know the local edit slice"],
                    &["the file does not exist yet"],
                    &["create_file"],
                ),
                tool_manifest(
                    "search_codebase",
                    ToolFamily::KnowledgeAndGovernance,
                    "Run ranked repository search.",
                    &["semantic or ranked retrieval matters more than exact grep"],
                    &["you already know the exact file and line to read"],
                    &["read_file"],
                ),
                tool_manifest(
                    "todo",
                    ToolFamily::Planning,
                    "Persist task state for the runtime.",
                    &["you need visible task-state changes across steps"],
                    &["you only need scratch notes"],
                    &[],
                ),
            ],
        );

        assert!(rendered.contains("Knowledge and governance [search_codebase]"));
        assert!(rendered.contains("Editing [apply_patch]"));
        assert!(rendered.contains("Planning [todo]"));
        assert!(rendered.contains("- apply_patch [Editing | stable]: Apply a targeted patch to an existing file."));
        assert!(rendered.contains("  avoid when: the file does not exist yet"));
        assert!(rendered.contains("- search_codebase [Knowledge and governance | stable]: Run ranked repository search."));
    }

    #[test]
    fn assembler_builds_cacheable_blocks_and_keeps_relevant_summaries() {
        let assembled = DefaultContextAssembler
            .assemble_step_context(step_request(), &FixedTokenCounter)
            .unwrap();

        assert!(!assembled.selected_step_summaries.is_empty());
        assert_eq!(assembled.cache_diagnostics.request_input_tokens, 320);
        assert!(assembled
            .system_blocks
            .iter()
            .take(2)
            .all(|block| block.cache_control.is_some()));
        assert!(assembled
            .system_blocks
            .iter()
            .any(|block| block.text.contains("<todo_state step_id=\"execute\">")));
    }

    #[test]
    fn assembler_reports_all_cache_breakpoints_when_last_assistant_turn_exists() {
        let mut request = step_request();
        request.messages = vec![
            Message::assistant(vec![ContentBlock::text("previous answer")]),
            Message::user("continue"),
        ];

        let assembled = DefaultContextAssembler
            .assemble_step_context(request, &FixedTokenCounter)
            .unwrap();

        assert_eq!(
            assembled.cache_diagnostics.cache_breakpoints,
            vec![
                "tools".to_string(),
                "system:stable".to_string(),
                "system:summaries".to_string(),
                "messages:last_assistant_turn".to_string(),
            ]
        );
    }

    #[test]
    fn assembler_falls_back_to_estimated_tokens_when_provider_count_fails() {
        let assembled = DefaultContextAssembler
            .assemble_step_context(step_request(), &FailingTokenCounter)
            .unwrap();

        assert_eq!(
            assembled.cache_diagnostics.token_count_source,
            super::ContextTokenCountSource::Estimated
        );
        assert!(assembled.cache_diagnostics.request_input_tokens > 0);
    }

    #[test]
    fn assembler_prefers_step_input_summaries_over_newer_routing_history_under_budget() {
        let mut request = step_request();
        request.context_window = 240;
        request.max_output_tokens = 100;
        request.safety_margin_tokens = 20;
        request.session.step_summaries = vec![
            context_summary(
                "feature",
                "plan",
                "Plan",
                &format!("plan-marker {}", "p".repeat(120)),
            ),
            context_summary(
                "root",
                "select-workflow",
                "Routing",
                &format!("routing-marker {}", "r".repeat(120)),
            ),
        ];

        let assembled = DefaultContextAssembler
            .assemble_step_context(request, &SummaryLengthTokenCounter)
            .unwrap();

        assert_eq!(assembled.selected_step_summaries.len(), 1);
        assert_eq!(assembled.selected_step_summaries[0].step_id, "plan");
        assert_eq!(assembled.selected_step_summaries[0].workflow_id, "feature");
    }

    #[test]
    fn assembler_compacts_low_priority_history_on_current_path_when_triggered() {
        let mut request = step_request();
        request.context_window = 300;
        request.max_output_tokens = 100;
        request.safety_margin_tokens = 20;
        let long_low_summary = format!("low-head {} low-tail", "z".repeat(520));
        request.session.step_summaries = vec![
            context_summary(
                "feature",
                "plan",
                "Plan",
                &format!("plan-head {} plan-tail", "q".repeat(80)),
            ),
            context_summary("other", "history-1", "History 1", &long_low_summary),
            context_summary("other", "history-2", "History 2", &long_low_summary),
            context_summary("other", "history-3", "History 3", &long_low_summary),
            context_summary("other", "history-4", "History 4", &long_low_summary),
            context_summary("other", "history-5", "History 5", &long_low_summary),
        ];

        let assembled = DefaultContextAssembler
            .assemble_step_context(request, &SummaryLengthTokenCounter)
            .unwrap();

        let compacted_low = assembled
            .selected_step_summaries
            .iter()
            .find(|summary| summary.workflow_id == "other")
            .expect("expected at least one low-priority summary to remain after compaction");
        assert!(compacted_low.summary.contains("..."));
        assert!(compacted_low.summary.contains("low-head"));
        assert!(compacted_low.summary.contains("low-tail"));
        assert!(compacted_low.summary.len() < long_low_summary.len());
    }

    #[test]
    fn output_repair_context_disables_tools_and_includes_repair_envelope() {
        let blocks = DefaultContextAssembler
            .assemble_output_repair_context(OutputRepairContextRequest {
                step_request: step_request(),
                failure: OutputRepairFailure {
                    error_kind: "extract_failed".to_string(),
                    message: "missing key".to_string(),
                    previous_response_preview: "prev".to_string(),
                    extracted_json_preview: None,
                },
            })
            .unwrap();

        assert!(blocks
            .iter()
            .any(|block| block.text.contains("Visible tools: none")));
        assert!(blocks
            .iter()
            .any(|block| block.text.contains("<output_repair step_id=\"execute\">")));
    }

    #[test]
    fn render_output_contract_keeps_existing_shape() {
        let rendered = render_output_contract(
            Path::new("/tmp/project"),
            &omega_workflow::StepOutputContract::Required {
                format: DataFormat::Json,
                schema_path: None,
                max_retries: 2,
                recovery_mode: OutputRecoveryMode::RepairThenRegenerate,
            },
        );

        assert!(rendered.contains("mode: required"));
        assert!(rendered.contains("format: json"));
        assert!(rendered.contains("rules: respond with a single JSON object"));
    }

    #[test]
    fn local_diagnostics_snapshot_tracks_context_and_document_metrics() {
        let facade = OmegaContextFacade::local(PathBuf::from("/tmp/omega-context-diagnostics"));
        let assembled = DefaultContextAssembler
            .assemble_step_context(step_request(), &FixedTokenCounter)
            .unwrap();

        facade.diagnostics.record_context_assembly(
            &assembled.cache_diagnostics,
            &assembled.selected_step_summaries,
            3,
        );
        facade.diagnostics.record_document_scan(&ScanResult {
            files_indexed: 12,
            chunks_indexed: 48,
            deleted_marked: 0,
            vector_ignored_files: 0,
            vector_ignored_paths: Vec::new(),
            indexed_paths: Vec::new(),
            embedded_paths: Vec::new(),
            manifest_path: ".omega/store/files.jsonl".to_string(),
            keyword_index_path: ".omega/store/tantivy".to_string(),
            active_version: None,
            pending_version: None,
            archived_version_path: None,
        });
        facade
            .diagnostics
            .record_document_health(&DocumentHealthReport {
                total_docs: 12,
                structure_violations: Vec::new(),
                naming_violations: Vec::new(),
                orphaned_docs: Vec::new(),
                broken_crossrefs: Vec::new(),
                stale_docs: Vec::new(),
                missing_frontmatter: Vec::new(),
                overall_health: HealthScore::NeedsAttention,
            });
        facade
            .memory
            .archive_turn(&TurnData {
                turn_id: 1,
                workflow_id: "feature".to_string(),
                user_intent: "archive one turn".to_string(),
                summaries: vec![context_summary("feature", "plan", "Plan", "Plan summary")],
                signals: TurnRetentionSignals {
                    changed_paths: vec!["crates/omega-context/src/lib.rs".to_string()],
                    ..TurnRetentionSignals::default()
                },
            })
            .unwrap();
        facade.sync_memory_snapshot();

        let diagnostics = facade.diagnostics.context_diagnostics();
        assert_eq!(diagnostics.budget.request_input_tokens, 320);
        assert_eq!(diagnostics.budget.selected_summary_count, 1);
        assert_eq!(diagnostics.budget.available_summary_count, 3);
        assert_eq!(diagnostics.memory.total_turns_archived, 1);
        assert_eq!(diagnostics.memory.compactions_triggered, 1);
        assert_eq!(diagnostics.document.total_files_indexed, 12);
        assert_eq!(diagnostics.document.total_chunks, 48);
        assert_eq!(
            diagnostics.document.governance_health,
            Some(HealthScore::NeedsAttention)
        );
        assert_eq!(diagnostics.cache, Some(assembled.cache_diagnostics));
    }

    #[test]
    fn recall_budget_config_defaults_when_file_missing() {
        let root = PathBuf::from("/tmp/omega-context-recall-budget-missing");

        let config = RecallBudgetConfig::load(&root);

        assert_eq!(config, RecallBudgetConfig::default());
    }

    #[test]
    fn recall_budget_config_loads_repo_override_from_model_toml() {
        let root = std::env::temp_dir().join(format!(
            "omega-context-recall-budget-{}",
            std::process::id()
        ));
        let omega_dir = root.join(".omega");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&omega_dir).unwrap();
        std::fs::write(
            omega_dir.join("model.toml"),
            "[context.recall]\nmemory_hits_tokens = 64\nobservation_hits_tokens = 48\ndocument_hits_tokens = 96\n",
        )
        .unwrap();

        let config = RecallBudgetConfig::load(&root);

        assert_eq!(
            config,
            RecallBudgetConfig {
                memory_hits_tokens: 64,
                observation_hits_tokens: 48,
                document_hits_tokens: 96,
            }
        );
    }

    #[test]
    fn recall_query_bundle_collects_step_and_structured_anchors() {
        let request = step_request();

        let bundle = build_recall_query_bundle(&request);
        let queries = bundle.planned_queries();

        assert!(!queries.is_empty());
        assert!(queries.iter().any(|query| query.contains("fix this bug")));
        assert!(queries.iter().any(|query| query.contains("Inspect")));
        assert!(queries.iter().any(|query| query.contains("goal")) || queries.iter().any(|query| query.contains("fix")));
        assert!(
            !bundle.concept_queries.is_empty()
                || !bundle.path_hints.is_empty()
                || !bundle.doc_type_hints.is_empty()
        );
    }

    #[test]
    fn dedupe_document_hits_keeps_highest_score_per_path() {
        let deduped = dedupe_document_hits_by_path(vec![
            SearchResult {
                path: "docs/specs/omega-context-management.md".to_string(),
                score: 0.2,
                preview: "older".to_string(),
                language: None,
                file_type: FileType::Doc,
                doc_type: Some(DocType::Spec),
                status: FileStatus::Active,
                modified_at: 10,
                total_tokens: 40,
                mode_used: SearchMode::Hybrid,
                degraded_from: None,
            },
            SearchResult {
                path: "docs/specs/omega-context-management.md".to_string(),
                score: 0.8,
                preview: "newer".to_string(),
                language: None,
                file_type: FileType::Doc,
                doc_type: Some(DocType::Spec),
                status: FileStatus::Active,
                modified_at: 20,
                total_tokens: 40,
                mode_used: SearchMode::Hybrid,
                degraded_from: None,
            },
        ]);

        assert_eq!(deduped.len(), 1);
        assert_eq!(deduped[0].preview, "newer");
    }

    #[test]
    fn compact_context_rewrites_old_archived_summaries() {
        let root = std::env::temp_dir().join(format!(
            "omega-context-compaction-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let memory = LocalMemoryService::new(root.clone());
        let long_summary = format!(
            "old-head {} old-tail",
            "detail ".repeat(120)
        );

        for turn_id in 1..=6 {
            memory
                .archive_turn(&TurnData {
                    turn_id,
                    workflow_id: "feature".to_string(),
                    user_intent: format!("archive turn {turn_id}"),
                    summaries: vec![context_summary(
                        "feature",
                        "plan",
                        "Plan",
                        &long_summary,
                    )],
                    signals: TurnRetentionSignals::default(),
                })
                .unwrap();
        }

        let result = memory
            .compact_context(CompactionPolicy {
                trigger: "budget_threshold".to_string(),
            })
            .unwrap();
        let turns = memory.store.get_turn_history(10).unwrap();
        let oldest = turns.iter().find(|turn| turn.turn_id == 1).unwrap();
        let newest = turns.iter().find(|turn| turn.turn_id == 6).unwrap();

        assert!(result.changed);
        assert!(oldest.summaries[0].summary.contains("..."));
        assert!(oldest.summaries[0].summary.len() < long_summary.len());
        assert_eq!(newest.summaries[0].summary, long_summary);
    }
}
