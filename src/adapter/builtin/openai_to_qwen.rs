//! OpenAI to Qwen adapter - adds Qwen-specific headers for OpenAI-compatible API
//!
//! This adapter transforms OpenAI-compatible requests to Qwen API format by adding
//! the required headers and handling stream options.

use async_trait::async_trait;
use crate::adapter::Adapter;
use crate::config::ProviderConfig;
use crate::error::LlmMapError;
use crate::types::{RequestTransform, ResponseTransform, StreamChunkTransform};

/// OpenAI to Qwen adapter that adds Qwen-specific headers.
///
/// Qwen API is OpenAI-compatible but requires specific headers:
/// - `User-Agent`: QwenCode user agent
/// - `X-DashScope-*`: Qwen-specific headers for caching and auth
///
/// For streaming requests, adds `stream_options.include_usage: true`
pub struct OpenAIToQwenAdapter;

#[async_trait]
impl Adapter for OpenAIToQwenAdapter {
    type Error = LlmMapError;

    /// Transform OpenAI-compatible request to Qwen API format.
    ///
    /// Adds:
    /// - Qwen-specific headers (User-Agent, X-DashScope-*)
    /// - For streaming: `stream_options.include_usage: true`
    async fn transform_request(
        &self,
        mut body: serde_json::Value,
        _provider_config: &ProviderConfig,
        _headers: &http::HeaderMap,
    ) -> Result<RequestTransform, Self::Error> {
        // For streaming requests, add stream_options to include usage
        if let Some(stream) = body.get("stream").and_then(|v| v.as_bool()) {
            if stream {
                if let Some(obj) = body.as_object_mut() {
                    obj.insert(
                        "stream_options".to_string(),
                        serde_json::json!({
                            "include_usage": true
                        }),
                    );
                }
            }
        }

        // Add Qwen-specific headers
        let mut headers = http::HeaderMap::new();
        headers.insert(
            "Content-Type",
            "application/json".parse().unwrap(),
        );
        headers.insert(
            "User-Agent",
            "QwenCode/0.11.0 (linux; x64)".parse().unwrap(),
        );
        headers.insert(
            "Accept",
            "application/json".parse().unwrap(),
        );
        headers.insert(
            "X-DashScope-CacheControl",
            "enable".parse().unwrap(),
        );
        headers.insert(
            "X-DashScope-UserAgent",
            "QwenCode/0.11.0 (linux; x64)".parse().unwrap(),
        );
        headers.insert(
            "X-DashScope-AuthType",
            "qwen-oauth".parse().unwrap(),
        );

        Ok(RequestTransform::new(body).with_headers(headers))
    }

    /// Transform response (passthrough - Qwen returns OpenAI-compatible format).
    async fn transform_response(
        &self,
        body: serde_json::Value,
        _status: http::StatusCode,
        _headers: &http::HeaderMap,
    ) -> Result<ResponseTransform, Self::Error> {
        // Qwen API returns OpenAI-compatible format, so passthrough
        Ok(ResponseTransform::new(body))
    }

    /// Transform stream chunk (passthrough - Qwen returns OpenAI-compatible format).
    async fn transform_stream_chunk(
        &self,
        chunk: serde_json::Value,
    ) -> Result<StreamChunkTransform, Self::Error> {
        // Qwen API returns OpenAI-compatible format, so passthrough
        Ok(StreamChunkTransform::new(chunk))
    }
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use http::HeaderMap;

    fn create_test_config() -> ProviderConfig {
        ProviderConfig {
            base_url: "https://portal.qwen.ai/v1".to_string(),
            api_key: "any-key".to_string(),
            endpoint: crate::config::Endpoint::Qwen,
            adapter: vec![],
            headers: vec![],
            body: vec![],
            models: vec![],
        }
    }

    #[tokio::test]
    async fn test_openai_to_qwen_request_transform() {
        let adapter = OpenAIToQwenAdapter;
        let body = serde_json::json!({
            "model": "coder-model",
            "messages": [
                {"role": "user", "content": "Hello"}
            ]
        });
        let config = create_test_config();
        let headers = HeaderMap::new();

        let result = adapter
            .transform_request(body.clone(), &config, &headers)
            .await
            .unwrap();

        // Verify body is unchanged (OpenAI-compatible)
        assert_eq!(result.body, body);

        // Verify Qwen headers are added
        assert!(result.headers.is_some());
        let headers = result.headers.unwrap();
        assert_eq!(
            headers.get("Content-Type").unwrap(),
            "application/json"
        );
        assert_eq!(
            headers.get("User-Agent").unwrap(),
            "QwenCode/0.11.0 (linux; x64)"
        );
        assert_eq!(
            headers.get("X-DashScope-CacheControl").unwrap(),
            "enable"
        );
        assert_eq!(
            headers.get("X-DashScope-AuthType").unwrap(),
            "qwen-oauth"
        );
    }

    #[tokio::test]
    async fn test_openai_to_qwen_streaming_request() {
        let adapter = OpenAIToQwenAdapter;
        let body = serde_json::json!({
            "model": "coder-model",
            "messages": [
                {"role": "user", "content": "Hello"}
            ],
            "stream": true
        });
        let config = create_test_config();
        let headers = HeaderMap::new();

        let result = adapter
            .transform_request(body, &config, &headers)
            .await
            .unwrap();

        // Verify stream_options is added for streaming requests
        assert_eq!(
            result.body["stream_options"],
            serde_json::json!({
                "include_usage": true
            })
        );
    }

    #[tokio::test]
    async fn test_openai_to_qwen_non_streaming_request() {
        let adapter = OpenAIToQwenAdapter;
        let body = serde_json::json!({
            "model": "coder-model",
            "messages": [
                {"role": "user", "content": "Hello"}
            ],
            "stream": false
        });
        let config = create_test_config();
        let headers = HeaderMap::new();

        let result = adapter
            .transform_request(body, &config, &headers)
            .await
            .unwrap();

        // Verify stream_options is NOT added for non-streaming requests
        assert!(result.body.get("stream_options").is_none());
    }

    #[tokio::test]
    async fn test_openai_to_qwen_response_passthrough() {
        let adapter = OpenAIToQwenAdapter;
        let body = serde_json::json!({
            "id": "chatcmpl-123",
            "choices": [
                {
                    "message": {"role": "assistant", "content": "Hi there!"}
                }
            ],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 20,
                "total_tokens": 30
            }
        });
        let status = http::StatusCode::OK;
        let headers = HeaderMap::new();

        let result = adapter
            .transform_response(body.clone(), status, &headers)
            .await
            .unwrap();

        // Verify response is passed through unchanged
        assert_eq!(result.body, body);
    }

    #[tokio::test]
    async fn test_openai_to_qwen_stream_chunk_passthrough() {
        let adapter = OpenAIToQwenAdapter;
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

        // Verify chunk is passed through unchanged
        assert_eq!(result.data, chunk);
        assert!(result.event.is_none());
    }
}
