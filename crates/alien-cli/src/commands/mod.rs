pub mod build;
pub mod commands;
pub mod deploy;
pub mod deployments;
pub mod destroy;
pub mod dev_helpers;
pub mod init;
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
pub use commands::{commands_task, commands_task_dev, CommandsArgs};
pub use deploy::{deploy_task, DeployArgs};
pub use deployments::{deployments_task, DeploymentsArgs};
pub use destroy::{destroy_task, DestroyArgs};
pub use dev_helpers::{
    build_and_post_release_simple, build_dev_status, build_embedded_dev_manager,
    create_initial_deployment, ensure_server_running, ensure_server_running_for_dev_session,
    ensure_server_running_with_env, fetch_all_dev_deployment_live_states,
    fetch_dev_deployment_live_state, prepare_dev_session_deployment, start_embedded_dev_manager,
    wait_for_dev_deployment_ready, wait_for_dev_deployment_ready_with_progress, write_dev_status,
    CliEnvVar, DevDeploymentLiveState, DevDeploymentSnapshot,
};
pub use init::{init_task, InitArgs};
pub use onboard::{onboard_task, OnboardArgs};
pub use release::{release_command, ReleaseArgs};
pub use vault::{vault_remote_task, vault_task, VaultArgs, VaultRemoteArgs};
pub use whoami::{whoami_task, WhoamiArgs};
