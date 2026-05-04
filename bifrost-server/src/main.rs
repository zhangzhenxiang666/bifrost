//! Bifrost Server - Standalone server binary
//!
//! This is the entry point for the bifrost-server binary.
//! It loads configuration and runs the proxy server.

use bifrost_server::Config;
use std::fs;
use std::io::Write;
use tracing_rolling_file::*;
use tracing_subscriber::prelude::__tracing_subscriber_SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

/// Environment variable for startup socket path
const STARTUP_SOCKET_ENV: &str = "BIFROST_STARTUP_SOCKET";

/// Send startup result to CLI via Unix Domain Socket
#[cfg(unix)]
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

/// Send startup result to CLI via TCP localhost
#[cfg(windows)]
fn send_startup_result(address: &str, success: bool, message: Option<&str>) {
    use std::net::TcpStream;

    if let Ok(mut stream) = TcpStream::connect(address) {
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
    // Set up logging to file with size-based rotation and date-based naming
    let log_dir = dirs::home_dir()
        .expect("Failed to get home directory")
        .join(".bifrost")
        .join("logs");

    // Create log file with date in filename
    let date = chrono::Local::now().format("%Y-%m-%d").to_string();
    let log_path = log_dir.join(format!("{}.log", date));
    let file_appender = RollingFileAppenderBase::new(
        log_path.to_string_lossy().to_string(),
        RollingConditionBase::new()
            .daily()
            .max_size(50 * 1024 * 1024), // daily or 50MB
        24, // keep 24 files max
    )
    .expect("Failed to build log appender");

    let (non_blocking, _guard) = file_appender.get_non_blocking_appender();

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
                .json()
                .with_writer(non_blocking),
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

#[cfg(unix)]
fn main() -> anyhow::Result<()> {
    use daemonize::Daemonize;
    let pid_file = dirs::home_dir()
        .expect("Failed to get home directory")
        .join(".bifrost")
        .join("bifrost.pid");

    // Log directory for daemon output files
    let log_dir = dirs::home_dir()
        .expect("Failed to get home directory")
        .join(".bifrost")
        .join("logs");

    // Daemonize the process
    let stdout_file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_dir.join("bifrost.out"))?;
    let stderr_file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_dir.join("bifrost.err"))?;

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

#[cfg(windows)]
fn main() -> anyhow::Result<()> {
    use std::os::windows::process::CommandExt;

    const CREATE_NO_WINDOW: u32 = 0x08000000;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x00000200;
    const DAEMON_ENV: &str = "BIFROST_DAEMON_RUNNING";

    // If not already running as daemon, re-spawn ourselves as a detached process
    if std::env::var(DAEMON_ENV).is_err() {
        let current_exe = std::env::current_exe().expect("Failed to get current executable path");

        // Set up log files for daemon stdout/stderr (mirrors Unix Daemonize behavior)
        let log_dir = dirs::home_dir()
            .expect("Failed to get home directory")
            .join(".bifrost")
            .join("logs");
        fs::create_dir_all(&log_dir)?;

        let stdout_file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_dir.join("bifrost.out"))?;
        let stderr_file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_dir.join("bifrost.err"))?;

        let mut child = std::process::Command::new(current_exe);
        child
            .env(DAEMON_ENV, "1")
            .creation_flags(CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP)
            .stdout(stdout_file)
            .stderr(stderr_file);

        // Forward the startup socket env var to the daemon child
        if let Ok(socket) = std::env::var(STARTUP_SOCKET_ENV) {
            child.env(STARTUP_SOCKET_ENV, socket);
        }

        child.spawn()?;
        // Parent exits immediately, daemon child continues in background
        return Ok(());
    }

    // We are the daemon child - write PID and run server
    let pid_file = dirs::home_dir()
        .expect("Failed to get home directory")
        .join(".bifrost")
        .join("bifrost.pid");

    if let Some(parent) = pid_file.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&pid_file, std::process::id().to_string())?;

    // Check for startup IPC address from CLI
    let startup_address = std::env::var(STARTUP_SOCKET_ENV).ok();

    // Run the server (blocking)
    let result = run_server();

    // Clean up PID file on exit
    if pid_file.exists() {
        fs::remove_file(&pid_file).ok();
    }

    match result {
        Ok(()) => Ok(()),
        Err(e) => {
            // Notify CLI of failure
            if let Some(ref address) = startup_address {
                send_startup_result(address, false, Some(&e.to_string()));
            }
            Err(e)
        }
    }
}
