//! Transform OpenAI Responses API requests to Chat Completions format
//!
//! This module provides functions to convert OpenAI Responses API request format
//! to Chat Completions API compatible format.

use crate::error::LlmMapError;
use serde_json::{Value, json};

/// Convert OpenAI Responses API request to Chat Completions format.
///
/// This function transforms the Responses API request format to Chat Completions format,
/// handling input messages, tools, and various field mappings.
///
/// # Arguments
///
/// * `body` - The JSON request body in Responses API format
///
/// # Returns
///
/// A `Result` containing the transformed request in Chat Completions format,
/// or an `LlmMapError` if the transformation fails.
pub fn responses_to_chat_request(body: Value) -> Result<Value, LlmMapError> {
    let Value::Object(mut obj) = body else {
        return Err(LlmMapError::Validation(
            "Request body must be an object".into(),
        ));
    };

    let instructions = obj.remove("instructions");
    let input = obj.remove("input");
    let mut chat_messages = Vec::new();

    if let Some(input_val) = input {
        let messages = transform_input_to_messages(input_val, instructions)?;
        chat_messages.extend(messages);
    } else if let Some(instr) = instructions {
        let system_text = extract_text_from_value(instr);
        if !system_text.is_empty() {
            chat_messages.push(json!({
                "role": "system",
                "content": system_text
            }));
        }
    }

    obj.insert("messages".to_string(), Value::Array(chat_messages));

    if let Some(Value::Array(tools)) = obj.remove("tools") {
        let transformed_tools = transform_tools_to_chat_format(tools);
        obj.insert("tools".to_string(), transformed_tools);
    }

    if let Some(tool_choice) = obj.remove("tool_choice") {
        let transformed = transform_tool_choice_to_chat_format(tool_choice);
        obj.insert("tool_choice".to_string(), transformed);
    }

    if let Some(Value::Object(mut reasoning_obj)) = obj.remove("reasoning")
        && let Some(effort) = reasoning_obj.remove("effort")
    {
        obj.insert("reasoning_effort".to_string(), effort);
    }

    if let Some(max_output) = obj.remove("max_output_tokens") {
        obj.insert("max_tokens".to_string(), max_output);
    }

    if let Some(Value::Object(text_obj)) = obj.remove("text")
        && let Some(verbosity) = text_obj.get("verbosity")
    {
        obj.insert("verbosity".to_string(), verbosity.clone());
    }

    if let Some(stream_options) = obj.remove("stream_options")
        && let Some(transformed) = transform_stream_options(stream_options)
    {
        obj.insert("stream_options".to_string(), transformed);
    }

    Ok(Value::Object(obj))
}

// --- Helpers used by multiple transform functions ---

/// Normalize a Responses API role to Chat API role.
fn normalize_chat_role(role: &str) -> &str {
    match role {
        "developer" => "user",
        other => other,
    }
}

/// Build a chat message with proper content handling.
///
/// Converts empty content arrays to empty strings for Chat API compatibility.
fn build_chat_message(role: &str, content: Value) -> Value {
    let mut msg = serde_json::Map::new();
    msg.insert("role".to_string(), Value::String(role.to_string()));
    msg.insert(
        "content".to_string(),
        match content {
            Value::Array(ref arr) if arr.is_empty() => Value::String(String::new()),
            other => other,
        },
    );
    Value::Object(msg)
}

/// Try to merge content into the last user message if consecutive.
///
/// Returns `None` if merged (content consumed), `Some(content)` if not merged (content returned).
/// This handles consecutive user messages by combining their content into a single array.
fn try_merge_user_content(messages: &mut [Value], role: &str, content: Value) -> Option<Value> {
    if role != "user" {
        return Some(content);
    }

    let Some(Value::Object(msg_obj)) = messages.last_mut() else {
        return Some(content);
    };

    // Only merge if the last message is also a user message (not tool or assistant)
    if msg_obj.get("role").and_then(|v| v.as_str()) != Some("user") {
        return Some(content);
    }

    // Get existing content and convert to array of content blocks
    let existing_content = msg_obj.remove("content").unwrap_or(Value::Null);
    let mut existing_blocks = content_value_to_blocks(existing_content);

    // Convert new content to blocks and append
    let new_blocks = content_value_to_blocks(content);
    existing_blocks.extend(new_blocks);

    // If only one block and it's a simple text without detail, flatten to string
    if existing_blocks.len() == 1
        && let Value::Object(ref obj) = existing_blocks[0]
        && obj.get("type").and_then(|v| v.as_str()) == Some("text")
        && obj.contains_key("text")
        && !obj.contains_key("detail")
        && let Some(text) = obj.get("text")
    {
        msg_obj.insert("content".to_string(), text.clone());
        return None;
    }

    msg_obj.insert("content".to_string(), Value::Array(existing_blocks));
    None
}

/// Convert a content value to a vector of content blocks.
///
/// Handles string content (converts to text block) and array content (returns as-is).
fn content_value_to_blocks(content: Value) -> Vec<Value> {
    match content {
        Value::String(s) => {
            if s.is_empty() {
                Vec::new()
            } else {
                vec![json!({
                    "type": "text",
                    "text": s
                })]
            }
        }
        Value::Array(arr) => arr,
        Value::Null => Vec::new(),
        _ => Vec::new(),
    }
}

/// Try to merge a tool_call into the last assistant message.
///
/// Returns `None` if merged (tool_call consumed), `Some(tool_call)` if not merged (returned).
/// Merges when the immediately preceding message is an assistant message,
/// allowing tool_calls alongside content (Chat API supports this).
fn try_merge_tool_call(messages: &mut [Value], tool_call: Value) -> Option<Value> {
    let Some(Value::Object(msg_obj)) = messages.last_mut() else {
        return Some(tool_call);
    };

    if msg_obj.get("role").and_then(|v| v.as_str()) != Some("assistant") {
        return Some(tool_call);
    }

    let mut tool_calls = msg_obj
        .remove("tool_calls")
        .and_then(|v| match v {
            Value::Array(arr) => Some(arr),
            _ => None,
        })
        .unwrap_or_default();
    tool_calls.push(tool_call);
    msg_obj.insert("tool_calls".to_string(), Value::Array(tool_calls));

    None
}

