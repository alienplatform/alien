//! CLI argument parsing for alien-worker-runtime.

use clap::{Parser, ValueEnum};

use crate::error::{ErrorData, Result};
use alien_error::AlienError;

/// Alien Runtime - runs applications on any platform.
#[derive(Parser, Debug)]
#[command(name = "alien-worker-runtime")]
#[command(version, about, long_about = None)]
pub struct Cli {
    /// Application command to run
    #[arg(last = true, required = true)]
    pub command: Vec<String>,

    /// Transport type
    #[arg(short, long, env = "ALIEN_TRANSPORT", default_value = "lambda")]
    pub transport: TransportType,

    /// Worker app protocol gRPC server address (Control + WaitUntil)
    #[arg(
        long,
        env = "ALIEN_WORKER_GRPC_ADDRESS",
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

    // Local/HTTP proxy options
    /// Port for Local/HTTP proxy transport
    #[arg(long, env = "PORT", default_value_t = 8080)]
    pub local_port: u16,

    /// Maximum time a pushed command may execute in the Worker application.
    #[arg(long, env = "ALIEN_WORKER_TIMEOUT_SECONDS", default_value_t = 300)]
    pub worker_timeout_seconds: u64,
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

        if !(1..=u64::from(alien_core::MAX_WORKER_TIMEOUT_SECONDS))
            .contains(&self.worker_timeout_seconds)
        {
            return Err(AlienError::new(ErrorData::ConfigurationInvalid {
                message: format!(
                    "Worker timeout must be between 1 and {} seconds",
                    alien_core::MAX_WORKER_TIMEOUT_SECONDS
                ),
                field: Some("worker_timeout_seconds".to_string()),
            }));
        }

        Ok(())
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
    /// Plain HTTP proxy, exposed on all interfaces
    Http,
    /// Local Platform - simple HTTP proxy (for VMs, edge, bare metal)
    Local,
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
        let cli = Cli::try_parse_from(["alien-worker-runtime", "--", "bun", "index.ts"]).unwrap();
        assert_eq!(cli.command, vec!["bun", "index.ts"]);
        assert_eq!(cli.transport, TransportType::Lambda);
        assert_eq!(cli.worker_timeout_seconds, 300);
    }

    #[test]
    fn test_parse_cloudrun() {
        let cli = Cli::try_parse_from([
            "alien-worker-runtime",
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
    fn test_parse_http_transport() {
        let cli = Cli::try_parse_from(["alien-worker-runtime", "--transport", "http", "--", "app"])
            .unwrap();

        assert_eq!(cli.transport, TransportType::Http);
        assert_eq!(cli.local_port, 8080);
    }

    #[test]
    fn test_parse_rejects_retired_passthrough_transport() {
        let error = Cli::try_parse_from([
            "alien-worker-runtime",
            "--transport",
            "passthrough",
            "--",
            "app",
        ])
        .expect_err("passthrough is not a Worker transport");

        let message = error.to_string();
        assert!(message.contains("invalid value 'passthrough'"));
        for transport in ["lambda", "cloud-run", "container-app", "http", "local"] {
            assert!(
                message.contains(transport),
                "error should list the supported Worker transport {transport}: {message}"
            );
        }
    }

    #[test]
    fn test_parse_worker_timeout() {
        let cli = Cli::try_parse_from([
            "alien-worker-runtime",
            "--worker-timeout-seconds",
            "3600",
            "--",
            "app",
        ])
        .unwrap();

        assert_eq!(cli.worker_timeout_seconds, 3600);
        cli.validate().unwrap();
    }

    #[test]
    fn test_validate_rejects_zero_worker_timeout() {
        let cli = Cli::try_parse_from([
            "alien-worker-runtime",
            "--worker-timeout-seconds",
            "0",
            "--",
            "app",
        ])
        .unwrap();

        let error = cli.validate().unwrap_err();
        assert!(error.to_string().contains("between 1 and 3600"));
    }

    #[test]
    fn test_validate_rejects_worker_timeout_above_one_hour() {
        let cli = Cli::try_parse_from([
            "alien-worker-runtime",
            "--worker-timeout-seconds",
            "3601",
            "--",
            "app",
        ])
        .unwrap();

        let error = cli.validate().unwrap_err();
        assert!(error.to_string().contains("between 1 and 3600"));
    }
}
