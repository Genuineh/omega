#[cfg(test)]
use reqwest::header::HeaderMap;
use serde::Deserialize;
use serde_json::Value;

use crate::{ClientError, ProviderCapabilityError};

use super::stream::{
    annotate_stream_error, parse_json_lines, parse_sse_events, validate_stream_event_sequence,
    AnthropicEventStream,
};
use super::transport::AnthropicTransport;
use super::types::{
    AnthropicBatchResult, AnthropicCountTokensRequest, AnthropicMessage, AnthropicMessageBatch,
    AnthropicMessageBatchCreateRequest, AnthropicMessageCreateRequest, AnthropicModelInfo,
    AnthropicProviderCapabilities, AnthropicProviderConfig, AnthropicTokenCount,
};

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
        Ok(Box::pin(futures_util::stream::iter(
            events.into_iter().map(Ok),
        )))
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

#[derive(Debug, Deserialize)]
struct AnthropicListResponse<T> {
    data: Vec<T>,
}
