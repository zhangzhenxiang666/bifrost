use anyhow::{Context, Result};
use colored::Colorize;

use super::printing::{print_header, print_process_table, print_success, print_warning};
use super::utils::{get_process_info, get_stored_pid, is_server_running, stop_process};
use crate::commands::start::ServerConfig;
use crate::config::get_config_path;

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

    stop_process(pid);

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
