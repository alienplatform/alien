//! Configuration for alien-worker-runtime.
//!
//! Configuration can be built from CLI arguments or programmatically via the builder.

mod cli;
pub use cli::{Cli, LambdaMode, TransportType};

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use bon::Builder;
use serde::{Deserialize, Serialize};

use crate::{error::ErrorData, otlp, Result};
use alien_error::AlienError;

const ENV_ALIEN_RUNTIME_SEND_OTLP: &str = "ALIEN_RUNTIME_SEND_OTLP";
const DEFAULT_COMMAND_TIMEOUT_SECONDS: u64 = 300;

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

    /// Send via OTLP (for Functions - alien-worker-runtime is the capture boundary)
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
    /// Worker transport type.
    ///
    /// Programmatic callers must select this explicitly so a Worker cannot
    /// accidentally start with a workload type that has no invocation proxy.
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
    /// Worker app protocol gRPC server address.
    #[builder(default = "127.0.0.1:51351".to_string())]
    pub worker_grpc_address: String,
    /// How to export captured application logs
    #[builder(default = LogExporter::None)]
    pub log_exporter: LogExporter,
    /// Maximum time to wait for a pushed command to finish in the application.
    #[builder(default = Duration::from_secs(DEFAULT_COMMAND_TIMEOUT_SECONDS))]
    pub command_timeout: Duration,
}

impl std::fmt::Debug for RuntimeConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut env_var_keys = self.env_vars.keys().collect::<Vec<_>>();
        env_var_keys.sort_unstable();

        f.debug_struct("RuntimeConfig")
            .field("transport", &self.transport)
            .field("transport_port", &self.transport_port)
            .field("lambda_mode", &self.lambda_mode)
            .field("command", &self.command)
            .field("working_dir", &self.working_dir)
            .field("env_var_count", &env_var_keys.len())
            .field("env_var_keys", &env_var_keys)
            .field("worker_grpc_address", &self.worker_grpc_address)
            .field("log_exporter", &self.log_exporter)
            .field("command_timeout", &self.command_timeout)
            .finish()
    }
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

        // Populate env_vars from process environment for standalone binary
        let env_vars: HashMap<String, String> = std::env::vars().collect();
        let log_exporter = LogExporter::from_env_vars(&env_vars);

        Ok(Self {
            transport: cli.transport,
            transport_port: match cli.transport {
                TransportType::CloudRun => cli.cloudrun_port,
                TransportType::ContainerApp => cli.containerapp_port,
                TransportType::Http | TransportType::Local => cli.local_port,
                _ => 8080,
            },
            lambda_mode: cli.lambda_mode,
            command: cli.command,
            working_dir: None,
            env_vars,
            worker_grpc_address: cli.worker_grpc_address,
            log_exporter,
            command_timeout: Duration::from_secs(cli.worker_timeout_seconds),
        })
    }

    /// Read the controller-injected Worker timeout used by embedded runtimes.
    /// Missing values retain compatibility with metadata created before this
    /// runtime setting existed.
    pub fn command_timeout_from_env_vars(env_vars: &HashMap<String, String>) -> Result<Duration> {
        let Some(value) = env_vars.get(alien_core::ENV_ALIEN_WORKER_TIMEOUT_SECONDS) else {
            return Ok(Duration::from_secs(DEFAULT_COMMAND_TIMEOUT_SECONDS));
        };

        let timeout_seconds = value.parse::<u64>().map_err(|error| {
            AlienError::new(ErrorData::ConfigurationInvalid {
                message: format!("Worker timeout must be an integer number of seconds: {error}"),
                field: Some(alien_core::ENV_ALIEN_WORKER_TIMEOUT_SECONDS.to_string()),
            })
        })?;
        if !(1..=u64::from(alien_core::MAX_WORKER_TIMEOUT_SECONDS)).contains(&timeout_seconds) {
            return Err(AlienError::new(ErrorData::ConfigurationInvalid {
                message: format!(
                    "Worker timeout must be between 1 and {} seconds",
                    alien_core::MAX_WORKER_TIMEOUT_SECONDS
                ),
                field: Some(alien_core::ENV_ALIEN_WORKER_TIMEOUT_SECONDS.to_string()),
            }));
        }

        Ok(Duration::from_secs(timeout_seconds))
    }

    /// Validate the configuration
    pub fn validate(&self) -> Result<()> {
        if self.command.is_empty() {
            return Err(AlienError::new(ErrorData::ConfigurationInvalid {
                message: "Application command is required".to_string(),
                field: Some("command".to_string()),
            }));
        }

        if self.command_timeout.is_zero()
            || self.command_timeout
                > Duration::from_secs(u64::from(alien_core::MAX_WORKER_TIMEOUT_SECONDS))
        {
            return Err(AlienError::new(ErrorData::ConfigurationInvalid {
                message: format!(
                    "Worker timeout must be between 1 and {} seconds",
                    alien_core::MAX_WORKER_TIMEOUT_SECONDS
                ),
                field: Some("command_timeout".to_string()),
            }));
        }

        Ok(())
    }
}