/// Transform Responses API input to Chat API messages format.
fn transform_input_to_messages(
    input: Value,
    instructions: Option<Value>,
) -> Result<Vec<Value>, LlmMapError> {
    let items = match input {
        Value::Array(arr) => arr,
        Value::String(s) => {
            let mut messages = Vec::new();
            if let Some(instr) = instructions {
                let system_text = extract_text_from_value(instr);
                if !system_text.is_empty() {
                    messages.push(json!({
                        "role": "system",
                        "content": system_text
                    }));
                }
            }
            messages.push(json!({
                "role": "user",
                "content": s
            }));
            return Ok(messages);
        }
        _ => {
            return Err(LlmMapError::Validation(
                "Input must be a string or array".into(),
            ));
        }
    };

    let mut messages = Vec::new();
    let mut system_texts: Vec<String> = Vec::new();

    // Collect instructions as first system text
    if let Some(instr) = instructions {
        let system_text = extract_text_from_value(instr);
        if !system_text.is_empty() {
            system_texts.push(system_text);
        }
    }

    let mut found_non_developer = false;
    let mut pending_tool_call_count = 0usize;
    let mut buffered_messages: Vec<Value> = Vec::new();

    for item in items {
        // Extract role from the item for leading developer detection
        let item_role = match &item {
            Value::Object(obj) => obj.get("role").and_then(|v| v.as_str()),
            _ => None,
        };

        // Collect leading consecutive developer messages into system
        if !found_non_developer && item_role == Some("developer") {
            let text = extract_developer_text(&item);
            if !text.is_empty() {
                system_texts.push(text);
            }
            continue;
        }

        // Once we hit a non-developer message, flush collected system texts
        if !system_texts.is_empty() {
            let merged = system_texts.join("\n\n");
            messages.push(json!({
                "role": "system",
                "content": merged
            }));
            system_texts.clear();
        }

        found_non_developer = true;

        if let Value::String(s) = item {
            // Try to merge consecutive user messages
            if let Some(Value::String(s)) =
                try_merge_user_content(&mut messages, "user", Value::String(s))
            {
                messages.push(json!({
                    "role": "user",
                    "content": s
                }));
            }
            continue;
        }

        let Value::Object(mut obj) = item else {
            continue;
        };

        let type_field = obj.get("type").and_then(|v| v.as_str());

        match type_field {
            Some("function_call") => {
                let call_id = obj
                    .remove("call_id")
                    .and_then(|v| v.as_str().map(|s| s.to_string()))
                    .ok_or_else(|| {
                        LlmMapError::Validation("function_call missing call_id".into())
                    })?;
                let name = obj
                    .remove("name")
                    .and_then(|v| v.as_str().map(|s| s.to_string()))
                    .ok_or_else(|| LlmMapError::Validation("function_call missing name".into()))?;
                let arguments = obj.remove("arguments").ok_or_else(|| {
                    LlmMapError::Validation("function_call missing arguments".into())
                })?;

                pending_tool_call_count += 1;

                let tool_call = json!({
                    "id": call_id,
                    "type": "function",
                    "function": {
                        "name": name,
                        "arguments": arguments
                    }
                });

                if let Some(tool_call) = try_merge_tool_call(&mut messages, tool_call) {
                    let mut new_msg = serde_json::Map::new();
                    new_msg.insert("role".to_string(), Value::String("assistant".to_string()));
                    new_msg.insert("content".to_string(), Value::Null);
                    new_msg.insert("tool_calls".to_string(), Value::Array(vec![tool_call]));
                    messages.push(Value::Object(new_msg));
                }
            }
            Some("function_call_output") => {
                let call_id = obj
                    .remove("call_id")
                    .and_then(|v| v.as_str().map(|s| s.to_string()))
                    .ok_or_else(|| {
                        LlmMapError::Validation("function_call_output missing call_id".into())
                    })?;

                let output = obj.remove("output");
                let content = extract_text_from_value(output.unwrap_or(Value::Null));

                messages.push(json!({
                    "role": "tool",
                    "tool_call_id": call_id,
                    "content": content
                }));

                pending_tool_call_count = pending_tool_call_count.saturating_sub(1);
                if pending_tool_call_count == 0 {
                    messages.append(&mut buffered_messages);
                }
            }
            Some("reasoning") => {
                let reasoning_content = extract_reasoning_content(&obj);
                if reasoning_content.is_empty() {
                    continue;
                }

                if pending_tool_call_count > 0 {
                    let mut new_msg = serde_json::Map::new();
                    new_msg.insert("role".to_string(), Value::String("assistant".to_string()));
                    new_msg.insert("content".to_string(), Value::Null);
                    new_msg.insert(
                        "reasoning_content".to_string(),
                        Value::String(reasoning_content),
                    );
                    buffered_messages.push(Value::Object(new_msg));
                    continue;
                }

                // Create a new assistant message with reasoning_content
                let mut new_msg = serde_json::Map::new();
                new_msg.insert("role".to_string(), Value::String("assistant".to_string()));
                new_msg.insert("content".to_string(), Value::Null);
                new_msg.insert(
                    "reasoning_content".to_string(),
                    Value::String(reasoning_content),
                );
                messages.push(Value::Object(new_msg));
            }
            // "message" type and raw objects with role+content converge to the same path
            Some("message") | None | Some(_) => {
                // Determine the role string first, before mutating obj
                let is_message_type = type_field == Some("message");
                let role = obj
                    .get("role")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                let role_str = if is_message_type {
                    role.as_deref().unwrap_or("user")
                } else if let Some(ref r) = role {
                    r.as_str()
                } else {
                    continue;
                };

                let content = obj.remove("content");
                let Some(content_val) = content else {
                    continue;
                };

                let transformed = transform_response_message_content(content_val)?;

                // Buffer non-tool items that appear between tool calls and their results,
                // to avoid breaking the Chat API constraint that tool messages must
                // immediately follow the assistant message with matching tool_calls.
                if pending_tool_call_count > 0 {
                    let normalized_role = normalize_chat_role(role_str);
                    buffered_messages.push(build_chat_message(normalized_role, transformed));
                    continue;
                }

                // Try to merge content into previous assistant message with only reasoning_content
                if role_str == "assistant"
                    && let Some(Value::Object(msg_obj)) = messages.last_mut()
                    && msg_obj.get("role").and_then(|v| v.as_str()) == Some("assistant")
                    && matches!(msg_obj.get("content"), Some(Value::Null))
                    && msg_obj.contains_key("reasoning_content")
                {
                    msg_obj.insert("content".to_string(), transformed);
                    continue;
                }

                // Try to merge consecutive user messages
                let normalized_role = normalize_chat_role(role_str);
                if let Some(transformed) =
                    try_merge_user_content(&mut messages, normalized_role, transformed)
                {
                    messages.push(build_chat_message(normalized_role, transformed));
                }
            }
        }
    }

    // Flush any remaining system texts
    if !system_texts.is_empty() {
        let merged = system_texts.join("\n\n");
        messages.push(json!({
            "role": "system",
            "content": merged
        }));
    }

    // Flush any buffered messages that remain (e.g., if no function_call_output was found)
    if !buffered_messages.is_empty() {
        messages.append(&mut buffered_messages);
    }

    Ok(messages)
}

