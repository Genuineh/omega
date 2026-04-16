use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use omega_plan::{
    NewPlannedTask, PlannedTaskKind, PlannedTaskStatus, PlannedTaskUpdate, ProjectPlanAccess,
    ProjectPlanStore, TaskArtifactKind, TaskArtifactLink, TaskDependencyOperation,
    TaskLinkSurface, TaskOrderPlacement, TaskPriority,
};
use omega_project_layout::OmegaProjectLayout;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use walkdir::WalkDir;

use crate::{ArchiveTrigger, DocType, DocumentMutationMode};

const STRUCTURED_DOCS_SCHEMA_VERSION: u32 = 1;
const ROOT_RECORD_SET: &str = "root";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructuredDocsManifest {
    pub schema_version: u32,
    pub generated_root: String,
    pub record_sets: Vec<String>,
    pub task_store_path: String,
    #[serde(default = "default_project_task_store_path")]
    pub project_task_store_path: String,
    #[serde(default = "default_project_task_log_dir")]
    pub project_task_log_dir: String,
    #[serde(default = "default_project_plan_manifest_path")]
    pub project_plan_manifest_path: String,
    pub relation_store_path: String,
    pub render_state_path: String,
    #[serde(default)]
    pub content_revision: u64,
    #[serde(default)]
    pub projection_version: u64,
    #[serde(default)]
    pub last_generation_id: Option<String>,
    pub updated_at: u64,
}

impl Default for StructuredDocsManifest {
    fn default() -> Self {
        Self {
            schema_version: STRUCTURED_DOCS_SCHEMA_VERSION,
            generated_root: "docs".to_string(),
            record_sets: known_record_sets(),
            task_store_path: "docs-data/tasks/doc-tasks.jsonl".to_string(),
            project_task_store_path: default_project_task_store_path(),
            project_task_log_dir: default_project_task_log_dir(),
            project_plan_manifest_path: default_project_plan_manifest_path(),
            relation_store_path: "docs-data/relations/links.jsonl".to_string(),
            render_state_path: "docs-data/render/render-state.json".to_string(),
            content_revision: 0,
            projection_version: 0,
            last_generation_id: None,
            updated_at: unix_timestamp_now(),
        }
    }
}

fn default_project_task_store_path() -> String {
    "docs-data/tasks/project-tasks.jsonl".to_string()
}

fn default_project_task_log_dir() -> String {
    "docs-data/tasks/logs".to_string()
}

