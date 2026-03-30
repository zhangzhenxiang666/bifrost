//! Anthropic ↔ OpenAI format conversion utilities
//!
//! This module provides bidirectional conversion between Anthropic and OpenAI API formats.
//!
//! ## Submodules
//!
//! - `request` - Transform Anthropic requests to OpenAI format
//! - `response` - Transform OpenAI responses to Anthropic format
//! - `message` - Message-level conversion utilities

pub mod message;
pub mod request;
pub mod response;
