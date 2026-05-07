#![cfg(test)]

use serde_json::Value;
use std::io;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedSseEvent {
    pub event: String,
    pub data: NormalizedSseData,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NormalizedSseData {
    Done,
    Json(Value),
    Text(String),
}

pub fn load_sse_fixture(path: &str) -> io::Result<Vec<NormalizedSseEvent>> {
    let content = std::fs::read_to_string(sse_fixture_path(path))?;
    Ok(parse_sse_events(&content))
}

pub fn normalize_stream_events(events: Vec<(Value, Option<String>)>) -> Vec<NormalizedSseEvent> {
    events
        .into_iter()
        .map(|(data, event)| NormalizedSseEvent {
            event: event.unwrap_or_default(),
            data: NormalizedSseData::Json(scrub_dynamic_json(data)),
        })
        .collect()
}

pub fn parse_sse_events(content: &str) -> Vec<NormalizedSseEvent> {
    content
        .split("\n\n")
        .filter_map(|raw_event| parse_sse_event(raw_event.trim_end_matches('\r')))
        .collect()
}

fn sse_fixture_path(path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("testdata")
        .join("sse")
        .join(path)
}

fn parse_sse_event(raw_event: &str) -> Option<NormalizedSseEvent> {
    if raw_event.trim().is_empty() {
        return None;
    }

    let mut event = String::new();
    let mut data = String::new();

    for line in raw_event.lines() {
        let line = line.trim_end_matches('\r');
        if let Some(event_value) = line.strip_prefix("event:") {
            event = trim_optional_space(event_value).to_string();
        } else if let Some(data_value) = line.strip_prefix("data:") {
            if !data.is_empty() {
                data.push('\n');
            }
            data.push_str(trim_optional_space(data_value));
        }
    }

    if data.is_empty() {
        return None;
    }

    Some(NormalizedSseEvent {
        event,
        data: normalize_sse_data(&data),
    })
}

fn normalize_sse_data(data: &str) -> NormalizedSseData {
    if data == "[DONE]" {
        return NormalizedSseData::Done;
    }

    serde_json::from_str(data)
        .map(scrub_dynamic_json)
        .map(NormalizedSseData::Json)
        .unwrap_or_else(|_| NormalizedSseData::Text(data.to_string()))
}

fn scrub_dynamic_json(mut value: Value) -> Value {
    scrub_dynamic_value(&mut value);
    value
}

fn scrub_dynamic_value(value: &mut Value) {
    match value {
        Value::Object(object) => {
            for (key, value) in object {
                if key == "created" || key == "created_at" {
                    *value = Value::String("<dynamic_timestamp>".to_string());
                } else if key == "output"
                    && let Value::Array(items) = value
                {
                    for item in items.iter_mut() {
                        scrub_dynamic_value(item);
                    }
                    items.sort_by_key(output_item_sort_key);
                } else {
                    scrub_dynamic_value(value);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                scrub_dynamic_value(item);
            }
        }
        Value::String(text) if text.starts_with("item_") => {
            *text = "<dynamic_item_id>".to_string();
        }
        _ => {}
    }
}

fn output_item_sort_key(item: &Value) -> (u8, String) {
    let item_type = item
        .get("type")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let rank = match item_type {
        "reasoning" => 0,
        "message" => 1,
        "function_call" => 2,
        _ => 3,
    };
    let id = item
        .get("id")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();
    (rank, id)
}

fn trim_optional_space(value: &str) -> &str {
    value.strip_prefix(' ').unwrap_or(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_events_with_json_and_done_data() {
        let events = parse_sse_events(
            r#"event: message_start
data: {"type":"message_start"}

data: [DONE]
"#,
        );

        assert_eq!(
            events,
            vec![
                NormalizedSseEvent {
                    event: "message_start".to_string(),
                    data: NormalizedSseData::Json(json!({ "type": "message_start" })),
                },
                NormalizedSseEvent {
                    event: String::new(),
                    data: NormalizedSseData::Done,
                },
            ]
        );
    }

    #[test]
    fn normalizes_converter_output_events() {
        let events = normalize_stream_events(vec![(
            json!({ "type": "response.output_text.delta" }),
            Some("response.output_text.delta".to_string()),
        )]);

        assert_eq!(
            events,
            vec![NormalizedSseEvent {
                event: "response.output_text.delta".to_string(),
                data: NormalizedSseData::Json(json!({ "type": "response.output_text.delta" })),
            }]
        );
    }

    #[test]
    fn scrubs_dynamic_timestamps_and_item_ids() {
        let events = normalize_stream_events(vec![(
            json!({
                "created": 1712530587,
                "created_at": 1712530588,
                "item_id": "item_abcdefghijklmnop"
            }),
            None,
        )]);

        assert_eq!(
            events,
            vec![NormalizedSseEvent {
                event: String::new(),
                data: NormalizedSseData::Json(json!({
                    "created": "<dynamic_timestamp>",
                    "created_at": "<dynamic_timestamp>",
                    "item_id": "<dynamic_item_id>"
                })),
            }]
        );
    }
}
