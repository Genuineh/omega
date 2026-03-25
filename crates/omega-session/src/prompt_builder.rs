use std::path::Path;

use omega_workflow::{DataFormat, StepOutputContract};
use serde_json::Value;

use crate::runner::{OutputValidationFailure, StepExecutionInput};
use crate::session_state::{RoutingContext, SessionContext, StepSummary};

pub(crate) fn build_step_system_prompt(input: &StepExecutionInput) -> String {
    let mut sections = vec![
        input
            .resolved_skills
            .build_system_prompt(&input.base_system),
        format!("Workflow phase: {}", input.step.label),
        render_visible_tools(input.resolved_tools.tool_names()),
    ];
    let session_context = render_session_context(&input.session_context);
    if !session_context.trim().is_empty() {
        sections.push(format!(
            "<session_context>\n{}\n</session_context>",
            session_context.trim_end()
        ));
    }
    if let Some(structured_input) = input.structured_input.as_ref() {
        sections.push(format!(
            "<structured_input step_id=\"{}\">\n{}\n</structured_input>",
            input.step.id,
            render_structured_input(structured_input)
        ));
    }
    if let Some(todo_snapshot) = input.todo_snapshot.as_deref() {
        sections.push(format!(
            "<todo_state step_id=\"{}\">\n{}\n</todo_state>",
            input.step.id, todo_snapshot
        ));
    }
    let output_contract = render_output_contract(&input.cwd, &input.step.output_contract);
    if !output_contract.is_empty() {
        sections.push(format!(
            "<output_contract step_id=\"{}\">\n{}\n</output_contract>",
            input.step.id, output_contract
        ));
    }
    if !input.step_prompt.trim().is_empty() {
        sections.push(format!(
            "<workflow_prompt step_id=\"{}\" prompt_path=\"{}\">\n{}\n</workflow_prompt>",
            input.step.id,
            input.step.prompt_path.display(),
            input.step_prompt.trim_end()
        ));
    }

    sections.join("\n\n")
}

pub(crate) fn build_output_repair_system_prompt(
    input: &StepExecutionInput,
    failure: &OutputValidationFailure,
) -> String {
    let mut sections = vec![
        input
            .resolved_skills
            .build_system_prompt(&input.base_system),
        format!(
            "Workflow phase: {} (structured output repair)",
            input.step.label
        ),
        "Visible tools: none".to_string(),
    ];
    let session_context = render_session_context(&input.session_context);
    if !session_context.trim().is_empty() {
        sections.push(format!(
            "<session_context>\n{}\n</session_context>",
            session_context.trim_end()
        ));
    }
    if let Some(structured_input) = input.structured_input.as_ref() {
        sections.push(format!(
            "<structured_input step_id=\"{}\">\n{}\n</structured_input>",
            input.step.id,
            render_structured_input(structured_input)
        ));
    }
    if let Some(todo_snapshot) = input.todo_snapshot.as_deref() {
        sections.push(format!(
            "<todo_state step_id=\"{}\">\n{}\n</todo_state>",
            input.step.id, todo_snapshot
        ));
    }
    let output_contract = render_output_contract(&input.cwd, &input.step.output_contract);
    if !output_contract.is_empty() {
        sections.push(format!(
            "<output_contract step_id=\"{}\">\n{}\n</output_contract>",
            input.step.id, output_contract
        ));
    }
    sections.push(render_output_repair_envelope(input, failure));

    sections.join("\n\n")
}

fn render_output_repair_envelope(
    input: &StepExecutionInput,
    failure: &OutputValidationFailure,
) -> String {
    let mut lines = vec![
        "mode: repair_structured_output".to_string(),
        format!("error_kind: {}", failure.error_kind.as_str()),
        format!("validation_error: {}", failure.message),
        format!(
            "previous_response_preview: {}",
            failure.previous_response_preview
        ),
    ];
    if let Some(extracted_json_preview) = failure.extracted_json_preview() {
        lines.push(format!(
            "extracted_json_preview: {}",
            extracted_json_preview
        ));
    }
    let required_contract = render_output_contract(&input.cwd, &input.step.output_contract);
    if !required_contract.is_empty() {
        lines.push("required_contract:".to_string());
        lines.extend(required_contract.lines().map(ToOwned::to_owned));
    }
    lines.push(
        "repair_rules: preserve the meaning of the previous answer when possible".to_string(),
    );
    lines.push("repair_rules: do not add prose before or after the JSON".to_string());
    lines.push("repair_rules: if information is missing, infer only from the previous answer and existing structured_input".to_string());
    format!(
        "<output_repair step_id=\"{}\">\n{}\n</output_repair>",
        input.step.id,
        lines.join("\n")
    )
}

pub(crate) fn render_routing_context(routing: &RoutingContext) -> String {
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

pub(crate) fn render_visible_tools(tool_names: &[String]) -> String {
    if tool_names.is_empty() {
        "Visible tools: none".to_string()
    } else {
        format!("Visible tools: {}", tool_names.join(", "))
    }
}

fn render_step_summaries(step_summaries: &[StepSummary]) -> String {
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

pub(crate) fn render_session_context(session_context: &SessionContext) -> String {
    let mut sections = Vec::new();
    if !session_context.latest_user_turn.trim().is_empty() {
        sections.push(format!(
            "<latest_user_turn>\n{}\n</latest_user_turn>",
            session_context.latest_user_turn.trim_end()
        ));
    }

    let routing_context = render_routing_context(&session_context.routing);
    if !routing_context.trim().is_empty() {
        sections.push(format!(
            "<workflow_runtime>\n{}\n</workflow_runtime>",
            routing_context.trim_end()
        ));
    }

    if !session_context.step_summaries.is_empty() {
        sections.push(format!(
            "<step_summaries>\n{}\n</step_summaries>",
            render_step_summaries(&session_context.step_summaries)
        ));
    }

    sections.join("\n\n")
}

pub(crate) fn render_structured_input(structured_input: &Value) -> String {
    serde_json::to_string_pretty(structured_input).unwrap_or_else(|_| structured_input.to_string())
}

pub(crate) fn render_output_contract(root: &Path, output_contract: &StepOutputContract) -> String {
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
        } => {
            let mut lines = vec![
                "mode: optional".to_string(),
                format!("format: {}", format.as_str()),
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
    let raw = std::fs::read_to_string(path).ok()?;
    let schema = serde_json::from_str::<Value>(&raw).ok()?;
    serde_json::to_string_pretty(&schema).ok()
}

fn render_output_format_rules(format: DataFormat) -> Vec<String> {
    match format {
        DataFormat::Json => vec![
            "response_rules: return exactly one valid JSON value".to_string(),
            "response_rules: do not add prose before or after the JSON".to_string(),
            "response_rules: do not wrap the JSON in markdown fences".to_string(),
        ],
    }
}
