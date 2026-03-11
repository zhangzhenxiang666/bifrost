//! Converter module for LLM provider format transformations
//!
//! This module provides shared conversion logic for transforming requests and responses
//! between different LLM provider formats (Anthropic, OpenAI, Qwen, etc.).
//!
//! ## Module Structure
//!
//! - `anthropic_openai` - Bidirectional conversion between Anthropic and OpenAI formats
//! - `stream` - Streaming response conversion (OpenAI → Anthropic event stream)
//! - `qwen` - Qwen-specific utilities (OAuth, headers)

pub mod anthropic_openai;
pub mod qwen;
pub mod stream;

pub use anthropic_openai::*;
pub use qwen::*;
pub use stream::*;
