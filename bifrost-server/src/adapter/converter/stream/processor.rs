//! Stream processor for converting OpenAI-format streams to Anthropic-format events

use crate::adapter::converter::stream::state::OpenAIStreamState;
use crate::error::LlmMapError;
use crate::model::StreamChunkTransform;
use serde_json::{Value, json};
use std::cell::UnsafeCell;

/// Stream processor for converting OpenAI-format stream chunks to Anthropic-format events.
///
/// # Safety Invariant
///
/// `stream_state` is wrapped in `UnsafeCell` to allow interior mutability without
/// any locking overhead. This is sound under the following architectural guarantee:
///
/// - One `OpenAIStreamProcessor` instance is created per request.
/// - All method calls on that instance occur sequentially; no two call-sites
///   ever execute concurrently on the same instance.
///
/// Consequently there is never more than one live `&` or `&mut` reference to
/// `stream_state` at a time, which is the only condition `UnsafeCell` requires.
/// Violating this invariant is undefined behavior.
pub struct OpenAIStreamProcessor {
    stream_state: UnsafeCell<OpenAIStreamState>,
}

// SAFETY: The processor is never shared across threads concurrently.
// Each request owns its own instance and drives it from a single async task.
unsafe impl Sync for OpenAIStreamProcessor {}

// SAFETY: Ownership may cross thread boundaries at Tokio await points, but only
// one thread holds the processor at any given moment, and `OpenAIStreamState`
// contains no thread-local state.
unsafe impl Send for OpenAIStreamProcessor {}

impl Default for OpenAIStreamProcessor {
    fn default() -> Self {
        Self::new()
    }
}

impl OpenAIStreamProcessor {
    pub fn new() -> Self {
        Self {
            stream_state: UnsafeCell::new(OpenAIStreamState::new()),
        }
    }

    /// Immutable access — use for all reads.
    ///
    /// SAFETY: No `&mut` obtained via `state_mut` may be alive simultaneously.
    /// Upheld because every `state_mut()` call is a single-expression statement
    /// whose returned reference expires before the next statement executes.
    #[inline(always)]
    fn state(&self) -> &OpenAIStreamState {
        unsafe { &*self.stream_state.get() }
    }

    /// Mutable access — call only for the single mutation, then let the
    /// reference expire immediately at the semicolon / end of the expression.
    ///
    /// SAFETY: Same guarantee as above; never store the returned reference
    /// across any other `state()` / `state_mut()` call.
    #[inline(always)]
    #[allow(clippy::mut_from_ref)]
    fn state_mut(&self) -> &mut OpenAIStreamState {
        unsafe { &mut *self.stream_state.get() }
    }

    pub fn openai_stream_to_anthropic_stream(
        &self,
        chunk: Value,
    ) -> Result<StreamChunkTransform, LlmMapError> {
        let obj = chunk
            .as_object()
            .ok_or_else(|| LlmMapError::Validation("Invalid chunk format".into()))?;

        let choices = obj.get("choices").and_then(|v| v.as_array());
        let Some(choice) = choices.and_then(|c| c.first()).and_then(|v| v.as_object()) else {
            return Ok(StreamChunkTransform::new(json!({ "type": "ping" })));
        };

        let delta = choice.get("delta").and_then(|v| v.as_object());
        let finish_reason = choice.get("finish_reason").and_then(|v| v.as_str());
        let usage = obj.get("usage").and_then(|v| v.as_object());

        // Read-only check — no &mut needed here.
        if !self.state().has_sent_message_start() {
            let mut events = self.generate_initial_events(obj, delta, usage)?.events;
            if let Some(reason) = finish_reason {
                events.extend(self.generate_finishing_events(Some(reason), usage)?.events);
            }
            Ok(StreamChunkTransform::new_multi(events))
        } else {
            self.generate_content_events(delta, finish_reason, usage)
        }
    }

