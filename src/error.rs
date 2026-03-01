//! Error types for LLM Map service

use thiserror::Error;

#[derive(Error, Debug)]
pub enum LlmMapError {
    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Provider error: {0}")]
    Provider(String),

    #[error("Adapter error: {0}")]
    Adapter(String),

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("Internal error: {0}")]
    Internal(#[from] anyhow::Error),
}

pub type Result<T> = std::result::Result<T, LlmMapError>;
