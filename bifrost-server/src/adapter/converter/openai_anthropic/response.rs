//! Transform Anthropic responses to OpenAI Chat Completions format
//!
//! This module provides functions to convert Anthropic API response format (non-SSE)
//! to OpenAI Chat Completions API format.

use crate::error::LlmMapError;
use serde_json::{Value, json};

/// Convert an Anthropic response to OpenAI Chat Completions format.
///
/// This function transforms an Anthropic Messages API response (non-streaming)
/// into the OpenAI Chat Completions API compatible format.
///
/// Anthropic response example:
/// ```json
/// {
///   "id": "msg_xxx",
///   "type": "message",
///   "role": "assistant",
///   "content": [
///     {"type": "text", "text": "Hello"},
///     {"type": "tool_use", "id": "tool_xxx", "name": "get_weather", "input": {"city": "Tokyo"}}
///   ],
///   "model": "claude-3-5-sonnet-20241022",
///   "stop_reason": "end_turn",
///   "usage": {"input_tokens": 100, "output_tokens": 50}
/// }
/// ```
///
/// OpenAI Chat Completions response example:
/// ```json
/// {
///   "id": "msg_xxx",
///   "object": "chat.completion",
///   "created": 1234567890,
///   "model": "claude-3-5-sonnet-20241022",
///   "choices": [{
///     "index": 0,
///     "message": {
///       "role": "assistant",
///       "content": "Hello"
///     },
///     "finish_reason": "stop"
///   }],
///   "usage": {
///     "prompt_tokens": 100,
///     "completion_tokens": 50,
///     "total_tokens": 150
///   }
/// }
/// ```
///
/// # Arguments
///
/// * `body` - The JSON response body in Anthropic format
///
/// # Returns
///
/// A `Result` containing the transformed response in OpenAI Chat Completions format,
/// or an `LlmMapError` if the transformation fails.
pub fn anthropic_to_openai_response(body: Value) -> Result<Value, LlmMapError> {
    let Value::Object(mut obj) = body else {
        return Err(LlmMapError::Validation(
            "Response body must be an object".into(),
        ));
    };

    let id = obj.remove("id").unwrap_or(Value::String(String::new()));
    let model = obj.remove("model").unwrap_or(Value::String(String::new()));
    let stop_reason = obj.remove("stop_reason");
    let usage = obj.remove("usage");
    let content = obj.remove("content");

    // Transform Anthropic content blocks to OpenAI message content
    let (message_content, finish_reason, reasoning_content, tool_calls) =
        transform_content_to_message(content, stop_reason)?;

    // Get current timestamp for "created" field
    let created = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    // Transform usage from Anthropic format to OpenAI format
    let transformed_usage = transform_usage(usage);

    let mut message_obj = serde_json::Map::new();
    message_obj.insert("role".to_string(), Value::String("assistant".into()));
    message_obj.insert("content".to_string(), message_content);
    if let Some(reasoning) = reasoning_content {
        message_obj.insert("reasoning_content".to_string(), Value::String(reasoning));
    }
    if !tool_calls.is_empty() {
        message_obj.insert("tool_calls".to_string(), Value::Array(tool_calls));
    }

    let choices = json!([{
        "index": 0,
        "message": Value::Object(message_obj),
        "finish_reason": finish_reason
    }]);

    let mut result = serde_json::Map::new();
    result.insert("id".to_string(), id);
    result.insert(
        "object".to_string(),
        Value::String("chat.completion".into()),
    );
    result.insert("created".to_string(), Value::Number(created.into()));
    result.insert("model".to_string(), model);
    result.insert("choices".to_string(), choices);

    if let Some(usage_val) = transformed_usage {
        result.insert("usage".to_string(), usage_val);
    }

    Ok(Value::Object(result))
}

