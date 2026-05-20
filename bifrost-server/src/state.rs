use crate::provider::registry::ProviderRegistry;
use std::sync::Arc;

/// Application state for route handlers
#[derive(Debug, Clone)]
pub struct AppState {
    pub registry: Arc<ProviderRegistry>,
    /// Current proxy configuration, if any
    pub proxy: Option<String>,
}

impl AppState {
    /// Create AppState (proxy is None when no proxy is configured)
    pub fn new(registry: ProviderRegistry, proxy: Option<String>) -> Self {
        Self {
            registry: Arc::new(registry),
            proxy,
        }
    }
}

impl From<ProviderRegistry> for AppState {
    fn from(registry: ProviderRegistry) -> Self {
        Self::new(registry, None)
    }
}
