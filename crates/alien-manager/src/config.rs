use std::path::PathBuf;

/// Configuration for alien-manager.
#[derive(Debug, Clone)]
pub struct ManagerConfig {
    /// HTTP server port.
    pub port: u16,
    /// Path to SQLite database file.
    pub db_path: PathBuf,
    /// Directory for local state (KV, storage, etc.).
    pub state_dir: PathBuf,
    /// Deployment loop interval in seconds.
    pub deployment_interval_secs: u64,
    /// Heartbeat interval in seconds.
    pub heartbeat_interval_secs: u64,
    /// OTLP endpoint for telemetry forwarding.
    pub otlp_endpoint: Option<String>,
    /// Whether this is dev mode (permissive auth, local credentials, in-memory telemetry).
    pub dev_mode: bool,
}

impl ManagerConfig {
    pub fn base_url(&self) -> String {
        format!("http://localhost:{}", self.port)
    }

    pub fn commands_base_url(&self) -> String {
        // Used by CommandServer and runtime polling — they add /commands/{id}/response and /commands/leases
        format!("http://localhost:{}/v1", self.port)
    }
}

impl Default for ManagerConfig {
    fn default() -> Self {
        Self {
            port: 8080,
            db_path: PathBuf::from("alien-manager.db"),
            state_dir: PathBuf::from(".alien-manager"),
            deployment_interval_secs: 10,
            heartbeat_interval_secs: 60,
            otlp_endpoint: None,
            dev_mode: false,
        }
    }
}
