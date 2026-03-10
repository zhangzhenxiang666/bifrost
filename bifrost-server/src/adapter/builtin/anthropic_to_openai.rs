use crate::{
    adapter::Adapter,
    adapter::util::{self, ContentType, OpenAIStreamState},
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

        let is_first_chunk = delta
            .and_then(|d| d.get("role").and_then(|v| v.as_str()))
            .is_some();

        if is_first_chunk {
            self.generate_initial_events(obj, delta)
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

        let thinking_opt = delta
            .and_then(|d| d.get("reasoning_content"))
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());

        let text_opt = delta
            .and_then(|d| d.get("content"))
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());

        let mut state = self.stream_state.lock().unwrap();

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

        for content_type in state.blocks_started().iter().rev() {
            let index = match content_type {
                ContentType::Thinking => state.thinking_block_index(),
                ContentType::Text => state.text_block_index(),
            };

            if index != usize::MAX {
                let block_stop = json!({
                    "type": "content_block_stop",
                    "index": index
                });
                events.push((block_stop, Some("content_block_stop".to_string())));
            }
        }

        let message_delta = json!({
            "type": "message_delta",
            "delta": {
                "stop_reason": "end_turn",
                "stop_sequence": null
            },
            "usage": {"output_tokens": 1}
        });
        events.push((message_delta, Some("message_delta".to_string())));

        let message_stop = json!({
            "type": "message_stop"
        });
        events.push((message_stop, Some("message_stop".to_string())));

        state.reset();

        Ok(StreamChunkTransform::new_multi(events))
    }
}
