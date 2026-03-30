use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use futures_util::stream::{self, Stream};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

fn is_false(value: &bool) -> bool {
    !*value
}

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PromptCacheControl {
    #[serde(rename = "type")]
    pub kind: String,
}

impl PromptCacheControl {
    pub fn ephemeral() -> Self {
        Self {
            kind: "ephemeral".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SystemBlock {
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_control: Option<PromptCacheControl>,
}

impl SystemBlock {
    pub fn text<S: Into<String>>(text: S) -> Self {
        Self {
            text: text.into(),
            cache_control: None,
        }
    }

    pub fn with_cache_control(mut self, cache_control: PromptCacheControl) -> Self {
        self.cache_control = Some(cache_control);
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub system_blocks: Vec<SystemBlock>,
    pub messages: Vec<Message>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<ToolDefinition>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub cache_last_assistant_turn: bool,
    pub max_tokens: u32,
}

impl ChatRequest {
    pub fn new(messages: Vec<Message>) -> Self {
        Self {
            system: None,
            system_blocks: Vec::new(),
            messages,
            tools: Vec::new(),
            cache_last_assistant_turn: false,
            max_tokens: 8_000,
        }
    }

    pub fn with_system<S: Into<String>>(mut self, system: S) -> Self {
        self.system = Some(system.into());
        self
    }

    pub fn with_system_blocks(mut self, system_blocks: Vec<SystemBlock>) -> Self {
        self.system_blocks = system_blocks;
        self
    }

    pub fn with_tools(mut self, tools: Vec<ToolDefinition>) -> Self {
        self.tools = tools;
        self
    }

    pub fn with_cache_last_assistant_turn(mut self, cache_last_assistant_turn: bool) -> Self {
        self.cache_last_assistant_turn = cache_last_assistant_turn;
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_creation_input_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read_input_tokens: Option<u32>,
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
        self.stop_reason.as_deref() == Some(crate::STOP_REASON_TOOL_USE)
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

    async fn count_tokens(&self, _request: ChatRequest) -> Result<u32, ClientError> {
        Err(ProviderCapabilityError {
            provider: self.provider_name().to_string(),
            operation: "messages.count_tokens".to_string(),
            detail: "precise token counting is not supported by this client".to_string(),
        }
        .into())
    }

    async fn chat_stream(&self, request: ChatRequest) -> Result<ChatEventStream, ClientError> {
        let response = self.chat(request).await?;
        Ok(Box::pin(stream::iter(
            response.to_events().into_iter().map(Ok),
        )))
    }

    fn provider_name(&self) -> &'static str;
}
