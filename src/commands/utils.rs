use crate::config::get_pid_file_path;
use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Read;
use std::net::TcpStream;
use std::time::{Duration, Instant};
use sysinfo::{Pid, System};

pub const STARTUP_SOCKET_ENV: &str = "BIFROST_STARTUP_SOCKET";

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "status")]
pub enum ServerStartResult {
    #[serde(rename = "success")]
    Success { pid: u32 },
    #[serde(rename = "failure")]
    Failure { message: String },
}

pub struct StartupChannel {
    pub address: String,
    #[cfg(unix)]
    listener: std::os::unix::net::UnixListener,
    #[cfg(windows)]
    listener: std::net::TcpListener,
}

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

pub fn is_process_running(pid: u32) -> bool {
    let mut system = System::new();
    system.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    system.process(Pid::from_u32(pid)).is_some()
}

pub fn is_port_in_use(port: u16) -> bool {
    TcpStream::connect(format!("127.0.0.1:{}", port)).is_ok()
}

pub fn is_server_running() -> bool {
    if let Some(pid) = get_stored_pid() {
        return is_process_running(pid);
    }
    false
}

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

pub fn get_env_proxy() -> Option<String> {
    std::env::var("HTTPS_PROXY")
        .ok()
        .or_else(|| std::env::var("HTTP_PROXY").ok())
}

pub fn stop_process(pid: u32) {
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
        force_kill_process(pid);
    }

    if let Ok(pid_file) = get_pid_file_path()
        && pid_file.exists()
    {
        fs::remove_file(&pid_file).ok();
    }
}

pub fn force_kill_process(pid: u32) {
    #[cfg(unix)]
    {
        std::process::Command::new("kill")
            .arg("-9")
            .arg(pid.to_string())
            .output()
            .ok();
    }

    #[cfg(windows)]
    {
        std::process::Command::new("taskkill")
            .arg("/F")
            .arg("/PID")
            .arg(pid.to_string())
            .output()
            .ok();
    }
}

pub fn create_startup_channel() -> anyhow::Result<StartupChannel> {
    #[cfg(unix)]
    {
        use crate::config::get_bifrost_dir;
        let socket_path =
            get_bifrost_dir()?.join(format!("bifrost_startup_{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&socket_path);
        let listener = std::os::unix::net::UnixListener::bind(&socket_path)
            .context("Failed to create startup socket")?;
        listener
            .set_nonblocking(true)
            .context("Failed to set non-blocking mode")?;
        Ok(StartupChannel {
            address: socket_path.to_string_lossy().to_string(),
            listener,
        })
    }

    #[cfg(windows)]
    {
        use std::net::TcpListener;

        let listener =
            TcpListener::bind("127.0.0.1:0").context("Failed to bind startup TCP listener")?;
        listener
            .set_nonblocking(true)
            .context("Failed to set non-blocking mode")?;
        let address = listener
            .local_addr()
            .context("Failed to get local address")?
            .to_string();
        Ok(StartupChannel { address, listener })
    }
}

pub fn wait_for_startup_result(channel: &StartupChannel) -> anyhow::Result<ServerStartResult> {
    let deadline = Instant::now() + Duration::from_millis(500);
    let mut buffer = Vec::new();

    loop {
        if Instant::now() >= deadline {
            return Err(anyhow::anyhow!("Timeout waiting for server startup result"));
        }

        #[cfg(unix)]
        let accept_result = channel.listener.accept();
        #[cfg(windows)]
        let accept_result = channel.listener.accept();

        match accept_result {
            Ok((mut stream, _)) => {
                stream.read_to_end(&mut buffer)?;
                return Ok(serde_json::from_slice(&buffer)?);
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => {
                return Err(anyhow::anyhow!("Startup listener accept error: {}", e));
            }
        }
    }
}

/// Result of an extended startup check after initial timeout.
pub enum ExtendedStartupResult {
    /// Server is running (port is accepting connections).
    ServerRunning,
    /// Server failed to start.
    Failure { message: String },
}

/// After the startup channel times out, perform an extended check:
///
/// 1. Immediately try connecting to the server port.
/// 2. If that fails, wait up to 2 seconds while checking process health,
///    the startup channel for failure messages, and the server port.
pub fn extended_startup_check(
    port: u16,
    server_pid: u32,
    channel: &StartupChannel,
) -> ExtendedStartupResult {
    // 1. Immediate port check
    if is_port_in_use(port) {
        return ExtendedStartupResult::ServerRunning;
    }

    // 2. Process already dead?
    if !is_process_running(server_pid) {
        return ExtendedStartupResult::Failure {
            message: "Server process terminated unexpectedly".to_string(),
        };
    }

    // 3. Extended wait loop (up to 2s, checking every 500ms)
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(500));

        if !is_process_running(server_pid) {
            return ExtendedStartupResult::Failure {
                message: "Server process terminated unexpectedly".to_string(),
            };
        }

        // Check if server sent a startup message during the wait
        let channel_msg = accept_startup_channel(channel);
        if let Some(msg) = channel_msg {
            match msg {
                ServerStartResult::Failure { message } => {
                    return ExtendedStartupResult::Failure { message };
                }
                ServerStartResult::Success { .. } => {
                    return ExtendedStartupResult::ServerRunning;
                }
            }
        }

        if is_port_in_use(port) {
            return ExtendedStartupResult::ServerRunning;
        }
    }

    // 4. One final port check after deadline
    if is_port_in_use(port) {
        ExtendedStartupResult::ServerRunning
    } else {
        ExtendedStartupResult::Failure {
            message: "Server failed to start within the expected time".to_string(),
        }
    }
}

/// Try to read a startup result from the channel without blocking indefinitely.
fn accept_startup_channel(channel: &StartupChannel) -> Option<ServerStartResult> {
    #[cfg(unix)]
    let result = channel.listener.accept();
    #[cfg(windows)]
    let result = channel.listener.accept();

    match result {
        Ok((mut stream, _)) => {
            let mut buf = Vec::new();
            #[cfg(unix)]
            stream
                .set_read_timeout(Some(Duration::from_millis(100)))
                .ok();
            #[cfg(windows)]
            stream
                .set_read_timeout(Some(Duration::from_millis(100)))
                .ok();
            stream.read_to_end(&mut buf).ok().and_then(|_| {
                if !buf.is_empty() {
                    serde_json::from_slice(&buf).ok()
                } else {
                    None
                }
            })
        }
        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => None,
        _ => None,
    }
}
