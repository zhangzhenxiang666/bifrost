//! Stream processor for converting Anthropic-format SSE events to OpenAI Chat Completions format

use crate::error::LlmMapError;
use crate::model::StreamChunkTransform;
use serde_json::{Value, json};
use std::cell::UnsafeCell;

fn get_created_time() -> Option<u64> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs())
}

const FLAG_CURRENT_BLOCK_IS_TOOL: u8 = 0b001;
const FLAG_TOOL_USE_SENT: u8 = 0b010;

struct AnthropicToOpenAIStreamState {
    id: String,
    model: String,
    input_tokens: u32,
    cache_read_input_tokens: u32,
    cache_creation_input_tokens: u32,
    tool_index: u32,
    flags: u8,
}

impl Default for AnthropicToOpenAIStreamState {
    fn default() -> Self {
        Self::new()
    }
}

impl AnthropicToOpenAIStreamState {
    pub fn new() -> Self {
        Self {
            id: String::new(),
            model: String::new(),
            input_tokens: 0,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
            tool_index: 0,
            flags: 0,
        }
    }
}

/// Stream processor for converting Anthropic-format SSE events to OpenAI Chat Completions chunks.
///
/// # Safety Invariant
///
/// `state` is wrapped in `UnsafeCell` to allow interior mutability without
/// any locking overhead. This is sound under the following architectural guarantee:
///
/// - One [`AnthropicToOpenAIStreamProcessor`] instance is created per request.
/// - All method calls on that instance occur sequentially; no two call-sites
///   ever execute concurrently on the same instance.
///
/// Consequently there is never more than one live `&` or `&mut` reference to
/// `state` at a time, which is the only condition `UnsafeCell` requires.
/// Violating this invariant is undefined behavior.
pub struct AnthropicToOpenAIStreamProcessor {
    state: UnsafeCell<AnthropicToOpenAIStreamState>,
}

// SAFETY: The processor is never shared across threads concurrently.
// Each request owns its own instance and drives it from a single async task.
unsafe impl Sync for AnthropicToOpenAIStreamProcessor {}

// SAFETY: Ownership may cross thread boundaries at Tokio await points, but only
// one thread holds the processor at any given moment, and `AnthropicToOpenAIStreamState`
// contains no thread-local state.
unsafe impl Send for AnthropicToOpenAIStreamProcessor {}

impl Default for AnthropicToOpenAIStreamProcessor {
    fn default() -> Self {
        Self::new()
    }
}

impl AnthropicToOpenAIStreamProcessor {
    pub fn new() -> Self {
        Self {
            state: UnsafeCell::new(AnthropicToOpenAIStreamState::new()),
        }
    }

    /// Immutable access — use for all reads.
    ///
    /// SAFETY: No `&mut` obtained via `state_mut` may be alive simultaneously.
    /// Upheld because every `state_mut()` call is a single-expression statement
    /// whose returned reference expires before the next statement executes.
    #[inline(always)]
    fn state(&self) -> &AnthropicToOpenAIStreamState {
        unsafe { &*self.state.get() }
    }

    /// Mutable access — call only for the single mutation, then let the
    /// reference expire immediately at the semicolon / end of the expression.
    ///
    /// SAFETY: Same guarantee as above; never store the returned reference
    /// across any other `state()` / `state_mut()` call.
    #[inline(always)]
    #[allow(clippy::mut_from_ref)]
    fn state_mut(&self) -> &mut AnthropicToOpenAIStreamState {
        unsafe { &mut *self.state.get() }
    }

    pub fn anthropic_to_openai_stream(
        &self,
        event_type: &str,
        chunk: Value,
    ) -> Result<StreamChunkTransform, LlmMapError> {
        match event_type {
            "message_start" => convert_message_start(chunk, self.state_mut()),
            "content_block_start" => convert_content_block_start(chunk, self.state_mut()),
            "content_block_delta" => convert_content_block_delta(chunk, self.state()),
            "content_block_stop" => convert_content_block_stop(self.state_mut()),
            "message_delta" => convert_message_delta(chunk, self.state_mut()),
            _ => Ok(StreamChunkTransform::new_empty()),
        }
    }
}

