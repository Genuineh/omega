use std::pin::Pin;
use std::time::Duration;

use futures_util::stream::{self, Stream};
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, CONTENT_TYPE};
use reqwest::{Client, Method};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::{debug, trace};

use crate::{ClientError, ProviderCapabilityError, Role};

pub type AnthropicEventStream =
    Pin<Box<dyn Stream<Item = Result<AnthropicStreamEvent, ClientError>> + Send>>;

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
    fn merge_from(&mut self, other: &Self) {
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AnthropicStreamEvent {
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
    MessageComplete {
        #[serde(default)]
        stop_reason: Option<String>,
        #[serde(default)]
        usage: Option<AnthropicUsage>,
    },
}

#[derive(Debug, Default)]
pub struct AnthropicMessageAccumulator {
    id: Option<String>,
    model: Option<String>,
    content: Vec<AnthropicContentBlock>,
    stop_reason: Option<String>,
    usage: Option<AnthropicUsage>,
    current_text: Option<String>,
    current_thinking: Option<(String, Option<String>)>,
}

impl AnthropicMessageAccumulator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push_event(&mut self, event: AnthropicStreamEvent) -> Result<(), ClientError> {
        match event {
            AnthropicStreamEvent::MessageStart { id, model } => {
                if self.id.is_some() {
                    return Err(ClientError::Stream(
                        "anthropic stream emitted multiple message_start events".to_string(),
                    ));
                }
                self.id = Some(id);
                self.model = model;
            }
            AnthropicStreamEvent::TextDelta { text } => {
                self.flush_thinking_block();
                self.current_text
                    .get_or_insert_with(String::new)
                    .push_str(&text);
            }
            AnthropicStreamEvent::ThinkingDelta {
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
            AnthropicStreamEvent::ToolUse { id, name, input } => {
                self.flush_open_block();
                self.content
                    .push(AnthropicContentBlock::ToolUse { id, name, input });
            }
            AnthropicStreamEvent::MessageComplete { stop_reason, usage } => {
                self.stop_reason = stop_reason;
                self.usage = usage;
            }
        }

        Ok(())
    }

    pub fn finish(mut self) -> Result<AnthropicMessage, ClientError> {
        self.flush_open_block();
        let id = self.id.ok_or_else(|| {
            ClientError::Stream("anthropic stream finished without message_start".to_string())
        })?;

        Ok(AnthropicMessage {
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
                self.content.push(AnthropicContentBlock::text(text));
            }
        }
    }

    fn flush_thinking_block(&mut self) {
        if let Some((thinking, signature)) = self.current_thinking.take() {
            if !thinking.is_empty() {
                self.content.push(AnthropicContentBlock::Thinking {
                    thinking,
                    signature,
                });
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct AnthropicClient {
    provider_name: &'static str,
    config: AnthropicProviderConfig,
    capabilities: AnthropicProviderCapabilities,
    transport: AnthropicTransport,
}

impl AnthropicClient {
    pub fn new(
        provider_name: &'static str,
        config: AnthropicProviderConfig,
        capabilities: AnthropicProviderCapabilities,
    ) -> Result<Self, ClientError> {
        let transport = AnthropicTransport::new(provider_name, config.clone())?;
        Ok(Self {
            provider_name,
            config,
            capabilities,
            transport,
        })
    }

    pub fn provider_name(&self) -> &'static str {
        self.provider_name
    }

    pub fn config(&self) -> &AnthropicProviderConfig {
        &self.config
    }

    pub fn capabilities(&self) -> AnthropicProviderCapabilities {
        self.capabilities
    }

    pub fn messages(&self) -> MessagesService<'_> {
        MessagesService { client: self }
    }

    pub fn models(&self) -> ModelsService<'_> {
        ModelsService { client: self }
    }

    pub fn message_batches(&self) -> MessageBatchesService<'_> {
        MessageBatchesService { client: self }
    }

    #[cfg(test)]
    pub(crate) fn messages_endpoint(&self) -> String {
        self.transport.endpoint("/v1/messages")
    }

    #[cfg(test)]
    pub(crate) fn build_headers(&self, betas: &[String]) -> Result<HeaderMap, ClientError> {
        self.transport.build_headers(betas, false)
    }

    fn ensure_capability(
        &self,
        supported: bool,
        operation: &str,
        detail: &str,
    ) -> Result<(), ClientError> {
        if supported {
            Ok(())
        } else {
            Err(ProviderCapabilityError {
                provider: self.provider_name.to_string(),
                operation: operation.to_string(),
                detail: detail.to_string(),
            }
            .into())
        }
    }

    fn ensure_message_request_supported(
        &self,
        request: &AnthropicMessageCreateRequest,
    ) -> Result<(), ClientError> {
        if !request.tools.is_empty() {
            self.ensure_capability(
                self.capabilities.tools,
                "messages.tools",
                "tool definitions are not supported by this provider",
            )?;
        }
        if request.thinking.is_some() {
            self.ensure_capability(
                self.capabilities.thinking,
                "messages.thinking",
                "thinking blocks are not supported by this provider",
            )?;
        }
        if request.contains_cache_control() {
            self.ensure_capability(
                self.capabilities.prompt_caching,
                "messages.prompt_caching",
                "prompt caching markers are not supported by this provider",
            )?;
        }
        if request.stream {
            self.ensure_capability(
                self.capabilities.streaming,
                "messages.create_stream",
                "streaming responses are not supported by this provider",
            )?;
        }
        Ok(())
    }
}

pub struct MessagesService<'a> {
    client: &'a AnthropicClient,
}

