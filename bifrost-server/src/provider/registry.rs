//! Provider registry module for LLM service
//!
//! This module provides the [`ProviderRegistry`] which manages provider configurations
//! and builds adapter chains for request execution.

use crate::adapter::Adapter;
use crate::adapter::builtin::{
    AnthropicToOpenAIAdapter, OpenAIToAnthropicAdapter, PassthroughAdapter, ResponsesToChatAdapter,
};
use crate::adapter::chain::OnionExecutor;
use crate::error::{LlmMapError, Result};
use crate::provider::client::HttpClient;
use crate::routes::RouteEndpoint;
use crate::types::{AliasEntry, Config, Endpoint, ProviderConfig};
use std::collections::HashMap;

/// Registry that manages provider configurations and builds adapter chains.
///
/// The registry is responsible for:
/// - Loading provider configurations from the root config
/// - Providing access to provider information
/// - Building adapter chains (OnionExecutor) for specific providers
#[derive(Debug, Clone)]
pub struct ProviderRegistry {
    providers: HashMap<String, ProviderConfig>,
    http_client: HttpClient,
    alias: HashMap<String, AliasEntry>,
}

impl ProviderRegistry {
    /// Create a new provider registry from configuration.
    ///
    /// This will load all provider configurations from the config file
    /// and prepare them for use.
    ///
    /// # Arguments
    ///
    /// * `config` - The root configuration containing provider settings
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use bifrost_server::Config;
    /// use bifrost_server::provider::ProviderRegistry;
    ///
    /// let config = Config::from_file("config.toml").unwrap();
    /// let registry = ProviderRegistry::from_config(&config);
    /// ```
    /// Create a new provider registry from configuration.
    ///
    /// # Panics
    /// Panics if the HTTP client fails to build (e.g., invalid proxy URL).
    /// This is acceptable for startup initialization.
    pub fn from_config(config: &Config) -> Self {
        let mut providers = HashMap::new();

        // Build provider info from config
        for (id, provider_config) in &config.provider {
            providers.insert(id.clone(), provider_config.clone());
        }

        // Create HTTP client with configurable timeout and retry settings
        let timeout_secs = config.server.timeout_secs.unwrap_or(600);
        let retry_config = crate::provider::client::RetryConfig {
            max_retries: config.server.max_retries.unwrap_or(5),
            backoff_base_ms: config.server.retry_backoff_base_ms.unwrap_or(700),
        };
        let http_client =
            HttpClient::with_retry(timeout_secs, config.server.proxy.as_deref(), retry_config);

        Self {
            providers,
            http_client,
            alias: config.alias.clone(),
        }
    }

    /// Get provider information by ID.
    ///
    /// # Arguments
    ///
    /// * `id` - The provider identifier (e.g., "anthropic-code")
    ///
    /// # Returns
    ///
    /// `Some(&ProviderInfo)` if the provider exists, `None` otherwise.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use bifrost_server::Config;
    /// # use bifrost_server::provider::ProviderRegistry;
    /// # let config = Config::from_file("config.toml").unwrap();
    /// let registry = ProviderRegistry::from_config(&config);
    /// if let Some(provider) = registry.get("anthropic-code") {
    ///     println!("Base URL: {}", provider.base_url);
    /// }
    /// ```
    pub fn get(&self, id: &str) -> Option<&ProviderConfig> {
        self.providers.get(id)
    }

    /// Build an adapter chain (OnionExecutor) for the specified provider.
    ///
    /// This method creates the adapter chain based on the provider's endpoint type.
    /// The adapter is dynamically created internally based on route and endpoint combination.
    ///
    /// # Arguments
    ///
    /// * `provider_id` - The provider identifier
    ///
    /// # Returns
    ///
    /// `Ok(OnionExecutor)` if the provider exists and adapter chain is built successfully,
    /// `Err(LlmMapError)` if the provider is not found.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use bifrost_server::Config;
    /// # use bifrost_server::provider::ProviderRegistry;
    /// # use bifrost_server::routes::RouteEndpoint;
    /// # let config = Config::from_file("config.toml").unwrap();
    /// let registry = ProviderRegistry::from_config(&config);
    /// let executor = registry.build_executor("anthropic-code", &RouteEndpoint::OpenAIChat).unwrap();
    /// ```
    pub fn build_executor(
        &self,
        provider_id: &str,
        route: &RouteEndpoint,
    ) -> Result<OnionExecutor> {
        let provider = self.providers.get(provider_id).ok_or_else(|| {
            LlmMapError::Provider(format!("Provider '{}' not found", provider_id))
        })?;

        let adapters = self.build_adapter_chain(route, &provider.endpoint)?;

        Ok(OnionExecutor::new(adapters))
    }

