use std::fs;
use std::path::PathBuf;

use anyhow::Result;
use omega_tools::{ToolErrorKind, ToolHandler, ToolResult};
use serde_json::{json, Value};
use tracing::warn;

use crate::path_safety::{resolve_file_root, safe_path_within_root};
use crate::shared::{
    build_change_tool_result, build_tool_error, count_occurrences, line_count, preview_text,
    render_text_diff,
};

#[derive(Debug, Clone)]
struct PatchEdit {
    old_text: String,
    new_text: String,
    replace_all: bool,
}

#[derive(Debug, Clone)]
pub struct ApplyPatchHandler {
    root: PathBuf,
}

impl ApplyPatchHandler {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root: resolve_file_root(root),
        }
    }

    fn parse_edits(
        &self,
        input: &Value,
        path_arg: &str,
    ) -> std::result::Result<Vec<PatchEdit>, ToolResult> {
        let edits = match input.get("edits") {
            Some(Value::Array(edits)) if !edits.is_empty() => edits,
            Some(Value::Array(_)) | Some(_) => {
                return Err(build_tool_error(
                    "Error: Field 'edits' must be a non-empty array".to_string(),
                    json!({ "path": path_arg }),
                    ToolErrorKind::Validation,
                ));
            }
            None => {
                return Err(build_tool_error(
                    "Error: Missing required field 'edits'".to_string(),
                    json!({ "path": path_arg }),
                    ToolErrorKind::Validation,
                ));
            }
        };

        let mut parsed = Vec::with_capacity(edits.len());
        for (index, edit) in edits.iter().enumerate() {
            let old_text = match edit.get("old_text").and_then(Value::as_str) {
                Some(text) if !text.is_empty() => text,
                Some(_) => {
                    return Err(build_tool_error(
                        format!(
                            "Error: Edit {} field 'old_text' must not be empty",
                            index + 1
                        ),
                        json!({ "path": path_arg, "edit_index": index + 1 }),
                        ToolErrorKind::Validation,
                    ));
                }
                None => {
                    return Err(build_tool_error(
                        format!(
                            "Error: Edit {} is missing required field 'old_text'",
                            index + 1
                        ),
                        json!({ "path": path_arg, "edit_index": index + 1 }),
                        ToolErrorKind::Validation,
                    ));
                }
            };
            let new_text = match edit.get("new_text").and_then(Value::as_str) {
                Some(text) => text,
                None => {
                    return Err(build_tool_error(
                        format!(
                            "Error: Edit {} is missing required field 'new_text'",
                            index + 1
                        ),
                        json!({ "path": path_arg, "edit_index": index + 1 }),
                        ToolErrorKind::Validation,
                    ));
                }
            };
            let replace_all = edit
                .get("replace_all")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            parsed.push(PatchEdit {
                old_text: old_text.to_string(),
                new_text: new_text.to_string(),
                replace_all,
            });
        }

        Ok(parsed)
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

        let edits = match self.parse_edits(&input, path_arg) {
            Ok(edits) => edits,
            Err(error) => return error,
        };

        let resolved = match safe_path_within_root(&self.root, path_arg) {
            Ok(path) => path,
            Err(message) => {
                warn!(apply_patch.blocked_reason = %message, apply_patch.path = %path_arg);
                return build_tool_error(
                    message,
                    json!({ "path": path_arg }),
                    ToolErrorKind::Policy,
                );
            }
        };

        let original_content = match fs::read_to_string(&resolved) {
            Ok(content) => content,
            Err(error) => {
                warn!(apply_patch.error = %error, apply_patch.path = %path_arg);
                return build_tool_error(
                    format!("Error: {error}"),
                    json!({ "path": path_arg }),
                    ToolErrorKind::Execution,
                );
            }
        };

        let mut updated_content = original_content.clone();
        let mut total_replacements = 0usize;
        let mut replace_all_edits = 0usize;

        for (index, edit) in edits.iter().enumerate() {
            let occurrence_count = count_occurrences(&updated_content, &edit.old_text);
            if occurrence_count == 0 {
                return build_tool_error(
                    format!(
                        "Error: Edit {} text not found in {path_arg}. Read the file again and provide a more specific exact snippet.",
                        index + 1
                    ),
                    json!({
                        "path": path_arg,
                        "edit_index": index + 1,
                        "match_count": 0,
                        "old_text_preview": preview_text(&edit.old_text, 80),
                    }),
                    ToolErrorKind::Validation,
                );
            }
            if !edit.replace_all && occurrence_count > 1 {
                return build_tool_error(
                    format!(
                        "Error: Edit {} matched {} locations in {path_arg}. Use a more specific old_text or set replace_all.",
                        index + 1,
                        occurrence_count
                    ),
                    json!({
                        "path": path_arg,
                        "edit_index": index + 1,
                        "match_count": occurrence_count,
                        "replace_all": false,
                        "old_text_preview": preview_text(&edit.old_text, 80),
                    }),
                    ToolErrorKind::Validation,
                );
            }

            if edit.replace_all {
                updated_content = updated_content.replace(&edit.old_text, &edit.new_text);
                total_replacements += occurrence_count;
                replace_all_edits += 1;
            } else {
                updated_content = updated_content.replacen(&edit.old_text, &edit.new_text, 1);
                total_replacements += 1;
            }
        }

        if updated_content == original_content {
            return build_tool_error(
                format!("Error: Patch did not change {path_arg}"),
                json!({
                    "path": path_arg,
                    "edit_count": edits.len(),
                }),
                ToolErrorKind::Validation,
            );
        }

        if let Err(error) = fs::write(&resolved, &updated_content) {
            warn!(apply_patch.error = %error, apply_patch.path = %path_arg);
            return build_tool_error(
                format!("Error: {error}"),
                json!({ "path": path_arg }),
                ToolErrorKind::Execution,
            );
        }

        let diff = render_text_diff(path_arg, &original_content, &updated_content);
        let lines_before = line_count(&original_content);
        let lines_after = line_count(&updated_content);
        build_change_tool_result(
            format!(
                "Applied patch to {path_arg} ({} edit{}, {} replacement{})",
                edits.len(),
                if edits.len() == 1 { "" } else { "s" },
                total_replacements,
                if total_replacements == 1 { "" } else { "s" },
            ),
            json!({
                "path": path_arg,
                "edit_count": edits.len(),
                "replace_all_edits": replace_all_edits,
                "replacements": total_replacements,
                "bytes_before": original_content.len(),
                "bytes_after": updated_content.len(),
                "bytes_delta": updated_content.len() as i64 - original_content.len() as i64,
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

impl ToolHandler for ApplyPatchHandler {
    fn name(&self) -> &str {
        "apply_patch"
    }

    fn description(&self) -> &str {
        "Apply one or more exact text replacements to a file atomically."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Relative path to the file to patch."
                },
                "edits": {
                    "type": "array",
                    "description": "Ordered exact-text replacements to apply atomically.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "old_text": {
                                "type": "string",
                                "description": "Exact existing text to replace. Must match exactly once unless replace_all is true."
                            },
                            "new_text": {
                                "type": "string",
                                "description": "Replacement text."
                            },
                            "replace_all": {
                                "type": "boolean",
                                "description": "Replace every exact match for this edit when true. Defaults to false."
                            }
                        },
                        "required": ["old_text", "new_text"]
                    },
                    "minItems": 1
                }
            },
            "required": ["path", "edits"]
        })
    }

    fn execute(&self, input: Value) -> Result<String> {
        Ok(self.run(input).output)
    }

    fn execute_v2(&self, input: Value) -> Result<ToolResult> {
        Ok(self.run(input))
    }
}
