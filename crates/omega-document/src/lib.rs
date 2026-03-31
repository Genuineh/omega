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
use globset::{Glob, GlobSetBuilder};
use lancedb::index::Index as LanceIndex;
use lancedb::query::{ExecutableQuery, QueryBase};
use omega_todo::{TodoItem, TodoManager};
use serde::{Deserialize, Serialize};
use tantivy::collector::TopDocs;
use tantivy::query::QueryParser;
use tantivy::schema::{Field, Schema, Value, FAST, INDEXED, STORED, STRING, TEXT};
use tantivy::{doc, Index};
use walkdir::WalkDir;

const STORE_DIR: &str = ".omega/store";
const FILE_MANIFEST_PATH: &str = ".omega/store/files.jsonl";
const TODO_STORE_PATH: &str = ".omega/store/todos.jsonl";
const TANTIVY_DIR: &str = ".omega/store/tantivy";
const LANCE_DIR: &str = ".omega/store/lance";
const INDEX_COMMIT_LOG_PATH: &str = ".omega/store/index-commit-log.json";
const DOC_RULES_PATH: &str = ".omega/doc-rules.toml";
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
    pub manifest_path: String,
    pub keyword_index_path: String,
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
        let mut active_records = BTreeMap::new();
        let mut deleted_marked = 0usize;
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
                    last_indexed_at: now,
                },
            );
        }

        for (path, previous) in previous_records {
            if active_records.contains_key(&path) {
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
        self.file_store.write_records(&records)?;
        if manifest_changed
            || !self.keyword_index.index_dir.exists()
            || previous_commit.tantivy_revision != revision
        {
            self.keyword_index.rebuild(&records, &chunks)?;
        }

        let lance_revision = if manifest_changed
            || !self.vector_index.db_dir.exists()
            || previous_commit.lance_revision != Some(revision)
        {
            self.vector_index
                .rebuild(&records, &chunks, revision)
                .map(|_| Some(revision))
                .or_else(|_| {
                    if !manifest_changed && previous_commit.lance_revision == Some(revision) {
                        Ok::<Option<u64>, anyhow::Error>(previous_commit.lance_revision)
                    } else {
                        Ok::<Option<u64>, anyhow::Error>(previous_commit.lance_revision)
                    }
                })?
        } else {
            previous_commit.lance_revision
        };
        self.file_store.write_commit_log(&IndexCommitLog {
            current_manifest_revision: revision,
            tantivy_revision: revision,
            lance_revision,
            manifest_hash,
            committed_at: now,
        })?;

        Ok(ScanResult {
            files_indexed: records.len(),
            chunks_indexed: chunks.len(),
            deleted_marked,
            manifest_path: FILE_MANIFEST_PATH.to_string(),
            keyword_index_path: TANTIVY_DIR.to_string(),
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
            if rules
                .lifecycle
                .required_frontmatter
                .iter()
                .any(|field| field == "status")
                && !file_has_frontmatter_status(&self.root.join(&record.path))?
            {
                missing_frontmatter.push(record.path.clone());
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
        if rules
            .lifecycle
            .required_frontmatter
            .iter()
            .any(|field| field == "status")
            && !content_starts_with_frontmatter_status(content)
        {
            validation_issues
                .push("content is missing required frontmatter field 'status'".to_string());
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
}

impl FileStore {
    fn new(root: PathBuf) -> Self {
        Self {
            manifest_path: root.join(FILE_MANIFEST_PATH),
            commit_log_path: root.join(INDEX_COMMIT_LOG_PATH),
            root,
        }
    }

    fn ensure_store_dirs(&self) -> Result<()> {
        fs::create_dir_all(self.root.join(STORE_DIR))
            .with_context(|| format!("failed to create {}", self.root.join(STORE_DIR).display()))
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
        let payload = records
            .iter()
            .map(serde_json::to_string)
            .collect::<std::result::Result<Vec<_>, _>>()?
            .join("\n");
        fs::write(&self.manifest_path, format!("{payload}\n"))
            .with_context(|| format!("failed to write manifest {}", self.manifest_path.display()))
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
        let payload = serde_json::to_string_pretty(log)?;
        fs::write(&self.commit_log_path, payload).with_context(|| {
            format!(
                "failed to write index commit log {}",
                self.commit_log_path.display()
            )
        })
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
        if self.index_dir.exists() {
            fs::remove_dir_all(&self.index_dir).with_context(|| {
                format!("failed to clear index dir {}", self.index_dir.display())
            })?;
        }
        fs::create_dir_all(&self.index_dir)
            .with_context(|| format!("failed to create index dir {}", self.index_dir.display()))?;
        let schema = keyword_schema();
        let fields = KeywordFields::new(&schema)?;
        let index = Index::create_in_dir(&self.index_dir, schema.clone())?;
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
        fs::create_dir_all(&self.db_dir).with_context(|| {
            format!(
                "failed to create vector index dir {}",
                self.db_dir.display()
            )
        })?;
        let chunk_rows = build_lance_chunk_rows(chunks)?;
        let file_rows = build_lance_file_rows(records, &chunk_rows);
        let file_batch = build_lance_file_batch(&file_rows)?;
        let chunk_batch = build_lance_chunk_batch(&chunk_rows)?;
        let files_schema = lance_file_schema();
        let chunks_schema = lance_chunk_schema();
        let turns_schema = lance_turn_schema();
        let db_dir = self.db_dir.clone();

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

fn build_lance_chunk_rows(chunks: &[Chunk]) -> Result<Vec<LanceChunkRow>> {
    if chunks.is_empty() {
        return Ok(Vec::new());
    }
    let texts = chunks
        .iter()
        .map(|chunk| format!("{}\n{}", chunk.file_path, chunk.content_preview))
        .collect::<Vec<_>>();
    let embeddings = embed_passages(&texts)?;
    Ok(chunks
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
            let mut model = TextEmbedding::try_new(InitOptions::new(EmbeddingModel::AllMiniLML6V2))
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
            let mut model = TextEmbedding::try_new(InitOptions::new(EmbeddingModel::AllMiniLML6V2))
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
                required_frontmatter: vec!["status".to_string()],
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

    #[test]
    fn scan_workspace_writes_manifest_and_keyword_search_finds_matches() {
        let root = temp_root("scan-search");
        seed_repo(&root);
        let documents = OmegaDocument::new(root.clone());

        let scan = documents.scan_workspace().unwrap();
        assert!(scan.files_indexed >= 5);
        assert!(root.join(".omega/store/files.jsonl").exists());

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
