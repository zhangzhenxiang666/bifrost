//! Message-level conversion utilities for OpenAI -> Anthropic

use serde_json::{Value, json};

pub fn extract_system_text(system: Value) -> String {
    match system {
        Value::String(s) => s,
        Value::Array(blocks) => blocks
            .into_iter()
            .filter_map(|b| match b {
                Value::Object(mut obj)
                    if obj.get("type").and_then(|t| t.as_str()) == Some("text") =>
                {
                    match obj.remove("text") {
                        Some(Value::String(s)) => Some(s),
                        _ => None,
                    }
                }
                _ => None,
            })
            .fold(String::new(), |mut acc, s| {
                if !acc.is_empty() {
                    acc.push(' ');
                }
                acc.push_str(&s);
                acc
            }),
        _ => String::new(),
    }
}

fn parse_image_block(url: &str) -> Value {
    if let Some(rest) = url.strip_prefix("data:").and_then(|s| s.split_once(';')) {
        let (media_type, after_semi) = rest;
        let data = after_semi.strip_prefix("base64,").unwrap_or(after_semi);
        return json!({
            "type": "image",
            "source": { "type": "base64", "media_type": media_type, "data": data }
        });
    }
    json!({ "type": "image", "source": { "type": "url", "url": url } })
}

fn convert_user_content(content: Option<Value>) -> Value {
    match content {
        Some(Value::Array(arr)) => {
            let parts = arr
                .into_iter()
                .map(|block| match &block {
                    Value::Object(obj)
                        if obj.get("type").and_then(|t| t.as_str()) == Some("image_url") =>
                    {
                        let url = obj
                            .get("image_url")
                            .and_then(|i| i.get("url"))
                            .and_then(|u| u.as_str())
                            .unwrap_or("");
                        parse_image_block(url)
                    }
                    _ => block,
                })
                .collect();
            Value::Array(parts)
        }
        Some(other) => other,
        None => Value::Null,
    }
}

fn convert_assistant_message(mut msg_obj: serde_json::Map<String, Value>) -> Value {
    let content = msg_obj.remove("content");
    let tool_calls = msg_obj.remove("tool_calls");
    let reasoning_content = msg_obj.remove("reasoning_content");

    let cap = tool_calls
        .as_ref()
        .and_then(|v| v.as_array())
        .map_or(1, |a| a.len() + 2);
    let mut parts = Vec::with_capacity(cap);

    // Handle reasoning_content (for o1/o3 models) -> Anthropic thinking block
    if let Some(Value::String(ref reasoning)) = reasoning_content
        && !reasoning.is_empty()
    {
        parts.push(json!({ "type": "thinking", "thinking": reasoning }));
    }

    if let Some(Value::String(ref text)) = content
        && !text.is_empty()
    {
        parts.push(json!({ "type": "text", "text": text }));
    }

    if let Some(Value::Array(calls)) = tool_calls {
        for call in calls {
            if let Value::Object(mut call_obj) = call {
                let id = call_obj.remove("id").unwrap_or(Value::Null);

                let (name, args) =
                    if let Some(Value::Object(mut func)) = call_obj.remove("function") {
                        let name = func.remove("name").unwrap_or(Value::Null);
                        let args = func
                            .remove("arguments")
                            .and_then(|v| match v {
                                Value::String(s) => serde_json::from_str(&s).ok(),
                                _ => None,
                            })
                            .unwrap_or_else(|| json!({}));
                        (name, args)
                    } else {
                        (Value::Null, json!({}))
                    };

                parts.push(json!({
                    "type": "tool_use",
                    "id": id,
                    "name": name,
                    "input": args
                }));
            }
        }
    }

    let new_content = match parts.len() {
        0 => Value::Null,
        _ => Value::Array(parts),
    };

    json!({ "role": "assistant", "content": new_content })
}

fn flush_pending_user_blocks(blocks: &mut Vec<Value>, messages: &mut Vec<Value>) {
    if blocks.is_empty() {
        return;
    }

    messages.push(json!({
        "role": "user",
        "content": Value::Array(std::mem::take(blocks))
    }));
}

