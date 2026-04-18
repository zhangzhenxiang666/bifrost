//! Passthrough adapter - passes data through without modification
//!
//! This adapter is used when no transformation is needed. It simply passes
//! the original data through unchanged.

use crate::adapter::Adapter;
use crate::error::LlmMapError;
use crate::model::{RequestContext, RequestTransform};
use async_trait::async_trait;

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
        context: RequestContext,
    ) -> Result<RequestTransform, Self::Error> {
        Ok(RequestTransform::new(context.body))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ResponseContext;
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

        let ctx = RequestContext::new(body.clone());
        let result = adapter.transform_request(ctx).await.unwrap();

        assert_eq!(result.body, body);
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
            .transform_response(ResponseContext::new(body.clone(), status, &headers))
            .await
            .unwrap();

        assert_eq!(result.body, body);
        assert!(result.status.is_none());
        assert!(result.headers.is_none());
    }
}
