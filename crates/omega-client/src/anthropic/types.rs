use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::Role;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AnthropicCacheControl {
    #[serde(rename = "type")]
    pub kind: String,
}

impl AnthropicCacheControl {
    pub fn ephemeral() -> Self {
        Self {
            kind: "ephemeral".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AnthropicContentBlock {
    Text {
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        cache_control: Option<AnthropicCacheControl>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        citations: Vec<Value>,
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

impl AnthropicContentBlock {
    pub fn text<S: Into<String>>(text: S) -> Self {
        Self::Text {
            text: text.into(),
            cache_control: None,
            citations: Vec::new(),
        }
    }

    pub fn cache_control(&self) -> Option<&AnthropicCacheControl> {
        match self {
            Self::Text { cache_control, .. } => cache_control.as_ref(),
            Self::Thinking { .. } | Self::ToolUse { .. } | Self::ToolResult { .. } => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum AnthropicMessageContent {
    Text(String),
    Blocks(Vec<AnthropicContentBlock>),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AnthropicMessageParam {
    pub role: Role,
    pub content: AnthropicMessageContent,
}

impl AnthropicMessageParam {
    pub fn text<S: Into<String>>(role: Role, content: S) -> Self {
        Self {
            role,
            content: AnthropicMessageContent::Text(content.into()),
        }
    }

    pub fn blocks(role: Role, content: Vec<AnthropicContentBlock>) -> Self {
        Self {
            role,
            content: AnthropicMessageContent::Blocks(content),
        }
    }

    pub fn contains_cache_control(&self) -> bool {
        match &self.content {
            AnthropicMessageContent::Text(_) => false,
            AnthropicMessageContent::Blocks(blocks) => blocks.iter().any(|block| {
                matches!(
                    block,
                    AnthropicContentBlock::Text {
                        cache_control: Some(_),
                        ..
                    }
                )
            }),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AnthropicSystemBlock {
    #[serde(rename = "type")]
    pub kind: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_control: Option<AnthropicCacheControl>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub citations: Vec<Value>,
}

impl AnthropicSystemBlock {
    pub fn text<S: Into<String>>(text: S) -> Self {
        Self {
            kind: "text".to_string(),
            text: text.into(),
            cache_control: None,
            citations: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AnthropicToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_control: Option<AnthropicCacheControl>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strict: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AnthropicToolChoice {
    Auto,
    Any,
    Tool { name: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AnthropicThinkingConfig {
    Enabled { budget_tokens: u32 },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AnthropicMessageCreateRequest {
    pub model: String,
    pub max_tokens: u32,
    pub messages: Vec<AnthropicMessageParam>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub system: Vec<AnthropicSystemBlock>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<AnthropicToolDefinition>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<AnthropicToolChoice>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stop_sequences: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<AnthropicThinkingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_control: Option<AnthropicCacheControl>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub betas: Vec<String>,
}

impl AnthropicMessageCreateRequest {
    pub fn new(
        model: impl Into<String>,
        messages: Vec<AnthropicMessageParam>,
        max_tokens: u32,
    ) -> Self {
        Self {
            model: model.into(),
            max_tokens,
            messages,
            system: Vec::new(),
            tools: Vec::new(),
            tool_choice: None,
            stream: false,
            metadata: None,
            stop_sequences: Vec::new(),
            temperature: None,
            top_p: None,
            top_k: None,
            thinking: None,
            cache_control: None,
            betas: Vec::new(),
        }
    }

    pub fn contains_cache_control(&self) -> bool {
        self.cache_control.is_some()
            || self
                .system
                .iter()
                .any(|block| block.cache_control.is_some())
            || self.tools.iter().any(|tool| tool.cache_control.is_some())
            || self
                .messages
                .iter()
                .any(AnthropicMessageParam::contains_cache_control)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct AnthropicUsage {
    #[serde(default)]
    pub input_tokens: u32,
    #[serde(default)]
    pub output_tokens: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_creation_input_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read_input_tokens: Option<u32>,
}

impl AnthropicUsage {
    pub(crate) fn merge_from(&mut self, other: &Self) {
        if other.input_tokens != 0 {
            self.input_tokens = other.input_tokens;
        }
        if other.output_tokens != 0 {
            self.output_tokens = other.output_tokens;
        }
        if other.cache_creation_input_tokens.is_some() {
            self.cache_creation_input_tokens = other.cache_creation_input_tokens;
        }
        if other.cache_read_input_tokens.is_some() {
            self.cache_read_input_tokens = other.cache_read_input_tokens;
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AnthropicMessage {
    pub id: String,
    #[serde(default)]
    pub model: Option<String>,
    pub content: Vec<AnthropicContentBlock>,
    #[serde(default)]
    pub stop_reason: Option<String>,
    #[serde(default)]
    pub usage: Option<AnthropicUsage>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AnthropicCountTokensRequest {
    pub model: String,
    pub messages: Vec<AnthropicMessageParam>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub system: Vec<AnthropicSystemBlock>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<AnthropicToolDefinition>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AnthropicTokenCount {
    #[serde(default)]
    pub input_tokens: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AnthropicModelInfo {
    pub id: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AnthropicMessageBatchRequest {
    pub custom_id: String,
    pub params: AnthropicMessageCreateRequest,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AnthropicMessageBatchCreateRequest {
    pub requests: Vec<AnthropicMessageBatchRequest>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AnthropicMessageBatchRequestCounts {
    #[serde(default)]
    pub processing: Option<u32>,
    #[serde(default)]
    pub succeeded: Option<u32>,
    #[serde(default)]
    pub errored: Option<u32>,
    #[serde(default)]
    pub canceled: Option<u32>,
    #[serde(default)]
    pub expired: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AnthropicMessageBatch {
    pub id: String,
    #[serde(default)]
    pub processing_status: Option<String>,
    #[serde(default)]
    pub request_counts: Option<AnthropicMessageBatchRequestCounts>,
    #[serde(default)]
    pub results_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AnthropicBatchResult {
    pub custom_id: String,
    #[serde(default)]
    pub result: Option<Value>,
    #[serde(default)]
    pub error: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnthropicProviderConfig {
    pub api_key: String,
    pub default_model: String,
    pub base_url: String,
    pub anthropic_version: String,
    pub default_betas: Vec<String>,
    pub connect_timeout: Duration,
    pub timeout: Duration,
}

impl AnthropicProviderConfig {
    pub fn new(
        api_key: impl Into<String>,
        default_model: impl Into<String>,
        base_url: impl Into<String>,
        anthropic_version: impl Into<String>,
    ) -> Self {
        Self {
            api_key: api_key.into(),
            default_model: default_model.into(),
            base_url: base_url.into(),
            anthropic_version: anthropic_version.into(),
            default_betas: Vec::new(),
            connect_timeout: Duration::from_secs(10),
            timeout: Duration::from_secs(60),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnthropicProviderCapabilities {
    pub tools: bool,
    pub thinking: bool,
    pub prompt_caching: bool,
    pub streaming: bool,
    pub count_tokens: bool,
    pub models: bool,
    pub message_batches: bool,
}

impl AnthropicProviderCapabilities {
    pub fn minimax() -> Self {
        Self {
            tools: true,
            thinking: true,
            prompt_caching: true,
            streaming: true,
            count_tokens: true,
            models: true,
            message_batches: true,
        }
    }
}

fn is_false(value: &bool) -> bool {
    !*value
}
