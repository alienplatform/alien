use std::future::Future;
use std::num::NonZeroU64;
use std::time::{Duration, Instant};

use crate::commands::destroy::{kept_deployer_secrets, print_kept_deployer_secrets};
use crate::commands::event_display::{print_event_table, EventDisplayRow};
use crate::deployment_tracking::DeploymentTracker;
use crate::error::{ErrorData, Result};
use crate::execution_context::ExecutionMode;
use crate::interaction::{ConfirmationMode, InteractionMode};
use crate::output::{print_json, prompt_confirm};
use crate::ui::{
    command, contextual_heading, deployment_resource_detail, dim_label, format_resource_status,
    heading, make_table, print_table, render_human_error, status_cell, success_line,
};
use alien_cli_common::network::{self, NetworkArgs};
use alien_core::{is_valid_resource_prefix, ComputeClusterOutputs, RESOURCE_PREFIX_ERROR_MESSAGE};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_manager_api::types::{DeleteDeploymentAction, DeploymentResponse};
use alien_manager_api::SdkResultExt as ManagerSdkResultExt;
use alien_manager_api::SdkResultExtReadingBody as _;
use alien_platform_api::types::{
    CreateDeploymentTokenId, CreateDeploymentTokenRequest, CreateDeploymentTokenWorkspace,
    CreateDeploymentWorkspace, DeploymentDetailResponse, DeploymentDetailResponseUpdateState,
    DeploymentListItemResponse, DeploymentUpdateOperationStatus,
    DeploymentUpdateOperationSummaryInner, GetDeploymentId, GetDeploymentWorkspace,
    ListDeploymentsIncludeItem, NewDeploymentRequest, PinDeploymentReleaseId,
    PinDeploymentReleaseWorkspace, PinReleaseRequest, PinReleaseRequestReleaseId,
    CreateVolumeRestoreRequest, VolumeRestore,
};
use alien_platform_api::SdkResultExt as _;
use clap::{Parser, Subcommand, ValueEnum};
use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DeploymentMutationOutput<'a> {
    deployment_id: &'a str,
    action: &'static str,
    accepted: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DeploymentRedeployOutput {
    deployment_id: String,
    action: &'static str,
    accepted: bool,
    operation_id: String,
    operation_status: String,
    successful: bool,
    elapsed_seconds: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UpdateOperationDisposition {
    Pending,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, Copy)]
struct PlatformRedeployOptions {
    wait: bool,
    timeout: Duration,
    interval: Duration,
    json: bool,
}

#[derive(Debug, Clone, Copy)]
struct UpdateOperationWaitOptions<'a> {
    workspace: &'a str,
    deployment_id: &'a str,
    operation_id: &'a str,
    timeout: Duration,
    interval: Duration,
    json: bool,
}

/// Telemetry (monitoring) mode for a deployment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum MonitoringMode {
    /// Automatically use the best available OTLP config: the parent manager's
    /// built-in log store or external OTLP integration (e.g. Axiom, Datadog).
    Auto,
    /// Disable all monitoring — no OTLP logs for containers or worker VMs.
    Off,
}

#[derive(Parser, Debug, Clone)]
#[command(
    about = "Deployment commands",
    long_about = "Manage deployments in the Alien platform.",
    after_help = "EXAMPLES:
    alien deployments list
    alien deployments describe production/api
    alien deployments resources production/api --json
    alien deployments events production/api
    alien deployments wait production/api --for ready --timeout 10m
    alien deployments machines production/api --json
    alien deployments untrack my-sandbox

RELATED COMMANDS:
    alien logs --deployment production/api --follow
    alien debug production/api -- <command>
    alien commands --help"
)]
pub struct DeploymentsArgs {
    #[command(subcommand)]
    pub cmd: DeploymentsCmd,
}

impl DeploymentsArgs {
    pub fn wants_json_output(&self) -> bool {
        matches!(
            &self.cmd,
            DeploymentsCmd::Create { format, .. } if format == "json"
        ) || matches!(
            &self.cmd,
            DeploymentsCmd::Ls { json: true, .. }
                | DeploymentsCmd::Get { json: true, .. }
                | DeploymentsCmd::Resources { json: true, .. }
                | DeploymentsCmd::Events { json: true, .. }
                | DeploymentsCmd::Wait { json: true, .. }
                | DeploymentsCmd::Machines { json: true, .. }
                | DeploymentsCmd::Volumes { json: true, .. }
                | DeploymentsCmd::RestoreVolume { json: true, .. }
                | DeploymentsCmd::CancelVolumeRestore { json: true, .. }
                | DeploymentsCmd::Retry { json: true, .. }
                | DeploymentsCmd::Redeploy { json: true, .. }
                | DeploymentsCmd::Pin { json: true, .. }
                | DeploymentsCmd::SetChannel { json: true, .. }
        )
    }
}

