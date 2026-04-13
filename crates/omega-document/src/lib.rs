use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use arrow_array::types::Float32Type;
use arrow_array::{
    ArrayRef, FixedSizeListArray, RecordBatch, StringArray, UInt32Array, UInt64Array,
};
use arrow_schema::{DataType, Field as ArrowField, Schema as ArrowSchema};
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
use futures_util::{future::BoxFuture, FutureExt, TryStreamExt};
use globset::{Glob, GlobSet, GlobSetBuilder};
use lancedb::index::Index as LanceIndex;
use lancedb::query::{ExecutableQuery, QueryBase};
use omega_project_layout::{
    DOC_RULES_PATH, STOREIGNORE_PATH, STORE_COMMIT_LOG_PATH as INDEX_COMMIT_LOG_PATH,
    STORE_DIR_PATH as STORE_DIR, STORE_HISTORY_DIR_PATH as STORE_HISTORY_DIR,
    STORE_LANCE_DIR_PATH as LANCE_DIR, STORE_MANIFEST_PATH as FILE_MANIFEST_PATH,
    STORE_STAGING_DIR_PATH as STORE_STAGING_DIR, STORE_TANTIVY_DIR_PATH as TANTIVY_DIR,
    STORE_TODOS_PATH as TODO_STORE_PATH, STORE_VERSION_PATH,
};
use omega_todo::{TodoItem, TodoManager};
use serde::{Deserialize, Serialize};
use tantivy::collector::TopDocs;
use tantivy::query::QueryParser;
use tantivy::schema::{Field, Schema, Value, FAST, INDEXED, STORED, STRING, TEXT};
use tantivy::{doc, Index};
use walkdir::WalkDir;

