use axum::response::sse::KeepAliveStream;
use axum::response::sse::{Event, KeepAlive, Sse};
use std::pin::Pin;
use tokio_stream::Stream;

const EXCLUDED_HEADERS: &[&str] = &[
    "host",
    "connection",
    "keep-alive",
    "transfer-encoding",
    "te",
    "trailer",
    "upgrade",
    "proxy-connection",
    "proxy-authenticate",
    "proxy-authorization",
    "content-length",
    "accept-encoding",
    "authorization",
    "x-api-key",
];

/// Remove excluded headers from the HeaderMap, including both hardcoded headers
/// and dynamically configured headers from exclude_headers config.
pub fn remove_excluded_headers(
    headers: &mut http::header::HeaderMap,
    extra_exclude_headers: Option<&[String]>,
) {
    for key in EXCLUDED_HEADERS {
        headers.remove(*key);
    }
    if let Some(extra_headers) = extra_exclude_headers {
        for header_name in extra_headers {
            if let Ok(key) = header_name.parse::<http::header::HeaderName>() {
                headers.remove(key);
            }
        }
    }
}

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

/// Extracts the path after `/openai/` or `/anthropic/`                                                                                         
///                                                                                                                                             
/// Examples:                                                                                                                                   
/// - `/openai/chat/completions` -> `chat/completions`                                                                                          
/// - `/anthropic/messages` -> `messages`
pub fn extract_endpoint(url: &str) -> Option<&str> {
    url.strip_prefix("/openai/")
        .or_else(|| url.strip_prefix("/anthropic/"))
        .map(|s| s.trim_start_matches('/'))
        .filter(|s| !s.is_empty())
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
    Sse::new(boxed).keep_alive(KeepAlive::new().interval(Duration::from_millis(100)))
}

#[cfg(test)]
mod tests {
    use super::extract_endpoint;

    #[test]
    fn test_extract_endpoint_openai() {
        assert_eq!(
            extract_endpoint("/openai/chat/completions"),
            Some("chat/completions")
        );
        assert_eq!(extract_endpoint("/openai/tt"), Some("tt"));
        assert_eq!(extract_endpoint("/openai/tt/tt"), Some("tt/tt"));
    }

    #[test]
    fn test_extract_endpoint_anthropic() {
        assert_eq!(extract_endpoint("/anthropic/messages"), Some("messages"));
        assert_eq!(
            extract_endpoint("/anthropic/messages/123"),
            Some("messages/123")
        );
    }

    #[test]
    fn test_extract_endpoint_invalid() {
        assert_eq!(extract_endpoint("/google/chat/completions"), None);
        assert_eq!(extract_endpoint("/openai"), None);
        assert_eq!(extract_endpoint("/anthropic"), None);
    }
}
