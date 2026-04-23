//! OpenAI-compatible route for chat completions endpoint

use crate::routes::handler;
use crate::state::AppState;
use crate::{error::Result, routes::RouteEndpoint};
use axum::{Json, extract::State};
use serde_json::Value;

/// OpenAI-compatible chat completions endpoint.
#[axum::debug_handler]
pub async fn chat_completions(
    State(state): State<AppState>,
    headers: http::header::HeaderMap,
    Json(body): Json<Value>,
) -> Result<axum::response::Response> {
    let is_stream = body
        .get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    handler::handle_llm_request(&state, RouteEndpoint::OpenAIChat, headers, body, is_stream).await
}

#[axum::debug_handler]
pub async fn chat_completions_v1(
    state: State<AppState>,
    headers: http::header::HeaderMap,
    body: Json<Value>,
) -> Result<axum::response::Response> {
    chat_completions(state, headers, body).await
}

#[axum::debug_handler]
pub async fn responses(
    state: State<AppState>,
    headers: http::header::HeaderMap,
    Json(body): Json<Value>,
) -> Result<axum::response::Response> {
    let is_stream = body
        .get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    handler::handle_llm_request(
        &state,
        RouteEndpoint::OpenAIResponses,
        headers,
        body,
        is_stream,
    )
    .await
}

#[axum::debug_handler]
pub async fn responses_v1(
    state: State<AppState>,
    headers: http::header::HeaderMap,
    body: Json<Value>,
) -> Result<axum::response::Response> {
    responses(state, headers, body).await
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
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn create_test_config(mock_server_uri: &str) -> Config {
        let mut provider = HashMap::new();
        provider.insert(
            "test-provider".to_string(),
            ProviderConfig {
                base_url: mock_server_uri.to_string(),
                api_key: "test-key".to_string(),
                endpoint: Endpoint::OpenAI,
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
    async fn test_chat_completions_non_stream_request() {
        let mock_server = MockServer::start().await;
        let expected_response = json!({
            "id": "chatcmpl-123",
            "object": "chat.completion",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "Hello from mock server"
                }
            }]
        });

        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(header("Authorization", "Bearer test-key"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&expected_response))
            .mount(&mock_server)
            .await;

        let state = create_test_state(&mock_server.uri());
        let app = axum::Router::new()
            .route(
                "/openai/chat/completions",
                axum::routing::post(chat_completions),
            )
            .with_state(state);

        let request = Request::builder()
            .method("POST")
            .uri("/openai/chat/completions")
            .header("Content-Type", "application/json")
            .body(Body::from(
                serde_json::to_string(&json!({
                    "model": "test-provider@test-model",
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
    async fn test_chat_completions_stream_request() {
        let mock_server = MockServer::start().await;
        let sse_response =
            "data: {\"id\":\"chatcmpl-123\",\"choices\":[{\"delta\":{\"content\":\"Hello\"}}]}\n\n
data: {\"id\":\"chatcmpl-123\",\"choices\":[{\"delta\":{\"content\":\" World\"}}]}\n\n
data: [DONE]\n\n";

        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(header("Authorization", "Bearer test-key"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(sse_response)
                    .insert_header("content-type", "text/event-stream"),
            )
            .mount(&mock_server)
            .await;

        let state = create_test_state(&mock_server.uri());
        let app = axum::Router::new()
            .route(
                "/openai/chat/completions",
                axum::routing::post(chat_completions),
            )
            .with_state(state);

        let request = Request::builder()
            .method("POST")
            .uri("/openai/chat/completions")
            .header("Content-Type", "application/json")
            .body(Body::from(
                serde_json::to_string(&json!({
                    "model": "test-provider@test-model",
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
    async fn test_chat_completions_missing_model() {
        let state = create_test_state("http://dummy-server");
        let app = axum::Router::new()
            .route(
                "/v1/chat/completions",
                axum::routing::post(chat_completions),
            )
            .with_state(state);

        let request = Request::builder()
            .method("POST")
            .uri("/v1/chat/completions")
            .header("Content-Type", "application/json")
            .body(Body::from(
                serde_json::to_string(&json!({
                    "messages": [{"role": "user", "content": "Hello"}]
                }))
                .unwrap(),
            ))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_chat_completions_invalid_model_format() {
        let state = create_test_state("http://dummy-server");
        let app = axum::Router::new()
            .route(
                "/v1/chat/completions",
                axum::routing::post(chat_completions),
            )
            .with_state(state);

        let request = Request::builder()
            .method("POST")
            .uri("/v1/chat/completions")
            .header("Content-Type", "application/json")
            .body(Body::from(
                serde_json::to_string(&json!({
                    "model": "invalid-format",
                    "messages": [{"role": "user", "content": "Hello"}]
                }))
                .unwrap(),
            ))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_chat_completions_provider_not_found() {
        let state = create_test_state("http://dummy-server");
        let app = axum::Router::new()
            .route(
                "/v1/chat/completions",
                axum::routing::post(chat_completions),
            )
            .with_state(state);

        let request = Request::builder()
            .method("POST")
            .uri("/v1/chat/completions")
            .header("Content-Type", "application/json")
            .body(Body::from(
                serde_json::to_string(&json!({
                    "model": "non-existent@test-model",
                    "messages": [{"role": "user", "content": "Hello"}]
                }))
                .unwrap(),
            ))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    }
}
