//! Adapter module for LLM provider integrations
//!
//! This module provides the core trait and types for implementing LLM provider adapters.
//! Adapters transform requests and responses between the internal format and provider-specific formats.

pub mod builtin;
pub mod chain;
pub mod util;

use crate::config::ProviderConfig;
use crate::model::{RequestTransform, ResponseTransform, StreamChunkTransform};

pub use builtin::PassthroughAdapter;
pub use chain::OnionExecutor;

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
    /// * `body` - The original request body as JSON
    /// * `provider_config` - The provider configuration containing base_url, api_key, etc.
    /// * `headers` - The HTTP headers to be sent
    ///
    /// # Returns
    ///
    /// A [`RequestTransform`] containing the modified request data.
    /// Use [`RequestTransform::with_url`] and [`RequestTransform::with_headers`]
    /// to specify changes to URL and headers.
    async fn transform_request(
        &self,
        body: serde_json::Value,
        provider_config: &ProviderConfig,
        headers: &http::HeaderMap,
    ) -> Result<RequestTransform, Self::Error>;

    /// Transform an incoming response from the LLM provider.
    ///
    /// This method is called with the response body, status code, and headers.
    /// The adapter can modify these to match the expected internal format.
    ///
    /// # Arguments
    ///
    /// * `body` - The response body as JSON
    /// * `status` - The HTTP status code
    /// * `headers` - The response headers
    ///
    /// # Returns
    ///
    /// A [`ResponseTransform`] containing the modified response data.
    #[allow(unused_variables)]
    async fn transform_response(
        &self,
        body: serde_json::Value,
        status: http::StatusCode,
        headers: &http::HeaderMap,
    ) -> Result<ResponseTransform, Self::Error> {
        Ok(ResponseTransform::new(body))
    }

    /// Transform a streaming response chunk from the LLM provider.
    ///
    /// This method is called for each chunk in a streaming response.
    /// The adapter can modify the chunk data and optionally specify an SSE event type.
    ///
    /// # Arguments
    ///
    /// * `chunk` - The streaming chunk data as JSON
    ///
    /// # Returns
    ///
    /// A [`StreamChunkTransform`] containing the modified chunk data.
    /// Use [`StreamChunkTransform::with_event`] to specify an SSE event type.
    #[allow(unused_variables)]
    async fn transform_stream_chunk(
        &self,
        chunk: serde_json::Value,
        event: &str,
        provider_config: &ProviderConfig,
    ) -> Result<StreamChunkTransform, Self::Error> {
        Ok(StreamChunkTransform::new(chunk))
    }
}
