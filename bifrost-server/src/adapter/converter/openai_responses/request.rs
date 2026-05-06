//! Transform OpenAI Responses API requests to Chat Completions format
//!
//! This module provides functions to convert OpenAI Responses API request format
//! to Chat Completions API compatible format.

use crate::adapter::converter::create_null_string;
use crate::adapter::converter::openai_responses::NamespaceMappings;
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
pub fn responses_to_chat_request(body: Value) -> Result<(Value, NamespaceMappings), LlmMapError> {
    let Value::Object(mut obj) = body else {
        return Err(LlmMapError::Validation(
            "Request body must be an object".into(),
        ));
    };

    let instructions = obj.remove("instructions");
    let input = obj.remove("input");
    let mut namespace_mappings = NamespaceMappings::new();
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
        let (transformed_tools, mappings) = transform_tools_to_chat_format(tools);
        namespace_mappings = mappings;
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

    Ok((Value::Object(obj), namespace_mappings))
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

    // Validate and fix assistant messages to comply with Chat API requirements.
    // The Chat API requires every assistant message to have either `content` or `tool_calls`.
    // If an assistant message has neither (e.g., from a `reasoning` item that only produced
    // `reasoning_content` and no subsequent `content` or `tool_calls` merged in), set
    // `content` to an empty string to satisfy the requirement.
    for msg in &mut messages {
        if let Some(obj) = msg.as_object_mut()
            && obj.get("role").and_then(|v| v.as_str()) == Some("assistant")
        {
            let has_content = match obj.get("content") {
                Some(Value::String(s)) => !s.is_empty(),
                Some(Value::Array(arr)) => !arr.is_empty(),
                Some(Value::Null) => false,
                None => false,
                Some(_) => true,
            };
            let has_tool_calls = obj.get("tool_calls").is_some()
                && obj
                    .get("tool_calls")
                    .is_some_and(|v| !v.as_array().is_some_and(|a| a.is_empty()));

            if !has_content && !has_tool_calls {
                obj.insert("content".to_string(), create_null_string());
            }
        }
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
fn transform_tools_to_chat_format(tools: Vec<Value>) -> (Value, NamespaceMappings) {
    let mut namespace_mappings = NamespaceMappings::new();
    let mut transformed: Vec<Value> = Vec::new();

    for tool in tools {
        let Value::Object(mut obj) = tool else {
            continue;
        };

        let tool_type = obj.get("type").and_then(|v| v.as_str());
        match tool_type {
            Some("function") => {
                let mut func_obj = serde_json::Map::new();
                for field in ["name", "description", "parameters", "strict"] {
                    if let Some(value) = obj.remove(field) {
                        func_obj.insert(field.to_string(), value);
                    }
                }
                transformed.push(json!({
                    "type": "function",
                    "function": Value::Object(func_obj)
                }));
            }
            Some("namespace") => {
                let Some(namespace_name) = obj
                    .remove("name")
                    .and_then(|v| v.as_str().map(String::from))
                else {
                    continue;
                };
                let Some(sub_tools) = obj.remove("tools").and_then(|v| v.as_array().cloned())
                else {
                    continue;
                };

                for sub_tool in sub_tools {
                    let Value::Object(mut sub_obj) = sub_tool else {
                        continue;
                    };
                    if sub_obj.get("type").and_then(|v| v.as_str()) != Some("function") {
                        continue;
                    }

                    let mut func_obj = serde_json::Map::new();
                    for field in ["name", "description", "parameters", "strict"] {
                        if let Some(value) = sub_obj.remove(field) {
                            func_obj.insert(field.to_string(), value);
                        }
                    }

                    // Prefix the tool name with the namespace name.
                    if let Some(name) = func_obj.get("name").and_then(|v| v.as_str()) {
                        let prefixed_name = format!("{namespace_name}{name}");
                        func_obj.insert("name".to_string(), Value::String(prefixed_name));
                    }

                    transformed.push(json!({
                        "type": "function",
                        "function": Value::Object(func_obj)
                    }));
                }

                namespace_mappings.add_namespace(namespace_name);
            }
            _ => continue,
        }
    }

    (Value::Array(transformed), namespace_mappings)
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
    //  Full Integration / Smoke Test
    // ============================================

    #[test]
    fn test_full_conversation_with_tools() {
        let input = json!({
            "model": "gpt-4o",
            "input": "Hello",
            "instructions": "You are a helpful assistant.",
            "tools": [{
                "type": "function",
                "name": "get_weather",
                "description": "Get weather for a city",
                "parameters": { "type": "object", "properties": { "city": { "type": "string" } } }
            }],
            "tool_choice": { "type": "function", "name": "get_weather" },
            "reasoning": { "effort": "medium" },
            "max_output_tokens": 1000,
            "stream": true,
            "temperature": 0.7
        });

        let expected = json!({
            "model": "gpt-4o",
            "messages": [
                { "role": "system", "content": "You are a helpful assistant." },
                { "role": "user", "content": "Hello" }
            ],
            "tools": [{ "type": "function", "function": { "name": "get_weather", "description": "Get weather for a city", "parameters": { "type": "object", "properties": { "city": { "type": "string" } } } } }],
            "tool_choice": { "type": "function", "function": { "name": "get_weather" } },
            "reasoning_effort": "medium",
            "max_tokens": 1000,
            "stream": true,
            "temperature": 0.7
        });

        let (result, _mappings) = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected);
    }

    // ============================================
    //  Input Parsing
    // ============================================

    #[test]
    fn test_input_parsing() {
        // String input
        let input = json!({ "model": "gpt-4o", "input": "Hello, how are you?" });
        let expected = json!({
            "model": "gpt-4o",
            "messages": [{ "role": "user", "content": "Hello, how are you?" }]
        });
        let (result, _mappings) = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected, "string input");

        // Mixed input: string + user message → merged
        let input = json!({ "model": "gpt-4o", "input": ["Hello!", { "role": "user", "content": "How are you?" }] });
        let expected = json!({
            "model": "gpt-4o",
            "messages": [{
                "role": "user",
                "content": [
                    { "type": "text", "text": "Hello!" },
                    { "type": "text", "text": "How are you?" }
                ]
            }]
        });
        let (result, _mappings) = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected, "mixed string + user message");

        // Only instructions, no input
        let input = json!({ "model": "gpt-4o", "instructions": "Be helpful." });
        let expected = json!({
            "model": "gpt-4o",
            "messages": [{ "role": "system", "content": "Be helpful." }]
        });
        let (result, _mappings) = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected, "instructions only");
    }

    // ============================================
    //  Role Mapping
    // ============================================

    #[test]
    fn test_role_mapping() {
        // system
        let input = json!({
            "model": "gpt-4o",
            "input": [{
                "role": "system",
                "content": [{ "type": "input_text", "text": "You are helpful." }]
            }]
        });
        let expected = json!({
            "model": "gpt-4o",
            "messages": [{ "role": "system", "content": "You are helpful." }]
        });
        let (result, _mappings) = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected, "system role");

        // user
        let input = json!({
            "model": "gpt-4o",
            "input": [{
                "role": "user",
                "content": [{ "type": "input_text", "text": "Hello" }]
            }]
        });
        let expected =
            json!({ "model": "gpt-4o", "messages": [{ "role": "user", "content": "Hello" }] });
        let (result, _mappings) = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected, "user role");

        // assistant
        let input = json!({
            "model": "gpt-4o",
            "input": [{
                "role": "assistant",
                "content": [{ "type": "output_text", "text": "Sure!" }]
            }]
        });
        let expected =
            json!({ "model": "gpt-4o", "messages": [{ "role": "assistant", "content": "Sure!" }] });
        let (result, _mappings) = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected, "assistant role");

        // middle developer → user (merged with preceding user)
        let input = json!({
            "model": "gpt-4o",
            "input": [
                { "role": "user", "content": "Hello" },
                {
                    "role": "developer",
                    "content": [{ "type": "input_text", "text": "You are Codex." }]
                }
            ]
        });
        let expected = json!({
            "model": "gpt-4o",
            "messages": [{
                "role": "user",
                "content": [
                    { "type": "text", "text": "Hello" },
                    { "type": "text", "text": "You are Codex." }
                ]
            }]
        });
        let (result, _mappings) = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected, "middle developer → user");
    }

    #[test]
    fn test_developer_role_handling() {
        // Multiple leading developers → system
        let input = json!({ "model": "gpt-4o", "input": [{ "role": "developer", "content": [{ "type": "input_text", "text": "You are a coding assistant." }] }, { "role": "developer", "content": [{ "type": "input_text", "text": "Be concise." }] }, { "role": "user", "content": "Hello" }] });
        let expected = json!({ "model": "gpt-4o", "messages": [{ "role": "system", "content": "You are a coding assistant.\n\nBe concise." }, { "role": "user", "content": "Hello" }] });
        let (result, _mappings) = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected, "multiple leading developers");

        // Single leading developer → system
        let input = json!({ "model": "gpt-4o", "input": [{ "role": "developer", "content": [{ "type": "input_text", "text": "You are helpful." }] }, { "role": "user", "content": "Hello" }] });
        let expected = json!({ "model": "gpt-4o", "messages": [{ "role": "system", "content": "You are helpful." }, { "role": "user", "content": "Hello" }] });
        let (result, _mappings) = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected, "single leading developer");

        // Mixed leading + middle developer
        let input = json!({ "model": "gpt-4o", "input": [{ "role": "developer", "content": [{ "type": "input_text", "text": "System instruction 1." }] }, { "role": "developer", "content": [{ "type": "input_text", "text": "System instruction 2." }] }, { "role": "user", "content": "Hello" }, { "role": "developer", "content": [{ "type": "input_text", "text": "Middle developer text." }] }, { "role": "user", "content": "Continue" }] });
        let expected = json!({ "model": "gpt-4o", "messages": [{ "role": "system", "content": "System instruction 1.\n\nSystem instruction 2." }, { "role": "user", "content": [{ "type": "text", "text": "Hello" }, { "type": "text", "text": "Middle developer text." }, { "type": "text", "text": "Continue" }] }] });
        let (result, _mappings) = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected, "mixed leading + middle");
    }

    // ============================================
    //  Instructions
    // ============================================

    #[test]
    fn test_instructions_handling() {
        // String instructions
        let input = json!({ "model": "gpt-4o", "input": "Hello", "instructions": "You are a helpful assistant." });
        let expected = json!({ "model": "gpt-4o", "messages": [{ "role": "system", "content": "You are a helpful assistant." }, { "role": "user", "content": "Hello" }] });
        let (result, _mappings) = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected, "instructions as string");

        // Array instructions
        let input = json!({ "model": "gpt-4o", "input": "Hello", "instructions": [{ "type": "input_text", "text": "You are a coding assistant." }] });
        let expected = json!({ "model": "gpt-4o", "messages": [{ "role": "system", "content": "You are a coding assistant." }, { "role": "user", "content": "Hello" }] });
        let (result, _mappings) = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected, "instructions as array");

        // Instructions + leading developer merged
        let input = json!({ "model": "gpt-4o", "input": [{ "role": "developer", "content": [{ "type": "input_text", "text": "Be concise." }] }, { "role": "user", "content": "Hello" }], "instructions": "You are a coding assistant." });
        let expected = json!({ "model": "gpt-4o", "messages": [{ "role": "system", "content": "You are a coding assistant.\n\nBe concise." }, { "role": "user", "content": "Hello" }] });
        let (result, _mappings) = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected, "instructions + developer");

        // Instructions + multiple leading developers
        let input = json!({ "model": "gpt-4o", "input": [{ "role": "developer", "content": [{ "type": "input_text", "text": "Be concise." }] }, { "role": "developer", "content": [{ "type": "input_text", "text": "Use simple language." }] }, { "role": "user", "content": "Hello" }], "instructions": "You are a coding assistant." });
        let expected = json!({ "model": "gpt-4o", "messages": [{ "role": "system", "content": "You are a coding assistant.\n\nBe concise.\n\nUse simple language." }, { "role": "user", "content": "Hello" }] });
        let (result, _mappings) = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected, "instructions + multiple developers");

        // Only instructions (no input)
        let input = json!({ "model": "gpt-4o", "instructions": "Just instructions." });
        let expected = json!({ "model": "gpt-4o", "messages": [{ "role": "system", "content": "Just instructions." }] });
        let (result, _mappings) = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected, "instructions only");
    }

    // ============================================
    //  Consecutive Message Merging
    // ============================================

    #[test]
    fn test_consecutive_user_messages_merged() {
        let input = json!({ "model": "gpt-4o", "input": [{ "role": "system", "content": [{ "type": "input_text", "text": "You are helpful." }] }, { "role": "user", "content": [{ "type": "input_text", "text": "First message." }] }, { "role": "user", "content": [{ "type": "input_text", "text": "Second message." }] }] });
        let expected = json!({ "model": "gpt-4o", "messages": [{ "role": "system", "content": "You are helpful." }, { "role": "user", "content": [{ "type": "text", "text": "First message." }, { "type": "text", "text": "Second message." }] }] });
        let (result, _mappings) = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected);
    }

    // ============================================
    //  Content Block Conversion
    // ============================================

    #[test]
    fn test_content_blocks() {
        // User with text + image
        let input = json!({ "model": "gpt-4o", "input": [{ "role": "user", "content": [{ "type": "input_text", "text": "Text content" }, { "type": "input_image", "image_url": "https://example.com/img.png" }] }] });
        let expected = json!({ "model": "gpt-4o", "messages": [{ "role": "user", "content": [{ "type": "text", "text": "Text content" }, { "type": "image_url", "image_url": { "url": "https://example.com/img.png" } }] }] });
        let (result, _mappings) = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected, "user with text + image");

        // Assistant with text + image
        let input = json!({ "model": "gpt-4o", "input": [{ "type": "message", "role": "assistant", "content": [{ "type": "output_text", "text": "Here is the image:" }, { "type": "input_image", "image_url": "https://example.com/result.png" }] }] });
        let expected = json!({ "model": "gpt-4o", "messages": [{ "role": "assistant", "content": [{ "type": "text", "text": "Here is the image:" }, { "type": "image_url", "image_url": { "url": "https://example.com/result.png" } }] }] });
        let (result, _mappings) = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected, "assistant with text + image");

        // Image with base64 data
        let input = json!({ "model": "gpt-4o", "input": [{ "role": "user", "content": [{ "type": "input_image", "image_url": "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==" }] }] });
        let expected = json!({ "model": "gpt-4o", "messages": [{ "role": "user", "content": [{ "type": "image_url", "image_url": { "url": "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==" } }] }] });
        let (result, _mappings) = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected, "base64 image");

        // Image with detail
        let input = json!({ "model": "gpt-4o", "input": [{ "role": "user", "content": [{ "type": "input_image", "image_url": "https://example.com/photo.jpg", "detail": "high" }] }] });
        let expected = json!({ "model": "gpt-4o", "messages": [{ "role": "user", "content": [{ "type": "image_url", "image_url": { "url": "https://example.com/photo.jpg", "detail": "high" } }] }] });
        let (result, _mappings) = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected, "image with detail");

        // Multimodal text + image
        let input = json!({ "model": "gpt-4o", "input": [{ "role": "user", "content": [{ "type": "input_text", "text": "Look at this image:" }, { "type": "input_image", "image_url": "https://example.com/photo.jpg" }] }] });
        let expected = json!({ "model": "gpt-4o", "messages": [{ "role": "user", "content": [{ "type": "text", "text": "Look at this image:" }, { "type": "image_url", "image_url": { "url": "https://example.com/photo.jpg" } }] }] });
        let (result, _mappings) = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected, "multimodal text + image");
    }

    #[test]
    fn test_developer_content_filtering() {
        let input = json!({ "model": "gpt-4o", "input": [{ "role": "developer", "content": [{ "type": "input_text", "text": "You are a helpful assistant." }, { "type": "input_image", "image_url": "https://example.com/img.png" }] }] });
        let expected = json!({ "model": "gpt-4o", "messages": [{ "role": "system", "content": "You are a helpful assistant." }] });
        let (result, _mappings) = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected);
    }

    // ============================================
    //  Tool Calls
    // ============================================

    #[test]
    fn test_tool_calls() {
        // Single function_call
        let input = json!({ "model": "gpt-4o", "input": [{ "type": "function_call", "call_id": "call_abc", "name": "get_weather", "arguments": "{\"city\": \"Tokyo\"}" }] });
        let expected = json!({ "model": "gpt-4o", "messages": [{ "role": "assistant", "content": null, "tool_calls": [{ "id": "call_abc", "type": "function", "function": { "name": "get_weather", "arguments": "{\"city\": \"Tokyo\"}" } }] }] });
        let (result, _mappings) = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected, "single function_call");

        // Single function_call_output
        let input = json!({ "model": "gpt-4o", "input": [{ "type": "function_call_output", "call_id": "call_abc123", "output": "{\"temperature\": 22}" }] });
        let expected = json!({ "model": "gpt-4o", "messages": [{ "role": "tool", "tool_call_id": "call_abc123", "content": "{\"temperature\": 22}" }] });
        let (result, _mappings) = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected, "single function_call_output");

        // Full flow: function_call → output → assistant
        let input = json!({ "model": "gpt-4o", "input": [{ "type": "function_call", "call_id": "call_1", "name": "get_weather", "arguments": "{\"city\": \"Tokyo\"}" }, { "type": "function_call_output", "call_id": "call_1", "output": "{\"temperature\": 22}" }, { "type": "message", "role": "assistant", "content": [{ "type": "output_text", "text": "The weather in Tokyo is 22°C." }] }] });
        let expected = json!({ "model": "gpt-4o", "messages": [{ "role": "assistant", "content": null, "tool_calls": [{ "id": "call_1", "type": "function", "function": { "name": "get_weather", "arguments": "{\"city\": \"Tokyo\"}" } }] }, { "role": "tool", "tool_call_id": "call_1", "content": "{\"temperature\": 22}" }, { "role": "assistant", "content": "The weather in Tokyo is 22°C." }] });
        let (result, _mappings) = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected, "full call→output→response");
    }

    #[test]
    fn test_multiple_consecutive_function_calls_merged() {
        let input = json!({ "model": "gpt-4o", "input": [{ "type": "function_call", "call_id": "call_1", "name": "exec_command", "arguments": "{\"cmd\": \"ls -la\"}" }, { "type": "function_call", "call_id": "call_2", "name": "exec_command", "arguments": "{\"cmd\": \"pwd\"}" }] });
        let expected = json!({ "model": "gpt-4o", "messages": [{ "role": "assistant", "content": null, "tool_calls": [{ "id": "call_1", "type": "function", "function": { "name": "exec_command", "arguments": "{\"cmd\": \"ls -la\"}" } }, { "id": "call_2", "type": "function", "function": { "name": "exec_command", "arguments": "{\"cmd\": \"pwd\"}" } }] }] });
        let (result, _mappings) = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_function_call_not_merged_across_assistant_text() {
        // Two separate function_call flows separated by an assistant text message.
        // The second function_call is merged into the preceding assistant text.
        let input = json!({ "model": "gpt-4o", "input": [{ "type": "function_call", "call_id": "call_1", "name": "exec_command", "arguments": "{\"cmd\": \"ls\"}" }, { "type": "function_call_output", "call_id": "call_1", "output": "file1.txt" }, { "type": "message", "role": "assistant", "content": [{ "type": "output_text", "text": "Result shown." }] }, { "type": "function_call", "call_id": "call_2", "name": "exec_command", "arguments": "{\"cmd\": \"pwd\"}" }, { "type": "function_call_output", "call_id": "call_2", "output": "/home" }] });
        let expected = json!({ "model": "gpt-4o", "messages": [{ "role": "assistant", "content": null, "tool_calls": [{ "id": "call_1", "type": "function", "function": { "name": "exec_command", "arguments": "{\"cmd\": \"ls\"}" } }] }, { "role": "tool", "tool_call_id": "call_1", "content": "file1.txt" }, { "role": "assistant", "content": "Result shown.", "tool_calls": [{ "id": "call_2", "type": "function", "function": { "name": "exec_command", "arguments": "{\"cmd\": \"pwd\"}" } }] }, { "role": "tool", "tool_call_id": "call_2", "content": "/home" }] });
        let (result, _mappings) = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected);
    }

    // ============================================
    //  Tool Configuration
    // ============================================

    #[test]
    fn test_tools_configuration() {
        // Basic tools format
        let input = json!({ "model": "gpt-4o", "input": "test", "tools": [{ "type": "function", "name": "get_weather", "description": "Get weather", "parameters": { "type": "object", "properties": {} } }] });
        let expected = json!({ "model": "gpt-4o", "messages": [{ "role": "user", "content": "test" }], "tools": [{ "type": "function", "function": { "name": "get_weather", "description": "Get weather", "parameters": { "type": "object", "properties": {} } } }] });
        let (result, _mappings) = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected, "tools format");

        // Non-function tools filtered
        let input = json!({ "model": "gpt-4o", "input": "test", "tools": [{ "type": "function", "name": "get_weather", "description": "Get weather", "parameters": { "type": "object", "properties": {} } }, { "type": "web_search", "external_web_access": false }] });
        let expected = json!({ "model": "gpt-4o", "messages": [{ "role": "user", "content": "test" }], "tools": [{ "type": "function", "function": { "name": "get_weather", "description": "Get weather", "parameters": { "type": "object", "properties": {} } } }] });
        let (result, _mappings) = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected, "non-function filtered");

        // tool_choice function object
        let input = json!({ "model": "gpt-4o", "input": "test", "tool_choice": { "type": "function", "name": "get_weather" } });
        let expected = json!({ "model": "gpt-4o", "messages": [{ "role": "user", "content": "test" }], "tool_choice": { "type": "function", "function": { "name": "get_weather" } } });
        let (result, _mappings) = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected, "tool_choice function");

        // tool_choice auto/none strings
        for choice in ["auto", "none"] {
            let input = json!({ "model": "gpt-4o", "input": "test", "tool_choice": choice });
            let expected = json!({ "model": "gpt-4o", "messages": [{ "role": "user", "content": "test" }], "tool_choice": choice });
            let (result, _mappings) = responses_to_chat_request(input).unwrap();
            assert_eq!(result, expected, "tool_choice: {choice}");
        }
    }

    // ============================================
    //  Namespace Tools
    // ============================================

    #[test]
    fn test_namespace_tools() {
        // Appended after regular function tools
        let input = json!({ "model": "gpt-4o", "input": "test", "tools": [{ "type": "function", "name": "get_weather", "description": "Get weather", "parameters": { "type": "object", "properties": {} } }, { "type": "namespace", "name": "mcp__weather__", "tools": [{ "type": "function", "name": "get_forecast", "parameters": { "type": "object", "properties": { "city": { "type": "string" } } } }] }] });
        let (result, mappings) = responses_to_chat_request(input).unwrap();
        assert_eq!(
            result["tools"].as_array().unwrap().len(),
            2,
            "namespace appended"
        );
        assert_eq!(
            result["tools"][1]["function"]["name"],
            "mcp__weather__get_forecast"
        );
        assert_eq!(
            mappings.split_name("mcp__weather__get_forecast"),
            Some(("mcp__weather__".into(), "get_forecast".into()))
        );

        // Only namespace tools
        let input = json!({ "model": "gpt-4o", "input": "test", "tools": [{ "type": "namespace", "name": "mcp__fs__", "tools": [{ "type": "function", "name": "read_file", "parameters": { "type": "object", "properties": { "path": { "type": "string" } } } }] }] });
        let (result, mappings) = responses_to_chat_request(input).unwrap();
        assert_eq!(
            result["tools"].as_array().unwrap().len(),
            1,
            "only namespace"
        );
        assert_eq!(result["tools"][0]["function"]["name"], "mcp__fs__read_file");
        assert_eq!(
            mappings.split_name("mcp__fs__read_file"),
            Some(("mcp__fs__".into(), "read_file".into()))
        );

        // With strict field
        let input = json!({ "model": "gpt-4o", "input": "test", "tools": [{ "type": "namespace", "name": "mcp__calc__", "tools": [{ "type": "function", "name": "add", "strict": true, "parameters": { "type": "object", "properties": {} } }] }] });
        let (result, _mappings) = responses_to_chat_request(input).unwrap();
        assert_eq!(
            result["tools"][0]["function"]["strict"], true,
            "strict preserved"
        );

        // Empty namespace skipped
        let input = json!({ "model": "gpt-4o", "input": "test", "tools": [{ "type": "namespace", "name": "mcp__empty__", "tools": [] }, { "type": "function", "name": "ping", "parameters": { "type": "object", "properties": {} } }] });
        let (result, _mappings) = responses_to_chat_request(input).unwrap();
        assert_eq!(
            result["tools"].as_array().unwrap().len(),
            1,
            "empty namespace"
        );

        // Non-function in namespace skipped
        let input = json!({ "model": "gpt-4o", "input": "test", "tools": [{ "type": "namespace", "name": "mcp__mixed__", "tools": [{ "type": "function", "name": "valid_tool", "parameters": { "type": "object", "properties": {} } }, { "type": "web_search", "name": "search" }] }] });
        let (result, _mappings) = responses_to_chat_request(input).unwrap();
        assert_eq!(
            result["tools"].as_array().unwrap().len(),
            1,
            "mixed namespace"
        );
        assert_eq!(
            result["tools"][0]["function"]["name"],
            "mcp__mixed__valid_tool"
        );
    }

    // ============================================
    //  Field Mapping
    // ============================================

    #[test]
    fn test_field_mapping() {
        // max_output_tokens → max_tokens
        let input = json!({ "model": "gpt-4o", "input": "test", "max_output_tokens": 1000 });
        let expected = json!({ "model": "gpt-4o", "messages": [{ "role": "user", "content": "test" }], "max_tokens": 1000 });
        let (result, _mappings) = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected, "max_output_tokens → max_tokens");

        // reasoning.effort → reasoning_effort
        let input =
            json!({ "model": "gpt-4o", "input": "test", "reasoning": { "effort": "medium" } });
        let expected = json!({ "model": "gpt-4o", "messages": [{ "role": "user", "content": "test" }], "reasoning_effort": "medium" });
        let (result, _mappings) = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected, "reasoning.effort");

        // Passthrough fields
        let input = json!({ "model": "gpt-4o", "input": "test", "stream": true, "temperature": 0.7, "top_p": 0.9, "parallel_tool_calls": true });
        let expected = json!({ "model": "gpt-4o", "messages": [{ "role": "user", "content": "test" }], "stream": true, "temperature": 0.7, "top_p": 0.9, "parallel_tool_calls": true });
        let (result, _mappings) = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected, "passthrough");
    }

    #[test]
    fn test_stream_options() {
        // include_obfuscation + useful field → filtered
        let input = json!({ "model": "gpt-4o", "input": "test", "stream_options": { "include_obfuscation": true, "foo": "bar" } });
        let expected = json!({ "model": "gpt-4o", "messages": [{ "role": "user", "content": "test" }], "stream_options": { "foo": "bar" } });
        let (result, _mappings) = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected, "obfuscation filtered");

        // Only include_obfuscation → removed
        let input = json!({ "model": "gpt-4o", "input": "test", "stream_options": { "include_obfuscation": true } });
        let (result, _mappings) = responses_to_chat_request(input).unwrap();
        assert!(
            !result.as_object().unwrap().contains_key("stream_options"),
            "empty stream_options removed"
        );

        // Useful fields preserved
        let input = json!({ "model": "gpt-4o", "input": "test", "stream_options": { "include_usage": true } });
        let expected = json!({ "model": "gpt-4o", "messages": [{ "role": "user", "content": "test" }], "stream_options": { "include_usage": true } });
        let (result, _mappings) = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected, "include_usage");
    }

    // ============================================
    //  Reasoning Content
    // ============================================

    #[test]
    fn test_reasoning() {
        // Basic reasoning → reasoning_content
        let input = json!({ "model": "gpt-4o", "input": [{ "type": "reasoning", "summary": [{ "type": "summary_text", "text": "Thought about the problem." }], "content": [{ "type": "reasoning_text", "text": "Let me think..." }] }] });
        let expected = json!({ "model": "gpt-4o", "messages": [{ "role": "assistant", "content": "", "reasoning_content": "Thought about the problem.\n\nLet me think..." }] });
        let (result, _mappings) = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected, "basic reasoning");

        // Reasoning + content + tool_calls merged
        let input = json!({ "model": "gpt-4o", "input": [{ "type": "reasoning", "summary": [{ "type": "summary_text", "text": "Analyzed." }], "content": [{ "type": "reasoning_text", "text": "Step by step." }] }, { "type": "message", "role": "assistant", "content": [{ "type": "output_text", "text": "Solution." }] }, { "type": "function_call", "call_id": "c1", "name": "exec", "arguments": "{}" }] });
        let expected = json!({ "model": "gpt-4o", "messages": [{ "role": "assistant", "content": "Solution.", "reasoning_content": "Analyzed.\n\nStep by step.", "tool_calls": [{ "id": "c1", "type": "function", "function": { "name": "exec", "arguments": "{}" } }] }] });
        let (result, _mappings) = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected, "reasoning + content + tool_calls");
    }

    #[test]
    fn test_reasoning_followed_by_user() {
        let input = json!({ "model": "gpt-4o", "input": [{ "type": "reasoning", "summary": [{ "type": "summary_text", "text": "Thinking step by step." }], "content": null }, { "type": "message", "role": "user", "content": [{ "type": "input_text", "text": "continue" }] }] });
        let expected = json!({ "model": "gpt-4o", "messages": [{ "role": "assistant", "content": "", "reasoning_content": "Thinking step by step." }, { "role": "user", "content": "continue" }] });
        let (result, _mappings) = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected);
    }

    // ============================================
    //  Skipped / Converted Content
    // ============================================

    #[test]
    fn test_skipped_and_converted_content() {
        // input_file skipped
        let input = json!({ "model": "gpt-4o", "input": [{ "role": "user", "content": [{ "type": "input_text", "text": "Read this file:" }, { "type": "input_file", "file_id": "f123" }] }] });
        let expected = json!({ "model": "gpt-4o", "messages": [{ "role": "user", "content": "Read this file:" }] });
        let (result, _mappings) = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected, "input_file skipped");

        // refusal → text
        let input = json!({ "model": "gpt-4o", "input": [{ "type": "message", "role": "assistant", "content": [{ "type": "refusal", "text": "I cannot answer that." }] }] });
        let expected = json!({ "model": "gpt-4o", "messages": [{ "role": "assistant", "content": "I cannot answer that." }] });
        let (result, _mappings) = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected, "refusal → text");
    }

    #[test]
    fn test_text_format_and_verbosity() {
        // text.verbosity → verbosity
        let input = json!({ "model": "gpt-4o", "input": "test", "text": { "verbosity": "high" } });
        let expected = json!({ "model": "gpt-4o", "messages": [{ "role": "user", "content": "test" }], "verbosity": "high" });
        let (result, _mappings) = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected, "text.verbosity");

        // text.format dropped
        let input =
            json!({ "model": "gpt-4o", "input": "test", "text": { "format": { "type": "text" } } });
        let expected =
            json!({ "model": "gpt-4o", "messages": [{ "role": "user", "content": "test" }] });
        let (result, _mappings) = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected, "text.format dropped");
    }

    // ============================================
    //  Complex Orchestration
    // ============================================

    #[test]
    fn test_developer_between_tool_call_and_result() {
        let input = json!({ "model": "gpt-4o", "input": [{ "type": "function_call", "call_id": "call_1", "name": "exec_command", "arguments": "{\"cmd\": \"ls\"}" }, { "type": "message", "role": "developer", "content": [{ "type": "input_text", "text": "Approved." }] }, { "type": "function_call_output", "call_id": "call_1", "output": "file1.txt" }] });
        let expected = json!({ "model": "gpt-4o", "messages": [{ "role": "assistant", "content": null, "tool_calls": [{ "id": "call_1", "type": "function", "function": { "name": "exec_command", "arguments": "{\"cmd\": \"ls\"}" } }] }, { "role": "tool", "tool_call_id": "call_1", "content": "file1.txt" }, { "role": "user", "content": "Approved." }] });
        let (result, _mappings) = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_multiple_tool_calls_with_interleaved_developer() {
        let input = json!({ "model": "gpt-4o", "input": [{ "type": "function_call", "call_id": "call_1", "name": "exec_command", "arguments": "{\"cmd\": \"ls\"}" }, { "type": "function_call", "call_id": "call_2", "name": "exec_command", "arguments": "{\"cmd\": \"pwd\"}" }, { "type": "message", "role": "developer", "content": [{ "type": "input_text", "text": "Both approved." }] }, { "type": "function_call_output", "call_id": "call_1", "output": "file1.txt" }, { "type": "function_call_output", "call_id": "call_2", "output": "/home" }, { "type": "message", "role": "user", "content": [{ "type": "input_text", "text": "Continue" }] }] });
        let expected = json!({ "model": "gpt-4o", "messages": [{ "role": "assistant", "content": null, "tool_calls": [{ "id": "call_1", "type": "function", "function": { "name": "exec_command", "arguments": "{\"cmd\": \"ls\"}" } }, { "id": "call_2", "type": "function", "function": { "name": "exec_command", "arguments": "{\"cmd\": \"pwd\"}" } }] }, { "role": "tool", "tool_call_id": "call_1", "content": "file1.txt" }, { "role": "tool", "tool_call_id": "call_2", "content": "/home" }, { "role": "user", "content": [{ "type": "text", "text": "Both approved." }, { "type": "text", "text": "Continue" }] }] });
        let (result, _mappings) = responses_to_chat_request(input).unwrap();
        assert_eq!(result, expected);
    }
}
