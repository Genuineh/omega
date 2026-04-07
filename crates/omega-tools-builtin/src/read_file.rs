use std::fs;
use std::path::PathBuf;

use anyhow::Result;
use omega_tools::{ToolErrorKind, ToolHandler, ToolResult};
use serde_json::{json, Value};
use tracing::{debug, info, warn};

use crate::path_safety::{resolve_file_root, safe_path_within_root};
use crate::shared::{build_tool_error, build_tool_result, parse_positive_integer_field};

const MAX_READ_CHARS: usize = 50_000;

#[derive(Debug, Clone)]
pub struct ReadHandler {
    root: PathBuf,
}

impl ReadHandler {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root: resolve_file_root(root),
        }
    }

    fn run(&self, input: Value) -> ToolResult {
        let path_arg = match input.get("path").and_then(Value::as_str).map(str::trim) {
            Some(path) if !path.is_empty() => path,
            _ => {
                return build_tool_error(
                    "Error: Missing required field 'path'".to_string(),
                    json!({}),
                    ToolErrorKind::Validation,
                );
            }
        };
        let start_line = match parse_positive_integer_field(&input, "start_line") {
            Ok(value) => value,
            Err(message) => {
                return build_tool_error(
                    message,
                    json!({ "path": path_arg }),
                    ToolErrorKind::Validation,
                );
            }
        };
        let end_line = match parse_positive_integer_field(&input, "end_line") {
            Ok(value) => value,
            Err(message) => {
                return build_tool_error(
                    message,
                    json!({ "path": path_arg }),
                    ToolErrorKind::Validation,
                );
            }
        };
        let parsed_line_limit = match parse_positive_integer_field(&input, "limit") {
            Ok(value) => value,
            Err(message) => {
                return build_tool_error(
                    message,
                    json!({ "path": path_arg }),
                    ToolErrorKind::Validation,
                );
            }
        };
        let line_limit = if start_line.is_some() || end_line.is_some() {
            None
        } else {
            parsed_line_limit
        };
        if matches!((start_line, end_line), (Some(start), Some(end)) if end < start) {
            return build_tool_error(
                "Error: Field 'end_line' must be greater than or equal to 'start_line'".to_string(),
                json!({ "path": path_arg }),
                ToolErrorKind::Validation,
            );
        }
        info!(read_file.path = %path_arg);

        let resolved = match safe_path_within_root(&self.root, path_arg) {
            Ok(path) => path,
            Err(message) => {
                warn!(read_file.blocked_reason = %message, read_file.path = %path_arg);
                return build_tool_error(
                    message,
                    json!({ "path": path_arg }),
                    ToolErrorKind::Policy,
                );
            }
        };

        let text = match fs::read_to_string(&resolved) {
            Ok(text) => text,
            Err(error) => {
                warn!(read_file.error = %error, read_file.path = %path_arg);
                return build_tool_error(
                    format!("Error: {error}"),
                    json!({ "path": path_arg }),
                    ToolErrorKind::Execution,
                );
            }
        };

        let total_lines = text.lines().count();
        let total_chars = text.chars().count();
        let mut truncated = false;
        let mut omitted_lines = 0usize;
        let mut returned_line_count = total_lines;

        let lines: Vec<&str> = text.lines().collect();
        let mut result = if let Some(limit) = line_limit {
            if limit < lines.len() {
                omitted_lines = lines.len() - limit;
                truncated = true;
                returned_line_count = limit;
                debug!(
                    read_file.limited = true,
                    read_file.omitted_lines = omitted_lines
                );
                format!(
                    "{}\n... ({} more lines)",
                    lines[..limit].join("\n"),
                    omitted_lines
                )
            } else {
                returned_line_count = lines.len();
                text
            }
        } else if start_line.is_some() || end_line.is_some() {
            let start = start_line.unwrap_or(1);
            let end = end_line.unwrap_or(total_lines.max(start));
            let start_index = start.saturating_sub(1);
            if start_index >= lines.len() {
                returned_line_count = 0;
                String::new()
            } else {
                let end_index = end.min(lines.len());
                returned_line_count = end_index.saturating_sub(start_index);
                lines[start_index..end_index].join("\n")
            }
        } else {
            text
        };

        if result.chars().count() > MAX_READ_CHARS {
            truncated = true;
            debug!(
                read_file.truncated = true,
                read_file.max_chars = MAX_READ_CHARS
            );
            result = result.chars().take(MAX_READ_CHARS).collect();
        }

        let mut metadata = json!({
            "path": path_arg,
            "line_count": total_lines,
            "char_count": total_chars,
            "returned_line_count": returned_line_count,
        });
        if let Some(limit) = line_limit {
            metadata["line_limit"] = json!(limit);
        }
        if let Some(start_line) = start_line {
            metadata["start_line"] = json!(start_line);
        }
        if let Some(end_line) = end_line {
            metadata["end_line"] = json!(end_line);
        }
        if omitted_lines > 0 {
            metadata["omitted_lines"] = json!(omitted_lines);
        }
        if truncated {
            metadata["max_chars"] = json!(MAX_READ_CHARS);
        }

        info!(read_file.bytes = result.len());
        build_tool_result(result, metadata, truncated, None)
    }
}

impl ToolHandler for ReadHandler {
    fn name(&self) -> &str {
        "read_file"
    }

    fn description(&self) -> &str {
        "Read file contents."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Relative path to the file within the workspace root."
                },
                "start_line": {
                    "type": "integer",
                    "description": "Optional 1-based first line to read. Cannot be combined with limit.",
                    "minimum": 1
                },
                "end_line": {
                    "type": "integer",
                    "description": "Optional 1-based last line to read, inclusive. Cannot be combined with limit.",
                    "minimum": 1
                },
                "limit": {
                    "type": "integer",
                    "description": "Optional legacy maximum number of lines to return from the start of the file. Cannot be combined with start_line/end_line.",
                    "minimum": 1
                }
            },
            "required": ["path"]
        })
    }

    fn execute(&self, input: Value) -> Result<String> {
        Ok(self.run(input).output)
    }

    fn execute_v2(&self, input: Value) -> Result<ToolResult> {
        Ok(self.run(input))
    }
}
