use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use std::time::Instant;

use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, CONTENT_TYPE, RETRY_AFTER};
use reqwest::{Client, Method, StatusCode};
use serde::de::DeserializeOwned;
use serde::Serialize;
use tokio::sync::{Mutex as AsyncMutex, OwnedSemaphorePermit, Semaphore};
use tracing::{debug, trace, warn};

use crate::ClientError;

use super::types::AnthropicProviderConfig;

const SERVER_ERROR_RETRY_ATTEMPTS: u32 = 2;
const RATE_LIMIT_RETRY_ATTEMPTS: u32 = 3;

#[derive(Debug, Clone)]
pub(super) struct AnthropicTransport {
    provider_name: &'static str,
    config: AnthropicProviderConfig,
    http_client: Client,
    request_gate: Arc<ProviderRequestGate>,
}

#[derive(Debug)]
struct ProviderRequestGate {
    request_throttle_interval: Duration,
    semaphore: Arc<Semaphore>,
    last_request_started_at: AsyncMutex<Option<Instant>>,
}

impl ProviderRequestGate {
    fn new(config: &AnthropicProviderConfig) -> Self {
        Self {
            request_throttle_interval: config.request_throttle_interval,
            semaphore: Arc::new(Semaphore::new(config.max_concurrent_requests.max(1))),
            last_request_started_at: AsyncMutex::new(None),
        }
    }

    async fn acquire(&self) -> OwnedSemaphorePermit {
        let permit = self
            .semaphore
            .clone()
            .acquire_owned()
            .await
            .expect("provider request semaphore should remain open");
        self.wait_for_request_slot().await;
        permit
    }

