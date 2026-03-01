//! Anthropic-compatible route for messages endpoint
//!
//! This module provides the Anthropic-compatible `/v1/messages` endpoint
//! that supports both streaming and non-streaming requests.

use axum::{
    extract::State,
    Json,
};
use serde_json::Value;

use crate::error::{LlmMapError, Result};
use crate::types::response::GatewayResponse;
use crate::utils::sse::convert_to_sse;

/// Application state for route handlers
/// Re-exported from openai module for consistency
pub use super::openai::AppState;

/// Parse `provider@model` format into provider ID and model name.
///
/// # Arguments
/// * `model` - The model string in format `provider@model` or just `model`
///
/// # Returns
/// * `Ok((provider_id, model_name))` if parsing succeeds
/// * `Err(LlmMapError)` if the format is invalid
///
/// # Examples
/// ```
/// // With provider: "qwen-code@claude-3" -> ("qwen-code", "claude-3")
/// // Without provider: "claude-3" -> returns error
/// ```
fn parse_model(model: &str) -> Result<(&str, &str)> {
    model
        .split_once('@')
        .ok_or_else(|| LlmMapError::Validation(
            "Invalid model format. Expected 'provider@model' format".to_string()
        ))
}

/// Anthropic-compatible messages endpoint.
///
/// This handler accepts POST requests to `/v1/messages` and:
/// - Parses the `model` field in `provider@model` format
/// - Builds an adapter chain for the specified provider
/// - Executes the request through the adapter chain
/// - Returns either JSON or SSE stream based on the `stream` field
///
/// # Arguments
/// * `state` - Application state containing the provider registry
/// * `body` - The request body as JSON value
///
/// # Returns
/// * `GatewayResponse::Json` for non-streaming requests
/// * `GatewayResponse::Sse` for streaming requests
///
/// # Errors
/// * `400 Bad Request` - Invalid model format or missing fields
/// * `404 Not Found` - Provider not found in registry
/// * `500 Internal Server Error` - Adapter execution failed
#[axum::debug_handler]
pub async fn messages(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> Result<GatewayResponse> {
    // Extract stream flag (default to false)
    let is_stream = body
        .get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // Extract model field
    let model = body
        .get("model")
        .and_then(|v| v.as_str())
        .ok_or_else(|| LlmMapError::Validation(
            "Missing required field: model".to_string()
        ))?;

    // Parse provider@model format
    let (provider_id, _model_name) = parse_model(model)?;

    // Build adapter chain for the provider
    let executor = state.registry.build_executor(provider_id)?;

    // Execute the request through the adapter chain
    let headers = http::HeaderMap::new();
    let transform = executor.execute_request(body.clone(), &headers).await?;

    // For now, we just echo back the transformed request as response
    // In a real implementation, this would make an HTTP call to the upstream provider
    let response_body = transform.body;

    // Return appropriate response type based on stream flag
    Ok(if is_stream {
        GatewayResponse::Sse(convert_to_sse(response_body))
    } else {
        GatewayResponse::Json(Json(response_body))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, Endpoint, ProviderConfig};
    use crate::provider::registry::ProviderRegistry;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use serde_json::json;
    use std::collections::HashMap;
    use tower::util::ServiceExt;

    /// Create a test configuration with a single provider
    fn create_test_config() -> Config {
        let mut provider = HashMap::new();
        provider.insert(
            "test-provider".to_string(),
            ProviderConfig {
                base_url: "https://api.test.com".to_string(),
                api_key: "test-key".to_string(),
                endpoint: Endpoint::Anthropic,
                adapter: vec![],
                headers: vec![],
                body: vec![],
                models: vec![],
            },
        );

        Config {
            provider,
            server: crate::config::ServerConfig::default(),
        }
    }

    /// Create test app state
    fn create_test_state() -> AppState {
        let config = create_test_config();
        let registry = ProviderRegistry::from_config(&config);
        AppState { registry }
    }

    // ========== parse_model tests ==========

    #[test]
    fn test_parse_model_with_provider() {
        let result = parse_model("qwen-code@claude-3");
        assert!(result.is_ok());
        let (provider_id, model_name) = result.unwrap();
        assert_eq!(provider_id, "qwen-code");
        assert_eq!(model_name, "claude-3");
    }

    #[test]
    fn test_parse_model_without_provider() {
        let result = parse_model("claude-3");
        assert!(result.is_err());
        
        if let Err(e) = result {
            assert!(e.to_string().contains("provider@model"));
        }
    }

    #[test]
    fn test_parse_model_empty_string() {
        let result = parse_model("");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_model_multiple_at_signs() {
        let result = parse_model("provider@model@extra");
        assert!(result.is_ok());
        let (provider_id, model_name) = result.unwrap();
        assert_eq!(provider_id, "provider");
        assert_eq!(model_name, "model@extra");
    }

    // ========== messages handler tests ==========

    #[tokio::test]
    async fn test_messages_non_stream_request() {
        let state = create_test_state();
        let app = axum::Router::new()
            .route("/v1/messages", axum::routing::post(messages))
            .with_state(state);

        let request = Request::builder()
            .method("POST")
            .uri("/v1/messages")
            .header("Content-Type", "application/json")
            .body(Body::from(
                serde_json::to_string(&json!({
                    "model": "test-provider@claude-3",
                    "messages": [
                        {"role": "user", "content": "Hello"}
                    ],
                    "max_tokens": 1024,
                    "stream": false
                })).unwrap()
            ))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_messages_stream_request() {
        let state = create_test_state();
        let app = axum::Router::new()
            .route("/v1/messages", axum::routing::post(messages))
            .with_state(state);

        let request = Request::builder()
            .method("POST")
            .uri("/v1/messages")
            .header("Content-Type", "application/json")
            .body(Body::from(
                serde_json::to_string(&json!({
                    "model": "test-provider@claude-3",
                    "messages": [
                        {"role": "user", "content": "Hello"}
                    ],
                    "max_tokens": 1024,
                    "stream": true
                })).unwrap()
            ))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        
        assert_eq!(response.status(), StatusCode::OK);
        // SSE responses should have text/event-stream content type
        let content_type = response.headers().get("content-type");
        assert!(content_type.is_some());
        assert!(content_type.unwrap().to_str().unwrap().contains("text/event-stream"));
    }

    #[tokio::test]
    async fn test_messages_missing_model() {
        let state = create_test_state();
        let app = axum::Router::new()
            .route("/v1/messages", axum::routing::post(messages))
            .with_state(state);

        let request = Request::builder()
            .method("POST")
            .uri("/v1/messages")
            .header("Content-Type", "application/json")
            .body(Body::from(
                serde_json::to_string(&json!({
                    "messages": [
                        {"role": "user", "content": "Hello"}
                    ],
                    "max_tokens": 1024
                })).unwrap()
            ))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_messages_invalid_model_format() {
        let state = create_test_state();
        let app = axum::Router::new()
            .route("/v1/messages", axum::routing::post(messages))
            .with_state(state);

        let request = Request::builder()
            .method("POST")
            .uri("/v1/messages")
            .header("Content-Type", "application/json")
            .body(Body::from(
                serde_json::to_string(&json!({
                    "model": "invalid-format-without-at-sign",
                    "messages": [
                        {"role": "user", "content": "Hello"}
                    ],
                    "max_tokens": 1024
                })).unwrap()
            ))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_messages_provider_not_found() {
        let state = create_test_state();
        let app = axum::Router::new()
            .route("/v1/messages", axum::routing::post(messages))
            .with_state(state);

        let request = Request::builder()
            .method("POST")
            .uri("/v1/messages")
            .header("Content-Type", "application/json")
            .body(Body::from(
                serde_json::to_string(&json!({
                    "model": "non-existent-provider@claude-3",
                    "messages": [
                        {"role": "user", "content": "Hello"}
                    ],
                    "max_tokens": 1024
                })).unwrap()
            ))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    }

    #[tokio::test]
    async fn test_messages_default_stream_false() {
        let state = create_test_state();
        let app = axum::Router::new()
            .route("/v1/messages", axum::routing::post(messages))
            .with_state(state);

        // Don't specify stream field - should default to false (JSON response)
        let request = Request::builder()
            .method("POST")
            .uri("/v1/messages")
            .header("Content-Type", "application/json")
            .body(Body::from(
                serde_json::to_string(&json!({
                    "model": "test-provider@claude-3",
                    "messages": [
                        {"role": "user", "content": "Hello"}
                    ],
                    "max_tokens": 1024
                })).unwrap()
            ))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        
        assert_eq!(response.status(), StatusCode::OK);
    }
}
