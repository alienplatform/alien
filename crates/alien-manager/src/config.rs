use std::path::PathBuf;

/// Configuration for alien-manager.
#[derive(Debug, Clone)]
pub struct ManagerConfig {
    /// HTTP server port.
    pub port: u16,
    /// Path to SQLite database file. Required when using the default sqlite providers.
    pub db_path: Option<PathBuf>,
    /// Directory for local state (KV, storage, etc.).
    /// Required when using default sqlite providers or the `Local` deployment platform.
    pub state_dir: Option<PathBuf>,
    /// Deployment loop interval in seconds.
    pub deployment_interval_secs: u64,
    /// Heartbeat interval in seconds.
    pub heartbeat_interval_secs: u64,
    /// OTLP endpoint for telemetry forwarding.
    pub otlp_endpoint: Option<String>,
    /// Whether this is dev mode (permissive auth, local credentials, in-memory telemetry).
    pub dev_mode: bool,
    /// Public base URL for this manager instance (used for command response URLs, OAuth callbacks, etc.).
    /// Defaults to http://localhost:{port} when not set.
    pub base_url: Option<String>,
}

impl ManagerConfig {
    pub fn base_url(&self) -> String {
        self.base_url
            .clone()
            .unwrap_or_else(|| format!("http://localhost:{}", self.port))
    }

    pub fn commands_base_url(&self) -> String {
        format!("{}/v1", self.base_url())
    }
}

impl Default for ManagerConfig {
    fn default() -> Self {
        Self {
            port: 8080,
            db_path: Some(PathBuf::from("alien-manager.db")),
            state_dir: Some(PathBuf::from(".alien-manager")),
            deployment_interval_secs: 10,
            heartbeat_interval_secs: 60,
            otlp_endpoint: None,
            dev_mode: false,
            base_url: None,
        }
    }
}
