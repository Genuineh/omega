use crate::config::RunConfig;
use crate::manifest::BenchmarkCase;
use crate::result::ToolTraceEntry;

use std::sync::Arc;
use std::time::Duration;

use omega_core::{DynLlmClient, MinimaxClient, MinimaxConfig};
use omega_session::{
    AgentSession, AgentSessionConfig, ConversationMessage, RuntimeEnvelopeRecorder, RuntimeMessage,
    ToolRunStatus,
};
use omega_workflow::LoadedWorkflowCatalog;

/// Execution output from a benchmark target.
pub struct TargetOutput {
    pub response: Option<String>,
    pub tool_trace: Vec<ToolTraceEntry>,
    pub delivery_summary: Option<serde_json::Value>,
    pub total_tokens: u64,
    pub latency_ms: u64,
}

/// Frontend-neutral execution boundary for benchmark evaluation.
///
/// A `BenchmarkTarget` takes a case prompt plus config and returns
/// structured output without depending on TUI or any specific UI surface.
pub trait BenchmarkTarget {
    /// Execute a single benchmark case and collect results.
    fn execute(&self, case: &BenchmarkCase, config: &RunConfig) -> anyhow::Result<TargetOutput>;
}

/// Stub target for development and offline scoring.
///
/// Returns empty output so suites can run scorers against pre-recorded
/// results or fixture data without requiring a live Omega runtime.
pub struct StubTarget;

impl BenchmarkTarget for StubTarget {
    fn execute(&self, _case: &BenchmarkCase, _config: &RunConfig) -> anyhow::Result<TargetOutput> {
        Ok(TargetOutput {
            response: None,
            tool_trace: Vec::new(),
            delivery_summary: None,
            total_tokens: 0,
            latency_ms: 0,
        })
    }
}

/// Live target that runs each case through a real `AgentSession`.
///
/// Creates a fresh session per case to avoid cross-case state pollution.
/// Uses `RuntimeEnvelopeRecorder` to capture messages without requiring
/// a TUI, and extracts tool trace + response from the recorded envelopes.
pub struct OmegaTarget {
    client: DynLlmClient,
    cwd: std::path::PathBuf,
}

impl OmegaTarget {
    /// Build a live target from environment variables.
    ///
    /// Reads API key, model, and base URL from the standard `OMEGA_*` /
    /// `ANTHROPIC_*` env vars (same as `omega-app`).
    pub fn from_env(cwd: std::path::PathBuf) -> anyhow::Result<Self> {
        let config = MinimaxConfig::from_env().map_err(|e| anyhow::anyhow!("{e}"))?;
        let client: DynLlmClient =
            Arc::new(MinimaxClient::new(config).map_err(|e| anyhow::anyhow!("{e}"))?);
        Ok(Self { client, cwd })
    }

    /// Build a live target with a pre-configured client (for testing).
    pub fn new(client: DynLlmClient, cwd: std::path::PathBuf) -> Self {
        Self { client, cwd }
    }
}

impl BenchmarkTarget for OmegaTarget {
    fn execute(&self, case: &BenchmarkCase, config: &RunConfig) -> anyhow::Result<TargetOutput> {
        let start = std::time::Instant::now();

        let runtime = tokio::runtime::Runtime::new()?;
        let loaded_catalog = LoadedWorkflowCatalog::load(&self.cwd);

        let session = AgentSession::new(AgentSessionConfig {
            client: self.client.clone(),
            system: format!(
                "You are a coding agent at {}. Act, don't explain.",
                self.cwd.display()
            ),
            cwd: self.cwd.clone(),
            runtime_handle: runtime.handle().clone(),
            scene_catalog: loaded_catalog.scene_catalog,
            workflow_catalog: loaded_catalog.workflow_catalog,
            prompt_catalog: loaded_catalog.prompt_catalog,
            context_window: 200_000,
            max_output_tokens: 32_000,
            bash_allowed_commands: omega_core::default_bash_allowed_commands(),
            batch_max_requests: omega_core::default_batch_max_requests(),
        })?;

        let recorder = RuntimeEnvelopeRecorder::new();
        let turn_id = 1u64;
        session.spawn_turn_with_test_bridge(
            case.prompt.clone(),
            turn_id,
            recorder.runtime_bridge(),
        )?;

        let timeout =
            Duration::from_secs(config.effective_timeout(case.timeout_secs).unwrap_or(120));

        let messages =
            std::panic::catch_unwind(|| recorder.wait_for_turn_finished_messages(turn_id, timeout));

        let messages = match messages {
            Ok(msgs) => msgs,
            Err(_) => {
                // Timeout — recorder panics on deadline expiry
                let partial = recorder.runtime_messages();
                let tool_trace = extract_tool_trace(&partial);
                return Ok(TargetOutput {
                    response: None,
                    tool_trace,
                    delivery_summary: None,
                    total_tokens: 0,
                    latency_ms: start.elapsed().as_millis() as u64,
                });
            }
        };

        let response = extract_response_text(&messages);
        let tool_trace = extract_tool_trace(&messages);

        Ok(TargetOutput {
            response,
            tool_trace,
            delivery_summary: None,
            total_tokens: 0,
            latency_ms: start.elapsed().as_millis() as u64,
        })
    }
}

