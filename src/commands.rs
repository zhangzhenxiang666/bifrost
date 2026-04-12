//! CLI command implementations

use crate::config::{
    BIFROST_DIR, cleanup_old_logs, get_bifrost_dir, get_config_path, get_logs_dir,
    get_pid_file_path, get_today_log_path, init_bifrost_dir,
};
use anyhow::{Context, Result};
use clap::Subcommand;
use colored::Colorize;
use flate2::read::GzDecoder;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Read;
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};
use sysinfo::{Pid, System};
use tabled::{
    Table, Tabled,
    settings::{
        Alignment, Remove, Style,
        object::{Columns, Rows},
    },
};
use tar::Archive;

/// Environment variable for startup socket path
const STARTUP_SOCKET_ENV: &str = "BIFROST_STARTUP_SOCKET";

/// Predefined adapter names
const PREDEFINED_ADAPTERS: &[&str] = &[
    "passthrough",
    "openai_to_qwen",
    "openai-to-qwen",
    "anthropic_to_openai",
    "anthropic-to-openai",
    "anthropic_to_qwen",
    "anthropic-to-qwen",
];

/// Server startup result sent via Unix Domain Socket
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "status")]
enum ServerStartResult {
    #[serde(rename = "success")]
    Success { pid: u32 },
    #[serde(rename = "failure")]
    Failure { message: String },
}

#[derive(Subcommand)]
pub enum Commands {
    /// Start the LLM Map server as a daemon
    #[command(arg_required_else_help = false)]
    Start,

    /// Stop the LLM Map server
    #[command(arg_required_else_help = false)]
    Stop,

    /// Restart the LLM Map server
    #[command(arg_required_else_help = false)]
    Restart,

    /// Show the current status of the LLM Map server
    #[command(arg_required_else_help = false)]
    Status,

    /// List all providers from the running server
    #[command(arg_required_else_help = false)]
    List,

    /// Upgrade bifrost and bifrost-server to the latest version
    #[command(arg_required_else_help = false)]
    Upgrade,
}

/// Print a formatted info message with colored label
pub fn print_info(label: &str, value: &str) {
    println!("{:<18} {}", label.bold().cyan(), value);
}

/// Print a formatted success message with green checkmark
pub fn print_success(message: &str) {
    println!("{} {}", "✓".green().bold(), message.green());
}

/// Print a formatted error message with red X
pub fn print_error(message: &str) {
    eprintln!("{} {}", "✗".red().bold(), message.red());
}

/// Print a formatted warning message with yellow triangle
pub fn print_warning(message: &str) {
    println!("{} {}", "⚠".yellow().bold(), message.yellow());
}

/// Print a section header with purple background
pub fn print_header(title: &str) {
    println!("\n{}", title.bold().white().on_purple());
}

/// Print a 2-column table for key-value pairs
pub fn print_kv_table(rows: &[(&str, String)]) {
    let mut table = Table::new(rows);
    table
        .with(Style::modern())
        .modify(Columns::first(), Alignment::left())
        .modify(Columns::last(), Alignment::left());
    table.with(Remove::row(Rows::new(0..1)));
    println!("{}", table);
}

/// Print process info table
pub fn print_process_table(
    pid: u32,
    name: &str,
    memory: f32,
    cpu: f32,
    port: Option<u16>,
    proxy: Option<&str>,
) {
    let mut rows = vec![
        ("PID", pid.to_string()),
        ("Process", name.to_string()),
        ("Memory", format!("{:.2} MB", memory)),
        ("CPU", format!("{:.1}%", cpu)),
    ];
    if let Some(port) = port {
        rows.push(("Port", port.to_string()));
    }
    if let Some(proxy) = proxy {
        rows.push(("Proxy", proxy.to_string()));
    } else {
        rows.push(("Proxy", "None".to_string()));
    }
    print_kv_table(&rows);
}

/// Status response from /status endpoint
#[derive(Debug, Deserialize)]
struct StatusResponse {
    proxy: Option<String>,
    providers: Vec<ProviderInfo>,
}

/// Provider information from status endpoint
#[derive(Debug, Deserialize, Tabled, Clone)]
struct ProviderInfo {
    name: String,
    endpoint: String,
}

/// Get proxy from environment variables (HTTPS_PROXY > HTTP_PROXY)
fn get_env_proxy() -> Option<String> {
    std::env::var("HTTPS_PROXY")
        .ok()
        .or_else(|| std::env::var("HTTP_PROXY").ok())
}