#[derive(Subcommand, Debug, Clone)]
pub enum DeploymentsCmd {
    /// Create a new deployment
    Create {
        /// Deployment display name
        #[arg(long)]
        name: String,

        /// Project ID or name
        #[arg(long)]
        project: String,

        /// Deployment group ID (required)
        #[arg(long)]
        deployment_group: String,

        /// Platform (aws, gcp, azure)
        #[arg(long)]
        platform: String,

        /// Physical-name prefix for generated cloud resources.
        /// Omit to let the manager generate one.
        #[arg(long, value_parser = parse_resource_prefix)]
        resource_prefix: Option<String>,

        /// Plain environment variables in KEY=VALUE format (can be used multiple times)
        #[arg(long)]
        env: Vec<String>,

        /// Secret environment variables in KEY=VALUE format (can be used multiple times)
        #[arg(long)]
        secret: Vec<String>,

        /// Plain environment variables with target functions in KEY=VALUE:pattern1,pattern2 format (can be used multiple times)
        #[arg(long)]
        env_targeted: Vec<String>,

        /// Secret environment variables with target functions in KEY=VALUE:pattern1,pattern2 format (can be used multiple times)
        #[arg(long)]
        secret_targeted: Vec<String>,

        /// Disable push (Operator handles deployments instead of manager)
        #[arg(long)]
        no_push: bool,

        /// Disable heartbeat capability
        #[arg(long)]
        no_heartbeat: bool,

        /// Telemetry / monitoring mode.
        /// "auto" (default) uses the parent manager's built-in log store or external OTLP integration.
        /// "off" disables all monitoring.
        #[arg(long, value_enum, default_value_t = MonitoringMode::Auto)]
        monitoring: MonitoringMode,

        #[command(flatten)]
        network: NetworkArgs,

        /// Output format (json or text)
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// List deployments
    #[command(visible_alias = "list")]
    Ls {
        /// Project to list deployments for (optional, uses linked project by default)
        #[arg(long)]
        project: Option<String>,

        /// Print machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Get deployment details
    #[command(visible_aliases = ["describe", "show", "status"])]
    Get {
        /// Deployment ID, or <deployment-group-name>/<deployment-name>
        id: String,

        /// Print machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Show a safe resource summary without resource configuration or secrets
    Resources {
        /// Deployment ID, or <deployment-group-name>/<deployment-name>
        id: String,

        /// Print stable machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Show deployment lifecycle and configuration events
    Events {
        /// Deployment ID, or <deployment-group-name>/<deployment-name>
        id: String,

        /// Maximum number of recent events to return
        #[arg(long, default_value_t = 20, value_parser = clap::value_parser!(u64).range(1..=100))]
        limit: u64,

        /// Print the API response as machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Wait until a deployment is ready or reaches a terminal state
    Wait {
        /// Deployment ID, or <deployment-group-name>/<deployment-name>
        id: String,

        /// Condition to wait for
        #[arg(long = "for", value_enum, default_value_t = DeploymentWaitCondition::Ready)]
        condition: DeploymentWaitCondition,

        /// Maximum wait duration, such as 30s, 10m, or 1h
        #[arg(long, default_value = "10m", value_parser = parse_wait_duration)]
        timeout: Duration,

        /// Poll interval, such as 1s or 5s
        #[arg(long, default_value = "2s", value_parser = parse_wait_duration)]
        interval: Duration,

        /// Print a stable machine-readable result
        #[arg(long)]
        json: bool,
    },
    /// Show the machines connected to a deployment
    Machines {
        /// Deployment ID, or <deployment-group-name>/<deployment-name>
        id: String,

        /// Print the complete inventory as machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Show container volumes, their latest snapshots and restores
    Volumes {
        /// Deployment ID, or <deployment-group-name>/<deployment-name>
        id: String,

        /// Print machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Replace a replica's persistent volume with a volume made from a snapshot
    ///
    /// The replica is stopped while its volume is swapped. The replaced volume is
    /// snapshotted before it is deleted, so the restore can be undone by restoring
    /// that snapshot.
    RestoreVolume {
        /// Deployment ID, or <deployment-group-name>/<deployment-name>
        id: String,

        /// Container resource that owns the volume
        #[arg(long)]
        resource: String,

        /// Replica ordinal whose volume is replaced
        #[arg(long)]
        ordinal: u32,

        /// Snapshot to restore, as shown by `alien deployments volumes`
        #[arg(long)]
        snapshot: String,

        /// Skip confirmation prompt
        #[arg(long)]
        yes: bool,

        /// Print the restore request as machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Cancel a pending volume restore
    ///
    /// Use it when a restore keeps failing. A restore whose volume was already
    /// swapped still finishes.
    CancelVolumeRestore {
        /// Deployment ID, or <deployment-group-name>/<deployment-name>
        id: String,

        /// Restore request ID, as shown by `alien deployments volumes`
        request_id: String,

        /// Print the cancelled request as machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Delete a deployment
    Delete {
        /// Deployment ID, or <deployment-group-name>/<deployment-name>
        id: String,

        /// Remove only the deployment record, without cleaning up its resources. Use when the
        /// resources are already gone (for example, the CloudFormation stack or Helm release was
        /// deleted) and the deployment is stuck.
        #[arg(long)]
        forget: bool,

        /// Skip confirmation prompt
        #[arg(long)]
        yes: bool,
    },
    /// Retry a deployment
    Retry {
        /// Deployment ID, or <deployment-group-name>/<deployment-name>
        id: String,
        /// Print the updated deployment as machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Redeploy a deployment with the same release
    Redeploy {
        /// Deployment ID, or <deployment-group-name>/<deployment-name>
        id: String,
        /// Wait for this exact redeploy operation to finish (Platform mode only)
        #[arg(long)]
        wait: bool,
        /// Maximum wait duration when --wait is used
        #[arg(long, default_value = "10m", value_parser = parse_wait_duration)]
        timeout: Duration,
        /// Poll interval when --wait is used
        #[arg(long, default_value = "2s", value_parser = parse_wait_duration)]
        interval: Duration,
        /// Print the updated deployment as machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Pin a deployment to a specific release
    Pin {
        /// Deployment ID
        id: String,
        /// Release ID to pin to (omit to unpin and use active release)
        release_id: Option<String>,
        /// Print the pin response as machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Change the channel followed by an unpinned deployment
    SetChannel {
        /// Deployment ID
        id: String,
        /// Channel to follow
        channel: String,
        /// Print the updated deployment as machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Create a deployment token (deployment-scoped API key)
    #[command(visible_alias = "tokens")]
    Token {
        /// Deployment ID
        id: String,
    },
    /// Forget a deployment this machine tracks locally, discarding its stored key
    Untrack {
        /// Deployment name, as passed to `alien deploy --name`
        name: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum DeploymentWaitCondition {
    /// Running with the desired release applied
    Ready,
    /// Any synchronized success or failure state
    Terminal,
    /// Fully deleted
    Deleted,
}

pub(crate) fn parse_resource_prefix(value: &str) -> std::result::Result<String, String> {
    if is_valid_resource_prefix(value) {
        Ok(value.to_string())
    } else {
        Err(RESOURCE_PREFIX_ERROR_MESSAGE.to_string())
    }
}

pub async fn deployments_task(args: DeploymentsArgs, ctx: ExecutionMode) -> Result<()> {
    if let DeploymentsCmd::Delete { yes, .. } = &args.cmd {
        delete_confirmation_mode(*yes)?;
    }

    // Untracking only touches this machine's registry. Demanding a reachable
    // manager first would make an entry unclearable exactly when it is stale.
    if !matches!(args.cmd, DeploymentsCmd::Untrack { .. }) {
        ctx.ensure_ready().await?;
    }

    match args.cmd {
        // Platform mode lists canonical platform records; local modes list manager records.
        DeploymentsCmd::Ls { project, json } => {
            #[cfg(feature = "platform")]
            if ctx.is_platform() {
                return list_platform_deployments_task(&ctx, project.as_deref(), json).await;
            }
            let manager = resolve_manager_client(&ctx, project.as_deref(), !json).await?;
            list_deployments_task(&manager, json).await
        }
        DeploymentsCmd::Get { id, json } => {
            #[cfg(feature = "platform")]
            if ctx.is_platform() {
                let resolved = crate::platform_deployment_resolver::resolve_with_manager(
                    &ctx, &id, None, !json,
                )
                .await?;
                return get_deployment_task(
                    &ctx,
                    &resolved.manager.client,
                    resolved.detail.id.as_str(),
                    Some(&resolved.detail),
                    json,
                )
                .await;
            }
            let manager = resolve_manager_client(&ctx, None, !json).await?;
            get_deployment_task(&ctx, &manager, &id, None, json).await
        }
        DeploymentsCmd::Resources { id, json } => {
            #[cfg(feature = "platform")]
            if ctx.is_platform() {
                let resolved = crate::platform_deployment_resolver::resolve_with_manager(
                    &ctx, &id, None, !json,
                )
                .await?;
                let deployment = resolve_deployment_reference(
                    &resolved.manager.client,
                    &String::from(resolved.detail.id),
                )
                .await?;
                return resources_task(&deployment, json);
            }
            let manager = resolve_manager_client(&ctx, None, !json).await?;
            let deployment = resolve_deployment_reference(&manager, &id).await?;
            resources_task(&deployment, json)
        }
        DeploymentsCmd::Events { id, limit, json } => {
            if !ctx.is_platform() {
                return Err(AlienError::new(ErrorData::ValidationError {
                    field: "command".to_string(),
                    message: "This manager doesn't keep deployment event history. Use `alien logs --deployment` for recent activity.".to_string(),
                }));
            }
            let workspace = ctx.resolve_workspace_with_bootstrap(!json).await?;
            let client = ctx.sdk_client().await?;
            let deployment = crate::platform_deployment_resolver::resolve(
                &ctx, &client, &workspace, &id, None, !json,
            )
            .await?;
            deployment_events_task(
                &client,
                workspace.as_str(),
                &String::from(deployment.id),
                limit,
                json,
            )
            .await
        }
        DeploymentsCmd::Wait {
            id,
            condition,
            timeout,
            interval,
            json,
        } => {
            #[cfg(feature = "platform")]
            if ctx.is_platform() {
                let resolved = crate::platform_deployment_resolver::resolve_with_manager(
                    &ctx, &id, None, !json,
                )
                .await?;
                return wait_for_deployment(
                    &resolved.manager.client,
                    &String::from(resolved.detail.id),
                    condition,
                    timeout,
                    interval,
                    json,
                )
                .await;
            }
            let manager = resolve_manager_client(&ctx, None, !json).await?;
            wait_for_deployment(&manager, &id, condition, timeout, interval, json).await
        }
        DeploymentsCmd::Machines { id, json } => {
            if !ctx.is_platform() {
                return Err(AlienError::new(ErrorData::ValidationError {
                    field: "command".to_string(),
                    message: "This manager doesn't report machine inventory.".to_string(),
                }));
            }
            let workspace = ctx.resolve_workspace_with_bootstrap(!json).await?;
            let client = ctx.sdk_client().await?;
            let deployment = crate::platform_deployment_resolver::resolve(
                &ctx, &client, &workspace, &id, None, !json,
            )
            .await?;
            machines_inventory_task(
                &client,
                workspace.as_str(),
                &String::from(deployment.id),
                json,
            )
            .await
        }
        DeploymentsCmd::Volumes { id, json } => {
            #[cfg(feature = "platform")]
            if ctx.is_platform() {
                let workspace = ctx.resolve_workspace_with_bootstrap(!json).await?;
                let resolved = crate::platform_deployment_resolver::resolve_with_manager(
                    &ctx, &id, None, !json,
                )
                .await?;
                let deployment_id = String::from(resolved.detail.id);
                let deployment =
                    resolve_deployment_reference(&resolved.manager.client, &deployment_id).await?;
                let client = ctx.sdk_client().await?;
                let restores = list_platform_volume_restores(
                    &client,
                    workspace.as_str(),
                    &deployment_id,
                )
                .await?;
                return volumes_task(&deployment, Some(restores), json);
            }
            let manager = resolve_manager_client(&ctx, None, !json).await?;
            let deployment = resolve_deployment_reference(&manager, &id).await?;
            volumes_task(&deployment, None, json)
        }
        DeploymentsCmd::RestoreVolume {
            id,
            resource,
            ordinal,
            snapshot,
            yes,
            json,
        } => {
            if !ctx.is_platform() {
                return Err(AlienError::new(ErrorData::ValidationError {
                    field: "command".to_string(),
                    message: "Volume restores are available on Alien Platform deployments."
                        .to_string(),
                }));
            }
            let confirmation_mode = restore_confirmation_mode(yes, json)?;
            let workspace = ctx.resolve_workspace_with_bootstrap(!json).await?;
            let client = ctx.sdk_client().await?;
            let deployment = crate::platform_deployment_resolver::resolve(
                &ctx, &client, &workspace, &id, None, !json,
            )
            .await?;
            restore_volume_task(
                &client,
                workspace.as_str(),
                &deployment,
                VolumeRestoreTarget {
                    resource,
                    ordinal,
                    snapshot,
                },
                confirmation_mode,
                json,
            )
            .await
        }
        DeploymentsCmd::CancelVolumeRestore {
            id,
            request_id,
            json,
        } => {
            if !ctx.is_platform() {
                return Err(AlienError::new(ErrorData::ValidationError {
                    field: "command".to_string(),
                    message: "Volume restores are available on Alien Platform deployments."
                        .to_string(),
                }));
            }
            let workspace = ctx.resolve_workspace_with_bootstrap(!json).await?;
            let client = ctx.sdk_client().await?;
            let deployment = crate::platform_deployment_resolver::resolve(
                &ctx, &client, &workspace, &id, None, !json,
            )
            .await?;
            cancel_volume_restore_task(
                &client,
                workspace.as_str(),
                &String::from(deployment.id),
                &request_id,
                json,
            )
            .await
        }
        DeploymentsCmd::Delete { id, forget, yes } => {
            let action = if forget {
                DeleteDeploymentAction::Forget
            } else {
                DeleteDeploymentAction::Cleanup
            };
            #[cfg(feature = "platform")]
            if ctx.is_platform() {
                let resolved = crate::platform_deployment_resolver::resolve_with_manager(
                    &ctx, &id, None, true,
                )
                .await?;
                return delete_deployment_task(
                    &resolved.manager.client,
                    &String::from(resolved.detail.id),
                    action,
                    yes,
                )
                .await;
            }
            let manager = resolve_manager_client(&ctx, None, true).await?;
            delete_deployment_task(&manager, &id, action, yes).await
        }
        DeploymentsCmd::Retry { id, json } => {
            #[cfg(feature = "platform")]
            if ctx.is_platform() {
                let resolved = crate::platform_deployment_resolver::resolve_with_manager(
                    &ctx, &id, None, !json,
                )
                .await?;
                return retry_deployment_task(
                    &resolved.manager.client,
                    &String::from(resolved.detail.id),
                    json,
                )
                .await;
            }
            let manager = resolve_manager_client(&ctx, None, !json).await?;
            retry_deployment_task(&manager, &id, json).await
        }
        DeploymentsCmd::Redeploy {
            id,
            wait,
            timeout,
            interval,
            json,
        } => {
            #[cfg(feature = "platform")]
            if ctx.is_platform() {
                let workspace = ctx.resolve_workspace_with_bootstrap(!json).await?;
                let client = ctx.sdk_client().await?;
                let deployment = crate::platform_deployment_resolver::resolve(
                    &ctx, &client, &workspace, &id, None, !json,
                )
                .await?;
                return redeploy_platform_deployment_task(
                    &client,
                    workspace.as_str(),
                    &deployment,
                    PlatformRedeployOptions {
                        wait,
                        timeout,
                        interval,
                        json,
                    },
                )
                .await;
            }
            if wait {
                return Err(AlienError::new(ErrorData::ValidationError {
                    field: "wait".to_string(),
                    message: "Operation-correlated redeploy waiting requires Platform mode."
                        .to_string(),
                }));
            }
            let manager = resolve_manager_client(&ctx, None, !json).await?;
            redeploy_deployment_task(&manager, &id, json).await
        }

        // --- Platform API operations (create, pin, token) ---
        DeploymentsCmd::Create {
            name,
            project,
            deployment_group,
            platform,
            resource_prefix,
            env,
            secret,
            env_targeted,
            secret_targeted,
            no_push,
            no_heartbeat,
            monitoring,
            network: network_args,
            format,
        } => {
            if ctx.is_dev() {
                return Err(AlienError::new(ErrorData::ValidationError {
                    field: "command".to_string(),
                    message:
                        "Use `alien dev deploy --name <deployment> --platform local` to create deployments in dev mode."
                            .to_string(),
                }));
            }

            let client = ctx.sdk_client().await?;
            let workspace_name = ctx.resolve_platform_workspace_context(true).await?.name;

            create_deployment_task(
                &ctx,
                &client,
                &workspace_name,
                &name,
                &project,
                &deployment_group,
                &platform,
                resource_prefix.as_deref(),
                env,
                secret,
                env_targeted,
                secret_targeted,
                no_push,
                no_heartbeat,
                monitoring,
                &network_args,
                &format,
            )
            .await
        }
        DeploymentsCmd::Pin {
            id,
            release_id,
            json,
        } => {
            if ctx.is_dev() {
                return Err(AlienError::new(ErrorData::ValidationError {
                    field: "command".to_string(),
                    message: "`alien dev deployments pin` is not available in `alien dev`."
                        .to_string(),
                }));
            }
            if !ctx.is_platform() {
                let client = resolve_manager_client(&ctx, None, !json).await?;
                let deployment =
                    crate::deployment_resolver::resolve(&client, &id, ctx.is_dev()).await?;
                return crate::commands::release_channels_manager::pin(
                    &client,
                    &deployment.id,
                    release_id.as_deref(),
                    json,
                )
                .await;
            }
            let client = ctx.sdk_client().await?;
            let workspace_name = ctx.resolve_platform_workspace_context(true).await?.name;
            pin_deployment_task(&client, &workspace_name, &id, release_id, json).await
        }
        DeploymentsCmd::SetChannel { id, channel, json } => {
            if !ctx.is_platform() {
                let client = resolve_manager_client(&ctx, None, !json).await?;
                let deployment =
                    crate::deployment_resolver::resolve(&client, &id, ctx.is_dev()).await?;
                return crate::commands::release_channels_manager::set_channel(
                    &client,
                    &deployment.id,
                    &channel,
                    json,
                )
                .await;
            }
            let client = ctx.sdk_client().await?;
            let workspace_name = ctx.resolve_platform_workspace_context(!json).await?.name;
            let body = alien_platform_api::types::SetDeploymentReleaseChannelBody {
                channel: channel.as_str().try_into().map_err(|_| {
                    AlienError::new(ErrorData::ValidationError {
                        field: "channel".to_string(),
                        message: "Channel names must start with a letter and contain only lowercase letters, numbers, and hyphens.".to_string(),
                    })
                })?,
            };
            let response = client
                .set_deployment_release_channel()
                .id(id.as_str())
                .workspace(workspace_name.as_str())
                .body(body)
                .send()
                .await
                .into_sdk_error()
                .context(ErrorData::ApiRequestFailed {
                    message: format!("setting deployment '{id}' to channel '{channel}'"),
                    url: None,
                })?
                .into_inner();
            if json {
                print_json(&response)
            } else {
                println!("Deployment {id} now follows {channel}.");
                Ok(())
            }
        }
        DeploymentsCmd::Token { id } => {
            if ctx.is_dev() {
                return Err(AlienError::new(ErrorData::ValidationError {
                    field: "command".to_string(),
                    message: "`alien dev deployments token` is not available in `alien dev`."
                        .to_string(),
                }));
            }
            if ctx.is_standalone() {
                return Err(AlienError::new(ErrorData::ValidationError {
                    field: "command".to_string(),
                    message: "This manager issues each deployment's token when it registers. \
                              For a backend that calls deployments, create a scoped token with \
                              `alien tokens create --tunnel`."
                        .to_string(),
                }));
            }
            let client = ctx.sdk_client().await?;
            let workspace_name = ctx.resolve_platform_workspace_context(true).await?.name;
            token_deployment_task(&client, &workspace_name, &id).await
        }
        DeploymentsCmd::Untrack { name } => untrack_deployment_task(&name),
    }
}

/// Drop this machine's tracked entry for `name`, so the next deploy registers afresh.
fn untrack_deployment_task(name: &str) -> Result<()> {
    match DeploymentTracker::new()?.remove_deployment(name)? {
        Some(deployment) => println!(
            "{}",
            success_line(&format!(
                "Untracked '{}' ({}).",
                name, deployment.deployment_id
            ))
        ),
        None => println!("No deployment named '{name}' is tracked on this machine."),
    }
    Ok(())
}

async fn deployment_events_task(
    client: &alien_platform_api::Client,
    workspace: &str,
    deployment_id: &str,
    limit: u64,
    json: bool,
) -> Result<()> {
    let limit = NonZeroU64::new(limit).ok_or_else(|| {
        AlienError::new(ErrorData::ValidationError {
            field: "limit".to_string(),
            message: "event limit must be greater than zero".to_string(),
        })
    })?;
    let response = client
        .list_events()
        .workspace(workspace)
        .deployment_id(deployment_id)
        .limit(limit)
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: format!("listing events for deployment '{deployment_id}'"),
            url: None,
        })?
        .into_inner();

    if json {
        return print_json(&response);
    }

    let rows = response
        .items
        .iter()
        .map(|event| {
            EventDisplayRow::try_new(
                String::from(event.id.clone()),
                event.created_at,
                &event.data,
                &event.state,
            )
        })
        .collect::<Result<Vec<_>>>()?;
    print_event_table(&rows);
    Ok(())
}

async fn machines_inventory_task(
    client: &alien_platform_api::Client,
    workspace: &str,
    deployment_id: &str,
    json: bool,
) -> Result<()> {
    let mut response = client
        .list_deployment_machines()
        .id(deployment_id)
        .workspace(workspace)
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: format!("listing machines for deployment '{deployment_id}'"),
            url: None,
        })?
        .into_inner();

    if json {
        return print_json(&response);
    }

    response.machines.sort_by(|left, right| {
        machine_network_rank(right)
            .cmp(&machine_network_rank(left))
            .then_with(|| left.cluster_resource_id.cmp(&right.cluster_resource_id))
            .then_with(|| left.machine_id.cmp(&right.machine_id))
    });
    if response.machines.is_empty() {
        println!("(no machines)");
        return Ok(());
    }

    let mut table = make_table(&[
        "Cluster",
        "Machine",
        "Status",
        "Heartbeat",
        "Network",
        "Peers",
        "Machine agent",
    ]);
    for machine in &response.machines {
        let network = machine
            .network_health
            .map(|health| health.to_string())
            .unwrap_or_else(|| "unknown".to_string());
        let peers = if matches!(
            machine.network_health,
            Some(alien_platform_api::types::DeploymentMachineNetworkHealth::Healthy)
                | Some(alien_platform_api::types::DeploymentMachineNetworkHealth::Degraded)
        ) {
            machine
                .wireguard_mesh
                .as_ref()
                .map(|observation| {
                    format!(
                        "{}/{}",
                        observation.reachable_peer_count, observation.expected_peer_count
                    )
                })
                .unwrap_or_else(|| "—".to_string())
        } else {
            "—".to_string()
        };
        let version = machine
            .horizond_version
            .as_ref()
            .cloned()
            .unwrap_or_else(|| "unknown".to_string());
        table.add_row(vec![
            machine.cluster_resource_id.clone().into(),
            machine.machine_id.clone().into(),
            status_cell(&machine.status),
            machine.last_heartbeat.clone().into(),
            network.into(),
            peers.into(),
            version.into(),
        ]);
    }
    print_table(table);

    for machine in response
        .machines
        .iter()
        .filter(|machine| machine_network_rank(machine) > 0)
    {
        if let Some(observation) = machine.wireguard_mesh.as_ref() {
            if matches!(
                machine.network_health,
                Some(alien_platform_api::types::DeploymentMachineNetworkHealth::Degraded)
            ) && !observation.missing_peer_machine_ids.is_empty()
            {
                println!(
                    "{} missing peers: {}",
                    machine.machine_id,
                    observation.missing_peer_machine_ids.join(", ")
                );
            }
        }
    }

    Ok(())
}

fn machine_network_rank(machine: &alien_platform_api::types::DeploymentMachine) -> u8 {
    network_health_rank(machine.network_health)
}

fn network_health_rank(
    health: Option<alien_platform_api::types::DeploymentMachineNetworkHealth>,
) -> u8 {
    match health {
        Some(alien_platform_api::types::DeploymentMachineNetworkHealth::Degraded) => 2,
        Some(alien_platform_api::types::DeploymentMachineNetworkHealth::Unknown)
        | Some(alien_platform_api::types::DeploymentMachineNetworkHealth::Healthy)
        | None => 0,
    }
}

#[cfg(test)]
mod machines_inventory_tests {
    use super::*;
    use alien_platform_api::types::DeploymentMachineNetworkHealth;

    #[test]
    fn machine_inventory_sorts_network_divergence_first() {
        assert_eq!(
            network_health_rank(Some(DeploymentMachineNetworkHealth::Degraded)),
            2
        );
        assert_eq!(network_health_rank(None), 0);
        assert_eq!(
            network_health_rank(Some(DeploymentMachineNetworkHealth::Healthy)),
            0
        );
    }

    #[test]
    fn machine_inventory_json_mode_routes_errors_as_json() {
        let args = DeploymentsArgs {
            cmd: DeploymentsCmd::Machines {
                id: "deployment".to_string(),
                json: true,
            },
        };
        assert!(args.wants_json_output());
    }

    #[test]
    fn create_json_format_routes_errors_as_json() {
        let args = DeploymentsArgs::try_parse_from([
            "deployments",
            "create",
            "--name",
            "example",
            "--project",
            "project",
            "--deployment-group",
            "group",
            "--platform",
            "aws",
            "--format",
            "json",
        ])
        .expect("create arguments should parse");

        assert!(args.wants_json_output());
    }
}

// ---------------------------------------------------------------------------
// Manager client resolution
// ---------------------------------------------------------------------------

/// Resolve a manager API client for the current execution mode.
///
/// Uses the linked project (or `--project` override) to discover the manager URL
/// in platform mode. In dev/standalone modes, the manager URL is known directly.
///
/// All `deployments` subcommands are Manager API operations that never push
/// container images, so we resolve metadata-only and skip the artifact-repo
/// provisioning step. On platform/dev clusters that provisioning is a cloud
/// API round trip that adds ~10–15s per invocation for a repo we never use.
pub(crate) async fn resolve_manager_client(
    ctx: &ExecutionMode,
    project_override: Option<&str>,
    allow_bootstrap: bool,
) -> Result<alien_manager_api::Client> {
    let (_, project_link) = ctx
        .resolve_project(project_override, allow_bootstrap)
        .await?;
    // The platform parameter is only used in platform mode for build-config
    // discovery; the manager URL is the same regardless of platform.
    let manager_ctx = ctx
        .resolve_manager_metadata_only(&project_link.project_id, "aws")
        .await?;
    Ok(manager_ctx.client)
}

/// Resolve a deployment by name or ID via the shared resolver. The `is_dev`
/// argument only affects the "not found" hint surfaced to the user; the
/// `deployments` subcommand doesn't carry a dev/platform flag at this layer
/// so we conservatively assume non-dev (callers in dev mode go through
/// `dev_helpers`).
async fn resolve_deployment_reference(
    client: &alien_manager_api::Client,
    reference: &str,
) -> Result<DeploymentResponse> {
    crate::deployment_resolver::resolve(client, reference, false).await
}

// ---------------------------------------------------------------------------
// Manager API operations (unified for all modes)
// ---------------------------------------------------------------------------

async fn list_deployments_task(client: &alien_manager_api::Client, json: bool) -> Result<()> {
    let response = client
        .list_deployments()
        .include(vec!["deploymentGroup".to_string()])
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: "listing deployments".to_string(),
            url: None,
        })?
        .into_inner();

    if json {
        return print_json(&response.items);
    }

    if response.items.is_empty() {
        println!("(no deployments)");
        return Ok(());
    }

    let mut table = make_table(&[
        "Reference",
        "ID",
        "Status",
        "Platform",
        "Current release",
        "Desired release",
        "Updated",
    ]);
    for deployment in &response.items {
        table.add_row(vec![
            deployment_reference(deployment).into(),
            deployment.id.clone().into(),
            status_cell(&deployment.status),
            deployment.platform.to_string().into(),
            deployment
                .current_release_id
                .clone()
                .unwrap_or_else(|| "—".to_string())
                .into(),
            desired_release_cell(deployment).into(),
            deployment
                .updated_at
                .clone()
                .unwrap_or_else(|| deployment.created_at.clone())
                .into(),
        ]);
    }
    print_table(table);

    Ok(())
}

#[cfg(feature = "platform")]
async fn list_platform_deployments_task(
    ctx: &ExecutionMode,
    project: Option<&str>,
    json: bool,
) -> Result<()> {
    let (project_id, _) = ctx.resolve_project(project, !json).await?;
    let workspace = ctx.resolve_workspace_query_with_bootstrap(!json).await?;
    let client = ctx.sdk_client().await?;
    let mut deployments = Vec::new();
    let mut cursor = None;

    loop {
        let mut request = client
            .list_deployments()
            .project(project_id.as_str())
            .include(vec![ListDeploymentsIncludeItem::DeploymentGroup]);
        if let Some(workspace) = workspace.as_deref() {
            request = request.workspace(workspace);
        }
        if let Some(next_cursor) = cursor.as_deref() {
            request = request.cursor(next_cursor);
        }
        let response = request
            .send()
            .await
            .into_sdk_error()
            .context(ErrorData::ApiRequestFailed {
                message: "listing deployments".to_string(),
                url: None,
            })?
            .into_inner();
        deployments.extend(response.items);
        cursor = response.next_cursor;
        if cursor.is_none() {
            break;
        }
    }

    if json {
        return print_json(&deployments);
    }

    if deployments.is_empty() {
        println!("(no deployments)");
        return Ok(());
    }

    let mut table = make_table(&[
        "Reference",
        "ID",
        "Status",
        "Platform",
        "Current release",
        "Desired release",
        "Updated",
    ]);
    for deployment in &deployments {
        let status = deployment.status.to_string();
        table.add_row(vec![
            platform_deployment_reference(deployment).into(),
            deployment.id.to_string().into(),
            status_cell(&status),
            deployment.platform.to_string().into(),
            deployment
                .current_release_id
                .as_ref()
                .map(|id| String::from(id.clone()))
                .unwrap_or_else(|| "—".to_string())
                .into(),
            platform_desired_release_cell(deployment).into(),
            deployment.updated_at.to_string().into(),
        ]);
    }
    print_table(table);

    Ok(())
}

fn platform_deployment_reference(deployment: &DeploymentListItemResponse) -> String {
    deployment
        .deployment_group
        .as_ref()
        .map(|group| {
            let group_name = String::from(group.name.clone());
            format!("{group_name}/{}", deployment.name)
        })
        .unwrap_or_else(|| deployment.id.to_string())
}

fn platform_desired_release_cell(deployment: &DeploymentListItemResponse) -> String {
    let Some(desired) = deployment.desired_release_id.as_ref() else {
        return "—".to_string();
    };
    let desired = String::from(desired.clone());
    if deployment
        .current_release_id
        .as_ref()
        .map(|id| String::from(id.clone()))
        == Some(desired.clone())
    {
        "—".to_string()
    } else {
        desired
    }
}

fn deployment_reference(deployment: &DeploymentResponse) -> String {
    deployment
        .deployment_group
        .as_ref()
        .map(|group| format!("{}/{}", group.name, deployment.name))
        .unwrap_or_else(|| deployment.id.clone())
}

fn desired_release_cell(deployment: &DeploymentResponse) -> String {
    let Some(desired) = deployment.desired_release_id.as_deref() else {
        return "—".to_string();
    };
    if deployment.current_release_id.as_deref() == Some(desired) {
        "—".to_string()
    } else {
        desired.to_string()
    }
}

async fn get_deployment_task(
    ctx: &ExecutionMode,
    client: &alien_manager_api::Client,
    reference: &str,
    platform_detail: Option<&DeploymentDetailResponse>,
    json: bool,
) -> Result<()> {
    let deployment = resolve_deployment_reference(client, reference).await?;
    let observed_resources = observed_rollout_resources(ctx, &deployment, !json).await?;

    if json {
        return print_json(&DeploymentDetailOutput {
            deployment: &deployment,
            update_state: platform_detail.and_then(|detail| detail.update_state.as_ref()),
            observed_resources,
        });
    }

    println!(
        "{}",
        contextual_heading("Showing deployment", &deployment.name, &[])
    );
    println!("{} {}", dim_label("ID"), deployment.id);
    println!("{} {}", dim_label("Status"), deployment.status);
    println!("{} {}", dim_label("Platform"), deployment.platform);
    println!("{} {}", dim_label("Group"), deployment.deployment_group_id);
    println!("{} {}", dim_label("Created"), deployment.created_at);

    if let Some(updated_at) = &deployment.updated_at {
        println!("{} {}", dim_label("Updated"), updated_at);
    }

    if let Some(current_release_id) = &deployment.current_release_id {
        println!("{} {}", dim_label("Current release"), current_release_id);
    }

    if let Some(desired_release_id) = &deployment.desired_release_id {
        println!("{} {}", dim_label("Desired release"), desired_release_id);
    }

    if let Some(error) = &deployment.error {
        let error: alien_error::AlienError = serde_json::from_value(error.clone())
            .into_alien_error()
            .context(ErrorData::JsonError {
                operation: "deserialization".to_string(),
                reason: "Failed to convert deployment error".to_string(),
            })?;
        println!("{}", render_human_error(&error));
    }

    if let Some(update_state) = platform_detail.and_then(|detail| detail.update_state.as_ref()) {
        print_deployment_update_state(update_state);
    }

    if let Some(stack_state) = &deployment.stack_state {
        let stack_state: alien_core::StackState = serde_json::from_value(stack_state.clone())
            .into_alien_error()
            .context(ErrorData::JsonError {
                operation: "deserialization".to_string(),
                reason: "Failed to convert deployment stack state".to_string(),
            })?;
        print_stack_resources(&stack_state);
    }

    print_observed_rollout_resources(&observed_resources);

    if let Some(env_info) = &deployment.environment_info {
        let env_str = serde_json::to_string_pretty(env_info)
            .into_alien_error()
            .context(ErrorData::JsonError {
                operation: "serialization".to_string(),
                reason: "Failed to serialize environment info".to_string(),
            })?;
        println!("  Environment Info: {}", env_str);
    }

    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DeploymentDetailOutput<'a> {
    #[serde(flatten)]
    deployment: &'a DeploymentResponse,
    #[serde(skip_serializing_if = "Option::is_none")]
    update_state: Option<&'a DeploymentDetailResponseUpdateState>,
    observed_resources: Vec<ObservedRolloutResource>,
}

fn print_deployment_update_state(update_state: &DeploymentDetailResponseUpdateState) {
    let operations = deployment_update_operations(update_state);
    if operations.is_empty() {
        return;
    }

    println!("{}", heading("Deployment updates"));
    let mut table = make_table(&[
        "Operation",
        "Status",
        "Target release",
        "Reasons",
        "Action required",
    ]);
    for operation in operations {
        table.add_row(vec![
            String::from(operation.id.clone()).into(),
            status_cell(&operation.status.to_string()),
            String::from(operation.target_release_id.clone()).into(),
            operation
                .reasons
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ")
                .into(),
            operation
                .action_required
                .clone()
                .unwrap_or_else(|| "—".to_string())
                .into(),
        ]);
    }
    print_table(table);
}

fn deployment_update_operations(
    update_state: &DeploymentDetailResponseUpdateState,
) -> Vec<&DeploymentUpdateOperationSummaryInner> {
    let mut operations = Vec::new();
    for operation in [
        update_state.active.0.as_ref(),
        update_state.next.0.as_ref(),
        update_state.latest.0.as_ref(),
    ]
    .into_iter()
    .flatten()
    {
        if operations
            .iter()
            .any(|existing: &&DeploymentUpdateOperationSummaryInner| existing.id == operation.id)
        {
            continue;
        }
        operations.push(operation);
    }
    operations
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ObservedRolloutResource {
    resource_id: String,
    resource_type: String,
    desired_image: Option<String>,
    observed_image: Option<String>,
    provider_updated_at: Option<String>,
    observed_at: Option<String>,
    stale: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

async fn observed_rollout_resources(
    ctx: &ExecutionMode,
    deployment: &DeploymentResponse,
    allow_bootstrap: bool,
) -> Result<Vec<ObservedRolloutResource>> {
    if ctx.is_dev() || ctx.is_standalone() {
        return Ok(Vec::new());
    }

    #[cfg(not(feature = "platform"))]
    return Ok(Vec::new());

    #[cfg(feature = "platform")]
    {
        let Some(stack_state) = deployment.stack_state.as_ref() else {
            return Ok(Vec::new());
        };
        let stack_state: alien_core::StackState = serde_json::from_value(stack_state.clone())
            .into_alien_error()
            .context(ErrorData::JsonError {
                operation: "deserialization".to_string(),
                reason: "Failed to inspect deployment resources".to_string(),
            })?;
        let (_, project_link) = ctx.resolve_project(None, allow_bootstrap).await?;
        let workspace = ctx
            .resolve_workspace_query_with_bootstrap(allow_bootstrap)
            .await?;
        let client = ctx.sdk_client().await?;
        let mut observations = Vec::new();

        for (resource_id, resource) in stack_state.resources {
            if resource.resource_type != "container" && resource.resource_type != "daemon" {
                continue;
            }

            let mut request = client
                .get_resource_deployment_detail()
                .area(resource.resource_type.as_str())
                .deployment_id(deployment.id.as_str())
                .resource_id(resource_id.as_str())
                .project(project_link.project_id.as_str());
            if let Some(workspace) = workspace.as_deref() {
                request = request.workspace(workspace);
            }

            match request.send().await.into_sdk_error() {
                Ok(response) => {
                    let detail = response.into_inner();
                    let value = serde_json::to_value(&detail).into_alien_error().context(
                        ErrorData::JsonError {
                            operation: "serialization".to_string(),
                            reason: "Failed to inspect provider observation".to_string(),
                        },
                    )?;
                    observations.push(ObservedRolloutResource {
                        resource_id,
                        resource_type: resource.resource_type,
                        desired_image: detail.deployment.desired_image,
                        observed_image: json_string(
                            &value,
                            &["heartbeat", "heartbeat", "data", "data", "observedImage"],
                        )
                        .or_else(|| {
                            json_string(
                                &value,
                                &["heartbeat", "heartbeat", "data", "data", "image"],
                            )
                        }),
                        provider_updated_at: json_string(
                            &value,
                            &[
                                "heartbeat",
                                "heartbeat",
                                "data",
                                "data",
                                "latestUpdateTimestamp",
                            ],
                        ),
                        observed_at: json_string(&value, &["heartbeat", "observedAt"]),
                        stale: detail.deployment.platform_stale || detail.deployment.provider_stale,
                        error: None,
                    });
                }
                Err(error) => {
                    let desired_image = desired_image_from_resource(&resource);
                    observations.push(ObservedRolloutResource {
                        resource_id,
                        resource_type: resource.resource_type,
                        desired_image,
                        observed_image: None,
                        provider_updated_at: None,
                        observed_at: None,
                        stale: false,
                        error: Some(error.to_string()),
                    });
                }
            }
        }

        Ok(observations)
    }
}

fn json_string(value: &serde_json::Value, path: &[&str]) -> Option<String> {
    path.iter()
        .try_fold(value, |current, segment| current.get(segment))
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
}

fn desired_image_from_resource(resource: &alien_core::StackResourceState) -> Option<String> {
    serde_json::to_value(resource)
        .ok()
        .and_then(|value| json_string(&value, &["config", "code", "image"]))
}

fn print_observed_rollout_resources(resources: &[ObservedRolloutResource]) {
    if resources.is_empty() {
        return;
    }
    println!("{}", heading("Observed rollout state"));
    let mut table = make_table(&[
        "Resource",
        "Type",
        "Desired image",
        "Observed image",
        "Provider updated",
        "State",
    ]);
    for resource in resources {
        let state = observed_rollout_state(resource);
        table.add_row(vec![
            comfy_table::Cell::new(&resource.resource_id),
            comfy_table::Cell::new(&resource.resource_type),
            comfy_table::Cell::new(resource.desired_image.as_deref().unwrap_or("—")),
            comfy_table::Cell::new(resource.observed_image.as_deref().unwrap_or("—")),
            comfy_table::Cell::new(
                resource
                    .provider_updated_at
                    .as_deref()
                    .or(resource.observed_at.as_deref())
                    .unwrap_or("—"),
            ),
            comfy_table::Cell::new(state),
        ]);
    }
    print_table(table);
}

fn observed_rollout_state(resource: &ObservedRolloutResource) -> &'static str {
    if resource.error.is_some() {
        "unavailable"
    } else if resource.stale {
        "stale"
    } else {
        // Missing image identity is not evidence that the rollout reached its target.
        match (&resource.desired_image, &resource.observed_image) {
            (Some(desired), Some(observed)) if desired == observed => "converged",
            (Some(_), Some(_)) => "rollout pending",
            _ => "unavailable",
        }
    }
}

async fn delete_deployment_task(
    client: &alien_manager_api::Client,
    reference: &str,
    action: DeleteDeploymentAction,
    yes: bool,
) -> Result<()> {
    let confirmation_mode = delete_confirmation_mode(yes)?;
    let deployment = resolve_deployment_reference(client, reference).await?;
    let forget = matches!(action, DeleteDeploymentAction::Forget);
    let runtime_metadata: Option<alien_core::RuntimeMetadata> = deployment
        .runtime_metadata
        .as_ref()
        .map(|metadata| serde_json::to_value(metadata).and_then(serde_json::from_value))
        .transpose()
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to deserialize runtime_metadata".to_string(),
        })?;

    println!(
        "{}",
        contextual_heading(
            if forget {
                "Forgetting deployment"
            } else {
                "Deleting deployment"
            },
            &deployment.name,
            &[]
        )
    );
    println!("{} {}", dim_label("ID"), deployment.id);
    println!("{} {}", dim_label("Status"), deployment.status);
    if forget {
        println!(
            "{}",
            dim_label(
                "Only the deployment record is removed. Any resources still running are left in place."
            )
        );
    }

    let question = if forget {
        "Are you sure you want to forget this deployment?"
    } else {
        "Are you sure you want to delete this deployment?"
    };
    if matches!(confirmation_mode, ConfirmationMode::Prompt) && !prompt_confirm(question, false)? {
        println!("{}", dim_label("Deletion cancelled."));
        return Ok(());
    }

    let accepted = client
        .delete_deployment()
        .id(&deployment.id)
        .body(alien_manager_api::types::DeleteDeploymentRequest { action })
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: "deleting deployment".to_string(),
            url: None,
        })?
        .into_inner();

    // The server says what it accepted, e.g. that runtime cleanup is done but the setup (a
    // CloudFormation stack) still has to be deleted, which a fixed message would hide.
    println!("{}", success_line(&format!("{}.", accepted.message)));
    // A forgotten deployment has no record left to read.
    if !forget {
        print_kept_deployer_secrets(&kept_deployer_secrets(runtime_metadata.as_ref()));
        println!(
            "{} {}",
            dim_label("Next"),
            command(&format!("alien deployments get {}", deployment.id))
        );
    }

    Ok(())
}

async fn retry_deployment_task(
    client: &alien_manager_api::Client,
    reference: &str,
    json: bool,
) -> Result<()> {
    let deployment = resolve_deployment_reference(client, reference).await?;

    if !json {
        println!(
            "{}",
            contextual_heading("Retrying deployment", &deployment.name, &[])
        );
        println!("{} {}", dim_label("ID"), deployment.id);
        println!("{} {}", dim_label("Status"), deployment.status);
    }

    client
        .retry_deployment()
        .id(&deployment.id)
        .send()
        .await
        .into_sdk_error_reading_body()
        .await
        .context(ErrorData::ApiRequestFailed {
            message: "retrying deployment".to_string(),
            url: None,
        })?;

    if json {
        return print_json(&DeploymentMutationOutput {
            deployment_id: &deployment.id,
            action: "retry",
            accepted: true,
        });
    }

    println!("{}", success_line("Retry requested."));
    println!(
        "{} {}",
        dim_label("Next"),
        command(&format!("alien deployments get {}", deployment.id))
    );

    Ok(())
}

async fn redeploy_deployment_task(
    client: &alien_manager_api::Client,
    reference: &str,
    json: bool,
) -> Result<()> {
    let deployment = resolve_deployment_reference(client, reference).await?;

    if !json {
        println!(
            "{}",
            contextual_heading("Redeploying deployment", &deployment.name, &[])
        );
        println!("{} {}", dim_label("ID"), deployment.id);
        println!("{} {}", dim_label("Status"), deployment.status);
        if let Some(current_release_id) = &deployment.current_release_id {
            println!("{} {}", dim_label("Current release"), current_release_id);
        }
    }

    client
        .redeploy()
        .id(&deployment.id)
        .send()
        .await
        .into_sdk_error_reading_body()
        .await
        .context(ErrorData::ApiRequestFailed {
            message: "redeploying deployment".to_string(),
            url: None,
        })?;

    if json {
        return print_json(&DeploymentMutationOutput {
            deployment_id: &deployment.id,
            action: "redeploy",
            accepted: true,
        });
    }

    println!("{}", success_line("Redeploy requested."));
    println!(
        "{} {}",
        dim_label("Next"),
        command(&format!("alien deployments get {}", deployment.id))
    );

    Ok(())
}

async fn redeploy_platform_deployment_task(
    client: &alien_platform_api::Client,
    workspace: &str,
    deployment: &DeploymentDetailResponse,
    options: PlatformRedeployOptions,
) -> Result<()> {
    let deployment_id = String::from(deployment.id.clone());
    if !options.json {
        println!(
            "{}",
            contextual_heading("Redeploying deployment", &deployment.name, &[])
        );
        println!("{} {}", dim_label("ID"), deployment_id);
        println!("{} {}", dim_label("Status"), deployment.status);
    }

    let response = client
        .redeploy_deployment()
        .id(deployment_id.as_str())
        .workspace(workspace)
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: "redeploying deployment".to_string(),
            url: None,
        })?
        .into_inner();
    let operation = response.operation.0.ok_or_else(|| {
        AlienError::new(ErrorData::ApiRequestFailed {
            message: format!(
                "Platform accepted redeploy for {deployment_id} without an operation identity"
            ),
            url: None,
        })
    })?;
    let operation_id = String::from(operation.id.clone());

    if !options.wait {
        return print_redeploy_result(&deployment_id, &operation, Duration::ZERO, options.json);
    }

    wait_for_platform_update_operation(
        client,
        operation,
        UpdateOperationWaitOptions {
            workspace,
            deployment_id: &deployment_id,
            operation_id: &operation_id,
            timeout: options.timeout,
            interval: options.interval,
            json: options.json,
        },
    )
    .await
}

async fn wait_for_platform_update_operation(
    client: &alien_platform_api::Client,
    operation: DeploymentUpdateOperationSummaryInner,
    options: UpdateOperationWaitOptions<'_>,
) -> Result<()> {
    let mut print_progress = |progress: &str| {
        if !options.json {
            eprintln!("{} {progress}", dim_label("Redeploy operation:"));
        }
    };
    let (operation, elapsed) =
        await_update_operation(operation, options, &mut print_progress, || async {
            client
                .get_deployment_update_operation()
                .id(options.deployment_id)
                .operation_id(options.operation_id)
                .workspace(options.workspace)
                .send()
                .await
                .into_sdk_error()
                .context(ErrorData::ApiRequestFailed {
                    message: format!(
                        "reading redeploy operation {} for deployment {}",
                        options.operation_id, options.deployment_id
                    ),
                    url: None,
                })?
                .into_inner()
                .0
                .ok_or_else(|| {
                    AlienError::new(ErrorData::ApiRequestFailed {
                        message: format!(
                            "Platform returned an empty redeploy operation {} for deployment {}",
                            options.operation_id, options.deployment_id
                        ),
                        url: None,
                    })
                })
        })
        .await?;
    print_redeploy_result(options.deployment_id, &operation, elapsed, options.json)
}

/// Polls an update operation until it finishes, reporting each change of its
/// status or of the action it waits for through `on_progress`.
async fn await_update_operation<F, Fut>(
    mut operation: DeploymentUpdateOperationSummaryInner,
    options: UpdateOperationWaitOptions<'_>,
    on_progress: &mut dyn FnMut(&str),
    mut poll: F,
) -> Result<(DeploymentUpdateOperationSummaryInner, Duration)>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<DeploymentUpdateOperationSummaryInner>>,
{
    let started = Instant::now();
    let deadline = tokio::time::Instant::now() + options.timeout;
    let mut last_progress = None;
    loop {
        let progress = update_operation_progress(&operation);
        if last_progress.as_ref() != Some(&progress) {
            on_progress(&progress);
            last_progress = Some(progress);
        }

        match update_operation_disposition(operation.status) {
            UpdateOperationDisposition::Succeeded => {
                return Ok((operation, started.elapsed()));
            }
            UpdateOperationDisposition::Failed => {
                return Err(AlienError::new(ErrorData::ApiRequestFailed {
                    message: format!(
                        "Redeploy operation {} for deployment {} reached {}{}",
                        options.operation_id,
                        options.deployment_id,
                        operation.status,
                        operation
                            .action_required
                            .as_deref()
                            .map(|action| format!(": {action}"))
                            .unwrap_or_default()
                    ),
                    url: None,
                }));
            }
            UpdateOperationDisposition::Pending => {}
        }

        if tokio::time::Instant::now() >= deadline {
            return Err(redeploy_wait_timeout_error(options, &operation));
        }
        tokio::time::sleep_until((tokio::time::Instant::now() + options.interval).min(deadline))
            .await;
        if tokio::time::Instant::now() >= deadline {
            return Err(redeploy_wait_timeout_error(options, &operation));
        }
        operation = tokio::time::timeout_at(deadline, poll())
            .await
            .map_err(|_| redeploy_wait_timeout_error(options, &operation))??;
    }
}

/// One line saying where an update operation is and, while it waits on
/// someone, what it waits for: `queued (redeploy): <action required>`.
fn update_operation_progress(operation: &DeploymentUpdateOperationSummaryInner) -> String {
    let reasons = operation
        .reasons
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    let mut progress = operation.status.to_string();
    if !reasons.is_empty() {
        progress.push_str(&format!(" ({reasons})"));
    }
    if let Some(action) = operation.action_required.as_deref() {
        progress.push_str(&format!(": {action}"));
    }
    progress
}

fn redeploy_wait_timeout_error(
    options: UpdateOperationWaitOptions<'_>,
    operation: &DeploymentUpdateOperationSummaryInner,
) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::ApiRequestFailed {
        message: format!(
            "Timed out after {:.1}s waiting for redeploy operation {} on deployment {} (last status: {})",
            options.timeout.as_secs_f64(),
            options.operation_id,
            options.deployment_id,
            update_operation_progress(operation)
        ),
        url: None,
    })
}

fn update_operation_disposition(
    status: DeploymentUpdateOperationStatus,
) -> UpdateOperationDisposition {
    match status {
        DeploymentUpdateOperationStatus::Queued | DeploymentUpdateOperationStatus::Applying => {
            UpdateOperationDisposition::Pending
        }
        DeploymentUpdateOperationStatus::Succeeded => UpdateOperationDisposition::Succeeded,
        DeploymentUpdateOperationStatus::Blocked
        | DeploymentUpdateOperationStatus::Failed
        | DeploymentUpdateOperationStatus::Superseded => UpdateOperationDisposition::Failed,
    }
}

fn print_redeploy_result(
    deployment_id: &str,
    operation: &DeploymentUpdateOperationSummaryInner,
    elapsed: Duration,
    json: bool,
) -> Result<()> {
    let output = DeploymentRedeployOutput {
        deployment_id: deployment_id.to_string(),
        action: "redeploy",
        accepted: true,
        operation_id: String::from(operation.id.clone()),
        operation_status: operation.status.to_string(),
        successful: operation.status == DeploymentUpdateOperationStatus::Succeeded,
        elapsed_seconds: elapsed.as_secs_f64(),
    };
    if json {
        return print_json(&output);
    }
    if output.successful {
        println!(
            "{}",
            success_line(&format!(
                "Redeploy operation {} succeeded in {:.1}s.",
                output.operation_id, output.elapsed_seconds
            ))
        );
    } else {
        println!("{}", success_line("Redeploy requested."));
        println!("{} {}", dim_label("Operation"), output.operation_id);
        println!("{} {}", dim_label("Status"), output.operation_status);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Display helpers
// ---------------------------------------------------------------------------

fn print_stack_resources(stack_state: &alien_core::StackState) {
    println!("{}", heading("Resources"));
    let mut resources: Vec<_> = stack_state.resources.iter().collect();
    resources.sort_by(|(left_name, _), (right_name, _)| left_name.cmp(right_name));

    let mut table = make_table(&["Name", "Type", "Status", "Details"]);
    for (resource_name, resource) in resources {
        table.add_row(vec![
            resource_name.to_string().into(),
            resource.resource_type.clone().into(),
            status_cell(format_resource_status(resource.status)),
            deployment_resource_detail(resource)
                .unwrap_or_else(|| "—".to_string())
                .into(),
        ]);
    }
    print_table(table);
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct ResourceSummary {
    name: String,
    #[serde(rename = "type")]
    resource_type: String,
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    current_machines: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    desired_machines: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    capacity_groups: Option<Vec<CapacityGroupSummary>>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct CapacityGroupSummary {
    id: String,
    current_machines: u32,
    desired_machines: u32,
    instance_type: String,
}

fn resources_task(deployment: &DeploymentResponse, json: bool) -> Result<()> {
    let summaries = deployment
        .stack_state
        .as_ref()
        .map(|value| {
            serde_json::from_value::<alien_core::StackState>(value.clone())
                .into_alien_error()
                .context(ErrorData::JsonError {
                    operation: "deserialization".to_string(),
                    reason: "Failed to inspect deployment resources".to_string(),
                })
                .map(resource_summaries)
        })
        .transpose()?
        .unwrap_or_default();

    if json {
        return print_json(&summaries);
    }
    if summaries.is_empty() {
        println!("{}", dim_label("No resources have been created yet."));
        return Ok(());
    }

    let mut table = make_table(&["Name", "Type", "Status", "Machines", "Details"]);
    for resource in summaries {
        let machines = match (resource.current_machines, resource.desired_machines) {
            (Some(current), Some(desired)) => format!("{current}/{desired}"),
            _ => "—".to_string(),
        };
        table.add_row(vec![
            resource.name,
            resource.resource_type,
            resource.status,
            machines,
            resource.detail.unwrap_or_else(|| "—".to_string()),
        ]);
    }
    print_table(table);
    Ok(())
}

fn resource_summaries(stack_state: alien_core::StackState) -> Vec<ResourceSummary> {
    let mut summaries: Vec<_> = stack_state
        .resources
        .into_iter()
        .map(|(name, resource)| {
            let capacity_groups = resource
                .outputs
                .as_ref()
                .and_then(|outputs| outputs.downcast_ref::<ComputeClusterOutputs>())
                .map(|outputs| {
                    outputs
                        .capacity_group_statuses
                        .iter()
                        .map(|group| CapacityGroupSummary {
                            id: group.group_id.clone(),
                            current_machines: group.current_machines,
                            desired_machines: group.desired_machines,
                            instance_type: group.instance_type.clone(),
                        })
                        .collect::<Vec<_>>()
                });
            let current_machines = capacity_groups
                .as_ref()
                .map(|groups| groups.iter().map(|group| group.current_machines).sum());
            let desired_machines = capacity_groups
                .as_ref()
                .map(|groups| groups.iter().map(|group| group.desired_machines).sum());
            ResourceSummary {
                name,
                resource_type: resource.resource_type.clone(),
                status: format_resource_status(resource.status).to_ascii_lowercase(),
                detail: deployment_resource_detail(&resource),
                current_machines,
                desired_machines,
                capacity_groups,
            }
        })
        .collect();
    summaries.sort_by(|left, right| left.name.cmp(&right.name));
    summaries
}

#[derive(Debug, Clone)]
struct VolumeRestoreTarget {
    resource: String,
    ordinal: u32,
    snapshot: String,
}

fn restore_confirmation_mode(yes: bool, json: bool) -> Result<ConfirmationMode> {
    InteractionMode::current(json).confirmation_mode(
        yes,
        "Restoring a volume replaces a replica's data and needs confirmation. Re-run with `--yes`.",
    )
}

async fn restore_volume_task(
    client: &alien_platform_api::Client,
    workspace: &str,
    deployment: &DeploymentDetailResponse,
    target: VolumeRestoreTarget,
    confirmation_mode: ConfirmationMode,
    json: bool,
) -> Result<()> {
    let deployment_id = String::from(deployment.id.clone());
    if !json {
        println!(
            "{}",
            contextual_heading("Restoring volume", &deployment.name, &[])
        );
        println!("{} {}", dim_label("ID"), deployment_id);
        println!("{} {}", dim_label("Resource"), target.resource);
        println!("{} {}", dim_label("Replica"), target.ordinal);
        println!("{} {}", dim_label("Snapshot"), target.snapshot);
        println!(
            "{}",
            dim_label(
                "The replica is stopped while its volume is swapped. Its current volume is snapshotted before it is deleted."
            )
        );
    }
    if matches!(confirmation_mode, ConfirmationMode::Prompt)
        && !prompt_confirm("Replace this replica's volume?", false)?
    {
        println!("{}", dim_label("Restore cancelled."));
        return Ok(());
    }

    let body = CreateVolumeRestoreRequest {
        resource_id: target.resource.as_str().try_into().map_err(|_| {
            AlienError::new(ErrorData::ValidationError {
                field: "resource".to_string(),
                message: "Resource IDs are 1 to 64 characters.".to_string(),
            })
        })?,
        ordinal: target.ordinal.into(),
        snapshot_id: target.snapshot.as_str().try_into().map_err(|_| {
            AlienError::new(ErrorData::ValidationError {
                field: "snapshot".to_string(),
                message: "Snapshot IDs are 1 to 1024 characters.".to_string(),
            })
        })?,
    };
    let restore = client
        .create_deployment_volume_restore()
        .id(deployment_id.as_str())
        .workspace(workspace)
        .body(body)
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: format!("requesting a volume restore for deployment '{deployment_id}'"),
            url: None,
        })?
        .into_inner();

    if json {
        return print_json(&restore);
    }
    println!("{}", success_line("Volume restore requested."));
    println!("{} {}", dim_label("Request"), String::from(restore.id));
    println!(
        "{} {}",
        dim_label("Next"),
        command(&format!("alien deployments volumes {deployment_id}"))
    );
    Ok(())
}

async fn cancel_volume_restore_task(
    client: &alien_platform_api::Client,
    workspace: &str,
    deployment_id: &str,
    request_id: &str,
    json: bool,
) -> Result<()> {
    let cancelled = client
        .cancel_deployment_volume_restore()
        .id(deployment_id)
        .request_id(request_id)
        .workspace(workspace)
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: format!(
                "cancelling volume restore '{request_id}' of deployment '{deployment_id}'"
            ),
            url: None,
        })?
        .into_inner();

    if json {
        return print_json(&cancelled);
    }
    println!("{}", success_line("Volume restore cancelled."));
    println!(
        "{}",
        dim_label("If the deployment failed on this restore, retry it to bring it back to running.")
    );
    println!(
        "{} {}",
        dim_label("Next"),
        command(&format!("alien deployments retry {deployment_id}"))
    );
    Ok(())
}

async fn list_platform_volume_restores(
    client: &alien_platform_api::Client,
    workspace: &str,
    deployment_id: &str,
) -> Result<Vec<VolumeRestore>> {
    Ok(client
        .list_deployment_volume_restores()
        .id(deployment_id)
        .workspace(workspace)
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: format!("listing volume restores for deployment '{deployment_id}'"),
            url: None,
        })?
        .into_inner()
        .items)
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct VolumeSummary {
    resource: String,
    ordinal: u32,
    volume_id: String,
    zone: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_snapshot_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_snapshot_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_restore: Option<alien_core::VolumeRestoreOutput>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct VolumesOutput {
    volumes: Vec<VolumeSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    restores: Option<Vec<VolumeRestore>>,
}

fn volumes_task(
    deployment: &DeploymentResponse,
    restores: Option<Vec<VolumeRestore>>,
    json: bool,
) -> Result<()> {
    let volumes = deployment
        .stack_state
        .as_ref()
        .map(|value| {
            serde_json::from_value::<alien_core::StackState>(value.clone())
                .into_alien_error()
                .context(ErrorData::JsonError {
                    operation: "deserialization".to_string(),
                    reason: "Failed to inspect deployment volumes".to_string(),
                })
                .map(volume_summaries)
        })
        .transpose()?
        .unwrap_or_default();

    if json {
        return print_json(&VolumesOutput { volumes, restores });
    }

    if volumes.is_empty() {
        println!(
            "{}",
            dim_label("No containers with persistent storage have reported volumes yet.")
        );
    } else {
        let mut table = make_table(&[
            "Resource",
            "Replica",
            "Volume",
            "Zone",
            "Latest snapshot",
            "Snapshot time",
        ]);
        for volume in &volumes {
            table.add_row(vec![
                volume.resource.clone(),
                volume.ordinal.to_string(),
                volume.volume_id.clone(),
                volume.zone.clone(),
                volume
                    .last_snapshot_id
                    .clone()
                    .unwrap_or_else(|| "—".to_string()),
                volume
                    .last_snapshot_at
                    .clone()
                    .unwrap_or_else(|| "—".to_string()),
            ]);
        }
        print_table(table);
    }

    let Some(restores) = restores.filter(|restores| !restores.is_empty()) else {
        return Ok(());
    };
    println!();
    println!("{}", heading("Restores"));
    let mut table = make_table(&[
        "Request", "Resource", "Replica", "Snapshot", "Status", "Requested",
    ]);
    for restore in restores {
        table.add_row(vec![
            String::from(restore.id).into(),
            restore.resource_id.into(),
            restore.ordinal.to_string().into(),
            restore.snapshot_id.into(),
            status_cell(&restore.status.to_string()),
            restore
                .created_at
                .format("%Y-%m-%d %H:%M UTC")
                .to_string()
                .into(),
        ]);
    }
    print_table(table);
    Ok(())
}

fn volume_summaries(stack_state: alien_core::StackState) -> Vec<VolumeSummary> {
    let mut volumes: Vec<_> = stack_state
        .resources
        .iter()
        .flat_map(|(name, resource)| {
            resource
                .outputs
                .as_ref()
                .and_then(|outputs| outputs.downcast_ref::<alien_core::ContainerOutputs>())
                .map(|outputs| outputs.volumes.as_slice())
                .unwrap_or_default()
                .iter()
                .map(move |volume| VolumeSummary {
                    resource: name.clone(),
                    ordinal: volume.ordinal,
                    volume_id: volume.volume_id.clone(),
                    zone: volume.zone.clone(),
                    last_snapshot_id: volume.last_snapshot_id.clone(),
                    last_snapshot_at: volume.last_snapshot_at.clone(),
                    last_restore: volume.last_restore.clone(),
                })
        })
        .collect();
    volumes.sort_by(|left, right| {
        left.resource
            .cmp(&right.resource)
            .then(left.ordinal.cmp(&right.ordinal))
    });
    volumes
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DeploymentWaitOutput {
    deployment_id: String,
    condition: &'static str,
    status: String,
    successful: bool,
    current_release_id: Option<String>,
    desired_release_id: Option<String>,
    elapsed_seconds: f64,
}

async fn wait_for_deployment(
    client: &alien_manager_api::Client,
    reference: &str,
    condition: DeploymentWaitCondition,
    timeout: Duration,
    interval: Duration,
    json: bool,
) -> Result<()> {
    let started = Instant::now();
    let mut last_status = None;
    loop {
        let deployment = match resolve_deployment_reference(client, reference).await {
            Ok(deployment) => deployment,
            // A transient read failure says nothing about the deployment: keep waiting, and
            // report it only if it lasts until the timeout.
            Err(error) if error.retryable && started.elapsed() < timeout => {
                if !json {
                    eprintln!("{} {}", dim_label("Retrying after:"), error.message);
                }
                tokio::time::sleep(interval.min(timeout.saturating_sub(started.elapsed()))).await;
                continue;
            }
            Err(error) => return Err(error),
        };
        let status = parse_deployment_status(&deployment.status)?;
        if !json && last_status.as_deref() != Some(deployment.status.as_str()) {
            eprintln!("{} {}", dim_label("Deployment status:"), deployment.status);
            last_status = Some(deployment.status.clone());
        }

        if wait_condition_met(condition, status, &deployment) {
            let output = DeploymentWaitOutput {
                deployment_id: deployment.id.clone(),
                condition: condition.as_str(),
                status: deployment.status.clone(),
                successful: !status.is_failed(),
                current_release_id: deployment.current_release_id.clone(),
                desired_release_id: deployment.desired_release_id.clone(),
                elapsed_seconds: started.elapsed().as_secs_f64(),
            };
            if json {
                return print_json(&output);
            }
            println!(
                "{}",
                success_line(&format!(
                    "Deployment reached {} ({}) in {:.1}s.",
                    condition.as_str(),
                    deployment.status,
                    output.elapsed_seconds
                ))
            );
            return Ok(());
        }

        if condition == DeploymentWaitCondition::Ready && status.is_synced() {
            return Err(AlienError::new(ErrorData::ApiRequestFailed {
                message: format!(
                    "Deployment {} reached {} before becoming ready",
                    deployment.id, deployment.status
                ),
                url: None,
            }));
        }
        if started.elapsed() >= timeout {
            return Err(AlienError::new(ErrorData::ApiRequestFailed {
                message: format!(
                    "Timed out after {:.1}s waiting for deployment {} to reach {} (last status: {})",
                    timeout.as_secs_f64(),
                    deployment.id,
                    condition.as_str(),
                    deployment.status
                ),
                url: None,
            }));
        }
        tokio::time::sleep(interval.min(timeout.saturating_sub(started.elapsed()))).await;
    }
}

fn parse_deployment_status(value: &str) -> Result<alien_core::DeploymentStatus> {
    serde_json::from_value(serde_json::Value::String(value.to_string()))
        .into_alien_error()
        .context(ErrorData::JsonError {
            operation: "deserialization".to_string(),
            reason: format!("Unknown deployment status '{value}'"),
        })
}

fn wait_condition_met(
    condition: DeploymentWaitCondition,
    status: alien_core::DeploymentStatus,
    deployment: &DeploymentResponse,
) -> bool {
    match condition {
        DeploymentWaitCondition::Ready => {
            status == alien_core::DeploymentStatus::Running
                && deployment
                    .desired_release_id
                    .as_ref()
                    .is_none_or(|desired| deployment.current_release_id.as_ref() == Some(desired))
        }
        DeploymentWaitCondition::Terminal => status.is_synced(),
        DeploymentWaitCondition::Deleted => status == alien_core::DeploymentStatus::Deleted,
    }
}

impl DeploymentWaitCondition {
    fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Terminal => "terminal",
            Self::Deleted => "deleted",
        }
    }
}

fn parse_wait_duration(value: &str) -> std::result::Result<Duration, String> {
    let trimmed = value.trim();
    let unit_start = trimmed
        .find(|character: char| !character.is_ascii_digit())
        .ok_or_else(|| "duration must include a unit: s, m, or h".to_string())?;
    let (amount, unit) = trimmed.split_at(unit_start);
    let amount = amount
        .parse::<u64>()
        .map_err(|error| format!("invalid duration amount: {error}"))?;
    let seconds = match unit {
        "s" => amount,
        "m" => amount.saturating_mul(60),
        "h" => amount.saturating_mul(60 * 60),
        _ => return Err("duration unit must be one of: s, m, h".to_string()),
    };
    if seconds == 0 {
        return Err("duration must be greater than zero".to_string());
    }
    Ok(Duration::from_secs(seconds))
}

fn delete_confirmation_mode(yes: bool) -> Result<ConfirmationMode> {
    InteractionMode::current(false).confirmation_mode(
        yes,
        "Deployment deletion requires a real terminal. Re-run with `--yes`.",
    )
}

// ---------------------------------------------------------------------------
// Platform API operations (create, pin, token — need platform-level features)
// ---------------------------------------------------------------------------

async fn create_deployment_task(
    ctx: &ExecutionMode,
    client: &alien_platform_api::Client,
    workspace: &str,
    name: &str,
    project_name: &str,
    deployment_group_id: &str,
    platform_str: &str,
    resource_prefix: Option<&str>,
    env_vars: Vec<String>,
    secret_vars: Vec<String>,
    env_targeted_vars: Vec<String>,
    secret_targeted_vars: Vec<String>,
    no_push: bool,
    no_heartbeat: bool,
    monitoring: MonitoringMode,
    network_args: &NetworkArgs,
    format: &str,
) -> Result<()> {
    let project = if ctx.is_dev() {
        crate::project_link::ProjectLink::new(
            workspace.to_string(),
            "local-dev".to_string(),
            "local-dev".to_string(),
        )
    } else {
        let http = ctx.auth_http().await?;
        crate::project_link::get_project_by_name(&http, workspace, Some(workspace), project_name)
            .await?
    };

    let platform = match platform_str {
        "aws" => alien_platform_api::types::NewDeploymentRequestPlatform::Aws,
        "gcp" => alien_platform_api::types::NewDeploymentRequestPlatform::Gcp,
        "azure" => alien_platform_api::types::NewDeploymentRequestPlatform::Azure,
        "kubernetes" | "k8s" => alien_platform_api::types::NewDeploymentRequestPlatform::Kubernetes,
        "local" => alien_platform_api::types::NewDeploymentRequestPlatform::Local,
        _ => {
            return Err(AlienError::new(ErrorData::ValidationError {
                field: "platform".to_string(),
                message: format!(
                    "Unknown platform: {}. Valid values: aws, gcp, azure",
                    platform_str
                ),
            })
            .into());
        }
    };

    let mut variables: Vec<alien_platform_api::types::EnvironmentVariableConfig> = Vec::new();

    for env_str in env_vars {
        let (key, value) = parse_env_var(&env_str).ok_or_else(|| {
            AlienError::new(ErrorData::ValidationError {
                field: "env".to_string(),
                message: format!("Invalid format for --env: '{}'. Use KEY=VALUE", env_str),
            })
        })?;

        let name = alien_platform_api::types::EnvironmentVariableConfigName::try_from(key)
            .into_alien_error()
            .context(ErrorData::ValidationError {
                field: "env".to_string(),
                message: format!(
                    "Invalid variable name in --env: '{}'. Must match pattern ^[A-Z_][A-Z0-9_]*$",
                    env_str
                ),
            })?;

        let value = alien_platform_api::types::EnvironmentVariableConfigValue::try_from(value)
            .into_alien_error()
            .context(ErrorData::ValidationError {
                field: "env".to_string(),
                message: format!(
                    "Invalid variable value in --env: '{}'. Must not exceed 10000 characters",
                    env_str
                ),
            })?;

        variables.push(alien_platform_api::types::EnvironmentVariableConfig {
            name,
            value,
            type_: alien_platform_api::types::EnvironmentVariableType::Plain,
            target_resources: None,
        });
    }

    for secret_str in secret_vars {
        let (key, value) = parse_env_var(&secret_str).ok_or_else(|| {
            AlienError::new(ErrorData::ValidationError {
                field: "secret".to_string(),
                message: format!(
                    "Invalid format for --secret: '{}'. Use KEY=VALUE",
                    secret_str
                ),
            })
        })?;

        let name = alien_platform_api::types::EnvironmentVariableConfigName::try_from(key)
            .into_alien_error()
            .context(ErrorData::ValidationError {
                field: "secret".to_string(),
                message: format!("Invalid variable name in --secret: '{}'. Must match pattern ^[A-Z_][A-Z0-9_]*$", secret_str),
            })?;

        let value = alien_platform_api::types::EnvironmentVariableConfigValue::try_from(value)
            .into_alien_error()
            .context(ErrorData::ValidationError {
                field: "secret".to_string(),
                message: format!(
                    "Invalid variable value in --secret: '{}'. Must not exceed 10000 characters",
                    secret_str
                ),
            })?;

        variables.push(alien_platform_api::types::EnvironmentVariableConfig {
            name,
            value,
            type_: alien_platform_api::types::EnvironmentVariableType::Secret,
            target_resources: None,
        });
    }

    for env_targeted_str in env_targeted_vars {
        let (key, value, patterns) =
            parse_targeted_env_var(&env_targeted_str).ok_or_else(|| {
                AlienError::new(ErrorData::ValidationError {
                    field: "env-targeted".to_string(),
                    message: format!(
                        "Invalid format for --env-targeted: '{}'. Use KEY=VALUE:pattern1,pattern2",
                        env_targeted_str
                    ),
                })
            })?;

        let name = alien_platform_api::types::EnvironmentVariableConfigName::try_from(key)
            .into_alien_error()
            .context(ErrorData::ValidationError {
                field: "env-targeted".to_string(),
                message: format!("Invalid variable name in --env-targeted: '{}'. Must match pattern ^[A-Z_][A-Z0-9_]*$", env_targeted_str),
            })?;

        let value = alien_platform_api::types::EnvironmentVariableConfigValue::try_from(value)
            .into_alien_error()
            .context(ErrorData::ValidationError {
                field: "env-targeted".to_string(),
                message: format!("Invalid variable value in --env-targeted: '{}'. Must not exceed 10000 characters", env_targeted_str),
            })?;

        let target_resources: Vec<alien_platform_api::types::EnvironmentVariableConfigTargetResourcesItem> = patterns
            .into_iter()
            .map(|p| {
                alien_platform_api::types::EnvironmentVariableConfigTargetResourcesItem::try_from(p)
                    .into_alien_error()
                    .context(ErrorData::ValidationError {
                        field: "env-targeted".to_string(),
                        message: format!("Invalid target resource pattern in --env-targeted: '{}'. Must match pattern ^[a-zA-Z0-9_-]+(\\*)?$", env_targeted_str),
                    })
            })
            .collect::<Result<Vec<_>>>()?;

        variables.push(alien_platform_api::types::EnvironmentVariableConfig {
            name,
            value,
            type_: alien_platform_api::types::EnvironmentVariableType::Plain,
            target_resources: Some(target_resources),
        });
    }

    for secret_targeted_str in secret_targeted_vars {
        let (key, value, patterns) = parse_targeted_env_var(&secret_targeted_str)
            .ok_or_else(|| AlienError::new(ErrorData::ValidationError {
                field: "secret-targeted".to_string(),
                message: format!("Invalid format for --secret-targeted: '{}'. Use KEY=VALUE:pattern1,pattern2", secret_targeted_str),
            }))?;

        let name = alien_platform_api::types::EnvironmentVariableConfigName::try_from(key)
            .into_alien_error()
            .context(ErrorData::ValidationError {
                field: "secret-targeted".to_string(),
                message: format!("Invalid variable name in --secret-targeted: '{}'. Must match pattern ^[A-Z_][A-Z0-9_]*$", secret_targeted_str),
            })?;

        let value = alien_platform_api::types::EnvironmentVariableConfigValue::try_from(value)
            .into_alien_error()
            .context(ErrorData::ValidationError {
                field: "secret-targeted".to_string(),
                message: format!("Invalid variable value in --secret-targeted: '{}'. Must not exceed 10000 characters", secret_targeted_str),
            })?;

        let target_resources: Vec<alien_platform_api::types::EnvironmentVariableConfigTargetResourcesItem> = patterns
            .into_iter()
            .map(|p| {
                alien_platform_api::types::EnvironmentVariableConfigTargetResourcesItem::try_from(p)
                    .into_alien_error()
                    .context(ErrorData::ValidationError {
                        field: "secret-targeted".to_string(),
                        message: format!("Invalid target resource pattern in --secret-targeted: '{}'. Must match pattern ^[a-zA-Z0-9_-]+(\\*)?$", secret_targeted_str),
                    })
            })
            .collect::<Result<Vec<_>>>()?;

        variables.push(alien_platform_api::types::EnvironmentVariableConfig {
            name,
            value,
            type_: alien_platform_api::types::EnvironmentVariableType::Secret,
            target_resources: Some(target_resources),
        });
    }

    let network_settings =
        network::parse_network_settings(network_args, platform_str).map_err(|e| {
            AlienError::new(ErrorData::ValidationError {
                field: "network".to_string(),
                message: e,
            })
        })?;

    let sdk_network = network_settings
        .map(|ns| {
            let json = serde_json::to_value(&ns).into_alien_error().context(
                ErrorData::ConfigurationError {
                    message: "Failed to serialize network settings".to_string(),
                },
            )?;
            serde_json::from_value(json)
                .into_alien_error()
                .context(ErrorData::ConfigurationError {
                    message: "Failed to convert network settings to SDK type".to_string(),
                })
        })
        .transpose()?;

    let stack_settings = alien_platform_api::types::NewDeploymentRequestStackSettings {
        endpoint_access: network_args
            .endpoint_access
            .map(|access| serde_json::from_value(serde_json::json!(access)))
            .transpose()
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "Failed to convert endpoint access to SDK type".to_string(),
            })?,
        compute: None,
        deployment_model: Some(if no_push {
            alien_platform_api::types::NewDeploymentRequestStackSettingsDeploymentModel::Pull
        } else {
            alien_platform_api::types::NewDeploymentRequestStackSettingsDeploymentModel::Push
        }),
        heartbeats: Some(if no_heartbeat {
            alien_platform_api::types::NewDeploymentRequestStackSettingsHeartbeats::Off
        } else {
            alien_platform_api::types::NewDeploymentRequestStackSettingsHeartbeats::On
        }),
        telemetry: Some(match monitoring {
            MonitoringMode::Off => {
                alien_platform_api::types::NewDeploymentRequestStackSettingsTelemetry::Off
            }
            MonitoringMode::Auto => {
                alien_platform_api::types::NewDeploymentRequestStackSettingsTelemetry::Auto
            }
        }),
        updates: Some(alien_platform_api::types::NewDeploymentRequestStackSettingsUpdates::Auto),
        network: sdk_network,
        domains: None,
        external_bindings: None,
        kubernetes: None,
        public_endpoints: None,
    };

    let environment_variable_count = variables.len();
    let request = NewDeploymentRequest {
        setup_item: None,
        name: alien_platform_api::types::NewDeploymentRequestName::try_from(name.to_string())
            .into_alien_error()
            .context(ErrorData::ValidationError {
                field: "name".to_string(),
                message: "Invalid deployment name format".to_string(),
            })?,
        project: alien_platform_api::types::NewDeploymentRequestProject::try_from(
            project.project_id.to_string(),
        )
        .into_alien_error()
        .context(ErrorData::ValidationError {
            field: "project".to_string(),
            message: "Invalid project format".to_string(),
        })?,
        platform,
        deployment_group_id: Some(deployment_group_id.to_string()),
        resource_prefix: resource_prefix
            .map(TryInto::try_into)
            .transpose()
            .into_alien_error()
            .context(ErrorData::ValidationError {
                field: "resource_prefix".to_string(),
                message: "Invalid resource prefix".to_string(),
            })?,
        stack_settings: Some(stack_settings),
        environment_variables: if variables.is_empty() {
            None
        } else {
            Some(variables)
        },
        manager_id: None,
        operator_permission: None,
        operator_scope: None,
        pinned_release_id: None,
        release_channel: "production"
            .try_into()
            .expect("production is a valid channel"),
        environment_info: None,
        input_values: std::collections::HashMap::new(),
        public_subdomain: None,
        initial_desired_release:
            alien_platform_api::types::NewDeploymentRequestInitialDesiredRelease::Active,
        setup_method: None,
        setup_metadata: None,
        setup_handoff: ::std::default::Default::default(),
    };

    let workspace_param = CreateDeploymentWorkspace::try_from(workspace)
        .into_alien_error()
        .context(ErrorData::ValidationError {
            field: "workspace".to_string(),
            message: "workspace name format is invalid".to_string(),
        })?;

    let response = client
        .create_deployment()
        .workspace(&workspace_param)
        .body(&request)
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: "creating deployment".to_string(),
            url: None,
        })?;

    let deployment_response = response.into_inner();

    let deployment = &deployment_response.deployment;
    let token = deployment_response.token.as_ref();

    if format == "json" {
        let json = serde_json::to_string_pretty(&deployment)
            .into_alien_error()
            .context(ErrorData::JsonError {
                operation: "serialization".to_string(),
                reason: "Failed to serialize deployment response".to_string(),
            })?;
        println!("{}", json);
    } else {
        println!("Deployment created successfully!");
        println!("   ID: {}", *deployment.id);
        println!("   Name: {}", *deployment.name);
        println!("   Project: {}", project.project_name);
        println!("   Platform: {:?}", deployment.platform);
        println!("   Deployment Group: {}", *deployment.deployment_group_id);
        println!("   Status: {:?}", deployment.status);
        if environment_variable_count > 0 {
            println!(
                "   Environment Variables: {} configured",
                environment_variable_count
            );
        }
        if let Some(token) = token {
            println!("   Token: {}", token);
        }
    }

    Ok(())
}

