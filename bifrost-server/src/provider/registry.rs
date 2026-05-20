//! Provider registry module for LLM service
//!
//! This module provides the [`ProviderRegistry`] which manages provider configurations
//! and builds adapter chains for request execution.

use crate::adapter::Adapter;
use crate::adapter::builtin::{
    AnthropicToOpenAIAdapter, OpenAIToAnthropicAdapter, PassthroughAdapter, ResponsesToChatAdapter,
};
use crate::adapter::chain::OnionExecutor;
use crate::error::{LlmMapError, Result};
use crate::provider::client::HttpClient;
use crate::routes::RouteEndpoint;
use crate::types::{AliasEntry, Config, Endpoint, IMPLICIT_PROVIDER_DEPLOYMENT_ID, ProviderConfig};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const DEFAULT_DEPLOYMENT_COOLDOWN_BASE_MS: u64 = 30_000;
const DEFAULT_DEPLOYMENT_COOLDOWN_MAX_MS: u64 = 300_000;

/// Resolved provider deployment that is safe to use for one upstream attempt.
#[derive(Debug, Clone)]
pub struct ProviderDeployment {
    pub id: String,
    pub base_url: String,
    pub api_key: String,
    pub weight: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeploymentRuntimeState {
    Available,
    Cooling,
    Disabled,
}

impl DeploymentRuntimeState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::Cooling => "cooling",
            Self::Disabled => "disabled",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProviderDeploymentSnapshot {
    pub id: String,
    pub enabled: bool,
    pub implicit: bool,
    pub weight: u32,
    pub automatic: bool,
    pub state: DeploymentRuntimeState,
    pub consecutive_failures: u32,
    pub cooldown_remaining_ms: Option<u64>,
}

#[derive(Debug)]
struct DeploymentPool {
    deployments: Vec<Arc<DeploymentRuntime>>,
    automatic_selector: SmoothWeightedSelector,
    base_cooldown: Duration,
    max_cooldown: Duration,
}

#[derive(Debug)]
struct SmoothWeightedSelector {
    entries: Vec<WeightedDeployment>,
    current_weights: Mutex<Vec<i128>>,
}

#[derive(Debug, Clone, Copy)]
struct WeightedDeployment {
    deployment_index: usize,
    weight: u32,
}

#[derive(Debug)]
struct DeploymentRuntime {
    deployment: ProviderDeployment,
    health: Mutex<DeploymentHealth>,
}

#[derive(Debug)]
struct DeploymentHealth {
    consecutive_failures: u32,
    cooldown_until: Option<Instant>,
    current_cooldown: Duration,
}

impl DeploymentHealth {
    fn new(base_cooldown: Duration) -> Self {
        Self {
            consecutive_failures: 0,
            cooldown_until: None,
            current_cooldown: base_cooldown,
        }
    }
}

impl DeploymentRuntime {
    fn new(deployment: ProviderDeployment, base_cooldown: Duration) -> Self {
        Self {
            deployment,
            health: Mutex::new(DeploymentHealth::new(base_cooldown)),
        }
    }

    fn is_available(&self, now: Instant) -> bool {
        let health = self.health.lock().unwrap();
        health.cooldown_until.is_none_or(|until| now >= until)
    }

    fn mark_success(&self, base_cooldown: Duration) {
        let mut health = self.health.lock().unwrap();
        health.consecutive_failures = 0;
        health.cooldown_until = None;
        health.current_cooldown = base_cooldown;
    }

    fn mark_failure(&self, now: Instant, max_cooldown: Duration) -> (u32, Duration) {
        let mut health = self.health.lock().unwrap();
        health.consecutive_failures = health.consecutive_failures.saturating_add(1);
        let cooldown = health.current_cooldown.min(max_cooldown);
        health.cooldown_until = Some(now + cooldown);
        health.current_cooldown = cooldown
            .checked_mul(2)
            .unwrap_or(max_cooldown)
            .min(max_cooldown);
        (health.consecutive_failures, cooldown)
    }

