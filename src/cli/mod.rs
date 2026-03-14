//! CLI module for bifrost service management

pub mod commands;
pub mod config;

use clap::{Parser, Subcommand};
use colored::Colorize;
use tabled::{
    Table,
    settings::{
        Alignment, Remove, Style,
        object::{Columns, Rows},
    },
};

/// LLM Map - A mapping service for LLM providers
#[derive(Parser)]
#[command(name = "bifrost")]
#[command(author, version, about, long_about = None)]
#[command(propagate_version = true)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
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
    // Create table with modern style, remove auto-generated header
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
