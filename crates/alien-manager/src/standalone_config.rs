//! TOML-based configuration for the alien-manager binary.
//!
//! Loaded from `alien-manager.toml` (or a CLI-specified path). Falls back to
//! sensible defaults when no file exists, so zero-config startup works out of
//! the box.

use alien_core::bindings::{
    ArtifactRegistryBinding, KvBinding, ServiceAccountBinding, StorageBinding,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::config::ManagerConfig;

// ── Defaults ────────────────────────────────────────────────────────────────

fn default_port() -> u16 {
    8080
}
fn default_host() -> String {
    "0.0.0.0".to_string()
}
fn default_deployment_interval() -> u64 {
    10
}
fn default_heartbeat_interval() -> u64 {
    60
}
fn default_state_dir() -> PathBuf {
    PathBuf::from("alien-data")
}

// ── Top-level config ────────────────────────────────────────────────────────

/// Manager configuration loaded from `alien-manager.toml`.
///
/// Every field has a sensible default, so an empty file (or no file) produces
/// a working manager with an embedded local artifact registry and SQLite storage.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ManagerTomlConfig {
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub database: DatabaseConfig,
    #[serde(default)]
    pub artifact_registry: ArtifactRegistrySection,
    #[serde(default)]
    pub commands: CommandsSection,
    #[serde(default)]
    pub impersonation: ImpersonationSection,
    #[serde(default)]
    pub telemetry: TelemetryConfig,
}

// ── Section configs ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ServerConfig {
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_host")]
    pub host: String,
    /// Public base URL for this manager instance (used for command response
    /// URLs, deploy pages, etc.). Defaults to `http://localhost:{port}`.
    pub base_url: Option<String>,
    /// Base URL for release binary downloads (alien-deploy, alien-agent).
    /// Defaults to `https://releases.alien.dev`.
    pub releases_url: Option<String>,
    #[serde(default = "default_deployment_interval")]
    pub deployment_interval_secs: u64,
    #[serde(default = "default_heartbeat_interval")]
    pub heartbeat_interval_secs: u64,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            port: default_port(),
            host: default_host(),
            base_url: None,
            releases_url: None,
            deployment_interval_secs: default_deployment_interval(),
            heartbeat_interval_secs: default_heartbeat_interval(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct DatabaseConfig {
    /// Path to the SQLite database file.
    /// Defaults to `{state_dir}/manager.db` when not set.
    pub path: Option<PathBuf>,
    /// Directory for all persistent state (database, KV, storage, admin token, etc.).
    #[serde(default = "default_state_dir")]
    pub state_dir: PathBuf,
    /// Optional AEGIS-256 encryption key for database-at-rest encryption.
    pub encryption_key: Option<String>,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            path: None,
            state_dir: default_state_dir(),
            encryption_key: None,
        }
    }
}

/// Artifact registry configuration.
///
/// The `default` binding is used for all platforms unless overridden.
/// Platform-specific bindings (aws, gcp, azure) take precedence when
/// deploying to that platform — e.g., Lambda requires ECR.
///
/// Uses the same typed binding enums as `alien.ts` stack definitions.
///
/// ```toml
/// [artifact-registry]
/// # Embedded local registry (default — no config needed)
///
/// # Or use ECR for AWS Lambda deployments:
/// # [artifact-registry.aws]
/// # service = "ecr"
/// # repositoryPrefix = "alien-artifacts"
/// # pullRoleArn = "arn:aws:iam::123:role/ecr-pull"
/// # pushRoleArn = "arn:aws:iam::123:role/ecr-push"
/// ```
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ArtifactRegistrySection {
    /// Default artifact registry binding. When not set, an embedded
    /// in-process local OCI registry is started automatically.
    pub default: Option<ArtifactRegistryBinding>,
    /// AWS-specific artifact registry (ECR). Used for Lambda deployments.
    pub aws: Option<ArtifactRegistryBinding>,
    /// GCP-specific artifact registry (GAR). Used for Cloud Run deployments.
    pub gcp: Option<ArtifactRegistryBinding>,
    /// Azure-specific artifact registry (ACR). Used for Container Apps deployments.
    pub azure: Option<ArtifactRegistryBinding>,
}

/// Commands protocol backend storage.
///
/// The KV store holds command state; the storage backend holds large
/// request/response payloads (>150KB).
///
/// Default: local filesystem in `{state_dir}/commands_kv` and
/// `{state_dir}/commands_storage`.
///
/// For push-mode deployments (Lambda, Cloud Run), use cloud-backed storage
/// so runtimes can access presigned URLs:
///
/// ```toml
/// [commands]
/// kv = { service = "dynamodb", tableName = "alien-commands", region = "us-east-1" }
/// storage = { service = "s3", bucketName = "alien-command-storage" }
/// ```
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct CommandsSection {
    /// KV store for command state.
    pub kv: Option<KvBinding>,
    /// Blob storage for large command payloads.
    pub storage: Option<StorageBinding>,
}

