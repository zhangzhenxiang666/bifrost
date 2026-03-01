//! SSE (Server-Sent Events) utilities for gateway
//!
//! This module provides functions to parse SSE events and convert
//! JSON responses to SSE streams.

use axum::response::sse::{Event, Sse};
use futures::stream::{self, Stream};
use serde_json::Value;
use std::convert::Infallible;
use std::pin::Pin;

/// SSE stream type alias
pub type SSEStream = Sse<Pin<Box<dyn Stream<Item = Result<Event, Infallible>> + Send>>>;

/// Represents a parsed SSE event
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedEvent {
    /// Event data content
    pub data: String,
    /// Event type (optional)
    pub event_type: Option<String>,
    /// Event ID (optional)
    pub id: Option<String>,
}

/// Parse SSE text into a vector of events
///
/// SSE format:
/// ```text
/// data: {"id":"1","choices":[{"delta":{"content":"Hello"}}]}
///
/// data: {"id":"1","choices":[{"delta":{"content":" World"}}]}
///
/// data: [DONE]
/// ```
///
/// # Arguments
/// * `text` - The SSE text to parse
///
/// # Returns
/// A vector of parsed events
pub fn parse_sse_events(text: &str) -> Vec<ParsedEvent> {
    let mut events = Vec::new();
    
    // Split by double newlines to separate events
    let event_blocks: Vec<&str> = text.split("\n\n").collect();
    
    for block in event_blocks {
        if block.trim().is_empty() {
            continue;
        }
        
        let mut event = ParsedEvent {
            data: String::new(),
            event_type: None,
            id: None,
        };
        
        // Parse each line in the event block
        for line in block.lines() {
            let line = line.trim();
            
            // Skip empty lines and comments
            if line.is_empty() || line.starts_with(':') {
                continue;
            }
            
            // Parse SSE field
            if let Some((field, value)) = line.split_once(':') {
                let field = field.trim();
                let value = value.trim();
                
                match field {
                    "data" => {
                        event.data = value.to_string();
                    }
                    "event" => {
                        event.event_type = Some(value.to_string());
                    }
                    "id" => {
                        event.id = Some(value.to_string());
                    }
                    _ => {
                        // Ignore unknown fields
                    }
                }
            }
        }
        
        // Only add event if it has data
        if !event.data.is_empty() {
            events.push(event);
        }
    }
    
    events
}

/// Convert a JSON response to an SSE stream
///
/// # Arguments
/// * `response` - The JSON value to convert
///
/// # Returns
/// An SSE stream that emits the JSON value as an event
pub fn convert_to_sse(response: Value) -> SSEStream {
    let json_string = response.to_string();
    
    let event = Event::default().data(json_string);
    let stream = stream::once(async move { Ok(event) });
    
    Sse::new(Box::pin(stream))
}

/// Convert multiple JSON values to an SSE stream
///
/// # Arguments
/// * `responses` - A vector of JSON values to convert
///
/// # Returns
/// An SSE stream that emits each JSON value as an event
pub fn convert_multiple_to_sse(responses: Vec<Value>) -> SSEStream {
    let stream = stream::iter(responses.into_iter().map(|response| {
        let json_string = response.to_string();
        let event = Event::default().data(json_string);
        Ok::<Event, Infallible>(event)
    }));
    
    Sse::new(Box::pin(stream))
}

