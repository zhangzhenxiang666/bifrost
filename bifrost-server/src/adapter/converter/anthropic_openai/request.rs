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