    fn snapshot(&self, now: Instant, implicit: bool) -> ProviderDeploymentSnapshot {
        let health = self.health.lock().unwrap();
        let cooldown_remaining_ms = health
            .cooldown_until
            .and_then(|until| until.checked_duration_since(now))
            .map(|duration| duration.as_millis() as u64);
        let state = if cooldown_remaining_ms.is_some() {
            DeploymentRuntimeState::Cooling
        } else {
            DeploymentRuntimeState::Available
        };

        ProviderDeploymentSnapshot {
            id: self.deployment.id.clone(),
            enabled: true,
            implicit,
            weight: self.deployment.weight,
            automatic: self.deployment.weight > 0,
            state,
            consecutive_failures: health.consecutive_failures,
            cooldown_remaining_ms,
        }
    }
}

impl DeploymentPool {
    fn new(
        deployments: Vec<ProviderDeployment>,
        base_cooldown: Duration,
        max_cooldown: Duration,
    ) -> Self {
        let automatic_selector = SmoothWeightedSelector::new(&deployments);
        let deployments = deployments
            .into_iter()
            .map(|deployment| Arc::new(DeploymentRuntime::new(deployment, base_cooldown)))
            .collect();

        Self {
            deployments,
            automatic_selector,
            base_cooldown,
            max_cooldown,
        }
    }

    fn find_runtime(&self, deployment_id: &str) -> Option<Arc<DeploymentRuntime>> {
        self.deployments
            .iter()
            .find(|runtime| runtime.deployment.id == deployment_id)
            .cloned()
    }
}

impl SmoothWeightedSelector {
    fn new(deployments: &[ProviderDeployment]) -> Self {
        let entries: Vec<_> = deployments
            .iter()
            .enumerate()
            .filter(|(_, deployment)| deployment.weight > 0)
            .map(|(deployment_index, deployment)| WeightedDeployment {
                deployment_index,
                weight: deployment.weight,
            })
            .collect();

        Self {
            current_weights: Mutex::new(vec![0; entries.len()]),
            entries,
        }
    }

    fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn plan(&self) -> Vec<usize> {
        if self.entries.is_empty() {
            return Vec::new();
        }

        let candidate_positions: Vec<_> = (0..self.entries.len()).collect();
        self.plan_candidates(&candidate_positions)
    }

    fn plan_candidates(&self, candidate_positions: &[usize]) -> Vec<usize> {
        debug_assert!(!candidate_positions.is_empty());

        let mut current_weights = self.current_weights.lock().unwrap();
        let first_position = self.next_position(candidate_positions, &mut current_weights);
        let mut plan_positions = Vec::with_capacity(candidate_positions.len());
        plan_positions.push(first_position);

        if candidate_positions.len() > 1 {
            let mut simulated_weights = current_weights.clone();
            let mut remaining_positions: Vec<_> = candidate_positions
                .iter()
                .copied()
                .filter(|position| *position != first_position)
                .collect();

            while !remaining_positions.is_empty() {
                let next_position =
                    self.next_position(&remaining_positions, &mut simulated_weights);
                plan_positions.push(next_position);
                remaining_positions.retain(|position| *position != next_position);
            }
        }

        plan_positions
            .into_iter()
            .map(|position| self.entries[position].deployment_index)
            .collect()
    }

