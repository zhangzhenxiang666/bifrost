use crate::provider::registry::ProviderRegistry;
use std::sync::Arc;
use std::sync::OnceLock;

static GLOBAL_STATE: OnceLock<AppState> = OnceLock::new();

pub fn set_global_state(state: AppState) {
    GLOBAL_STATE.set(state).expect("global state already set");
}

pub fn get_global_state() -> &'static AppState {
    GLOBAL_STATE.get().expect("global state not set")
}

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