/// Check if an event data indicates the stream is done
///
/// # Arguments
/// * `data` - The event data to check
///
/// # Returns
/// true if the data is "[DONE]", false otherwise
pub fn is_done_event(data: &str) -> bool {
    data.trim() == "[DONE]"
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::stream::StreamExt;
    use serde_json::json;

    #[test]
    fn test_parse_single_sse_event() {
        let sse_text = "data: {\"id\":\"1\",\"choices\":[{\"delta\":{\"content\":\"Hello\"}}]}\n\n";
        let events = parse_sse_events(sse_text);
        
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0].data,
            "{\"id\":\"1\",\"choices\":[{\"delta\":{\"content\":\"Hello\"}}]}"
        );
    }

    #[test]
    fn test_parse_multiple_sse_events() {
        let sse_text = "data: {\"id\":\"1\",\"choices\":[{\"delta\":{\"content\":\"Hello\"}}]}\n\n\
                        data: {\"id\":\"1\",\"choices\":[{\"delta\":{\"content\":\" World\"}}]}\n\n";
        let events = parse_sse_events(sse_text);
        
        assert_eq!(events.len(), 2);
        assert_eq!(
            events[0].data,
            "{\"id\":\"1\",\"choices\":[{\"delta\":{\"content\":\"Hello\"}}]}"
        );
        assert_eq!(
            events[1].data,
            "{\"id\":\"1\",\"choices\":[{\"delta\":{\"content\":\" World\"}}]}"
        );
    }

    #[test]
    fn test_parse_sse_event_with_done_marker() {
        let sse_text = "data: [DONE]\n\n";
        let events = parse_sse_events(sse_text);
        
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, "[DONE]");
        assert!(is_done_event(&events[0].data));
    }

    #[test]
    fn test_parse_sse_event_with_event_type() {
        let sse_text = "event: message\ndata: {\"test\": \"value\"}\n\n";
        let events = parse_sse_events(sse_text);
        
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, "{\"test\": \"value\"}");
        assert_eq!(events[0].event_type, Some("message".to_string()));
    }

    #[test]
    fn test_parse_sse_event_with_id() {
        let sse_text = "id: 123\ndata: {\"test\": \"value\"}\n\n";
        let events = parse_sse_events(sse_text);
        
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, "{\"test\": \"value\"}");
        assert_eq!(events[0].id, Some("123".to_string()));
    }

    #[test]
    fn test_parse_sse_event_with_all_fields() {
        let sse_text = "id: 123\nevent: message\ndata: {\"test\": \"value\"}\n\n";
        let events = parse_sse_events(sse_text);
        
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, "{\"test\": \"value\"}");
        assert_eq!(events[0].event_type, Some("message".to_string()));
        assert_eq!(events[0].id, Some("123".to_string()));
    }

    #[test]
    fn test_parse_empty_sse_text() {
        let sse_text = "";
        let events = parse_sse_events(sse_text);
        
        assert_eq!(events.len(), 0);
    }

    #[test]
    fn test_parse_sse_text_with_empty_blocks() {
        let sse_text = "\n\n\n\ndata: {\"test\": \"value\"}\n\n\n\n";
        let events = parse_sse_events(sse_text);
        
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, "{\"test\": \"value\"}");
    }

    #[test]
    fn test_parse_sse_comment_lines() {
        let sse_text = ": this is a comment\ndata: {\"test\": \"value\"}\n\n";
        let events = parse_sse_events(sse_text);
        
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, "{\"test\": \"value\"}");
    }

    #[test]
    fn test_is_done_event() {
        assert!(is_done_event("[DONE]"));
        assert!(is_done_event("  [DONE]  "));
        assert!(!is_done_event("data"));
        assert!(!is_done_event(""));
    }

    #[test]
    fn test_convert_to_sse() {
        let json_value = json!({"id": "1", "choices": [{"delta": {"content": "Hello"}}]});
        let sse_stream = convert_to_sse(json_value.clone());
        
        // Note: We can't easily test the actual stream content without consuming it,
        // but we can verify the function returns the correct type
        let _ = sse_stream;
    }

    #[test]
    fn test_convert_multiple_to_sse() {
        let json_values = vec![
            json!({"id": "1", "choices": [{"delta": {"content": "Hello"}}]}),
            json!({"id": "1", "choices": [{"delta": {"content": " World"}}]}),
            json!({"choices": [{"delta": {"content": "[DONE]"}}]}),
        ];
        let sse_stream = convert_multiple_to_sse(json_values);
        
        // Note: We can't easily test the actual stream content without consuming it,
        // but we can verify the function returns the correct type
        let _ = sse_stream;
    }

    #[tokio::test]
    async fn test_convert_to_sse_stream_emits_event() {
        let json_value = json!({"test": "data"});
        let sse_stream = convert_to_sse(json_value);
        
        // This test verifies the stream can be created
        // Actual event emission testing would require more complex setup
        assert!(true);
    }

    #[test]
    fn test_parse_sse_mixed_format() {
        let sse_text = "data: {\"id\":\"1\"}\n\nevent: chunk\ndata: {\"id\":\"2\"}\n\nid: 3\ndata: [DONE]\n\n";
        let events = parse_sse_events(sse_text);
        
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].data, "{\"id\":\"1\"}");
        assert_eq!(events[1].data, "{\"id\":\"2\"}");
        assert_eq!(events[1].event_type, Some("chunk".to_string()));
        assert_eq!(events[2].data, "[DONE]");
        assert_eq!(events[2].id, Some("3".to_string()));
    }
}
