use std::collections::BTreeSet;
use std::path::Path;

#[cfg(test)]
use omega_workflow::StepOutputContract;
use omega_workflow::{DataFormat, WorkflowStep, EXECUTE_STEP_ID, EXPLORE_STEP_ID, PLAN_STEP_ID};
use serde::Deserialize;
use serde_json::Value;

use crate::prompt_builder::render_output_contract;

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

pub(crate) fn validate_feature_step_output(
    step: &WorkflowStep,
    value: &Value,
) -> anyhow::Result<()> {
    match step.id.as_str() {
        EXPLORE_STEP_ID => {
            parse_feature_explore_output(value.clone())?;
            Ok(())
        }
        PLAN_STEP_ID => {
            parse_feature_plan_output(value.clone())?;
            Ok(())
        }
        EXECUTE_STEP_ID => {
            parse_feature_execute_output(value.clone())?;
            Ok(())
        }
        _ => Ok(()),
    }
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
            "Your previous response for step '{}' failed validation: {}. Re-run the step and satisfy the expected structured output.",
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