const DEFAULT_MAX_RESULTS: usize = 10;
const SEARCH_PREVIEW_LIMIT: usize = 200;
const CHUNK_TARGET_CHARS: usize = 2_000;
const ESTIMATED_TOKEN_DIVISOR: usize = 4;
const DEFAULT_STALE_THRESHOLD_DAYS: u64 = 30;
const EMBEDDING_DIMENSIONS: i32 = 384;
const LANCE_FILES_TABLE: &str = "files";
const LANCE_CHUNKS_TABLE: &str = "chunks";
const LANCE_TURNS_TABLE: &str = "turns";
const HYBRID_RRF_K: f32 = 60.0;
const STORE_SCHEMA_VERSION: u32 = 1;

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
pub struct Chunk {
    pub id: String,
    pub file_path: String,
    pub byte_range_start: u64,
    pub byte_range_end: u64,
    pub content_hash: String,
    pub estimated_tokens: u32,
    pub content_preview: String,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
struct IndexCommitLog {
    current_manifest_revision: u64,
    tantivy_revision: u64,
    lance_revision: Option<u64>,
    manifest_hash: String,
    committed_at: u64,
}

impl IndexCommitLog {
    fn lance_ready(&self) -> bool {
        self.current_manifest_revision > 0
            && self.lance_revision == Some(self.current_manifest_revision)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EmbeddingBackendKind {
    FastEmbed,
    Mock,
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
            max_results: DEFAULT_MAX_RESULTS,
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
    fn as_str(self) -> &'static str {
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
struct StoreVersionLedger {
    active_version: Option<DocumentStoreVersion>,
    pending_version: Option<DocumentStoreVersion>,
    last_error: Option<String>,
    archived_version_path: Option<String>,
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DocumentOpResult {
    pub mode: Option<DocumentMutationMode>,
    pub ok: bool,
    pub message: String,
    pub plan: Option<DocumentChangePlan>,
    pub health: Option<DocumentHealthReport>,
    pub files: Vec<FileRecord>,
    pub warnings: Vec<String>,
}

impl DocumentOpResult {}

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

#[derive(Debug, Clone)]
pub struct OmegaDocument {
    root: PathBuf,
    file_store: FileStore,
    keyword_index: KeywordIndex,
    vector_index: VectorIndex,
    todo_store: PersistentTodoStore,
    rules_path: PathBuf,
    stale_threshold_days: u64,
}

impl OmegaDocument {
    pub fn new(root: PathBuf) -> Self {
        Self {
            file_store: FileStore::new(root.clone()),
            keyword_index: KeywordIndex::new(root.clone()),
            vector_index: VectorIndex::new(root.clone()),
            todo_store: PersistentTodoStore::new(root.clone()),
            rules_path: root.join(DOC_RULES_PATH),
            stale_threshold_days: DEFAULT_STALE_THRESHOLD_DAYS,
            root,
        }
    }

    pub fn scan_workspace(&self) -> Result<ScanResult> {
        self.file_store.ensure_store_dirs()?;
        let now = unix_timestamp_now();
        let previous_records = self.file_store.load_records_map()?;
        let previous_commit = self.file_store.load_commit_log()?;
        let previous_ledger = self.file_store.load_version_ledger()?;
        let storeignore = StoreIgnoreRules::load(&self.root)?;
        let mut active_records = BTreeMap::new();
        let mut deleted_marked = 0usize;
        let mut vector_ignored_files = 0usize;
        let mut vector_ignored_paths = Vec::new();
        let mut indexed_paths = Vec::new();
        let mut embedded_paths = Vec::new();
        let mut chunks = Vec::new();

        for entry in WalkDir::new(&self.root)
            .into_iter()
            .filter_entry(|entry| should_visit_path(&self.root, entry.path()))
            .filter_map(|entry| entry.ok())
        {
            if !entry.file_type().is_file() {
                continue;
            }

            let absolute_path = entry.path();
            let relative_path = normalize_relative_path(&self.root, absolute_path)?;
            if storeignore.is_match(&relative_path) {
                vector_ignored_files += 1;
                if vector_ignored_paths.len() < 10 {
                    vector_ignored_paths.push(relative_path);
                }
                continue;
            }
            let metadata = entry.metadata().with_context(|| {
                format!("failed to read metadata for {}", absolute_path.display())
            })?;
            let content_bytes = fs::read(absolute_path)
                .with_context(|| format!("failed to read file {}", absolute_path.display()))?;
            let content_hash = blake3::hash(&content_bytes).to_hex().to_string();
            let file_type = classify_file_type(&relative_path);
            let doc_type = classify_doc_type(&relative_path);
            let language = infer_language(&relative_path);
            let (chunk_count, total_tokens, new_chunks) = if matches!(file_type, FileType::Asset) {
                (0u32, 0u32, Vec::new())
            } else {
                let text = String::from_utf8_lossy(&content_bytes);
                let file_chunks = ChunkManager::chunk_file(&relative_path, text.as_ref());
                let file_tokens = estimate_tokens(text.as_ref());
                (file_chunks.len() as u32, file_tokens, file_chunks)
            };
            if indexed_paths.len() < 10 {
                indexed_paths.push(relative_path.clone());
            }
            if chunk_count > 0 && embedded_paths.len() < 10 {
                embedded_paths.push(relative_path.clone());
            }
            chunks.extend(new_chunks);

            let existing_tags = previous_records
                .get(&relative_path)
                .map(|record| record.tags.clone())
                .unwrap_or_default();

            active_records.insert(
                relative_path.clone(),
                FileRecord {
                    path: relative_path.clone(),
                    size_bytes: metadata.len(),
                    modified_at: metadata.modified().ok().map(unix_timestamp).unwrap_or(now),
                    created_at: metadata.created().ok().map(unix_timestamp).unwrap_or(now),
                    language,
                    file_type,
                    doc_type,
                    status: status_for_relative_path(&relative_path),
                    content_hash,
                    chunk_count,
                    total_tokens,
                    tags: existing_tags,
                    vector_index_eligible: true,
                    last_indexed_at: now,
                },
            );
        }

        for (path, previous) in previous_records {
            if active_records.contains_key(&path) {
                continue;
            }
            if storeignore.is_match(&path) {
                continue;
            }
            deleted_marked += 1;
            active_records.insert(
                path.clone(),
                FileRecord {
                    path,
                    status: FileStatus::Deleted,
                    last_indexed_at: now,
                    ..previous
                },
            );
        }

        let records = active_records.into_values().collect::<Vec<_>>();
        let manifest_hash = manifest_hash_for_records(&records);
        let manifest_changed = previous_commit.manifest_hash != manifest_hash;
        let revision = if manifest_changed {
            previous_commit.current_manifest_revision.saturating_add(1)
        } else {
            previous_commit.current_manifest_revision.max(1)
        };
        let build_required = manifest_changed
            || !self.keyword_index.index_dir.exists()
            || previous_commit.tantivy_revision != revision
            || !self.vector_index.db_dir.exists()
            || previous_commit.lance_revision != Some(revision);
        let mut active_version = previous_ledger.active_version.clone();
        let mut pending_version = previous_ledger.pending_version.clone();
        let mut archived_version_path = None;

        if build_required {
            let version_id = format!("store-v{:010}-{now}", revision);
            let staged = self.file_store.staged_layout(&version_id);
            remove_path_if_exists(&staged.root)?;
            fs::create_dir_all(&staged.root)
                .with_context(|| format!("failed to create {}", staged.root.display()))?;
            FileStore::write_records_to(&staged.manifest_path, &records)?;
            KeywordIndex::rebuild_at(&staged.tantivy_dir, &records, &chunks)?;

            let staged_storage_path = relative_store_path(&self.root, &staged.root)?;
            let mut staged_version = build_store_version(
                &version_id,
                revision,
                Some(revision),
                now,
                &manifest_hash,
                &staged_storage_path,
                &records,
                chunks.len(),
                deleted_marked,
            );

            match VectorIndex::rebuild_at(&staged.lance_dir, &records, &chunks, revision) {
                Ok(()) => {
                    let commit_log = IndexCommitLog {
                        current_manifest_revision: revision,
                        tantivy_revision: revision,
                        lance_revision: Some(revision),
                        manifest_hash: manifest_hash.clone(),
                        committed_at: now,
                    };
                    FileStore::write_commit_log_to(&staged.commit_log_path, &commit_log)?;
                    if let Some(previous_active) = previous_ledger.active_version.as_ref() {
                        archived_version_path = self.file_store.archive_active_version(previous_active)?;
                    }
                    if let Some(previous_pending) = previous_ledger.pending_version.as_ref() {
                        remove_path_if_exists(&self.root.join(&previous_pending.storage_path))?;
                    }
                    staged_version.promoted_at = Some(now);
                    staged_version.storage_path = STORE_DIR.to_string();
                    active_version = Some(staged_version.clone());
                    pending_version = None;
                    self.file_store.replace_active_with_stage(
                        &staged,
                        &commit_log,
                        &StoreVersionLedger {
                            active_version: Some(staged_version),
                            pending_version: None,
                            last_error: None,
                            archived_version_path: archived_version_path.clone(),
                        },
                    )?;
                }
                Err(error) => {
                    let error_text = format!("{error:#}");
                    pending_version = Some(staged_version);
                    self.file_store.write_version_ledger(&StoreVersionLedger {
                        active_version: previous_ledger.active_version.clone(),
                        pending_version: pending_version.clone(),
                        last_error: Some(error_text),
                        archived_version_path: previous_ledger.archived_version_path.clone(),
                    })?;
                    active_version = previous_ledger.active_version.clone();
                }
            }
        } else if active_version.is_none() && previous_commit.current_manifest_revision > 0 {
            let version_id = format!("store-v{:010}-legacy", previous_commit.current_manifest_revision);
            let promoted = build_store_version(
                &version_id,
                previous_commit.current_manifest_revision,
                previous_commit.lance_revision,
                previous_commit.committed_at,
                &previous_commit.manifest_hash,
                STORE_DIR,
                &records,
                chunks.len(),
                deleted_marked,
            );
            active_version = Some(promoted.clone());
            self.file_store.write_version_ledger(&StoreVersionLedger {
                active_version: Some(promoted),
                pending_version: None,
                last_error: None,
                archived_version_path: None,
            })?;
        }

        Ok(ScanResult {
            files_indexed: records.len(),
            chunks_indexed: chunks.len(),
            deleted_marked,
            vector_ignored_files,
            vector_ignored_paths,
            indexed_paths,
            embedded_paths,
            manifest_path: FILE_MANIFEST_PATH.to_string(),
            keyword_index_path: TANTIVY_DIR.to_string(),
            active_version,
            pending_version,
            archived_version_path,
        })
    }

    pub fn search(&self, query: SearchQuery) -> Result<Vec<SearchResult>> {
        if !self.file_store.manifest_path.exists() || !self.keyword_index.index_dir.exists() {
            self.scan_workspace()?;
        }
        let records = self.file_store.load_records()?;
        let commit_log = self.file_store.load_commit_log()?;
        if query
            .text
            .as_deref()
            .is_none_or(|text| text.trim().is_empty())
        {
            return self.filter_only_results(&records, &query);
        }
        match query.mode {
            SearchMode::Keyword => self.keyword_index.search(&records, query),
            SearchMode::Semantic => {
                if commit_log.lance_ready() {
                    self.vector_index
                        .search(&records, &query)
                        .or_else(|_| self.keyword_index.search(&records, query))
                } else {
                    self.keyword_index.search(&records, query)
                }
            }
            SearchMode::Hybrid => {
                if !commit_log.lance_ready() {
                    return self.keyword_index.search(&records, query);
                }
                let semantic_results = self.vector_index.search(&records, &query)?;
                let keyword_results = self.keyword_index.search(&records, query.clone())?;
                Ok(merge_hybrid_results(
                    keyword_results,
                    semantic_results,
                    query.sort.unwrap_or(SortField::Relevance),
                    normalize_max_results(query.max_results),
                ))
            }
        }
    }

    pub fn manage_document(&self, op: DocumentOp) -> Result<DocumentOpResult> {
        match op {
            DocumentOp::Create {
                mode,
                path,
                doc_type,
                title,
                content,
            } => self.create_document(mode, &path, doc_type, &title, &content),
            DocumentOp::Archive {
                mode,
                path,
                reason,
                replaced_by,
            } => self.archive_document(mode, &path, reason, replaced_by.as_deref()),
            DocumentOp::UpdateMetadata {
                mode,
                path,
                updates,
            } => self.update_metadata(mode, &path, &updates),
            DocumentOp::HealthCheck => Ok(DocumentOpResult {
                mode: None,
                ok: true,
                message: "document health report generated".to_string(),
                plan: None,
                health: Some(self.check_document_health()?),
                files: Vec::new(),
                warnings: Vec::new(),
            }),
            DocumentOp::List { doc_type, status } => {
                self.scan_workspace()?;
                let files = self
                    .file_store
                    .load_records()?
                    .into_iter()
                    .filter(|record| {
                        doc_type.is_none_or(|doc_type| record.doc_type == Some(doc_type))
                    })
                    .filter(|record| {
                        status
                            .as_ref()
                            .is_none_or(|status| &record.status == status)
                    })
                    .collect::<Vec<_>>();
                Ok(DocumentOpResult {
                    mode: None,
                    ok: true,
                    message: format!("{} file(s) listed", files.len()),
                    plan: None,
                    health: None,
                    files,
                    warnings: Vec::new(),
                })
            }
        }
    }

    pub fn manage_todo(&self, op: TodoOp) -> Result<TodoOpResult> {
        match op {
            TodoOp::Current => Ok(TodoOpResult {
                message: "loaded current todo snapshot".to_string(),
                current: self.todo_store.current()?,
                history: Vec::new(),
            }),
            TodoOp::Replace { items } => {
                let current = Some(self.todo_store.replace(items)?);
                Ok(TodoOpResult {
                    message: "updated persistent todo snapshot".to_string(),
                    current,
                    history: Vec::new(),
                })
            }
            TodoOp::History { limit } => Ok(TodoOpResult {
                message: format!("loaded todo history (limit={limit})"),
                current: None,
                history: self.todo_store.history(limit)?,
            }),
        }
    }

    pub fn check_document_health(&self) -> Result<DocumentHealthReport> {
        self.scan_workspace()?;
        let rules = self.load_rules()?;
        let records = self.file_store.load_records()?;
        let active_docs = records
            .iter()
            .filter(|record| {
                matches!(record.file_type, FileType::Doc)
                    && matches!(record.status, FileStatus::Active | FileStatus::Archived)
            })
            .cloned()
            .collect::<Vec<_>>();
        let mut structure_violations = Vec::new();
        let mut naming_violations = Vec::new();
        let mut missing_frontmatter = Vec::new();

        for required in &rules.structure.required_files {
            if !self.root.join(required).exists() {
                structure_violations.push(StructureViolation {
                    path: required.clone(),
                    message: "required file is missing".to_string(),
                });
            }
        }

        for expected_dir in &rules.structure.expected_dirs {
            if !self.root.join(&expected_dir.path).exists() {
                structure_violations.push(StructureViolation {
                    path: expected_dir.path.clone(),
                    message: format!("expected {} directory is missing", expected_dir.purpose),
                });
            }
        }

        if rules.naming.lowercase_dirs {
            for entry in WalkDir::new(self.root.join("docs"))
                .into_iter()
                .filter_map(|entry| entry.ok())
                .filter(|entry| entry.file_type().is_dir())
            {
                let relative = normalize_relative_path(&self.root, entry.path())?;
                if relative
                    .split('/')
                    .any(|segment| segment.chars().any(|ch| ch.is_ascii_uppercase()))
                {
                    naming_violations.push(NamingViolation {
                        path: relative,
                        message: "documentation directories must use lowercase names".to_string(),
                    });
                }
            }
        }

        for record in &active_docs {
            // ADR docs use a legacy format with different frontmatter requirements
            let is_adr = record.path.starts_with("docs/decisions/") && record.path.ends_with(".md");
            let missing = if is_adr {
                get_missing_frontmatter_fields(
                    &self.root.join(&record.path),
                    &rules.adr.required_frontmatter,
                )?
            } else {
                get_missing_frontmatter_fields(
                    &self.root.join(&record.path),
                    &rules.lifecycle.required_frontmatter,
                )?
            };
            for field in missing {
                missing_frontmatter.push(format!("{}: missing '{}'", record.path, field));
            }
        }

        let readme = self.root.join("README.md");
        let readme_contents = fs::read_to_string(&readme).unwrap_or_default();
        let orphaned_docs = active_docs
            .iter()
            .filter(|record| {
                matches_readme_patterns(&rules.cross_ref.readme_must_index, &record.path)
            })
            .filter(|record| !readme_contents.contains(&record.path))
            .map(|record| record.path.clone())
            .collect::<Vec<_>>();

        let broken_crossrefs = find_broken_crossrefs(&self.root, &active_docs)?;
        let stale_docs = active_docs
            .iter()
            .filter_map(|record| {
                let age_days = now_age_days(record.modified_at);
                (age_days >= self.stale_threshold_days).then(|| StaleDoc {
                    path: record.path.clone(),
                    days_since_modified: age_days,
                })
            })
            .collect::<Vec<_>>();

        let issue_count = structure_violations.len()
            + naming_violations.len()
            + orphaned_docs.len()
            + broken_crossrefs.len()
            + stale_docs.len()
            + missing_frontmatter.len();
        let overall_health = if issue_count == 0 {
            HealthScore::Good
        } else if !structure_violations.is_empty() || !broken_crossrefs.is_empty() {
            HealthScore::Critical
        } else {
            HealthScore::NeedsAttention
        };

        Ok(DocumentHealthReport {
            total_docs: active_docs.len(),
            structure_violations,
            naming_violations,
            orphaned_docs,
            broken_crossrefs,
            stale_docs,
            missing_frontmatter,
            overall_health,
        })
    }

    fn filter_only_results(
        &self,
        records: &[FileRecord],
        query: &SearchQuery,
    ) -> Result<Vec<SearchResult>> {
        let mut filtered = records
            .iter()
            .filter(|record| matches_filters(record, &query.filters))
            .map(|record| {
                Ok(SearchResult {
                    path: record.path.clone(),
                    score: 0.0,
                    preview: file_preview(&self.root.join(&record.path))?,
                    language: record.language.clone(),
                    file_type: record.file_type,
                    doc_type: record.doc_type,
                    status: record.status.clone(),
                    modified_at: record.modified_at,
                    total_tokens: record.total_tokens,
                    mode_used: SearchMode::Keyword,
                    degraded_from: (!matches!(query.mode, SearchMode::Keyword))
                        .then_some(query.mode),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        apply_sort(&mut filtered, query.sort.unwrap_or(SortField::ModifiedDesc));
        filtered.truncate(normalize_max_results(query.max_results));
        Ok(filtered)
    }

    fn create_document(
        &self,
        mode: DocumentMutationMode,
        path: &str,
        doc_type: DocType,
        _title: &str,
        content: &str,
    ) -> Result<DocumentOpResult> {
        let rules = self.load_rules()?;
        let mut validation_issues = Vec::new();
        if self.root.join(path).exists() {
            validation_issues.push("target path already exists".to_string());
        }
        if !path_matches_doc_type(path, doc_type) {
            validation_issues.push(format!(
                "path '{path}' does not match expected location for {doc_type:?}"
            ));
        }
        if rules.naming.lowercase_dirs
            && path
                .split('/')
                .filter(|segment| !segment.contains('.'))
                .any(|segment| segment.chars().any(|ch| ch.is_ascii_uppercase()))
        {
            validation_issues
                .push("documentation directories must use lowercase names".to_string());
        }
        let missing_fields = find_missing_frontmatter_fields(content, &rules.lifecycle.required_frontmatter);
        for field in &missing_fields {
            validation_issues
                .push(format!("content is missing required frontmatter field '{field}'"));
        }
        let plan = DocumentChangePlan {
            primary_path: path.to_string(),
            affected_paths: vec![path.to_string()],
            validation_issues: validation_issues.clone(),
            proposed_mutations: vec![DocumentMutation::WriteFile {
                path: path.to_string(),
            }],
        };
        match mode {
            DocumentMutationMode::Check | DocumentMutationMode::Plan => Ok(DocumentOpResult {
                mode: Some(mode),
                ok: validation_issues.is_empty(),
                message: format!("document create {mode:?} completed"),
                plan: Some(plan),
                health: None,
                files: Vec::new(),
                warnings: Vec::new(),
            }),
            DocumentMutationMode::Apply => {
                if !validation_issues.is_empty() {
                    return Ok(DocumentOpResult {
                        mode: Some(mode),
                        ok: false,
                        message: "document create apply blocked by validation issues".to_string(),
                        plan: Some(plan),
                        health: None,
                        files: Vec::new(),
                        warnings: Vec::new(),
                    });
                }
                let target = self.root.join(path);
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent).with_context(|| {
                        format!(
                            "failed to create document parent directory {}",
                            parent.display()
                        )
                    })?;
                }
                fs::write(&target, content)
                    .with_context(|| format!("failed to write document {}", target.display()))?;
                self.scan_workspace()?;
                Ok(DocumentOpResult {
                    mode: Some(mode),
                    ok: true,
                    message: format!("created document at {path}"),
                    plan: Some(plan),
                    health: None,
                    files: vec![self
                        .file_store
                        .load_records_map()?
                        .remove(path)
                        .unwrap_or_else(|| FileRecord {
                            path: path.to_string(),
                            size_bytes: content.len() as u64,
                            modified_at: unix_timestamp_now(),
                            created_at: unix_timestamp_now(),
                            language: infer_language(path),
                            file_type: classify_file_type(path),
                            doc_type: Some(doc_type),
                            status: status_for_relative_path(path),
                            content_hash: blake3::hash(content.as_bytes()).to_hex().to_string(),
                            chunk_count: 1,
                            total_tokens: estimate_tokens(content),
                            tags: Vec::new(),
                            vector_index_eligible: true,
                            last_indexed_at: unix_timestamp_now(),
                        })],
                    warnings: Vec::new(),
                })
            }
        }
    }

    fn archive_document(
        &self,
        mode: DocumentMutationMode,
        path: &str,
        reason: ArchiveTrigger,
        replaced_by: Option<&str>,
    ) -> Result<DocumentOpResult> {
        let source = self.root.join(path);
        let filename = Path::new(path)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("archived.md");
        let target_path = format!("docs/archive/{filename}");
        let mut validation_issues = Vec::new();
        if !source.exists() {
            validation_issues.push("source document does not exist".to_string());
        }
        if path.starts_with("docs/archive/") {
            validation_issues.push("document is already archived".to_string());
        }
        let mut affected_paths = vec![path.to_string(), target_path.clone()];
        for extra in ["README.md", "docs/TODO.md"] {
            if self.root.join(extra).exists() {
                affected_paths.push(extra.to_string());
            }
        }
        let plan = DocumentChangePlan {
            primary_path: path.to_string(),
            affected_paths,
            validation_issues: validation_issues.clone(),
            proposed_mutations: vec![
                DocumentMutation::PrependArchiveNote {
                    path: path.to_string(),
                },
                DocumentMutation::MoveFile {
                    from: path.to_string(),
                    to: target_path.clone(),
                },
            ],
        };
        match mode {
            DocumentMutationMode::Check | DocumentMutationMode::Plan => Ok(DocumentOpResult {
                mode: Some(mode),
                ok: validation_issues.is_empty(),
                message: format!("document archive {mode:?} completed"),
                plan: Some(plan),
                health: None,
                files: Vec::new(),
                warnings: Vec::new(),
            }),
            DocumentMutationMode::Apply => {
                if !validation_issues.is_empty() {
                    return Ok(DocumentOpResult {
                        mode: Some(mode),
                        ok: false,
                        message: "document archive apply blocked by validation issues".to_string(),
                        plan: Some(plan),
                        health: None,
                        files: Vec::new(),
                        warnings: Vec::new(),
                    });
                }
                let target = self.root.join(&target_path);
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent).with_context(|| {
                        format!("failed to create archive directory {}", parent.display())
                    })?;
                }
                let content = fs::read_to_string(&source).with_context(|| {
                    format!("failed to read source document {}", source.display())
                })?;
                let note = archive_note(reason, replaced_by);
                fs::write(&target, format!("{note}\n\n{content}")).with_context(|| {
                    format!("failed to write archived document {}", target.display())
                })?;
                fs::remove_file(&source).with_context(|| {
                    format!("failed to remove archived source {}", source.display())
                })?;
                self.scan_workspace()?;
                Ok(DocumentOpResult {
                    mode: Some(mode),
                    ok: true,
                    message: format!("archived document {path} -> {target_path}"),
                    plan: Some(plan),
                    health: None,
                    files: self
                        .file_store
                        .load_records()?
                        .into_iter()
                        .filter(|record| record.path == target_path)
                        .collect(),
                    warnings: vec![
                        "README.md and docs/TODO.md may require follow-up link updates".to_string(),
                    ],
                })
            }
        }
    }

    fn update_metadata(
        &self,
        mode: DocumentMutationMode,
        path: &str,
        updates: &[MetadataUpdate],
    ) -> Result<DocumentOpResult> {
        self.scan_workspace()?;
        let mut records = self.file_store.load_records()?;
        let Some(record) = records.iter_mut().find(|record| record.path == path) else {
            return Ok(DocumentOpResult {
                mode: Some(mode),
                ok: false,
                message: format!("no indexed file found at {path}"),
                plan: None,
                health: None,
                files: Vec::new(),
                warnings: Vec::new(),
            });
        };
        let mut next = record.clone();
        for update in updates {
            match update {
                MetadataUpdate::AddTag(tag) => {
                    if !next.tags.iter().any(|existing| existing == tag) {
                        next.tags.push(tag.clone());
                    }
                }
                MetadataUpdate::RemoveTag(tag) => {
                    next.tags.retain(|existing| existing != tag);
                }
                MetadataUpdate::SetStatus(status) => {
                    next.status = status.clone();
                }
            }
        }
        let plan = DocumentChangePlan {
            primary_path: path.to_string(),
            affected_paths: vec![path.to_string(), FILE_MANIFEST_PATH.to_string()],
            validation_issues: Vec::new(),
            proposed_mutations: vec![DocumentMutation::UpdateManifest {
                path: path.to_string(),
            }],
        };
        if !matches!(mode, DocumentMutationMode::Apply) {
            return Ok(DocumentOpResult {
                mode: Some(mode),
                ok: true,
                message: format!("document metadata {mode:?} completed"),
                plan: Some(plan),
                health: None,
                files: vec![next],
                warnings: Vec::new(),
            });
        }
        *record = next.clone();
        self.file_store.write_records(&records)?;
        Ok(DocumentOpResult {
            mode: Some(mode),
            ok: true,
            message: format!("updated metadata for {path}"),
            plan: Some(plan),
            health: None,
            files: vec![next],
            warnings: Vec::new(),
        })
    }

    fn load_rules(&self) -> Result<DocGovernanceRules> {
        if !self.rules_path.exists() {
            return Ok(DocGovernanceRules::default());
        }
        let contents = fs::read_to_string(&self.rules_path)
            .with_context(|| format!("failed to read rules file {}", self.rules_path.display()))?;
        toml::from_str(&contents)
            .with_context(|| format!("failed to parse rules file {}", self.rules_path.display()))
    }
}

#[derive(Debug, Clone)]
pub struct FileStore {
    root: PathBuf,
    manifest_path: PathBuf,
    commit_log_path: PathBuf,
    version_ledger_path: PathBuf,
    history_dir: PathBuf,
    staging_dir: PathBuf,
}

#[derive(Debug, Clone)]
struct StoreLayout {
    root: PathBuf,
    manifest_path: PathBuf,
    commit_log_path: PathBuf,
    tantivy_dir: PathBuf,
    lance_dir: PathBuf,
}

impl FileStore {
    fn new(root: PathBuf) -> Self {
        Self {
            manifest_path: root.join(FILE_MANIFEST_PATH),
            commit_log_path: root.join(INDEX_COMMIT_LOG_PATH),
            version_ledger_path: root.join(STORE_VERSION_PATH),
            history_dir: root.join(STORE_HISTORY_DIR),
            staging_dir: root.join(STORE_STAGING_DIR),
            root,
        }
    }

    fn ensure_store_dirs(&self) -> Result<()> {
        fs::create_dir_all(self.root.join(STORE_DIR))
            .with_context(|| format!("failed to create {}", self.root.join(STORE_DIR).display()))
            .and_then(|_| {
                fs::create_dir_all(&self.history_dir)
                    .with_context(|| format!("failed to create {}", self.history_dir.display()))
            })
            .and_then(|_| {
                fs::create_dir_all(&self.staging_dir)
                    .with_context(|| format!("failed to create {}", self.staging_dir.display()))
            })
    }

    fn active_layout(&self) -> StoreLayout {
        StoreLayout {
            root: self.root.join(STORE_DIR),
            manifest_path: self.manifest_path.clone(),
            commit_log_path: self.commit_log_path.clone(),
            tantivy_dir: self.root.join(TANTIVY_DIR),
            lance_dir: self.root.join(LANCE_DIR),
        }
    }

    fn staged_layout(&self, version_id: &str) -> StoreLayout {
        let root = self.staging_dir.join(version_id);
        StoreLayout {
            manifest_path: root.join("files.jsonl"),
            commit_log_path: root.join("index-commit-log.json"),
            tantivy_dir: root.join("tantivy"),
            lance_dir: root.join("lance"),
            root,
        }
    }

    fn history_layout(&self, version_id: &str) -> StoreLayout {
        let root = self.history_dir.join(version_id);
        StoreLayout {
            manifest_path: root.join("files.jsonl"),
            commit_log_path: root.join("index-commit-log.json"),
            tantivy_dir: root.join("tantivy"),
            lance_dir: root.join("lance"),
            root,
        }
    }

    fn load_records(&self) -> Result<Vec<FileRecord>> {
        if !self.manifest_path.exists() {
            return Ok(Vec::new());
        }
        let contents = fs::read_to_string(&self.manifest_path)
            .with_context(|| format!("failed to read manifest {}", self.manifest_path.display()))?;
        contents
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| serde_json::from_str::<FileRecord>(line).context("invalid manifest line"))
            .collect()
    }

    fn load_records_map(&self) -> Result<BTreeMap<String, FileRecord>> {
        Ok(self
            .load_records()?
            .into_iter()
            .map(|record| (record.path.clone(), record))
            .collect())
    }

    fn write_records(&self, records: &[FileRecord]) -> Result<()> {
        self.ensure_store_dirs()?;
        Self::write_records_to(&self.manifest_path, records)
    }

    fn write_records_to(path: &Path, records: &[FileRecord]) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        let payload = records
            .iter()
            .map(serde_json::to_string)
            .collect::<std::result::Result<Vec<_>, _>>()?
            .join("\n");
        fs::write(path, format!("{payload}\n"))
            .with_context(|| format!("failed to write manifest {}", path.display()))
    }

    fn load_commit_log(&self) -> Result<IndexCommitLog> {
        if !self.commit_log_path.exists() {
            return Ok(IndexCommitLog::default());
        }
        let contents = fs::read_to_string(&self.commit_log_path).with_context(|| {
            format!(
                "failed to read index commit log {}",
                self.commit_log_path.display()
            )
        })?;
        serde_json::from_str(&contents).context("invalid index commit log")
    }

    fn write_commit_log(&self, log: &IndexCommitLog) -> Result<()> {
        self.ensure_store_dirs()?;
        Self::write_commit_log_to(&self.commit_log_path, log)
    }

    fn write_commit_log_to(path: &Path, log: &IndexCommitLog) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        let payload = serde_json::to_string_pretty(log)?;
        fs::write(path, payload)
            .with_context(|| format!("failed to write index commit log {}", path.display()))
    }

    fn load_version_ledger(&self) -> Result<StoreVersionLedger> {
        if !self.version_ledger_path.exists() {
            return Ok(StoreVersionLedger::default());
        }
        let contents = fs::read_to_string(&self.version_ledger_path).with_context(|| {
            format!(
                "failed to read store version ledger {}",
                self.version_ledger_path.display()
            )
        })?;
        serde_json::from_str(&contents).context("invalid store version ledger")
    }

    fn write_version_ledger(&self, ledger: &StoreVersionLedger) -> Result<()> {
        self.ensure_store_dirs()?;
        let payload = serde_json::to_string_pretty(ledger)?;
        fs::write(&self.version_ledger_path, payload).with_context(|| {
            format!(
                "failed to write store version ledger {}",
                self.version_ledger_path.display()
            )
        })
    }

    fn archive_active_version(&self, version: &DocumentStoreVersion) -> Result<Option<String>> {
        let active = self.active_layout();
        if !active.manifest_path.exists()
            && !active.commit_log_path.exists()
            && !active.tantivy_dir.exists()
            && !active.lance_dir.exists()
        {
            return Ok(None);
        }

        let archived = self.history_layout(&version.version_id);
        if archived.root.exists() {
            fs::remove_dir_all(&archived.root).with_context(|| {
                format!("failed to replace archived store {}", archived.root.display())
            })?;
        }
        fs::create_dir_all(&archived.root)
            .with_context(|| format!("failed to create {}", archived.root.display()))?;
        copy_path(&active.manifest_path, &archived.manifest_path)?;
        copy_path(&active.commit_log_path, &archived.commit_log_path)?;
        copy_path(&active.tantivy_dir, &archived.tantivy_dir)?;
        copy_path(&active.lance_dir, &archived.lance_dir)?;
        Ok(Some(relative_store_path(&self.root, &archived.root)?))
    }

    fn replace_active_with_stage(
        &self,
        staged: &StoreLayout,
        commit_log: &IndexCommitLog,
        ledger: &StoreVersionLedger,
    ) -> Result<()> {
        let active = self.active_layout();
        remove_path_if_exists(&active.manifest_path)?;
        remove_path_if_exists(&active.commit_log_path)?;
        remove_path_if_exists(&active.tantivy_dir)?;
        remove_path_if_exists(&active.lance_dir)?;

        if let Some(parent) = active.manifest_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        fs::rename(&staged.manifest_path, &active.manifest_path).with_context(|| {
            format!(
                "failed to promote staged manifest {} -> {}",
                staged.manifest_path.display(),
                active.manifest_path.display()
            )
        })?;
        fs::rename(&staged.tantivy_dir, &active.tantivy_dir).with_context(|| {
            format!(
                "failed to promote staged tantivy index {} -> {}",
                staged.tantivy_dir.display(),
                active.tantivy_dir.display()
            )
        })?;
        fs::rename(&staged.lance_dir, &active.lance_dir).with_context(|| {
            format!(
                "failed to promote staged lance index {} -> {}",
                staged.lance_dir.display(),
                active.lance_dir.display()
            )
        })?;
        Self::write_commit_log_to(&active.commit_log_path, commit_log)?;
        self.write_version_ledger(ledger)?;
        remove_path_if_exists(&staged.commit_log_path)?;
        remove_path_if_exists(&staged.root)?;
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct PersistentTodoStore {
    path: PathBuf,
}

impl PersistentTodoStore {
    fn new(root: PathBuf) -> Self {
        Self {
            path: root.join(TODO_STORE_PATH),
        }
    }

    fn current(&self) -> Result<Option<TodoSnapshot>> {
        Ok(self.history(1)?.into_iter().next())
    }

    fn replace(&self, items: Vec<TodoItem>) -> Result<TodoSnapshot> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create todo store dir {}", parent.display()))?;
        }
        let mut manager = TodoManager::new();
        let rendered = manager.update(items.clone())?;
        let snapshot = TodoSnapshot {
            saved_at: unix_timestamp_now(),
            items,
            rendered,
        };
        let line = serde_json::to_string(&snapshot)?;
        let mut existing = if self.path.exists() {
            fs::read_to_string(&self.path)
                .with_context(|| format!("failed to read todo store {}", self.path.display()))?
        } else {
            String::new()
        };
        existing.push_str(&line);
        existing.push('\n');
        fs::write(&self.path, existing)
            .with_context(|| format!("failed to write todo store {}", self.path.display()))?;
        Ok(snapshot)
    }

    fn history(&self, limit: usize) -> Result<Vec<TodoSnapshot>> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }
        let contents = fs::read_to_string(&self.path)
            .with_context(|| format!("failed to read todo store {}", self.path.display()))?;
        let mut snapshots = contents
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| {
                serde_json::from_str::<TodoSnapshot>(line).context("invalid todo snapshot line")
            })
            .collect::<Result<Vec<_>>>()?;
        snapshots.reverse();
        snapshots.truncate(limit);
        Ok(snapshots)
    }
}