/// Scripted target for deterministic CI testing.
///
/// Runs each case through a real `AgentSession` run loop but replaces the
/// LLM client with `ScriptedLlmClient`, so execution is repeatable and
/// network-free. Fixture files in `suites/<suite>/scripted/` provide the
/// scripted responses indexed by `case_id`.
pub struct ScriptedTarget {
    /// Per-case scripted client factories, keyed by case_id.
    scripts: std::collections::BTreeMap<String, Vec<omega_client::ChatResponse>>,
}

impl ScriptedTarget {
    /// Load scripted responses from a suite's `scripted/` directory.
    ///
    /// Expects files named `<case_id>.json`, each containing a JSON array
    /// of `ChatResponse` objects.
    pub fn load(suite_dir: &std::path::Path) -> anyhow::Result<Self> {
        let scripted_dir = suite_dir.join("scripted");
        let mut scripts = std::collections::BTreeMap::new();

        if scripted_dir.is_dir() {
            for entry in std::fs::read_dir(&scripted_dir)? {
                let entry = entry?;
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("json") {
                    let case_id = path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("")
                        .to_string();
                    let content = std::fs::read_to_string(&path)?;
                    let responses: Vec<omega_client::ChatResponse> =
                        serde_json::from_str(&content)?;
                    scripts.insert(case_id, responses);
                }
            }
        }

        Ok(Self { scripts })
    }
}

impl BenchmarkTarget for ScriptedTarget {
    fn execute(&self, case: &BenchmarkCase, config: &RunConfig) -> anyhow::Result<TargetOutput> {
        let responses = self
            .scripts
            .get(&case.id)
            .ok_or_else(|| anyhow::anyhow!("no scripted responses for case: {}", case.id))?;

        let start = std::time::Instant::now();
        let tmp_dir = tempfile::TempDir::new()?;
        let cwd = tmp_dir.path().to_path_buf();

        // Minimal fixture so LoadedWorkflowCatalog doesn't fail
        let _ = std::fs::create_dir_all(cwd.join("docs/specs"));
        let _ = std::fs::write(cwd.join("README.md"), "# Benchmark Fixture\n");

        let client: DynLlmClient = Arc::new(
            omega_client::test_support::ScriptedLlmClient::from_responses(responses.clone()),
        );
        let runtime = tokio::runtime::Runtime::new()?;
        let loaded_catalog = LoadedWorkflowCatalog::load(&cwd);

        let session = AgentSession::new(AgentSessionConfig {
            client,
            system: "You are a benchmark test agent.".to_string(),
            cwd: cwd.clone(),
            runtime_handle: runtime.handle().clone(),
            scene_catalog: loaded_catalog.scene_catalog,
            workflow_catalog: loaded_catalog.workflow_catalog,
            prompt_catalog: loaded_catalog.prompt_catalog,
            context_window: 200_000,
            max_output_tokens: 32_000,
            bash_allowed_commands: omega_core::default_bash_allowed_commands(),
            batch_max_requests: omega_core::default_batch_max_requests(),
        })?;

        let recorder = RuntimeEnvelopeRecorder::new();
        let turn_id = 1u64;
        session.spawn_turn_with_test_bridge(
            case.prompt.clone(),
            turn_id,
            recorder.runtime_bridge(),
        )?;

        let timeout =
            Duration::from_secs(config.effective_timeout(case.timeout_secs).unwrap_or(30));

        let messages =
            std::panic::catch_unwind(|| recorder.wait_for_turn_finished_messages(turn_id, timeout));

        let messages = match messages {
            Ok(msgs) => msgs,
            Err(_) => {
                let partial = recorder.runtime_messages();
                let tool_trace = extract_tool_trace(&partial);
                return Ok(TargetOutput {
                    response: None,
                    tool_trace,
                    delivery_summary: None,
                    total_tokens: 0,
                    latency_ms: start.elapsed().as_millis() as u64,
                });
            }
        };

        let response = extract_response_text(&messages);
        let tool_trace = extract_tool_trace(&messages);

        Ok(TargetOutput {
            response,
            tool_trace,
            delivery_summary: None,
            total_tokens: 0,
            latency_ms: start.elapsed().as_millis() as u64,
        })
    }
}

