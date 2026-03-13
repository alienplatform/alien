//! Manager state types
//!
//! Used by the Logs view to select which manager to stream logs from.
//! In dev mode, the dev server acts as the manager.
//! In platform mode, users select from available managers.

/// Manager status for display
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManagerStatus {
    Healthy,
    Degraded,
    Unhealthy,
    Unknown,
}

impl ManagerStatus {
    pub fn from_str(s: &str) -> Self {
        match s {
            "healthy" => Self::Healthy,
            "degraded" => Self::Degraded,
            "unhealthy" => Self::Unhealthy,
            _ => Self::Unknown,
        }
    }

    pub fn display_text(&self) -> &'static str {
        match self {
            Self::Healthy => "Healthy",
            Self::Degraded => "Degraded",
            Self::Unhealthy => "Unhealthy",
            Self::Unknown => "Unknown",
        }
    }
}

/// Manager item for display
#[derive(Debug, Clone)]
pub struct ManagerItem {
    pub id: String,
    pub name: String,
    pub status: ManagerStatus,
    /// Base URL for this manager (proxies DeepStore requests)
    pub url: Option<String>,
    /// Whether this manager has DeepStore configured
    pub has_deepstore: bool,
}

impl ManagerItem {
    /// Check if this manager can be used for logs
    pub fn can_stream_logs(&self) -> bool {
        self.url.is_some() && self.has_deepstore && self.status != ManagerStatus::Unknown
    }
}

/// Connection status for log streaming
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogsConnectionStatus {
    /// Not connected (no manager selected or dev mode without logs)
    Disconnected,
    /// Connecting to the log stream
    Connecting,
    /// Connected and streaming
    Connected,
    /// Connection failed
    Error(String),
}

impl Default for LogsConnectionStatus {
    fn default() -> Self {
        Self::Disconnected
    }
}

impl LogsConnectionStatus {
    pub fn display_text(&self) -> &str {
        match self {
            Self::Disconnected => "Disconnected",
            Self::Connecting => "Connecting...",
            Self::Connected => "Connected",
            Self::Error(msg) => msg,
        }
    }

    pub fn is_connected(&self) -> bool {
        matches!(self, Self::Connected)
    }
}
