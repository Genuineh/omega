use std::collections::HashMap;
use std::time::Instant;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tracing::instrument;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolErrorKind {
    UnknownTool,
    Validation,
    Policy,
    Execution,
    Timeout,
}

impl ToolErrorKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::UnknownTool => "unknown_tool",
            Self::Validation => "validation",
            Self::Policy => "policy",
            Self::Execution => "execution",
            Self::Timeout => "timeout",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolResult {
    pub output: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
    pub metadata: Value,
    pub truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_kind: Option<ToolErrorKind>,
}

impl ToolResult {
    pub fn success(output: impl Into<String>) -> Self {
        Self {
            output: output.into(),
            preview: None,
            metadata: json!({}),
            truncated: false,
            error_kind: None,
        }
    }

    pub fn error(output: impl Into<String>, error_kind: ToolErrorKind) -> Self {
        Self::success(output).with_error_kind(error_kind)
    }

    pub fn with_preview(mut self, preview: impl Into<String>) -> Self {
        self.preview = Some(preview.into());
        self
    }

    pub fn with_optional_preview(mut self, preview: Option<String>) -> Self {
        self.preview = preview;
        self
    }

    pub fn with_metadata(mut self, metadata: Value) -> Self {
        self.metadata = metadata;
        self
    }

    pub fn with_truncated(mut self, truncated: bool) -> Self {
        self.truncated = truncated;
        self
    }

    pub fn with_error_kind(mut self, error_kind: ToolErrorKind) -> Self {
        self.error_kind = Some(error_kind);
        self
    }

    pub fn is_error(&self) -> bool {
        self.error_kind.is_some() || self.output.starts_with("Error:")
    }

    pub fn has_metadata(&self) -> bool {
        !matches!(&self.metadata, Value::Object(map) if map.is_empty()) && !self.metadata.is_null()
    }
}

impl From<String> for ToolResult {
    fn from(output: String) -> Self {
        Self::success(output)
    }
}

impl From<&str> for ToolResult {
    fn from(output: &str) -> Self {
        Self::success(output)
    }
}

/// A tool that the LLM can invoke during the agent loop.
///
/// Each handler declares its name, description, and JSON Schema for input,
/// then executes when the dispatcher routes a `tool_use` block to it.
pub trait ToolHandler: Send + Sync {
    /// Unique tool name as seen by the model (e.g. `"bash"`, `"read_file"`).
    fn name(&self) -> &str;

    /// Short human-readable description shown to the model.
    fn description(&self) -> &str;

    /// JSON Schema object describing the tool's input parameters.
    fn input_schema(&self) -> Value;

    /// Execute the tool with the given input and return a text result.
    fn execute(&self, input: Value) -> Result<String>;

    /// Execute the tool with the typed Tool Contract V2 result.
    ///
    /// Existing handlers can keep implementing `execute`; the default adapter
    /// wraps the legacy string output into a `ToolResult`.
    fn execute_v2(&self, input: Value) -> Result<ToolResult> {
        self.execute(input).map(ToolResult::from)
    }
}

/// Routes `tool_use` calls to the correct [`ToolHandler`] by name.
pub struct ToolDispatcher {
    handlers: HashMap<String, Box<dyn ToolHandler>>,
}

