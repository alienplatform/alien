//! Connection state types
//!
//! Represents the current connection context - where the TUI is connected to.

/// Connection information displayed in the header
#[derive(Debug, Clone)]
pub enum ConnectionInfo {
    /// Local dev server with actual URL
    Dev { url: String },
    /// Platform API with actual URL
    Platform { url: String },
}

impl ConnectionInfo {
    /// Create connection info for dev mode
    pub fn dev() -> Self {
        // Default URL, should be overridden with actual URL
        Self::Dev {
            url: "http://localhost:9090".to_string(),
        }
    }

    /// Create connection info for dev mode with custom URL
    pub fn dev_with_url(url: String) -> Self {
        Self::Dev { url }
    }

    /// Create connection info for platform mode
    pub fn platform() -> Self {
        // Default URL, should be overridden with actual URL
        Self::Platform {
            url: "https://api.alien.dev".to_string(),
        }
    }

    /// Create connection info for platform mode with custom URL
    pub fn platform_with_url(url: String) -> Self {
        Self::Platform { url }
    }

    /// Check if this is dev mode
    pub fn is_dev(&self) -> bool {
        matches!(self, Self::Dev { .. })
    }

    /// Get the display text for the header (the actual URL)
    pub fn display_text(&self) -> &str {
        match self {
            Self::Dev { url } => url,
            Self::Platform { url } => url,
        }
    }

    /// Get a short mode label
    pub fn mode_label(&self) -> &'static str {
        match self {
            Self::Dev { .. } => "LOCAL",
            Self::Platform { .. } => "PROD",
        }
    }
}