fn default_project_plan_manifest_path() -> String {
    "docs-data/tasks/project-plan.toml".to_string()
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
    pub frontmatter: BTreeMap<String, Value>,
    #[serde(default)]
    pub sections: Vec<StructuredDocumentSection>,
    #[serde(default)]
    pub relations: Vec<StructuredDocumentRelation>,
    pub render: StructuredDocumentRender,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructuredDocTaskRecord {
    pub task_id: String,
    pub title: String,
    #[serde(default)]
    pub plan_task_id: Option<String>,
    #[serde(default)]
    pub kind: PlannedTaskKind,
    #[serde(default)]
    pub status: PlannedTaskStatus,
    #[serde(default)]
    pub priority: TaskPriority,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub requirement: String,
    #[serde(default)]
    pub acceptance: Vec<String>,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub presentation_links: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub doc_scope: Vec<DocType>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructuredDocRelationRecord {
    pub relation_id: String,
    pub source: String,
    pub kind: String,
    pub target: String,
    #[serde(default)]
    pub metadata: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructuredDocsRenderState {
    pub schema_version: u32,
    pub generated_root: String,
    #[serde(default)]
    pub content_revision: Option<u64>,
    #[serde(default)]
    pub projection_version: Option<u64>,
    #[serde(default)]
    pub generation_id: Option<String>,
    pub last_rendered_at: Option<u64>,
    pub rendered_doc_ids: Vec<String>,
    pub generated_paths: Vec<String>,
    pub last_validation_ok: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructuredDocsSnapshot {
    pub manifest: StructuredDocsManifest,
    pub render_state: StructuredDocsRenderState,
    pub records: Vec<StructuredDocumentRecord>,
    pub doc_tasks: Vec<StructuredDocTaskRecord>,
    pub relations: Vec<StructuredDocRelationRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructuredDocVersionInfo {
    pub source_doc_id: String,
    pub content_revision: u64,
    pub projection_version: u64,
    pub generation_id: String,
}

impl Default for StructuredDocsRenderState {
    fn default() -> Self {
        Self {
            schema_version: STRUCTURED_DOCS_SCHEMA_VERSION,
            generated_root: "docs".to_string(),
            content_revision: None,
            projection_version: None,
            generation_id: None,
            last_rendered_at: None,
            rendered_doc_ids: Vec::new(),
            generated_paths: Vec::new(),
            last_validation_ok: None,
        }
    }
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
    #[serde(default)]
    pub version_mismatches: Vec<StructuredDocsValidationIssue>,
    pub broken_relations: Vec<StructuredDocsValidationIssue>,
    /// Markdown files found under `docs/` that are not registered in any
    /// `docs-data/records/` record's `presentation_path`. Only populated
    /// during full (non-filtered) validation runs.
    #[serde(default)]
    pub unregistered_files: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructuredDocsExtractionReport {
    pub extracted_doc_ids: Vec<String>,
    pub extracted_paths: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct StructuredDocumentOpOutcome {
    pub message: String,
    pub manifest: Option<StructuredDocsManifest>,
    pub records: Vec<StructuredDocumentRecord>,
    pub doc_tasks: Vec<StructuredDocTaskRecord>,
    pub relations: Vec<StructuredDocRelationRecord>,
    pub render_state: Option<StructuredDocsRenderState>,
    pub validation: Option<StructuredDocsValidationReport>,
    pub extraction: Option<StructuredDocsExtractionReport>,
    pub warnings: Vec<String>,
}

pub(crate) struct StructuredDocsManager {
    root: PathBuf,
    layout: OmegaProjectLayout,
}

impl StructuredDocsManager {
    pub(crate) fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        Self {
            layout: OmegaProjectLayout::new(root.clone()),
            root,
        }
    }

    pub(crate) fn upsert_record(
        &self,
        mode: DocumentMutationMode,
        record: StructuredDocumentRecord,
    ) -> Result<StructuredDocumentOpOutcome> {
        let record = canonicalize_record(record)?;
        validate_record(&record)?;
        self.ensure_layout()?;
        let mut manifest = self.load_manifest()?;
        let mut outcome = StructuredDocumentOpOutcome {
            message: format!(
                "{} structured doc record {}",
                mode_label(mode),
                record.doc_id
            ),
            manifest: Some(manifest.clone()),
            records: vec![record.clone()],
            ..StructuredDocumentOpOutcome::default()
        };

        if !matches!(mode, DocumentMutationMode::Apply) {
            return Ok(outcome);
        }

        let record_set = record_set_name(record.doc_type);
        let mut records = self.load_doc_records(record_set)?;
        upsert_by_key(&mut records, &record.doc_id, |item| item.doc_id.clone(), record.clone());
        records.sort_by(|left, right| left.doc_id.cmp(&right.doc_id));
        self.save_doc_records(record_set, &records)?;
        bump_content_revision(&mut manifest);
        ensure_contains(&mut manifest.record_sets, record_set.to_string());
        self.save_manifest(&manifest)?;
        outcome.message = format!("updated structured doc record {}", record.doc_id);
        outcome.manifest = Some(manifest);
        outcome.records = vec![record];
        Ok(outcome)
    }

    pub(crate) fn upsert_task(
        &self,
        mode: DocumentMutationMode,
        task: StructuredDocTaskRecord,
    ) -> Result<StructuredDocumentOpOutcome> {
        let mut task = canonicalize_doc_task(task)?;
        validate_doc_task(&task)?;
        self.ensure_layout()?;
        let mut manifest = self.load_manifest()?;
        let stored_tasks = self.load_doc_tasks()?;
        if task.plan_task_id.is_none() {
            task.plan_task_id = stored_tasks
                .iter()
                .find(|candidate| candidate.task_id == task.task_id)
                .and_then(|candidate| candidate.plan_task_id.clone());
        }

        let mut outcome = StructuredDocumentOpOutcome {
            message: format!("{} structured doc task {}", mode_label(mode), task.task_id),
            manifest: Some(manifest.clone()),
            doc_tasks: vec![task.clone()],
            ..StructuredDocumentOpOutcome::default()
        };

        if matches!(mode, DocumentMutationMode::Apply) {
            outcome
                .warnings
                .extend(self.sync_doc_task_to_plan(&mut task, &stored_tasks)?);
            let mut tasks = self.load_doc_tasks().unwrap_or(stored_tasks);
            upsert_by_key(&mut tasks, &task.task_id, |item| item.task_id.clone(), task.clone());
            tasks.sort_by(|left, right| left.task_id.cmp(&right.task_id));
            self.save_doc_tasks(&tasks)?;
            bump_content_revision(&mut manifest);
            self.save_manifest(&manifest)?;
            outcome.message = format!("updated structured doc task {}", task.task_id);
            outcome.manifest = Some(manifest);
            outcome.doc_tasks = vec![task];
        }

        Ok(outcome)
    }

    pub(crate) fn upsert_relation(
        &self,
        mode: DocumentMutationMode,
        relation: StructuredDocRelationRecord,
    ) -> Result<StructuredDocumentOpOutcome> {
        let relation = canonicalize_relation(relation)?;
        validate_relation(&relation)?;
        self.ensure_layout()?;
        let mut manifest = self.load_manifest()?;
        let mut outcome = StructuredDocumentOpOutcome {
            message: format!(
                "{} structured relation {}",
                mode_label(mode),
                relation.relation_id
            ),
            manifest: Some(manifest.clone()),
            relations: vec![relation.clone()],
            ..StructuredDocumentOpOutcome::default()
        };

        if !matches!(mode, DocumentMutationMode::Apply) {
            return Ok(outcome);
        }

        let mut relations = self.load_doc_relations()?;
        upsert_by_key(
            &mut relations,
            &relation.relation_id,
            |item| item.relation_id.clone(),
            relation.clone(),
        );
        relations.sort_by(|left, right| left.relation_id.cmp(&right.relation_id));
        self.save_doc_relations(&relations)?;
        bump_content_revision(&mut manifest);
        self.save_manifest(&manifest)?;
        outcome.message = format!("updated structured relation {}", relation.relation_id);
        outcome.manifest = Some(manifest);
        outcome.relations = vec![relation];
        Ok(outcome)
    }

    pub(crate) fn delete_record(
        &self,
        mode: DocumentMutationMode,
        doc_id: &str,
    ) -> Result<StructuredDocumentOpOutcome> {
        self.ensure_layout()?;
        let mut manifest = self.load_manifest()?;

        for record_set in known_record_sets() {
            let mut records = self.load_doc_records(&record_set)?;
            let Some(index) = records.iter().position(|record| record.doc_id == doc_id) else {
                continue;
            };
            let removed = records.remove(index);
            let mut render_state = self.load_render_state()?;

            if matches!(mode, DocumentMutationMode::Apply) {
                self.save_doc_records(&record_set, &records)?;
                let target = self.root.join(&removed.render.presentation_path);
                if target.exists() {
                    fs::remove_file(&target)
                        .with_context(|| format!("failed to remove {}", target.display()))?;
                }
                render_state
                    .rendered_doc_ids
                    .retain(|candidate| candidate != &removed.doc_id);
                render_state
                    .generated_paths
                    .retain(|candidate| candidate != &removed.render.presentation_path);
                self.save_render_state(&render_state)?;
                bump_content_revision(&mut manifest);
                self.save_manifest(&manifest)?;
            }

            return Ok(StructuredDocumentOpOutcome {
                message: format!("{} structured doc record {}", mode_label(mode), doc_id),
                manifest: Some(manifest),
                records: vec![removed],
                render_state: Some(render_state),
                ..StructuredDocumentOpOutcome::default()
            });
        }

        anyhow::bail!("unknown structured doc record '{}'", doc_id)
    }

    pub(crate) fn archive_record(
        &self,
        mode: DocumentMutationMode,
        doc_id: &str,
        reason: ArchiveTrigger,
        replaced_by: Option<&str>,
    ) -> Result<StructuredDocumentOpOutcome> {
        self.ensure_layout()?;
        let mut manifest = self.load_manifest()?;

        for record_set in known_record_sets() {
            let mut records = self.load_doc_records(&record_set)?;
            let Some(index) = records.iter().position(|record| record.doc_id == doc_id) else {
                continue;
            };
            let mut archived = records.remove(index);
            let previous_path = archived.render.presentation_path.clone();
            archived.doc_type = DocType::Archive;
            archived.status = Some(match reason {
                ArchiveTrigger::Superseded => "superseded".to_string(),
                ArchiveTrigger::CompletedAndInactive
                | ArchiveTrigger::StructurallyOutdated
                | ArchiveTrigger::HistoryOnly => "deprecated".to_string(),
            });
            let archived_at = unix_timestamp_now();
            archived.updated = Some(timestamp_to_date_string(archived_at));
            archived.source_path = format!("docs/archive/{}.md", archived.slug);
            archived.render.presentation_path = archived.source_path.clone();
            archived.frontmatter.insert("archived".to_string(), Value::Bool(true));
            archived.frontmatter.insert(
                "archived_date".to_string(),
                Value::String(timestamp_to_date_string(archived_at)),
            );
            archived.frontmatter.insert(
                "reason".to_string(),
                Value::String(archive_trigger_label(reason).to_string()),
            );
            archived.frontmatter.insert(
                "replaced_by".to_string(),
                replaced_by
                    .map(|value| Value::String(value.to_string()))
                    .unwrap_or(Value::String("N/A".to_string())),
            );
            ensure_archive_note(&mut archived, reason, replaced_by);

            let mut archive_records = self.load_doc_records("archive")?;
            upsert_by_key(
                &mut archive_records,
                &archived.doc_id,
                |item| item.doc_id.clone(),
                archived.clone(),
            );
            archive_records.sort_by(|left, right| left.doc_id.cmp(&right.doc_id));

            let mut render_state = self.load_render_state()?;
            if matches!(mode, DocumentMutationMode::Apply) {
                self.save_doc_records(&record_set, &records)?;
                self.save_doc_records("archive", &archive_records)?;
                let target = self.root.join(&previous_path);
                if target.exists() {
                    fs::remove_file(&target)
                        .with_context(|| format!("failed to remove {}", target.display()))?;
                }
                render_state
                    .rendered_doc_ids
                    .retain(|candidate| candidate != &archived.doc_id);
                render_state
                    .generated_paths
                    .retain(|candidate| candidate != &previous_path);
                self.save_render_state(&render_state)?;
                bump_content_revision(&mut manifest);
                ensure_contains(&mut manifest.record_sets, "archive".to_string());
                self.save_manifest(&manifest)?;
            }

            return Ok(StructuredDocumentOpOutcome {
                message: format!("{} structured doc record {}", mode_label(mode), doc_id),
                manifest: Some(manifest),
                records: vec![archived],
                render_state: Some(render_state),
                ..StructuredDocumentOpOutcome::default()
            });
        }

        anyhow::bail!("unknown structured doc record '{}'", doc_id)
    }

    pub(crate) fn delete_task(
        &self,
        mode: DocumentMutationMode,
        task_id: &str,
    ) -> Result<StructuredDocumentOpOutcome> {
        self.ensure_layout()?;
        let mut tasks = self.load_doc_tasks()?;
        let index = tasks
            .iter()
            .position(|task| task.task_id == task_id)
            .ok_or_else(|| anyhow::anyhow!("unknown structured doc task '{}'", task_id))?;
        let removed = tasks.remove(index);

        if matches!(mode, DocumentMutationMode::Apply) {
            self.save_doc_tasks(&tasks)?;
            if let Some(plan_task_id) = removed.plan_task_id.as_ref() {
                let plan_store = ProjectPlanStore::open_or_scaffold(&self.root)?;
                if plan_store.get_task(plan_task_id)?.is_some() {
                    plan_store.update_task(
                        plan_task_id,
                        PlannedTaskUpdate {
                            status: Some(PlannedTaskStatus::Archived),
                            ..PlannedTaskUpdate::default()
                        },
                    )?;
                }
            }
            let mut manifest = self.load_manifest()?;
            bump_content_revision(&mut manifest);
            self.save_manifest(&manifest)?;
        }

        Ok(StructuredDocumentOpOutcome {
            message: format!("{} structured doc task {}", mode_label(mode), task_id),
            doc_tasks: vec![removed],
            ..StructuredDocumentOpOutcome::default()
        })
    }

    pub(crate) fn delete_relation(
        &self,
        mode: DocumentMutationMode,
        relation_id: &str,
    ) -> Result<StructuredDocumentOpOutcome> {
        self.ensure_layout()?;
        let mut relations = self.load_doc_relations()?;
        let index = relations
            .iter()
            .position(|relation| relation.relation_id == relation_id)
            .ok_or_else(|| anyhow::anyhow!("unknown structured doc relation '{}'", relation_id))?;
        let removed = relations.remove(index);
        if matches!(mode, DocumentMutationMode::Apply) {
            self.save_doc_relations(&relations)?;
            let mut manifest = self.load_manifest()?;
            bump_content_revision(&mut manifest);
            self.save_manifest(&manifest)?;
        }

        Ok(StructuredDocumentOpOutcome {
            message: format!("{} structured doc relation {}", mode_label(mode), relation_id),
            relations: vec![removed],
            ..StructuredDocumentOpOutcome::default()
        })
    }

    pub(crate) fn snapshot(&self) -> Result<StructuredDocsSnapshot> {
        self.ensure_layout()?;
        Ok(StructuredDocsSnapshot {
            manifest: self.load_manifest()?,
            render_state: self.load_render_state()?,
            records: self.load_all_doc_records()?,
            doc_tasks: self.load_doc_tasks()?,
            relations: self.load_doc_relations()?,
        })
    }

    pub(crate) fn render_projection(
        &self,
        mode: DocumentMutationMode,
        doc_ids: Vec<String>,
    ) -> Result<StructuredDocumentOpOutcome> {
        self.ensure_layout()?;
        let mut manifest = self.load_manifest()?;
        let mut warnings = Vec::new();
        let records = self.select_records(&doc_ids, &mut warnings)?;
        let mut generated_paths = Vec::new();
        let version_info = projection_version_for_render(&mut manifest, mode);
        for record in &records {
            generated_paths.push(record.render.presentation_path.clone());
            if matches!(mode, DocumentMutationMode::Apply) {
                let rendered = render_document(record, Some(version_info_for_record(&version_info, &record.doc_id)));
                let target = self.root.join(&record.render.presentation_path);
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent)
                        .with_context(|| format!("failed to create {}", parent.display()))?;
                }
                fs::write(&target, rendered)
                    .with_context(|| format!("failed to write {}", target.display()))?;
            }
        }

        let render_state = StructuredDocsRenderState {
            schema_version: STRUCTURED_DOCS_SCHEMA_VERSION,
            generated_root: manifest.generated_root.clone(),
            content_revision: Some(version_info.content_revision),
            projection_version: Some(version_info.projection_version),
            generation_id: Some(version_info.generation_id.clone()),
            last_rendered_at: matches!(mode, DocumentMutationMode::Apply).then(unix_timestamp_now),
            rendered_doc_ids: records.iter().map(|record| record.doc_id.clone()).collect(),
            generated_paths: generated_paths.clone(),
            last_validation_ok: None,
        };
        if matches!(mode, DocumentMutationMode::Apply) {
            self.save_manifest(&manifest)?;
            self.save_render_state(&render_state)?;
        }

        Ok(StructuredDocumentOpOutcome {
            message: format!(
                "{} structured docs projection for {} document(s)",
                if matches!(mode, DocumentMutationMode::Apply) {
                    "rendered"
                } else {
                    mode_label(mode)
                },
                records.len()
            ),
            manifest: Some(manifest),
            records,
            render_state: Some(render_state),
            warnings,
            ..StructuredDocumentOpOutcome::default()
        })
    }

    pub(crate) fn validate_projection(
        &self,
        doc_ids: Vec<String>,
    ) -> Result<StructuredDocumentOpOutcome> {
        self.ensure_layout()?;
        let manifest = self.load_manifest()?;
        let render_state = self.load_render_state()?;
        let mut warnings = Vec::new();
        let records = self.select_records(&doc_ids, &mut warnings)?;
        let all_records = self.load_all_doc_records()?;
        let record_map = all_records
            .iter()
            .map(|record| (record.doc_id.clone(), record.clone()))
            .collect::<BTreeMap<_, _>>();
        let mut report = StructuredDocsValidationReport {
            ok: true,
            checked_doc_ids: records.iter().map(|record| record.doc_id.clone()).collect(),
            compared_paths: Vec::new(),
            missing_files: Vec::new(),
            mismatched_files: Vec::new(),
            version_mismatches: Vec::new(),
            broken_relations: Vec::new(),
            unregistered_files: Vec::new(),
            warnings,
        };

        if let Some(issue) = validate_render_state_versions(&manifest, &render_state) {
            report.ok = false;
            report.version_mismatches.push(issue);
        }

        let expected_version = current_expected_version(&manifest, &render_state);

        for record in &records {
            let expected = normalize_markdown(&render_document(
                record,
                Some(version_info_for_record(&expected_version, &record.doc_id)),
            ));
            report
                .compared_paths
                .push(record.render.presentation_path.clone());
            let target = self.root.join(&record.render.presentation_path);
            if !target.exists() {
                report.missing_files.push(record.render.presentation_path.clone());
                report.ok = false;
                continue;
            }
            let actual = fs::read_to_string(&target)
                .with_context(|| format!("failed to read {}", target.display()))?;
            if let Some(issue) = validate_generated_doc_version(
                &record.render.presentation_path,
                &actual,
                &expected_version,
                &record.doc_id,
            )? {
                report.ok = false;
                report.version_mismatches.push(issue);
            }

            let normalized_actual = normalize_generated_content(&actual)?;
            let normalized_expected = normalize_generated_content(&expected)?;
            if normalized_actual != normalized_expected {
                report.ok = false;
                report.mismatched_files.push(StructuredDocsValidationIssue {
                    path: record.render.presentation_path.clone(),
                    message: "rendered projection does not match current file".to_string(),
                    expected_preview: Some(preview_text(&normalized_expected, 200)),
                    actual_preview: Some(preview_text(&normalized_actual, 200)),
                });
            }
            for relation in &record.relations {
                if relation_target_exists(
                    &self.root,
                    &record_map,
                    &record.render.presentation_path,
                    &relation.target,
                ) {
                    continue;
                }
                report.ok = false;
                report.broken_relations.push(StructuredDocsValidationIssue {
                    path: record.render.presentation_path.clone(),
                    message: format!(
                        "relation target '{}' for kind '{}' does not resolve",
                        relation.target, relation.kind
                    ),
                    expected_preview: None,
                    actual_preview: None,
                });
            }
        }

        // Unregistered-file check: only for full (unfiltered) validations.
        if doc_ids.is_empty() {
            let registered: std::collections::HashSet<String> = all_records
                .iter()
                .map(|r| r.render.presentation_path.clone())
                .collect();
            let docs_dir = self.root.join("docs");
            let unregistered = collect_unregistered_md_files(&docs_dir, &self.root, &registered);
            for path in &unregistered {
                report.warnings.push(format!("unregistered file in docs/: {path}"));
            }
            report.unregistered_files = unregistered;
        }

        let issue_count = report.missing_files.len()
            + report.mismatched_files.len()
            + report.version_mismatches.len()
            + report.broken_relations.len();
        let warn_count = report.unregistered_files.len();
        Ok(StructuredDocumentOpOutcome {
            message: if report.ok && warn_count == 0 {
                format!("validated structured docs projection for {} document(s)", records.len())
            } else if report.ok {
                format!(
                    "validated structured docs projection for {} document(s) with {} warning(s)",
                    records.len(),
                    warn_count
                )
            } else {
                format!(
                    "structured docs projection has {} issue(s)",
                    issue_count
                )
            },
            records,
            validation: Some(report),
            ..StructuredDocumentOpOutcome::default()
        })
    }

    pub(crate) fn extract_sources(
        &self,
        mode: DocumentMutationMode,
        sources: Vec<String>,
        doc_type_override: Option<DocType>,
    ) -> Result<StructuredDocumentOpOutcome> {
        self.ensure_layout()?;
        let mut warnings = Vec::new();
        let markdown_sources = self.collect_markdown_sources(&sources, &mut warnings)?;
        let mut extracted = Vec::new();
        for relative_path in markdown_sources {
            let doc_type = doc_type_override
                .or_else(|| infer_doc_type_from_path(&relative_path))
                .unwrap_or(DocType::Guide);
            extracted.push(self.extract_record_from_markdown(&relative_path, doc_type)?);
        }

        if matches!(mode, DocumentMutationMode::Apply) {
            for record in &extracted {
                self.persist_record(record.clone())?;
            }
        }

        let extraction = StructuredDocsExtractionReport {
            extracted_doc_ids: extracted.iter().map(|record| record.doc_id.clone()).collect(),
            extracted_paths: extracted.iter().map(|record| record.source_path.clone()).collect(),
            warnings: warnings.clone(),
        };

        Ok(StructuredDocumentOpOutcome {
            message: format!(
                "{} structured extraction for {} markdown source(s)",
                mode_label(mode),
                extracted.len()
            ),
            records: extracted,
            extraction: Some(extraction),
            warnings,
            ..StructuredDocumentOpOutcome::default()
        })
    }

    fn persist_record(&self, record: StructuredDocumentRecord) -> Result<()> {
        let mut manifest = self.load_manifest()?;
        let record_set = record_set_name(record.doc_type);
        let mut records = self.load_doc_records(record_set)?;
        let record_id = record.doc_id.clone();
        upsert_by_key(&mut records, &record_id, |item| item.doc_id.clone(), record);
        records.sort_by(|left, right| left.doc_id.cmp(&right.doc_id));
        self.save_doc_records(record_set, &records)?;
        bump_content_revision(&mut manifest);
        ensure_contains(&mut manifest.record_sets, record_set.to_string());
        self.save_manifest(&manifest)?;
        Ok(())
    }

    fn extract_record_from_markdown(
        &self,
        relative_path: &str,
        doc_type: DocType,
    ) -> Result<StructuredDocumentRecord> {
        let absolute = self.root.join(relative_path);
        let content = fs::read_to_string(&absolute)
            .with_context(|| format!("failed to read {}", absolute.display()))?;
        let (mut frontmatter, body) = split_frontmatter(&content)?;
        strip_generated_frontmatter(&mut frontmatter);
        let status = take_string_field(&mut frontmatter, "status");
        let owner = take_string_field(&mut frontmatter, "owner");
        let created = take_string_field(&mut frontmatter, "created");
        let updated = take_string_field(&mut frontmatter, "updated");
        let version = take_string_field(&mut frontmatter, "version");
        let fallback_title = title_from_slug(&slug_from_path(relative_path));
        let (title, sections) = parse_title_and_sections(&body, &fallback_title);
        let relations = extract_relations_from_markdown(&body);
        canonicalize_record(StructuredDocumentRecord {
            doc_id: String::new(),
            doc_type,
            slug: slug_from_path(relative_path),
            title,
            status,
            owner,
            created,
            updated,
            version,
            source_path: relative_path.to_string(),
            frontmatter,
            sections,
            relations,
            render: StructuredDocumentRender {
                template: template_for_doc_type(doc_type).to_string(),
                presentation_path: relative_path.to_string(),
            },
        })
    }

    fn select_records(
        &self,
        doc_ids: &[String],
        warnings: &mut Vec<String>,
    ) -> Result<Vec<StructuredDocumentRecord>> {
        let all_records = self.load_all_doc_records()?;
        if doc_ids.is_empty() {
            return Ok(all_records);
        }
        let mut selected = Vec::new();
        for doc_id in doc_ids {
            match all_records.iter().find(|record| record.doc_id == *doc_id) {
                Some(record) => selected.push(record.clone()),
                None => warnings.push(format!("unknown structured doc id '{doc_id}'")),
            }
        }
        Ok(selected)
    }

    fn collect_markdown_sources(
        &self,
        sources: &[String],
        warnings: &mut Vec<String>,
    ) -> Result<Vec<String>> {
        let mut results = Vec::new();
        for source in sources {
            let candidate = self.root.join(source);
            if !candidate.exists() {
                warnings.push(format!("source '{}' does not exist", source));
                continue;
            }
            if candidate.is_file() {
                if candidate.extension().and_then(|value| value.to_str()) == Some("md") {
                    results.push(normalize_relative_path(&self.root, &candidate)?);
                } else {
                    warnings.push(format!("source '{}' is not a markdown file", source));
                }
                continue;
            }
            for entry in WalkDir::new(&candidate)
                .into_iter()
                .filter_map(|entry| entry.ok())
                .filter(|entry| entry.file_type().is_file())
            {
                let path = entry.path();
                if path.extension().and_then(|value| value.to_str()) != Some("md") {
                    continue;
                }
                results.push(normalize_relative_path(&self.root, path)?);
            }
        }
        results.sort();
        results.dedup();
        Ok(results)
    }

    fn ensure_layout(&self) -> Result<()> {
        fs::create_dir_all(self.layout.docs_data_records_dir())
            .with_context(|| format!("failed to create {}", self.layout.docs_data_records_dir().display()))?;
        fs::create_dir_all(self.layout.docs_data_tasks_dir())
            .with_context(|| format!("failed to create {}", self.layout.docs_data_tasks_dir().display()))?;
        fs::create_dir_all(self.layout.docs_data_relations_dir())
            .with_context(|| format!("failed to create {}", self.layout.docs_data_relations_dir().display()))?;
        fs::create_dir_all(self.layout.docs_data_render_dir())
            .with_context(|| format!("failed to create {}", self.layout.docs_data_render_dir().display()))?;
        if !self.layout.docs_data_manifest_path().exists() {
            self.save_manifest(&StructuredDocsManifest::default())?;
        }
        if !self.layout.docs_data_render_state_path().exists() {
            self.save_render_state(&StructuredDocsRenderState::default())?;
        }
        Ok(())
    }

    fn load_manifest(&self) -> Result<StructuredDocsManifest> {
        if !self.layout.docs_data_manifest_path().exists() {
            return Ok(StructuredDocsManifest::default());
        }
        let content = fs::read_to_string(self.layout.docs_data_manifest_path()).with_context(|| {
            format!(
                "failed to read {}",
                self.layout.docs_data_manifest_path().display()
            )
        })?;
        serde_json::from_str(&content).context("failed to parse structured docs manifest")
    }

    fn save_manifest(&self, manifest: &StructuredDocsManifest) -> Result<()> {
        fs::write(
            self.layout.docs_data_manifest_path(),
            serde_json::to_string_pretty(manifest)?,
        )
        .with_context(|| {
            format!(
                "failed to write {}",
                self.layout.docs_data_manifest_path().display()
            )
        })
    }

    fn load_render_state(&self) -> Result<StructuredDocsRenderState> {
        if !self.layout.docs_data_render_state_path().exists() {
            return Ok(StructuredDocsRenderState::default());
        }
        let content = fs::read_to_string(self.layout.docs_data_render_state_path()).with_context(|| {
            format!(
                "failed to read {}",
                self.layout.docs_data_render_state_path().display()
            )
        })?;
        serde_json::from_str(&content).context("failed to parse structured docs render state")
    }

    fn save_render_state(&self, state: &StructuredDocsRenderState) -> Result<()> {
        fs::write(
            self.layout.docs_data_render_state_path(),
            serde_json::to_string_pretty(state)?,
        )
        .with_context(|| {
            format!(
                "failed to write {}",
                self.layout.docs_data_render_state_path().display()
            )
        })
    }

    fn load_doc_records(&self, record_set: &str) -> Result<Vec<StructuredDocumentRecord>> {
        load_jsonl(&self.layout.docs_data_record_path(record_set))
    }

    fn save_doc_records(
        &self,
        record_set: &str,
        records: &[StructuredDocumentRecord],
    ) -> Result<()> {
        save_jsonl(&self.layout.docs_data_record_path(record_set), records)
    }

    fn load_all_doc_records(&self) -> Result<Vec<StructuredDocumentRecord>> {
        let mut records = Vec::new();
        for record_set in known_record_sets() {
            records.extend(self.load_doc_records(&record_set)?);
        }
        records.sort_by(|left, right| left.doc_id.cmp(&right.doc_id));
        records.dedup_by(|left, right| left.doc_id == right.doc_id);
        Ok(records)
    }

    fn load_doc_tasks(&self) -> Result<Vec<StructuredDocTaskRecord>> {
        load_jsonl(&self.layout.docs_data_doc_tasks_path())
    }

    fn save_doc_tasks(&self, tasks: &[StructuredDocTaskRecord]) -> Result<()> {
        save_jsonl(&self.layout.docs_data_doc_tasks_path(), tasks)
    }

    fn load_doc_relations(&self) -> Result<Vec<StructuredDocRelationRecord>> {
        load_jsonl(&self.layout.docs_data_links_path())
    }

    fn save_doc_relations(&self, relations: &[StructuredDocRelationRecord]) -> Result<()> {
        save_jsonl(&self.layout.docs_data_links_path(), relations)
    }

    fn sync_doc_task_to_plan(
        &self,
        task: &mut StructuredDocTaskRecord,
        stored_tasks: &[StructuredDocTaskRecord],
    ) -> Result<Vec<String>> {
        let plan_store = ProjectPlanStore::open_or_scaffold(&self.root)?;
        let mut warnings = Vec::new();
        let synced_tasks = self.load_doc_tasks().unwrap_or_else(|_| stored_tasks.to_vec());
        let desired_dependencies = resolve_plan_dependencies(
            &plan_store,
            &task.task_id,
            &task.depends_on,
            &synced_tasks,
            &mut warnings,
        )?;
        let desired_links = task
            .presentation_links
            .iter()
            .filter(|path| !path.trim().is_empty())
            .map(|path| TaskArtifactLink {
                kind: infer_artifact_kind(path),
                path: path.trim().to_string(),
                label: None,
            })
            .collect::<Vec<_>>();

        let plan_task_id = task
            .plan_task_id
            .clone()
            .or_else(|| {
                synced_tasks
                    .iter()
                    .find(|candidate| candidate.task_id == task.task_id)
                    .and_then(|candidate| candidate.plan_task_id.clone())
            })
            .filter(|task_id| plan_store.get_task(task_id).ok().flatten().is_some());
        let plan_task = if let Some(plan_task_id) = plan_task_id {
            let current = plan_store
                .get_task(&plan_task_id)?
                .ok_or_else(|| anyhow::anyhow!("unknown plan task '{}'", plan_task_id))?;
            if current.kind != task.kind {
                warnings.push(format!(
                    "plan task '{}' keeps existing kind '{}' because omega-plan does not support kind mutation",
                    plan_task_id,
                    match current.kind {
                        PlannedTaskKind::Task => "task",
                        PlannedTaskKind::Feature => "feature",
                        PlannedTaskKind::Research => "research",
                        PlannedTaskKind::Refactor => "refactor",
                        PlannedTaskKind::Chore => "chore",
                    }
                ));
            }
            let mut updated = plan_store.update_task(
                &plan_task_id,
                PlannedTaskUpdate {
                    title: Some(task.title.clone()),
                    summary: Some(task.summary.clone()),
                    requirement: Some(task.requirement.clone()),
                    status: Some(task.status),
                    acceptance: Some(task.acceptance.clone()),
                    tags: Some(task.tags.clone()),
                },
            )?;
            if updated.priority != task.priority {
                updated = plan_store.reprioritize_task(
                    &plan_task_id,
                    task.priority,
                    TaskOrderPlacement::default(),
                )?;
            }
            sync_dependencies(&plan_store, &updated.id, &updated.depends_on, &desired_dependencies)?;
            sync_design_links(&plan_store, &updated.id, &desired_links)?;
            updated
        } else {
            let created = plan_store.create_task(NewPlannedTask {
                title: task.title.clone(),
                kind: task.kind,
                status: task.status,
                priority: task.priority,
                summary: task.summary.clone(),
                requirement: task.requirement.clone(),
                acceptance: task.acceptance.clone(),
                parent_id: None,
                depends_on: desired_dependencies.clone(),
                tags: task.tags.clone(),
            })?;
            sync_design_links(&plan_store, &created.id, &desired_links)?;
            created
        };
        task.plan_task_id = Some(plan_task.id);
        Ok(warnings)
    }
}

fn sync_dependencies(
    store: &ProjectPlanStore,
    task_id: &str,
    current_dependencies: &[String],
    desired_dependencies: &[String],
) -> Result<()> {
    let current = current_dependencies.iter().cloned().collect::<BTreeSet<_>>();
    let desired = desired_dependencies.iter().cloned().collect::<BTreeSet<_>>();
    for dependency in current.difference(&desired) {
        store.mutate_dependency(task_id, dependency, TaskDependencyOperation::Remove)?;
    }
    for dependency in desired.difference(&current) {
        store.mutate_dependency(task_id, dependency, TaskDependencyOperation::Add)?;
    }
    Ok(())
}

fn sync_design_links(
    store: &ProjectPlanStore,
    task_id: &str,
    desired_links: &[TaskArtifactLink],
) -> Result<()> {
    for link in desired_links {
        store.add_artifact_link(task_id, TaskLinkSurface::Design, link.clone())?;
    }
    Ok(())
}

fn resolve_plan_dependencies(
    store: &ProjectPlanStore,
    task_id: &str,
    desired_dependencies: &[String],
    stored_tasks: &[StructuredDocTaskRecord],
    warnings: &mut Vec<String>,
) -> Result<Vec<String>> {
    let mut resolved = Vec::new();
    for dependency in desired_dependencies {
        if dependency == task_id {
            warnings.push(format!("ignoring self dependency '{}'", dependency));
            continue;
        }
        let plan_task_id = if dependency.starts_with("TASK-") {
            Some(dependency.clone())
        } else {
            stored_tasks
                .iter()
                .find(|candidate| candidate.task_id == *dependency)
                .and_then(|candidate| candidate.plan_task_id.clone())
        };
        let Some(plan_task_id) = plan_task_id else {
            warnings.push(format!(
                "dependency '{}' has no synced omega-plan task yet",
                dependency
            ));
            continue;
        };
        if store.get_task(&plan_task_id)?.is_none() {
            warnings.push(format!(
                "dependency '{}' resolved to missing omega-plan task '{}'",
                dependency, plan_task_id
            ));
            continue;
        }
        resolved.push(plan_task_id);
    }
    resolved.sort();
    resolved.dedup();
    Ok(resolved)
}

fn render_document(record: &StructuredDocumentRecord, version_info: Option<StructuredDocVersionInfo>) -> String {
    let mut frontmatter = BTreeMap::new();
    if let Some(status) = record.status.as_ref() {
        frontmatter.insert("status".to_string(), Value::String(status.clone()));
    }
    if let Some(owner) = record.owner.as_ref() {
        frontmatter.insert("owner".to_string(), Value::String(owner.clone()));
    }
    if let Some(created) = record.created.as_ref() {
        frontmatter.insert("created".to_string(), Value::String(created.clone()));
    }
    if let Some(updated) = record.updated.as_ref() {
        frontmatter.insert("updated".to_string(), Value::String(updated.clone()));
    }
    if let Some(version) = record.version.as_ref() {
        frontmatter.insert("version".to_string(), Value::String(version.clone()));
    }
    for (key, value) in &record.frontmatter {
        frontmatter.insert(key.clone(), value.clone());
    }
    if let Some(version_info) = version_info {
        frontmatter.insert(
            "source_doc_id".to_string(),
            Value::String(version_info.source_doc_id),
        );
        frontmatter.insert(
            "content_revision".to_string(),
            Value::Number(version_info.content_revision.into()),
        );
        frontmatter.insert(
            "projection_version".to_string(),
            Value::Number(version_info.projection_version.into()),
        );
        frontmatter.insert(
            "generation_id".to_string(),
            Value::String(version_info.generation_id),
        );
    }

    let mut output = String::new();
    if !frontmatter.is_empty() {
        output.push_str("---\n");
        for (key, value) in &frontmatter {
            render_frontmatter_entry(&mut output, key, value, 0);
        }
        output.push_str("---\n\n");
    }
    output.push_str("# ");
    output.push_str(record.title.trim());
    output.push_str("\n");
    if !record.sections.is_empty() {
        output.push('\n');
    }
    for (index, section) in record.sections.iter().enumerate() {
        output.push_str("## ");
        output.push_str(section.heading.trim());
        output.push_str("\n\n");
        output.push_str(section.body_markdown.trim_end());
        output.push('\n');
        if index + 1 != record.sections.len() {
            output.push('\n');
        }
    }
    normalize_markdown(&output)
}

fn bump_content_revision(manifest: &mut StructuredDocsManifest) {
    manifest.content_revision = manifest.content_revision.saturating_add(1);
    manifest.updated_at = unix_timestamp_now();
}

fn projection_version_for_render(
    manifest: &mut StructuredDocsManifest,
    mode: DocumentMutationMode,
) -> StructuredDocVersionInfo {
    if matches!(mode, DocumentMutationMode::Apply) {
        manifest.projection_version = manifest.projection_version.saturating_add(1);
        manifest.last_generation_id = Some(format_generation_id(
            manifest.projection_version,
            manifest.content_revision,
        ));
    }

    StructuredDocVersionInfo {
        source_doc_id: String::new(),
        content_revision: manifest.content_revision,
        projection_version: manifest.projection_version,
        generation_id: manifest
            .last_generation_id
            .clone()
            .unwrap_or_else(|| format_generation_id(manifest.projection_version, manifest.content_revision)),
    }
}

fn current_expected_version(
    manifest: &StructuredDocsManifest,
    render_state: &StructuredDocsRenderState,
) -> StructuredDocVersionInfo {
    StructuredDocVersionInfo {
        source_doc_id: String::new(),
        content_revision: manifest.content_revision,
        projection_version: render_state
            .projection_version
            .unwrap_or(manifest.projection_version),
        generation_id: render_state
            .generation_id
            .clone()
            .or_else(|| manifest.last_generation_id.clone())
            .unwrap_or_else(|| format_generation_id(manifest.projection_version, manifest.content_revision)),
    }
}

fn version_info_for_record(
    version_info: &StructuredDocVersionInfo,
    doc_id: &str,
) -> StructuredDocVersionInfo {
    StructuredDocVersionInfo {
        source_doc_id: doc_id.to_string(),
        content_revision: version_info.content_revision,
        projection_version: version_info.projection_version,
        generation_id: version_info.generation_id.clone(),
    }
}

fn format_generation_id(projection_version: u64, content_revision: u64) -> String {
    format!("gen_{projection_version:06}_r{content_revision:06}")
}

fn strip_generated_frontmatter(frontmatter: &mut BTreeMap<String, Value>) {
    for key in [
        "source_doc_id",
        "content_revision",
        "projection_version",
        "generation_id",
    ] {
        frontmatter.remove(key);
    }
}

fn normalize_generated_content(content: &str) -> Result<String> {
    let (mut frontmatter, body) = split_frontmatter(content)?;
    strip_generated_frontmatter(&mut frontmatter);

    let mut output = String::new();
    if !frontmatter.is_empty() {
        output.push_str("---\n");
        for (key, value) in &frontmatter {
            render_frontmatter_entry(&mut output, key, value, 0);
        }
        output.push_str("---\n\n");
    }
    output.push_str(body.trim_start_matches('\n'));
    Ok(normalize_markdown(&output))
}

fn validate_render_state_versions(
    manifest: &StructuredDocsManifest,
    render_state: &StructuredDocsRenderState,
) -> Option<StructuredDocsValidationIssue> {
    let Some(rendered_revision) = render_state.content_revision else {
        return Some(StructuredDocsValidationIssue {
            path: manifest.render_state_path.clone(),
            message: "render state is missing content_revision".to_string(),
            expected_preview: Some(manifest.content_revision.to_string()),
            actual_preview: None,
        });
    };
    let Some(rendered_projection) = render_state.projection_version else {
        return Some(StructuredDocsValidationIssue {
            path: manifest.render_state_path.clone(),
            message: "render state is missing projection_version".to_string(),
            expected_preview: Some(manifest.projection_version.to_string()),
            actual_preview: None,
        });
    };
    let Some(rendered_generation) = render_state.generation_id.as_ref() else {
        return Some(StructuredDocsValidationIssue {
            path: manifest.render_state_path.clone(),
            message: "render state is missing generation_id".to_string(),
            expected_preview: manifest.last_generation_id.clone(),
            actual_preview: None,
        });
    };

    if rendered_revision != manifest.content_revision {
        return Some(StructuredDocsValidationIssue {
            path: manifest.render_state_path.clone(),
            message: "render state content_revision does not match manifest content_revision".to_string(),
            expected_preview: Some(manifest.content_revision.to_string()),
            actual_preview: Some(rendered_revision.to_string()),
        });
    }
    if rendered_projection != manifest.projection_version {
        return Some(StructuredDocsValidationIssue {
            path: manifest.render_state_path.clone(),
            message: "render state projection_version does not match manifest projection_version".to_string(),
            expected_preview: Some(manifest.projection_version.to_string()),
            actual_preview: Some(rendered_projection.to_string()),
        });
    }
    if Some(rendered_generation.clone()) != manifest.last_generation_id {
        return Some(StructuredDocsValidationIssue {
            path: manifest.render_state_path.clone(),
            message: "render state generation_id does not match manifest last_generation_id".to_string(),
            expected_preview: manifest.last_generation_id.clone(),
            actual_preview: Some(rendered_generation.clone()),
        });
    }
    None
}

fn validate_generated_doc_version(
    path: &str,
    actual: &str,
    expected_version: &StructuredDocVersionInfo,
    expected_doc_id: &str,
) -> Result<Option<StructuredDocsValidationIssue>> {
    let (mut frontmatter, _) = split_frontmatter(actual)?;
    let actual_doc_id = take_string_field(&mut frontmatter, "source_doc_id");
    let actual_content_revision = take_u64_field(&mut frontmatter, "content_revision");
    let actual_projection_version = take_u64_field(&mut frontmatter, "projection_version");
    let actual_generation_id = take_string_field(&mut frontmatter, "generation_id");

    if actual_doc_id.as_deref() != Some(expected_doc_id) {
        return Ok(Some(StructuredDocsValidationIssue {
            path: path.to_string(),
            message: "generated doc source_doc_id does not match canonical record id".to_string(),
            expected_preview: Some(expected_doc_id.to_string()),
            actual_preview: actual_doc_id,
        }));
    }
    if actual_content_revision != Some(expected_version.content_revision) {
        return Ok(Some(StructuredDocsValidationIssue {
            path: path.to_string(),
            message: "generated doc content_revision does not match current canonical revision".to_string(),
            expected_preview: Some(expected_version.content_revision.to_string()),
            actual_preview: actual_content_revision.map(|value| value.to_string()),
        }));
    }
    if actual_projection_version != Some(expected_version.projection_version) {
        return Ok(Some(StructuredDocsValidationIssue {
            path: path.to_string(),
            message: "generated doc projection_version does not match current projection version".to_string(),
            expected_preview: Some(expected_version.projection_version.to_string()),
            actual_preview: actual_projection_version.map(|value| value.to_string()),
        }));
    }
    if actual_generation_id.as_deref() != Some(expected_version.generation_id.as_str()) {
        return Ok(Some(StructuredDocsValidationIssue {
            path: path.to_string(),
            message: "generated doc generation_id does not match current generation".to_string(),
            expected_preview: Some(expected_version.generation_id.clone()),
            actual_preview: actual_generation_id,
        }));
    }

    Ok(None)
}

fn ensure_archive_note(
    record: &mut StructuredDocumentRecord,
    reason: ArchiveTrigger,
    replaced_by: Option<&str>,
) {
    let body = match replaced_by {
        Some(replaced_by) if !replaced_by.trim().is_empty() => format!(
            "Archived because {}. Replaced by `{}`.",
            archive_trigger_label(reason),
            replaced_by.trim()
        ),
        _ => format!("Archived because {}.", archive_trigger_label(reason)),
    };

    if let Some(section) = record
        .sections
        .iter_mut()
        .find(|section| section.section_id == "archive-note")
    {
        section.heading = "Archive Note".to_string();
        section.body_markdown = body;
        return;
    }

    record.sections.insert(
        0,
        StructuredDocumentSection {
            section_id: "archive-note".to_string(),
            heading: "Archive Note".to_string(),
            body_markdown: body,
        },
    );
}

fn archive_trigger_label(reason: ArchiveTrigger) -> &'static str {
    match reason {
        ArchiveTrigger::Superseded => "it was superseded",
        ArchiveTrigger::CompletedAndInactive => "it was completed and is no longer active",
        ArchiveTrigger::StructurallyOutdated => "it became structurally outdated",
        ArchiveTrigger::HistoryOnly => "it is retained for history only",
    }
}

fn timestamp_to_date_string(timestamp: u64) -> String {
    timestamp_to_date(timestamp)
}

fn timestamp_to_date(timestamp: u64) -> String {
    let days = (timestamp / 86_400) as i64;
    let (year, month, day) = civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02}")
}

fn civil_from_days(days_since_epoch: i64) -> (i64, i64, i64) {
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    let year = y + if m <= 2 { 1 } else { 0 };
    (year, m, d)
}

fn render_frontmatter_entry(output: &mut String, key: &str, value: &Value, indent: usize) {
    let padding = " ".repeat(indent);
    match value {
        Value::Array(values) => {
            if values.is_empty() {
                output.push_str(&format!("{padding}{key}: []\n"));
                return;
            }
            output.push_str(&format!("{padding}{key}:\n"));
            for item in values {
                match item {
                    Value::Object(_map) => {
                        output.push_str(&format!("{padding}  - {}\n", serde_json::to_string(item).unwrap_or_else(|_| "{}".to_string())));
                    }
                    _ => output.push_str(&format!(
                        "{padding}  - {}\n",
                        render_scalar(item)
                    )),
                }
            }
        }
        Value::Object(map) => {
            if map.is_empty() {
                output.push_str(&format!("{padding}{key}: {{}}\n"));
                return;
            }
            output.push_str(&format!("{padding}{key}:\n"));
            let mut ordered = map.iter().collect::<Vec<_>>();
            ordered.sort_by(|left, right| left.0.cmp(right.0));
            for (child_key, child_value) in ordered {
                render_frontmatter_entry(output, child_key, child_value, indent + 2);
            }
        }
        _ => output.push_str(&format!("{padding}{key}: {}\n", render_scalar(value))),
    }
}

fn render_scalar(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::String(value) => render_string_scalar(value),
        Value::Array(_) | Value::Object(_) => serde_json::to_string(value).unwrap_or_default(),
    }
}

fn render_string_scalar(value: &str) -> String {
    let needs_quotes = value.is_empty()
        || value.contains(':')
        || value.contains('#')
        || value.contains('[')
        || value.contains(']')
        || value.starts_with('{')
        || value.starts_with('-')
        || value.starts_with(' ')
        || value.ends_with(' ');
    if needs_quotes {
        serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_string())
    } else {
        value.to_string()
    }
}