    fn next_position(&self, candidate_positions: &[usize], current_weights: &mut [i128]) -> usize {
        let total_weight: i128 = candidate_positions
            .iter()
            .map(|position| self.entries[*position].weight as i128)
            .sum();
        debug_assert!(total_weight > 0);

        let mut selected_position = candidate_positions[0];
        for position in candidate_positions.iter().copied() {
            current_weights[position] += self.entries[position].weight as i128;
            if current_weights[position] > current_weights[selected_position] {
                selected_position = position;
            }
        }

        current_weights[selected_position] -= total_weight;
        selected_position
    }
}

/// Registry that manages provider configurations and builds adapter chains.
///
/// The registry is responsible for:
/// - Loading provider configurations from the root config
/// - Providing access to provider information
/// - Building adapter chains (OnionExecutor) for specific providers
#[derive(Debug, Clone)]
pub struct ProviderRegistry {
    providers: HashMap<String, ProviderConfig>,
    deployment_pools: HashMap<String, Arc<DeploymentPool>>,
    http_client: HttpClient,
    alias: HashMap<String, AliasEntry>,
}

impl ProviderRegistry {
    /// Create a new provider registry from configuration.
    ///
    /// This will load all provider configurations from the config file
    /// and prepare them for use.
    ///
    /// # Arguments
    ///
    /// * `config` - The root configuration containing provider settings
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use bifrost_server::Config;
    /// use bifrost_server::provider::ProviderRegistry;
    ///
    /// let config = Config::from_file("config.toml").unwrap();
    /// let registry = ProviderRegistry::from_config(&config);
    /// ```
    /// Create a new provider registry from configuration.
    ///
    /// # Panics
    /// Panics if the HTTP client fails to build (e.g., invalid proxy URL).
    /// This is acceptable for startup initialization.
    pub fn from_config(config: &Config) -> Self {
        let mut providers = HashMap::new();
        let mut deployment_pools = HashMap::new();
        let base_cooldown = Duration::from_millis(
            config
                .server
                .deployment_cooldown_base_ms
                .unwrap_or(DEFAULT_DEPLOYMENT_COOLDOWN_BASE_MS),
        );
        let max_cooldown = Duration::from_millis(
            config
                .server
                .deployment_cooldown_max_ms
                .unwrap_or(DEFAULT_DEPLOYMENT_COOLDOWN_MAX_MS),
        );

        // Build provider info from config
        for (id, provider_config) in &config.provider {
            deployment_pools.insert(
                id.clone(),
                Arc::new(DeploymentPool::new(
                    Self::build_deployments(provider_config),
                    base_cooldown,
                    max_cooldown,
                )),
            );
            providers.insert(id.clone(), provider_config.clone());
        }

        // Create HTTP client with configurable timeout and retry settings
        let timeout_secs = config.server.timeout_secs.unwrap_or(600);
        let retry_config = crate::provider::client::RetryConfig {
            max_retries: config.server.max_retries.unwrap_or(5),
            backoff_base_ms: config.server.retry_backoff_base_ms.unwrap_or(700),
            retry_status_codes: config.server.retry_status_codes.clone(),
        };
        let http_client =
            HttpClient::with_retry(timeout_secs, config.server.proxy.as_deref(), retry_config);

        Self {
            providers,
            deployment_pools,
            http_client,
            alias: config.alias.clone(),
        }
    }

    fn build_deployments(provider: &ProviderConfig) -> Vec<ProviderDeployment> {
        let mut deployments = Vec::new();
        if !provider.api_key.trim().is_empty() {
            deployments.push(ProviderDeployment {
                id: IMPLICIT_PROVIDER_DEPLOYMENT_ID.to_string(),
                base_url: provider.base_url.clone(),
                api_key: provider.api_key.clone(),
                weight: 1,
            });
        }

        deployments.extend(
            provider
                .deployments
                .iter()
                .filter(|deployment| deployment.enabled)
                .map(|deployment| ProviderDeployment {
                    id: deployment.id.clone(),
                    base_url: deployment.base_url.clone(),
                    api_key: deployment.api_key.clone(),
                    weight: deployment.weight,
                }),
        );

        deployments
    }

    /// Return the deployment attempt order for a single provider request.
    ///
    /// A pinned deployment returns exactly one entry. An unpinned request starts
    /// with the provider's next smooth weighted choice and then lists the
    /// remaining automatic deployments for failover.
    pub fn deployment_plan(
        &self,
        provider_id: &str,
        deployment_id: Option<&str>,
    ) -> Result<Vec<ProviderDeployment>> {
        let pool = self.deployment_pools.get(provider_id).ok_or_else(|| {
            LlmMapError::Provider(format!("Provider '{}' not found", provider_id))
        })?;

        if let Some(deployment_id) = deployment_id {
            return pool
                .deployments
                .iter()
                .find(|runtime| runtime.deployment.id == deployment_id)
                .map(|runtime| vec![runtime.deployment.clone()])
                .ok_or_else(|| {
                    LlmMapError::Provider(format!(
                        "Deployment '{}' not found for provider '{}'",
                        deployment_id, provider_id
                    ))
                });
        }

        if pool.deployments.is_empty() {
            return Err(LlmMapError::Provider(format!(
                "Provider '{}' has no enabled deployments",
                provider_id
            )));
        }
        if pool.automatic_selector.is_empty() {
            return Err(LlmMapError::Provider(format!(
                "Provider '{}' has no automatic deployments; specify a deployment or set weight > 0",
                provider_id
            )));
        }

        let now = Instant::now();
        let mut available = Vec::new();
        let mut cooling = Vec::new();

        for deployment_index in pool.automatic_selector.plan() {
            let runtime = &pool.deployments[deployment_index];
            if runtime.is_available(now) {
                available.push(runtime.deployment.clone());
            } else {
                cooling.push(runtime.deployment.clone());
            }
        }

        if available.is_empty() {
            Ok(cooling)
        } else {
            Ok(available)
        }
    }

