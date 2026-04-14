use std::fs;

use anyhow::Result;
use colored::Colorize;
use sysinfo::{Pid, System};

use super::config_check::validate_config;
use super::printing::print_error;
use super::start::cmd_start_internal;
use super::utils::{get_stored_pid, is_process_running, is_server_running};

pub fn cmd_restart() -> Result<()> {
    println!("\n{}", "Bifrost - Restart Server".bold().white().on_cyan());

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
                std::process::Command::new("kill")
                    .arg("-9")
                    .arg(pid.to_string())
                    .output()
                    .ok();
            }

            let pid_file = crate::config::get_pid_file_path()?;
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