/// Extract tool trace entries from recorded runtime messages.
fn extract_tool_trace(messages: &[omega_session::RuntimeMessageEnvelope]) -> Vec<ToolTraceEntry> {
    messages
        .iter()
        .filter_map(|envelope| match &envelope.message {
            RuntimeMessage::Conversation(ConversationMessage::CompleteToolRun { id, status }) => {
                // Find the matching BeginToolRun to get tool_name
                let begin = messages.iter().find_map(|e| match &e.message {
                    RuntimeMessage::Conversation(ConversationMessage::BeginToolRun {
                        tool_run,
                    }) if tool_run.id == *id => Some(tool_run),
                    _ => None,
                });
                let tool_name = begin.map(|tr| tr.tool_name.clone()).unwrap_or_default();
                Some(ToolTraceEntry {
                    tool_name,
                    arguments: serde_json::Value::Null,
                    result: begin.and_then(|tr| tr.result_preview.clone()),
                    is_error: *status == ToolRunStatus::Failed,
                })
            }
            _ => None,
        })
        .collect()
}

/// Extract the final response text from recorded runtime messages.
///
/// The session emits the final assistant text via `ConversationMessage::Text`
/// (source: Assistant, kind: Result). Streaming chunks arrive as
/// `ConversationMessage::AppendSection`. Both are captured here so offline
/// scripted runs (which emit AppendSection) and live runs (which emit Text
/// for the assembled final reply) both produce a non-None response.
fn extract_response_text(messages: &[omega_session::RuntimeMessageEnvelope]) -> Option<String> {
    use omega_session::{RuntimeContentKind, RuntimeSource};

    let mut parts = Vec::new();
    for envelope in messages {
        match &envelope.message {
            RuntimeMessage::Conversation(ConversationMessage::AppendSection {
                delta: omega_session::ResponseSectionDelta::Text(text),
                ..
            }) => {
                parts.push(text.clone());
            }
            RuntimeMessage::Conversation(ConversationMessage::Text {
                source: RuntimeSource::Assistant,
                kind: RuntimeContentKind::Result,
                text,
                ..
            }) => {
                parts.push(text.clone());
            }
            _ => {}
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(""))
    }
}

/// Recorded target that replays pre-recorded case outputs.
///
/// Use this to run scorers against a previously captured run without
/// re-executing against a live model.
pub struct RecordedTarget {
    /// Map of case_id -> recorded output.
    records: std::collections::BTreeMap<String, RecordedOutput>,
}

/// A single recorded output for replay.
#[derive(serde::Deserialize)]
pub struct RecordedOutput {
    pub case_id: String,
    pub response: Option<String>,
    #[serde(default)]
    pub tool_trace: Vec<ToolTraceEntry>,
    #[serde(default)]
    pub delivery_summary: Option<serde_json::Value>,
    #[serde(default)]
    pub total_tokens: u64,
    #[serde(default)]
    pub latency_ms: u64,
}

impl RecordedTarget {
    /// Load recorded outputs from a JSON array file.
    pub fn load(path: &std::path::Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let outputs: Vec<RecordedOutput> = serde_json::from_str(&content)?;
        let records = outputs
            .into_iter()
            .map(|o| (o.case_id.clone(), o))
            .collect();
        Ok(Self { records })
    }
}

impl BenchmarkTarget for RecordedTarget {
    fn execute(&self, case: &BenchmarkCase, _config: &RunConfig) -> anyhow::Result<TargetOutput> {
        match self.records.get(&case.id) {
            Some(rec) => Ok(TargetOutput {
                response: rec.response.clone(),
                tool_trace: rec.tool_trace.clone(),
                delivery_summary: rec.delivery_summary.clone(),
                total_tokens: rec.total_tokens,
                latency_ms: rec.latency_ms,
            }),
            None => anyhow::bail!("no recorded output for case: {}", case.id),
        }
    }
}
