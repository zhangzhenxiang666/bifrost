//! OpenAI to Qwen adapter - adds Qwen-specific headers for OpenAI-compatible API
//!
//! This adapter transforms OpenAI-compatible requests to Qwen API format by adding
//! the required headers and handling stream options.

use crate::adapter::Adapter;
use crate::adapter::converter::qwen;
use crate::error::LlmMapError;
use crate::model::{RequestContext, RequestTransform};
use async_trait::async_trait;
use serde_json::json;

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
        context: RequestContext<'_>,
    ) -> Result<RequestTransform, Self::Error> {
        // Initialize OAuth credentials manager
        qwen::ensure_oauth_manager_initialized()?;

        // Ensure token is valid (refresh if expired)

        let manager = qwen::OAUTH_CREDS_MANAGER.get().ok_or_else(|| {
            LlmMapError::Validation(
                "OAuth credentials manager not initialized. This should not happen.".to_string(),
            )
        })?;

        manager.ensure_valid_token().await?;
        let access_token = manager.get_access_token();

        // For streaming requests, add stream_options to include usage
        let mut body = context.body;
        if let Some(stream) = body.get("stream").and_then(|v| v.as_bool())
            && stream
            && let Some(obj) = body.as_object_mut()
        {
            obj.insert(
                "stream_options".to_string(),
                json!({
                    "include_usage": true
                }),
            );
        }

        let request = RequestTransform::new(body).with_url(crate::util::join_url_paths(
            &context.provider_config.base_url,
            "chat/completions",
        ));

        // Add Qwen-specific headers using utility function
        let auth_header = format!("Bearer {}", access_token);
        let headers = qwen::add_qwen_headers(&auth_header)?;

        Ok(request.with_headers(headers))
    }
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ProviderConfig;
    use http::HeaderMap;

    /// Initialize OAuth credentials for tests
    fn init_test_credentials() {
        use chrono::Utc;

        // Create a future timestamp (1 year from now)
        let future_date = Utc::now() + chrono::Duration::days(365);

        let creds = qwen::OAuthCredentials {
            access_token: "test_access_token_12345".to_string(),
            token_type: "Bearer".to_string(),
            refresh_token: Some("test_refresh_token_67890".to_string()),
            resource_url: "portal.qwen.ai".to_string(),
            expiry_date: future_date,
        };

        // Initialize the static OnceLock for tests
        let manager = qwen::OAuthCredentialsManager::new(creds);
        let _ = qwen::OAUTH_CREDS_MANAGER.set(manager);
    }

    fn create_test_config() -> ProviderConfig {
        ProviderConfig {
            base_url: "https://portal.qwen.ai/v1".to_string(),
            api_key: "any-key".to_string(),
            endpoint: crate::types::Endpoint::OpenAI,
            adapter: vec![],
            headers: None,
            body: None,
            models: None,
            exclude_headers: None,
            extend: false,
        }
    }

    #[tokio::test]
    async fn test_openai_to_qwen_request_transform() {
        init_test_credentials();

        let adapter = OpenAIToQwenAdapter;
        let body = json!({
            "model": "coder-model",
            "messages": [
                {"role": "user", "content": "Hello"}
            ]
        });
        let config = create_test_config();
        let headers = HeaderMap::new();

        let uri = http::Uri::from_static("https://openai.com/v1");
        let ctx = RequestContext::new(&uri, body.clone(), &config, &headers);
        let result = adapter.transform_request(ctx).await.unwrap();

        // Verify body is unchanged (OpenAI-compatible)
        assert_eq!(result.body, body);

        // Verify Qwen headers are added
        assert!(result.headers.is_some());
        let headers = result.headers.unwrap();
        assert_eq!(headers.get("Content-Type").unwrap(), "application/json");
        assert_eq!(headers.get("X-DashScope-CacheControl").unwrap(), "enable");
        assert_eq!(headers.get("X-DashScope-AuthType").unwrap(), "qwen-oauth");
    }

    #[tokio::test]
    async fn test_openai_to_qwen_streaming_request() {
        init_test_credentials();

        let adapter = OpenAIToQwenAdapter;
        let body = json!({
            "model": "coder-model",
            "messages": [
                {"role": "user", "content": "Hello"}
            ],
            "stream": true
        });
        let config = create_test_config();
        let headers = HeaderMap::new();

        let uri = http::Uri::from_static("https://openai.com/v1");
        let ctx = RequestContext::new(&uri, body, &config, &headers);
        let result = adapter.transform_request(ctx).await.unwrap();

        // Verify stream_options is added for streaming requests
        assert_eq!(
            result.body["stream_options"],
            json!({
                "include_usage": true
            })
        );
    }

    #[tokio::test]
    async fn test_openai_to_qwen_non_streaming_request() {
        init_test_credentials();

        let adapter = OpenAIToQwenAdapter;
        let body = json!({
            "model": "coder-model",
            "messages": [
                {"role": "user", "content": "Hello"}
            ],
            "stream": false
        });
        let config = create_test_config();
        let headers = HeaderMap::new();

        let uri = http::Uri::from_static("https://openai.com/v1");
        let ctx = RequestContext::new(&uri, body, &config, &headers);
        let result = adapter.transform_request(ctx).await.unwrap();

        // Verify stream_options is NOT added for non-streaming requests
        assert!(result.body.get("stream_options").is_none());
    }

    #[tokio::test]
    async fn test_openai_to_qwen_response_passthrough() {
        let adapter = OpenAIToQwenAdapter;
        let body = json!({
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