    /// Mark a deployment as healthy after a non-retryable upstream result.
    pub fn record_deployment_success(&self, provider_id: &str, deployment_id: &str) {
        let Some(pool) = self.deployment_pools.get(provider_id) else {
            return;
        };
        let Some(runtime) = pool.find_runtime(deployment_id) else {
            return;
        };

        runtime.mark_success(pool.base_cooldown);
    }

    /// Mark a deployment as temporarily unhealthy after a retryable upstream failure.
    pub fn record_deployment_failure(&self, provider_id: &str, deployment_id: &str) {
        let Some(pool) = self.deployment_pools.get(provider_id) else {
            return;
        };
        let Some(runtime) = pool.find_runtime(deployment_id) else {
            return;
        };

        let (consecutive_failures, cooldown) =
            runtime.mark_failure(Instant::now(), pool.max_cooldown);
        tracing::warn!(
            provider_id = %provider_id,
            deployment_id = %deployment_id,
            consecutive_failures,
            cooldown_ms = cooldown.as_millis(),
            r#type = "deployment-health",
            "Deployment entered cooldown after retryable upstream failure"
        );
    }

    /// Return configured deployments and their current routing health.
    pub fn deployment_snapshots(&self, provider_id: &str) -> Vec<ProviderDeploymentSnapshot> {
        let Some(provider) = self.providers.get(provider_id) else {
            return Vec::new();
        };
        let Some(pool) = self.deployment_pools.get(provider_id) else {
            return Vec::new();
        };

        let now = Instant::now();
        let mut snapshots = Vec::new();

        if !provider.api_key.trim().is_empty()
            && let Some(runtime) = pool.find_runtime(IMPLICIT_PROVIDER_DEPLOYMENT_ID)
        {
            snapshots.push(runtime.snapshot(now, true));
        }

        snapshots.extend(provider.deployments.iter().map(|deployment| {
            if deployment.enabled
                && let Some(runtime) = pool.find_runtime(&deployment.id)
            {
                return runtime.snapshot(now, false);
            }

            ProviderDeploymentSnapshot {
                id: deployment.id.clone(),
                enabled: false,
                implicit: false,
                weight: deployment.weight,
                automatic: false,
                state: DeploymentRuntimeState::Disabled,
                consecutive_failures: 0,
                cooldown_remaining_ms: None,
            }
        }));

        snapshots
    }

    /// Get provider information by ID.
    ///
    /// # Arguments
    ///
    /// * `id` - The provider identifier (e.g., "anthropic-code")
    ///
    /// # Returns
    ///
    /// `Some(&ProviderInfo)` if the provider exists, `None` otherwise.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use bifrost_server::Config;
    /// # use bifrost_server::provider::ProviderRegistry;
    /// # let config = Config::from_file("config.toml").unwrap();
    /// let registry = ProviderRegistry::from_config(&config);
    /// if let Some(provider) = registry.get("anthropic-code") {
    ///     println!("Base URL: {}", provider.base_url);
    /// }
    /// ```
    pub fn get(&self, id: &str) -> Option<&ProviderConfig> {
        self.providers.get(id)
    }

    /// Build an adapter chain (OnionExecutor) for the specified provider.
    ///
    /// This method creates the adapter chain based on the provider's endpoint type.
    /// The adapter is dynamically created internally based on route and endpoint combination.
    ///
    /// # Arguments
    ///
    /// * `provider_id` - The provider identifier
    ///
    /// # Returns
    ///
    /// `Ok(OnionExecutor)` if the provider exists and adapter chain is built successfully,
    /// `Err(LlmMapError)` if the provider is not found.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use bifrost_server::Config;
    /// # use bifrost_server::provider::ProviderRegistry;
    /// # use bifrost_server::routes::RouteEndpoint;
    /// # let config = Config::from_file("config.toml").unwrap();
    /// let registry = ProviderRegistry::from_config(&config);
    /// let executor = registry.build_executor("anthropic-code", &RouteEndpoint::OpenAIChat).unwrap();
    /// ```
    pub fn build_executor(
        &self,
        provider_id: &str,
        route: &RouteEndpoint,
    ) -> Result<OnionExecutor> {
        let provider = self.providers.get(provider_id).ok_or_else(|| {
            LlmMapError::Provider(format!("Provider '{}' not found", provider_id))
        })?;

        let adapters = self.build_adapter_chain(route, &provider.endpoint)?;

        Ok(OnionExecutor::new(adapters))
    }

