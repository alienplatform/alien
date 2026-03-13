//! Deployment-related state types
//!
//! State for both the deployments list view and the deployment detail view.

use alien_core::{
    DeploymentModel, EnvironmentInfo, HeartbeatsMode, ResourceLifecycle, ResourceOutputs,
    ResourceStatus, ResourceType, StackSettings, TelemetryMode, UpdatesMode,
};
use alien_error::{AlienError, GenericError};
use chrono::{DateTime, Utc};
use ratatui::style::Color;
use std::collections::{HashSet, VecDeque};

// Re-export types from alien-core with TUI-friendly aliases
pub use alien_core::DeploymentStatus;
pub use alien_core::Platform as DeploymentPlatform;

/// Display ResourceStatus as kebab-case string
pub fn format_resource_status(status: &ResourceStatus) -> &'static str {
    match status {
        ResourceStatus::Pending => "pending",
        ResourceStatus::Provisioning => "provisioning",
        ResourceStatus::ProvisionFailed => "provision-failed",
        ResourceStatus::Running => "running",
        ResourceStatus::Updating => "updating",
        ResourceStatus::UpdateFailed => "update-failed",
        ResourceStatus::Deleting => "deleting",
        ResourceStatus::DeleteFailed => "delete-failed",
        ResourceStatus::Deleted => "deleted",
        ResourceStatus::RefreshFailed => "refresh-failed",
    }
}

/// Display helpers for DeploymentStatus
impl DeploymentStatusExt for DeploymentStatus {
    fn is_healthy(&self) -> bool {
        matches!(self, DeploymentStatus::Running)
    }

    fn is_transitioning(&self) -> bool {
        matches!(
            self,
            DeploymentStatus::Pending
                | DeploymentStatus::Provisioning
                | DeploymentStatus::InitialSetup
                | DeploymentStatus::Updating
                | DeploymentStatus::UpdatePending
                | DeploymentStatus::DeletePending
                | DeploymentStatus::Deleting
        )
    }

    fn is_failed(&self) -> bool {
        matches!(
            self,
            DeploymentStatus::InitialSetupFailed
                | DeploymentStatus::ProvisioningFailed
                | DeploymentStatus::RefreshFailed
                | DeploymentStatus::UpdateFailed
                | DeploymentStatus::DeleteFailed
        )
    }

    /// Display as kebab-case string (matches backend format)
    fn display(&self) -> &'static str {
        match self {
            DeploymentStatus::Pending => "pending",
            DeploymentStatus::Provisioning => "provisioning",
            DeploymentStatus::InitialSetup => "initial-setup",
            DeploymentStatus::Running => "running",
            DeploymentStatus::Updating => "updating",
            DeploymentStatus::UpdatePending => "update-pending",
            DeploymentStatus::InitialSetupFailed => "initial-setup-failed",
            DeploymentStatus::ProvisioningFailed => "provisioning-failed",
            DeploymentStatus::RefreshFailed => "refresh-failed",
            DeploymentStatus::UpdateFailed => "update-failed",
            DeploymentStatus::DeleteFailed => "delete-failed",
            DeploymentStatus::DeletePending => "delete-pending",
            DeploymentStatus::Deleting => "deleting",
            DeploymentStatus::Deleted => "deleted",
        }
    }
}

/// Extension trait for DeploymentStatus display methods
pub trait DeploymentStatusExt {
    fn is_healthy(&self) -> bool;
    fn is_transitioning(&self) -> bool;
    fn is_failed(&self) -> bool;
    fn display(&self) -> &'static str;
}

/// Deployment item for list display
#[derive(Debug, Clone)]
pub struct DeploymentItem {
    pub id: String,
    pub name: String,
    pub deployment_group_id: String,
    pub deployment_group_name: Option<String>,
    pub status: DeploymentStatus,
    pub platform: DeploymentPlatform,
    pub release_info: Option<ReleaseInfo>,
}

/// Release info for display
#[derive(Debug, Clone)]
pub struct ReleaseInfo {
    pub id: String,
    pub git_commit_sha: Option<String>,
    pub git_branch: Option<String>,
    pub created_at: DateTime<Utc>,
}

// ============ Deployment Detail State ============

/// Log level for display
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

impl LogLevel {
    /// Parse log level from a severity string (e.g., "INFO", "ERROR", "WARN")
    pub fn from_str(s: &str) -> Self {
        let lower = s.to_lowercase();
        match lower.as_str() {
            "error" | "err" | "fatal" | "critical" => LogLevel::Error,
            "warn" | "warning" => LogLevel::Warn,
            "debug" | "trace" => LogLevel::Debug,
            _ => LogLevel::Info,
        }
    }

