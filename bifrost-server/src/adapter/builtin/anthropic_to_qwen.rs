//! Anthropic to Qwen adapter - transforms Anthropic requests to Qwen API format
//!
//! This adapter combines:
//! 1. Anthropic → OpenAI request transformation (using shared utils)
//! 2. OpenAI → Qwen header transformation (using shared utils)
//! 3. Qwen OpenAI-format response → Anthropic response transformation
//!
//! The adapter chain is: Anthropic → OpenAI → Qwen

use crate::adapter::Adapter;
use crate::adapter::util::{self, ContentType, OpenAIStreamState};
use crate::config::ProviderConfig;
use crate::error::LlmMapError;
use crate::model::{RequestTransform, ResponseTransform, StreamChunkTransform};
use async_trait::async_trait;
use serde_json::{Value, json};
use std::sync::Mutex;

pub struct AnthropicToQwenAdapter {
    /// Unified stream state for OpenAI → Anthropic stream conversion
    stream_state: Mutex<OpenAIStreamState>,
}

impl Default for AnthropicToQwenAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl AnthropicToQwenAdapter {
    pub fn new() -> Self {
        Self {
            stream_state: Mutex::new(OpenAIStreamState::new()),
        }
    }
}

#[async_trait]
impl Adapter for AnthropicToQwenAdapter {
    type Error = LlmMapError;

