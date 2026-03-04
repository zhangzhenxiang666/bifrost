//! Onion model executor for adapter chain processing
//!
//! This module provides the [`OnionExecutor`] which manages the execution of
//! multiple adapters in an onion architecture pattern:
//!
//! - Request flow: Adapter A → Adapter B → Adapter C → Upstream
//! - Response flow: Upstream → Adapter C → Adapter B → Adapter A → Client

use http::HeaderMap;

use crate::adapter::Adapter;
use crate::config::ProviderConfig;
use crate::error::{LlmMapError, Result};
use crate::types::{RequestTransform, ResponseTransform, StreamChunkTransform};

/// Executor that manages the adapter chain in an onion architecture.
///
/// The executor handles:
/// - Forward execution of request adapters (A → B → C)
/// - Reverse execution of response adapters (C → B → A)
/// - Response header passthrough (excluding content-length and transfer-encoding)
pub struct OnionExecutor {
    adapters: Vec<Box<dyn Adapter<Error = LlmMapError>>>,
    provider_config: ProviderConfig,
}

impl OnionExecutor {
    /// Create a new executor with the given adapter chain and provider configuration.
    ///
    /// # Arguments
    ///
    /// * `adapters` - Vector of adapters to execute in order
    /// * `provider_config` - The provider configuration (base_url, api_key, headers, body)
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use llm_map::adapter::{Adapter, OnionExecutor};
    /// use llm_map::config::{Endpoint, ProviderConfig};
    /// use llm_map::error::LlmMapError;
    /// # struct MyAdapter;
    /// # #[async_trait::async_trait]
    /// # impl Adapter for MyAdapter {
    /// #     type Error = LlmMapError;
    /// #     async fn transform_request(&self, body: serde_json::Value, provider_config: &ProviderConfig, headers: &http::HeaderMap) -> Result<llm_map::types::RequestTransform, Self::Error> { Ok(llm_map::types::RequestTransform::new(body)) }
    /// #     async fn transform_response(&self, body: serde_json::Value, status: http::StatusCode, headers: &http::HeaderMap) -> Result<llm_map::types::ResponseTransform, Self::Error> { Ok(llm_map::types::ResponseTransform::new(body)) }
    /// #     async fn transform_stream_chunk(&self, chunk: serde_json::Value) -> Result<llm_map::types::StreamChunkTransform, Self::Error> { Ok(llm_map::types::StreamChunkTransform::new(chunk)) }
    /// # }
    /// # let provider_config = ProviderConfig {
    /// #     base_url: "https://api.example.com".to_string(),
    /// #     api_key: "test-key".to_string(),
    /// #     endpoint: Endpoint::OpenAI,
    /// #     adapter: vec![],
    /// #     headers: None,
    /// #     body: None,
    /// #     models: None,
    /// # };
    /// let adapters: Vec<Box<dyn Adapter<Error = LlmMapError>>> = vec![
    ///     Box::new(MyAdapter),
    /// ];
    /// let executor = OnionExecutor::new(adapters, provider_config);
    /// ```
    pub fn new(
        adapters: Vec<Box<dyn Adapter<Error = LlmMapError>>>,
        provider_config: ProviderConfig,
    ) -> Self {
        Self {
            adapters,
            provider_config,
        }
    }

