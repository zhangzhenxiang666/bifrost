//! Message-level conversion utilities for Anthropic ↔ OpenAI
//!
//! This module provides low-level message transformation functions.

use crate::adapter::converter::create_null;
use serde_json::{Value, json};

/// Extract system text from Anthropic system field (string or content blocks)
pub fn extract_system_text(system: Value) -> String {
    match system {
        Value::String(s) => s,
        Value::Array(blocks) => blocks
            .iter()
            .filter_map(|b| {
                b.get("type")
                    .and_then(|t| t.as_str())
                    .filter(|t| *t == "text")
                    .and_then(|_| b.get("text").and_then(|t| t.as_str()))
            })
            .collect::<Vec<_>>()
            .join("\n\n"),
        _ => String::new(),
    }
}

/// Transform a single Anthropic message into 1+ OpenAI messages
/// Returns Vec because tool_result blocks become separate tool messages
pub fn transform_message_anthropic_to_openai(msg: Value) -> Vec<Value> {
    let Value::Object(mut obj) = msg else {
        return vec![];
    };

    let role = obj
        .get("role")
        .and_then(|v| v.as_str())
        .unwrap_or("user")
        .to_string();

    // Transform user message: extract tool_result blocks as separate tool messages
    if role == "user"
        && let Some(content) = obj.remove("content")
    {
        // Handle both string and array content
        let content_array = match content {
            Value::Array(arr) => arr,
            Value::String(text) => vec![json!({"type": "text", "text": text})],
            _ => vec![],
        };

        let (remaining_content, mut result) = extract_tool_results_from_user_message(content_array);

        let should_add_user_message = match &remaining_content {
            Value::String(s) => !s.is_empty(),
            Value::Array(arr) => !arr.is_empty(),
            Value::Null => false,
            _ => false,
        };

        if should_add_user_message {
            obj.insert("content".into(), remaining_content);
            result.push(Value::Object(obj));
        }
        return result;
    }

    // Transform assistant message: convert tool_use blocks to tool_calls
    if role == "assistant"
        && let Some(content) = obj.remove("content")
    {
        // Handle both string and array content
        let content_array = match content {
            Value::Array(arr) => arr,
            Value::String(text) => vec![json!({"type": "text", "text": text})],
            _ => vec![],
        };

        let (transformed_content, tool_calls, reasoning_content) =
            transform_assistant_content_with_tool_use(content_array);
        obj.insert("content".into(), transformed_content);

        if !tool_calls.is_empty() {
            obj.insert("tool_calls".into(), Value::Array(tool_calls));
        }

        if let Some(reasoning) = reasoning_content {
            obj.insert("reasoning_content".into(), Value::String(reasoning));
        }

        return vec![Value::Object(obj)];
    }

    // For other cases, transform content blocks normally
    if let Some(content) = obj.remove("content") {
        // Handle both string and array content
        let content_array = match content {
            Value::Array(arr) => arr,
            Value::String(text) => vec![json!({"type": "text", "text": text})],
            _ => vec![],
        };

        let transformed = transform_regular_content_blocks(content_array);
        obj.insert("content".into(), transformed);
    }

    vec![Value::Object(obj)]
}

/// Extract tool_result blocks from user message and convert them to separate tool messages
/// Returns (remaining_content, tool_messages)
pub fn extract_tool_results_from_user_message(blocks: Vec<Value>) -> (Value, Vec<Value>) {
    let mut text_parts: Vec<Value> = Vec::new();
    let mut tool_messages: Vec<Value> = Vec::new();

    for block in blocks {
        let Value::Object(mut obj) = block else {
            continue;
        };

        obj.remove("cache_control");

        let block_type = obj.get("type").and_then(|v| v.as_str());
        match block_type {
            Some("tool_result") => {
                // Convert tool_result to a separate tool message
                let tool_call_id = obj.remove("tool_use_id").unwrap_or_else(create_null);
                let content = obj.remove("content").unwrap_or(Value::String("".into()));

                tool_messages.push(json!({
                    "role": "tool",
                    "tool_call_id": tool_call_id,
                    "content": content
                }));
            }
            Some("text") => {
                text_parts.push(Value::Object(obj));
            }
            Some("image") => {
                if let Some(transformed) = transform_image_block(&obj) {
                    text_parts.push(transformed);
                }
            }
            _ => {}
        }
    }

    let remaining_content = if text_parts.is_empty() {
        Value::Null
    } else if text_parts.len() == 1 {
        // For single text block, extract the text as string
        let block = text_parts.remove(0);
        if let Some(text) = block.get("text").and_then(|v| v.as_str()) {
            Value::String(text.to_string())
        } else {
            Value::Array(vec![block])
        }
    } else {
        Value::Array(text_parts)
    };

    (remaining_content, tool_messages)
}

