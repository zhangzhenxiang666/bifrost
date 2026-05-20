//! Status route for viewing application state

use crate::state::AppState;
use axum::{Json, extract::State};
use serde::{Deserialize, Serialize};

/// Response type for status endpoint
#[derive(Debug, Serialize, Deserialize)]
pub struct StatusResponse {
    /// Server package version
    pub version: String,
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
    /// Provider deployment routing and health status
    pub deployments: Vec<DeploymentInfo>,
}

/// Deployment status exposed by the status endpoint.
#[derive(Debug, Serialize, Deserialize)]
pub struct DeploymentInfo {
    /// Deployment ID used by model suffixes, headers, and aliases
    pub id: String,
    /// Whether the deployment is enabled in config
    pub enabled: bool,
    /// Whether this deployment comes from provider-level base_url/api_key
    pub implicit: bool,
    /// Automatic routing weight
    pub weight: u32,
    /// Whether unpinned requests can select this deployment automatically
    pub automatic: bool,
    /// Runtime health state
    pub state: String,
    /// Consecutive retryable failures observed by the runtime
    pub consecutive_failures: u32,
    /// Remaining cooldown duration, when state is cooling
    pub cooldown_remaining_ms: Option<u64>,
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
            deployments: state
                .registry
                .deployment_snapshots(name)
                .into_iter()
                .map(|deployment| DeploymentInfo {
                    id: deployment.id,
                    enabled: deployment.enabled,
                    implicit: deployment.implicit,
                    weight: deployment.weight,
                    automatic: deployment.automatic,
                    state: deployment.state.as_str().to_string(),
                    consecutive_failures: deployment.consecutive_failures,
                    cooldown_remaining_ms: deployment.cooldown_remaining_ms,
                })
                .collect(),
        })
        .collect();

    Json(StatusResponse {
        version: env!("CARGO_PKG_VERSION").to_string(),
        proxy: state.proxy.clone(),
        providers,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::registry::ProviderRegistry;
    use crate::types::{Config, Endpoint, ProviderConfig, ProviderDeploymentConfig};
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
                deployments: Vec::new(),
                headers: None,
                body: None,
                models: None,
                exclude_headers: None,
                extend: false,
                body_policy: None,
            },
        );
        provider.insert(
            "anthropic-provider".to_string(),
            ProviderConfig {
                base_url: "https://api.anthropic.com".to_string(),
                api_key: "anthropic-key".to_string(),
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

    fn create_test_state(proxy: Option<String>) -> AppState {
        let config = create_test_config();
        let registry = ProviderRegistry::from_config(&config);
        AppState::new(registry, proxy)
    }

    fn create_deployment_status_state() -> AppState {
        let mut provider = HashMap::new();
        provider.insert(
            "test-provider".to_string(),
            ProviderConfig {
                base_url: String::new(),
                api_key: String::new(),
                endpoint: Endpoint::OpenAI,
                deployments: vec![
                    ProviderDeploymentConfig {
                        id: "auto".to_string(),
                        base_url: "https://auto.example.com".to_string(),
                        api_key: "auto-key".to_string(),
                        enabled: true,
                        weight: 2,
                    },
                    ProviderDeploymentConfig {
                        id: "manual".to_string(),
                        base_url: "https://manual.example.com".to_string(),
                        api_key: "manual-key".to_string(),
                        enabled: true,
                        weight: 0,
                    },
                    ProviderDeploymentConfig {
                        id: "disabled".to_string(),
                        base_url: "https://disabled.example.com".to_string(),
                        api_key: "disabled-key".to_string(),
                        enabled: false,
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
        let registry = ProviderRegistry::from_config(&Config {
            provider,
            server: crate::types::ServerConfig::default(),
            alias: HashMap::new(),
        });
        AppState::from(registry)
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

        let body = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        let json: StatusResponse = serde_json::from_slice(&body).unwrap();

        assert_eq!(json.version, env!("CARGO_PKG_VERSION"));
        assert!(json.proxy.is_none());
        assert_eq!(json.providers.len(), 2);

        let provider_names: Vec<_> = json.providers.iter().map(|p| p.name.as_str()).collect();
        assert!(provider_names.contains(&"test-provider"));
        assert!(provider_names.contains(&"anthropic-provider"));

        let test_provider = json
            .providers
            .iter()
            .find(|provider| provider.name == "test-provider")
            .unwrap();
        assert_eq!(test_provider.deployments.len(), 1);
        assert_eq!(test_provider.deployments[0].id, "main");
        assert!(test_provider.deployments[0].enabled);
        assert!(test_provider.deployments[0].implicit);
        assert_eq!(test_provider.deployments[0].weight, 1);
        assert!(test_provider.deployments[0].automatic);
        assert_eq!(test_provider.deployments[0].state, "available");
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

        let body = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        let json: StatusResponse = serde_json::from_slice(&body).unwrap();

        assert_eq!(json.proxy.as_deref(), Some("http://proxy.example.com:8080"));
        assert_eq!(json.providers.len(), 2);
    }

    #[tokio::test]
    async fn test_status_reports_deployment_routing_modes() {
        let state = create_deployment_status_state();
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

        let body = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        let json: StatusResponse = serde_json::from_slice(&body).unwrap();
        let deployments = &json.providers[0].deployments;

        let auto = deployments
            .iter()
            .find(|deployment| deployment.id == "auto")
            .unwrap();
        assert!(auto.enabled);
        assert!(auto.automatic);
        assert_eq!(auto.weight, 2);
        assert_eq!(auto.state, "available");

        let manual = deployments
            .iter()
            .find(|deployment| deployment.id == "manual")
            .unwrap();
        assert!(manual.enabled);
        assert!(!manual.automatic);
        assert_eq!(manual.weight, 0);
        assert_eq!(manual.state, "available");

        let disabled = deployments
            .iter()
            .find(|deployment| deployment.id == "disabled")
            .unwrap();
        assert!(!disabled.enabled);
        assert!(!disabled.automatic);
        assert_eq!(disabled.state, "disabled");
    }
}
