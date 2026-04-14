//! Built-in adapters for LLM providers
//!
//! This module provides built-in adapter implementations that can be used
//! out of the box without custom implementation.
//!
//! This module provides built-in adapter implementations that can be used
//! out of the box without custom implementation.

// pub mod anthropic_openai;
pub mod anthropic_to_openai;
pub mod anthropic_to_qwen;
pub mod openai_to_qwen;
pub mod passthrough;
pub mod responses_to_chat;

pub use anthropic_to_openai::AnthropicToOpenAIAdapter;
pub use anthropic_to_qwen::AnthropicToQwenAdapter;
pub use openai_to_qwen::OpenAIToQwenAdapter;
pub use passthrough::PassthroughAdapter;
pub use responses_to_chat::ResponsesToChatAdapter;