impl MessagesService<'_> {
    pub async fn create(
        &self,
        request: AnthropicMessageCreateRequest,
    ) -> Result<AnthropicMessage, ClientError> {
        self.client.ensure_message_request_supported(&request)?;
        self.client
            .transport
            .post_json("/v1/messages", &request, &request.betas)
            .await
    }

    pub async fn create_stream(
        &self,
        mut request: AnthropicMessageCreateRequest,
    ) -> Result<AnthropicEventStream, ClientError> {
        request.stream = true;
        self.client.ensure_message_request_supported(&request)?;
        let body = self
            .client
            .transport
            .post_text("/v1/messages", &request, &request.betas, true)
            .await?;
        let events = parse_sse_events(&body).map_err(|error| {
            annotate_stream_error(self.client.provider_name(), "/v1/messages", error)
        })?;
        validate_stream_event_sequence(&events, &body).map_err(|error| {
            annotate_stream_error(self.client.provider_name(), "/v1/messages", error)
        })?;
        Ok(Box::pin(stream::iter(events.into_iter().map(Ok))))
    }

    pub async fn count_tokens(
        &self,
        request: AnthropicCountTokensRequest,
    ) -> Result<AnthropicTokenCount, ClientError> {
        self.client.ensure_capability(
            self.client.capabilities.count_tokens,
            "messages.count_tokens",
            "token counting is not supported by this provider",
        )?;
        self.client
            .transport
            .post_json("/v1/messages/count_tokens", &request, &[])
            .await
    }

    pub fn body_value(
        &self,
        request: &AnthropicMessageCreateRequest,
    ) -> Result<Value, ClientError> {
        serde_json::to_value(request).map_err(ClientError::Serialization)
    }
}

pub struct ModelsService<'a> {
    client: &'a AnthropicClient,
}

impl ModelsService<'_> {
    pub async fn list(&self) -> Result<Vec<AnthropicModelInfo>, ClientError> {
        self.client.ensure_capability(
            self.client.capabilities.models,
            "models.list",
            "model listing is not supported by this provider",
        )?;
        let response: AnthropicListResponse<AnthropicModelInfo> =
            self.client.transport.get_json("/v1/models", &[]).await?;
        Ok(response.data)
    }
}

pub struct MessageBatchesService<'a> {
    client: &'a AnthropicClient,
}