#[derive(Debug, Clone)]
struct KeywordIndex {
    index_dir: PathBuf,
}

impl KeywordIndex {
    fn new(root: PathBuf) -> Self {
        Self {
            index_dir: root.join(TANTIVY_DIR),
        }
    }

    fn rebuild(&self, records: &[FileRecord], chunks: &[Chunk]) -> Result<()> {
        Self::rebuild_at(&self.index_dir, records, chunks)
    }

    fn rebuild_at(index_dir: &Path, records: &[FileRecord], chunks: &[Chunk]) -> Result<()> {
        if index_dir.exists() {
            fs::remove_dir_all(index_dir)
                .with_context(|| format!("failed to clear index dir {}", index_dir.display()))?;
        }
        fs::create_dir_all(index_dir)
            .with_context(|| format!("failed to create index dir {}", index_dir.display()))?;
        let schema = keyword_schema();
        let fields = KeywordFields::new(&schema)?;
        let index = Index::create_in_dir(index_dir, schema.clone())?;
        let mut writer = index.writer(20_000_000)?;

        let records_by_path = records
            .iter()
            .map(|record| (record.path.clone(), record))
            .collect::<BTreeMap<_, _>>();

        for chunk in chunks {
            let Some(record) = records_by_path.get(&chunk.file_path) else {
                continue;
            };
            if matches!(record.status, FileStatus::Deleted) {
                continue;
            }
            let _ = writer.add_document(doc!(
                fields.path => record.path.clone(),
                fields.content => chunk.content_preview.clone(),
                fields.preview => chunk.content_preview.clone(),
                fields.language => record.language.clone().unwrap_or_default(),
                fields.file_type => format_file_type(record.file_type),
                fields.doc_type => record.doc_type.map(format_doc_type).unwrap_or_default(),
                fields.status => format_file_status(&record.status),
                fields.modified_at => record.modified_at,
                fields.total_tokens => record.total_tokens as u64,
            ));
        }

        writer.commit()?;
        Ok(())
    }