/// Transform Anthropic content blocks to OpenAI message content format.
///
/// Returns a tuple of (transformed_content, finish_reason, reasoning_content, tool_calls).
fn transform_content_to_message(
    content: Option<Value>,
    stop_reason: Option<Value>,
) -> Result<(Value, Value, Option<String>, Vec<Value>), LlmMapError> {
    let content = match content {
        Some(Value::Array(arr)) => arr,
        Some(other) => vec![other],
        None => return Ok((Value::Null, map_stop_reason(stop_reason), None, Vec::new())),
    };

    if content.is_empty() {
        return Ok((Value::Null, map_stop_reason(stop_reason), None, Vec::new()));
    }

    let mut text_parts: Vec<String> = Vec::new();
    let mut tool_calls: Vec<Value> = Vec::new();
    let mut reasoning_content: Option<String> = None;

    for block in content {
        let Value::Object(block_obj) = block else {
            continue;
        };

        match block_obj.get("type").and_then(|v| v.as_str()) {
            Some("thinking") => {
                if let Some(thinking) = block_obj.get("thinking").and_then(|v| v.as_str())
                    && reasoning_content.is_none()
                {
                    reasoning_content = Some(thinking.to_string());
                }
            }
            Some("text") => {
                if let Some(text) = block_obj.get("text").and_then(|v| v.as_str()) {
                    text_parts.push(text.to_string());
                }
            }
            Some("tool_use") => {
                let id = block_obj.get("id").cloned().unwrap_or(Value::Null);
                let name = block_obj.get("name").cloned().unwrap_or(Value::Null);
                let input = block_obj.get("input").cloned().unwrap_or(json!({}));
                let arguments = serde_json::to_string(&input).unwrap_or_else(|_| "{}".to_string());

                tool_calls.push(json!({
                    "id": id,
                    "type": "function",
                    "function": {
                        "name": name,
                        "arguments": arguments
                    }
                }));
            }
            _ => {}
        }
    }

    let combined_text = text_parts.join("");
    let message_content = if combined_text.trim().is_empty() {
        Value::Null
    } else {
        Value::String(combined_text)
    };

    Ok((
        message_content,
        map_stop_reason(stop_reason),
        reasoning_content,
        tool_calls,
    ))
}

/// Map Anthropic stop_reason to OpenAI finish_reason.
fn map_stop_reason(stop_reason: Option<Value>) -> Value {
    match stop_reason {
        Some(Value::String(s)) => match s.as_str() {
            "end_turn" | "stop" => Value::String("stop".to_string()),
            "tool_use" => Value::String("tool_calls".to_string()),
            "max_tokens" => Value::String("length".to_string()),
            _ => Value::Null,
        },
        _ => Value::Null,
    }
}

