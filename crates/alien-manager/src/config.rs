use std::path::PathBuf;

use alien_core::Platform;

/// DeepStore configuration for telemetry query and forwarding.
#[derive(Debug, Clone, Default)]
pub struct DeepStoreConfig {
    /// DeepStore OTLP endpoint URL for telemetry forwarding.
    pub otlp_url: Option<String>,
    /// DeepStore query endpoint URL for searching logs.
    pub query_url: Option<String>,
    /// DeepStore JWT public key (PEM) for validating Query JWTs.
    pub jwt_public_key: Option<String>,
    /// DeepStore database ID for telemetry.
    pub database_id: Option<String>,
}

/// GCP OAuth configuration for onboarding flow.
#[derive(Debug, Clone, Default)]
pub struct GcpOAuthConfig {
    /// GCP OAuth Client ID.
    pub client_id: Option<String>,
    /// GCP OAuth Client Secret.
    pub client_secret: Option<String>,
}

/// Configuration for the Platform (SaaS) operating mode.
#[derive(Debug, Clone)]
pub struct PlatformConfig {
    /// Alien API base URL (default: https://api.alien.dev).
    pub api_url: String,
    /// Manager-scoped API key for authenticating with the Platform API.
    pub api_key: String,
    /// Primary platform for standalone mode (manager's own KV/Storage infrastructure).
    pub primary_platform: Platform,
    /// DeepStore configuration.
    pub deepstore: DeepStoreConfig,
    /// GCP OAuth configuration.
    pub gcp_oauth: GcpOAuthConfig,
}

/// Configuration for alien-manager.
///
/// This struct carries runtime configuration — ports, paths, intervals, URLs.
/// It does NOT determine which providers are used. Provider selection is the
/// caller's responsibility: the binary `main.rs` (standalone or platform mode)
/// and `alien dev` each wire the builder with appropriate trait implementations.
#[derive(Debug, Clone)]
pub struct ManagerConfig {
    /// HTTP server port.
    pub port: u16,
    /// HTTP server host/bind address.
    pub host: String,
    /// Path to SQLite database file. Required when using the default sqlite providers.
    pub db_path: Option<PathBuf>,
    /// Directory for local state (KV, storage, etc.).
    /// Required when using default sqlite providers or the `Local` deployment platform.
    pub state_dir: Option<PathBuf>,
    /// Deployment loop interval in seconds.
    pub deployment_interval_secs: u64,
    /// Heartbeat interval in seconds.
    pub heartbeat_interval_secs: u64,
    /// Self-heartbeat interval in seconds (platform mode).
    pub self_heartbeat_interval_secs: u64,
    /// OTLP endpoint for telemetry forwarding.
    pub otlp_endpoint: Option<String>,
    /// Public base URL for this manager instance (used for command response URLs, OAuth callbacks, etc.).
    /// Defaults to http://localhost:{port} when not set.
    pub base_url: Option<String>,
    /// Base URL for release binary downloads (alien-deploy, alien-agent).
    /// Defaults to https://releases.alien.dev. Configurable via ALIEN_RELEASES_URL env var.
    pub releases_url: Option<String>,
    /// Target platforms this manager handles (platform mode).
    pub targets: Vec<Platform>,
    /// Disable the deployment loop.
    pub disable_deployment_loop: bool,
    /// Disable the heartbeat loop.
    pub disable_heartbeat_loop: bool,
    /// Whether deployments should emit OTLP logs back to this manager even
    /// without an external OTLP endpoint.
    ///
    /// `alien dev` enables this so locally-run workloads send logs back to the
    /// embedded manager. Standalone and platform managers leave it disabled
    /// unless they have a real OTLP forwarding endpoint configured.
    pub enable_local_log_ingest: bool,
}

impl ManagerConfig {
    /// Whether deployments should emit OTLP logs to this manager instance.
    pub fn enable_local_log_ingest(&self) -> bool {
        self.enable_local_log_ingest
    }

    pub fn base_url(&self) -> String {
        self.base_url
            .clone()
            .unwrap_or_else(|| format!("http://localhost:{}", self.port))
    }

    pub fn commands_base_url(&self) -> String {
        format!("{}/v1", self.base_url())
    }

    pub fn releases_url(&self) -> String {
        self.releases_url
            .clone()
            .unwrap_or_else(|| "https://releases.alien.dev".to_string())
    }
}

impl Default for ManagerConfig {
    fn default() -> Self {
        Self {
            port: 8080,
            host: "0.0.0.0".to_string(),
            db_path: Some(PathBuf::from("alien-manager.db")),
            state_dir: Some(PathBuf::from(".alien-manager")),
            deployment_interval_secs: 10,
            heartbeat_interval_secs: 60,
            self_heartbeat_interval_secs: 60,
            otlp_endpoint: None,
            base_url: None,
            releases_url: None,
            targets: Vec::new(),
            disable_deployment_loop: false,
            disable_heartbeat_loop: false,
            enable_local_log_ingest: false,
        }
    }
}