fn split_frontmatter(content: &str) -> Result<(BTreeMap<String, Value>, String)> {
    if !content.starts_with("---\n") {
        return Ok((BTreeMap::new(), content.to_string()));
    }
    let remainder = &content[4..];
    let Some(index) = remainder.find("\n---\n") else {
        anyhow::bail!("unterminated frontmatter block");
    };
    let raw_frontmatter = &remainder[..index];
    let body = &remainder[index + 5..];
    if raw_frontmatter.trim().is_empty() {
        return Ok((BTreeMap::new(), body.to_string()));
    }
    let yaml_map = serde_yaml::from_str::<BTreeMap<String, serde_yaml::Value>>(raw_frontmatter)
        .context("failed to parse frontmatter yaml")?;
    let mut frontmatter = BTreeMap::new();
    for (key, value) in yaml_map {
        frontmatter.insert(key, yaml_to_json(value));
    }
    Ok((frontmatter, body.to_string()))
}

fn yaml_to_json(value: serde_yaml::Value) -> Value {
    match value {
        serde_yaml::Value::Null => Value::Null,
        serde_yaml::Value::Bool(value) => Value::Bool(value),
        serde_yaml::Value::Number(value) => {
            if let Some(number) = value.as_i64() {
                Value::Number(number.into())
            } else if let Some(number) = value.as_u64() {
                Value::Number(number.into())
            } else if let Some(number) = value.as_f64() {
                serde_json::Number::from_f64(number)
                    .map(Value::Number)
                    .unwrap_or(Value::Null)
            } else {
                Value::Null
            }
        }
        serde_yaml::Value::String(value) => Value::String(value),
        serde_yaml::Value::Sequence(values) => {
            Value::Array(values.into_iter().map(yaml_to_json).collect())
        }
        serde_yaml::Value::Mapping(values) => {
            let mut map = serde_json::Map::new();
            for (key, value) in values {
                let key = match key {
                    serde_yaml::Value::String(value) => value,
                    other => serde_json::to_string(&yaml_to_json(other)).unwrap_or_default(),
                };
                map.insert(key, yaml_to_json(value));
            }
            Value::Object(map)
        }
        serde_yaml::Value::Tagged(value) => yaml_to_json(value.value),
    }
}

