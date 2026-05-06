//! Transform OpenAI requests to Anthropic format

use crate::error::LlmMapError;
use serde_json::{Value, json};

use super::message::transform_openai_messages;

pub fn transform_request(body: Value) -> Result<Value, LlmMapError> {
    let Value::Object(mut obj) = body else {
        return Err(LlmMapError::Validation(
            "Request body must be an object".into(),
        ));
    };

    if let Some(reasoning_effort) = obj.remove("reasoning_effort") {
        let effort = match reasoning_effort.as_str() {
            Some("low") => "low",
            Some("medium") => "medium",
            Some("high") => "high",
            Some("xhigh") => "max",
            _ => "medium",
        };
        obj.insert("output_config".to_string(), json!({ "effort": effort }));
    }

    if let Some(Value::Array(tools_arr)) = obj.remove("tools") {
        let transformed_tools = transform_tools(tools_arr);
        obj.insert("tools".to_string(), transformed_tools);
    }

    if let Some(tool_choice) = obj.remove("tool_choice") {
        let transformed_tool_choice = transform_tool_choice(tool_choice);
        obj.insert("tool_choice".to_string(), transformed_tool_choice);
    }

    let messages = obj.remove("messages");
    let (system_text, transformed_messages) = match messages {
        Some(Value::Array(msgs)) => transform_openai_messages(msgs),
        _ => (None, Vec::new()),
    };

    if let Some(system) = system_text {
        obj.insert("system".to_string(), Value::String(system));
    }

    obj.insert("messages".to_string(), Value::Array(transformed_messages));

    if !obj.contains_key("max_tokens") {
        if let Some(max_completion_tokens) = obj.remove("max_completion_tokens") {
            obj.insert("max_tokens".to_string(), max_completion_tokens);
        } else {
            obj.insert("max_tokens".to_string(), Value::Number(4096.into()));
        }
    } else {
        obj.remove("max_completion_tokens");
    }

    if !obj.contains_key("max_tokens") {
        obj.insert("max_tokens".to_string(), Value::Number(4096.into()));
    }

    Ok(Value::Object(obj))
}

fn transform_tools(tools: Vec<Value>) -> Value {
    let transformed: Vec<Value> = tools
        .into_iter()
        .filter_map(|tool| {
            let Value::Object(mut obj) = tool else {
                return None;
            };

            if obj.get("type").and_then(|t| t.as_str()) != Some("function") {
                return None;
            }

            let function = obj.remove("function")?;
            let Value::Object(mut func_obj) = function else {
                return None;
            };

            let name = func_obj.remove("name")?;
            let description = func_obj.remove("description");
            let parameters = func_obj.remove("parameters");

            let mut result = serde_json::Map::new();
            result.insert("name".to_string(), name);
            if let Some(d) = description {
                result.insert("description".to_string(), d);
            }
            if let Some(p) = parameters {
                result.insert("input_schema".to_string(), p);
            }

            Some(Value::Object(result))
        })
        .collect();

    Value::Array(transformed)
}

