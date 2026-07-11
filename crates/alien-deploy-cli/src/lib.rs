//! Alien Deploy CLI
//!
//! CLI for deploying and managing applications in remote environments.
//! Talks directly to an alien-manager instance.

pub mod commands;
pub mod deployment_tracking;
pub mod error;
pub mod output;

use crate::commands::{
    down_command, join_command, leave_command, list_command, operator_command, register_command,
    status_command, up_command, DownArgs, JoinArgs, LeaveArgs, ListArgs, OperatorArgs,
    RegisterArgs, StatusArgs, UpArgs,
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
    Deploy(UpArgs),
    /// Destroy a deployment and its resources
    Destroy(DownArgs),
    /// Show deployment status
    Status(StatusArgs),
    /// List tracked deployments
    List(ListArgs),
    /// Manage the alien-operator background service
    Operator(OperatorArgs),
    /// Register an externally-provisioned stack (CloudFormation Outputs,
    /// Terraform, …) with a manager.
    Register(RegisterArgs),
    /// Join this Linux host to a Machines deployment.
    Join(JoinArgs),
    /// Leave a Machines deployment from this host.
    Leave(LeaveArgs),
}

pub fn setup_tracing(verbose: bool) {
    let filter = if verbose {
        EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new("debug,turso_core=warn,hyper_util=warn"))
    } else {
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("error"))
    };

    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_target(false))
        .init();
}