/// Get status from running server
fn get_status_from_server(port: u16) -> Result<Option<StatusResponse>> {
    let url = format!("http://127.0.0.1:{}/status", port);
    match reqwest::blocking::get(&url) {
        Ok(resp) => Ok(Some(resp.json::<StatusResponse>()?)),
        Err(e) if e.is_connect() => Ok(None),
        Err(e) => Err(anyhow::anyhow!("Failed to connect to server: {}", e)),
    }
}

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

    std::thread::sleep(std::time::Duration::from_millis(200));

    system.refresh_processes(sysinfo::ProcessesToUpdate::All, true);

    if let Some(process) = system.process(Pid::from_u32(pid)) {
        let name = process.name().to_string_lossy().to_string();
        let memory_mb = process.memory() as f32 / 1024.0 / 1024.0;
        let cpu_percent = process.cpu_usage();
        return Some((name, memory_mb, cpu_percent));
    }

    None
}

/// Server configuration (minimal structure for CLI needs)
#[derive(Debug, Clone)]
struct ServerConfig {
    port: u16,
    proxy: Option<String>,
}

impl ServerConfig {
    fn from_file(path: &PathBuf) -> Result<Self> {
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

/// Get the bifrost-server binary path (default: ~/.bifrost/bin/bifrost-server)
fn get_server_binary_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(BIFROST_DIR)
        .join("bin")
        .join("bifrost-server")
}

/// Create a Unix Domain Socket for startup communication
fn create_startup_socket() -> Result<(std::os::unix::net::UnixListener, PathBuf)> {
    let socket_path =
        get_bifrost_dir()?.join(format!("bifrost_startup_{}.sock", std::process::id()));

    // Remove existing socket file if any
    let _ = std::fs::remove_file(&socket_path);

    let listener = std::os::unix::net::UnixListener::bind(&socket_path)
        .context("Failed to create startup socket")?;

    // Set non-blocking mode for timeout handling
    listener
        .set_nonblocking(true)
        .context("Failed to set non-blocking mode")?;

    Ok((listener, socket_path))
}

/// Wait for startup result from server via Unix Domain Socket
fn wait_for_startup_result(
    listener: &std::os::unix::net::UnixListener,
) -> Result<ServerStartResult> {
    let deadline = Instant::now() + Duration::from_millis(500);
    let mut buffer = Vec::new();

    loop {
        if Instant::now() >= deadline {
            return Err(anyhow::anyhow!("Timeout waiting for server startup result"));
        }

        match listener.accept() {
            Ok((mut stream, _)) => {
                stream.read_to_end(&mut buffer)?;
                return Ok(serde_json::from_slice(&buffer)?);
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => {
                return Err(anyhow::anyhow!("UDS accept error: {}", e));
            }
        }
    }
}

/// Validate config.toml before starting server
fn validate_config() -> Result<(), String> {
    let config_path = get_config_path().map_err(|e| e.to_string())?;

    // Config file doesn't exist is valid (will use defaults)
    if !config_path.exists() {
        return Ok(());
    }

    let content = fs::read_to_string(&config_path).map_err(|e| e.to_string())?;

    // TOML syntax validation via bifrost-config
    let config: bifrost_config::Config =
        toml::from_str(&content).map_err(|e| format!("TOML syntax error: {}", e))?;

    // Semantic validation: adapter names
    for adapter in config.used_adapters() {
        if !PREDEFINED_ADAPTERS.contains(&adapter.as_str()) {
            return Err(format!(
                "Unknown adapter '{}'. Valid adapters are: {:?}",
                adapter, PREDEFINED_ADAPTERS
            ));
        }
    }

    Ok(())
}

/// Start command implementation
pub fn cmd_start() -> Result<()> {
    println!("\n{}", "LLM Map - Start Server".bold().white().on_blue());
    println!();
    cmd_start_internal()
}

/// Internal start logic (used by restart)
pub fn cmd_start_internal() -> Result<()> {
    init_bifrost_dir()?;

    // Validate config before starting
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

    // Create Unix Domain Socket for startup communication
    let (listener, socket_path) = create_startup_socket()?;

    let stdout_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&stdout_path)?;
    let stderr_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&stderr_path)?;

    let child = Command::new(&server_binary)
        .env(STARTUP_SOCKET_ENV, socket_path.to_string_lossy().as_ref())
        .stdout(stdout_file)
        .stderr(stderr_file)
        .spawn()
        .context(format!(
            "Failed to start server binary at {:?}",
            server_binary
        ))?;

    let server_pid = child.id();

    // Wait for startup result via UDS (only failures are reported)
    let startup_failed = match wait_for_startup_result(&listener) {
        Ok(result) => {
            // Received a message - must be a failure
            match result {
                ServerStartResult::Failure { message } => Some(message),
                ServerStartResult::Success { .. } => None,
            }
        }
        Err(_) => {
            // Timeout or error - check if process is still running
            if !is_process_running(server_pid) {
                Some("Server process terminated immediately".to_string())
            } else {
                None
            }
        }
    };

