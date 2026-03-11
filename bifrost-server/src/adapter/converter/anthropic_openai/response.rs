//! Transform OpenAI responses to Anthropic format
//!
//! This module provides functions to convert OpenAI API response format to Anthropic format.

use crate::error::LlmMapError;
use serde_json::{json, Value};

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
