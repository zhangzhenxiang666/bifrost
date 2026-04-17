//! OpenAI ↔ Anthropic format conversion utilities
//!
//! This module provides bidirectional conversion between OpenAI Chat Completions
//! and Anthropic Messages API formats.
//!
//! ## Submodules
//!
//! - `request` - Transform OpenAI requests to Anthropic format
//! - `response` - Transform Anthropic responses to OpenAI format
//! - `message` - Message-level conversion utilities
//! - `stream` - Streaming response conversion (Anthropic → OpenAI event stream)

pub mod message;
pub mod request;
pub mod response;
pub mod stream;