/// Transform Anthropic usage to OpenAI usage format.
///
/// Anthropic: { "input_tokens": 100, "output_tokens": 50 }
/// OpenAI: { "prompt_tokens": 100, "completion_tokens": 50, "total_tokens": 150 }
fn transform_usage(usage: Option<Value>) -> Option<Value> {
    let Value::Object(usage_obj) = usage? else {
        return None;
    };

    let input_tokens = usage_obj
        .get("input_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let output_tokens = usage_obj
        .get("output_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    Some(json!({
        "prompt_tokens": input_tokens,
        "completion_tokens": output_tokens,
        "total_tokens": input_tokens + output_tokens
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_text_response() {
        let input = json!({
            "id": "msg_abc123",
            "type": "message",
            "role": "assistant",
            "content": [
                {"type": "text", "text": "Hello, world!"}
            ],
            "model": "claude-3-5-sonnet-20241022",
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 10, "output_tokens": 5}
        });

        let result = anthropic_to_openai_response(input).unwrap();
        let expected = json!({
            "id": "msg_abc123",
            "object": "chat.completion",
            "created": result["created"],
            "model": "claude-3-5-sonnet-20241022",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "Hello, world!"
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 5,
                "total_tokens": 15
            }
        });
        assert_eq!(result, expected);
    }

    #[test]
    fn test_empty_content_response() {
        let input = json!({
            "id": "msg_empty",
            "type": "message",
            "role": "assistant",
            "content": [],
            "model": "claude-3-5-sonnet-20241022",
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 5, "output_tokens": 0}
        });

        let result = anthropic_to_openai_response(input).unwrap();
        let expected = json!({
            "id": "msg_empty",
            "object": "chat.completion",
            "created": result["created"],
            "model": "claude-3-5-sonnet-20241022",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": null
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 5,
                "completion_tokens": 0,
                "total_tokens": 5
            }
        });
        assert_eq!(result, expected);
    }

    #[test]
    fn test_null_content_response() {
        let input = json!({
            "id": "msg_null",
            "type": "message",
            "role": "assistant",
            "content": null,
            "model": "claude-3-5-sonnet-20241022",
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 5, "output_tokens": 0}
        });

        let result = anthropic_to_openai_response(input).unwrap();
        let expected = json!({
            "id": "msg_null",
            "object": "chat.completion",
            "created": result["created"],
            "model": "claude-3-5-sonnet-20241022",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": null
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 5,
                "completion_tokens": 0,
                "total_tokens": 5
            }
        });
        assert_eq!(result, expected);
    }

    #[test]
    fn test_tool_use_response() {
        let input = json!({
            "id": "msg_tool",
            "type": "message",
            "role": "assistant",
            "content": [
                {"type": "tool_use", "id": "toolu_abc", "name": "get_weather", "input": {"city": "Tokyo"}}
            ],
            "model": "claude-3-5-sonnet-20241022",
            "stop_reason": "tool_use",
            "usage": {"input_tokens": 15, "output_tokens": 10}
        });

        let result = anthropic_to_openai_response(input).unwrap();
        let expected = json!({
            "id": "msg_tool",
            "object": "chat.completion",
            "created": result["created"],
            "model": "claude-3-5-sonnet-20241022",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [
                        {"id": "toolu_abc", "type": "function", "function": {"name": "get_weather", "arguments": "{\"city\":\"Tokyo\"}"}}
                    ]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": {
                "prompt_tokens": 15,
                "completion_tokens": 10,
                "total_tokens": 25
            }
        });
        assert_eq!(result, expected);
    }

    #[test]
    fn test_text_and_tool_use_response() {
        let input = json!({
            "id": "msg_mixed",
            "type": "message",
            "role": "assistant",
            "content": [
                {"type": "text", "text": "Let me check the weather."},
                {"type": "tool_use", "id": "toolu_xyz", "name": "get_weather", "input": {"city": "Tokyo"}}
            ],
            "model": "claude-3-5-sonnet-20241022",
            "stop_reason": "tool_use",
            "usage": {"input_tokens": 20, "output_tokens": 15}
        });

        let result = anthropic_to_openai_response(input).unwrap();
        let expected = json!({
            "id": "msg_mixed",
            "object": "chat.completion",
            "created": result["created"],
            "model": "claude-3-5-sonnet-20241022",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "Let me check the weather.",
                    "tool_calls": [
                        {"id": "toolu_xyz", "type": "function", "function": {"name": "get_weather", "arguments": "{\"city\":\"Tokyo\"}"}}
                    ]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": {
                "prompt_tokens": 20,
                "completion_tokens": 15,
                "total_tokens": 35
            }
        });
        assert_eq!(result, expected);
    }

    #[test]
    fn test_multiple_tool_use_response() {
        let input = json!({
            "id": "msg_multi_tool",
            "type": "message",
            "role": "assistant",
            "content": [
                {"type": "tool_use", "id": "toolu_1", "name": "get_weather", "input": {"city": "Tokyo"}},
                {"type": "tool_use", "id": "toolu_2", "name": "get_time", "input": {"timezone": "JST"}}
            ],
            "model": "claude-3-5-sonnet-20241022",
            "stop_reason": "tool_use",
            "usage": {"input_tokens": 25, "output_tokens": 20}
        });

        let result = anthropic_to_openai_response(input).unwrap();
        let expected = json!({
            "id": "msg_multi_tool",
            "object": "chat.completion",
            "created": result["created"],
            "model": "claude-3-5-sonnet-20241022",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [
                        {"id": "toolu_1", "type": "function", "function": {"name": "get_weather", "arguments": "{\"city\":\"Tokyo\"}"}},
                        {"id": "toolu_2", "type": "function", "function": {"name": "get_time", "arguments": "{\"timezone\":\"JST\"}"}}
                    ]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": {
                "prompt_tokens": 25,
                "completion_tokens": 20,
                "total_tokens": 45
            }
        });
        assert_eq!(result, expected);
    }

    #[test]
    fn test_max_tokens_stop_reason() {
        let input = json!({
            "id": "msg_max",
            "type": "message",
            "role": "assistant",
            "content": [{"type": "text", "text": "This is trunc"}],
            "model": "claude-3-5-sonnet-20241022",
            "stop_reason": "max_tokens",
            "usage": {"input_tokens": 100, "output_tokens": 4096}
        });

        let result = anthropic_to_openai_response(input).unwrap();
        let expected = json!({
            "id": "msg_max",
            "object": "chat.completion",
            "created": result["created"],
            "model": "claude-3-5-sonnet-20241022",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "This is trunc"
                },
                "finish_reason": "length"
            }],
            "usage": {
                "prompt_tokens": 100,
                "completion_tokens": 4096,
                "total_tokens": 4196
            }
        });
        assert_eq!(result, expected);
    }

    #[test]
    fn test_null_stop_reason() {
        let input = json!({
            "id": "msg_no_stop",
            "type": "message",
            "role": "assistant",
            "content": [{"type": "text", "text": "Hello"}],
            "model": "claude-3-5-sonnet-20241022",
            "stop_reason": null,
            "usage": {"input_tokens": 5, "output_tokens": 1}
        });

        let result = anthropic_to_openai_response(input).unwrap();
        let expected = json!({
            "id": "msg_no_stop",
            "object": "chat.completion",
            "created": result["created"],
            "model": "claude-3-5-sonnet-20241022",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "Hello"
                },
                "finish_reason": null
            }],
            "usage": {
                "prompt_tokens": 5,
                "completion_tokens": 1,
                "total_tokens": 6
            }
        });
        assert_eq!(result, expected);
    }

    #[test]
    fn test_missing_usage() {
        let input = json!({
            "id": "msg_no_usage",
            "type": "message",
            "role": "assistant",
            "content": [{"type": "text", "text": "Hello"}],
            "model": "claude-3-5-sonnet-20241022",
            "stop_reason": "end_turn"
        });

        let result = anthropic_to_openai_response(input).unwrap();
        let expected = json!({
            "id": "msg_no_usage",
            "object": "chat.completion",
            "created": result["created"],
            "model": "claude-3-5-sonnet-20241022",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "Hello"
                },
                "finish_reason": "stop"
            }]
        });
        assert_eq!(result, expected);
    }

    #[test]
    fn test_missing_optional_fields() {
        let input = json!({
            "id": "msg_minimal",
            "content": [{"type": "text", "text": "Hi"}]
        });

        let result = anthropic_to_openai_response(input).unwrap();
        let expected = json!({
            "id": "msg_minimal",
            "object": "chat.completion",
            "created": result["created"],
            "model": "",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "Hi"
                },
                "finish_reason": null
            }]
        });
        assert_eq!(result, expected);
    }

    #[test]
    fn test_invalid_body_not_object() {
        let input = json!("not an object");
        let result = anthropic_to_openai_response(input);
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_body_array() {
        let input = json!([1, 2, 3]);
        let result = anthropic_to_openai_response(input);
        assert!(result.is_err());
    }

    #[test]
    fn test_tool_use_with_empty_input() {
        let input = json!({
            "id": "msg_empty_tool",
            "type": "message",
            "role": "assistant",
            "content": [
                {"type": "tool_use", "id": "toolu_empty", "name": "do_nothing", "input": {}}
            ],
            "model": "claude-3-5-sonnet-20241022",
            "stop_reason": "tool_use",
            "usage": {"input_tokens": 10, "output_tokens": 5}
        });

        let result = anthropic_to_openai_response(input).unwrap();
        let expected = json!({
            "id": "msg_empty_tool",
            "object": "chat.completion",
            "created": result["created"],
            "model": "claude-3-5-sonnet-20241022",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [
                        {"id": "toolu_empty", "type": "function", "function": {"name": "do_nothing", "arguments": "{}"}}
                    ]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 5,
                "total_tokens": 15
            }
        });
        assert_eq!(result, expected);
    }

    #[test]
    fn test_complex_nested_input() {
        let input = json!({
            "id": "msg_complex",
            "type": "message",
            "role": "assistant",
            "content": [
                {"type": "text", "text": "The weather is "},
                {"type": "text", "text": "sunny today."},
                {"type": "tool_use", "id": "toolu_nested", "name": "search", "input": {"query": "weather Tokyo", "filters": {"type": "accurate"}}}
            ],
            "model": "claude-3-5-sonnet-20241022",
            "stop_reason": "tool_use",
            "usage": {"input_tokens": 50, "output_tokens": 30}
        });

        let result = anthropic_to_openai_response(input).unwrap();
        let expected = json!({
            "id": "msg_complex",
            "object": "chat.completion",
            "created": result["created"],
            "model": "claude-3-5-sonnet-20241022",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "The weather is sunny today.",
                    "tool_calls": [
                        {"id": "toolu_nested", "type": "function", "function": {"name": "search", "arguments": "{\"filters\":{\"type\":\"accurate\"},\"query\":\"weather Tokyo\"}"}}
                    ]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": {
                "prompt_tokens": 50,
                "completion_tokens": 30,
                "total_tokens": 80
            }
        });
        assert_eq!(result, expected);
    }

    #[test]
    fn test_empty_text_blocks_filtered() {
        let input = json!({
            "id": "msg_empty_text",
            "type": "message",
            "role": "assistant",
            "content": [
                {"type": "text", "text": ""},
                {"type": "tool_use", "id": "toolu_filter", "name": "test", "input": {}}
            ],
            "model": "claude-3-5-sonnet-20241022",
            "stop_reason": "tool_use",
            "usage": {"input_tokens": 5, "output_tokens": 3}
        });

        let result = anthropic_to_openai_response(input).unwrap();
        let expected = json!({
            "id": "msg_empty_text",
            "object": "chat.completion",
            "created": result["created"],
            "model": "claude-3-5-sonnet-20241022",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [
                        {"id": "toolu_filter", "type": "function", "function": {"name": "test", "arguments": "{}"}}
                    ]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": {
                "prompt_tokens": 5,
                "completion_tokens": 3,
                "total_tokens": 8
            }
        });
        assert_eq!(result, expected);
    }

    #[test]
    fn test_whitespace_only_text_filtered() {
        let input = json!({
            "id": "msg_whitespace",
            "type": "message",
            "role": "assistant",
            "content": [
                {"type": "text", "text": "   "},
                {"type": "text", "text": ""},
                {"type": "tool_use", "id": "toolu_ws", "name": "test", "input": {}}
            ],
            "model": "claude-3-5-sonnet-20241022",
            "stop_reason": "tool_use",
            "usage": {"input_tokens": 5, "output_tokens": 3}
        });

        let result = anthropic_to_openai_response(input).unwrap();
        let expected = json!({
            "id": "msg_whitespace",
            "object": "chat.completion",
            "created": result["created"],
            "model": "claude-3-5-sonnet-20241022",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [
                        {"id": "toolu_ws", "type": "function", "function": {"name": "test", "arguments": "{}"}}
                    ]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": {
                "prompt_tokens": 5,
                "completion_tokens": 3,
                "total_tokens": 8
            }
        });
        assert_eq!(result, expected);
    }

    #[test]
    fn test_multiple_text_blocks_converted_to_single_string() {
        let input = json!({
            "id": "msg_multi_text",
            "type": "message",
            "role": "assistant",
            "content": [
                {"type": "text", "text": "Hello"},
                {"type": "text", "text": " "},
                {"type": "text", "text": "World"}
            ],
            "model": "claude-3-5-sonnet-20241022",
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 10, "output_tokens": 3}
        });

        let result = anthropic_to_openai_response(input).unwrap();
        let expected = json!({
            "id": "msg_multi_text",
            "object": "chat.completion",
            "created": result["created"],
            "model": "claude-3-5-sonnet-20241022",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "Hello World"
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 3,
                "total_tokens": 13
            }
        });
        assert_eq!(result, expected);
    }

    #[test]
    fn test_tool_use_with_special_characters_in_arguments() {
        let input = json!({
            "id": "msg_special",
            "type": "message",
            "role": "assistant",
            "content": [
                {"type": "tool_use", "id": "toolu_special", "name": "escape_test", "input": {
                    "json_string": "{\"key\": \"value with \\\"quotes\\\" and \n newlines\"}",
                    "unicode": "Hello 世界 🌍",
                    "emoji": "😀🎉"
                }}
            ],
            "model": "claude-3-5-sonnet-20241022",
            "stop_reason": "tool_use",
            "usage": {"input_tokens": 25, "output_tokens": 20}
        });

        let result = anthropic_to_openai_response(input).unwrap();
        let expected = json!({
            "id": "msg_special",
            "object": "chat.completion",
            "created": result["created"],
            "model": "claude-3-5-sonnet-20241022",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [
                        {"id": "toolu_special", "type": "function", "function": {"name": "escape_test", "arguments": "{\"emoji\":\"😀🎉\",\"json_string\":\"{\\\"key\\\": \\\"value with \\\\\\\"quotes\\\\\\\" and \\n newlines\\\"}\",\"unicode\":\"Hello 世界 🌍\"}"}}
                    ]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": {
                "prompt_tokens": 25,
                "completion_tokens": 20,
                "total_tokens": 45
            }
        });
        assert_eq!(result, expected);
    }

    #[test]
    fn test_thinking_block_ignored() {
        let input = json!({
            "id": "msg_thinking",
            "type": "message",
            "role": "assistant",
            "content": [
                {"type": "thinking", "thinking": "Let me think about this..."},
                {"type": "text", "text": "Here's my answer."}
            ],
            "model": "claude-3-5-sonnet-20241022",
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 20, "output_tokens": 10}
        });

        let result = anthropic_to_openai_response(input).unwrap();
        let expected = json!({
            "id": "msg_thinking",
            "object": "chat.completion",
            "created": result["created"],
            "model": "claude-3-5-sonnet-20241022",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "Here's my answer.",
                    "reasoning_content": "Let me think about this..."
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 20,
                "completion_tokens": 10,
                "total_tokens": 30
            }
        });
        assert_eq!(result, expected);
    }

    #[test]
    fn test_thinking_text_and_tools() {
        let input = json!({
            "id": "msg_full",
            "type": "message",
            "role": "assistant",
            "content": [
                {"type": "thinking", "thinking": "I need to check the weather and search for news."},
                {"type": "text", "text": "Let me check the weather in Tokyo and search for news."},
                {"type": "tool_use", "id": "toolu_1", "name": "get_weather", "input": {"city": "Tokyo", "country": "Japan"}},
                {"type": "tool_use", "id": "toolu_2", "name": "search_news", "input": {"query": "Tokyo weather", "limit": 5}}
            ],
            "model": "claude-3-5-sonnet-20241022",
            "stop_reason": "tool_use",
            "usage": {"input_tokens": 150, "output_tokens": 85}
        });

        let result = anthropic_to_openai_response(input).unwrap();
        let expected = json!({
            "id": "msg_full",
            "object": "chat.completion",
            "created": result["created"],
            "model": "claude-3-5-sonnet-20241022",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "Let me check the weather in Tokyo and search for news.",
                    "reasoning_content": "I need to check the weather and search for news.",
                    "tool_calls": [
                        {"id": "toolu_1", "type": "function", "function": {"name": "get_weather", "arguments": "{\"city\":\"Tokyo\",\"country\":\"Japan\"}"}},
                        {"id": "toolu_2", "type": "function", "function": {"name": "search_news", "arguments": "{\"limit\":5,\"query\":\"Tokyo weather\"}"}}
                    ]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": {
                "prompt_tokens": 150,
                "completion_tokens": 85,
                "total_tokens": 235
            }
        });
        assert_eq!(result, expected);
    }

    #[test]
    fn test_only_thinking_block() {
        let input = json!({
            "id": "msg_only_thinking",
            "type": "message",
            "role": "assistant",
            "content": [
                {"type": "thinking", "thinking": "Just thinking..."}
            ],
            "model": "claude-3-5-sonnet-20241022",
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 10, "output_tokens": 5}
        });

        let result = anthropic_to_openai_response(input).unwrap();
        let expected = json!({
            "id": "msg_only_thinking",
            "object": "chat.completion",
            "created": result["created"],
            "model": "claude-3-5-sonnet-20241022",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": null,
                    "reasoning_content": "Just thinking..."
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 5,
                "total_tokens": 15
            }
        });
        assert_eq!(result, expected);
    }

    #[test]
    fn test_full_response_with_all_content_types() {
        let input = json!({
            "id": "msg_complete",
            "type": "message",
            "role": "assistant",
            "content": [
                {"type": "thinking", "thinking": "First I'll calculate, then search."},
                {"type": "text", "text": "Let me "},
                {"type": "text", "text": "run both queries."},
                {"type": "tool_use", "id": "call_1", "name": "calculate", "input": {"expr": "15 * 23 + 9"}},
                {"type": "tool_use", "id": "call_2", "name": "search", "input": {"q": "weather Tokyo"}},
                {"type": "thinking", "thinking": "Got results, now I can answer."},
                {"type": "text", "text": "The answer is 354. Weather in Tokyo is sunny."}
            ],
            "model": "claude-3-5-sonnet-20241022",
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 200, "output_tokens": 120}
        });

        let result = anthropic_to_openai_response(input).unwrap();
        let expected = json!({
            "id": "msg_complete",
            "object": "chat.completion",
            "created": result["created"],
            "model": "claude-3-5-sonnet-20241022",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "Let me run both queries.The answer is 354. Weather in Tokyo is sunny.",
                    "reasoning_content": "First I'll calculate, then search.",
                    "tool_calls": [
                        {"id": "call_1", "type": "function", "function": {"name": "calculate", "arguments": "{\"expr\":\"15 * 23 + 9\"}"}},
                        {"id": "call_2", "type": "function", "function": {"name": "search", "arguments": "{\"q\":\"weather Tokyo\"}"}}
                    ]
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 200,
                "completion_tokens": 120,
                "total_tokens": 320
            }
        });
        assert_eq!(result, expected);
    }
}
