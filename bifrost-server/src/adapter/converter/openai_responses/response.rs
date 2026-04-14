//! Transform Chat API responses to Responses API format
//!
//! This module provides functions to convert Chat Completions API response format
//! to OpenAI Responses API compatible format (non-streaming).

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
pub fn chat_to_responses_response(body: Value) -> Result<Value, LlmMapError> {
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
        output.extend(build_output_items_from_message(msg_obj)?);
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
            if let Some(fc_item) = transform_tool_call_to_function_call(tc)? {
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
fn transform_tool_call_to_function_call(tc: &Value) -> Result<Option<Value>, LlmMapError> {
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

    let name = func.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let arguments = func.get("arguments").and_then(|v| v.as_str()).unwrap_or("");

    let mut fc_item = serde_json::Map::new();
    fc_item.insert("id".to_string(), Value::String(super::create_item_id("fc")));
    fc_item.insert(
        "type".to_string(),
        Value::String("function_call".to_string()),
    );
    fc_item.insert("call_id".to_string(), Value::String(call_id.to_string()));
    fc_item.insert("name".to_string(), Value::String(name.to_string()));
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

    #[test]
    fn test_simple_text_response() {
        let input = json!({
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
        });

        let mut result = chat_to_responses_response(input).unwrap();
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
            }
        });

        strip_output_item_ids(&mut result);
        assert_eq!(result, expected);
    }

    #[test]
    fn test_empty_content_response() {
        let input = json!({
            "id": "chatcmpl_empty",
            "choices": [{
                "finish_reason": "stop",
                "index": 0,
                "message": {
                    "content": "",
                    "refusal": null,
                    "role": "assistant"
                }
            }],
            "created": 1712530587,
            "model": "gpt-4o",
            "object": "chat.completion"
        });

        let mut result = chat_to_responses_response(input).unwrap();
        let expected = json!({
            "id": "chatcmpl_empty",
            "created_at": 1712530587,
            "model": "gpt-4o",
            "object": "response",
            "status": "completed",
            "output": [],
            "parallel_tool_calls": true,
            "tools": []
        });

        strip_output_item_ids(&mut result);
        assert_eq!(result, expected);
    }

    #[test]
    fn test_null_content_response() {
        let input = json!({
            "id": "chatcmpl_null",
            "choices": [{
                "finish_reason": "stop",
                "index": 0,
                "message": {
                    "content": null,
                    "refusal": null,
                    "role": "assistant"
                }
            }],
            "created": 1712530587,
            "model": "gpt-4o",
            "object": "chat.completion"
        });

        let mut result = chat_to_responses_response(input).unwrap();
        let expected = json!({
            "id": "chatcmpl_null",
            "created_at": 1712530587,
            "model": "gpt-4o",
            "object": "response",
            "status": "completed",
            "output": [],
            "parallel_tool_calls": true,
            "tools": []
        });

        strip_output_item_ids(&mut result);
        assert_eq!(result, expected);
    }

    #[test]
    fn test_refusal_response() {
        let input = json!({
            "id": "chatcmpl_refusal",
            "choices": [{
                "finish_reason": "stop",
                "index": 0,
                "message": {
                    "content": null,
                    "refusal": "I cannot provide that information.",
                    "role": "assistant"
                }
            }],
            "created": 1712530587,
            "model": "gpt-4o",
            "object": "chat.completion"
        });

        let mut result = chat_to_responses_response(input).unwrap();
        let expected = json!({
            "id": "chatcmpl_refusal",
            "created_at": 1712530587,
            "model": "gpt-4o",
            "object": "response",
            "status": "completed",
            "output": [{
                "type": "message",
                "role": "assistant",
                "status": "completed",
                "content": [{
                    "type": "refusal",
                    "refusal": "I cannot provide that information."
                }]
            }],
            "parallel_tool_calls": true,
            "tools": []
        });

        strip_output_item_ids(&mut result);
        assert_eq!(result, expected);
    }

    #[test]
    fn test_empty_refusal_not_included() {
        let input = json!({
            "id": "chatcmpl_empty_refusal",
            "choices": [{
                "finish_reason": "stop",
                "index": 0,
                "message": {
                    "content": "Hello!",
                    "refusal": "",
                    "role": "assistant"
                }
            }],
            "created": 1712530587,
            "model": "gpt-4o",
            "object": "chat.completion"
        });

        let mut result = chat_to_responses_response(input).unwrap();
        let expected = json!({
            "id": "chatcmpl_empty_refusal",
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
                    "text": "Hello!",
                    "annotations": [],
                    "logprobs": null
                }]
            }],
            "parallel_tool_calls": true,
            "tools": []
        });

        strip_output_item_ids(&mut result);
        assert_eq!(result, expected);
    }

    #[test]
    fn test_single_tool_call() {
        let input = json!({
            "id": "chatcmpl_tool",
            "choices": [{
                "finish_reason": "tool_calls",
                "index": 0,
                "message": {
                    "content": null,
                    "refusal": null,
                    "role": "assistant",
                    "tool_calls": [{
                        "id": "call_abc123",
                        "type": "function",
                        "function": {
                            "name": "get_weather",
                            "arguments": "{\"city\": \"Tokyo\"}"
                        }
                    }]
                }
            }],
            "created": 1712530587,
            "model": "gpt-4o",
            "object": "chat.completion"
        });

        let mut result = chat_to_responses_response(input).unwrap();
        let expected = json!({
            "id": "chatcmpl_tool",
            "created_at": 1712530587,
            "model": "gpt-4o",
            "object": "response",
            "status": "completed",
            "output": [{
                "type": "function_call",
                "call_id": "call_abc123",
                "name": "get_weather",
                "arguments": "{\"city\": \"Tokyo\"}",
                "status": "completed"
            }],
            "parallel_tool_calls": true,
            "tools": []
        });

        strip_output_item_ids(&mut result);
        assert_eq!(result, expected);
    }

    #[test]
    fn test_multiple_tool_calls() {
        let input = json!({
            "id": "chatcmpl_multi_tool",
            "choices": [{
                "finish_reason": "tool_calls",
                "index": 0,
                "message": {
                    "content": null,
                    "refusal": null,
                    "role": "assistant",
                    "tool_calls": [
                        {
                            "id": "call_1",
                            "type": "function",
                            "function": {
                                "name": "get_weather",
                                "arguments": "{\"city\": \"Tokyo\"}"
                            }
                        },
                        {
                            "id": "call_2",
                            "type": "function",
                            "function": {
                                "name": "get_time",
                                "arguments": "{\"timezone\": \"JST\"}"
                            }
                        }
                    ]
                }
            }],
            "created": 1712530587,
            "model": "gpt-4o",
            "object": "chat.completion"
        });

        let mut result = chat_to_responses_response(input).unwrap();
        let expected = json!({
            "id": "chatcmpl_multi_tool",
            "created_at": 1712530587,
            "model": "gpt-4o",
            "object": "response",
            "status": "completed",
            "output": [
                {
                    "type": "function_call",
                    "call_id": "call_1",
                    "name": "get_weather",
                    "arguments": "{\"city\": \"Tokyo\"}",
                    "status": "completed"
                },
                {
                    "type": "function_call",
                    "call_id": "call_2",
                    "name": "get_time",
                    "arguments": "{\"timezone\": \"JST\"}",
                    "status": "completed"
                }
            ],
            "parallel_tool_calls": true,
            "tools": []
        });

        strip_output_item_ids(&mut result);
        assert_eq!(result, expected);
    }

    #[test]
    fn test_content_and_tool_calls_together() {
        let input = json!({
            "id": "chatcmpl_content_tool",
            "choices": [{
                "finish_reason": "tool_calls",
                "index": 0,
                "message": {
                    "content": "Let me check that for you.",
                    "refusal": null,
                    "role": "assistant",
                    "tool_calls": [{
                        "id": "call_abc",
                        "type": "function",
                        "function": {
                            "name": "search",
                            "arguments": "{\"query\": \"weather\"}"
                        }
                    }]
                }
            }],
            "created": 1712530587,
            "model": "gpt-4o",
            "object": "chat.completion"
        });

        let mut result = chat_to_responses_response(input).unwrap();
        let expected = json!({
            "id": "chatcmpl_content_tool",
            "created_at": 1712530587,
            "model": "gpt-4o",
            "object": "response",
            "status": "completed",
            "output": [
                {
                    "type": "message",
                    "role": "assistant",
                    "status": "completed",
                    "content": [{
                        "type": "output_text",
                        "text": "Let me check that for you.",
                        "annotations": [],
                        "logprobs": null
                    }]
                },
                {
                    "type": "function_call",
                    "call_id": "call_abc",
                    "name": "search",
                    "arguments": "{\"query\": \"weather\"}",
                    "status": "completed"
                }
            ],
            "parallel_tool_calls": true,
            "tools": []
        });

        strip_output_item_ids(&mut result);
        assert_eq!(result, expected);
    }

    #[test]
    fn test_non_function_tool_call_filtered() {
        let input = json!({
            "id": "chatcmpl_code_interp",
            "choices": [{
                "finish_reason": "tool_calls",
                "index": 0,
                "message": {
                    "content": null,
                    "refusal": null,
                    "role": "assistant",
                    "tool_calls": [
                        {
                            "id": "call_1",
                            "type": "function",
                            "function": {
                                "name": "calculate",
                                "arguments": "{}"
                            }
                        },
                        {
                            "id": "call_2",
                            "type": "code_interpreter",
                            "code": "print('hello')"
                        }
                    ]
                }
            }],
            "created": 1712530587,
            "model": "gpt-4o",
            "object": "chat.completion"
        });

        let mut result = chat_to_responses_response(input).unwrap();
        let expected = json!({
            "id": "chatcmpl_code_interp",
            "created_at": 1712530587,
            "model": "gpt-4o",
            "object": "response",
            "status": "completed",
            "output": [{
                "type": "function_call",
                "call_id": "call_1",
                "name": "calculate",
                "arguments": "{}",
                "status": "completed"
            }],
            "parallel_tool_calls": true,
            "tools": []
        });

        strip_output_item_ids(&mut result);
        assert_eq!(result, expected);
    }

    #[test]
    fn test_url_citation_annotation() {
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

        let mut result = chat_to_responses_response(input).unwrap();
        let expected = json!({
            "id": "chatcmpl_annot",
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
                    "text": "According to the source, the answer is 42.",
                    "annotations": [{
                        "type": "url_citation",
                        "start_index": 0,
                        "end_index": 12,
                        "url": "https://example.com",
                        "title": "Example Source"
                    }],
                    "logprobs": null
                }]
            }],
            "parallel_tool_calls": true,
            "tools": []
        });

        strip_output_item_ids(&mut result);
        assert_eq!(result, expected);
    }

    #[test]
    fn test_usage_with_details() {
        let input = json!({
            "id": "chatcmpl_usage",
            "choices": [{
                "finish_reason": "stop",
                "index": 0,
                "message": {
                    "content": "Hello!",
                    "refusal": null,
                    "role": "assistant"
                }
            }],
            "created": 1712530587,
            "model": "gpt-4o",
            "object": "chat.completion",
            "usage": {
                "completion_tokens": 42,
                "prompt_tokens": 125,
                "total_tokens": 167,
                "completion_tokens_details": {
                    "reasoning_tokens": 15,
                    "audio_tokens": null,
                    "accepted_prediction_tokens": null,
                    "rejected_prediction_tokens": null
                },
                "prompt_tokens_details": {
                    "audio_tokens": null,
                    "cached_tokens": 64
                }
            }
        });

        let mut result = chat_to_responses_response(input).unwrap();
        let expected = json!({
            "id": "chatcmpl_usage",
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
                    "text": "Hello!",
                    "annotations": [],
                    "logprobs": null
                }]
            }],
            "parallel_tool_calls": true,
            "tools": [],
            "usage": {
                "input_tokens": 125,
                "output_tokens": 42,
                "total_tokens": 167,
                "input_tokens_details": { "cached_tokens": 64 },
                "output_tokens_details": { "reasoning_tokens": 15 }
            }
        });

        strip_output_item_ids(&mut result);
        assert_eq!(result, expected);
    }

    #[test]
    fn test_usage_without_details() {
        let input = json!({
            "id": "chatcmpl_no_details",
            "choices": [{
                "finish_reason": "stop",
                "index": 0,
                "message": {
                    "content": "Hi",
                    "refusal": null,
                    "role": "assistant"
                }
            }],
            "created": 1712530587,
            "model": "gpt-4o",
            "object": "chat.completion",
            "usage": {
                "completion_tokens": 3,
                "prompt_tokens": 10,
                "total_tokens": 13
            }
        });

        let mut result = chat_to_responses_response(input).unwrap();
        let expected = json!({
            "id": "chatcmpl_no_details",
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
                    "text": "Hi",
                    "annotations": [],
                    "logprobs": null
                }]
            }],
            "parallel_tool_calls": true,
            "tools": [],
            "usage": {
                "input_tokens": 10,
                "output_tokens": 3,
                "total_tokens": 13,
                "input_tokens_details": { "cached_tokens": 0 },
                "output_tokens_details": { "reasoning_tokens": 0 }
            }
        });

        strip_output_item_ids(&mut result);
        assert_eq!(result, expected);
    }

    #[test]
    fn test_usage_without_total_tokens() {
        let input = json!({
            "id": "chatcmpl_no_total",
            "choices": [{
                "finish_reason": "stop",
                "index": 0,
                "message": {
                    "content": "Hi",
                    "refusal": null,
                    "role": "assistant"
                }
            }],
            "created": 1712530587,
            "model": "gpt-4o",
            "object": "chat.completion",
            "usage": {
                "completion_tokens": 3,
                "prompt_tokens": 10
            }
        });

        let mut result = chat_to_responses_response(input).unwrap();
        let expected = json!({
            "id": "chatcmpl_no_total",
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
                    "text": "Hi",
                    "annotations": [],
                    "logprobs": null
                }]
            }],
            "parallel_tool_calls": true,
            "tools": [],
            "usage": {
                "input_tokens": 10,
                "output_tokens": 3,
                "total_tokens": 13,
                "input_tokens_details": { "cached_tokens": 0 },
                "output_tokens_details": { "reasoning_tokens": 0 }
            }
        });

        strip_output_item_ids(&mut result);
        assert_eq!(result, expected);
    }

    #[test]
    fn test_no_usage_field() {
        let input = json!({
            "id": "chatcmpl_no_usage",
            "choices": [{
                "finish_reason": "stop",
                "index": 0,
                "message": {
                    "content": "Hi",
                    "refusal": null,
                    "role": "assistant"
                }
            }],
            "created": 1712530587,
            "model": "gpt-4o",
            "object": "chat.completion"
        });

        let mut result = chat_to_responses_response(input).unwrap();
        let expected = json!({
            "id": "chatcmpl_no_usage",
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
                    "text": "Hi",
                    "annotations": [],
                    "logprobs": null
                }]
            }],
            "parallel_tool_calls": true,
            "tools": []
        });

        strip_output_item_ids(&mut result);
        assert_eq!(result, expected);
    }

    #[test]
    fn test_finish_reason_stop() {
        let input = json!({
            "id": "chatcmpl_stop",
            "choices": [{
                "finish_reason": "stop",
                "index": 0,
                "message": {
                    "content": "Done.",
                    "refusal": null,
                    "role": "assistant"
                }
            }],
            "created": 1712530587,
            "model": "gpt-4o",
            "object": "chat.completion"
        });

        let result = chat_to_responses_response(input).unwrap();
        assert_eq!(result["status"], "completed");
    }

    #[test]
    fn test_finish_reason_length() {
        let input = json!({
            "id": "chatcmpl_length",
            "choices": [{
                "finish_reason": "length",
                "index": 0,
                "message": {
                    "content": "This is a trunca",
                    "refusal": null,
                    "role": "assistant"
                }
            }],
            "created": 1712530587,
            "model": "gpt-4o",
            "object": "chat.completion"
        });

        let result = chat_to_responses_response(input).unwrap();
        assert_eq!(result["status"], "incomplete");
    }

    #[test]
    fn test_finish_reason_tool_calls() {
        let input = json!({
            "id": "chatcmpl_tools",
            "choices": [{
                "finish_reason": "tool_calls",
                "index": 0,
                "message": {
                    "content": null,
                    "refusal": null,
                    "role": "assistant",
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": { "name": "x", "arguments": "{}" }
                    }]
                }
            }],
            "created": 1712530587,
            "model": "gpt-4o",
            "object": "chat.completion"
        });

        let result = chat_to_responses_response(input).unwrap();
        assert_eq!(result["status"], "completed");
    }

    #[test]
    fn test_finish_reason_content_filter() {
        let input = json!({
            "id": "chatcmpl_filter",
            "choices": [{
                "finish_reason": "content_filter",
                "index": 0,
                "message": {
                    "content": "Filtered content",
                    "refusal": null,
                    "role": "assistant"
                }
            }],
            "created": 1712530587,
            "model": "gpt-4o",
            "object": "chat.completion"
        });

        let result = chat_to_responses_response(input).unwrap();
        assert_eq!(result["status"], "incomplete");
    }

    #[test]
    fn test_empty_choices() {
        let input = json!({
            "id": "chatcmpl_empty",
            "choices": [],
            "created": 1712530587,
            "model": "gpt-4o",
            "object": "chat.completion"
        });

        let mut result = chat_to_responses_response(input).unwrap();
        let expected = json!({
            "id": "chatcmpl_empty",
            "created_at": 1712530587,
            "model": "gpt-4o",
            "object": "response",
            "status": "incomplete",
            "output": [],
            "parallel_tool_calls": true,
            "tools": []
        });

        strip_output_item_ids(&mut result);
        assert_eq!(result, expected);
    }

    #[test]
    fn test_service_tier_preserved() {
        let input = json!({
            "id": "chatcmpl_tier",
            "choices": [{
                "finish_reason": "stop",
                "index": 0,
                "message": {
                    "content": "Hi",
                    "refusal": null,
                    "role": "assistant"
                }
            }],
            "created": 1712530587,
            "model": "gpt-4o",
            "object": "chat.completion",
            "service_tier": "default"
        });

        let result = chat_to_responses_response(input).unwrap();
        assert_eq!(result["service_tier"], "default");
    }

    #[test]
    fn test_system_fingerprint_as_metadata() {
        let input = json!({
            "id": "chatcmpl_fp",
            "choices": [{
                "finish_reason": "stop",
                "index": 0,
                "message": {
                    "content": "Hi",
                    "refusal": null,
                    "role": "assistant"
                }
            }],
            "created": 1712530587,
            "model": "gpt-4o",
            "object": "chat.completion",
            "system_fingerprint": "fp_12345abcde"
        });

        let result = chat_to_responses_response(input).unwrap();
        assert_eq!(result["metadata"]["system_fingerprint"], "fp_12345abcde");
    }

    #[test]
    fn test_invalid_body_not_object() {
        let input = json!("not an object");
        let result = chat_to_responses_response(input);
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_body_array() {
        let input = json!([1, 2, 3]);
        let result = chat_to_responses_response(input);
        assert!(result.is_err());
    }

    #[test]
    fn test_reasoning_content_converted() {
        let input = json!({
            "id": "chatcmpl_reasoning",
            "choices": [{
                "finish_reason": "stop",
                "index": 0,
                "message": {
                    "content": "The answer is 354.",
                    "refusal": null,
                    "role": "assistant",
                    "reasoning_content": "The user is asking for 15 * 23 + 9. Let me calculate: 15 * 23 = 345, then 345 + 9 = 354."
                }
            }],
            "created": 1712530587,
            "model": "gpt-4o",
            "object": "chat.completion"
        });

        let mut result = chat_to_responses_response(input).unwrap();
        let expected = json!({
            "id": "chatcmpl_reasoning",
            "created_at": 1712530587,
            "model": "gpt-4o",
            "object": "response",
            "status": "completed",
            "output": [
                {
                    "type": "reasoning",
                    "summary": [{
                        "type": "summary_text",
                        "text": "The user is asking for 15 * 23 + 9. Let me calculate: 15 * 23 = 345, then 345 + 9 = 354."
                    }],
                    "status": "completed"
                },
                {
                    "type": "message",
                    "role": "assistant",
                    "status": "completed",
                    "content": [{
                        "type": "output_text",
                        "text": "The answer is 354.",
                        "annotations": [],
                        "logprobs": null
                    }]
                }
            ],
            "parallel_tool_calls": true,
            "tools": []
        });

        strip_output_item_ids(&mut result);
        assert_eq!(result, expected);
    }

    #[test]
    fn test_reasoning_content_only() {
        let input = json!({
            "id": "chatcmpl_reasoning_only",
            "choices": [{
                "finish_reason": "stop",
                "index": 0,
                "message": {
                    "content": null,
                    "refusal": null,
                    "role": "assistant",
                    "reasoning_content": "Thinking about the problem..."
                }
            }],
            "created": 1712530587,
            "model": "gpt-4o",
            "object": "chat.completion"
        });

        let mut result = chat_to_responses_response(input).unwrap();
        let expected = json!({
            "id": "chatcmpl_reasoning_only",
            "created_at": 1712530587,
            "model": "gpt-4o",
            "object": "response",
            "status": "completed",
            "output": [{
                "type": "reasoning",
                "summary": [{
                    "type": "summary_text",
                    "text": "Thinking about the problem..."
                }],
                "status": "completed"
            }],
            "parallel_tool_calls": true,
            "tools": []
        });

        strip_output_item_ids(&mut result);
        assert_eq!(result, expected);
    }

    #[test]
    fn test_empty_reasoning_content_skipped() {
        let input = json!({
            "id": "chatcmpl_empty_reasoning",
            "choices": [{
                "finish_reason": "stop",
                "index": 0,
                "message": {
                    "content": "Hello!",
                    "refusal": null,
                    "role": "assistant",
                    "reasoning_content": ""
                }
            }],
            "created": 1712530587,
            "model": "gpt-4o",
            "object": "chat.completion"
        });

        let mut result = chat_to_responses_response(input).unwrap();
        let expected = json!({
            "id": "chatcmpl_empty_reasoning",
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
                    "text": "Hello!",
                    "annotations": [],
                    "logprobs": null
                }]
            }],
            "parallel_tool_calls": true,
            "tools": []
        });

        strip_output_item_ids(&mut result);
        assert_eq!(result, expected);
    }

    #[test]
    fn test_full_response_with_all_fields() {
        let input = json!({
            "id": "chatcmpl_full",
            "choices": [{
                "finish_reason": "stop",
                "index": 0,
                "message": {
                    "content": "The weather in Tokyo is sunny.",
                    "refusal": null,
                    "role": "assistant",
                    "reasoning_content": "I need to look up weather data for Tokyo. Let me call the weather API.",
                    "annotations": [{
                        "type": "url_citation",
                        "url_citation": {
                            "start_index": 19,
                            "end_index": 25,
                            "url": "https://weather.example.com/tokyo",
                            "title": "Tokyo Weather"
                        }
                    }],
                    "tool_calls": [{
                        "id": "call_abc123",
                        "type": "function",
                        "function": {
                            "name": "get_weather",
                            "arguments": "{\"city\": \"Tokyo\", \"country\": \"Japan\"}"
                        }
                    }]
                }
            }],
            "created": 1712530587,
            "model": "gpt-4o",
            "object": "chat.completion",
            "service_tier": "default",
            "system_fingerprint": "fp_12345abcde",
            "usage": {
                "completion_tokens": 18,
                "prompt_tokens": 25,
                "total_tokens": 43,
                "completion_tokens_details": {
                    "reasoning_tokens": 5
                },
                "prompt_tokens_details": {
                    "cached_tokens": 10
                }
            }
        });

        let mut result = chat_to_responses_response(input).unwrap();
        let expected = json!({
            "id": "chatcmpl_full",
            "created_at": 1712530587,
            "model": "gpt-4o",
            "object": "response",
            "status": "completed",
            "output": [
                {
                    "type": "reasoning",
                    "summary": [{
                        "type": "summary_text",
                        "text": "I need to look up weather data for Tokyo. Let me call the weather API."
                    }],
                    "status": "completed"
                },
                {
                    "type": "message",
                    "role": "assistant",
                    "status": "completed",
                    "content": [{
                        "type": "output_text",
                        "text": "The weather in Tokyo is sunny.",
                        "annotations": [{
                            "type": "url_citation",
                            "start_index": 19,
                            "end_index": 25,
                            "url": "https://weather.example.com/tokyo",
                            "title": "Tokyo Weather"
                        }],
                        "logprobs": null
                    }]
                },
                {
                    "type": "function_call",
                    "call_id": "call_abc123",
                    "name": "get_weather",
                    "arguments": "{\"city\": \"Tokyo\", \"country\": \"Japan\"}",
                    "status": "completed"
                }
            ],
            "parallel_tool_calls": true,
            "tools": [],
            "usage": {
                "input_tokens": 25,
                "output_tokens": 18,
                "total_tokens": 43,
                "input_tokens_details": { "cached_tokens": 10 },
                "output_tokens_details": { "reasoning_tokens": 5 }
            },
            "service_tier": "default",
            "metadata": { "system_fingerprint": "fp_12345abcde" }
        });

        strip_output_item_ids(&mut result);
        assert_eq!(result, expected);
    }

    #[test]
    fn test_multimodal_array_content() {
        let input = json!({
            "id": "chatcmpl_multimodal",
            "choices": [{
                "finish_reason": "stop",
                "index": 0,
                "message": {
                    "content": [
                        {"type": "text", "text": "Here is the image:"},
                        {"type": "image_url", "image_url": {"url": "https://example.com/img.png"}}
                    ],
                    "refusal": null,
                    "role": "assistant"
                }
            }],
            "created": 1712530587,
            "model": "gpt-4o",
            "object": "chat.completion"
        });

        let mut result = chat_to_responses_response(input).unwrap();
        let expected = json!({
            "id": "chatcmpl_multimodal",
            "created_at": 1712530587,
            "model": "gpt-4o",
            "object": "response",
            "status": "completed",
            "output": [{
                "type": "message",
                "role": "assistant",
                "status": "completed",
                "content": [
                    {
                        "type": "output_text",
                        "text": "Here is the image:",
                        "annotations": [],
                        "logprobs": null
                    },
                    {
                        "type": "output_text",
                        "text": "![image](https://example.com/img.png)",
                        "annotations": [],
                        "logprobs": null
                    }
                ]
            }],
            "parallel_tool_calls": true,
            "tools": []
        });

        strip_output_item_ids(&mut result);
        assert_eq!(result, expected);
    }

    #[test]
    fn test_multiple_choices() {
        let input = json!({
            "id": "chatcmpl_multi_choice",
            "choices": [
                {
                    "finish_reason": "stop",
                    "index": 0,
                    "message": {
                        "content": "First answer.",
                        "refusal": null,
                        "role": "assistant"
                    }
                },
                {
                    "finish_reason": "stop",
                    "index": 1,
                    "message": {
                        "content": "Second answer.",
                        "refusal": null,
                        "role": "assistant"
                    }
                }
            ],
            "created": 1712530587,
            "model": "gpt-4o",
            "object": "chat.completion"
        });

        let mut result = chat_to_responses_response(input).unwrap();
        let expected = json!({
            "id": "chatcmpl_multi_choice",
            "created_at": 1712530587,
            "model": "gpt-4o",
            "object": "response",
            "status": "completed",
            "output": [
                {
                    "type": "message",
                    "role": "assistant",
                    "status": "completed",
                    "content": [{
                        "type": "output_text",
                        "text": "First answer.",
                        "annotations": [],
                        "logprobs": null
                    }]
                },
                {
                    "type": "message",
                    "role": "assistant",
                    "status": "completed",
                    "content": [{
                        "type": "output_text",
                        "text": "Second answer.",
                        "annotations": [],
                        "logprobs": null
                    }]
                }
            ],
            "parallel_tool_calls": true,
            "tools": []
        });

        strip_output_item_ids(&mut result);
        assert_eq!(result, expected);
    }

    #[test]
    fn test_status_determined_by_first_relevant_choice() {
        let input = json!({
            "id": "chatcmpl_status_order",
            "choices": [
                {
                    "finish_reason": "stop",
                    "index": 0,
                    "message": {"content": "OK", "role": "assistant"}
                },
                {
                    "finish_reason": "length",
                    "index": 1,
                    "message": {"content": "Truncated", "role": "assistant"}
                }
            ],
            "created": 1712530587,
            "model": "gpt-4o",
            "object": "chat.completion"
        });

        let mut result = chat_to_responses_response(input).unwrap();
        let expected = json!({
            "id": "chatcmpl_status_order",
            "created_at": 1712530587,
            "model": "gpt-4o",
            "object": "response",
            "status": "completed",
            "output": [
                {
                    "type": "message",
                    "role": "assistant",
                    "status": "completed",
                    "content": [{
                        "type": "output_text",
                        "text": "OK",
                        "annotations": [],
                        "logprobs": null
                    }]
                },
                {
                    "type": "message",
                    "role": "assistant",
                    "status": "completed",
                    "content": [{
                        "type": "output_text",
                        "text": "Truncated",
                        "annotations": [],
                        "logprobs": null
                    }]
                }
            ],
            "parallel_tool_calls": true,
            "tools": []
        });

        strip_output_item_ids(&mut result);
        assert_eq!(result, expected);
    }
}
