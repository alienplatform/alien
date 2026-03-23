pub mod build;
pub mod deploy;
pub mod deployments;
pub mod destroy;
pub mod dev_helpers;
pub mod dev_registry;
pub mod onboard;
pub mod release;
pub mod serve;
pub mod vault;
pub mod whoami;

/// Platform-specific commands (login, logout, workspaces, projects, link, unlink).
/// Only available when the `platform` feature is enabled.
#[cfg(feature = "platform")]
pub mod platform;

pub use build::{build_command, BuildArgs};
pub use deploy::{deploy_task, DeployArgs};
pub use deployments::{deployments_task, DeploymentsArgs};
pub use destroy::{destroy_task, DestroyArgs};
pub use dev_helpers::{
    build_and_post_release_simple, create_initial_deployment, ensure_server_running,
    ensure_server_running_with_env, CliEnvVar,
};
pub use onboard::{onboard_task, OnboardArgs};
pub use release::{release_command, ReleaseArgs};
pub use serve::{serve_task, ServeArgs};
pub use vault::{vault_task, VaultArgs};
pub use whoami::{whoami_task, WhoamiArgs};
