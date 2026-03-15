//! Types module for shared data structures

pub struct RequestTransform {
    pub body: serde_json::Value,
    pub url: Option<String>,
    pub headers: Option<http::HeaderMap>,
}

impl RequestTransform {
    pub fn new(body: serde_json::Value) -> Self {
        Self {
            body,
            url: None,
            headers: None,
        }
    }

    pub fn with_url(mut self, url: impl Into<String>) -> Self {
        self.url = Some(url.into());
        self
    }

    pub fn with_headers(mut self, headers: http::HeaderMap) -> Self {
        self.headers = Some(headers);
        self
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

/// Function type for building endpoint URLs
pub type UrlBuilder = dyn Fn(&str, &str) -> String + Send + Sync;

/// Configuration for an endpoint
pub struct EndpointConfig {
    /// Default URL path pattern for this endpoint
    pub default_path_pattern: String,
    /// Custom URL builder function (optional)
    pub url_builder: Option<Box<UrlBuilder>>,
}

impl EndpointConfig {
    /// Create a new endpoint configuration with a simple path pattern
    pub fn new(default_path_pattern: impl Into<String>) -> Self {
        Self {
            default_path_pattern: default_path_pattern.into(),
            url_builder: None,
        }
    }

    /// Create a new endpoint configuration with a custom URL builder
    pub fn with_builder<F>(url_builder: F) -> Self
    where
        F: Fn(&str, &str) -> String + Send + Sync + 'static,
    {
        Self {
            default_path_pattern: String::new(),
            url_builder: Some(Box::new(url_builder)),
        }
    }

    /// Build the URL for this endpoint
    pub fn build_url(&self, base_url: &str, model: &str) -> String {
        if let Some(builder) = &self.url_builder {
            builder(base_url, model)
        } else {
            let path = self.default_path_pattern.replace("{model}", model);
            crate::util::join_url_paths(base_url, &path)
        }
    }
}
