use std::env;
use std::time::Duration;

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
    pub request_throttle_interval: Duration,
    pub max_concurrent_requests: usize,
    pub rate_limit_retry_delay: Duration,
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
            request_throttle_interval: Duration::from_millis(100),
            max_concurrent_requests: 1,
            rate_limit_retry_delay: Duration::from_secs(10),
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
        let request_throttle_interval = env_duration_millis(
            &[
                "OMEGA_PROVIDER_REQUEST_THROTTLE_MS",
                "ANTHROPIC_PROVIDER_REQUEST_THROTTLE_MS",
            ],
            Duration::from_millis(100),
        )?;
        let max_concurrent_requests = env_usize(
            &[
                "OMEGA_PROVIDER_MAX_CONCURRENT_REQUESTS",
                "ANTHROPIC_PROVIDER_MAX_CONCURRENT_REQUESTS",
            ],
            1,
        )?;
        let rate_limit_retry_delay = env_duration_millis(
            &[
                "OMEGA_PROVIDER_RATE_LIMIT_RETRY_DELAY_MS",
                "ANTHROPIC_PROVIDER_RATE_LIMIT_RETRY_DELAY_MS",
            ],
            Duration::from_secs(10),
        )?;

        Ok(Self::with_base_url(api_key, model, base_url)
            .with_request_throttle_interval(request_throttle_interval)
            .with_max_concurrent_requests(max_concurrent_requests)
            .with_rate_limit_retry_delay(rate_limit_retry_delay))
    }

    pub fn anthropic_provider_config(&self) -> AnthropicProviderConfig {
        AnthropicProviderConfig::new(
            self.api_key.clone(),
            self.model.clone(),
            self.base_url.clone(),
            self.anthropic_version.clone(),
        )
        .with_request_throttle_interval(self.request_throttle_interval)
        .with_max_concurrent_requests(self.max_concurrent_requests)
        .with_rate_limit_retry_delay(self.rate_limit_retry_delay)
    }

    pub fn provider_capabilities(&self) -> AnthropicProviderCapabilities {
        AnthropicProviderCapabilities::minimax()
    }
}

impl MinimaxConfig {
    pub fn with_request_throttle_interval(mut self, request_throttle_interval: Duration) -> Self {
        self.request_throttle_interval = request_throttle_interval;
        self
    }

    pub fn with_max_concurrent_requests(mut self, max_concurrent_requests: usize) -> Self {
        self.max_concurrent_requests = max_concurrent_requests.max(1);
        self
    }

    pub fn with_rate_limit_retry_delay(mut self, rate_limit_retry_delay: Duration) -> Self {
        self.rate_limit_retry_delay = rate_limit_retry_delay;
        self
    }
}

fn env_duration_millis(names: &[&str], default: Duration) -> Result<Duration, ClientError> {
    for name in names {
        match env::var(name) {
            Ok(value) => {
                let millis = value.parse::<u64>().map_err(|_| {
                    ClientError::Config(format!("{name} must be an integer number of milliseconds"))
                })?;
                return Ok(Duration::from_millis(millis));
            }
            Err(env::VarError::NotPresent) => continue,
            Err(error) => {
                return Err(ClientError::Config(format!(
                    "failed to read {name}: {error}"
                )));
            }
        }
    }

    Ok(default)
}

fn env_usize(names: &[&str], default: usize) -> Result<usize, ClientError> {
    for name in names {
        match env::var(name) {
            Ok(value) => {
                let parsed = value.parse::<usize>().map_err(|_| {
                    ClientError::Config(format!("{name} must be a positive integer"))
                })?;
                if parsed == 0 {
                    return Err(ClientError::Config(format!(
                        "{name} must be greater than 0"
                    )));
                }
                return Ok(parsed);
            }
            Err(env::VarError::NotPresent) => continue,
            Err(error) => {
                return Err(ClientError::Config(format!(
                    "failed to read {name}: {error}"
                )));
            }
        }
    }

    Ok(default)
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
