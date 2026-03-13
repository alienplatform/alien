// Re-export error printing from alien-cli-common
pub use alien_cli_common::tui::{error_ui::ErrorColors, ErrorPrinter};

// ============ Common utilities ============
pub mod common;
pub use common::{
    widgets, StepState, StepStatus, MAX_VIEWPORT_HEIGHT, MIN_VIEWPORT_HEIGHT, SPINNER_FRAMES,
};

// ============ Build & Release UIs ============
pub mod build_ui;
pub use build_ui::{
    calculate_required_height, BuildPhase, BuildResult, BuildState, BuildUi, BuildUiComponent,
    BuildUiEvent, BuildUiProps, ResourceBuildState, ResourceType,
};

pub mod release_ui;
pub use release_ui::{
    FunctionPushPhase, FunctionPushState, ReleaseResult, ReleaseState, ReleaseUi,
    ReleaseUiComponent, ReleaseUiEvent, ReleaseUiProps,
};

// ============ New TUI architecture ============
pub mod app;
pub mod dialogs;
pub mod framework;
pub mod services;
pub mod state;
pub mod views;

// Re-export state types
pub use state::{
    Action, AppState, CommandItem, CommandState, DeploymentDetailState, DeploymentGroupItem,
    DeploymentItem, DeploymentMetadata, DeploymentStatus, InputMode, ListState, LogLevel, LogLine,
    PackageItem, PackageStatus, ReleaseItem, ResourceInfo, SearchState, ViewId,
};

// Re-export services
pub use services::AppServices;

// Re-export app
pub use app::{run_app, AppConfig, AppController, AppMode, AppViewState};

// Re-export dialog types
pub use dialogs::{
    DeploymentGroupInfo, NewDeploymentDialog, NewDeploymentResult, Platform as DialogPlatform,
};