    /// Build the adapter chain based on route and endpoint type.
    ///
    /// # Arguments
    ///
    /// * `route` - The route endpoint (OpenAIChat, OpenAIResponses, AnthropicMessages)
    /// * `endpoint` - The provider endpoint type (OpenAI or Anthropic)
    ///
    /// # Returns
    ///
    /// A vector of boxed adapters ready for execution.
    fn build_adapter_chain(
        &self,
        route: &RouteEndpoint,
        endpoint: &Endpoint,
    ) -> Result<Vec<Box<dyn Adapter<Error = LlmMapError>>>> {
        let mut adapters: Vec<Box<dyn Adapter<Error = LlmMapError>>> = Vec::new();

        match (route, endpoint) {
            (RouteEndpoint::OpenAIChat, Endpoint::OpenAI) => {
                adapters.push(Box::new(PassthroughAdapter));
            }
            (RouteEndpoint::OpenAIChat, Endpoint::Anthropic) => {
                adapters.push(Box::new(OpenAIToAnthropicAdapter::new()));
            }
            (RouteEndpoint::OpenAIResponses, Endpoint::OpenAI) => {
                adapters.push(Box::new(ResponsesToChatAdapter::new()));
            }
            (RouteEndpoint::OpenAIResponses, Endpoint::Anthropic) => {
                adapters.push(Box::new(ResponsesToChatAdapter::new()));
                adapters.push(Box::new(OpenAIToAnthropicAdapter::new()));
            }
            (RouteEndpoint::AnthropicMessages, Endpoint::Anthropic) => {
                adapters.push(Box::new(PassthroughAdapter));
            }
            (RouteEndpoint::AnthropicMessages, Endpoint::OpenAI) => {
                adapters.push(Box::new(AnthropicToOpenAIAdapter::new()));
            }
        }

        Ok(adapters)
    }

    /// Get the HTTP client for making upstream requests.
    pub fn http_client(&self) -> &HttpClient {
        &self.http_client
    }

    /// Get the number of registered providers.
    pub fn provider_count(&self) -> usize {
        self.providers.len()
    }

    /// Check if a provider exists in the registry.
    pub fn has_provider(&self, id: &str) -> bool {
        self.providers.contains_key(id)
    }

    /// Get all providers as a reference to the underlying HashMap.
    pub fn providers(&self) -> &HashMap<String, ProviderConfig> {
        &self.providers
    }

    pub fn get_alias_entry(&self, alias: &str) -> Option<&AliasEntry> {
        self.alias.get(alias)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{BodyEntry, Endpoint, HeaderEntry};
    use serde_json::json;

    /// Create a test configuration with a single provider
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

        Config {
            provider,
            server: crate::types::ServerConfig::default(),
            alias: HashMap::new(),
        }
    }

    fn create_test_config_with_endpoint() -> Config {
        let mut provider = HashMap::new();
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

    #[test]
    fn test_from_config() {
        let config = create_test_config();
        let registry = ProviderRegistry::from_config(&config);

        assert_eq!(registry.provider_count(), 1);
        assert!(registry.has_provider("test-provider"));
        assert!(!registry.has_provider("non-existent"));
    }

    #[test]
    fn test_get_provider() {
        let config = create_test_config();
        let registry = ProviderRegistry::from_config(&config);

        let provider = registry.get("test-provider");
        assert!(provider.is_some());

        let provider = provider.unwrap();
        assert_eq!(provider.base_url, "https://api.test.com");
        assert_eq!(provider.api_key, "test-key");
    }

    #[test]
    fn test_legacy_provider_fields_create_implicit_main_deployment() {
        let config = create_test_config();
        let registry = ProviderRegistry::from_config(&config);

        let plan = registry.deployment_plan("test-provider", None).unwrap();
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].id, IMPLICIT_PROVIDER_DEPLOYMENT_ID);
        assert_eq!(plan[0].weight, 1);

