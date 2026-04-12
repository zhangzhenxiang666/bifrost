//! Passthrough adapter - passes data through without modification
//!
//! This adapter is used when no transformation is needed. It simply passes
//! the original data through unchanged.

use crate::adapter::{ANTHROPIC_VERSION, Adapter, X_API_KEY};
use crate::error::LlmMapError;
use crate::model::{RequestContext, RequestTransform, StreamChunkTransform};
use crate::types::{Endpoint, ProviderConfig};
use crate::util;
use async_trait::async_trait;
use http::HeaderMap;

/// Passthrough adapter that does not modify any data.
///
/// This adapter is useful when:
/// - No transformation is needed
/// - You want to use the raw provider API directly
/// - For testing purposes
pub struct PassthroughAdapter;

#[async_trait]
impl Adapter for PassthroughAdapter {
    type Error = LlmMapError;

    async fn transform_request(
        &self,
        context: RequestContext<'_>,
    ) -> Result<RequestTransform, Self::Error> {
        let mut request = RequestTransform::new(context.body);
        let mut headers = HeaderMap::new();

        request.url = Some(util::join_url_paths(
            &context.provider_config.base_url,
            util::extract_endpoint(context.uri.path())
                .ok_or_else(|| LlmMapError::Validation("Invalid endpoint".to_string()))?,
        ));

        match context.provider_config.endpoint {
            Endpoint::OpenAI => {
                headers.insert(
                    http::header::AUTHORIZATION,
                    http::header::HeaderValue::from_bytes(
                        format!("Bearer {}", context.provider_config.api_key).as_bytes(),
                    )
                    .unwrap(),
                );
            }
            Endpoint::Anthropic => {
                headers.insert(
                    X_API_KEY.clone(),
                    http::header::HeaderValue::from_bytes(
                        context.provider_config.api_key.as_bytes(),
                    )
                    .unwrap(),
                );
                headers.insert(ANTHROPIC_VERSION.0.clone(), ANTHROPIC_VERSION.1.clone());
                headers.insert(
                    http::header::USER_AGENT,
                    "Anthropic/Python 0.84.0".parse().unwrap(),
                );
            }
        };
        Ok(request.with_headers(headers))
    }

    async fn transform_stream_chunk(
        &self,
        chunk: serde_json::Value,
        event: &str,
        _provider_config: &ProviderConfig,
    ) -> Result<StreamChunkTransform, Self::Error> {
        if !event.is_empty() {
            Ok(StreamChunkTransform::new_with_event(chunk, event))
        } else {
            Ok(StreamChunkTransform::new(chunk))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::HeaderMap;

    #[tokio::test]
    async fn test_passthrough_request_not_modified() {
        let adapter = PassthroughAdapter;
        let body = serde_json::json!({
            "model": "gpt-4",
            "messages": [
                {"role": "user", "content": "Hello"}
            ]
        });
        let config = ProviderConfig {
            base_url: "https://api.example.com".to_string(),
            api_key: "sk-test".to_string(),
            endpoint: crate::types::Endpoint::OpenAI,
            adapter: vec![],
            headers: None,
            body: None,
            models: None,
            exclude_headers: None,
            extend: false,
        };
        let headers = HeaderMap::new();

        let uri = http::Uri::from_static("/openai/chat/completions");
        let ctx = RequestContext::new(&uri, body.clone(), &config, &headers);
        let result = adapter.transform_request(ctx).await.unwrap();

        assert_eq!(result.body, body);
        // Verify URL is set based on endpoint type
        assert_eq!(
            result.url,
            Some("https://api.example.com/chat/completions".to_string())
        );
        assert!(result.headers.is_some());
    }

    #[tokio::test]
    async fn test_passthrough_response_not_modified() {
        let adapter = PassthroughAdapter;
        let body = serde_json::json!({
            "id": "chatcmpl-123",
            "choices": [
                {
                    "message": {"role": "assistant", "content": "Hi there!"}
                }
            ]
        });
        let status = http::StatusCode::OK;
        let headers = HeaderMap::new();

        let result = adapter
            .transform_response(body.clone(), status, &headers)
            .await
            .unwrap();

        assert_eq!(result.body, body);
        assert!(result.status.is_none());
        assert!(result.headers.is_none());
    }
}
