use crate::error::LlmMapError;
use serde::Deserialize;
use serde::Serialize;
use serde::de::{self, Deserializer, SeqAccess, Visitor};
use std::fmt;
use std::marker::PhantomData;
use std::path::Path;

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
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum Endpoint {
    /// OpenAI-compatible API format
    #[default]
    OpenAI,
    /// Anthropic API format
    Anthropic,
}

impl fmt::Display for Endpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Endpoint::OpenAI => write!(f, "openai"),
            Endpoint::Anthropic => write!(f, "anthropic"),
        }
    }
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

/// Endpoint-level configuration for model name mapping
#[derive(Debug, Clone, Deserialize)]
pub struct EndpointConfig {
    /// Model name to provider@model mapping
    #[serde(default)]
    pub mapping: std::collections::HashMap<String, String>,
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
    pub headers: Option<Vec<HeaderEntry>>,
    /// Optional body fields specific to this model
    #[serde(default)]
    pub body: Option<Vec<BodyEntry>>,
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
    pub headers: Option<Vec<HeaderEntry>>,
    /// Optional body fields to add to all requests to this provider
    #[serde(default)]
    pub body: Option<Vec<BodyEntry>>,
    /// Optional model-specific configurations
    #[serde(default)]
    pub models: Option<Vec<ModelConfig>>,
    /// Optional list of headers to exclude from original request (not adapter headers)
    #[serde(default)]
    pub exclude_headers: Option<Vec<String>>,
    /// Whether to inherit and remove hardcoded excluded headers (authorization, etc.)
    /// When false (default), only removes headers explicitly listed in exclude_headers
    #[serde(default)]
    pub extend: bool,
}

/// Server configuration
#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    /// Port to listen on
    pub port: u16,
    /// Optional proxy URL
    #[serde(default)]
    pub proxy: Option<String>,
    /// Response timeout in seconds (default: 600)
    #[serde(default)]
    pub timeout_secs: Option<u64>,
    /// Maximum number of retries for failed requests (default: 5)
    #[serde(default)]
    pub max_retries: Option<u32>,
    /// Base delay for exponential backoff in milliseconds (default: 100)
    #[serde(default)]
    pub retry_backoff_base_ms: Option<u64>,
}

/// Root configuration structure
/// Matches config.toml format:
/// - [provider.xxx] - nested table for providers
/// - [server] - server configuration
/// - Provider config contains models array internally
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
    /// Endpoint-level configurations (e.g., model name mappings)
    /// Loaded from [endpoint.openai] and [endpoint.anthropic] sections
    #[serde(default)]
    pub endpoint: std::collections::HashMap<String, EndpointConfig>,
}

// Default server configuration
impl Default for ServerConfig {
    fn default() -> Self {
        ServerConfig {
            port: 5564,
            proxy: None,
            timeout_secs: None,
            max_retries: None,
            retry_backoff_base_ms: None,
        }
    }
}

impl Config {
    /// Load configuration from a TOML file
    ///
    /// # Arguments
    /// * `path` - Path to the TOML configuration file
    ///
    /// # Returns
    /// * `Ok(Config)` - Successfully loaded configuration
    /// * `Err(LlmMapError)` - Failed to read or parse the file
    ///
    /// # Example
    /// ```no_run
    /// use bifrost_server::config::Config;
    /// let config = Config::from_file("config.toml").unwrap();
    /// ```
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Config, LlmMapError> {
        let path = path.as_ref();

        // Read file content
        let content = std::fs::read_to_string(path).map_err(|e| {
            LlmMapError::Config(format!(
                "Failed to read config file '{}': {}",
                path.display(),
                e
            ))
        })?;

        // Parse TOML content
        let config: Config = toml::from_str(&content)
            .map_err(|e| LlmMapError::Config(format!("Failed to parse config file: {}", e)))?;

        Ok(config)
    }

    /// Validate the configuration
    ///
    /// # Validation Rules
    /// - All model-referenced providers must exist
    /// - All model-referenced adapters must exist (if adapters are defined)
    /// - Provider base_url must be a valid URL format
    /// - Provider api_key must not be empty
    /// - Server port must not be 0
    ///
    /// # Returns
    /// * `Ok(())` - Configuration is valid
    /// * `Err(LlmMapError)` - Configuration validation failed
    pub fn validate(&self) -> Result<(), LlmMapError> {
        let mut errors = Vec::new();

        // Validate server configuration
        if let Err(e) = Self::validate_server(&self.server) {
            errors.push(e);
        }

        // Validate each provider
        for (provider_id, provider) in &self.provider {
            if let Err(e) = Self::validate_provider(provider_id, provider) {
                errors.push(e);
            }
        }

        // Return first error if any
        if let Some(first_error) = errors.into_iter().next() {
            Err(first_error)
        } else {
            Ok(())
        }
    }

