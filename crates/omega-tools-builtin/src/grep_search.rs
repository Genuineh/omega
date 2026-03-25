use std::fs;
use std::path::PathBuf;

use anyhow::Result;
use glob::Pattern;
use omega_tools::{ToolErrorKind, ToolHandler, ToolResult};
use regex::RegexBuilder;
use serde_json::{json, Value};

use crate::path_safety::{resolve_file_root, safe_path_within_root};
use crate::shared::{
    build_tool_error, build_tool_result, collect_search_files, parse_limit_field, preview_text,
    render_lines_result, workspace_relative_path, MAX_OUTPUT_CHARS,
};

const DEFAULT_GREP_SEARCH_LIMIT: usize = 200;
const DEFAULT_GREP_LINE_PREVIEW_CHARS: usize = 160;

#[derive(Debug, Clone)]
pub struct GrepSearchHandler {
    root: PathBuf,
}

impl GrepSearchHandler {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root: resolve_file_root(root),
        }
    }

    fn run(&self, input: Value) -> ToolResult {
        let query = match input.get("query").and_then(Value::as_str).map(str::trim) {
            Some(query) if !query.is_empty() => query,
            _ => {
                return build_tool_error(
                    "Error: Missing required field 'query'".to_string(),
                    json!({}),
                    ToolErrorKind::Validation,
                );
            }
        };
        let path_arg = input
            .get("path")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|path| !path.is_empty())
            .unwrap_or(".");
        let limit = match parse_limit_field(&input, "limit", DEFAULT_GREP_SEARCH_LIMIT) {
            Ok(limit) => limit,
            Err(message) => {
                return build_tool_error(
                    message,
                    json!({ "query": query, "path": path_arg }),
                    ToolErrorKind::Validation,
                );
            }
        };
        let is_regex = input
            .get("is_regex")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let case_sensitive = input
            .get("case_sensitive")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let include_pattern = input
            .get("include_pattern")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|pattern| !pattern.is_empty());
        let include_pattern = match include_pattern.map(Pattern::new).transpose() {
            Ok(pattern) => pattern,
            Err(error) => {
                return build_tool_error(
                    format!("Error: Invalid include_pattern: {error}"),
                    json!({ "query": query, "path": path_arg }),
                    ToolErrorKind::Validation,
                );
            }
        };
        let regex = if is_regex {
            match RegexBuilder::new(query)
                .case_insensitive(!case_sensitive)
                .build()
            {
                Ok(regex) => Some(regex),
                Err(error) => {
                    return build_tool_error(
                        format!("Error: Invalid regex: {error}"),
                        json!({ "query": query, "path": path_arg }),
                        ToolErrorKind::Validation,
                    );
                }
            }
        } else {
            None
        };
        let query_lower = (!is_regex && !case_sensitive).then(|| query.to_lowercase());

        let resolved = match safe_path_within_root(&self.root, path_arg) {
            Ok(path) => path,
            Err(message) => {
                return build_tool_error(
                    message,
                    json!({ "query": query, "path": path_arg }),
                    ToolErrorKind::Policy,
                );
            }
        };

        let mut files = Vec::new();
        if let Err(error) = collect_search_files(&resolved, &mut files) {
            return build_tool_error(
                format!("Error: {error}"),
                json!({ "query": query, "path": path_arg }),
                ToolErrorKind::Execution,
            );
        }
        files.sort();

        let mut matches = Vec::new();
        let mut searched_file_count = 0usize;
        let mut skipped_non_utf8_files = 0usize;
        let mut found_more_matches = false;
        'files: for file in files {
            let relative_path = workspace_relative_path(&self.root, &file);
            if include_pattern
                .as_ref()
                .is_some_and(|pattern| !pattern.matches(&relative_path))
            {
                continue;
            }

            let bytes = match fs::read(&file) {
                Ok(bytes) => bytes,
                Err(_) => continue,
            };
            let text = match String::from_utf8(bytes) {
                Ok(text) => text,
                Err(_) => {
                    skipped_non_utf8_files += 1;
                    continue;
                }
            };
            searched_file_count += 1;

            for (index, line) in text.lines().enumerate() {
                let matched = if let Some(regex) = &regex {
                    regex.is_match(line)
                } else if case_sensitive {
                    line.contains(query)
                } else {
                    line.to_lowercase()
                        .contains(query_lower.as_deref().unwrap_or_default())
                };

                if matched {
                    if matches.len() == limit {
                        found_more_matches = true;
                        break 'files;
                    }
                    matches.push(format!(
                        "{}:{}:{}",
                        relative_path,
                        index + 1,
                        preview_text(line, DEFAULT_GREP_LINE_PREVIEW_CHARS)
                    ));
                }
            }
        }

        let returned_match_count = matches.len();
        let captured = render_lines_result(
            matches,
            MAX_OUTPUT_CHARS,
            found_more_matches,
            None,
            "(no matches)",
        );
        let mut metadata = json!({
            "query": query,
            "path": path_arg,
            "is_regex": is_regex,
            "case_sensitive": case_sensitive,
            "searched_file_count": searched_file_count,
            "returned_match_count": returned_match_count,
            "limit": limit,
            "skipped_non_utf8_files": skipped_non_utf8_files,
        });
        if let Some(pattern) = input.get("include_pattern").and_then(Value::as_str) {
            metadata["include_pattern"] = json!(pattern);
        }

        build_tool_result(captured.output, metadata, captured.truncated, None)
    }
}

impl ToolHandler for GrepSearchHandler {
    fn name(&self) -> &str {
        "grep_search"
    }

    fn description(&self) -> &str {
        "Search workspace file contents and return matching lines."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Text or regex pattern to search for."
                },
                "path": {
                    "type": "string",
                    "description": "Optional relative file or directory path to scope the search. Defaults to '.'."
                },
                "is_regex": {
                    "type": "boolean",
                    "description": "Interpret query as a regular expression when true."
                },
                "case_sensitive": {
                    "type": "boolean",
                    "description": "Use case-sensitive matching when true. Defaults to false."
                },
                "include_pattern": {
                    "type": "string",
                    "description": "Optional glob applied to relative file paths before searching, for example 'crates/**/*.rs'."
                },
                "limit": {
                    "type": "integer",
                    "description": "Optional maximum number of matching lines to return.",
                    "minimum": 1
                }
            },
            "required": ["query"]
        })
    }

    fn execute(&self, input: Value) -> Result<String> {
        Ok(self.run(input).output)
    }

    fn execute_v2(&self, input: Value) -> Result<ToolResult> {
        Ok(self.run(input))
    }
}
