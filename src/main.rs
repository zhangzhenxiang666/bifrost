mod commands;
mod config;

use clap::Parser;
use clap::builder::Styles;
use clap::builder::styling::{AnsiColor, Effects};
use commands::Commands;

fn styles() -> Styles {
    Styles::styled()
        .header(AnsiColor::Green.on_default() | Effects::BOLD)
        .usage(AnsiColor::Green.on_default() | Effects::BOLD)
        .literal(AnsiColor::BrightCyan.on_default() | Effects::BOLD)
        .placeholder(AnsiColor::BrightCyan.on_default())
        .error(AnsiColor::BrightRed.on_default() | Effects::BOLD)
        .valid(AnsiColor::BrightCyan.on_default() | Effects::BOLD)
        .invalid(AnsiColor::BrightYellow.on_default() | Effects::BOLD)
}

#[derive(Parser)]
#[command(
    name = "bifrost",
    author,
    version,
    about = "Bifrost - A mapping service for LLM providers",
    long_about = None,
    propagate_version = true,
    styles = styles()
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    commands::handle_command(cli.command)
}
