//! Application-level state types
//!
//! Core types used across the TUI: view identifiers, input modes, and actions.

use std::time::Duration;

/// Build state for dev mode
#[derive(Debug, Clone)]
pub enum BuildState {
    Idle,
    /// Initial build on startup - TUI should be read-only during this
    Initializing,
    Building,
    Built {
        duration: Duration,
    },
    Failed {
        error: String,
    },
}

impl BuildState {
    /// Get header display text for this build state
    pub fn header_display(&self) -> Option<String> {
        match self {
            BuildState::Idle => None,
            BuildState::Initializing => Some("Initializing...".to_string()),
            BuildState::Building => Some("Building...".to_string()),
            BuildState::Built { duration } => Some(format!(
                "Built in {:.1}s - Press B to rebuild",
                duration.as_secs_f64()
            )),
            BuildState::Failed { .. } => Some("Build Failed - Press B to retry".to_string()),
        }
    }

    /// Check if the TUI is still initializing (should ignore most input)
    pub fn is_initializing(&self) -> bool {
        matches!(self, BuildState::Initializing)
    }
}

/// Unique identifier for a view
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ViewId {
    Deployments,
    DeploymentDetail(String), // Contains deployment ID
    DeploymentGroups,
    Commands,
    Releases,
    Packages,
    Logs,
}

impl ViewId {
    pub fn title(&self) -> &'static str {
        match self {
            ViewId::Deployments => "Deployments",
            ViewId::DeploymentDetail(_) => "Deployment Detail",
            ViewId::DeploymentGroups => "Deployment Groups",
            ViewId::Commands => "Commands",
            ViewId::Releases => "Releases",
            ViewId::Packages => "Packages",
            ViewId::Logs => "Logs",
        }
    }
}

/// Input mode for the TUI
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InputMode {
    #[default]
    Normal,
    Search,
    Dialog,
}

/// Actions returned by views to control app flow
#[derive(Debug, Clone)]
pub enum Action {
    /// No action needed
    None,
    /// Quit the application
    Quit,
    /// Navigate to deployment detail view
    NavigateToDeployment(String),
    /// Navigate to a specific view
    NavigateToView(ViewId),
    /// Navigate back (e.g., from detail to list)
    NavigateBack,
    /// Refresh the current view's data
    Refresh,
    /// Show error message
    ShowError(String),
    /// Open new deployment dialog
    OpenNewDeploymentDialog,
    /// Create a new deployment via API
    CreateDeployment {
        platform: String,
        name: String,
        deployment_group_id: String,
    },
    /// Delete a deployment via API
    DeleteDeployment(String),
    /// Switch log source (agent manager) - triggers reconnection
    SwitchLogSource,
    /// Search logs via DeepStore query (platform mode only)
    SearchLogs(String),
    /// Trigger rebuild (dev mode only)
    TriggerRebuild,
    /// Show error dialog with AlienError details
    ShowErrorDialog(alien_error::AlienError<alien_error::GenericError>),
    /// Navigate to logs view filtered by deployment
    NavigateToLogsFilteredByDeployment {
        deployment_id: String,
        deployment_name: String,
    },
    /// Navigate to commands view filtered by deployment
    NavigateToCommandsFilteredByDeployment {
        deployment_id: String,
        deployment_name: String,
    },
    /// Clear filters in the current view
    ClearFilters,
}

/// Search state for filtering
#[derive(Debug, Clone, Default)]
pub struct SearchState {
    /// Whether search input is active
    active: bool,
    /// Current search query
    query: String,
}

impl SearchState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_active(&self) -> bool {
        !self.query.is_empty()
    }

    pub fn is_input_active(&self) -> bool {
        self.active
    }

    pub fn query(&self) -> Option<&str> {
        if self.query.is_empty() {
            None
        } else {
            Some(&self.query)
        }
    }

    pub fn activate(&mut self) {
        self.active = true;
    }

    pub fn deactivate(&mut self) {
        self.active = false;
    }

    pub fn clear(&mut self) {
        self.query.clear();
        self.active = false;
    }

    pub fn input(&mut self, c: char) {
        self.query.push(c);
    }

    pub fn backspace(&mut self) {
        self.query.pop();
    }
}

/// Application state shared across views
#[derive(Clone, Default)]
pub struct AppState {
    /// Current input mode
    pub input_mode: InputMode,
    /// Search state
    pub search: SearchState,
    /// Spinner frame for animations
    pub spinner_frame: usize,
}

impl AppState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn tick(&mut self) {
        self.spinner_frame = self.spinner_frame.wrapping_add(1);
    }
}