async fn pin_deployment_task(
    client: &alien_platform_api::Client,
    workspace: &str,
    deployment_id: &str,
    release_id: Option<String>,
    json: bool,
) -> Result<()> {
    let workspace_param = GetDeploymentWorkspace::try_from(workspace)
        .into_alien_error()
        .context(ErrorData::ValidationError {
            field: "workspace".to_string(),
            message: "workspace name format is invalid".to_string(),
        })?;
    let deployment_id_param = GetDeploymentId::try_from(deployment_id)
        .into_alien_error()
        .context(ErrorData::ValidationError {
            field: "deployment_id".to_string(),
            message: "deployment ID format is invalid".to_string(),
        })?;
    let response = client
        .get_deployment()
        .id(&deployment_id_param)
        .workspace(&workspace_param)
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: "retrieving deployment details".to_string(),
            url: None,
        })?;
    let deployment = response.into_inner();

    if !json {
        println!(
            "{}",
            contextual_heading("Pinning deployment", deployment.name.as_ref(), &[])
        );
        println!("{} {}", dim_label("ID"), *deployment.id);
        println!("{} {:?}", dim_label("Status"), deployment.status);

        if let Some(ref release_id_str) = release_id {
            println!("Pinning to release: {}", release_id_str);
        } else {
            println!("Unpinning deployment (will use active release)");
        }
    }

    let pin_workspace_param = PinDeploymentReleaseWorkspace::try_from(workspace)
        .into_alien_error()
        .context(ErrorData::ValidationError {
            field: "workspace".to_string(),
            message: "workspace name format is invalid".to_string(),
        })?;
    let pin_deployment_id_param = PinDeploymentReleaseId::try_from(deployment_id)
        .into_alien_error()
        .context(ErrorData::ValidationError {
            field: "deployment_id".to_string(),
            message: "deployment ID format is invalid".to_string(),
        })?;

    let release_id_param = release_id
        .map(|id| {
            PinReleaseRequestReleaseId::try_from(id.clone())
                .into_alien_error()
                .context(ErrorData::ValidationError {
                    field: "release_id".to_string(),
                    message: format!("Invalid release ID format: '{}'", id),
                })
        })
        .transpose()?;

    let pin_request = PinReleaseRequest {
        release_id: release_id_param,
    };

    let response = client
        .pin_deployment_release()
        .id(&pin_deployment_id_param)
        .workspace(&pin_workspace_param)
        .body(&pin_request)
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: "pinning deployment release".to_string(),
            url: None,
        })?;
    let pin_response = response.into_inner();

    if json {
        return print_json(&pin_response);
    }

    println!("{}", success_line("Deployment pin updated."));
    println!("{} {}", dim_label("Message"), pin_response.message);

    Ok(())
}