    fn search(&self, records: &[FileRecord], query: SearchQuery) -> Result<Vec<SearchResult>> {
        let mode_used = SearchMode::Keyword;
        let degraded_from = (!matches!(query.mode, SearchMode::Keyword)).then_some(query.mode);
        let index = Index::open_in_dir(&self.index_dir)
            .with_context(|| format!("failed to open index {}", self.index_dir.display()))?;
        let schema = index.schema();
        let fields = KeywordFields::new(&schema)?;
        let reader = index.reader()?;
        let searcher = reader.searcher();
        let text = query.text.clone().unwrap_or_default();
        let parser = QueryParser::for_index(&index, vec![fields.content, fields.path]);
        let tantivy_query = parser.parse_query(text.trim())?;
        let fetch_limit = normalize_max_results(query.max_results)
            .saturating_mul(5)
            .max(10);
        let top_docs = searcher.search(&tantivy_query, &TopDocs::with_limit(fetch_limit))?;
        let records_by_path = records
            .iter()
            .map(|record| (record.path.clone(), record))
            .collect::<BTreeMap<_, _>>();
        let mut results = Vec::new();
        let mut seen_paths = BTreeSet::new();

        for (score, address) in top_docs {
            let retrieved: tantivy::TantivyDocument = searcher.doc(address)?;
            let Some(path) = retrieved
                .get_first(fields.path)
                .and_then(|value| value.as_str())
            else {
                continue;
            };
            if !seen_paths.insert(path.to_string()) {
                continue;
            }
            let Some(record) = records_by_path.get(path) else {
                continue;
            };
            if !matches_filters(record, &query.filters) {
                continue;
            }
            let preview = retrieved
                .get_first(fields.preview)
                .and_then(|value| value.as_str())
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| file_preview_fallback(record));
            results.push(SearchResult {
                path: record.path.clone(),
                score,
                preview,
                language: record.language.clone(),
                file_type: record.file_type,
                doc_type: record.doc_type,
                status: record.status.clone(),
                modified_at: record.modified_at,
                total_tokens: record.total_tokens,
                mode_used,
                degraded_from,
            });
        }

        apply_sort(&mut results, query.sort.unwrap_or(SortField::Relevance));
        results.truncate(normalize_max_results(query.max_results));
        Ok(results)
    }
}

#[derive(Debug, Clone)]
struct KeywordFields {
    path: Field,
    content: Field,
    preview: Field,
    language: Field,
    file_type: Field,
    doc_type: Field,
    status: Field,
    modified_at: Field,
    total_tokens: Field,
}

impl KeywordFields {
    fn new(schema: &Schema) -> Result<Self> {
        Ok(Self {
            path: schema.get_field("path")?,
            content: schema.get_field("content")?,
            preview: schema.get_field("preview")?,
            language: schema.get_field("language")?,
            file_type: schema.get_field("file_type")?,
            doc_type: schema.get_field("doc_type")?,
            status: schema.get_field("status")?,
            modified_at: schema.get_field("modified_at")?,
            total_tokens: schema.get_field("total_tokens")?,
        })
    }
}

fn keyword_schema() -> Schema {
    let mut builder = Schema::builder();
    builder.add_text_field("path", STRING | STORED);
    builder.add_text_field("content", TEXT | STORED);
    builder.add_text_field("preview", STORED);
    builder.add_text_field("language", STRING | STORED);
    builder.add_text_field("file_type", STRING | STORED);
    builder.add_text_field("doc_type", STRING | STORED);
    builder.add_text_field("status", STRING | STORED);
    builder.add_u64_field("modified_at", INDEXED | STORED | FAST);
    builder.add_u64_field("total_tokens", INDEXED | STORED | FAST);
    builder.build()
}

#[derive(Debug, Clone)]
struct VectorIndex {
    db_dir: PathBuf,
}

impl VectorIndex {
    fn new(root: PathBuf) -> Self {
        Self {
            db_dir: root.join(LANCE_DIR),
        }
    }

    fn rebuild(&self, records: &[FileRecord], chunks: &[Chunk], _revision: u64) -> Result<()> {
        Self::rebuild_at(&self.db_dir, records, chunks, _revision)
    }

    fn rebuild_at(db_dir: &Path, records: &[FileRecord], chunks: &[Chunk], _revision: u64) -> Result<()> {
        fs::create_dir_all(db_dir).with_context(|| {
            format!("failed to create vector index dir {}", db_dir.display())
        })?;
        let chunk_rows = build_lance_chunk_rows(records, chunks)?;
        let file_rows = build_lance_file_rows(records, &chunk_rows);
        let file_batch = build_lance_file_batch(&file_rows)?;
        let chunk_batch = build_lance_chunk_batch(&chunk_rows)?;
        let files_schema = lance_file_schema();
        let chunks_schema = lance_chunk_schema();
        let turns_schema = lance_turn_schema();
        let db_dir = db_dir.to_path_buf();

        run_async_operation(move || {
            async move {
                let db_uri = db_dir.to_string_lossy().to_string();
                let db = lancedb::connect(&db_uri)
                    .execute()
                    .await
                    .context("failed to connect to LanceDB")?;
                let root_namespace: Vec<String> = Vec::new();
                let _ = db.drop_table(LANCE_FILES_TABLE, &root_namespace).await;
                let _ = db.drop_table(LANCE_CHUNKS_TABLE, &root_namespace).await;
                let _ = db.drop_table(LANCE_TURNS_TABLE, &root_namespace).await;

                if file_rows.is_empty() {
                    db.create_empty_table(LANCE_FILES_TABLE, files_schema)
                        .execute()
                        .await
                        .context("failed to create empty LanceDB files table")?;
                } else {
                    let table = db
                        .create_table(LANCE_FILES_TABLE, file_batch)
                        .execute()
                        .await
                        .context("failed to create LanceDB files table")?;
                    let _ = table
                        .create_index(&["embedding"], LanceIndex::Auto)
                        .execute()
                        .await;
                }

                if chunk_rows.is_empty() {
                    db.create_empty_table(LANCE_CHUNKS_TABLE, chunks_schema)
                        .execute()
                        .await
                        .context("failed to create empty LanceDB chunks table")?;
                } else {
                    let table = db
                        .create_table(LANCE_CHUNKS_TABLE, chunk_batch)
                        .execute()
                        .await
                        .context("failed to create LanceDB chunks table")?;
                    let _ = table
                        .create_index(&["embedding"], LanceIndex::Auto)
                        .execute()
                        .await;
                }

                db.create_empty_table(LANCE_TURNS_TABLE, turns_schema)
                    .execute()
                    .await
                    .context("failed to create empty LanceDB turns table")?;

                Ok(())
            }
            .boxed()
        })
    }

    fn search(&self, records: &[FileRecord], query: &SearchQuery) -> Result<Vec<SearchResult>> {
        let text = query.text.clone().unwrap_or_default();
        let query_embedding = embed_query(&text)?;
        let fetch_limit = normalize_max_results(query.max_results)
            .saturating_mul(8)
            .max(16);
        let db_dir = self.db_dir.clone();
        let semantic_hits = run_async_operation(move || {
            async move {
                let db_uri = db_dir.to_string_lossy().to_string();
                let db = lancedb::connect(&db_uri)
                    .execute()
                    .await
                    .context("failed to connect to LanceDB")?;
                let table = db
                    .open_table(LANCE_CHUNKS_TABLE)
                    .execute()
                    .await
                    .context("failed to open LanceDB chunks table")?;
                let stream = table
                    .query()
                    .nearest_to(query_embedding.as_slice())?
                    .limit(fetch_limit)
                    .execute()
                    .await
                    .context("failed to execute LanceDB vector search")?;
                let batches = stream
                    .try_collect::<Vec<RecordBatch>>()
                    .await
                    .context("failed to collect LanceDB search batches")?;
                semantic_hits_from_batches(&batches)
            }
            .boxed()
        })?;

        let records_by_path = records
            .iter()
            .map(|record| (record.path.clone(), record))
            .collect::<BTreeMap<_, _>>();
        let mut results = Vec::new();
        let mut seen_paths = BTreeSet::new();
        for (rank, hit) in semantic_hits.into_iter().enumerate() {
            if !seen_paths.insert(hit.file_path.clone()) {
                continue;
            }
            let Some(record) = records_by_path.get(&hit.file_path) else {
                continue;
            };
            if !record.vector_index_eligible {
                continue;
            }
            if !matches_filters(record, &query.filters) {
                continue;
            }
            results.push(SearchResult {
                path: record.path.clone(),
                score: reciprocal_rank(rank),
                preview: hit.content_preview,
                language: record.language.clone(),
                file_type: record.file_type,
                doc_type: record.doc_type,
                status: record.status.clone(),
                modified_at: record.modified_at,
                total_tokens: record.total_tokens,
                mode_used: SearchMode::Semantic,
                degraded_from: None,
            });
        }

        apply_sort(&mut results, query.sort.unwrap_or(SortField::Relevance));
        results.truncate(normalize_max_results(query.max_results));
        Ok(results)
    }
}

#[derive(Debug, Clone)]
struct LanceFileRow {
    path: String,
    size_bytes: u64,
    modified_at: u64,
    language: String,
    file_type: String,
    doc_type: String,
    status: String,
    content_hash: String,
    total_tokens: u32,
    tags_json: String,
    embedding: Vec<f32>,
}

#[derive(Debug, Clone)]
struct LanceChunkRow {
    chunk_id: String,
    file_path: String,
    byte_range_start: u64,
    byte_range_end: u64,
    content_hash: String,
    estimated_tokens: u32,
    embedding: Vec<f32>,
    content_preview: String,
}

#[derive(Debug, Clone)]
struct SemanticHit {
    file_path: String,
    content_preview: String,
}

fn build_lance_chunk_rows(records: &[FileRecord], chunks: &[Chunk]) -> Result<Vec<LanceChunkRow>> {
    let eligible_paths = records
        .iter()
        .filter(|record| record.vector_index_eligible)
        .filter(|record| !matches!(record.status, FileStatus::Deleted))
        .map(|record| record.path.as_str())
        .collect::<BTreeSet<_>>();
    let eligible_chunks = chunks
        .iter()
        .filter(|chunk| eligible_paths.contains(chunk.file_path.as_str()))
        .collect::<Vec<_>>();
    if eligible_chunks.is_empty() {
        return Ok(Vec::new());
    }
    let texts = eligible_chunks
        .iter()
        .map(|chunk| format!("{}\n{}", chunk.file_path, chunk.content_preview))
        .collect::<Vec<_>>();
    let embeddings = embed_passages(&texts)?;
    Ok(eligible_chunks
        .iter()
        .zip(embeddings)
        .map(|(chunk, embedding)| LanceChunkRow {
            chunk_id: chunk.id.clone(),
            file_path: chunk.file_path.clone(),
            byte_range_start: chunk.byte_range_start,
            byte_range_end: chunk.byte_range_end,
            content_hash: chunk.content_hash.clone(),
            estimated_tokens: chunk.estimated_tokens,
            embedding,
            content_preview: chunk.content_preview.clone(),
        })
        .collect())
}

