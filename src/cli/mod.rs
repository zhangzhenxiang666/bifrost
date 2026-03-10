//! CLI module for bifrost service management

pub mod commands;
pub mod config;

use clap::{Parser, Subcommand};
use colored::Colorize;

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

/// Print a formatted info message
pub fn print_info(label: &str, value: &str) {
    println!("  {:<18} {}", label.bold().cyan(), value);
}

/// Print a formatted success message
pub fn print_success(message: &str) {
    println!("  {} {}", "✓".green().bold(), message);
}

/// Print a formatted error message
pub fn print_error(message: &str) {
    eprintln!("  {} {}", "✗".red().bold(), message.red());
}

/// Print a formatted warning message
pub fn print_warning(message: &str) {
    println!("  {} {}", "⚠".yellow().bold(), message.yellow());
}

/// Print a section header
pub fn print_header(title: &str) {
    println!("\n{}", title.bold().purple().underline());
}