    // Clean up socket file
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

    // Read the actual daemon PID from the file written by Daemonize
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

/// Stop command implementation
pub fn cmd_stop() -> Result<()> {
    println!("\n{}", "Bifrost - Stop Server".bold().white().on_red());
    println!();

    if !is_server_running() {
        print_warning("Server is not running");
        println!();
        println!(
            "{} To start the server, run: {}",
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
        if let Ok(config) = ServerConfig::from_file(&get_config_path()?) {
            let proxy = config.proxy.as_deref().unwrap_or("None");
            print_process_table(pid, &name, memory, cpu, Some(config.port), Some(proxy));
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
        "{} To start the server, run: {}",
        "→".cyan(),
        "bifrost start".bold()
    );
    println!();

    Ok(())
}

/// Restart command implementation
pub fn cmd_restart() -> Result<()> {
    println!("\n{}", "Bifrost - Restart Server".bold().white().on_cyan());

    // Validate config before restarting
    if let Err(e) = validate_config() {
        print_error(&format!("Config validation failed:\n\n{}", e));
        return Err(anyhow::anyhow!("Config validation failed"));
    }

    if is_server_running() {
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
        println!("\n{}", "→ Server stopped".bold().green());
    } else {
        println!("\n{}", "⚠ Server was not running".bold().yellow());
    }

    cmd_start_internal()?;

    Ok(())
}

/// Status command implementation
pub fn cmd_status() -> Result<()> {
    let config_path = get_config_path()?;

    if is_server_running() {
        println!(
            "\n{}",
            "Bifrost - Server Status: Running".bold().white().on_green()
        );
        println!();

        let (proxy, port): (Option<String>, Option<u16>) =
            if let Some(status) = get_status_from_server(5564).ok().flatten() {
                (status.proxy, Some(5564))
            } else {
                let config = ServerConfig::from_file(&config_path).ok();
                let env_proxy = get_env_proxy();
                let proxy = config.as_ref().and_then(|c| c.proxy.clone()).or(env_proxy);
                (proxy, config.map(|c| c.port))
            };

        if let Some(pid) = get_stored_pid() {
            if let Some((name, memory, cpu)) = get_process_info(pid) {
                print_process_table(pid, &name, memory, cpu, port, proxy.as_deref());
            } else {
                print_process_table(pid, "unknown", 0.0, 0.0, port, proxy.as_deref());
            }
        }
    } else {
        println!(
            "\n{}",
            "Bifrost - Server Status: Stopped"
                .bold()
                .white()
                .on_yellow()
        );
        println!(
            "{} To start the server, run: {}",
            "→".cyan(),
            "bifrost start".bold()
        );
        println!();
    }

    Ok(())
}

/// List command implementation
pub fn cmd_list() -> Result<()> {
    println!("\n{}", "Bifrost - Provider List".bold().white().on_green());
    println!();

    if !is_server_running() {
        print_warning("Server is not running");
        println!(
            "{} To start the server, run: {}",
            "→".cyan(),
            "bifrost start".bold()
        );
        println!();
        return Ok(());
    }

    match get_status_from_server(5564) {
        Ok(Some(status)) => {
            if status.providers.is_empty() {
                print_warning("No providers configured");
            } else {
                print_providers_table(&status.providers);
            }
            println!();
        }
        Ok(None) => {
            print_error("Failed to connect to server");
            println!();
        }
        Err(e) => {
            print_error(&format!("Failed to get status: {}", e));
            println!();
        }
    }

    Ok(())
}

/// Print providers in a table format
fn print_providers_table(providers: &[ProviderInfo]) {
    use tabled::settings::Style;

    let mut sorted = providers.to_vec();
    sorted.sort_by(|a, b| a.name.cmp(&b.name));

    let mut table = Table::new(&sorted);
    table.with(Style::sharp());
    println!("{}", table);
}

/// Platform descriptor for binary asset naming
struct Platform {
    suffix: String,
}

impl Platform {
    fn detect() -> Self {
        let os = std::process::Command::new("uname")
            .arg("-s")
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_else(|_| "Linux".to_string());

        let arch = std::process::Command::new("uname")
            .arg("-m")
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_else(|_| "x86_64".to_string());

        let suffix = match (os.as_str(), arch.as_str()) {
            ("Linux", "x86_64") => "linux-amd64",
            ("Linux", "aarch64") | ("Linux", "arm64") => "linux-aarch64",
            ("Darwin", "x86_64") => "darwin-amd64",
            ("Darwin", "arm64") => "darwin-aarch64",
            _ => "linux-amd64",
        };

        Platform {
            suffix: suffix.to_string(),
        }
    }
}

const GITHUB_REPO: &str = "zhangzhenxiang666/bifrost";

/// Fetch the latest release tag from GitHub API
fn fetch_remote_version() -> Result<String> {
    let url = format!(
        "https://api.github.com/repos/{}/releases/latest",
        GITHUB_REPO
    );
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;
    let resp = client
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "bifrost-upgrade/1.0")
        .send()?;
    let json: serde_json::Value = resp.json()?;
    let tag = json["tag_name"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("failed to parse tag_name"))?;
    Ok(tag.to_string())
}

/// Get the current local version by running `bifrost -V`
fn get_local_version() -> Result<semver::Version> {
    let binary_path = get_server_binary_path().with_file_name("bifrost");
    let output = std::process::Command::new(&binary_path)
        .arg("-V")
        .output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let version_str = stdout.trim().trim_start_matches("bifrost ");
    semver::Version::parse(version_str)
        .map_err(|_| anyhow::anyhow!("failed to parse version from: {}", version_str))
}

fn download_and_extract(github_tag: &str, platform: &Platform) -> Result<std::path::PathBuf> {
    let asset_name = format!("bifrost-{}-{}.tar.gz", github_tag, platform.suffix);
    let download_url = format!(
        "https://github.com/{}/releases/download/{}/{}",
        GITHUB_REPO, github_tag, asset_name
    );

    println!("Downloading: {}", asset_name);

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()?;
    let mut resp = client.get(&download_url).send()?;
    resp.error_for_status_ref()?;

    let temp_dir = std::env::temp_dir().join(format!("bifrost-upgrade-{}", std::process::id()));
    std::fs::create_dir_all(&temp_dir)?;

    let archive_path = temp_dir.join(&asset_name);
    let mut file = std::fs::File::create(&archive_path)?;
    std::io::copy(&mut resp, &mut file)?;

    println!("Extracting...");
    let tar_gz = std::fs::File::open(&archive_path)?;
    let decoder = GzDecoder::new(tar_gz);
    let mut archive = Archive::new(decoder);
    archive.unpack(&temp_dir)?;

    Ok(temp_dir)
}

fn install_binaries(temp_dir: &std::path::Path, platform: &Platform) -> Result<()> {
    let server_binary_path = get_server_binary_path();
    let install_dir = server_binary_path.parent().unwrap();
    std::fs::create_dir_all(install_dir)?;

    let bifrost_src = temp_dir.join(format!("bifrost-{}", platform.suffix));
    let server_src = temp_dir.join(format!("bifrost-server-{}", platform.suffix));
    let bifrost_dst = install_dir.join("bifrost");
    let server_dst = install_dir.join("bifrost-server");

    std::fs::rename(&bifrost_src, &bifrost_dst).or_else(|_| {
        std::fs::copy(&bifrost_src, &bifrost_dst).and_then(|_| std::fs::remove_file(&bifrost_src))
    })?;
    std::fs::rename(&server_src, &server_dst).or_else(|_| {
        std::fs::copy(&server_src, &server_dst).and_then(|_| std::fs::remove_file(&server_src))
    })?;

    std::fs::remove_dir_all(temp_dir).ok();

    println!("Installing binaries... Done");

    Ok(())
}

/// Upgrade command implementation
pub fn cmd_upgrade() -> Result<()> {
    println!();

    let platform = Platform::detect();

    let remote_tag = fetch_remote_version()?;
    let remote_tag_stripped = remote_tag.trim_start_matches('v');
    let remote = semver::Version::parse(remote_tag_stripped)
        .map_err(|_| anyhow::anyhow!("failed to parse remote version: {}", remote_tag_stripped))?;

    let local = match get_local_version() {
        Ok(v) => v,
        Err(e) => {
            print_warning(&format!(
                "Failed to get local version: {}. Proceeding with upgrade anyway.",
                e
            ));
            semver::Version::new(0, 0, 0)
        }
    };

    if local >= remote {
        println!("✓ Already up to date (v{})", local);
        println!();
        return Ok(());
    }

    println!("Checking version: v{} < v{} (remote)", local, remote);

    let temp_dir = download_and_extract(&remote_tag, &platform)?;

    let server_was_running = is_server_running();

    if server_was_running {
        print_info("Stopping service...", "");
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
            if let Ok(pid_file) = get_pid_file_path() {
                fs::remove_file(&pid_file).ok();
            }
        }
        println!("Done");
    } else {
        print_info("Stopping service...", "Not running, skipping");
    }

    if let Err(e) = install_binaries(&temp_dir, &platform) {
        return Err(anyhow::anyhow!("Install failed: {}", e));
    }

    if server_was_running {
        print_info("Restarting service...", "");
        cmd_start_internal()?;
        println!("Done");
    }

    println!("✓ Upgrade complete (v{} → v{})", local, remote);
    println!();

    Ok(())
}
