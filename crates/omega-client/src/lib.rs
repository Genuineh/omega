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
#[path = "lib_tests.rs"]
mod tests;