impl ToolDispatcher {
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
        }
    }

    /// Register a tool handler. Replaces any existing handler with the same name.
    pub fn register(&mut self, handler: Box<dyn ToolHandler>) {
        self.handlers.insert(handler.name().to_string(), handler);
    }

    /// Dispatch a tool call by name, returning the tool's text output.
    ///
    /// Returns an error string (not a Rust error) for unknown tools so that
    /// the model receives feedback and can self-correct, matching the Python
    /// reference behaviour: `f"Unknown tool: {name}"`.
    #[instrument(
        skip(self, input),
        fields(
            tool_exec.tool_name = %name,
            tool_exec.duration_ms,
            tool_exec.success,
            tool_exec.error_kind,
            tool_exec.truncated
        )
    )]
    pub fn dispatch(&self, name: &str, input: Value) -> Result<ToolResult> {
        let start = Instant::now();
        let result = match self.handlers.get(name) {
            Some(handler) => handler.execute_v2(input),
            None => Ok(ToolResult::error(
                format!("Unknown tool: {name}"),
                ToolErrorKind::UnknownTool,
            )),
        };
        let duration_ms = start.elapsed().as_millis() as u64;

        let success = result
            .as_ref()
            .is_ok_and(|tool_result| !tool_result.is_error());
        tracing::Span::current().record("tool_exec.duration_ms", duration_ms);
        tracing::Span::current().record("tool_exec.success", success);
        tracing::Span::current().record(
            "tool_exec.error_kind",
            result
                .as_ref()
                .ok()
                .and_then(|tool_result| tool_result.error_kind)
                .map(ToolErrorKind::as_str)
                .unwrap_or(""),
        );
        tracing::Span::current().record(
            "tool_exec.truncated",
            result
                .as_ref()
                .is_ok_and(|tool_result| tool_result.truncated),
        );

        result
    }

    /// Dispatch a tool call and return only the legacy text output.
    pub fn dispatch_text(&self, name: &str, input: Value) -> Result<String> {
        self.dispatch(name, input).map(|result| result.output)
    }

    /// Generate the `tools` array expected by the Anthropic messages API.
    ///
    /// Each entry is `{ "name", "description", "input_schema" }` — the same
    /// shape as `omega_client::ToolDefinition` so callers can deserialize or
    /// pass directly as `Value`.
    pub fn to_schemas(&self) -> Vec<Value> {
        let mut schemas: Vec<_> = self
            .handlers
            .values()
            .map(|h| {
                json!({
                    "name": h.name(),
                    "description": h.description(),
                    "input_schema": h.input_schema(),
                })
            })
            .collect();
        // Deterministic order for tests and prompt stability.
        schemas.sort_by(|a, b| {
            a["name"]
                .as_str()
                .unwrap_or("")
                .cmp(b["name"].as_str().unwrap_or(""))
        });
        schemas
    }

    /// Generate the `tools` array for a subset of tool names.
    ///
    /// Unknown names are ignored. The output remains sorted by tool name for
    /// deterministic prompts and tests.
    pub fn to_schemas_filtered(&self, names: &[&str]) -> Vec<Value> {
        let name_set = names
            .iter()
            .copied()
            .collect::<std::collections::HashSet<_>>();
        let mut schemas: Vec<_> = self
            .handlers
            .values()
            .filter(|handler| name_set.contains(handler.name()))
            .map(|handler| {
                json!({
                    "name": handler.name(),
                    "description": handler.description(),
                    "input_schema": handler.input_schema(),
                })
            })
            .collect();
        schemas.sort_by(|a, b| {
            a["name"]
                .as_str()
                .unwrap_or("")
                .cmp(b["name"].as_str().unwrap_or(""))
        });
        schemas
    }

    /// Returns the number of registered tool handlers.
    pub fn len(&self) -> usize {
        self.handlers.len()
    }

    /// Returns `true` if no handlers are registered.
    pub fn is_empty(&self) -> bool {
        self.handlers.is_empty()
    }

    /// Check whether a handler is registered for the given tool name.
    pub fn has_tool(&self, name: &str) -> bool {
        self.handlers.contains_key(name)
    }

    /// Returns a sorted list of registered tool names.
    pub fn tool_names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.handlers.keys().map(|k| k.as_str()).collect();
        names.sort();
        names
    }
}

impl Default for ToolDispatcher {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use omega_client::ToolDefinition;

    // ── Test helper: a trivial echo tool ──────────────────────────────

    struct EchoTool;

    impl ToolHandler for EchoTool {
        fn name(&self) -> &str {
            "echo"
        }

        fn description(&self) -> &str {
            "Echoes input back."
        }

        fn input_schema(&self) -> Value {
            json!({
                "type": "object",
                "properties": {
                    "text": { "type": "string" }
                },
                "required": ["text"]
            })
        }

