//! Transform Anthropic requests to OpenAI format
//!
//! This module provides functions to convert Anthropic API request format to OpenAI-compatible format.

use super::super::extract_passthrough_fields;
use crate::error::LlmMapError;
use serde_json::{Value, json};

use super::message::{
    extract_system_text, transform_message_anthropic_to_openai,
    transform_tool_choice_anthropic_to_openai, transform_tools_anthropic_to_openai,
};

/// Helper function to transform Anthropic request to OpenAI format
pub fn anthropic_to_openai_request(body: Value) -> Result<Value, LlmMapError> {
    let Value::Object(mut obj) = body else {
        return Err(LlmMapError::Validation(
            "Request body must be an object".into(),
        ));
    };

    // Build final request
    let mut result = serde_json::Map::new();

    // Extract and transform tools (if present)
    if let Some(Value::Array(tools)) = obj.remove("tools") {
        let transformed_tools = transform_tools_anthropic_to_openai(tools);
        result.insert("tools".to_string(), transformed_tools);
    }

    // Extract and transform tool_choice (if present)
    if let Some(tool_choice) = obj.remove("tool_choice") {
        let transformed = transform_tool_choice_anthropic_to_openai(tool_choice);
        result.insert("tool_choice".to_string(), transformed);
    }

    // Extract system message first (if exists)
    let system_msg = obj.remove("system").map(|system| {
        let system_content = extract_system_text(system);
        json!({
            "role": "system",
            "content": system_content
        })
    });

    // remove thinking field
    obj.remove("thinking");

    // Extract and transform ouput_config (if present)
    if let Some(Value::Object(mut output_config)) = obj.remove("output_config")
        && let Some(effort) = output_config.remove("effort")
        && let Some(effort_str) = effort.as_str()
    {
        let effort_level = match effort_str {
            "low" => "low",
            "medium" => "medium",
            "high" => "high",
            "max" => "xhigh",
            _ => "none",
        };
        result.insert(
            "reasoning_effort".to_string(),
            Value::String(effort_level.to_string()),
        );
    }

    // Take ownership of messages array
    let messages = if let Some(Value::Array(msgs)) = obj.remove("messages") {
        msgs
    } else {
        Vec::new()
    };

    let mut openai_messages = Vec::new();

    // Add system message first if exists
    if let Some(sys) = system_msg {
        openai_messages.push(sys);
    }

    // Transform each message
    for msg in messages {
        let transformed = transform_message_anthropic_to_openai(msg);
        openai_messages.extend(transformed);
    }

    result.insert("messages".to_string(), Value::Array(openai_messages));

    extract_passthrough_fields(
        &mut obj,
        &mut result,
        &[
            "model",
            "max_tokens",
            "stream",
            "metadata",
            "temperature",
            "top_p",
        ],
    );

    Ok(Value::Object(result))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ============================================
    // System message tests
    // ============================================

    #[test]
    fn test_system_message_as_string() {
        let input = json!({
            "model": "claude-sonnet-4-20250514",
            "system": "You are a helpful assistant.",
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let expected = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [
                {"role": "system", "content": "You are a helpful assistant."},
                {"role": "user", "content": "Hello"}
            ]
        });

        let result = anthropic_to_openai_request(input).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_system_message_as_array() {
        let input = json!({
            "model": "claude-sonnet-4-20250514",
            "system": [{"type": "text", "text": "You are a coding assistant."}],
            "messages": [{"role": "user", "content": "Help me write code."}]
        });

        let expected = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [
                {"role": "system", "content": "You are a coding assistant."},
                {"role": "user", "content": "Help me write code."}
            ]
        });

        let result = anthropic_to_openai_request(input).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_no_system_message() {
        let input = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let expected = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_openai_request(input).unwrap();
        assert_eq!(result, expected);
    }

    // ============================================
    // Message role transformation tests
    // ============================================

    #[test]
    fn test_user_message() {
        let input = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [{"role": "user", "content": "Hello, how are you?"}]
        });

        let expected = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [{"role": "user", "content": "Hello, how are you?"}]
        });

        let result = anthropic_to_openai_request(input).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_assistant_message() {
        let input = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [{"role": "assistant", "content": "I'm doing well, thank you!"}]
        });

        let expected = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [{"role": "assistant", "content": "I'm doing well, thank you!"}]
        });

        let result = anthropic_to_openai_request(input).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_assistant_message_with_tool_use() {
        let input = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [{
                "role": "assistant",
                "content": [
                    {"type": "text", "text": "Let me search for that."},
                    {
                        "type": "tool_use",
                        "id": "toolu_abc123",
                        "name": "web_search",
                        "input": {"query": "weather in Tokyo"}
                    }
                ]
            }]
        });

        let expected = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [{
                "role": "assistant",
                "content": "Let me search for that.",
                "tool_calls": [{
                    "id": "toolu_abc123",
                    "type": "function",
                    "function": {
                        "name": "web_search",
                        "arguments": "{\"query\":\"weather in Tokyo\"}"
                    }
                }]
            }]
        });

        let result = anthropic_to_openai_request(input).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_tool_result_message() {
        let input = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "text", "text": "What's the weather?"},
                    {
                        "type": "tool_result",
                        "tool_use_id": "toolu_abc123",
                        "content": "The weather in Tokyo is sunny, 25°C."
                    }
                ]
            }]
        });

        let expected = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [
                {"role": "tool", "tool_call_id": "toolu_abc123", "content": "The weather in Tokyo is sunny, 25°C."},
                {"role": "user", "content": "What's the weather?"}
            ]
        });

        let result = anthropic_to_openai_request(input).unwrap();
        assert_eq!(result, expected);
    }

    // ============================================
    // Image transformation tests
    // ============================================

    #[test]
    fn test_image_with_base64_source() {
        let input = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [{
                "role": "user",
                "content": [{
                    "type": "image",
                    "source": {
                        "type": "base64",
                        "media_type": "image/png",
                        "data": "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg=="
                    }
                }]
            }]
        });

        let result = anthropic_to_openai_request(input).unwrap();
        let messages = result["messages"].as_array().unwrap();
        let content = messages[0]["content"].as_array().unwrap();

        assert_eq!(content[0]["type"], "image_url");
        let url = content[0]["image_url"]["url"].as_str().unwrap();
        assert!(url.starts_with("data:image/png;base64,"));
    }

    #[test]
    fn test_image_with_url_source() {
        let input = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [{
                "role": "user",
                "content": [{
                    "type": "image",
                    "source": {
                        "type": "url",
                        "url": "https://example.com/photo.jpg"
                    }
                }]
            }]
        });

        let result = anthropic_to_openai_request(input).unwrap();
        let messages = result["messages"].as_array().unwrap();
        let content = messages[0]["content"].as_array().unwrap();

        assert_eq!(content[0]["type"], "image_url");
    }

    // ============================================
    // Tools transformation tests
    // ============================================

    #[test]
    fn test_tools_basic() {
        let input = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [{"role": "user", "content": "What's the weather?"}],
            "tools": [{
                "name": "get_weather",
                "description": "Get weather for a city",
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "city": {"type": "string"}
                    },
                    "required": ["city"]
                }
            }]
        });

        let expected = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [{"role": "user", "content": "What's the weather?"}],
            "tools": [{
                "type": "function",
                "function": {
                    "name": "get_weather",
                    "description": "Get weather for a city",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "city": {"type": "string"}
                        },
                        "required": ["city"]
                    }
                }
            }]
        });

        let result = anthropic_to_openai_request(input).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_tools_with_strict() {
        let input = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [{"role": "user", "content": "test"}],
            "tools": [{
                "name": "get_weather",
                "description": "Get weather",
                "input_schema": {"type": "object", "properties": {}},
                "strict": true
            }]
        });

        let expected = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [{"role": "user", "content": "test"}],
            "tools": [{
                "type": "function",
                "function": {
                    "name": "get_weather",
                    "description": "Get weather",
                    "parameters": {"type": "object", "properties": {}}
                }
            }]
        });

        let result = anthropic_to_openai_request(input).unwrap();
        assert_eq!(result, expected);
    }

    // ============================================
    // Tool choice transformation tests
    // ============================================

    #[test]
    fn test_tool_choice_auto() {
        let input = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [{"role": "user", "content": "test"}],
            "tool_choice": {"type": "auto"}
        });

        let expected = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [{"role": "user", "content": "test"}],
            "tool_choice": "auto"
        });

        let result = anthropic_to_openai_request(input).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_tool_choice_none() {
        let input = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [{"role": "user", "content": "test"}],
            "tool_choice": {"type": "none"}
        });

        let expected = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [{"role": "user", "content": "test"}],
            "tool_choice": "none"
        });

        let result = anthropic_to_openai_request(input).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_tool_choice_any() {
        let input = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [{"role": "user", "content": "test"}],
            "tool_choice": {"type": "any"}
        });

        let expected = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [{"role": "user", "content": "test"}],
            "tool_choice": "required"
        });

        let result = anthropic_to_openai_request(input).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_tool_choice_specific_function() {
        let input = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [{"role": "user", "content": "test"}],
            "tool_choice": {"type": "tool", "name": "get_weather"}
        });

        let expected = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [{"role": "user", "content": "test"}],
            "tool_choice": {"type": "function", "function": {"name": "get_weather"}}
        });

        let result = anthropic_to_openai_request(input).unwrap();
        assert_eq!(result, expected);
    }

    // ============================================
    // Output config / reasoning effort tests
    // ============================================

    #[test]
    fn test_output_config_effort_low() {
        let input = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [{"role": "user", "content": "test"}],
            "output_config": {"effort": "low"}
        });

        let expected = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [{"role": "user", "content": "test"}],
            "reasoning_effort": "low"
        });

        let result = anthropic_to_openai_request(input).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_output_config_effort_medium() {
        let input = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [{"role": "user", "content": "test"}],
            "output_config": {"effort": "medium"}
        });

        let expected = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [{"role": "user", "content": "test"}],
            "reasoning_effort": "medium"
        });

        let result = anthropic_to_openai_request(input).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_output_config_effort_high() {
        let input = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [{"role": "user", "content": "test"}],
            "output_config": {"effort": "high"}
        });

        let expected = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [{"role": "user", "content": "test"}],
            "reasoning_effort": "high"
        });

        let result = anthropic_to_openai_request(input).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_output_config_effort_max() {
        let input = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [{"role": "user", "content": "test"}],
            "output_config": {"effort": "max"}
        });

        let expected = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [{"role": "user", "content": "test"}],
            "reasoning_effort": "xhigh"
        });

        let result = anthropic_to_openai_request(input).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_output_config_effort_unknown_defaults_to_none() {
        let input = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [{"role": "user", "content": "test"}],
            "output_config": {"effort": "unknown_effort"}
        });

        let expected = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [{"role": "user", "content": "test"}],
            "reasoning_effort": "none"
        });

        let result = anthropic_to_openai_request(input).unwrap();
        assert_eq!(result, expected);
    }

    // ============================================
    // Thinking field removal tests
    // ============================================

    #[test]
    fn test_thinking_field_removed() {
        let input = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [{"role": "user", "content": "test"}],
            "thinking": {"type": "enabled", "budget_tokens": 1000}
        });

        let expected = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [{"role": "user", "content": "test"}]
        });

        let result = anthropic_to_openai_request(input).unwrap();
        assert_eq!(result, expected);
    }

    // ============================================
    // Passthrough fields tests
    // ============================================

    #[test]
    fn test_passthrough_fields() {
        let input = json!({
            "model": "claude-opus-4-20250514",
            "messages": [{"role": "user", "content": "test"}],
            "max_tokens": 1024,
            "temperature": 0.7,
            "top_p": 0.9,
            "stream": false
        });

        let expected = json!({
            "model": "claude-opus-4-20250514",
            "messages": [{"role": "user", "content": "test"}],
            "max_tokens": 1024,
            "temperature": 0.7,
            "top_p": 0.9,
            "stream": false
        });

        let result = anthropic_to_openai_request(input).unwrap();
        assert_eq!(result, expected);
    }

    // ============================================
    // Edge case tests
    // ============================================

    #[test]
    fn test_empty_messages() {
        let input = json!({
            "model": "claude-sonnet-4-20250514"
        });

        let expected = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": []
        });

        let result = anthropic_to_openai_request(input).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_non_object_body() {
        let input = json!("not an object");
        let result = anthropic_to_openai_request(input);
        assert!(result.is_err());
    }

    #[test]
    fn test_full_conversation_with_tools() {
        let input = json!({
            "model": "claude-sonnet-4-20250514",
            "system": "You are a helpful assistant.",
            "messages": [
                {"role": "user", "content": "What's the weather in Tokyo?"},
                {
                    "role": "assistant",
                    "content": [
                        {"type": "text", "text": "I'll check that for you."},
                        {"type": "tool_use", "id": "call_abc123", "name": "get_weather", "input": {"city": "Tokyo"}}
                    ]
                },
                {
                    "role": "user",
                    "content": [{"type": "tool_result", "tool_use_id": "call_abc123", "content": "Sunny, 25°C"}]
                },
                {"role": "assistant", "content": "The weather in Tokyo is sunny with a temperature of 25°C."}
            ],
            "tools": [{
                "name": "get_weather",
                "description": "Get weather for a city",
                "input_schema": {"type": "object", "properties": {"city": {"type": "string"}}, "required": ["city"]}
            }],
            "tool_choice": {"type": "auto"},
            "max_tokens": 1024
        });

        let expected = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [
                {"role": "system", "content": "You are a helpful assistant."},
                {"role": "user", "content": "What's the weather in Tokyo?"},
                {
                    "role": "assistant",
                    "content": "I'll check that for you.",
                    "tool_calls": [{
                        "id": "call_abc123",
                        "type": "function",
                        "function": {"name": "get_weather", "arguments": "{\"city\":\"Tokyo\"}"}
                    }]
                },
                {"role": "tool", "tool_call_id": "call_abc123", "content": "Sunny, 25°C"},
                {"role": "assistant", "content": "The weather in Tokyo is sunny with a temperature of 25°C."}
            ],
            "tools": [{
                "type": "function",
                "function": {
                    "name": "get_weather",
                    "description": "Get weather for a city",
                    "parameters": {"type": "object", "properties": {"city": {"type": "string"}}, "required": ["city"]}
                }
            }],
            "tool_choice": "auto",
            "max_tokens": 1024
        });

        let result = anthropic_to_openai_request(input).unwrap();
        assert_eq!(result, expected);
    }
}
