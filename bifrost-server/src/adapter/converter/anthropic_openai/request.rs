//! Transform Anthropic requests to OpenAI format
//!
//! This module provides functions to convert Anthropic API request format to OpenAI-compatible format.

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

    // Move other fields
    for (key, value) in obj {
        result.insert(key, value);
    }

    Ok(Value::Object(result))
}
