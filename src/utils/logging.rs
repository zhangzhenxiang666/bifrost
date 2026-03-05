//! Logging utilities for tracing configuration

use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

/// Initialize logging with tracing-subscriber
///
/// Configures:
/// - Colorized output
/// - Compact format with timestamp
/// - RUST_LOG environment variable support
pub fn init_logging() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_target(false)
                .with_line_number(false)
                .with_thread_ids(false)
                .with_thread_names(false)
                .with_file(false)
                .with_ansi(true)
                .with_timer(tracing_subscriber::fmt::time::ChronoUtc::rfc_3339())
                .compact(),
        )
        .with(EnvFilter::new("info"))
        .init();
}