impl MessageBatchesService<'_> {
    pub async fn create(
        &self,
        request: AnthropicMessageBatchCreateRequest,
    ) -> Result<AnthropicMessageBatch, ClientError> {
        self.client.ensure_capability(
            self.client.capabilities.message_batches,
            "message_batches.create",
            "message batches are not supported by this provider",
        )?;
        self.client
            .transport
            .post_json("/v1/messages/batches", &request, &[])
            .await
    }

    pub async fn get(&self, batch_id: &str) -> Result<AnthropicMessageBatch, ClientError> {
        self.client.ensure_capability(
            self.client.capabilities.message_batches,
            "message_batches.get",
            "message batches are not supported by this provider",
        )?;
        self.client
            .transport
            .get_json(&format!("/v1/messages/batches/{batch_id}"), &[])
            .await
    }

    pub async fn list(&self) -> Result<Vec<AnthropicMessageBatch>, ClientError> {
        self.client.ensure_capability(
            self.client.capabilities.message_batches,
            "message_batches.list",
            "message batches are not supported by this provider",
        )?;
        let response: AnthropicListResponse<AnthropicMessageBatch> = self
            .client
            .transport
            .get_json("/v1/messages/batches", &[])
            .await?;
        Ok(response.data)
    }

    pub async fn results(&self, batch_id: &str) -> Result<Vec<AnthropicBatchResult>, ClientError> {
        self.client.ensure_capability(
            self.client.capabilities.message_batches,
            "message_batches.results",
            "message batches are not supported by this provider",
        )?;
        let body = self
            .client
            .transport
            .get_text(&format!("/v1/messages/batches/{batch_id}/results"), &[])
            .await?;
        parse_json_lines(&body)
    }
}

#[derive(Debug, Clone)]
struct AnthropicTransport {
    provider_name: &'static str,
    config: AnthropicProviderConfig,
    http_client: Client,
}

impl AnthropicTransport {
    fn new(
        provider_name: &'static str,
        config: AnthropicProviderConfig,
    ) -> Result<Self, ClientError> {
        let http_client = Client::builder()
            .connect_timeout(config.connect_timeout)
            .timeout(config.timeout)
            .build()
            .map_err(ClientError::Http)?;
        Ok(Self {
            provider_name,
            config,
            http_client,
        })
    }

    fn endpoint(&self, path: &str) -> String {
        format!(
            "{}/{}",
            self.config.base_url.trim_end_matches('/'),
            path.trim_start_matches('/')
        )
    }

    fn build_headers(
        &self,
        request_betas: &[String],
        accept_sse: bool,
    ) -> Result<HeaderMap, ClientError> {
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
        if accept_sse {
            headers.insert(ACCEPT, HeaderValue::from_static("text/event-stream"));
        }

        let mut betas = self.config.default_betas.clone();
        for beta in request_betas {
            if !betas.iter().any(|existing| existing == beta) {
                betas.push(beta.clone());
            }
        }
        if !betas.is_empty() {
            headers.insert("anthropic-beta", HeaderValue::from_str(&betas.join(","))?);
        }

        Ok(headers)
    }

    async fn post_json<T: Serialize, R: DeserializeOwned>(
        &self,
        path: &str,
        body: &T,
        betas: &[String],
    ) -> Result<R, ClientError> {
        let body_value = serde_json::to_value(body).map_err(ClientError::Serialization)?;
        if let Ok(body_str) = serde_json::to_string(&body_value) {
            trace!(provider = self.provider_name, endpoint = path, request_json = %body_str);
        }

        let response = self
            .http_client
            .request(Method::POST, self.endpoint(path))
            .headers(self.build_headers(betas, false)?)
            .json(&body_value)
            .send()
            .await?;
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        debug!(provider = self.provider_name, endpoint = path, response_json = %body);

        if !status.is_success() {
            return Err(ClientError::Api { status, body });
        }

        serde_json::from_str(&body)
            .map_err(|error| ClientError::Decode(format!("failed to decode response: {error}")))
    }

    async fn post_text<T: Serialize>(
        &self,
        path: &str,
        body: &T,
        betas: &[String],
        accept_sse: bool,
    ) -> Result<String, ClientError> {
        let body_value = serde_json::to_value(body).map_err(ClientError::Serialization)?;
        if let Ok(body_str) = serde_json::to_string(&body_value) {
            trace!(provider = self.provider_name, endpoint = path, request_json = %body_str);
        }

        let response = self
            .http_client
            .request(Method::POST, self.endpoint(path))
            .headers(self.build_headers(betas, accept_sse)?)
            .json(&body_value)
            .send()
            .await?;
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        debug!(provider = self.provider_name, endpoint = path, response_json = %body);

        if !status.is_success() {
            return Err(ClientError::Api { status, body });
        }

        Ok(body)
    }

