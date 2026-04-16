use std::env;
use std::fs;
use std::path::PathBuf;

use anyhow::{anyhow, bail, Context, Result};
use omega_document::{
    ArchiveTrigger, DocType, DocumentMutationMode, DocumentOp, DocumentOpResult, OmegaDocument,
    StructuredDocRelationRecord, StructuredDocTaskRecord, StructuredDocumentRecord,
    StructuredDocsCutoverResult, StructuredDocsSnapshot, StructuredDocsValidationReport,
};
use omega_project_layout::OmegaProjectLayout;
use serde::Serialize;
use serde_json::json;

use crate::exit_codes::{INVALID_INPUT, INVALID_ROOT, STRICT_WARNING, SUCCESS, WRITE_FAILURE};
use crate::output::{to_pretty_json, CliOutput};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ListKind {
    Docs,
    Tasks,
    Relations,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EntityKind {
    Record,
    Task,
    Relation,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum GetOutput {
    Record {
        record: StructuredDocumentRecord,
        content_revision: u64,
        projection_version: Option<u64>,
        generation_id: Option<String>,
    },
    Task {
        task: StructuredDocTaskRecord,
        content_revision: u64,
        projection_version: Option<u64>,
        generation_id: Option<String>,
    },
    Relation {
        relation: StructuredDocRelationRecord,
        content_revision: u64,
        projection_version: Option<u64>,
        generation_id: Option<String>,
    },
}

pub fn run<I>(args: I) -> CliOutput
where
    I: IntoIterator<Item = String>,
{
    match run_inner(args.into_iter().collect()) {
        Ok(output) => output,
        Err(error) => CliOutput::new(
            classify_error(&error),
            String::new(),
            format!("{error}\n"),
        ),
    }
}

fn classify_error(error: &anyhow::Error) -> i32 {
    let message = error.to_string();
    if message.contains("failed to write ")
        || message.contains("failed to remove ")
        || message.contains("failed to create ")
    {
        WRITE_FAILURE
    } else {
        INVALID_INPUT
    }
}

fn run_inner(mut args: Vec<String>) -> Result<CliOutput> {
    if args.is_empty() {
        return Ok(CliOutput::new(INVALID_INPUT, usage(), String::new()));
    }

    let command = args.remove(0);
    match command.as_str() {
        "doctor" => doctor_command(args),
        "render" => render_command(args),
        "validate" => validate_command(args),
        "extract" => extract_command(args),
        "cutover" => cutover_command(args),
        "archive" => archive_command(args),
        "get" => get_command(args),
        "list" => list_command(args),
        "record" => nested_mutation_command(EntityKind::Record, args),
        "task" => nested_mutation_command(EntityKind::Task, args),
        "relation" => nested_mutation_command(EntityKind::Relation, args),
        "remove" => remove_command(args),
        "help" | "--help" | "-h" => Ok(CliOutput::new(SUCCESS, usage(), String::new())),
        other => Ok(CliOutput::new(
            INVALID_INPUT,
            usage(),
            format!("unknown command '{other}'\n"),
        )),
    }
}

fn doctor_command(mut args: Vec<String>) -> Result<CliOutput> {
    let json = take_bool_flag(&mut args, "--json");
    let root = take_root(&mut args)?;
    if !args.is_empty() {
        bail!("unexpected arguments for doctor: {}", args.join(" "));
    }

    let layout = OmegaProjectLayout::new(root.clone());
    let mut warnings = Vec::new();
    let root_exists = root.is_dir();
    let docs_exists = root.join("docs").is_dir();
    let docs_data_exists = layout.docs_data_dir().is_dir();
    let manifest_exists = layout.docs_data_manifest_path().is_file();

    if !docs_exists {
        warnings.push("missing docs/ directory".to_string());
    }
    if !docs_data_exists {
        warnings.push("missing docs-data/ directory".to_string());
    }
    if !manifest_exists {
        warnings.push("missing docs-data manifest".to_string());
    }

    let mut version = None;
    let mut validation_summary = None;
    let mut version_issues = 0usize;
    if root_exists && docs_exists && docs_data_exists && manifest_exists {
        let documents = OmegaDocument::new(root.clone());
        let snapshot = documents.structured_docs_snapshot()?;
        version = Some(json!({
            "content_revision": snapshot.manifest.content_revision,
            "projection_version": snapshot.manifest.projection_version,
            "last_generation_id": snapshot.manifest.last_generation_id,
            "rendered_content_revision": snapshot.render_state.content_revision,
            "rendered_projection_version": snapshot.render_state.projection_version,
            "rendered_generation_id": snapshot.render_state.generation_id,
        }));

        let validation = documents.manage_document(DocumentOp::ValidateProjection { doc_ids: Vec::new() })?;
        if let Some(report) = validation.validation {
            version_issues = report.version_mismatches.len();
            validation_summary = Some(json!({
                "ok": report.ok,
                "missing_files": report.missing_files.len(),
                "mismatched_files": report.mismatched_files.len(),
                "version_mismatches": report.version_mismatches.len(),
                "broken_relations": report.broken_relations.len(),
                "unregistered_files": report.unregistered_files.len(),
            }));
            if !report.version_mismatches.is_empty() {
                warnings.push(format!(
                    "structured docs version drift detected in {} place(s)",
                    report.version_mismatches.len()
                ));
            }
            if !report.unregistered_files.is_empty() {
                warnings.push(format!(
                    "{} unregistered file(s) in docs/ not in docs-data/records/: {}",
                    report.unregistered_files.len(),
                    report.unregistered_files.join(", ")
                ));
            }
        }
    }

    let ok = root_exists
        && docs_exists
        && docs_data_exists
        && manifest_exists
        && version_issues == 0;
    let payload = json!({
        "command": "doctor",
        "root": root.display().to_string(),
        "ok": ok,
        "checks": {
            "root_exists": root_exists,
            "docs_exists": docs_exists,
            "docs_data_exists": docs_data_exists,
            "manifest_exists": manifest_exists,
        },
        "version": version,
        "validation": validation_summary,
        "warnings": warnings,
    });

    if json {
        return Ok(CliOutput::new(
            if !root_exists || !docs_exists || !docs_data_exists || !manifest_exists {
                INVALID_ROOT
            } else if ok {
                SUCCESS
            } else {
                INVALID_INPUT
            },
            to_pretty_json(&payload),
            String::new(),
        ));
    }

    let mut stdout = format!("doctor: {}\n", if ok { "ok" } else { "invalid root" });
    stdout.push_str(&format!("root: {}\n", root.display()));
    if !payload["warnings"].as_array().unwrap_or(&Vec::new()).is_empty() {
        stdout.push_str(&format!("warnings: {}\n", payload["warnings"].as_array().unwrap().len()));
        for warning in payload["warnings"].as_array().unwrap() {
            stdout.push_str(&format!("- {}\n", warning.as_str().unwrap_or_default()));
        }
    }
    if let Some(version) = payload["version"].as_object() {
        stdout.push_str(&format!(
            "content_revision: {}\nprojection_version: {}\n",
            version
                .get("content_revision")
                .and_then(|value| value.as_u64())
                .unwrap_or_default(),
            version
                .get("projection_version")
                .and_then(|value| value.as_u64())
                .unwrap_or_default(),
        ));
    }
    Ok(CliOutput::new(
        if !root_exists || !docs_exists || !docs_data_exists || !manifest_exists {
            INVALID_ROOT
        } else if ok {
            SUCCESS
        } else {
            INVALID_INPUT
        },
        stdout,
        String::new(),
    ))
}

fn render_command(mut args: Vec<String>) -> Result<CliOutput> {
    let json = take_bool_flag(&mut args, "--json");
    let root = take_root(&mut args)?;
    let mode = take_mode(&mut args, DocumentMutationMode::Apply)?;
    let doc_ids = args;
    let documents = OmegaDocument::new(root);
    let result = documents.manage_document(DocumentOp::RenderProjection { mode, doc_ids })?;
    Ok(render_document_result_output(result, json, false))
}

fn validate_command(mut args: Vec<String>) -> Result<CliOutput> {
    let json = take_bool_flag(&mut args, "--json");
    let strict = take_bool_flag(&mut args, "--strict");
    let root = take_root(&mut args)?;
    let doc_ids = args;
    let documents = OmegaDocument::new(root);
    let result = documents.manage_document(DocumentOp::ValidateProjection { doc_ids })?;
    Ok(render_document_result_output(result, json, strict))
}

fn extract_command(mut args: Vec<String>) -> Result<CliOutput> {
    let json = take_bool_flag(&mut args, "--json");
    let root = take_root(&mut args)?;
    let mode = take_mode(&mut args, DocumentMutationMode::Apply)?;
    let doc_type = take_doc_type(&mut args)?;
    if args.is_empty() {
        bail!("extract requires at least one source path");
    }
    let documents = OmegaDocument::new(root);
    let result = documents.manage_document(DocumentOp::ExtractSource {
        mode,
        sources: args,
        doc_type,
    })?;
    Ok(render_document_result_output(result, json, false))
}

fn cutover_command(mut args: Vec<String>) -> Result<CliOutput> {
    let json = take_bool_flag(&mut args, "--json");
    let root = take_root(&mut args)?;
    let doc_type = take_doc_type(&mut args)?;
    if args.is_empty() {
        args.push("docs".to_string());
    }
    let documents = OmegaDocument::new(root.clone());
    let result = documents.run_structured_docs_cutover(DocumentMutationMode::Apply, args, doc_type)?;
    Ok(render_cutover_output(root, result, json))
}

fn get_command(mut args: Vec<String>) -> Result<CliOutput> {
    let json = take_bool_flag(&mut args, "--json");
    let root = take_root(&mut args)?;
    if args.len() != 1 {
        bail!("get requires exactly one id");
    }
    let id = args.remove(0);
    let snapshot = OmegaDocument::new(root).structured_docs_snapshot()?;
    let Some(found) = find_by_id(&snapshot, &id) else {
        return Ok(CliOutput::new(INVALID_INPUT, String::new(), format!("unknown id '{id}'\n")));
    };

    if json {
        return Ok(CliOutput::new(SUCCESS, to_pretty_json(&found), String::new()));
    }

    let stdout = match found {
        GetOutput::Record {
            record,
            content_revision,
            projection_version,
            generation_id,
        } => format!(
            "record {}\ntitle: {}\ntype: {:?}\nsource: {}\ncontent_revision: {}\nprojection_version: {}\ngeneration_id: {}\n",
            record.doc_id,
            record.title,
            record.doc_type,
            record.source_path,
            content_revision,
            projection_version.map(|value| value.to_string()).unwrap_or_else(|| "n/a".to_string()),
            generation_id.unwrap_or_else(|| "n/a".to_string()),
        ),
        GetOutput::Task {
            task,
            content_revision,
            projection_version,
            generation_id,
        } => format!(
            "task {}\ntitle: {}\nstatus: {:?}\npriority: {:?}\ncontent_revision: {}\nprojection_version: {}\ngeneration_id: {}\n",
            task.task_id,
            task.title,
            task.status,
            task.priority,
            content_revision,
            projection_version.map(|value| value.to_string()).unwrap_or_else(|| "n/a".to_string()),
            generation_id.unwrap_or_else(|| "n/a".to_string()),
        ),
        GetOutput::Relation {
            relation,
            content_revision,
            projection_version,
            generation_id,
        } => format!(
            "relation {}\n{} -{}-> {}\ncontent_revision: {}\nprojection_version: {}\ngeneration_id: {}\n",
            relation.relation_id,
            relation.source,
            relation.kind,
            relation.target,
            content_revision,
            projection_version.map(|value| value.to_string()).unwrap_or_else(|| "n/a".to_string()),
            generation_id.unwrap_or_else(|| "n/a".to_string()),
        ),
    };
    Ok(CliOutput::new(SUCCESS, stdout, String::new()))
}

fn list_command(mut args: Vec<String>) -> Result<CliOutput> {
    let json = take_bool_flag(&mut args, "--json");
    let root = take_root(&mut args)?;
    let doc_type = take_doc_type(&mut args)?;
    if args.len() != 1 {
        bail!("list requires one target: docs, tasks, or relations");
    }
    let list_kind = match args[0].as_str() {
        "docs" => ListKind::Docs,
        "tasks" => ListKind::Tasks,
        "relations" => ListKind::Relations,
        other => bail!("unknown list target '{other}'"),
    };

    let snapshot = OmegaDocument::new(root).structured_docs_snapshot()?;
    match list_kind {
        ListKind::Docs => {
            let records = snapshot
                .records
                .into_iter()
                .filter(|record| doc_type.is_none_or(|expected| record.doc_type == expected))
                .collect::<Vec<_>>();
            if json {
                return Ok(CliOutput::new(
                    SUCCESS,
                    to_pretty_json(&json!({ "kind": "docs", "records": records })),
                    String::new(),
                ));
            }
            let mut stdout = format!("docs: {}\n", records.len());
            for record in records {
                stdout.push_str(&format!("- {} :: {}\n", record.doc_id, record.title));
            }
            Ok(CliOutput::new(SUCCESS, stdout, String::new()))
        }
        ListKind::Tasks => {
            if json {
                return Ok(CliOutput::new(
                    SUCCESS,
                    to_pretty_json(&json!({ "kind": "tasks", "tasks": snapshot.doc_tasks })),
                    String::new(),
                ));
            }
            let mut stdout = format!("tasks: {}\n", snapshot.doc_tasks.len());
            for task in snapshot.doc_tasks {
                stdout.push_str(&format!("- {} :: {}\n", task.task_id, task.title));
            }
            Ok(CliOutput::new(SUCCESS, stdout, String::new()))
        }
        ListKind::Relations => {
            if json {
                return Ok(CliOutput::new(
                    SUCCESS,
                    to_pretty_json(&json!({ "kind": "relations", "relations": snapshot.relations })),
                    String::new(),
                ));
            }
            let mut stdout = format!("relations: {}\n", snapshot.relations.len());
            for relation in snapshot.relations {
                stdout.push_str(&format!(
                    "- {} :: {} -{}-> {}\n",
                    relation.relation_id, relation.source, relation.kind, relation.target
                ));
            }
            Ok(CliOutput::new(SUCCESS, stdout, String::new()))
        }
    }
}

fn nested_mutation_command(kind: EntityKind, mut args: Vec<String>) -> Result<CliOutput> {
    let json = take_bool_flag(&mut args, "--json");
    let root = take_root(&mut args)?;
    let mode = take_mode(&mut args, DocumentMutationMode::Apply)?;
    let input_path = take_option_flag(&mut args, "--input")?
        .ok_or_else(|| anyhow!("--input is required"))?;
    if args.len() != 1 || args[0] != "upsert" {
        bail!("expected '<record|task|relation> upsert --input <path>'");
    }

    let payload = fs::read_to_string(&input_path)
        .with_context(|| format!("failed to read {}", input_path))?;
    let documents = OmegaDocument::new(root);
    let result = match kind {
        EntityKind::Record => documents.manage_document(DocumentOp::UpsertRecord {
            mode,
            record: serde_json::from_str::<StructuredDocumentRecord>(&payload)
                .context("failed to parse record json")?,
        })?,
        EntityKind::Task => documents.manage_document(DocumentOp::UpsertTask {
            mode,
            task: serde_json::from_str::<StructuredDocTaskRecord>(&payload)
                .context("failed to parse task json")?,
        })?,
        EntityKind::Relation => documents.manage_document(DocumentOp::UpsertRelation {
            mode,
            relation: serde_json::from_str::<StructuredDocRelationRecord>(&payload)
                .context("failed to parse relation json")?,
        })?,
    };
    Ok(render_document_result_output(result, json, false))
}

fn remove_command(mut args: Vec<String>) -> Result<CliOutput> {
    let json = take_bool_flag(&mut args, "--json");
    let root = take_root(&mut args)?;
    let mode = take_mode(&mut args, DocumentMutationMode::Apply)?;
    let kind = take_option_flag(&mut args, "--kind")?;
    if args.len() != 1 {
        bail!("remove requires exactly one id");
    }
    let id = args.remove(0);
    let entity_kind = kind
        .as_deref()
        .map(parse_entity_kind)
        .transpose()?
        .unwrap_or_else(|| infer_entity_kind(&id));

    let documents = OmegaDocument::new(root);
    let result = match entity_kind {
        EntityKind::Record => documents.manage_document(DocumentOp::DeleteRecord { mode, doc_id: id })?,
        EntityKind::Task => documents.manage_document(DocumentOp::DeleteTask { mode, task_id: id })?,
        EntityKind::Relation => documents.manage_document(DocumentOp::DeleteRelation {
            mode,
            relation_id: id,
        })?,
    };
    Ok(render_document_result_output(result, json, false))
}

fn archive_command(mut args: Vec<String>) -> Result<CliOutput> {
    let json = take_bool_flag(&mut args, "--json");
    let root = take_root(&mut args)?;
    let mode = take_mode(&mut args, DocumentMutationMode::Apply)?;
    let reason = match take_option_flag(&mut args, "--reason")? {
        Some(reason) => parse_archive_trigger(&reason)?,
        None => ArchiveTrigger::HistoryOnly,
    };
    let replaced_by = take_option_flag(&mut args, "--replaced-by")?;
    if args.len() != 1 {
        bail!("archive requires exactly one doc id");
    }
    let documents = OmegaDocument::new(root);
    let result = documents.manage_document(DocumentOp::ArchiveRecord {
        mode,
        doc_id: args.remove(0),
        reason,
        replaced_by,
    })?;
    Ok(render_document_result_output(result, json, false))
}

fn take_root(args: &mut Vec<String>) -> Result<PathBuf> {
    let root = take_option_flag(args, "--root")?
        .map(PathBuf::from)
        .unwrap_or(env::current_dir().context("failed to resolve current directory")?);
    Ok(root)
}

fn take_bool_flag(args: &mut Vec<String>, flag: &str) -> bool {
    if let Some(index) = args.iter().position(|arg| arg == flag) {
        args.remove(index);
        true
    } else {
        false
    }
}

fn take_option_flag(args: &mut Vec<String>, flag: &str) -> Result<Option<String>> {
    let Some(index) = args.iter().position(|arg| arg == flag) else {
        return Ok(None);
    };
    if index + 1 >= args.len() {
        bail!("missing value after '{flag}'");
    }
    let value = args.remove(index + 1);
    args.remove(index);
    Ok(Some(value))
}

fn take_mode(args: &mut Vec<String>, default: DocumentMutationMode) -> Result<DocumentMutationMode> {
    let mut mode = default;
    let mut index = 0usize;
    while index < args.len() {
        let parsed = match args[index].as_str() {
            "--check" | "check" => Some(DocumentMutationMode::Check),
            "--plan" | "plan" => Some(DocumentMutationMode::Plan),
            "--apply" | "apply" => Some(DocumentMutationMode::Apply),
            _ => None,
        };
        if let Some(parsed) = parsed {
            mode = parsed;
            args.remove(index);
        } else {
            index += 1;
        }
    }
    Ok(mode)
}

fn take_doc_type(args: &mut Vec<String>) -> Result<Option<DocType>> {
    let Some(value) = take_option_flag(args, "--doc-type")? else {
        return Ok(None);
    };
    parse_doc_type(&value).map(Some)
}

fn parse_doc_type(value: &str) -> Result<DocType> {
    match value.trim().to_ascii_lowercase().as_str() {
        "spec" => Ok(DocType::Spec),
        "prd" => Ok(DocType::Prd),
        "guide" => Ok(DocType::Guide),
        "adr" => Ok(DocType::Adr),
        "whitepaper" => Ok(DocType::Whitepaper),
        "todo" => Ok(DocType::Todo),
        "archive" => Ok(DocType::Archive),
        "readme" => Ok(DocType::Readme),
        "changelog" => Ok(DocType::Changelog),
        other => bail!("unknown doc type '{other}'"),
    }
}

fn parse_entity_kind(value: &str) -> Result<EntityKind> {
    match value.trim().to_ascii_lowercase().as_str() {
        "record" | "doc" => Ok(EntityKind::Record),
        "task" => Ok(EntityKind::Task),
        "relation" => Ok(EntityKind::Relation),
        other => bail!("unknown entity kind '{other}'"),
    }
}

fn parse_archive_trigger(value: &str) -> Result<ArchiveTrigger> {
    match value.trim().to_ascii_lowercase().as_str() {
        "superseded" => Ok(ArchiveTrigger::Superseded),
        "completed_and_inactive" | "completed-and-inactive" | "completed" => {
            Ok(ArchiveTrigger::CompletedAndInactive)
        }
        "structurally_outdated" | "structurally-outdated" | "outdated" => {
            Ok(ArchiveTrigger::StructurallyOutdated)
        }
        "history_only" | "history-only" | "history" => Ok(ArchiveTrigger::HistoryOnly),
        other => bail!("unknown archive reason '{other}'"),
    }
}

fn infer_entity_kind(id: &str) -> EntityKind {
    if id.contains(':') {
        EntityKind::Record
    } else if id.starts_with("DOC-") || id.starts_with("TASK-") {
        EntityKind::Task
    } else {
        EntityKind::Relation
    }
}

fn find_by_id(snapshot: &StructuredDocsSnapshot, id: &str) -> Option<GetOutput> {
    if let Some(record) = snapshot.records.iter().find(|record| record.doc_id == id) {
        return Some(GetOutput::Record {
            record: record.clone(),
            content_revision: snapshot.manifest.content_revision,
            projection_version: snapshot.render_state.projection_version,
            generation_id: snapshot.render_state.generation_id.clone(),
        });
    }
    if let Some(task) = snapshot.doc_tasks.iter().find(|task| task.task_id == id) {
        return Some(GetOutput::Task {
            task: task.clone(),
            content_revision: snapshot.manifest.content_revision,
            projection_version: snapshot.render_state.projection_version,
            generation_id: snapshot.render_state.generation_id.clone(),
        });
    }
    snapshot
        .relations
        .iter()
        .find(|relation| relation.relation_id == id)
        .map(|relation| GetOutput::Relation {
            relation: relation.clone(),
            content_revision: snapshot.manifest.content_revision,
            projection_version: snapshot.render_state.projection_version,
            generation_id: snapshot.render_state.generation_id.clone(),
        })
}

fn render_cutover_output(root: PathBuf, result: StructuredDocsCutoverResult, json: bool) -> CliOutput {
    let ok = result.extract.ok
        && result.render.as_ref().is_some_and(|render| render.ok)
        && result.validate.as_ref().is_some_and(|validate| validate.ok);
    if json {
        let payload = json!({
            "command": "cutover",
            "root": root.display().to_string(),
            "ok": ok,
            "extract": result.extract,
            "render": result.render,
            "validate": result.validate,
        });
        return CliOutput::new(if ok { SUCCESS } else { INVALID_INPUT }, to_pretty_json(&payload), String::new());
    }

    let mut stdout = String::new();
    stdout.push_str(&format!("[extract] {}\n", result.extract.message));
    if let Some(render) = result.render.as_ref() {
        stdout.push_str(&format!("[render] {}\n", render.message));
    }
    if let Some(validate) = result.validate.as_ref() {
        stdout.push_str(&format!("[validate] {}\n", validate.message));
        append_validation_summary(&mut stdout, validate.validation.as_ref());
    }
    if ok {
        stdout.push_str(&format!("cutover complete for {}\n", root.display()));
    }
    CliOutput::new(if ok { SUCCESS } else { INVALID_INPUT }, stdout, String::new())
}

fn render_document_result_output(result: DocumentOpResult, json: bool, strict: bool) -> CliOutput {
    let mut exit_code = if result.ok { SUCCESS } else { INVALID_INPUT };
    if strict && exit_code == SUCCESS && !result.warnings.is_empty() {
        exit_code = STRICT_WARNING;
    }

    if json {
        return CliOutput::new(exit_code, to_pretty_json(&result), String::new());
    }

    let mut stdout = String::new();
    stdout.push_str(&result.message);
    stdout.push('\n');
    if !result.records.is_empty() {
        stdout.push_str(&format!("records: {}\n", result.records.len()));
    }
    if !result.doc_tasks.is_empty() {
        stdout.push_str(&format!("tasks: {}\n", result.doc_tasks.len()));
    }
    if !result.relations.is_empty() {
        stdout.push_str(&format!("relations: {}\n", result.relations.len()));
    }
    if !result.warnings.is_empty() {
        stdout.push_str(&format!("warnings: {}\n", result.warnings.len()));
        for warning in &result.warnings {
            stdout.push_str(&format!("- {}\n", warning));
        }
    }
    append_validation_summary(&mut stdout, result.validation.as_ref());

    let stderr = if exit_code == WRITE_FAILURE {
        "write failure\n".to_string()
    } else {
        String::new()
    };
    CliOutput::new(exit_code, stdout, stderr)
}

fn append_validation_summary(output: &mut String, report: Option<&StructuredDocsValidationReport>) {
    let Some(report) = report else {
        return;
    };
    let has_issues = !report.ok;
    let has_unregistered = !report.unregistered_files.is_empty();
    if !has_issues && !has_unregistered {
        return;
    }
    if has_issues {
        output.push_str(&format!(
            "validation failed: missing={} mismatched={} version_mismatches={} broken_relations={}\n",
            report.missing_files.len(),
            report.mismatched_files.len(),
            report.version_mismatches.len(),
            report.broken_relations.len(),
        ));
    }
    for path in &report.missing_files {
        output.push_str(&format!("missing: {}\n", path));
    }
    for issue in &report.mismatched_files {
        output.push_str(&format!("mismatch: {} :: {}\n", issue.path, issue.message));
    }
    for issue in &report.version_mismatches {
        output.push_str(&format!("version mismatch: {} :: {}\n", issue.path, issue.message));
    }
    for issue in &report.broken_relations {
        output.push_str(&format!("broken relation: {} :: {}\n", issue.path, issue.message));
    }
    if has_unregistered {
        output.push_str(&format!(
            "warning: {} unregistered file(s) in docs/ (not in docs-data/records/)\n",
            report.unregistered_files.len()
        ));
        for path in &report.unregistered_files {
            output.push_str(&format!("unregistered: {}\n", path));
        }
    }
}

fn usage() -> String {
    [
        "omega-doc commands:",
        "  doctor [--root <path>] [--json]",
        "  render [DOC_ID...] [--root <path>] [--check|--plan|--apply] [--json]",
        "  validate [DOC_ID...] [--root <path>] [--json] [--strict]",
        "  extract <SOURCE...> [--root <path>] [--doc-type <type>] [--check|--plan|--apply] [--json]",
        "  cutover <SOURCE...> [--root <path>] [--doc-type <type>] [--json]",
        "  archive <doc-id> [--reason <reason>] [--replaced-by <doc-id>] [--root <path>] [--check|--plan|--apply] [--json]",
        "  get <doc-id|task-id|relation-id> [--root <path>] [--json]",
        "  list <docs|tasks|relations> [--root <path>] [--type <doc-type>] [--json]",
        "  record upsert --input <json> [--root <path>] [--check|--plan|--apply] [--json]",
        "  task upsert --input <json> [--root <path>] [--check|--plan|--apply] [--json]",
        "  relation upsert --input <json> [--root <path>] [--check|--plan|--apply] [--json]",
        "  remove <id> [--kind <record|task|relation>] [--root <path>] [--check|--plan|--apply] [--json]",
    ]
    .join("\n")
        + "\n"
}