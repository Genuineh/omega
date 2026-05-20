use std::cmp::Ordering;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

use anyhow::{Context, Result};
use omega_project_layout::OmegaProjectLayout;
use serde::{Deserialize, Serialize};

pub const PLAN_SCHEMA_VERSION: u32 = 1;
const TASK_ORDER_GAP: i64 = 1_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PlannedTaskKind {
    #[default]
    Task,
    Feature,
    Research,
    Refactor,
    Chore,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PlannedTaskStatus {
    #[serde(alias = "pending")]
    #[default]
    Backlog,
    Ready,
    #[serde(alias = "in-progress", alias = "inprogress")]
    InProgress,
    Blocked,
    #[serde(alias = "completed")]
    Done,
    #[serde(alias = "replaced")]
    Archived,
}

impl PlannedTaskStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Backlog => "backlog",
            Self::Ready => "ready",
            Self::InProgress => "in_progress",
            Self::Blocked => "blocked",
            Self::Done => "done",
            Self::Archived => "archived",
        }
    }

    pub fn parse_cli(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "backlog" => Some(Self::Backlog),
            "ready" => Some(Self::Ready),
            "in_progress" | "in-progress" | "inprogress" => Some(Self::InProgress),
            "blocked" => Some(Self::Blocked),
            "done" => Some(Self::Done),
            "archived" => Some(Self::Archived),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TaskPriority {
    P0,
    P1,
    #[default]
    P2,
    P3,
}

impl TaskPriority {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::P0 => "p0",
            Self::P1 => "p1",
            Self::P2 => "p2",
            Self::P3 => "p3",
        }
    }

    pub fn parse_cli(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "p0" => Some(Self::P0),
            "p1" => Some(Self::P1),
            "p2" => Some(Self::P2),
            "p3" => Some(Self::P3),
            _ => None,
        }
    }

    fn rank(self) -> u8 {
        match self {
            Self::P0 => 0,
            Self::P1 => 1,
            Self::P2 => 2,
            Self::P3 => 3,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TaskArtifactKind {
    Prd,
    Spec,
    Guide,
    Adr,
    #[default]
    Code,
    Test,
    Delivery,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TaskArtifactLink {
    pub kind: TaskArtifactKind,
    pub path: String,
    pub label: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlannedTask {
    pub id: String,
    pub title: String,
    pub kind: PlannedTaskKind,
    pub status: PlannedTaskStatus,
    pub priority: TaskPriority,
    pub order_key: i64,
    pub summary: String,
    pub requirement: String,
    pub acceptance: Vec<String>,
    pub parent_id: Option<String>,
    pub depends_on: Vec<String>,
    pub tags: Vec<String>,
    pub design_links: Vec<TaskArtifactLink>,
    pub implementation_links: Vec<TaskArtifactLink>,
    #[serde(default)]
    pub presentation_links: Vec<TaskArtifactLink>,
    #[serde(default)]
    pub doc_scope: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectPlanManifest {
    pub schema_version: u32,
    pub next_task_seq: u64,
}

impl Default for ProjectPlanManifest {
    fn default() -> Self {
        Self {
            schema_version: PLAN_SCHEMA_VERSION,
            next_task_seq: 1,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TaskListFilter {
    pub status: Option<PlannedTaskStatus>,
    pub priority: Option<TaskPriority>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewPlannedTask {
    pub title: String,
    pub kind: PlannedTaskKind,
    pub status: PlannedTaskStatus,
    pub priority: TaskPriority,
    pub summary: String,
    pub requirement: String,
    pub acceptance: Vec<String>,
    pub parent_id: Option<String>,
    pub depends_on: Vec<String>,
    pub tags: Vec<String>,
    pub doc_scope: Vec<String>,
    pub presentation_links: Vec<TaskArtifactLink>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlannedTaskUpdate {
    pub title: Option<String>,
    pub summary: Option<String>,
    pub requirement: Option<String>,
    pub status: Option<PlannedTaskStatus>,
    pub acceptance: Option<Vec<String>>,
    pub tags: Option<Vec<String>>,
    pub doc_scope: Option<Vec<String>>,
    pub presentation_links: Option<Vec<TaskArtifactLink>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TaskOrderPlacement {
    pub before_task_id: Option<String>,
    pub after_task_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskDependencyOperation {
    Add,
    Remove,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskLinkSurface {
    Design,
    Implementation,
}

impl TaskLinkSurface {
    pub fn parse_cli(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "design" => Some(Self::Design),
            "implementation" => Some(Self::Implementation),
            _ => None,
        }
    }
}

impl NewPlannedTask {
    pub fn simple(title: impl Into<String>, priority: TaskPriority) -> Self {
        let title = title.into();
        Self {
            summary: title.clone(),
            requirement: title.clone(),
            title,
            kind: PlannedTaskKind::Task,
            status: PlannedTaskStatus::Backlog,
            priority,
            acceptance: Vec::new(),
            parent_id: None,
            depends_on: Vec::new(),
            tags: Vec::new(),
            doc_scope: Vec::new(),
            presentation_links: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskLogKind {
    Created,
    NoteAdded,
    DeliveryAttached,
    PartialDelivery,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskActor {
    User,
    Assistant,
    System,
    Command,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskLogEntry {
    pub seq: u64,
    pub kind: TaskLogKind,
    pub actor: TaskActor,
    pub summary: String,
    pub related_session_id: Option<String>,
    pub related_turn_id: Option<u64>,
    pub related_delivery_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectedProjectTaskContext {
    pub task_id: String,
    pub title: String,
    pub requirement: String,
    pub acceptance: Vec<String>,
    pub dependency_chain: Vec<String>,
    pub recent_logs: Vec<TaskLogEntry>,
    pub design_links: Vec<TaskArtifactLink>,
    pub implementation_links: Vec<TaskArtifactLink>,
    pub presentation_links: Vec<TaskArtifactLink>,
}

pub trait ProjectPlanAccess: Send + Sync {
    fn resolve_task_context(&self, task_id: &str) -> Result<Option<SelectedProjectTaskContext>>;
    fn get_task(&self, task_id: &str) -> Result<Option<PlannedTask>>;
    fn list_tasks(&self, filter: TaskListFilter) -> Result<Vec<PlannedTask>>;
}

#[derive(Debug, Clone)]
pub struct ProjectPlanStore {
    layout: OmegaProjectLayout,
}

impl ProjectPlanStore {
    pub fn open_or_scaffold(root: impl AsRef<Path>) -> Result<Self> {
        let store = Self {
            layout: OmegaProjectLayout::new(root.as_ref().to_path_buf()),
        };
        store.ensure_layout()?;
        let manifest_path = store.layout.docs_data_project_plan_manifest_path();
        if !manifest_path.exists() {
            store.save_manifest(&ProjectPlanManifest::default())?;
        }
        let manifest = store.load_manifest()?;
        if manifest.schema_version > PLAN_SCHEMA_VERSION {
            anyhow::bail!(
                "unsupported project plan schema {}; supported up to {}",
                manifest.schema_version,
                PLAN_SCHEMA_VERSION
            );
        }
        Ok(store)
    }

    pub fn create_task(&self, draft: NewPlannedTask) -> Result<PlannedTask> {
        let mut manifest = self.load_manifest()?;
        let id = format!("TASK-{:04}", manifest.next_task_seq);
        manifest.next_task_seq += 1;
        let task = PlannedTask {
            id: id.clone(),
            title: draft.title.trim().to_string(),
            kind: draft.kind,
            status: draft.status,
            priority: draft.priority,
            order_key: self.next_order_key(draft.priority)?,
            summary: draft.summary.trim().to_string(),
            requirement: draft.requirement.trim().to_string(),
            acceptance: draft.acceptance,
            parent_id: draft.parent_id,
            depends_on: draft.depends_on,
            tags: draft.tags,
            design_links: Vec::new(),
            implementation_links: Vec::new(),
            presentation_links: draft.presentation_links,
            doc_scope: draft.doc_scope,
        };
        self.save_task(&task)?;
        self.save_manifest(&manifest)?;
        self.append_log(
            &task.id,
            TaskLogKind::Created,
            TaskActor::Command,
            format!("Created task {}", task.title),
            None,
            None,
            None,
        )?;
        Ok(task)
    }

    pub fn update_task(&self, task_id: &str, update: PlannedTaskUpdate) -> Result<PlannedTask> {
        let mut task = self
            .get_task(task_id)?
            .ok_or_else(|| anyhow::anyhow!("unknown task '{task_id}'"))?;
        if let Some(title) = update.title {
            task.title = title.trim().to_string();
        }
        if let Some(summary) = update.summary {
            task.summary = summary.trim().to_string();
        }
        if let Some(requirement) = update.requirement {
            task.requirement = requirement.trim().to_string();
        }
        if let Some(status) = update.status {
            task.status = status;
        }
        if let Some(acceptance) = update.acceptance {
            task.acceptance = acceptance;
        }
        if let Some(tags) = update.tags {
            task.tags = tags;
        }
        if let Some(doc_scope) = update.doc_scope {
            task.doc_scope = doc_scope;
        }
        if let Some(presentation_links) = update.presentation_links {
            task.presentation_links = presentation_links;
        }
        self.save_task(&task)?;
        self.append_log(
            &task.id,
            TaskLogKind::NoteAdded,
            TaskActor::Command,
            "Updated task fields",
            None,
            None,
            None,
        )?;
        Ok(task)
    }

    pub fn reprioritize_task(
        &self,
        task_id: &str,
        priority: TaskPriority,
        placement: TaskOrderPlacement,
    ) -> Result<PlannedTask> {
        if placement.before_task_id.is_some() && placement.after_task_id.is_some() {
            anyhow::bail!("cannot specify both --before and --after");
        }
        let mut task = self
            .get_task(task_id)?
            .ok_or_else(|| anyhow::anyhow!("unknown task '{task_id}'"))?;
        let mut band = self
            .list_tasks(TaskListFilter {
                status: None,
                priority: Some(priority),
            })?
            .into_iter()
            .filter(|candidate| candidate.id != task_id)
            .collect::<Vec<_>>();

        let insert_index = if let Some(before_id) = placement.before_task_id.as_deref() {
            let index = band
                .iter()
                .position(|candidate| candidate.id == before_id)
                .ok_or_else(|| anyhow::anyhow!("unknown reference task '{before_id}'"))?;
            index
        } else if let Some(after_id) = placement.after_task_id.as_deref() {
            let index = band
                .iter()
                .position(|candidate| candidate.id == after_id)
                .ok_or_else(|| anyhow::anyhow!("unknown reference task '{after_id}'"))?;
            index + 1
        } else {
            band.len()
        };

        task.priority = priority;
        band.insert(insert_index, task.clone());
        for (index, candidate) in band.iter_mut().enumerate() {
            candidate.order_key = ((index as i64) + 1) * TASK_ORDER_GAP;
            self.save_task(candidate)?;
        }
        self.append_log(
            task_id,
            TaskLogKind::NoteAdded,
            TaskActor::Command,
            format!("Reprioritized task to {}", priority.as_str()),
            None,
            None,
            None,
        )?;
        self.get_task(task_id)?
            .ok_or_else(|| anyhow::anyhow!("unknown task '{task_id}' after reprioritize"))
    }

    pub fn mutate_dependency(
        &self,
        task_id: &str,
        other_id: &str,
        operation: TaskDependencyOperation,
    ) -> Result<PlannedTask> {
        let mut task = self
            .get_task(task_id)?
            .ok_or_else(|| anyhow::anyhow!("unknown task '{task_id}'"))?;
        let _other = self
            .get_task(other_id)?
            .ok_or_else(|| anyhow::anyhow!("unknown task '{other_id}'"))?;
        if task_id == other_id {
            anyhow::bail!("task cannot depend on itself");
        }

        match operation {
            TaskDependencyOperation::Add => {
                if !task
                    .depends_on
                    .iter()
                    .any(|dependency| dependency == other_id)
                {
                    if self.dependency_reaches(other_id, task_id)? {
                        anyhow::bail!(
                            "adding dependency {task_id} -> {other_id} would create a cycle"
                        );
                    }
                    task.depends_on.push(other_id.to_string());
                    task.depends_on.sort();
                }
            }
            TaskDependencyOperation::Remove => {
                task.depends_on.retain(|dependency| dependency != other_id);
            }
        }

        self.save_task(&task)?;
        self.append_log(
            &task.id,
            TaskLogKind::NoteAdded,
            TaskActor::Command,
            match operation {
                TaskDependencyOperation::Add => format!("Added dependency on {other_id}"),
                TaskDependencyOperation::Remove => format!("Removed dependency on {other_id}"),
            },
            None,
            None,
            None,
        )?;
        Ok(task)
    }

    pub fn add_artifact_link(
        &self,
        task_id: &str,
        surface: TaskLinkSurface,
        link: TaskArtifactLink,
    ) -> Result<PlannedTask> {
        let mut task = self
            .get_task(task_id)?
            .ok_or_else(|| anyhow::anyhow!("unknown task '{task_id}'"))?;
        let links = match surface {
            TaskLinkSurface::Design => &mut task.design_links,
            TaskLinkSurface::Implementation => &mut task.implementation_links,
        };
        if !links
            .iter()
            .any(|existing| existing.path == link.path && existing.kind == link.kind)
        {
            links.push(link.clone());
        }
        self.save_task(&task)?;
        self.append_log(
            &task.id,
            TaskLogKind::NoteAdded,
            TaskActor::Command,
            format!(
                "Linked {} artifact {}",
                match surface {
                    TaskLinkSurface::Design => "design",
                    TaskLinkSurface::Implementation => "implementation",
                },
                link.path
            ),
            None,
            None,
            None,
        )?;
        Ok(task)
    }

    pub fn append_note(
        &self,
        task_id: &str,
        actor: TaskActor,
        note: impl Into<String>,
        related_session_id: Option<String>,
        related_turn_id: Option<u64>,
    ) -> Result<TaskLogEntry> {
        if self.get_task(task_id)?.is_none() {
            anyhow::bail!("unknown task '{task_id}'");
        }
        self.append_log(
            task_id,
            TaskLogKind::NoteAdded,
            actor,
            note,
            related_session_id,
            related_turn_id,
            None,
        )
    }

    pub fn load_logs(&self, task_id: &str, limit: Option<usize>) -> Result<Vec<TaskLogEntry>> {
        let path = self.layout.docs_data_project_task_log_path(task_id);
        if !path.exists() {
            return Ok(Vec::new());
        }
        let file = fs::File::open(&path)
            .with_context(|| format!("failed to open plan task log {}", path.display()))?;
        let mut entries = BufReader::new(file)
            .lines()
            .map(|line| -> Result<Option<TaskLogEntry>> {
                let line = line?;
                if line.trim().is_empty() {
                    return Ok(None);
                }
                Ok(Some(
                    serde_json::from_str::<TaskLogEntry>(&line).with_context(|| {
                        format!("failed to parse task log entry from {}", path.display())
                    })?,
                ))
            })
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        if let Some(limit) = limit {
            if entries.len() > limit {
                entries = entries.split_off(entries.len() - limit);
            }
        }
        Ok(entries)
    }

    pub fn append_log(
        &self,
        task_id: &str,
        kind: TaskLogKind,
        actor: TaskActor,
        summary: impl Into<String>,
        related_session_id: Option<String>,
        related_turn_id: Option<u64>,
        related_delivery_id: Option<String>,
    ) -> Result<TaskLogEntry> {
        self.ensure_layout()?;
        let path = self.layout.docs_data_project_task_log_path(task_id);
        let seq = self.load_logs(task_id, None)?.len() as u64 + 1;
        let entry = TaskLogEntry {
            seq,
            kind,
            actor,
            summary: summary.into(),
            related_session_id,
            related_turn_id,
            related_delivery_id,
        };
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .with_context(|| format!("failed to append task log {}", path.display()))?;
        writeln!(file, "{}", serde_json::to_string(&entry)?)
            .with_context(|| format!("failed to write task log {}", path.display()))?;
        Ok(entry)
    }

    fn ensure_layout(&self) -> Result<()> {
        fs::create_dir_all(self.layout.docs_data_tasks_dir()).with_context(|| {
            format!(
                "failed to create docs-data tasks dir {}",
                self.layout.docs_data_tasks_dir().display()
            )
        })?;
        fs::create_dir_all(self.layout.docs_data_project_task_logs_dir()).with_context(|| {
            format!(
                "failed to create project task logs dir {}",
                self.layout.docs_data_project_task_logs_dir().display()
            )
        })?;
        Ok(())
    }

    fn load_manifest(&self) -> Result<ProjectPlanManifest> {
        let path = self.layout.docs_data_project_plan_manifest_path();
        let text = fs::read_to_string(&path)
            .with_context(|| format!("failed to read plan manifest {}", path.display()))?;
        toml::from_str(&text)
            .with_context(|| format!("failed to parse plan manifest {}", path.display()))
    }

    fn save_manifest(&self, manifest: &ProjectPlanManifest) -> Result<()> {
        let body = toml::to_string_pretty(manifest)?;
        let path = self.layout.docs_data_project_plan_manifest_path();
        fs::write(&path, body)
            .with_context(|| format!("failed to write plan manifest {}", path.display()))
    }

    fn save_task(&self, task: &PlannedTask) -> Result<()> {
        let mut tasks = self.load_tasks()?;
        if let Some(existing) = tasks.iter_mut().find(|existing| existing.id == task.id) {
            *existing = task.clone();
        } else {
            tasks.push(task.clone());
        }
        tasks.sort_by(compare_tasks);
        self.save_tasks(&tasks)
    }

    fn load_tasks(&self) -> Result<Vec<PlannedTask>> {
        let path = self.layout.docs_data_project_tasks_path();
        if !path.exists() {
            return Ok(Vec::new());
        }
        let file = fs::File::open(&path)
            .with_context(|| format!("failed to open project task store {}", path.display()))?;
        let mut tasks = BufReader::new(file)
            .lines()
            .map(|line| -> Result<Option<PlannedTask>> {
                let line = line?;
                if line.trim().is_empty() {
                    return Ok(None);
                }
                Ok(Some(
                    serde_json::from_str::<PlannedTask>(&line).with_context(|| {
                        format!("failed to parse project task from {}", path.display())
                    })?,
                ))
            })
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        tasks.sort_by(compare_tasks);
        Ok(tasks)
    }

    fn save_tasks(&self, tasks: &[PlannedTask]) -> Result<()> {
        let path = self.layout.docs_data_project_tasks_path();
        let mut file = fs::File::create(&path)
            .with_context(|| format!("failed to write project task store {}", path.display()))?;
        for task in tasks {
            writeln!(file, "{}", serde_json::to_string(task)?).with_context(|| {
                format!("failed to write project task store {}", path.display())
            })?;
        }
        Ok(())
    }

    fn dependency_reaches(&self, start_id: &str, target_id: &str) -> Result<bool> {
        if start_id == target_id {
            return Ok(true);
        }
        let Some(task) = self.get_task(start_id)? else {
            return Ok(false);
        };
        for dependency_id in &task.depends_on {
            if dependency_id == target_id || self.dependency_reaches(dependency_id, target_id)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn next_order_key(&self, priority: TaskPriority) -> Result<i64> {
        let max = self
            .list_tasks(TaskListFilter {
                status: None,
                priority: Some(priority),
            })?
            .into_iter()
            .map(|task| task.order_key)
            .max()
            .unwrap_or(0);
        Ok(max + TASK_ORDER_GAP)
    }
}

impl ProjectPlanAccess for ProjectPlanStore {
    fn resolve_task_context(&self, task_id: &str) -> Result<Option<SelectedProjectTaskContext>> {
        let Some(task) = self.get_task(task_id)? else {
            return Ok(None);
        };
        let dependency_chain = task
            .depends_on
            .iter()
            .map(|dependency_id| {
                Ok(self
                    .get_task(dependency_id)?
                    .map(|dependency| format!("{}: {}", dependency.id, dependency.title))
                    .unwrap_or_else(|| dependency_id.clone()))
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Some(SelectedProjectTaskContext {
            task_id: task.id.clone(),
            title: task.title.clone(),
            requirement: task.requirement.clone(),
            acceptance: task.acceptance.clone(),
            dependency_chain,
            recent_logs: self.load_logs(task_id, Some(8))?,
            design_links: task.design_links.clone(),
            implementation_links: task.implementation_links.clone(),
            presentation_links: task.presentation_links.clone(),
        }))
    }

    fn get_task(&self, task_id: &str) -> Result<Option<PlannedTask>> {
        Ok(self
            .load_tasks()?
            .into_iter()
            .find(|task| task.id == task_id))
    }

    fn list_tasks(&self, filter: TaskListFilter) -> Result<Vec<PlannedTask>> {
        self.ensure_layout()?;
        let mut tasks = self.load_tasks()?;
        tasks.retain(|task| {
            filter.status.is_none_or(|status| status == task.status)
                && filter
                    .priority
                    .is_none_or(|priority| priority == task.priority)
        });
        tasks.sort_by(compare_tasks);
        Ok(tasks)
    }
}

fn compare_tasks(left: &PlannedTask, right: &PlannedTask) -> Ordering {
    left.priority
        .rank()
        .cmp(&right.priority.rank())
        .then_with(|| left.order_key.cmp(&right.order_key))
        .then_with(|| left.id.cmp(&right.id))
}

#[cfg(test)]
mod tests {
    use omega_test_support::test_root;

    use super::{
        NewPlannedTask, PlannedTaskStatus, PlannedTaskUpdate, ProjectPlanAccess, ProjectPlanStore,
        TaskActor, TaskArtifactKind, TaskArtifactLink, TaskDependencyOperation, TaskLinkSurface,
        TaskListFilter, TaskLogKind, TaskOrderPlacement, TaskPriority,
    };

    #[test]
    fn open_or_scaffold_creates_manifest_and_task_dirs() {
        let root = test_root("plan-store-scaffold");
        let store = ProjectPlanStore::open_or_scaffold(root.path()).unwrap();

        assert!(root.join("docs-data/tasks/project-plan.toml").exists());
        assert!(root.join("docs-data/tasks/logs").exists());
        assert!(store
            .list_tasks(TaskListFilter::default())
            .unwrap()
            .is_empty());
    }

    #[test]
    fn create_task_persists_and_lists_by_priority_order() {
        let root = test_root("plan-store-create");
        let store = ProjectPlanStore::open_or_scaffold(root.path()).unwrap();

        let backlog = store
            .create_task(NewPlannedTask::simple("Backlog task", TaskPriority::P2))
            .unwrap();
        let urgent = store
            .create_task(NewPlannedTask::simple("Urgent task", TaskPriority::P0))
            .unwrap();

        let tasks = store.list_tasks(TaskListFilter::default()).unwrap();
        assert_eq!(tasks[0].id, urgent.id);
        assert_eq!(tasks[1].id, backlog.id);
        let stored =
            std::fs::read_to_string(root.join("docs-data/tasks/project-tasks.jsonl")).unwrap();
        assert!(stored.contains(&backlog.id));
    }

    #[test]
    fn create_task_persists_doc_scope_metadata() {
        let root = test_root("plan-store-doc-scope");
        let store = ProjectPlanStore::open_or_scaffold(root.path()).unwrap();

        let mut draft = NewPlannedTask::simple("Visible in TODO", TaskPriority::P1);
        draft.doc_scope = vec!["todo".to_string()];
        let created = store.create_task(draft).unwrap();

        let reopened = ProjectPlanStore::open_or_scaffold(root.path()).unwrap();
        let stored = reopened
            .get_task(&created.id)
            .unwrap()
            .expect("task should exist");
        assert_eq!(stored.doc_scope, vec!["todo".to_string()]);
    }

    #[test]
    fn resolve_task_context_includes_dependencies_and_recent_logs() {
        let root = test_root("plan-store-context");
        let store = ProjectPlanStore::open_or_scaffold(root.path()).unwrap();

        let dependency = store
            .create_task(NewPlannedTask::simple("Dependency", TaskPriority::P1))
            .unwrap();
        let mut task = NewPlannedTask::simple("Main task", TaskPriority::P1);
        task.depends_on = vec![dependency.id.clone()];
        let main = store.create_task(task).unwrap();
        store
            .append_log(
                &main.id,
                TaskLogKind::NoteAdded,
                TaskActor::User,
                "Need to finish dependency first",
                Some("sess-1".to_string()),
                Some(42),
                None,
            )
            .unwrap();

        let context = store
            .resolve_task_context(&main.id)
            .unwrap()
            .expect("expected task context");
        assert_eq!(context.task_id, main.id);
        assert_eq!(context.title, "Main task");
        assert_eq!(
            context.dependency_chain,
            vec![format!("{}: Dependency", dependency.id)]
        );
        assert_eq!(context.recent_logs.len(), 2);
    }

    #[test]
    fn list_filter_can_restrict_by_status() {
        let root = test_root("plan-store-filter");
        let store = ProjectPlanStore::open_or_scaffold(root.path()).unwrap();

        let mut done = NewPlannedTask::simple("Done task", TaskPriority::P2);
        done.status = PlannedTaskStatus::Done;
        store.create_task(done).unwrap();
        store
            .create_task(NewPlannedTask::simple("Open task", TaskPriority::P2))
            .unwrap();

        let done_tasks = store
            .list_tasks(TaskListFilter {
                status: Some(PlannedTaskStatus::Done),
                priority: None,
            })
            .unwrap();
        assert_eq!(done_tasks.len(), 1);
        assert_eq!(done_tasks[0].title, "Done task");
    }

    #[test]
    fn update_reprioritize_and_link_mutate_task_records() {
        let root = test_root("plan-store-update");
        let store = ProjectPlanStore::open_or_scaffold(root.path()).unwrap();
        let first = store
            .create_task(NewPlannedTask::simple("First task", TaskPriority::P2))
            .unwrap();
        let second = store
            .create_task(NewPlannedTask::simple("Second task", TaskPriority::P2))
            .unwrap();

        let updated = store
            .update_task(
                &first.id,
                PlannedTaskUpdate {
                    status: Some(PlannedTaskStatus::Ready),
                    summary: Some("Ready summary".to_string()),
                    acceptance: Some(vec!["passes tests".to_string()]),
                    ..PlannedTaskUpdate::default()
                },
            )
            .unwrap();
        assert_eq!(updated.status, PlannedTaskStatus::Ready);
        assert_eq!(updated.acceptance, vec!["passes tests".to_string()]);

        let reprioritized = store
            .reprioritize_task(&second.id, TaskPriority::P0, TaskOrderPlacement::default())
            .unwrap();
        assert_eq!(reprioritized.priority, TaskPriority::P0);

        let linked = store
            .add_artifact_link(
                &first.id,
                TaskLinkSurface::Design,
                TaskArtifactLink {
                    kind: TaskArtifactKind::Spec,
                    path: "docs/specs/omega-project-plan-system.md".to_string(),
                    label: Some("Spec".to_string()),
                },
            )
            .unwrap();
        assert_eq!(linked.design_links.len(), 1);
    }

    #[test]
    fn dependency_mutation_rejects_cycles() {
        let root = test_root("plan-store-dependency");
        let store = ProjectPlanStore::open_or_scaffold(root.path()).unwrap();
        let first = store
            .create_task(NewPlannedTask::simple("First task", TaskPriority::P2))
            .unwrap();
        let second = store
            .create_task(NewPlannedTask::simple("Second task", TaskPriority::P2))
            .unwrap();

        store
            .mutate_dependency(&first.id, &second.id, TaskDependencyOperation::Add)
            .unwrap();
        let error = store
            .mutate_dependency(&second.id, &first.id, TaskDependencyOperation::Add)
            .unwrap_err();
        assert!(error.to_string().contains("would create a cycle"));

        let detached = store
            .mutate_dependency(&first.id, &second.id, TaskDependencyOperation::Remove)
            .unwrap();
        assert!(detached.depends_on.is_empty());
    }
}
