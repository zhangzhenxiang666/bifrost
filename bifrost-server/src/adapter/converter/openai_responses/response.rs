//! Transform Chat API responses to Responses API format
//!
//! This module provides functions to convert Chat Completions API response format
//! to OpenAI Responses API compatible format (non-streaming).

use crate::adapter::converter::openai_responses::NamespaceMappings;
use crate::error::LlmMapError;
use serde_json::{Value, json};

/// Convert a Chat API response to Responses API format.
///
/// This function transforms a Chat Completions API response (non-streaming)
/// into the Responses API `Response` object format.
///
/// # Arguments
///
/// * `body` - The JSON response body in Chat API format
///
/// # Returns
///
/// A `Result` containing the transformed response in Responses API format,
/// or an `LlmMapError` if the transformation fails.
pub fn chat_to_responses_response(
    body: Value,
    namespace_mappings: &NamespaceMappings,
) -> Result<Value, LlmMapError> {
    let Value::Object(mut obj) = body else {
        return Err(LlmMapError::Validation(
            "Response body must be an object".into(),
        ));
    };

    let id = obj.remove("id").unwrap_or(Value::String(String::new()));
    let created = obj.remove("created").and_then(|v| v.as_u64()).unwrap_or(0);
    let model = obj.remove("model").unwrap_or(Value::String(String::new()));
    let service_tier = obj.remove("service_tier");
    let system_fingerprint = obj.remove("system_fingerprint");
    let usage = obj.remove("usage");

    let choices = obj
        .remove("choices")
        .and_then(|v| match v {
            Value::Array(arr) => Some(arr),
            _ => None,
        })
        .unwrap_or_default();

    let mut output: Vec<Value> = Vec::new();

    for choice in &choices {
        let Value::Object(choice_obj) = choice else {
            continue;
        };
        let message = choice_obj.get("message");
        let Some(Value::Object(msg_obj)) = message else {
            continue;
        };
        output.extend(build_output_items_from_message(
            msg_obj,
            namespace_mappings,
        )?);
    }

    let transformed_usage = transform_usage_to_responses_format(usage);

    let mut result = serde_json::Map::new();
    result.insert("id".to_string(), id);
    result.insert("created_at".to_string(), Value::Number(created.into()));
    result.insert("model".to_string(), model);
    result.insert("object".to_string(), Value::String("response".to_string()));
    result.insert(
        "status".to_string(),
        Value::String(determine_status_from_choices(&choices)),
    );
    result.insert("output".to_string(), Value::Array(output));
    result.insert("parallel_tool_calls".to_string(), Value::Bool(true));
    result.insert("tools".to_string(), Value::Array(Vec::new()));

    if let Some(usage_val) = transformed_usage {
        result.insert("usage".to_string(), usage_val);
    }

    if let Some(Value::String(ref s)) = service_tier
        && !s.is_empty()
    {
        result.insert("service_tier".to_string(), Value::String(s.clone()));
    }

    if let Some(Value::String(ref fp)) = system_fingerprint
        && !fp.is_empty()
    {
        result.insert("metadata".to_string(), json!({ "system_fingerprint": fp }));
    }

    Ok(Value::Object(result))
}

