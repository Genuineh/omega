use std::env;
use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use futures_util::stream::{self, Stream};
#[cfg(test)]
use reqwest::header::HeaderMap;
#[cfg(test)]
use reqwest::header::CONTENT_TYPE;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
#[cfg(test)]
use serde_json::json;
use serde_json::Value;
use thiserror::Error;

pub mod anthropic;
pub mod compat;

pub use anthropic::{
    parse_sse_events, AnthropicBatchResult, AnthropicCacheControl, AnthropicClient,
    AnthropicContentBlock, AnthropicCountTokensRequest, AnthropicEventStream, AnthropicMessage,
    AnthropicMessageAccumulator, AnthropicMessageBatch, AnthropicMessageBatchCreateRequest,
    AnthropicMessageBatchRequest, AnthropicMessageBatchRequestCounts, AnthropicMessageContent,
    AnthropicMessageCreateRequest, AnthropicMessageParam, AnthropicModelInfo,
    AnthropicProviderCapabilities, AnthropicProviderConfig, AnthropicStreamEvent,
    AnthropicSystemBlock, AnthropicThinkingConfig, AnthropicTokenCount, AnthropicToolChoice,
    AnthropicToolDefinition, AnthropicUsage,
};
pub use compat::AnthropicMessagesCompatClient;

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
pub type ChatEventStream = Pin<Box<dyn Stream<Item = Result<ChatEvent, ClientError>> + Send>>;

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

#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[error("provider '{provider}' does not support {operation}: {detail}")]
pub struct ProviderCapabilityError {
    pub provider: String,
    pub operation: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChatEvent {
    MessageStart {
        id: String,
        #[serde(default)]
        model: Option<String>,
    },
    TextDelta {
        text: String,
    },
    ThinkingDelta {
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
        #[serde(default)]
        is_error: Option<bool>,
    },
    MessageComplete {
        #[serde(default)]
        stop_reason: Option<String>,
        #[serde(default)]
        usage: Option<Usage>,
    },
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

    pub fn to_events(&self) -> Vec<ChatEvent> {
        let mut events = Vec::with_capacity(self.content.len() + 2);
        events.push(ChatEvent::MessageStart {
            id: self.id.clone(),
            model: self.model.clone(),
        });

        for block in &self.content {
            match block {
                ContentBlock::Text { text } => {
                    events.push(ChatEvent::TextDelta { text: text.clone() })
                }
                ContentBlock::Thinking {
                    thinking,
                    signature,
                } => events.push(ChatEvent::ThinkingDelta {
                    thinking: thinking.clone(),
                    signature: signature.clone(),
                }),
                ContentBlock::ToolUse { id, name, input } => events.push(ChatEvent::ToolUse {
                    id: id.clone(),
                    name: name.clone(),
                    input: input.clone(),
                }),
                ContentBlock::ToolResult {
                    tool_use_id,
                    content,
                    is_error,
                } => events.push(ChatEvent::ToolResult {
                    tool_use_id: tool_use_id.clone(),
                    content: content.clone(),
                    is_error: *is_error,
                }),
            }
        }

        events.push(ChatEvent::MessageComplete {
            stop_reason: self.stop_reason.clone(),
            usage: self.usage.clone(),
        });
        events
    }
}

#[derive(Debug, Default)]
pub struct ChatResponseBuilder {
    id: Option<String>,
    model: Option<String>,
    content: Vec<ContentBlock>,
    stop_reason: Option<String>,
    usage: Option<Usage>,
    current_text: Option<String>,
    current_thinking: Option<(String, Option<String>)>,
}

impl ChatResponseBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push_event(&mut self, event: ChatEvent) -> Result<(), ClientError> {
        match event {
            ChatEvent::MessageStart { id, model } => {
                if self.id.is_some() {
                    return Err(ClientError::Stream(
                        "chat stream emitted multiple message_start events".to_string(),
                    ));
                }
                self.id = Some(id);
                self.model = model;
            }
            ChatEvent::TextDelta { text } => {
                self.flush_thinking_block();
                self.current_text
                    .get_or_insert_with(String::new)
                    .push_str(&text);
            }
            ChatEvent::ThinkingDelta {
                thinking,
                signature,
            } => {
                self.flush_text_block();
                let current = self
                    .current_thinking
                    .get_or_insert_with(|| (String::new(), None));
                current.0.push_str(&thinking);
                if signature.is_some() {
                    current.1 = signature;
                }
            }
            ChatEvent::ToolUse { id, name, input } => {
                self.flush_open_block();
                self.content.push(ContentBlock::ToolUse { id, name, input });
            }
            ChatEvent::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => {
                self.flush_open_block();
                self.content.push(ContentBlock::ToolResult {
                    tool_use_id,
                    content,
                    is_error,
                });
            }
            ChatEvent::MessageComplete { stop_reason, usage } => {
                self.stop_reason = stop_reason;
                self.usage = usage;
            }
        }