/// Transform assistant content with tool_use blocks
pub fn transform_assistant_content_with_tool_use(
    blocks: Vec<Value>,
) -> (Value, Vec<Value>, Option<String>) {
    let mut text_parts: Vec<Value> = Vec::new();
    let mut tool_calls = Vec::new();
    let mut reasoning_content: Option<String> = None;

    for block in blocks {
        let Value::Object(mut obj) = block else {
            continue;
        };

        let block_type = obj.get("type").and_then(|v| v.as_str());
        match block_type {
            Some("text") => {
                text_parts.push(Value::Object(obj));
            }
            Some("tool_use") => {
                let id = obj.remove("id").unwrap_or_else(create_null);
                let name = obj.remove("name").unwrap_or_else(create_null);
                let input = obj
                    .remove("input")
                    .unwrap_or(Value::Object(serde_json::Map::new()));

                let arguments = serde_json::to_string(&input).unwrap_or_default();

                tool_calls.push(json!({
                    "id": id,
                    "type": "function",
                    "function": {
                        "name": name,
                        "arguments": arguments
                    }
                }));
            }
            Some("thinking") => {
                if let Some(thinking) = obj.get("thinking").and_then(|v| v.as_str())
                    && reasoning_content.is_none()
                {
                    reasoning_content = Some(thinking.to_string());
                }
            }
            _ => {}
        }
    }

    // Return text content
    let content = if text_parts.is_empty() {
        Value::String("".into())
    } else if text_parts.len() == 1 {
        // For single text block, extract the text as string
        let block = text_parts.remove(0);
        if let Some(text) = block.get("text").and_then(|v| v.as_str()) {
            Value::String(text.to_string())
        } else {
            Value::Array(vec![block])
        }
    } else {
        Value::Array(text_parts)
    };

    (content, tool_calls, reasoning_content)
}