/// Cross-account credential impersonation.
///
/// Each platform entry provides a service account identity that the manager
/// assumes when deploying to remote environments.
///
/// ```toml
/// [impersonation.aws]
/// service = "awsiam"
/// roleName = "alien-management"
/// roleArn = "arn:aws:iam::123456789:role/alien-management"
/// ```
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ImpersonationSection {
    /// AWS impersonation identity (IAM role for STS AssumeRole).
    pub aws: Option<ServiceAccountBinding>,
    /// GCP impersonation identity (service account for token exchange).
    pub gcp: Option<ServiceAccountBinding>,
    /// Azure impersonation identity (managed identity or service principal).
    pub azure: Option<ServiceAccountBinding>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct TelemetryConfig {
    /// OTLP HTTP endpoint for forwarding logs, traces, and metrics.
    pub otlp_endpoint: Option<String>,
    /// Custom HTTP headers sent with every OTLP request.
    /// Use this for authentication with services like Datadog, Honeycomb,
    /// Grafana Cloud, New Relic, etc.
    #[serde(default)]
    pub headers: HashMap<String, String>,
}

// ── Loading ─────────────────────────────────────────────────────────────────

impl Default for ManagerTomlConfig {
    fn default() -> Self {
        Self {
            server: ServerConfig::default(),
            database: DatabaseConfig::default(),
            artifact_registry: ArtifactRegistrySection::default(),
            commands: CommandsSection::default(),
            impersonation: ImpersonationSection::default(),
            telemetry: TelemetryConfig::default(),
        }
    }
}

impl ManagerTomlConfig {
    /// Load configuration from a TOML file.
    ///
    /// Resolution order:
    /// 1. If `path` is `Some`, load from that exact file (error if missing).
    /// 2. Otherwise try `alien-manager.toml` in the current working directory.
    /// 3. If no file exists, fall back to all defaults.
    ///
    /// After loading, environment variable overrides are applied.
    pub fn load(path: Option<&Path>) -> Result<Self, String> {
        let mut config = match path {
            Some(p) => {
                let contents = std::fs::read_to_string(p)
                    .map_err(|e| format!("Failed to read config file {}: {}", p.display(), e))?;
                toml::from_str(&contents)
                    .map_err(|e| format!("Failed to parse {}: {}", p.display(), e))?
            }
            None => {
                let default_path = PathBuf::from("alien-manager.toml");
                if default_path.exists() {
                    let contents = std::fs::read_to_string(&default_path).map_err(|e| {
                        format!(
                            "Failed to read config file {}: {}",
                            default_path.display(),
                            e
                        )
                    })?;
                    toml::from_str(&contents)
                        .map_err(|e| format!("Failed to parse {}: {}", default_path.display(), e))?
                } else {
                    Self::default()
                }
            }
        };

        config.apply_env_overrides();
        Ok(config)
    }

    /// Apply environment variable overrides for server and telemetry settings.
    fn apply_env_overrides(&mut self) {
        if let Ok(val) = std::env::var("PORT") {
            if let Ok(port) = val.parse::<u16>() {
                self.server.port = port;
            }
        }
        if let Ok(val) = std::env::var("HOST") {
            self.server.host = val;
        }
        if let Ok(val) = std::env::var("BASE_URL") {
            self.server.base_url = Some(val);
        }
        if let Ok(val) = std::env::var("OTLP_ENDPOINT") {
            self.telemetry.otlp_endpoint = Some(val);
        }
    }

    /// Convert to the runtime `ManagerConfig` used by `AlienManagerBuilder`.
    pub fn to_manager_config(&self) -> ManagerConfig {
        let db_path = self
            .database
            .path
            .clone()
            .unwrap_or_else(|| self.database.state_dir.join("manager.db"));

        ManagerConfig {
            port: self.server.port,
            host: self.server.host.clone(),
            db_path: Some(db_path),
            state_dir: Some(self.database.state_dir.clone()),
            deployment_interval_secs: self.server.deployment_interval_secs,
            heartbeat_interval_secs: self.server.heartbeat_interval_secs,
            self_heartbeat_interval_secs: 60,
            otlp_endpoint: self.telemetry.otlp_endpoint.clone(),
            base_url: self.server.base_url.clone(),
            releases_url: self.server.releases_url.clone(),
            targets: Vec::new(),
            disable_deployment_loop: false,
            disable_heartbeat_loop: false,
            enable_local_log_ingest: false,
            allowed_origins: None,
            response_signing_key: Vec::new(), // Set by caller after admin token bootstrap
        }
    }

    /// Whether an embedded local artifact registry should be started.
    pub fn needs_embedded_registry(&self) -> bool {
        matches!(
            &self.artifact_registry.default,
            None | Some(ArtifactRegistryBinding::Local(_))
        )
    }

    /// Generate a heavily-commented TOML template.
    pub fn generate_template() -> String {
        r#"# alien-manager.toml
#
# All values shown are defaults. Uncomment and edit as needed.

[server]
# port = 8080
# host = "0.0.0.0"
# base-url = "https://alien.example.com"
# deployment-interval-secs = 10
# heartbeat-interval-secs = 60

[database]
# state-dir = "alien-data"
# path = "alien-data/manager.db"
# encryption-key = ""

# ── Artifact Registry ──────────────────────────────────────────────
# Default: embedded local OCI registry (no config needed).
# Override per-platform for push-mode deployments:
#
# [artifact-registry.aws]
# service = "ecr"
# repositoryPrefix = "alien-artifacts"
# pullRoleArn = "arn:aws:iam::123:role/ecr-pull"
# pushRoleArn = "arn:aws:iam::123:role/ecr-push"
#
# [artifact-registry.gcp]
# service = "gar"
# repositoryName = "projects/my-project/locations/us-central1/repositories/alien"
# pullServiceAccountEmail = "pull@project.iam.gserviceaccount.com"
# pushServiceAccountEmail = "push@project.iam.gserviceaccount.com"

# ── Commands ───────────────────────────────────────────────────────
# Default: local filesystem. Use cloud backends for push-mode:
#
# [commands]
# kv = { service = "dynamodb", tableName = "alien-commands", region = "us-east-1" }
# storage = { service = "s3", bucketName = "alien-command-storage" }

# ── Impersonation ─────────────────────────────────────────────────
# Cross-account credential impersonation for deploying to remote environments.
#
# [impersonation.aws]
# service = "awsiam"
# roleName = "alien-management"
# roleArn = "arn:aws:iam::123:role/alien-management"
#
# [impersonation.gcp]
# service = "gcpserviceaccount"
# email = "alien-management@project.iam.gserviceaccount.com"

# ── Telemetry ──────────────────────────────────────────────────────
# [telemetry]
# otlp-endpoint = "https://otel.example.com:4318"
# [telemetry.headers]
# DD-API-KEY = "your-datadog-key"
# Authorization = "Basic base64encoded"
"#
        .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_round_trips_through_toml() {
        let config = ManagerTomlConfig::default();
        let toml_str = toml::to_string_pretty(&config).expect("serialize");
        let parsed: ManagerTomlConfig = toml::from_str(&toml_str).expect("deserialize");
        assert_eq!(parsed.server.port, 8080);
        assert_eq!(parsed.server.host, "0.0.0.0");
        assert_eq!(parsed.database.path, None);
        assert_eq!(parsed.database.state_dir, PathBuf::from("alien-data"));
    }

    #[test]
    fn partial_toml_uses_defaults_for_missing() {
        let toml_str = r#"
[server]
port = 3000
"#;
        let config: ManagerTomlConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.server.port, 3000);
        assert_eq!(config.server.host, "0.0.0.0");
        assert_eq!(config.database.path, None);
        assert_eq!(config.database.state_dir, PathBuf::from("alien-data"));
    }

    #[test]
    fn to_manager_config_derives_db_path_from_state_dir() {
        let sc = ManagerTomlConfig::default();
        let mc = sc.to_manager_config();
        assert_eq!(mc.db_path, Some(PathBuf::from("alien-data/manager.db")));
    }

    #[test]
    fn to_manager_config_explicit_db_path() {
        let mut sc = ManagerTomlConfig::default();
        sc.server.port = 9999;
        sc.server.host = "127.0.0.1".to_string();
        sc.database.path = Some(PathBuf::from("/tmp/test.db"));
        sc.telemetry.otlp_endpoint = Some("http://otel:4318".to_string());

        let mc = sc.to_manager_config();
        assert_eq!(mc.port, 9999);
        assert_eq!(mc.host, "127.0.0.1");
        assert_eq!(mc.db_path, Some(PathBuf::from("/tmp/test.db")));
        assert_eq!(mc.otlp_endpoint, Some("http://otel:4318".to_string()));
    }

    #[test]
    fn generate_template_is_valid_toml() {
        let template = ManagerTomlConfig::generate_template();
        let parsed: ManagerTomlConfig =
            toml::from_str(&template).expect("template should be valid TOML");
        assert_eq!(parsed.server.port, 8080);
    }

    #[test]
    fn needs_embedded_registry_default() {
        let config = ManagerTomlConfig::default();
        assert!(config.needs_embedded_registry());
    }

    #[test]
    fn needs_embedded_registry_ecr() {
        let toml_str = r#"
[artifact-registry]
aws = { service = "ecr", repositoryPrefix = "test" }
"#;
        let config: ManagerTomlConfig = toml::from_str(toml_str).unwrap();
        // default is still None (embedded), even though aws is set
        assert!(config.needs_embedded_registry());
    }

    #[test]
    fn ecr_table_style_allows_missing_role_arns() {
        let toml_str = r#"
[artifact-registry.aws]
service = "ecr"
repositoryPrefix = "test"
"#;
        let _config: ManagerTomlConfig = toml::from_str(toml_str).unwrap();
    }
}
