//! Common utility functions for Anthropic <-> OpenAI conversions
//!
//! This module provides reusable conversion functions that can be shared
//! between different adapter implementations.

use crate::error::LlmMapError;
use serde_json::{Value, json};

/// Helper function to transform Anthropic request to OpenAI format
pub fn anthropic_to_openai_request(body: Value) -> Result<Value, LlmMapError> {
    let Value::Object(mut obj) = body else {
        return Err(LlmMapError::Validation(
            "Request body must be an object".into(),
        ));
    };

    // Extract and transform tools (if present)
    if let Some(tools) = obj.remove("tools").and_then(|v| v.as_array().cloned()) {
        let transformed_tools = transform_tools_anthropic_to_openai(tools)?;
        obj.insert("tools".to_string(), transformed_tools);
    }

    // Extract and transform tool_choice (if present)
    if let Some(tool_choice) = obj.get("tool_choice") {
        let transformed = transform_tool_choice_anthropic_to_openai(tool_choice)?;
        obj.insert("tool_choice".to_string(), transformed);
    }

    // Extract system message first (if exists)
    let system_msg = obj.remove("system").map(|system| {
        let system_content = extract_system_text(system);
        json!({
            "role": "system",
            "content": system_content
        })
    });

    // Take ownership of messages array
    let messages = obj
        .remove("messages")
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default();

    let mut openai_messages = Vec::new();

    // Add system message first if exists
    if let Some(sys) = system_msg {
        openai_messages.push(sys);
    }

    // Transform each message
    for msg in messages {
        let transformed = transform_message_anthropic_to_openai(msg)?;
        openai_messages.extend(transformed);
    }

    // Build final request
    let mut result = serde_json::Map::new();
    result.insert("messages".to_string(), Value::Array(openai_messages));

    // Copy other fields
    for (key, value) in obj {
        if key != "messages" && key != "system" {
            result.insert(key, value);
        }
    }

    Ok(Value::Object(result))
}

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
    let Value::Object(obj) = body else {
        return Err(LlmMapError::Validation(
            "Response body must be an object".into(),
        ));
    };

    // Extract basic fields
    let id = obj.get("id").cloned().unwrap_or(Value::String("".into()));
    let model = obj
        .get("model")
        .cloned()
        .unwrap_or(Value::String("".into()));
    let created = obj.get("created").cloned();

    // Extract choices array
    let choices = obj
        .get("choices")
        .and_then(|v| v.as_array())
        .ok_or_else(|| LlmMapError::Validation("choices array is required".into()))?;

    // We only process the first choice (index 0)
    let first_choice = choices.first().and_then(|v| v.as_object()).ok_or_else(|| {
        LlmMapError::Validation("choices array must have at least one element".into())
    })?;

    let message = first_choice
        .get("message")
        .and_then(|v| v.as_object())
        .ok_or_else(|| LlmMapError::Validation("message object is required".into()))?;

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
    if let Some(tool_calls) = message.get("tool_calls").and_then(|v| v.as_array()) {
        for tool_call in tool_calls {
            if let Some(tc_obj) = tool_call.as_object() {
                let id = tc_obj
                    .get("id")
                    .cloned()
                    .unwrap_or(Value::String("".into()));
                let name = tc_obj
                    .get("function")
                    .and_then(|v| v.as_object())
                    .and_then(|v| v.get("name"))
                    .cloned()
                    .unwrap_or(Value::String("".into()));

                // Parse arguments from JSON string to object
                let arguments_str = tc_obj
                    .get("function")
                    .and_then(|v| v.as_object())
                    .and_then(|v| v.get("arguments"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("{}");

                let input: Value = serde_json::from_str(arguments_str)
                    .unwrap_or(Value::Object(serde_json::Map::new()));

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
        Some("content_filter") => "end_turn",
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