    /// Parse log level from log line content (heuristic)
    pub fn from_line(line: &str) -> Self {
        let lower = line.to_lowercase();
        if lower.contains("error") || lower.contains("err]") {
            LogLevel::Error
        } else if lower.contains("warn") {
            LogLevel::Warn
        } else if lower.contains("debug") || lower.contains("trace") {
            LogLevel::Debug
        } else {
            LogLevel::Info
        }
    }

    pub fn color(&self) -> Color {
        match self {
            LogLevel::Debug => Color::Rgb(107, 114, 128),
            LogLevel::Info => Color::Rgb(229, 231, 235),
            LogLevel::Warn => Color::Rgb(245, 158, 11),
            LogLevel::Error => Color::Rgb(239, 68, 68),
        }
    }
}

/// A log line from a running deployment
#[derive(Debug, Clone)]
pub struct LogLine {
    pub deployment_id: String,
    pub deployment_name: Option<String>,
    pub deployment_group_id: Option<String>,
    pub deployment_group_name: Option<String>,
    pub resource_id: String,
    pub content: String,
    pub timestamp: DateTime<Utc>,
    pub level: LogLevel,
}

impl LogLine {
    pub fn new(
        deployment_id: impl Into<String>,
        resource_id: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        let content = content.into();
        let level = LogLevel::from_line(&content);
        Self {
            deployment_id: deployment_id.into(),
            deployment_name: None,
            deployment_group_id: None,
            deployment_group_name: None,
            resource_id: resource_id.into(),
            content,
            timestamp: Utc::now(),
            level,
        }
    }

    /// Create a log line with a specific timestamp (for DeepStore logs)
    pub fn with_timestamp(mut self, timestamp: DateTime<Utc>) -> Self {
        self.timestamp = timestamp;
        self
    }

    /// Create a log line with a specific level (for DeepStore logs)
    pub fn with_level(mut self, level: impl AsRef<str>) -> Self {
        self.level = LogLevel::from_str(level.as_ref());
        self
    }
}

/// Resource state from a running deployment
#[derive(Debug, Clone)]
pub struct ResourceInfo {
    pub id: String,
    pub resource_type: ResourceType,
    pub lifecycle: ResourceLifecycle,
    pub status: ResourceStatus,
    pub outputs: Option<ResourceOutputs>,
}

impl ResourceInfo {
    pub fn is_frozen(&self) -> bool {
        matches!(self.lifecycle, ResourceLifecycle::Frozen)
    }
}

/// Deployment metadata for display
#[derive(Debug, Clone)]
pub struct DeploymentMetadata {
    pub created_at: String,
    pub platform: DeploymentPlatform,
    pub stack_settings: StackSettings,
    pub environment_info: Option<EnvironmentInfo>,
    pub current_release_id: Option<String>,
    pub error: Option<AlienError<GenericError>>,
}

// Display helper functions for formatting alien-core types

/// Format DeploymentModel for display
pub fn format_deployment_model(dm: &DeploymentModel) -> &str {
    match dm {
        DeploymentModel::Push => "Push",
        DeploymentModel::Pull => "Pull",
    }
}

/// Format UpdatesMode for display
pub fn format_updates_mode(mode: &UpdatesMode) -> &str {
    match mode {
        UpdatesMode::Auto => "Auto",
        UpdatesMode::ApprovalRequired => "Approval Required",
    }
}

/// Format TelemetryMode for display
pub fn format_telemetry_mode(mode: &TelemetryMode) -> &str {
    match mode {
        TelemetryMode::Off => "Off",
        TelemetryMode::Auto => "Auto",
        TelemetryMode::ApprovalRequired => "Approval Required",
    }
}

/// Format HeartbeatsMode for display
pub fn format_heartbeats_mode(mode: &HeartbeatsMode) -> &str {
    match mode {
        HeartbeatsMode::Off => "Off",
        HeartbeatsMode::On => "On",
    }
}

/// Format EnvironmentInfo summary for display
pub fn format_environment_info(env: &EnvironmentInfo) -> String {
    match env {
        EnvironmentInfo::Aws(info) => format!("{} ({})", info.account_id, info.region),
        EnvironmentInfo::Gcp(info) => format!("{} ({})", info.project_id, info.region),
        EnvironmentInfo::Azure(info) => {
            format!("Subscription {} ({})", info.subscription_id, info.location)
        }
        EnvironmentInfo::Local(info) => format!("{} ({})", info.hostname, info.os),
        EnvironmentInfo::Test(info) => format!("Test: {}", info.test_id),
    }
}

