use std::fs;
use std::path::PathBuf;

use anyhow::Result;
use omega_tools::{ToolErrorKind, ToolHandler, ToolResult};
use serde_json::{json, Value};
use tracing::{info, warn};

use crate::path_safety::{resolve_file_root, safe_path_within_root};
use crate::shared::{
    build_change_tool_result, build_tool_error, count_occurrences, line_count, preview_text,
    render_text_diff,
};

#[derive(Debug, Clone)]
pub struct EditHandler {
    root: PathBuf,
}

impl EditHandler {
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
        let old_text = match input.get("old_text").and_then(Value::as_str) {
            Some(text) => text,
            None => {
                return build_tool_error(
                    "Error: Missing required field 'old_text'".to_string(),
                    json!({ "path": path_arg }),
                    ToolErrorKind::Validation,
                );
            }
        };
        let new_text = match input.get("new_text").and_then(Value::as_str) {
            Some(text) => text,
            None => {
                return build_tool_error(
                    "Error: Missing required field 'new_text'".to_string(),
                    json!({ "path": path_arg }),
                    ToolErrorKind::Validation,
                );
            }
        };
        info!(edit_file.path = %path_arg);

        let resolved = match safe_path_within_root(&self.root, path_arg) {
            Ok(path) => path,
            Err(message) => {
                warn!(edit_file.blocked_reason = %message, edit_file.path = %path_arg);
                return build_tool_error(
                    message,
                    json!({ "path": path_arg }),
                    ToolErrorKind::Policy,
                );
            }
        };

        let content = match fs::read_to_string(&resolved) {
            Ok(content) => content,
            Err(error) => {
                warn!(edit_file.error = %error, edit_file.path = %path_arg);
                return build_tool_error(
                    format!("Error: {error}"),
                    json!({ "path": path_arg }),
                    ToolErrorKind::Execution,
                );
            }
        };

        let match_count = count_occurrences(&content, old_text);
        if match_count == 0 {
            warn!(edit_file.not_found = true, edit_file.path = %path_arg);
            return build_tool_error(
                format!(
                    "Error: Text not found in {path_arg}. Read the file again and provide a more specific exact snippet."
                ),
                json!({
                    "path": path_arg,
                    "old_text_bytes": old_text.len(),
                    "new_text_bytes": new_text.len(),
                    "match_count": 0,
                    "current_bytes": content.len(),
                    "lines_in_file": line_count(&content),
                    "old_text_preview": preview_text(old_text, 80),
                }),
                ToolErrorKind::Validation,
            );
        }

        let new_content = content.replacen(old_text, new_text, 1);
        if let Err(error) = fs::write(&resolved, &new_content) {
            warn!(edit_file.error = %error, edit_file.path = %path_arg);
            return build_tool_error(
                format!("Error: {error}"),
                json!({ "path": path_arg }),
                ToolErrorKind::Execution,
            );
        }

        let diff = render_text_diff(path_arg, &content, &new_content);
        let lines_before = line_count(&content);
        let lines_after = line_count(&new_content);
        let summary = if match_count > 1 {
            format!("Edited {path_arg} (replaced first match of {match_count})")
        } else {
            format!("Edited {path_arg} (1 replacement)")
        };

        build_change_tool_result(
            summary,
            json!({
                "path": path_arg,
                "old_text_bytes": old_text.len(),
                "new_text_bytes": new_text.len(),
                "replacements": 1,
                "match_count": match_count,
                "ambiguous_match": match_count > 1,
                "bytes_before": content.len(),
                "bytes_after": new_content.len(),
                "bytes_delta": new_content.len() as i64 - content.len() as i64,
                "lines_before": lines_before,
                "lines_after": lines_after,
                "line_delta": lines_after as i64 - lines_before as i64,
                "diff_available": diff.is_some(),
            }),
            diff,
            None,
        )
    }
}

impl ToolHandler for EditHandler {
    fn name(&self) -> &str {
        "edit_file"
    }

    fn description(&self) -> &str {
        "Replace exact text in file."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Relative path to the file to edit."
                },
                "old_text": {
                    "type": "string",
                    "description": "Exact text to find and replace. Must appear in the file."
                },
                "new_text": {
                    "type": "string",
                    "description": "Replacement text."
                }
            },
            "required": ["path", "old_text", "new_text"]
        })
    }

    fn execute(&self, input: Value) -> Result<String> {
        Ok(self.run(input).output)
    }

    fn execute_v2(&self, input: Value) -> Result<ToolResult> {
        Ok(self.run(input))
    }
}