async fn token_deployment_task(
    client: &alien_platform_api::Client,
    workspace: &str,
    deployment_id: &str,
) -> Result<()> {
    let workspace_param = CreateDeploymentTokenWorkspace::try_from(workspace)
        .into_alien_error()
        .context(ErrorData::ValidationError {
            field: "workspace".to_string(),
            message: "workspace name format is invalid".to_string(),
        })?;
    let deployment_id_param = CreateDeploymentTokenId::try_from(deployment_id)
        .into_alien_error()
        .context(ErrorData::ValidationError {
            field: "deployment_id".to_string(),
            message: "deployment ID format is invalid".to_string(),
        })?;

    let description = alien_platform_api::types::CreateDeploymentTokenRequestDescription::try_from(
        "CLI-generated deployment token",
    )
    .into_alien_error()
    .context(ErrorData::ValidationError {
        field: "description".to_string(),
        message: "Invalid description".to_string(),
    })?;

    let request = CreateDeploymentTokenRequest {
        description: Some(description),
        expires_at: None,
    };

    let response = client
        .create_deployment_token()
        .id(&deployment_id_param)
        .workspace(&workspace_param)
        .body(&request)
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: "creating deployment token".to_string(),
            url: None,
        })?;

    let token_response = response.into_inner();

    println!("{}", token_response.token);
    eprintln!("(deployment: {})", token_response.deployment_id);

    Ok(())
}