/// Log filter state
#[derive(Debug, Clone, Default)]
pub struct LogFilter {
    pub visible_services: HashSet<String>,
    pub visible_deployments: HashSet<String>,
    pub visible_levels: HashSet<LogLevel>,
    pub filter_active: bool,
}

impl LogFilter {
    pub fn is_visible(&self, log: &LogLine) -> bool {
        if !self.filter_active {
            return true;
        }

        let service_ok =
            self.visible_services.is_empty() || self.visible_services.contains(&log.resource_id);
        let deployment_ok = self.visible_deployments.is_empty()
            || self.visible_deployments.contains(&log.deployment_id);
        let level_ok = self.visible_levels.is_empty() || self.visible_levels.contains(&log.level);

        service_ok && deployment_ok && level_ok
    }

    /// Legacy method for deployment detail view
    pub fn is_service_visible(&self, service_id: &str) -> bool {
        if !self.filter_active {
            return true;
        }
        self.visible_services.is_empty() || self.visible_services.contains(service_id)
    }

    pub fn toggle_service(&mut self, service_id: &str) {
        if self.visible_services.contains(service_id) {
            self.visible_services.remove(service_id);
        } else {
            self.visible_services.insert(service_id.to_string());
        }
        self.update_filter_active();
    }

    pub fn toggle_deployment(&mut self, deployment_id: &str) {
        if self.visible_deployments.contains(deployment_id) {
            self.visible_deployments.remove(deployment_id);
        } else {
            self.visible_deployments.insert(deployment_id.to_string());
        }
        self.update_filter_active();
    }

    pub fn toggle_level(&mut self, level: LogLevel) {
        if self.visible_levels.contains(&level) {
            self.visible_levels.remove(&level);
        } else {
            self.visible_levels.insert(level);
        }
        self.update_filter_active();
    }

    fn update_filter_active(&mut self) {
        self.filter_active = !self.visible_services.is_empty()
            || !self.visible_deployments.is_empty()
            || !self.visible_levels.is_empty();
    }

    pub fn show_all(&mut self) {
        self.filter_active = false;
        self.visible_services.clear();
        self.visible_deployments.clear();
        self.visible_levels.clear();
    }

    pub fn display_text(&self) -> String {
        if !self.filter_active {
            "all".to_string()
        } else {
            let mut parts = Vec::new();
            if !self.visible_deployments.is_empty() {
                parts.push(format!("{} deployments", self.visible_deployments.len()));
            }
            if !self.visible_services.is_empty() {
                parts.push(format!("{} resources", self.visible_services.len()));
            }
            if !self.visible_levels.is_empty() {
                parts.push(format!("{} levels", self.visible_levels.len()));
            }
            if parts.is_empty() {
                "all".to_string()
            } else {
                parts.join(", ")
            }
        }
    }
}

/// State for the global Logs view (tab)
#[derive(Debug, Clone)]
pub struct LogsViewState {
    pub filter: LogFilter,
    pub scroll_offset: usize,
    pub auto_scroll: bool,
    /// Search query for filtering logs
    /// - Local dev: Client-side text filtering (substring match in content/deployment/resource)
    /// - Platform: DeepStore query language (passed to server for search)
    pub search_query: String,
    /// Whether user is currently typing in the search input
    pub is_searching: bool,
    /// Available managers (platform mode only)
    pub managers: Vec<super::managers::ManagerItem>,
    /// Currently selected manager index (None = use local/default)
    pub selected_manager_idx: Option<usize>,
    /// Connection status for log streaming
    pub connection_status: super::managers::LogsConnectionStatus,
    /// Whether logs are still being initialized
    pub initializing: bool,
}

impl Default for LogsViewState {
    fn default() -> Self {
        Self {
            filter: LogFilter::default(),
            scroll_offset: 0,
            auto_scroll: true,
            search_query: String::new(),
            is_searching: false,
            managers: Vec::new(),
            selected_manager_idx: None,
            connection_status: super::managers::LogsConnectionStatus::Disconnected,
            initializing: true, // Start as initializing
        }
    }
}

