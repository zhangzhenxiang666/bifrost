//! CLI command module - command definitions and dispatching

mod config_check;
mod list;
mod printing;
mod restart;
mod start;
mod status;
mod stop;
mod upgrade;
pub mod utils;

use clap::Subcommand;

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

    /// List all providers from the running server
    #[command(arg_required_else_help = false)]
    List,

    /// Upgrade bifrost and bifrost-server to the latest version
    #[command(arg_required_else_help = false)]
    Upgrade,
}

/// Handle command dispatch from main.rs
pub fn handle_command(command: Commands) -> anyhow::Result<()> {
    match command {
        Commands::Start => start::cmd_start(),
        Commands::Stop => stop::cmd_stop(),
        Commands::Restart => restart::cmd_restart(),
        Commands::Status => status::cmd_status(),
        Commands::List => list::cmd_list(),
        Commands::Upgrade => upgrade::cmd_upgrade(),
    }
}
