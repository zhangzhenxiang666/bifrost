//! Transform OpenAI responses to Anthropic format
//!
//! This module provides functions to convert OpenAI API response format to Anthropic format.

use crate::adapter::converter::create_null_string;
use crate::error::LlmMapError;
use serde_json::{Value, json};

/// Transform usage from OpenAI format to Anthropic format
pub fn transform_usage_openai_to_anthropic(usage: Option<&Value>) -> Value {
    let Some(usage_obj) = usage.and_then(|v| v.as_object()) else {
        return json!({
            "input_tokens": 0,
            "output_tokens": 0
        });
    };

    let input_tokens = usage_obj
        .get("prompt_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;

    let output_tokens = usage_obj
        .get("completion_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;

    // For Anthropic, we just use output_tokens
    // (OpenAI's completion_tokens already includes reasoning if present)
    json!({
        "input_tokens": input_tokens,
        "output_tokens": output_tokens
    })
}

/// Transform OpenAI response to Anthropic response format
pub fn openai_to_anthropic_response(body: Value) -> Result<Value, LlmMapError> {
    let Value::Object(mut obj) = body else {
        return Err(LlmMapError::Validation(
            "Response body must be an object".into(),
        ));
    };

    // Extract basic fields
    let id = obj.remove("id").unwrap_or_else(create_null_string);
    let model = obj.remove("model").unwrap_or_else(create_null_string);
    let created = obj.remove("created");

    // Extract choices array
    let Some(Value::Array(choices)) = obj.remove("choices") else {
        return Err(LlmMapError::Validation("choices array is required".into()));
    };

    // We only process the first choice (index 0)
    let Some(Value::Object(mut first_choice)) = choices.into_iter().next() else {
        return Err(LlmMapError::Validation(
            "choices array must have at least one element".into(),
        ));
    };

    let Some(Value::Object(mut message)) = first_choice.remove("message") else {
        return Err(LlmMapError::Validation("message object is required".into()));
    };

    let finish_reason = first_choice.get("finish_reason").and_then(|v| v.as_str());

    // Build content array from message
    let mut content: Vec<Value> = Vec::new();

    // 1. Handle reasoning_content if present (for o1/o3 models)
    if let Some(reasoning) = message.get("reasoning_content").and_then(|v| v.as_str()) {
        // Note: Anthropic uses "thinking" block type, not "reasoning_content"
        // But for compatibility, we'll use the thinking block format
        content.push(json!({
            "type": "thinking",
            "thinking": reasoning
        }));
    }

    // 2. Handle regular content (text)
    if let Some(content_value) = message.get("content") {
        if let Some(text) = content_value.as_str() {
            if !text.is_empty() {
                content.push(json!({
                    "type": "text",
                    "text": text
                }));
            }
        } else if let Some(arr) = content_value.as_array() {
            // Handle array content (less common in OpenAI responses)
            for block in arr {
                if let Some(text) = block.get("text").and_then(|v| v.as_str()) {
                    content.push(json!({
                        "type": "text",
                        "text": text
                    }));
                }
            }
        }
    }

    // 3. Handle tool_calls
    if let Some(Value::Array(tool_calls)) = message.remove("tool_calls") {
        for tool_call in tool_calls {
            if let Value::Object(mut tc_obj) = tool_call {
                let id = tc_obj.remove("id").unwrap_or_else(create_null_string);

                let Some(Value::Object(mut function)) = tc_obj.remove("function") else {
                    continue;
                };

                let name = function.remove("name").unwrap_or_else(create_null_string);

                // Parse arguments from JSON string to object
                let input = function
                    .remove("arguments")
                    .and_then(|v| {
                        v.as_str().map(|s| {
                            serde_json::from_str(s)
                                .unwrap_or_else(|_| Value::Object(Default::default()))
                        })
                    })
                    .unwrap_or_else(|| Value::Object(serde_json::Map::new()));

                content.push(json!({
                    "type": "tool_use",
                    "id": id,
                    "name": name,
                    "input": input
                }));
            }
        }
    }

    // Map finish_reason to stop_reason
    let stop_reason = match finish_reason {
        Some("tool_calls") => "tool_use",
        Some("stop") => "end_turn",
        Some("length") => "max_tokens",
        _ => "end_turn",
    };

    // Transform usage
    let usage = transform_usage_openai_to_anthropic(obj.get("usage"));

    // Build final response
    let mut response = serde_json::Map::new();
    response.insert("id".into(), id);
    response.insert("type".into(), Value::String("message".into()));
    response.insert("role".into(), Value::String("assistant".into()));
    response.insert("content".into(), Value::Array(content));
    response.insert("model".into(), model);
    response.insert("stop_reason".into(), Value::String(stop_reason.into()));
    response.insert("stop_sequence".into(), Value::Null);
    response.insert("usage".into(), usage);

    // Add created timestamp if present (Anthropic doesn't have this field, but we can store it)
    if let Some(created_val) = created {
        response.insert("created".into(), created_val);
    }

    Ok(Value::Object(response))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ============================================
    // Basic response transformation tests
    // ============================================

    #[test]
    fn test_simple_text_response() {
        let input = json!({
            "id": "msg_abc123",
            "model": "gpt-4",
            "choices": [{
                "index": 0,
                "finish_reason": "stop",
                "message": {
                    "role": "assistant",
                    "content": "Hello, world!"
                }
            }]
        });

        let expected = json!({
            "id": "msg_abc123",
            "type": "message",
            "role": "assistant",
            "model": "gpt-4",
            "content": [{"type": "text", "text": "Hello, world!"}],
            "stop_reason": "end_turn",
            "stop_sequence": null,
            "usage": {"input_tokens": 0, "output_tokens": 0}
        });

        let result = openai_to_anthropic_response(input).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_response_with_id_and_model() {
        let input = json!({
            "id": "msg_xyz789",
            "model": "claude-sonnet-4-20250514",
            "choices": [{
                "index": 0,
                "finish_reason": "stop",
                "message": {
                    "role": "assistant",
                    "content": "Test response"
                }
            }]
        });

        let expected = json!({
            "id": "msg_xyz789",
            "type": "message",
            "role": "assistant",
            "model": "claude-sonnet-4-20250514",
            "content": [{"type": "text", "text": "Test response"}],
            "stop_reason": "end_turn",
            "stop_sequence": null,
            "usage": {"input_tokens": 0, "output_tokens": 0}
        });

        let result = openai_to_anthropic_response(input).unwrap();
        assert_eq!(result, expected);
    }

    // ============================================
    // Content type transformation tests
    // ============================================

    #[test]
    fn test_text_content_as_string() {
        let input = json!({
            "id": "msg_1",
            "model": "gpt-4",
            "choices": [{
                "index": 0,
                "finish_reason": "stop",
                "message": {
                    "role": "assistant",
                    "content": "Simple text response"
                }
            }]
        });

        let expected = json!({
            "id": "msg_1",
            "type": "message",
            "role": "assistant",
            "model": "gpt-4",
            "content": [{"type": "text", "text": "Simple text response"}],
            "stop_reason": "end_turn",
            "stop_sequence": null,
            "usage": {"input_tokens": 0, "output_tokens": 0}
        });

        let result = openai_to_anthropic_response(input).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_text_content_as_array() {
        let input = json!({
            "id": "msg_1",
            "model": "gpt-4",
            "choices": [{
                "index": 0,
                "finish_reason": "stop",
                "message": {
                    "role": "assistant",
                    "content": [
                        {"type": "text", "text": "First part."},
                        {"type": "text", "text": "Second part."}
                    ]
                }
            }]
        });

        let expected = json!({
            "id": "msg_1",
            "type": "message",
            "role": "assistant",
            "model": "gpt-4",
            "content": [
                {"type": "text", "text": "First part."},
                {"type": "text", "text": "Second part."}
            ],
            "stop_reason": "end_turn",
            "stop_sequence": null,
            "usage": {"input_tokens": 0, "output_tokens": 0}
        });

        let result = openai_to_anthropic_response(input).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_empty_content() {
        let input = json!({
            "id": "msg_1",
            "model": "gpt-4",
            "choices": [{
                "index": 0,
                "finish_reason": "stop",
                "message": {
                    "role": "assistant",
                    "content": ""
                }
            }]
        });

        let expected = json!({
            "id": "msg_1",
            "type": "message",
            "role": "assistant",
            "model": "gpt-4",
            "content": [],
            "stop_reason": "end_turn",
            "stop_sequence": null,
            "usage": {"input_tokens": 0, "output_tokens": 0}
        });

        let result = openai_to_anthropic_response(input).unwrap();
        assert_eq!(result, expected);
    }

    // ============================================
    // Reasoning content tests (o1/o3 models)
    // ============================================

    #[test]
    fn test_reasoning_content_in_response() {
        let input = json!({
            "id": "msg_1",
            "model": "gpt-4",
            "choices": [{
                "index": 0,
                "finish_reason": "stop",
                "message": {
                    "role": "assistant",
                    "content": "The answer is 42.",
                    "reasoning_content": "Let me think about this problem step by step..."
                }
            }]
        });

        let expected = json!({
            "id": "msg_1",
            "type": "message",
            "role": "assistant",
            "model": "gpt-4",
            "content": [
                {"type": "thinking", "thinking": "Let me think about this problem step by step..."},
                {"type": "text", "text": "The answer is 42."}
            ],
            "stop_reason": "end_turn",
            "stop_sequence": null,
            "usage": {"input_tokens": 0, "output_tokens": 0}
        });

        let result = openai_to_anthropic_response(input).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_empty_reasoning_content_still_creates_thinking_block() {
        let input = json!({
            "id": "msg_1",
            "model": "gpt-4",
            "choices": [{
                "index": 0,
                "finish_reason": "stop",
                "message": {
                    "role": "assistant",
                    "content": "Hello!",
                    "reasoning_content": ""
                }
            }]
        });

        let expected = json!({
            "id": "msg_1",
            "type": "message",
            "role": "assistant",
            "model": "gpt-4",
            "content": [
                {"type": "thinking", "thinking": ""},
                {"type": "text", "text": "Hello!"}
            ],
            "stop_reason": "end_turn",
            "stop_sequence": null,
            "usage": {"input_tokens": 0, "output_tokens": 0}
        });

        let result = openai_to_anthropic_response(input).unwrap();
        assert_eq!(result, expected);
    }

    // ============================================
    // Tool calls transformation tests
    // ============================================

    #[test]
    fn test_single_tool_call() {
        let input = json!({
            "id": "msg_1",
            "model": "gpt-4",
            "choices": [{
                "index": 0,
                "finish_reason": "tool_calls",
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_abc123",
                        "type": "function",
                        "function": {
                            "name": "get_weather",
                            "arguments": "{\"city\": \"Tokyo\"}"
                        }
                    }]
                }
            }]
        });

        let expected = json!({
            "id": "msg_1",
            "type": "message",
            "role": "assistant",
            "model": "gpt-4",
            "content": [{
                "type": "tool_use",
                "id": "call_abc123",
                "name": "get_weather",
                "input": {"city": "Tokyo"}
            }],
            "stop_reason": "tool_use",
            "stop_sequence": null,
            "usage": {"input_tokens": 0, "output_tokens": 0}
        });

        let result = openai_to_anthropic_response(input).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_multiple_tool_calls() {
        let input = json!({
            "id": "msg_1",
            "model": "gpt-4",
            "choices": [{
                "index": 0,
                "finish_reason": "tool_calls",
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [
                        {
                            "id": "call_1",
                            "type": "function",
                            "function": {"name": "get_weather", "arguments": "{\"city\": \"Tokyo\"}"}
                        },
                        {
                            "id": "call_2",
                            "type": "function",
                            "function": {"name": "get_time", "arguments": "{\"timezone\": \"JST\"}"}
                        }
                    ]
                }
            }]
        });

        let expected = json!({
            "id": "msg_1",
            "type": "message",
            "role": "assistant",
            "model": "gpt-4",
            "content": [
                {"type": "tool_use", "id": "call_1", "name": "get_weather", "input": {"city": "Tokyo"}},
                {"type": "tool_use", "id": "call_2", "name": "get_time", "input": {"timezone": "JST"}}
            ],
            "stop_reason": "tool_use",
            "stop_sequence": null,
            "usage": {"input_tokens": 0, "output_tokens": 0}
        });

        let result = openai_to_anthropic_response(input).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_tool_call_with_complex_arguments() {
        let input = json!({
            "id": "msg_1",
            "model": "gpt-4",
            "choices": [{
                "index": 0,
                "finish_reason": "tool_calls",
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_complex",
                        "type": "function",
                        "function": {
                            "name": "search_recipes",
                            "arguments": "{\"query\": \"pasta\", \"filters\": {\"cuisine\": \"Italian\", \"max_time\": 30}}"
                        }
                    }]
                }
            }]
        });

        let expected = json!({
            "id": "msg_1",
            "type": "message",
            "role": "assistant",
            "model": "gpt-4",
            "content": [{
                "type": "tool_use",
                "id": "call_complex",
                "name": "search_recipes",
                "input": {"query": "pasta", "filters": {"cuisine": "Italian", "max_time": 30}}
            }],
            "stop_reason": "tool_use",
            "stop_sequence": null,
            "usage": {"input_tokens": 0, "output_tokens": 0}
        });

        let result = openai_to_anthropic_response(input).unwrap();
        assert_eq!(result, expected);
    }

    // ============================================
    // Usage transformation tests
    // ============================================

    #[test]
    fn test_usage_with_tokens() {
        let input = json!({
            "id": "msg_1",
            "model": "gpt-4",
            "choices": [{
                "index": 0,
                "finish_reason": "stop",
                "message": {"role": "assistant", "content": "Hi"}
            }],
            "usage": {
                "prompt_tokens": 100,
                "completion_tokens": 50,
                "total_tokens": 150
            }
        });

        let expected = json!({
            "id": "msg_1",
            "type": "message",
            "role": "assistant",
            "model": "gpt-4",
            "content": [{"type": "text", "text": "Hi"}],
            "stop_reason": "end_turn",
            "stop_sequence": null,
            "usage": {"input_tokens": 100, "output_tokens": 50}
        });

        let result = openai_to_anthropic_response(input).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_usage_with_large_numbers() {
        let input = json!({
            "id": "msg_1",
            "model": "gpt-4",
            "choices": [{
                "index": 0,
                "finish_reason": "stop",
                "message": {"role": "assistant", "content": "Hi"}
            }],
            "usage": {
                "prompt_tokens": 100000,
                "completion_tokens": 50000,
                "total_tokens": 150000
            }
        });

        let expected = json!({
            "id": "msg_1",
            "type": "message",
            "role": "assistant",
            "model": "gpt-4",
            "content": [{"type": "text", "text": "Hi"}],
            "stop_reason": "end_turn",
            "stop_sequence": null,
            "usage": {"input_tokens": 100000, "output_tokens": 50000}
        });

        let result = openai_to_anthropic_response(input).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_transform_usage_default_values() {
        let expected = json!({
            "input_tokens": 0,
            "output_tokens": 0
        });

        let result = transform_usage_openai_to_anthropic(None);
        assert_eq!(result, expected);
    }

    #[test]
    fn test_transform_usage_partial_data() {
        let input = json!({
            "prompt_tokens": 42
        });

        let expected = json!({
            "input_tokens": 42,
            "output_tokens": 0
        });

        let result = transform_usage_openai_to_anthropic(Some(&input));
        assert_eq!(result, expected);
    }

    // ============================================
    // Finish reason / stop reason tests
    // ============================================

    #[test]
    fn test_stop_reason_end_turn() {
        let input = json!({
            "id": "msg_1",
            "model": "gpt-4",
            "choices": [{
                "index": 0,
                "finish_reason": "stop",
                "message": {"role": "assistant", "content": "Done."}
            }]
        });

        let expected = json!({
            "id": "msg_1",
            "type": "message",
            "role": "assistant",
            "model": "gpt-4",
            "content": [{"type": "text", "text": "Done."}],
            "stop_reason": "end_turn",
            "stop_sequence": null,
            "usage": {"input_tokens": 0, "output_tokens": 0}
        });

        let result = openai_to_anthropic_response(input).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_stop_reason_tool_calls() {
        let input = json!({
            "id": "msg_1",
            "model": "gpt-4",
            "choices": [{
                "index": 0,
                "finish_reason": "tool_calls",
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": {"name": "x", "arguments": "{}"}
                    }]
                }
            }]
        });

        let expected = json!({
            "id": "msg_1",
            "type": "message",
            "role": "assistant",
            "model": "gpt-4",
            "content": [{"type": "tool_use", "id": "call_1", "name": "x", "input": {}}],
            "stop_reason": "tool_use",
            "stop_sequence": null,
            "usage": {"input_tokens": 0, "output_tokens": 0}
        });

        let result = openai_to_anthropic_response(input).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_stop_reason_max_tokens() {
        let input = json!({
            "id": "msg_1",
            "model": "gpt-4",
            "choices": [{
                "index": 0,
                "finish_reason": "length",
                "message": {"role": "assistant", "content": "Truncated..."}
            }]
        });

        let expected = json!({
            "id": "msg_1",
            "type": "message",
            "role": "assistant",
            "model": "gpt-4",
            "content": [{"type": "text", "text": "Truncated..."}],
            "stop_reason": "max_tokens",
            "stop_sequence": null,
            "usage": {"input_tokens": 0, "output_tokens": 0}
        });

        let result = openai_to_anthropic_response(input).unwrap();
        assert_eq!(result, expected);
    }

    // ============================================
    // Error handling tests
    // ============================================

    #[test]
    fn test_non_object_body() {
        let input = json!("not an object");
        let result = openai_to_anthropic_response(input);
        assert!(result.is_err());
    }

    #[test]
    fn test_missing_choices() {
        let input = json!({
            "id": "msg_1",
            "model": "gpt-4"
        });

        let result = openai_to_anthropic_response(input);
        assert!(result.is_err());
    }

    #[test]
    fn test_empty_choices() {
        let input = json!({
            "id": "msg_1",
            "model": "gpt-4",
            "choices": []
        });

        let result = openai_to_anthropic_response(input);
        assert!(result.is_err());
    }

    #[test]
    fn test_missing_message_in_choice() {
        let input = json!({
            "id": "msg_1",
            "model": "gpt-4",
            "choices": [{
                "index": 0,
                "finish_reason": "stop"
            }]
        });

        let result = openai_to_anthropic_response(input);
        assert!(result.is_err());
    }

    // ============================================
    // Field passthrough tests
    // ============================================

    #[test]
    fn test_created_timestamp_preserved() {
        let input = json!({
            "id": "msg_1",
            "model": "gpt-4",
            "created": 1712530587,
            "choices": [{
                "index": 0,
                "finish_reason": "stop",
                "message": {"role": "assistant", "content": "Hi"}
            }]
        });

        let expected = json!({
            "id": "msg_1",
            "type": "message",
            "role": "assistant",
            "model": "gpt-4",
            "created": 1712530587,
            "content": [{"type": "text", "text": "Hi"}],
            "stop_reason": "end_turn",
            "stop_sequence": null,
            "usage": {"input_tokens": 0, "output_tokens": 0}
        });

        let result = openai_to_anthropic_response(input).unwrap();
        assert_eq!(result, expected);
    }

    // ============================================
    // Complex response tests
    // ============================================

    #[test]
    fn test_full_response_with_reasoning_and_tool_call() {
        let input = json!({
            "id": "msg_full",
            "model": "gpt-4",
            "created": 1712530587,
            "choices": [{
                "index": 0,
                "finish_reason": "tool_calls",
                "message": {
                    "role": "assistant",
                    "content": "Let me search for that.",
                    "reasoning_content": "I need to find information about the weather.",
                    "tool_calls": [{
                        "id": "call_search",
                        "type": "function",
                        "function": {
                            "name": "web_search",
                            "arguments": "{\"query\": \"weather Tokyo\"}"
                        }
                    }]
                }
            }],
            "usage": {
                "prompt_tokens": 25,
                "completion_tokens": 30,
                "total_tokens": 55
            }
        });

        let expected = json!({
            "id": "msg_full",
            "type": "message",
            "role": "assistant",
            "model": "gpt-4",
            "created": 1712530587,
            "content": [
                {"type": "thinking", "thinking": "I need to find information about the weather."},
                {"type": "text", "text": "Let me search for that."},
                {"type": "tool_use", "id": "call_search", "name": "web_search", "input": {"query": "weather Tokyo"}}
            ],
            "stop_reason": "tool_use",
            "stop_sequence": null,
            "usage": {"input_tokens": 25, "output_tokens": 30}
        });

        let result = openai_to_anthropic_response(input).unwrap();
        assert_eq!(result, expected);
    }
}
