use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use omega_context::{
    ContextDiagnostics, ContextDocumentDiagnostics, ContextFacadeServices,
    ContextMemoryDiagnostics, GovernanceEventSignal, OmegaContextFacade,
};
use serde::{Deserialize, Serialize};

const PROJECT_DIR: &str = ".omega";
const PROJECT_METADATA_FILE: &str = "project.json";
const PROJECT_CONFIG_TOML: &str = "project.toml";
const PROJECT_CONFIG_JSON: &str = "project.json";
const PROJECT_SESSIONS_DIR: &str = "sessions";
const PROJECT_ID_LEN: usize = 12;
const SESSION_SNAPSHOT_SUFFIX: &str = ".snapshot.json";
const SESSION_REPLAY_LOG_SUFFIX: &str = ".log.jsonl";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectRecord {
    pub project_id: String,
    pub display_name: String,
    pub root: PathBuf,
    pub detection_kind: ProjectDetectionKind,
    pub created_at: u64,
    pub last_opened_at: u64,
    pub active_session_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProjectDetectionKind {
    Explicit,
    CurrentFile,
    Cwd,
    LooseDirectory,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectSessionRef {
    pub session_id: String,
    pub title: String,
    pub started_at: u64,
    pub last_active_at: u64,
    pub status: ProjectSessionStatus,
    pub turn_count: u64,
    pub last_user_turn_preview: Option<String>,
    pub resume_ready: bool,
    pub archived_turn_count: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProjectSessionStatus {
    Active,
    Idle,
    Archived,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectKnowledgeSummary {
    pub document: ContextDocumentDiagnostics,
    pub memory: ContextMemoryDiagnostics,
    pub session_count: usize,
    pub active_session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectDetailSnapshot {
    pub record: ProjectRecord,
    pub sessions: Vec<ProjectSessionRef>,
    pub knowledge: ProjectKnowledgeSummary,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectResolutionInput {
    pub current_file_path: Option<PathBuf>,
    pub cwd: PathBuf,
    pub explicit_root: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct ProjectSessionUpdate {
    pub session_id: String,
    pub title: Option<String>,
    pub status: ProjectSessionStatus,
    pub turn_count: u64,
    pub last_user_turn_preview: Option<String>,
    pub archived_turn_count: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ProjectSessionSnapshot {
    pub schema_version: u32,
    pub project_id: String,
    pub session_id: String,
    pub saved_at: u64,
    pub last_completed_turn_id: Option<u64>,
    pub latest_user_turn: Option<String>,
    pub recent_turn_summaries: Vec<ProjectSessionTurnSummary>,
    pub routing: ProjectSessionRoutingSnapshot,
    pub skill_routing: ProjectSkillRoutingSnapshot,
    pub step_summaries: Vec<ProjectSessionStepSummary>,
    pub step_outputs: BTreeMap<String, serde_json::Value>,
    pub governance_events: Vec<GovernanceEventSignal>,
    pub todo_items: Vec<ProjectSessionTodoItem>,
    pub structured_input: Option<serde_json::Value>,
    pub last_known_cwd: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ProjectSessionTurnSummary {
    pub turn_id: u64,
    pub workflow_id: String,
    pub user_intent: String,
    pub summary_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ProjectSessionRoutingSnapshot {
    pub recognized_scene_id: Option<String>,
    pub selected_workflow_id: Option<String>,
    pub active_workflow_id: String,
    pub active_workflow_role: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ProjectSkillRoutingSnapshot {
    pub selected_skill_ids: Vec<String>,
    pub loaded_skill_ids: Vec<String>,
    pub ignored_skill_ids: Vec<String>,
    pub selection_reason: Option<String>,
    pub source_step_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ProjectSessionStepSummary {
    pub workflow_id: String,
    pub step_id: String,
    pub title: String,
    pub summary: String,
    pub estimated_tokens: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectSessionTodoItem {
    pub id: String,
    pub text: String,
    pub status: ProjectSessionTodoStatus,
    pub active_form: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectSessionTodoStatus {
    Pending,
    InProgress,
    Completed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionReplayEntry {
    pub session_id: String,
    pub recorded_at: u64,
    pub kind: SessionReplayEntryKind,
    pub title: Option<String>,
    pub body: String,
    pub state: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionReplayEntryKind {
    UserTurn,
    AssistantResponse,
    CommandSection,
    ToolSummary,
    SystemNotice,
}

pub struct ProjectRegistry {
    handles: Mutex<BTreeMap<String, Arc<OmegaProjectHandle>>>,
}

impl Default for ProjectRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ProjectRegistry {
    pub fn new() -> Self {
        Self {
            handles: Mutex::new(BTreeMap::new()),
        }
    }

    pub fn resolve(&self, input: ProjectResolutionInput) -> Result<Arc<OmegaProjectHandle>> {
        let resolved = resolve_project_record(&input)?;
        let mut handles = self.handles.lock().unwrap();
        if let Some(existing) = handles.get(&resolved.project_id) {
            existing.touch()?;
            return Ok(Arc::clone(existing));
        }

        let handle = Arc::new(OmegaProjectHandle::open(resolved)?);
        handles.insert(handle.project_id().to_string(), Arc::clone(&handle));
        Ok(handle)
    }

    pub fn list(&self) -> Vec<ProjectRecord> {
        let mut records = self
            .handles
            .lock()
            .unwrap()
            .values()
            .map(|handle| handle.record())
            .collect::<Vec<_>>();
        records.sort_by(|left, right| {
            right
                .last_opened_at
                .cmp(&left.last_opened_at)
                .then_with(|| left.display_name.cmp(&right.display_name))
        });
        records
    }

    pub fn forget(&self, project_id: &str) -> Option<Arc<OmegaProjectHandle>> {
        self.handles.lock().unwrap().remove(project_id)
    }
}

pub struct OmegaProjectHandle {
    record: Mutex<ProjectRecord>,
    context_facade: Arc<OmegaContextFacade>,
}

impl OmegaProjectHandle {
    pub fn open(record: ProjectRecord) -> Result<Self> {
        ensure_project_layout(&record.root)?;
        write_project_record(&record)?;
        Ok(Self {
            context_facade: Arc::new(OmegaContextFacade::from_services(
                ContextFacadeServices::local(record.root.clone()),
            )),
            record: Mutex::new(record),
        })
    }

    pub fn project_id(&self) -> String {
        self.record.lock().unwrap().project_id.clone()
    }

    pub fn root(&self) -> PathBuf {
        self.record.lock().unwrap().root.clone()
    }

    pub fn display_name(&self) -> String {
        self.record.lock().unwrap().display_name.clone()
    }

    pub fn record(&self) -> ProjectRecord {
        self.record.lock().unwrap().clone()
    }

    pub fn context_facade(&self) -> Arc<OmegaContextFacade> {
        Arc::clone(&self.context_facade)
    }

    pub fn touch(&self) -> Result<()> {
        let mut record = self.record.lock().unwrap();
        record.last_opened_at = now_unix_seconds();
        write_project_record(&record)
    }

    pub fn upsert_session(&self, update: ProjectSessionUpdate) -> Result<ProjectSessionRef> {
        let session_id = update.session_id.clone();
        let session_path = self.session_path(&update.session_id);
        let now = now_unix_seconds();
        let existing = if session_path.exists() {
            Some(read_session_record(&session_path)?)
        } else {
            None
        };
        let session = ProjectSessionRef {
            session_id,
            title: update
                .title
                .or_else(|| existing.as_ref().map(|entry| entry.title.clone()))
                .unwrap_or_else(|| "Untitled Session".to_string()),
            started_at: existing.as_ref().map(|entry| entry.started_at).unwrap_or(now),
            last_active_at: now,
            status: update.status,
            turn_count: update.turn_count,
            last_user_turn_preview: update.last_user_turn_preview,
            resume_ready: self.snapshot_path(&update.session_id).exists()
                && self.replay_log_path(&update.session_id).exists(),
            archived_turn_count: update
                .archived_turn_count
                .or_else(|| existing.as_ref().map(|entry| entry.archived_turn_count))
                .unwrap_or(0),
        };
        write_session_record(&session_path, &session)?;

        let mut record = self.record.lock().unwrap();
        record.active_session_id = match session.status {
            ProjectSessionStatus::Active => Some(session.session_id.clone()),
            ProjectSessionStatus::Idle | ProjectSessionStatus::Archived => record
                .active_session_id
                .clone()
                .filter(|active| active != &session.session_id),
        };
        record.last_opened_at = now;
        write_project_record(&record)?;

        Ok(session)
    }

    pub fn list_sessions(&self) -> Result<Vec<ProjectSessionRef>> {
        let sessions_dir = self.sessions_dir();
        if !sessions_dir.exists() {
            return Ok(Vec::new());
        }

        let mut sessions = fs::read_dir(&sessions_dir)
            .with_context(|| format!("read project sessions directory {}", sessions_dir.display()))?
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "json"))
            .filter(|entry| {
                entry
                    .path()
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| {
                        !name.ends_with(SESSION_SNAPSHOT_SUFFIX)
                            && !name.ends_with(SESSION_REPLAY_LOG_SUFFIX)
                    })
            })
            .map(|entry| read_session_record(&entry.path()))
            .collect::<Result<Vec<_>>>()?;

        sessions.sort_by(|left, right| {
            session_status_rank(left.status)
                .cmp(&session_status_rank(right.status))
                .then_with(|| right.last_active_at.cmp(&left.last_active_at))
                .then_with(|| left.session_id.cmp(&right.session_id))
        });
        Ok(sessions)
    }

    pub fn detail_snapshot(&self) -> Result<ProjectDetailSnapshot> {
        let diagnostics = self.context_facade.diagnostics.context_diagnostics();
        let record = self.record();
        let sessions = self.list_sessions()?;
        Ok(ProjectDetailSnapshot {
            knowledge: ProjectKnowledgeSummary {
                document: diagnostics.document,
                memory: diagnostics.memory,
                session_count: sessions.len(),
                active_session_id: record.active_session_id.clone(),
            },
            record,
            sessions,
        })
    }

    pub fn context_diagnostics(&self) -> ContextDiagnostics {
        self.context_facade.diagnostics.context_diagnostics()
    }

    pub fn load_session(&self, session_id: &str) -> Result<ProjectSessionRef> {
        read_session_record(&self.session_path(session_id))
    }

    pub fn save_session_snapshot(&self, snapshot: &ProjectSessionSnapshot) -> Result<()> {
        let path = self.snapshot_path(&snapshot.session_id);
        write_json_record(&path, snapshot)?;
        self.refresh_resume_ready(&snapshot.session_id)?;
        Ok(())
    }

    pub fn load_session_snapshot(&self, session_id: &str) -> Result<Option<ProjectSessionSnapshot>> {
        let path = self.snapshot_path(session_id);
        if !path.exists() {
            return Ok(None);
        }
        read_json_record(&path).map(Some)
    }

    pub fn append_replay_entries(
        &self,
        session_id: &str,
        entries: &[SessionReplayEntry],
    ) -> Result<()> {
        if entries.is_empty() {
            return self.refresh_resume_ready(session_id).map(|_| ());
        }
        let path = self.replay_log_path(session_id);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("create replay log directory {}", parent.display()))?;
        }
        let mut bytes = if path.exists() {
            fs::read(&path).with_context(|| format!("read replay log {}", path.display()))?
        } else {
            Vec::new()
        };
        for entry in entries {
            bytes.extend_from_slice(&serde_json::to_vec(entry)?);
            bytes.push(b'\n');
        }
        fs::write(&path, bytes).with_context(|| format!("write replay log {}", path.display()))?;
        self.refresh_resume_ready(session_id)?;
        Ok(())
    }

    pub fn load_replay_log(&self, session_id: &str) -> Result<Vec<SessionReplayEntry>> {
        let path = self.replay_log_path(session_id);
        if !path.exists() {
            return Ok(Vec::new());
        }
        let content = fs::read_to_string(&path)
            .with_context(|| format!("read replay log {}", path.display()))?;
        content
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| serde_json::from_str(line).map_err(anyhow::Error::from))
            .collect()
    }

    pub fn delete_session_artifacts(&self, session_id: &str) -> Result<()> {
        let session_path = self.session_path(session_id);
        if session_path.exists() {
            fs::remove_file(&session_path)
                .with_context(|| format!("delete session record {}", session_path.display()))?;
        }
        let snapshot_path = self.snapshot_path(session_id);
        if snapshot_path.exists() {
            fs::remove_file(&snapshot_path)
                .with_context(|| format!("delete session snapshot {}", snapshot_path.display()))?;
        }
        let replay_path = self.replay_log_path(session_id);
        if replay_path.exists() {
            fs::remove_file(&replay_path)
                .with_context(|| format!("delete replay log {}", replay_path.display()))?;
        }

        let mut record = self.record.lock().unwrap();
        if record.active_session_id.as_deref() == Some(session_id) {
            record.active_session_id = None;
            write_project_record(&record)?;
        }
        Ok(())
    }

    pub fn delete_local_state(&self) -> Result<()> {
        let omega_dir = self.root().join(PROJECT_DIR);
        if omega_dir.exists() {
            fs::remove_dir_all(&omega_dir)
                .with_context(|| format!("delete project state {}", omega_dir.display()))?;
        }
        Ok(())
    }

    fn sessions_dir(&self) -> PathBuf {
        self.root().join(PROJECT_DIR).join(PROJECT_SESSIONS_DIR)
    }

    fn session_path(&self, session_id: &str) -> PathBuf {
        self.sessions_dir().join(format!("{session_id}.json"))
    }

    fn snapshot_path(&self, session_id: &str) -> PathBuf {
        self.sessions_dir()
            .join(format!("{session_id}{SESSION_SNAPSHOT_SUFFIX}"))
    }

    fn replay_log_path(&self, session_id: &str) -> PathBuf {
        self.sessions_dir()
            .join(format!("{session_id}{SESSION_REPLAY_LOG_SUFFIX}"))
    }

    fn refresh_resume_ready(&self, session_id: &str) -> Result<ProjectSessionRef> {
        let session_path = self.session_path(session_id);
        let mut record = read_session_record(&session_path)?;
        record.resume_ready = self.snapshot_path(session_id).exists()
            && self.replay_log_path(session_id).exists();
        write_session_record(&session_path, &record)?;
        Ok(record)
    }
}

fn resolve_project_record(input: &ProjectResolutionInput) -> Result<ProjectRecord> {
    let resolution = if let Some(explicit_root) = input.explicit_root.as_deref() {
        ProjectResolution {
            root: resolve_explicit_root(explicit_root)?,
            detection_kind: ProjectDetectionKind::Explicit,
        }
    } else if let Some(current_file) = input.current_file_path.as_deref() {
        ProjectResolution {
            root: detect_project_root(current_file, true)?,
            detection_kind: detect_kind_for_path(current_file, true),
        }
    } else {
        ProjectResolution {
            root: detect_project_root(&input.cwd, false)?,
            detection_kind: detect_kind_for_path(&input.cwd, false),
        }
    };

    let metadata_path = resolution.root.join(PROJECT_DIR).join(PROJECT_METADATA_FILE);
    if metadata_path.exists() {
        let mut record = read_project_record(&metadata_path)?;
        record.last_opened_at = now_unix_seconds();
        record.detection_kind = resolution.detection_kind;
        return Ok(record);
    }

    let project_id = stable_project_id(&resolution.root);
    let display_name = read_project_name(&resolution.root)?.unwrap_or_else(|| {
        resolution
            .root
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_else(|| resolution.root.as_os_str().to_str().unwrap_or("project"))
            .to_string()
    });
    let now = now_unix_seconds();
    Ok(ProjectRecord {
        project_id,
        display_name,
        root: resolution.root,
        detection_kind: resolution.detection_kind,
        created_at: now,
        last_opened_at: now,
        active_session_id: None,
    })
}

#[derive(Debug, Clone)]
struct ProjectResolution {
    root: PathBuf,
    detection_kind: ProjectDetectionKind,
}

fn detect_project_root(path: &Path, is_file_hint: bool) -> Result<PathBuf> {
    let start = if is_file_hint && path.is_file() {
        path.parent().unwrap_or(path)
    } else {
        path
    };

    if let Some(root) = walk_project_markers(start) {
        return canonicalize_lossy(&root);
    }

    canonicalize_lossy(start)
}

fn resolve_explicit_root(path: &Path) -> Result<PathBuf> {
    let root = if path.is_file() {
        path.parent().unwrap_or(path)
    } else {
        path
    };
    canonicalize_lossy(root)
}

fn walk_project_markers(start: &Path) -> Option<PathBuf> {
    for candidate in start.ancestors() {
        if candidate.join(PROJECT_DIR).join(PROJECT_CONFIG_TOML).exists()
            || candidate.join(PROJECT_DIR).join(PROJECT_CONFIG_JSON).exists()
            || candidate.join(".git").exists()
            || candidate.join("Cargo.toml").exists()
            || candidate.join("package.json").exists()
        {
            return Some(candidate.to_path_buf());
        }
    }
    None
}

fn detect_kind_for_path(path: &Path, is_file_hint: bool) -> ProjectDetectionKind {
    if walk_project_markers(if is_file_hint && path.is_file() {
        path.parent().unwrap_or(path)
    } else {
        path
    })
    .is_some()
    {
        if is_file_hint {
            ProjectDetectionKind::CurrentFile
        } else {
            ProjectDetectionKind::Cwd
        }
    } else {
        ProjectDetectionKind::LooseDirectory
    }
}

fn read_project_name(root: &Path) -> Result<Option<String>> {
    let toml_path = root.join(PROJECT_DIR).join(PROJECT_CONFIG_TOML);
    if toml_path.exists() {
        let content = fs::read_to_string(&toml_path)
            .with_context(|| format!("read project config {}", toml_path.display()))?;
        let parsed: ProjectConfigToml = toml::from_str(&content)
            .with_context(|| format!("parse project config {}", toml_path.display()))?;
        if parsed.name.as_deref().is_some_and(|value| !value.trim().is_empty()) {
            return Ok(parsed.name);
        }
    }

    let json_path = root.join(PROJECT_DIR).join(PROJECT_CONFIG_JSON);
    if json_path.exists() {
        let content = fs::read_to_string(&json_path)
            .with_context(|| format!("read project config {}", json_path.display()))?;
        let parsed: ProjectConfigJson = serde_json::from_str(&content)
            .with_context(|| format!("parse project config {}", json_path.display()))?;
        if parsed.name.as_deref().is_some_and(|value| !value.trim().is_empty()) {
            return Ok(parsed.name);
        }
    }

    Ok(None)
}

#[derive(Debug, Deserialize)]
struct ProjectConfigToml {
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ProjectConfigJson {
    name: Option<String>,
}

fn ensure_project_layout(root: &Path) -> Result<()> {
    fs::create_dir_all(root.join(PROJECT_DIR).join(PROJECT_SESSIONS_DIR)).with_context(|| {
        format!(
            "create project layout at {}",
            root.join(PROJECT_DIR).display()
        )
    })?;
    Ok(())
}

fn write_project_record(record: &ProjectRecord) -> Result<()> {
    ensure_project_layout(&record.root)?;
    let path = record.root.join(PROJECT_DIR).join(PROJECT_METADATA_FILE);
    fs::write(&path, serde_json::to_vec_pretty(record)?)
        .with_context(|| format!("write project record {}", path.display()))?;
    Ok(())
}

fn read_project_record(path: &Path) -> Result<ProjectRecord> {
    let content = fs::read(path).with_context(|| format!("read project record {}", path.display()))?;
    serde_json::from_slice(&content)
        .with_context(|| format!("parse project record {}", path.display()))
}

fn write_session_record(path: &Path, record: &ProjectSessionRef) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create session catalog {}", parent.display()))?;
    }
    fs::write(path, serde_json::to_vec_pretty(record)?)
        .with_context(|| format!("write session record {}", path.display()))?;
    Ok(())
}

fn write_json_record<T>(path: &Path, value: &T) -> Result<()>
where
    T: Serialize,
{
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create record directory {}", parent.display()))?;
    }
    fs::write(path, serde_json::to_vec_pretty(value)?)
        .with_context(|| format!("write json record {}", path.display()))?;
    Ok(())
}

fn read_json_record<T>(path: &Path) -> Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    let content = fs::read(path).with_context(|| format!("read json record {}", path.display()))?;
    serde_json::from_slice(&content)
        .with_context(|| format!("parse json record {}", path.display()))
}

fn read_session_record(path: &Path) -> Result<ProjectSessionRef> {
    let content = fs::read(path).with_context(|| format!("read session record {}", path.display()))?;
    serde_json::from_slice(&content)
        .with_context(|| format!("parse session record {}", path.display()))
}

fn canonicalize_lossy(path: &Path) -> Result<PathBuf> {
    if path.exists() {
        Ok(path.canonicalize().with_context(|| format!("canonicalize {}", path.display()))?)
    } else {
        Ok(path.to_path_buf())
    }
}

fn stable_project_id(root: &Path) -> String {
    let digest = blake3::hash(root.to_string_lossy().as_bytes()).to_hex();
    digest[..PROJECT_ID_LEN].to_string()
}

fn session_status_rank(status: ProjectSessionStatus) -> u8 {
    match status {
        ProjectSessionStatus::Active => 0,
        ProjectSessionStatus::Idle => 1,
        ProjectSessionStatus::Archived => 2,
    }
}

fn now_unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
    .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_project_root_from_current_file() {
        let temp_dir = tempfile::tempdir().unwrap();
        let root = temp_dir.path().join("workspace");
        let nested = root.join("crates/omega-app/src");
        fs::create_dir_all(&nested).unwrap();
        fs::write(root.join("Cargo.toml"), "[workspace]\n").unwrap();
        let current_file = nested.join("main.rs");
        fs::write(&current_file, "fn main() {}\n").unwrap();

        let registry = ProjectRegistry::new();
        let handle = registry
            .resolve(ProjectResolutionInput {
                current_file_path: Some(current_file),
                cwd: nested.clone(),
                explicit_root: None,
            })
            .unwrap();

        assert_eq!(handle.root(), root.canonicalize().unwrap());
        assert_eq!(handle.record().detection_kind, ProjectDetectionKind::CurrentFile);
        assert!(root.join(PROJECT_DIR).join(PROJECT_METADATA_FILE).exists());
    }

    #[test]
    fn explicit_root_overrides_detected_markers() {
        let temp_dir = tempfile::tempdir().unwrap();
        let root = temp_dir.path().join("explicit-root");
        let child = root.join("nested/workspace");
        fs::create_dir_all(&child).unwrap();
        fs::create_dir_all(child.join(".git")).unwrap();

        let registry = ProjectRegistry::new();
        let handle = registry
            .resolve(ProjectResolutionInput {
                current_file_path: None,
                cwd: child.clone(),
                explicit_root: Some(root.clone()),
            })
            .unwrap();

        assert_eq!(handle.root(), root.canonicalize().unwrap());
        assert_eq!(handle.record().detection_kind, ProjectDetectionKind::Explicit);
    }

    #[test]
    fn session_catalog_persists_and_sorts_by_last_activity() {
        let temp_dir = tempfile::tempdir().unwrap();
        let root = temp_dir.path().join("workspace");
        fs::create_dir_all(&root).unwrap();

        let registry = ProjectRegistry::new();
        let handle = registry
            .resolve(ProjectResolutionInput {
                current_file_path: None,
                cwd: root.clone(),
                explicit_root: Some(root.clone()),
            })
            .unwrap();

        handle
            .upsert_session(ProjectSessionUpdate {
                session_id: "session-a".to_string(),
                title: Some("Session A".to_string()),
                status: ProjectSessionStatus::Idle,
                turn_count: 2,
                last_user_turn_preview: Some("first".to_string()),
                archived_turn_count: None,
            })
            .unwrap();
        handle
            .upsert_session(ProjectSessionUpdate {
                session_id: "session-b".to_string(),
                title: Some("Session B".to_string()),
                status: ProjectSessionStatus::Active,
                turn_count: 4,
                last_user_turn_preview: Some("second".to_string()),
                archived_turn_count: None,
            })
            .unwrap();

        let sessions = handle.list_sessions().unwrap();
        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0].session_id, "session-b");
        assert_eq!(handle.record().active_session_id.as_deref(), Some("session-b"));
        assert!(!sessions[0].resume_ready);
    }

    #[test]
    fn session_snapshot_and_replay_log_round_trip_updates_resume_ready() {
        let temp_dir = tempfile::tempdir().unwrap();
        let root = temp_dir.path().join("workspace");
        fs::create_dir_all(&root).unwrap();

        let registry = ProjectRegistry::new();
        let handle = registry
            .resolve(ProjectResolutionInput {
                current_file_path: None,
                cwd: root.clone(),
                explicit_root: Some(root.clone()),
            })
            .unwrap();

        handle
            .upsert_session(ProjectSessionUpdate {
                session_id: "session-a".to_string(),
                title: Some("Session A".to_string()),
                status: ProjectSessionStatus::Idle,
                turn_count: 2,
                last_user_turn_preview: Some("first".to_string()),
                archived_turn_count: Some(3),
            })
            .unwrap();

        handle
            .save_session_snapshot(&ProjectSessionSnapshot {
                schema_version: 1,
                project_id: handle.project_id(),
                session_id: "session-a".to_string(),
                saved_at: 42,
                last_completed_turn_id: Some(2),
                latest_user_turn: Some("resume me".to_string()),
                recent_turn_summaries: vec![ProjectSessionTurnSummary {
                    turn_id: 2,
                    workflow_id: "feature".to_string(),
                    user_intent: "resume me".to_string(),
                    summary_count: 1,
                }],
                selected_workflow_id: Some("feature".to_string()),
                selected_skill_ids: vec!["review".to_string()],
                loaded_skill_ids: vec!["review".to_string()],
                todo_snapshot: vec!["todo one".to_string()],
                structured_input: None,
                last_known_cwd: Some(root.clone()),
            })
            .unwrap();

        handle
            .append_replay_entries(
                "session-a",
                &[SessionReplayEntry {
                    session_id: "session-a".to_string(),
                    recorded_at: 43,
                    kind: SessionReplayEntryKind::AssistantResponse,
                    title: Some("summary".to_string()),
                    body: "completed work".to_string(),
                    state: Some("complete".to_string()),
                }],
            )
            .unwrap();

        let session = handle.load_session("session-a").unwrap();
        assert!(session.resume_ready);
        assert_eq!(session.archived_turn_count, 3);

        let snapshot = handle.load_session_snapshot("session-a").unwrap().unwrap();
        assert_eq!(snapshot.last_completed_turn_id, Some(2));
        assert_eq!(snapshot.selected_skill_ids, vec!["review".to_string()]);

        let replay = handle.load_replay_log("session-a").unwrap();
        assert_eq!(replay.len(), 1);
        assert_eq!(replay[0].body, "completed work");
    }

    #[test]
    fn delete_session_artifacts_removes_catalog_snapshot_and_replay_log() {
        let temp_dir = tempfile::tempdir().unwrap();
        let root = temp_dir.path().join("workspace");
        fs::create_dir_all(&root).unwrap();

        let registry = ProjectRegistry::new();
        let handle = registry
            .resolve(ProjectResolutionInput {
                current_file_path: None,
                cwd: root.clone(),
                explicit_root: Some(root.clone()),
            })
            .unwrap();

        handle
            .upsert_session(ProjectSessionUpdate {
                session_id: "session-a".to_string(),
                title: Some("Session A".to_string()),
                status: ProjectSessionStatus::Active,
                turn_count: 2,
                last_user_turn_preview: Some("first".to_string()),
                archived_turn_count: Some(1),
            })
            .unwrap();
        handle
            .save_session_snapshot(&ProjectSessionSnapshot {
                schema_version: 1,
                project_id: handle.project_id(),
                session_id: "session-a".to_string(),
                saved_at: 42,
                last_completed_turn_id: Some(2),
                latest_user_turn: Some("resume me".to_string()),
                recent_turn_summaries: Vec::new(),
                selected_workflow_id: None,
                selected_skill_ids: Vec::new(),
                loaded_skill_ids: Vec::new(),
                todo_snapshot: Vec::new(),
                structured_input: None,
                last_known_cwd: None,
            })
            .unwrap();
        handle
            .append_replay_entries(
                "session-a",
                &[SessionReplayEntry {
                    session_id: "session-a".to_string(),
                    recorded_at: 43,
                    kind: SessionReplayEntryKind::SystemNotice,
                    title: None,
                    body: "note".to_string(),
                    state: None,
                }],
            )
            .unwrap();

        handle.delete_session_artifacts("session-a").unwrap();

        assert!(handle.load_session_snapshot("session-a").unwrap().is_none());
        assert!(handle.load_replay_log("session-a").unwrap().is_empty());
        assert!(handle.load_session("session-a").is_err());
        assert_eq!(handle.record().active_session_id, None);
    }

    #[test]
    fn project_name_prefers_repo_local_config() {
        let temp_dir = tempfile::tempdir().unwrap();
        let root = temp_dir.path().join("workspace");
        fs::create_dir_all(root.join(PROJECT_DIR)).unwrap();
        fs::write(
            root.join(PROJECT_DIR).join(PROJECT_CONFIG_TOML),
            "name = \"Omega Docs\"\n",
        )
        .unwrap();

        let registry = ProjectRegistry::new();
        let handle = registry
            .resolve(ProjectResolutionInput {
                current_file_path: None,
                cwd: root.clone(),
                explicit_root: Some(root.clone()),
            })
            .unwrap();

        assert_eq!(handle.display_name(), "Omega Docs");
    }
}