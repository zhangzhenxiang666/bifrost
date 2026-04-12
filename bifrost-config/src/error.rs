//! Configuration error types

use thiserror::Error;

/// Error type for configuration-related failures
#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("Configuration error: {0}")]
    InvalidConfig(String),

    #[error("Failed to read config file: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Failed to parse TOML: {0}")]
    TomlParseError(String),
}

impl From<toml::de::Error> for ConfigError {
    fn from(e: toml::de::Error) -> Self {
        ConfigError::TomlParseError(e.message().to_string())
    }
}
