//! Configuration module for LLM Map service
//!
//! Provides configuration structures and deserialization logic
//! for TOML-based configuration files.

mod loader;
mod validator;

use serde::de::{self, Deserializer, SeqAccess, Visitor};
use serde::Deserialize;
use std::fmt;
use std::marker::PhantomData;
// =============================================================================
// OneOrMany - Custom deserializer for T or Vec<T>
// =============================================================================

/// A wrapper that can deserialize either a single value or an array of values.
///
/// This is useful for configuration where users can specify either:
/// - A single value: `adapters = "adapter1"`
/// - An array: `adapters = ["adapter1", "adapter2"]`
///
/// Both forms will be deserialized as `Vec<T>`.
#[derive(Debug, Clone, PartialEq)]
pub struct OneOrMany<T>(pub Vec<T>);

impl<T> OneOrMany<T> {
    /// Returns a reference to the inner vector
    pub fn as_vec(&self) -> &Vec<T> {
        &self.0
    }

    /// Converts into the inner vector
    pub fn into_vec(self) -> Vec<T> {
        self.0
    }
}

impl<T> From<OneOrMany<T>> for Vec<T> {
    fn from(one_or_many: OneOrMany<T>) -> Self {
        one_or_many.0
    }
}

impl<T> From<Vec<T>> for OneOrMany<T> {
    fn from(vec: Vec<T>) -> Self {
        OneOrMany(vec)
    }
}

impl<T> Default for OneOrMany<T> {
    fn default() -> Self {
        OneOrMany(Vec::new())
    }
}

/// Visitor for deserializing OneOrMany
struct OneOrManyVisitor<T> {
    marker: PhantomData<T>,
}

impl<T> OneOrManyVisitor<T> {
    fn new() -> Self {
        OneOrManyVisitor {
            marker: PhantomData,
        }
    }
}

impl<'de, T> Visitor<'de> for OneOrManyVisitor<T>
where
    T: Deserialize<'de>,
{
    type Value = Vec<T>;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a single value or a list of values")
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        // Deserialize a single string value as a Vec with one element
        let single: T = Deserialize::deserialize(de::value::StrDeserializer::new(value))?;
        Ok(vec![single])
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut vec = Vec::new();
        while let Some(element) = seq.next_element()? {
            vec.push(element);
        }
        Ok(vec)
    }
}

impl<'de, T> Deserialize<'de> for OneOrMany<T>
where
    T: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let visitor = OneOrManyVisitor::new();
        deserializer.deserialize_any(visitor).map(OneOrMany)
    }
}

/// Custom deserializer function for `#[serde(deserialize_with = "one_or_many")]`
pub fn one_or_many<'de, T, D>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    T: Deserialize<'de>,
    D: Deserializer<'de>,
{
    OneOrMany::<T>::deserialize(deserializer).map(|v| v.into_vec())
}

// =============================================================================
// Configuration Structures
// =============================================================================

/// Header key-value pair for HTTP requests
#[derive(Debug, Clone, Deserialize)]
pub struct HeaderEntry {
    /// Header name
    pub name: String,
    /// Header value
    pub value: String,
}

/// Body key-value pair for HTTP requests
#[derive(Debug, Clone, Deserialize)]
pub struct BodyEntry {
    /// Body field name
    pub name: String,
    /// Body field value (can be string, number, boolean, etc.)
    pub value: serde_json::Value,
}
// =============================================================================
// Endpoint Type
// =============================================================================

/// Provider endpoint type for API compatibility
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum Endpoint {
    /// OpenAI-compatible API format
    #[default]
    Openai,
    /// Anthropic API format
    Anthropic,
    /// Qwen API format
    Qwen,
    /// Other/custom endpoint types
    #[serde(other)]
    Other,
}

impl Endpoint {
    /// Check if this is an OpenAI-compatible endpoint
    pub fn is_openai(&self) -> bool {
        matches!(self, Endpoint::Openai)
    }

    /// Check if this is an Anthropic endpoint
    pub fn is_anthropic(&self) -> bool {
        matches!(self, Endpoint::Anthropic)
    }

    /// Check if this is a Qwen endpoint
    pub fn is_qwen(&self) -> bool {
        matches!(self, Endpoint::Qwen)
    }
}

// Default to OpenAI for backward compatibility

/// Header key-value pair for HTTP requests
/// Model-specific configuration
#[derive(Debug, Clone, Deserialize)]
pub struct ModelConfig {
    /// Model identifier/name
    pub name: String,
    /// Optional headers specific to this model
    #[serde(default)]
    pub headers: Vec<HeaderEntry>,
    /// Optional body fields specific to this model
    #[serde(default)]
    pub body: Vec<BodyEntry>,
}