fn convert_message_start(
    chunk: Value,
    state: &mut AnthropicToOpenAIStreamState,
) -> Result<StreamChunkTransform, LlmMapError> {
    let obj = chunk
        .as_object()
        .ok_or_else(|| LlmMapError::Validation("message_start: data must be object".into()))?;

    let message = obj
        .get("message")
        .and_then(|v| v.as_object())
        .ok_or_else(|| LlmMapError::Validation("message_start: missing message".into()))?;

    state.id = message
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    state.model = message
        .get("model")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let usage = message.get("usage").and_then(|v| v.as_object());
    state.input_tokens = usage
        .and_then(|u| u.get("input_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    state.cache_read_input_tokens = usage
        .and_then(|u| u.get("cache_read_input_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    state.cache_creation_input_tokens = usage
        .and_then(|u| u.get("cache_creation_input_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;

    Ok(StreamChunkTransform::new_empty())
}

fn convert_content_block_start(
    chunk: Value,
    state: &mut AnthropicToOpenAIStreamState,
) -> Result<StreamChunkTransform, LlmMapError> {
    // 对于标准的anthropic规范的sse事件content_block_start事件其实对于openai标准来说只有一个tool_use事件需要发送tool_call_id以及设置tool_index
    if state.id.is_empty() {
        return Ok(StreamChunkTransform::new_empty());
    }

    let obj = chunk.as_object().ok_or_else(|| {
        LlmMapError::Validation("content_block_start: data must be object".into())
    })?;

    let index = obj.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
    let block = obj.get("content_block").and_then(|v| v.as_object());

    if let Some(block) = block
        && let Some(block_type) = block.get("type").and_then(|v| v.as_str())
        && block_type == "tool_use"
    {
        // 标记开始的block是一个tool_use
        state.flags |= FLAG_CURRENT_BLOCK_IS_TOOL;
        // 标记发送过tool_use事件
        state.flags |= FLAG_TOOL_USE_SENT;

        let id = block.get("id").and_then(|v| v.as_str()).unwrap_or("");
        let name = block.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let delta = json!({
            "role": "assistant",
            "tool_calls": [{
                "index": state.tool_index,
                "id": id,
                "type": "function",
                "function": { "name": name, "arguments": "" }
            }]
        });
        let choice = json!({
            "finish_reason": null,
            "delta": delta,
            "index": index,
        });

        Ok(StreamChunkTransform::new(
            json!({"id": state.id, "model": state.model, "created": get_created_time().unwrap_or(0), "object": "chat.completion.chunk", "choices": [choice]}),
        ))
    } else {
        Ok(StreamChunkTransform::new_empty())
    }
}

fn convert_content_block_stop(
    state: &mut AnthropicToOpenAIStreamState,
) -> Result<StreamChunkTransform, LlmMapError> {
    // 如果结束的是工具块，则 OpenAI 的 tool_index 准备指向下一个
    if state.flags & FLAG_CURRENT_BLOCK_IS_TOOL != 0 {
        state.tool_index += 1;
        state.flags &= !FLAG_CURRENT_BLOCK_IS_TOOL; // 重置标记
    }

    Ok(StreamChunkTransform::new_empty())
}

fn convert_content_block_delta(
    chunk: Value,
    state: &AnthropicToOpenAIStreamState,
) -> Result<StreamChunkTransform, LlmMapError> {
    let delta = chunk.get("delta").and_then(|v| v.as_object());
    let Some(delta) = delta else {
        return Ok(StreamChunkTransform::new_empty());
    };

    let delta_type = delta.get("type").and_then(|v| v.as_str()).unwrap_or("");

    let content_delta = match delta_type {
        "text_delta" => {
            let text = delta.get("text").and_then(|v| v.as_str()).unwrap_or("");
            json!({ "content": text })
        }
        "input_json_delta" => {
            let partial = delta
                .get("partial_json")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            json!({
                "tool_calls": [{
                    "index": state.tool_index,
                    "function": { "arguments": partial }
                }]
            })
        }
        "thinking_delta" => {
            let thinking = delta.get("thinking").and_then(|v| v.as_str()).unwrap_or("");
            json!({ "reasoning_content": thinking })
        }
        _ => return Ok(StreamChunkTransform::new_empty()),
    };

    Ok(StreamChunkTransform::new(json!({
        "id": state.id,
        "model": state.model,
        "created": get_created_time().unwrap_or(0),
        "object": "chat.completion.chunk",
        "choices": [{
            "index": 0,
            "delta": content_delta,
            "finish_reason": null
        }]
    })))
}

fn convert_message_delta(
    data: Value,
    state: &mut AnthropicToOpenAIStreamState,
) -> Result<StreamChunkTransform, LlmMapError> {
    if state.id.is_empty() {
        return Ok(StreamChunkTransform::new_empty());
    }

    let obj = data
        .as_object()
        .ok_or_else(|| LlmMapError::Validation("message_delta: data must be object".into()))?;

    let delta = obj.get("delta").and_then(|v| v.as_object());
    let usage = obj.get("usage").and_then(|v| v.as_object());

    let stop_reason = delta
        .and_then(|d| d.get("stop_reason"))
        .and_then(|v| v.as_str())
        .map(|s| match s {
            "tool_use" => "tool_calls",
            "max_tokens" => "length",
            _ => {
                if state.flags & FLAG_TOOL_USE_SENT != 0 {
                    "tool_calls"
                } else {
                    "stop"
                }
            }
        })
        .unwrap_or("stop");

    // Some providers send input_tokens in message_delta instead of message_start.
    if let Some(delta_input_tokens) = usage
        .and_then(|u| u.get("input_tokens"))
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        && state.input_tokens == 0
    {
        state.input_tokens = delta_input_tokens;
    }
    if let Some(delta_cache_read) = usage
        .and_then(|u| u.get("cache_read_input_tokens"))
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        && state.cache_read_input_tokens == 0
    {
        state.cache_read_input_tokens = delta_cache_read;
    }
    if let Some(delta_cache_creation) = usage
        .and_then(|u| u.get("cache_creation_input_tokens"))
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        && state.cache_creation_input_tokens == 0
    {
        state.cache_creation_input_tokens = delta_cache_creation;
    }

    let output_tokens = usage
        .and_then(|u| u.get("output_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;

    let prompt_tokens =
        state.input_tokens + state.cache_read_input_tokens + state.cache_creation_input_tokens;

    let mut usage_obj = json!({
        "prompt_tokens": prompt_tokens,
        "completion_tokens": output_tokens,
        "total_tokens": prompt_tokens + output_tokens
    });

    if state.cache_read_input_tokens > 0
        && let Some(obj) = usage_obj.as_object_mut()
    {
        obj.insert(
            "prompt_tokens_details".to_string(),
            json!({ "cached_tokens": state.cache_read_input_tokens }),
        );
    }

    let data = json!({
    "id": state.id,
    "model": state.model,
    "created": get_created_time().unwrap_or(0),
    "object": "chat.completion.chunk",
    "choices": [{
        "index": 0,
        "delta": {},
        "finish_reason": stop_reason
    }],
    "usage": usage_obj});

    Ok(StreamChunkTransform::new(data))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::converter::stream_test_utils::{
        NormalizedSseData, load_sse_fixture, normalize_stream_events,
    };

    #[test]
    fn test_full_anthropic_messages_sse_fixture_to_openai_chat_stream() {
        let processor = AnthropicToOpenAIStreamProcessor::new();
        let input_events = load_sse_fixture("input/anthropic_messages_full.sse").unwrap();
        let expected_events =
            load_sse_fixture("expected/anthropic_to_openai_chat_full.sse").unwrap();
        let mut output_events = Vec::new();

        for input_event in input_events {
            let NormalizedSseData::Json(chunk) = input_event.data else {
                continue;
            };
            output_events.extend(
                processor
                    .anthropic_to_openai_stream(&input_event.event, chunk)
                    .unwrap()
                    .into_events(),
            );
        }

        assert_eq!(normalize_stream_events(output_events), expected_events);
    }

    #[test]
    fn test_stream_usage_with_input_tokens_in_message_delta() {
        let processor = AnthropicToOpenAIStreamProcessor::new();

        // message_start without input_tokens
        let start = json!({
            "type": "message_start",
            "message": {
                "id": "msg_delta_input",
                "type": "message",
                "role": "assistant",
                "model": "claude-3-5-sonnet",
                "content": [],
                "stop_reason": null,
                "usage": {"input_tokens": 0, "output_tokens": 1}
            }
        });
        let _ = processor
            .anthropic_to_openai_stream("message_start", start)
            .unwrap();

        // Some content
        let block_start = json!({
            "type": "content_block_start",
            "index": 0,
            "content_block": {"type": "text", "text": ""}
        });
        let _ = processor
            .anthropic_to_openai_stream("content_block_start", block_start)
            .unwrap();

        let delta = json!({
            "type": "content_block_delta",
            "index": 0,
            "delta": {"type": "text_delta", "text": "Hello"}
        });
        let _ = processor
            .anthropic_to_openai_stream("content_block_delta", delta)
            .unwrap();

        let block_stop = json!({"type": "content_block_stop", "index": 0});
        let _ = processor
            .anthropic_to_openai_stream("content_block_stop", block_stop)
            .unwrap();

        // message_delta with input_tokens (provider sends it here instead)
        let msg_delta = json!({
            "type": "message_delta",
            "delta": {"stop_reason": "end_turn"},
            "usage": {"input_tokens": 200, "output_tokens": 50}
        });
        let result = processor
            .anthropic_to_openai_stream("message_delta", msg_delta)
            .unwrap();
        let events = result.into_events();
        assert_eq!(events.len(), 1);
        let event = &events[0].0;
        assert_eq!(event["usage"]["prompt_tokens"], 200);
        assert_eq!(event["usage"]["completion_tokens"], 50);
        assert_eq!(event["usage"]["total_tokens"], 250);
    }

    #[test]
    fn test_stream_usage_with_cache_tokens_in_message_delta() {
        let processor = AnthropicToOpenAIStreamProcessor::new();

        // message_start with input_tokens but no cache tokens
        let start = json!({
            "type": "message_start",
            "message": {
                "id": "msg_cache_delta",
                "type": "message",
                "role": "assistant",
                "model": "claude-3-5-sonnet",
                "content": [],
                "stop_reason": null,
                "usage": {"input_tokens": 80, "output_tokens": 1}
            }
        });
        let _ = processor
            .anthropic_to_openai_stream("message_start", start)
            .unwrap();

        // Some content
        let block_start = json!({
            "type": "content_block_start",
            "index": 0,
            "content_block": {"type": "text", "text": ""}
        });
        let _ = processor
            .anthropic_to_openai_stream("content_block_start", block_start)
            .unwrap();

        let block_stop = json!({"type": "content_block_stop", "index": 0});
        let _ = processor
            .anthropic_to_openai_stream("content_block_stop", block_stop)
            .unwrap();

        // message_delta with cache tokens
        let msg_delta = json!({
            "type": "message_delta",
            "delta": {"stop_reason": "end_turn"},
            "usage": {
                "output_tokens": 50,
                "cache_read_input_tokens": 20
            }
        });
        let result = processor
            .anthropic_to_openai_stream("message_delta", msg_delta)
            .unwrap();
        let events = result.into_events();
        assert_eq!(events.len(), 1);
        let event = &events[0].0;
        // prompt_tokens = input_tokens(80) + cache_read(20) = 100
        assert_eq!(event["usage"]["prompt_tokens"], 100);
        assert_eq!(event["usage"]["completion_tokens"], 50);
        assert_eq!(event["usage"]["total_tokens"], 150);
        assert_eq!(event["usage"]["prompt_tokens_details"]["cached_tokens"], 20);
    }

    #[test]
    fn test_stream_usage_with_all_input_tokens_in_message_delta() {
        let processor = AnthropicToOpenAIStreamProcessor::new();

        let start = json!({
            "type": "message_start",
            "message": {
                "id": "msg_all_usage_delta",
                "type": "message",
                "role": "assistant",
                "model": "claude-3-5-sonnet",
                "content": [],
                "stop_reason": null,
                "usage": {"input_tokens": 0, "output_tokens": 0}
            }
        });
        let _ = processor
            .anthropic_to_openai_stream("message_start", start)
            .unwrap();

        let msg_delta = json!({
            "type": "message_delta",
            "delta": {"stop_reason": "end_turn"},
            "usage": {
                "input_tokens": 60,
                "cache_read_input_tokens": 20,
                "cache_creation_input_tokens": 10,
                "output_tokens": 50
            }
        });
        let result = processor
            .anthropic_to_openai_stream("message_delta", msg_delta)
            .unwrap();
        let events = result.into_events();
        assert_eq!(events.len(), 1);
        let event = &events[0].0;
        assert_eq!(event["usage"]["prompt_tokens"], 90);
        assert_eq!(event["usage"]["completion_tokens"], 50);
        assert_eq!(event["usage"]["total_tokens"], 140);
        assert_eq!(event["usage"]["prompt_tokens_details"]["cached_tokens"], 20);
    }
}
