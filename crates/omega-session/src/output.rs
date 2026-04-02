use std::collections::BTreeSet;
use std::path::Path;

#[cfg(test)]
use omega_workflow::StepOutputContract;
use omega_workflow::{
    DataFormat, WorkflowStep, DEEP_RESEARCH_WORKFLOW_ID, EXECUTE_STEP_ID, EXPLORE_STEP_ID,
    PLAN_STEP_ID, RESEARCH_WORKFLOW_ID,
};
use serde::Deserialize;
use serde_json::Value;

use omega_context::render_output_contract;

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
struct FeatureExploreOutput {
    objective: String,
    key_findings: Vec<String>,
    constraints: Vec<String>,
    risks: Vec<String>,
    affected_paths: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub(crate) struct FeaturePlanOutput {
    pub(crate) goal: String,
    pub(crate) tasks: Vec<FeaturePlanTask>,
    pub(crate) validation_targets: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub(crate) struct FeaturePlanTask {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) description: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub(crate) struct FeatureExecuteOutput {
    pub(crate) completed_tasks: Vec<String>,
    pub(crate) open_tasks: Vec<String>,
    pub(crate) validation_results: Vec<FeatureValidationResult>,
    pub(crate) changed_paths: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub(crate) struct FeatureValidationResult {
    target: String,
    status: String,
    #[serde(default)]
    details: Option<String>,
}

#[cfg(test)]
pub(crate) fn validate_structured_output(
    output_contract: &StepOutputContract,
    final_text: &str,
) -> anyhow::Result<Option<Value>> {
    match output_contract {
        StepOutputContract::None => Ok(None),
        StepOutputContract::Required { format, .. } => parse_structured_output(*format, final_text)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "expected {} output but response was not valid {}",
                    format.as_str(),
                    format.as_str()
                )
            })
            .map(Some),
        StepOutputContract::Optional { format, .. } => {
            Ok(parse_structured_output(*format, final_text))
        }
    }
}

#[cfg(test)]
fn parse_structured_output(format: DataFormat, final_text: &str) -> Option<Value> {
    parse_structured_output_candidates(format, final_text)
        .into_iter()
        .next()
}

pub(crate) fn parse_structured_output_candidates(
    format: DataFormat,
    final_text: &str,
) -> Vec<Value> {
    match format {
        DataFormat::Json => parse_json_values(final_text),
    }
}

pub(crate) fn validate_schema_file(
    root: &Path,
    schema_path: &Path,
    value: &Value,
) -> anyhow::Result<()> {
    let path = if schema_path.is_absolute() {
        schema_path.to_path_buf()
    } else {
        root.join(schema_path)
    };
    let raw = std::fs::read_to_string(&path)
        .map_err(|error| anyhow::anyhow!("failed to read schema {}: {error}", path.display()))?;
    let schema = serde_json::from_str::<Value>(&raw)
        .map_err(|error| anyhow::anyhow!("failed to parse schema {}: {error}", path.display()))?;
    validate_schema_value(&schema, value, "$", &path)
}

fn validate_schema_value(
    schema: &Value,
    value: &Value,
    location: &str,
    schema_path: &Path,
) -> anyhow::Result<()> {
    match schema.get("type").and_then(|value| value.as_str()) {
        Some("object") => {
            let object = value.as_object().ok_or_else(|| {
                anyhow::anyhow!(
                    "schema {} expected object at {}",
                    schema_path.display(),
                    location
                )
            })?;
            if let Some(required) = schema.get("required").and_then(|value| value.as_array()) {
                for key in required.iter().filter_map(|value| value.as_str()) {
                    if !object.contains_key(key) {
                        anyhow::bail!(
                            "schema {} missing required key {}{}",
                            schema_path.display(),
                            if location == "$" { "" } else { "." },
                            if location == "$" {
                                key.to_string()
                            } else {
                                format!("{location}.{key}")
                            }
                        );
                    }
                }
            }
            if let Some(properties) = schema.get("properties").and_then(|value| value.as_object()) {
                for (key, property_schema) in properties {
                    if let Some(property_value) = object.get(key) {
                        let property_location = if location == "$" {
                            format!("$.{key}")
                        } else {
                            format!("{location}.{key}")
                        };
                        validate_schema_value(
                            property_schema,
                            property_value,
                            &property_location,
                            schema_path,
                        )?;
                    }
                }
            }
            Ok(())
        }
        Some("array") => {
            let items = value.as_array().ok_or_else(|| {
                anyhow::anyhow!(
                    "schema {} expected array at {}",
                    schema_path.display(),
                    location
                )
            })?;
            if let Some(item_schema) = schema.get("items") {
                for (index, item) in items.iter().enumerate() {
                    validate_schema_value(
                        item_schema,
                        item,
                        &format!("{}[{}]", location, index),
                        schema_path,
                    )?;
                }
            }
            Ok(())
        }
        Some("string") if value.is_string() => Ok(()),
        Some("string") => anyhow::bail!(
            "schema {} expected string at {}",
            schema_path.display(),
            location
        ),
        Some("number") if value.is_number() => Ok(()),
        Some("number") => anyhow::bail!(
            "schema {} expected number at {}",
            schema_path.display(),
            location
        ),
        Some("boolean") if value.is_boolean() => Ok(()),
        Some("boolean") => anyhow::bail!(
            "schema {} expected boolean at {}",
            schema_path.display(),
            location
        ),
        _ => Ok(()),
    }
}