    /// Build the adapter chain based on route and endpoint type.
    ///
    /// # Arguments
    ///
    /// * `route` - The route endpoint (OpenAIChat, OpenAIResponses, AnthropicMessages)
    /// * `endpoint` - The provider endpoint type (OpenAI or Anthropic)
    ///
    /// # Returns
    ///
    /// A vector of boxed adapters ready for execution.
    fn build_adapter_chain(
        &self,
        route: &RouteEndpoint,
        endpoint: &Endpoint,
    ) -> Result<Vec<Box<dyn Adapter<Error = LlmMapError>>>> {
        let mut adapters: Vec<Box<dyn Adapter<Error = LlmMapError>>> = Vec::new();

        match (route, endpoint) {
            (RouteEndpoint::OpenAIChat, Endpoint::OpenAI) => {
                adapters.push(Box::new(PassthroughAdapter));
            }
            (RouteEndpoint::OpenAIChat, Endpoint::Anthropic) => {
                adapters.push(Box::new(OpenAIToAnthropicAdapter::new()));
            }
            (RouteEndpoint::OpenAIResponses, Endpoint::OpenAI) => {
                adapters.push(Box::new(ResponsesToChatAdapter::new()));
            }
            (RouteEndpoint::OpenAIResponses, Endpoint::Anthropic) => {
                adapters.push(Box::new(ResponsesToChatAdapter::new()));
                adapters.push(Box::new(OpenAIToAnthropicAdapter::new()));
            }
            (RouteEndpoint::AnthropicMessages, Endpoint::Anthropic) => {
                adapters.push(Box::new(PassthroughAdapter));
            }
            (RouteEndpoint::AnthropicMessages, Endpoint::OpenAI) => {
                adapters.push(Box::new(AnthropicToOpenAIAdapter::new()));
            }
        }

        Ok(adapters)
    }

    /// Get the HTTP client for making upstream requests.
    pub fn http_client(&self) -> &HttpClient {
        &self.http_client
    }

    /// Get the number of registered providers.
    pub fn provider_count(&self) -> usize {
        self.providers.len()
    }

    /// Check if a provider exists in the registry.
    pub fn has_provider(&self, id: &str) -> bool {
        self.providers.contains_key(id)
    }

    /// Get all providers as a reference to the underlying HashMap.
    pub fn providers(&self) -> &HashMap<String, ProviderConfig> {
        &self.providers
    }

