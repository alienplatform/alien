//! CLI commands for invoking remote commands on deployments.
//!
//! Two entry points:
//! - `alien dev commands invoke` — invokes against the local dev server
//! - `alien commands invoke` — invokes against the standalone/platform manager

use crate::error::{ErrorData, Result};
use crate::execution_context::ExecutionMode;
use alien_commands_client::{CommandsClient, CommandsClientConfig, InvokeOptions};
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

Commands are application RPC handled by your Worker, Container, or Daemon.
See also: `alien operations --help` for Remote Operator operations, which carry
a risk tier, approval policy, and access requests.

Docs: https://alien.dev/docs/commands"
)]
pub struct CommandsArgs {
    #[command(subcommand)]
    pub action: CommandsAction,
}

#[derive(Subcommand, Debug, Clone)]
pub enum CommandsAction {
    /// Invoke a command on a deployment and wait for the result
    Invoke {
        /// Deployment ID, or <deployment-group-name>/<deployment-name>
        #[arg(long)]
        deployment: String,

        /// Command name to invoke
        #[arg(long)]
        command: String,

        /// Resource ID to invoke when several resources handle commands
        #[arg(long)]
        target: Option<String>,

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
            target,
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
                InvokeOptions {
                    timeout: Some(Duration::from_secs(timeout)),
                    target_resource_id: target,
                    deadline: None,
                    idempotency_key: None,
                },
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
    options: InvokeOptions,
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
        allow_local_storage: true,
        ..Default::default()
    };
    let commands_url = format!("{}/v1", manager_url.trim_end_matches('/'));
    let client =
        CommandsClient::with_http_client(&commands_url, &deployment_id, http_client, config);

    let result: serde_json::Value = client
        .invoke_with_options(command, params, Some(options))
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
        "waiting-for-secrets" => Some(DeploymentStatus::WaitingForSecrets),
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
        DeploymentStatus::WaitingForSecrets => "waiting-for-secrets",
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

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::{
        Method::{GET, POST},
        MockServer,
    };
    use serde_json::json;

    #[tokio::test]
    async fn selected_target_reaches_command_request() {
        let server = MockServer::start_async().await;
        server.mock_async(|when, then| {
            when.method(GET).path("/v1/deployments/dep_test");
            then.json_body(json!({"id":"dep_test", "name":"docproc", "platform":"test", "status":"running",
                "deploymentGroupId":"dg_test", "deploymentProtocolVersion":1,"projectId":"prj_test",
                "workspaceId":"ws_test","retryRequested":false,"updateBlocked":false,"createdAt":"2026-01-01T00:00:00Z"}));
        }).await;
        let create = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v1/commands")
                    .body_contains("\"targetResourceId\":\"worker-a\"");
                then.json_body(json!({"commandId":"cmd_test"}));
            })
            .await;
        server.mock_async(|when, then| {
            when.method(GET).path("/v1/commands/cmd_test");
            then.json_body(json!({"state":"SUCCEEDED","response":{"response":{"mode":"inline","inlineBase64":"e30="}}}));
        }).await;
        let manager = alien_manager_api::Client::new(&server.base_url());
        invoke_command(
            &manager,
            &server.base_url(),
            reqwest::Client::new(),
            "dep_test",
            "ping",
            "{}",
            InvokeOptions {
                timeout: Some(Duration::from_secs(2)),
                target_resource_id: Some("worker-a".to_string()),
                deadline: None,
                idempotency_key: None,
            },
            false,
        )
        .await
        .expect("targeted command completes");
        create.assert_async().await;
    }
}
