//! OpenAI to Qwen adapter - adds Qwen-specific headers for OpenAI-compatible API
//!
//! This adapter transforms OpenAI-compatible requests to Qwen API format by adding
//! the required headers and handling stream options.

use crate::adapter::Adapter;
use crate::config::ProviderConfig;
use crate::error::LlmMapError;
use crate::model::{RequestTransform, StreamChunkTransform};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::env;
use std::path::{Path, PathBuf};
use std::sync::{OnceLock, RwLock};
/// OAuth credentials structure for Qwen API authentication
#[derive(Debug, Clone, Serialize, Deserialize)]
struct OAuthCredentials {
    /// Access token for API authentication
    access_token: String,

    /// Token type, defaults to "Bearer"
    #[serde(default = "default_token_type")]
    token_type: String,

    /// Refresh token for obtaining new access tokens (optional)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    refresh_token: Option<String>,

    /// Resource URL for the API
    #[serde(default)]
    resource_url: String,

    /// Expiry date as milliseconds timestamp
    #[serde(with = "chrono::serde::ts_milliseconds")]
    expiry_date: DateTime<Utc>,
}

/// Default token type function
fn default_token_type() -> String {
    "Bearer".to_string()
}

impl OAuthCredentials {
    /// Load credentials from a JSON file
    pub fn from_file(path: &Path) -> Result<Self, LlmMapError> {
        let content = std::fs::read_to_string(path).map_err(|e| {
            LlmMapError::Validation(format!("Failed to read credentials file: {}", e))
        })?;

        let creds: OAuthCredentials = serde_json::from_str(&content).map_err(|e| {
            LlmMapError::Validation(format!("Failed to parse credentials JSON: {}", e))
        })?;

        // Validate that access_token is not empty
        if creds.access_token.is_empty() {
            return Err(LlmMapError::Validation(
                "Access token cannot be empty".to_string(),
            ));
        }

        Ok(creds)
    }
}

fn get_oauth_file_path() -> Result<PathBuf, LlmMapError> {
    let home = env::var("HOME")
        .or_else(|_| env::var("USERPROFILE"))
        .map_err(|_| {
            LlmMapError::Validation(
                "Home directory not found. Set HOME or USERPROFILE environment variable"
                    .to_string(),
            )
        })?;
    Ok(PathBuf::from(home).join(".qwen").join("oauth_creds.json"))
}

/// OAuth credentials manager with refresh token support
struct OAuthCredentialsManager {
    creds: RwLock<OAuthCredentials>,
}

impl OAuthCredentialsManager {
    fn new(creds: OAuthCredentials) -> Self {
        Self {
            creds: RwLock::new(creds),
        }
    }

    fn get_access_token(&self) -> String {
        let creds = self.creds.read().unwrap();
        creds.access_token.clone()
    }

    fn is_token_expired(&self) -> bool {
        let creds = self.creds.read().unwrap();
        creds.expiry_date <= Utc::now()
    }

    async fn refresh_token(&self, refresh_token: &str) -> Result<(), LlmMapError> {
        let urlencoded = format!(
            "grant_type=refresh_token&refresh_token={}&client_id={}",
            urlencoding::encode(refresh_token),
            "f0304373b74a44d2b584a3fb70ca9e56"
        );

        let response = reqwest::Client::new()
            .post("https://chat.qwen.ai/api/v1/oauth2/token")
            .header(
                http::header::CONTENT_TYPE,
                "application/x-www-form-urlencoded",
            )
            .header(http::header::ACCEPT, "application/json")
            .body(urlencoded)
            .send()
            .await
            .map_err(|e| LlmMapError::Validation(format!("Failed to refresh token: {}", e)))?;

        if !response.status().is_success() {
            return Err(LlmMapError::Validation(format!(
                "Token refresh failed with status: {}",
                response.status()
            )));
        }

        #[derive(Debug, Deserialize)]
        struct RefreshResponse {
            status: String,
            access_token: String,
            refresh_token: String,
            token_type: String,
            expires_in: u64,
            #[serde(default)]
            _scope: String,
            #[serde(default)]
            resource_url: String,
        }

        let data: RefreshResponse = response.json().await.map_err(|e| {
            LlmMapError::Validation(format!("Failed to parse refresh response: {}", e))
        })?;

        if data.status != "success" {
            return Err(LlmMapError::Validation(format!(
                "Token refresh returned non-success status: {}",
                data.status
            )));
        }

        // Calculate expiry date (subtract 1 minute buffer)
        let expiry_date = Utc::now() + chrono::Duration::seconds(data.expires_in as i64 - 60);

        let new_creds = OAuthCredentials {
            access_token: data.access_token,
            token_type: data.token_type,
            refresh_token: Some(data.refresh_token.clone()),
            resource_url: data.resource_url,
            expiry_date,
        };

        // Update credentials
        {
            let mut creds = self.creds.write().unwrap();
            *creds = new_creds.clone();
        }

        // Save to file
        Self::save_credentials_to_file(&new_creds).await?;

        Ok(())
    }

