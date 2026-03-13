//! Aggregated application view state

use super::config::AppMode;
use crate::tui::dialogs::{DeploymentGroupInfo, ErrorDialog, NewDeploymentDialog};
use crate::tui::state::{
    AppState, BuildState, CommandItem, ConnectionInfo, DeploymentDetailState, DeploymentGroupItem,
    DeploymentItem, ListState, LogLine, LogsViewState, PackageItem, ReleaseItem, ViewId,
};
use std::collections::{HashMap, VecDeque};

/// Maximum number of logs to keep in the global buffer
const MAX_LOGS: usize = 10_000;

/// All view states aggregated into one struct
pub struct AppViewState {
    /// App mode (dev vs platform)
    pub mode: AppMode,
    /// Connection info for the header (where we're connected to)
    pub connection: ConnectionInfo,
    /// Global app state (input mode, search, spinner)
    pub app: AppState,
    /// Current active view
    pub current_view: ViewId,
    /// Deployments list state
    pub deployments: ListState<DeploymentItem>,
    /// Deployment detail state (when viewing a specific deployment)
    pub deployment_detail: Option<DeploymentDetailState>,
    /// Deployment groups list state
    pub deployment_groups: ListState<DeploymentGroupItem>,
    /// Commands list state
    pub commands: ListState<CommandItem>,
    /// Releases list state
    pub releases: ListState<ReleaseItem>,
    /// Packages list state
    pub packages: ListState<PackageItem>,
    /// Logs view state (for the Logs tab)
    pub logs_view: LogsViewState,
    /// Navigation history for back navigation
    pub history: Vec<ViewId>,
    /// New deployment dialog (when open)
    pub new_deployment_dialog: Option<NewDeploymentDialog>,
    /// Error dialog (when open)
    pub error_dialog: Option<ErrorDialog>,
    /// Global log buffer - always accumulates regardless of current view
    pub logs: VecDeque<LogLine>,
    /// In dev mode, the deployment ID that's actively being deployed
    /// All incoming logs are associated with this deployment
    pub active_deployment_id: Option<String>,
    /// Build state (dev mode only)
    pub build_state: Option<BuildState>,
    /// Deployment ID to name cache for enriching logs
    pub deployment_name_cache: HashMap<String, String>,
    /// Deployment ID to deployment group cache for enriching logs
    pub deployment_group_cache: HashMap<String, (String, String)>, // deployment_id -> (dg_id, dg_name)
    /// Filter for commands view (deployment_id to filter by)
    pub commands_filter_deployment_id: Option<String>,
}

impl Default for AppViewState {
    fn default() -> Self {
        Self::new(AppMode::Platform, ConnectionInfo::platform())
    }
}

impl AppViewState {
    /// Create a new app view state
    pub fn new(mode: AppMode, connection: ConnectionInfo) -> Self {
        Self {
            mode,
            connection,
            app: AppState::new(),
            current_view: ViewId::Deployments,
            deployments: ListState::loading(),
            deployment_detail: None,
            deployment_groups: ListState::new(),
            commands: ListState::new(),
            releases: ListState::new(),
            packages: ListState::new(),
            logs_view: LogsViewState::new(),
            history: Vec::new(),
            new_deployment_dialog: None,
            error_dialog: None,
            logs: VecDeque::with_capacity(MAX_LOGS),
            active_deployment_id: None,
            build_state: None,
            deployment_name_cache: HashMap::new(),
            deployment_group_cache: HashMap::new(),
            commands_filter_deployment_id: None,
        }
    }

    /// Open the new deployment dialog with deployment groups
    pub fn open_new_deployment_dialog(&mut self, deployment_groups: Vec<DeploymentGroupInfo>) {
        let is_dev_mode = self.mode == AppMode::Dev;
        let dialog =
            NewDeploymentDialog::new(is_dev_mode).with_deployment_groups(deployment_groups);
        self.new_deployment_dialog = Some(dialog);
        self.app.input_mode = crate::tui::state::InputMode::Dialog;
    }

    /// Close the new deployment dialog
    pub fn close_new_deployment_dialog(&mut self) {
        self.new_deployment_dialog = None;
        self.app.input_mode = crate::tui::state::InputMode::Normal;
    }

    /// Check if new deployment dialog is open
    pub fn is_new_deployment_dialog_open(&self) -> bool {
        self.new_deployment_dialog.is_some()
    }

    /// Open the error dialog
    pub fn open_error_dialog(&mut self, error: alien_error::AlienError<alien_error::GenericError>) {
        self.error_dialog = Some(ErrorDialog::new(error));
        self.app.input_mode = crate::tui::state::InputMode::Dialog;
    }

    /// Close the error dialog
    pub fn close_error_dialog(&mut self) {
        self.error_dialog = None;
        self.app.input_mode = crate::tui::state::InputMode::Normal;
    }

    /// Check if error dialog is open
    pub fn is_error_dialog_open(&self) -> bool {
        self.error_dialog.is_some()
    }

