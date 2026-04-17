//! Adapter module for LLM provider integrations
//!
//! This module provides the core trait and types for implementing LLM provider adapters.
//! Adapters transform requests and responses between the internal format and provider-specific formats.
//!
//! ## Modules
//!
//! - `builtin` - Built-in adapter implementations
//! - `chain` - Onion-style adapter chain execution
//! - `converter` - Shared format conversion utilities
//! - `util` - Legacy utilities (deprecated, use `converter` instead)

pub mod builtin;
pub mod chain;
pub mod converter;

use crate::model::{
    RequestContext, RequestTransform, ResponseContext, ResponseTransform, StreamChunkContext,
    StreamChunkTransform,
};

pub static X_API_KEY: http::HeaderName = http::header::HeaderName::from_static("x-api-key");
pub static ANTHROPIC_VERSION: (http::HeaderName, http::header::HeaderValue) = (
    http::header::HeaderName::from_static("anthropic-version"),
    http::header::HeaderValue::from_static("2023-06-01"),
);

#[async_trait::async_trait]
pub trait Adapter: Send + Sync {
    /// The error type returned by this adapter.
    ///
    /// Should implement [`std::error::Error`] and be [`Send`] + [`Sync`] for thread safety.
    type Error: std::error::Error + Send + Sync;

    /// Transform an outgoing request before sending to the LLM provider.
    ///
    /// This method is called with the original request body and headers.
    /// The adapter can access the provider configuration (base_url, api_key, headers, body)
    /// and modify the request to match the provider's API format.
    ///
    /// # Arguments
    ///
    /// * `context` - The request context containing URI, body, provider config, and headers
    ///
    /// # Returns
    ///
    /// A [`RequestTransform`] containing the modified request data.
    /// Use [`RequestTransform::with_url`] and [`RequestTransform::with_headers`]
    /// to specify changes to URL and headers.
    async fn transform_request(
        &self,
        context: RequestContext<'_>,
    ) -> Result<RequestTransform, Self::Error>;

    /// Transform an incoming response from the LLM provider.
    ///
    /// This method is called with the response body, status code, and headers.
    /// The adapter can modify these to match the expected internal format.
    ///
    /// # Arguments
    ///
    /// * `context` - The response context containing body, status, and headers
    ///
    /// # Returns
    ///
    /// A [`ResponseTransform`] containing the modified response data.
    async fn transform_response(
        &self,
        context: ResponseContext<'_>,
    ) -> Result<ResponseTransform, Self::Error> {
        Ok(ResponseTransform::new(context.body))
    }

    /// Transform a streaming response chunk from the LLM provider.
    ///
    /// This method is called for each chunk in a streaming response.
    /// The adapter can modify the chunk data and optionally specify an SSE event type.
    ///
    /// # Arguments
    ///
    /// * `context` - The stream chunk context containing chunk, event, and provider config
    ///
    /// # Returns
    ///
    /// A [`StreamChunkTransform`] containing the modified chunk data.
    /// Use [`StreamChunkTransform::with_event`] to specify an SSE event type.
    async fn transform_stream_chunk(
        &self,
        context: StreamChunkContext<'_>,
    ) -> Result<StreamChunkTransform, Self::Error> {
        Ok(StreamChunkTransform::new(context.chunk))
    }
}
