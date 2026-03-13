//! TUI Dialogs
//!
//! Modal dialog components for user input.

pub mod error_dialog;
pub mod new_deployment;

pub use error_dialog::ErrorDialog;
pub use new_deployment::{DeploymentGroupInfo, NewDeploymentDialog, NewDeploymentResult, Platform};