    async fn ensure_valid_token(&self) -> Result<(), LlmMapError> {
        if self.is_token_expired() {
            let refresh_token = {
                let creds = self.creds.read().unwrap();
                creds.refresh_token.clone()
            };

            if let Some(refresh_token) = refresh_token {
                self.refresh_token(&refresh_token).await?;
            } else {
                return Err(LlmMapError::Validation(
                    "Token expired and no refresh token available".to_string(),
                ));
            }
        }
        Ok(())
    }

    async fn save_credentials_to_file(creds: &OAuthCredentials) -> Result<(), LlmMapError> {
        let oauth_file = get_oauth_file_path()?;

        // Ensure directory exists
        if let Some(parent) = oauth_file.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|e| {
                LlmMapError::Validation(format!("Failed to create .qwen directory: {}", e))
            })?;
        }

        let content = serde_json::to_string_pretty(creds).map_err(|e| {
            LlmMapError::Validation(format!("Failed to serialize credentials: {}", e))
        })?;

        tokio::fs::write(&oauth_file, content).await.map_err(|e| {
            LlmMapError::Validation(format!("Failed to save credentials file: {}", e))
        })?;

        Ok(())
    }
}

static OAUTH_CREDS_MANAGER: OnceLock<OAuthCredentialsManager> = OnceLock::new();
///
/// Qwen API is OpenAI-compatible but requires specific headers:
/// - `User-Agent`: QwenCode user agent
/// - `X-DashScope-*`: Qwen-specific headers for caching and auth
///
/// For streaming requests, adds `stream_options.include_usage: true`
pub struct OpenAIToQwenAdapter;

#[async_trait]
impl Adapter for OpenAIToQwenAdapter {
    type Error = LlmMapError;

    /// Transform OpenAI-compatible request to Qwen API format.
    ///
    /// Adds:
    /// - Qwen-specific headers (User-Agent, X-DashScope-*)
    /// - For streaming: `stream_options.include_usage: true`
    async fn transform_request(
        &self,
        mut body: serde_json::Value,
        provider_config: &ProviderConfig,
        _headers: &http::HeaderMap,
    ) -> Result<RequestTransform, Self::Error> {
        // Initialize credentials manager if not already loaded
        if OAUTH_CREDS_MANAGER.get().is_none() {
            let oauth_file = get_oauth_file_path()?;
            let creds = OAuthCredentials::from_file(&oauth_file).map_err(|e| {
                LlmMapError::Validation(format!("Failed to load OAuth credentials: {}", e))
            })?;

            let manager = OAuthCredentialsManager::new(creds);

            if OAUTH_CREDS_MANAGER.set(manager).is_err() {
                // Another thread initialized first, that's fine
            }
        }

        // Ensure token is valid (refresh if expired)
        let manager = OAUTH_CREDS_MANAGER.get().ok_or_else(|| {
            LlmMapError::Validation(
                "OAuth credentials manager not initialized. This should not happen.".to_string(),
            )
        })?;

        manager.ensure_valid_token().await?;
        let access_token = manager.get_access_token();

        // For streaming requests, add stream_options to include usage
        if let Some(stream) = body.get("stream").and_then(|v| v.as_bool())
            && stream
            && let Some(obj) = body.as_object_mut()
        {
            obj.insert(
                "stream_options".to_string(),
                serde_json::json!({
                    "include_usage": true
                }),
            );
        }

        let request = RequestTransform::new(body)
            .with_url(format!("{}/chat/completions", provider_config.base_url));

        // Add Qwen-specific headers
        let mut headers = http::HeaderMap::new();
        headers.insert(
            http::header::CONTENT_TYPE,
            "application/json".parse().unwrap(),
        );
        headers.insert(
            http::header::USER_AGENT,
            "QwenCode/0.11.0 (linux; x64)".parse().unwrap(),
        );
        headers.insert(http::header::ACCEPT, "application/json".parse().unwrap());
        headers.insert("X-DashScope-CacheControl", "enable".parse().unwrap());
        headers.insert(
            "X-DashScope-UserAgent",
            "QwenCode/0.11.0 (linux; x64)".parse().unwrap(),
        );
        // Get access token from manager (guaranteed to be valid at this point)
        let auth_header = format!("Bearer {}", access_token);

        headers.insert("X-DashScope-AuthType", "qwen-oauth".parse().unwrap());
        headers.insert(
            http::header::AUTHORIZATION,
            http::HeaderValue::from_str(&auth_header).map_err(|e| {
                LlmMapError::Validation(format!("Invalid authorization header: {}", e))
            })?,
        );
        Ok(request.with_headers(headers))
    }

