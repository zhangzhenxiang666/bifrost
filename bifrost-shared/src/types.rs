//! Configuration types for Bifrost

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Provider endpoint type
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Endpoint {
    #[default]
    OpenAI,
    Anthropic,
}

impl Endpoint {
    /// Check if this is an OpenAI-compatible endpoint
    pub fn is_openai(&self) -> bool {
        matches!(self, Endpoint::OpenAI)
    }

    /// Check if this is an Anthropic endpoint
    pub fn is_anthropic(&self) -> bool {
        matches!(self, Endpoint::Anthropic)
    }
}

impl std::fmt::Display for Endpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Endpoint::OpenAI => write!(f, "openai"),
            Endpoint::Anthropic => write!(f, "anthropic"),
        }
    }
}

/// Header key-value pair
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HeaderEntry {
    pub name: String,
    pub value: String,
}

/// Body key-value pair
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BodyEntry {
    pub name: String,
    pub value: serde_json::Value,
}

/// Complex mapping configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MappingConfig {
    pub target: String,
    #[serde(default)]
    pub headers: Option<Vec<HeaderEntry>>,
    #[serde(default)]
    pub body: Option<Vec<BodyEntry>>,
}

/// Single mapping entry
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum AliasEntry {
    Simple(String),
    Complex(MappingConfig),
}

/// Model-specific configuration
#[derive(Debug, Clone, Deserialize)]
pub struct ModelConfig {
    pub name: String,
    #[serde(default)]
    pub headers: Option<Vec<HeaderEntry>>,
    #[serde(default)]
    pub body: Option<Vec<BodyEntry>>,
}

/// Provider configuration
#[derive(Debug, Clone, Deserialize)]
pub struct ProviderConfig {
    pub base_url: String,
    pub api_key: String,
    pub endpoint: Endpoint,
    #[serde(default)]
    pub headers: Option<Vec<HeaderEntry>>,
    #[serde(default)]
    pub body: Option<Vec<BodyEntry>>,
    #[serde(default)]
    pub models: Option<Vec<ModelConfig>>,
    #[serde(default)]
    pub exclude_headers: Option<Vec<String>>,
    #[serde(default)]
    pub extend: bool,
}

/// Server configuration
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ServerConfig {
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default)]
    pub proxy: Option<String>,
    #[serde(default)]
    pub timeout_secs: Option<u64>,
    #[serde(default)]
    pub max_retries: Option<u32>,
    #[serde(default)]
    pub retry_backoff_base_ms: Option<u64>,
}

fn default_port() -> u16 {
    5564
}

/// Root configuration structure
#[derive(Debug, Clone, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub provider: HashMap<String, ProviderConfig>,
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub alias: HashMap<String, AliasEntry>,
}

impl Config {
    /// Load configuration from a TOML file
    pub fn from_file<P: AsRef<std::path::Path>>(path: P) -> Result<Self, crate::ConfigError> {
        let content = std::fs::read_to_string(path.as_ref())?;
        Self::from_toml(&content)
    }

    /// Parse TOML content into Config
    pub fn from_toml(content: &str) -> Result<Self, crate::ConfigError> {
        Ok(toml::from_str(content)?)
    }

    /// Validate the configuration
    pub fn validate(&self) -> Result<(), crate::ConfigError> {
        if self.server.port == 0 {
            return Err(crate::ConfigError::InvalidConfig(
                "Server port cannot be 0".into(),
            ));
        }
        for (provider_id, provider) in &self.provider {
            if provider.api_key.trim().is_empty() {
                return Err(crate::ConfigError::InvalidConfig(format!(
                    "Provider '{}' has an empty api_key",
                    provider_id
                )));
            }
            if provider.base_url.trim().is_empty() {
                return Err(crate::ConfigError::InvalidConfig(format!(
                    "Provider '{}' has an empty base_url",
                    provider_id
                )));
            }
            if !provider.base_url.starts_with("http://")
                && !provider.base_url.starts_with("https://")
            {
                return Err(crate::ConfigError::InvalidConfig(format!(
                    "Provider '{}' has an invalid base_url format: '{}'",
                    provider_id, provider.base_url
                )));
            }
        }
        Ok(())
    }
}