/// Extract text content from a developer message item.
fn extract_developer_text(item: &Value) -> String {
    let Value::Object(obj) = item else {
        return String::new();
    };

    let Some(content) = obj.get("content") else {
        return String::new();
    };

    extract_text_from_value(content.clone())
}

/// Transform content blocks from Responses API format to Chat API format.
fn transform_response_message_content(content: Value) -> Result<Value, LlmMapError> {
    let blocks = match content {
        Value::Array(arr) => arr,
        Value::String(s) => {
            return Ok(Value::String(s));
        }
        _ => {
            return Ok(Value::String(String::new()));
        }
    };

    let mut chat_content: Vec<Value> = Vec::new();

    for block in blocks {
        let Value::Object(mut obj) = block else {
            continue;
        };

        let block_type = obj.get("type").and_then(|v| v.as_str());

        match block_type {
            Some("input_text") | Some("output_text") => {
                if let Some(text) = obj.remove("text") {
                    let detail = obj.remove("detail");
                    let mut text_obj = serde_json::Map::new();
                    text_obj.insert("type".to_string(), Value::String("text".to_string()));
                    text_obj.insert("text".to_string(), text);
                    if let Some(d) = detail {
                        text_obj.insert("detail".to_string(), d);
                    }
                    chat_content.push(Value::Object(text_obj));
                }
            }
            Some("input_image") => {
                if let Some(image_url) = obj.remove("image_url") {
                    let detail = obj.remove("detail");
                    let mut image_url_obj = serde_json::Map::new();
                    image_url_obj.insert("url".to_string(), image_url);
                    if let Some(d) = detail {
                        image_url_obj.insert("detail".to_string(), d);
                    }
                    chat_content.push(json!({
                        "type": "image_url",
                        "image_url": Value::Object(image_url_obj)
                    }));
                }
            }
            Some("refusal") => {
                if let Some(text) = obj.remove("text") {
                    chat_content.push(json!({
                        "type": "text",
                        "text": text
                    }));
                }
            }
            Some("input_file") => {
                continue;
            }
            _ => {
                continue;
            }
        }
    }

    if chat_content.is_empty() {
        return Ok(Value::String(String::new()));
    }

    // Flatten single text block without detail to a plain string
    if chat_content.len() == 1 {
        let first = chat_content.into_iter().next().unwrap();
        if let Value::Object(ref obj) = first
            && obj.get("type").and_then(|v| v.as_str()) == Some("text")
            && obj.contains_key("text")
            && !obj.contains_key("detail")
            && let Some(text) = obj.get("text")
        {
            return Ok(text.clone());
        }
        return Ok(Value::Array(vec![first]));
    }

    Ok(Value::Array(chat_content))
}

/// Transform tools from Responses API format to Chat API format.
///
/// Responses: {type: "function", name: "x", description: "...", parameters: {...}, strict: true}
/// Chat:      {type: "function", function: {name: "x", description: "...", parameters: {...}, strict: true}}
fn transform_tools_to_chat_format(tools: Vec<Value>) -> Value {
    let transformed: Vec<Value> = tools
        .into_iter()
        .filter_map(|tool| {
            let Value::Object(mut obj) = tool else {
                return None;
            };

            if obj.get("type").and_then(|v| v.as_str()) != Some("function") {
                return None;
            }

            let mut func_obj = serde_json::Map::new();
            for field in ["name", "description", "parameters", "strict"] {
                if let Some(value) = obj.remove(field) {
                    func_obj.insert(field.to_string(), value);
                }
            }

            Some(json!({
                "type": "function",
                "function": Value::Object(func_obj)
            }))
        })
        .collect();

    Value::Array(transformed)
}

/// Transform tool_choice from Responses API format to Chat API format.
///
/// Responses: {type: "function", name: "x"}
/// Chat:     {type: "function", function: {name: "x"}}
fn transform_tool_choice_to_chat_format(tool_choice: Value) -> Value {
    match tool_choice.get("type").and_then(|v| v.as_str()) {
        Some("function") => {
            let Value::Object(mut obj) = tool_choice else {
                return tool_choice;
            };
            let mut func_obj = serde_json::Map::new();
            if let Some(n) = obj.remove("name") {
                func_obj.insert("name".to_string(), n);
            }
            json!({
                "type": "function",
                "function": Value::Object(func_obj)
            })
        }
        Some("auto") => Value::String("auto".into()),
        Some("none") => Value::String("none".into()),
        _ => tool_choice,
    }
}

fn transform_stream_options(stream_options: Value) -> Option<Value> {
    let Value::Object(mut obj) = stream_options else {
        return None;
    };

    obj.remove("include_obfuscation");

    if obj.is_empty() {
        None
    } else {
        Some(Value::Object(obj))
    }
}

