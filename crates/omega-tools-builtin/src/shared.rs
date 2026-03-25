use std::fs;
use std::path::{Path, PathBuf};

use omega_tools::{ToolErrorKind, ToolResult};
use serde_json::Value;
use similar::TextDiff;

pub(crate) const MAX_OUTPUT_CHARS: usize = 50_000;

#[derive(Debug, Clone)]
pub(crate) struct CapturedOutput {
    pub(crate) output: String,
    pub(crate) truncated: bool,
}

pub(crate) fn preview_text(text: &str, limit: usize) -> String {
    let mut chars = text.chars();
    let preview: String = chars.by_ref().take(limit).collect();
    if chars.next().is_some() {
        format!("{}...", preview)
    } else {
        text.to_string()
    }
}

pub(crate) fn truncate_output_chars(output: String, max_chars: usize) -> CapturedOutput {
    if output.chars().count() <= max_chars {
        return CapturedOutput {
            output,
            truncated: false,
        };
    }

    CapturedOutput {
        output: output.chars().take(max_chars).collect(),
        truncated: true,
    }
}

pub(crate) fn optional_preview(text: &str, limit: usize) -> Option<String> {
    let preview = preview_text(text, limit);
    (!preview.is_empty()).then_some(preview)
}

pub(crate) fn build_tool_result(
    output: String,
    metadata: Value,
    truncated: bool,
    error_kind: Option<ToolErrorKind>,
) -> ToolResult {
    let preview = optional_preview(&output, 120);
    let mut result = ToolResult::success(output)
        .with_optional_preview(preview)
        .with_metadata(metadata)
        .with_truncated(truncated);
    if let Some(error_kind) = error_kind {
        result = result.with_error_kind(error_kind);
    }
    result
}

pub(crate) fn build_tool_error(
    output: String,
    metadata: Value,
    error_kind: ToolErrorKind,
) -> ToolResult {
    build_tool_result(output, metadata, false, Some(error_kind))
}

pub(crate) fn parse_positive_integer_field(
    input: &Value,
    field: &str,
) -> std::result::Result<Option<usize>, String> {
    match input.get(field) {
        None => Ok(None),
        Some(Value::Number(number)) => number
            .as_u64()
            .filter(|value| *value > 0)
            .map(|value| Some(value as usize))
            .ok_or_else(|| format!("Error: Field '{field}' must be a positive integer")),
        Some(_) => Err(format!("Error: Field '{field}' must be a positive integer")),
    }
}

pub(crate) fn parse_limit_field(
    input: &Value,
    field: &str,
    default: usize,
) -> std::result::Result<usize, String> {
    Ok(parse_positive_integer_field(input, field)?.unwrap_or(default))
}

pub(crate) fn workspace_relative_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

pub(crate) fn build_glob_pattern(
    root: &Path,
    pattern: &str,
) -> std::result::Result<String, String> {
    let pattern = pattern.trim();
    if pattern.is_empty() {
        return Err("Error: Missing required field 'pattern'".to_string());
    }
    if Path::new(pattern).is_absolute() {
        return Err("Error: Pattern must be relative to the workspace root".to_string());
    }

    Ok(root.join(pattern).to_string_lossy().replace('\\', "/"))
}

pub(crate) fn render_lines_result(
    mut lines: Vec<String>,
    max_chars: usize,
    truncated_by_limit: bool,
    omitted_count: Option<usize>,
    empty_message: &str,
) -> CapturedOutput {
    if lines.is_empty() {
        return CapturedOutput {
            output: empty_message.to_string(),
            truncated: false,
        };
    }

    if truncated_by_limit {
        let suffix = omitted_count
            .map(|count| format!("... ({count} more results)"))
            .unwrap_or_else(|| "... (more results)".to_string());
        lines.push(suffix);
    }

    let captured = truncate_output_chars(lines.join("\n"), max_chars);
    CapturedOutput {
        truncated: truncated_by_limit || captured.truncated,
        ..captured
    }
}

pub(crate) fn line_count(text: &str) -> usize {
    if text.is_empty() {
        0
    } else {
        text.lines().count()
    }
}

pub(crate) fn count_occurrences(haystack: &str, needle: &str) -> usize {
    if needle.is_empty() {
        0
    } else {
        haystack.match_indices(needle).count()
    }
}

pub(crate) fn render_text_diff(path: &str, before: &str, after: &str) -> Option<String> {
    if before == after {
        return None;
    }

    let before_label = if before.is_empty() {
        "/dev/null".to_string()
    } else {
        format!("a/{path}")
    };
    let after_label = format!("b/{path}");

    Some(
        TextDiff::from_lines(before, after)
            .unified_diff()
            .context_radius(3)
            .header(&before_label, &after_label)
            .to_string(),
    )
}

pub(crate) fn build_change_tool_result(
    summary: String,
    metadata: Value,
    diff: Option<String>,
    error_kind: Option<ToolErrorKind>,
) -> ToolResult {
    let output = match diff {
        Some(diff) if !diff.trim().is_empty() => format!("{summary}\n\n{diff}"),
        _ => summary.clone(),
    };
    let captured = truncate_output_chars(output, MAX_OUTPUT_CHARS);

    let mut result = ToolResult::success(captured.output)
        .with_preview(summary)
        .with_metadata(metadata)
        .with_truncated(captured.truncated);
    if let Some(error_kind) = error_kind {
        result = result.with_error_kind(error_kind);
    }
    result
}

pub(crate) fn collect_search_files(path: &Path, files: &mut Vec<PathBuf>) -> std::io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    let file_type = metadata.file_type();
    if file_type.is_symlink() {
        return Ok(());
    }
    if metadata.is_file() {
        files.push(path.to_path_buf());
        return Ok(());
    }
    if !metadata.is_dir() {
        return Ok(());
    }

    let mut entries = fs::read_dir(path)?.collect::<std::result::Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        collect_search_files(&entry.path(), files)?;
    }

    Ok(())
}

pub(crate) fn preview_json_input(value: &Value, limit: usize) -> String {
    preview_text(
        &serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string()),
        limit,
    )
}
