//! Configuration validator module
//!
//! Provides functionality to validate loaded configuration.

use super::{Config, ProviderConfig};
use crate::error::{LlmMapError, Result};

impl Config {
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
    pub fn validate(&self) -> Result<()> {
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
    fn validate_server(server: &super::ServerConfig) -> Result<()> {
        if server.port == 0 {
            return Err(LlmMapError::Config("Server port cannot be 0".to_string()));
        }
        Ok(())
    }

    /// Validate a single provider configuration
    fn validate_provider(provider_id: &str, provider: &ProviderConfig) -> Result<()> {
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
    use std::collections::HashMap;

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
            headers: Vec::new(),
            body: Vec::new(),
            models: Vec::new(),
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
                crate::config::Endpoint::Openai,
                vec!["openai-to-qwen"],
            ),
        );

        let config = Config {
            provider: provider_map,
            server: super::super::ServerConfig {
                port: 5564,
                proxy: None,
            },
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
                crate::config::Endpoint::Openai,
                vec![],
            ),
        );

        let config = Config {
            provider: provider_map,
            server: super::super::ServerConfig {
                port: 5564,
                proxy: None,
            },
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
            create_test_provider("", "sk-key", crate::config::Endpoint::Openai, vec![]),
        );

        let config = Config {
            provider: provider_map,
            server: super::super::ServerConfig {
                port: 5564,
                proxy: None,
            },
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
                crate::config::Endpoint::Openai,
                vec![],
            ),
        );

        let config = Config {
            provider: provider_map,
            server: super::super::ServerConfig {
                port: 5564,
                proxy: None,
            },
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
            server: super::super::ServerConfig {
                port: 0,
                proxy: None,
            },
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
                crate::config::Endpoint::Openai,
                vec![],
            ),
        );

        let config = Config {
            provider: provider_map,
            server: super::super::ServerConfig {
                port: 5564,
                proxy: None,
            },
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
                crate::config::Endpoint::Openai,
                vec![],
            ),
        );

        let config = Config {
            provider: provider_map,
            server: super::super::ServerConfig {
                port: 5564,
                proxy: None,
            },
        };

        let result = config.validate();
        assert!(result.is_ok());
    }
}