fn parse_title_and_sections(
    body: &str,
    fallback_title: &str,
) -> (String, Vec<StructuredDocumentSection>) {
    let mut lines = body.lines().peekable();
    while matches!(lines.peek(), Some(line) if line.trim().is_empty()) {
        lines.next();
    }
    let mut title = fallback_title.to_string();
    let mut remainder = String::new();
    let mut consumed_title = false;
    while let Some(line) = lines.next() {
        if !consumed_title && line.starts_with("# ") {
            title = line.trim_start_matches("# ").trim().to_string();
            consumed_title = true;
            continue;
        }
        remainder.push_str(line);
        remainder.push('\n');
    }
    let remainder = remainder.trim_start_matches('\n').to_string();
    if remainder.trim().is_empty() {
        return (title, Vec::new());
    }
    let mut sections = Vec::new();
    let mut current_heading: Option<String> = None;
    let mut current_body = String::new();
    for line in remainder.lines() {
        if line.starts_with("## ") {
            if let Some(heading) = current_heading.take() {
                push_section(&mut sections, &heading, &current_body);
            } else if !current_body.trim().is_empty() {
                push_named_section(&mut sections, "overview", "Overview", &current_body);
            }
            current_heading = Some(line.trim_start_matches("## ").trim().to_string());
            current_body.clear();
            continue;
        }
        current_body.push_str(line);
        current_body.push('\n');
    }
    if let Some(heading) = current_heading {
        push_section(&mut sections, &heading, &current_body);
    } else if !current_body.trim().is_empty() {
        push_named_section(&mut sections, "body", "Body", &current_body);
    }
    (title, sections)
}