impl LogExporter {
    pub fn from_env_vars(env_vars: &HashMap<String, String>) -> Self {
        if runtime_otlp_disabled(env_vars) {
            return LogExporter::None;
        }

        let Some(endpoint) = env_vars
            .get("OTEL_EXPORTER_OTLP_LOGS_ENDPOINT")
            .or_else(|| env_vars.get("OTEL_EXPORTER_OTLP_ENDPOINT"))
            .cloned()
        else {
            return LogExporter::None;
        };

        let service_name = env_vars
            .get("OTEL_SERVICE_NAME")
            .cloned()
            .unwrap_or_else(|| "alien-worker-runtime".to_string());

        LogExporter::Otlp {
            endpoint,
            headers: otlp_headers_from_env_vars(env_vars),
            service_name,
        }
    }

    pub fn with_runtime_secrets(self, runtime_secrets: &HashMap<String, String>) -> Self {
        match self {
            LogExporter::Otlp {
                endpoint,
                mut headers,
                service_name,
            } => {
                headers.extend(otlp_headers_from_env_vars(runtime_secrets));
                LogExporter::Otlp {
                    endpoint,
                    headers,
                    service_name,
                }
            }
            LogExporter::None => LogExporter::None,
        }
    }

    pub fn to_otlp_config(&self) -> Option<otlp::OtlpConfig> {
        match self {
            LogExporter::None => None,
            LogExporter::Otlp {
                endpoint,
                headers,
                service_name,
            } => Some(otlp::OtlpConfig {
                endpoint: endpoint.clone(),
                headers: headers.clone(),
                service_name: service_name.clone(),
                service_version: std::env::var("OTEL_SERVICE_VERSION")
                    .unwrap_or_else(|_| env!("CARGO_PKG_VERSION").to_string()),
            }),
        }
    }
}

fn runtime_otlp_disabled(env_vars: &HashMap<String, String>) -> bool {
    env_vars
        .get(ENV_ALIEN_RUNTIME_SEND_OTLP)
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "false" | "0" | "off"
            )
        })
        .unwrap_or(false)
}