/// Build output items from a ChatCompletionMessage.
///
/// A single message may produce multiple output items:
/// - A `message` item (for content/refusal)
/// - `function_call` items (for each tool_call)
fn build_output_items_from_message(
    msg_obj: &serde_json::Map<String, Value>,
    namespace_mappings: &NamespaceMappings,
) -> Result<Vec<Value>, LlmMapError> {
    let mut items: Vec<Value> = Vec::new();

    if let Some(reasoning_text) = msg_obj.get("reasoning_content").and_then(|v| v.as_str())
        && !reasoning_text.is_empty()
    {
        let mut reasoning_item = serde_json::Map::new();
        reasoning_item.insert(
            "id".to_string(),
            Value::String(super::create_item_id("rsn")),
        );
        reasoning_item.insert("type".to_string(), Value::String("reasoning".to_string()));
        reasoning_item.insert(
            "summary".to_string(),
            json!([{ "type": "summary_text", "text": reasoning_text }]),
        );
        reasoning_item.insert("status".to_string(), Value::String("completed".to_string()));
        items.push(Value::Object(reasoning_item));
    }

    let content = msg_obj.get("content");
    let refusal = msg_obj.get("refusal").and_then(|v| v.as_str());
    let mut content_array: Vec<Value> = Vec::new();

    if let Some(text) = content.and_then(|v| v.as_str())
        && !text.is_empty()
    {
        let mut text_obj = serde_json::Map::new();
        text_obj.insert("type".to_string(), Value::String("output_text".to_string()));
        text_obj.insert("text".to_string(), Value::String(text.to_string()));
        text_obj.insert("annotations".to_string(), Value::Array(Vec::new()));
        text_obj.insert("logprobs".to_string(), Value::Null);
        content_array.push(Value::Object(text_obj));
    }

    if let Some(Value::Array(arr)) = content {
        for block in arr {
            if let Some(part) = transform_content_block_to_output_text(block)? {
                content_array.push(part);
            }
        }
    }

    if let Some(refusal_text) = refusal
        && !refusal_text.is_empty()
    {
        let mut refusal_obj = serde_json::Map::new();
        refusal_obj.insert("type".to_string(), Value::String("refusal".to_string()));
        refusal_obj.insert(
            "refusal".to_string(),
            Value::String(refusal_text.to_string()),
        );
        content_array.push(Value::Object(refusal_obj));
    }

    if let Some(Value::Array(annotations)) = msg_obj.get("annotations")
        && let Some(first_text) = content_array
            .iter_mut()
            .find(|v| v.get("type").and_then(|t| t.as_str()) == Some("output_text"))
    {
        let transformed_annots = transform_annotations(annotations);
        if let Value::Object(text_obj) = first_text {
            text_obj.insert("annotations".to_string(), transformed_annots);
        }
    }

    if !content_array.is_empty() {
        let mut msg_item = serde_json::Map::new();
        msg_item.insert(
            "id".to_string(),
            Value::String(super::create_item_id("msg")),
        );
        msg_item.insert("type".to_string(), Value::String("message".to_string()));
        msg_item.insert("role".to_string(), Value::String("assistant".to_string()));
        msg_item.insert("status".to_string(), Value::String("completed".to_string()));
        msg_item.insert("content".to_string(), Value::Array(content_array));
        items.push(Value::Object(msg_item));
    }

    if let Some(Value::Array(tool_calls)) = msg_obj.get("tool_calls") {
        for tc in tool_calls {
            if let Some(fc_item) = transform_tool_call_to_function_call(tc, namespace_mappings)? {
                items.push(fc_item);
            }
        }
    }

    Ok(items)
}

/// Transform a single content block to ResponseOutputText.
fn transform_content_block_to_output_text(block: &Value) -> Result<Option<Value>, LlmMapError> {
    let Value::Object(block_obj) = block else {
        return Ok(None);
    };

    let block_type = block_obj.get("type").and_then(|v| v.as_str());

    match block_type {
        Some("text") => {
            let text = block_obj.get("text").and_then(|v| v.as_str());
            if let Some(t) = text {
                let mut text_obj = serde_json::Map::new();
                text_obj.insert("type".to_string(), Value::String("output_text".to_string()));
                text_obj.insert("text".to_string(), Value::String(t.to_string()));
                text_obj.insert("annotations".to_string(), Value::Array(Vec::new()));
                text_obj.insert("logprobs".to_string(), Value::Null);
                return Ok(Some(Value::Object(text_obj)));
            }
        }
        Some("image_url") => {
            // Image content - convert to output_text with image reference
            if let Some(image_url_obj) = block_obj.get("image_url").and_then(|v| v.as_object()) {
                let url = image_url_obj.get("url").and_then(|v| v.as_str());
                if let Some(u) = url {
                    let mut text_obj = serde_json::Map::new();
                    text_obj.insert("type".to_string(), Value::String("output_text".to_string()));
                    text_obj.insert("text".to_string(), Value::String(format!("![image]({u})")));
                    text_obj.insert("annotations".to_string(), Value::Array(Vec::new()));
                    text_obj.insert("logprobs".to_string(), Value::Null);
                    return Ok(Some(Value::Object(text_obj)));
                }
            }
        }
        _ => {}
    }

    Ok(None)
}

/// Transform message-level annotations to Responses API annotation format.
fn transform_annotations(annotations: &[Value]) -> Value {
    let mut result: Vec<Value> = Vec::new();

    for annot in annotations {
        let Value::Object(annot_obj) = annot else {
            continue;
        };

        let annot_type = annot_obj.get("type").and_then(|v| v.as_str());

        if annot_type == Some("url_citation") {
            // Chat API: { type: "url_citation", url_citation: { start_index, end_index, url, title } }
            // Responses: { type: "url_citation", start_index, end_index, url, title }
            if let Some(url_citation) = annot_obj.get("url_citation").and_then(|v| v.as_object()) {
                let mut transformed = serde_json::Map::new();
                transformed.insert(
                    "type".to_string(),
                    Value::String("url_citation".to_string()),
                );

                if let Some(start) = url_citation.get("start_index") {
                    transformed.insert("start_index".to_string(), start.clone());
                }
                if let Some(end) = url_citation.get("end_index") {
                    transformed.insert("end_index".to_string(), end.clone());
                }
                if let Some(url) = url_citation.get("url") {
                    transformed.insert("url".to_string(), url.clone());
                }
                if let Some(title) = url_citation.get("title") {
                    transformed.insert("title".to_string(), title.clone());
                }

                result.push(Value::Object(transformed));
            }
        }
    }

    Value::Array(result)
}