    async fn wait_for_request_slot(&self) {
        let mut last_request_started_at = self.last_request_started_at.lock().await;
        if let Some(previous) = *last_request_started_at {
            let elapsed = previous.elapsed();
            if elapsed < self.request_throttle_interval {
                tokio::time::sleep(self.request_throttle_interval - elapsed).await;
            }
        }
        *last_request_started_at = Some(Instant::now());
    }
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
        let request_gate = provider_request_gate(provider_name, &config);
        Ok(Self {
            provider_name,
            config,
            http_client,
            request_gate,
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

        let headers = self.build_headers(betas, false)?;
        let endpoint = self.endpoint(path);
        let (status, body) = self
            .send_text_response_with_retry(path, move || {
                self.http_client
                    .request(Method::POST, endpoint.clone())
                    .headers(headers.clone())
                    .json(&body_value)
            })
            .await?;
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

        let headers = self.build_headers(betas, accept_sse)?;
        let endpoint = self.endpoint(path);
        let (status, body) = self
            .send_text_response_with_retry(path, move || {
                self.http_client
                    .request(Method::POST, endpoint.clone())
                    .headers(headers.clone())
                    .json(&body_value)
            })
            .await?;
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
        let headers = self.build_headers(betas, false)?;
        let endpoint = self.endpoint(path);
        let (status, body) = self
            .send_text_response_with_retry(path, move || {
                self.http_client
                    .request(Method::GET, endpoint.clone())
                    .headers(headers.clone())
            })
            .await?;
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
        let headers = self.build_headers(betas, false)?;
        let endpoint = self.endpoint(path);
        let (status, body) = self
            .send_text_response_with_retry(path, move || {
                self.http_client
                    .request(Method::GET, endpoint.clone())
                    .headers(headers.clone())
            })
            .await?;
        debug!(provider = self.provider_name, endpoint = path, response_json = %body);

        if !status.is_success() {
            return Err(ClientError::Api { status, body });
        }

        Ok(body)
    }

    async fn send_text_response_with_retry<F>(
        &self,
        path: &str,
        mut build_request: F,
    ) -> Result<(StatusCode, String), ClientError>
    where
        F: FnMut() -> reqwest::RequestBuilder,
    {
        let mut attempt = 0u32;
        loop {
            let _permit = self.request_gate.acquire().await;
            let response = build_request().send().await?;
            let status = response.status();
            let retry_after = retry_after_delay(response.headers());
            let body = response.text().await.unwrap_or_default();

            if status.is_server_error() && attempt < SERVER_ERROR_RETRY_ATTEMPTS {
                attempt += 1;
                warn!(
                    provider = self.provider_name,
                    endpoint = path,
                    status = %status,
                    attempt,
                    "retrying provider request after server error"
                );
                tokio::time::sleep(Duration::from_millis(u64::from(attempt) * 100)).await;
                continue;
            }

            if status == StatusCode::TOO_MANY_REQUESTS && attempt < RATE_LIMIT_RETRY_ATTEMPTS {
                attempt += 1;
                let delay = rate_limit_retry_delay(retry_after, self.config.rate_limit_retry_delay);
                warn!(
                    provider = self.provider_name,
                    endpoint = path,
                    status = %status,
                    attempt,
                    delay_ms = delay.as_millis(),
                    "retrying provider request after rate limit"
                );
                tokio::time::sleep(delay).await;
                continue;
            }

            return Ok((status, body));
        }
    }
}

fn retry_after_delay(headers: &HeaderMap) -> Option<Duration> {
    let retry_after = headers.get(RETRY_AFTER)?.to_str().ok()?.trim();
    let seconds = retry_after.parse::<u64>().ok()?;
    Some(Duration::from_secs(seconds))
}

fn rate_limit_retry_delay(retry_after: Option<Duration>, configured_delay: Duration) -> Duration {
    retry_after
        .map(|delay| delay.max(configured_delay))
        .unwrap_or(configured_delay)
}

fn provider_request_gate(
    provider_name: &'static str,
    config: &AnthropicProviderConfig,
) -> Arc<ProviderRequestGate> {
    static GATES: OnceLock<Mutex<HashMap<String, Arc<ProviderRequestGate>>>> = OnceLock::new();

    let key = format!(
        "{}|{}|{}|{}",
        provider_name,
        config.base_url,
        config.request_throttle_interval.as_millis(),
        config.max_concurrent_requests,
    );
    let registry = GATES.get_or_init(|| Mutex::new(HashMap::new()));
    let mut registry = registry.lock().expect("provider gate registry mutex poisoned");
    registry
        .entry(key)
        .or_insert_with(|| Arc::new(ProviderRequestGate::new(config)))
        .clone()
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::Instant;

    use serde_json::json;

    use super::*;

    fn test_config(address: std::net::SocketAddr) -> AnthropicProviderConfig {
        AnthropicProviderConfig::new(
            "key".to_string(),
            "model".to_string(),
            format!("http://{}", address),
            "2023-06-01".to_string(),
        )
        .with_request_throttle_interval(Duration::ZERO)
        .with_rate_limit_retry_delay(Duration::ZERO)
    }

    #[tokio::test]
    async fn post_json_retries_server_errors_before_succeeding() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let attempts = Arc::new(Mutex::new(0usize));
        let attempts_for_thread = Arc::clone(&attempts);

        let server = thread::spawn(move || {
            for response_index in 0..3 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut buffer = [0u8; 4096];
                let _ = stream.read(&mut buffer).unwrap();
                *attempts_for_thread.lock().unwrap() += 1;

                let response = if response_index < 2 {
                    "HTTP/1.1 500 Internal Server Error\r\nContent-Length: 5\r\nConnection: close\r\n\r\nerror"
                        .to_string()
                } else {
                    let body = json!({"id":"msg-1"}).to_string();
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    )
                };
                stream.write_all(response.as_bytes()).unwrap();
                stream.flush().unwrap();
            }
        });

        let config = test_config(address);
        let transport = AnthropicTransport::new("test", config).unwrap();
        let response: serde_json::Value = transport
            .post_json("/v1/messages", &json!({"ping": true}), &[])
            .await
            .unwrap();

        assert_eq!(response["id"], "msg-1");
        assert_eq!(*attempts.lock().unwrap(), 3);
        server.join().unwrap();
    }

    #[tokio::test]
    async fn post_json_retries_rate_limits_before_succeeding() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let attempts = Arc::new(Mutex::new(0usize));
        let attempts_for_thread = Arc::clone(&attempts);

        let server = thread::spawn(move || {
            for response_index in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut buffer = [0u8; 4096];
                let _ = stream.read(&mut buffer).unwrap();
                *attempts_for_thread.lock().unwrap() += 1;

                let response = if response_index == 0 {
                    "HTTP/1.1 429 Too Many Requests\r\nRetry-After: 0\r\nContent-Length: 5\r\nConnection: close\r\n\r\nerror"
                        .to_string()
                } else {
                    let body = json!({"id":"msg-2"}).to_string();
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    )
                };
                stream.write_all(response.as_bytes()).unwrap();
                stream.flush().unwrap();
            }
        });

        let config = test_config(address);
        let transport = AnthropicTransport::new("test", config).unwrap();
        let response: serde_json::Value = transport
            .post_json("/v1/messages", &json!({"ping": true}), &[])
            .await
            .unwrap();

        assert_eq!(response["id"], "msg-2");
        assert_eq!(*attempts.lock().unwrap(), 2);
        server.join().unwrap();
    }

    #[test]
    fn configured_retry_delay_is_a_floor_over_retry_after() {
        assert_eq!(
            rate_limit_retry_delay(Some(Duration::from_secs(0)), Duration::from_secs(10)),
            Duration::from_secs(10)
        );
        assert_eq!(
            rate_limit_retry_delay(Some(Duration::from_secs(15)), Duration::from_secs(10)),
            Duration::from_secs(15)
        );
        assert_eq!(
            rate_limit_retry_delay(None, Duration::from_secs(10)),
            Duration::from_secs(10)
        );
    }

    #[tokio::test]
    async fn provider_request_gate_limits_concurrency() {
        let gate = Arc::new(ProviderRequestGate::new(
            &AnthropicProviderConfig::new("key", "model", "http://example.com", "2023-06-01")
                .with_request_throttle_interval(Duration::ZERO)
                .with_max_concurrent_requests(1),
        ));

        let first = gate.acquire().await;
        let completed = Arc::new(AtomicBool::new(false));
        let completed_ref = Arc::clone(&completed);
        let gate_ref = Arc::clone(&gate);
        let second_request = tokio::spawn(async move {
            let _second = gate_ref.acquire().await;
            completed_ref.store(true, Ordering::SeqCst);
        });

        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(!completed.load(Ordering::SeqCst));
        drop(first);
        second_request
            .await
            .expect("second request should proceed after permit release");
        assert!(completed.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn provider_request_gate_throttles_request_starts() {
        let gate = ProviderRequestGate::new(
            &AnthropicProviderConfig::new("key", "model", "http://example.com", "2023-06-01")
                .with_request_throttle_interval(Duration::from_millis(30))
                .with_max_concurrent_requests(2),
        );

        let _first = gate.acquire().await;
        let started = Instant::now();
        let _second = gate.acquire().await;

        assert!(started.elapsed() >= Duration::from_millis(25));
    }
}
