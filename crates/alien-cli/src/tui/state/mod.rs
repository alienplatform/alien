//! Pure state types for TUI views
//!
//! This module contains all state types that views need to render.
//! These are plain data structures with no async operations or SDK dependencies.

pub mod managers;
pub mod app;
pub mod commands;
pub mod connection;
pub mod deployment_groups;
pub mod deployments;
pub mod list;
pub mod packages;
pub mod releases;

pub use managers::{ManagerItem, ManagerStatus, LogsConnectionStatus};
pub use app::{Action, AppState, BuildState, InputMode, SearchState, ViewId};
pub use commands::{CommandItem, CommandState};
pub use connection::ConnectionInfo;
pub use deployment_groups::DeploymentGroupItem;
pub use deployments::{
    format_deployment_model, format_environment_info, format_heartbeats_mode,
    format_resource_status, format_telemetry_mode, format_updates_mode, DeploymentDetailState,
    DeploymentItem, DeploymentMetadata, DeploymentPlatform, DeploymentStatus, LogFilter, LogLevel,
    LogLine, LogsViewState, ResourceInfo,
};
pub use list::ListState;
pub use packages::{PackageItem, PackageStatus};
pub use releases::ReleaseItem;