    fn generate_initial_events(
        &self,
        obj: &serde_json::Map<String, Value>,
        delta: Option<&serde_json::Map<String, Value>>,
        usage: Option<&serde_json::Map<String, Value>>,
    ) -> Result<StreamChunkTransform, LlmMapError> {
        let mut events = Vec::new();

        let id = obj.get("id").and_then(|v| v.as_str()).unwrap_or("");
        let model = obj.get("model").and_then(|v| v.as_str()).unwrap_or("");
        let input_tokens = usage
            .and_then(|u| u.get("prompt_tokens"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;
        let output_tokens = usage
            .and_then(|u| u.get("completion_tokens"))
            .and_then(|v| v.as_u64())
            .unwrap_or(1) as u32;

        events.push((
            json!({
                "type": "message_start",
                "message": {
                    "id": id,
                    "type": "message",
                    "role": "assistant",
                    "content": [],
                    "model": model,
                    "stop_reason": null,
                    "stop_sequence": null,
                    "usage": {
                        "input_tokens": input_tokens,
                        "output_tokens": output_tokens
                    }
                }
            }),
            Some("message_start".to_string()),
        ));

        // Two independent mutations; each &mut expires at its own semicolon.
        self.state_mut().reset();
        self.state_mut().set_message_start_sent();

        events.extend(
            self.generate_content_events_from_delta(delta, None, None)?
                .events,
        );
        Ok(StreamChunkTransform::new_multi(events))
    }

    fn generate_content_events(
        &self,
        delta: Option<&serde_json::Map<String, Value>>,
        finish_reason: Option<&str>,
        usage: Option<&serde_json::Map<String, Value>>,
    ) -> Result<StreamChunkTransform, LlmMapError> {
        self.generate_content_events_from_delta(delta, finish_reason, usage)
    }

    fn generate_content_events_from_delta(
        &self,
        delta: Option<&serde_json::Map<String, Value>>,
        finish_reason: Option<&str>,
        usage: Option<&serde_json::Map<String, Value>>,
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

        let tool_calls_opt = delta
            .and_then(|d| d.get("tool_calls"))
            .and_then(|v| v.as_array())
            .cloned();

        // ── Thinking block ────────────────────────────────────────────────────
        if let Some(thinking) = thinking_opt {
            // Read: is this the first thinking chunk?
            if self.state().thinking_block_index() == usize::MAX {
                // Each line below is one independent mutation; &mut expires at ";".
                let new_index = self.state_mut().increment_next_block_index();
                self.state_mut().start_thinking_block(new_index);
                if let Some(old) = self.state_mut().set_current_active_block(new_index) {
                    events.push((
                        json!({
                            "type": "content_block_stop",
                            "index": old
                        }),
                        Some("content_block_stop".to_string()),
                    ));
                }
                events.push((
                    json!({
                        "type": "content_block_start",
                        "index": new_index,
                        "content_block": {
                            "type": "thinking",
                            "thinking": ""
                        }
                    }),
                    Some("content_block_start".to_string()),
                ));
            }
            // Read: stable index for the delta event.
            let idx = self.state().thinking_block_index();
            events.push((
                json!({
                    "type": "content_block_delta",
                    "index": idx,
                    "delta": {
                        "type": "thinking_delta",
                        "thinking": thinking
                    }
                }),
                Some("content_block_delta".to_string()),
            ));
        }

        // ── Text block ────────────────────────────────────────────────────────
        if let Some(text) = text_opt {
            if self.state().text_block_index() == usize::MAX {
                let new_index = self.state_mut().increment_next_block_index();
                self.state_mut().start_text_block(new_index);
                if let Some(old) = self.state_mut().set_current_active_block(new_index) {
                    events.push((
                        json!({
                            "type": "content_block_stop",
                            "index": old
                        }),
                        Some("content_block_stop".to_string()),
                    ));
                }
                events.push((
                    json!({
                        "type": "content_block_start",
                        "index": new_index,
                        "content_block": {
                            "type": "text",
                            "text": ""
                        }
                    }),
                    Some("content_block_start".to_string()),
                ));
            }
            let idx = self.state().text_block_index();
            events.push((
                json!({
                    "type": "content_block_delta",
                    "index": idx,
                    "delta": {
                        "type": "text_delta",
                        "text": text
                    }
                }),
                Some("content_block_delta".to_string()),
            ));
        }

        // ── Tool calls ────────────────────────────────────────────────────────
        if let Some(tool_calls) = tool_calls_opt {
            for tool_call_value in tool_calls {
                let tool_call = tool_call_value
                    .as_object()
                    .ok_or_else(|| LlmMapError::Validation("Invalid tool_call format".into()))?;

                let tool_call_index = tool_call
                    .get("index")
                    .and_then(|v| v.as_u64())
                    .map(|i| i as usize)
                    .ok_or_else(|| LlmMapError::Validation("tool_call missing index".into()))?;

                let id = tool_call.get("id").and_then(|v| v.as_str()).unwrap_or("");
                let name = tool_call
                    .get("function")
                    .and_then(|v| v.as_object())
                    .and_then(|v| v.get("name"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                // Mutate: get-or-create; &mut expires at the semicolon.
                let (block_index, needs_start) = self
                    .state_mut()
                    .get_or_create_tool_call_block(tool_call_index);

                if needs_start {
                    if let Some(old) = self.state_mut().set_current_active_block(block_index) {
                        events.push((
                            json!({
                                "type": "content_block_stop",
                                "index": old
                            }),
                            Some("content_block_stop".to_string()),
                        ));
                    }
                    events.push((
                        json!({
                            "type": "content_block_start",
                            "index": block_index,
                            "content_block": {
                                "type": "tool_use",
                                "id": id,
                                "name": name,
                                "input": {}
                            }
                        }),
                        Some("content_block_start".to_string()),
                    ));
                }

                if let Some(arguments) = tool_call
                    .get("function")
                    .and_then(|v| v.as_object())
                    .and_then(|v| v.get("arguments"))
                    .and_then(|v| v.as_str())
                {
                    events.push((
                        json!({
                            "type": "content_block_delta",
                            "index": block_index,
                            "delta": {
                                "type": "input_json_delta",
                                "partial_json": arguments
                            }
                        }),
                        Some("content_block_delta".to_string()),
                    ));
                }
            }
        }

        // ── Finish ────────────────────────────────────────────────────────────
        if let Some(reason) = finish_reason {
            events.extend(self.generate_finishing_events(Some(reason), usage)?.events);
        }

        Ok(StreamChunkTransform::new_multi(events))
    }

    fn generate_finishing_events(
        &self,
        finish_reason: Option<&str>,
        usage: Option<&serde_json::Map<String, Value>>,
    ) -> Result<StreamChunkTransform, LlmMapError> {
        let mut events = Vec::new();

        // Mutate: close active block; &mut expires at the semicolon.
        if let Some(old) = self.state_mut().set_current_active_block(usize::MAX) {
            events.push((
                json!({
                    "type": "content_block_stop",
                    "index": old
                }),
                Some("content_block_stop".to_string()),
            ));
        }

        let stop_reason = match finish_reason {
            Some("tool_calls") => "tool_use",
            Some("stop") => "end_turn",
            Some("length") => "max_tokens",
            Some("content_filter") => "end_turn",
            _ => "end_turn",
        };

        let output_tokens = usage
            .and_then(|u| u.get("completion_tokens"))
            .and_then(|v| v.as_u64())
            .unwrap_or(1) as u32;

        events.push((
            json!({
                "type": "message_delta",
                "delta": {
                    "stop_reason": stop_reason,
                    "stop_sequence": null
                },
                "usage": {
                    "output_tokens": output_tokens
                }
            }),
            Some("message_delta".to_string()),
        ));
        events.push((
            json!({ "type": "message_stop" }),
            Some("message_stop".to_string()),
        ));

        // Mutate: reset for safety; &mut expires at the semicolon.
        self.state_mut().reset();

        Ok(StreamChunkTransform::new_multi(events))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_call_streaming_conversion() {
        let processor = OpenAIStreamProcessor::new();

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
                            "arguments": "{\"query\": \"hello\"}",
                        }
                    }]
                },
                "finish_reason": null
            }]
        });

        let result = processor.openai_stream_to_anthropic_stream(chunk).unwrap();
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
        let processor = OpenAIStreamProcessor::new();

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

        let result1 = processor.openai_stream_to_anthropic_stream(chunk1).unwrap();
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

        let result2 = processor.openai_stream_to_anthropic_stream(chunk2).unwrap();
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
        let processor = OpenAIStreamProcessor::new();

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

        let result = processor.openai_stream_to_anthropic_stream(chunk).unwrap();
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

    #[test]
    fn test_usage_extraction_with_usage_in_message_start() {
        let processor = OpenAIStreamProcessor::new();

        // Chunk with usage information
        let chunk = json!({
            "id": "chatcmpl-123",
            "model": "gpt-4",
            "choices": [{
                "index": 0,
                "delta": {
                    "role": "assistant"
                },
                "finish_reason": null
            }],
            "usage": {
                "prompt_tokens": 100,
                "completion_tokens": 50,
                "total_tokens": 150
            }
        });

        let result = processor.openai_stream_to_anthropic_stream(chunk).unwrap();
        let events = result.events;

        // First event should be message_start with actual usage
        assert_eq!(events[0].0["type"], "message_start");
        assert_eq!(events[0].0["message"]["usage"]["input_tokens"], 100);
        assert_eq!(events[0].0["message"]["usage"]["output_tokens"], 50);
    }

    #[test]
    fn test_usage_extraction_with_usage_in_final_chunk() {
        let processor = OpenAIStreamProcessor::new();

        // First chunk without usage
        let chunk1 = json!({
            "id": "chatcmpl-123",
            "model": "gpt-4",
            "choices": [{
                "index": 0,
                "delta": {
                    "role": "assistant",
                    "content": "Hello"
                },
                "finish_reason": null
            }]
        });

        let result1 = processor.openai_stream_to_anthropic_stream(chunk1).unwrap();
        let events1 = result1.events;

        // message_start should have default usage
        assert_eq!(events1[0].0["message"]["usage"]["input_tokens"], 0);
        assert_eq!(events1[0].0["message"]["usage"]["output_tokens"], 1);

        // Second chunk with usage and finish_reason
        let chunk2 = json!({
            "id": "chatcmpl-123",
            "model": "gpt-4",
            "choices": [{
                "index": 0,
                "delta": {},
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 200,
                "completion_tokens": 80,
                "total_tokens": 280
            }
        });

        let result2 = processor.openai_stream_to_anthropic_stream(chunk2).unwrap();
        let events2 = result2.events;

        // Find message_delta event - should have actual output_tokens
        let message_delta = events2.iter().find(|e| e.0["type"] == "message_delta");
        assert!(message_delta.is_some());
        let message_delta = message_delta.unwrap();
        assert_eq!(message_delta.0["usage"]["output_tokens"], 80);
    }

    #[test]
    fn test_usage_extraction_without_usage_fallback() {
        let processor = OpenAIStreamProcessor::new();

        // Chunk without usage information
        let chunk = json!({
            "id": "chatcmpl-123",
            "model": "gpt-4",
            "choices": [{
                "index": 0,
                "delta": {
                    "role": "assistant"
                },
                "finish_reason": null
            }]
        });

        let result = processor.openai_stream_to_anthropic_stream(chunk).unwrap();
        let events = result.events;

        // Should use fallback values
        assert_eq!(events[0].0["message"]["usage"]["input_tokens"], 0);
        assert_eq!(events[0].0["message"]["usage"]["output_tokens"], 1);
    }
}