        fn execute(&self, input: Value) -> Result<String> {
            let text = input["text"].as_str().unwrap_or("(no text)").to_string();
            Ok(text)
        }
    }

    struct FailTool;

    impl ToolHandler for FailTool {
        fn name(&self) -> &str {
            "fail"
        }

        fn description(&self) -> &str {
            "Always fails."
        }

        fn input_schema(&self) -> Value {
            json!({"type": "object"})
        }

        fn execute(&self, _input: Value) -> Result<String> {
            anyhow::bail!("intentional failure")
        }
    }

    struct SecondTool;

    impl ToolHandler for SecondTool {
        fn name(&self) -> &str {
            "second"
        }

        fn description(&self) -> &str {
            "Second tool"
        }

        fn input_schema(&self) -> Value {
            json!({"type": "object"})
        }

        fn execute(&self, _input: Value) -> Result<String> {
            Ok("second".to_string())
        }
    }

    // ── Dispatcher tests ──────────────────────────────────────────────

    #[test]
    fn new_dispatcher_is_empty() {
        let d = ToolDispatcher::new();
        assert!(d.is_empty());
        assert_eq!(d.len(), 0);
    }

    #[test]
    fn default_dispatcher_is_empty() {
        let d = ToolDispatcher::default();
        assert!(d.is_empty());
    }

    #[test]
    fn register_and_dispatch() {
        let mut d = ToolDispatcher::new();
        d.register(Box::new(EchoTool));

        assert_eq!(d.len(), 1);
        assert!(!d.is_empty());
        assert!(d.has_tool("echo"));
        assert!(!d.has_tool("bash"));

        let result = d.dispatch("echo", json!({"text": "hello"})).unwrap();
        assert_eq!(result.output, "hello");
        assert!(!result.is_error());
    }

    #[test]
    fn dispatch_unknown_tool_returns_error_string() {
        let d = ToolDispatcher::new();
        let result = d.dispatch("nonexistent", json!({})).unwrap();
        assert_eq!(result.output, "Unknown tool: nonexistent");
        assert_eq!(result.error_kind, Some(ToolErrorKind::UnknownTool));
    }

    #[test]
    fn dispatch_failing_tool_propagates_error() {
        let mut d = ToolDispatcher::new();
        d.register(Box::new(FailTool));

        let result = d.dispatch("fail", json!({}));
        assert!(result.is_err());
        assert!(
            result.unwrap_err().to_string().contains("intentional"),
            "should propagate tool error"
        );
    }

    #[test]
    fn to_schemas_produces_valid_tool_definitions() {
        let mut d = ToolDispatcher::new();
        d.register(Box::new(EchoTool));

        let schemas = d.to_schemas();
        assert_eq!(schemas.len(), 1);

        let schema = &schemas[0];
        assert_eq!(schema["name"], "echo");
        assert_eq!(schema["description"], "Echoes input back.");
        assert_eq!(schema["input_schema"]["type"], "object");
        assert!(schema["input_schema"]["properties"]["text"].is_object());
    }

    #[test]
    fn to_schemas_deserialize_into_omega_client_tool_definition() {
        let mut d = ToolDispatcher::new();
        d.register(Box::new(EchoTool));

        let schemas = d.to_schemas();
        let tool: ToolDefinition = serde_json::from_value(schemas[0].clone())
            .expect("schema should deserialize into omega_client::ToolDefinition");

        assert_eq!(tool.name, "echo");
        assert_eq!(tool.description, "Echoes input back.");
        assert_eq!(tool.input_schema["type"], "object");
        assert_eq!(tool.input_schema["required"][0], "text");
    }

    #[test]
    fn to_schemas_sorted_by_name() {
        let mut d = ToolDispatcher::new();
        d.register(Box::new(FailTool)); // "fail"
        d.register(Box::new(EchoTool)); // "echo"

        let schemas = d.to_schemas();
        assert_eq!(schemas[0]["name"], "echo");
        assert_eq!(schemas[1]["name"], "fail");
    }