pub async fn run_cli(cli: Cli) -> Result<()> {
    // Load embedded config if present (for pre-configured / rebranded binaries)
    let embedded_config: Option<DeployCliConfig> = load_embedded_config().ok().flatten();

    setup_tracing(cli.verbose);

    match cli.command {
        Commands::Deploy(args) => up_command(args, embedded_config.as_ref()).await,
        Commands::Destroy(args) => down_command(args, embedded_config.as_ref()).await,
        Commands::Status(args) => status_command(args, embedded_config.as_ref()).await,
        Commands::List(args) => list_command(args, embedded_config.as_ref()).await,
        Commands::Operator(args) => operator_command(args).await,
        Commands::Register(args) => register_command(args).await,
        Commands::Join(args) => join_command(args, embedded_config.as_ref()).await,
        Commands::Leave(args) => leave_command(args).await,
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
            "deploy",
            "--token",
            "dg_abc123",
            "--platform",
            "aws",
            "--manager-url",
            "https://manager.example.com",
        ])
        .unwrap();

        assert!(!cli.verbose);
        assert!(matches!(cli.command, Commands::Deploy(_)));
    }

    #[test]
    fn test_parse_up_command_token_file() {
        let cli = Cli::try_parse_from([
            "alien-deploy",
            "deploy",
            "--token-file",
            "/run/alien/token",
            "--platform",
            "local",
        ])
        .unwrap();

        let Commands::Deploy(args) = cli.command else {
            panic!("expected deploy variant");
        };
        assert_eq!(
            args.token_file.as_deref(),
            Some(std::path::Path::new("/run/alien/token"))
        );
    }

    #[test]
    fn test_parse_verbose_flag() {
        let cli = Cli::try_parse_from(["alien-deploy", "-v", "list"]).unwrap();
        assert!(cli.verbose);
        assert!(matches!(cli.command, Commands::List(_)));
    }

    #[test]
    fn test_parse_list_command_token_file() {
        let cli = Cli::try_parse_from([
            "alien-deploy",
            "list",
            "--token-file",
            "/run/alien/token",
            "--platform",
            "machines",
        ])
        .unwrap();

        let Commands::List(args) = cli.command else {
            panic!("expected list variant");
        };
        assert_eq!(
            args.token_file.as_deref(),
            Some(std::path::Path::new("/run/alien/token"))
        );
        assert_eq!(args.platform.as_deref(), Some("machines"));
    }

    #[test]
    fn test_parse_operator_install() {
        let cli = Cli::try_parse_from([
            "alien-deploy",
            "operator",
            "install",
            "--sync-url",
            "https://manager.example.com",
            "--sync-token",
            "ax_dg_abc",
            "--deployment-id",
            "dep_abc",
            "--platform",
            "local",
        ])
        .unwrap();

        assert!(matches!(cli.command, Commands::Operator(_)));
    }

    #[test]
    fn test_parse_operator_status() {
        let cli = Cli::try_parse_from(["alien-deploy", "operator", "status"]).unwrap();
        assert!(matches!(cli.command, Commands::Operator(_)));
    }

    #[test]
    fn test_parse_down_command() {
        let cli = Cli::try_parse_from(["alien-deploy", "destroy", "--name", "prod"]).unwrap();
        assert!(matches!(cli.command, Commands::Destroy(_)));
    }

    #[test]
    fn test_parse_down_command_token_file() {
        let cli = Cli::try_parse_from([
            "alien-deploy",
            "destroy",
            "--name",
            "prod",
            "--token-file",
            "/run/alien/token",
        ])
        .unwrap();

        let Commands::Destroy(args) = cli.command else {
            panic!("expected destroy variant");
        };
        assert_eq!(
            args.token_file.as_deref(),
            Some(std::path::Path::new("/run/alien/token"))
        );
    }

    #[test]
    fn test_parse_status_command_token_file() {
        let cli = Cli::try_parse_from([
            "alien-deploy",
            "status",
            "--name",
            "prod",
            "--token-file",
            "/run/alien/token",
            "--json",
        ])
        .unwrap();

        let Commands::Status(args) = cli.command else {
            panic!("expected status variant");
        };
        assert_eq!(
            args.token_file.as_deref(),
            Some(std::path::Path::new("/run/alien/token"))
        );
        assert!(args.json);
    }

    #[test]
    fn test_parse_register_cloudformation_command() {
        let cli = Cli::try_parse_from([
            "alien-deploy",
            "register",
            "--import",
            "cloudformation",
            "--stack-name",
            "acme-prod",
            "--region",
            "us-east-1",
            "--manager-url",
            "https://manager.example.com",
            "--token",
            "dg_abc",
        ])
        .unwrap();
        let Commands::Register(args) = cli.command else {
            panic!("expected register variant");
        };
        assert_eq!(
            args.import,
            crate::commands::register::ImportKind::Cloudformation
        );
        assert_eq!(args.stack_name.as_deref(), Some("acme-prod"));
        assert_eq!(args.region, "us-east-1");
        assert_eq!(args.manager_url, "https://manager.example.com");
        assert_eq!(args.token, "dg_abc");
    }

    #[test]
    fn test_parse_join_command() {
        let cli = Cli::try_parse_from([
            "alien-deploy",
            "join",
            "--token",
            "jt_secret",
            "--capacity-group",
            "gpu",
            "--zone",
            "rack-1",
            "--bundle-url",
            "https://packages.example.com/machines/manifest.json",
            "--control-plane-url",
            "https://control.example.com",
            "--cluster-id",
            "cluster-123",
            "--dry-run",
        ])
        .unwrap();

        let Commands::Join(args) = cli.command else {
            panic!("expected join variant");
        };
        assert_eq!(args.token.as_deref(), Some("jt_secret"));
        assert_eq!(args.capacity_group, "gpu");
        assert_eq!(args.zone.as_deref(), Some("rack-1"));
        assert_eq!(
            args.control_plane_url.as_deref(),
            Some("https://control.example.com")
        );
        assert_eq!(args.cluster_id.as_deref(), Some("cluster-123"));
        assert!(args.dry_run);
    }

    #[test]
    fn test_parse_leave_command() {
        let cli = Cli::try_parse_from(["alien-deploy", "leave", "--purge", "--dry-run"]).unwrap();

        let Commands::Leave(args) = cli.command else {
            panic!("expected leave variant");
        };
        assert!(args.purge);
        assert!(args.dry_run);
    }
}
