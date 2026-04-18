//! Built-in adapters for LLM providers
//!
//! This module provides built-in adapter implementations that can be used
//! out of the box without custom implementation.

pub mod anthropic_to_openai;
pub mod openai_to_anthropic;
pub mod passthrough;
pub mod responses_to_chat;

pub use anthropic_to_openai::AnthropicToOpenAIAdapter;
pub use openai_to_anthropic::OpenAIToAnthropicAdapter;
pub use passthrough::PassthroughAdapter;
pub use responses_to_chat::ResponsesToChatAdapter;