/// Transform an Anthropic image block to OpenAI image_url format.
///
/// Returns `Some(Value)` with the transformed image_url block, or `None` if the
/// block is invalid or missing required fields.
fn transform_image_block(block: &serde_json::Map<String, Value>) -> Option<Value> {
    let source = block.get("source")?;
    let source = source.as_object()?;

    let media_type = source
        .get("media_type")
        .and_then(|v| v.as_str())
        .unwrap_or("image/png");
    let data = source
        .get("data")
        .or_else(|| source.get("url"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let image_type = source
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("base64");

    let url = if image_type == "base64" {
        format!("data:{};base64,{}", media_type, data)
    } else {
        data.to_string()
    };

    Some(json!({
        "type": "image_url",
        "image_url": { "url": url }
    }))
}

/// Transform regular content blocks (text, image)
pub fn transform_regular_content_blocks(blocks: Vec<Value>) -> Value {
    let mut text_parts: Vec<Value> = Vec::new();

    for block in blocks {
        let Value::Object(obj) = block else {
            continue;
        };

        let block_type = obj.get("type").and_then(|v| v.as_str());
        match block_type {
            Some("text") => {
                text_parts.push(Value::Object(obj));
            }
            Some("image") => {
                if let Some(transformed) = transform_image_block(&obj) {
                    text_parts.push(transformed);
                }
            }
            _ => {}
        }
    }

    if text_parts.is_empty() {
        Value::String("".into())
    } else if text_parts.len() == 1 {
        // For single text block, extract the text as string
        let block = text_parts.remove(0);
        if let Some(text) = block.get("text").and_then(|v| v.as_str()) {
            Value::String(text.to_string())
        } else {
            Value::Array(vec![block])
        }
    } else {
        Value::Array(text_parts)
    }
}

/// Transform tools from Anthropic format to OpenAI format
pub fn transform_tools_anthropic_to_openai(tools: Vec<Value>) -> Value {
    let mut transformed = Vec::with_capacity(tools.len());

    for tool in tools {
        let Value::Object(mut obj) = tool else {
            continue;
        };

        let name = obj.remove("name").unwrap_or_else(create_null);
        let description = obj.remove("description").unwrap_or_else(create_null);
        let parameters = obj
            .into_iter()
            .find_map(|(k, v)| (k == "input_schema").then_some(v))
            .unwrap_or(Value::Object(serde_json::Map::new()));

        transformed.push(json!({
            "type": "function",
            "function": {
                "name": name,
                "description": description,
                "parameters": parameters
            }
        }));
    }

    Value::Array(transformed)
}

/// Transform tool_choice from Anthropic format to OpenAI format
pub fn transform_tool_choice_anthropic_to_openai(tool_choice: Value) -> Value {
    let Value::Object(mut obj) = tool_choice else {
        // If it's already a string (e.g., "auto", "none", "required"), return as-is
        return tool_choice;
    };

    let tool_type = obj.get("type").and_then(|v| v.as_str()).unwrap_or("auto");

    match tool_type {
        "auto" => Value::String("auto".into()),
        "none" => Value::String("none".into()),
        "any" => Value::String("required".into()), // Anthropic "any" -> OpenAI "required"
        "tool" => {
            let name = obj.remove("name").unwrap_or_else(create_null);
            json!({
                "type": "function",
                "function": {
                    "name": name
                }
            })
        }
        _ => Value::String("auto".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ============================================
    // extract_system_text tests
    // ============================================

    #[test]
    fn test_extract_system_text_variants() {
        for (input, expected) in [
            (
                json!("You are a helpful assistant."),
                "You are a helpful assistant.",
            ),
            (
                json!([{"type": "text", "text": "You are a coding assistant."}]),
                "You are a coding assistant.",
            ),
            (
                json!([
                    {"type": "text", "text": "First part."},
                    {"type": "text", "text": "Second part."}
                ]),
                "First part.\n\nSecond part.",
            ),
            (
                json!([
                    {"type": "text", "text": "Hello."},
                    {"type": "image", "source": {"type": "url", "url": "https://example.com/img.png"}},
                    {"type": "text", "text": "World."}
                ]),
                "Hello.\n\nWorld.",
            ),
            (json!([]), ""),
            (json!(123), ""),
        ] {
            let result = extract_system_text(input);
            assert_eq!(result, expected);
        }
    }

    // ============================================
    // transform_message_anthropic_to_openai tests
    // ============================================

    #[test]
    fn test_transform_message_content() {
        for (input, expected) in [
            // user with string content
            (
                json!({"role": "user", "content": "Hello, how are you?"}),
                vec![json!({"role": "user", "content": "Hello, how are you?"})],
            ),
            // user with single text block (flattened to string)
            (
                json!({"role": "user", "content": [{"type": "text", "text": "Hello!"}]}),
                vec![json!({"role": "user", "content": "Hello!"})],
            ),
            // assistant with string content
            (
                json!({"role": "assistant", "content": "I'm doing well!"}),
                vec![json!({"role": "assistant", "content": "I'm doing well!"})],
            ),
        ] {
            let result = transform_message_anthropic_to_openai(input);
            assert_eq!(result, expected);
        }
    }

    #[test]
    fn test_transform_user_message_with_tool_result() {
        let input = json!({
            "role": "user",
            "content": [
                {"type": "text", "text": "What's the weather?"},
                {"type": "tool_result", "tool_use_id": "call_abc", "content": "Sunny, 25°C"}
            ]
        });
        let expected = vec![
            json!({"role": "tool", "tool_call_id": "call_abc", "content": "Sunny, 25°C"}),
            json!({"role": "user", "content": "What's the weather?"}),
        ];
        let result = transform_message_anthropic_to_openai(input);
        assert_eq!(result, expected);
    }

    #[test]
    fn test_transform_non_object_message() {
        let input = json!("just a string");
        let result = transform_message_anthropic_to_openai(input);
        assert!(result.is_empty());
    }

    // ============================================
    // extract_tool_results_from_user_message tests
    // ============================================

    #[test]
    fn test_extract_tool_results_basic() {
        let input = vec![
            json!({"type": "text", "text": "Hello"}),
            json!({"type": "tool_result", "tool_use_id": "call_1", "content": "Result 1"}),
            json!({"type": "tool_result", "tool_use_id": "call_2", "content": "Result 2"}),
        ];
        let (remaining, tool_messages) = extract_tool_results_from_user_message(input);
        assert_eq!(remaining, json!("Hello"));
        assert_eq!(
            tool_messages,
            vec![
                json!({"role": "tool", "tool_call_id": "call_1", "content": "Result 1"}),
                json!({"role": "tool", "tool_call_id": "call_2", "content": "Result 2"}),
            ]
        );
    }

    #[test]
    fn test_extract_tool_results_with_image() {
        let input = vec![
            json!({"type": "image", "source": {"type": "url", "url": "https://example.com/img.png"}}),
            json!({"type": "tool_result", "tool_use_id": "call_1", "content": "Analyzed"}),
        ];
        let (remaining, tool_messages) = extract_tool_results_from_user_message(input);
        assert_eq!(
            remaining,
            json!([{"type": "image_url", "image_url": {"url": "https://example.com/img.png"}}])
        );
        assert_eq!(
            tool_messages,
            vec![json!({"role": "tool", "tool_call_id": "call_1", "content": "Analyzed"}),]
        );
    }

    #[test]
    fn test_extract_tool_results_only_tool_results() {
        let input =
            vec![json!({"type": "tool_result", "tool_use_id": "call_1", "content": "Only tool"})];
        let (remaining, tool_messages) = extract_tool_results_from_user_message(input);
        assert_eq!(remaining, json!(null));
        assert_eq!(
            tool_messages,
            vec![json!({"role": "tool", "tool_call_id": "call_1", "content": "Only tool"}),]
        );
    }

    #[test]
    fn test_extract_tool_results_cache_control_removed() {
        let input = vec![
            json!({"type": "text", "text": "Hello", "cache_control": {"type": "ephemeral"}}),
            json!({"type": "tool_result", "tool_use_id": "call_1", "content": "Result"}),
        ];
        let (remaining, _) = extract_tool_results_from_user_message(input);
        assert_eq!(remaining, json!("Hello"));
    }

    // ============================================
    // transform_assistant_content_with_tool_use tests
    // ============================================

    #[test]
    fn test_transform_assistant_content_text_only() {
        let input = vec![json!({"type": "text", "text": "Hello world"})];
        let (content, tool_calls, _) = transform_assistant_content_with_tool_use(input);
        assert_eq!(content, json!("Hello world"));
        assert!(tool_calls.is_empty());
    }

    #[test]
    fn test_transform_assistant_content_tool_use_only() {
        let input = vec![
            json!({"type": "tool_use", "id": "call_1", "name": "get_weather", "input": {"city": "Tokyo"}}),
        ];
        let (content, tool_calls, _) = transform_assistant_content_with_tool_use(input);
        assert_eq!(content, json!(""));
        assert_eq!(
            tool_calls,
            vec![
                json!({"id": "call_1", "type": "function", "function": {"name": "get_weather", "arguments": "{\"city\":\"Tokyo\"}"}}),
            ]
        );
    }

    #[test]
    fn test_transform_assistant_content_mixed() {
        let input = vec![
            json!({"type": "text", "text": "The weather is"}),
            json!({"type": "tool_use", "id": "call_1", "name": "get_weather", "input": {}}),
            json!({"type": "text", "text": "Let me check."}),
        ];
        let (content, tool_calls, _) = transform_assistant_content_with_tool_use(input);
        assert_eq!(
            content,
            json!([{"type": "text", "text": "The weather is"}, {"type": "text", "text": "Let me check."}])
        );
        assert_eq!(
            tool_calls,
            vec![
                json!({"id": "call_1", "type": "function", "function": {"name": "get_weather", "arguments": "{}"}}),
            ]
        );
    }

    #[test]
    fn test_transform_assistant_content_with_thinking() {
        let input = vec![
            json!({"type": "thinking", "thinking": "Let me think about this..."}),
            json!({"type": "text", "text": "The answer is 42."}),
        ];
        let (content, tool_calls, reasoning) = transform_assistant_content_with_tool_use(input);
        assert_eq!(content, json!("The answer is 42."));
        assert!(tool_calls.is_empty());
        assert_eq!(reasoning, Some("Let me think about this...".to_string()));
    }

    #[test]
    fn test_transform_assistant_content_thinking_and_tool_use() {
        let input = vec![
            json!({"type": "thinking", "thinking": "I need to check the weather."}),
            json!({"type": "text", "text": "Let me search."}),
            json!({"type": "tool_use", "id": "call_1", "name": "get_weather", "input": {"city": "Tokyo"}}),
        ];
        let (content, tool_calls, reasoning) = transform_assistant_content_with_tool_use(input);
        assert_eq!(content, json!("Let me search."));
        assert_eq!(
            tool_calls,
            vec![
                json!({"id": "call_1", "type": "function", "function": {"name": "get_weather", "arguments": "{\"city\":\"Tokyo\"}"}}),
            ]
        );
        assert_eq!(reasoning, Some("I need to check the weather.".to_string()));
    }

    #[test]
    fn test_transform_assistant_content_only_thinking() {
        let input = vec![json!({"type": "thinking", "thinking": "Just thinking..."})];
        let (content, tool_calls, reasoning) = transform_assistant_content_with_tool_use(input);
        assert_eq!(content, json!(""));
        assert!(tool_calls.is_empty());
        assert_eq!(reasoning, Some("Just thinking...".to_string()));
    }

    #[test]
    fn test_transform_assistant_content_multiple_thinking_uses_first() {
        let input = vec![
            json!({"type": "thinking", "thinking": "First thought."}),
            json!({"type": "thinking", "thinking": "Second thought."}),
            json!({"type": "text", "text": "Result."}),
        ];
        let (_, _, reasoning) = transform_assistant_content_with_tool_use(input);
        assert_eq!(reasoning, Some("First thought.".to_string()));
    }

    // ============================================
    // transform_regular_content_blocks tests
    // ============================================

    #[test]
    fn test_transform_regular_content_text() {
        let input = vec![json!({"type": "text", "text": "Hello"})];
        let result = transform_regular_content_blocks(input);
        assert_eq!(result, json!("Hello"));
    }

    #[test]
    fn test_transform_regular_content_image() {
        let input = vec![json!({
            "type": "image",
            "source": {
                "type": "base64",
                "media_type": "image/png",
                "data": "abc123"
            }
        })];
        let result = transform_regular_content_blocks(input);
        assert_eq!(
            result,
            json!([{"type": "image_url", "image_url": {"url": "data:image/png;base64,abc123"}}])
        );
    }

    #[test]
    fn test_transform_regular_content_multiple() {
        let input = vec![
            json!({"type": "text", "text": "Hello"}),
            json!({"type": "text", "text": "World"}),
        ];
        let result = transform_regular_content_blocks(input);
        assert_eq!(
            result,
            json!([{"type": "text", "text": "Hello"}, {"type": "text", "text": "World"}])
        );
    }

    #[test]
    fn test_transform_regular_content_empty() {
        let input: Vec<serde_json::Value> = vec![];
        let result = transform_regular_content_blocks(input);
        assert_eq!(result, json!(""));
    }

    // ============================================
    // transform_tools_anthropic_to_openai tests
    // ============================================

    #[test]
    fn test_transform_tools_basic() {
        let input = vec![json!({
            "name": "get_weather",
            "description": "Get weather for a city",
            "input_schema": {
                "type": "object",
                "properties": {"city": {"type": "string"}}
            }
        })];
        let result = transform_tools_anthropic_to_openai(input);
        assert_eq!(
            result,
            json!([{
                "type": "function",
                "function": {
                    "name": "get_weather",
                    "description": "Get weather for a city",
                    "parameters": {"type": "object", "properties": {"city": {"type": "string"}}}
                }
            }])
        );
    }

    #[test]
    fn test_transform_tools_with_strict() {
        let input = vec![json!({
            "name": "get_weather",
            "description": "Get weather",
            "input_schema": {"type": "object", "properties": {}},
            "strict": true
        })];
        let result = transform_tools_anthropic_to_openai(input);
        assert_eq!(
            result,
            json!([{
                "type": "function",
                "function": {
                    "name": "get_weather",
                    "description": "Get weather",
                    "parameters": {"type": "object", "properties": {}}
                }
            }])
        );
    }

    #[test]
    fn test_transform_tools_non_object_filtered() {
        let input = vec![
            json!("not an object"),
            json!({
                "name": "valid_tool",
                "description": "A valid tool",
                "input_schema": {"type": "object", "properties": {}}
            }),
        ];
        let result = transform_tools_anthropic_to_openai(input);
        assert_eq!(
            result,
            json!([{
                "type": "function",
                "function": {
                    "name": "valid_tool",
                    "description": "A valid tool",
                    "parameters": {"type": "object", "properties": {}}
                }
            }])
        );
    }

    // ============================================
    // transform_tool_choice_anthropic_to_openai tests
    // ============================================

    #[test]
    fn test_transform_tool_choice_variants() {
        for (input, expected) in [
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
            let result = transform_tool_choice_anthropic_to_openai(input);
            assert_eq!(result, expected);
        }
    }
}