pub fn transform_openai_messages(messages: Vec<Value>) -> (Option<String>, Vec<Value>) {
    let mut result = Vec::with_capacity(messages.len());
    let mut extracted_system = None;
    let mut pending_user_blocks: Vec<Value> = Vec::new();

    for msg in messages {
        let Value::Object(mut msg_obj) = msg else {
            flush_pending_user_blocks(&mut pending_user_blocks, &mut result);
            result.push(msg);
            continue;
        };

        let role = msg_obj
            .get("role")
            .and_then(|v| v.as_str())
            .unwrap_or("user")
            .to_owned();

        match role.as_str() {
            "system" if extracted_system.is_none() => {
                flush_pending_user_blocks(&mut pending_user_blocks, &mut result);
                let content = msg_obj.remove("content").unwrap_or_default();
                extracted_system = Some(extract_system_text(content));
            }

            "tool" => {
                let tool_use_id = msg_obj.remove("tool_call_id").unwrap_or(Value::Null);
                let content = msg_obj.remove("content").unwrap_or_default();
                pending_user_blocks.push(json!({
                    "type": "tool_result",
                    "tool_use_id": tool_use_id,
                    "content": content
                }));
            }

            "user" => {
                let content = convert_user_content(msg_obj.remove("content"));
                match content {
                    Value::String(s) if !s.is_empty() => {
                        pending_user_blocks.push(json!({"type": "text", "text": s}));
                    }
                    Value::Array(arr) => {
                        pending_user_blocks.extend(arr);
                    }
                    _ => {}
                }
            }

            "assistant" => {
                flush_pending_user_blocks(&mut pending_user_blocks, &mut result);
                result.push(convert_assistant_message(msg_obj));
            }

            _ => {
                flush_pending_user_blocks(&mut pending_user_blocks, &mut result);
                result.push(Value::Object(msg_obj));
            }
        }
    }

    flush_pending_user_blocks(&mut pending_user_blocks, &mut result);

    (extracted_system, result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_user_message() {
        let input = vec![json!({"role": "user", "content": "Hello"})];
        let expected =
            vec![json!({"role": "user", "content": [{"type": "text", "text": "Hello"}]})];
        let (system, result) = transform_openai_messages(input);
        assert!(system.is_none());
        assert_eq!(result, expected);
    }

    #[test]
    fn test_system_extraction() {
        let input = vec![
            json!({"role": "system", "content": "You are helpful."}),
            json!({"role": "user", "content": "Hi"}),
        ];
        let expected = vec![json!({"role": "user", "content": [{"type": "text", "text": "Hi"}]})];
        let (system, result) = transform_openai_messages(input);
        assert_eq!(system, Some("You are helpful.".to_string()));
        assert_eq!(result, expected);
    }

    #[test]
    fn test_assistant_with_tool_calls() {
        let input = vec![json!({
            "role": "assistant",
            "content": "Let me check.",
            "tool_calls": [
                {"id": "call_1", "type": "function", "function": {"name": "get_weather", "arguments": "{\"city\": \"Tokyo\"}"}}
            ]
        })];
        let expected = vec![json!({"role": "assistant", "content": [
            {"type": "text", "text": "Let me check."},
            {"type": "tool_use", "id": "call_1", "name": "get_weather", "input": {"city": "Tokyo"}}
        ]})];
        let (system, result) = transform_openai_messages(input);
        assert!(system.is_none());
        assert_eq!(result, expected);
    }

    #[test]
    fn test_tool_results_merged() {
        let input = vec![
            json!({
                "role": "assistant",
                "content": "Checking...",
                "tool_calls": [
                    {"id": "call_1", "type": "function", "function": {"name": "get_weather", "arguments": "{}"}}
                ]
            }),
            json!({"role": "tool", "tool_call_id": "call_1", "content": "Sunny, 25°C"}),
        ];
        let expected = vec![
            json!({"role": "assistant", "content": [
                {"type": "text", "text": "Checking..."},
                {"type": "tool_use", "id": "call_1", "name": "get_weather", "input": {}}
            ]}),
            json!({"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "call_1", "content": "Sunny, 25°C"}
            ]}),
        ];
        let (system, result) = transform_openai_messages(input);
        assert!(system.is_none());
        assert_eq!(result, expected);
    }

    #[test]
    fn test_multiple_tool_results() {
        let input = vec![
            json!({
                "role": "assistant",
                "content": "Checking both...",
                "tool_calls": [
                    {"id": "call_1", "type": "function", "function": {"name": "get_weather", "arguments": "{}"}},
                    {"id": "call_2", "type": "function", "function": {"name": "get_time", "arguments": "{}"}}
                ]
            }),
            json!({"role": "tool", "tool_call_id": "call_1", "content": "Sunny"}),
            json!({"role": "tool", "tool_call_id": "call_2", "content": "10:00"}),
        ];
        let expected = vec![
            json!({"role": "assistant", "content": [
                {"type": "text", "text": "Checking both..."},
                {"type": "tool_use", "id": "call_1", "name": "get_weather", "input": {}},
                {"type": "tool_use", "id": "call_2", "name": "get_time", "input": {}}
            ]}),
            json!({"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "call_1", "content": "Sunny"},
                {"type": "tool_result", "tool_use_id": "call_2", "content": "10:00"}
            ]}),
        ];
        let (system, result) = transform_openai_messages(input);
        assert!(system.is_none());
        assert_eq!(result, expected);
    }

    #[test]
    fn test_full_conversation_flow() {
        let input = json!([
            {"role": "user", "content": "What's the weather in Tokyo and can you run ls command?"},
            {"role": "assistant", "content": "I'll check both for you.", "tool_calls": [
                {"id": "call_1", "type": "function", "function": {"name": "get_weather", "arguments": "{\"city\": \"Tokyo\"}"}},
                {"id": "call_2", "type": "function", "function": {"name": "exec_command", "arguments": "{\"cmd\": \"ls\"}"}}
            ]},
            {"role": "tool", "tool_call_id": "call_1", "content": "Sunny, 25°C"},
            {"role": "tool", "tool_call_id": "call_2", "content": "total 300\ndrwxr-xr-x  5 user  4096 Apr 17 10:00 ."},
            {"role": "assistant", "content": "The weather in Tokyo is sunny, 25°C. And ls shows the directory has 5 items."},
            {"role": "user", "content": [
                {"type": "text", "text": "Thanks! Here's a screenshot of my terminal:"},
                {"type": "image_url", "image_url": {"url": "https://example.com/screenshot.png"}}
            ]},
            {"role": "assistant", "content": "You're welcome! I can see your terminal screenshot shows the directory listing."}
        ]);
        let input: Vec<Value> = serde_json::from_value(input).unwrap();

        let expected = json!([
            {"role": "user", "content": [
                {"type": "text", "text": "What's the weather in Tokyo and can you run ls command?"}
            ]},
            {"role": "assistant", "content": [
                {"type": "text", "text": "I'll check both for you."},
                {"type": "tool_use", "id": "call_1", "name": "get_weather", "input": {"city": "Tokyo"}},
                {"type": "tool_use", "id": "call_2", "name": "exec_command", "input": {"cmd": "ls"}}
            ]},
            {"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "call_1", "content": "Sunny, 25°C"},
                {"type": "tool_result", "tool_use_id": "call_2", "content": "total 300\ndrwxr-xr-x  5 user  4096 Apr 17 10:00 ."}
            ]},
            {"role": "assistant", "content": [
                {"type": "text", "text": "The weather in Tokyo is sunny, 25°C. And ls shows the directory has 5 items."}
            ]},
            {"role": "user", "content": [
                {"type": "text", "text": "Thanks! Here's a screenshot of my terminal:"},
                {"type": "image", "source": {"type": "url", "url": "https://example.com/screenshot.png"}}
            ]},
            {"role": "assistant", "content": [
                {"type": "text", "text": "You're welcome! I can see your terminal screenshot shows the directory listing."}
            ]}
        ]);
        let expected: Vec<Value> = serde_json::from_value(expected).unwrap();

        let (system, result) = transform_openai_messages(input);
        assert!(system.is_none());
        assert_eq!(result, expected);
    }

    #[test]
    fn test_consecutive_user_messages_merged() {
        let input = vec![
            json!({"role": "user", "content": "Hello"}),
            json!({"role": "user", "content": "World"}),
        ];
        let expected = vec![json!({"role": "user", "content": [
            {"type": "text", "text": "Hello"},
            {"type": "text", "text": "World"}
        ]})];
        let (system, result) = transform_openai_messages(input);
        assert!(system.is_none());
        assert_eq!(result, expected);
    }

    #[test]
    fn test_consecutive_user_messages_with_array_content_merged() {
        let input = vec![
            json!({"role": "user", "content": "Hello"}),
            json!({"role": "user", "content": [
                {"type": "text", "text": "World"},
                {"type": "image_url", "image_url": {"url": "https://example.com/img.png"}}
            ]}),
        ];
        let expected = vec![json!({"role": "user", "content": [
            {"type": "text", "text": "Hello"},
            {"type": "text", "text": "World"},
            {"type": "image", "source": {"type": "url", "url": "https://example.com/img.png"}}
        ]})];
        let (system, result) = transform_openai_messages(input);
        assert!(system.is_none());
        assert_eq!(result, expected);
    }

    #[test]
    fn test_tool_results_and_following_user_messages_merged() {
        let input = vec![
            json!({"role": "assistant", "content": "Let me check.", "tool_calls": [
                {"id": "call_1", "type": "function", "function": {"name": "get_weather", "arguments": "{}"}}
            ]}),
            json!({"role": "tool", "tool_call_id": "call_1", "content": "Sunny"}),
            json!({"role": "user", "content": "Thanks!"}),
            json!({"role": "user", "content": "Continue"}),
            json!({"role": "user", "content": [{"type": "text", "text": "Continue"}]}),
            json!({"role": "user", "content": [{"type": "text", "text": "Continue"}, {"type": "image_url", "image_url": {"url": "https://example.com/cont.png"}}]}),
        ];
        let expected = vec![
            json!({"role": "assistant", "content": [
                {"type": "text", "text": "Let me check."},
                {"type": "tool_use", "id": "call_1", "name": "get_weather", "input": {}}
            ]}),
            json!({"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "call_1", "content": "Sunny"},
                {"type": "text", "text": "Thanks!"},
                {"type": "text", "text": "Continue"},
                {"type": "text", "text": "Continue"},
                {"type": "text", "text": "Continue"},
                {"type": "image", "source": {"type": "url", "url": "https://example.com/cont.png"}}
            ]}),
        ];
        let (system, result) = transform_openai_messages(input);
        assert!(system.is_none());
        assert_eq!(result, expected);
    }

    #[test]
    fn test_non_consecutive_user_messages_not_merged() {
        let input = vec![
            json!({"role": "user", "content": "First"}),
            json!({"role": "assistant", "content": "Reply"}),
            json!({"role": "user", "content": "Second"}),
        ];
        let expected = vec![
            json!({"role": "user", "content": [{"type": "text", "text": "First"}]}),
            json!({"role": "assistant", "content": [{"type": "text", "text": "Reply"}]}),
            json!({"role": "user", "content": [{"type": "text", "text": "Second"}]}),
        ];
        let (system, result) = transform_openai_messages(input);
        assert!(system.is_none());
        assert_eq!(result, expected);
    }

    #[test]
    fn test_assistant_reasoning_content() {
        let cases = vec![
            (
                json!({"role": "assistant", "content": "The answer is 42.", "reasoning_content": "Let me think..."}),
                json!({"role": "assistant", "content": [
                    {"type": "thinking", "thinking": "Let me think..."},
                    {"type": "text", "text": "The answer is 42."}
                ]}),
                "thinking + text",
            ),
            (
                json!({"role": "assistant", "content": "Hello!", "reasoning_content": ""}),
                json!({"role": "assistant", "content": [
                    {"type": "text", "text": "Hello!"}
                ]}),
                "empty reasoning filtered out",
            ),
            (
                json!({"role": "assistant", "reasoning_content": "Thinking about the problem..."}),
                json!({"role": "assistant", "content": [
                    {"type": "thinking", "thinking": "Thinking about the problem..."}
                ]}),
                "only reasoning, no content",
            ),
        ];

        for (input_msg, expected_msg, description) in cases {
            let (system, result) = transform_openai_messages(vec![input_msg]);
            assert!(system.is_none());
            assert_eq!(result, vec![expected_msg], "{}", description);
        }
    }

    #[test]
    fn test_assistant_with_reasoning_content_and_tool_calls() {
        let input = vec![json!({
            "role": "assistant",
            "content": "Let me search for that.",
            "reasoning_content": "I need to find information about the weather.",
            "tool_calls": [
                {"id": "call_1", "type": "function", "function": {"name": "web_search", "arguments": "{\"query\": \"weather Tokyo\"}"}}
            ]
        })];
        let expected = vec![json!({"role": "assistant", "content": [
            {"type": "thinking", "thinking": "I need to find information about the weather."},
            {"type": "text", "text": "Let me search for that."},
            {"type": "tool_use", "id": "call_1", "name": "web_search", "input": {"query": "weather Tokyo"}}
        ]})];
        let (system, result) = transform_openai_messages(input);
        assert!(system.is_none());
        assert_eq!(result, expected);
    }
}