fn push_section(
    sections: &mut Vec<StructuredDocumentSection>,
    heading: &str,
    body: &str,
) {
    push_named_section(sections, &slugify(heading), heading, body)
}

fn push_named_section(
    sections: &mut Vec<StructuredDocumentSection>,
    section_id: &str,
    heading: &str,
    body: &str,
) {
    let body_markdown = body.trim().to_string();
    if body_markdown.is_empty() {
        return;
    }
    sections.push(StructuredDocumentSection {
        section_id: section_id.to_string(),
        heading: heading.to_string(),
        body_markdown,
    });
}

fn extract_relations_from_markdown(markdown: &str) -> Vec<StructuredDocumentRelation> {
    markdown_link_targets(markdown)
        .into_iter()
        .filter(|target| {
            !target.starts_with("http://")
                && !target.starts_with("https://")
                && !target.starts_with('#')
                && !target.starts_with("mailto:")
        })
        .map(|target| StructuredDocumentRelation {
            kind: "references".to_string(),
            target,
        })
        .collect()
}

fn relation_target_exists(
    root: &Path,
    record_map: &BTreeMap<String, StructuredDocumentRecord>,
    source_path: &str,
    target: &str,
) -> bool {
    if record_map.contains_key(target) {
        return true;
    }
    if target.starts_with("TASK-") || target.starts_with("DOC-") {
        return true;
    }
    let normalized = normalize_relation_target(source_path, target);
    if root.join(&normalized).exists() {
        return true;
    }
    record_map
        .values()
        .any(|record| record.render.presentation_path == normalized)
}

