use crate::adapter::Adapter;
use crate::adapter::converter::anthropic_openai::request::anthropic_to_openai_request;
use crate::adapter::converter::anthropic_openai::response::openai_to_anthropic_response;
use crate::adapter::converter::stream::OpenAIToAnthropicStreamProcessor;
use crate::error::LlmMapError;
use crate::model::{
    RequestContext, RequestTransform, ResponseContext, ResponseTransform, StreamChunkContext,
    StreamChunkTransform,
};
use async_trait::async_trait;
use http::HeaderMap;

pub struct AnthropicToOpenAIAdapter {
    stream_processor: OpenAIToAnthropicStreamProcessor,
}

impl Default for AnthropicToOpenAIAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl AnthropicToOpenAIAdapter {
    pub fn new() -> Self {
        Self {
            stream_processor: OpenAIToAnthropicStreamProcessor::new(),
        }
    }
}

#[async_trait]
impl Adapter for AnthropicToOpenAIAdapter {
    type Error = LlmMapError;
    async fn transform_request(
        &self,
        context: RequestContext<'_>,
    ) -> Result<RequestTransform, Self::Error> {
        let body = anthropic_to_openai_request(context.body)?;
        let mut headers = HeaderMap::new();

        headers.insert(
            http::header::AUTHORIZATION,
            http::header::HeaderValue::from_bytes(
                format!("Bearer {}", context.provider_config.api_key).as_bytes(),
            )
            .unwrap(),
        );

        Ok(RequestTransform::new(body)
            .with_headers(headers)
            .with_url(crate::util::join_url_paths(
                &context.provider_config.base_url,
                "chat/completions",
            )))
    }

    async fn transform_response(
        &self,
        context: ResponseContext<'_>,
    ) -> Result<ResponseTransform, Self::Error> {
        let body = openai_to_anthropic_response(context.body)?;
        Ok(ResponseTransform::new(body))
    }

    async fn transform_stream_chunk(
        &self,
        context: StreamChunkContext<'_>,
    ) -> Result<StreamChunkTransform, Self::Error> {
        self.stream_processor
            .openai_stream_to_anthropic_stream(context.chunk)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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

        let result1 = adapter
            .stream_processor
            .openai_stream_to_anthropic_stream(chunk1)
            .unwrap();
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

        let result2 = adapter
            .stream_processor
            .openai_stream_to_anthropic_stream(chunk2)
            .unwrap();
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

        let result = adapter
            .stream_processor
            .openai_stream_to_anthropic_stream(chunk)
            .unwrap();
        let events = result.events;

        // Should have: message_start, content_block_start (tool_use), content_block_delta, content_block_stop, message_delta, message_stop
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