    async fn get_json<R: DeserializeOwned>(
        &self,
        path: &str,
        betas: &[String],
    ) -> Result<R, ClientError> {
        let response = self
            .http_client
            .request(Method::GET, self.endpoint(path))
            .headers(self.build_headers(betas, false)?)
            .send()
            .await?;
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        debug!(provider = self.provider_name, endpoint = path, response_json = %body);

        if !status.is_success() {
            return Err(ClientError::Api { status, body });
        }

        serde_json::from_str(&body)
            .map_err(|error| ClientError::Decode(format!("failed to decode response: {error}")))
    }

    async fn get_text(&self, path: &str, betas: &[String]) -> Result<String, ClientError> {
        let response = self
            .http_client
            .request(Method::GET, self.endpoint(path))
            .headers(self.build_headers(betas, false)?)
            .send()
            .await?;
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        debug!(provider = self.provider_name, endpoint = path, response_json = %body);

        if !status.is_success() {
            return Err(ClientError::Api { status, body });
        }

        Ok(body)
    }
}

#[derive(Debug, Deserialize)]
struct AnthropicListResponse<T> {
    data: Vec<T>,
}

#[derive(Default)]
struct StreamParseState {
    pending_tool_use: Option<PendingToolUse>,
    stop_reason: Option<String>,
    usage: Option<AnthropicUsage>,
}

struct PendingToolUse {
    id: String,
    name: String,
    input_value: Option<Value>,
    partial_json: String,
}

pub fn parse_sse_events(body: &str) -> Result<Vec<AnthropicStreamEvent>, ClientError> {
    let mut events = Vec::new();
    let mut state = StreamParseState::default();

    for frame in body.split("\n\n") {
        let payload = extract_sse_data(frame);
        if payload.is_empty() || payload == "[DONE]" {
            continue;
        }

        let value: Value = serde_json::from_str(&payload).map_err(|error| {
            ClientError::Stream(format!(
                "failed to parse SSE payload: {error}: {payload}; {}",
                stream_body_diagnostics(body)
            ))
        })?;
        consume_sse_value(&value, &mut state, &mut events)?;
    }

    Ok(events)
}

fn validate_stream_event_sequence(
    events: &[AnthropicStreamEvent],
    body: &str,
) -> Result<(), ClientError> {
    let start_count = events
        .iter()
        .filter(|event| matches!(event, AnthropicStreamEvent::MessageStart { .. }))
        .count();

    if start_count == 0 {
        return Err(ClientError::Stream(format!(
            "stream missing initial message_start; {}",
            stream_body_diagnostics(body)
        )));
    }

    if !matches!(
        events.first(),
        Some(AnthropicStreamEvent::MessageStart { .. })
    ) {
        return Err(ClientError::Stream(format!(
            "stream started with {:?} instead of message_start; {}",
            event_name(events.first()),
            stream_body_diagnostics(body)
        )));
    }

    if start_count > 1 {
        return Err(ClientError::Stream(format!(
            "stream emitted multiple message_start events; {}",
            stream_body_diagnostics(body)
        )));
    }

    Ok(())
}

fn annotate_stream_error(provider_name: &str, path: &str, error: ClientError) -> ClientError {
    match error {
        ClientError::Stream(message) => {
            ClientError::Stream(format!("{provider_name} {path}: {message}"))
        }
        other => other,
    }
}

fn event_name(event: Option<&AnthropicStreamEvent>) -> &'static str {
    match event {
        Some(AnthropicStreamEvent::MessageStart { .. }) => "message_start",
        Some(AnthropicStreamEvent::TextDelta { .. }) => "text_delta",
        Some(AnthropicStreamEvent::ThinkingDelta { .. }) => "thinking_delta",
        Some(AnthropicStreamEvent::ToolUse { .. }) => "tool_use",
        Some(AnthropicStreamEvent::MessageComplete { .. }) => "message_complete",
        None => "no_event",
    }
}

