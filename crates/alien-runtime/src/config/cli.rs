//! CLI argument parsing for alien-runtime.

use clap::{Parser, ValueEnum};
use std::time::Duration;

use crate::error::{ErrorData, Result};
use alien_error::AlienError;

/// Alien Runtime - runs applications on any platform.
#[derive(Parser, Debug)]
#[command(name = "alien-runtime")]
#[command(version, about, long_about = None)]
pub struct Cli {
    /// Application command to run
    #[arg(last = true, required = true)]
    pub command: Vec<String>,

    /// Transport type
    #[arg(short, long, env = "ALIEN_TRANSPORT", default_value = "lambda")]
    pub transport: TransportType,

    /// gRPC bindings address
    #[arg(
        long,
        env = "ALIEN_BINDINGS_ADDRESS",
        default_value = "127.0.0.1:51351"
    )]
    pub bindings_address: String,

    // Lambda options
    /// Lambda mode (buffered or streaming)
    #[arg(long, env = "ALIEN_LAMBDA_MODE", default_value = "buffered")]
    pub lambda_mode: LambdaMode,

    // CloudRun options
    /// Port for Cloud Run transport
    #[arg(long, env = "PORT", default_value_t = 8080)]
    pub cloudrun_port: u16,

    // ContainerApp options
    /// Port for Container App transport
    #[arg(long, env = "PORT", default_value_t = 8080)]
    pub containerapp_port: u16,

    // Local options
    /// Port for Local transport
    #[arg(long, env = "PORT", default_value_t = 8080)]
    pub local_port: u16,

    // Commands Polling options
    /// Enable commands polling
    #[arg(long, env = "ALIEN_COMMANDS_POLLING_ENABLED")]
    pub commands_polling_enabled: bool,

    /// Commands polling URL
    #[arg(long, env = "ALIEN_COMMANDS_POLLING_URL")]
    pub commands_polling_url: Option<String>,

    /// Commands polling interval in seconds
    #[arg(
        long,
        env = "ALIEN_COMMANDS_POLLING_INTERVAL_SECS",
        default_value_t = 5
    )]
    pub commands_polling_interval_secs: u64,

    /// Deployment ID for commands polling (required when commands polling is enabled)
    #[arg(long, env = "ALIEN_DEPLOYMENT_ID")]
    pub deployment_id: Option<String>,

    /// Commands polling authentication token
    #[arg(long, env = "ALIEN_COMMANDS_TOKEN")]
    pub commands_token: Option<String>,
}

impl Cli {
    /// Parse from environment and command line
    pub fn parse_args() -> Self {
        Cli::parse()
    }

    /// Try to parse from iterator (for testing)
    pub fn try_parse_from<I, T>(iter: I) -> std::result::Result<Self, clap::Error>
    where
        I: IntoIterator<Item = T>,
        T: Into<std::ffi::OsString> + Clone,
    {
        <Self as Parser>::try_parse_from(iter)
    }

    /// Validate configuration
    pub fn validate(&self) -> Result<()> {
        if self.command.is_empty() {
            return Err(AlienError::new(ErrorData::ConfigurationInvalid {
                message: "Application command is required".to_string(),
                field: Some("command".to_string()),
            }));
        }

        if self.commands_polling_enabled {
            if self.commands_polling_url.is_none() {
                return Err(AlienError::new(ErrorData::ConfigurationInvalid {
                    message: "Commands polling URL required when polling is enabled".to_string(),
                    field: Some("ALIEN_COMMANDS_POLLING_URL".to_string()),
                }));
            }
            if self.deployment_id.is_none() {
                return Err(AlienError::new(ErrorData::ConfigurationInvalid {
                    message: "Deployment ID required when commands polling is enabled".to_string(),
                    field: Some("ALIEN_DEPLOYMENT_ID".to_string()),
                }));
            }
            // Note: commands_token validation deferred to CommandsPolling::from_env()
            // Token can come from env var OR vault secrets (loaded after config)
        }

        Ok(())
    }

    /// Get commands polling interval as Duration
    pub fn commands_polling_interval_duration(&self) -> Duration {
        Duration::from_secs(self.commands_polling_interval_secs)
    }
}

/// Transport types
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum TransportType {
    /// AWS Lambda
    Lambda,
    /// Google Cloud Run
    CloudRun,
    /// Azure Container Apps
    ContainerApp,
    /// Local Platform - simple HTTP proxy (for VMs, edge, bare metal)
    Local,
    /// Passthrough - app handles HTTP directly
    Passthrough,
}

impl Default for TransportType {
    fn default() -> Self {
        Self::Lambda
    }
}

/// Lambda execution mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum LambdaMode {
    /// Buffer request/response
    Buffered,
    /// Stream request/response
    Streaming,
}

impl Default for LambdaMode {
    fn default() -> Self {
        Self::Buffered
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_basic() {
        let cli = Cli::try_parse_from(["alien-runtime", "--", "bun", "index.ts"]).unwrap();
        assert_eq!(cli.command, vec!["bun", "index.ts"]);
        assert_eq!(cli.transport, TransportType::Lambda);
    }

    #[test]
    fn test_parse_cloudrun() {
        let cli = Cli::try_parse_from([
            "alien-runtime",
            "--transport",
            "cloud-run",
            "--cloudrun-port",
            "9000",
            "--",
            "node",
            "server.js",
        ])
        .unwrap();
        assert_eq!(cli.transport, TransportType::CloudRun);
        assert_eq!(cli.cloudrun_port, 9000);
    }

    #[test]
    fn test_validate_commands_polling() {
        // Missing URL and deployment_id
        let cli = Cli::try_parse_from(["alien-runtime", "--commands-polling-enabled", "--", "app"])
            .unwrap();
        assert!(cli.validate().is_err());

        // Missing deployment_id
        let cli = Cli::try_parse_from([
            "alien-runtime",
            "--commands-polling-enabled",
            "--commands-polling-url",
            "http://example.com",
            "--",
            "app",
        ])
        .unwrap();
        let err = cli.validate().expect_err("should require deployment_id");
        assert_eq!(err.code, "CONFIGURATION_INVALID");
        assert!(err.message.contains("Deployment ID"));

        // Required fields present (token not required at config time)
        let cli = Cli::try_parse_from([
            "alien-runtime",
            "--commands-polling-enabled",
            "--commands-polling-url",
            "http://example.com",
            "--deployment-id",
            "test-agent-123",
            "--",
            "app",
        ])
        .unwrap();
        assert!(
            cli.validate().is_ok(),
            "Token validation deferred to runtime"
        );
    }
}
