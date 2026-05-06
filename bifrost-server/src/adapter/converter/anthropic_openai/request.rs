//! Transform Anthropic requests to OpenAI format
//!
//! This module provides functions to convert Anthropic API request format to OpenAI-compatible format.

use crate::error::LlmMapError;
use serde_json::{Value, json};

use super::message::{
    extract_system_text, transform_message_anthropic_to_openai,
    transform_tool_choice_anthropic_to_openai, transform_tools_anthropic_to_openai,
};

pub fn anthropic_to_openai_request(body: Value) -> Result<Value, LlmMapError> {
    let Value::Object(mut obj) = body else {
        return Err(LlmMapError::Validation(
            "Request body must be an object".into(),
        ));
    };

    if let Some(Value::Array(tools)) = obj.remove("tools") {
        let transformed_tools = transform_tools_anthropic_to_openai(tools);
        obj.insert("tools".to_string(), transformed_tools);
    }

    if let Some(tool_choice) = obj.remove("tool_choice") {
        let transformed = transform_tool_choice_anthropic_to_openai(tool_choice);
        obj.insert("tool_choice".to_string(), transformed);
    }

    let system_msg = obj.remove("system").map(|system| {
        let system_content = extract_system_text(system);
        json!({
            "role": "system",
            "content": system_content
        })
    });

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
        obj.insert(
            "reasoning_effort".to_string(),
            Value::String(effort_level.to_string()),
        );
    }

    let messages = if let Some(Value::Array(msgs)) = obj.remove("messages") {
        msgs
    } else {
        Vec::new()
    };

    let mut openai_messages = Vec::new();

    if let Some(sys) = system_msg {
        openai_messages.push(sys);
    }

    for msg in messages {
        let transformed = transform_message_anthropic_to_openai(msg);
        openai_messages.extend(transformed);
    }

    obj.insert("messages".to_string(), Value::Array(openai_messages));

    Ok(Value::Object(obj))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ============================================
    // 全量覆盖测试
    // ============================================

    #[test]
    fn test_full_conversation_with_all_features() {
        let input = json!({
            "model": "claude-sonnet-4-20250514",
            "system": "You are a helpful assistant.",
            "messages": [
                {
                    "role": "user",
                    "content": [
                        {"type": "text", "text": "Draw a cat"},
                        {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "<base64_data>"}}
                    ]
                },
                {
                    "role": "assistant",
                    "content": [
                        {"type": "text", "text": "I'll draw that for you"},
                        {"type": "tool_use", "id": "call_1", "name": "generate_image", "input": {"prompt": "a cute cat"}}
                    ]
                },
                {
                    "role": "user",
                    "content": [{"type": "tool_result", "tool_use_id": "call_1", "content": "Image generated successfully"}]
                },
                {"role": "assistant", "content": "Here's your cat image."}
            ],
            "tools": [{
                "name": "generate_image",
                "description": "Generate an image from a prompt",
                "input_schema": {"type": "object", "properties": {"prompt": {"type": "string"}}, "required": ["prompt"]},
                "strict": true
            }],
            "tool_choice": {"type": "tool", "name": "generate_image"},
            "output_config": {"effort": "high"},
            "thinking": {"type": "enabled", "budget_tokens": 1000},
            "max_tokens": 1024,
            "temperature": 0.7,
            "top_p": 0.9,
            "stream": false
        });

        let expected = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [
                {"role": "system", "content": "You are a helpful assistant."},
                {
                    "role": "user",
                    "content": [
                        {"type": "text", "text": "Draw a cat"},
                        {"type": "image_url", "image_url": {"url": "data:image/png;base64,<base64_data>"}}
                    ]
                },
                {
                    "role": "assistant",
                    "content": "I'll draw that for you",
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": {"name": "generate_image", "arguments": "{\"prompt\":\"a cute cat\"}"}
                    }]
                },
                {"role": "tool", "tool_call_id": "call_1", "content": "Image generated successfully"},
                {"role": "assistant", "content": "Here's your cat image."}
            ],
            "tools": [{
                "type": "function",
                "function": {
                    "name": "generate_image",
                    "description": "Generate an image from a prompt",
                    "parameters": {"type": "object", "properties": {"prompt": {"type": "string"}}, "required": ["prompt"]}
                }
            }],
            "tool_choice": {"type": "function", "function": {"name": "generate_image"}},
            "reasoning_effort": "high",
            "thinking": {"type": "enabled", "budget_tokens": 1000},
            "max_tokens": 1024,
            "temperature": 0.7,
            "top_p": 0.9,
            "stream": false
        });

        let result = anthropic_to_openai_request(input).unwrap();
        assert_eq!(result, expected);
    }

    // ============================================
    // 参数化映射测试
    // ============================================

    #[test]
    fn test_system_message_formats() {
        // system as string
        let input = json!({
            "model": "claude-sonnet-4-20250514",
            "system": "You are helpful.",
            "messages": [{"role": "user", "content": "Hello"}]
        });
        let expected = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [
                {"role": "system", "content": "You are helpful."},
                {"role": "user", "content": "Hello"}
            ]
        });
        assert_eq!(anthropic_to_openai_request(input).unwrap(), expected);

        // system as array with single text block
        let input = json!({
            "model": "claude-sonnet-4-20250514",
            "system": [{"type": "text", "text": "Be concise."}],
            "messages": [{"role": "user", "content": "Hello"}]
        });
        let expected = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [
                {"role": "system", "content": "Be concise."},
                {"role": "user", "content": "Hello"}
            ]
        });
        assert_eq!(anthropic_to_openai_request(input).unwrap(), expected);

        // system as array with multiple text blocks (joined by space)
        let input = json!({
            "model": "claude-sonnet-4-20250514",
            "system": [
                {"type": "text", "text": "First part."},
                {"type": "text", "text": "Second part."}
            ],
            "messages": [{"role": "user", "content": "Hello"}]
        });
        let expected = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [
                {"role": "system", "content": "First part.\n\nSecond part."},
                {"role": "user", "content": "Hello"}
            ]
        });
        assert_eq!(anthropic_to_openai_request(input).unwrap(), expected);

        // no system field → no system message
        let input = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [{"role": "user", "content": "Hello"}]
        });
        let expected = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [{"role": "user", "content": "Hello"}]
        });
        assert_eq!(anthropic_to_openai_request(input).unwrap(), expected);
    }

    #[test]
    fn test_tool_choice_variants() {
        for (input_choice, expected_choice) in [
            (json!({"type": "auto"}), json!("auto")),
            (json!({"type": "none"}), json!("none")),
            (json!({"type": "any"}), json!("required")),
            (
                json!({"type": "tool", "name": "get_weather"}),
                json!({"type": "function", "function": {"name": "get_weather"}}),
            ),
            (json!("auto"), json!("auto")),
            (json!({"type": "unknown"}), json!("auto")),
        ] {
            let input = json!({
                "model": "claude-sonnet-4-20250514",
                "messages": [{"role": "user", "content": "Hello"}],
                "tool_choice": input_choice
            });
            let expected = json!({
                "model": "claude-sonnet-4-20250514",
                "messages": [{"role": "user", "content": "Hello"}],
                "tool_choice": expected_choice
            });
            assert_eq!(anthropic_to_openai_request(input).unwrap(), expected);
        }
    }

    #[test]
    fn test_reasoning_effort_mapping() {
        for (input_effort, expected_effort) in [
            ("low", "low"),
            ("medium", "medium"),
            ("high", "high"),
            ("max", "xhigh"),
            ("unknown", "none"),
        ] {
            let input = json!({
                "model": "claude-sonnet-4-20250514",
                "messages": [{"role": "user", "content": "Hello"}],
                "output_config": {"effort": input_effort}
            });
            let expected = json!({
                "model": "claude-sonnet-4-20250514",
                "messages": [{"role": "user", "content": "Hello"}],
                "reasoning_effort": expected_effort
            });
            assert_eq!(
                anthropic_to_openai_request(input).unwrap(),
                expected,
                "effort mapping: {input_effort} → {expected_effort}"
            );
        }
    }

    // ============================================
    // 边界与异常测试
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
}
