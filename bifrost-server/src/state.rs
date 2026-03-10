use crate::provider::registry::ProviderRegistry;
use std::sync::Arc;

/// Application state for route handlers
#[derive(Clone)]
pub struct AppState {
    pub registry: Arc<ProviderRegistry>,
}

impl From<ProviderRegistry> for AppState {
    fn from(registry: ProviderRegistry) -> Self {
        Self {
            registry: Arc::new(registry),
        }
    }
}
