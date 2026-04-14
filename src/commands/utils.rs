use crate::config::get_pid_file_path;
use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Read;
use std::net::TcpStream;
use std::path::PathBuf;
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
        std::process::Command::new("kill")
            .arg("-9")
            .arg(pid.to_string())
            .output()
            .ok();
    }

    if let Ok(pid_file) = get_pid_file_path()
        && pid_file.exists()
    {
        fs::remove_file(&pid_file).ok();
    }
}

pub fn create_startup_socket() -> anyhow::Result<(std::os::unix::net::UnixListener, PathBuf)> {
    use crate::config::get_bifrost_dir;
    let socket_path =
        get_bifrost_dir()?.join(format!("bifrost_startup_{}.sock", std::process::id()));

    let _ = std::fs::remove_file(&socket_path);

    let listener = std::os::unix::net::UnixListener::bind(&socket_path)
        .context("Failed to create startup socket")?;

    listener
        .set_nonblocking(true)
        .context("Failed to set non-blocking mode")?;

    Ok((listener, socket_path))
}

pub fn wait_for_startup_result(
    listener: &std::os::unix::net::UnixListener,
) -> anyhow::Result<ServerStartResult> {
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