fn transform_tool_choice(tool_choice: Value) -> Value {
    match tool_choice {
        Value::String(s) => match s.as_str() {
            "required" => json!({"type": "any"}),
            "none" => json!({"type": "none"}),
            "auto" => json!({"type": "auto"}),
            _ => json!({"type": "auto"}),
        },
        Value::Object(obj) => {
            let t = obj.get("type").and_then(|v| v.as_str()).unwrap_or("auto");
            if t == "function" {
                let name = obj
                    .get("function")
                    .and_then(|f| f.get("name"))
                    .cloned()
                    .unwrap_or(Value::Null);
                json!({"type": "tool", "name": name})
            } else {
                json!({"type": t})
            }
        }
        _ => json!({"type": "auto"}),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_non_object_body_error() {
        let input = json!("not an object");
        let result = transform_request(input);
        assert!(result.is_err());
    }

    #[test]
    fn test_reasoning_effort() {
        let cases = [
            ("low", "low"),
            ("medium", "medium"),
            ("high", "high"),
            ("xhigh", "max"),
            ("unknown", "medium"),
        ];
        for (input_effort, expected_effort) in cases {
            let input = json!({
                "model": "gpt-4o",
                "messages": [{"role": "user", "content": "Hello"}],
                "reasoning_effort": input_effort
            });

            let expected = json!({
                "model": "gpt-4o",
                "messages": [{"role": "user", "content": [{"type": "text", "text": "Hello"}]}],
                "output_config": {"effort": expected_effort},
                "max_tokens": 4096
            });

            let result = transform_request(input).unwrap();
            assert_eq!(result, expected, "reasoning_effort: {}", input_effort);
        }
    }

    #[test]
    fn test_max_tokens_handling() {
        // max_completion_tokens maps to max_tokens when max_tokens is absent
        let input = json!({
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": "Hello"}],
            "max_completion_tokens": 2048
        });

        let expected = json!({
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": [{"type": "text", "text": "Hello"}]}],
            "max_tokens": 2048
        });

        assert_eq!(transform_request(input).unwrap(), expected);

        // max_tokens takes priority over max_completion_tokens
        let input = json!({
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": "Hello"}],
            "max_tokens": 1024,
            "max_completion_tokens": 2048
        });

        let expected = json!({
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": [{"type": "text", "text": "Hello"}]}],
            "max_tokens": 1024
        });

        assert_eq!(transform_request(input).unwrap(), expected);

        // Default max_tokens when neither is specified
        let input = json!({
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let expected = json!({
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": [{"type": "text", "text": "Hello"}]}],
            "max_tokens": 4096
        });

        assert_eq!(transform_request(input).unwrap(), expected);
    }

    #[test]
    fn test_tools_transform() {
        let input = json!({
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": "Hello"}],
            "tools": [{
                "type": "function",
                "function": {
                    "name": "get_weather",
                    "description": "Get weather for a city",
                    "parameters": {"type": "object", "properties": {"city": {"type": "string"}}}
                }
            }]
        });

        let expected = json!({
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": [{"type": "text", "text": "Hello"}]}],
            "tools": [{
                "name": "get_weather",
                "description": "Get weather for a city",
                "input_schema": {"type": "object", "properties": {"city": {"type": "string"}}}
            }],
            "max_tokens": 4096
        });

        let result = transform_request(input).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_tool_choice() {
        // String variants
        let cases = [
            ("required", json!({"type": "any"})),
            ("none", json!({"type": "none"})),
            ("auto", json!({"type": "auto"})),
            ("unknown", json!({"type": "auto"})),
        ];
        for (choice, expected_choice) in cases {
            let input = json!({
                "model": "gpt-4o",
                "messages": [{"role": "user", "content": "Hello"}],
                "tool_choice": choice
            });

            let expected = json!({
                "model": "gpt-4o",
                "messages": [{"role": "user", "content": [{"type": "text", "text": "Hello"}]}],
                "tool_choice": expected_choice,
                "max_tokens": 4096
            });

            let result = transform_request(input).unwrap();
            assert_eq!(result, expected, "tool_choice string: {}", choice);
        }

        // Function object variant
        let input = json!({
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": "Hello"}],
            "tool_choice": {"type": "function", "function": {"name": "get_weather"}}
        });

        let expected = json!({
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": [{"type": "text", "text": "Hello"}]}],
            "tool_choice": {"type": "tool", "name": "get_weather"},
            "max_tokens": 4096
        });

        assert_eq!(transform_request(input).unwrap(), expected);
    }

    #[test]
    fn test_full_conversation_integration() {
        // Smoke test covering all orchestration features
        let input = json!({
            "model": "gpt-4o",
            "messages": [
                {"role": "system", "content": "You are a helpful assistant."},
                {"role": "user", "content": "Hello"}
            ],
            "tools": [{
                "type": "function",
                "function": {
                    "name": "get_weather",
                    "parameters": {"type": "object", "properties": {"city": {"type": "string"}}}
                }
            }],
            "tool_choice": "required",
            "reasoning_effort": "high",
            "temperature": 0.7,
            "stream": false
        });

        let expected = json!({
            "model": "gpt-4o",
            "system": "You are a helpful assistant.",
            "messages": [{"role": "user", "content": [{"type": "text", "text": "Hello"}]}],
            "tools": [{
                "name": "get_weather",
                "input_schema": {"type": "object", "properties": {"city": {"type": "string"}}}
            }],
            "tool_choice": {"type": "any"},
            "output_config": {"effort": "high"},
            "max_tokens": 4096,
            "temperature": 0.7,
            "stream": false
        });

        let result = transform_request(input).unwrap();
        assert_eq!(result, expected);
    }
}
