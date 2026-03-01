//! Types module for shared data structures

use std::fmt::Display;
use std::ops::Deref;

// ============================================================================
// Newtype 类型定义
// ============================================================================

/// API Key - 用于认证
#[derive(Debug, Clone)]
pub struct ApiKey(String);

impl ApiKey {
    pub fn new(key: impl Into<String>) -> Self {
        Self(key.into())
    }

    /// 隐藏敏感信息：`sk-verylongkey12345` → `sk-ve****2345`
    pub fn mask(&self) -> String {
        let key = &self.0;
        if key.len() < 8 {
            return "***".to_string();
        }
        format!("{}****{}", &key[..5], &key[key.len() - 4..])
    }
}

impl Deref for ApiKey {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsRef<str> for ApiKey {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl Display for ApiKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.mask())
    }
}

/// Model ID - 模型标识
#[derive(Debug, Clone)]
pub struct ModelId(String);

impl ModelId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
}

impl Deref for ModelId {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsRef<str> for ModelId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl Display for ModelId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", &self.0)
    }
}

/// Provider ID - 提供商标识
#[derive(Debug, Clone)]
pub struct ProviderId(String);

impl ProviderId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
}

impl Deref for ProviderId {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsRef<str> for ProviderId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl Display for ProviderId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", &self.0)
    }
}

/// Adapter ID - 适配器标识
#[derive(Debug, Clone)]
pub struct AdapterId(String);

impl AdapterId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
}

impl Deref for AdapterId {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsRef<str> for AdapterId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl Display for AdapterId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", &self.0)
    }
}

/// Request ID - 请求标识
#[derive(Debug, Clone)]
pub struct RequestId(String);

impl RequestId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
}

impl Deref for RequestId {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsRef<str> for RequestId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl Display for RequestId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", &self.0)
    }
}

// ============================================================================
// Transform 类型定义
// ============================================================================

/// 请求转换结果
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

/// 响应转换结果
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

/// 流式块转换结果
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

    // ========== Newtype 类型测试 ==========

    #[test]
    fn test_api_key_new() {
        let key = ApiKey::new("sk-test123456");
        assert_eq!(key.as_ref(), "sk-test123456");
    }

    #[test]
    fn test_api_key_mask_long() {
        let key = ApiKey::new("sk-verylongkey12345");
        assert_eq!(key.mask(), "sk-ve****2345");
    }

    #[test]
    fn test_api_key_mask_short() {
        let key = ApiKey::new("short");
        assert_eq!(key.mask(), "***");
    }

    #[test]
    fn test_api_key_display() {
        let key = ApiKey::new("sk-test123456");
        assert_eq!(format!("{}", key), "sk-te****3456");
    }

    #[test]
    fn test_model_id() {
        let id = ModelId::new("gpt-4");
        assert_eq!(id.as_ref(), "gpt-4");
        assert_eq!(format!("{}", id), "gpt-4");
    }

    #[test]
    fn test_provider_id() {
        let id = ProviderId::new("openai");
        assert_eq!(id.as_ref(), "openai");
        assert_eq!(format!("{}", id), "openai");
    }

    #[test]
    fn test_adapter_id() {
        let id = AdapterId::new("passthrough");
        assert_eq!(id.as_ref(), "passthrough");
        assert_eq!(format!("{}", id), "passthrough");
    }

    #[test]
    fn test_request_id() {
        let id = RequestId::new("req-123");
        assert_eq!(id.as_ref(), "req-123");
        assert_eq!(format!("{}", id), "req-123");
    }

    // ========== Transform 类型测试 ==========

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
    fn test_request_transform_with_headers() {
        let body = serde_json::json!({"key": "value"});
        let mut headers = http::HeaderMap::new();
        headers.insert("Content-Type", "application/json".parse().unwrap());
        let transform = RequestTransform::new(body).with_headers(headers.clone());
        assert!(transform.headers.is_some());
        assert_eq!(
            transform.headers.unwrap().get("Content-Type").unwrap(),
            "application/json"
        );
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

    #[test]
    fn test_stream_chunk_transform_with_event() {
        let data = serde_json::json!({"choices": []});
        let transform = StreamChunkTransform::new(data).with_event("message");
        assert_eq!(transform.event, Some("message".to_string()));
    }
}
