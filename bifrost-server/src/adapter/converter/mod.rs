//! Converter module for LLM provider format transformations
//!
//! This module provides shared conversion logic for transforming requests and responses
//! between different LLM provider formats (Anthropic, OpenAI, etc.).
//!
//! ## Module Structure
//!
//! - `anthropic_openai` - Bidirectional conversion between Anthropic and OpenAI formats
//!   - `stream` - Streaming response conversion (OpenAI → Anthropic event stream)
//! - `openai_responses` - OpenAI Responses API conversion

pub mod anthropic_openai;
pub mod openai_anthropic;
pub mod openai_responses;

pub fn create_null() -> serde_json::Value {
    serde_json::Value::Null
}

pub fn create_null_string() -> serde_json::Value {
    serde_json::Value::String("".into())
}

/// Moves the specified fields from `obj` into `result` if they exist.
pub fn extract_passthrough_fields(
    obj: &mut serde_json::Map<String, serde_json::Value>,
    result: &mut serde_json::Map<String, serde_json::Value>,
    fields: &[&str],
) {
    for field in fields {
        if let Some(value) = obj.remove(*field) {
            result.insert(field.to_string(), value);
        }
    }
}
