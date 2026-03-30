use std::env;

use async_trait::async_trait;
#[cfg(test)]
use reqwest::header::HeaderMap;
#[cfg(test)]
use serde_json::Value;

use crate::anthropic::{AnthropicClient, AnthropicProviderCapabilities, AnthropicProviderConfig};
use crate::{
    AnthropicMessagesCompatClient, ChatEventStream, ChatRequest, ChatResponse, ClientError,
    LlmClient,
};

pub const MINIMAX_DEFAULT_MODEL: &str = "MiniMax-M2.5";
pub const MINIMAX_GLOBAL_BASE_URL: &str = "https://api.minimax.io/anthropic";
pub const MINIMAX_CHINA_BASE_URL: &str = "https://api.minimaxi.com/anthropic";
pub(crate) const ANTHROPIC_VERSION: &str = "2023-06-01";

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
    pub(crate) fn messages_endpoint(&self) -> String {
        self.compat_client.messages_endpoint()
    }

    #[cfg(test)]
    pub(crate) fn build_headers(&self) -> Result<HeaderMap, ClientError> {
        self.compat_client.build_headers(&[])
    }

    #[cfg(test)]
    pub(crate) fn build_body(&self, request: ChatRequest) -> Result<Value, ClientError> {
        self.compat_client.build_body(request)
    }
}

#[async_trait]
impl LlmClient for MinimaxClient {
    async fn chat(&self, request: ChatRequest) -> Result<ChatResponse, ClientError> {
        self.compat_client().chat(request).await
    }

    async fn count_tokens(&self, request: ChatRequest) -> Result<u32, ClientError> {
        self.compat_client().count_tokens(request).await
    }

    async fn chat_stream(&self, request: ChatRequest) -> Result<ChatEventStream, ClientError> {
        self.compat_client().chat_stream(request).await
    }

    fn provider_name(&self) -> &'static str {
        "minimax"
    }
}
