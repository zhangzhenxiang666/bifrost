use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use colored::Colorize;

use super::config_check::validate_config;
use super::printing::{
    print_error, print_header, print_info, print_kv_table, print_process_table, print_success,
    print_warning,
};
use super::utils::{
    STARTUP_SOCKET_ENV, ServerStartResult, create_startup_socket, get_env_proxy, get_process_info,
    get_stored_pid, is_port_in_use, is_process_running, is_server_running, wait_for_startup_result,
};
use crate::config::{
    BIFROST_DIR, cleanup_old_logs, get_config_path, get_logs_dir, get_today_log_path,
    init_bifrost_dir,
};

pub struct ServerConfig {
    pub port: u16,
    pub proxy: Option<String>,
}

impl ServerConfig {
    pub fn from_file(path: &PathBuf) -> Result<Self> {
        let content = fs::read_to_string(path)?;
        let config: toml::Value = toml::from_str(&content)?;

        let port = config
            .get("server")
            .and_then(|s| s.get("port"))
            .and_then(|v| v.as_integer())
            .unwrap_or(5564) as u16;

        let proxy = config
            .get("server")
            .and_then(|s| s.get("proxy"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        Ok(ServerConfig { port, proxy })
    }
}

fn get_server_binary_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(BIFROST_DIR)
        .join("bin")
        .join("bifrost-server")
}

pub fn cmd_start() -> Result<()> {
    println!("\n{}", "LLM Map - Start Server".bold().white().on_blue());
    println!();
    cmd_start_internal()
}

pub fn cmd_start_internal() -> Result<()> {
    init_bifrost_dir()?;

    if let Err(e) = validate_config() {
        print_error(&format!("Config validation failed:\n\n{}", e));
        return Err(anyhow::anyhow!("Config validation failed"));
    }

    let deleted = cleanup_old_logs()?;
    if deleted > 0 {
        print_info("Cleaned up", &format!("{} old log file(s)", deleted));
    }

    if is_server_running() {
        print_warning("Server is already running");
        println!();

        if let Some(pid) = get_stored_pid()
            && let Some((name, memory, cpu)) = get_process_info(pid)
        {
            let config_path = get_config_path()?;
            if let Ok(config) = ServerConfig::from_file(&config_path) {
                let env_proxy = get_env_proxy();
                let proxy = config.proxy.clone().or(env_proxy);
                print_process_table(pid, &name, memory, cpu, Some(config.port), proxy.as_deref());
            } else {
                let env_proxy = get_env_proxy();
                print_process_table(pid, &name, memory, cpu, None, env_proxy.as_deref());
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

    let config_path = get_config_path()?;
    let config = if config_path.exists() && fs::metadata(&config_path)?.len() > 0 {
        ServerConfig::from_file(&config_path)?
    } else {
        let default_config = r#"[server]
port = 5564
"#;
        fs::write(&config_path, default_config)?;
        ServerConfig::from_file(&config_path)?
    };

    let port = config.port;

    if is_port_in_use(port) {
        print_error(&format!("Port {} is already in use", port));
        return Err(anyhow::anyhow!("Port {} is already in use", port));
    }

    let log_file = get_today_log_path()?;
    let stdout_path = get_logs_dir()?.join("bifrost.out");
    let stderr_path = get_logs_dir()?.join("bifrost.err");

    print_header("Starting Bifrost Server");

    let env_proxy = get_env_proxy();
    let proxy = config.proxy.clone().or(env_proxy);

    let config_rows = vec![
        ("Port", port.to_string()),
        ("Config", config_path.display().to_string()),
        ("Log file", log_file.display().to_string()),
        ("Proxy", proxy.unwrap_or_else(|| "None".to_string())),
    ];
    print_kv_table(&config_rows);
    println!();
    println!("  {} Output log: {}", "→".cyan(), stdout_path.display());
    println!("  {} Error log: {}", "→".cyan(), stderr_path.display());

    let server_binary = get_server_binary_path();
    println!(
        "  {} Server binary: {}",
        "→".cyan(),
        server_binary.display()
    );
    println!();

    let (listener, socket_path) = create_startup_socket()?;

    let stdout_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&stdout_path)?;
    let stderr_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&stderr_path)?;

    let child = std::process::Command::new(&server_binary)
        .env(STARTUP_SOCKET_ENV, socket_path.to_string_lossy().as_ref())
        .stdout(stdout_file)
        .stderr(stderr_file)
        .spawn()
        .context(format!(
            "Failed to start server binary at {:?}",
            server_binary
        ))?;

    let server_pid = child.id();

    let startup_failed = match wait_for_startup_result(&listener) {
        Ok(result) => match result {
            ServerStartResult::Failure { message } => Some(message),
            ServerStartResult::Success { .. } => None,
        },
        Err(_) => {
            if !is_process_running(server_pid) {
                Some("Server process terminated immediately".to_string())
            } else {
                None
            }
        }
    };

    let _ = std::fs::remove_file(&socket_path);

    if let Some(message) = startup_failed {
        print_error(&format!("Server failed to start: {}", message));
        println!(
            "  {} Check error log: {}",
            "→".cyan(),
            stderr_path.display()
        );
        return Err(anyhow::anyhow!("Server start failed: {}", message));
    }

    let actual_pid =
        get_stored_pid().context("Failed to read PID file - server may have failed to start")?;

    print_success("Server started successfully");
    println!();
    println!(
        "  {} Server PID: {}",
        "→".cyan(),
        actual_pid.to_string().bold()
    );
    println!(
        "  {} To stop the server, run: {}",
        "→".cyan(),
        "bifrost stop".bold()
    );
    println!();

    Ok(())
}
