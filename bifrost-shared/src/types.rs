//! Configuration types for Bifrost

use serde::{Deserialize, Deserializer, Serialize};
use std::collections::{HashMap, HashSet};

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

/// Header key-value pair with optional endpoint condition
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HeaderEntry {
    pub name: String,
    pub value: String,
    /// Optional condition: only apply this header when the request comes from the specified endpoint.
    /// Valid values: "openai_chat" / "openai-chat", "openai_responses" / "openai-responses", "anthropic".
    /// If None, applies to all endpoints.
    #[serde(default)]
    pub condition: Option<String>,
}

/// Body key-value pair with optional endpoint condition
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BodyEntry {
    pub name: String,
    pub value: serde_json::Value,
    /// Optional condition: only apply this body field when the request comes from the specified endpoint.
    /// Valid values: "openai_chat" / "openai-chat", "openai_responses" / "openai-responses", "anthropic".
    /// If None, applies to all endpoints.
    #[serde(default)]
    pub condition: Option<String>,
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
    "reasoning_effort",
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
#[derive(Debug, Clone, Deserialize)]
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
    /// HTTP status codes that should trigger a retry.
    /// Defaults to {429, 500, 502, 503, 504} if not specified.
    #[serde(default)]
    pub retry_status_codes: Option<HashSet<u16>>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            port: default_port(),
            proxy: None,
            timeout_secs: None,
            max_retries: Some(5),
            retry_backoff_base_ms: Some(700),
            retry_status_codes: None,
        }
    }
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

fn validate_condition(condition: &Option<String>, context: &str) -> Result<(), crate::ConfigError> {
    if let Some(cond) = condition {
        let cond_normalized = cond.to_lowercase().replace('-', "_");
        let valid = ["openai_chat", "openai_responses", "anthropic"]
            .iter()
            .any(|&v| v == cond_normalized);
        if !valid {
            return Err(crate::ConfigError::InvalidConfig(format!(
                "{} has invalid condition '{}'. Valid values: openai_chat/openai-chat, openai_responses/openai-responses, anthropic",
                context, cond
            )));
        }
    }
    Ok(())
}

