use axum::response::sse::KeepAliveStream;
use axum::response::sse::{Event, KeepAlive, Sse};
use tokio_stream::Stream;
use std::pin::Pin;

pub fn extend_overwrite(base: &mut http::header::HeaderMap, other: http::header::HeaderMap) {
    let mut last_key: Option<http::header::HeaderName> = None;

    for (key, value) in other {
        match key {
            Some(k) => {
                // New key encountered: remove existing values from base to ensure overwrite semantics
                base.remove(&k);
                base.append(k.clone(), value);
                last_key = Some(k);
            }
            None => {
                // Subsequent value for the same key (already removed above), just append
                if let Some(ref k) = last_key {
                    base.append(k.clone(), value);
                }
            }
        }
    }
}

/// Join two URL path components, handling slashes properly
pub fn join_url_paths(base: &str, path: &str) -> String {
    let base = base.trim_end_matches('/');
    let path = path.trim_start_matches('/');
    format!("{}/{}", base, path)
}

/// Parse `provider@model` format into provider ID and model name
pub fn parse_model(model: &str) -> crate::error::Result<(&str, &str)> {
    model.split_once('@').ok_or_else(|| {
        crate::error::LlmMapError::Validation(
            "Invalid model format. Expected 'provider@model' format".to_string(),
        )
    })
}

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
