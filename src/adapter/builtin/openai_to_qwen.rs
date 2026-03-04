//! OpenAI to Qwen adapter - adds Qwen-specific headers for OpenAI-compatible API
//!
//! This adapter transforms OpenAI-compatible requests to Qwen API format by adding
//! the required headers and handling stream options.

use std::env;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::adapter::Adapter;
use crate::config::ProviderConfig;
use crate::error::LlmMapError;
use crate::types::{RequestTransform, ResponseTransform, StreamChunkTransform};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
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

fn get_oauth_file_path() -> PathBuf {
    let home = env::var("HOME")
        .or_else(|_| env::var("USERPROFILE"))
        .expect("Home directory not found");
    PathBuf::from(home).join(".qwen").join("oauth_creds.json")
}

static OAUTH_CREDS: OnceLock<OAuthCredentials> = OnceLock::new();
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
        // Initialize credentials if not already loaded
        if OAUTH_CREDS.get().is_none() {
            let oauth_file = get_oauth_file_path();
            let creds = OAuthCredentials::from_file(&oauth_file).map_err(|e| {
                LlmMapError::Validation(format!("Failed to load OAuth credentials: {}", e))
            })?;

            if OAUTH_CREDS.set(creds).is_err() {
                // Another thread initialized first, that's fine
            }
        }

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
        headers.insert("Content-Type", "application/json".parse().unwrap());
        headers.insert(
            "User-Agent",
            "QwenCode/0.11.0 (linux; x64)".parse().unwrap(),
        );
        headers.insert("Accept", "application/json".parse().unwrap());
        headers.insert("X-DashScope-CacheControl", "enable".parse().unwrap());
        headers.insert(
            "X-DashScope-UserAgent",
            "QwenCode/0.11.0 (linux; x64)".parse().unwrap(),
        );
        // Get credentials (guaranteed to be initialized at this point)
        let creds = OAUTH_CREDS
            .get()
            .expect("OAuth credentials should be initialized");
        let auth_header = format!("Bearer {}", creds.access_token);

        headers.insert("X-DashScope-AuthType", "qwen-oauth".parse().unwrap());
        headers.insert(
            http::header::AUTHORIZATION,
            http::HeaderValue::from_str(&auth_header).map_err(|e| {
                LlmMapError::Validation(format!("Invalid authorization header: {}", e))
            })?,
        );
        Ok(request.with_headers(headers))
    }

    /// Transform response (passthrough - Qwen returns OpenAI-compatible format).
    async fn transform_response(
        &self,
        body: serde_json::Value,
        _status: http::StatusCode,
        _headers: &http::HeaderMap,
    ) -> Result<ResponseTransform, Self::Error> {
        // Qwen API returns OpenAI-compatible format, so passthrough
        Ok(ResponseTransform::new(body))
    }

    /// Transform stream chunk (passthrough - Qwen returns OpenAI-compatible format).
    async fn transform_stream_chunk(
        &self,
        chunk: serde_json::Value,
    ) -> Result<StreamChunkTransform, Self::Error> {
        // Qwen API returns OpenAI-compatible format, so passthrough
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
        let _ = OAUTH_CREDS.set(creds);
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

    #[tokio::test]
    async fn test_openai_to_qwen_stream_chunk_passthrough() {
        let adapter = OpenAIToQwenAdapter;
        let chunk = serde_json::json!({
            "id": "chatcmpl-123",
            "choices": [
                {
                    "delta": {"content": "Hello"}
                }
            ]
        });

        let result = adapter.transform_stream_chunk(chunk.clone()).await.unwrap();

        // Verify chunk is passed through unchanged
        assert_eq!(result.data, chunk);
        assert!(result.event.is_none());
    }
}
