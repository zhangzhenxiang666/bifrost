//! Context types for adapter request/response lifecycle
//!
//! This module provides context structures that hold metadata about
//! requests and responses during the adapter transformation process.

use crate::types::{AdapterId, ModelId, ProviderId, RequestId};
use http::HeaderMap;
use std::time::SystemTime;

/// Context information for an outgoing request.
///
/// This structure holds all relevant metadata about a request
/// before it is transformed and sent to the LLM provider.
///
/// # Fields
///
/// * `request_id` - Unique identifier for this request
/// * `adapter_id` - The adapter being used for transformation
/// * `provider_id` - The target LLM provider
/// * `model_id` - The model being requested
/// * `url` - The target URL
/// * `headers` - HTTP headers for the request
/// * `created_at` - When this request context was created
///
/// # Example
///
/// ```rust
/// use llm_map::adapter::RequestContext;
/// use llm_map::types::{AdapterId, ModelId, ProviderId, RequestId};
///
/// let ctx = RequestContext::new(
///     RequestId::new("req-123"),
///     AdapterId::new("openai"),
///     ProviderId::new("openai"),
///     ModelId::new("gpt-4"),
/// );
/// ```
#[derive(Debug, Clone)]
pub struct RequestContext {
    /// Unique identifier for this request
    pub request_id: RequestId,

    /// The adapter being used for transformation
    pub adapter_id: AdapterId,

    /// The target LLM provider
    pub provider_id: ProviderId,

    /// The model being requested
    pub model_id: ModelId,

    /// The target URL (may be modified by adapter)
    pub url: String,

    /// HTTP headers for the request (may be modified by adapter)
    pub headers: HeaderMap,

    /// When this request context was created
    pub created_at: SystemTime,
}

impl RequestContext {
    /// Create a new request context with the given identifiers.
    ///
    /// # Arguments
    ///
    /// * `request_id` - Unique identifier for this request
    /// * `adapter_id` - The adapter being used
    /// * `provider_id` - The target provider
    /// * `model_id` - The model being requested
    ///
    /// The `url` and `headers` fields are initialized to empty defaults,
    /// and `created_at` is set to the current system time.
    pub fn new(
        request_id: RequestId,
        adapter_id: AdapterId,
        provider_id: ProviderId,
        model_id: ModelId,
    ) -> Self {
        Self {
            request_id,
            adapter_id,
            provider_id,
            model_id,
            url: String::new(),
            headers: HeaderMap::new(),
            created_at: SystemTime::now(),
        }
    }

    /// Create a new request context with a target URL.
    ///
    /// This is a convenience constructor that sets the URL in addition
    /// to the basic identifiers.
    pub fn with_url(
        request_id: RequestId,
        adapter_id: AdapterId,
        provider_id: ProviderId,
        model_id: ModelId,
        url: impl Into<String>,
    ) -> Self {
        let mut ctx = Self::new(request_id, adapter_id, provider_id, model_id);
        ctx.url = url.into();
        ctx
    }

    /// Create a new request context with headers.
    ///
    /// This is a convenience constructor that sets the headers in addition
    /// to the basic identifiers.
    pub fn with_headers(
        request_id: RequestId,
        adapter_id: AdapterId,
        provider_id: ProviderId,
        model_id: ModelId,
        headers: HeaderMap,
    ) -> Self {
        let mut ctx = Self::new(request_id, adapter_id, provider_id, model_id);
        ctx.headers = headers;
        ctx
    }
}

/// Context information for an incoming response.
///
/// This structure holds all relevant metadata about a response
/// after it is received from the LLM provider and before it is
/// transformed to the internal format.
///
/// # Fields
///
/// * `request_id` - The request ID this response corresponds to
/// * `adapter_id` - The adapter used for transformation
/// * `provider_id` - The LLM provider that sent the response
/// * `model_id` - The model that generated the response
/// * `status` - HTTP status code of the response
/// * `headers` - HTTP headers from the response
/// * `received_at` - When this response was received
///
/// # Example
///
/// ```rust
/// use llm_map::adapter::ResponseContext;
/// use llm_map::types::{AdapterId, ModelId, ProviderId, RequestId};
/// use http::StatusCode;
///
/// let ctx = ResponseContext::new(
///     RequestId::new("req-123"),
///     AdapterId::new("openai"),
///     ProviderId::new("openai"),
///     ModelId::new("gpt-4"),
///     StatusCode::OK,
/// );
/// ```
#[derive(Debug, Clone)]
pub struct ResponseContext {
    /// The request ID this response corresponds to
    pub request_id: RequestId,

