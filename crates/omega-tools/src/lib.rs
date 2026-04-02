use std::collections::HashMap;
use std::time::Instant;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tracing::instrument;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolFamily {
    WorkspaceInspection,
    WebResearch,
    Editing,
    Planning,
    Interaction,
    EscapeHatch,
    KnowledgeAndGovernance,
    Other,
}

impl ToolFamily {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::WorkspaceInspection => "workspace_inspection",
            Self::WebResearch => "web_research",
            Self::Editing => "editing",
            Self::Planning => "planning",
            Self::Interaction => "interaction",
            Self::EscapeHatch => "escape_hatch",
            Self::KnowledgeAndGovernance => "knowledge_and_governance",
            Self::Other => "other",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolStability {
    Stable,
    Preview,
    Experimental,
}

impl ToolStability {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stable => "stable",
            Self::Preview => "preview",
            Self::Experimental => "experimental",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TruncationStrategy {
    Tail,
    Head,
    Middle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolOutputFormat {
    PlainText,
    Json,
    Diff,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryScopeLevel {
    None,
    Session,
    Project,
    Global,
}

impl MemoryScopeLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Session => "session",
            Self::Project => "project",
            Self::Global => "global",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolPromptProfile {
    pub summary: String,
    pub when_to_use: Vec<String>,
    pub when_not_to_use: Vec<String>,
    pub prefer_over: Vec<String>,
    pub fallback_to: Vec<String>,
    pub examples: Vec<String>,
    pub anti_patterns: Vec<String>,
}

impl ToolPromptProfile {
    pub fn from_summary(summary: impl Into<String>) -> Self {
        Self {
            summary: summary.into(),
            when_to_use: Vec::new(),
            when_not_to_use: Vec::new(),
            prefer_over: Vec::new(),
            fallback_to: Vec::new(),
            examples: Vec::new(),
            anti_patterns: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolIoProfile {
    pub max_output_bytes: usize,
    pub truncation_strategy: TruncationStrategy,
    pub output_format: ToolOutputFormat,
    pub normalize_input: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolUiProfile {
    pub invocation_preview: bool,
    pub result_preview: bool,
    pub detail_overlay: bool,
    pub action_affordances: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolContextProfile {
    pub needs_workspace_root: bool,
    pub needs_step_metadata: bool,
    pub needs_selection: bool,
    pub memory_scope: MemoryScopeLevel,
    pub network_context: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolPermissionProfile {
    pub permission_class: String,
    pub default_policy_mode: String,
    pub requires_approval: bool,
    pub denial_remediation: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolStorageProfile {
    pub writes_session_journal: bool,
    pub produces_artifact: bool,
    pub writes_memory: bool,
    pub writes_todo: bool,
    pub replayable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolObservabilityProfile {
    pub invocation_metric: String,
    pub success_metric: String,
    pub failure_metric: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolExecutionContext {
    pub workspace_root: String,
    pub workflow_id: String,
    pub workflow_role: String,
    pub step_id: String,
    pub step_label: String,
    pub turn_id: u64,
    pub current_item_id: Option<String>,
    pub current_item_index: Option<usize>,
    pub current_item_total: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolManifestMetadata {
    pub id: String,
    pub display_name: String,
    pub description: String,
    pub input_schema: Value,
    pub family: ToolFamily,
    pub stability: ToolStability,
    pub prompt: ToolPromptProfile,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub io: Option<ToolIoProfile>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ui: Option<ToolUiProfile>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<ToolContextProfile>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permissions: Option<ToolPermissionProfile>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub storage: Option<ToolStorageProfile>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observability: Option<ToolObservabilityProfile>,
}

impl ToolManifestMetadata {
    pub fn legacy(
        id: impl Into<String>,
        description: impl Into<String>,
        input_schema: Value,
    ) -> Self {
        let id = id.into();
        let description = description.into();
        Self {
            display_name: humanize_tool_name(&id),
            prompt: ToolPromptProfile::from_summary(description.clone()),
            id,
            description,
            input_schema,
            family: ToolFamily::Other,
            stability: ToolStability::Stable,
            io: None,
            ui: None,
            context: None,
            permissions: None,
            storage: None,
            observability: None,
        }
    }

    pub fn to_schema_value(&self) -> Value {
        json!({
            "name": self.id,
            "description": self.description,
            "input_schema": self.input_schema,
        })
    }

    pub fn with_family(mut self, family: ToolFamily) -> Self {
        self.family = family;
        self
    }

    pub fn with_stability(mut self, stability: ToolStability) -> Self {
        self.stability = stability;
        self
    }

    pub fn with_prompt_profile(mut self, prompt: ToolPromptProfile) -> Self {
        self.prompt = prompt;
        self
    }

    pub fn with_ui(mut self, ui: ToolUiProfile) -> Self {
        self.ui = Some(ui);
        self
    }

    pub fn with_context(mut self, context: ToolContextProfile) -> Self {
        self.context = Some(context);
        self
    }

    pub fn with_permissions(mut self, permissions: ToolPermissionProfile) -> Self {
        self.permissions = Some(permissions);
        self
    }

    pub fn with_storage(mut self, storage: ToolStorageProfile) -> Self {
        self.storage = Some(storage);
        self
    }

    pub fn with_observability(mut self, observability: ToolObservabilityProfile) -> Self {
        self.observability = Some(observability);
        self
    }
}

pub struct ToolManifest {
    pub id: String,
    pub display_name: String,
    pub family: ToolFamily,
    pub stability: ToolStability,
    pub prompt: ToolPromptProfile,
    pub io: Option<ToolIoProfile>,
    pub ui: Option<ToolUiProfile>,
    pub context: Option<ToolContextProfile>,
    pub permissions: Option<ToolPermissionProfile>,
    pub storage: Option<ToolStorageProfile>,
    pub observability: Option<ToolObservabilityProfile>,
    handler: Box<dyn ToolHandler>,
}

impl ToolManifest {
    pub fn new(
        id: impl Into<String>,
        display_name: impl Into<String>,
        family: ToolFamily,
        stability: ToolStability,
        prompt: ToolPromptProfile,
        handler: Box<dyn ToolHandler>,
    ) -> Self {
        Self {
            id: id.into(),
            display_name: display_name.into(),
            family,
            stability,
            prompt,
            io: None,
            ui: None,
            context: None,
            permissions: None,
            storage: None,
            observability: None,
            handler,
        }
    }

    pub fn legacy(handler: Box<dyn ToolHandler>) -> Self {
        let id = handler.name().to_string();
        let description = handler.description().to_string();
        Self::new(
            id.clone(),
            humanize_tool_name(&id),
            ToolFamily::Other,
            ToolStability::Stable,
            ToolPromptProfile::from_summary(description),
            handler,
        )
    }

    pub fn with_io(mut self, io: ToolIoProfile) -> Self {
        self.io = Some(io);
        self
    }

    pub fn with_ui(mut self, ui: ToolUiProfile) -> Self {
        self.ui = Some(ui);
        self
    }

    pub fn with_context(mut self, context: ToolContextProfile) -> Self {
        self.context = Some(context);
        self
    }

    pub fn with_permissions(mut self, permissions: ToolPermissionProfile) -> Self {
        self.permissions = Some(permissions);
        self
    }

    pub fn with_storage(mut self, storage: ToolStorageProfile) -> Self {
        self.storage = Some(storage);
        self
    }

    pub fn with_observability(mut self, observability: ToolObservabilityProfile) -> Self {
        self.observability = Some(observability);
        self
    }

    pub fn metadata(&self) -> ToolManifestMetadata {
        ToolManifestMetadata {
            id: self.id.clone(),
            display_name: self.display_name.clone(),
            description: self.handler.description().to_string(),
            input_schema: self.handler.input_schema(),
            family: self.family,
            stability: self.stability,
            prompt: self.prompt.clone(),
            io: self.io.clone(),
            ui: self.ui.clone(),
            context: self.context.clone(),
            permissions: self.permissions.clone(),
            storage: self.storage.clone(),
            observability: self.observability.clone(),
        }
    }

    fn handler(&self) -> &dyn ToolHandler {
        self.handler.as_ref()
    }
}

fn humanize_tool_name(name: &str) -> String {
    name.split('_')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => format!(
                    "{}{}",
                    first.to_ascii_uppercase(),
                    chars.as_str().to_ascii_lowercase()
                ),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolRemediationKind {
    ChooseKnownTool,
    AdjustInput,
    UseAllowedAlternative,
    RetryOrFallback,
    RetryNarrower,
}

impl ToolRemediationKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ChooseKnownTool => "choose_known_tool",
            Self::AdjustInput => "adjust_input",
            Self::UseAllowedAlternative => "use_allowed_alternative",
            Self::RetryOrFallback => "retry_or_fallback",
            Self::RetryNarrower => "retry_narrower",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolRemediation {
    pub kind: ToolRemediationKind,
    pub suggestion: String,
    pub alternative_tools: Vec<String>,
    pub recoverable: bool,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remediation: Option<ToolRemediation>,
}

impl ToolResult {
    pub fn success(output: impl Into<String>) -> Self {
        Self {
            output: output.into(),
            preview: None,
            metadata: json!({}),
            truncated: false,
            error_kind: None,
            remediation: None,
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

    pub fn with_remediation(mut self, remediation: ToolRemediation) -> Self {
        self.remediation = Some(remediation);
        self
    }

    pub fn is_error(&self) -> bool {
        self.error_kind.is_some() || self.output.starts_with("Error:")
    }

    pub fn has_metadata(&self) -> bool {
        !matches!(&self.metadata, Value::Object(map) if map.is_empty()) && !self.metadata.is_null()
    }

    pub fn as_content_value(&self) -> Value {
        if self.preview.is_none()
            && !self.has_metadata()
            && !self.truncated
            && self.error_kind.is_none()
            && self.remediation.is_none()
        {
            Value::String(self.output.clone())
        } else {
            serde_json::to_value(self).unwrap_or_else(|_| Value::String(self.output.clone()))
        }
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
    manifests: HashMap<String, ToolManifest>,
    aliases: HashMap<String, String>,
}

impl ToolDispatcher {
    pub fn new() -> Self {
        Self {
            manifests: HashMap::new(),
            aliases: HashMap::new(),
        }
    }

    /// Register a tool handler. Replaces any existing handler with the same name.
    pub fn register(&mut self, handler: Box<dyn ToolHandler>) {
        self.register_manifest(ToolManifest::legacy(handler));
    }

    /// Register a tool manifest. Replaces any existing tool with the same id.
    pub fn register_manifest(&mut self, manifest: ToolManifest) {
        self.manifests.insert(manifest.id.clone(), manifest);
    }

    /// Register a compatibility alias that resolves to an existing tool id at dispatch time.
    ///
    /// Aliases are intentionally hidden from exported schemas and manifest metadata so the
    /// model only sees the canonical tool surface.
    pub fn register_alias(
        &mut self,
        alias: impl Into<String>,
        target: impl Into<String>,
    ) -> Result<()> {
        let alias = alias.into();
        let target = target.into();

        if alias.trim().is_empty() {
            anyhow::bail!("tool alias must be non-empty");
        }
        if target.trim().is_empty() {
            anyhow::bail!("tool alias target must be non-empty");
        }
        if alias == target {
            anyhow::bail!("tool alias '{alias}' cannot target itself");
        }
        if self.manifests.contains_key(alias.as_str()) {
            anyhow::bail!("tool alias '{alias}' conflicts with a registered tool id");
        }
        if !self.manifests.contains_key(target.as_str()) {
            anyhow::bail!("tool alias '{alias}' targets unknown tool '{target}'");
        }

        self.aliases.insert(alias, target);
        Ok(())
    }

    fn resolve_name<'a>(&'a self, name: &'a str) -> &'a str {
        self.aliases.get(name).map(String::as_str).unwrap_or(name)
    }

    pub fn remediation_for(&self, name: &str, error_kind: ToolErrorKind) -> ToolRemediation {
        let resolved_name = self.resolve_name(name);
        let known_tools = self.tool_names();
        build_tool_remediation(
            resolved_name,
            self.manifests
                .get(resolved_name)
                .map(ToolManifest::metadata)
                .as_ref(),
            error_kind,
            &known_tools,
        )
    }

    pub fn error_result(
        &self,
        name: &str,
        output: impl Into<String>,
        error_kind: ToolErrorKind,
    ) -> ToolResult {
        self.with_dispatcher_defaults(name, ToolResult::error(output, error_kind))
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
        let resolved_name = self.resolve_name(name);
        let result = match self.manifests.get(resolved_name) {
            Some(manifest) => manifest
                .handler()
                .execute_v2(input)
                .map(|result| self.with_dispatcher_defaults(name, result)),
            None => Ok(self.error_result(
                name,
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

    fn with_dispatcher_defaults(&self, name: &str, mut result: ToolResult) -> ToolResult {
        if let Some(error_kind) = result.error_kind {
            if result.remediation.is_none() {
                result = result.with_remediation(self.remediation_for(name, error_kind));
            }
        }
        result
    }

    /// Generate the `tools` array expected by the Anthropic messages API.
    ///
    /// Each entry is `{ "name", "description", "input_schema" }` — the same
    /// shape as `omega_client::ToolDefinition` so callers can deserialize or
    /// pass directly as `Value`.
    pub fn to_schemas(&self) -> Vec<Value> {
        let mut schemas: Vec<_> = self
            .manifests
            .values()
            .map(ToolManifest::metadata)
            .map(|metadata| metadata.to_schema_value())
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
            .map(|name| self.resolve_name(name))
            .collect::<std::collections::HashSet<_>>();
        let mut schemas: Vec<_> = self
            .manifests
            .values()
            .filter(|manifest| name_set.contains(manifest.id.as_str()))
            .map(ToolManifest::metadata)
            .map(|metadata| metadata.to_schema_value())
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
        self.manifests.len()
    }

    /// Returns `true` if no handlers are registered.
    pub fn is_empty(&self) -> bool {
        self.manifests.is_empty()
    }

    /// Check whether a handler is registered for the given tool name.
    pub fn has_tool(&self, name: &str) -> bool {
        self.manifests.contains_key(name) || self.aliases.contains_key(name)
    }

    /// Returns a sorted list of registered tool names.
    pub fn tool_names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.manifests.keys().map(|k| k.as_str()).collect();
        names.sort();
        names
    }

    /// Return manifest metadata for all registered tools in deterministic order.
    pub fn manifest_metadata(&self) -> Vec<ToolManifestMetadata> {
        let mut manifests = self
            .manifests
            .values()
            .map(ToolManifest::metadata)
            .collect::<Vec<_>>();
        manifests.sort_by(|left, right| left.id.cmp(&right.id));
        manifests
    }

    /// Return manifest metadata for a single registered tool.
    pub fn manifest_for(&self, name: &str) -> Option<ToolManifestMetadata> {
        self.manifests
            .get(self.resolve_name(name))
            .map(ToolManifest::metadata)
    }
}

impl Default for ToolDispatcher {
    fn default() -> Self {
        Self::new()
    }
}

fn build_tool_remediation(
    name: &str,
    manifest: Option<&ToolManifestMetadata>,
    error_kind: ToolErrorKind,
    known_tools: &[&str],
) -> ToolRemediation {
    let alternative_tools = manifest
        .map(|manifest| manifest.prompt.fallback_to.clone())
        .filter(|tools| !tools.is_empty())
        .unwrap_or_else(|| {
            if error_kind == ToolErrorKind::UnknownTool {
                known_tools
                    .iter()
                    .copied()
                    .filter(|candidate| *candidate != name)
                    .take(5)
                    .map(str::to_string)
                    .collect()
            } else {
                Vec::new()
            }
        });

    let (kind, suggestion) = match error_kind {
        ToolErrorKind::UnknownTool => (
            ToolRemediationKind::ChooseKnownTool,
            "Re-check the requested tool name against the visible tool list, then call the closest matching known tool.",
        ),
        ToolErrorKind::Validation => (
            ToolRemediationKind::AdjustInput,
            "Adjust the tool input to satisfy the schema and retry the same tool with only the required, well-typed fields.",
        ),
        ToolErrorKind::Policy => (
            ToolRemediationKind::UseAllowedAlternative,
            manifest
                .and_then(|manifest| manifest.permissions.as_ref())
                .and_then(|permissions| permissions.denial_remediation.as_deref())
                .unwrap_or(
                    "Do not retry this tool in the current step. Switch to an allowed visible tool or continue without this action.",
                ),
        ),
        ToolErrorKind::Execution => (
            ToolRemediationKind::RetryOrFallback,
            "Retry with narrower scope or corrected inputs. If the same operation keeps failing, switch to a safer fallback tool.",
        ),
        ToolErrorKind::Timeout => (
            ToolRemediationKind::RetryNarrower,
            "Reduce the request scope or output size and retry. Prefer a narrower tool or a fallback that returns less data.",
        ),
    };

    ToolRemediation {
        kind,
        suggestion: suggestion.to_string(),
        alternative_tools,
        recoverable: true,
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
    fn to_schemas_filtered_resolves_aliases_to_canonical_schema() {
        let mut d = ToolDispatcher::new();
        d.register(Box::new(EchoTool));
        d.register_alias("echo_alias", "echo").unwrap();

        let defs: Vec<ToolDefinition> = d
            .to_schemas_filtered(&["echo_alias"])
            .into_iter()
            .map(|value| serde_json::from_value(value).unwrap())
            .collect();

        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].name, "echo");
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
    fn aliases_dispatch_but_do_not_expand_visible_schema_surface() {
        let mut d = ToolDispatcher::new();
        d.register(Box::new(EchoTool));
        d.register_alias("echo_alias", "echo").unwrap();

        assert!(d.has_tool("echo_alias"));
        assert_eq!(d.dispatch("echo_alias", json!({"text": "aliased"})).unwrap().output, "aliased");
        assert_eq!(d.to_schemas().len(), 1);
        assert_eq!(d.tool_names(), vec!["echo"]);
        assert_eq!(d.manifest_for("echo_alias").expect("alias manifest metadata").id, "echo");
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

    #[test]
    fn register_manifest_preserves_manifest_metadata() {
        let mut d = ToolDispatcher::new();
        d.register_manifest(ToolManifest::new(
            "echo",
            "Echo",
            ToolFamily::Interaction,
            ToolStability::Preview,
            ToolPromptProfile {
                summary: "Structured echo tool".to_string(),
                when_to_use: vec!["you need a simple round-trip test".to_string()],
                when_not_to_use: vec!["you need filesystem access".to_string()],
                prefer_over: vec!["bash".to_string()],
                fallback_to: vec!["bash".to_string()],
                examples: vec!["echo hello".to_string()],
                anti_patterns: vec!["using it as a file reader".to_string()],
            },
            Box::new(EchoTool),
        ));

        let manifest = d.manifest_for("echo").expect("manifest should exist");
        assert_eq!(manifest.id, "echo");
        assert_eq!(manifest.display_name, "Echo");
        assert_eq!(manifest.family, ToolFamily::Interaction);
        assert_eq!(manifest.stability, ToolStability::Preview);
        assert_eq!(manifest.prompt.summary, "Structured echo tool");
        assert_eq!(manifest.description, "Echoes input back.");
        assert_eq!(manifest.input_schema["type"], "object");
    }

    #[test]
    fn dispatch_adds_manifest_based_remediation_to_error_results() {
        struct FailingResultTool;

        impl ToolHandler for FailingResultTool {
            fn name(&self) -> &str {
                "failing_result"
            }

            fn description(&self) -> &str {
                "Returns a typed error result"
            }

            fn input_schema(&self) -> Value {
                json!({"type": "object"})
            }

            fn execute(&self, _input: Value) -> Result<String> {
                Ok("unused".to_string())
            }

            fn execute_v2(&self, _input: Value) -> Result<ToolResult> {
                Ok(ToolResult::error("failed", ToolErrorKind::Execution))
            }
        }

        let mut d = ToolDispatcher::new();
        d.register_manifest(ToolManifest::new(
            "failing_result",
            "Failing Result",
            ToolFamily::WorkspaceInspection,
            ToolStability::Stable,
            ToolPromptProfile {
                summary: "A tool that fails with a typed result".to_string(),
                when_to_use: vec!["never".to_string()],
                when_not_to_use: vec!["always".to_string()],
                prefer_over: vec![],
                fallback_to: vec!["read_file".to_string(), "grep_search".to_string()],
                examples: vec![],
                anti_patterns: vec![],
            },
            Box::new(FailingResultTool),
        ));

        let result = d.dispatch("failing_result", json!({})).unwrap();

        assert_eq!(result.error_kind, Some(ToolErrorKind::Execution));
        let remediation = result.remediation.expect("remediation should be attached");
        assert_eq!(remediation.kind, ToolRemediationKind::RetryOrFallback);
        assert_eq!(remediation.alternative_tools, vec!["read_file", "grep_search"]);
        assert!(remediation.recoverable);
    }

    #[test]
    fn legacy_register_wraps_handler_into_default_manifest() {
        let mut d = ToolDispatcher::new();
        d.register(Box::new(EchoTool));

        let manifest = d.manifest_for("echo").expect("manifest should exist");
        assert_eq!(manifest.display_name, "Echo");
        assert_eq!(manifest.family, ToolFamily::Other);
        assert_eq!(manifest.stability, ToolStability::Stable);
        assert_eq!(manifest.prompt.summary, "Echoes input back.");
    }
}
