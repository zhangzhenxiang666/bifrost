//! Types module for shared data structures

use http::HeaderMap;

/// Context for request transformation containing all input parameters.
///
/// Adapters can access URI, body, provider config, and headers through this struct.
/// New fields can be added here in the future without changing existing adapter implementations.
pub struct RequestContext {
    pub body: serde_json::Value,
}

impl RequestContext {
    pub fn new(body: serde_json::Value) -> Self {
        Self { body }
    }
}

/// Context for response transformation containing all input parameters.
pub struct ResponseContext<'a> {
    pub body: serde_json::Value,
    pub status: http::StatusCode,
    pub headers: &'a HeaderMap,
}

impl<'a> ResponseContext<'a> {
    pub fn new(body: serde_json::Value, status: http::StatusCode, headers: &'a HeaderMap) -> Self {
        Self {
            body,
            status,
            headers,
        }
    }
}

/// Context for stream chunk transformation containing all input parameters.
pub struct StreamChunkContext<'a> {
    pub chunk: serde_json::Value,
    pub event: &'a str,
}

impl<'a> StreamChunkContext<'a> {
    pub fn new(chunk: serde_json::Value, event: &'a str) -> Self {
        Self { chunk, event }
    }
}

pub struct RequestTransform {
    pub body: serde_json::Value,
}

impl RequestTransform {
    pub fn new(body: serde_json::Value) -> Self {
        Self { body }
    }
}

pub struct ResponseTransform {
    pub body: serde_json::Value,
    pub status: Option<http::StatusCode>,
    pub headers: Option<http::HeaderMap>,
}

impl ResponseTransform {
    pub fn new(body: serde_json::Value) -> Self {
        Self {
            body,
            status: None,
            headers: None,
        }
    }

    pub fn with_status(mut self, status: http::StatusCode) -> Self {
        self.status = Some(status);
        self
    }

    pub fn with_headers(mut self, headers: http::HeaderMap) -> Self {
        self.headers = Some(headers);
        self
    }
}

/// Represents a transformed stream chunk with multiple events
pub struct StreamChunkTransform {
    /// List of (data, event_name) pairs - always kept in sync
    pub events: Vec<(serde_json::Value, Option<String>)>,
}

impl StreamChunkTransform {
    /// Create a transform with a single event (no event name)
    pub fn new(data: serde_json::Value) -> Self {
        Self {
            events: vec![(data, None)],
        }
    }

    pub fn new_empty() -> Self {
        Self { events: vec![] }
    }

    /// Create a transform with a single event and event name
    pub fn new_with_event(data: serde_json::Value, event: impl Into<String>) -> Self {
        Self {
            events: vec![(data, Some(event.into()))],
        }
    }

    /// Create a transform with multiple events
    pub fn new_multi(events: Vec<(serde_json::Value, Option<String>)>) -> Self {
        Self { events }
    }

    /// Get the first event's data (for adapter chain compatibility)
    pub fn data(&self) -> Option<&serde_json::Value> {
        self.events.first().map(|(data, _)| data)
    }

    /// Get all events for final processing
    pub fn into_events(self) -> Vec<(serde_json::Value, Option<String>)> {
        self.events
    }
}