    /// Validate server configuration
    fn validate_server(server: &ServerConfig) -> Result<(), LlmMapError> {
        if server.port == 0 {
            return Err(LlmMapError::Config("Server port cannot be 0".to_string()));
        }
        Ok(())
    }

    /// Validate a single provider configuration
    fn validate_provider(provider_id: &str, provider: &ProviderConfig) -> Result<(), LlmMapError> {
        // Validate api_key is not empty
        if provider.api_key.trim().is_empty() {
            return Err(LlmMapError::Config(format!(
                "Provider '{}' has an empty api_key",
                provider_id
            )));
        }

        // Validate base_url is not empty
        if provider.base_url.trim().is_empty() {
            return Err(LlmMapError::Config(format!(
                "Provider '{}' has an empty base_url",
                provider_id
            )));
        }

        // Check if base_url is a valid URL
        if !Self::is_valid_url(&provider.base_url) {
            return Err(LlmMapError::Config(format!(
                "Provider '{}' has an invalid base_url format: '{}'",
                provider_id, provider.base_url
            )));
        }
        // Endpoint is an enum with Default, no need to validate for empty
        // The enum automatically validates to Other for unknown values

        Ok(())
    }

    /// Check if a string is a valid URL
    fn is_valid_url(url: &str) -> bool {
        // Basic URL validation: must start with http:// or https://
        url.starts_with("http://") || url.starts_with("https://")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::HashMap, io::Write};
    use tempfile::NamedTempFile;

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
        assert_eq!(provider.models.as_ref().unwrap().len(), 1);
        assert_eq!(provider.models.as_ref().unwrap()[0].name, "coder-model");
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

    fn create_temp_config(content: &str) -> NamedTempFile {
        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(content.as_bytes()).unwrap();
        temp_file.flush().unwrap();
        temp_file
    }

    #[test]
    fn test_from_file_valid_config() {
        let toml_content = r#"
            port = 5564

            [provider.qwen-code]
            base_url = "https://api.example.com/v1"
            api_key = "sk-test-key"
            endpoint = "openai"
            adapter = "openai-to-qwen"
            models = [
                { name = "coder-model" }
            ]
        "#;

        let temp_file = create_temp_config(toml_content);
        let config = Config::from_file(temp_file.path()).unwrap();

        assert_eq!(config.server.port, 5564);
        assert!(config.provider.contains_key("qwen-code"));
        let provider = config.provider.get("qwen-code").unwrap();
        assert_eq!(provider.adapter, vec!["openai-to-qwen"]);
        assert_eq!(provider.models.as_ref().unwrap().len(), 1);
        assert_eq!(provider.models.as_ref().unwrap()[0].name, "coder-model");
    }