fn otlp_headers_from_env_vars(env_vars: &HashMap<String, String>) -> HashMap<String, String> {
    let mut headers = HashMap::new();

    if let Some(auth_header) = env_vars.get("OTEL_EXPORTER_OTLP_HEADERS_AUTHORIZATION") {
        headers.insert("authorization".to_string(), auth_header.clone());
    }

    if let Some(headers_str) = env_vars.get("OTEL_EXPORTER_OTLP_HEADERS") {
        for header in headers_str.split(',') {
            if let Some((key, value)) = header.split_once('=') {
                headers.insert(key.trim().to_lowercase(), value.trim().to_string());
            }
        }
    }

    headers
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clear_otlp_env_vars() {
        std::env::remove_var("OTEL_EXPORTER_OTLP_LOGS_ENDPOINT");
        std::env::remove_var("OTEL_EXPORTER_OTLP_ENDPOINT");
        std::env::remove_var("OTEL_EXPORTER_OTLP_HEADERS");
        std::env::remove_var("OTEL_EXPORTER_OTLP_HEADERS_AUTHORIZATION");
        std::env::remove_var("OTEL_SERVICE_NAME");
        std::env::remove_var(ENV_ALIEN_RUNTIME_SEND_OTLP);
    }

    #[test]
    fn test_config_from_cli() {
        clear_otlp_env_vars();

        let cli = Cli::try_parse_from(["alien-worker-runtime", "--", "bun", "index.ts"]).unwrap();
        let config = RuntimeConfig::from_cli_struct(cli).unwrap();

        assert_eq!(config.transport, TransportType::Lambda);
        assert_eq!(config.command, vec!["bun", "index.ts"]);
        assert_eq!(config.command_timeout, Duration::from_secs(300));
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
        assert_eq!(config.command_timeout, Duration::from_secs(300));
    }

    #[test]
    fn command_timeout_reads_controller_environment() {
        let env_vars = HashMap::from([(
            alien_core::ENV_ALIEN_WORKER_TIMEOUT_SECONDS.to_string(),
            "3600".to_string(),
        )]);

        assert_eq!(
            RuntimeConfig::command_timeout_from_env_vars(&env_vars).unwrap(),
            Duration::from_secs(3600)
        );
    }

    #[test]
    fn command_timeout_rejects_invalid_controller_environment() {
        for value in ["0", "3601", "not-a-number"] {
            let env_vars = HashMap::from([(
                alien_core::ENV_ALIEN_WORKER_TIMEOUT_SECONDS.to_string(),
                value.to_string(),
            )]);

            assert!(RuntimeConfig::command_timeout_from_env_vars(&env_vars).is_err());
        }
    }

    #[test]
    fn programmatic_config_rejects_worker_timeout_above_one_hour() {
        let config = RuntimeConfig::builder()
            .transport(TransportType::Local)
            .command(vec!["app".to_string()])
            .command_timeout(Duration::from_secs(3601))
            .build();

        let error = config.validate().expect_err("timeout above one hour");
        assert!(error.to_string().contains("between 1 and 3600"));
    }

    #[test]
    fn runtime_config_debug_never_prints_environment_values() {
        let config = RuntimeConfig::builder()
            .transport(TransportType::Local)
            .command(vec!["./app".to_string()])
            .working_dir(PathBuf::from("/app"))
            .env_vars(HashMap::from([
                (
                    "ALIEN_COMMANDS_TOKEN".to_string(),
                    "commands-token-value".to_string(),
                ),
                (
                    "OTEL_EXPORTER_OTLP_HEADERS".to_string(),
                    "authorization=Bearer otlp-secret".to_string(),
                ),
                ("APP_SECRET".to_string(), "app-secret-value".to_string()),
            ]))
            .build();

        let debug = format!("{config:?}");

        assert!(debug.contains("env_var_count: 3"));
        assert!(debug.contains("ALIEN_COMMANDS_TOKEN"));
        assert!(debug.contains("OTEL_EXPORTER_OTLP_HEADERS"));
        assert!(debug.contains("APP_SECRET"));
        for secret_value in [
            "commands-token-value",
            "authorization=Bearer otlp-secret",
            "app-secret-value",
        ] {
            assert!(!debug.contains(secret_value));
        }
    }

    #[test]
    fn test_log_exporter_respects_runtime_otlp_disable_flag() {
        clear_otlp_env_vars();
        std::env::set_var(
            "OTEL_EXPORTER_OTLP_LOGS_ENDPOINT",
            "https://example.com/v1/logs",
        );
        std::env::set_var(ENV_ALIEN_RUNTIME_SEND_OTLP, "false");

        let cli = Cli::try_parse_from(["alien-worker-runtime", "--", "bun", "index.ts"]).unwrap();
        let config = RuntimeConfig::from_cli_struct(cli).unwrap();

        assert!(matches!(config.log_exporter, LogExporter::None));

        clear_otlp_env_vars();
    }
}
