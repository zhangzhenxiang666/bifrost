//! HTTP client module for LLM service
//!
//! Provides a wrapper around reqwest::Client with support for
//! both non-streaming and streaming requests.

use bytes::Bytes;
use eventsource_stream::{Eventsource, EventStreamError};
use futures::stream::Stream;
use futures::StreamExt;
use http::HeaderMap;
use reqwest::{Client, Response};
use serde_json::Value;

/// HTTP client wrapper with configurable timeout
pub struct HttpClient {
    client: Client,
}

impl HttpClient {
    /// Create a new HttpClient with the specified timeout in seconds
    pub fn new(timeout_secs: u64) -> Self {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(timeout_secs))
            .build()
            .expect("Failed to build HTTP client");
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
        headers: &HeaderMap,
    ) -> Result<Response, reqwest::Error> {
        self.client
            .post(url)
            .headers(headers.clone())
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
    pub async fn send_sse_stream(
        &self,
        url: &str,
        body: Value,
        headers: &HeaderMap,
    ) -> impl Stream<Item = Result<eventsource_stream::Event, EventStreamError<reqwest::Error>>> {
        use http::header::ACCEPT;

        let mut headers = headers.clone();
        headers.insert(ACCEPT, "text/event-stream".parse().unwrap());

        let response = self
            .client
            .post(url)
            .headers(headers)
            .json(&body)
            .send()
            .await;

        match response {
            Ok(resp) => resp.bytes_stream().eventsource().boxed(),
            Err(e) => {
                // Return a stream that immediately yields an error
                // Use Transport variant to wrap reqwest::Error
                futures::stream::once(async move {
                    Err(EventStreamError::Transport(e))
                })
                .boxed()
            }
        }
    }

    /// Send a streaming POST request (legacy API, returns raw bytes stream)
    ///
    /// # Arguments
    /// * `url` - The target URL
    /// * `body` - JSON body to send
    /// * `headers` - HTTP headers to include
    ///
    /// # Returns
    /// A stream of response chunks as Bytes
    pub async fn send_stream(
        &self,
        url: &str,
        body: Value,
        headers: &HeaderMap,
    ) -> impl Stream<Item = Result<Bytes, reqwest::Error>> {
        let response = self
            .client
            .post(url)
            .headers(headers.clone())
            .json(&body)
            .send()
            .await;

        match response {
            Ok(resp) => {
                let stream = resp.bytes_stream();
                futures::stream::unfold(Some(stream), |state_option| async {
                    if let Some(mut stream) = state_option {
                        if let Some(chunk_result) = stream.next().await {
                            return Some((chunk_result, Some(stream)));
                        }
                    }
                    None
                })
                .boxed()
            }
            Err(e) => {
                // Return a stream that immediately yields an error
                futures::stream::once(async move { Err(e) }).boxed()
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
    use std::time::Duration;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn test_http_client_new() {
        let client = HttpClient::new(600);
        // Just verify it can be created
        assert!(true);
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

        let client = HttpClient::new(600);
        let url = format!("{}/test", mock_server.uri());
        let body = json!({"query": "test"});
        let headers = HeaderMap::new();

        let response = client.send_request(&url, body, &headers).await.unwrap();
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

        let client = HttpClient::new(600);
        let url = format!("{}/test", mock_server.uri());
        let body = json!({"query": "test"});

        let mut headers = HeaderMap::new();
        headers.insert("Authorization", "Bearer test-token".parse().unwrap());
        headers.insert("Content-Type", "application/json".parse().unwrap());

        let response = client.send_request(&url, body, &headers).await.unwrap();
        assert_eq!(response.status(), 200);
    }

    #[tokio::test]
    async fn test_send_stream_basic() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/stream"))
            .respond_with(ResponseTemplate::new(200).set_body_string("chunk1chunk2chunk3"))
            .mount(&mock_server)
            .await;

        let client = HttpClient::new(600);
        let url = format!("{}/stream", mock_server.uri());
        let body = json!({"query": "stream test"});
        let headers = HeaderMap::new();

        let stream = client.send_stream(&url, body, &headers).await;
        let chunks: Vec<_> = stream.collect().await;

        // Verify we received chunks
        assert!(!chunks.is_empty());
    }

    #[tokio::test]
    async fn test_timeout_configuration() {
        // Test that client can be created with different timeout values
        let client_short = HttpClient::new(5);
        let client_long = HttpClient::new(600);

        // Both should be usable
        assert!(true);
    }

    #[tokio::test]
    async fn test_send_request_error_handling() {
        // Test with invalid URL
        let client = HttpClient::new(600);
        let body = json!({"query": "test"});
        let headers = HeaderMap::new();

        // This should fail with connection error
        let result = client.send_request("http://invalid-url-that-does-not-exist:99999/test", body, &headers).await;
        assert!(result.is_err());
    }
}
