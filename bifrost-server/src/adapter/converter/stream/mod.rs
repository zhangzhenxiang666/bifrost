//! Streaming response conversion utilities
//!
//! This module provides utilities for converting streaming responses between formats.
//! Currently supports OpenAI → Anthropic stream event conversion.

mod state;

pub mod processor;

pub use state::OpenAIStreamState;
pub use processor::OpenAIStreamProcessor;
