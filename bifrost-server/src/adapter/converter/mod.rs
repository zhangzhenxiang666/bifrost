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

#[cfg(test)]
pub(crate) mod stream_test_utils;

pub fn create_null() -> serde_json::Value {
    serde_json::Value::Null
}

pub fn create_null_string() -> serde_json::Value {
    serde_json::Value::String("".into())
}