/// Transform a tool_call to a ResponseFunctionToolCall output item.
///
/// Chat API: { id: "call_xxx", type: "function", function: { name: "...", arguments: "..." } }
/// Responses: { id: "fc_xxx", type: "function_call", call_id: "call_xxx", name: "...", arguments: "..." }
fn transform_tool_call_to_function_call(
    tc: &Value,
    namespace_mappings: &NamespaceMappings,
) -> Result<Option<Value>, LlmMapError> {
    let Value::Object(tc_obj) = tc else {
        return Ok(None);
    };

    let call_id = tc_obj.get("id").and_then(|v| v.as_str()).unwrap_or("");
    let tc_type = tc_obj.get("type").and_then(|v| v.as_str());

    if tc_type != Some("function") {
        return Ok(None);
    }

    let function_obj = tc_obj.get("function").and_then(|v| v.as_object());
    let Some(func) = function_obj else {
        return Ok(None);
    };

    let full_name = func.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let arguments = func.get("arguments").and_then(|v| v.as_str()).unwrap_or("");

    // Check if the tool call name has a namespace prefix; if so, split it.
    let (final_name, namespace) =
        if let Some((ns, short_name)) = namespace_mappings.split_name(full_name) {
            (short_name, Some(ns))
        } else {
            (full_name.to_string(), None)
        };

    let mut fc_item = serde_json::Map::new();
    fc_item.insert("id".to_string(), Value::String(super::create_item_id("fc")));
    fc_item.insert(
        "type".to_string(),
        Value::String("function_call".to_string()),
    );
    fc_item.insert("call_id".to_string(), Value::String(call_id.to_string()));
    fc_item.insert("name".to_string(), Value::String(final_name));
    if let Some(ns) = namespace {
        fc_item.insert("namespace".to_string(), Value::String(ns));
    }
    fc_item.insert(
        "arguments".to_string(),
        Value::String(arguments.to_string()),
    );
    fc_item.insert("status".to_string(), Value::String("completed".to_string()));

    Ok(Some(Value::Object(fc_item)))
}

