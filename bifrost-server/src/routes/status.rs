//! Status route for viewing application state

use crate::state::AppState;
use axum::{Json, extract::State};
use serde::{Deserialize, Serialize};

/// Response type for status endpoint
#[derive(Debug, Serialize, Deserialize)]
pub struct StatusResponse {
    /// Current proxy configuration, if any
    pub proxy: Option<String>,
    /// List of registered providers
    pub providers: Vec<ProviderInfo>,
}

/// Provider information for status response
#[derive(Debug, Serialize, Deserialize)]
pub struct ProviderInfo {
    /// Provider name/ID
    pub name: String,
    /// Provider endpoint type
    pub endpoint: String,
}

/// GET /status - Returns application status
#[axum::debug_handler]
pub async fn status(State(state): State<AppState>) -> Json<StatusResponse> {
    let providers = state
        .registry
        .providers()
        .iter()
        .map(|(name, config)| ProviderInfo {
            name: name.clone(),
            endpoint: config.endpoint.to_string(),
        })
        .collect();

    Json(StatusResponse {
        proxy: state.proxy.clone(),
        providers,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::registry::ProviderRegistry;
    use crate::types::{Config, Endpoint, ProviderConfig};
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use std::collections::HashMap;
    use tower::util::ServiceExt;

    fn create_test_config() -> Config {
        let mut provider = HashMap::new();
        provider.insert(
            "test-provider".to_string(),
            ProviderConfig {
                base_url: "https://api.test.com".to_string(),
                api_key: "test-key".to_string(),
                endpoint: Endpoint::OpenAI,
                headers: None,
                body: None,
                models: None,
                exclude_headers: None,
                extend: false,
            },
        );
        provider.insert(
            "anthropic-provider".to_string(),
            ProviderConfig {
                base_url: "https://api.anthropic.com".to_string(),
                api_key: "anthropic-key".to_string(),
                endpoint: Endpoint::Anthropic,
                headers: None,
                body: None,
                models: None,
                exclude_headers: None,
                extend: false,
            },
        );
        Config {
            provider,
            server: crate::types::ServerConfig::default(),
            alias: HashMap::new(),
        }
    }

    fn create_test_state(proxy: Option<String>) -> AppState {
        let config = create_test_config();
        let registry = ProviderRegistry::from_config(&config);
        AppState::new(registry, proxy)
    }

    #[tokio::test]
    async fn test_status_without_proxy() {
        let state = create_test_state(None);
        let app = axum::Router::new()
            .route("/status", axum::routing::get(status))
            .with_state(state);

        let request = Request::builder()
            .method("GET")
            .uri("/status")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), 256)
            .await
            .unwrap();
        let json: StatusResponse = serde_json::from_slice(&body).unwrap();

        assert!(json.proxy.is_none());
        assert_eq!(json.providers.len(), 2);

        let provider_names: Vec<_> = json.providers.iter().map(|p| p.name.as_str()).collect();
        assert!(provider_names.contains(&"test-provider"));
        assert!(provider_names.contains(&"anthropic-provider"));
    }

    #[tokio::test]
    async fn test_status_with_proxy() {
        let state = create_test_state(Some("http://proxy.example.com:8080".to_string()));
        let app = axum::Router::new()
            .route("/status", axum::routing::get(status))
            .with_state(state);

        let request = Request::builder()
            .method("GET")
            .uri("/status")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), 256)
            .await
            .unwrap();
        let json: StatusResponse = serde_json::from_slice(&body).unwrap();

        assert_eq!(json.proxy.as_deref(), Some("http://proxy.example.com:8080"));
        assert_eq!(json.providers.len(), 2);
    }
}
