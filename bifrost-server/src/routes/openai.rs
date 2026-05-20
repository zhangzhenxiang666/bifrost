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
    use crate::types::{
        AliasEntry, Config, Endpoint, MappingConfig, ProviderConfig, ProviderDeploymentConfig,
        ServerConfig,
    };
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
                endpoint: Endpoint::OpenAI,
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

    fn create_deployment_pool_config(base_url_a: &str, base_url_b: &str) -> Config {
        let mut provider = HashMap::new();
        provider.insert(
            "test-provider".to_string(),
            ProviderConfig {
                base_url: String::new(),
                api_key: String::new(),
                endpoint: Endpoint::OpenAI,
                deployments: vec![
                    ProviderDeploymentConfig {
                        id: "a".to_string(),
                        base_url: base_url_a.to_string(),
                        api_key: "key-a".to_string(),
                        enabled: true,
                        weight: 1,
                    },
                    ProviderDeploymentConfig {
                        id: "b".to_string(),
                        base_url: base_url_b.to_string(),
                        api_key: "key-b".to_string(),
                        enabled: true,
                        weight: 1,
                    },
                ],
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
            server: ServerConfig {
                max_retries: Some(0),
                ..ServerConfig::default()
            },
            alias: HashMap::new(),
        }
    }

    fn create_test_state(mock_server_uri: &str) -> AppState {
        let config = create_test_config(mock_server_uri);
        let registry = ProviderRegistry::from_config(&config);
        AppState::from(registry)
    }

    fn create_deployment_pool_state(base_url_a: &str, base_url_b: &str) -> AppState {
        let config = create_deployment_pool_config(base_url_a, base_url_b);
        let registry = ProviderRegistry::from_config(&config);
        AppState::from(registry)
    }

    fn create_deployment_pool_state_with_alias(base_url_a: &str, base_url_b: &str) -> AppState {
        let mut config = create_deployment_pool_config(base_url_a, base_url_b);
        config.alias.insert(
            "gpt-4o".to_string(),
            AliasEntry::Simple("test-provider@test-model".to_string()),
        );
        let registry = ProviderRegistry::from_config(&config);
        AppState::from(registry)
    }

    fn create_deployment_pool_state_with_alias_deployment(
        base_url_a: &str,
        base_url_b: &str,
    ) -> AppState {
        let mut config = create_deployment_pool_config(base_url_a, base_url_b);
        config.alias.insert(
            "gpt-4o".to_string(),
            AliasEntry::Complex(MappingConfig {
                target: "test-provider@test-model".to_string(),
                deployment: Some("b".to_string()),
                headers: None,
                body: None,
            }),
        );
        let registry = ProviderRegistry::from_config(&config);
        AppState::from(registry)
    }

    fn create_openai_request(model: &str) -> Request<Body> {
        Request::builder()
            .method("POST")
            .uri("/openai/chat/completions")
            .header("Content-Type", "application/json")
            .body(Body::from(
                serde_json::to_string(&json!({
                    "model": model,
                    "messages": [{"role": "user", "content": "Hello"}],
                    "stream": false
                }))
                .unwrap(),
            ))
            .unwrap()
    }

    fn create_openai_request_with_deployment_header(
        model: &str,
        deployment: &str,
    ) -> Request<Body> {
        Request::builder()
            .method("POST")
            .uri("/openai/chat/completions")
            .header("Content-Type", "application/json")
            .header("x-bifrost-deployment", deployment)
            .body(Body::from(
                serde_json::to_string(&json!({
                    "model": model,
                    "messages": [{"role": "user", "content": "Hello"}],
                    "stream": false
                }))
                .unwrap(),
            ))
            .unwrap()
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
    async fn test_deployment_pool_round_robin() {
        let mock_server = MockServer::start().await;
        let expected_response = json!({
            "id": "chatcmpl-123",
            "object": "chat.completion",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "Hello"}
            }]
        });

        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(header("Authorization", "Bearer key-a"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&expected_response))
            .expect(1)
            .mount(&mock_server)
            .await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(header("Authorization", "Bearer key-b"))
            .and(body_json(json!({
                "model": "test-model",
                "messages": [{"role": "user", "content": "Hello"}],
                "stream": false
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(&expected_response))
            .expect(1)
            .mount(&mock_server)
            .await;

        let state = create_deployment_pool_state(&mock_server.uri(), &mock_server.uri());
        let app = axum::Router::new()
            .route(
                "/openai/chat/completions",
                axum::routing::post(chat_completions),
            )
            .with_state(state);

        let response = app
            .clone()
            .oneshot(create_openai_request("test-provider@test-model"))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get("x-bifrost-deployment").unwrap(), "a");

        let response = app
            .oneshot(create_openai_request("test-provider@test-model"))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get("x-bifrost-deployment").unwrap(), "b");
    }

    #[tokio::test]
    async fn test_model_suffix_pins_deployment() {
        let mock_server = MockServer::start().await;
        let expected_response = json!({
            "id": "chatcmpl-123",
            "object": "chat.completion",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "Hello"}
            }]
        });

        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(header("Authorization", "Bearer key-b"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&expected_response))
            .expect(1)
            .mount(&mock_server)
            .await;

        let state = create_deployment_pool_state(&mock_server.uri(), &mock_server.uri());
        let app = axum::Router::new()
            .route(
                "/openai/chat/completions",
                axum::routing::post(chat_completions),
            )
            .with_state(state);

        let response = app
            .oneshot(create_openai_request("test-provider@test-model#b"))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get("x-bifrost-deployment").unwrap(), "b");
        assert_eq!(
            response.headers().get("x-bifrost-fallback-count").unwrap(),
            "0"
        );
    }

    #[tokio::test]
    async fn test_model_suffix_can_select_deployment_base_url() {
        let subscription_server = MockServer::start().await;
        let payg_server = MockServer::start().await;
        let expected_response = json!({
            "id": "chatcmpl-payg",
            "object": "chat.completion",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "Hello from payg"}
            }]
        });

        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(header("Authorization", "Bearer key-b"))
            .and(body_json(json!({
                "model": "test-model",
                "messages": [{"role": "user", "content": "Hello"}],
                "stream": false
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(&expected_response))
            .expect(1)
            .mount(&payg_server)
            .await;

        let state = create_deployment_pool_state(&subscription_server.uri(), &payg_server.uri());
        let app = axum::Router::new()
            .route(
                "/openai/chat/completions",
                axum::routing::post(chat_completions),
            )
            .with_state(state);

        let response = app
            .oneshot(create_openai_request("test-provider@test-model#b"))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get("x-bifrost-deployment").unwrap(), "b");
    }

    #[tokio::test]
    async fn test_alias_suffix_pins_deployment() {
        let mock_server = MockServer::start().await;
        let expected_response = json!({
            "id": "chatcmpl-123",
            "object": "chat.completion",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "Hello"}
            }]
        });

        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(header("Authorization", "Bearer key-b"))
            .and(body_json(json!({
                "model": "test-model",
                "messages": [{"role": "user", "content": "Hello"}],
                "stream": false
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(&expected_response))
            .expect(1)
            .mount(&mock_server)
            .await;

        let state = create_deployment_pool_state_with_alias(&mock_server.uri(), &mock_server.uri());
        let app = axum::Router::new()
            .route(
                "/openai/chat/completions",
                axum::routing::post(chat_completions),
            )
            .with_state(state);

        let response = app
            .oneshot(create_openai_request("gpt-4o#b"))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get("x-bifrost-deployment").unwrap(), "b");
    }

    #[tokio::test]
    async fn test_complex_alias_deployment_pins_upstream() {
        let mock_server = MockServer::start().await;
        let expected_response = json!({
            "id": "chatcmpl-123",
            "object": "chat.completion",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "Hello"}
            }]
        });

        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(header("Authorization", "Bearer key-b"))
            .and(body_json(json!({
                "model": "test-model",
                "messages": [{"role": "user", "content": "Hello"}],
                "stream": false
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(&expected_response))
            .expect(1)
            .mount(&mock_server)
            .await;

        let state = create_deployment_pool_state_with_alias_deployment(
            &mock_server.uri(),
            &mock_server.uri(),
        );
        let app = axum::Router::new()
            .route(
                "/openai/chat/completions",
                axum::routing::post(chat_completions),
            )
            .with_state(state);

        let response = app.oneshot(create_openai_request("gpt-4o")).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get("x-bifrost-deployment").unwrap(), "b");
    }

    #[tokio::test]
    async fn test_header_deployment_overrides_model_suffix() {
        let mock_server = MockServer::start().await;
        let expected_response = json!({
            "id": "chatcmpl-123",
            "object": "chat.completion",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "Hello"}
            }]
        });

        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(header("Authorization", "Bearer key-b"))
            .and(body_json(json!({
                "model": "test-model",
                "messages": [{"role": "user", "content": "Hello"}],
                "stream": false
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(&expected_response))
            .expect(1)
            .mount(&mock_server)
            .await;

        let state = create_deployment_pool_state(&mock_server.uri(), &mock_server.uri());
        let app = axum::Router::new()
            .route(
                "/openai/chat/completions",
                axum::routing::post(chat_completions),
            )
            .with_state(state);

        let response = app
            .oneshot(create_openai_request_with_deployment_header(
                "test-provider@test-model#a",
                "b",
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get("x-bifrost-deployment").unwrap(), "b");
    }

    #[tokio::test]
    async fn test_unpinned_request_fails_over_to_next_deployment() {
        let mock_server = MockServer::start().await;
        let expected_response = json!({
            "id": "chatcmpl-123",
            "object": "chat.completion",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "Hello"}
            }]
        });

        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(header("Authorization", "Bearer key-a"))
            .respond_with(ResponseTemplate::new(429).set_body_string("rate limited"))
            .expect(1)
            .mount(&mock_server)
            .await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(header("Authorization", "Bearer key-b"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&expected_response))
            .expect(1)
            .mount(&mock_server)
            .await;

        let state = create_deployment_pool_state(&mock_server.uri(), &mock_server.uri());
        let app = axum::Router::new()
            .route(
                "/openai/chat/completions",
                axum::routing::post(chat_completions),
            )
            .with_state(state);

        let response = app
            .oneshot(create_openai_request("test-provider@test-model"))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get("x-bifrost-deployment").unwrap(), "b");
        assert_eq!(
            response.headers().get("x-bifrost-fallback-count").unwrap(),
            "1"
        );
    }

    #[tokio::test]
    async fn test_unpinned_request_skips_cooling_deployment_after_failure() {
        let mock_server = MockServer::start().await;
        let expected_response = json!({
            "id": "chatcmpl-123",
            "object": "chat.completion",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "Hello"}
            }]
        });

        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(header("Authorization", "Bearer key-a"))
            .respond_with(ResponseTemplate::new(429).set_body_string("rate limited"))
            .expect(1)
            .mount(&mock_server)
            .await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(header("Authorization", "Bearer key-b"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&expected_response))
            .expect(1)
            .mount(&mock_server)
            .await;

        let state = create_deployment_pool_state(&mock_server.uri(), &mock_server.uri());
        let app = axum::Router::new()
            .route(
                "/openai/chat/completions",
                axum::routing::post(chat_completions),
            )
            .with_state(state);

        let response = app
            .clone()
            .oneshot(create_openai_request("test-provider@test-model#a"))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(response.headers().get("x-bifrost-deployment").unwrap(), "a");

        let response = app
            .oneshot(create_openai_request("test-provider@test-model"))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get("x-bifrost-deployment").unwrap(), "b");
        assert_eq!(
            response.headers().get("x-bifrost-fallback-count").unwrap(),
            "0"
        );
    }

    #[tokio::test]
    async fn test_pinned_deployment_does_not_fail_over() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(header("Authorization", "Bearer key-a"))
            .respond_with(ResponseTemplate::new(429).set_body_string("rate limited"))
            .expect(1)
            .mount(&mock_server)
            .await;

        let state = create_deployment_pool_state(&mock_server.uri(), &mock_server.uri());
        let app = axum::Router::new()
            .route(
                "/openai/chat/completions",
                axum::routing::post(chat_completions),
            )
            .with_state(state);

        let response = app
            .oneshot(create_openai_request("test-provider@test-model#a"))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(response.headers().get("x-bifrost-deployment").unwrap(), "a");
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
