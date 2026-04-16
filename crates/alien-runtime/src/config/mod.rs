//! Configuration for alien-runtime.
//!
//! Configuration can be built from CLI arguments or programmatically via the builder.

mod cli;
pub use cli::{Cli, LambdaMode, TransportType};

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use bon::Builder;
use serde::{Deserialize, Serialize};

use crate::error::ErrorData;
use crate::Result;
use alien_error::AlienError;

/// A log line from the application subprocess.
#[derive(Debug, Clone)]
pub struct AppLogLine {
    /// The log line content (without trailing newline)
    pub line: String,
    /// Whether this is from stdout (true) or stderr (false)  
    pub is_stdout: bool,
}

/// How captured application logs should be exported.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum LogExporter {
    /// No exporting - print to stdout/stderr (for Containers - orchestrator captures)
    None,

    /// Send via OTLP (for Functions - alien-runtime is the capture boundary)
    #[serde(rename_all = "camelCase")]
    Otlp {
        /// OTLP endpoint URL (e.g., "http://localhost:9090/v1/logs")
        endpoint: String,
        /// HTTP headers for authentication
        #[serde(default)]
        headers: HashMap<String, String>,
        /// Service name for resource identification
        service_name: String,
    },
}

/// Runtime configuration.
///
/// Can be constructed from CLI arguments via `from_cli()` or programmatically via the builder.
#[derive(Clone, Builder)]
#[builder(on(String, into), on(PathBuf, into))]
pub struct RuntimeConfig {
    /// Transport type (defaults to passthrough for programmatic use)
    #[builder(default = TransportType::Passthrough)]
    pub transport: TransportType,
    /// Port for CloudRun/ContainerApp transports
    #[builder(default = 8080)]
    pub transport_port: u16,
    /// Lambda mode (only used when transport is Lambda)
    #[builder(default = LambdaMode::Buffered)]
    pub lambda_mode: LambdaMode,
    /// Application command to run
    pub command: Vec<String>,
    /// Working directory for the application (defaults to current dir)
    pub working_dir: Option<PathBuf>,
    /// Environment variables to pass to the application
    #[builder(default)]
    pub env_vars: HashMap<String, String>,
    /// gRPC bindings address
    #[builder(default = "127.0.0.1:51351".to_string())]
    pub bindings_address: String,
    /// Commands polling configuration
    pub commands_polling: Option<CommandsPollingConfig>,
    /// How to export captured application logs
    #[builder(default = LogExporter::None)]
    pub log_exporter: LogExporter,
}

impl std::fmt::Debug for RuntimeConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RuntimeConfig")
            .field("transport", &self.transport)
            .field("transport_port", &self.transport_port)
            .field("lambda_mode", &self.lambda_mode)
            .field("command", &self.command)
            .field("working_dir", &self.working_dir)
            .field("env_vars", &self.env_vars)
            .field("bindings_address", &self.bindings_address)
            .field("commands_polling", &self.commands_polling)
            .field("log_exporter", &self.log_exporter)
            .finish()
    }
}

/// Commands polling configuration
#[derive(Debug, Clone)]
pub struct CommandsPollingConfig {
    /// Polling URL
    pub url: String,
    /// Polling interval
    pub interval: Duration,
    /// Deployment ID - required for commands polling to work correctly
    /// Should come from ALIEN_DEPLOYMENT_ID environment variable
    pub deployment_id: String,
    /// Authentication token for commands server
    /// Should come from ALIEN_COMMANDS_TOKEN environment variable
    pub token: String,
}

impl RuntimeConfig {
    /// Create configuration from CLI arguments
    pub fn from_cli() -> Result<Self> {
        let cli = Cli::parse_args();
        Self::from_cli_struct(cli)
    }

    /// Create configuration from a Cli struct (for testing)
    pub fn from_cli_struct(cli: Cli) -> Result<Self> {
        cli.validate()?;

        // Don't build programmatic commands config from CLI - let CommandsPolling::from_env() handle it
        // Token may come from vault secrets (loaded after config), not just env vars
        let commands_polling = None;

        // When running from CLI (standalone binary), build LogExporter from environment
        // Function controllers (AWS/GCP/Azure/Kubernetes) set OTEL_* env vars
        let log_exporter = if let Some(endpoint) = std::env::var("OTEL_EXPORTER_OTLP_LOGS_ENDPOINT")
            .or_else(|_| std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT"))
            .ok()
        {
            // Parse headers from environment
            let mut headers = HashMap::new();
            if let Ok(headers_str) = std::env::var("OTEL_EXPORTER_OTLP_HEADERS") {
                for header in headers_str.split(',') {
                    if let Some((key, value)) = header.split_once('=') {
                        headers.insert(key.trim().to_lowercase(), value.trim().to_string());
                    }
                }
            }

            let service_name =
                std::env::var("OTEL_SERVICE_NAME").unwrap_or_else(|_| "alien-runtime".to_string());

            LogExporter::Otlp {
                endpoint,
                headers,
                service_name,
            }
        } else {
            // No OTLP config = container mode (orchestrator captures)
            LogExporter::None
        };

        // Populate env_vars from process environment for standalone binary
        // This allows CommandsPolling::from_env() to read ALIEN_COMMANDS_POLLING_* vars
        let env_vars: HashMap<String, String> = std::env::vars().collect();

        Ok(Self {
            transport: cli.transport,
            transport_port: match cli.transport {
                TransportType::CloudRun => cli.cloudrun_port,
                TransportType::ContainerApp => cli.containerapp_port,
                TransportType::Local => cli.local_port,
                _ => 8080,
            },
            lambda_mode: cli.lambda_mode,
            command: cli.command,
            working_dir: None,
            env_vars,
            bindings_address: cli.bindings_address,
            commands_polling,
            log_exporter,
        })
    }

    /// Validate the configuration
    pub fn validate(&self) -> Result<()> {
        if self.command.is_empty() {
            return Err(AlienError::new(ErrorData::ConfigurationInvalid {
                message: "Application command is required".to_string(),
                field: Some("command".to_string()),
            }));
        }

        if let Some(ref commands) = self.commands_polling {
            if commands.url.is_empty() {
                return Err(AlienError::new(ErrorData::ConfigurationInvalid {
                    message: "Commands polling URL is required when polling is enabled".to_string(),
                    field: Some("commands_polling.url".to_string()),
                }));
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_from_cli() {
        let cli = Cli::try_parse_from(["alien-runtime", "--", "bun", "index.ts"]).unwrap();
        let config = RuntimeConfig::from_cli_struct(cli).unwrap();

        assert_eq!(config.transport, TransportType::Lambda);
        assert_eq!(config.command, vec!["bun", "index.ts"]);
    }

    #[test]
    fn test_config_builder() {
        let config = RuntimeConfig::builder()
            .transport(TransportType::CloudRun)
            .transport_port(9000)
            .command(vec!["./app".to_string()])
            .working_dir(PathBuf::from("/app"))
            .env_vars(HashMap::from([("MY_VAR".to_string(), "value".to_string())]))
            .build();

        assert_eq!(config.transport, TransportType::CloudRun);
        assert_eq!(config.transport_port, 9000);
        assert_eq!(config.command, vec!["./app"]);
        assert_eq!(config.working_dir, Some(PathBuf::from("/app")));
        assert_eq!(config.env_vars.get("MY_VAR"), Some(&"value".to_string()));
    }
}