// ---------------------------------------------------------------------------
// Env var parsing helpers
// ---------------------------------------------------------------------------

/// Parse KEY=VALUE format
fn parse_env_var(input: &str) -> Option<(String, String)> {
    let parts: Vec<&str> = input.splitn(2, '=').collect();
    if parts.len() == 2 {
        Some((parts[0].to_string(), parts[1].to_string()))
    } else {
        None
    }
}

/// Parse KEY=VALUE:pattern1,pattern2 format
fn parse_targeted_env_var(input: &str) -> Option<(String, String, Vec<String>)> {
    let parts: Vec<&str> = input.splitn(2, '=').collect();
    if parts.len() != 2 {
        return None;
    }

    let key = parts[0].to_string();
    let value_and_patterns = parts[1];

    // Split from the right by ':' to get VALUE and patterns
    // This handles values with colons (like URLs: https://...)
    let value_parts: Vec<&str> = value_and_patterns.rsplitn(2, ':').collect();
    if value_parts.len() != 2 {
        return None;
    }

    // rsplitn returns parts in reverse order, so [1] is value, [0] is patterns
    let value = value_parts[1].to_string();
    let patterns_str = value_parts[0];

    let patterns: Vec<String> = patterns_str
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    if patterns.is_empty() {
        return None;
    }

    Some((key, value, patterns))
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_manager_api::types::{DeploymentGroupMinimal, Platform};
    use alien_platform_api::types::{DeploymentUpdateOperationStatus, DeploymentUpdateReason};
    use axum::{
        extract::State,
        http::StatusCode,
        response::{IntoResponse, Response},
        routing::get,
        Json, Router,
    };
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    fn update_operation(
        id: &str,
        status: DeploymentUpdateOperationStatus,
        action_required: Option<&str>,
    ) -> DeploymentUpdateOperationSummaryInner {
        DeploymentUpdateOperationSummaryInner::builder()
            .id(id)
            .status(status)
            .reasons(vec![DeploymentUpdateReason::Release])
            .target_release_id(format!("rel_{}", "a".repeat(28)))
            .changed_keys(Vec::<String>::new())
            .action_required(action_required.map(str::to_string))
            .requested_at(chrono::Utc::now())
            .try_into()
            .expect("valid operation")
    }

    fn redeploy_operation(
        id: &str,
        status: DeploymentUpdateOperationStatus,
    ) -> DeploymentUpdateOperationSummaryInner {
        DeploymentUpdateOperationSummaryInner::builder()
            .id(id)
            .status(status)
            .reasons(vec![DeploymentUpdateReason::Redeploy])
            .changed_keys(Vec::<String>::new())
            .requested_at(chrono::Utc::now())
            .target_release_id(format!("rel_{}", "a".repeat(28)))
            .try_into()
            .expect("valid operation")
    }

    fn deployment_with_releases(
        current: Option<&str>,
        desired: Option<&str>,
    ) -> DeploymentResponse {
        DeploymentResponse::builder()
            .id("dep_1")
            .name("example")
            .status("running")
            .platform(Platform::Aws)
            .deployment_group_id("dg_1")
            .deployment_protocol_version(1u32)
            .project_id("prj_1")
            .workspace_id("ws_1")
            .retry_requested(false)
            .created_at("2026-07-16T00:00:00Z")
            .current_release_id(current.map(str::to_string))
            .desired_release_id(desired.map(str::to_string))
            .try_into()
            .expect("valid deployment response")
    }

    /// Serve `/v1/deployments/dep_1` on loopback, answering each request with the next
    /// status from `statuses` (the last one repeats).
    async fn fake_manager(statuses: Vec<u16>) -> alien_manager_api::Client {
        type ManagerState = (Arc<Vec<u16>>, Arc<AtomicUsize>);

        async fn read_deployment(State((statuses, calls)): State<ManagerState>) -> Response {
            let call = calls.fetch_add(1, Ordering::SeqCst);
            let status = statuses[call.min(statuses.len() - 1)];
            if status == 200 {
                Json(deployment_with_releases(Some("rel_a"), Some("rel_a"))).into_response()
            } else {
                StatusCode::from_u16(status)
                    .expect("valid status")
                    .into_response()
            }
        }

        let app = Router::new()
            .route("/v1/deployments/dep_1", get(read_deployment))
            .with_state((Arc::new(statuses), Arc::new(AtomicUsize::new(0))));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind loopback");
        let addr = listener.local_addr().expect("local addr");
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve");
        });
        alien_manager_api::Client::new(&format!("http://{addr}"))
    }

    /// Serve the deployment read and delete routes on loopback, recording each delete body.
    async fn fake_delete_manager() -> (
        alien_manager_api::Client,
        Arc<std::sync::Mutex<Vec<serde_json::Value>>>,
    ) {
        type Bodies = Arc<std::sync::Mutex<Vec<serde_json::Value>>>;

        async fn read_deployment() -> Response {
            Json(deployment_with_releases(Some("rel_a"), Some("rel_a"))).into_response()
        }

        async fn delete_deployment(
            State(bodies): State<Bodies>,
            Json(body): Json<serde_json::Value>,
        ) -> Response {
            let action = body["action"].clone();
            bodies.lock().expect("bodies lock").push(body);
            (
                StatusCode::ACCEPTED,
                Json(serde_json::json!({
                    "action": action,
                    "cleanupRequired": false,
                    "message": "Deployment record deleted",
                })),
            )
                .into_response()
        }

        let bodies: Bodies = Arc::default();
        let app = Router::new()
            .route("/v1/deployments/dep_1", get(read_deployment))
            .route(
                "/v1/deployments/dep_1/delete",
                axum::routing::post(delete_deployment),
            )
            .with_state(bodies.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind loopback");
        let addr = listener.local_addr().expect("local addr");
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve");
        });
        (
            alien_manager_api::Client::new(&format!("http://{addr}")),
            bodies,
        )
    }

    #[tokio::test]
    async fn delete_forget_asks_the_server_to_drop_only_the_record() {
        let parsed = DeploymentsArgs::try_parse_from([
            "deployments",
            "delete",
            "dep_1",
            "--forget",
            "--yes",
        ])
        .expect("--forget parses");
        let DeploymentsCmd::Delete { id, forget, yes } = parsed.cmd else {
            panic!("expected the delete subcommand");
        };
        assert!(forget && yes);

        let (client, bodies) = fake_delete_manager().await;
        delete_deployment_task(&client, &id, DeleteDeploymentAction::Forget, yes)
            .await
            .expect("forget should be accepted");
        delete_deployment_task(&client, &id, DeleteDeploymentAction::Cleanup, yes)
            .await
            .expect("cleanup should be accepted");

        assert_eq!(
            *bodies.lock().expect("bodies lock"),
            vec![
                serde_json::json!({ "action": "forget" }),
                serde_json::json!({ "action": "cleanup" }),
            ]
        );
    }

    #[tokio::test]
    async fn waiting_rides_out_a_transient_read_failure() {
        let client = fake_manager(vec![500, 200]).await;

        wait_for_deployment(
            &client,
            "dep_1",
            DeploymentWaitCondition::Ready,
            Duration::from_secs(5),
            Duration::from_millis(10),
            true,
        )
        .await
        .expect("a 500 followed by a running deployment should satisfy the wait");
    }

    #[tokio::test]
    async fn waiting_rides_out_an_unreachable_manager_until_the_timeout() {
        // Nothing listens on a port that was bound and released, so every read is a
        // connection error.
        let addr = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind loopback")
            .local_addr()
            .expect("local addr");
        let client = alien_manager_api::Client::new(&format!("http://{addr}"));
        let timeout = Duration::from_millis(300);
        let started = Instant::now();

        let error = wait_for_deployment(
            &client,
            "dep_1",
            DeploymentWaitCondition::Ready,
            timeout,
            Duration::from_millis(10),
            true,
        )
        .await
        .expect_err("an unreachable manager must fail once the wait times out");

        assert!(
            started.elapsed() >= timeout,
            "a connection error must not end the wait early: {}",
            error.message
        );
        assert!(error.retryable, "{}", error.message);
    }

    #[tokio::test]
    async fn a_missing_deployment_is_reported_as_not_found() {
        let client = fake_manager(vec![404]).await;

        let error = wait_for_deployment(
            &client,
            "dep_1",
            DeploymentWaitCondition::Ready,
            Duration::from_secs(5),
            Duration::from_millis(10),
            true,
        )
        .await
        .expect_err("a 404 must end the wait");

        assert_eq!(
            error.message,
            "API request failed: Deployment 'dep_1' was not found."
        );
        assert!(!error.retryable);
    }

    #[tokio::test]
    async fn a_read_failure_is_not_reported_as_not_found() {
        let client = fake_manager(vec![500]).await;

        let error = wait_for_deployment(
            &client,
            "dep_1",
            DeploymentWaitCondition::Ready,
            Duration::from_millis(100),
            Duration::from_millis(10),
            true,
        )
        .await
        .expect_err("a persistent 500 must fail once the wait times out");

        assert_eq!(
            error.message,
            "API request failed: Failed to read deployment 'dep_1'"
        );
        assert_eq!(error.http_status_code, Some(500));
    }

    #[test]
    fn desired_release_column_only_shows_an_unreached_target() {
        assert_eq!(
            desired_release_cell(&deployment_with_releases(Some("rel_a"), Some("rel_b"))),
            "rel_b"
        );
        assert_eq!(
            desired_release_cell(&deployment_with_releases(Some("rel_a"), Some("rel_a"))),
            "—"
        );
        assert_eq!(
            desired_release_cell(&deployment_with_releases(Some("rel_a"), None)),
            "—"
        );
        assert_eq!(
            desired_release_cell(&deployment_with_releases(None, Some("rel_first"))),
            "rel_first"
        );
    }

    #[test]
    fn blocked_update_remains_visible_beside_the_serving_release() {
        let operation_id = format!("duop_{}", "b".repeat(28));
        let blocked = update_operation(
            &operation_id,
            DeploymentUpdateOperationStatus::Blocked,
            Some("Update the deployment setup before retrying"),
        );
        let state = DeploymentDetailResponseUpdateState::builder()
            .active(None::<DeploymentUpdateOperationSummaryInner>)
            .next(None::<DeploymentUpdateOperationSummaryInner>)
            .latest(Some(blocked))
            .try_into()
            .expect("valid update state");

        let visible = deployment_update_operations(&state);

        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].id.as_str(), operation_id);
        assert_eq!(visible[0].status, DeploymentUpdateOperationStatus::Blocked);
        assert_eq!(
            visible[0].action_required.as_deref(),
            Some("Update the deployment setup before retrying")
        );
    }

    #[test]
    fn deployment_reference_includes_the_group_name() {
        let mut deployment = deployment_with_releases(Some("rel_a"), None);
        deployment.deployment_group = Some(
            DeploymentGroupMinimal::builder()
                .id("dg_1")
                .name("production")
                .try_into()
                .expect("valid deployment group"),
        );

        assert_eq!(deployment_reference(&deployment), "production/example");
    }

    #[test]
    fn deployment_reference_falls_back_to_the_id_without_group_data() {
        let deployment = deployment_with_releases(Some("rel_a"), None);

        assert_eq!(deployment_reference(&deployment), "dep_1");
    }

    #[test]
    fn agent_friendly_status_and_resource_commands_parse() {
        let status =
            DeploymentsArgs::try_parse_from(["deployments", "status", "production/api", "--json"])
                .expect("status alias should parse");
        assert!(matches!(status.cmd, DeploymentsCmd::Get { json: true, .. }));

        let resources = DeploymentsArgs::try_parse_from([
            "deployments",
            "resources",
            "production/api",
            "--json",
        ])
        .expect("resource summary should parse");
        assert!(matches!(
            resources.cmd,
            DeploymentsCmd::Resources { json: true, .. }
        ));

        let events = DeploymentsArgs::try_parse_from([
            "deployments",
            "events",
            "production/api",
            "--limit",
            "50",
            "--json",
        ])
        .expect("event history should parse");
        assert!(matches!(
            events.cmd,
            DeploymentsCmd::Events {
                limit: 50,
                json: true,
                ..
            }
        ));

        let wait = DeploymentsArgs::try_parse_from([
            "deployments",
            "wait",
            "production/api",
            "--for",
            "ready",
            "--timeout",
            "5m",
            "--json",
        ])
        .expect("wait command should parse");
        assert!(matches!(
            wait.cmd,
            DeploymentsCmd::Wait {
                condition: DeploymentWaitCondition::Ready,
                json: true,
                ..
            }
        ));

        let redeploy = DeploymentsArgs::try_parse_from([
            "deployments",
            "redeploy",
            "production/api",
            "--wait",
            "--timeout",
            "45m",
            "--interval",
            "5s",
            "--json",
        ])
        .expect("operation-correlated redeploy wait should parse");
        assert!(matches!(
            redeploy.cmd,
            DeploymentsCmd::Redeploy {
                wait: true,
                timeout,
                interval,
                json: true,
                ..
            } if timeout == Duration::from_secs(45 * 60)
                && interval == Duration::from_secs(5)
        ));
    }

    #[test]
    fn deployment_events_rejects_an_api_limit_above_the_maximum() {
        let result = DeploymentsArgs::try_parse_from([
            "deployments",
            "events",
            "production/api",
            "--limit",
            "101",
        ]);

        assert!(result.is_err(), "limits above the API maximum must fail");
    }

    #[test]
    fn ready_wait_requires_release_convergence() {
        let running = parse_deployment_status("running").expect("known status");
        let converged = deployment_with_releases(Some("rel_a"), Some("rel_a"));
        let pending = deployment_with_releases(Some("rel_a"), Some("rel_b"));
        assert!(wait_condition_met(
            DeploymentWaitCondition::Ready,
            running,
            &converged
        ));
        assert!(!wait_condition_met(
            DeploymentWaitCondition::Ready,
            running,
            &pending
        ));
    }

    #[test]
    fn wait_duration_rejects_zero_and_missing_units() {
        assert_eq!(parse_wait_duration("2m").unwrap(), Duration::from_secs(120));
        assert!(parse_wait_duration("0s").is_err());
        assert!(parse_wait_duration("30").is_err());
    }

    #[test]
    fn update_operation_disposition_is_terminal_only_for_final_states() {
        assert_eq!(
            update_operation_disposition(DeploymentUpdateOperationStatus::Queued),
            UpdateOperationDisposition::Pending
        );
        assert_eq!(
            update_operation_disposition(DeploymentUpdateOperationStatus::Applying),
            UpdateOperationDisposition::Pending
        );
        assert_eq!(
            update_operation_disposition(DeploymentUpdateOperationStatus::Succeeded),
            UpdateOperationDisposition::Succeeded
        );
        for status in [
            DeploymentUpdateOperationStatus::Blocked,
            DeploymentUpdateOperationStatus::Failed,
            DeploymentUpdateOperationStatus::Superseded,
        ] {
            assert_eq!(
                update_operation_disposition(status),
                UpdateOperationDisposition::Failed
            );
        }
    }

    #[tokio::test]
    async fn correlated_wait_ignores_general_readiness_until_exact_operation_succeeds() {
        let operation_id = format!("duop_{}", "a".repeat(28));
        let options = UpdateOperationWaitOptions {
            workspace: "test-workspace",
            deployment_id: "dep_test",
            operation_id: &operation_id,
            timeout: Duration::from_secs(1),
            interval: Duration::from_millis(1),
            json: true,
        };
        let mut observations = std::collections::VecDeque::from([
            redeploy_operation(&operation_id, DeploymentUpdateOperationStatus::Applying),
            redeploy_operation(&operation_id, DeploymentUpdateOperationStatus::Succeeded),
        ]);
        let initial = redeploy_operation(&operation_id, DeploymentUpdateOperationStatus::Queued);

        let (completed, _) = await_update_operation(initial, options, &mut |_| {}, || {
            std::future::ready(Ok(observations
                .pop_front()
                .expect("wait should consume the next exact operation observation")))
        })
        .await
        .expect("exact operation should succeed");

        assert_eq!(completed.status, DeploymentUpdateOperationStatus::Succeeded);
        assert!(observations.is_empty(), "wait must poll through applying");
    }

    #[tokio::test]
    async fn correlated_wait_reports_what_the_operation_is_waiting_for() {
        let operation_id = format!("duop_{}", "a".repeat(28));
        let options = UpdateOperationWaitOptions {
            workspace: "test-workspace",
            deployment_id: "dep_test",
            operation_id: &operation_id,
            timeout: Duration::from_secs(1),
            interval: Duration::from_millis(1),
            json: true,
        };
        let waiting = "Write deployer secret 'Database password'";
        let mut observations = std::collections::VecDeque::from([
            update_operation(
                &operation_id,
                DeploymentUpdateOperationStatus::Queued,
                Some(waiting),
            ),
            update_operation(
                &operation_id,
                DeploymentUpdateOperationStatus::Queued,
                Some(waiting),
            ),
            update_operation(
                &operation_id,
                DeploymentUpdateOperationStatus::Applying,
                None,
            ),
            update_operation(
                &operation_id,
                DeploymentUpdateOperationStatus::Succeeded,
                None,
            ),
        ]);
        let initial =
            update_operation(&operation_id, DeploymentUpdateOperationStatus::Queued, None);
        let mut progress = Vec::new();

        await_update_operation(
            initial,
            options,
            &mut |line| progress.push(line.to_string()),
            || std::future::ready(Ok(observations.pop_front().expect("next observation"))),
        )
        .await
        .expect("operation should succeed");

        assert_eq!(
            progress,
            vec![
                "queued (release)".to_string(),
                format!("queued (release): {waiting}"),
                "applying (release)".to_string(),
                "succeeded (release)".to_string(),
            ],
            "each change is reported once, with the action the operation waits for"
        );
    }

    #[tokio::test]
    async fn correlated_wait_timeout_names_what_the_operation_waits_for() {
        let operation_id = format!("duop_{}", "a".repeat(28));
        let options = UpdateOperationWaitOptions {
            workspace: "test-workspace",
            deployment_id: "dep_test",
            operation_id: &operation_id,
            timeout: Duration::from_millis(20),
            interval: Duration::from_millis(1),
            json: true,
        };
        let waiting = "Write deployer secret 'Database password'";
        let initial = update_operation(
            &operation_id,
            DeploymentUpdateOperationStatus::Queued,
            Some(waiting),
        );

        let error = await_update_operation(initial, options, &mut |_| {}, || {
            std::future::ready(Ok(update_operation(
                &operation_id,
                DeploymentUpdateOperationStatus::Queued,
                Some(waiting),
            )))
        })
        .await
        .expect_err("a queued operation must time out");

        assert!(error.message.contains("Timed out"), "{}", error.message);
        assert!(error.message.contains(waiting), "{}", error.message);
    }

    #[tokio::test]
    async fn correlated_wait_bounds_a_slow_poll_by_the_absolute_deadline() {
        let operation_id = format!("duop_{}", "a".repeat(28));
        let timeout = Duration::from_millis(25);
        let options = UpdateOperationWaitOptions {
            workspace: "test-workspace",
            deployment_id: "dep_test",
            operation_id: &operation_id,
            timeout,
            interval: Duration::from_millis(1),
            json: true,
        };
        let initial = redeploy_operation(&operation_id, DeploymentUpdateOperationStatus::Queued);
        let started = Instant::now();

        let error = await_update_operation(initial, options, &mut |_| {}, || async {
            tokio::time::sleep(Duration::from_secs(5)).await;
            Ok(redeploy_operation(
                &operation_id,
                DeploymentUpdateOperationStatus::Succeeded,
            ))
        })
        .await
        .expect_err("slow HTTP-equivalent poll must time out");

        assert_eq!(error.code, "API_REQUEST_FAILED");
        assert!(error.message.contains("Timed out"));
        assert!(
            started.elapsed() < Duration::from_millis(250),
            "absolute timeout must include the poll await"
        );
    }

    #[test]
    fn volume_restore_commands_parse_in_their_documented_form() {
        let restore = DeploymentsArgs::try_parse_from([
            "deployments",
            "restore-volume",
            "production/db",
            "--resource",
            "postgres",
            "--ordinal",
            "1",
            "--snapshot",
            "snap-0123",
        ])
        .expect("restore-volume should parse");
        assert!(matches!(
            restore.cmd,
            DeploymentsCmd::RestoreVolume { ref resource, ordinal: 1, ref snapshot, yes: false, json: false, .. }
                if resource == "postgres" && snapshot == "snap-0123"
        ));

        let cancel = DeploymentsArgs::try_parse_from([
            "deployments",
            "cancel-volume-restore",
            "production/db",
            "vrst_0123",
            "--json",
        ])
        .expect("cancel-volume-restore should parse");
        assert!(cancel.wants_json_output());
        assert!(matches!(
            cancel.cmd,
            DeploymentsCmd::CancelVolumeRestore { ref request_id, json: true, .. }
                if request_id == "vrst_0123"
        ));
    }

    fn container_config(id: &str) -> serde_json::Value {
        let container = alien_core::Container::new(id.to_string())
            .cluster("compute".to_string())
            .code(alien_core::ContainerCode::Image {
                image: "postgres:16".to_string(),
            })
            .cpu(alien_core::ResourceSpec {
                min: "1".to_string(),
                desired: "1".to_string(),
            })
            .memory(alien_core::ResourceSpec {
                min: "1Gi".to_string(),
                desired: "1Gi".to_string(),
            })
            .permissions("execution".to_string())
            .build();
        serde_json::to_value(alien_core::Resource::new(container))
            .expect("container config should serialize")
    }

    #[test]
    fn volume_summaries_list_every_container_volume_in_order() {
        let stack_state: alien_core::StackState = serde_json::from_value(serde_json::json!({
            "platform": "aws",
            "resourcePrefix": "test",
            "resources": {
                "web": {
                    "type": "container",
                    "status": "running",
                    "config": container_config("web"),
                    "outputs": {
                        "type": "container",
                        "name": "web",
                        "status": "running",
                        "currentReplicas": 1,
                        "desiredReplicas": 1,
                        "internalDns": "web.svc",
                        "replicas": []
                    }
                },
                "db": {
                    "type": "container",
                    "status": "running",
                    "config": container_config("db"),
                    "outputs": {
                        "type": "container",
                        "name": "db",
                        "status": "running",
                        "currentReplicas": 2,
                        "desiredReplicas": 2,
                        "internalDns": "db.svc",
                        "replicas": [],
                        "volumes": [
                            { "ordinal": 1, "volumeId": "vol-1", "zone": "us-east-1b" },
                            {
                                "ordinal": 0,
                                "volumeId": "vol-0",
                                "zone": "us-east-1a",
                                "lastSnapshotId": "snap-0",
                                "lastSnapshotAt": "2026-10-05T00:00:00Z",
                                "lastRestore": {
                                    "requestId": "vrst_1",
                                    "snapshotId": "snap-old",
                                    "replacedVolumeSnapshotId": "snap-replaced",
                                    "completedAt": "2026-10-04T00:00:00Z"
                                }
                            }
                        ]
                    }
                },
                "jobs": {
                    "type": "queue",
                    "status": "running",
                    "config": { "type": "queue", "id": "jobs" }
                }
            }
        }))
        .expect("stack state should deserialize");

        let volumes = volume_summaries(stack_state);
        assert_eq!(
            volumes
                .iter()
                .map(|volume| (volume.resource.as_str(), volume.ordinal, volume.volume_id.as_str()))
                .collect::<Vec<_>>(),
            vec![("db", 0, "vol-0"), ("db", 1, "vol-1")]
        );
        assert_eq!(volumes[0].last_snapshot_id.as_deref(), Some("snap-0"));
        assert_eq!(
            volumes[0]
                .last_restore
                .as_ref()
                .map(|restore| restore.request_id.as_str()),
            Some("vrst_1")
        );
        assert_eq!(volumes[1].last_snapshot_id, None);
    }

    #[test]
    fn resource_summary_json_has_no_arbitrary_configuration_fields() {
        let summary = ResourceSummary {
            name: "compute".to_string(),
            resource_type: "compute".to_string(),
            status: "ready".to_string(),
            detail: None,
            current_machines: Some(2),
            desired_machines: Some(3),
            capacity_groups: Some(vec![CapacityGroupSummary {
                id: "general".to_string(),
                current_machines: 2,
                desired_machines: 3,
                instance_type: "example-instance".to_string(),
            }]),
        };
        let value = serde_json::to_value(summary).expect("summary should serialize");
        assert_eq!(value["currentMachines"], 2);
        assert_eq!(value["desiredMachines"], 3);
        assert!(value.get("config").is_none());
        assert!(value.get("outputs").is_none());
        assert!(value.get("internalState").is_none());
    }

    #[test]
    fn observed_rollout_state_distinguishes_mismatch_stale_and_unavailable() {
        let mut resource = ObservedRolloutResource {
            resource_id: "api".to_string(),
            resource_type: "container".to_string(),
            desired_image: Some("registry.example/api:desired".to_string()),
            observed_image: Some("registry.example/api:previous".to_string()),
            provider_updated_at: Some("2026-07-16T00:00:00Z".to_string()),
            observed_at: Some("2026-07-16T00:00:01Z".to_string()),
            stale: false,
            error: None,
        };
        assert_eq!(observed_rollout_state(&resource), "rollout pending");

        resource.observed_image = resource.desired_image.clone();
        assert_eq!(observed_rollout_state(&resource), "converged");

        for (desired_image, observed_image) in [
            (Some("registry.example/api:desired"), None),
            (None, Some("registry.example/api:desired")),
            (None, None),
        ] {
            resource.desired_image = desired_image.map(str::to_string);
            resource.observed_image = observed_image.map(str::to_string);
            assert_eq!(observed_rollout_state(&resource), "unavailable");
        }

        resource.desired_image = Some("registry.example/api:desired".to_string());
        resource.observed_image = Some("registry.example/api:previous".to_string());
        resource.stale = true;
        assert_eq!(observed_rollout_state(&resource), "stale");

        resource.error = Some("heartbeat unavailable".to_string());
        assert_eq!(observed_rollout_state(&resource), "unavailable");
    }

    #[test]
    fn test_parse_targeted_env_var_with_url() {
        let result =
            parse_targeted_env_var("PLATFORM_BASE_URL=https://example.com:deployment-manager");
        assert!(result.is_some());
        let (key, value, patterns) = result.unwrap();
        assert_eq!(key, "PLATFORM_BASE_URL");
        assert_eq!(value, "https://example.com");
        assert_eq!(patterns, vec!["deployment-manager"]);
    }

    #[test]
    fn test_parse_targeted_env_var_with_port() {
        let result = parse_targeted_env_var("API_URL=https://example.com:8080:api-*");
        assert!(result.is_some());
        let (key, value, patterns) = result.unwrap();
        assert_eq!(key, "API_URL");
        assert_eq!(value, "https://example.com:8080");
        assert_eq!(patterns, vec!["api-*"]);
    }

    #[test]
    fn test_parse_targeted_env_var_multiple_patterns() {
        let result = parse_targeted_env_var("DATABASE_URL=postgres://localhost:api-*,worker");
        assert!(result.is_some());
        let (key, value, patterns) = result.unwrap();
        assert_eq!(key, "DATABASE_URL");
        assert_eq!(value, "postgres://localhost");
        assert_eq!(patterns, vec!["api-*", "worker"]);
    }

    #[test]
    fn test_parse_targeted_env_var_simple() {
        let result = parse_targeted_env_var("LOG_LEVEL=info:api-*");
        assert!(result.is_some());
        let (key, value, patterns) = result.unwrap();
        assert_eq!(key, "LOG_LEVEL");
        assert_eq!(value, "info");
        assert_eq!(patterns, vec!["api-*"]);
    }
}