pub(crate) fn validate_workflow_step_output(
    _root: &Path,
    workflow_id: &str,
    step: &WorkflowStep,
    value: &Value,
) -> anyhow::Result<()> {
    match step.id.as_str() {
        EXPLORE_STEP_ID => {
            parse_feature_explore_output(value.clone())?;
            Ok(())
        }
        PLAN_STEP_ID => {
            let output = parse_feature_plan_output(value.clone())?;
            if workflow_id == DEEP_RESEARCH_WORKFLOW_ID {
                validate_research_plan_output(&output)?;
            }
            Ok(())
        }
        EXECUTE_STEP_ID => {
            let output = parse_feature_execute_output(value.clone())?;
            if matches!(workflow_id, RESEARCH_WORKFLOW_ID | DEEP_RESEARCH_WORKFLOW_ID)
                && !output.changed_paths.is_empty()
            {
                anyhow::bail!(
                    "research execute output must keep changed_paths empty because the workflow is read-only"
                );
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn validate_research_plan_output(output: &FeaturePlanOutput) -> anyhow::Result<()> {
    for task in &output.tasks {
        let combined = format!("{}\n{}", task.title, task.description);
        if research_plan_text_requires_write_access(&combined) {
            anyhow::bail!(
                "research plan task '{}' must stay read-only and must not require file edits, code changes, or other write-capable actions",
                task.id
            );
        }
    }

    for target in &output.validation_targets {
        if research_plan_text_requires_write_access(target) {
            anyhow::bail!(
                "research validation target '{}' must stay read-only and executable with read-only tooling",
                target
            );
        }
    }

    Ok(())
}

fn research_plan_text_requires_write_access(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }

    if trimmed.contains("apply_patch")
        || trimmed.contains("write_file")
        || trimmed.contains("edit_file")
        || trimmed.contains("create_file")
    {
        return true;
    }

    if starts_with_read_only_analysis_prefix(trimmed) {
        // With a read-only prefix, only check exact write phrases.
        // Skip the bag-of-words action+target check that causes false
        // positives when analytical text mentions concepts like "update",
        // "config", or "module" without intending a write operation.
        return contains_exact_write_phrases(trimmed);
    }

    contains_explicit_write_operation(trimmed)
}

fn starts_with_read_only_analysis_prefix(text: &str) -> bool {
    const ASCII_PREFIXES: &[&str] = &[
        "analyze",
        "assess",
        "audit",
        "check",
        "confirm",
        "evaluate",
        "gather",
        "identify",
        "inspect",
        "investigate",
        "report",
        "review",
        "study",
        "summarize",
        "survey",
        "validate",
        "verify",
    ];
    const CJK_PREFIXES: &[&str] = &[
        "分析", "评估", "审查", "检查", "确认", "验证", "汇总", "调查", "研究", "梳理", "审计",
        "收集", "识别", "输出",
    ];

    let normalized = text.trim_start().to_ascii_lowercase();
    ASCII_PREFIXES
        .iter()
        .any(|prefix| normalized.starts_with(prefix))
        || CJK_PREFIXES.iter().any(|prefix| text.starts_with(prefix))
}

fn contains_explicit_write_operation(text: &str) -> bool {
    contains_exact_write_phrases(text) || contains_ascii_write_pair(&text.to_ascii_lowercase())
}

/// Matches explicit multi-word write phrases and CJK write pairs.
/// Does NOT include the bag-of-words action+target check, so it is safe
/// for analytical text that mentions optimization concepts like "update",
/// "config", or "module" without intending a write operation.
fn contains_exact_write_phrases(text: &str) -> bool {
    let normalized = text.to_ascii_lowercase();

    const ASCII_WRITE_PHRASES: &[&str] = &[
        "add test",
        "add tests",
        "apply patch",
        "change code",
        "create patch",
        "create file",
        "delete file",
        "edit code",
        "edit config",
        "edit doc",
        "edit docs",
        "edit file",
        "implement fix",
        "implement the code",
        "modify code",
        "modify config",
        "refactor module",
        "remove file",
        "rename file",
        "update code",
        "update config",
        "update doc",
        "update docs",
        "write code",
        "write test",
        "write tests",
    ];
    const CJK_WRITE_PHRASES: &[&str] = &[
        "修改代码",
        "修改配置",
        "修改文件",
        "修改文档",
        "更新代码",
        "更新配置",
        "更新文档",
        "添加测试",
        "补充测试",
        "编写测试",
        "实现修复",
        "实现功能",
        "重构模块",
        "重命名文件",
        "删除文件",
        "移除文件",
        "创建补丁",
        "编写代码",
        "写入文件",
    ];

    ASCII_WRITE_PHRASES
        .iter()
        .any(|phrase| normalized.contains(phrase))
        || CJK_WRITE_PHRASES.iter().any(|phrase| text.contains(phrase))
        || contains_cjk_write_pair(text, "补充", "测试")
        || contains_cjk_write_pair(text, "添加", "测试")
        || contains_cjk_write_pair(text, "编写", "测试")
        || contains_cjk_write_pair(text, "修改", "文件")
        || contains_cjk_write_pair(text, "修改", "代码")
        || contains_cjk_write_pair(text, "修改", "文档")
        || contains_cjk_write_pair(text, "更新", "配置")
}

fn contains_ascii_write_pair(normalized: &str) -> bool {
    const ACTIONS: &[&str] = &[
        "add",
        "apply",
        "change",
        "create",
        "delete",
        "edit",
        "implement",
        "migrate",
        "modify",
        "patch",
        "refactor",
        "remove",
        "rename",
        "update",
        "write",
    ];
    const TARGETS: &[&str] = &[
        "code",
        "config",
        "doc",
        "docs",
        "documentation",
        "file",
        "files",
        "module",
        "modules",
        "prompt",
        "prompts",
        "schema",
        "schemas",
        "test",
        "tests",
        "workflow",
        "workflows",
    ];

    let tokens = normalized
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>();
    let has_action = tokens
        .iter()
        .any(|token| ACTIONS.iter().any(|action| token == action));
    let has_target = tokens
        .iter()
        .any(|token| TARGETS.iter().any(|target| token == target));

    (has_action && has_target)
        || normalized.contains("apply changes")
        || normalized.contains("make changes")
}

fn contains_cjk_write_pair(text: &str, action: &str, target: &str) -> bool {
    text.contains(action) && text.contains(target)
}

fn parse_feature_explore_output(value: Value) -> anyhow::Result<FeatureExploreOutput> {
    let output = serde_json::from_value::<FeatureExploreOutput>(value)?;
    if output.objective.trim().is_empty() {
        anyhow::bail!("explore output objective must be non-empty");
    }
    if output.key_findings.is_empty() {
        anyhow::bail!("explore output must include at least one key finding");
    }
    for finding in &output.key_findings {
        if finding.trim().is_empty() {
            anyhow::bail!("explore key_findings must be non-empty strings");
        }
    }
    for constraint in &output.constraints {
        if constraint.trim().is_empty() {
            anyhow::bail!("explore constraints must be non-empty strings");
        }
    }
    for risk in &output.risks {
        if risk.trim().is_empty() {
            anyhow::bail!("explore risks must be non-empty strings");
        }
    }
    for path in &output.affected_paths {
        if path.trim().is_empty() {
            anyhow::bail!("explore affected_paths must be non-empty strings");
        }
    }
    Ok(output)
}

pub(crate) fn parse_feature_plan_output(value: Value) -> anyhow::Result<FeaturePlanOutput> {
    let output = serde_json::from_value::<FeaturePlanOutput>(value)?;
    if output.goal.trim().is_empty() {
        anyhow::bail!("plan output goal must be non-empty");
    }
    if output.tasks.is_empty() {
        anyhow::bail!("plan output must include at least one task");
    }
    let mut seen_ids = BTreeSet::new();
    for task in &output.tasks {
        if task.id.trim().is_empty() {
            anyhow::bail!("plan task id must be non-empty");
        }
        if !seen_ids.insert(task.id.trim().to_string()) {
            anyhow::bail!("plan task ids must be unique");
        }
        if task.title.trim().is_empty() || task.description.trim().is_empty() {
            anyhow::bail!("plan task title and description must be non-empty");
        }
    }
    Ok(output)
}

pub(crate) fn parse_feature_execute_output(value: Value) -> anyhow::Result<FeatureExecuteOutput> {
    let output = serde_json::from_value::<FeatureExecuteOutput>(value)?;
    let completed = output
        .completed_tasks
        .iter()
        .map(|id| id.trim())
        .collect::<BTreeSet<_>>();
    for task_id in &output.open_tasks {
        let task_id = task_id.trim();
        if task_id.is_empty() {
            anyhow::bail!("execute output open_tasks must not contain empty ids");
        }
        if completed.contains(task_id) {
            anyhow::bail!("execute output task ids cannot be both completed and open");
        }
    }
    for result in &output.validation_results {
        if result.target.trim().is_empty() || result.status.trim().is_empty() {
            anyhow::bail!("execute validation_results entries must include target and status");
        }
        if result
            .details
            .as_deref()
            .is_some_and(|details| details.trim().is_empty())
        {
            anyhow::bail!("execute validation result details must be non-empty when present");
        }
    }
    for path in &output.changed_paths {
        if path.trim().is_empty() {
            anyhow::bail!("execute changed_paths must be non-empty strings");
        }
    }
    Ok(output)
}

pub(crate) fn build_output_validation_feedback(
    root: &Path,
    step: &WorkflowStep,
    validation_error: &str,
) -> String {
    let contract = render_output_contract(root, &step.output_contract);
    if contract.is_empty() {
        format!(
            "Your previous response for step '{}' failed validation: {}. Re-run the step and satisfy the required response constraints.",
            step.id, validation_error
        )
    } else {
        format!(
            "Your previous response for step '{}' failed validation: {}. Re-run the step and respond with valid structured output matching this contract:\n\n{}",
            step.id, validation_error, contract
        )
    }
}

pub(crate) fn parse_json_value(text: &str) -> Option<Value> {
    parse_json_values(text).into_iter().next()
}

pub(crate) fn parse_json_values(text: &str) -> Vec<Value> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    let mut values = Vec::new();
    let mut seen = BTreeSet::new();
    push_json_candidate(trimmed, &mut values, &mut seen);

    for prefix in ["```json", "```JSON", "```"] {
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            let body = rest.trim();
            if let Some(body) = body.strip_suffix("```") {
                push_json_candidate(body.trim(), &mut values, &mut seen);
            }
        }
    }

    for candidate in extract_top_level_json_candidates(trimmed) {
        push_json_candidate(candidate, &mut values, &mut seen);
    }

    // Unwrap array candidates: if a candidate is a JSON array of objects,
    // also push each object element so schema-level "type: object" checks
    // can match individual items inside an array wrapper.
    let unwrapped: Vec<Value> = values
        .iter()
        .filter_map(|v| v.as_array())
        .flatten()
        .filter(|v| v.is_object())
        .cloned()
        .collect();
    for element in unwrapped {
        let fingerprint = serde_json::to_string(&element).unwrap_or_default();
        if seen.insert(fingerprint) {
            values.push(element);
        }
    }

    values
}

fn push_json_candidate(candidate: &str, values: &mut Vec<Value>, seen: &mut BTreeSet<String>) {
    let Ok(value) = serde_json::from_str::<Value>(candidate) else {
        return;
    };
    let fingerprint =
        serde_json::to_string(&value).unwrap_or_else(|_| candidate.trim().to_string());
    if seen.insert(fingerprint) {
        values.push(value);
    }
}

fn extract_top_level_json_candidates(text: &str) -> Vec<&str> {
    let mut candidates = Vec::new();
    let mut search_start = 0;

    while search_start < text.len() {
        let Some((relative_start, _)) = text[search_start..]
            .char_indices()
            .find(|(_, character)| matches!(character, '{' | '['))
        else {
            break;
        };
        let start = search_start + relative_start;
        let candidate = &text[start..];
        let Some(end) = find_top_level_json_end(candidate) else {
            search_start = start + 1;
            continue;
        };
        let candidate = &candidate[..end];
        if serde_json::from_str::<Value>(candidate).is_ok() {
            candidates.push(candidate);
            search_start = start + end;
        } else {
            search_start = start + 1;
        }
    }

    candidates
}

fn find_top_level_json_end(text: &str) -> Option<usize> {
    let mut depth = 0u32;
    let mut in_string = false;
    let mut escaped = false;

    for (index, character) in text.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                in_string = false;
            }
            continue;
        }

        match character {
            '"' => in_string = true,
            '{' | '[' => depth += 1,
            '}' | ']' => {
                if depth == 0 {
                    return None;
                }
                depth -= 1;
                if depth == 0 {
                    return Some(index + character.len_utf8());
                }
            }
            _ => {}
        }
    }

    None
}
