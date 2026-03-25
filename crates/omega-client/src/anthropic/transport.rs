use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, CONTENT_TYPE};
use reqwest::{Client, Method};
use serde::de::DeserializeOwned;
use serde::Serialize;
use tracing::{debug, trace};

use crate::ClientError;

use super::types::AnthropicProviderConfig;

#[derive(Debug, Clone)]
pub(super) struct AnthropicTransport {
    provider_name: &'static str,
    config: AnthropicProviderConfig,
    http_client: Client,
}

impl AnthropicTransport {
    pub(super) fn new(
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

    pub(super) fn endpoint(&self, path: &str) -> String {
        format!(
            "{}/{}",
            self.config.base_url.trim_end_matches('/'),
            path.trim_start_matches('/')
        )
    }

    pub(super) fn build_headers(
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

    pub(super) async fn post_json<T: Serialize, R: DeserializeOwned>(
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

    pub(super) async fn post_text<T: Serialize>(
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

    pub(super) async fn get_json<R: DeserializeOwned>(
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

    pub(super) async fn get_text(
        &self,
        path: &str,
        betas: &[String],
    ) -> Result<String, ClientError> {
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
