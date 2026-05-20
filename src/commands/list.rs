use anyhow::Result;
use colored::Colorize;
use serde::Deserialize;
use tabled::{Table, Tabled};

use super::printing::{print_error, print_warning};
use super::utils::is_server_running;

#[derive(Debug, Deserialize)]
pub struct StatusResponse {
    #[serde(default)]
    pub version: Option<String>,
    pub proxy: Option<String>,
    pub providers: Vec<ProviderInfo>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ProviderInfo {
    pub name: String,
    pub endpoint: String,
    #[serde(default)]
    pub deployments: Vec<DeploymentInfo>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct DeploymentInfo {
    pub id: String,
    pub enabled: bool,
    #[serde(default)]
    pub implicit: bool,
    pub weight: u32,
    pub automatic: bool,
    pub state: String,
    pub consecutive_failures: u32,
    pub cooldown_remaining_ms: Option<u64>,
}

#[derive(Tabled)]
struct ProviderRow {
    name: String,
    endpoint: String,
    automatic: String,
    manual: String,
    cooling: String,
    disabled: String,
}

fn get_status_from_server(port: u16) -> Result<Option<StatusResponse>> {
    let url = format!("http://127.0.0.1:{}/status", port);
    match reqwest::blocking::get(&url) {
        Ok(resp) => Ok(Some(resp.json::<StatusResponse>()?)),
        Err(e) if e.is_connect() => Ok(None),
        Err(e) => Err(anyhow::anyhow!("Failed to connect to server: {}", e)),
    }
}

fn format_deployment_ids<'a>(deployments: impl Iterator<Item = &'a DeploymentInfo>) -> String {
    let values: Vec<String> = deployments
        .map(|deployment| {
            let implicit = if deployment.implicit { "*" } else { "" };
            if deployment.automatic {
                format!("{}{}({})", deployment.id, implicit, deployment.weight)
            } else {
                format!("{}{}", deployment.id, implicit)
            }
        })
        .collect();
    if values.is_empty() {
        "-".to_string()
    } else {
        values.join(", ")
    }
}

fn format_cooling(deployments: &[DeploymentInfo]) -> String {
    let values: Vec<String> = deployments
        .iter()
        .filter(|deployment| deployment.state == "cooling")
        .map(|deployment| {
            let seconds = deployment.cooldown_remaining_ms.unwrap_or_default() / 1000;
            format!(
                "{}({}s, fails={})",
                deployment.id, seconds, deployment.consecutive_failures
            )
        })
        .collect();
    if values.is_empty() {
        "-".to_string()
    } else {
        values.join(", ")
    }
}

pub(crate) fn print_providers_table(providers: &[ProviderInfo]) {
    use tabled::settings::Style;

    let mut sorted: Vec<ProviderRow> = providers
        .iter()
        .map(|provider| ProviderRow {
            name: provider.name.clone(),
            endpoint: provider.endpoint.clone(),
            automatic: format_deployment_ids(
                provider
                    .deployments
                    .iter()
                    .filter(|deployment| deployment.enabled && deployment.automatic),
            ),
            manual: format_deployment_ids(provider.deployments.iter().filter(|deployment| {
                deployment.enabled && !deployment.automatic && deployment.state != "disabled"
            })),
            cooling: format_cooling(&provider.deployments),
            disabled: format_deployment_ids(
                provider
                    .deployments
                    .iter()
                    .filter(|deployment| !deployment.enabled || deployment.state == "disabled"),
            ),
        })
        .collect();
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
