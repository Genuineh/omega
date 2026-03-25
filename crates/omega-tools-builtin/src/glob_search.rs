use std::path::PathBuf;

use anyhow::Result;
use glob::{glob_with, MatchOptions};
use omega_tools::{ToolErrorKind, ToolHandler, ToolResult};
use serde_json::{json, Value};

use crate::path_safety::{normalize_file_path, resolve_file_root};
use crate::shared::{
    build_glob_pattern, build_tool_error, build_tool_result, parse_limit_field,
    render_lines_result, workspace_relative_path, MAX_OUTPUT_CHARS,
};

const DEFAULT_GLOB_SEARCH_LIMIT: usize = 200;
const GLOB_MATCH_OPTIONS: MatchOptions = MatchOptions {
    case_sensitive: true,
    require_literal_separator: false,
    require_literal_leading_dot: false,
};

#[derive(Debug, Clone)]
pub struct GlobSearchHandler {
    root: PathBuf,
}

impl GlobSearchHandler {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root: resolve_file_root(root),
        }
    }

    fn run(&self, input: Value) -> ToolResult {
        let pattern = match input.get("pattern").and_then(Value::as_str).map(str::trim) {
            Some(pattern) if !pattern.is_empty() => pattern,
            _ => {
                return build_tool_error(
                    "Error: Missing required field 'pattern'".to_string(),
                    json!({}),
                    ToolErrorKind::Validation,
                );
            }
        };
        let limit = match parse_limit_field(&input, "limit", DEFAULT_GLOB_SEARCH_LIMIT) {
            Ok(limit) => limit,
            Err(message) => {
                return build_tool_error(
                    message,
                    json!({ "pattern": pattern }),
                    ToolErrorKind::Validation,
                );
            }
        };
        let absolute_pattern = match build_glob_pattern(&self.root, pattern) {
            Ok(pattern) => pattern,
            Err(message) => {
                return build_tool_error(
                    message,
                    json!({ "pattern": pattern }),
                    ToolErrorKind::Validation,
                );
            }
        };

        let mut matches = Vec::new();
        let paths = match glob_with(&absolute_pattern, GLOB_MATCH_OPTIONS) {
            Ok(paths) => paths,
            Err(error) => {
                return build_tool_error(
                    format!("Error: Invalid glob pattern: {error}"),
                    json!({ "pattern": pattern }),
                    ToolErrorKind::Validation,
                );
            }
        };
        for path in paths.flatten() {
            let normalized =
                std::fs::canonicalize(&path).unwrap_or_else(|_| normalize_file_path(&path));
            if normalized.starts_with(&self.root) {
                matches.push(workspace_relative_path(&self.root, &normalized));
            }
        }
        matches.sort();
        matches.dedup();

        let total_matches = matches.len();
        let omitted_count = total_matches.saturating_sub(limit);
        if total_matches > limit {
            matches.truncate(limit);
        }
        let captured = render_lines_result(
            matches,
            MAX_OUTPUT_CHARS,
            total_matches > limit,
            Some(omitted_count),
            "(no matches)",
        );

        build_tool_result(
            captured.output,
            json!({
                "pattern": pattern,
                "match_count": total_matches,
                "returned_match_count": total_matches.min(limit),
                "limit": limit,
            }),
            captured.truncated,
            None,
        )
    }
}

impl ToolHandler for GlobSearchHandler {
    fn name(&self) -> &str {
        "glob_search"
    }

    fn description(&self) -> &str {
        "Find workspace paths that match a glob pattern."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Glob pattern relative to the workspace root, for example 'crates/**/*.rs'."
                },
                "limit": {
                    "type": "integer",
                    "description": "Optional maximum number of matched paths to return.",
                    "minimum": 1
                }
            },
            "required": ["pattern"]
        })
    }

    fn execute(&self, input: Value) -> Result<String> {
        Ok(self.run(input).output)
    }

    fn execute_v2(&self, input: Value) -> Result<ToolResult> {
        Ok(self.run(input))
    }
}
