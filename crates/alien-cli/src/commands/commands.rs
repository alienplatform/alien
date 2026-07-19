//! CLI commands for invoking remote commands on deployments.
//!
//! Two entry points:
//! - `alien dev commands invoke` — invokes against the local dev server
//! - `alien commands invoke` — invokes against the standalone/platform manager

use crate::error::{ErrorData, Result};
use crate::execution_context::ExecutionMode;
use alien_commands_client::{CommandsClient, CommandsClientConfig};
use alien_core::DeploymentStatus;
use alien_error::{AlienError, Context, IntoAlienError};
use clap::{Parser, Subcommand};
use std::time::Duration;

#[derive(Parser, Debug, Clone)]
#[command(
    about = "Invoke remote commands on deployments",
    long_about = "Invoke remote commands on deployments.

Commands let you run code inside a deployment's environment — query databases,
trigger syncs, execute tool calls — without any inbound networking.

EXAMPLES:
    # Invoke a command in dev mode
    alien dev commands invoke --deployment default --command list-tables

    # Invoke with parameters
    alien dev commands invoke --deployment default --command query \\
      --params '{\"sql\": \"SELECT * FROM users\"}'

    # Invoke against a standalone manager
    alien commands invoke --deployment acme-corp --command generate-report \\
      --params '{\"startDate\": \"2025-01-01\"}'

See also: https://alien.dev/docs/commands"
)]
pub struct CommandsArgs {
    #[command(subcommand)]
    pub action: CommandsAction,
}

#[derive(Subcommand, Debug, Clone)]
pub enum CommandsAction {
    /// Invoke a command on a deployment and wait for the result
    Invoke {
        /// Deployment name or ID
        #[arg(long)]
        deployment: String,

        /// Command name to invoke
        #[arg(long)]
        command: String,

        /// Command parameters as JSON (default: {})
        #[arg(long, default_value = "{}")]
        params: String,

        /// Timeout in seconds (default: 60)
        #[arg(long, default_value = "60")]
        timeout: u64,
    },
}

/// Execute commands task — works in all execution modes.
pub async fn commands_task(args: CommandsArgs, ctx: ExecutionMode) -> Result<()> {
    match args.action {
        CommandsAction::Invoke {
            deployment,
            command,
            params,
            timeout,
        } => {
            let is_dev = ctx.is_dev();

            // Platform deployments retain their owning manager even when a
            // project's defaults change. Direct modes still use their known manager.
            #[cfg(feature = "platform")]
            let (manager, deployment) = if ctx.is_platform() {
                let resolved = crate::platform_deployment_resolver::resolve_with_manager(
                    &ctx,
                    &deployment,
                    None,
                    true,
                )
                .await?;
                (resolved.manager, String::from(resolved.detail.id))
            } else {
                let (_, project_link) = ctx.resolve_project(None, true).await?;
                (
                    ctx.resolve_manager_metadata_only(&project_link.project_id, "aws")
                        .await?,
                    deployment,
                )
            };
            #[cfg(not(feature = "platform"))]
            let manager = {
                let (_, project_link) = ctx.resolve_project(None, true).await?;
                ctx.resolve_manager_metadata_only(&project_link.project_id, "aws")
                    .await?
            };

            invoke_command(
                &manager.client,
                &manager.manager_url,
                manager.http_client.clone(),
                &deployment,
                &command,
                &params,
                timeout,
                is_dev,
            )
            .await
        }
    }
}

/// Execute commands task in dev mode.
pub async fn commands_task_dev(args: CommandsArgs, port: u16) -> Result<()> {
    let ctx = ExecutionMode::Dev { port };
    commands_task(args, ctx).await
}

async fn invoke_command(
    manager: &alien_manager_api::Client,
    manager_url: &str,
    http_client: reqwest::Client,
    deployment_name: &str,
    command: &str,
    params_json: &str,
    timeout_secs: u64,
    is_dev: bool,
) -> Result<()> {
    // Resolve deployment name to ID and ensure it's ready.
    let deployment_id = resolve_deployment_id(manager, deployment_name, is_dev).await?;

    // Parse params
    let params: serde_json::Value = serde_json::from_str(params_json)
        .into_alien_error()
        .context(ErrorData::ValidationError {
            field: "params".to_string(),
            message: "Invalid JSON".to_string(),
        })?;

    // Use alien-commands-client for the invoke flow (handles polling, base64, storage download)
    let config = CommandsClientConfig {
        timeout: Duration::from_secs(timeout_secs),
        allow_local_storage: true,
        ..Default::default()
    };
    let commands_url = format!("{}/v1", manager_url.trim_end_matches('/'));
    let client =
        CommandsClient::with_http_client(&commands_url, &deployment_id, http_client, config);

    let result: serde_json::Value = client
        .invoke(command, params)
        .await
        .into_alien_error()
        .context(ErrorData::ApiRequestFailed {
            message: format!("Command '{}' failed", command),
            url: Some(commands_url),
        })?;

    println!(
        "{}",
        serde_json::to_string_pretty(&result).unwrap_or_else(|_| format!("{:?}", result))
    );

    Ok(())
}