fn stream_body_diagnostics(body: &str) -> String {
    let payloads = body
        .split("\n\n")
        .map(extract_sse_data)
        .filter(|payload| !payload.is_empty() && payload != "[DONE]")
        .collect::<Vec<_>>();
    let event_types = payloads
        .iter()
        .filter_map(|payload| {
            serde_json::from_str::<Value>(payload)
                .ok()
                .and_then(|value| {
                    value
                        .get("type")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                })
        })
        .take(6)
        .collect::<Vec<_>>();
    let preview = payloads
        .iter()
        .take(2)
        .map(|payload| preview_stream_fragment(payload, 120))
        .collect::<Vec<_>>()
        .join(" | ");

    format!(
        "frame_count={}, event_types={:?}, body_preview={} bytes:{}",
        payloads.len(),
        event_types,
        if preview.is_empty() {
            "<empty>"
        } else {
            preview.as_str()
        },
        body.len()
    )
}

fn preview_stream_fragment(text: &str, limit: usize) -> String {
    let mut chars = text.chars();
    let preview = chars.by_ref().take(limit).collect::<String>();
    let preview = preview.replace('\n', "\\n");
    if chars.next().is_some() {
        format!("{}...", preview)
    } else {
        preview
    }
}

fn extract_sse_data(frame: &str) -> String {
    frame
        .lines()
        .filter_map(|line| line.strip_prefix("data:"))
        .map(str::trim)
        .collect::<Vec<_>>()
        .join("\n")
}