    async fn transform_stream_chunk(
        &self,
        chunk: serde_json::Value,
        _event: &str,
        _provider_config: &ProviderConfig,
    ) -> Result<StreamChunkTransform, Self::Error> {
        // Pass through unchanged for OpenAI to Qwen
        Ok(StreamChunkTransform::new(chunk))
    }
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use http::HeaderMap;

    /// Initialize OAuth credentials for tests
    fn init_test_credentials() {
        // Create a future timestamp (1 year from now)
        let future_date = Utc::now() + chrono::Duration::days(365);

        let creds = OAuthCredentials {
            access_token: "test_access_token_12345".to_string(),
            token_type: "Bearer".to_string(),
            refresh_token: Some("test_refresh_token_67890".to_string()),
            resource_url: "portal.qwen.ai".to_string(),
            expiry_date: future_date,
        };

        // Initialize the static OnceLock for tests
        let manager = OAuthCredentialsManager::new(creds);
        let _ = OAUTH_CREDS_MANAGER.set(manager);
    }

    fn create_test_config() -> ProviderConfig {
        ProviderConfig {
            base_url: "https://portal.qwen.ai/v1".to_string(),
            api_key: "any-key".to_string(),
            endpoint: crate::config::Endpoint::OpenAI,
            adapter: vec![],
            headers: None,
            body: None,
            models: None,
        }
    }

    #[tokio::test]
    async fn test_openai_to_qwen_request_transform() {
        init_test_credentials();

        let adapter = OpenAIToQwenAdapter;
        let body = serde_json::json!({
            "model": "coder-model",
            "messages": [
                {"role": "user", "content": "Hello"}
            ]
        });
        let config = create_test_config();
        let headers = HeaderMap::new();

        let result = adapter
            .transform_request(body.clone(), &config, &headers)
            .await
            .unwrap();

        // Verify body is unchanged (OpenAI-compatible)
        assert_eq!(result.body, body);

        // Verify Qwen headers are added
        assert!(result.headers.is_some());
        let headers = result.headers.unwrap();
        assert_eq!(headers.get("Content-Type").unwrap(), "application/json");
        assert_eq!(
            headers.get("User-Agent").unwrap(),
            "QwenCode/0.11.0 (linux; x64)"
        );
        assert_eq!(headers.get("X-DashScope-CacheControl").unwrap(), "enable");
        assert_eq!(headers.get("X-DashScope-AuthType").unwrap(), "qwen-oauth");
    }

    #[tokio::test]
    async fn test_openai_to_qwen_streaming_request() {
        let adapter = OpenAIToQwenAdapter;
        let body = serde_json::json!({
            "model": "coder-model",
            "messages": [
                {"role": "user", "content": "Hello"}
            ],
            "stream": true
        });
        let config = create_test_config();
        let headers = HeaderMap::new();

        let result = adapter
            .transform_request(body, &config, &headers)
            .await
            .unwrap();

        // Verify stream_options is added for streaming requests
        assert_eq!(
            result.body["stream_options"],
            serde_json::json!({
                "include_usage": true
            })
        );
    }

    #[tokio::test]
    async fn test_openai_to_qwen_non_streaming_request() {
        let adapter = OpenAIToQwenAdapter;
        let body = serde_json::json!({
            "model": "coder-model",
            "messages": [
                {"role": "user", "content": "Hello"}
            ],
            "stream": false
        });
        let config = create_test_config();
        let headers = HeaderMap::new();

        let result = adapter
            .transform_request(body, &config, &headers)
            .await
            .unwrap();

        // Verify stream_options is NOT added for non-streaming requests
        assert!(result.body.get("stream_options").is_none());
    }

    #[tokio::test]
    async fn test_openai_to_qwen_response_passthrough() {
        let adapter = OpenAIToQwenAdapter;
        let body = serde_json::json!({
            "id": "chatcmpl-123",
            "choices": [
                {
                    "message": {"role": "assistant", "content": "Hi there!"}
                }
            ],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 20,
                "total_tokens": 30
            }
        });
        let status = http::StatusCode::OK;
        let headers = HeaderMap::new();

        let result = adapter
            .transform_response(body.clone(), status, &headers)
            .await
            .unwrap();

        // Verify response is passed through unchanged
        assert_eq!(result.body, body);
    }
}
