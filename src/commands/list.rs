use anyhow::Result;
use colored::Colorize;
use serde::Deserialize;
use tabled::{Table, Tabled};

use super::printing::{print_error, print_warning};
use super::utils::is_server_running;

#[derive(Debug, Deserialize)]
pub struct StatusResponse {
    pub proxy: Option<String>,
    pub providers: Vec<ProviderInfo>,
}

#[derive(Debug, Deserialize, Tabled, Clone)]
pub struct ProviderInfo {
    name: String,
    endpoint: String,
}

fn get_status_from_server(port: u16) -> Result<Option<StatusResponse>> {
    let url = format!("http://127.0.0.1:{}/status", port);
    match reqwest::blocking::get(&url) {
        Ok(resp) => Ok(Some(resp.json::<StatusResponse>()?)),
        Err(e) if e.is_connect() => Ok(None),
        Err(e) => Err(anyhow::anyhow!("Failed to connect to server: {}", e)),
    }
}

fn print_providers_table(providers: &[ProviderInfo]) {
    use tabled::settings::Style;

    let mut sorted = providers.to_vec();
    sorted.sort_by(|a, b| a.name.cmp(&b.name));

    let mut table = Table::new(&sorted);
    table.with(Style::sharp());
    println!("{}", table);
}

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