fn build_lance_file_rows(
    records: &[FileRecord],
    chunk_rows: &[LanceChunkRow],
) -> Vec<LanceFileRow> {
    let mut sums = BTreeMap::<String, (Vec<f32>, usize)>::new();
    for chunk in chunk_rows {
        let entry = sums
            .entry(chunk.file_path.clone())
            .or_insert_with(|| (vec![0.0; EMBEDDING_DIMENSIONS as usize], 0));
        for (slot, value) in entry.0.iter_mut().zip(chunk.embedding.iter()) {
            *slot += *value;
        }
        entry.1 += 1;
    }

    records
        .iter()
        .filter(|record| !matches!(record.status, FileStatus::Deleted))
        .filter(|record| record.vector_index_eligible)
        .map(|record| {
            let embedding = sums
                .get(&record.path)
                .map(|(sum, count)| average_embedding(sum, *count))
                .unwrap_or_else(zero_embedding);
            LanceFileRow {
                path: record.path.clone(),
                size_bytes: record.size_bytes,
                modified_at: record.modified_at,
                language: record.language.clone().unwrap_or_default(),
                file_type: format_file_type(record.file_type),
                doc_type: record.doc_type.map(format_doc_type).unwrap_or_default(),
                status: format_file_status(&record.status),
                content_hash: record.content_hash.clone(),
                total_tokens: record.total_tokens,
                tags_json: serde_json::to_string(&record.tags).unwrap_or_else(|_| "[]".to_string()),
                embedding,
            }
        })
        .collect()
}

