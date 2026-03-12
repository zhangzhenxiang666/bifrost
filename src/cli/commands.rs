//! CLI command implementations

use super::config::{
    cleanup_old_logs, get_config_path, get_logs_dir, get_pid_file_path, get_today_log_path,
    init_bifrost_dir,
};
use super::{
    print_error, print_header, print_info, print_kv_table, print_process_table, print_success,
    print_warning,
};
use anyhow::{Context, Result};
use colored::Colorize;
use daemonize::Daemonize;
use std::fs;
use std::net::TcpStream;
use std::process::Command;
use sysinfo::{Pid, System};
use tracing_subscriber::prelude::__tracing_subscriber_SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

/// Get the current process PID from PID file
pub fn get_stored_pid() -> Option<u32> {
    let pid_file = get_pid_file_path().ok()?;

    if !pid_file.exists() {
        return None;
    }

    fs::read_to_string(&pid_file)
        .ok()?
        .trim()
        .parse::<u32>()
        .ok()
}

/// Check if a process with given PID is running
pub fn is_process_running(pid: u32) -> bool {
    let mut system = System::new();
    system.refresh_processes(sysinfo::ProcessesToUpdate::All, true);

    system.process(Pid::from_u32(pid)).is_some()
}

/// Check if the server is currently running
pub fn is_server_running() -> bool {
    if let Some(pid) = get_stored_pid() {
        return is_process_running(pid);
    }
    false
}

/// Check if a port is in use
pub fn is_port_in_use(port: u16) -> bool {
    TcpStream::connect(format!("127.0.0.1:{}", port)).is_ok()
}

/// Get process information for a given PID
pub fn get_process_info(pid: u32) -> Option<(String, f32, f32)> {
    let mut system = System::new();
    system.refresh_processes(sysinfo::ProcessesToUpdate::All, true);

    if let Some(process) = system.process(Pid::from_u32(pid)) {
        let name = process.name().to_string_lossy().to_string();
        let memory_mb = process.memory() as f32 / 1024.0 / 1024.0;
        let cpu_percent = process.cpu_usage();
        return Some((name, memory_mb, cpu_percent));
    }

    None
}

/// Start command implementation
pub fn cmd_start() -> Result<()> {
    println!("\n{}", "LLM Map - Start Server".bold().white().on_blue());
    println!();
    cmd_start_internal()
}

/// Internal start logic (used by restart)
fn cmd_start_internal() -> Result<()> {
    // Initialize directory structure
    init_bifrost_dir()?;

    // Clean up old logs
    let deleted = cleanup_old_logs()?;
    if deleted > 0 {
        print_info("Cleaned up", &format!("{} old log file(s)", deleted));
    }

    // Check if server is already running
    if is_server_running() {
        print_warning("Server is already running");
        println!();

        if let Some(pid) = get_stored_pid()
            && let Some((name, memory, cpu)) = get_process_info(pid)
        {
            let config_path = get_config_path()?;
            if let Ok(config) = bifrost_server::config::Config::from_file(&config_path) {
                let proxy = config.server.proxy.as_deref().unwrap_or("None");
                print_process_table(
                    pid,
                    &name,
                    memory,
                    cpu,
                    Some(config.server.port),
                    Some(proxy),
                );
            } else {
                print_process_table(pid, &name, memory, cpu, None, None);
            }
        }

        println!();
        println!(
            "  {} To stop the server, run: {}",
            "→".cyan(),
            "bifrost stop".bold()
        );
        println!();

        return Ok(());
    }

    // Load configuration
    let config_path = get_config_path()?;
    let config = if config_path.exists() && fs::metadata(&config_path)?.len() > 0 {
        bifrost_server::config::Config::from_file(&config_path)
            .context("Failed to load configuration")?
    } else {
        let default_config = r#"[server]
port = 5564
"#;
        fs::write(&config_path, default_config)?;
        bifrost_server::config::Config::from_file(&config_path)?
    };

    let port = config.server.port;

    // Check if port is already in use
    if is_port_in_use(port) {
        print_error(&format!("Port {} is already in use", port));
        return Err(anyhow::anyhow!("Port {} is already in use", port));
    }

    // Get paths
    let pid_file = get_pid_file_path()?;
    let log_file = get_today_log_path()?;
    let stdout_path = get_logs_dir()?.join("bifrost.out");
    let stderr_path = get_logs_dir()?.join("bifrost.err");

    print_header("Starting Bifrost Server");

    // Display configuration as table
    let config_rows = vec![
        ("Port", port.to_string()),
        ("Config", config_path.display().to_string()),
        ("Log file", log_file.display().to_string()),
        (
            "Proxy",
            config
                .server
                .proxy
                .clone()
                .unwrap_or_else(|| "None".to_string()),
        ),
    ];
    print_kv_table(&config_rows);
    println!();
    println!("  {} Output log: {}", "→".cyan(), stdout_path.display());
    println!("  {} Error log: {}", "→".cyan(), stderr_path.display());
    // Open files for stdout and stderr
    let stdout_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&stdout_path)?;
    let stderr_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&stderr_path)?;

    // Daemonize the process
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
            // Set up logging to file (use blocking writer for simplicity)
            let log_dir = log_file.parent().unwrap().to_path_buf();
            let log_filename = log_file.file_name().unwrap().to_string_lossy().to_string();

            // Create a file appender that writes directly to the log file
            let file_appender = tracing_appender::rolling::never(&log_dir, &log_filename);

            // Initialize tracing for file logging
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

            // Write PID to file
            fs::write(&pid_file, std::process::id().to_string()).ok();

            // Run the server (this will block)
            bifrost_server::run_server(config)?;

            Ok(())
        }
        Err(e) => {
            print_error(&format!("Failed to daemonize: {}", e));
            Err(anyhow::anyhow!("Daemonize error: {}", e))
        }
    }
}