fn normalize_relation_target(source_path: &str, target: &str) -> String {
    let target = target.split('#').next().unwrap_or(target).trim();
    if target.is_empty() {
        return normalize_relative_like_path(source_path);
    }
    if target.starts_with("http://")
        || target.starts_with("https://")
        || target.starts_with("mailto:")
    {
        return target.to_string();
    }
    let target_path = Path::new(target);
    if target_path.is_absolute() || looks_repo_relative_target(target) {
        return normalize_relative_like_path(target.trim_start_matches('/'));
    }
    let base = Path::new(source_path)
        .parent()
        .unwrap_or_else(|| Path::new(""));
    normalize_relative_like_path(&base.join(target_path).to_string_lossy())
}

fn looks_repo_relative_target(target: &str) -> bool {
    let Some(first) = Path::new(target)
        .components()
        .next()
        .and_then(|component| match component {
            std::path::Component::Normal(value) => value.to_str(),
            _ => None,
        })
    else {
        return false;
    };
    matches!(
        first,
        "docs"
            | "crates"
            | "scripts"
            | ".omega"
            | ".omega-state"
            | ".claude"
            | ".github"
            | "README.md"
            | "CHANGELOG.md"
            | "LICENSE"
    )
}

fn canonicalize_record(mut record: StructuredDocumentRecord) -> Result<StructuredDocumentRecord> {
    let derived_slug = if record.slug.trim().is_empty() {
        slug_from_path(&record.source_path)
    } else {
        slugify(record.slug.trim())
    };
    record.slug = derived_slug;
    if record.title.trim().is_empty() {
        anyhow::bail!("structured doc record title cannot be empty");
    }
    if record.source_path.trim().is_empty() {
        record.source_path = default_presentation_path(record.doc_type, &record.slug);
    } else {
        record.source_path = normalize_relative_like_path(&record.source_path);
    }
    record.render.template = if record.render.template.trim().is_empty() {
        template_for_doc_type(record.doc_type).to_string()
    } else {
        record.render.template.trim().to_string()
    };
    record.render.presentation_path = if record.render.presentation_path.trim().is_empty() {
        default_presentation_path(record.doc_type, &record.slug)
    } else {
        normalize_relative_like_path(&record.render.presentation_path)
    };
    record.doc_id = if record.doc_id.trim().is_empty() {
        format!("{}:{}", doc_type_tag(record.doc_type), record.slug)
    } else {
        record.doc_id.trim().to_string()
    };
    dedupe_sort_relations(&mut record.relations);
    Ok(record)
}

