//! TUI view demos

pub mod commands;
pub mod deploy_dialog;
pub mod deployment_detail;
pub mod deployment_groups;
pub mod deployments_list;
pub mod error_dialog;
pub mod header;
pub mod logs_view;
pub mod packages;
pub mod releases;
pub mod search;
pub mod tabs;

pub use commands::CommandsDemo;
pub use deploy_dialog::DeployDialogDemo;
pub use deployment_detail::DeploymentDetailDemo;
pub use deployment_groups::DeploymentGroupsDemo;
pub use deployments_list::DeploymentsListDemo;
pub use error_dialog::ErrorDialogDemo;
pub use header::HeaderDemo;
pub use logs_view::LogsViewDemo;
pub use packages::PackagesDemo;
pub use releases::ReleasesDemo;
pub use search::SearchDemo;
pub use tabs::TabsDemo;