fn validate_protected_field(name: &str, context: &str) -> Result<(), crate::ConfigError> {
    if PROTECTED_BODY_FIELDS.contains(&name) {
        return Err(crate::ConfigError::InvalidConfig(format!(
            "{} contains protected field '{}'. Protected fields: {}",
            context,
            name,
            protected_fields_list()
        )));
    }
    Ok(())
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

            if let Some(headers) = &provider.headers {
                for entry in headers {
                    validate_condition(
                        &entry.condition,
                        &format!("Provider '{}' header '{}'", provider_id, entry.name),
                    )?;
                }
            }

            if let Some(body_entries) = &provider.body {
                for entry in body_entries {
                    validate_condition(
                        &entry.condition,
                        &format!("Provider '{}' body field '{}'", provider_id, entry.name),
                    )?;
                    validate_protected_field(
                        &entry.name,
                        &format!("Provider '{}' body config", provider_id),
                    )?;
                }
            }

            if let Some(models) = &provider.models {
                for model in models {
                    if let Some(headers) = &model.headers {
                        for entry in headers {
                            validate_condition(
                                &entry.condition,
                                &format!(
                                    "Provider '{}' model '{}' header '{}'",
                                    provider_id, model.name, entry.name
                                ),
                            )?;
                        }
                    }

                    if let Some(body_entries) = &model.body {
                        for entry in body_entries {
                            validate_condition(
                                &entry.condition,
                                &format!(
                                    "Provider '{}' model '{}' body field '{}'",
                                    provider_id, model.name, entry.name
                                ),
                            )?;
                            validate_protected_field(
                                &entry.name,
                                &format!(
                                    "Provider '{}' model '{}' body config",
                                    provider_id, model.name
                                ),
                            )?;
                        }
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

        for (alias_name, alias_entry) in &self.alias {
            if let AliasEntry::Complex(mapping) = alias_entry {
                if let Some(headers) = &mapping.headers {
                    for entry in headers {
                        validate_condition(
                            &entry.condition,
                            &format!("Alias '{}' header '{}'", alias_name, entry.name),
                        )?;
                    }
                }

                if let Some(body_entries) = &mapping.body {
                    for entry in body_entries {
                        validate_condition(
                            &entry.condition,
                            &format!("Alias '{}' body field '{}'", alias_name, entry.name),
                        )?;
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

    #[test]
    fn test_body_entry_with_condition() {
        let toml = r#"
            name = "temperature"
            value = 0.7
            condition = "openai_chat"
        "#;
        let entry: BodyEntry = toml::from_str(toml).unwrap();
        assert_eq!(entry.name, "temperature");
        assert_eq!(entry.condition, Some("openai_chat".to_string()));
    }

    #[test]
    fn test_body_entry_without_condition() {
        let toml = r#"
            name = "temperature"
            value = 0.7
        "#;
        let entry: BodyEntry = toml::from_str(toml).unwrap();
        assert_eq!(entry.name, "temperature");
        assert_eq!(entry.condition, None);
    }

    #[test]
    fn test_header_entry_with_condition() {
        let toml = r#"
            name = "X-Custom-Header"
            value = "custom-value"
            condition = "anthropic"
        "#;
        let entry: HeaderEntry = toml::from_str(toml).unwrap();
        assert_eq!(entry.name, "X-Custom-Header");
        assert_eq!(entry.condition, Some("anthropic".to_string()));
    }

    #[test]
    fn test_retry_status_codes_config() {
        let toml = r#"
            port = 8080
            retry_status_codes = [429, 500, 503]
        "#;
        let config: ServerConfig = toml::from_str(toml).unwrap();
        assert_eq!(
            config.retry_status_codes,
            Some([429, 500, 503].iter().copied().collect())
        );
    }

    #[test]
    fn test_retry_status_codes_default() {
        let toml = r#"
            port = 8080
        "#;
        let config: ServerConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.retry_status_codes, None);
    }

    #[test]
    fn test_validate_invalid_condition_in_header() {
        let config = Config::from_toml(
            r#"
            [provider.test]
            base_url = "https://api.example.com"
            api_key = "test-key"
            endpoint = "openai"

            [[provider.test.headers]]
            name = "X-Custom"
            value = "value"
            condition = "unknown"
        "#,
        )
        .unwrap();
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validate_condition_with_dash_or_underscore() {
        let config = Config::from_toml(
            r#"
            [provider.test]
            base_url = "https://api.example.com"
            api_key = "test-key"
            endpoint = "openai"

            [[provider.test.body]]
            name = "temperature"
            value = 0.7
            condition = "openai-chat"

            [[provider.test.body]]
            name = "top_p"
            value = 0.9
            condition = "openai_responses"
        "#,
        )
        .unwrap();
        let result = config.validate();
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_condition_in_alias() {
        let config = Config::from_toml(
            r#"
            [provider.test]
            base_url = "https://api.example.com"
            api_key = "test-key"
            endpoint = "openai"

            [alias.my-model]
            target = "test@gpt-4"

            [[alias.my-model.body]]
            name = "temperature"
            value = 0.7
            condition = "openai_chat"

            [[alias.my-model.headers]]
            name = "X-Custom"
            value = "value"
            condition = "anthropic"
        "#,
        )
        .unwrap();
        let result = config.validate();
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_invalid_condition_in_alias() {
        let config = Config::from_toml(
            r#"
            [provider.test]
            base_url = "https://api.example.com"
            api_key = "test-key"
            endpoint = "openai"

            [alias.my-model]
            target = "test@gpt-4"

            [[alias.my-model.body]]
            name = "temperature"
            value = 0.7
            condition = "invalid"
        "#,
        )
        .unwrap();
        assert!(config.validate().is_err());
    }
}