fn build_lance_file_batch(rows: &[LanceFileRow]) -> Result<RecordBatch> {
    RecordBatch::try_new(
        lance_file_schema(),
        vec![
            Arc::new(StringArray::from(
                rows.iter().map(|row| row.path.clone()).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt64Array::from(
                rows.iter().map(|row| row.size_bytes).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt64Array::from(
                rows.iter().map(|row| row.modified_at).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                rows.iter()
                    .map(|row| row.language.clone())
                    .collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                rows.iter()
                    .map(|row| row.file_type.clone())
                    .collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                rows.iter()
                    .map(|row| row.doc_type.clone())
                    .collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                rows.iter()
                    .map(|row| row.status.clone())
                    .collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                rows.iter()
                    .map(|row| row.content_hash.clone())
                    .collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt32Array::from(
                rows.iter().map(|row| row.total_tokens).collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                rows.iter()
                    .map(|row| row.tags_json.clone())
                    .collect::<Vec<_>>(),
            )) as ArrayRef,
            vector_array(
                rows.iter()
                    .map(|row| row.embedding.clone())
                    .collect::<Vec<_>>(),
            ),
        ],
    )
    .context("failed to build LanceDB file batch")
}

fn build_lance_chunk_batch(rows: &[LanceChunkRow]) -> Result<RecordBatch> {
    RecordBatch::try_new(
        lance_chunk_schema(),
        vec![
            Arc::new(StringArray::from(
                rows.iter()
                    .map(|row| row.chunk_id.clone())
                    .collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                rows.iter()
                    .map(|row| row.file_path.clone())
                    .collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt64Array::from(
                rows.iter()
                    .map(|row| row.byte_range_start)
                    .collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt64Array::from(
                rows.iter()
                    .map(|row| row.byte_range_end)
                    .collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(StringArray::from(
                rows.iter()
                    .map(|row| row.content_hash.clone())
                    .collect::<Vec<_>>(),
            )) as ArrayRef,
            Arc::new(UInt32Array::from(
                rows.iter()
                    .map(|row| row.estimated_tokens)
                    .collect::<Vec<_>>(),
            )) as ArrayRef,
            vector_array(
                rows.iter()
                    .map(|row| row.embedding.clone())
                    .collect::<Vec<_>>(),
            ),
            Arc::new(StringArray::from(
                rows.iter()
                    .map(|row| row.content_preview.clone())
                    .collect::<Vec<_>>(),
            )) as ArrayRef,
        ],
    )
    .context("failed to build LanceDB chunk batch")
}

fn lance_file_schema() -> Arc<ArrowSchema> {
    Arc::new(ArrowSchema::new(vec![
        ArrowField::new("path", DataType::Utf8, false),
        ArrowField::new("size_bytes", DataType::UInt64, false),
        ArrowField::new("modified_at", DataType::UInt64, false),
        ArrowField::new("language", DataType::Utf8, false),
        ArrowField::new("file_type", DataType::Utf8, false),
        ArrowField::new("doc_type", DataType::Utf8, false),
        ArrowField::new("status", DataType::Utf8, false),
        ArrowField::new("content_hash", DataType::Utf8, false),
        ArrowField::new("total_tokens", DataType::UInt32, false),
        ArrowField::new("tags_json", DataType::Utf8, false),
        ArrowField::new(
            "embedding",
            DataType::FixedSizeList(
                Arc::new(ArrowField::new("item", DataType::Float32, true)),
                EMBEDDING_DIMENSIONS,
            ),
            true,
        ),
    ]))
}

fn lance_chunk_schema() -> Arc<ArrowSchema> {
    Arc::new(ArrowSchema::new(vec![
        ArrowField::new("chunk_id", DataType::Utf8, false),
        ArrowField::new("file_path", DataType::Utf8, false),
        ArrowField::new("byte_range_start", DataType::UInt64, false),
        ArrowField::new("byte_range_end", DataType::UInt64, false),
        ArrowField::new("content_hash", DataType::Utf8, false),
        ArrowField::new("estimated_tokens", DataType::UInt32, false),
        ArrowField::new(
            "embedding",
            DataType::FixedSizeList(
                Arc::new(ArrowField::new("item", DataType::Float32, true)),
                EMBEDDING_DIMENSIONS,
            ),
            true,
        ),
        ArrowField::new("content_preview", DataType::Utf8, false),
    ]))
}

fn lance_turn_schema() -> Arc<ArrowSchema> {
    Arc::new(ArrowSchema::new(vec![
        ArrowField::new("turn_id", DataType::UInt64, false),
        ArrowField::new("timestamp", DataType::UInt64, false),
        ArrowField::new("user_intent", DataType::Utf8, false),
        ArrowField::new("workflow_id", DataType::Utf8, false),
        ArrowField::new("decisions_json", DataType::Utf8, false),
        ArrowField::new("changed_paths_json", DataType::Utf8, false),
        ArrowField::new(
            "summary_embedding",
            DataType::FixedSizeList(
                Arc::new(ArrowField::new("item", DataType::Float32, true)),
                EMBEDDING_DIMENSIONS,
            ),
            true,
        ),
    ]))
}

fn semantic_hits_from_batches(batches: &[RecordBatch]) -> Result<Vec<SemanticHit>> {
    let mut hits = Vec::new();
    for batch in batches {
        let file_path_index = batch
            .schema()
            .index_of("file_path")
            .context("missing file_path column")?;
        let preview_index = batch
            .schema()
            .index_of("content_preview")
            .context("missing content_preview column")?;
        let file_paths = batch
            .column(file_path_index)
            .as_any()
            .downcast_ref::<StringArray>()
            .context("file_path column is not Utf8")?;
        let previews = batch
            .column(preview_index)
            .as_any()
            .downcast_ref::<StringArray>()
            .context("content_preview column is not Utf8")?;

        for row in 0..batch.num_rows() {
            hits.push(SemanticHit {
                file_path: file_paths.value(row).to_string(),
                content_preview: previews.value(row).to_string(),
            });
        }
    }
    Ok(hits)
}

fn vector_array(embeddings: Vec<Vec<f32>>) -> ArrayRef {
    Arc::new(
        FixedSizeListArray::from_iter_primitive::<Float32Type, _, _>(
            embeddings
                .into_iter()
                .map(|embedding| Some(embedding.into_iter().map(Some).collect::<Vec<_>>())),
            EMBEDDING_DIMENSIONS,
        ),
    ) as ArrayRef
}

fn embed_passages(texts: &[String]) -> Result<Vec<Vec<f32>>> {
    match embedding_backend_kind() {
        EmbeddingBackendKind::FastEmbed => {
            let mut model = TextEmbedding::try_new(
                InitOptions::new(EmbeddingModel::AllMiniLML6V2)
                    .with_show_download_progress(false),
            )
            .context("failed to initialize fastembed model")?;
            let embeddings = model
                .embed(
                    texts
                        .iter()
                        .map(|text| format!("passage: {text}"))
                        .collect::<Vec<_>>(),
                    None,
                )
                .context("failed to generate passage embeddings")?;
            Ok(embeddings.into_iter().map(normalize_embedding).collect())
        }
        EmbeddingBackendKind::Mock => Ok(texts
            .iter()
            .map(|text| mock_embedding(&format!("passage:{text}")))
            .collect()),
    }
}

fn embed_query(text: &str) -> Result<Vec<f32>> {
    match embedding_backend_kind() {
        EmbeddingBackendKind::FastEmbed => {
            let mut model = TextEmbedding::try_new(
                InitOptions::new(EmbeddingModel::AllMiniLML6V2)
                    .with_show_download_progress(false),
            )
            .context("failed to initialize fastembed model")?;
            let embeddings = model
                .embed(vec![format!("query: {text}")], None)
                .context("failed to generate query embedding")?;
            embeddings
                .into_iter()
                .next()
                .map(normalize_embedding)
                .context("missing query embedding")
        }
        EmbeddingBackendKind::Mock => Ok(mock_embedding(&format!("query:{text}"))),
    }
}

fn embedding_backend_kind() -> EmbeddingBackendKind {
    if cfg!(test)
        || std::env::var("OMEGA_DOCUMENT_EMBEDDING_BACKEND")
            .ok()
            .as_deref()
            == Some("mock")
    {
        EmbeddingBackendKind::Mock
    } else {
        EmbeddingBackendKind::FastEmbed
    }
}

fn normalize_embedding(mut embedding: Vec<f32>) -> Vec<f32> {
    embedding.resize(EMBEDDING_DIMENSIONS as usize, 0.0);
    let norm = embedding
        .iter()
        .map(|value| value * value)
        .sum::<f32>()
        .sqrt();
    if norm > 0.0 {
        for value in &mut embedding {
            *value /= norm;
        }
    }
    embedding
}

fn mock_embedding(text: &str) -> Vec<f32> {
    let mut embedding = vec![0.0; EMBEDDING_DIMENSIONS as usize];
    for token in text.split(|ch: char| !ch.is_alphanumeric()) {
        if token.is_empty() {
            continue;
        }
        let hash = blake3::hash(token.to_ascii_lowercase().as_bytes());
        let bytes = hash.as_bytes();
        let slot = u16::from_le_bytes([bytes[0], bytes[1]]) as usize % embedding.len();
        let sign = if bytes[2] % 2 == 0 { 1.0 } else { -1.0 };
        embedding[slot] += sign;
    }
    normalize_embedding(embedding)
}

fn average_embedding(sum: &[f32], count: usize) -> Vec<f32> {
    if count == 0 {
        return zero_embedding();
    }
    normalize_embedding(sum.iter().map(|value| *value / count as f32).collect())
}

fn zero_embedding() -> Vec<f32> {
    vec![0.0; EMBEDDING_DIMENSIONS as usize]
}

fn reciprocal_rank(rank: usize) -> f32 {
    1.0 / (HYBRID_RRF_K + rank as f32 + 1.0)
}

fn merge_hybrid_results(
    keyword_results: Vec<SearchResult>,
    semantic_results: Vec<SearchResult>,
    sort: SortField,
    max_results: usize,
) -> Vec<SearchResult> {
    let mut merged = BTreeMap::<String, SearchResult>::new();
    for (rank, result) in semantic_results.into_iter().enumerate() {
        merged
            .entry(result.path.clone())
            .and_modify(|existing| {
                existing.score += reciprocal_rank(rank);
                if existing.preview.is_empty() {
                    existing.preview = result.preview.clone();
                }
            })
            .or_insert(SearchResult {
                score: reciprocal_rank(rank),
                mode_used: SearchMode::Hybrid,
                degraded_from: None,
                ..result
            });
    }
    for (rank, result) in keyword_results.into_iter().enumerate() {
        merged
            .entry(result.path.clone())
            .and_modify(|existing| {
                existing.score += reciprocal_rank(rank);
                if existing.preview.is_empty() {
                    existing.preview = result.preview.clone();
                }
                existing.mode_used = SearchMode::Hybrid;
                existing.degraded_from = None;
            })
            .or_insert(SearchResult {
                score: reciprocal_rank(rank),
                mode_used: SearchMode::Hybrid,
                degraded_from: None,
                ..result
            });
    }
    let mut results = merged.into_values().collect::<Vec<_>>();
    apply_sort(&mut results, sort);
    results.truncate(max_results);
    results
}

fn run_async_operation<T, F>(operation: F) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce() -> BoxFuture<'static, Result<T>> + Send + 'static,
{
    if tokio::runtime::Handle::try_current().is_ok() {
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let result = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .context("failed to build tokio runtime")
                .and_then(|runtime| runtime.block_on(operation()));
            let _ = sender.send(result);
        });
        receiver
            .recv()
            .context("async vector operation thread terminated before returning")?
    } else {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .context("failed to build tokio runtime")?
            .block_on(operation())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct DocGovernanceRules {
    structure: StructureRules,
    naming: NamingRules,
    lifecycle: LifecycleRules,
    cross_ref: CrossRefRules,
    #[serde(default)]
    adr: AdrRules,
}

impl Default for DocGovernanceRules {
    fn default() -> Self {
        Self {
            structure: StructureRules {
                required_files: vec![
                    "README.md".to_string(),
                    "docs/TODO.md".to_string(),
                    "LICENSE".to_string(),
                ],
                expected_dirs: vec![
                    ExpectedDir::new(
                        "docs/specs",
                        "Formal specs, contracts, and repository rules",
                    ),
                    ExpectedDir::new("docs/prds", "Plans, architecture, and design details"),
                    ExpectedDir::new("docs/guide", "Usage guides and contributor workflows"),
                    ExpectedDir::new("docs/decisions", "Durable architecture decisions"),
                    ExpectedDir::new(
                        "docs/archive",
                        "Retired, superseded, or historical documents",
                    ),
                ],
            },
            naming: NamingRules {
                lowercase_dirs: true,
            },
            lifecycle: LifecycleRules {
                archive_checklist: vec![
                    "Add archive note at top of file".to_string(),
                    "Update README.md links".to_string(),
                    "Update docs/TODO.md".to_string(),
                    "Record in CHANGELOG.md if milestone".to_string(),
                ],
                required_frontmatter: vec![
                    "status".to_string(),
                    "last_verified_commit".to_string(),
                    "owner".to_string(),
                ],
            },
            cross_ref: CrossRefRules {
                readme_must_index: vec![
                    "docs/specs/*.md".to_string(),
                    "docs/prds/*.md".to_string(),
                    "docs/guide/*.md".to_string(),
                ],
                archive_update_targets: vec!["README.md".to_string(), "docs/TODO.md".to_string()],
                replacement_must_backlink: true,
            },
            adr: AdrRules::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct StructureRules {
    required_files: Vec<String>,
    expected_dirs: Vec<ExpectedDir>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ExpectedDir {
    path: String,
    purpose: String,
    file_pattern: String,
    doc_type: DocType,
}

impl ExpectedDir {
    fn new(path: &str, purpose: &str) -> Self {
        Self {
            path: path.to_string(),
            purpose: purpose.to_string(),
            file_pattern: "*.md".to_string(),
            doc_type: classify_expected_doc_type(path),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct NamingRules {
    lowercase_dirs: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct LifecycleRules {
    archive_checklist: Vec<String>,
    required_frontmatter: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct CrossRefRules {
    readme_must_index: Vec<String>,
    archive_update_targets: Vec<String>,
    replacement_must_backlink: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct AdrRules {
    required_frontmatter: Vec<String>,
}

impl Default for AdrRules {
    fn default() -> Self {
        Self {
            required_frontmatter: vec![
                "adr_number".to_string(),
                "date".to_string(),
                "status".to_string(),
                "author".to_string(),
            ],
        }
    }
}

pub struct ChunkManager;

impl ChunkManager {
    pub fn chunk_file(path: &str, content: &str) -> Vec<Chunk> {
        if content.trim().is_empty() {
            return Vec::new();
        }
        let segments = if path.ends_with(".md") {
            split_markdown_chunks(content)
        } else if path.ends_with(".rs") {
            split_rust_chunks(content)
        } else {
            split_fixed_chunks(content)
        };

        segments
            .into_iter()
            .enumerate()
            .filter_map(|(index, (start, end, segment))| {
                let trimmed = segment.trim();
                (!trimmed.is_empty()).then(|| Chunk {
                    id: format!("{path}#{index}"),
                    file_path: path.to_string(),
                    byte_range_start: start as u64,
                    byte_range_end: end as u64,
                    content_hash: blake3::hash(trimmed.as_bytes()).to_hex().to_string(),
                    estimated_tokens: estimate_tokens(trimmed),
                    content_preview: preview_text(trimmed, SEARCH_PREVIEW_LIMIT),
                })
            })
            .collect()
    }
}

fn should_visit_path(root: &Path, path: &Path) -> bool {
    let Ok(relative) = path.strip_prefix(root) else {
        return true;
    };
    if relative.as_os_str().is_empty() {
        return true;
    }
    let normalized = relative.to_string_lossy().replace('\\', "/");
    !(normalized == ".git"
        || normalized.starts_with(".git/")
        || normalized == "target"
        || normalized.starts_with("target/")
        || normalized == STORE_DIR
        || normalized.starts_with(&format!("{STORE_DIR}/")))
}

fn normalize_relative_path(root: &Path, path: &Path) -> Result<String> {
    Ok(path
        .strip_prefix(root)
        .with_context(|| format!("{} is not under {}", path.display(), root.display()))?
        .to_string_lossy()
        .replace('\\', "/"))
}

fn classify_file_type(path: &str) -> FileType {
    if path.contains("/tests/") || path.ends_with("_test.rs") || path.ends_with(".test.ts") {
        FileType::Test
    } else if matches!(
        Path::new(path).extension().and_then(|ext| ext.to_str()),
        Some("rs" | "ts" | "tsx" | "js" | "jsx" | "py" | "go" | "java" | "kt")
    ) {
        FileType::Source
    } else if matches!(
        Path::new(path).extension().and_then(|ext| ext.to_str()),
        Some("md" | "txt" | "adoc")
    ) {
        FileType::Doc
    } else if matches!(
        Path::new(path).extension().and_then(|ext| ext.to_str()),
        Some("toml" | "json" | "yaml" | "yml")
    ) {
        FileType::Config
    } else if matches!(
        Path::new(path).extension().and_then(|ext| ext.to_str()),
        Some("png" | "jpg" | "jpeg" | "gif" | "webp" | "svg")
    ) {
        FileType::Asset
    } else {
        FileType::Other
    }
}

fn classify_doc_type(path: &str) -> Option<DocType> {
    if path == "README.md" {
        Some(DocType::Readme)
    } else if path == "CHANGELOG.md" {
        Some(DocType::Changelog)
    } else if path == "docs/TODO.md" {
        Some(DocType::Todo)
    } else if path.starts_with("docs/specs/") {
        Some(DocType::Spec)
    } else if path.starts_with("docs/prds/") {
        Some(DocType::Prd)
    } else if path.starts_with("docs/guide/") {
        Some(DocType::Guide)
    } else if path.starts_with("docs/decisions/") {
        Some(DocType::Adr)
    } else if path.starts_with("docs/archive/") {
        Some(DocType::Archive)
    } else {
        None
    }
}

fn classify_expected_doc_type(path: &str) -> DocType {
    match path {
        "docs/specs" => DocType::Spec,
        "docs/prds" => DocType::Prd,
        "docs/guide" => DocType::Guide,
        "docs/decisions" => DocType::Adr,
        "docs/archive" => DocType::Archive,
        _ => DocType::Guide,
    }
}

fn infer_language(path: &str) -> Option<String> {
    match Path::new(path).extension().and_then(|ext| ext.to_str()) {
        Some("rs") => Some("rust".to_string()),
        Some("md") => Some("markdown".to_string()),
        Some("toml") => Some("toml".to_string()),
        Some("json") => Some("json".to_string()),
        Some("yml" | "yaml") => Some("yaml".to_string()),
        Some("ts" | "tsx") => Some("typescript".to_string()),
        Some("js" | "jsx") => Some("javascript".to_string()),
        Some("py") => Some("python".to_string()),
        _ => None,
    }
}

fn status_for_relative_path(path: &str) -> FileStatus {
    if path.starts_with("docs/archive/") {
        FileStatus::Archived
    } else {
        FileStatus::Active
    }
}

fn path_matches_doc_type(path: &str, doc_type: DocType) -> bool {
    match doc_type {
        DocType::Spec => path.starts_with("docs/specs/"),
        DocType::Prd => path.starts_with("docs/prds/"),
        DocType::Guide => path.starts_with("docs/guide/"),
        DocType::Adr => path.starts_with("docs/decisions/"),
        DocType::Todo => path == "docs/TODO.md",
        DocType::Archive => path.starts_with("docs/archive/"),
        DocType::Readme => path == "README.md",
        DocType::Changelog => path == "CHANGELOG.md",
    }
}

fn format_file_type(file_type: FileType) -> String {
    match file_type {
        FileType::Source => "source",
        FileType::Doc => "doc",
        FileType::Config => "config",
        FileType::Asset => "asset",
        FileType::Test => "test",
        FileType::Other => "other",
    }
    .to_string()
}

fn format_doc_type(doc_type: DocType) -> String {
    match doc_type {
        DocType::Spec => "spec",
        DocType::Prd => "prd",
        DocType::Guide => "guide",
        DocType::Adr => "adr",
        DocType::Todo => "todo",
        DocType::Archive => "archive",
        DocType::Readme => "readme",
        DocType::Changelog => "changelog",
    }
    .to_string()
}

fn format_file_status(status: &FileStatus) -> String {
    match status {
        FileStatus::Active => "active".to_string(),
        FileStatus::Deleted => "deleted".to_string(),
        FileStatus::Archived => "archived".to_string(),
        FileStatus::Moved { to } => format!("moved:{to}"),
    }
}

fn matches_filters(record: &FileRecord, filters: &[SearchFilter]) -> bool {
    filters.iter().all(|filter| match filter {
        SearchFilter::Language(languages) => record
            .language
            .as_ref()
            .is_some_and(|language| languages.iter().any(|value| value == language)),
        SearchFilter::FileType(file_types) => file_types.contains(&record.file_type),
        SearchFilter::DocType(doc_types) => record
            .doc_type
            .is_some_and(|doc_type| doc_types.contains(&doc_type)),
        SearchFilter::PathGlob(pattern) => glob_matches(pattern, &record.path),
        SearchFilter::ModifiedAfter(timestamp) => record.modified_at > *timestamp,
        SearchFilter::ModifiedBefore(timestamp) => record.modified_at < *timestamp,
        SearchFilter::Status(statuses) => statuses.iter().any(|status| status == &record.status),
        SearchFilter::Tag(tags) => record
            .tags
            .iter()
            .any(|tag| tags.iter().any(|value| value == tag)),
        SearchFilter::MinTokens(min_tokens) => record.total_tokens >= *min_tokens,
        SearchFilter::MaxTokens(max_tokens) => record.total_tokens <= *max_tokens,
    })
}

fn glob_matches(pattern: &str, path: &str) -> bool {
    let Ok(glob) = Glob::new(pattern) else {
        return false;
    };
    let mut builder = GlobSetBuilder::new();
    builder.add(glob);
    let Ok(set) = builder.build() else {
        return false;
    };
    set.is_match(path)
}

struct StoreIgnoreRules {
    matcher: Option<GlobSet>,
}

impl StoreIgnoreRules {
    fn load(root: &Path) -> Result<Self> {
        let path = root.join(STOREIGNORE_PATH);
        if !path.exists() {
            return Ok(Self { matcher: None });
        }

        let contents = fs::read_to_string(&path)
            .with_context(|| format!("failed to read storeignore file {}", path.display()))?;
        let mut builder = GlobSetBuilder::new();
        let mut pattern_count = 0usize;

        for (index, line) in contents.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let pattern = normalize_storeignore_pattern(trimmed);
            let glob = Glob::new(&pattern).with_context(|| {
                format!(
                    "invalid .storeignore pattern at {}:{}",
                    path.display(),
                    index + 1
                )
            })?;
            builder.add(glob);
            pattern_count += 1;
        }

        let matcher = if pattern_count == 0 {
            None
        } else {
            Some(builder.build().context("failed to compile .storeignore rules")?)
        };

        Ok(Self { matcher })
    }

    fn is_match(&self, path: &str) -> bool {
        self.matcher.as_ref().is_some_and(|matcher| matcher.is_match(path))
    }
}

fn normalize_storeignore_pattern(pattern: &str) -> String {
    let normalized = pattern.trim().replace('\\', "/");
    if let Some(prefix) = normalized.strip_suffix('/') {
        format!("{prefix}/**")
    } else {
        normalized
    }
}

fn matches_readme_patterns(patterns: &[String], path: &str) -> bool {
    if patterns.is_empty() {
        return false;
    }
    patterns.iter().any(|pattern| glob_matches(pattern, path))
}

fn file_has_frontmatter_status(path: &Path) -> Result<bool> {
    if !path.exists() {
        return Ok(false);
    }
    let content = fs::read_to_string(path)
        .with_context(|| format!("failed to read document {}", path.display()))?;
    Ok(content_starts_with_frontmatter_status(&content))
}

fn content_starts_with_frontmatter_status(content: &str) -> bool {
    let mut lines = content.lines();
    if lines.next() != Some("---") {
        return false;
    }
    for line in lines {
        if line == "---" {
            return false;
        }
        if line.trim_start().starts_with("status:") {
            return true;
        }
    }
    false
}

fn get_missing_frontmatter_fields(path: &Path, required_fields: &[String]) -> Result<Vec<String>> {
    if !path.exists() {
        return Ok(required_fields.iter().cloned().collect());
    }
    let content = fs::read_to_string(path)
        .with_context(|| format!("failed to read document {}", path.display()))?;
    let missing = find_missing_frontmatter_fields(&content, required_fields);
    Ok(missing)
}

fn find_missing_frontmatter_fields(content: &str, required_fields: &[String]) -> Vec<String> {
    let mut lines = content.lines();
    if lines.next() != Some("---") {
        // No frontmatter at all
        return required_fields.iter().cloned().collect();
    }
    
    let mut found_fields = BTreeSet::new();
    for line in lines.by_ref() {
        if line == "---" {
            break;
        }
        if let Some((key, _)) = line.split_once(':') {
            found_fields.insert(key.trim().to_string());
        }
    }
    
    required_fields
        .iter()
        .filter(|field| !found_fields.contains(*field))
        .cloned()
        .collect()
}

fn find_broken_crossrefs(root: &Path, docs: &[FileRecord]) -> Result<Vec<CrossRefIssue>> {
    let existing_paths = docs
        .iter()
        .map(|record| record.path.clone())
        .chain([
            "README.md".to_string(),
            "docs/TODO.md".to_string(),
            "CHANGELOG.md".to_string(),
        ])
        .collect::<BTreeSet<_>>();
    let mut issues = Vec::new();
    for record in docs {
        let path = root.join(&record.path);
        if !path.exists() {
            continue;
        }
        let content = fs::read_to_string(&path)
            .with_context(|| format!("failed to read document {}", path.display()))?;
        for target in markdown_link_targets(&content) {
            if target.starts_with("http://")
                || target.starts_with("https://")
                || target.starts_with('#')
                || target.starts_with("mailto:")
            {
                continue;
            }
            let normalized = normalize_link_target(&record.path, &target);
            if !existing_paths.contains(&normalized) && !root.join(&normalized).exists() {
                issues.push(CrossRefIssue {
                    path: record.path.clone(),
                    target,
                    message: "linked document does not exist".to_string(),
                });
            }
        }
    }
    Ok(issues)
}

fn markdown_link_targets(content: &str) -> Vec<String> {
    let mut targets = Vec::new();
    let bytes = content.as_bytes();
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index] == b']' && index + 1 < bytes.len() && bytes[index + 1] == b'(' {
            let start = index + 2;
            if let Some(end_offset) = content[start..].find(')') {
                let target = content[start..start + end_offset].trim();
                if !target.is_empty() {
                    targets.push(target.to_string());
                }
                index = start + end_offset + 1;
                continue;
            }
        }
        index += 1;
    }
    targets
}

fn normalize_link_target(source_path: &str, target: &str) -> String {
    let target = target.split('#').next().unwrap_or(target);
    let target_path = Path::new(target);
    if target_path.is_absolute() {
        return target.trim_start_matches('/').to_string();
    }
    let base = Path::new(source_path)
        .parent()
        .unwrap_or_else(|| Path::new(""));
    normalize_path_string(&base.join(target_path))
}

fn copy_path(source: &Path, destination: &Path) -> Result<()> {
    if !source.exists() {
        return Ok(());
    }
    if source.is_dir() {
        copy_dir_recursive(source, destination)
    } else {
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        fs::copy(source, destination).with_context(|| {
            format!(
                "failed to copy {} -> {}",
                source.display(),
                destination.display()
            )
        })?;
        Ok(())
    }
}

fn copy_dir_recursive(source: &Path, destination: &Path) -> Result<()> {
    fs::create_dir_all(destination)
        .with_context(|| format!("failed to create {}", destination.display()))?;
    for entry in fs::read_dir(source)
        .with_context(|| format!("failed to read {}", source.display()))?
    {
        let entry = entry.with_context(|| format!("failed to read {}", source.display()))?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        copy_path(&source_path, &destination_path)?;
    }
    Ok(())
}

fn remove_path_if_exists(path: &Path) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    if path.is_dir() {
        fs::remove_dir_all(path)
            .with_context(|| format!("failed to remove {}", path.display()))?;
    } else {
        fs::remove_file(path)
            .with_context(|| format!("failed to remove {}", path.display()))?;
    }
    Ok(())
}

fn relative_store_path(root: &Path, path: &Path) -> Result<String> {
    normalize_relative_path(root, path)
}

fn normalize_path_string(path: &Path) -> String {
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                parts.pop();
            }
            std::path::Component::Normal(value) => parts.push(value.to_string_lossy().to_string()),
            _ => {}
        }
    }
    parts.join("/")
}

fn preview_text(text: &str, limit: usize) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= limit {
        collapsed
    } else {
        collapsed.chars().take(limit).collect()
    }
}

fn file_preview(path: &Path) -> Result<String> {
    let bytes =
        fs::read(path).with_context(|| format!("failed to read file {}", path.display()))?;
    Ok(preview_text(
        &String::from_utf8_lossy(&bytes),
        SEARCH_PREVIEW_LIMIT,
    ))
}

fn file_preview_fallback(record: &FileRecord) -> String {
    format!("{} [{}]", record.path, format_file_type(record.file_type))
}

fn manifest_hash_for_records(records: &[FileRecord]) -> String {
    let mut hasher = blake3::Hasher::new();
    for record in records {
        hasher.update(record.path.as_bytes());
        hasher.update(record.content_hash.as_bytes());
        hasher.update(format_file_status(&record.status).as_bytes());
        hasher.update(&record.modified_at.to_le_bytes());
        hasher.update(&record.size_bytes.to_le_bytes());
    }
    hasher.finalize().to_hex().to_string()
}

fn build_store_version(
    version_id: &str,
    manifest_revision: u64,
    lance_revision: Option<u64>,
    built_at: u64,
    manifest_hash: &str,
    storage_path: &str,
    records: &[FileRecord],
    chunk_count: usize,
    deleted_marked: usize,
) -> DocumentStoreVersion {
    DocumentStoreVersion {
        version_id: version_id.to_string(),
        schema_version: STORE_SCHEMA_VERSION,
        manifest_revision,
        tantivy_revision: manifest_revision,
        lance_revision,
        built_at,
        promoted_at: None,
        build_trigger: "scan_workspace".to_string(),
        total_files_indexed: records.len() as u64,
        total_chunks: chunk_count as u64,
        total_embeddings: chunk_count as u64,
        deleted_marked: deleted_marked as u64,
        manifest_hash: manifest_hash.to_string(),
        storage_path: storage_path.to_string(),
    }
}

fn apply_sort(results: &mut [SearchResult], sort: SortField) {
    match sort {
        SortField::Relevance => results.sort_by(|left, right| right.score.total_cmp(&left.score)),
        SortField::ModifiedDesc => {
            results.sort_by(|left, right| right.modified_at.cmp(&left.modified_at))
        }
        SortField::TokensAsc => {
            results.sort_by(|left, right| left.total_tokens.cmp(&right.total_tokens))
        }
    }
}

fn split_markdown_chunks(content: &str) -> Vec<(usize, usize, &str)> {
    split_by_predicate(content, |line| line.starts_with("## "))
}

fn split_rust_chunks(content: &str) -> Vec<(usize, usize, &str)> {
    split_by_predicate(content, |line| {
        let trimmed = line.trim_start();
        trimmed.starts_with("fn ") || trimmed.starts_with("impl ") || trimmed.starts_with("mod ")
    })
}

fn split_fixed_chunks(content: &str) -> Vec<(usize, usize, &str)> {
    if content.chars().count() <= CHUNK_TARGET_CHARS {
        return vec![(0, content.len(), content)];
    }

    let mut chunks = Vec::new();
    let mut start = 0usize;
    let mut current_chars = 0usize;
    for (index, ch) in content.char_indices() {
        current_chars += 1;
        if current_chars >= CHUNK_TARGET_CHARS && ch == '\n' {
            chunks.push((start, index, &content[start..index]));
            start = index + ch.len_utf8();
            current_chars = 0;
        }
    }
    if start < content.len() {
        chunks.push((start, content.len(), &content[start..]));
    }
    chunks
}

fn split_by_predicate<'a>(
    content: &'a str,
    is_boundary: impl Fn(&str) -> bool,
) -> Vec<(usize, usize, &'a str)> {
    let mut boundaries = vec![0usize];
    let mut offset = 0usize;
    for line in content.lines() {
        if offset > 0 && is_boundary(line) {
            boundaries.push(offset);
        }
        offset += line.len() + 1;
    }
    boundaries.push(content.len());
    boundaries
        .windows(2)
        .filter_map(|window| {
            let start = window[0];
            let end = window[1];
            let segment = &content[start..end];
            (!segment.trim().is_empty()).then_some((start, end, segment))
        })
        .collect()
}

fn estimate_tokens(text: &str) -> u32 {
    text.chars().count().div_ceil(ESTIMATED_TOKEN_DIVISOR) as u32
}

fn normalize_max_results(limit: usize) -> usize {
    if limit == 0 {
        DEFAULT_MAX_RESULTS
    } else {
        limit.min(50)
    }
}

fn archive_note(reason: ArchiveTrigger, replaced_by: Option<&str>) -> String {
    let mut lines = vec![
        format!("> Archived: {}", unix_timestamp_now()),
        format!("> Reason: {}", format_archive_trigger(reason)),
    ];
    if let Some(replaced_by) = replaced_by {
        lines.push(format!("> Replaced by: {replaced_by}"));
    }
    lines.join("\n")
}

fn format_archive_trigger(trigger: ArchiveTrigger) -> &'static str {
    match trigger {
        ArchiveTrigger::Superseded => "superseded",
        ArchiveTrigger::CompletedAndInactive => "completed_and_inactive",
        ArchiveTrigger::StructurallyOutdated => "structurally_outdated",
        ArchiveTrigger::HistoryOnly => "history_only",
    }
}

fn unix_timestamp_now() -> u64 {
    unix_timestamp(SystemTime::now())
}

fn unix_timestamp(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn now_age_days(timestamp: u64) -> u64 {
    let now = unix_timestamp_now();
    now.saturating_sub(timestamp) / 86_400
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::{
        ArchiveTrigger, DocType, DocumentMutationMode, DocumentOp, FileStatus, OmegaDocument,
        SearchMode, SearchQuery, TodoOp,
    };
    use omega_project_layout::{STORE_DIR_PATH, STORE_MANIFEST_PATH, STORE_VERSION_PATH};
    use omega_todo::{TodoItem, TodoStatus};

    static TEST_COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn temp_root(name: &str) -> std::path::PathBuf {
        let counter = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "omega-document-{name}-{}-{}",
            std::process::id(),
            counter
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn seed_repo(root: &std::path::Path) {
        std::fs::create_dir_all(root.join("docs/specs")).unwrap();
        std::fs::create_dir_all(root.join("docs/archive")).unwrap();
        std::fs::create_dir_all(root.join("crates/demo/src")).unwrap();
        std::fs::write(root.join("README.md"), "See docs/specs/example.md\n").unwrap();
        std::fs::write(root.join("LICENSE"), "MIT\n").unwrap();
        std::fs::write(root.join("docs/TODO.md"), "# TODO\n").unwrap();
        std::fs::write(
            root.join("docs/specs/example.md"),
            "---\nstatus: draft\n---\n\n# Example\n\nkeyword anchor\n",
        )
        .unwrap();
        std::fs::write(
            root.join("crates/demo/src/lib.rs"),
            "pub fn example() { println!(\"keyword anchor\"); }\n",
        )
        .unwrap();
    }

    fn write_storeignore(root: &std::path::Path, rules: &str) {
        std::fs::create_dir_all(root.join(".omega")).unwrap();
        std::fs::write(root.join(".omega/.storeignore"), rules).unwrap();
    }

    #[test]
    fn scan_workspace_writes_manifest_and_keyword_search_finds_matches() {
        let root = temp_root("scan-search");
        seed_repo(&root);
        let documents = OmegaDocument::new(root.clone());

        let scan = documents.scan_workspace().unwrap();
        assert!(scan.files_indexed >= 5);
        assert!(root.join(STORE_MANIFEST_PATH).exists());

        let results = documents
            .search(SearchQuery {
                text: Some("keyword anchor".to_string()),
                mode: SearchMode::Keyword,
                filters: Vec::new(),
                sort: None,
                max_results: 5,
            })
            .unwrap();

        assert!(!results.is_empty());
        assert!(results
            .iter()
            .any(|result| result.path == "docs/specs/example.md"));
    }

    #[test]
    fn scan_workspace_writes_active_store_version_metadata() {
        let root = temp_root("store-version-active");
        seed_repo(&root);
        let documents = OmegaDocument::new(root.clone());

        let scan = documents.scan_workspace().unwrap();
        let ledger = documents.file_store.load_version_ledger().unwrap();
        let active = ledger.active_version.expect("active version should exist");

        assert!(root.join(STORE_VERSION_PATH).exists());
        assert_eq!(scan.active_version.as_ref(), Some(&active));
        assert!(active.version_id.starts_with("store-v"));
        assert_eq!(active.storage_path, STORE_DIR_PATH);
        assert!(active.promoted_at.is_some());
        assert!(active.manifest_revision >= 1);
    }

    #[test]
    fn scan_workspace_archives_previous_active_version_before_promotion() {
        let root = temp_root("store-version-history");
        seed_repo(&root);
        let documents = OmegaDocument::new(root.clone());

        let first_scan = documents.scan_workspace().unwrap();
        let first_version = first_scan
            .active_version
            .clone()
            .expect("first scan should promote an active version");

        std::fs::write(
            root.join("docs/specs/example.md"),
            "---\nstatus: draft\n---\n\n# Example\n\nkeyword anchor\n\nsecond revision\n",
        )
        .unwrap();

        let second_scan = documents.scan_workspace().unwrap();
        let second_version = second_scan
            .active_version
            .clone()
            .expect("second scan should promote an active version");

        assert_ne!(first_version.version_id, second_version.version_id);
        let archived_path = second_scan
            .archived_version_path
            .expect("previous active version should be archived");
        let archived_root = root.join(&archived_path);
        assert!(archived_root.exists());
        assert!(archived_root.join("files.jsonl").exists());
        assert!(archived_root.join("index-commit-log.json").exists());
        assert!(archived_root.join("tantivy").exists());
        assert!(archived_root.join("lance").exists());
    }

    #[test]
    fn semantic_search_uses_lance_vector_index() {
        let root = temp_root("semantic-search");
        seed_repo(&root);
        let documents = OmegaDocument::new(root);

        documents.scan_workspace().unwrap();
        let results = documents
            .search(SearchQuery {
                text: Some("example specification keyword".to_string()),
                mode: SearchMode::Semantic,
                filters: Vec::new(),
                sort: None,
                max_results: 5,
            })
            .unwrap();

        assert!(!results.is_empty());
        assert!(results
            .iter()
            .all(|result| result.mode_used == SearchMode::Semantic));
        assert!(results
            .iter()
            .any(|result| result.path == "docs/specs/example.md"));
    }

    #[test]
    fn hybrid_search_merges_keyword_and_semantic_results() {
        let root = temp_root("hybrid-search");
        seed_repo(&root);
        let documents = OmegaDocument::new(root);

        documents.scan_workspace().unwrap();
        let results = documents
            .search(SearchQuery {
                text: Some("keyword anchor".to_string()),
                mode: SearchMode::Hybrid,
                filters: Vec::new(),
                sort: None,
                max_results: 5,
            })
            .unwrap();

        assert!(!results.is_empty());
        assert!(results
            .iter()
            .all(|result| result.mode_used == SearchMode::Hybrid));
        assert!(results
            .iter()
            .any(|result| result.path == "docs/specs/example.md"));
    }

    #[test]
    fn scan_workspace_excludes_storeignored_files_from_manifest() {
        let root = temp_root("storeignore-scan");
        seed_repo(&root);
        std::fs::write(
            root.join("docs/specs/ignored.md"),
            "---\nstatus: draft\n---\n\n# Ignored\n\nignored vector anchor\n",
        )
        .unwrap();
        write_storeignore(&root, "docs/specs/ignored.md\n");
        let documents = OmegaDocument::new(root);

        let scan = documents.scan_workspace().unwrap();
        let records = documents.file_store.load_records_map().unwrap();

        assert_eq!(scan.vector_ignored_files, 1);
        assert_eq!(scan.vector_ignored_paths, vec!["docs/specs/ignored.md".to_string()]);
        assert!(!records.contains_key("docs/specs/ignored.md"));
        assert!(records["docs/specs/example.md"].vector_index_eligible);
    }

    #[test]
    fn scan_workspace_applies_storeignore_to_dot_directories_and_root_files() {
        let root = temp_root("storeignore-dot-paths");
        seed_repo(&root);
        std::fs::create_dir_all(root.join(".claude/skills/demo")).unwrap();
        std::fs::create_dir_all(root.join(".github/workflows")).unwrap();
        std::fs::create_dir_all(root.join(".omega/prompts")).unwrap();
        std::fs::write(root.join(".claude/skills/demo/SKILL.md"), "demo skill").unwrap();
        std::fs::write(root.join(".github/workflows/ci.yml"), "name: ci\n").unwrap();
        std::fs::write(root.join("Cargo.lock"), "# lock\n").unwrap();
        std::fs::write(root.join(".omega/prompts/cache.txt"), "cache\n").unwrap();
        write_storeignore(&root, ".claude/\n.github/\nCargo.lock\n.omega/**\n");
        let documents = OmegaDocument::new(root);

        let scan = documents.scan_workspace().unwrap();
        let records = documents.file_store.load_records_map().unwrap();

        assert_eq!(scan.vector_ignored_files, 5);
        assert!(scan
            .vector_ignored_paths
            .contains(&".claude/skills/demo/SKILL.md".to_string()));
        assert!(scan
            .vector_ignored_paths
            .contains(&".github/workflows/ci.yml".to_string()));
        assert!(scan.vector_ignored_paths.contains(&"Cargo.lock".to_string()));
        assert!(scan
            .vector_ignored_paths
            .contains(&".omega/prompts/cache.txt".to_string()));
        assert!(scan
            .vector_ignored_paths
            .contains(&".omega/.storeignore".to_string()));
        assert!(!records.contains_key(".claude/skills/demo/SKILL.md"));
        assert!(!records.contains_key(".github/workflows/ci.yml"));
        assert!(!records.contains_key("Cargo.lock"));
        assert!(!records.contains_key(".omega/prompts/cache.txt"));
        assert!(!records.contains_key(".omega/.storeignore"));
    }

    #[test]
    fn keyword_search_skips_storeignored_files() {
        let root = temp_root("storeignore-keyword");
        seed_repo(&root);
        std::fs::write(
            root.join("docs/specs/ignored.md"),
            "---\nstatus: draft\n---\n\n# Ignored\n\nignored vector anchor\n",
        )
        .unwrap();
        write_storeignore(&root, "docs/specs/ignored.md\n");
        let documents = OmegaDocument::new(root);

        documents.scan_workspace().unwrap();
        let results = documents
            .search(SearchQuery {
                text: Some("ignored vector anchor".to_string()),
                mode: SearchMode::Keyword,
                filters: Vec::new(),
                sort: None,
                max_results: 5,
            })
            .unwrap();

        assert!(results
            .iter()
            .all(|result| result.path != "docs/specs/ignored.md"));
    }

    #[test]
    fn semantic_search_skips_storeignored_files() {
        let root = temp_root("storeignore-semantic");
        seed_repo(&root);
        std::fs::write(
            root.join("docs/specs/ignored.md"),
            "---\nstatus: draft\n---\n\n# Ignored\n\nignored vector anchor\n",
        )
        .unwrap();
        write_storeignore(&root, "docs/specs/ignored.md\n");
        let documents = OmegaDocument::new(root);

        documents.scan_workspace().unwrap();
        let results = documents
            .search(SearchQuery {
                text: Some("ignored vector anchor".to_string()),
                mode: SearchMode::Semantic,
                filters: Vec::new(),
                sort: None,
                max_results: 5,
            })
            .unwrap();

        assert!(results
            .iter()
            .all(|result| result.path != "docs/specs/ignored.md"));
    }

    #[test]
    fn hybrid_search_skips_storeignored_files() {
        let root = temp_root("storeignore-hybrid");
        seed_repo(&root);
        std::fs::write(
            root.join("docs/specs/ignored.md"),
            "---\nstatus: draft\n---\n\n# Ignored\n\nignored vector anchor\n",
        )
        .unwrap();
        write_storeignore(&root, "docs/specs/ignored.md\n");
        let documents = OmegaDocument::new(root);

        documents.scan_workspace().unwrap();
        let results = documents
            .search(SearchQuery {
                text: Some("ignored vector anchor".to_string()),
                mode: SearchMode::Hybrid,
                filters: Vec::new(),
                sort: None,
                max_results: 5,
            })
            .unwrap();

        assert!(results
            .iter()
            .all(|result| result.path != "docs/specs/ignored.md"));
    }

    #[test]
    fn semantic_search_degrades_to_keyword_when_lance_revision_lags() {
        let root = temp_root("semantic-degraded");
        seed_repo(&root);
        let documents = OmegaDocument::new(root.clone());

        documents.scan_workspace().unwrap();
        std::fs::write(
            root.join(super::INDEX_COMMIT_LOG_PATH),
            serde_json::to_string_pretty(&super::IndexCommitLog {
                current_manifest_revision: 2,
                tantivy_revision: 2,
                lance_revision: Some(1),
                manifest_hash: "stale".to_string(),
                committed_at: super::unix_timestamp_now(),
            })
            .unwrap(),
        )
        .unwrap();

        let results = documents
            .search(SearchQuery {
                text: Some("keyword anchor".to_string()),
                mode: SearchMode::Semantic,
                filters: Vec::new(),
                sort: None,
                max_results: 5,
            })
            .unwrap();

        assert!(!results.is_empty());
        assert!(results
            .iter()
            .all(|result| result.mode_used == SearchMode::Keyword));
        assert!(results
            .iter()
            .all(|result| result.degraded_from == Some(SearchMode::Semantic)));
    }

    #[test]
    fn archive_document_moves_file_and_inserts_archive_note() {
        let root = temp_root("archive");
        seed_repo(&root);
        let documents = OmegaDocument::new(root.clone());

        let result = documents
            .manage_document(DocumentOp::Archive {
                mode: DocumentMutationMode::Apply,
                path: "docs/specs/example.md".to_string(),
                reason: ArchiveTrigger::Superseded,
                replaced_by: Some("docs/specs/new-example.md".to_string()),
            })
            .unwrap();

        assert!(result.ok);
        assert!(!root.join("docs/specs/example.md").exists());
        let archived = std::fs::read_to_string(root.join("docs/archive/example.md")).unwrap();
        assert!(archived.contains("Archived:"));
        assert!(archived.contains("Replaced by: docs/specs/new-example.md"));
    }

    #[test]
    fn persistent_todo_store_keeps_latest_snapshot_and_history() {
        let root = temp_root("todo-store");
        seed_repo(&root);
        let documents = OmegaDocument::new(root);

        let replace = documents
            .manage_todo(TodoOp::Replace {
                items: vec![TodoItem {
                    id: Some("task-1".to_string()),
                    text: "Write docs".to_string(),
                    status: TodoStatus::InProgress,
                    active_form: Some("Writing docs".to_string()),
                }],
            })
            .unwrap();
        assert!(replace.current.is_some());
        assert!(replace
            .current
            .as_ref()
            .unwrap()
            .rendered
            .contains("Write docs"));

        let current = documents.manage_todo(TodoOp::Current).unwrap();
        assert_eq!(current.current.unwrap().items.len(), 1);

        let history = documents.manage_todo(TodoOp::History { limit: 5 }).unwrap();
        assert_eq!(history.history.len(), 1);
    }

    #[test]
    fn health_report_flags_missing_required_files() {
        let root = temp_root("health");
        std::fs::create_dir_all(root.join("docs/specs")).unwrap();
        std::fs::write(root.join("README.md"), "# Readme\n").unwrap();
        let documents = OmegaDocument::new(root);

        let report = documents.check_document_health().unwrap();
        assert_eq!(report.overall_health, super::HealthScore::Critical);
        assert!(report
            .structure_violations
            .iter()
            .any(|issue| issue.path == "docs/TODO.md"));
    }

    #[test]
    fn list_filters_by_status() {
        let root = temp_root("list");
        seed_repo(&root);
        let documents = OmegaDocument::new(root);
        documents.scan_workspace().unwrap();

        let listed = documents
            .manage_document(DocumentOp::List {
                doc_type: Some(DocType::Spec),
                status: Some(FileStatus::Active),
            })
            .unwrap();

        assert!(listed
            .files
            .iter()
            .any(|record| record.path == "docs/specs/example.md"));
    }
}
