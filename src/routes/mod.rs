//! Routes module for HTTP endpoints

pub mod openai;

pub use openai::{chat_completions, AppState};
