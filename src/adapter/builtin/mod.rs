//! Built-in adapters for LLM Map
//!
//! This module provides built-in adapter implementations that can be used
//! out of the box without custom implementation.

pub mod openai_to_qwen;
pub mod passthrough;

pub use openai_to_qwen::OpenAIToQwenAdapter;
pub use passthrough::PassthroughAdapter;