/// Stop command implementation
pub fn cmd_stop() -> Result<()> {
    println!("\n{}", "Bifrost - Stop Server".bold().white().on_red());
    println!();

    if !is_server_running() {
        print_warning("Server is not running");
        println!();
        println!(
            "  {} To start the server, run: {}",
            "→".cyan(),
            "bifrost start".bold()
        );
        println!();
        return Ok(());
    }

    let pid = get_stored_pid().context("Failed to get PID")?;

    print_header("Stopping Bifrost Server");
    println!();

    if let Some((name, memory, cpu)) = get_process_info(pid) {
        if let Ok(config) = bifrost_server::config::Config::from_file(&get_config_path()?) {
            let proxy = config.server.proxy.as_deref().unwrap_or("None");
            print_process_table(
                pid,
                &name,
                memory,
                cpu,
                Some(config.server.port),
                Some(proxy),
            );
        } else {
            print_process_table(pid, &name, memory, cpu, None, None);
        }
    }
    println!();

    let mut system = System::new();
    system.refresh_processes(sysinfo::ProcessesToUpdate::All, true);

    if let Some(process) = system.process(Pid::from_u32(pid)) {
        process.kill();
    }

    let mut attempts = 0;
    while is_process_running(pid) && attempts < 10 {
        std::thread::sleep(std::time::Duration::from_millis(500));
        attempts += 1;
    }

    if is_process_running(pid) {
        print_warning("Process did not terminate gracefully, forcing...");
        Command::new("kill")
            .arg("-9")
            .arg(pid.to_string())
            .output()
            .context("Failed to force kill process")?;
    }

    let pid_file = get_pid_file_path()?;
    if pid_file.exists() {
        fs::remove_file(&pid_file).ok();
    }

    print_success("Server stopped successfully");

    println!();
    println!(
        "  {} To start the server, run: {}",
        "→".cyan(),
        "bifrost start".bold()
    );
    println!();

    Ok(())
}

/// Restart command implementation
pub fn cmd_restart() -> Result<()> {
    println!("\n{}", "Bifrost - Restart Server".bold().white().on_cyan());
    if is_server_running() {
        // Stop server silently without detailed output
        if let Some(pid) = get_stored_pid() {
            let mut system = System::new();
            system.refresh_processes(sysinfo::ProcessesToUpdate::All, true);

            if let Some(process) = system.process(Pid::from_u32(pid)) {
                process.kill();
            }

            let mut attempts = 0;
            while is_process_running(pid) && attempts < 10 {
                std::thread::sleep(std::time::Duration::from_millis(500));
                attempts += 1;
            }

            if is_process_running(pid) {
                Command::new("kill")
                    .arg("-9")
                    .arg(pid.to_string())
                    .output()
                    .ok();
            }

            let pid_file = get_pid_file_path()?;
            if pid_file.exists() {
                fs::remove_file(&pid_file).ok();
            }
        }
        print_success("Server stopped");
    } else {
        print_warning("Server was not running");
    }

    // Start server without header (reuse start logic but skip the banner)
    cmd_start_internal()?;

    Ok(())
}

/// Status command implementation
pub fn cmd_status() -> Result<()> {
    println!("\n{}", "Bifrost - Server Status".bold().white().on_purple());
    println!();

    let config_path = get_config_path()?;

    if is_server_running() {
        print_success("Server is running");
        println!();

        if let Some(pid) = get_stored_pid() {
            if let Some((name, memory, cpu)) = get_process_info(pid) {
                if let Ok(config) = bifrost_server::config::Config::from_file(&config_path) {
                    let proxy = config.server.proxy.as_deref().unwrap_or("None");
                    print_process_table(
                        pid,
                        &name,
                        memory,
                        cpu,
                        Some(config.server.port),
                        Some(proxy),
                    );
                } else {
                    print_process_table(pid, &name, memory, cpu, None, None);
                }
            } else {
                print_process_table(pid, "unknown", 0.0, 0.0, None, None);
            }
        }
    } else {
        print_warning("Server is not running");
        println!();
        println!(
            "  {} To start the server, run: {}",
            "→".cyan(),
            "bifrost start".bold()
        );
    }

    println!();

    Ok(())
}
