//! Anthropic-compatible route for messages endpoint

use crate::adapter::converter::anthropic_openai::message::remove_claude_code_billing_header_from_system;
use crate::routes::handler;
use crate::state::AppState;
use crate::{error::Result, routes::RouteEndpoint};
use axum::{Json, extract::State};
use serde_json::Value;

/// Anthropic-compatible messages endpoint.
#[axum::debug_handler]
pub async fn messages_v1(
    State(state): State<AppState>,
    headers: http::header::HeaderMap,
    Json(mut body): Json<Value>,
) -> Result<axum::response::Response> {
    remove_claude_code_billing_header_from_system(&mut body);

    let is_stream = body
        .get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    handler::handle_llm_request(
        &state,
        RouteEndpoint::AnthropicMessages,
        headers,
        body,
        is_stream,
    )
    .await
}

#[axum::debug_handler]
pub async fn messages(
    state: State<AppState>,
    headers: http::header::HeaderMap,
    body: Json<Value>,
) -> Result<axum::response::Response> {
    messages_v1(state, headers, body).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::registry::ProviderRegistry;
    use crate::types::{Config, Endpoint, ProviderConfig};
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use serde_json::json;
    use std::collections::HashMap;
    use tower::util::ServiceExt;
    use wiremock::matchers::{body_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn create_test_config(mock_server_uri: &str) -> Config {
        let mut provider = HashMap::new();
        provider.insert(
            "test-provider".to_string(),
            ProviderConfig {
                base_url: mock_server_uri.to_string(),
                api_key: "test-key".to_string(),
                endpoint: Endpoint::Anthropic,
                deployments: Vec::new(),
                headers: None,
                body: None,
                models: None,
                exclude_headers: None,
                extend: false,
                body_policy: None,
            },
        );
        Config {
            provider,
            server: crate::types::ServerConfig::default(),
            alias: HashMap::new(),
        }
    }

    fn create_test_state(mock_server_uri: &str) -> AppState {
        let config = create_test_config(mock_server_uri);
        let registry = ProviderRegistry::from_config(&config);
        AppState::from(registry)
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
            .route("/anthropic/v1/messages", axum::routing::post(messages_v1))
            .with_state(state);

        let request = Request::builder()
            .method("POST")
            .uri("/anthropic/v1/messages")
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
    async fn test_messages_removes_claude_code_billing_header_before_passthrough() {
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
            .and(body_json(json!({
                "model": "test-model",
                "max_tokens": 1024,
                "system": [
                    {
                        "type": "text",
                        "text": "You are Claude Code.",
                        "cache_control": {"type": "ephemeral"}
                    }
                ],
                "messages": [{"role": "user", "content": "Hello"}],
                "stream": false
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(&expected_response))
            .mount(&mock_server)
            .await;

        let state = create_test_state(&mock_server.uri());
        let app = axum::Router::new()
            .route("/anthropic/v1/messages", axum::routing::post(messages_v1))
            .with_state(state);

        let request = Request::builder()
            .method("POST")
            .uri("/anthropic/v1/messages")
            .header("Content-Type", "application/json")
            .body(Body::from(
                serde_json::to_string(&json!({
                    "model": "test-provider@test-model",
                    "max_tokens": 1024,
                    "system": [
                        {
                            "type": "text",
                            "text": "x-anthropic-billing-header: cc_version=2.1.123.5d3; cc_entrypoint=cli; cch=b5d3a;"
                        },
                        {
                            "type": "text",
                            "text": "You are Claude Code.",
                            "cache_control": {"type": "ephemeral"}
                        }
                    ],
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
            .route("/anthropic/v1/messages", axum::routing::post(messages_v1))
            .with_state(state);

        let request = Request::builder()
            .method("POST")
            .uri("/anthropic/v1/messages")
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
            .route("/v1/messages", axum::routing::post(messages_v1))
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
            .route("/v1/messages", axum::routing::post(messages_v1))
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
            .route("/v1/messages", axum::routing::post(messages_v1))
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
