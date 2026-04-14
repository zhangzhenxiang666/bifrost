use anyhow::Result;
use colored::Colorize;

use super::list::StatusResponse;
use super::printing::print_process_table;
use super::start::ServerConfig;
use super::utils::{get_env_proxy, get_process_info, get_stored_pid, is_server_running};
use crate::config::get_config_path;

fn get_status_from_server(port: u16) -> Result<Option<StatusResponse>> {
    let url = format!("http://127.0.0.1:{}/status", port);
    match reqwest::blocking::get(&url) {
        Ok(resp) => Ok(Some(resp.json::<StatusResponse>()?)),
        Err(e) if e.is_connect() => Ok(None),
        Err(e) => Err(anyhow::anyhow!("Failed to connect to server: {}", e)),
    }
}

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