    /// Add a log line to the global buffer
    /// In dev mode, the deployment_id should come from active_deployment_id
    pub fn add_log(&mut self, mut log: LogLine) {
        // If no deployment_id set on log, use the active deployment
        if log.deployment_id.is_empty() {
            if let Some(ref deployment_id) = self.active_deployment_id {
                log.deployment_id = deployment_id.clone();
            }
        }

        // Enrich with deployment name from cache
        log = self.enrich_log_with_deployment_name(log);

        self.logs.push_back(log);

        // Mark logs as no longer initializing once we have logs
        if self.logs_view.initializing {
            self.logs_view.initializing = false;
        }

        // Trim if too many
        while self.logs.len() > MAX_LOGS {
            self.logs.pop_front();
        }
    }

    /// Set the active deployment ID (dev mode)
    /// All incoming logs without deployment_id will be associated with this deployment
    pub fn set_active_deployment(&mut self, deployment_id: String) {
        self.active_deployment_id = Some(deployment_id);
    }

    /// Navigate to a view
    pub fn navigate_to(&mut self, view: ViewId) {
        // Don't add to history if navigating to same view
        if self.current_view != view {
            self.history.push(self.current_view.clone());
            self.current_view = view;
        }
    }

    /// Navigate back
    pub fn navigate_back(&mut self) -> bool {
        if let Some(prev) = self.history.pop() {
            self.current_view = prev;
            self.deployment_detail = None;
            true
        } else {
            false
        }
    }

    /// Get available tabs for the tab bar based on mode
    pub fn available_tabs(&self) -> Vec<ViewId> {
        match self.mode {
            // Dev mode: Deployments, Deployment Groups, Commands, and Logs
            // (no Releases or Packages - those are platform-specific)
            AppMode::Dev => vec![
                ViewId::Deployments,
                ViewId::DeploymentGroups,
                ViewId::Commands,
                ViewId::Logs,
            ],
            // Platform mode: all views
            AppMode::Platform => vec![
                ViewId::Deployments,
                ViewId::DeploymentGroups,
                ViewId::Commands,
                ViewId::Releases,
                ViewId::Packages,
                ViewId::Logs,
            ],
        }
    }

    /// Tick the spinner
    pub fn tick(&mut self) {
        self.app.tick();
    }

    /// Update deployment name cache from deployments list
    /// Called when deployments are loaded/refreshed
    pub fn update_deployment_name_cache(&mut self) {
        self.deployment_name_cache.clear();
        self.deployment_group_cache.clear();
        for deployment in &self.deployments.items {
            self.deployment_name_cache
                .insert(deployment.id.clone(), deployment.name.clone());
            if let Some(ref dg_name) = deployment.deployment_group_name {
                self.deployment_group_cache.insert(
                    deployment.id.clone(),
                    (deployment.deployment_group_id.clone(), dg_name.clone()),
                );
            }
        }

        // Re-enrich all existing logs with updated deployment/deployment group info
        for log in self.logs.iter_mut() {
            if log.deployment_name.is_none() {
                log.deployment_name = self.deployment_name_cache.get(&log.deployment_id).cloned();
            }
            if log.deployment_group_name.is_none() {
                if let Some((dg_id, dg_name)) = self.deployment_group_cache.get(&log.deployment_id)
                {
                    log.deployment_group_id = Some(dg_id.clone());
                    log.deployment_group_name = Some(dg_name.clone());
                }
            }
        }
    }

    /// Enrich a log line with deployment name and deployment group from cache
    pub fn enrich_log_with_deployment_name(&self, mut log: LogLine) -> LogLine {
        if log.deployment_name.is_none() {
            log.deployment_name = self.deployment_name_cache.get(&log.deployment_id).cloned();
        }
        if log.deployment_group_name.is_none() {
            if let Some((dg_id, dg_name)) = self.deployment_group_cache.get(&log.deployment_id) {
                log.deployment_group_id = Some(dg_id.clone());
                log.deployment_group_name = Some(dg_name.clone());
            }
        }
        log
    }

    /// Filter commands by deployment and navigate to commands view
    pub fn filter_commands_by_deployment(&mut self, deployment_id: String) {
        self.commands_filter_deployment_id = Some(deployment_id);
    }

    /// Clear commands filter
    pub fn clear_commands_filter(&mut self) {
        self.commands_filter_deployment_id = None;
    }

    /// Get filtered commands based on current filter
    pub fn get_filtered_commands(&self) -> Vec<&CommandItem> {
        if let Some(ref filter_deployment_id) = self.commands_filter_deployment_id {
            self.commands
                .items
                .iter()
                .filter(|cmd| &cmd.deployment_id == filter_deployment_id)
                .collect()
        } else {
            self.commands.items.iter().collect()
        }
    }

    /// Get commands filter display text
    pub fn get_commands_filter_display(&self) -> Option<String> {
        self.commands_filter_deployment_id
            .as_ref()
            .map(|deployment_id| {
                let deployment_name = self
                    .deployment_name_cache
                    .get(deployment_id)
                    .map(|s| s.as_str())
                    .unwrap_or(deployment_id);
                format!("Deployment: {}", deployment_name)
            })
    }
}
