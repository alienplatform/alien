//! Alien Deploy CLI
//!
//! CLI for deploying and managing applications in remote environments.
//! Talks directly to an alien-manager instance.

pub mod commands;
pub mod deployment_tracking;
pub mod error;
pub mod output;

use crate::commands::{
    agent_command, down_command, list_command, status_command, up_command, AgentArgs, DownArgs,
    ListArgs, StatusArgs, UpArgs,
};
use crate::error::Result;
use alien_core::embedded_config::{load_embedded_config, DeployCliConfig};
use clap::{Parser, Subcommand};
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

#[derive(Parser)]
#[command(
    name = "alien-deploy",
    about = "Alien Deploy — deploy and manage applications",
    version
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,

    /// Verbose logging
    #[arg(short, long, global = true)]
    pub verbose: bool,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Deploy the application to a target environment
    Up(UpArgs),
    /// Destroy a deployment and its resources
    Down(DownArgs),
    /// Show deployment status
    Status(StatusArgs),
    /// List tracked deployments
    List(ListArgs),
    /// Manage the alien-agent background service
    Agent(AgentArgs),
}

pub fn setup_tracing(verbose: bool) {
    let filter = if verbose {
        EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new("debug,turso_core=warn,hyper_util=warn"))
    } else {
        EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new("info,turso_core=warn,hyper_util=warn"))
    };

    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_target(false))
        .init();
}

pub async fn run_cli(cli: Cli) -> Result<()> {
    // Load embedded config if present (for white-labeled or pre-configured binaries)
    let embedded_config: Option<DeployCliConfig> = load_embedded_config().ok().flatten();

    setup_tracing(cli.verbose);

    match cli.command {
        Commands::Up(args) => up_command(args, embedded_config.as_ref()).await,
        Commands::Down(args) => down_command(args).await,
        Commands::Status(args) => status_command(args).await,
        Commands::List(args) => list_command(args).await,
        Commands::Agent(args) => agent_command(args).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn test_parse_up_command() {
        let cli = Cli::try_parse_from([
            "alien-deploy",
            "up",
            "--token",
            "dg_abc123",
            "--platform",
            "aws",
            "--manager-url",
            "https://manager.example.com",
        ])
        .unwrap();

        assert!(!cli.verbose);
        assert!(matches!(cli.command, Commands::Up(_)));
    }

    #[test]
    fn test_parse_verbose_flag() {
        let cli = Cli::try_parse_from(["alien-deploy", "-v", "list"]).unwrap();
        assert!(cli.verbose);
        assert!(matches!(cli.command, Commands::List(_)));
    }

    #[test]
    fn test_parse_agent_install() {
        let cli = Cli::try_parse_from([
            "alien-deploy",
            "agent",
            "install",
            "--sync-url",
            "https://manager.example.com",
            "--sync-token",
            "tok_abc",
            "--platform",
            "local",
        ])
        .unwrap();

        assert!(matches!(cli.command, Commands::Agent(_)));
    }

    #[test]
    fn test_parse_agent_status() {
        let cli = Cli::try_parse_from(["alien-deploy", "agent", "status"]).unwrap();
        assert!(matches!(cli.command, Commands::Agent(_)));
    }

    #[test]
    fn test_parse_down_command() {
        let cli = Cli::try_parse_from(["alien-deploy", "down", "--name", "prod"]).unwrap();
        assert!(matches!(cli.command, Commands::Down(_)));
    }
}
