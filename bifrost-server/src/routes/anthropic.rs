//! Anthropic-compatible route for messages endpoint

use crate::routes::handler;
use crate::state::AppState;
use crate::{error::Result, model::EndpointConfig};
use axum::{Json, extract::State};
use serde_json::Value;
use std::sync::OnceLock;

/// Endpoint configuration for Anthropic-compatible endpoints
fn anthropic_config() -> &'static EndpointConfig {
    static CONFIG: OnceLock<EndpointConfig> = OnceLock::new();
    CONFIG.get_or_init(|| EndpointConfig::new("/v1/messages"))
}

/// Anthropic-compatible messages endpoint.
#[axum::debug_handler]
pub async fn messages(
    State(state): State<AppState>,
    headers: http::header::HeaderMap,
    Json(body): Json<Value>,
) -> Result<axum::response::Response> {
    let is_stream = body
        .get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    handler::handle_llm_request(&state, &headers, body, anthropic_config(), is_stream).await
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
    use std::sync::Arc;
    use tower::util::ServiceExt;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn create_test_config(mock_server_uri: &str) -> Config {
        let mut provider = HashMap::new();
        provider.insert(
            "test-provider".to_string(),
            ProviderConfig {
                base_url: mock_server_uri.to_string(),
                api_key: "test-key".to_string(),
                endpoint: Endpoint::Anthropic,
                adapter: vec![],
                headers: None,
                body: None,
                models: None,
            },
        );
        Config {
            provider,
            server: crate::config::ServerConfig::default(),
        }
    }

    fn create_test_state(mock_server_uri: &str) -> AppState {
        let config = create_test_config(mock_server_uri);
        let registry = ProviderRegistry::from_config(&config);
        AppState {
            registry: Arc::new(registry),
        }
    }

    #[tokio::test]
    async fn test_messages_non_stream_request() {
        let mock_server = MockServer::start().await;
        let expected_response = json!({
            "id": "msg_123",
            "type": "message",
            "role": "assistant",
            "content": [{
                "type": "text",
                "text": "Hello from mock server"
            }]
        });

        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .and(header("x-api-key", "test-key"))
            .and(header("anthropic-version", "2023-06-01"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&expected_response))
            .mount(&mock_server)
            .await;

        let state = create_test_state(&mock_server.uri());
        let app = axum::Router::new()
            .route("/v1/messages", axum::routing::post(messages))
            .with_state(state);

        let request = Request::builder()
            .method("POST")
            .uri("/v1/messages")
            .header("Content-Type", "application/json")
            .body(Body::from(
                serde_json::to_string(&json!({
                    "model": "test-provider@test-model",
                    "max_tokens": 1024,
                    "messages": [{"role": "user", "content": "Hello"}],
                    "stream": false
                }))
                .unwrap(),
            ))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_messages_stream_request() {
        let mock_server = MockServer::start().await;
        let sse_response =
            "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_123\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[],\"model\":\"test-model\"}}\n\n
event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello\"}}\n\n
event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n";

        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .and(header("x-api-key", "test-key"))
            .and(header("anthropic-version", "2023-06-01"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(sse_response)
                    .insert_header("content-type", "text/event-stream"),
            )
            .mount(&mock_server)
            .await;

        let state = create_test_state(&mock_server.uri());
        let app = axum::Router::new()
            .route("/v1/messages", axum::routing::post(messages))
            .with_state(state);

        let request = Request::builder()
            .method("POST")
            .uri("/v1/messages")
            .header("Content-Type", "application/json")
            .body(Body::from(
                serde_json::to_string(&json!({
                    "model": "test-provider@test-model",
                    "max_tokens": 1024,
                    "messages": [{"role": "user", "content": "Hello"}],
                    "stream": true
                }))
                .unwrap(),
            ))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_messages_missing_model() {
        let state = create_test_state("http://dummy-server");
        let app = axum::Router::new()
            .route("/v1/messages", axum::routing::post(messages))
            .with_state(state);

        let request = Request::builder()
            .method("POST")
            .uri("/v1/messages")
            .header("Content-Type", "application/json")
            .body(Body::from(
                serde_json::to_string(&json!({
                    "max_tokens": 1024,
                    "messages": [{"role": "user", "content": "Hello"}]
                }))
                .unwrap(),
            ))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_messages_invalid_model_format() {
        let state = create_test_state("http://dummy-server");
        let app = axum::Router::new()
            .route("/v1/messages", axum::routing::post(messages))
            .with_state(state);

        let request = Request::builder()
            .method("POST")
            .uri("/v1/messages")
            .header("Content-Type", "application/json")
            .body(Body::from(
                serde_json::to_string(&json!({
                    "model": "invalid-format",
                    "max_tokens": 1024,
                    "messages": [{"role": "user", "content": "Hello"}]
                }))
                .unwrap(),
            ))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_messages_provider_not_found() {
        let state = create_test_state("http://dummy-server");
        let app = axum::Router::new()
            .route("/v1/messages", axum::routing::post(messages))
            .with_state(state);

        let request = Request::builder()
            .method("POST")
            .uri("/v1/messages")
            .header("Content-Type", "application/json")
            .body(Body::from(
                serde_json::to_string(&json!({
                    "model": "non-existent@test-model",
                    "max_tokens": 1024,
                    "messages": [{"role": "user", "content": "Hello"}]
                }))
                .unwrap(),
            ))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    }
}
