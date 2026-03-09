//! SSE (Server-Sent Events) utilities for gateway
//!
//! This module provides functions to parse SSE events and convert
//! JSON responses to SSE streams.

use axum::response::sse::KeepAliveStream;
use axum::response::sse::{Event, KeepAlive, Sse};
use futures::stream::Stream;
use std::pin::Pin;

/// Type alias for boxed SSE event stream
type BoxedEventStream = Pin<Box<dyn Stream<Item = Result<Event, axum::BoxError>> + Send>>;

/// Helper to create SSE stream with KeepAlive configured for immediate flushing
/// This ensures chunks are sent immediately without buffering
///
/// # Arguments
/// * `stream` - The SSE event stream to wrap
///
/// # Returns
/// An SSE stream with 50ms KeepAlive interval for immediate flushing
#[allow(clippy::type_complexity)]
pub fn create_sse_stream(
    stream: impl Stream<Item = Result<Event, axum::BoxError>> + Send + 'static,
) -> Sse<KeepAliveStream<BoxedEventStream>> {
    use std::time::Duration;

    // Box the stream first, then apply KeepAlive
    let boxed: BoxedEventStream = Box::pin(stream);

    // Create Sse with KeepAlive for immediate flushing
    Sse::new(boxed).keep_alive(KeepAlive::new().interval(Duration::from_millis(50)))
}

/// Check if an event data indicates the stream is done
///
/// # Arguments
/// * `data` - The event data to check
///
/// # Returns
/// true if the data is "[DONE]", false otherwise
pub fn is_done_event(data: &str) -> bool {
    data.starts_with("[DONE]")
}
