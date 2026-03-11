//! Qwen-specific HTTP headers utility
//!
//! This module provides functions to add Qwen-specific headers to HTTP requests.

use crate::error::LlmMapError;

/// Helper function to add Qwen-specific headers to a request
pub fn add_qwen_headers(auth_header: &str) -> Result<http::HeaderMap, LlmMapError> {
    let mut headers = http::HeaderMap::new();
    headers.insert(
        http::header::CONTENT_TYPE,
        "application/json".parse().unwrap(),
    );
    headers.insert(
        http::header::USER_AGENT,
        "QwenCode/0.11.0 (linux; x64)".parse().unwrap(),
    );
    headers.insert(http::header::ACCEPT, "application/json".parse().unwrap());
    headers.insert("X-DashScope-CacheControl", "enable".parse().unwrap());
    headers.insert(
        "X-DashScope-UserAgent",
        "QwenCode/0.11.0 (linux; x64)".parse().unwrap(),
    );
    headers.insert("X-DashScope-AuthType", "qwen-oauth".parse().unwrap());
    headers.insert(
        http::header::AUTHORIZATION,
        http::HeaderValue::from_str(auth_header)
            .map_err(|e| LlmMapError::Validation(format!("Invalid authorization header: {}", e)))?,
    );
    Ok(headers)
}
