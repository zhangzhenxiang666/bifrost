//! Passthrough adapter - passes data through without modification
//!
//! This adapter is used when no transformation is needed. It simply passes
//! the original data through unchanged.

use async_trait::async_trait;
use crate::adapter::Adapter;
use crate::config::ProviderConfig;
use crate::error::LlmMapError;
use crate::types::{RequestTransform, ResponseTransform, StreamChunkTransform};

/// Passthrough adapter that does not modify any data.
///
/// This adapter is useful when:
/// - No transformation is needed
/// - You want to use the raw provider API directly
/// - For testing purposes
pub struct PassthroughAdapter;

#[async_trait]
impl Adapter for PassthroughAdapter {
    type Error = LlmMapError;

    async fn transform_request(
        &self,
        body: serde_json::Value,
        _provider_config: &ProviderConfig,
        _headers: &http::HeaderMap,
    ) -> Result<RequestTransform, Self::Error> {
        Ok(RequestTransform::new(body))
    }

    async fn transform_response(
        &self,
        body: serde_json::Value,
        _status: http::StatusCode,
        _headers: &http::HeaderMap,
    ) -> Result<ResponseTransform, Self::Error> {
        Ok(ResponseTransform::new(body))
    }

    async fn transform_stream_chunk(
        &self,
        chunk: serde_json::Value,
    ) -> Result<StreamChunkTransform, Self::Error> {
        Ok(StreamChunkTransform::new(chunk))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::HeaderMap;

    #[tokio::test]
    async fn test_passthrough_request_not_modified() {
        let adapter = PassthroughAdapter;
        let body = serde_json::json!({
            "model": "gpt-4",
            "messages": [
                {"role": "user", "content": "Hello"}
            ]
        });
        let config = ProviderConfig {
            base_url: "https://api.example.com".to_string(),
            api_key: "sk-test".to_string(),
            endpoint: "openai".to_string(),
            adapter: vec![],
            headers: vec![],
            body: vec![],
            models: vec![],
        };
        let headers = HeaderMap::new();

        let result = adapter
            .transform_request(body.clone(), &config, &headers)
            .await
            .unwrap();

        assert_eq!(result.body, body);
        assert!(result.url.is_none());
        assert!(result.headers.is_none());
    }

    #[tokio::test]
    async fn test_passthrough_response_not_modified() {
        let adapter = PassthroughAdapter;
        let body = serde_json::json!({
            "id": "chatcmpl-123",
            "choices": [
                {
                    "message": {"role": "assistant", "content": "Hi there!"}
                }
            ]
        });
        let status = http::StatusCode::OK;
        let headers = HeaderMap::new();

        let result = adapter
            .transform_response(body.clone(), status, &headers)
            .await
            .unwrap();

        assert_eq!(result.body, body);
        assert!(result.status.is_none());
        assert!(result.headers.is_none());
    }

    #[tokio::test]
    async fn test_passthrough_stream_chunk_not_modified() {
        let adapter = PassthroughAdapter;
        let chunk = serde_json::json!({
            "id": "chatcmpl-123",
            "choices": [
                {
                    "delta": {"content": "Hello"}
                }
            ]
        });

        let result = adapter
            .transform_stream_chunk(chunk.clone())
            .await
            .unwrap();

        assert_eq!(result.data, chunk);
        assert!(result.event.is_none());
    }
}
