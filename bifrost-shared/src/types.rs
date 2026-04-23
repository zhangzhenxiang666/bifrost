//! Configuration types for Bifrost

use serde::{Deserialize, Deserializer, Serialize};
use std::collections::HashMap;

/// Body field transformation policy for provider request conversion.
///
/// This policy is applied at the end of the adapter's request converter
/// to determine how unprocessed (unknown) fields from the original request
/// should be handled.
///
/// TOML examples:
/// ```toml
/// body_policy = "drop_unknown"
/// body_policy = { allowlist = ["temperature", "top_p"] }
/// body_policy = { blocklist = ["prediction"] }
/// ```
#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BodyTransformPolicy {
    /// Preserve all unprocessed fields (default).
    #[default]
    PreserveUnknown,
    /// Drop all unprocessed fields.
    DropUnknown,
    /// Keep only the specified fields plus the converter's output.
    Allowlist(Vec<String>),
    /// Drop the specified fields, keep everything else.
    Blocklist(Vec<String>),
}

impl<'de> Deserialize<'de> for BodyTransformPolicy {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Inner {
            Simple(String),
            Allowlist { allowlist: Vec<String> },
            Blocklist { blocklist: Vec<String> },
        }

        match Inner::deserialize(deserializer)? {
            Inner::Simple(s) => match s.as_str() {
                "preserve_unknown" => Ok(BodyTransformPolicy::PreserveUnknown),
                "drop_unknown" => Ok(BodyTransformPolicy::DropUnknown),
                _ => Err(serde::de::Error::unknown_variant(
                    &s,
                    &["preserve_unknown", "drop_unknown"],
                )),
            },
            Inner::Allowlist { allowlist: fields } => Ok(BodyTransformPolicy::Allowlist(fields)),
            Inner::Blocklist { blocklist: fields } => Ok(BodyTransformPolicy::Blocklist(fields)),
        }
    }
}

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

fn protected_fields_list() -> String {
    PROTECTED_BODY_FIELDS
        .iter()
        .map(|f| format!("'{}'", f))
        .collect::<Vec<_>>()
        .join(", ")
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

pub const PROTECTED_BODY_FIELDS: &[&str] = &[
    "model",
    "messages",
    "input",
    "instructions",
    "tools",
    "tool_choice",
    "stream",
    "max_tokens",
    "max_completion_tokens",
    "max_output_tokens",
    "system",
    "temperature",
    "top_p",
    "metadata",
    "reasoning_effort",
    "parallel_tool_calls",
    "store",
    "service_tier",
    "prompt_cache_key",
    "prompt_cache_retention",
    "user",
    "safety_identifier",
    "verbosity",
    "stream_options",
    "thinking",
    "output_config",
    "reasoning",
    "text",
    "previous_response_id",
    "conversation",
    "include",
    "context_management",
];

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
    /// Body transformation policy applied after request conversion.
    #[serde(default)]
    pub body_policy: Option<BodyTransformPolicy>,
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

            if let Some(body_entries) = &provider.body {
                for entry in body_entries {
                    if PROTECTED_BODY_FIELDS.contains(&entry.name.as_str()) {
                        return Err(crate::ConfigError::InvalidConfig(format!(
                            "Provider '{}' body config contains protected field '{}'. \
                             Protected fields: {}",
                            provider_id,
                            entry.name,
                            protected_fields_list()
                        )));
                    }
                }
            }

            if let Some(policy) = &provider.body_policy {
                let fields = match policy {
                    BodyTransformPolicy::Allowlist(fields) => Some(fields),
                    BodyTransformPolicy::Blocklist(fields) => Some(fields),
                    _ => None,
                };
                if let Some(field_list) = fields {
                    for field in field_list {
                        if PROTECTED_BODY_FIELDS.contains(&field.as_str()) {
                            return Err(crate::ConfigError::InvalidConfig(format!(
                                "Provider '{}' body_policy contains protected field '{}'. \
                                 Protected fields: {}",
                                provider_id,
                                field,
                                protected_fields_list()
                            )));
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_body_policy_simple_string() {
        let toml = r#"
            base_url = "https://api.example.com"
            api_key = "test-key"
            endpoint = "openai"
            body_policy = "drop_unknown"
        "#;
        let provider: ProviderConfig = toml::from_str(toml).unwrap();
        assert_eq!(provider.body_policy, Some(BodyTransformPolicy::DropUnknown));
    }

    #[test]
    fn test_body_policy_allowlist() {
        let toml = r#"
            base_url = "https://api.example.com"
            api_key = "test-key"
            endpoint = "openai"
            body_policy = { allowlist = ["temperature", "top_p"] }
        "#;
        let provider: ProviderConfig = toml::from_str(toml).unwrap();
        assert_eq!(
            provider.body_policy,
            Some(BodyTransformPolicy::Allowlist(vec![
                "temperature".into(),
                "top_p".into()
            ]))
        );
    }

    #[test]
    fn test_body_policy_blocklist() {
        let toml = r#"
            base_url = "https://api.example.com"
            api_key = "test-key"
            endpoint = "openai"
            body_policy = { blocklist = ["prediction"] }
        "#;
        let provider: ProviderConfig = toml::from_str(toml).unwrap();
        assert_eq!(
            provider.body_policy,
            Some(BodyTransformPolicy::Blocklist(vec!["prediction".into()]))
        );
    }

    #[test]
    fn test_body_policy_omitted() {
        let toml = r#"
            base_url = "https://api.example.com"
            api_key = "test-key"
            endpoint = "openai"
        "#;
        let provider: ProviderConfig = toml::from_str(toml).unwrap();
        assert_eq!(provider.body_policy, None);
    }

    #[test]
    fn test_body_policy_invalid_simple_string() {
        let toml = r#"
            base_url = "https://api.example.com"
            api_key = "test-key"
            endpoint = "openai"
            body_policy = "invalid_policy"
        "#;
        let result: Result<ProviderConfig, _> = toml::from_str(toml);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_protected_fields_in_body() {
        let config = Config::from_toml(
            r#"
            [provider.test]
            base_url = "https://api.example.com"
            api_key = "test-key"
            endpoint = "openai"
            body = [{ name = "model", value = "gpt-4" }]
        "#,
        )
        .unwrap();
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validate_protected_fields_in_allowlist() {
        let config = Config::from_toml(
            r#"
            [provider.test]
            base_url = "https://api.example.com"
            api_key = "test-key"
            endpoint = "openai"
            body_policy = { allowlist = ["temperature", "model"] }
        "#,
        )
        .unwrap();
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validate_protected_fields_in_blocklist() {
        let config = Config::from_toml(
            r#"
            [provider.test]
            base_url = "https://api.example.com"
            api_key = "test-key"
            endpoint = "openai"
            body_policy = { blocklist = ["messages"] }
        "#,
        )
        .unwrap();
        assert!(config.validate().is_err());
    }
}
