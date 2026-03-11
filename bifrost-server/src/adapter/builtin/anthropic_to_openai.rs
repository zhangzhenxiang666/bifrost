use crate::{
    adapter::Adapter,
    adapter::util::{self, OpenAIStreamState},
    config::ProviderConfig,
    error::LlmMapError,
    model::{RequestTransform, ResponseTransform, StreamChunkTransform},
};
use async_trait::async_trait;
use http::HeaderMap;
use serde_json::{Value, json};
use std::sync::Mutex;

pub struct AnthropicToOpenAIAdapter {
    stream_state: Mutex<OpenAIStreamState>,
}

impl Default for AnthropicToOpenAIAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl AnthropicToOpenAIAdapter {
    pub fn new() -> Self {
        Self {
            stream_state: Mutex::new(OpenAIStreamState::new()),
        }
    }
}

#[async_trait]
impl Adapter for AnthropicToOpenAIAdapter {
    type Error = LlmMapError;
    async fn transform_request(
        &self,
        body: Value,
        provider_config: &ProviderConfig,
        _headers: &http::HeaderMap,
    ) -> Result<RequestTransform, Self::Error> {
        let body = util::anthropic_to_openai_request(body)?;
        let mut headers = HeaderMap::new();

        headers.insert(
            http::header::AUTHORIZATION,
            http::header::HeaderValue::from_bytes(
                format!("Bearer {}", provider_config.api_key).as_bytes(),
            )
            .unwrap(),
        );

        Ok(RequestTransform::new(body)
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
        let body = util::openai_to_anthropic_response(body)?;
        Ok(ResponseTransform::new(body))
    }

    async fn transform_stream_chunk(
        &self,
        chunk: Value,
        _event: &str,
        _provider_config: &ProviderConfig,
    ) -> Result<StreamChunkTransform, Self::Error> {
        self.openai_stream_to_anthropic_stream(chunk)
    }
}

// ==================== OpenAI Stream → Anthropic Stream Conversion ====================

impl AnthropicToOpenAIAdapter {
    fn openai_stream_to_anthropic_stream(
        &self,
        chunk: Value,
    ) -> Result<StreamChunkTransform, LlmMapError> {
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

        {
            let mut state = self.stream_state.lock().unwrap();
            state.reset();
            state.set_message_start_sent();
        }

        // Don't pass finish_reason here - it will be handled by the caller
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

                // Close previous active block before starting new one
                if let Some(old_index) = state.set_current_active_block(new_index) {
                    let block_stop = json!({
                        "type": "content_block_stop",
                        "index": old_index
                    });
                    events.push((block_stop, Some("content_block_stop".to_string())));
                }

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

                // Close previous active block before starting new one
                if let Some(old_index) = state.set_current_active_block(new_index) {
                    let block_stop = json!({
                        "type": "content_block_stop",
                        "index": old_index
                    });
                    events.push((block_stop, Some("content_block_stop".to_string())));
                }

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
                let (block_index, needs_start) =
                    state.get_or_create_tool_call_block(tool_call_index);

                // Get tool_call id and name (only present in first chunk)
                let id = tool_call.get("id").and_then(|v| v.as_str()).unwrap_or("");
                let name = tool_call
                    .get("function")
                    .and_then(|v| v.as_object())
                    .and_then(|v| v.get("name"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                // Close previous active block before starting new tool_call block
                if needs_start {
                    if let Some(old_index) = state.set_current_active_block(block_index) {
                        let block_stop = json!({
                            "type": "content_block_stop",
                            "index": old_index
                        });
                        events.push((block_stop, Some("content_block_stop".to_string())));
                    }

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
                    && let Some(arguments) = function.get("arguments").and_then(|v| v.as_str())
                {
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

        // Close the currently active block (if any)
        // All previous blocks have already been closed during stream processing
        if let Some(old_index) = state.set_current_active_block(usize::MAX) {
            let block_stop = json!({
                "type": "content_block_stop",
                "index": old_index
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

    #[test]
    fn test_tool_call_streaming_conversion() {
        let adapter = AnthropicToOpenAIAdapter::new();

        // First chunk: with role to trigger message_start + tool_call
        let chunk = json!({
            "id": "chatcmpl-123",
            "model": "gpt-4",
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
        assert_eq!(
            third_event["delta"]["partial_json"],
            "{\"query\": \"hello\"}"
        );
    }

    #[test]
    fn test_thinking_then_tool_call_streaming() {
        let adapter = AnthropicToOpenAIAdapter::new();

        // First chunk: thinking content (with role to trigger message_start)
        let chunk1 = json!({
            "id": "chatcmpl-123",
            "model": "gpt-4",
            "choices": [{
                "index": 0,
                "delta": {
                    "role": "assistant",
                    "reasoning_content": "Let me think..."
                },
                "finish_reason": null
            }]
        });

        let result1 = adapter.openai_stream_to_anthropic_stream(chunk1).unwrap();
        let events1 = result1.events;

        // Should have message_start, content_block_start (thinking), content_block_delta (thinking)
        assert!(events1.len() >= 3);
        assert_eq!(events1[0].0["type"], "message_start");

        // Second chunk: tool_call
        let chunk2 = json!({
            "id": "chatcmpl-123",
            "model": "gpt-4",
            "choices": [{
                "index": 0,
                "delta": {
                    "tool_calls": [{
                        "index": 0,
                        "id": "call_abc123",
                        "type": "function",
                        "function": {
                            "name": "search",
                            "arguments": "{}"
                        }
                    }]
                },
                "finish_reason": null
            }]
        });

        let result2 = adapter.openai_stream_to_anthropic_stream(chunk2).unwrap();
        let events2 = result2.events;

        // Should have content_block_stop (thinking), content_block_start (tool_use), content_block_delta
        assert!(
            events2.len() >= 3,
            "Expected at least 3 events, got {:?}",
            events2.iter().map(|e| &e.0["type"]).collect::<Vec<_>>()
        );
        // First event should be content_block_stop for thinking block
        assert_eq!(
            events2[0].0["type"], "content_block_stop",
            "First event should be content_block_stop, got: {:?}",
            events2[0]
        );
        // Second event should be content_block_start for tool_use
        assert_eq!(
            events2[1].0["type"], "content_block_start",
            "Second event should be content_block_start, got: {:?}",
            events2[1]
        );
        assert_eq!(events2[1].0["content_block"]["type"], "tool_use");
    }

    #[test]
    fn test_tool_call_with_finish_reason() {
        let adapter = AnthropicToOpenAIAdapter::new();

        // Chunk with role + tool_call + finish_reason
        let chunk = json!({
            "id": "chatcmpl-123",
            "model": "gpt-4",
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
                            "arguments": "{}"
                        }
                    }]
                },
                "finish_reason": "tool_calls"
            }]
        });

        let result = adapter.openai_stream_to_anthropic_stream(chunk).unwrap();
        let events = result.events;

        // Should have: message_start, content_block_start (tool_use), content_block_delta, content_block_stop, message_delta, message_stop
        // But message_start is sent, then content events, then finish events
        assert!(
            events.len() >= 4,
            "Expected at least 4 events, got {}. Events: {:?}",
            events.len(),
            events
                .iter()
                .map(|(e, _)| e["type"].to_string())
                .collect::<Vec<_>>()
        );

        // Find content_block_stop events
        let stop_events: Vec<_> = events
            .iter()
            .filter(|e| e.0["type"] == "content_block_stop")
            .collect();
        assert!(!stop_events.is_empty());

        // Last event should be message_stop
        assert_eq!(events.last().unwrap().0["type"], "message_stop");
    }
}