    #[test]
    fn to_schemas_filtered_returns_sorted_subset() {
        let mut d = ToolDispatcher::new();
        d.register(Box::new(EchoTool));
        d.register(Box::new(SecondTool));

        let schemas = d.to_schemas_filtered(&["second", "echo", "missing"]);
        let defs: Vec<ToolDefinition> = schemas
            .into_iter()
            .map(|value| serde_json::from_value(value).unwrap())
            .collect();

        assert_eq!(defs.len(), 2);
        assert_eq!(defs[0].name, "echo");
        assert_eq!(defs[1].name, "second");
    }

    #[test]
    fn register_replaces_existing_handler() {
        struct EchoV2;
        impl ToolHandler for EchoV2 {
            fn name(&self) -> &str {
                "echo"
            }
            fn description(&self) -> &str {
                "Echo v2"
            }
            fn input_schema(&self) -> Value {
                json!({"type": "object"})
            }
            fn execute(&self, _input: Value) -> Result<String> {
                Ok("v2".into())
            }
        }

        let mut d = ToolDispatcher::new();
        d.register(Box::new(EchoTool));
        d.register(Box::new(EchoV2));

        assert_eq!(d.len(), 1, "should still have 1 handler");
        let result = d.dispatch("echo", json!({})).unwrap();
        assert_eq!(result.output, "v2");
    }

    #[test]
    fn tool_names_returns_sorted_list() {
        let mut d = ToolDispatcher::new();
        d.register(Box::new(FailTool));
        d.register(Box::new(EchoTool));

        assert_eq!(d.tool_names(), vec!["echo", "fail"]);
    }

    #[test]
    fn dispatch_with_missing_optional_field() {
        let mut d = ToolDispatcher::new();
        d.register(Box::new(EchoTool));

        // input without "text" field — handler returns "(no text)"
        let result = d.dispatch("echo", json!({})).unwrap();
        assert_eq!(result.output, "(no text)");
    }

    #[test]
    fn multiple_tools_dispatch_correctly() {
        let mut d = ToolDispatcher::new();
        d.register(Box::new(EchoTool));
        d.register(Box::new(FailTool));

        assert_eq!(d.len(), 2);
        assert_eq!(
            d.dispatch("echo", json!({"text": "ok"})).unwrap().output,
            "ok"
        );
        assert!(d.dispatch("fail", json!({})).is_err());
    }

    #[test]
    fn legacy_execute_is_wrapped_into_tool_result_v2() {
        let mut d = ToolDispatcher::new();
        d.register(Box::new(EchoTool));

        let result = d.dispatch("echo", json!({"text": "wrapped"})).unwrap();

        assert_eq!(result, ToolResult::success("wrapped"));
    }

    #[test]
    fn dispatch_text_preserves_legacy_output_shape() {
        let mut d = ToolDispatcher::new();
        d.register(Box::new(EchoTool));

        let result = d.dispatch_text("echo", json!({"text": "plain"})).unwrap();

        assert_eq!(result, "plain");
    }

    #[test]
    fn dispatcher_uses_execute_v2_when_handler_overrides_it() {
        struct PreviewTool;

        impl ToolHandler for PreviewTool {
            fn name(&self) -> &str {
                "preview"
            }

            fn description(&self) -> &str {
                "Preview-aware tool"
            }

            fn input_schema(&self) -> Value {
                json!({"type": "object"})
            }

            fn execute(&self, _input: Value) -> Result<String> {
                Ok("legacy".to_string())
            }

            fn execute_v2(&self, _input: Value) -> Result<ToolResult> {
                Ok(ToolResult::success("typed")
                    .with_preview("typed preview")
                    .with_metadata(json!({"source": "v2"})))
            }
        }

        let mut d = ToolDispatcher::new();
        d.register(Box::new(PreviewTool));

        let result = d.dispatch("preview", json!({})).unwrap();

        assert_eq!(result.output, "typed");
        assert_eq!(result.preview.as_deref(), Some("typed preview"));
        assert_eq!(result.metadata["source"], "v2");
    }
}
