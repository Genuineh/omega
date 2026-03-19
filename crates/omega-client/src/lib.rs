use std::env;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use thiserror::Error;
use tracing::{debug, instrument, trace};

pub const MINIMAX_DEFAULT_MODEL: &str = "MiniMax-M2.5";
pub const MINIMAX_GLOBAL_BASE_URL: &str = "https://api.minimax.io/anthropic";
pub const MINIMAX_CHINA_BASE_URL: &str = "https://api.minimaxi.com/anthropic";
const ANTHROPIC_VERSION: &str = "2023-06-01";

/// Known stop reason values returned by the Anthropic-compatible API.
pub const STOP_REASON_END_TURN: &str = "end_turn";
pub const STOP_REASON_TOOL_USE: &str = "tool_use";
pub const STOP_REASON_MAX_TOKENS: &str = "max_tokens";
pub const STOP_REASON_STOP_SEQUENCE: &str = "stop_sequence";

pub type DynLlmClient = Arc<dyn LlmClient>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    User,
    Assistant,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Message {
    pub role: Role,
    pub content: MessageContent,
}

impl Message {
    pub fn user<S>(content: S) -> Self
    where
        S: Into<String>,
    {
        Self {
            role: Role::User,
            content: MessageContent::Text(content.into()),
        }
    }

    pub fn assistant(content: Vec<ContentBlock>) -> Self {
        Self {
            role: Role::Assistant,
            content: MessageContent::Blocks(content),
        }
    }