fn consume_sse_value(
    value: &Value,
    state: &mut StreamParseState,
    events: &mut Vec<AnthropicStreamEvent>,
) -> Result<(), ClientError> {
    let event_type = value
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| ClientError::Stream("SSE payload missing type".to_string()))?;

    match event_type {
        "ping" => {}
        "message_start" => {
            let message = value
                .get("message")
                .ok_or_else(|| ClientError::Stream("message_start missing message".to_string()))?;
            let id = message
                .get("id")
                .and_then(Value::as_str)
                .ok_or_else(|| ClientError::Stream("message_start missing id".to_string()))?;
            let model = message
                .get("model")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned);
            if let Some(usage) = parse_usage_field(message.get("usage"))? {
                state.usage = Some(usage);
            }
            events.push(AnthropicStreamEvent::MessageStart {
                id: id.to_string(),
                model,
            });
        }
        "content_block_start" => {
            let block = value.get("content_block").ok_or_else(|| {
                ClientError::Stream("content_block_start missing content_block".to_string())
            })?;
            match block.get("type").and_then(Value::as_str) {
                Some("text") => {
                    if let Some(text) = block.get("text").and_then(Value::as_str) {
                        if !text.is_empty() {
                            events.push(AnthropicStreamEvent::TextDelta {
                                text: text.to_string(),
                            });
                        }
                    }
                }
                Some("thinking") => {
                    if let Some(thinking) = block.get("thinking").and_then(Value::as_str) {
                        if !thinking.is_empty() {
                            events.push(AnthropicStreamEvent::ThinkingDelta {
                                thinking: thinking.to_string(),
                                signature: block
                                    .get("signature")
                                    .and_then(Value::as_str)
                                    .map(ToOwned::to_owned),
                            });
                        }
                    }
                }
                Some("tool_use") => {
                    let id = block
                        .get("id")
                        .and_then(Value::as_str)
                        .ok_or_else(|| ClientError::Stream("tool_use missing id".to_string()))?;
                    let name = block
                        .get("name")
                        .and_then(Value::as_str)
                        .ok_or_else(|| ClientError::Stream("tool_use missing name".to_string()))?;
                    state.pending_tool_use = Some(PendingToolUse {
                        id: id.to_string(),
                        name: name.to_string(),
                        input_value: block.get("input").cloned(),
                        partial_json: String::new(),
                    });
                }
                Some(other) => {
                    return Err(ClientError::Stream(format!(
                        "unsupported content_block_start type: {other}"
                    )))
                }
                None => {
                    return Err(ClientError::Stream(
                        "content_block_start missing content_block.type".to_string(),
                    ))
                }
            }
        }
        "content_block_delta" => {
            let delta = value.get("delta").ok_or_else(|| {
                ClientError::Stream("content_block_delta missing delta".to_string())
            })?;
            match delta.get("type").and_then(Value::as_str) {
                Some("text_delta") => {
                    let text = delta.get("text").and_then(Value::as_str).ok_or_else(|| {
                        ClientError::Stream("text_delta missing text".to_string())
                    })?;
                    events.push(AnthropicStreamEvent::TextDelta {
                        text: text.to_string(),
                    });
                }
                Some("thinking_delta") => {
                    let thinking =
                        delta
                            .get("thinking")
                            .and_then(Value::as_str)
                            .ok_or_else(|| {
                                ClientError::Stream("thinking_delta missing thinking".to_string())
                            })?;
                    events.push(AnthropicStreamEvent::ThinkingDelta {
                        thinking: thinking.to_string(),
                        signature: None,
                    });
                }
                Some("signature_delta") => {
                    let signature =
                        delta
                            .get("signature")
                            .and_then(Value::as_str)
                            .ok_or_else(|| {
                                ClientError::Stream("signature_delta missing signature".to_string())
                            })?;
                    events.push(AnthropicStreamEvent::ThinkingDelta {
                        thinking: String::new(),
                        signature: Some(signature.to_string()),
                    });
                }
                Some("input_json_delta") => {
                    let partial_json = delta
                        .get("partial_json")
                        .and_then(Value::as_str)
                        .ok_or_else(|| {
                            ClientError::Stream("input_json_delta missing partial_json".to_string())
                        })?;
                    let tool_use = state.pending_tool_use.as_mut().ok_or_else(|| {
                        ClientError::Stream(
                            "received input_json_delta without an open tool_use block".to_string(),
                        )
                    })?;
                    tool_use.partial_json.push_str(partial_json);
                }
                Some(other) => {
                    return Err(ClientError::Stream(format!(
                        "unsupported content_block_delta type: {other}"
                    )))
                }
                None => {
                    return Err(ClientError::Stream(
                        "content_block_delta missing delta.type".to_string(),
                    ))
                }
            }
        }
        "content_block_stop" => {
            if let Some(tool_use) = state.pending_tool_use.take() {
                let input = if !tool_use.partial_json.is_empty() {
                    serde_json::from_str(&tool_use.partial_json).map_err(|error| {
                        ClientError::Stream(format!(
                            "failed to parse tool_use partial_json: {error}"
                        ))
                    })?
                } else {
                    tool_use
                        .input_value
                        .unwrap_or(Value::Object(Default::default()))
                };
                events.push(AnthropicStreamEvent::ToolUse {
                    id: tool_use.id,
                    name: tool_use.name,
                    input,
                });
            }
        }
        "message_delta" => {
            if let Some(delta) = value.get("delta") {
                if let Some(stop_reason) = delta.get("stop_reason").and_then(Value::as_str) {
                    state.stop_reason = Some(stop_reason.to_string());
                }
            }
            if let Some(usage) = parse_usage_field(value.get("usage"))? {
                let current = state.usage.get_or_insert_with(AnthropicUsage::default);
                current.merge_from(&usage);
            }
        }
        "message_stop" => {
            events.push(AnthropicStreamEvent::MessageComplete {
                stop_reason: state.stop_reason.take(),
                usage: state.usage.take(),
            });
        }
        other => {
            return Err(ClientError::Stream(format!(
                "unsupported anthropic SSE event type: {other}"
            )))
        }
    }

    Ok(())
}

fn parse_usage_field(value: Option<&Value>) -> Result<Option<AnthropicUsage>, ClientError> {
    match value {
        None => Ok(None),
        Some(Value::Null) => Ok(None),
        Some(value) => serde_json::from_value(value.clone())
            .map(Some)
            .map_err(|error| ClientError::Stream(format!("failed to parse usage: {error}"))),
    }
}

fn parse_json_lines<T: DeserializeOwned>(body: &str) -> Result<Vec<T>, ClientError> {
    body.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            serde_json::from_str(line).map_err(|error| {
                ClientError::Decode(format!("failed to decode jsonl line: {error}"))
            })
        })
        .collect()
}

fn is_false(value: &bool) -> bool {
    !*value
}