        Ok(())
    }

    pub fn finish(mut self) -> Result<ChatResponse, ClientError> {
        self.flush_open_block();
        let id = self.id.ok_or_else(|| {
            ClientError::Stream("chat stream finished without message_start".to_string())
        })?;

        Ok(ChatResponse {
            id,
            model: self.model,
            content: self.content,
            stop_reason: self.stop_reason,
            usage: self.usage,
        })
    }

    fn flush_open_block(&mut self) {
        self.flush_text_block();
        self.flush_thinking_block();
    }

    fn flush_text_block(&mut self) {
        if let Some(text) = self.current_text.take() {
            if !text.is_empty() {
                self.content.push(ContentBlock::Text { text });
            }
        }
    }

    fn flush_thinking_block(&mut self) {
        if let Some((thinking, signature)) = self.current_thinking.take() {
            if !thinking.is_empty() {
                self.content.push(ContentBlock::Thinking {
                    thinking,
                    signature,
                });
            }
        }
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
    #[error("response decoding failed: {0}")]
    Decode(String),
    #[error("stream processing failed: {0}")]
    Stream(String),
    #[error("configuration error: {0}")]
    Config(String),
    #[error(transparent)]
    UnsupportedCapability(#[from] ProviderCapabilityError),
}

#[async_trait]
pub trait LlmClient: Send + Sync {
    async fn chat(&self, request: ChatRequest) -> Result<ChatResponse, ClientError>;
    async fn chat_stream(&self, request: ChatRequest) -> Result<ChatEventStream, ClientError> {
        let response = self.chat(request).await?;
        Ok(Box::pin(stream::iter(
            response.to_events().into_iter().map(Ok),
        )))
    }
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
            .or_else(|_| env::var("ANTHROPIC_API_KEY"))
            .map_err(|_| {
                ClientError::Config(
                    "OMEGA_API_KEY, OMEGA_MINIMAX_API_KEY, or ANTHROPIC_API_KEY must be set".into(),
                )
            })?;
        let model = env::var("OMEGA_MODEL_ID")
            .or_else(|_| env::var("ANTHROPIC_MODEL"))
            .unwrap_or_else(|_| MINIMAX_DEFAULT_MODEL.to_string());
        let base_url = env::var("OMEGA_BASE_URL")
            .or_else(|_| env::var("ANTHROPIC_BASE_URL"))
            .unwrap_or_else(|_| MINIMAX_GLOBAL_BASE_URL.to_string());

        Ok(Self::with_base_url(api_key, model, base_url))
    }

    pub fn anthropic_provider_config(&self) -> AnthropicProviderConfig {
        AnthropicProviderConfig::new(
            self.api_key.clone(),
            self.model.clone(),
            self.base_url.clone(),
            self.anthropic_version.clone(),
        )
    }

    pub fn provider_capabilities(&self) -> AnthropicProviderCapabilities {
        AnthropicProviderCapabilities::minimax()
    }
}

#[derive(Debug, Clone)]
pub struct MinimaxClient {
    config: MinimaxConfig,
    compat_client: AnthropicMessagesCompatClient,
}

impl MinimaxClient {
    pub fn new(config: MinimaxConfig) -> Result<Self, ClientError> {
        let provider_config = config.anthropic_provider_config();
        let anthropic_client =
            AnthropicClient::new("minimax", provider_config, config.provider_capabilities())?;
        Ok(Self {
            compat_client: AnthropicMessagesCompatClient::new(anthropic_client),
            config,
        })
    }

    pub fn config(&self) -> &MinimaxConfig {
        &self.config
    }

    fn compat_client(&self) -> &AnthropicMessagesCompatClient {
        &self.compat_client
    }

    #[cfg(test)]
    fn messages_endpoint(&self) -> String {
        self.compat_client.messages_endpoint()
    }

    #[cfg(test)]
    fn build_headers(&self) -> Result<HeaderMap, ClientError> {
        self.compat_client.build_headers(&[])
    }

    #[cfg(test)]
    fn build_body(&self, request: ChatRequest) -> Result<Value, ClientError> {
        self.compat_client.build_body(request)
    }
}

#[async_trait]
impl LlmClient for MinimaxClient {
    async fn chat(&self, request: ChatRequest) -> Result<ChatResponse, ClientError> {
        self.compat_client().chat(request).await
    }

    async fn chat_stream(&self, request: ChatRequest) -> Result<ChatEventStream, ClientError> {
        self.compat_client().chat_stream(request).await
    }