impl LogsViewState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Get the currently selected manager
    pub fn selected_manager(&self) -> Option<&super::managers::ManagerItem> {
        self.selected_manager_idx
            .and_then(|idx| self.managers.get(idx))
    }

    /// Select the next manager
    pub fn select_next_manager(&mut self) {
        if self.managers.is_empty() {
            return;
        }
        self.selected_manager_idx = match self.selected_manager_idx {
            None => Some(0),
            Some(idx) if idx + 1 >= self.managers.len() => None,
            Some(idx) => Some(idx + 1),
        };
    }

    /// Select the previous manager
    pub fn select_prev_manager(&mut self) {
        if self.managers.is_empty() {
            return;
        }
        self.selected_manager_idx = match self.selected_manager_idx {
            None => Some(self.managers.len() - 1),
            Some(0) => None,
            Some(idx) => Some(idx - 1),
        };
    }

    /// Set the available managers
    pub fn set_managers(&mut self, managers: Vec<super::managers::ManagerItem>) {
        self.managers = managers;
        // Auto-select first if none selected and we have managers
        if self.selected_manager_idx.is_none() && !self.managers.is_empty() {
            self.selected_manager_idx = Some(0);
        }
    }

    /// Filter by a specific deployment
    pub fn filter_by_deployment(&mut self, deployment_id: String) {
        self.filter.visible_deployments.clear();
        self.filter.visible_deployments.insert(deployment_id);
        self.filter.update_filter_active();
    }

    /// Clear all filters
    pub fn clear_filters(&mut self) {
        self.filter.show_all();
    }

    /// Get the filter display text for the UI
    pub fn get_filter_display(
        &self,
        deployment_name_cache: &std::collections::HashMap<String, String>,
    ) -> Option<String> {
        if !self.filter.filter_active || self.filter.visible_deployments.is_empty() {
            return None;
        }

        // If filtering by a single deployment, show deployment name
        if self.filter.visible_deployments.len() == 1 {
            let deployment_id = self.filter.visible_deployments.iter().next().unwrap();
            let deployment_name = deployment_name_cache
                .get(deployment_id)
                .map(|s| s.as_str())
                .unwrap_or(deployment_id);
            return Some(format!("Deployment: {}", deployment_name));
        }

        // Multiple deployments
        Some(format!(
            "Deployments: {}",
            self.filter.visible_deployments.len()
        ))
    }

    /// Filter logs from the global buffer
    pub fn filter_logs<'a>(&self, global_logs: &'a VecDeque<LogLine>) -> Vec<&'a LogLine> {
        global_logs
            .iter()
            .filter(|log| self.filter.is_visible(log))
            .filter(|log| {
                if self.search_query.is_empty() {
                    true
                } else {
                    let query = self.search_query.to_lowercase();
                    log.content.to_lowercase().contains(&query)
                        || log.resource_id.to_lowercase().contains(&query)
                        || log.deployment_id.to_lowercase().contains(&query)
                }
            })
            .collect()
    }

    /// Scroll logs up
    pub fn scroll_up(&mut self, amount: usize, total_logs: usize) {
        let max = total_logs.saturating_sub(1);
        self.scroll_offset = (self.scroll_offset + amount).min(max);
        self.auto_scroll = false;
    }

    /// Scroll logs down
    pub fn scroll_down(&mut self, amount: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(amount);
        if self.scroll_offset == 0 {
            self.auto_scroll = true;
        }
    }

    /// Reset scroll to bottom (newest logs)
    pub fn scroll_to_bottom(&mut self) {
        self.scroll_offset = 0;
        self.auto_scroll = true;
    }
}

/// State for the deployment detail view
///
/// Focuses on resources and deployment metadata. Logs are in the dedicated Logs view.
#[derive(Debug, Clone)]
pub struct DeploymentDetailState {
    pub deployment_id: String,
    pub deployment_name: String,
    pub deployment_group_id: String,
    pub deployment_group_name: Option<String>,
    pub status: DeploymentStatus,
    pub resources: Vec<ResourceInfo>,
    pub metadata: Option<DeploymentMetadata>,
}

impl DeploymentDetailState {
    pub fn new(deployment_id: String, deployment_name: String, status: DeploymentStatus) -> Self {
        Self {
            deployment_id,
            deployment_name,
            deployment_group_id: String::new(),
            deployment_group_name: None,
            status,
            resources: Vec::new(),
            metadata: None,
        }
    }

    pub fn with_deployment_group(
        mut self,
        deployment_group_id: String,
        deployment_group_name: Option<String>,
    ) -> Self {
        self.deployment_group_id = deployment_group_id;
        self.deployment_group_name = deployment_group_name;
        self
    }

    /// Update resources
    pub fn update_resources(&mut self, resources: Vec<ResourceInfo>) {
        self.resources = resources;
    }

    /// Update metadata
    pub fn update_metadata(&mut self, metadata: DeploymentMetadata) {
        self.metadata = Some(metadata);
    }

    /// Update status
    pub fn update_status(&mut self, status: DeploymentStatus) {
        self.status = status;
    }
}