fn canonicalize_doc_task(mut task: StructuredDocTaskRecord) -> Result<StructuredDocTaskRecord> {
    task.task_id = task.task_id.trim().to_string();
    task.title = task.title.trim().to_string();
    if task.summary.trim().is_empty() {
        task.summary = task.title.clone();
    } else {
        task.summary = task.summary.trim().to_string();
    }
    if task.requirement.trim().is_empty() {
        task.requirement = task.summary.clone();
    } else {
        task.requirement = task.requirement.trim().to_string();
    }
    trim_dedupe_sort(&mut task.acceptance);
    trim_dedupe_sort(&mut task.depends_on);
    trim_dedupe_sort(&mut task.presentation_links);
    trim_dedupe_sort(&mut task.tags);
    task.doc_scope.sort_by(|left, right| doc_type_tag(*left).cmp(doc_type_tag(*right)));
    task.doc_scope.dedup();
    Ok(task)
}

fn canonicalize_relation(
    mut relation: StructuredDocRelationRecord,
) -> Result<StructuredDocRelationRecord> {
    relation.relation_id = relation.relation_id.trim().to_string();
    relation.source = relation.source.trim().to_string();
    relation.kind = relation.kind.trim().to_string();
    relation.target = relation.target.trim().to_string();
    Ok(relation)
}

fn validate_record(record: &StructuredDocumentRecord) -> Result<()> {
    if record.doc_id.trim().is_empty() {
        anyhow::bail!("structured doc record doc_id cannot be empty");
    }
    let prefix = format!("{}:", doc_type_tag(record.doc_type));
    if !record.doc_id.starts_with(&prefix) {
        anyhow::bail!(
            "structured doc record doc_id '{}' must start with '{}'",
            record.doc_id,
            prefix
        );
    }
    if record.slug.trim().is_empty() {
        anyhow::bail!("structured doc record slug cannot be empty");
    }
    if record.render.presentation_path.trim().is_empty() {
        anyhow::bail!("structured doc record presentation_path cannot be empty");
    }
    Ok(())
}

fn validate_doc_task(task: &StructuredDocTaskRecord) -> Result<()> {
    if task.task_id.trim().is_empty() {
        anyhow::bail!("structured doc task id cannot be empty");
    }
    if task.title.trim().is_empty() {
        anyhow::bail!("structured doc task title cannot be empty");
    }
    Ok(())
}

fn validate_relation(relation: &StructuredDocRelationRecord) -> Result<()> {
    if relation.relation_id.trim().is_empty() {
        anyhow::bail!("structured relation id cannot be empty");
    }
    if relation.source.trim().is_empty() {
        anyhow::bail!("structured relation source cannot be empty");
    }
    if relation.kind.trim().is_empty() {
        anyhow::bail!("structured relation kind cannot be empty");
    }
    if relation.target.trim().is_empty() {
        anyhow::bail!("structured relation target cannot be empty");
    }
    Ok(())
}

