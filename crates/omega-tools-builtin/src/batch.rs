use std::path::PathBuf;
use std::thread;

use anyhow::Result;
use omega_tools::{ToolErrorKind, ToolHandler, ToolResult};
use serde_json::{json, Value};

use crate::glob_search::GlobSearchHandler;
use crate::grep_search::GrepSearchHandler;
use crate::list_dir::ListDirHandler;
use crate::path_safety::resolve_file_root;
use crate::read_file::ReadHandler;
use crate::shared::{
    build_tool_error, preview_json_input, truncate_output_chars, MAX_OUTPUT_CHARS,
};

const MAX_BATCH_REQUESTS: usize = 8;
const BATCH_ALLOWED_TOOLS: &[&str] = &["glob_search", "grep_search", "list_dir", "read_file"];

pub fn default_batch_max_requests() -> usize {
    MAX_BATCH_REQUESTS
}

#[derive(Debug, Clone)]
pub struct BatchHandler {
    root: PathBuf,
    max_requests: usize,
}

impl BatchHandler {
    pub fn new(root: PathBuf) -> Self {
        Self::with_max_requests(root, MAX_BATCH_REQUESTS)
    }

    pub fn with_max_requests(root: PathBuf, max_requests: usize) -> Self {
        Self {
            root: resolve_file_root(root),
            max_requests: max_requests.max(1),
        }
    }

    fn run(&self, input: Value) -> ToolResult {
        let requests = match input.get("requests") {
            Some(Value::Array(requests)) if !requests.is_empty() => requests,
            Some(Value::Array(_)) => {
                return build_tool_error(
                    "Error: Field 'requests' must be a non-empty array".to_string(),
                    json!({}),
                    ToolErrorKind::Validation,
                );
            }
            Some(_) => {
                return build_tool_error(
                    "Error: Field 'requests' must be a non-empty array".to_string(),
                    json!({}),
                    ToolErrorKind::Validation,
                );
            }
            None => {
                return build_tool_error(
                    "Error: Missing required field 'requests'".to_string(),
                    json!({}),
                    ToolErrorKind::Validation,
                );
            }
        };

        if requests.len() > self.max_requests {
            return build_tool_error(
                format!(
                    "Error: Batch supports at most {} requests per call",
                    self.max_requests
                ),
                json!({
                    "request_count": requests.len(),
                    "max_request_count": self.max_requests,
                }),
                ToolErrorKind::Validation,
            );
        }

        let mut immediate_results = Vec::new();
        let mut jobs = Vec::new();

        for (index, request) in requests.iter().enumerate() {
            let request_id = request
                .get("id")
                .and_then(Value::as_str)
                .filter(|id| !id.trim().is_empty())
                .map(ToOwned::to_owned);

            let tool_name = match request.get("tool").and_then(Value::as_str) {
                Some(tool_name) if !tool_name.trim().is_empty() => tool_name.trim().to_string(),
                _ => {
                    immediate_results.push((
                        index,
                        request_id,
                        "<missing>".to_string(),
                        json!({}),
                        build_tool_error(
                            format!(
                                "Error: Batch request {} is missing required field 'tool'",
                                index + 1
                            ),
                            json!({ "request_index": index + 1 }),
                            ToolErrorKind::Validation,
                        ),
                    ));
                    continue;
                }
            };

            let tool_input = match request.get("input") {
                Some(Value::Object(_)) => {
                    request.get("input").cloned().unwrap_or_else(|| json!({}))
                }
                Some(_) => {
                    immediate_results.push((
                        index,
                        request_id,
                        tool_name,
                        json!({}),
                        build_tool_error(
                            format!(
                                "Error: Batch request {} field 'input' must be an object",
                                index + 1
                            ),
                            json!({ "request_index": index + 1 }),
                            ToolErrorKind::Validation,
                        ),
                    ));
                    continue;
                }
                None => {
                    immediate_results.push((
                        index,
                        request_id,
                        tool_name,
                        json!({}),
                        build_tool_error(
                            format!(
                                "Error: Batch request {} is missing required field 'input'",
                                index + 1
                            ),
                            json!({ "request_index": index + 1 }),
                            ToolErrorKind::Validation,
                        ),
                    ));
                    continue;
                }
            };

            let root = self.root.clone();
            let job_tool_name = tool_name.clone();
            let job_input = tool_input.clone();
            jobs.push(thread::spawn(move || {
                (
                    index,
                    request_id,
                    job_tool_name.clone(),
                    job_input.clone(),
                    execute_batch_readonly_request(root, &job_tool_name, job_input),
                )
            }));
        }

        let mut ordered_results = vec![None; requests.len()];
        for (index, request_id, tool_name, tool_input, tool_result) in immediate_results {
            ordered_results[index] = Some((request_id, tool_name, tool_input, tool_result));
        }

        for job in jobs {
            let (index, request_id, tool_name, tool_input, tool_result) = match job.join() {
                Ok(result) => result,
                Err(_) => {
                    return build_tool_error(
                        "Error: Batch worker panicked".to_string(),
                        json!({}),
                        ToolErrorKind::Execution,
                    );
                }
            };
            ordered_results[index] = Some((request_id, tool_name, tool_input, tool_result));
        }

        let mut success_count = 0usize;
        let mut failure_count = 0usize;
        let mut any_nested_truncated = false;
        let mut result_metadata = Vec::with_capacity(ordered_results.len());
        let mut output_sections = Vec::new();

        for (index, entry) in ordered_results.into_iter().enumerate() {
            let (request_id, tool_name, tool_input, tool_result) = entry.unwrap_or_else(|| {
                (
                    None,
                    "<missing>".to_string(),
                    json!({}),
                    build_tool_error(
                        "Error: Batch request was not executed".to_string(),
                        json!({ "request_index": index + 1 }),
                        ToolErrorKind::Execution,
                    ),
                )
            });

            let ok = !tool_result.is_error();
            if ok {
                success_count += 1;
            } else {
                failure_count += 1;
            }
            any_nested_truncated |= tool_result.truncated;

            output_sections.push(format!("=== [{}] {} ===", index + 1, tool_name));
            if let Some(request_id) = request_id.as_deref() {
                output_sections.push(format!("id: {request_id}"));
            }
            output_sections.push(format!(
                "request: {}",
                preview_batch_request(&tool_name, &tool_input)
            ));
            output_sections.push(format!("status: {}", if ok { "ok" } else { "error" }));
            if let Some(error_kind) = tool_result.error_kind {
                output_sections.push(format!("error_kind: {}", error_kind.as_str()));
            }
            output_sections.push(tool_result.output.clone());

            result_metadata.push(json!({
                "index": index + 1,
                "id": request_id,
                "tool": tool_name,
                "request_preview": preview_batch_request(&tool_name, &tool_input),
                "ok": ok,
                "error_kind": tool_result.error_kind,
                "truncated": tool_result.truncated,
                "preview": tool_result.preview,
                "metadata": tool_result.metadata,
            }));
        }

        let summary = format!(
            "Batch completed {} requests ({} succeeded, {} failed)",
            requests.len(),
            success_count,
            failure_count,
        );
        let mut output = Vec::with_capacity(output_sections.len() + 2);
        output.push(summary.clone());
        if !output_sections.is_empty() {
            output.push(String::new());
            output.extend(output_sections);
        }
        let captured = truncate_output_chars(output.join("\n"), MAX_OUTPUT_CHARS);

        ToolResult::success(captured.output)
            .with_preview(summary)
            .with_metadata(json!({
                "request_count": requests.len(),
                "success_count": success_count,
                "failure_count": failure_count,
                "allowed_tools": BATCH_ALLOWED_TOOLS,
                "results": result_metadata,
            }))
            .with_truncated(any_nested_truncated || captured.truncated)
    }
}