    pub fn tool_results(results: Vec<ContentBlock>) -> Self {
        Self {
            role: Role::User,
            content: MessageContent::Blocks(results),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum MessageContent {
    Text(String),
    Blocks(Vec<ContentBlock>),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text {
        text: String,
    },
    Thinking {
        thinking: String,
        #[serde(default)]
        signature: Option<String>,
    },
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
    ToolResult {
        tool_use_id: String,
        content: Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        is_error: Option<bool>,
    },
}

impl ContentBlock {
    pub fn text<S: Into<String>>(text: S) -> Self {
        Self::Text { text: text.into() }
    }

    pub fn tool_use<S: Into<String>>(id: S, name: S, input: Value) -> Self {
        Self::ToolUse {
            id: id.into(),
            name: name.into(),
            input,
        }
    }

    pub fn tool_result<S: Into<String>>(tool_use_id: S, content: S) -> Self {
        Self::ToolResult {
            tool_use_id: tool_use_id.into(),
            content: Value::String(content.into()),
            is_error: None,
        }
    }

    pub fn tool_result_error<S: Into<String>>(tool_use_id: S, content: S) -> Self {
        Self::ToolResult {
            tool_use_id: tool_use_id.into(),
            content: Value::String(content.into()),
            is_error: Some(true),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    pub messages: Vec<Message>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<ToolDefinition>,
    pub max_tokens: u32,
}

impl ChatRequest {
    pub fn new(messages: Vec<Message>) -> Self {
        Self {
            system: None,
            messages,
            tools: Vec::new(),
            max_tokens: 8_000,
        }
    }

    pub fn with_system<S: Into<String>>(mut self, system: S) -> Self {
        self.system = Some(system.into());
        self
    }

    pub fn with_tools(mut self, tools: Vec<ToolDefinition>) -> Self {
        self.tools = tools;
        self
    }

    pub fn with_max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = max_tokens;
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatResponse {
    pub id: String,
    #[serde(default)]
    pub model: Option<String>,
    pub content: Vec<ContentBlock>,
    #[serde(rename = "stop_reason")]
    pub stop_reason: Option<String>,
    #[serde(default)]
    pub usage: Option<Usage>,
}

impl ChatResponse {
    pub fn is_tool_use(&self) -> bool {
        self.stop_reason.as_deref() == Some(STOP_REASON_TOOL_USE)
    }

    pub fn text_content(&self) -> String {
        self.content
            .iter()
            .filter_map(|block| match block {
                ContentBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("")
    }

    pub fn tool_use_blocks(&self) -> Vec<&ContentBlock> {
        self.content
            .iter()
            .filter(|block| matches!(block, ContentBlock::ToolUse { .. }))
            .collect()
    }
}

#[derive(Debug, Error)]
pub enum ClientError {
    #[error("http request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("json serialization failed: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("invalid header value: {0}")]
    InvalidHeader(#[from] reqwest::header::InvalidHeaderValue),
    #[error("provider returned status {status}: {body}")]
    Api { status: StatusCode, body: String },
    #[error("configuration error: {0}")]
    Config(String),
}

#[async_trait]
pub trait LlmClient: Send + Sync {
    async fn chat(&self, request: ChatRequest) -> Result<ChatResponse, ClientError>;
    fn provider_name(&self) -> &'static str;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MinimaxConfig {
    pub api_key: String,
    pub model: String,
    pub base_url: String,
    pub anthropic_version: String,
}

impl MinimaxConfig {
    pub fn international<S>(api_key: S, model: S) -> Self
    where
        S: Into<String>,
    {
        Self::with_base_url(api_key, model, MINIMAX_GLOBAL_BASE_URL)
    }

    pub fn china_mainland<S>(api_key: S, model: S) -> Self
    where
        S: Into<String>,
    {
        Self::with_base_url(api_key, model, MINIMAX_CHINA_BASE_URL)
    }

    pub fn with_base_url<S1, S2, S3>(api_key: S1, model: S2, base_url: S3) -> Self
    where
        S1: Into<String>,
        S2: Into<String>,
        S3: Into<String>,
    {
        Self {
            api_key: api_key.into(),
            model: model.into(),
            base_url: base_url.into(),
            anthropic_version: ANTHROPIC_VERSION.to_string(),
        }
    }

    pub fn from_env() -> Result<Self, ClientError> {
        let api_key = env::var("OMEGA_API_KEY")
            .or_else(|_| env::var("OMEGA_MINIMAX_API_KEY"))
            .map_err(|_| {
                ClientError::Config("OMEGA_API_KEY or OMEGA_MINIMAX_API_KEY must be set".into())
            })?;
        let model =
            env::var("OMEGA_MODEL_ID").unwrap_or_else(|_| MINIMAX_DEFAULT_MODEL.to_string());
        let base_url =
            env::var("OMEGA_BASE_URL").unwrap_or_else(|_| MINIMAX_GLOBAL_BASE_URL.to_string());

        Ok(Self::with_base_url(api_key, model, base_url))
    }
}

#[derive(Debug, Clone)]
pub struct MinimaxClient {
    http_client: Client,
    config: MinimaxConfig,
}

impl MinimaxClient {
    pub fn new(config: MinimaxConfig) -> Result<Self, ClientError> {
        let http_client = Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(60))
            .build()
            .map_err(ClientError::Http)?;
        Ok(Self {
            http_client,
            config,
        })
    }

    pub fn config(&self) -> &MinimaxConfig {
        &self.config
    }

    fn messages_endpoint(&self) -> String {
        format!("{}/v1/messages", self.config.base_url.trim_end_matches('/'))
    }

    fn build_headers(&self) -> Result<HeaderMap, ClientError> {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(
            "x-api-key",
            HeaderValue::from_str(self.config.api_key.as_str())?,
        );
        headers.insert(
            "anthropic-version",
            HeaderValue::from_str(self.config.anthropic_version.as_str())?,
        );
        Ok(headers)
    }

    fn build_body(&self, request: ChatRequest) -> Result<Value, ClientError> {
        let mut body = json!({
            "model": self.config.model,
            "max_tokens": request.max_tokens,
            "messages": request.messages,
        });

        if let Some(system) = request.system {
            body["system"] = Value::String(system);
        }

        if !request.tools.is_empty() {
            body["tools"] = serde_json::to_value(request.tools)?;
        }

        Ok(body)
    }
}

#[async_trait]
impl LlmClient for MinimaxClient {
    #[instrument(
        skip(self, request),
        fields(
            llm_call.model = %self.config.model,
            llm_call.max_tokens = request.max_tokens,
            llm_call.provider = "minimax",
            llm_call.stop_reason,
            llm_call.duration_ms,
            llm_call.input_tokens,
            llm_call.output_tokens
        )
    )]
    async fn chat(&self, request: ChatRequest) -> Result<ChatResponse, ClientError> {
        let start = Instant::now();
        let body_value = self.build_body(request)?;

        // TRACE level: log raw request JSON
        if let Ok(body_str) = serde_json::to_string(&body_value) {
            trace!(llm_call.request_json = %body_str);
        }

        let response = self
            .http_client
            .post(self.messages_endpoint())
            .headers(self.build_headers()?)
            .json(&body_value)
            .send()
            .await?;

        let status = response.status();
        let body = response.text().await.unwrap_or_default();

        // TRACE level: log raw response JSON
        debug!(llm_call.response_json = %body);

        if !status.is_success() {
            return Err(ClientError::Api { status, body });
        }

        let chat_response = serde_json::from_str::<ChatResponse>(&body).map_err(|e| {
            ClientError::Config(format!("failed to parse response: {e}\nraw body: {body}"))
        })?;

        let duration_ms = start.elapsed().as_millis() as u64;

        // Record span fields
        if let Some(ref usage) = chat_response.usage {
            tracing::Span::current().record("llm_call.input_tokens", usage.input_tokens);
            tracing::Span::current().record("llm_call.output_tokens", usage.output_tokens);
        }
        if let Some(ref stop_reason) = chat_response.stop_reason {
            tracing::Span::current().record("llm_call.stop_reason", stop_reason.as_str());
        }
        tracing::Span::current().record("llm_call.duration_ms", duration_ms);

        Ok(chat_response)
    }

    fn provider_name(&self) -> &'static str {
        "minimax"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Config & Client construction ──────────────────────────────────

    #[test]
    fn minimax_endpoints_match_region() {
        let global = MinimaxConfig::international("key", MINIMAX_DEFAULT_MODEL);
        let mainland = MinimaxConfig::china_mainland("key", MINIMAX_DEFAULT_MODEL);

        assert_eq!(global.base_url, MINIMAX_GLOBAL_BASE_URL);
        assert_eq!(mainland.base_url, MINIMAX_CHINA_BASE_URL);
    }

    #[test]
    fn minimax_config_with_custom_base_url() {
        let cfg = MinimaxConfig::with_base_url("key", "model-x", "https://example.com/api");
        assert_eq!(cfg.base_url, "https://example.com/api");
        assert_eq!(cfg.model, "model-x");
        assert_eq!(cfg.anthropic_version, ANTHROPIC_VERSION);
    }

    #[test]
    fn minimax_client_new_returns_ok() {
        let result = MinimaxClient::new(MinimaxConfig::international("key", MINIMAX_DEFAULT_MODEL));
        assert!(result.is_ok());
    }

    #[test]
    fn minimax_client_builds_messages_endpoint() {
        let client = MinimaxClient::new(MinimaxConfig::international("key", MINIMAX_DEFAULT_MODEL))
            .expect("client should build");

        assert_eq!(
            client.messages_endpoint(),
            "https://api.minimax.io/anthropic/v1/messages"
        );
    }

    #[test]
    fn minimax_client_messages_endpoint_strips_trailing_slash() {
        let client = MinimaxClient::new(MinimaxConfig::with_base_url(
            "k",
            "m",
            "https://example.com/",
        ))
        .expect("client should build");

        assert_eq!(
            client.messages_endpoint(),
            "https://example.com/v1/messages"
        );
    }

    #[test]
    fn minimax_client_provider_name() {
        let client = MinimaxClient::new(MinimaxConfig::international("key", MINIMAX_DEFAULT_MODEL))
            .expect("client should build");
        assert_eq!(client.provider_name(), "minimax");
    }

    #[test]
    fn from_env_fails_without_api_key() {
        // Unset both potential env vars to ensure failure
        env::remove_var("OMEGA_API_KEY");
        env::remove_var("OMEGA_MINIMAX_API_KEY");
        let result = MinimaxConfig::from_env();
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("OMEGA_API_KEY"),
            "error should mention env var: {err}"
        );
    }

    // ── Message constructors ──────────────────────────────────────────

    #[test]
    fn message_user_creates_text_content() {
        let msg = Message::user("hello");
        assert_eq!(msg.role, Role::User);
        assert_eq!(msg.content, MessageContent::Text("hello".into()));
    }

    #[test]
    fn message_assistant_creates_blocks_content() {
        let msg = Message::assistant(vec![ContentBlock::text("hi")]);
        assert_eq!(msg.role, Role::Assistant);
        match &msg.content {
            MessageContent::Blocks(blocks) => {
                assert_eq!(blocks.len(), 1);
                assert!(matches!(&blocks[0], ContentBlock::Text { text } if text == "hi"));
            }
            _ => panic!("expected Blocks"),
        }
    }

    #[test]
    fn message_tool_results_serialize_as_blocks() {
        let message = Message::tool_results(vec![ContentBlock::tool_result("tool-1", "done")]);
        let value = serde_json::to_value(message).expect("message should serialize");

        assert_eq!(value["role"], "user");
        assert_eq!(value["content"][0]["type"], "tool_result");
        assert_eq!(value["content"][0]["tool_use_id"], "tool-1");
        assert_eq!(value["content"][0]["content"], "done");
    }

    // ── ContentBlock constructors & serialization ─────────────────────

    #[test]
    fn content_block_text_roundtrip() {
        let block = ContentBlock::text("hello");
        let json = serde_json::to_value(&block).unwrap();
        assert_eq!(json["type"], "text");
        assert_eq!(json["text"], "hello");

        let back: ContentBlock = serde_json::from_value(json).unwrap();
        assert_eq!(back, block);
    }

    #[test]
    fn content_block_tool_use_roundtrip() {
        let block = ContentBlock::tool_use("id-1", "bash", json!({"command": "ls"}));
        let json = serde_json::to_value(&block).unwrap();
        assert_eq!(json["type"], "tool_use");
        assert_eq!(json["id"], "id-1");
        assert_eq!(json["name"], "bash");
        assert_eq!(json["input"]["command"], "ls");

        let back: ContentBlock = serde_json::from_value(json).unwrap();
        assert_eq!(back, block);
    }

    #[test]
    fn content_block_tool_result_roundtrip() {
        let block = ContentBlock::tool_result("id-1", "output");
        let json = serde_json::to_value(&block).unwrap();
        assert_eq!(json["type"], "tool_result");
        assert_eq!(json["tool_use_id"], "id-1");
        assert_eq!(json["content"], "output");
        assert!(
            json.get("is_error").is_none(),
            "is_error should be skipped when None"
        );

        let back: ContentBlock = serde_json::from_value(json).unwrap();
        assert_eq!(back, block);
    }

    #[test]
    fn content_block_tool_result_error() {
        let block = ContentBlock::tool_result_error("id-1", "fail");
        let json = serde_json::to_value(&block).unwrap();
        assert_eq!(json["is_error"], true);
    }

    // ── ChatRequest builder & serialization ───────────────────────────

    #[test]
    fn chat_request_defaults() {
        let req = ChatRequest::new(vec![Message::user("hi")]);
        assert!(req.system.is_none());
        assert!(req.tools.is_empty());
        assert_eq!(req.max_tokens, 8_000);
    }

    #[test]
    fn chat_request_builder_chain() {
        let tool = ToolDefinition {
            name: "bash".into(),
            description: "run shell".into(),
            input_schema: json!({"type": "object"}),
        };
        let req = ChatRequest::new(vec![Message::user("hi")])
            .with_system("You are helpful.")
            .with_tools(vec![tool.clone()])
            .with_max_tokens(4_000);

        assert_eq!(req.system.as_deref(), Some("You are helpful."));
        assert_eq!(req.tools.len(), 1);
        assert_eq!(req.tools[0].name, "bash");
        assert_eq!(req.max_tokens, 4_000);
    }

    #[test]
    fn chat_request_serializes_correctly() {
        let req = ChatRequest::new(vec![Message::user("hi")]).with_system("sys");
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["system"], "sys");
        assert_eq!(json["messages"][0]["role"], "user");
        assert_eq!(json["messages"][0]["content"], "hi");
        assert_eq!(json["max_tokens"], 8000);
        assert!(json.get("tools").is_none() || json["tools"].as_array().unwrap().is_empty());
    }

    // ── ChatResponse deserialization & helpers ─────────────────────────

    #[test]
    fn chat_response_deserialize_end_turn() {
        let json = json!({
            "id": "msg_01",
            "model": "MiniMax-M2.5",
            "content": [{"type": "text", "text": "Hello!"}],
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 10, "output_tokens": 5}
        });
        let resp: ChatResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp.id, "msg_01");
        assert_eq!(resp.model.as_deref(), Some("MiniMax-M2.5"));
        assert!(!resp.is_tool_use());
        assert_eq!(resp.text_content(), "Hello!");
        assert!(resp.tool_use_blocks().is_empty());

        let usage = resp.usage.expect("usage should be present");
        assert_eq!(usage.input_tokens, 10);
        assert_eq!(usage.output_tokens, 5);
    }

    #[test]
    fn chat_response_deserialize_tool_use() {
        let json = json!({
            "id": "msg_02",
            "content": [
                {"type": "text", "text": "Let me run that."},
                {"type": "tool_use", "id": "tu_1", "name": "bash", "input": {"command": "ls"}}
            ],
            "stop_reason": "tool_use"
        });
        let resp: ChatResponse = serde_json::from_value(json).unwrap();
        assert!(resp.is_tool_use());
        assert_eq!(resp.text_content(), "Let me run that.");
        assert_eq!(resp.tool_use_blocks().len(), 1);
    }

    #[test]
    fn chat_response_missing_usage_defaults_to_none() {
        let json = json!({
            "id": "msg_03",
            "content": [{"type": "text", "text": "ok"}],
            "stop_reason": "end_turn"
        });
        let resp: ChatResponse = serde_json::from_value(json).unwrap();
        assert!(resp.usage.is_none());
    }

    #[test]
    fn chat_response_missing_model_defaults_to_none() {
        let json = json!({
            "id": "msg_04",
            "content": [],
            "stop_reason": "end_turn"
        });
        let resp: ChatResponse = serde_json::from_value(json).unwrap();
        assert!(resp.model.is_none());
    }

    #[test]
    fn chat_response_multiple_text_blocks_concatenated() {
        let resp = ChatResponse {
            id: "msg_05".into(),
            model: None,
            content: vec![ContentBlock::text("hello "), ContentBlock::text("world")],
            stop_reason: Some(STOP_REASON_END_TURN.into()),
            usage: None,
        };
        assert_eq!(resp.text_content(), "hello world");
    }

    // ── build_body ────────────────────────────────────────────────────

    #[test]
    fn build_body_minimal_request() {
        let client = MinimaxClient::new(MinimaxConfig::international("key", "model-a"))
            .expect("client should build");
        let req = ChatRequest::new(vec![Message::user("hi")]);
        let body = client.build_body(req).unwrap();

        assert_eq!(body["model"], "model-a");
        assert_eq!(body["max_tokens"], 8000);
        assert!(body.get("system").is_none());
        assert!(body.get("tools").is_none());
    }

    #[test]
    fn build_body_with_system_and_tools() {
        let client = MinimaxClient::new(MinimaxConfig::international("key", "model-a"))
            .expect("client should build");
        let tool = ToolDefinition {
            name: "bash".into(),
            description: "run".into(),
            input_schema: json!({"type": "object"}),
        };
        let req = ChatRequest::new(vec![Message::user("hi")])
            .with_system("sys prompt")
            .with_tools(vec![tool]);
        let body = client.build_body(req).unwrap();

        assert_eq!(body["system"], "sys prompt");
        assert_eq!(body["tools"][0]["name"], "bash");
    }

    // ── build_headers ─────────────────────────────────────────────────

    #[test]
    fn build_headers_contains_required_keys() {
        let client = MinimaxClient::new(MinimaxConfig::international("test-key", "model"))
            .expect("client should build");
        let headers = client.build_headers().unwrap();

        assert_eq!(headers.get(CONTENT_TYPE).unwrap(), "application/json");
        assert_eq!(headers.get("x-api-key").unwrap(), "test-key");
        assert_eq!(headers.get("anthropic-version").unwrap(), ANTHROPIC_VERSION);
    }

    // ── ToolDefinition serialization ──────────────────────────────────

    #[test]
    fn tool_definition_roundtrip() {
        let tool = ToolDefinition {
            name: "bash".into(),
            description: "Run shell command".into(),
            input_schema: json!({
                "type": "object",
                "properties": {"command": {"type": "string"}},
                "required": ["command"]
            }),
        };
        let json = serde_json::to_value(&tool).unwrap();
        let back: ToolDefinition = serde_json::from_value(json).unwrap();
        assert_eq!(back, tool);
    }

    // ── Error display ─────────────────────────────────────────────────

    #[test]
    fn client_error_display() {
        let err = ClientError::Config("missing key".into());
        assert_eq!(err.to_string(), "configuration error: missing key");

        let err = ClientError::Api {
            status: StatusCode::UNAUTHORIZED,
            body: "bad token".into(),
        };
        assert!(err.to_string().contains("401"));
        assert!(err.to_string().contains("bad token"));
    }

    // ── Stop reason constants ─────────────────────────────────────────

    #[test]
    fn stop_reason_constants_match_api_values() {
        assert_eq!(STOP_REASON_END_TURN, "end_turn");
        assert_eq!(STOP_REASON_TOOL_USE, "tool_use");
        assert_eq!(STOP_REASON_MAX_TOKENS, "max_tokens");
        assert_eq!(STOP_REASON_STOP_SEQUENCE, "stop_sequence");
    }

    // ── DynLlmClient trait object ─────────────────────────────────────

    #[test]
    fn minimax_client_can_be_wrapped_as_dyn() {
        let client = MinimaxClient::new(MinimaxConfig::international("key", MINIMAX_DEFAULT_MODEL))
            .expect("client should build");
        let _dyn_client: DynLlmClient = Arc::new(client);
    }
}