    fn provider_name(&self) -> &'static str {
        "minimax"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::StreamExt;

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
        env::remove_var("ANTHROPIC_API_KEY");
        let result = MinimaxConfig::from_env();
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("OMEGA_API_KEY"),
            "error should mention env var: {err}"
        );
    }

    #[test]
    fn from_env_accepts_anthropic_fallbacks() {
        env::remove_var("OMEGA_API_KEY");
        env::remove_var("OMEGA_MINIMAX_API_KEY");
        env::remove_var("OMEGA_MODEL_ID");
        env::remove_var("OMEGA_BASE_URL");
        env::set_var("ANTHROPIC_API_KEY", "anthropic-key");
        env::set_var("ANTHROPIC_MODEL", "claude-compatible");
        env::set_var("ANTHROPIC_BASE_URL", "https://anthropic.example.com");

        let config = MinimaxConfig::from_env().expect("env fallback should load config");

        assert_eq!(config.api_key, "anthropic-key");
        assert_eq!(config.model, "claude-compatible");
        assert_eq!(config.base_url, "https://anthropic.example.com");

        env::remove_var("ANTHROPIC_API_KEY");
        env::remove_var("ANTHROPIC_MODEL");
        env::remove_var("ANTHROPIC_BASE_URL");
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

    #[test]
    fn chat_response_roundtrips_through_events() {
        let response = ChatResponse {
            id: "msg_stream".to_string(),
            model: Some("mock".to_string()),
            content: vec![
                ContentBlock::Thinking {
                    thinking: "plan".to_string(),
                    signature: Some("sig-1".to_string()),
                },
                ContentBlock::text("hello"),
                ContentBlock::tool_use("tool-1", "bash", json!({"command": "pwd"})),
                ContentBlock::text("done"),
            ],
            stop_reason: Some(STOP_REASON_TOOL_USE.to_string()),
            usage: Some(Usage {
                input_tokens: 11,
                output_tokens: 7,
            }),
        };

        let mut builder = ChatResponseBuilder::new();
        for event in response.to_events() {
            builder.push_event(event).unwrap();
        }

        assert_eq!(builder.finish().unwrap(), response);
    }

    #[test]
    fn chat_response_builder_merges_sequential_deltas() {
        let mut builder = ChatResponseBuilder::new();
        builder
            .push_event(ChatEvent::MessageStart {
                id: "msg-1".to_string(),
                model: Some("mock".to_string()),
            })
            .unwrap();
        builder
            .push_event(ChatEvent::ThinkingDelta {
                thinking: "plan".to_string(),
                signature: None,
            })
            .unwrap();
        builder
            .push_event(ChatEvent::ThinkingDelta {
                thinking: " more".to_string(),
                signature: Some("sig".to_string()),
            })
            .unwrap();
        builder
            .push_event(ChatEvent::TextDelta {
                text: "hello".to_string(),
            })
            .unwrap();
        builder
            .push_event(ChatEvent::TextDelta {
                text: " world".to_string(),
            })
            .unwrap();
        builder
            .push_event(ChatEvent::MessageComplete {
                stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                usage: None,
            })
            .unwrap();

        let response = builder.finish().unwrap();

        assert_eq!(
            response.content,
            vec![
                ContentBlock::Thinking {
                    thinking: "plan more".to_string(),
                    signature: Some("sig".to_string()),
                },
                ContentBlock::text("hello world"),
            ]
        );
    }

    #[tokio::test]
    async fn default_chat_stream_replays_chat_response_events() {
        struct StreamingCompatClient;

        #[async_trait]
        impl LlmClient for StreamingCompatClient {
            async fn chat(&self, _request: ChatRequest) -> Result<ChatResponse, ClientError> {
                Ok(ChatResponse {
                    id: "msg-stream".to_string(),
                    model: Some("mock".to_string()),
                    content: vec![
                        ContentBlock::Thinking {
                            thinking: "draft".to_string(),
                            signature: None,
                        },
                        ContentBlock::text("answer"),
                    ],
                    stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                    usage: None,
                })
            }

            fn provider_name(&self) -> &'static str {
                "streaming-compat"
            }
        }

        let client = StreamingCompatClient;
        let mut stream = client
            .chat_stream(ChatRequest::new(vec![Message::user("hi")]))
            .await
            .unwrap();
        let mut events = Vec::new();

        while let Some(event) = stream.next().await {
            events.push(event.unwrap());
        }

        assert_eq!(
            events,
            vec![
                ChatEvent::MessageStart {
                    id: "msg-stream".to_string(),
                    model: Some("mock".to_string()),
                },
                ChatEvent::ThinkingDelta {
                    thinking: "draft".to_string(),
                    signature: None,
                },
                ChatEvent::TextDelta {
                    text: "answer".to_string(),
                },
                ChatEvent::MessageComplete {
                    stop_reason: Some(STOP_REASON_END_TURN.to_string()),
                    usage: None,
                },
            ]
        );
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

        assert_eq!(body["system"][0]["type"], "text");
        assert_eq!(body["system"][0]["text"], "sys prompt");
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