/// Resolve a deployment spec to its ID and assert it's ready for commands.
///
/// Delegates lookup to the shared `deployment_resolver`; this wrapper layers
/// on the "must be Running" check that's specific to command dispatch.
async fn resolve_deployment_id(
    manager: &alien_manager_api::Client,
    name: &str,
    is_dev: bool,
) -> Result<String> {
    let deployment = crate::deployment_resolver::resolve(manager, name, is_dev).await?;

    let status_raw = deployment.status.to_string();
    let status = parse_deployment_status(&status_raw).ok_or_else(|| {
        AlienError::new(ErrorData::ValidationError {
            field: "deployment.status".to_string(),
            message: format!(
                "Unknown deployment status '{}' for deployment '{}'.",
                status_raw, name
            ),
        })
    })?;

    if status != DeploymentStatus::Running {
        let status_cmd = if is_dev {
            "alien dev deployments ls"
        } else {
            "alien deployments ls"
        };
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "deployment".to_string(),
            message: format!(
                "Deployment '{}' is '{}' and cannot receive commands yet. Wait until it reaches 'running' (check with `{}`).",
                name,
                deployment_status_str(status),
                status_cmd
            ),
        }));
    }

    Ok(deployment.id.to_string())
}

fn parse_deployment_status(raw: &str) -> Option<DeploymentStatus> {
    match raw.to_ascii_lowercase().as_str() {
        "pending" => Some(DeploymentStatus::Pending),
        "preflights-failed" => Some(DeploymentStatus::PreflightsFailed),
        "initial-setup" => Some(DeploymentStatus::InitialSetup),
        "initial-setup-failed" => Some(DeploymentStatus::InitialSetupFailed),
        "provisioning" => Some(DeploymentStatus::Provisioning),
        "waiting-for-machines" => Some(DeploymentStatus::WaitingForMachines),
        "provisioning-failed" => Some(DeploymentStatus::ProvisioningFailed),
        "running" => Some(DeploymentStatus::Running),
        "refresh-failed" => Some(DeploymentStatus::RefreshFailed),
        "update-pending" => Some(DeploymentStatus::UpdatePending),
        "updating" => Some(DeploymentStatus::Updating),
        "update-failed" => Some(DeploymentStatus::UpdateFailed),
        "delete-pending" => Some(DeploymentStatus::DeletePending),
        "deleting" => Some(DeploymentStatus::Deleting),
        "delete-failed" => Some(DeploymentStatus::DeleteFailed),
        "teardown-required" => Some(DeploymentStatus::TeardownRequired),
        "teardown-failed" => Some(DeploymentStatus::TeardownFailed),
        "deleted" => Some(DeploymentStatus::Deleted),
        "error" => Some(DeploymentStatus::Error),
        _ => None,
    }
}

fn deployment_status_str(status: DeploymentStatus) -> &'static str {
    match status {
        DeploymentStatus::Pending => "pending",
        DeploymentStatus::PreflightsFailed => "preflights-failed",
        DeploymentStatus::InitialSetup => "initial-setup",
        DeploymentStatus::InitialSetupFailed => "initial-setup-failed",
        DeploymentStatus::Provisioning => "provisioning",
        DeploymentStatus::WaitingForMachines => "waiting-for-machines",
        DeploymentStatus::ProvisioningFailed => "provisioning-failed",
        DeploymentStatus::Running => "running",
        DeploymentStatus::RefreshFailed => "refresh-failed",
        DeploymentStatus::UpdatePending => "update-pending",
        DeploymentStatus::Updating => "updating",
        DeploymentStatus::UpdateFailed => "update-failed",
        DeploymentStatus::DeletePending => "delete-pending",
        DeploymentStatus::Deleting => "deleting",
        DeploymentStatus::DeleteFailed => "delete-failed",
        DeploymentStatus::TeardownRequired => "teardown-required",
        DeploymentStatus::TeardownFailed => "teardown-failed",
        DeploymentStatus::Deleted => "deleted",
        DeploymentStatus::Error => "error",
    }
}
