use std::fs;
use std::path::PathBuf;

use anyhow::Result;
use omega_tools::{ToolErrorKind, ToolHandler, ToolResult};
use serde_json::{json, Value};
use tracing::warn;

use crate::path_safety::{resolve_file_root, safe_path_within_root};
use crate::shared::{build_change_tool_result, build_tool_error, line_count, render_text_diff};

#[derive(Debug, Clone)]
pub struct CreateFileHandler {
    root: PathBuf,
}

impl CreateFileHandler {
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
        let content = match input.get("content").and_then(Value::as_str) {
            Some(content) => content,
            None => {
                return build_tool_error(
                    "Error: Missing required field 'content'".to_string(),
                    json!({ "path": path_arg }),
                    ToolErrorKind::Validation,
                );
            }
        };

        let resolved = match safe_path_within_root(&self.root, path_arg) {
            Ok(path) => path,
            Err(message) => {
                warn!(create_file.blocked_reason = %message, create_file.path = %path_arg);
                return build_tool_error(
                    message,
                    json!({ "path": path_arg }),
                    ToolErrorKind::Policy,
                );
            }
        };

        if resolved.exists() {
            return build_tool_error(
                format!("Error: File already exists at {path_arg}"),
                json!({
                    "path": path_arg,
                    "already_exists": true,
                }),
                ToolErrorKind::Validation,
            );
        }

        if let Some(parent) = resolved.parent() {
            if let Err(error) = fs::create_dir_all(parent) {
                warn!(create_file.error = %error, create_file.path = %path_arg);
                return build_tool_error(
                    format!("Error: {error}"),
                    json!({ "path": path_arg }),
                    ToolErrorKind::Execution,
                );
            }
        }

        if let Err(error) = fs::write(&resolved, content) {
            warn!(create_file.error = %error, create_file.path = %path_arg);
            return build_tool_error(
                format!("Error: {error}"),
                json!({ "path": path_arg }),
                ToolErrorKind::Execution,
            );
        }

        let diff = render_text_diff(path_arg, "", content);
        let lines_after = line_count(content);
        build_change_tool_result(
            format!("Created {path_arg} ({} bytes)", content.len()),
            json!({
                "path": path_arg,
                "bytes_written": content.len(),
                "bytes_before": 0,
                "bytes_after": content.len(),
                "bytes_delta": content.len() as i64,
                "lines_before": 0,
                "lines_after": lines_after,
                "line_delta": lines_after as i64,
                "created": true,
                "diff_available": diff.is_some(),
            }),
            diff,
            None,
        )
    }
}

impl ToolHandler for CreateFileHandler {
    fn name(&self) -> &str {
        "create_file"
    }

    fn description(&self) -> &str {
        "Create a new file. Fails if the path already exists."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Relative path to the file to create. Parent directories are created if they do not exist."
                },
                "content": {
                    "type": "string",
                    "description": "Text content to write to the new file."
                }
            },
            "required": ["path", "content"]
        })
    }

    fn execute(&self, input: Value) -> Result<String> {
        Ok(self.run(input).output)
    }

    fn execute_v2(&self, input: Value) -> Result<ToolResult> {
        Ok(self.run(input))
    }
}
