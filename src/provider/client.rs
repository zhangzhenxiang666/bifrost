//! HTTP client module for LLM service
//!
//! Provides a wrapper around reqwest::Client with support for
//! both non-streaming and streaming requests.

use eventsource_stream::{EventStreamError, Eventsource};
use futures::stream::Stream;

use http::HeaderMap;
use reqwest::{Client, Response};
use serde_json::Value;

/// HTTP client wrapper with configurable timeout
#[derive(Clone)]
pub struct HttpClient {
    client: Client,
}

impl HttpClient {
    /// Create a new HttpClient with the specified timeout in seconds and optional proxy
    pub fn new(timeout_secs: u64, proxy: Option<&str>) -> Self {
        let mut builder = Client::builder().timeout(std::time::Duration::from_secs(timeout_secs));

        if let Some(proxy_url) = proxy {
            builder =
                builder.proxy(reqwest::Proxy::all(proxy_url).expect("Failed to parse proxy URL"));
            builder = builder.danger_accept_invalid_certs(true);
        }

        let client = builder.build().expect("Failed to build HTTP client");
        Self { client }
    }

    /// Send a non-streaming POST request
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
        self.client
            .post(url)
            .headers(headers)
            .json(&body)
            .send()
            .await
    }

    /// Send a streaming POST request and process SSE events
    ///
    /// # Arguments
    /// * `url` - The target URL
    /// * `body` - JSON body to send
    /// * `headers` - HTTP headers to include
    ///
    /// # Returns
    /// A stream of SSE events
    /// Send a streaming POST request and process SSE events
    ///
    /// # Arguments
    /// * `url` - The target URL
    /// * `body` - JSON body to send
    /// * `headers` - HTTP headers to include
    ///
    /// # Returns
    /// A Result containing a stream of SSE events, or reqwest::Error on request failure
    pub async fn send_sse_stream(
        &self,
        url: String,
        body: Value,
        mut headers: HeaderMap,
    ) -> Result<
        impl Stream<Item = Result<eventsource_stream::Event, EventStreamError<reqwest::Error>>> + use<>,
        reqwest::Error,
    > {
        use http::header::ACCEPT;

        headers.insert(ACCEPT, "text/event-stream".parse().unwrap());

        let response = self
            .client
            .post(url)
            .headers(headers)
            .json(&body)
            .send()
            .await?;

        Ok(response.bytes_stream().eventsource())
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