    pub fn get_alias_entry(&self, alias: &str) -> Option<&AliasEntry> {
        self.alias.get(alias)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{BodyEntry, Endpoint, HeaderEntry};
    use serde_json::json;

    /// Create a test configuration with a single provider
    fn create_test_config() -> Config {
        let mut provider = HashMap::new();
        provider.insert(
            "test-provider".to_string(),
            ProviderConfig {
                base_url: "https://api.test.com".to_string(),
                api_key: "test-key".to_string(),
                endpoint: Endpoint::OpenAI,
                headers: None,
                body: None,
                models: None,
                exclude_headers: None,
                extend: false,
                body_policy: None,
            },
        );

        Config {
            provider,
            server: crate::types::ServerConfig::default(),
            alias: HashMap::new(),
        }
    }

    fn create_test_config_with_endpoint() -> Config {
        let mut provider = HashMap::new();
        provider.insert(
            "anthropic-provider".to_string(),
            ProviderConfig {
                base_url: "https://api.anthropic.com".to_string(),
                api_key: "anthropic-key".to_string(),
                endpoint: Endpoint::Anthropic,
                headers: None,
                body: None,
                models: None,
                exclude_headers: None,
                extend: false,
                body_policy: None,
            },
        );

        Config {
            provider,
            server: crate::types::ServerConfig::default(),
            alias: HashMap::new(),
        }
    }

    #[test]
    fn test_from_config() {
        let config = create_test_config();
        let registry = ProviderRegistry::from_config(&config);

        assert_eq!(registry.provider_count(), 1);
        assert!(registry.has_provider("test-provider"));
        assert!(!registry.has_provider("non-existent"));
    }

    #[test]
    fn test_get_provider() {
        let config = create_test_config();
        let registry = ProviderRegistry::from_config(&config);

        let provider = registry.get("test-provider");
        assert!(provider.is_some());

        let provider = provider.unwrap();
        assert_eq!(provider.base_url, "https://api.test.com");
        assert_eq!(provider.api_key, "test-key");
    }

    #[test]
    fn test_get_non_existent_provider() {
        let config = create_test_config();
        let registry = ProviderRegistry::from_config(&config);

        let provider = registry.get("non-existent");
        assert!(provider.is_none());
    }

    #[tokio::test]
    async fn test_build_executor_passthrough() {
        let config = create_test_config();
        let registry = ProviderRegistry::from_config(&config);

        let executor = registry
            .build_executor("test-provider", &RouteEndpoint::OpenAIChat)
            .unwrap();
        assert_eq!(executor.adapter_count(), 1);

        // Test that executor can execute request
        let body = json!({"test": "data"});

        let result = executor.execute_request(body).await.unwrap();

        assert_eq!(result.body, json!({"test": "data"}));
    }

    #[tokio::test]
    async fn test_build_executor_with_adapter() {
        let config = create_test_config_with_endpoint();
        let registry = ProviderRegistry::from_config(&config);

        let executor = registry
            .build_executor("anthropic-provider", &RouteEndpoint::OpenAIChat)
            .unwrap();
        assert_eq!(executor.adapter_count(), 1);

        // Test that executor can execute request with adapter
        let body = json!({
            "model": "test-model",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "Hello"}]
        });
        let result = executor.execute_request(body).await.unwrap();

        // Verify adapter transformed the request body
        assert!(result.body.get("messages").is_some());
        assert!(result.body.get("max_tokens").is_some());
    }

    #[test]
    fn test_build_executor_non_existent_provider() {
        let config = create_test_config();
        let registry = ProviderRegistry::from_config(&config);

        let result = registry.build_executor("non-existent", &RouteEndpoint::OpenAIChat);
        assert!(result.is_err());

        if let Err(e) = result {
            assert!(e.to_string().contains("not found"));
        }
    }

    #[test]
    fn test_provider_info_config_accessor() {
        let config = create_test_config();
        let registry = ProviderRegistry::from_config(&config);

        let provider_config = registry.get("test-provider").unwrap();

        assert_eq!(provider_config.base_url, "https://api.test.com");
        assert_eq!(provider_config.api_key, "test-key");
        assert_eq!(provider_config.endpoint, Endpoint::OpenAI);
    }

    #[test]
    fn test_http_client_access() {
        let config = create_test_config();
        let registry = ProviderRegistry::from_config(&config);

        let _client = registry.http_client();
        // Just verify we can access the client
        // HttpClient accessed successfully
    }

    #[test]
    fn test_config_with_headers_and_body() {
        let mut provider = HashMap::new();
        provider.insert(
            "custom-provider".to_string(),
            ProviderConfig {
                base_url: "https://api.custom.com".to_string(),
                api_key: "custom-key".to_string(),
                endpoint: Endpoint::OpenAI,
                headers: Some(vec![HeaderEntry {
                    name: "X-Custom-Header".to_string(),
                    value: "custom-value".to_string(),
                }]),
                body: Some(vec![BodyEntry {
                    name: "custom_field".to_string(),
                    value: json!("custom_value"),
                }]),
                models: None,
                exclude_headers: None,
                extend: false,
                body_policy: None,
            },
        );

        let config = Config {
            provider,
            server: crate::types::ServerConfig::default(),
            alias: HashMap::new(),
        };

        let registry = ProviderRegistry::from_config(&config);
        let provider_info = registry.get("custom-provider").unwrap();

        assert_eq!(provider_info.headers.as_ref().unwrap().len(), 1);
        assert_eq!(provider_info.body.as_ref().unwrap().len(), 1);
        assert_eq!(
            provider_info.headers.as_ref().unwrap()[0].name,
            "X-Custom-Header"
        );
        assert_eq!(provider_info.body.as_ref().unwrap()[0].name, "custom_field");
    }
}
