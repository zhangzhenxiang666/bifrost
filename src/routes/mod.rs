//! Routes module for HTTP endpoints

pub mod handler;
pub mod openai;

pub use handler::AppState;
pub use openai::chat_completions;