/// Provider configuration
#[derive(Debug, Clone, Deserialize)]
pub struct ProviderConfig {
    /// Base URL for the provider's API
    pub base_url: String,
    /// API key for authentication
    pub api_key: String,
    /// Provider endpoint type (e.g., "openai", "anthropic")
    pub endpoint: Endpoint,
    /// Adapters to apply for this provider (one or many)
    #[serde(default, deserialize_with = "one_or_many")]
    pub adapter: Vec<String>,
    /// Optional headers to add to all requests to this provider
    #[serde(default)]
    pub headers: Vec<HeaderEntry>,
    /// Optional body fields to add to all requests to this provider
    #[serde(default)]
    pub body: Vec<BodyEntry>,
    /// Optional model-specific configurations
    #[serde(default)]
    pub models: Vec<ModelConfig>,
}



/// Server configuration
#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    /// Port to listen on
    pub port: u16,
    /// Optional proxy URL
    #[serde(default)]
    pub proxy: Option<String>,
}

/// Root configuration structure
/// Matches config.toml format:
/// - [provider.xxx] - nested table for providers
/// - [server] - server configuration
/// Provider config contains models array internally
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    /// Provider configurations (keyed by provider ID)
    /// Loaded from [provider.xxx] sections
    #[serde(default)]
    pub provider: std::collections::HashMap<String, ProviderConfig>,
    /// Server configuration
    /// Loaded from [server] section
    #[serde(default)]
    pub server: ServerConfig,
}




// Default server configuration
impl Default for ServerConfig {
    fn default() -> Self {
        ServerConfig {
            port: 5564,
            proxy: None,
        }
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_one_or_many_single_string() {
        let toml_str = r#"
            adapter = "openai_to_qwen"
        "#;

        #[derive(Deserialize)]
        struct TestConfig {
            #[serde(deserialize_with = "one_or_many")]
            adapter: Vec<String>,
        }

        let config: TestConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.adapter, vec!["openai_to_qwen"]);
    }

    #[test]
    fn test_one_or_many_array() {
        let toml_str = r#"
            adapter = ["openai_to_qwen", "rate_limit"]
        "#;

        #[derive(Deserialize)]
        struct TestConfig {
            #[serde(deserialize_with = "one_or_many")]
            adapter: Vec<String>,
        }

        let config: TestConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.adapter, vec!["openai_to_qwen", "rate_limit"]);
    }

    #[test]
    fn test_one_or_many_wrapper_single() {
        let toml_str = r#"
            adapters = "adapter1"
        "#;

        #[derive(Deserialize)]
        struct TestConfig {
            adapters: OneOrMany<String>,
        }

        let config: TestConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.adapters.into_vec(), vec!["adapter1"]);
    }

    #[test]
    fn test_one_or_many_wrapper_array() {
        let toml_str = r#"
            adapters = ["adapter1", "adapter2", "adapter3"]
        "#;

        #[derive(Deserialize)]
        struct TestConfig {
            adapters: OneOrMany<String>,
        }

        let config: TestConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(
            config.adapters.into_vec(),
            vec!["adapter1", "adapter2", "adapter3"]
        );
    }

    #[test]
    fn test_config_from_toml() {
        let toml_str = r#"
            port = 5564

            [provider.qwen-code]
            base_url = "https://api.example.com"
            api_key = "sk-test-key"
            endpoint = "openai"
            adapter = "openai-to-qwen"
            models = [
                { name = "coder-model" }
            ]
        "#;

        let config: Config = toml::from_str(toml_str).unwrap();

        assert_eq!(config.server.port, 5564);
        assert!(config.provider.contains_key("qwen-code"));
        let provider = config.provider.get("qwen-code").unwrap();
        assert_eq!(provider.base_url, "https://api.example.com");
        assert_eq!(provider.adapter, vec!["openai-to-qwen"]);
        assert_eq!(provider.models.len(), 1);
        assert_eq!(provider.models[0].name, "coder-model");
    }

    #[test]
    fn test_config_with_multiple_adapters() {
        let toml_str = r#"
            [server]
            port = 8080

            [provider.test]
            base_url = "https://test.api.com"
            api_key = "test-key"
            endpoint = "openai"
            adapter = ["adapter1", "adapter2", "adapter3"]
        "#;

        let config: Config = toml::from_str(toml_str).unwrap();

        let provider = config.provider.get("test").unwrap();
        assert_eq!(provider.adapter, vec!["adapter1", "adapter2", "adapter3"]);
    }
}
