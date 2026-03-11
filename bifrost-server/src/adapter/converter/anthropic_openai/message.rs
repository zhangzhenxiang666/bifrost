//! Message-level conversion utilities for Anthropic ↔ OpenAI
//!
//! This module provides low-level message transformation functions.

use crate::error::LlmMapError;
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
            .join(" "),
        _ => String::new(),
    }
}

/// Transform a single Anthropic message into 1+ OpenAI messages
/// Returns Vec because tool_result blocks become separate tool messages
pub fn transform_message_anthropic_to_openai(msg: Value) -> Result<Vec<Value>, LlmMapError> {
    let Value::Object(mut obj) = msg else {
        return Ok(Vec::new());
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

        let (remaining_content, tool_messages) =
            extract_tool_results_from_user_message(content_array)?;

        // Add tool messages first (OpenAI requires tool messages right after assistant tool_call)
        let mut result = tool_messages;

        // Add user message with remaining text/image content
        if !remaining_content.is_null() {
            obj.insert("content".into(), remaining_content);
        } else {
            obj.insert("content".into(), Value::String("".into()));
        }
        result.push(Value::Object(obj));
        return Ok(result);
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

        let (transformed_content, tool_calls) =
            transform_assistant_content_with_tool_use(content_array)?;
        obj.insert("content".into(), transformed_content);

        if !tool_calls.is_empty() {
            obj.insert("tool_calls".into(), Value::Array(tool_calls));
        }
        return Ok(vec![Value::Object(obj)]);
    }

    // For other cases, transform content blocks normally
    if let Some(content) = obj.remove("content") {
        // Handle both string and array content
        let content_array = match content {
            Value::Array(arr) => arr,
            Value::String(text) => vec![json!({"type": "text", "text": text})],
            _ => vec![],
        };

        let transformed = transform_regular_content_blocks(content_array)?;
        obj.insert("content".into(), transformed);
    }

    Ok(vec![Value::Object(obj)])
}

/// Extract tool_result blocks from user message and convert them to separate tool messages
/// Returns (remaining_content, tool_messages)
pub fn extract_tool_results_from_user_message(
    blocks: Vec<Value>,
) -> Result<(Value, Vec<Value>), LlmMapError> {
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
                let tool_call_id = obj.get("tool_use_id").cloned().unwrap_or(Value::Null);
                let content = obj
                    .get("content")
                    .cloned()
                    .unwrap_or(Value::String("".into()));

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
                // Transform Anthropic image to OpenAI image_url format
                if let Some(source) = obj.get("source") {
                    let media_type = source
                        .get("media_type")
                        .and_then(|v| v.as_str())
                        .unwrap_or("image/png");
                    let data = source.get("data").and_then(|v| v.as_str()).unwrap_or("");
                    let image_type = source
                        .get("type")
                        .and_then(|v| v.as_str())
                        .unwrap_or("base64");

                    let url = if image_type == "base64" {
                        format!("data:{};base64,{}", media_type, data)
                    } else {
                        data.to_string()
                    };

                    text_parts.push(json!({
                        "type": "image_url",
                        "image_url": {
                            "url": url
                        }
                    }));
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

    Ok((remaining_content, tool_messages))
}

/// Transform assistant content with tool_use blocks
/// Returns (text_content, tool_calls)
pub fn transform_assistant_content_with_tool_use(
    blocks: Vec<Value>,
) -> Result<(Value, Vec<Value>), LlmMapError> {
    let mut text_parts: Vec<Value> = Vec::new();
    let mut tool_calls = Vec::new();

    for block in blocks {
        let Value::Object(obj) = block else {
            continue;
        };

        let block_type = obj.get("type").and_then(|v| v.as_str());
        match block_type {
            Some("text") => {
                text_parts.push(Value::Object(obj));
            }
            Some("tool_use") => {
                let id = obj.get("id").cloned().unwrap_or(Value::Null);
                let name = obj.get("name").cloned().unwrap_or(Value::Null);
                let input = obj
                    .get("input")
                    .cloned()
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

    Ok((content, tool_calls))
}

/// Transform regular content blocks (text, image)
pub fn transform_regular_content_blocks(blocks: Vec<Value>) -> Result<Value, LlmMapError> {
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
                // Transform Anthropic image to OpenAI image_url format
                if let Some(source) = obj.get("source") {
                    let media_type = source
                        .get("media_type")
                        .and_then(|v| v.as_str())
                        .unwrap_or("image/png");
                    let data = source.get("data").and_then(|v| v.as_str()).unwrap_or("");
                    let image_type = source
                        .get("type")
                        .and_then(|v| v.as_str())
                        .unwrap_or("base64");

                    let url = if image_type == "base64" {
                        format!("data:{};base64,{}", media_type, data)
                    } else {
                        data.to_string()
                    };

                    text_parts.push(json!({
                        "type": "image_url",
                        "image_url": {
                            "url": url
                        }
                    }));
                }
            }
            _ => {}
        }
    }

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

    Ok(content)
}

/// Transform tools from Anthropic format to OpenAI format
pub fn transform_tools_anthropic_to_openai(tools: Vec<Value>) -> Result<Value, LlmMapError> {
    let mut transformed = Vec::with_capacity(tools.len());

    for tool in tools {
        let Value::Object(obj) = tool else {
            continue;
        };

        let name = obj.get("name").cloned().unwrap_or(Value::Null);
        let description = obj.get("description").cloned().unwrap_or(Value::Null);
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

    Ok(Value::Array(transformed))
}

/// Transform tool_choice from Anthropic format to OpenAI format
pub fn transform_tool_choice_anthropic_to_openai(
    tool_choice: &Value,
) -> Result<Value, LlmMapError> {
    let Some(obj) = tool_choice.as_object() else {
        // If it's already a string (e.g., "auto", "none", "required"), return as-is
        return Ok(tool_choice.clone());
    };

    let tool_type = obj.get("type").and_then(|v| v.as_str()).unwrap_or("auto");

    match tool_type {
        "auto" => Ok(Value::String("auto".into())),
        "none" => Ok(Value::String("none".into())),
        "any" => Ok(Value::String("required".into())), // Anthropic "any" -> OpenAI "required"
        "tool" => {
            let name = obj.get("name").cloned().unwrap_or(Value::Null);
            Ok(json!({
                "type": "function",
                "function": {
                    "name": name
                }
            }))
        }
        _ => Ok(Value::String("auto".into())),
    }
}
