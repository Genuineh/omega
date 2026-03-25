use std::fs;
use std::path::PathBuf;

use anyhow::Result;
use omega_tools::{ToolErrorKind, ToolHandler, ToolResult};
use serde_json::{json, Value};
use tracing::{info, warn};

use crate::path_safety::{resolve_file_root, safe_path_within_root};
use crate::shared::{build_change_tool_result, build_tool_error, line_count, render_text_diff};

#[derive(Debug, Clone)]
pub struct WriteHandler {
    root: PathBuf,
}

impl WriteHandler {
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
        info!(write_file.path = %path_arg, write_file.bytes = content.len());

        let resolved = match safe_path_within_root(&self.root, path_arg) {
            Ok(path) => path,
            Err(message) => {
                warn!(write_file.blocked_reason = %message, write_file.path = %path_arg);
                return build_tool_error(
                    message,
                    json!({ "path": path_arg }),
                    ToolErrorKind::Policy,
                );
            }
        };

        let existed = resolved.exists();
        let previous_bytes = fs::metadata(&resolved)
            .ok()
            .and_then(|metadata| usize::try_from(metadata.len()).ok());
        let previous_content = fs::read_to_string(&resolved).ok();

        if let Some(parent) = resolved.parent() {
            if let Err(error) = fs::create_dir_all(parent) {
                warn!(write_file.error = %error, write_file.path = %path_arg);
                return build_tool_error(
                    format!("Error: {error}"),
                    json!({ "path": path_arg }),
                    ToolErrorKind::Execution,
                );
            }
        }

        if let Err(error) = fs::write(&resolved, content) {
            warn!(write_file.error = %error, write_file.path = %path_arg);
            return build_tool_error(
                format!("Error: {error}"),
                json!({ "path": path_arg }),
                ToolErrorKind::Execution,
            );
        }

        let diff = if let Some(previous_content) = previous_content.as_deref() {
            render_text_diff(path_arg, previous_content, content)
        } else if existed {
            None
        } else {
            render_text_diff(path_arg, "", content)
        };
        let lines_after = line_count(content);
        let lines_before = previous_content.as_deref().map(line_count);
        let line_delta = lines_before.map(|count| lines_after as i64 - count as i64);

        let summary = if existed {
            format!(
                "Wrote {} bytes to {path_arg} (previously {} bytes)",
                content.len(),
                previous_bytes.unwrap_or_default()
            )
        } else {
            format!("Wrote {} bytes to {path_arg}", content.len())
        };

        build_change_tool_result(
            summary,
            json!({
                "path": path_arg,
                "bytes_written": content.len(),
                "bytes_before": previous_bytes,
                "bytes_after": content.len(),
                "bytes_delta": previous_bytes.map(|bytes| content.len() as i64 - bytes as i64).unwrap_or(content.len() as i64),
                "lines_before": lines_before,
                "lines_after": lines_after,
                "line_delta": line_delta.unwrap_or(lines_after as i64),
                "previously_existed": existed,
                "diff_available": diff.is_some(),
            }),
            diff,
            None,
        )
    }
}

impl ToolHandler for WriteHandler {
    fn name(&self) -> &str {
        "write_file"
    }

    fn description(&self) -> &str {
        "Write content to file."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Relative path to the file to write. Parent directories are created if they do not exist."
                },
                "content": {
                    "type": "string",
                    "description": "Text content to write to the file."
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
