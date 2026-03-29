pub mod build;
pub mod deploy;
pub mod deployments;
pub mod destroy;
pub mod dev_helpers;
pub mod onboard;
pub mod release;
pub mod vault;
pub mod whoami;

/// Platform-specific commands (login, logout, workspaces, projects, link, unlink).
/// Only available when the `platform` feature is enabled.
#[cfg(feature = "platform")]
pub mod platform;

/// Manager commands (deploy, status, list, events, destroy).
/// Only available when the `platform` feature is enabled.
#[cfg(feature = "platform")]
pub mod manager;

pub use build::{build_command, BuildArgs};
pub use deploy::{deploy_task, DeployArgs};
pub use deployments::{deployments_task, DeploymentsArgs};
pub use destroy::{destroy_task, DestroyArgs};
pub use dev_helpers::{
    build_and_post_release_simple, build_embedded_dev_manager, create_initial_deployment,
    ensure_server_running, ensure_server_running_with_env, start_embedded_dev_manager, CliEnvVar,
};
pub use onboard::{onboard_task, OnboardArgs};
pub use release::{release_command, ReleaseArgs};
pub use vault::{vault_task, VaultArgs};
pub use whoami::{whoami_task, WhoamiArgs};