fn load_jsonl<T>(path: &Path) -> Result<Vec<T>>
where
    T: DeserializeOwned,
{
    if !path.exists() {
        return Ok(Vec::new());
    }
    let file = fs::File::open(path)
        .with_context(|| format!("failed to open {}", path.display()))?;
    let mut records = Vec::new();
    for (index, line) in BufReader::new(file).lines().enumerate() {
        let line = line.with_context(|| format!("failed to read {}", path.display()))?;
        if line.trim().is_empty() {
            continue;
        }
        records.push(serde_json::from_str(&line).with_context(|| {
            format!("failed to parse {} line {}", path.display(), index + 1)
        })?);
    }
    Ok(records)
}

fn save_jsonl<T>(path: &Path, records: &[T]) -> Result<()>
where
    T: Serialize,
{
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let mut file = fs::File::create(path)
        .with_context(|| format!("failed to create {}", path.display()))?;
    for record in records {
        file.write_all(serde_json::to_string(record)?.as_bytes())
            .with_context(|| format!("failed to write {}", path.display()))?;
        file.write_all(b"\n")
            .with_context(|| format!("failed to write {}", path.display()))?;
    }
    Ok(())
}

fn upsert_by_key<T, F>(items: &mut Vec<T>, key: &str, key_fn: F, replacement: T)
where
    F: Fn(&T) -> String,
{
    if let Some(index) = items.iter().position(|item| key_fn(item) == key) {
        items[index] = replacement;
    } else {
        items.push(replacement);
    }
}

fn ensure_contains(values: &mut Vec<String>, value: String) {
    if !values.iter().any(|current| current == &value) {
        values.push(value);
        values.sort();
    }
}

fn known_record_sets() -> Vec<String> {
    vec![
        "archive".to_string(),
        "decisions".to_string(),
        "guides".to_string(),
        "prds".to_string(),
        ROOT_RECORD_SET.to_string(),
        "specs".to_string(),
        "whitepapers".to_string(),
    ]
}

fn record_set_name(doc_type: DocType) -> &'static str {
    match doc_type {
        DocType::Spec => "specs",
        DocType::Prd => "prds",
        DocType::Guide => "guides",
        DocType::Adr => "decisions",
        DocType::Whitepaper => "whitepapers",
        DocType::Archive => "archive",
        DocType::Todo | DocType::Readme | DocType::Changelog => ROOT_RECORD_SET,
    }
}

fn infer_doc_type_from_path(path: &str) -> Option<DocType> {
    if path == "README.md" || path == "docs/README.md" {
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
    } else if path.starts_with("docs/whitepapers/") {
        Some(DocType::Whitepaper)
    } else if path.starts_with("docs/archive/") {
        Some(DocType::Archive)
    } else {
        None
    }
}

fn doc_type_tag(doc_type: DocType) -> &'static str {
    match doc_type {
        DocType::Spec => "spec",
        DocType::Prd => "prd",
        DocType::Guide => "guide",
        DocType::Adr => "adr",
        DocType::Whitepaper => "whitepaper",
        DocType::Todo => "todo",
        DocType::Archive => "archive",
        DocType::Readme => "readme",
        DocType::Changelog => "changelog",
    }
}

fn template_for_doc_type(doc_type: DocType) -> &'static str {
    match doc_type {
        DocType::Spec => "spec-v1",
        DocType::Prd => "prd-v1",
        DocType::Guide => "guide-v1",
        DocType::Adr => "adr-v1",
        DocType::Whitepaper => "whitepaper-v1",
        DocType::Todo => "todo-v1",
        DocType::Archive => "archive-v1",
        DocType::Readme => "readme-v1",
        DocType::Changelog => "changelog-v1",
    }
}

fn default_presentation_path(doc_type: DocType, slug: &str) -> String {
    match doc_type {
        DocType::Spec => format!("docs/specs/{slug}.md"),
        DocType::Prd => format!("docs/prds/{slug}.md"),
        DocType::Guide => format!("docs/guide/{slug}.md"),
        DocType::Adr => format!("docs/decisions/{slug}.md"),
        DocType::Whitepaper => format!("docs/whitepapers/{slug}.md"),
        DocType::Todo => "docs/TODO.md".to_string(),
        DocType::Archive => format!("docs/archive/{slug}.md"),
        DocType::Readme => "docs/README.md".to_string(),
        DocType::Changelog => "CHANGELOG.md".to_string(),
    }
}

fn infer_artifact_kind(path: &str) -> TaskArtifactKind {
    if path.starts_with("docs/specs/") {
        TaskArtifactKind::Spec
    } else if path.starts_with("docs/prds/") {
        TaskArtifactKind::Prd
    } else if path.starts_with("docs/guide/") || path == "docs/README.md" {
        TaskArtifactKind::Guide
    } else if path.starts_with("docs/decisions/") {
        TaskArtifactKind::Adr
    } else {
        TaskArtifactKind::Code
    }
}

fn slugify(value: &str) -> String {
    let mut slug = String::new();
    let mut previous_dash = false;
    for character in value.chars() {
        if character.is_ascii_alphanumeric() {
            slug.push(character.to_ascii_lowercase());
            previous_dash = false;
        } else if !previous_dash {
            slug.push('-');
            previous_dash = true;
        }
    }
    slug.trim_matches('-').to_string()
}

fn slug_from_path(path: &str) -> String {
    let normalized = normalize_relative_like_path(path);
    let stemmed = normalized
        .strip_suffix(".md")
        .or_else(|| normalized.strip_suffix(".txt"))
        .or_else(|| normalized.strip_suffix(".adoc"))
        .unwrap_or(&normalized);
    if stemmed.is_empty() {
        None
    } else {
        Some(slugify(stemmed))
    }
        .filter(|slug| !slug.is_empty())
        .unwrap_or_else(|| "document".to_string())
}

fn title_from_slug(slug: &str) -> String {
    slug.split('-')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut characters = part.chars();
            match characters.next() {
                Some(first) => {
                    let mut word = first.to_ascii_uppercase().to_string();
                    word.push_str(characters.as_str());
                    word
                }
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn take_string_field(map: &mut BTreeMap<String, Value>, key: &str) -> Option<String> {
    map.remove(key).and_then(|value| value.as_str().map(ToOwned::to_owned))
}

fn take_u64_field(map: &mut BTreeMap<String, Value>, key: &str) -> Option<u64> {
    map.remove(key).and_then(|value| match value {
        Value::Number(number) => number.as_u64().or_else(|| number.as_i64().map(|value| value as u64)),
        Value::String(value) => value.parse::<u64>().ok(),
        _ => None,
    })
}

fn dedupe_sort_relations(relations: &mut Vec<StructuredDocumentRelation>) {
    relations.sort_by(|left, right| {
        left.kind
            .cmp(&right.kind)
            .then(left.target.cmp(&right.target))
    });
    relations.dedup_by(|left, right| left.kind == right.kind && left.target == right.target);
}

fn trim_dedupe_sort(values: &mut Vec<String>) {
    for value in values.iter_mut() {
        *value = value.trim().to_string();
    }
    values.retain(|value| !value.is_empty());
    values.sort();
    values.dedup();
}

fn normalize_relative_path(root: &Path, path: &Path) -> Result<String> {
    Ok(path
        .strip_prefix(root)
        .with_context(|| format!("{} is not under {}", path.display(), root.display()))?
        .to_string_lossy()
        .replace('\\', "/"))
}

fn normalize_relative_like_path(path: &str) -> String {
    let mut parts = Vec::new();
    for component in Path::new(path).components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                parts.pop();
            }
            std::path::Component::Normal(value) => {
                parts.push(value.to_string_lossy().to_string())
            }
            _ => {}
        }
    }
    parts.join("/")
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

fn normalize_markdown(content: &str) -> String {
    let mut normalized = content
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n");
    if !normalized.ends_with('\n') {
        normalized.push('\n');
    }
    normalized
}

/// Walk `docs_dir` recursively and return relative paths (from `root`) for
/// every `.md` file that does not appear in `registered`.
fn collect_unregistered_md_files(
    docs_dir: &Path,
    root: &Path,
    registered: &std::collections::HashSet<String>,
) -> Vec<String> {
    let mut result = Vec::new();
    collect_md_files_recursive(docs_dir, root, registered, &mut result);
    result.sort();
    result
}

fn collect_md_files_recursive(
    dir: &Path,
    root: &Path,
    registered: &std::collections::HashSet<String>,
    out: &mut Vec<String>,
) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_md_files_recursive(&path, root, registered, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
            if let Ok(rel) = path.strip_prefix(root) {
                let rel_str = rel.to_string_lossy().replace('\\', "/");
                if !registered.contains::<str>(rel_str.as_ref()) {
                    out.push(rel_str.to_string());
                }
            }
        }
    }
}

fn preview_text(text: &str, limit: usize) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= limit {
        collapsed
    } else {
        collapsed.chars().take(limit).collect()
    }
}

fn mode_label(mode: DocumentMutationMode) -> &'static str {
    match mode {
        DocumentMutationMode::Check => "validated",
        DocumentMutationMode::Plan => "planned",
        DocumentMutationMode::Apply => "applied",
    }
}

fn unix_timestamp_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}