        let plan = registry
            .deployment_plan("test-provider", Some(IMPLICIT_PROVIDER_DEPLOYMENT_ID))
            .unwrap();
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].id, IMPLICIT_PROVIDER_DEPLOYMENT_ID);
    }

    #[test]
    fn test_get_non_existent_provider() {
        let config = create_test_config();
        let registry = ProviderRegistry::from_config(&config);

        let provider = registry.get("non-existent");
        assert!(provider.is_none());
    }

    #[tokio::test]
    async fn test_build_executor_passthrough() {
        let config = create_test_config();
        let registry = ProviderRegistry::from_config(&config);

        let executor = registry
            .build_executor("test-provider", &RouteEndpoint::OpenAIChat)
            .unwrap();
        assert_eq!(executor.adapter_count(), 1);

        // Test that executor can execute request
        let body = json!({"test": "data"});

        let result = executor.execute_request(body).await.unwrap();

        assert_eq!(result.body, json!({"test": "data"}));
    }

    #[tokio::test]
    async fn test_build_executor_with_adapter() {
        let config = create_test_config_with_endpoint();
        let registry = ProviderRegistry::from_config(&config);

        let executor = registry
            .build_executor("anthropic-provider", &RouteEndpoint::OpenAIChat)
            .unwrap();
        assert_eq!(executor.adapter_count(), 1);

        // Test that executor can execute request with adapter
        let body = json!({
            "model": "test-model",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "Hello"}]
        });
        let result = executor.execute_request(body).await.unwrap();

        // Verify adapter transformed the request body
        assert!(result.body.get("messages").is_some());
        assert!(result.body.get("max_tokens").is_some());
    }

    #[test]
    fn test_build_executor_non_existent_provider() {
        let config = create_test_config();
        let registry = ProviderRegistry::from_config(&config);

        let result = registry.build_executor("non-existent", &RouteEndpoint::OpenAIChat);
        assert!(result.is_err());

        if let Err(e) = result {
            assert!(e.to_string().contains("not found"));
        }
    }

    #[test]
    fn test_provider_info_config_accessor() {
        let config = create_test_config();
        let registry = ProviderRegistry::from_config(&config);

        let provider_config = registry.get("test-provider").unwrap();

        assert_eq!(provider_config.base_url, "https://api.test.com");
        assert_eq!(provider_config.api_key, "test-key");
        assert_eq!(provider_config.endpoint, Endpoint::OpenAI);
    }

    #[test]
    fn test_http_client_access() {
        let config = create_test_config();
        let registry = ProviderRegistry::from_config(&config);

        let _client = registry.http_client();
        // Just verify we can access the client
        // HttpClient accessed successfully
    }

    #[test]
    fn test_deployment_plan_uses_weights_for_first_choice() {
        let config = Config::from_toml(
            r#"
            [provider.test-provider]
            endpoint = "openai"

            [[provider.test-provider.deployments]]
            id = "a"
            base_url = "https://a.example.com"
            api_key = "key-a"
            weight = 2

            [[provider.test-provider.deployments]]
            id = "b"
            base_url = "https://b.example.com"
            api_key = "key-b"
            weight = 1
        "#,
        )
        .unwrap();
        config.validate().unwrap();
        let registry = ProviderRegistry::from_config(&config);

        let first_choices: Vec<String> = (0..3)
            .map(|_| {
                registry
                    .deployment_plan("test-provider", None)
                    .unwrap()
                    .first()
                    .unwrap()
                    .id
                    .clone()
            })
            .collect();

        assert_eq!(first_choices, ["a", "b", "a"]);
    }

    #[test]
    fn test_deployment_plan_handles_large_weights_without_expanding() {
        let config = Config::from_toml(
            r#"
            [provider.test-provider]
            endpoint = "openai"

            [[provider.test-provider.deployments]]
            id = "large"
            base_url = "https://large.example.com"
            api_key = "key-large"
            weight = 4294967295

            [[provider.test-provider.deployments]]
            id = "small"
            base_url = "https://small.example.com"
            api_key = "key-small"
            weight = 1
        "#,
        )
        .unwrap();
        config.validate().unwrap();
        let registry = ProviderRegistry::from_config(&config);

        let plan = registry.deployment_plan("test-provider", None).unwrap();
        assert_eq!(plan.len(), 2);
        assert_eq!(plan[0].id, "large");
    }

    #[test]
    fn test_deployment_plan_skips_cooling_deployment() {
        let config = Config::from_toml(
            r#"
            [server]
            deployment_cooldown_base_ms = 20
            deployment_cooldown_max_ms = 20

            [provider.test-provider]
            endpoint = "openai"

            [[provider.test-provider.deployments]]
            id = "a"
            base_url = "https://a.example.com"
            api_key = "key-a"

            [[provider.test-provider.deployments]]
            id = "b"
            base_url = "https://b.example.com"
            api_key = "key-b"
        "#,
        )
        .unwrap();
        config.validate().unwrap();
        let registry = ProviderRegistry::from_config(&config);

        assert_eq!(
            registry.deployment_plan("test-provider", None).unwrap()[0].id,
            "a"
        );
        registry.record_deployment_failure("test-provider", "a");

        let plan = registry.deployment_plan("test-provider", None).unwrap();
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].id, "b");

        std::thread::sleep(Duration::from_millis(25));
        let plan = registry.deployment_plan("test-provider", None).unwrap();
        assert_eq!(plan[0].id, "a");
    }

    #[test]
    fn test_deployment_plan_skips_zero_weight_unless_pinned() {
        let config = Config::from_toml(
            r#"
            [provider.test-provider]
            endpoint = "openai"

            [[provider.test-provider.deployments]]
            id = "auto"
            base_url = "https://auto.example.com"
            api_key = "key-auto"
            weight = 1

            [[provider.test-provider.deployments]]
            id = "manual"
            base_url = "https://manual.example.com"
            api_key = "key-manual"
            weight = 0
        "#,
        )
        .unwrap();
        config.validate().unwrap();
        let registry = ProviderRegistry::from_config(&config);

        let plan = registry.deployment_plan("test-provider", None).unwrap();
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].id, "auto");

        let plan = registry
            .deployment_plan("test-provider", Some("manual"))
            .unwrap();
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].id, "manual");
    }

    #[test]
    fn test_deployment_plan_requires_explicit_deployment_when_all_weights_are_zero() {
        let config = Config::from_toml(
            r#"
            [provider.test-provider]
            endpoint = "openai"

            [[provider.test-provider.deployments]]
            id = "manual"
            base_url = "https://manual.example.com"
            api_key = "key-manual"
            weight = 0
        "#,
        )
        .unwrap();
        config.validate().unwrap();
        let registry = ProviderRegistry::from_config(&config);

        assert!(
            registry
                .deployment_plan("test-provider", None)
                .unwrap_err()
                .to_string()
                .contains("no automatic deployments")
        );

        let plan = registry
            .deployment_plan("test-provider", Some("manual"))
            .unwrap();
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].id, "manual");
    }

    #[test]
    fn test_config_with_headers_and_body() {
        let mut provider = HashMap::new();
        provider.insert(
            "custom-provider".to_string(),
            ProviderConfig {
                base_url: "https://api.custom.com".to_string(),
                api_key: "custom-key".to_string(),
                endpoint: Endpoint::OpenAI,
                deployments: Vec::new(),
                headers: Some(vec![HeaderEntry {
                    name: "X-Custom-Header".to_string(),
                    value: "custom-value".to_string(),
                    condition: None,
                }]),
                body: Some(vec![BodyEntry {
                    name: "custom_field".to_string(),
                    value: json!("custom_value"),
                    condition: None,
                }]),
                models: None,
                exclude_headers: None,
                extend: false,
                body_policy: None,
            },
        );

        let config = Config {
            provider,
            server: crate::types::ServerConfig::default(),
            alias: HashMap::new(),
        };

        let registry = ProviderRegistry::from_config(&config);
        let provider_info = registry.get("custom-provider").unwrap();

        assert_eq!(provider_info.headers.as_ref().unwrap().len(), 1);
        assert_eq!(provider_info.body.as_ref().unwrap().len(), 1);
        assert_eq!(
            provider_info.headers.as_ref().unwrap()[0].name,
            "X-Custom-Header"
        );
        assert_eq!(provider_info.body.as_ref().unwrap()[0].name, "custom_field");
    }
}