    /// Execute the request transformation through the adapter chain.
    ///
    /// This method executes adapters in **forward order** (A → B → C),
    /// where each adapter's output becomes the next adapter's input.
    ///
    /// # Arguments
    ///
    /// * `body` - The request body as JSON
    /// * `headers` - The HTTP headers
    ///
    /// # Returns
    ///
    /// The final transformed request after passing through all adapters.
    /// The URL is automatically set from `provider_config.base_url`.
    ///
    /// # Flow
    ///
    /// ```text
    /// Original → Adapter A → Adapter B → Adapter C → Final
    /// ```
    pub async fn execute_request(
        &self,
        body: serde_json::Value,
        headers: &http::HeaderMap,
    ) -> Result<RequestTransform> {
        let mut current_body = body;
        let mut current_url = self.provider_config.base_url.clone();
        let mut current_headers = HeaderMap::new();

        // Forward execution: A → B → C
        for adapter in &self.adapters {
            let transform = adapter
                .transform_request(current_body, &self.provider_config, headers)
                .await
                .map_err(|e| LlmMapError::Adapter(e.to_string()))?;

            current_body = transform.body;
            if let Some(new_url) = transform.url {
                current_url = new_url;
            }
            if let Some(new_headers) = transform.headers {
                current_headers.extend(new_headers);
            }
        }

        Ok(RequestTransform::new(current_body)
            .with_url(current_url)
            .with_headers(current_headers))
    }

    /// Execute the response transformation through the adapter chain.
    ///
    /// This method executes adapters in **reverse order** (C → B → A),
    /// where each adapter's output becomes the next adapter's input.
    ///
    /// # Arguments
    ///
    /// * `body` - The response body as JSON
    /// * `status` - The HTTP status code
    /// * `upstream_headers` - The headers from the upstream response
    ///
    /// # Returns
    ///
    /// The final transformed response after passing through all adapters.
    ///
    /// # Flow
    ///
    /// ```text
    /// Upstream → Adapter C → Adapter B → Adapter A → Final
    /// ```
    ///
    /// # Header Passthrough
    ///
    /// This method implements header passthrough logic:
    /// - ✅ **Passthrough**: `x-ratelimit-*`, `retry-after`, `x-request-id`, and all other upstream headers
    /// - ❌ **Excluded**: `content-length` and `transfer-encoding` (will be recalculated by axum)
    pub async fn execute_response(
        &self,
        body: serde_json::Value,
        status: http::StatusCode,
        upstream_headers: &http::HeaderMap,
    ) -> Result<ResponseTransform> {
        let mut current_body = body;
        let mut current_status = status;
        let mut current_headers = http::HeaderMap::new();

        // Header passthrough: copy all upstream headers except content-length and transfer-encoding
        for (key, value) in upstream_headers {
            let key_name = key.as_str();
            if key_name != "content-length" && key_name != "transfer-encoding" {
                current_headers.insert(key, value.clone());
            }
        }

        // Reverse execution: C → B → A
        for adapter in self.adapters.iter().rev() {
            let transform = adapter
                .transform_response(current_body, current_status, upstream_headers)
                .await
                .map_err(|e| LlmMapError::Adapter(e.to_string()))?;

            current_body = transform.body;
            if let Some(new_status) = transform.status {
                current_status = new_status;
            }
            if let Some(new_headers) = transform.headers {
                // Merge adapter headers, allowing them to override passthrough headers
                for (key, value) in new_headers {
                    if let Some(key) = key {
                        current_headers.insert(key, value);
                    }
                }
            }
        }

        Ok(ResponseTransform::new(current_body)
            .with_status(current_status)
            .with_headers(current_headers))
    }

    pub async fn execute_stream_chunk(
        &self,
        chunk: serde_json::Value,
    ) -> Result<StreamChunkTransform> {
        let mut current_chunk = chunk;
        let mut current_event = None;
        for adapter in self.adapters.iter().rev() {
            let transform = adapter
                .transform_stream_chunk(current_chunk)
                .await
                .map_err(|e| LlmMapError::Adapter(e.to_string()))?;
            current_chunk = transform.data;
            current_event = transform.event;
        }
        let mut res = StreamChunkTransform::new(current_chunk);
        res.event = current_event;
        Ok(res)
    }