    /// The adapter used for transformation
    pub adapter_id: AdapterId,

    /// The LLM provider that sent the response
    pub provider_id: ProviderId,

    /// The model that generated the response
    pub model_id: ModelId,

    /// HTTP status code of the response
    pub status: http::StatusCode,

    /// HTTP headers from the response
    pub headers: HeaderMap,

    /// When this response was received
    pub received_at: SystemTime,
}

impl ResponseContext {
    /// Create a new response context with the given identifiers and status.
    ///
    /// # Arguments
    ///
    /// * `request_id` - The corresponding request ID
    /// * `adapter_id` - The adapter used
    /// * `provider_id` - The provider that sent the response
    /// * `model_id` - The model that generated the response
    /// * `status` - HTTP status code
    ///
    /// The `headers` field is initialized to an empty [`HeaderMap`],
    /// and `received_at` is set to the current system time.
    pub fn new(
        request_id: RequestId,
        adapter_id: AdapterId,
        provider_id: ProviderId,
        model_id: ModelId,
        status: http::StatusCode,
    ) -> Self {
        Self {
            request_id,
            adapter_id,
            provider_id,
            model_id,
            status,
            headers: HeaderMap::new(),
            received_at: SystemTime::now(),
        }
    }

    /// Create a new response context with headers.
    ///
    /// This is a convenience constructor that sets the response headers
    /// in addition to the basic identifiers.
    pub fn with_headers(
        request_id: RequestId,
        adapter_id: AdapterId,
        provider_id: ProviderId,
        model_id: ModelId,
        status: http::StatusCode,
        headers: HeaderMap,
    ) -> Self {
        Self {
            request_id,
            adapter_id,
            provider_id,
            model_id,
            status,
            headers,
            received_at: SystemTime::now(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_context_new() {
        let ctx = RequestContext::new(
            RequestId::new("req-123"),
            AdapterId::new("openai"),
            ProviderId::new("openai"),
            ModelId::new("gpt-4"),
        );

        assert_eq!(ctx.request_id.as_ref(), "req-123");
        assert_eq!(ctx.adapter_id.as_ref(), "openai");
        assert_eq!(ctx.provider_id.as_ref(), "openai");
        assert_eq!(ctx.model_id.as_ref(), "gpt-4");
        assert!(ctx.url.is_empty());
        assert!(ctx.headers.is_empty());
    }

    #[test]
    fn test_request_context_with_url() {
        let ctx = RequestContext::with_url(
            RequestId::new("req-123"),
            AdapterId::new("openai"),
            ProviderId::new("openai"),
            ModelId::new("gpt-4"),
            "https://api.openai.com/v1/chat/completions",
        );

        assert_eq!(ctx.url, "https://api.openai.com/v1/chat/completions");
    }

    #[test]
    fn test_response_context_new() {
        let ctx = ResponseContext::new(
            RequestId::new("req-123"),
            AdapterId::new("openai"),
            ProviderId::new("openai"),
            ModelId::new("gpt-4"),
            http::StatusCode::OK,
        );

        assert_eq!(ctx.request_id.as_ref(), "req-123");
        assert_eq!(ctx.status, http::StatusCode::OK);
        assert!(ctx.headers.is_empty());
    }

    #[test]
    fn test_response_context_with_headers() {
        let mut headers = HeaderMap::new();
        headers.insert("Content-Type", "application/json".parse().unwrap());

        let ctx = ResponseContext::with_headers(
            RequestId::new("req-123"),
            AdapterId::new("openai"),
            ProviderId::new("openai"),
            ModelId::new("gpt-4"),
            http::StatusCode::OK,
            headers.clone(),
        );

        assert_eq!(ctx.headers.get("Content-Type").unwrap(), "application/json");
    }
}