impl ToolHandler for BatchHandler {
    fn name(&self) -> &str {
        "batch"
    }

    fn description(&self) -> &str {
        "Run a small set of read-only workspace tools in parallel and aggregate their results."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "requests": {
                    "type": "array",
                    "description": format!(
                        "A small batch of read-only tool calls. Allowed tools: list_dir, glob_search, grep_search, read_file. Maximum requests per call: {}.",
                        self.max_requests
                    ),
                    "items": {
                        "type": "object",
                        "properties": {
                            "id": {
                                "type": "string",
                                "description": "Optional caller-provided identifier for this request."
                            },
                            "tool": {
                                "type": "string",
                                "description": "Read-only tool name to invoke."
                            },
                            "input": {
                                "type": "object",
                                "description": "Tool input payload for the selected read-only tool."
                            }
                        },
                        "required": ["tool", "input"]
                    },
                    "minItems": 1,
                    "maxItems": self.max_requests
                }
            },
            "required": ["requests"]
        })
    }

    fn execute(&self, input: Value) -> Result<String> {
        Ok(self.run(input).output)
    }

    fn execute_v2(&self, input: Value) -> Result<ToolResult> {
        Ok(self.run(input))
    }
}

fn preview_batch_request(tool: &str, input: &Value) -> String {
    input
        .get("path")
        .and_then(Value::as_str)
        .filter(|path| !path.trim().is_empty())
        .map(|path| format!("{tool} {path}"))
        .or_else(|| {
            input
                .get("query")
                .and_then(Value::as_str)
                .filter(|query| !query.trim().is_empty())
                .map(|query| format!("{tool} {}", crate::shared::preview_text(query, 60)))
        })
        .unwrap_or_else(|| format!("{tool} {}", preview_json_input(input, 80)))
}

fn execute_batch_readonly_request(root: PathBuf, tool: &str, input: Value) -> ToolResult {
    let execution = match tool {
        "list_dir" => ListDirHandler::new(root).execute_v2(input),
        "glob_search" => GlobSearchHandler::new(root).execute_v2(input),
        "grep_search" => GrepSearchHandler::new(root).execute_v2(input),
        "read_file" => ReadHandler::new(root).execute_v2(input),
        _ => Ok(build_tool_error(
            format!(
                "Error: Tool '{tool}' is not supported by batch. Allowed tools: {}",
                BATCH_ALLOWED_TOOLS.join(", ")
            ),
            json!({
                "tool": tool,
                "allowed_tools": BATCH_ALLOWED_TOOLS,
            }),
            ToolErrorKind::Validation,
        )),
    };

    execution.unwrap_or_else(|error| {
        build_tool_error(
            format!("Error: {error}"),
            json!({ "tool": tool }),
            ToolErrorKind::Execution,
        )
    })
}
