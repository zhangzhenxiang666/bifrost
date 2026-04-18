//! HTTP client module for LLM service
//!
//! Provides a wrapper around reqwest::Client with support for
//! both non-streaming and streaming requests.

use http::{HeaderMap, StatusCode};
use rand::Rng;
use reqwest::{Client, Response};
use serde_json::Value;
use std::time::Duration;

/// Retry configuration for HTTP requests
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Maximum number of retry attempts
    pub max_retries: u32,
    /// Base delay for exponential backoff in milliseconds
    pub backoff_base_ms: u64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 5,
            backoff_base_ms: 700,
        }
    }
}

/// HTTP client wrapper with configurable timeout and retry
#[derive(Debug, Clone)]
pub struct HttpClient {
    inner: Client,
    #[expect(dead_code)]
    timeout_secs: u64,
    retry_config: RetryConfig,
}

impl HttpClient {
    /// Create a new HttpClient with the specified timeout in seconds and optional proxy
    ///
    /// # Panics
    /// Panics if the proxy URL is invalid or if the HTTP client fails to build.
    /// In production, use `try_new()` instead for proper error handling.
    pub fn new(timeout_secs: u64, proxy: Option<&str>) -> Self {
        Self::try_new(timeout_secs, proxy, RetryConfig::default())
            .expect("Failed to create HTTP client")
    }

    /// Create a new HttpClient with retry configuration
    pub fn with_retry(timeout_secs: u64, proxy: Option<&str>, retry_config: RetryConfig) -> Self {
        Self::try_new(timeout_secs, proxy, retry_config).expect("Failed to create HTTP client")
    }

    /// Create a new HttpClient with proper error handling
    pub fn try_new(
        timeout_secs: u64,
        proxy: Option<&str>,
        retry_config: RetryConfig,
    ) -> Result<Self, crate::error::LlmMapError> {
        let mut builder = Client::builder().timeout(Duration::from_secs(timeout_secs));

        if let Some(proxy_url) = proxy {
            let proxy = reqwest::Proxy::all(proxy_url).map_err(|e| {
                crate::error::LlmMapError::Config(format!(
                    "Invalid proxy URL '{}': {}",
                    proxy_url, e
                ))
            })?;
            builder = builder.proxy(proxy);
            builder = builder.danger_accept_invalid_certs(true);
        }

        let client = builder.build().map_err(|e| {
            crate::error::LlmMapError::Internal(anyhow::anyhow!(
                "Failed to build HTTP client: {}",
                e
            ))
        })?;
        Ok(Self {
            inner: client,
            timeout_secs,
            retry_config,
        })
    }

    /// Check if a status code indicates the request should be retried
    ///
    /// Returns true for:
    /// - 429 Too Many Requests
    /// - 500 Internal Server Error
    /// - 502 Bad Gateway
    /// - 503 Service Unavailable
    /// - 504 Gateway Timeout
    pub fn should_retry_status(status: StatusCode) -> bool {
        matches!(
            status,
            StatusCode::TOO_MANY_REQUESTS  // 429
                | StatusCode::INTERNAL_SERVER_ERROR  // 500
                | StatusCode::BAD_GATEWAY             // 502
                | StatusCode::SERVICE_UNAVAILABLE     // 503
                | StatusCode::GATEWAY_TIMEOUT // 504
        )
    }

    /// Check if an error is retryable (network errors, timeouts, etc.)
    pub fn is_retryable_error(error: &reqwest::Error) -> bool {
        if error.is_timeout() {
            return true;
        }
        if error.is_connect() {
            return true;
        }
        if error.is_request() {
            return true;
        }
        // Also retry on server errors (checked via status code in response)
        false
    }

    /// Calculate exponential backoff delay with jitter
    ///
    /// delay = base_ms * 2^attempt + random_jitter
    pub fn calculate_backoff(attempt: u32, base_ms: u64) -> Duration {
        let exponential_delay = base_ms * 2u64.saturating_pow(attempt.min(10));
        let jitter = rand::thread_rng().gen_range(0..exponential_delay / 2 + 1);
        Duration::from_millis(exponential_delay + jitter)
    }

    /// Send a non-streaming POST request with retry
    ///
    /// # Arguments
    /// * `url` - The target URL
    /// * `body` - JSON body to send
    /// * `headers` - HTTP headers to include
    ///
    /// # Returns
    /// The full response as a reqwest::Response
    pub async fn send_request(
        &self,
        url: &str,
        body: Value,
        headers: HeaderMap,
    ) -> Result<Response, reqwest::Error> {
        self.send_request_with_retry(url, body, headers).await
    }

