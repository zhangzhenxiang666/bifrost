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

pub struct StreamChunkTransform {
    pub data: serde_json::Value,
    pub event: Option<String>,
}

impl StreamChunkTransform {
    pub fn new(data: serde_json::Value) -> Self {
        Self { data, event: None }
    }

    pub fn with_event(mut self, event: impl Into<String>) -> Self {
        self.event = Some(event.into());
        self
    }
}

// ============================================================================
// 单元测试
// ============================================================================

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
        assert_eq!(transform.data, data);
        assert!(transform.event.is_none());
    }
}