/// Transform usage from Chat API format to Responses API format.
///
/// Chat API: { prompt_tokens, completion_tokens, total_tokens, prompt_tokens_details, completion_tokens_details }
/// Responses: { input_tokens, output_tokens, total_tokens, input_tokens_details, output_tokens_details }
fn transform_usage_to_responses_format(usage: Option<Value>) -> Option<Value> {
    let Value::Object(usage_obj) = usage? else {
        return None;
    };

    let prompt_tokens = usage_obj
        .get("prompt_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let completion_tokens = usage_obj
        .get("completion_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let total_tokens = usage_obj
        .get("total_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(prompt_tokens + completion_tokens);

    // Transform prompt_tokens_details → input_tokens_details
    let input_tokens_details = usage_obj
        .get("prompt_tokens_details")
        .and_then(|v| v.as_object())
        .map(|details| {
            let mut result = serde_json::Map::new();
            if let Some(cached) = details.get("cached_tokens") {
                result.insert("cached_tokens".to_string(), cached.clone());
            } else {
                result.insert("cached_tokens".to_string(), Value::Number(0.into()));
            }
            Value::Object(result)
        })
        .unwrap_or_else(|| json!({ "cached_tokens": 0 }));

    // Transform completion_tokens_details → output_tokens_details
    let output_tokens_details = usage_obj
        .get("completion_tokens_details")
        .and_then(|v| v.as_object())
        .map(|details| {
            let mut result = serde_json::Map::new();
            if let Some(reasoning) = details.get("reasoning_tokens") {
                result.insert("reasoning_tokens".to_string(), reasoning.clone());
            } else {
                result.insert("reasoning_tokens".to_string(), Value::Number(0.into()));
            }
            Value::Object(result)
        })
        .unwrap_or_else(|| json!({ "reasoning_tokens": 0 }));

    Some(json!({
        "input_tokens": prompt_tokens,
        "input_tokens_details": input_tokens_details,
        "output_tokens": completion_tokens,
        "output_tokens_details": output_tokens_details,
        "total_tokens": total_tokens,
    }))
}

/// Determine response status from choices.
fn determine_status_from_choices(choices: &[Value]) -> String {
    if choices.is_empty() {
        return "incomplete".to_string();
    }

    for choice in choices {
        let Value::Object(choice_obj) = choice else {
            continue;
        };

        let finish_reason = choice_obj.get("finish_reason").and_then(|v| v.as_str());

        match finish_reason {
            Some("length") => return "incomplete".to_string(),
            Some("content_filter") => return "incomplete".to_string(),
            Some("stop") | Some("tool_calls") => return "completed".to_string(),
            _ => continue,
        }
    }

    "completed".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[expect(clippy::collapsible_if, clippy::collapsible_match)]
    fn strip_output_item_ids(value: &mut Value) {
        if let Value::Object(obj) = value {
            if let Some(output) = obj.get_mut("output") {
                if let Value::Array(items) = output {
                    for item in items {
                        if let Value::Object(item_obj) = item {
                            item_obj.remove("id");
                        }
                    }
                }
            }
        }
    }

    fn base_chat_response() -> serde_json::Map<String, Value> {
        json!({
            "id": "chatcmpl_abc123",
            "choices": [{
                "finish_reason": "stop",
                "index": 0,
                "message": {
                    "content": "Hello, world!",
                    "refusal": null,
                    "role": "assistant"
                }
            }],
            "created": 1712530587,
            "model": "gpt-4o",
            "object": "chat.completion",
            "usage": {
                "completion_tokens": 5,
                "prompt_tokens": 10,
                "total_tokens": 15
            }
        })
        .as_object()
        .unwrap()
        .clone()
    }

    // =============================================
    //  Full Integration / Smoke Test
    // =============================================

    #[test]
    fn test_full_response_with_all_fields() {
        let mut input = base_chat_response();
        input.insert("service_tier".to_string(), json!("default"));
        input.insert("system_fingerprint".to_string(), json!("fp_abc123"));

        let mut result =
            chat_to_responses_response(Value::Object(input), &NamespaceMappings::new()).unwrap();
        let expected = json!({
            "id": "chatcmpl_abc123",
            "created_at": 1712530587,
            "model": "gpt-4o",
            "object": "response",
            "status": "completed",
            "output": [{
                "type": "message",
                "role": "assistant",
                "status": "completed",
                "content": [{
                    "type": "output_text",
                    "text": "Hello, world!",
                    "annotations": [],
                    "logprobs": null
                }]
            }],
            "parallel_tool_calls": true,
            "tools": [],
            "usage": {
                "input_tokens": 10,
                "output_tokens": 5,
                "total_tokens": 15,
                "input_tokens_details": { "cached_tokens": 0 },
                "output_tokens_details": { "reasoning_tokens": 0 }
            },
            "service_tier": "default",
            "metadata": { "system_fingerprint": "fp_abc123" }
        });

        strip_output_item_ids(&mut result);
        assert_eq!(result, expected);
    }

    // =============================================
    //  Content Variants
    // =============================================

    #[test]
    fn test_content_variants() {
        // Simple text content
        let input = json!({
            "id": "chatcmpl_1", "choices": [{ "finish_reason": "stop", "index": 0,
                "message": { "content": "Hello!", "refusal": null, "role": "assistant" } }],
            "created": 1712530587, "model": "gpt-4o", "object": "chat.completion"
        });
        let result = chat_to_responses_response(input, &NamespaceMappings::new()).unwrap();
        assert_eq!(
            result["output"][0]["content"][0]["text"], "Hello!",
            "simple text"
        );

        // Empty content → no output items
        let input = json!({
            "id": "chatcmpl_2", "choices": [{ "finish_reason": "stop", "index": 0,
                "message": { "content": "", "refusal": null, "role": "assistant" } }],
            "created": 1712530587, "model": "gpt-4o", "object": "chat.completion"
        });
        let result = chat_to_responses_response(input, &NamespaceMappings::new()).unwrap();
        assert!(
            result["output"].as_array().unwrap().is_empty(),
            "empty content"
        );

        // Null content → no output items
        let input = json!({
            "id": "chatcmpl_3", "choices": [{ "finish_reason": "stop", "index": 0,
                "message": { "content": null, "refusal": null, "role": "assistant" } }],
            "created": 1712530587, "model": "gpt-4o", "object": "chat.completion"
        });
        let result = chat_to_responses_response(input, &NamespaceMappings::new()).unwrap();
        assert!(
            result["output"].as_array().unwrap().is_empty(),
            "null content"
        );

        // Refusal → output_text with refusal text
        let input = json!({
            "id": "chatcmpl_4", "choices": [{ "finish_reason": "stop", "index": 0,
                "message": { "content": null, "refusal": "I cannot answer.", "role": "assistant" } }],
            "created": 1712530587, "model": "gpt-4o", "object": "chat.completion"
        });
        let result = chat_to_responses_response(input, &NamespaceMappings::new()).unwrap();
        assert_eq!(
            result["output"][0]["content"][0]["type"], "refusal",
            "refusal type"
        );
        assert_eq!(
            result["output"][0]["content"][0]["refusal"], "I cannot answer.",
            "refusal text"
        );

        // Empty refusal not included
        let input = json!({
            "id": "chatcmpl_5", "choices": [{ "finish_reason": "stop", "index": 0,
                "message": { "content": "Hello", "refusal": "", "role": "assistant" } }],
            "created": 1712530587, "model": "gpt-4o", "object": "chat.completion"
        });
        let result = chat_to_responses_response(input, &NamespaceMappings::new()).unwrap();
        assert!(
            !result["output"][0]["content"]
                .as_array()
                .unwrap()
                .iter()
                .any(|c| c.get("type") == Some(&json!("refusal"))),
            "empty refusal excluded"
        );
    }

    // =============================================
    //  Content Blocks (multimodal array content)
    // =============================================

    #[test]
    fn test_multimodal_array_content() {
        let input = json!({
            "id": "chatcmpl_mm",
            "choices": [{
                "finish_reason": "stop",
                "index": 0,
                "message": {
                    "content": [
                        { "type": "text", "text": "First text" },
                        { "type": "image_url", "image_url": { "url": "https://example.com/img.png", "detail": "low" } },
                        { "type": "text", "text": "Second text" }
                    ],
                    "refusal": null,
                    "role": "assistant"
                }
            }],
            "created": 1712530587,
            "model": "gpt-4o",
            "object": "chat.completion"
        });

        let result = chat_to_responses_response(input, &NamespaceMappings::new()).unwrap();
        let output = result["output"][0]["content"].as_array().unwrap();
        assert_eq!(output.len(), 3, "three content blocks");
        assert_eq!(output[0]["type"], "output_text");
        assert_eq!(output[0]["text"], "First text");
        assert_eq!(output[1]["type"], "output_text");
        assert!(output[1]["text"].as_str().unwrap().contains("img.png"));
        assert_eq!(output[2]["type"], "output_text");
        assert_eq!(output[2]["text"], "Second text");
    }

    // =============================================
    //  Tool Calls
    // =============================================

    #[test]
    fn test_tool_calls() {
        let result = |input: Value| -> Value {
            let mut r = chat_to_responses_response(input, &NamespaceMappings::new()).unwrap();
            strip_output_item_ids(&mut r);
            r
        };

        // Single tool call
        let input = json!({
            "id": "chatcmpl_tc1", "choices": [{ "finish_reason": "tool_calls", "index": 0,
                "message": { "content": null, "refusal": null, "role": "assistant",
                    "tool_calls": [{ "id": "call_1", "type": "function",
                        "function": { "name": "get_weather", "arguments": "{\"city\":\"Tokyo\"}" } }]
                } }],
            "created": 1712530587, "model": "gpt-4o", "object": "chat.completion"
        });
        let r = result(input);
        assert_eq!(
            r["output"][0]["type"], "function_call",
            "single tool call type"
        );
        assert_eq!(r["output"][0]["call_id"], "call_1");

        // Multiple tool calls
        let input = json!({
            "id": "chatcmpl_tc2", "choices": [{ "finish_reason": "tool_calls", "index": 0,
                "message": { "content": null, "refusal": null, "role": "assistant",
                    "tool_calls": [
                        { "id": "call_1", "type": "function", "function": { "name": "get_weather", "arguments": "{}" } },
                        { "id": "call_2", "type": "function", "function": { "name": "get_time", "arguments": "{}" } }
                    ] } }],
            "created": 1712530587, "model": "gpt-4o", "object": "chat.completion"
        });
        let r = result(input);
        assert_eq!(r["output"].as_array().unwrap().len(), 2, "two tool calls");
        assert_eq!(r["output"][0]["name"], "get_weather");
        assert_eq!(r["output"][1]["name"], "get_time");

        // Content + tool calls together
        let input = json!({
            "id": "chatcmpl_tc3", "choices": [{ "finish_reason": "tool_calls", "index": 0,
                "message": { "content": "Check weather", "refusal": null, "role": "assistant",
                    "tool_calls": [{ "id": "call_1", "type": "function",
                        "function": { "name": "get_weather", "arguments": "{}" } }] } }],
            "created": 1712530587, "model": "gpt-4o", "object": "chat.completion"
        });
        let r = result(input);
        assert_eq!(
            r["output"].as_array().unwrap().len(),
            2,
            "content + tool call"
        );
        assert_eq!(r["output"][0]["type"], "message");
        assert_eq!(r["output"][1]["type"], "function_call");

        // Non-function tool call filtered
        let input = json!({
            "id": "chatcmpl_tc4", "choices": [{ "finish_reason": "tool_calls", "index": 0,
                "message": { "content": null, "refusal": null, "role": "assistant",
                    "tool_calls": [
                        { "id": "call_1", "type": "function", "function": { "name": "valid_func", "arguments": "{}" } },
                        { "id": "call_2", "type": "not_function", "function": { "name": "invalid", "arguments": "{}" } }
                    ] } }],
            "created": 1712530587, "model": "gpt-4o", "object": "chat.completion"
        });
        let r = result(input);
        assert_eq!(
            r["output"].as_array().unwrap().len(),
            1,
            "non-function filtered"
        );
        assert_eq!(r["output"][0]["name"], "valid_func");
    }

    // =============================================
    //  Usage
    // =============================================

    #[test]
    fn test_usage() {
        let result = |input: Value| -> Value {
            let mut r = chat_to_responses_response(input, &NamespaceMappings::new()).unwrap();
            strip_output_item_ids(&mut r);
            r
        };

        // With details
        let input = json!({
            "id": "chatcmpl_u1", "choices": [{ "finish_reason": "stop", "index": 0,
                "message": { "content": "Hi", "refusal": null, "role": "assistant" } }],
            "created": 1712530587, "model": "gpt-4o", "object": "chat.completion",
            "usage": {
                "completion_tokens": 5, "prompt_tokens": 10, "total_tokens": 15,
                "completion_tokens_details": { "reasoning_tokens": 2 },
                "prompt_tokens_details": { "cached_tokens": 3 }
            }
        });
        let r = result(input);
        assert_eq!(r["usage"]["input_tokens"], 10, "input_tokens");
        assert_eq!(r["usage"]["output_tokens"], 5, "output_tokens");
        assert_eq!(r["usage"]["total_tokens"], 15, "total_tokens");
        assert_eq!(
            r["usage"]["input_tokens_details"]["cached_tokens"], 3,
            "cached_tokens"
        );
        assert_eq!(
            r["usage"]["output_tokens_details"]["reasoning_tokens"], 2,
            "reasoning_tokens"
        );

        // Without details
        let input = json!({
            "id": "chatcmpl_u2", "choices": [{ "finish_reason": "stop", "index": 0,
                "message": { "content": "Hi", "refusal": null, "role": "assistant" } }],
            "created": 1712530587, "model": "gpt-4o", "object": "chat.completion",
            "usage": { "completion_tokens": 5, "prompt_tokens": 10, "total_tokens": 15 }
        });
        let r = result(input);
        assert_eq!(
            r["usage"]["input_tokens_details"]["cached_tokens"], 0,
            "default cached"
        );
        assert_eq!(
            r["usage"]["output_tokens_details"]["reasoning_tokens"], 0,
            "default reasoning"
        );

        // Without total_tokens (calculated from prompt + completion)
        let input = json!({
            "id": "chatcmpl_u3", "choices": [{ "finish_reason": "stop", "index": 0,
                "message": { "content": "Hi", "refusal": null, "role": "assistant" } }],
            "created": 1712530587, "model": "gpt-4o", "object": "chat.completion",
            "usage": { "completion_tokens": 5, "prompt_tokens": 10 }
        });
        let r = result(input);
        assert_eq!(r["usage"]["total_tokens"], 15, "computed total");

        // No usage field
        let input = json!({
            "id": "chatcmpl_u4", "choices": [{ "finish_reason": "stop", "index": 0,
                "message": { "content": "Hi", "refusal": null, "role": "assistant" } }],
            "created": 1712530587, "model": "gpt-4o", "object": "chat.completion"
        });
        let r = result(input);
        assert!(!r.as_object().unwrap().contains_key("usage"), "no usage");
    }

    // =============================================
    //  Finish Reason → Status
    // =============================================

    #[test]
    fn test_finish_reason_to_status() {
        for (reason, expected_status) in [
            ("stop", "completed"),
            ("length", "incomplete"),
            ("tool_calls", "completed"),
            ("content_filter", "incomplete"),
        ] {
            let input = json!({
                "id": "chatcmpl_fr", "choices": [{ "finish_reason": reason, "index": 0,
                    "message": { "content": "Hi", "refusal": null, "role": "assistant" } }],
                "created": 1712530587, "model": "gpt-4o", "object": "chat.completion"
            });
            let result = chat_to_responses_response(input, &NamespaceMappings::new()).unwrap();
            assert_eq!(result["status"], expected_status, "finish_reason: {reason}");
        }
    }

    // =============================================
    //  Reasoning Content
    // =============================================

    #[test]
    fn test_reasoning_content() {
        let result = |input: Value| -> Value {
            let mut r = chat_to_responses_response(input, &NamespaceMappings::new()).unwrap();
            strip_output_item_ids(&mut r);
            r
        };

        // reasoning_content converted to reasoning item
        let input = json!({
            "id": "chatcmpl_r1", "choices": [{ "finish_reason": "stop", "index": 0,
                "message": { "content": "Answer", "refusal": null, "role": "assistant",
                    "reasoning_content": "Let me think..." } }],
            "created": 1712530587, "model": "gpt-4o", "object": "chat.completion"
        });
        let r = result(input);
        assert_eq!(r["output"][0]["type"], "reasoning", "reasoning item type");
        assert_eq!(
            r["output"][0]["summary"][0]["text"], "Let me think...",
            "reasoning content"
        );

        // Only reasoning_content, no text content
        let input = json!({
            "id": "chatcmpl_r2", "choices": [{ "finish_reason": "stop", "index": 0,
                "message": { "content": null, "refusal": null, "role": "assistant",
                    "reasoning_content": "Thinking..." } }],
            "created": 1712530587, "model": "gpt-4o", "object": "chat.completion"
        });
        let r = result(input);
        assert_eq!(r["output"].as_array().unwrap().len(), 1, "only reasoning");
        assert_eq!(r["output"][0]["type"], "reasoning");

        // Empty reasoning_content skipped
        let input = json!({
            "id": "chatcmpl_r3", "choices": [{ "finish_reason": "stop", "index": 0,
                "message": { "content": "Answer", "refusal": null, "role": "assistant",
                    "reasoning_content": "" } }],
            "created": 1712530587, "model": "gpt-4o", "object": "chat.completion"
        });
        let r = result(input);
        assert!(
            r["output"][0]["type"] != "reasoning",
            "empty reasoning skipped"
        );
    }

    // =============================================
    //  Edge Cases
    // =============================================

    #[test]
    fn test_edge_cases() {
        // Empty choices → status incomplete
        let input = json!({
            "id": "chatcmpl_ec1", "choices": [],
            "created": 1712530587, "model": "gpt-4o", "object": "chat.completion"
        });
        let result = chat_to_responses_response(input, &NamespaceMappings::new()).unwrap();
        assert_eq!(result["status"], "incomplete", "empty choices");
        assert!(result["output"].as_array().unwrap().is_empty());

        // Service tier preserved
        let input = json!({
            "id": "chatcmpl_ec2", "choices": [{ "finish_reason": "stop", "index": 0,
                "message": { "content": "Hi", "refusal": null, "role": "assistant" } }],
            "created": 1712530587, "model": "gpt-4o", "object": "chat.completion",
            "service_tier": "default"
        });
        let result = chat_to_responses_response(input, &NamespaceMappings::new()).unwrap();
        assert_eq!(result["service_tier"], "default", "service tier");

        // System fingerprint → metadata
        let input = json!({
            "id": "chatcmpl_ec3", "choices": [{ "finish_reason": "stop", "index": 0,
                "message": { "content": "Hi", "refusal": null, "role": "assistant" } }],
            "created": 1712530587, "model": "gpt-4o", "object": "chat.completion",
            "system_fingerprint": "fp_xyz"
        });
        let result = chat_to_responses_response(input, &NamespaceMappings::new()).unwrap();
        assert_eq!(
            result["metadata"]["system_fingerprint"], "fp_xyz",
            "fingerprint"
        );
    }

    #[test]
    fn test_invalid_body() {
        let r = chat_to_responses_response(json!("not an object"), &NamespaceMappings::new());
        assert!(r.is_err(), "non-object body");

        let r = chat_to_responses_response(json!(["array"]), &NamespaceMappings::new());
        assert!(r.is_err(), "array body");
    }

    // =============================================
    //  Multiple Choices / Status Determination
    // =============================================

    #[test]
    fn test_multiple_choices_and_status() {
        let result = |input: Value| -> Value {
            let mut r = chat_to_responses_response(input, &NamespaceMappings::new()).unwrap();
            strip_output_item_ids(&mut r);
            r
        };

        // Multiple choices all completed
        let input = json!({
            "id": "chatcmpl_mc1", "choices": [
                { "finish_reason": "stop", "index": 0, "message": { "content": "A", "refusal": null, "role": "assistant" } },
                { "finish_reason": "stop", "index": 1, "message": { "content": "B", "refusal": null, "role": "assistant" } }
            ],
            "created": 1712530587, "model": "gpt-4o", "object": "chat.completion"
        });
        let r = result(input);
        assert_eq!(
            r["output"].as_array().unwrap().len(),
            2,
            "two choices output"
        );
        assert_eq!(r["status"], "completed");

        // First relevant choice determines status
        let input = json!({
            "id": "chatcmpl_mc2", "choices": [
                { "finish_reason": "stop", "index": 0, "message": { "content": "A", "refusal": null, "role": "assistant" } },
                { "finish_reason": "length", "index": 1, "message": { "content": "B", "refusal": null, "role": "assistant" } }
            ],
            "created": 1712530587, "model": "gpt-4o", "object": "chat.completion"
        });
        let r = result(input);
        assert_eq!(r["status"], "completed", "first choice determines status");
    }

    // =============================================
    //  URL Citation Annotations
    // =============================================

    #[test]
    fn test_url_citation_annotations() {
        let input = json!({
            "id": "chatcmpl_annot",
            "choices": [{
                "finish_reason": "stop",
                "index": 0,
                "message": {
                    "content": "According to the source, the answer is 42.",
                    "refusal": null,
                    "role": "assistant",
                    "annotations": [{
                        "type": "url_citation",
                        "url_citation": {
                            "start_index": 0,
                            "end_index": 12,
                            "url": "https://example.com",
                            "title": "Example Source"
                        }
                    }]
                }
            }],
            "created": 1712530587,
            "model": "gpt-4o",
            "object": "chat.completion"
        });

        let result = chat_to_responses_response(input, &NamespaceMappings::new()).unwrap();
        let content = &result["output"][0]["content"];
        let annotations = content[0]["annotations"].as_array().unwrap();
        assert_eq!(annotations.len(), 1, "one annotation");
        assert_eq!(annotations[0]["type"], "url_citation");
        assert_eq!(annotations[0]["url"], "https://example.com");
        assert_eq!(annotations[0]["title"], "Example Source");
        assert_eq!(annotations[0]["start_index"], 0);
        assert_eq!(annotations[0]["end_index"], 12);
    }

    // =============================================
    //  Namespace Tool Calls
    // =============================================

    #[test]
    fn test_namespace_tool_calls() {
        // Tool call with namespace prefix but no mappings → name stays as-is
        let input = json!({
            "id": "chatcmpl_ns1", "choices": [{ "finish_reason": "tool_calls", "index": 0,
                "message": { "content": null, "refusal": null, "role": "assistant",
                    "tool_calls": [{ "id": "call_abc", "type": "function",
                        "function": { "name": "mcp__weather__get_forecast", "arguments": "{\"city\":\"Tokyo\"}" } }] } }],
            "created": 1712530587, "model": "gpt-4o", "object": "chat.completion"
        });
        let result = chat_to_responses_response(input, &NamespaceMappings::new()).unwrap();
        assert_eq!(
            result["output"][0]["name"], "mcp__weather__get_forecast",
            "no mappings"
        );

        // With mappings → split into name + namespace
        let mut mappings = NamespaceMappings::new();
        mappings.add_namespace("mcp__weather__".to_string());
        let input = json!({
            "id": "chatcmpl_ns2", "choices": [{ "finish_reason": "tool_calls", "index": 0,
                "message": { "content": null, "refusal": null, "role": "assistant",
                    "tool_calls": [{ "id": "call_abc", "type": "function",
                        "function": { "name": "mcp__weather__get_forecast", "arguments": "{\"city\":\"Tokyo\"}" } }] } }],
            "created": 1712530587, "model": "gpt-4o", "object": "chat.completion"
        });
        let result = chat_to_responses_response(input, &mappings).unwrap();
        assert_eq!(
            result["output"][0]["name"], "get_forecast",
            "with mappings name"
        );
        assert_eq!(
            result["output"][0]["namespace"], "mcp__weather__",
            "with mappings namespace"
        );

        // Mixed: regular + namespaced tool calls
        let mut mappings = NamespaceMappings::new();
        mappings.add_namespace("mcp__weather__".to_string());
        let input = json!({
            "id": "chatcmpl_ns3", "choices": [{ "finish_reason": "tool_calls", "index": 0,
                "message": { "content": null, "refusal": null, "role": "assistant",
                    "tool_calls": [
                        { "id": "call_1", "type": "function", "function": { "name": "get_weather", "arguments": "{}" } },
                        { "id": "call_2", "type": "function", "function": { "name": "mcp__weather__get_forecast", "arguments": "{\"city\":\"Paris\"}" } }
                    ] } }],
            "created": 1712530587, "model": "gpt-4o", "object": "chat.completion"
        });
        let result = chat_to_responses_response(input, &mappings).unwrap();
        assert_eq!(result["output"][0]["name"], "get_weather", "regular name");
        assert!(
            !result["output"][0]
                .as_object()
                .unwrap()
                .contains_key("namespace"),
            "no namespace for regular"
        );
        assert_eq!(
            result["output"][1]["name"], "get_forecast",
            "namespaced name"
        );
        assert_eq!(
            result["output"][1]["namespace"], "mcp__weather__",
            "namespaced namespace"
        );
    }
}
