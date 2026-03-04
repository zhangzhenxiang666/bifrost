//! Configuration loader module
//!
//! Provides functionality to load configuration from TOML files.

use super::Config;
use crate::error::{LlmMapError, Result};
use std::path::Path;

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
    /// use llm_map::config::Config;
    /// let config = Config::from_file("config.toml").unwrap();
    /// ```
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Config> {
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

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
}
