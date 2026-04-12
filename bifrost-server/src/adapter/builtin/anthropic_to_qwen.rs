//! Anthropic to Qwen adapter - transforms Anthropic requests to Qwen API format
//!
//! This adapter combines:
//! 1. Anthropic → OpenAI request transformation (using shared utils)
//! 2. OpenAI → Qwen header transformation (using shared utils)
//! 3. Qwen OpenAI-format response → Anthropic response transformation
//!
//! The adapter chain is: Anthropic → OpenAI → Qwen

use crate::adapter::Adapter;
use crate::adapter::converter::anthropic_openai::request::anthropic_to_openai_request;
use crate::adapter::converter::anthropic_openai::response::openai_to_anthropic_response;
use crate::adapter::converter::qwen;
use crate::adapter::converter::stream::OpenAIToAnthropicStreamProcessor;
use crate::error::LlmMapError;
use crate::model::{RequestContext, RequestTransform, ResponseTransform, StreamChunkTransform};
use crate::types::ProviderConfig;
use async_trait::async_trait;
use serde_json::{Value, json};

pub struct AnthropicToQwenAdapter {
    /// Stream processor for OpenAI → Anthropic stream conversion
    stream_processor: OpenAIToAnthropicStreamProcessor,
}

impl Default for AnthropicToQwenAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl AnthropicToQwenAdapter {
    pub fn new() -> Self {
        Self {
            stream_processor: OpenAIToAnthropicStreamProcessor::new(),
        }
    }
}

#[async_trait]
impl Adapter for AnthropicToQwenAdapter {
    type Error = LlmMapError;

    async fn transform_request(
        &self,
        context: RequestContext<'_>,
    ) -> Result<RequestTransform, Self::Error> {
        // Step 1: Transform Anthropic request to OpenAI format
        let openai_body = anthropic_to_openai_request(context.body)?;

        // Step 2: Initialize OAuth credentials manager
        qwen::ensure_oauth_manager_initialized()?;

        // Step 3: Ensure token is valid (refresh if expired)

        let manager = qwen::OAUTH_CREDS_MANAGER.get().ok_or_else(|| {
            LlmMapError::Validation(
                "OAuth credentials manager not initialized. This should not happen.".to_string(),
            )
        })?;

        manager.ensure_valid_token().await?;
        let access_token = manager.get_access_token();

        // Step 4: For streaming requests, add stream_options to include usage
        let mut final_body = openai_body;
        if let Some(stream) = final_body.get("stream").and_then(|v| v.as_bool())
            && stream
            && let Some(obj) = final_body.as_object_mut()
        {
            obj.insert(
                "stream_options".to_string(),
                json!({
                    "include_usage": true
                }),
            );
        }

        // Step 5: Add Qwen-specific headers
        let auth_header = format!("Bearer {}", access_token);
        let headers = qwen::add_qwen_headers(&auth_header)?;

        Ok(RequestTransform::new(final_body)
            .with_headers(headers)
            .with_url(crate::util::join_url_paths(
                &context.provider_config.base_url,
                "chat/completions",
            )))
    }

    async fn transform_response(
        &self,
        body: Value,
        _status: http::StatusCode,
        _headers: &http::HeaderMap,
    ) -> Result<ResponseTransform, Self::Error> {
        // OpenAI response → Anthropic response
        let body = openai_to_anthropic_response(body)?;
        Ok(ResponseTransform::new(body))
    }