    /// Send a request with exponential backoff retry
    async fn send_request_with_retry(
        &self,
        url: &str,
        body: Value,
        headers: HeaderMap,
    ) -> Result<Response, reqwest::Error> {
        self.send_request_fn(|client| client.post(url).headers(headers.clone()).json(&body))
            .await
    }

    /// Send a request with custom builder and exponential backoff retry
    ///
    /// # Arguments
    /// * `build_request` - A closure that takes a `&Client` and returns a `RequestBuilder`.
    ///   Called each retry attempt.
    ///
    /// # Example
    /// ```ignore
    /// client.send_request_fn(|client| {
    ///     client
    ///         .post("https://api.example.com/token")
    ///         .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
    ///         .body(urlencoded)
    /// }).await
    /// ```
    pub async fn send_request_fn<F>(&self, build_request: F) -> Result<Response, reqwest::Error>
    where
        F: Fn(&Client) -> reqwest::RequestBuilder,
    {
        let mut attempt = 0;

        loop {
            let response = build_request(&self.inner).send().await;

            match response {
                Ok(resp) => {
                    let status = resp.status();
                    if Self::should_retry_status(status) && attempt < self.retry_config.max_retries
                    {
                        attempt += 1;
                        let delay =
                            Self::calculate_backoff(attempt, self.retry_config.backoff_base_ms);
                        tracing::warn!(
                            "Request failed with status {}, retrying in {:?} (attempt {}/{})",
                            status,
                            delay,
                            attempt,
                            self.retry_config.max_retries
                        );
                        tokio::time::sleep(delay).await;
                        continue;
                    }
                    return Ok(resp);
                }
                Err(e) => {
                    if Self::is_retryable_error(&e) && attempt < self.retry_config.max_retries {
                        attempt += 1;
                        let delay =
                            Self::calculate_backoff(attempt, self.retry_config.backoff_base_ms);
                        tracing::warn!(
                            "Request error: {}, retrying in {:?} (attempt {}/{})",
                            e,
                            delay,
                            attempt,
                            self.retry_config.max_retries
                        );
                        tokio::time::sleep(delay).await;
                        continue;
                    }
                    return Err(e);
                }
            }
        }
    }
}

// Helper wrapper for stream state (deprecated)
// This enum is no longer used with the new eventsource-stream implementation

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn test_http_client_new() {
        let _client = HttpClient::new(600, None);
        // Just verify it can be created
        // HttpClient created successfully
    }

    #[tokio::test]
    async fn test_send_request_success() {
        // Start a mock server
        let mock_server = MockServer::start().await;

        let expected_response = json!({
            "status": "success",
            "data": "test data"
        });

        Mock::given(method("POST"))
            .and(path("/test"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&expected_response))
            .mount(&mock_server)
            .await;

        let client = HttpClient::new(600, None);
        let url = format!("{}/test", mock_server.uri());
        let body = json!({"query": "test"});
        let headers = HeaderMap::new();

        let response = client.send_request(&url, body, headers).await.unwrap();
        let response_json: Value = response.json().await.unwrap();

        assert_eq!(response_json, expected_response);
    }

    #[tokio::test]
    async fn test_send_request_with_headers() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/test"))
            .respond_with(ResponseTemplate::new(200).set_body_string("OK"))
            .mount(&mock_server)
            .await;

        let client = HttpClient::new(600, None);
        let url = format!("{}/test", mock_server.uri());
        let body = json!({"query": "test"});

        let mut headers = HeaderMap::new();
        headers.insert("Authorization", "Bearer test-token".parse().unwrap());
        headers.insert("Content-Type", "application/json".parse().unwrap());

        let response = client.send_request(&url, body, headers).await.unwrap();
        assert_eq!(response.status(), 200);
    }

    #[tokio::test]
    async fn test_timeout_configuration() {
        // Test that client can be created with different timeout values
        let _client_short = HttpClient::new(5, None);
        let _client_long = HttpClient::new(600, None);

        // Both should be usable
        // Both clients created successfully
    }

    #[tokio::test]
    async fn test_send_request_error_handling() {
        // Test with invalid URL
        let client = HttpClient::new(600, None);
        let body = json!({"query": "test"});
        let headers = HeaderMap::new();

        // This should fail with connection error
        let result = client
            .send_request(
                "http://invalid-url-that-does-not-exist:99999/test",
                body,
                headers,
            )
            .await;
        assert!(result.is_err());
    }
}
