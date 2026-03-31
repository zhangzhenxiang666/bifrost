//! Bifrost Server - Standalone server binary
//!
//! This is the entry point for the bifrost-server binary.
//! It loads configuration and runs the proxy server.

use bifrost_server::config;
use chrono::Local;
use daemonize::Daemonize;
use std::fs;
use tracing_subscriber::prelude::__tracing_subscriber_SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

fn main() -> anyhow::Result<()> {
    // Use fixed default paths
    let config_path = dirs::home_dir()
        .expect("Failed to get home directory")
        .join(".bifrost")
        .join("config.toml");

    let pid_file = dirs::home_dir()
        .expect("Failed to get home directory")
        .join(".bifrost")
        .join("bifrost.pid");

    // Log file with date-based naming (YYYY-MM-DD.log)
    let log_file = dirs::home_dir()
        .expect("Failed to get home directory")
        .join(".bifrost")
        .join("logs")
        .join(format!("{}.log", Local::now().format("%Y-%m-%d")));
    // Daemonize the process
    let stdout_file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_file.parent().unwrap().join("bifrost.out"))?;
    let stderr_file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_file.parent().unwrap().join("bifrost.err"))?;

    let daemonize = Daemonize::new()
        .pid_file(&pid_file)
        .chown_pid_file(false)
        .working_directory(std::env::current_dir()?)
        .user(whoami::username().as_str())
        .stdout(stdout_file)
        .stderr(stderr_file);

    match daemonize.start() {
        Ok(_) => {
            // We're now in daemon mode - run the server
            // Set up logging to file
            let log_dir = log_file.parent().unwrap().to_path_buf();
            let log_filename = log_file.file_name().unwrap().to_string_lossy().to_string();
            let file_appender = tracing_appender::rolling::never(&log_dir, &log_filename);

            tracing_subscriber::registry()
                .with(
                    tracing_subscriber::fmt::layer()
                        .with_target(false)
                        .with_line_number(false)
                        .with_thread_ids(false)
                        .with_thread_names(false)
                        .with_file(false)
                        .with_ansi(false)
                        .with_timer(tracing_subscriber::fmt::time::ChronoLocal::rfc_3339())
                        .compact()
                        .with_writer(file_appender),
                )
                .with(tracing_subscriber::EnvFilter::new("info"))
                .try_init()
                .ok();

            // Load configuration
            let mut config = config::Config::from_file(&config_path)?;

            if let Ok(https_proxy) = std::env::var("HTTPS_PROXY")
                && config.server.proxy.is_none()
            {
                config.server.proxy = Some(https_proxy);
            }

            if let Ok(http_proxy) = std::env::var("HTTP_PROXY")
                && config.server.proxy.is_none()
            {
                config.server.proxy = Some(http_proxy);
            }

            // Run the server
            bifrost_server::run_server(config)?;
            Ok(())
        }
        Err(e) => Err(anyhow::anyhow!("Daemonize error: {}", e)),
    }
}