    async fn transform_stream_chunk(
        &self,
        chunk: Value,
        _event: &str,
        _provider_config: &ProviderConfig,
    ) -> Result<StreamChunkTransform, Self::Error> {
        // Qwen OpenAI-format stream → Anthropic stream
        self.stream_processor
            .openai_stream_to_anthropic_stream(chunk)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ProviderConfig;
    use http::HeaderMap;

    fn init_test_credentials() {
        use chrono::Utc;

        let future_date = Utc::now() + chrono::Duration::days(365);

        let creds = qwen::OAuthCredentials {
            access_token: "test_access_token_12345".to_string(),
            token_type: "Bearer".to_string(),
            refresh_token: Some("test_refresh_token_67890".to_string()),
            resource_url: "portal.qwen.ai".to_string(),
            expiry_date: future_date,
        };

        let manager = qwen::OAuthCredentialsManager::new(creds);
        let _ = qwen::OAUTH_CREDS_MANAGER.set(manager);
    }

    fn create_test_config() -> ProviderConfig {
        ProviderConfig {
            base_url: "https://dashscope.aliyuncs.com/compatible-mode/v1".to_string(),
            api_key: "test-api-key".to_string(),
            endpoint: crate::types::Endpoint::Anthropic,
            adapter: vec![],
            headers: None,
            body: None,
            models: None,
            exclude_headers: None,
            extend: false,
        }
    }

    #[tokio::test]
    async fn test_anthropic_to_qwen_request_transform() {
        init_test_credentials();

        let adapter = AnthropicToQwenAdapter::new();
        let body = json!({
            "model": "claude-3-5-sonnet-20241022",
            "max_tokens": 1024,
            "messages": [
                {"role": "user", "content": "Hello"}
            ]
        });
        let config = create_test_config();
        let headers = HeaderMap::new();
        let uri = http::Uri::from_static("https://openai.com/v1");
        let ctx = RequestContext::new(&uri, body, &config, &headers);
        let result = adapter.transform_request(ctx).await.unwrap();

        assert!(result.body.get("messages").is_some());
        assert!(result.headers.is_some());

        let headers = result.headers.unwrap();
        assert_eq!(headers.get("Content-Type").unwrap(), "application/json");
        assert_eq!(
            headers.get("User-Agent").unwrap(),
            "QwenCode/0.13.1 (linux; x64)"
        );
        assert_eq!(headers.get("X-DashScope-CacheControl").unwrap(), "enable");
    }

    #[tokio::test]
    async fn test_anthropic_to_qwen_response_transform() {
        let adapter = AnthropicToQwenAdapter::new();
        let body = json!({
            "id": "chatcmpl-123",
            "model": "qwen-coder",
            "choices": [{
                    "index": 0,
                    "message": {
                        "role": "assistant",
                        "content": "Hello from Qwen"
                    },
                    "finish_reason": "stop"
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
            .transform_response(body, status, &headers)
            .await
            .unwrap();

        assert_eq!(result.body["type"], "message");
        assert_eq!(result.body["role"], "assistant");
        assert!(result.body["content"].is_array());
    }

    #[tokio::test]
    async fn test_anthropic_to_qwen_streaming_request() {
        init_test_credentials();

        let adapter = AnthropicToQwenAdapter::new();
        let body = json!({
            "model": "claude-3-5-sonnet-20241022",
            "max_tokens": 1024,
            "messages": [
                {"role": "user", "content": "Hello"}
            ],
            "stream": true
        });
        let config = create_test_config();
        let headers = HeaderMap::new();
        let uri = http::Uri::from_static("https://openai.com/v1");
        let ctx = RequestContext::new(&uri, body, &config, &headers);
        let result = adapter.transform_request(ctx).await.unwrap();

        assert_eq!(
            result.body["stream_options"],
            json!({
                "include_usage": true
            })
        );
    }

    #[test]
    fn test_tool_call_streaming_conversion() {
        let adapter = AnthropicToQwenAdapter::new();

        // First chunk: with role to trigger message_start + tool_call
        let chunk = json!({
            "id": "chatcmpl-123",
            "model": "qwen-max",
            "choices": [{
                "index": 0,
                "delta": {
                    "role": "assistant",
                    "tool_calls": [{
                        "index": 0,
                        "id": "call_abc123",
                        "type": "function",
                        "function": {
                            "name": "search",
                            "arguments": "{\"query\": \"hello\"}"
                        }
                    }]
                },
                "finish_reason": null
            }]
        });

        let result = adapter
            .stream_processor
            .openai_stream_to_anthropic_stream(chunk)
            .unwrap();
        let events = result.events;

        // Should have: message_start, content_block_start (tool_use), content_block_delta
        assert!(events.len() >= 3);

        // First event should be message_start
        assert_eq!(events[0].0["type"], "message_start");

        // Second event should be content_block_start for tool_use
        let second_event = &events[1].0;
        assert_eq!(second_event["type"], "content_block_start");
        assert_eq!(second_event["content_block"]["type"], "tool_use");
        assert_eq!(second_event["content_block"]["id"], "call_abc123");
        assert_eq!(second_event["content_block"]["name"], "search");

        // Third event should be content_block_delta with input_json_delta
        let third_event = &events[2].0;
        assert_eq!(third_event["type"], "content_block_delta");
        assert_eq!(third_event["delta"]["type"], "input_json_delta");
        assert_eq!(
            third_event["delta"]["partial_json"],
            "{\"query\": \"hello\"}"
        );
    }
}
