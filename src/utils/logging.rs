//! Logging utilities for tracing configuration

use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

/// Initialize logging with tracing-subscriber
///
/// Configures:
/// - Timestamp in ISO 8601 format
/// - Log level display
/// - Module path and line numbers
/// - RUST_LOG environment variable support
pub fn init_logging() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_target(true)
                .with_line_number(true)
                .with_timer(tracing_subscriber::fmt::time::UtcTime::rfc_3339()),
        )
        .with(EnvFilter::from_default_env())
        .init();
}
