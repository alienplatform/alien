//! Application configuration

use crate::tui::state::BuildState;
use alien_platform_api::Client;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::sync::oneshot;

/// Configuration for the TUI app
///
/// Both dev and platform modes use the same SDK client - just pointing to different URLs.
/// The TUI is purely a view layer that queries an API and displays results.
pub struct AppConfig {
    /// SDK client (works for both dev server and platform API - they're API-compatible)
    pub sdk: Client,
    /// Mode determines which views are available
    pub mode: AppMode,
    /// Project ID for scoping logs (platform mode only)
    /// If not set, logs streaming won't work in platform mode
    pub project_id: Option<String>,
    /// DeepStore control plane URL for SSE streaming (platform mode only)
    /// If not set, uses the Agent Manager URL (works for local dev)
    pub deepstore_control_plane_url: Option<String>,
    /// Build status receiver (CLI → TUI, dev mode only)
    pub build_status_rx: Option<Receiver<BuildState>>,
    /// Rebuild trigger sender (TUI → CLI, dev mode only)
    pub rebuild_tx: Option<Sender<()>>,
    /// Terminal ready signal (TUI → CLI, dev mode only)
    /// Sent after terminal is initialized in raw mode, signals build task can start
    pub terminal_ready_tx: Option<oneshot::Sender<()>>,
}

/// App mode - determines available views and display
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AppMode {
    /// Dev mode - limited views (no releases/packages), logs come via log_rx channel
    Dev,
    /// Platform mode - all views available, logs stream from DeepStore
    Platform,
}

impl AppConfig {
    /// Create a dev mode configuration
    pub fn dev(sdk: Client) -> Self {
        Self {
            sdk,
            mode: AppMode::Dev,
            project_id: None,
            deepstore_control_plane_url: None,
            build_status_rx: None,
            rebuild_tx: None,
            terminal_ready_tx: None,
        }
    }

    /// Create a platform mode configuration  
    pub fn platform(sdk: Client) -> Self {
        Self {
            sdk,
            mode: AppMode::Platform,
            project_id: None,
            deepstore_control_plane_url: None,
            build_status_rx: None,
            rebuild_tx: None,
            terminal_ready_tx: None,
        }
    }

    /// Set the project ID for log scoping (platform mode only)
    pub fn with_project(mut self, project_id: String) -> Self {
        self.project_id = Some(project_id);
        self
    }

    /// Set the DeepStore control plane URL for SSE streaming
    pub fn with_deepstore_control_plane(mut self, url: String) -> Self {
        self.deepstore_control_plane_url = Some(url);
        self
    }

    /// Set build channels (dev mode only)
    pub fn with_build_channels(
        mut self,
        build_status_rx: Receiver<BuildState>,
        rebuild_tx: Sender<()>,
    ) -> Self {
        self.build_status_rx = Some(build_status_rx);
        self.rebuild_tx = Some(rebuild_tx);
        self
    }

    /// Set terminal ready signal (dev mode only)
    pub fn with_terminal_ready_signal(mut self, terminal_ready_tx: oneshot::Sender<()>) -> Self {
        self.terminal_ready_tx = Some(terminal_ready_tx);
        self
    }

    /// Check if this is dev mode
    pub fn is_dev(&self) -> bool {
        self.mode == AppMode::Dev
    }
}
