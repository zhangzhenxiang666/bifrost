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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_transform_new() {
        let body = serde_json::json!({"key": "value"});
        let transform = RequestTransform::new(body.clone());
        assert_eq!(transform.body, body);
        assert!(transform.url.is_none());
        assert!(transform.headers.is_none());
    }

    #[test]
    fn test_request_transform_with_url() {
        let body = serde_json::json!({"key": "value"});
        let transform = RequestTransform::new(body).with_url("https://api.example.com");
        assert_eq!(transform.url, Some("https://api.example.com".to_string()));
    }

    #[test]
    fn test_response_transform_new() {
        let body = serde_json::json!({"result": "ok"});
        let transform = ResponseTransform::new(body.clone());
        assert_eq!(transform.body, body);
        assert!(transform.status.is_none());
        assert!(transform.headers.is_none());
    }

    #[test]
    fn test_response_transform_with_status() {
        let body = serde_json::json!({"result": "ok"});
        let transform = ResponseTransform::new(body).with_status(http::StatusCode::OK);
        assert_eq!(transform.status, Some(http::StatusCode::OK));
    }

    #[test]
    fn test_stream_chunk_transform_new() {
        let data = serde_json::json!({"choices": []});
        let transform = StreamChunkTransform::new(data.clone());
        assert_eq!(transform.data(), Some(&data));
        assert_eq!(transform.events.len(), 1);
    }
}