    async fn transform_request(
        &self,
        body: Value,
        provider_config: &ProviderConfig,
        _headers: &http::HeaderMap,
    ) -> Result<RequestTransform, Self::Error> {
        // Step 1: Transform Anthropic request to OpenAI format
        let openai_body = util::anthropic_to_openai_request(body)?;

        // Step 2: Initialize OAuth credentials (same as OpenAIToQwenAdapter)
        if util::OAUTH_CREDS_MANAGER.get().is_none() {
            let oauth_file = util::get_oauth_file_path()?;
            let creds = util::OAuthCredentials::from_file(&oauth_file).map_err(|e| {
                LlmMapError::Validation(format!("Failed to load OAuth credentials: {}", e))
            })?;

            let manager = util::OAuthCredentialsManager::new(creds);

            if util::OAUTH_CREDS_MANAGER.set(manager).is_err() {
                // Another thread initialized first, that's fine
            }
        }

        // Step 3: Ensure token is valid (refresh if expired)
        let manager = util::OAUTH_CREDS_MANAGER.get().ok_or_else(|| {
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
        let headers = util::add_qwen_headers(&auth_header)?;

        Ok(RequestTransform::new(final_body)
            .with_headers(headers)
            .with_url(crate::util::join_url_paths(
                &provider_config.base_url,
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
        let body = util::openai_to_anthropic_response(body)?;
        Ok(ResponseTransform::new(body))
    }

    async fn transform_stream_chunk(
        &self,
        chunk: Value,
        _event: &str,
        _provider_config: &ProviderConfig,
    ) -> Result<StreamChunkTransform, Self::Error> {
        // Qwen OpenAI-format stream → Anthropic stream
        self.openai_stream_to_anthropic_stream(chunk)
    }
}

// ==================== OpenAI Stream → Anthropic Stream Conversion ====================

impl AnthropicToQwenAdapter {
    fn openai_stream_to_anthropic_stream(
        &self,
        chunk: Value,
    ) -> Result<StreamChunkTransform, LlmMapError> {
        // Parse chunk
        let obj = chunk
            .as_object()
            .ok_or_else(|| LlmMapError::Validation("Invalid chunk format".into()))?;

        let choices = obj.get("choices").and_then(|v| v.as_array());
        let Some(choice) = choices.and_then(|c| c.first()).and_then(|v| v.as_object()) else {
            return Ok(StreamChunkTransform::new(json!({"type": "ping"})));
        };

        let delta = choice.get("delta").and_then(|v| v.as_object());
        let finish_reason = choice.get("finish_reason").and_then(|v| v.as_str());

        // Check if message_start has already been sent (reliable state-based check)
        let message_start_sent = {
            let state = self.stream_state.lock().unwrap();
            state.has_sent_message_start()
        };

        if !message_start_sent {
            let mut events = self.generate_initial_events(obj, delta)?.events;
            
            // Handle finish_reason after generating initial events
            if finish_reason.is_some() {
                let finish_events = self.generate_finishing_events()?;
                events.extend(finish_events.events);
            }
            
            Ok(StreamChunkTransform::new_multi(events))
        } else {
            self.generate_content_events(delta, finish_reason)
        }
    }

    fn generate_initial_events(
        &self,
        obj: &serde_json::Map<String, Value>,
        delta: Option<&serde_json::Map<String, Value>>,
    ) -> Result<StreamChunkTransform, LlmMapError> {
        let mut events = Vec::new();

        let id = obj.get("id").and_then(|v| v.as_str()).unwrap_or("");
        let model = obj.get("model").and_then(|v| v.as_str()).unwrap_or("");

        // 1. Generate message_start event
        let message_start = json!({
            "type": "message_start",
            "message": {
                "id": id,
                "type": "message",
                "role": "assistant",
                "content": [],
                "model": model,
                "stop_reason": null,
                "stop_sequence": null,
                "usage": {"input_tokens": 0, "output_tokens": 1}
            }
        });
        events.push((message_start, Some("message_start".to_string())));

        // 2. Reset state
        {
            let mut state = self.stream_state.lock().unwrap();
            state.reset();
            state.set_message_start_sent();
        }

        // 3. Process delta content
        let delta_events = self.generate_content_events_from_delta(delta, None)?;
        events.extend(delta_events.events);

        Ok(StreamChunkTransform::new_multi(events))
    }

    fn generate_content_events(
        &self,
        delta: Option<&serde_json::Map<String, Value>>,
        finish_reason: Option<&str>,
    ) -> Result<StreamChunkTransform, LlmMapError> {
        self.generate_content_events_from_delta(delta, finish_reason)
    }

    fn generate_content_events_from_delta(
        &self,
        delta: Option<&serde_json::Map<String, Value>>,
        finish_reason: Option<&str>,
    ) -> Result<StreamChunkTransform, LlmMapError> {
        let mut events = Vec::new();

        // Extract thinking content
        let thinking_opt = delta
            .and_then(|d| d.get("reasoning_content"))
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());

        // Extract text content
        let text_opt = delta
            .and_then(|d| d.get("content"))
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());

        // Extract tool_calls
        let tool_calls_opt = delta
            .and_then(|d| d.get("tool_calls"))
            .and_then(|v| v.as_array())
            .cloned();

        let mut state = self.stream_state.lock().unwrap();

        // Process thinking content
        if let Some(thinking) = thinking_opt {
            if state.thinking_block_index() == usize::MAX {
                let new_index = state.increment_next_block_index();
                state.start_thinking_block(new_index);

                let block_start = json!({
                    "type": "content_block_start",
                    "index": new_index,
                    "content_block": {
                        "type": "thinking",
                        "thinking": ""
                    }
                });
                events.push((block_start, Some("content_block_start".to_string())));
            }

            let block_delta = json!({
                "type": "content_block_delta",
                "index": state.thinking_block_index(),
                "delta": {
                    "type": "thinking_delta",
                    "thinking": thinking
                }
            });
            events.push((block_delta, Some("content_block_delta".to_string())));
        }

        // Process text content
        if let Some(text) = text_opt {
            if state.text_block_index() == usize::MAX {
                let new_index = state.increment_next_block_index();
                state.start_text_block(new_index);

                let block_start = json!({
                    "type": "content_block_start",
                    "index": new_index,
                    "content_block": {
                        "type": "text",
                        "text": ""
                    }
                });
                events.push((block_start, Some("content_block_start".to_string())));
            }

            let block_delta = json!({
                "type": "content_block_delta",
                "index": state.text_block_index(),
                "delta": {
                    "type": "text_delta",
                    "text": text
                }
            });
            events.push((block_delta, Some("content_block_delta".to_string())));
        }

        // Process tool_calls
        if let Some(tool_calls) = tool_calls_opt {
            for tool_call_value in tool_calls {
                let tool_call = tool_call_value
                    .as_object()
                    .ok_or_else(|| LlmMapError::Validation("Invalid tool_call format".into()))?;
                // Get tool_call index from OpenAI
                let tool_call_index = tool_call
                    .get("index")
                    .and_then(|v| v.as_u64())
                    .map(|i| i as usize)
                    .ok_or_else(|| LlmMapError::Validation("tool_call missing index".into()))?;

                // Get or create block index for this tool_call
                let (block_index, needs_start) = state.get_or_create_tool_call_block(tool_call_index);

                // Get tool_call id and name (only present in first chunk)
                let id = tool_call.get("id").and_then(|v| v.as_str()).unwrap_or("");
                let name = tool_call
                    .get("function")
                    .and_then(|v| v.as_object())
                    .and_then(|v| v.get("name"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                // Send content_block_start only for the first chunk of this tool_call
                if needs_start {
                    let block_start = json!({
                        "type": "content_block_start",
                        "index": block_index,
                        "content_block": {
                            "type": "tool_use",
                            "id": id,
                            "name": name,
                            "input": {}
                        }
                    });
                    events.push((block_start, Some("content_block_start".to_string())));
                }

                // Process function arguments (incremental JSON)
                if let Some(function) = tool_call.get("function").and_then(|v| v.as_object())
                    && let Some(arguments) = function.get("arguments").and_then(|v| v.as_str()) {
                        let block_delta = json!({
                            "type": "content_block_delta",
                            "index": block_index,
                            "delta": {
                                "type": "input_json_delta",
                                "partial_json": arguments
                            }
                        });
                        events.push((block_delta, Some("content_block_delta".to_string())));
                    }
            }
        }

        // Process finish_reason
        if finish_reason.is_some() {
            drop(state);
            let finish_events = self.generate_finishing_events()?;
            events.extend(finish_events.events);
        }

        Ok(StreamChunkTransform::new_multi(events))
    }

    fn generate_finishing_events(&self) -> Result<StreamChunkTransform, LlmMapError> {
        let mut events = Vec::new();
        let mut state = self.stream_state.lock().unwrap();

        // Generate content_block_stop for each started block (in reverse order)
        for content_type in state.blocks_started().iter().rev() {
            let index = match content_type {
                ContentType::Thinking => state.thinking_block_index(),
                ContentType::Text => state.text_block_index(),
                ContentType::ToolCall => {
                    // ToolCall blocks are tracked separately via tool_call_indices
                    // They will be handled after this loop
                    continue;
                }
            };

            if index != usize::MAX {
                let block_stop = json!({
                    "type": "content_block_stop",
                    "index": index
                });
                events.push((block_stop, Some("content_block_stop".to_string())));
            }
        }

        // Generate content_block_stop for all tool_call blocks
        for tool_call_index in state.tool_call_indices().iter().rev() {
            let block_stop = json!({
                "type": "content_block_stop",
                "index": *tool_call_index
            });
            events.push((block_stop, Some("content_block_stop".to_string())));
        }

        // Generate message_delta
        let message_delta = json!({
            "type": "message_delta",
            "delta": {
                "stop_reason": "end_turn",
                "stop_sequence": null
            },
            "usage": {"output_tokens": 1}
        });
        events.push((message_delta, Some("message_delta".to_string())));

        // Generate message_stop
        let message_stop = json!({
            "type": "message_stop"
        });
        events.push((message_stop, Some("message_stop".to_string())));

        // Reset state
        state.reset();

        Ok(StreamChunkTransform::new_multi(events))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::HeaderMap;

    fn init_test_credentials() {
        use chrono::Utc;

        let future_date = Utc::now() + chrono::Duration::days(365);

        let creds = util::OAuthCredentials {
            access_token: "test_access_token_12345".to_string(),
            token_type: "Bearer".to_string(),
            refresh_token: Some("test_refresh_token_67890".to_string()),
            resource_url: "portal.qwen.ai".to_string(),
            expiry_date: future_date,
        };

        let manager = util::OAuthCredentialsManager::new(creds);
        let _ = util::OAUTH_CREDS_MANAGER.set(manager);
    }

    fn create_test_config() -> ProviderConfig {
        ProviderConfig {
            base_url: "https://dashscope.aliyuncs.com/compatible-mode/v1".to_string(),
            api_key: "test-api-key".to_string(),
            endpoint: crate::config::Endpoint::Anthropic,
            adapter: vec![],
            headers: None,
            body: None,
            models: None,
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

        let result = adapter
            .transform_request(body, &config, &headers)
            .await
            .unwrap();

        assert!(result.body.get("messages").is_some());
        assert!(result.headers.is_some());

        let headers = result.headers.unwrap();
        assert_eq!(headers.get("Content-Type").unwrap(), "application/json");
        assert_eq!(
            headers.get("User-Agent").unwrap(),
            "QwenCode/0.11.0 (linux; x64)"
        );
        assert_eq!(headers.get("X-DashScope-CacheControl").unwrap(), "enable");
    }

    #[tokio::test]
    async fn test_anthropic_to_qwen_response_transform() {
        let adapter = AnthropicToQwenAdapter::new();
        let body = json!({
            "id": "chatcmpl-123",
            "model": "qwen-coder",
            "choices": [
                {
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

        let result = adapter
            .transform_request(body, &config, &headers)
            .await
            .unwrap();

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

        let result = adapter.openai_stream_to_anthropic_stream(chunk).unwrap();
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
        assert_eq!(third_event["delta"]["partial_json"], "{\"query\": \"hello\"}");
    }
}