    /// Get the number of adapters in the chain.
    pub fn adapter_count(&self) -> usize {
        self.adapters.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    /// Helper function to create a test provider config
    fn test_provider_config() -> ProviderConfig {
        ProviderConfig {
            base_url: "https://example.com".to_string(),
            api_key: "test-key".to_string(),
            endpoint: crate::config::Endpoint::OpenAI,
            adapter: vec![],
            headers: None,
            body: None,
            models: None,
        }
    }

    /// Mock adapter for testing that records execution order
    struct MockAdapter {
        name: &'static str,
        execution_log: Arc<Mutex<Vec<String>>>,
    }

    impl MockAdapter {
        fn new(name: &'static str, log: Arc<Mutex<Vec<String>>>) -> Self {
            Self {
                name,
                execution_log: log,
            }
        }
    }

    #[async_trait]
    impl Adapter for MockAdapter {
        type Error = LlmMapError;

        async fn transform_request(
            &self,
            body: serde_json::Value,
            _provider_config: &ProviderConfig,
            _headers: &http::HeaderMap,
        ) -> Result<RequestTransform> {
            self.execution_log
                .lock()
                .await
                .push(format!("{}_request", self.name));
            Ok(RequestTransform::new(body))
        }

        async fn transform_response(
            &self,
            body: serde_json::Value,
            status: http::StatusCode,
            _headers: &http::HeaderMap,
        ) -> Result<ResponseTransform> {
            self.execution_log
                .lock()
                .await
                .push(format!("{}_response", self.name));
            Ok(ResponseTransform::new(body).with_status(status))
        }

        async fn transform_stream_chunk(
            &self,
            chunk: serde_json::Value,
        ) -> Result<crate::types::StreamChunkTransform> {
            Ok(crate::types::StreamChunkTransform::new(chunk))
        }
    }

    #[tokio::test]
    async fn test_request_execution_order() {
        let log = Arc::new(Mutex::new(Vec::new()));

        let adapters: Vec<Box<dyn Adapter<Error = LlmMapError>>> = vec![
            Box::new(MockAdapter::new("A", log.clone())),
            Box::new(MockAdapter::new("B", log.clone())),
            Box::new(MockAdapter::new("C", log.clone())),
        ];

        let provider_config = ProviderConfig {
            base_url: "https://example.com".to_string(),
            api_key: "test-key".to_string(),
            endpoint: crate::config::Endpoint::OpenAI,
            adapter: vec![],
            headers: None,
            body: None,
            models: None,
        };
        let executor = OnionExecutor::new(adapters, provider_config);
        let body = serde_json::json!({"test": "data"});
        let headers = http::HeaderMap::new();

        executor.execute_request(body, &headers).await.unwrap();

        let execution_order = log.lock().await;

        // Assert forward execution: A → B → C
        assert_eq!(
            *execution_order,
            vec!["A_request", "B_request", "C_request"]
        );
    }

    #[tokio::test]
    async fn test_response_execution_order() {
        let log = Arc::new(Mutex::new(Vec::new()));

        let adapters: Vec<Box<dyn Adapter<Error = LlmMapError>>> = vec![
            Box::new(MockAdapter::new("A", log.clone())),
            Box::new(MockAdapter::new("B", log.clone())),
            Box::new(MockAdapter::new("C", log.clone())),
        ];

        let executor = OnionExecutor::new(adapters, test_provider_config());
        let body = serde_json::json!({"result": "ok"});
        let mut headers = http::HeaderMap::new();
        headers.insert("content-type", "application/json".parse().unwrap());

        executor
            .execute_response(body, http::StatusCode::OK, &headers)
            .await
            .unwrap();

        let execution_order = log.lock().await;

        // Assert reverse execution: C → B → A
        assert_eq!(
            *execution_order,
            vec!["C_response", "B_response", "A_response"]
        );
    }

    #[tokio::test]
    async fn test_header_passthrough() {
        let log = Arc::new(Mutex::new(Vec::new()));

        let adapters: Vec<Box<dyn Adapter<Error = LlmMapError>>> =
            vec![Box::new(MockAdapter::new("A", log.clone()))];

        let executor = OnionExecutor::new(adapters, test_provider_config());
        let body = serde_json::json!({"result": "ok"});

        // Create upstream headers with various types
        let mut upstream_headers = http::HeaderMap::new();
        upstream_headers.insert("content-type", "application/json".parse().unwrap());
        upstream_headers.insert("x-ratelimit-limit", "100".parse().unwrap());
        upstream_headers.insert("x-ratelimit-remaining", "50".parse().unwrap());
        upstream_headers.insert("x-ratelimit-reset", "3600".parse().unwrap());
        upstream_headers.insert("retry-after", "60".parse().unwrap());
        upstream_headers.insert("x-request-id", "req-123".parse().unwrap());
        upstream_headers.insert("content-length", "1234".parse().unwrap());
        upstream_headers.insert("transfer-encoding", "chunked".parse().unwrap());

        let result = executor
            .execute_response(body, http::StatusCode::OK, &upstream_headers)
            .await
            .unwrap();

        let response_headers = result.headers.unwrap();

        // Assert passthrough headers are present
        assert_eq!(
            response_headers.get("content-type").unwrap(),
            "application/json"
        );
        assert_eq!(response_headers.get("x-ratelimit-limit").unwrap(), "100");
        assert_eq!(response_headers.get("x-ratelimit-remaining").unwrap(), "50");
        assert_eq!(response_headers.get("x-ratelimit-reset").unwrap(), "3600");
        assert_eq!(response_headers.get("retry-after").unwrap(), "60");
        assert_eq!(response_headers.get("x-request-id").unwrap(), "req-123");

        // Assert excluded headers are NOT present
        assert!(response_headers.get("content-length").is_none());
        assert!(response_headers.get("transfer-encoding").is_none());
    }

    #[tokio::test]
    async fn test_full_onion_flow() {
        let log = Arc::new(Mutex::new(Vec::new()));

        let adapters: Vec<Box<dyn Adapter<Error = LlmMapError>>> = vec![
            Box::new(MockAdapter::new("A", log.clone())),
            Box::new(MockAdapter::new("B", log.clone())),
            Box::new(MockAdapter::new("C", log.clone())),
        ];

        let executor = OnionExecutor::new(adapters, test_provider_config());

        // Execute request (forward: A → B → C)
        let request_body = serde_json::json!({"message": "hello"});
        let request_headers = http::HeaderMap::new();
        executor
            .execute_request(request_body, &request_headers)
            .await
            .unwrap();

        // Execute response (reverse: C → B → A)
        let response_body = serde_json::json!({"choices": []});
        let mut response_headers = http::HeaderMap::new();
        response_headers.insert("x-ratelimit-limit", "1000".parse().unwrap());
        executor
            .execute_response(response_body, http::StatusCode::OK, &response_headers)
            .await
            .unwrap();

        let execution_order = log.lock().await;

        // Assert complete onion flow
        assert_eq!(
            *execution_order,
            vec![
                "A_request",
                "B_request",
                "C_request",
                "C_response",
                "B_response",
                "A_response"
            ]
        );
    }

    #[tokio::test]
    async fn test_empty_adapter_chain() {
        let executor = OnionExecutor::new(vec![], test_provider_config());

        let request_body = serde_json::json!({"test": "data"});
        let request_headers = http::HeaderMap::new();

        let result = executor
            .execute_request(request_body.clone(), &request_headers)
            .await
            .unwrap();

        assert_eq!(result.body, request_body);
        assert_eq!(result.url, Some("https://example.com".to_string()));
    }

    #[tokio::test]
    async fn test_adapter_count() {
        let log = Arc::new(Mutex::new(Vec::new()));

        let adapters: Vec<Box<dyn Adapter<Error = LlmMapError>>> = vec![
            Box::new(MockAdapter::new("A", log.clone())),
            Box::new(MockAdapter::new("B", log.clone())),
        ];

        let executor = OnionExecutor::new(adapters, test_provider_config());
        assert_eq!(executor.adapter_count(), 2);
    }
}