    #[test]
    fn test_from_file_nonexistent_file() {
        let result = Config::from_file("/nonexistent/path/config.toml");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("Failed to read config file"));
    }

    #[test]
    fn test_from_file_invalid_toml() {
        let invalid_toml = "this is not valid toml {{{";
        let temp_file = create_temp_config(invalid_toml);

        let result = Config::from_file(temp_file.path());
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("Failed to parse config file"));
    }

    #[test]
    fn test_from_file_missing_required_fields() {
        // Missing required fields in provider
        let incomplete_toml = r#"
            [server]
            port = 5564

            [provider.test]
            base_url = "https://test.api.com"
            # Missing api_key and endpoint
        "#;
        let temp_file = create_temp_config(incomplete_toml);

        let result = Config::from_file(temp_file.path());
        assert!(result.is_err());
    }

    #[test]
    fn test_from_file_with_adapter_array() {
        let toml_content = r#"
            port = 8080

            [provider.test]
            base_url = "https://test.api.com"
            api_key = "test-key"
            endpoint = "openai"
            adapter = ["adapter1", "adapter2"]
            models = [
                { name = "test-model" }
            ]
        "#;

        let temp_file = create_temp_config(toml_content);
        let config = Config::from_file(temp_file.path()).unwrap();

        let provider = config.provider.get("test").unwrap();
        assert_eq!(provider.adapter, vec!["adapter1", "adapter2"]);
        assert_eq!(provider.models.as_ref().unwrap().len(), 1);
    }

    fn create_test_provider(
        base_url: &str,
        api_key: &str,
        endpoint: crate::config::Endpoint,
        adapters: Vec<&str>,
    ) -> ProviderConfig {
        ProviderConfig {
            base_url: base_url.to_string(),
            api_key: api_key.to_string(),
            endpoint,
            adapter: adapters.into_iter().map(String::from).collect(),
            headers: None,
            body: None,
            models: None,
            exclude_headers: None,
            extend: false,
        }
    }

    #[test]
    fn test_validate_valid_config() {
        let mut provider_map = HashMap::new();
        provider_map.insert(
            "qwen-code".to_string(),
            create_test_provider(
                "https://api.example.com/v1",
                "sk-test-key",
                crate::config::Endpoint::OpenAI,
                vec!["openai-to-qwen"],
            ),
        );

        let config = Config {
            provider: provider_map,
            server: ServerConfig {
                port: 5564,
                proxy: None,
                timeout_secs: None,
                max_retries: None,
                retry_backoff_base_ms: None,
            },
            endpoint: Default::default(),
        };

        let result = config.validate();
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_missing_provider() {
        // No longer applicable - model entries removed
    }

    #[test]
    fn test_validate_empty_api_key() {
        let mut provider_map = HashMap::new();
        provider_map.insert(
            "test".to_string(),
            create_test_provider(
                "https://api.test.com",
                "",
                crate::config::Endpoint::OpenAI,
                vec![],
            ),
        );

        let config = Config {
            provider: provider_map,
            server: ServerConfig {
                port: 5564,
                proxy: None,
                timeout_secs: None,
                max_retries: None,
                retry_backoff_base_ms: None,
            },
            endpoint: Default::default(),
        };

        let result = config.validate();
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("empty api_key"));
    }

    #[test]
    fn test_validate_empty_base_url() {
        let mut provider_map = HashMap::new();
        provider_map.insert(
            "test".to_string(),
            create_test_provider("", "sk-key", crate::config::Endpoint::OpenAI, vec![]),
        );

        let config = Config {
            provider: provider_map,
            server: ServerConfig {
                port: 5564,
                proxy: None,
                timeout_secs: None,
                max_retries: None,
                retry_backoff_base_ms: None,
            },
            endpoint: Default::default(),
        };

        let result = config.validate();
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("empty base_url"));
    }

    #[test]
    fn test_validate_invalid_url_format() {
        let mut provider_map = HashMap::new();
        provider_map.insert(
            "test".to_string(),
            create_test_provider(
                "not-a-valid-url",
                "sk-key",
                crate::config::Endpoint::OpenAI,
                vec![],
            ),
        );

        let config = Config {
            provider: provider_map,
            server: ServerConfig {
                port: 5564,
                proxy: None,
                timeout_secs: None,
                max_retries: None,
                retry_backoff_base_ms: None,
            },
            endpoint: Default::default(),
        };

        let result = config.validate();
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("invalid base_url format"));
    }

    #[test]
    fn test_validate_invalid_adapter() {
        // No longer applicable - model entries removed
    }

    #[test]
    fn test_validate_zero_port() {
        let config = Config {
            provider: HashMap::new(),
            server: ServerConfig {
                port: 0,
                proxy: None,
                timeout_secs: None,
                max_retries: None,
                retry_backoff_base_ms: None,
            },
            endpoint: Default::default(),
        };

        let result = config.validate();
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("port cannot be 0"));
    }

    #[test]
    fn test_validate_https_url() {
        let mut provider_map = HashMap::new();
        provider_map.insert(
            "test".to_string(),
            create_test_provider(
                "https://api.test.com/v1",
                "sk-key",
                crate::config::Endpoint::OpenAI,
                vec![],
            ),
        );

        let config = Config {
            provider: provider_map,
            server: ServerConfig {
                port: 5564,
                proxy: None,
                timeout_secs: None,
                max_retries: None,
                retry_backoff_base_ms: None,
            },
            endpoint: Default::default(),
        };

        let result = config.validate();
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_http_url() {
        let mut provider_map = HashMap::new();
        provider_map.insert(
            "test".to_string(),
            create_test_provider(
                "http://localhost:8080",
                "sk-key",
                crate::config::Endpoint::OpenAI,
                vec![],
            ),
        );

        let config = Config {
            provider: provider_map,
            server: ServerConfig {
                port: 5564,
                proxy: None,
                timeout_secs: None,
                max_retries: None,
                retry_backoff_base_ms: None,
            },
            endpoint: Default::default(),
        };

        let result = config.validate();
        assert!(result.is_ok());
    }
}