fn extract_text_from_value(value: Value) -> String {
    match value {
        Value::String(s) => s,
        Value::Array(arr) => arr
            .iter()
            .filter_map(|item| {
                if let Value::Object(obj) = item
                    && obj.get("type").and_then(|v| v.as_str()) == Some("input_text")
                {
                    return obj.get("text").and_then(|v| v.as_str());
                }
                None
            })
            .collect::<Vec<_>>()
            .join("\n\n"),
        _ => String::new(),
    }
}

/// Extract reasoning_content from a reasoning block.
///
/// Extracts text from summary and content arrays, joining them into a single string.
fn extract_reasoning_content(reasoning_obj: &serde_json::Map<String, Value>) -> String {
    let mut texts = Vec::new();

    if let Some(Value::Array(summary)) = reasoning_obj.get("summary") {
        for item in summary {
            if let Value::Object(obj) = item
                && obj.get("type").and_then(|v| v.as_str()) == Some("summary_text")
                && let Some(text) = obj.get("text").and_then(|v| v.as_str())
            {
                texts.push(text);
            }
        }
    }

    if let Some(Value::Array(content)) = reasoning_obj.get("content") {
        for item in content {
            if let Value::Object(obj) = item
                && obj.get("type").and_then(|v| v.as_str()) == Some("reasoning_text")
                && let Some(text) = obj.get("text").and_then(|v| v.as_str())
            {
                texts.push(text);
            }
        }
    }

    texts.join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    // ============================================
    // Input 类型的测试
    // ============================================

    #[test]
    fn test_string_input() {
        let input = json!({
            "model": "gpt-4o",
            "input": "Hello, how are you?"
        });

        let expected = json!({
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": "Hello, how are you?"}]
        });

        let result = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_mixed_input_types() {
        // 连续的 user 消息被合并
        let input = json!({
            "model": "gpt-4o",
            "input": [
                "Hello!",
                {"role": "user", "content": "How are you?"}
            ]
        });

        let expected = json!({
            "model": "gpt-4o",
            "messages": [
                {
                    "role": "user",
                    "content": [
                        {"type": "text", "text": "Hello!"},
                        {"type": "text", "text": "How are you?"}
                    ]
                }
            ]
        });

        let result = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected);
    }

    // ============================================
    // Role 类型的测试 (text content)
    // ============================================

    #[test]
    fn test_role_system() {
        let input = json!({
            "model": "gpt-4o",
            "input": [{"role": "system", "content": [{"type": "input_text", "text": "You are helpful."}]}]
        });

        let expected = json!({
            "model": "gpt-4o",
            "messages": [{"role": "system", "content": "You are helpful."}]
        });

        let result = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_role_developer() {
        // 中间的 developer -> user，连续的 user 消息被合并
        let input = json!({
            "model": "gpt-4o",
            "input": [
                {"role": "user", "content": "Hello"},
                {"role": "developer", "content": [{"type": "input_text", "text": "You are Codex."}]}
            ]
        });

        let expected = json!({
            "model": "gpt-4o",
            "messages": [
                {
                    "role": "user",
                    "content": [
                        {"type": "text", "text": "Hello"},
                        {"type": "text", "text": "You are Codex."}
                    ]
                }
            ]
        });

        let result = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_leading_developer_to_system() {
        // 开头连续的 developer 消息合并到 system
        let input = json!({
            "model": "gpt-4o",
            "input": [
                {"role": "developer", "content": [{"type": "input_text", "text": "You are a coding assistant."}]},
                {"role": "developer", "content": [{"type": "input_text", "text": "Be concise."}]},
                {"role": "user", "content": "Hello"}
            ]
        });

        let expected = json!({
            "model": "gpt-4o",
            "messages": [
                {"role": "system", "content": "You are a coding assistant.\n\nBe concise."},
                {"role": "user", "content": "Hello"}
            ]
        });

        let result = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_single_leading_developer_to_system() {
        // 单个开头的 developer 消息转为 system
        let input = json!({
            "model": "gpt-4o",
            "input": [
                {"role": "developer", "content": [{"type": "input_text", "text": "You are helpful."}]},
                {"role": "user", "content": "Hello"}
            ]
        });

        let expected = json!({
            "model": "gpt-4o",
            "messages": [
                {"role": "system", "content": "You are helpful."},
                {"role": "user", "content": "Hello"}
            ]
        });

        let result = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_developer_mixed_leading_and_middle() {
        // 开头的 developer 合并到 system，中间的 developer 当作 user，连续的 user 消息被合并
        let input = json!({
            "model": "gpt-4o",
            "input": [
                {"role": "developer", "content": [{"type": "input_text", "text": "System instruction 1."}]},
                {"role": "developer", "content": [{"type": "input_text", "text": "System instruction 2."}]},
                {"role": "user", "content": "Hello"},
                {"role": "developer", "content": [{"type": "input_text", "text": "Mid conversation developer."}]}
            ]
        });

        let expected = json!({
            "model": "gpt-4o",
            "messages": [
                {"role": "system", "content": "System instruction 1.\n\nSystem instruction 2."},
                {
                    "role": "user",
                    "content": [
                        {"type": "text", "text": "Hello"},
                        {"type": "text", "text": "Mid conversation developer."}
                    ]
                }
            ]
        });

        let result = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_instructions_and_developer_merged() {
        // instructions 和开头的 developer 消息合并为一个 system 消息
        let input = json!({
            "model": "gpt-4o",
            "instructions": "You are a helpful assistant.",
            "input": [
                {"role": "developer", "content": [{"type": "input_text", "text": "Be concise."}]},
                {"role": "user", "content": "Hello"}
            ]
        });

        let expected = json!({
            "model": "gpt-4o",
            "messages": [
                {"role": "system", "content": "You are a helpful assistant.\n\nBe concise."},
                {"role": "user", "content": "Hello"}
            ]
        });

        let result = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_instructions_and_multiple_developer_merged() {
        // instructions 和多个开头的 developer 消息合并
        let input = json!({
            "model": "gpt-4o",
            "instructions": "You are a coding assistant.",
            "input": [
                {"role": "developer", "content": [{"type": "input_text", "text": "Write clean code."}]},
                {"role": "developer", "content": [{"type": "input_text", "text": "Follow best practices."}]},
                {"role": "user", "content": "Write a function."}
            ]
        });

        let expected = json!({
            "model": "gpt-4o",
            "messages": [
                {"role": "system", "content": "You are a coding assistant.\n\nWrite clean code.\n\nFollow best practices."},
                {"role": "user", "content": "Write a function."}
            ]
        });

        let result = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_instructions_only() {
        // 只有 instructions，没有 developer
        let input = json!({
            "model": "gpt-4o",
            "instructions": "You are helpful.",
            "input": [
                {"role": "user", "content": "Hello"}
            ]
        });

        let expected = json!({
            "model": "gpt-4o",
            "messages": [
                {"role": "system", "content": "You are helpful."},
                {"role": "user", "content": "Hello"}
            ]
        });

        let result = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_role_user() {
        let input = json!({
            "model": "gpt-4o",
            "input": [{"role": "user", "content": [{"type": "input_text", "text": "Write a function."}]}]
        });

        let expected = json!({
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": "Write a function."}]
        });

        let result = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_consecutive_user_messages_merged() {
        // 连续的 user 消息应该合并为一个包含多个 content blocks 的消息
        let input = json!({
            "model": "gpt-4o",
            "input": [
                {"role": "user", "content": [{"type": "input_text", "text": "First message."}]},
                {"role": "user", "content": [{"type": "input_text", "text": "Second message."}]},
                {"role": "user", "content": [{"type": "input_text", "text": "Third message."}]},
                {"role": "assistant", "content": [{"type": "output_text", "text": "Response."}]}
            ]
        });

        let expected = json!({
            "model": "gpt-4o",
            "messages": [
                {
                    "role": "user",
                    "content": [
                        {"type": "text", "text": "First message."},
                        {"type": "text", "text": "Second message."},
                        {"type": "text", "text": "Third message."}
                    ]
                },
                {"role": "assistant", "content": "Response."}
            ]
        });

        let result = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_role_assistant() {
        let input = json!({
            "model": "gpt-4o",
            "input": [{"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "Sure!"}]}]
        });

        let expected = json!({
            "model": "gpt-4o",
            "messages": [{"role": "assistant", "content": "Sure!"}]
        });

        let result = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected);
    }

    // ============================================
    // Role 类型的测试 (array content - text + image)
    // ============================================

    #[test]
    fn test_role_user_with_array_content() {
        let input = json!({
            "model": "gpt-4o",
            "input": [{
                "role": "user",
                "content": [
                    {"type": "input_text", "text": "Text content"},
                    {"type": "input_image", "image_url": "https://example.com/img.png"}
                ]
            }]
        });

        let expected = json!({
            "model": "gpt-4o",
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "text", "text": "Text content"},
                    {"type": "image_url", "image_url": {"url": "https://example.com/img.png"}}
                ]
            }]
        });

        let result = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_role_developer_with_array_content() {
        // 开头的 developer 带 array content -> system
        let input = json!({
            "model": "gpt-4o",
            "input": [{
                "role": "developer",
                "content": [
                    {"type": "input_text", "text": "You are a helpful assistant."},
                    {"type": "input_image", "image_url": "https://example.com/img.png"}
                ]
            }]
        });

        // 开头的 developer 转为 system，但 array content 中有非文本内容会被过滤
        let expected = json!({
            "model": "gpt-4o",
            "messages": [{
                "role": "system",
                "content": "You are a helpful assistant."
            }]
        });

        let result = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_role_assistant_with_array_content() {
        let input = json!({
            "model": "gpt-4o",
            "input": [{
                "type": "message",
                "role": "assistant",
                "content": [
                    {"type": "output_text", "text": "Here is the image:"},
                    {"type": "input_image", "image_url": "https://example.com/result.png"}
                ]
            }]
        });

        let expected = json!({
            "model": "gpt-4o",
            "messages": [{
                "role": "assistant",
                "content": [
                    {"type": "text", "text": "Here is the image:"},
                    {"type": "image_url", "image_url": {"url": "https://example.com/result.png"}}
                ]
            }]
        });

        let result = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected);
    }

    // ============================================
    // Image 详细测试
    // ============================================

    #[test]
    fn test_input_image_base64() {
        let input = json!({
            "model": "gpt-4o",
            "input": [{
                "role": "user",
                "content": [{
                    "type": "input_image",
                    "image_url": "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg=="
                }]
            }]
        });

        let expected = json!({
            "model": "gpt-4o",
            "messages": [{
                "role": "user",
                "content": [{
                    "type": "image_url",
                    "image_url": {"url": "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg=="}
                }]
            }]
        });

        let result = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_input_image_with_detail() {
        let input = json!({
            "model": "gpt-4o",
            "input": [{
                "role": "user",
                "content": [{
                    "type": "input_image",
                    "image_url": "https://example.com/photo.jpg",
                    "detail": "high"
                }]
            }]
        });

        let expected = json!({
            "model": "gpt-4o",
            "messages": [{
                "role": "user",
                "content": [{
                    "type": "image_url",
                    "image_url": {"url": "https://example.com/photo.jpg", "detail": "high"}
                }]
            }]
        });

        let result = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_multimodal_text_and_image() {
        let input = json!({
            "model": "gpt-4o",
            "input": [{
                "role": "user",
                "content": [
                    {"type": "input_text", "text": "Look at this image:"},
                    {"type": "input_image", "image_url": "https://example.com/photo.jpg"}
                ]
            }]
        });

        let expected = json!({
            "model": "gpt-4o",
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "text", "text": "Look at this image:"},
                    {"type": "image_url", "image_url": {"url": "https://example.com/photo.jpg"}}
                ]
            }]
        });

        let result = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected);
    }

    // ============================================
    // 工具调用测试
    // ============================================

    #[test]
    fn test_function_call() {
        let input = json!({
            "model": "gpt-4o",
            "input": [{
                "type": "function_call",
                "call_id": "call_abc",
                "name": "get_weather",
                "arguments": "{\"city\": \"Tokyo\"}"
            }]
        });

        let expected = json!({
            "model": "gpt-4o",
            "messages": [{
                "role": "assistant",
                "content": null,
                "tool_calls": [{
                    "id": "call_abc",
                    "type": "function",
                    "function": {"name": "get_weather", "arguments": "{\"city\": \"Tokyo\"}"}
                }]
            }]
        });

        let result = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_function_call_output() {
        let input = json!({
            "model": "gpt-4o",
            "input": [{
                "type": "function_call_output",
                "call_id": "call_abc123",
                "output": "{\"temperature\": 22}"
            }]
        });

        let expected = json!({
            "model": "gpt-4o",
            "messages": [{
                "role": "tool",
                "tool_call_id": "call_abc123",
                "content": "{\"temperature\": 22}"
            }]
        });

        let result = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_function_call_and_output_flow() {
        // 完整的 function_call -> function_call_output 流程
        let input = json!({
            "model": "gpt-4o",
            "input": [
                {
                    "type": "function_call",
                    "call_id": "call_1",
                    "name": "get_weather",
                    "arguments": "{\"city\": \"Tokyo\"}"
                },
                {
                    "type": "function_call_output",
                    "call_id": "call_1",
                    "output": "{\"temperature\": 22}"
                },
                {
                    "type": "message",
                    "role": "assistant",
                    "content": [{"type": "output_text", "text": "The weather in Tokyo is 22°C."}]
                }
            ]
        });

        let expected = json!({
            "model": "gpt-4o",
            "messages": [
                {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": {"name": "get_weather", "arguments": "{\"city\": \"Tokyo\"}"}
                    }]
                },
                {
                    "role": "tool",
                    "tool_call_id": "call_1",
                    "content": "{\"temperature\": 22}"
                },
                {
                    "role": "assistant",
                    "content": "The weather in Tokyo is 22°C."
                }
            ]
        });

        let result = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_multiple_consecutive_function_calls_merged() {
        let input = json!({
            "model": "gpt-4o",
            "input": [
                {
                    "type": "function_call",
                    "call_id": "call_1",
                    "name": "exec_command",
                    "arguments": "{\"cmd\": \"ls -la\"}"
                },
                {
                    "type": "function_call",
                    "call_id": "call_2",
                    "name": "exec_command",
                    "arguments": "{\"cmd\": \"cat README.md\"}"
                },
                {
                    "type": "function_call_output",
                    "call_id": "call_1",
                    "output": "total 300"
                },
                {
                    "type": "function_call_output",
                    "call_id": "call_2",
                    "output": "# Bifrost"
                }
            ]
        });

        let expected = json!({
            "model": "gpt-4o",
            "messages": [
                {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [
                        {
                            "id": "call_1",
                            "type": "function",
                            "function": {"name": "exec_command", "arguments": "{\"cmd\": \"ls -la\"}"}
                        },
                        {
                            "id": "call_2",
                            "type": "function",
                            "function": {"name": "exec_command", "arguments": "{\"cmd\": \"cat README.md\"}"}
                        }
                    ]
                },
                {
                    "role": "tool",
                    "tool_call_id": "call_1",
                    "content": "total 300"
                },
                {
                    "role": "tool",
                    "tool_call_id": "call_2",
                    "content": "# Bifrost"
                }
            ]
        });

        let result = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_function_call_not_merged_across_assistant_text() {
        let input = json!({
            "model": "gpt-4o",
            "input": [
                {
                    "type": "function_call",
                    "call_id": "call_1",
                    "name": "exec_command",
                    "arguments": "{\"cmd\": \"ls\"}"
                },
                {
                    "type": "function_call",
                    "call_id": "call_2",
                    "name": "exec_command",
                    "arguments": "{\"cmd\": \"cat file\"}"
                },
                {
                    "type": "function_call_output",
                    "call_id": "call_1",
                    "output": "file1.txt"
                },
                {
                    "type": "function_call_output",
                    "call_id": "call_2",
                    "output": "file content"
                },
                {
                    "type": "message",
                    "role": "assistant",
                    "content": [{"type": "output_text", "text": "Let me check more."}]
                },
                {
                    "type": "function_call",
                    "call_id": "call_3",
                    "name": "exec_command",
                    "arguments": "{\"cmd\": \"grep pattern\"}"
                },
                {
                    "type": "function_call_output",
                    "call_id": "call_3",
                    "output": "matched line"
                }
            ]
        });

        let expected = json!({
            "model": "gpt-4o",
            "messages": [
                {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [
                        {
                            "id": "call_1",
                            "type": "function",
                            "function": {"name": "exec_command", "arguments": "{\"cmd\": \"ls\"}"}
                        },
                        {
                            "id": "call_2",
                            "type": "function",
                            "function": {"name": "exec_command", "arguments": "{\"cmd\": \"cat file\"}"}
                        }
                    ]
                },
                {
                    "role": "tool",
                    "tool_call_id": "call_1",
                    "content": "file1.txt"
                },
                {
                    "role": "tool",
                    "tool_call_id": "call_2",
                    "content": "file content"
                },
                {
                    "role": "assistant",
                    "content": "Let me check more.",
                    "tool_calls": [
                        {
                            "id": "call_3",
                            "type": "function",
                            "function": {"name": "exec_command", "arguments": "{\"cmd\": \"grep pattern\"}"}
                        }
                    ]
                },
                {
                    "role": "tool",
                    "tool_call_id": "call_3",
                    "content": "matched line"
                }
            ]
        });

        let result = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_tools_format() {
        let input = json!({
            "model": "gpt-4o",
            "input": "test",
            "tools": [{
                "type": "function",
                "name": "get_weather",
                "description": "Get weather for a city",
                "parameters": {
                    "type": "object",
                    "properties": {"city": {"type": "string"}}
                },
                "strict": true
            }]
        });

        let expected = json!({
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": "test"}],
            "tools": [{
                "type": "function",
                "function": {
                    "name": "get_weather",
                    "description": "Get weather for a city",
                    "parameters": {
                        "type": "object",
                        "properties": {"city": {"type": "string"}}
                    },
                    "strict": true
                }
            }]
        });

        let result = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_non_function_tools_filtered_out() {
        let input = json!({
            "model": "gpt-4o",
            "input": "test",
            "tools": [
                {
                    "type": "function",
                    "name": "get_weather",
                    "description": "Get weather",
                    "parameters": {"type": "object", "properties": {}}
                },
                {
                    "type": "web_search",
                    "external_web_access": false
                }
            ]
        });

        let expected = json!({
            "model": "gpt-4o",
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

        let result = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_tool_choice_format() {
        let input = json!({
            "model": "gpt-4o",
            "input": "test",
            "tool_choice": {"type": "function", "name": "get_weather"}
        });

        let expected = json!({
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": "test"}],
            "tool_choice": {"type": "function", "function": {"name": "get_weather"}}
        });

        let result = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected);
    }

    // ============================================
    // 其他字段转换测试
    // ============================================

    #[test]
    fn test_instructions_as_system_message() {
        let input = json!({
            "model": "gpt-4o",
            "input": "Hello",
            "instructions": "You are a helpful assistant."
        });

        let expected = json!({
            "model": "gpt-4o",
            "messages": [
                {"role": "system", "content": "You are a helpful assistant."},
                {"role": "user", "content": "Hello"}
            ]
        });

        let result = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_instructions_with_array_content() {
        let input = json!({
            "model": "gpt-4o",
            "input": "Hello",
            "instructions": [
                {"type": "input_text", "text": "You are a coding assistant."}
            ]
        });

        let expected = json!({
            "model": "gpt-4o",
            "messages": [
                {"role": "system", "content": "You are a coding assistant."},
                {"role": "user", "content": "Hello"}
            ]
        });

        let result = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_max_output_tokens() {
        let input = json!({
            "model": "gpt-4o",
            "input": "test",
            "max_output_tokens": 1000
        });

        let expected = json!({
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": "test"}],
            "max_tokens": 1000
        });

        let result = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_reasoning_effort() {
        let input = json!({
            "model": "gpt-4o",
            "input": "test",
            "reasoning": {"effort": "medium"}
        });

        let expected = json!({
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": "test"}],
            "reasoning_effort": "medium"
        });

        let result = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_passthrough_fields() {
        let input = json!({
            "model": "gpt-4o",
            "input": "test",
            "stream": true,
            "temperature": 0.7,
            "top_p": 0.9,
            "parallel_tool_calls": true
        });

        let expected = json!({
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": "test"}],
            "stream": true,
            "temperature": 0.7,
            "top_p": 0.9,
            "parallel_tool_calls": true
        });

        let result = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected);
    }

    // ============================================
    // 跳过/忽略的内容测试
    // ============================================

    #[test]
    fn test_reasoning_converted_to_reasoning_content() {
        let input = json!({
            "model": "gpt-4o",
            "input": [{
                "type": "reasoning",
                "id": "reasoning_1",
                "summary": [{"type": "summary_text", "text": "Thought about the problem."}],
                "content": [{"type": "reasoning_text", "text": "Let me think..."}]
            }]
        });

        let expected = json!({
            "model": "gpt-4o",
            "messages": [{
                "role": "assistant",
                "content": null,
                "reasoning_content": "Thought about the problem.\n\nLet me think..."
            }]
        });

        let result = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_reasoning_content_toolcalls_merged() {
        let input = json!({
            "model": "gpt-4o",
            "input": [
                {
                    "type": "reasoning",
                    "id": "reasoning_1",
                    "summary": [{"type": "summary_text", "text": "Analyzed the problem."}],
                    "content": [{"type": "reasoning_text", "text": "Let me think step by step."}]
                },
                {
                    "type": "message",
                    "role": "assistant",
                    "content": [{"type": "output_text", "text": "Here is the solution."}]
                },
                {
                    "type": "function_call",
                    "call_id": "call_1",
                    "name": "exec_command",
                    "arguments": "{\"cmd\": \"ls -la\"}"
                },
                {
                    "type": "function_call",
                    "call_id": "call_2",
                    "name": "exec_command",
                    "arguments": "{\"cmd\": \"cat file.txt\"}"
                }
            ]
        });

        let expected = json!({
            "model": "gpt-4o",
            "messages": [{
                "role": "assistant",
                "content": "Here is the solution.",
                "reasoning_content": "Analyzed the problem.\n\nLet me think step by step.",
                "tool_calls": [
                    {
                        "id": "call_1",
                        "type": "function",
                        "function": {"name": "exec_command", "arguments": "{\"cmd\": \"ls -la\"}"}
                    },
                    {
                        "id": "call_2",
                        "type": "function",
                        "function": {"name": "exec_command", "arguments": "{\"cmd\": \"cat file.txt\"}"}
                    }
                ]
            }]
        });

        let result = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_input_file_skipped() {
        let input = json!({
            "model": "gpt-4o",
            "input": [{
                "role": "user",
                "content": [
                    {"type": "input_text", "text": "Read this file:"},
                    {"type": "input_file", "file_id": "file_123", "filename": "doc.pdf"}
                ]
            }]
        });

        let expected = json!({
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": "Read this file:"}]
        });

        let result = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_refusal_converted_to_text() {
        let input = json!({
            "model": "gpt-4o",
            "input": [{
                "type": "message",
                "role": "assistant",
                "content": [{"type": "refusal", "text": "I cannot answer that."}]
            }]
        });

        let expected = json!({
            "model": "gpt-4o",
            "messages": [{"role": "assistant", "content": "I cannot answer that."}]
        });

        let result = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_stream_options_filter_obfuscation() {
        let input = json!({
            "model": "gpt-4o",
            "input": "test",
            "stream_options": {"include_obfuscation": true}
        });

        let expected = json!({
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": "test"}]
        });

        let result = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_stream_options_preserved() {
        let input = json!({
            "model": "gpt-4o",
            "input": "test",
            "stream_options": {"include_usage": true, "continuous_usage_stats": true}
        });

        let expected = json!({
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": "test"}],
            "stream_options": {"include_usage": true, "continuous_usage_stats": true}
        });

        let result = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_text_verbosity_to_verbosity() {
        let input = json!({
            "model": "gpt-4o",
            "input": "test",
            "text": {"verbosity": "high"}
        });

        let expected = json!({
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": "test"}],
            "verbosity": "high"
        });

        let result = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_text_format_dropped() {
        let input = json!({
            "model": "gpt-4o",
            "input": "test",
            "text": {"format": {"type": "json_schema", "json_schema": {"name": "test"}}}
        });

        let expected = json!({
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": "test"}]
        });

        let result = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected);
    }

    // ============================================
    // 完整流程测试
    // ============================================

    #[test]
    fn test_full_conversation_with_tools() {
        let input = json!({
            "model": "gpt-4o",
            "instructions": "You are a helpful coding assistant.",
            "input": [
                {"role": "user", "content": "Write a hello world function."},
                {
                    "type": "message",
                    "role": "assistant",
                    "content": [{"type": "output_text", "text": "Sure! Here it is:"}]
                },
                {
                    "type": "function_call",
                    "call_id": "call_1",
                    "name": "write_file",
                    "arguments": "{\"filename\": \"hello.py\", \"content\": \"print('Hello, World!')\"}"
                },
                {
                    "type": "function_call_output",
                    "call_id": "call_1",
                    "output": "File written successfully."
                },
                {
                    "type": "message",
                    "role": "assistant",
                    "content": [{"type": "output_text", "text": "I've written the hello world function for you."}]
                }
            ],
            "tools": [{
                "type": "function",
                "name": "write_file",
                "description": "Write content to a file",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "filename": {"type": "string"},
                        "content": {"type": "string"}
                    }
                },
                "strict": true
            }],
            "tool_choice": {"type": "function", "name": "write_file"},
            "max_output_tokens": 500,
            "temperature": 0.5,
            "stream": false
        });

        let expected = json!({
            "model": "gpt-4o",
            "messages": [
                {"role": "system", "content": "You are a helpful coding assistant."},
                {"role": "user", "content": "Write a hello world function."},
                {
                    "role": "assistant",
                    "content": "Sure! Here it is:",
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": {
                            "name": "write_file",
                            "arguments": "{\"filename\": \"hello.py\", \"content\": \"print('Hello, World!')\"}"
                        }
                    }]
                },
                {"role": "tool", "tool_call_id": "call_1", "content": "File written successfully."},
                {"role": "assistant", "content": "I've written the hello world function for you."}
            ],
            "tools": [{
                "type": "function",
                "function": {
                    "name": "write_file",
                    "description": "Write content to a file",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "filename": {"type": "string"},
                            "content": {"type": "string"}
                        }
                    },
                    "strict": true
                }
            }],
            "tool_choice": {"type": "function", "function": {"name": "write_file"}},
            "max_tokens": 500,
            "temperature": 0.5,
            "stream": false
        });

        let result = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_developer_message_between_tool_call_and_result() {
        // Responses API 允许在 function_call 和 function_call_output 之间插入
        // developer 消息（如审批通知）。Chat API 不允许 tool_calls 和 tool 消息
        // 之间有其他角色消息，因此 developer 消息应被缓冲到 tool 消息之后。
        let input = json!({
            "model": "gpt-4o",
            "input": [
                {
                    "type": "function_call",
                    "call_id": "call_1",
                    "name": "exec_command",
                    "arguments": "{\"cmd\": \"ls\"}"
                },
                {
                    "type": "message",
                    "role": "developer",
                    "content": [{"type": "input_text", "text": "Approved command prefix saved:\n- [\"/bin/bash\", \"-lc\", \"ls\"]"}]
                },
                {
                    "type": "function_call_output",
                    "call_id": "call_1",
                    "output": "file1.txt"
                }
            ]
        });

        let expected = json!({
            "model": "gpt-4o",
            "messages": [
                {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": {"name": "exec_command", "arguments": "{\"cmd\": \"ls\"}"}
                    }]
                },
                {
                    "role": "tool",
                    "tool_call_id": "call_1",
                    "content": "file1.txt"
                },
                {
                    "role": "user",
                    "content": "Approved command prefix saved:\n- [\"/bin/bash\", \"-lc\", \"ls\"]"
                }
            ]
        });

        let result = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_multiple_tool_calls_with_interleaved_developer() {
        // 多个并行的 tool call 之间插入了 developer 消息，
        // 验证 developer 被缓冲到所有 tool 结果之后
        let input = json!({
            "model": "gpt-4o",
            "input": [
                {
                    "type": "function_call",
                    "call_id": "call_1",
                    "name": "exec_command",
                    "arguments": "{\"cmd\": \"ls\"}"
                },
                {
                    "type": "function_call",
                    "call_id": "call_2",
                    "name": "exec_command",
                    "arguments": "{\"cmd\": \"pwd\"}"
                },
                {
                    "type": "message",
                    "role": "developer",
                    "content": [{"type": "input_text", "text": "Both commands approved."}]
                },
                {
                    "type": "function_call_output",
                    "call_id": "call_1",
                    "output": "file1.txt"
                },
                {
                    "type": "function_call_output",
                    "call_id": "call_2",
                    "output": "/home"
                },
                {
                    "type": "message",
                    "role": "user",
                    "content": [{"type": "input_text", "text": "继续"}]
                }
            ]
        });

        let expected = json!({
            "model": "gpt-4o",
            "messages": [
                {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [
                        {
                            "id": "call_1",
                            "type": "function",
                            "function": {"name": "exec_command", "arguments": "{\"cmd\": \"ls\"}"}
                        },
                        {
                            "id": "call_2",
                            "type": "function",
                            "function": {"name": "exec_command", "arguments": "{\"cmd\": \"pwd\"}"}
                        }
                    ]
                },
                {
                    "role": "tool",
                    "tool_call_id": "call_1",
                    "content": "file1.txt"
                },
                {
                    "role": "tool",
                    "tool_call_id": "call_2",
                    "content": "/home"
                },
                {
                    "role": "user",
                    "content": [
                        {"type": "text", "text": "Both commands approved."},
                        {"type": "text", "text": "继续"}
                    ]
                }
            ]
        });

        let result = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected);
    }
}
