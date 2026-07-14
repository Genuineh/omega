use omega_todo::TodoItem;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileType {
    Source,
    Doc,
    Config,
    Asset,
    Test,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocType {
    Spec,
    Prd,
    Guide,
    Adr,
    Whitepaper,
    Todo,
    Archive,
    Readme,
    Changelog,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileStatus {
    Active,
    Deleted,
    Archived,
    Moved { to: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileRecord {
    pub path: String,
    pub size_bytes: u64,
    pub modified_at: u64,
    pub created_at: u64,
    pub language: Option<String>,
    pub file_type: FileType,
    pub doc_type: Option<DocType>,
    pub status: FileStatus,
    pub content_hash: String,
    pub chunk_count: u32,
    pub total_tokens: u32,
    pub tags: Vec<String>,
    #[serde(default = "default_vector_index_eligible")]
    pub vector_index_eligible: bool,
    pub last_indexed_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanResult {
    pub files_indexed: usize,
    pub chunks_indexed: usize,
    pub deleted_marked: usize,
    #[serde(default)]
    pub vector_ignored_files: usize,
    #[serde(default)]
    pub vector_ignored_paths: Vec<String>,
    #[serde(default)]
    pub indexed_paths: Vec<String>,
    #[serde(default)]
    pub embedded_paths: Vec<String>,
    pub manifest_path: String,
    pub keyword_index_path: String,
    #[serde(default)]
    pub active_version: Option<DocumentStoreVersion>,
    #[serde(default)]
    pub pending_version: Option<DocumentStoreVersion>,
    #[serde(default)]
    pub archived_version_path: Option<String>,
}

fn default_vector_index_eligible() -> bool {
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchMode {
    Keyword,
    Semantic,
    Hybrid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortField {
    Relevance,
    ModifiedDesc,
    TokensAsc,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum SearchFilter {
    Language(Vec<String>),
    FileType(Vec<FileType>),
    DocType(Vec<DocType>),
    PathGlob(String),
    ModifiedAfter(u64),
    ModifiedBefore(u64),
    Status(Vec<FileStatus>),
    Tag(Vec<String>),
    MinTokens(u32),
    MaxTokens(u32),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchQuery {
    pub text: Option<String>,
    pub mode: SearchMode,
    pub filters: Vec<SearchFilter>,
    pub sort: Option<SortField>,
    pub max_results: usize,
}

impl Default for SearchQuery {
    fn default() -> Self {
        Self {
            text: None,
            mode: SearchMode::Keyword,
            filters: Vec::new(),
            sort: None,
            max_results: 10,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchResult {
    pub path: String,
    pub score: f32,
    pub preview: String,
    pub language: Option<String>,
    pub file_type: FileType,
    pub doc_type: Option<DocType>,
    pub status: FileStatus,
    pub modified_at: u64,
    pub total_tokens: u32,
    pub mode_used: SearchMode,
    pub degraded_from: Option<SearchMode>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentMutationMode {
    Check,
    Plan,
    Apply,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArchiveTrigger {
    Superseded,
    CompletedAndInactive,
    StructurallyOutdated,
    HistoryOnly,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum MetadataUpdate {
    AddTag(String),
    RemoveTag(String),
    SetStatus(FileStatus),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DocumentMutation {
    WriteFile { path: String },
    MoveFile { from: String, to: String },
    PrependArchiveNote { path: String },
    UpdateManifest { path: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentChangePlan {
    pub primary_path: String,
    pub affected_paths: Vec<String>,
    pub validation_issues: Vec<String>,
    pub proposed_mutations: Vec<DocumentMutation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructureViolation {
    pub path: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NamingViolation {
    pub path: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrossRefIssue {
    pub path: String,
    pub target: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StaleDoc {
    pub path: String,
    pub days_since_modified: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HealthScore {
    Good,
    NeedsAttention,
    Critical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentHealthStatus {
    #[default]
    NeverChecked,
    Good,
    NeedsAttention,
    Critical,
    Failed,
}

impl DocumentHealthStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NeverChecked => "never_checked",
            Self::Good => "good",
            Self::NeedsAttention => "needs_attention",
            Self::Critical => "critical",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct DocumentStoreVersion {
    pub version_id: String,
    pub schema_version: u32,
    pub manifest_revision: u64,
    pub tantivy_revision: u64,
    pub lance_revision: Option<u64>,
    pub built_at: u64,
    pub promoted_at: Option<u64>,
    pub build_trigger: String,
    pub total_files_indexed: u64,
    pub total_chunks: u64,
    pub total_embeddings: u64,
    pub deleted_marked: u64,
    pub manifest_hash: String,
    pub storage_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct DocumentOperatorUsage {
    pub operator: String,
    pub source: String,
    pub count: u64,
    pub last_used_at: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct DocumentActivitySummary {
    pub label: String,
    pub detail: String,
    pub at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentHealthReport {
    pub total_docs: usize,
    pub structure_violations: Vec<StructureViolation>,
    pub naming_violations: Vec<NamingViolation>,
    pub orphaned_docs: Vec<String>,
    pub broken_crossrefs: Vec<CrossRefIssue>,
    pub stale_docs: Vec<StaleDoc>,
    pub missing_frontmatter: Vec<String>,
    pub overall_health: HealthScore,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructuredDocsManifest {
    pub schema_version: u32,
    pub generated_root: String,
    pub record_sets: Vec<String>,
    pub relation_store_path: String,
    pub render_state_path: String,
    pub updated_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructuredDocumentSection {
    pub section_id: String,
    pub heading: String,
    pub body_markdown: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructuredDocumentRelation {
    pub kind: String,
    pub target: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructuredDocumentRender {
    pub template: String,
    pub presentation_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructuredDocumentRecord {
    pub doc_id: String,
    pub doc_type: DocType,
    pub slug: String,
    pub title: String,
    pub status: Option<String>,
    pub owner: Option<String>,
    pub created: Option<String>,
    pub updated: Option<String>,
    pub version: Option<String>,
    pub source_path: String,
    #[serde(default)]
    pub frontmatter: std::collections::BTreeMap<String, Value>,
    #[serde(default)]
    pub sections: Vec<StructuredDocumentSection>,
    #[serde(default)]
    pub relations: Vec<StructuredDocumentRelation>,
    pub render: StructuredDocumentRender,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructuredDocRelationRecord {
    pub relation_id: String,
    pub source: String,
    pub kind: String,
    pub target: String,
    #[serde(default)]
    pub metadata: std::collections::BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructuredDocsRenderState {
    pub schema_version: u32,
    pub generated_root: String,
    pub last_rendered_at: Option<u64>,
    pub rendered_doc_ids: Vec<String>,
    pub generated_paths: Vec<String>,
    pub last_validation_ok: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructuredDocsValidationIssue {
    pub path: String,
    pub message: String,
    pub expected_preview: Option<String>,
    pub actual_preview: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructuredDocsValidationReport {
    pub ok: bool,
    pub checked_doc_ids: Vec<String>,
    pub compared_paths: Vec<String>,
    pub missing_files: Vec<String>,
    pub mismatched_files: Vec<StructuredDocsValidationIssue>,
    pub broken_relations: Vec<StructuredDocsValidationIssue>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructuredDocsExtractionReport {
    pub extracted_doc_ids: Vec<String>,
    pub extracted_paths: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DocumentOpResult {
    pub mode: Option<DocumentMutationMode>,
    pub ok: bool,
    pub message: String,
    pub plan: Option<DocumentChangePlan>,
    pub health: Option<DocumentHealthReport>,
    pub files: Vec<FileRecord>,
    pub manifest: Option<StructuredDocsManifest>,
    pub records: Vec<StructuredDocumentRecord>,
    pub relations: Vec<StructuredDocRelationRecord>,
    pub render_state: Option<StructuredDocsRenderState>,
    pub validation: Option<StructuredDocsValidationReport>,
    pub extraction: Option<StructuredDocsExtractionReport>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DocumentOp {
    Create {
        mode: DocumentMutationMode,
        path: String,
        doc_type: DocType,
        title: String,
        content: String,
    },
    Archive {
        mode: DocumentMutationMode,
        path: String,
        reason: ArchiveTrigger,
        replaced_by: Option<String>,
    },
    UpdateMetadata {
        mode: DocumentMutationMode,
        path: String,
        updates: Vec<MetadataUpdate>,
    },
    HealthCheck,
    List {
        doc_type: Option<DocType>,
        status: Option<FileStatus>,
    },
    UpsertRecord {
        mode: DocumentMutationMode,
        record: StructuredDocumentRecord,
    },
    UpsertRelation {
        mode: DocumentMutationMode,
        relation: StructuredDocRelationRecord,
    },
    RenderProjection {
        mode: DocumentMutationMode,
        doc_ids: Vec<String>,
    },
    ValidateProjection {
        doc_ids: Vec<String>,
    },
    ExtractSource {
        mode: DocumentMutationMode,
        sources: Vec<String>,
        doc_type: Option<DocType>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TodoSnapshot {
    pub saved_at: u64,
    pub items: Vec<TodoItem>,
    pub rendered: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TodoOp {
    Current,
    Replace { items: Vec<TodoItem> },
    History { limit: usize },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TodoOpResult {
    pub message: String,
    pub current: Option<TodoSnapshot>,
    pub history: Vec<TodoSnapshot>,
}
