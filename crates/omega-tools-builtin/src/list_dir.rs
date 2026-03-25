use std::fs;
use std::path::PathBuf;

use anyhow::Result;
use omega_tools::{ToolErrorKind, ToolHandler, ToolResult};
use serde_json::{json, Value};

use crate::path_safety::{resolve_file_root, safe_path_within_root};
use crate::shared::{
    build_tool_error, build_tool_result, parse_limit_field, render_lines_result, MAX_OUTPUT_CHARS,
};

const DEFAULT_LIST_DIR_LIMIT: usize = 200;

#[derive(Debug, Clone)]
pub struct ListDirHandler {
    root: PathBuf,
}

impl ListDirHandler {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root: resolve_file_root(root),
        }
    }

    fn run(&self, input: Value) -> ToolResult {
        let path_arg = input
            .get("path")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|path| !path.is_empty())
            .unwrap_or(".");
        let limit = match parse_limit_field(&input, "limit", DEFAULT_LIST_DIR_LIMIT) {
            Ok(limit) => limit,
            Err(message) => {
                return build_tool_error(
                    message,
                    json!({ "path": path_arg }),
                    ToolErrorKind::Validation,
                );
            }
        };

        let resolved = match safe_path_within_root(&self.root, path_arg) {
            Ok(path) => path,
            Err(message) => {
                return build_tool_error(
                    message,
                    json!({ "path": path_arg }),
                    ToolErrorKind::Policy,
                );
            }
        };

        let metadata = match fs::metadata(&resolved) {
            Ok(metadata) => metadata,
            Err(error) => {
                return build_tool_error(
                    format!("Error: {error}"),
                    json!({ "path": path_arg }),
                    ToolErrorKind::Execution,
                );
            }
        };
        if !metadata.is_dir() {
            return build_tool_error(
                format!("Error: Path '{path_arg}' is not a directory"),
                json!({ "path": path_arg }),
                ToolErrorKind::Validation,
            );
        }

        let mut entries = match fs::read_dir(&resolved) {
            Ok(entries) => match entries.collect::<std::result::Result<Vec<_>, _>>() {
                Ok(entries) => entries,
                Err(error) => {
                    return build_tool_error(
                        format!("Error: {error}"),
                        json!({ "path": path_arg }),
                        ToolErrorKind::Execution,
                    );
                }
            },
            Err(error) => {
                return build_tool_error(
                    format!("Error: {error}"),
                    json!({ "path": path_arg }),
                    ToolErrorKind::Execution,
                );
            }
        };
        entries.sort_by_key(|entry| entry.file_name());

        let mut directory_count = 0usize;
        let mut file_count = 0usize;
        let mut rendered = Vec::with_capacity(entries.len());
        for entry in entries {
            let mut name = entry.file_name().to_string_lossy().to_string();
            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(error) => {
                    return build_tool_error(
                        format!("Error: {error}"),
                        json!({ "path": path_arg }),
                        ToolErrorKind::Execution,
                    );
                }
            };
            if file_type.is_dir() {
                directory_count += 1;
                name.push('/');
            } else {
                file_count += 1;
            }
            rendered.push(name);
        }

        let total_entries = rendered.len();
        let omitted_count = total_entries.saturating_sub(limit);
        if total_entries > limit {
            rendered.truncate(limit);
        }
        let captured = render_lines_result(
            rendered,
            MAX_OUTPUT_CHARS,
            total_entries > limit,
            Some(omitted_count),
            "(empty directory)",
        );

        build_tool_result(
            captured.output,
            json!({
                "path": path_arg,
                "entry_count": total_entries,
                "returned_entry_count": total_entries.min(limit),
                "directory_count": directory_count,
                "file_count": file_count,
                "limit": limit,
            }),
            captured.truncated,
            None,
        )
    }
}

impl ToolHandler for ListDirHandler {
    fn name(&self) -> &str {
        "list_dir"
    }

    fn description(&self) -> &str {
        "List directory contents within the workspace."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Optional relative directory path within the workspace root. Defaults to '.'."
                },
                "limit": {
                    "type": "integer",
                    "description": "Optional maximum number of entries to return.",
                    "minimum": 1
                }
            }
        })
    }

    fn execute(&self, input: Value) -> Result<String> {
        Ok(self.run(input).output)
    }

    fn execute_v2(&self, input: Value) -> Result<ToolResult> {
        Ok(self.run(input))
    }
}
