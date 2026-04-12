//! Bifrost Server - Standalone server binary
//!
//! This is the entry point for the bifrost-server binary.
//! It loads configuration and runs the proxy server.

use bifrost_server::Config;
use chrono::Local;
use daemonize::Daemonize;
use std::fs;
use std::io::Write;
use tracing_subscriber::prelude::__tracing_subscriber_SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

/// Environment variable for startup socket path
const STARTUP_SOCKET_ENV: &str = "BIFROST_STARTUP_SOCKET";

/// Send startup result to CLI via Unix Domain Socket
fn send_startup_result(socket_path: &str, success: bool, message: Option<&str>) {
    if let Ok(mut stream) = std::os::unix::net::UnixStream::connect(socket_path) {
        let msg = if success {
            serde_json::json!({
                "status": "success",
                "pid": std::process::id()
            })
        } else {
            serde_json::json!({
                "status": "failure",
                "message": message.unwrap_or("Unknown error")
            })
        };
        let _ = stream.write_all(msg.to_string().as_bytes());
        let _ = stream.flush();
    }
}

/// Run the server (blocking - runs until server stops)
fn run_server() -> anyhow::Result<()> {
    // Set up logging to file
    let log_dir = dirs::home_dir()
        .expect("Failed to get home directory")
        .join(".bifrost")
        .join("logs");
    let log_filename = format!("{}.log", Local::now().format("%Y-%m-%d"));
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

    // Use fixed default paths
    let config_path = dirs::home_dir()
        .expect("Failed to get home directory")
        .join(".bifrost")
        .join("config.toml");

    // Load configuration
    let mut config =
        Config::from_file(&config_path).map_err(|e| anyhow::anyhow!("Config error: {}", e))?;

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

    // Run the server - blocks until server stops
    bifrost_server::run_server(config)
}

fn main() -> anyhow::Result<()> {
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

    // Check for startup socket from CLI
    let startup_socket = std::env::var(STARTUP_SOCKET_ENV).ok();

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
            let result = run_server();
            match result {
                Ok(()) => Ok(()),
                Err(e) => {
                    // Server failed - send error via UDS if available
                    if let Some(ref socket_path) = startup_socket {
                        send_startup_result(socket_path, false, Some(&e.to_string()));
                    }
                    Err(e)
                }
            }
        }
        Err(e) => {
            // Fork failed - notify CLI if socket is available
            if let Some(ref socket_path) = startup_socket {
                send_startup_result(socket_path, false, Some(&format!("Daemonize error: {}", e)));
            }
            Err(anyhow::anyhow!("Daemonize error: {}", e))
        }
    }
}
