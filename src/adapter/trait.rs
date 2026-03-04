//! Adapter trait definition for LLM provider integrations
//!
//! This module defines the core [`Adapter`] trait that all LLM provider adapters must implement.
//! The trait uses [`macro@async_trait`] to allow async methods in traits.

use crate::config::ProviderConfig;
use crate::types::{RequestTransform, ResponseTransform, StreamChunkTransform};
use async_trait::async_trait;

/// Core trait for LLM provider adapters.
///
/// All LLM provider implementations (OpenAI, Anthropic, etc.) must implement this trait.
/// The trait provides three transformation methods that handle the request/response lifecycle:
///
/// - [`transform_request`](Adapter::transform_request) - Transform outgoing requests
/// - [`transform_response`](Adapter::transform_response) - Transform incoming responses
/// - [`transform_stream_chunk`](Adapter::transform_stream_chunk) - Transform streaming chunks
///
/// # Example
///
/// ```rust,no_run
/// use async_trait::async_trait;
/// use llm_map::adapter::Adapter;
/// use llm_map::config::{Endpoint, ProviderConfig};
/// use llm_map::types::{RequestTransform, ResponseTransform, StreamChunkTransform};
///
/// struct MyAdapter;
///
/// #[async_trait]
/// impl Adapter for MyAdapter {
///     type Error = llm_map::error::LlmMapError;
///
///     async fn transform_request(
///         &self,
///         body: serde_json::Value,
///         provider_config: &ProviderConfig,
///         headers: &http::HeaderMap,
///     ) -> Result<RequestTransform, Self::Error> {
///         Ok(RequestTransform::new(body))
///     }
///
///     async fn transform_response(
///         &self,
///         body: serde_json::Value,
///         status: http::StatusCode,
///         headers: &http::HeaderMap,
///     ) -> Result<ResponseTransform, Self::Error> {
///         Ok(ResponseTransform::new(body))
///     }
///
///     async fn transform_stream_chunk(
///         &self,
///         chunk: serde_json::Value,
///     ) -> Result<StreamChunkTransform, Self::Error> {
///         Ok(StreamChunkTransform::new(chunk))
///     }
/// }

#[async_trait]
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
    async fn transform_response(
        &self,
        body: serde_json::Value,
        status: http::StatusCode,
        headers: &http::HeaderMap,
    ) -> Result<ResponseTransform, Self::Error>;

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
    async fn transform_stream_chunk(
        &self,
        chunk: serde_json::Value,
    ) -> Result<StreamChunkTransform, Self::Error>;
}
