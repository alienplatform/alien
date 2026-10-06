//! Destroy command — tears down a deployment's cloud resources via the manager.
//!
//! Flow:
//! 1. Resolve the deployment from a token, local tracking, or authenticated manager state
//! 2. Discover manager (resolve_manager)
//! 3. Request deletion via manager
//! 4. Run deletion step loop (acquire → step → reconcile → release)

use crate::commands::deploy::deployment_manager_http_client;
use crate::deployment_tracking::{
    validate_token, DeploymentToken, DeploymentTracker, TrackedDeployment,
};
use crate::error::{ErrorData, Result};
use crate::execution_context::{ExecutionMode, ManagerContext};
use crate::ui::{command, contextual_heading, dim_label, success_line, FixedSteps};
use alien_core::{
    ClientConfig, DeployerSecretReport, DeploymentConfig, DeploymentState, DeploymentStatus,
    Platform,
};
use alien_deployment::loop_contract::{LoopOperation, LoopOutcome};
use alien_deployment::manager_api_transport::{
    acquire_setup_delete_deployment, combine_operation_and_finalization, final_reconcile,
    ManagerApiTransport, SetupDeleteAcquireOutcome,
};
use alien_deployment::runner::{preserve_semantic_failure, RunnerPolicy, RunnerResult};
use alien_error::{AlienError, Context, ContextError, IntoAlienError};
use alien_infra::ClientConfigExt;
use alien_manager_api::SdkResultExt as _;
#[cfg(feature = "platform")]
use alien_platform_api::SdkResultExt as _;
use clap::Parser;
use std::str::FromStr;
use tracing::info;
use uuid::Uuid;

#[derive(Parser, Debug, Clone)]
#[command(
    about = "Destroy resources from a deployment",
    long_about = "Destroy a deployment's cloud resources via the manager.",
    after_help = "EXAMPLES:
    # Destroy a tracked deployment
    alien destroy --name production --platform aws

    # Force-destroy (skip resource teardown)
    alien destroy --name production --platform aws --force"
)]
pub struct DestroyArgs {
    /// Deployment API key for setup teardown (optional with local tracking or an Alien login)
    #[arg(long)]
    pub token: Option<String>,

    /// Deployment ID, <group>/<name>, or a unique deployment name in the selected project
    #[arg(long)]
    pub name: String,

    /// Target platform (required by `alien destroy`; `alien dev destroy` is always local)
    #[arg(long)]
    pub platform: Option<String>,

    /// Force-destroy: skip resource teardown and delete the record immediately.
    #[arg(long)]
    pub force: bool,
}

pub async fn destroy_task(args: DestroyArgs, ctx: ExecutionMode) -> Result<()> {
    info!("Starting destroy command");
    println!("{}", contextual_heading("Destroying", &args.name, &[]));
    let steps = FixedSteps::new(&["Resolve deployment", "Resolve manager", "Delete resources"]);
    steps.activate(0, Some(format!("Deployment {}", args.name)));

    let platform_name = args.platform.clone().ok_or_else(|| {
        AlienError::new(ErrorData::ValidationError {
            field: "platform".to_string(),
            message: "--platform is required".to_string(),
        })
    })?;
    let platform = Platform::from_str(&platform_name).map_err(|e| {
        AlienError::new(ErrorData::ValidationError {
            field: "platform".to_string(),
            message: e,
        })
    })?;

    let tracked = if args.token.is_none() {
        DeploymentTracker::new()?
            .get_deployment(&args.name)
            .cloned()
    } else {
        None
    };
    let (tracked_deployment, manager_ctx) =
        resolve_destroy_target(&args, &ctx, &platform_name, tracked).await?;
    steps.complete(
        0,
        Some(format!(
            "{} ({})",
            args.name, tracked_deployment.deployment_id
        )),
    );
    steps.activate(1, Some("Discovering manager...".to_string()));

    steps.complete(1, Some(format!("Manager: {}", manager_ctx.manager_url)));

    destroy_tracked_deployment(&args, platform, &tracked_deployment, manager_ctx, steps).await
}

/// Resolve cloud cleanup independently of the machine that installed it.
async fn resolve_destroy_target(
    args: &DestroyArgs,
    ctx: &ExecutionMode,
    platform: &str,
    tracked: Option<TrackedDeployment>,
) -> Result<(TrackedDeployment, ManagerContext)> {
    if let Some(token) = &args.token {
        let DeploymentToken::Deployment {
            deployment_id,
            project_id,
            workspace_id,
        } = validate_token(token, &ctx.base_url()).await?
        else {
            return Err(AlienError::new(ErrorData::ValidationError {
                field: "token".to_string(),
                message: "Destroy requires a deployment-scoped token".to_string(),
            }));
        };
        let token_ctx = match ctx {
            ExecutionMode::Standalone { server_url, .. } => ExecutionMode::Standalone {
                server_url: server_url.clone(),
                api_key: token.clone(),
            },
            #[cfg(feature = "platform")]
            ExecutionMode::Platform {
                base_url,
                no_browser,
                workspace,
                project,
                ..
            } => ExecutionMode::Platform {
                base_url: base_url.clone(),
                api_key: Some(token.clone()),
                no_browser: *no_browser,
                workspace: workspace.clone(),
                project: project.clone(),
            },
            ExecutionMode::Dev { .. } => ctx.clone(),
        };
        let manager = token_ctx
            .resolve_manager_metadata_only(&project_id, platform)
            .await?;
        let deployment = manager
            .client
            .get_deployment()
            .id(&deployment_id)
            .send()
            .await
            .into_sdk_error()
            .context(ErrorData::ConfigurationError {
                message: "Failed to resolve the deployment token's target".to_string(),
            })?
            .into_inner();
        let reference_matches = if let Some((group, name)) = args.name.split_once('/') {
            let parent = deployment.deployment_group.as_ref().ok_or_else(|| {
                AlienError::new(ErrorData::ValidationError {
                    field: "name".to_string(),
                    message: format!(
                        "This manager does not expose the token target's group name. Use --name {deployment_id} with this token"
                    ),
                })
            })?;
            group == parent.name.as_str() && name == deployment.name.as_str()
        } else {
            args.name == deployment_id || args.name == deployment.name.as_str()
        };
        if !reference_matches {
            return Err(AlienError::new(ErrorData::ValidationError {
                field: "name".to_string(),
                message: "The supplied token belongs to a different deployment".to_string(),
            }));
        }
        if deployment.platform.to_string() != platform {
            return Err(AlienError::new(ErrorData::ValidationError {
                field: "platform".to_string(),
                message: format!("Deployment '{}' uses {}", args.name, deployment.platform),
            }));
        }
        return Ok((
            TrackedDeployment {
                name: deployment.name.to_string(),
                deployment_id,
                project_id,
                workspace_id,
                api_key: token.clone(),
            },
            manager,
        ));
    }
    if let Some(tracked) = tracked {
        let manager = ctx
            .resolve_manager_metadata_only(&tracked.project_id, platform)
            .await?;
        return Ok((tracked, manager));
    }

    let (project_id, _) = ctx.resolve_project(None, true).await?;
    #[cfg(feature = "platform")]
    let platform_target = if ctx.is_platform() {
        let reference = if args.name.starts_with("dep_") || args.name.contains('/') {
            args.name.clone()
        } else {
            resolve_platform_name(ctx, &args.name, &project_id).await?
        };
        Some(
            crate::platform_deployment_resolver::resolve_with_manager(ctx, &reference, None, true)
                .await?,
        )
    } else {
        None
    };
    #[cfg(feature = "platform")]
    let (manager, reference) = if let Some(target) = platform_target {
        (target.manager, String::from(target.detail.id))
    } else {
        (
            ctx.resolve_manager_metadata_only(&project_id, platform)
                .await?,
            args.name.clone(),
        )
    };
    #[cfg(not(feature = "platform"))]
    let (manager, reference) = (
        ctx.resolve_manager_metadata_only(&project_id, platform)
            .await?,
        args.name.clone(),
    );
    let deployment = if reference.starts_with("dep_") || reference.contains('/') {
        crate::deployment_resolver::resolve(&manager.client, &reference, ctx.is_dev()).await?
    } else {
        resolve_untracked_name(ctx, &manager.client, &reference, &project_id).await?
    };
    if deployment.project_id.as_str() != project_id {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "project".to_string(),
            message: "The deployment belongs to a different project".to_string(),
        }));
    }
    if deployment.platform.to_string() != platform {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "platform".to_string(),
            message: format!("Deployment '{}' uses {}", args.name, deployment.platform),
        }));
    }
    let api_key = if args.force {
        String::new()
    } else {
        #[cfg(feature = "platform")]
        {
            if !ctx.is_platform() {
                return Err(AlienError::new(ErrorData::ValidationError {
                    field: "token".to_string(),
                    message: "Supply --token with the deployment token to run setup teardown"
                        .to_string(),
                }));
            }
            let workspace = ctx.resolve_workspace_query_with_bootstrap(true).await?;
            let client = ctx.sdk_client().await?;
            let mut request = client.create_deployment_token().id(deployment.id.as_str());
            if let Some(workspace) = workspace.as_deref() {
                request = request.workspace(workspace);
            }
            request
                .body(&alien_platform_api::types::CreateDeploymentTokenRequest {
                    description: Some("CLI setup teardown".try_into().into_alien_error().context(
                        ErrorData::ConfigurationError {
                            message: "Invalid teardown token description".to_string(),
                        },
                    )?),
                    expires_at: Some(chrono::Utc::now() + chrono::Duration::hours(24)),
                })
                .send()
                .await
                .into_sdk_error()
                .context(ErrorData::ConfigurationError {
                    message: "Failed to create a deployment-scoped setup teardown token"
                        .to_string(),
                })?
                .into_inner()
                .token
        }
        #[cfg(not(feature = "platform"))]
        {
            return Err(AlienError::new(ErrorData::ValidationError {
                field: "token".to_string(),
                message: "Supply --token with the deployment token to run setup teardown"
                    .to_string(),
            }));
        }
    };
    Ok((
        TrackedDeployment {
            name: deployment.name.to_string(),
            deployment_id: deployment.id,
            project_id,
            workspace_id: deployment.workspace_id,
            api_key,
        },
        manager,
    ))
}

#[cfg(feature = "platform")]
async fn resolve_platform_name(
    ctx: &ExecutionMode,
    name: &str,
    project_id: &str,
) -> Result<String> {
    let mut ids: Vec<String> = Vec::new();
    let client = ctx.sdk_client().await?;
    let workspace = ctx.resolve_workspace_query_with_bootstrap(true).await?;
    let mut cursor: Option<String> = None;
    loop {
        let mut request = client.list_deployments().project(project_id);
        if let Some(workspace) = workspace.as_deref() {
            request = request.workspace(workspace);
        }
        if let Some(cursor) = cursor.as_deref() {
            request = request.cursor(cursor);
        }
        let page = request
            .send()
            .await
            .into_sdk_error()
            .context(ErrorData::ConfigurationError {
                message: format!("Failed to resolve deployment '{name}'"),
            })?
            .into_inner();
        ids.extend(
            page.items
                .into_iter()
                .filter(|d| d.name.as_str() == name)
                .map(|d| d.id.to_string()),
        );
        cursor = page.next_cursor;
        if cursor.is_none() {
            break;
        }
    }
    if ids.len() != 1 {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "name".to_string(),
            message: format!("Expected one deployment named '{name}' in the selected project; found {}. Supply an ID or <group>/<name>", ids.len()),
        }));
    }
    Ok(ids.remove(0))
}

/// Legacy `--name` remains usable, but only for an exact, unique match in the project.
async fn resolve_untracked_name(
    ctx: &ExecutionMode,
    manager: &alien_manager_api::Client,
    name: &str,
    project_id: &str,
) -> Result<alien_manager_api::types::DeploymentResponse> {
    let mut ids: Vec<String> = Vec::new();
    #[cfg(feature = "platform")]
    if ctx.is_platform() {
        let client = ctx.sdk_client().await?;
        let workspace = ctx.resolve_workspace_query_with_bootstrap(true).await?;
        let mut cursor: Option<String> = None;
        loop {
            let mut request = client.list_deployments().project(project_id);
            if let Some(workspace) = workspace.as_deref() {
                request = request.workspace(workspace);
            }
            if let Some(cursor) = cursor.as_deref() {
                request = request.cursor(cursor);
            }
            let page = request
                .send()
                .await
                .into_sdk_error()
                .context(ErrorData::ConfigurationError {
                    message: format!("Failed to resolve deployment '{name}'"),
                })?
                .into_inner();
            ids.extend(
                page.items
                    .into_iter()
                    .filter(|d| d.name.as_str() == name)
                    .map(|d| d.id.to_string()),
            );
            cursor = page.next_cursor;
            if cursor.is_none() {
                break;
            }
        }
    }
    if !ctx.is_platform() {
        let page = manager
            .list_deployments()
            .send()
            .await
            .into_sdk_error()
            .context(ErrorData::ConfigurationError {
                message: format!("Failed to resolve deployment '{name}'"),
            })?
            .into_inner();
        ids.extend(
            page.items
                .into_iter()
                .filter(|d| d.name.as_str() == name && d.project_id == project_id)
                .map(|d| d.id.to_string()),
        );
        if page.next_cursor.is_some() {
            return Err(AlienError::new(ErrorData::ValidationError {
                field: "name".to_string(),
                message: "This manager returned an incomplete list. Supply a deployment ID or <group>/<name> instead".to_string(),
            }));
        }
    }
    if ids.len() != 1 {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "name".to_string(),
            message: if ids.is_empty() {
                format!("Deployment '{name}' was not found in the selected project. Check `alien deployments ls`")
            } else {
                format!("Multiple deployments are named '{name}'. Supply a deployment ID or <group>/<name> instead")
            },
        }));
    }
    crate::deployment_resolver::resolve(manager, &ids[0], ctx.is_dev()).await
}

/// Drive local cleanup through the same setup-authority handoff as cloud cleanup.
pub(crate) async fn destroy_local_target(
    port: u16,
    deployment: alien_manager_api::types::DeploymentResponse,
) -> Result<()> {
    let manager = ExecutionMode::Dev { port }
        .resolve_manager_metadata_only(&deployment.project_id, "local")
        .await?;
    let tracked = TrackedDeployment {
        name: deployment.name,
        deployment_id: deployment.id,
        project_id: deployment.project_id,
        workspace_id: deployment.workspace_id,
        api_key: String::new(),
    };
    let args = DestroyArgs {
        name: tracked.name.clone(),
        platform: Some("local".to_string()),
        token: None,
        force: false,
    };
    let steps = FixedSteps::new(&["Resolve deployment", "Resolve manager", "Delete resources"]);
    destroy_tracked_deployment(&args, Platform::Local, &tracked, manager, steps).await
}

/// Delete a tracked deployment through its resolved manager.
async fn destroy_tracked_deployment(
    args: &DestroyArgs,
    platform: Platform,
    tracked_deployment: &TrackedDeployment,
    manager_ctx: ManagerContext,
    steps: FixedSteps,
) -> Result<()> {
    // Manager discovery may authenticate as the user, but teardown drives the
    // manager's sync endpoints, which only accept the deployment's own token.
    let operator_client = manager_ctx.client;
    let manager_client = if platform == Platform::Local && manager_ctx.auth_token.is_none() {
        operator_client.clone()
    } else {
        alien_manager_api::Client::new_with_client(
            &manager_ctx.manager_url,
            deployment_manager_http_client(
                &tracked_deployment.api_key,
                manager_ctx.workspace.as_deref(),
            )?,
        )
    };

    // Step 3: Delete via manager
    steps.activate(2, Some(tracked_deployment.deployment_id.clone()));

    if args.force {
        operator_client
            .delete_deployment()
            .id(&tracked_deployment.deployment_id)
            .body(alien_manager_api::types::DeleteDeploymentRequest {
                action: alien_manager_api::types::DeleteDeploymentAction::Forget,
            })
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "Failed to force-delete deployment record".to_string(),
            })?;

        steps.complete(2, Some("Force-deleted".to_string()));
        drop(steps);
        eprintln!();
        println!("{}", success_line("Deployment force-deleted."));
        return Ok(());
    }

    let pre_delete_deployment = manager_client
        .get_deployment()
        .id(&tracked_deployment.deployment_id)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to get deployment from manager".to_string(),
        })?
        .into_inner();

    if matches!(
        pre_delete_deployment.status.as_str(),
        "teardown-required" | "teardown-failed"
    ) {
        steps.activate(2, Some("Setup teardown required".to_string()));
    } else {
        // Request deletion
        operator_client
            .delete_deployment()
            .id(&tracked_deployment.deployment_id)
            .body(alien_manager_api::types::DeleteDeploymentRequest {
                action: alien_manager_api::types::DeleteDeploymentAction::Cleanup,
            })
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "Failed to request deployment deletion".to_string(),
            })?;
    }

    // A setup-capable runtime can delete the record immediately. Wait for its
    // completion or acquire the handoff before fetching state for our loop.
    let pre_delete_stack_settings: alien_core::StackSettings = pre_delete_deployment
        .stack_settings
        .map(serde_json::from_value)
        .transpose()
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to deserialize deployment settings before teardown".to_string(),
        })?
        .unwrap_or_default();
    // Acquire → step loop → reconcile → release
    let session = format!("cli-destroy-{}", Uuid::new_v4());
    let acquire_outcome = acquire_setup_delete_deployment(
        &manager_client,
        &tracked_deployment.deployment_id,
        &session,
        pre_delete_stack_settings.deployment_model,
    )
    .await
    .context(ErrorData::ConfigurationError {
        message: "Failed to acquire deployment lock for deletion".to_string(),
    })?;
    let execution_claim = match acquire_outcome {
        SetupDeleteAcquireOutcome::Acquired { execution_claim } => execution_claim,
        SetupDeleteAcquireOutcome::AlreadyDeleted => return Ok(()),
    };

    // Run the deletion step loop
    let client_config =
        ClientConfig::from_std_env(platform)
            .await
            .context(ErrorData::ConfigurationError {
                message: format!("Failed to build client config for platform {:?}", platform),
            })?;

    // Fetch deployment state
    let deployment = manager_client
        .get_deployment()
        .id(&tracked_deployment.deployment_id)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to get deployment from manager".to_string(),
        })?
        .into_inner();

    let status: DeploymentStatus =
        serde_json::from_value(serde_json::Value::String(deployment.status.clone()))
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: format!("Unknown deployment status: {}", deployment.status),
            })?;

    let mut current = DeploymentState {
        status,
        platform,
        current_release: None,
        target_release: None,
        stack_state: deployment
            .stack_state
            .map(serde_json::from_value)
            .transpose()
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "Failed to deserialize stack_state".to_string(),
            })?,
        error: None,
        environment_info: deployment
            .environment_info
            .map(serde_json::from_value)
            .transpose()
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "Failed to deserialize environment_info".to_string(),
            })?,
        runtime_metadata: deployment
            .runtime_metadata
            .map(|rm| serde_json::to_value(rm).and_then(serde_json::from_value))
            .transpose()
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "Failed to deserialize runtime_metadata".to_string(),
            })?,
        retry_requested: deployment.retry_requested,
        protocol_version: alien_core::DEPLOYMENT_PROTOCOL_VERSION,
    };

    // Deployer secrets were written by the deployer, not Alien, so deletion
    // keeps them. Name each one with the command that deletes it.
    let kept_deployer_secrets = kept_deployer_secrets(current.runtime_metadata.as_ref());

    let stack_settings: alien_core::StackSettings = deployment
        .stack_settings
        .map(serde_json::from_value)
        .transpose()
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to deserialize stack_settings".to_string(),
        })?
        .unwrap_or_default();

    let mut config: DeploymentConfig = serde_json::from_value(serde_json::json!({
        "stackSettings": serde_json::to_value(&stack_settings).unwrap_or_default(),
        "environmentVariables": {
            "variables": [],
            "hash": "",
            "createdAt": ""
        }
    }))
    .into_alien_error()
    .context(ErrorData::ConfigurationError {
        message: "Failed to construct deployment config".to_string(),
    })?;

    // Settling an adopted resource's deletion needs its binding, and the manager keeps
    // bindings in stack settings rather than on the deployment config the executor reads.
    if let Some(external_bindings) = stack_settings.external_bindings.clone() {
        config.external_bindings = external_bindings;
    }

    // Re-fetch under lock
    let deployment = manager_client
        .get_deployment()
        .id(&tracked_deployment.deployment_id)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to re-fetch deployment under lock".to_string(),
        })?
        .into_inner();

    current.status = serde_json::from_value(serde_json::Value::String(deployment.status.clone()))
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: format!("Unknown deployment status: {}", deployment.status),
        })?;
    current.stack_state = deployment
        .stack_state
        .map(serde_json::from_value)
        .transpose()
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to deserialize stack_state".to_string(),
        })?;

    let transport = ManagerApiTransport::with_execution_claim(
        manager_client.clone(),
        session.clone(),
        execution_claim.clone(),
    );
    let policy = RunnerPolicy {
        max_steps: 400,
        operation: LoopOperation::Delete,
        delay_strategy: alien_deployment::runner::DelayStrategy::Inline,
    };

    let runner_result = match alien_deployment::runner::run_step_loop(
        &mut current,
        &mut config,
        &client_config,
        &tracked_deployment.deployment_id,
        &policy,
        &transport,
        None,
        None,
    )
    .await
    {
        Ok(result)
            if result.loop_result.outcome == LoopOutcome::Success
                && current.status == DeploymentStatus::TeardownRequired =>
        {
            alien_deployment::setup_teardown::run_setup_teardown_after_handoff(
                &mut current,
                &mut config,
                &client_config,
                &tracked_deployment.deployment_id,
                &policy,
                &transport,
                None,
            )
            .await
            .map(|setup_result| setup_result.unwrap_or(result))
        }
        other => other,
    };
    let semantic_failure_status = runner_result.as_ref().ok().and_then(|result| {
        (result.loop_result.outcome == LoopOutcome::Failure)
            .then(|| result.loop_result.final_status.clone())
    });

    // Always reconcile + release
    let runner_result = combine_operation_and_finalization(
        preserve_semantic_failure(runner_result, &current),
        final_reconcile(
            &manager_client,
            &tracked_deployment.deployment_id,
            &session,
            execution_claim.as_ref(),
            &current,
        )
        .await,
    );

    if let Some(status) = semantic_failure_status {
        steps.fail(2, Some(format!("{status:?}")));
    }

    let RunnerResult {
        loop_result,
        steps_executed,
    } = runner_result.context(ErrorData::GenericError {
        message: "deletion step loop failed".to_string(),
    })?;

    info!(
        steps_executed = steps_executed,
        stop_reason = ?loop_result.stop_reason,
        outcome = ?loop_result.outcome,
        final_status = ?loop_result.final_status,
        "Deletion loop finished"
    );

    match loop_result.outcome {
        LoopOutcome::Success => {
            steps.complete(2, Some("Deleted".to_string()));
            println!("{}", success_line("Deployment destroyed."));
        }
        LoopOutcome::Failure => {
            steps.fail(2, Some(format!("{:?}", loop_result.final_status)));
            let failed = ErrorData::DeploymentFailed {
                message: format!("deletion failed at status {:?}", loop_result.final_status),
            };
            // The final state's headline error names each failed resource and its cause.
            return Err(
                match alien_deployment::deployment_headline_error_from_state(&current) {
                    Some(cause) => cause.context(failed),
                    None => AlienError::new(failed),
                },
            );
        }
        LoopOutcome::Neutral => {
            steps.complete(2, Some("Deletion in progress".to_string()));
        }
    }
    drop(steps);

    print_kept_deployer_secrets(&kept_deployer_secrets);
    println!(
        "{} {} ({})",
        dim_label("Deployment"),
        args.name,
        tracked_deployment.deployment_id
    );
    println!(
        "{} {}",
        dim_label("Next"),
        command(&format!(
            "alien deployments get {}",
            tracked_deployment.deployment_id
        ))
    );

    Ok(())
}

/// The deployer secret slots, all of them: a report can predate the deployer
/// writing the secret, so a slot last seen missing may hold a value by now.
pub(crate) fn kept_deployer_secrets(
    runtime_metadata: Option<&alien_core::RuntimeMetadata>,
) -> Vec<DeployerSecretReport> {
    runtime_metadata
        .map(|metadata| metadata.deployer_secrets.clone())
        .unwrap_or_default()
}

/// One line per kept deployer secret: what it is, where, and how to delete it.
fn kept_deployer_secret_lines(reports: &[DeployerSecretReport]) -> Vec<String> {
    reports
        .iter()
        .map(|report| {
            let location = match report.location.vault_name.as_deref() {
                Some(vault) => format!("{} in {vault}", report.location.name),
                None => report.location.name.clone(),
            };
            match report.location.delete_command.as_deref() {
                Some(delete) => format!("{} ({location}): {delete}", report.label),
                None => format!("{} ({location})", report.label),
            }
        })
        .collect()
}

pub(crate) fn print_kept_deployer_secrets(reports: &[DeployerSecretReport]) {
    if reports.is_empty() {
        return;
    }
    println!(
        "{}",
        dim_label(
            "Deployer secrets are kept (you write them, so Alien does not delete them). Delete any you wrote and no longer need:"
        )
    );
    for line in kept_deployer_secret_lines(reports) {
        println!("  {line}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        extract::State,
        http::{HeaderMap, StatusCode},
        response::{IntoResponse, Response},
        routing::{get, post},
        Json, Router,
    };
    use std::sync::{Arc, Mutex};

    const DEPLOYMENT_TOKEN: &str = "deployment-secret";

    fn deployer_secret(
        label: &str,
        status: alien_core::DeployerSecretStatus,
    ) -> DeployerSecretReport {
        let location = alien_core::deployer_secret_location(
            &alien_core::bindings::VaultBinding::parameter_store("stack-secrets"),
            &format!("input-{}", label.to_lowercase()),
            &alien_core::DeployerSecretLocationContext {
                aws_region: Some("us-east-1".to_string()),
                ..Default::default()
            },
        )
        .expect("location");
        DeployerSecretReport {
            input_id: label.to_lowercase(),
            label: label.to_string(),
            required: true,
            status,
            message: None,
            version: None,
            location,
        }
    }

    #[test]
    fn destroy_names_every_deployer_secret_slot_with_its_delete_command() {
        use alien_core::DeployerSecretStatus::{Invalid, Missing, Present};

        let state = DeploymentState {
            status: DeploymentStatus::DeletePending,
            platform: Platform::Aws,
            current_release: None,
            target_release: None,
            stack_state: None,
            error: None,
            environment_info: None,
            runtime_metadata: Some(alien_core::RuntimeMetadata {
                deployer_secrets: vec![
                    deployer_secret("Token", Present),
                    deployer_secret("Unwritten", Missing),
                    deployer_secret("Plaintext", Invalid),
                ],
                ..Default::default()
            }),
            retry_requested: false,
            protocol_version: alien_core::DEPLOYMENT_PROTOCOL_VERSION,
        };

        let kept = kept_deployer_secrets(state.runtime_metadata.as_ref());

        assert_eq!(
            kept_deployer_secret_lines(&kept),
            vec![
                "Token (stack-secrets-input-token): aws ssm delete-parameter --region us-east-1 --name 'stack-secrets-input-token'".to_string(),
                "Unwritten (stack-secrets-input-unwritten): aws ssm delete-parameter --region us-east-1 --name 'stack-secrets-input-unwritten'".to_string(),
                "Plaintext (stack-secrets-input-plaintext): aws ssm delete-parameter --region us-east-1 --name 'stack-secrets-input-plaintext'".to_string(),
            ],
            "a slot last reported missing may have been written since"
        );
    }

    #[derive(Default)]
    struct ManagerState {
        acquire_authorizations: Vec<String>,
        delete_authorizations: Vec<String>,
        deleted: bool,
    }

    type Shared = Arc<Mutex<ManagerState>>;

    async fn get_deployment(State(state): State<Shared>) -> Response {
        if state.lock().unwrap().deleted {
            return StatusCode::NOT_FOUND.into_response();
        }
        Json(deployment_record()).into_response()
    }

    fn deployment_record() -> serde_json::Value {
        serde_json::json!({
            "id": "dep_test",
            "name": "test",
            "platform": "test",
            "status": "teardown-required",
            "deploymentGroupId": "dg_test",
            "deploymentProtocolVersion": 1,
            "projectId": "proj_test",
            "workspaceId": "ws_test",
            "retryRequested": false,
            "createdAt": "2026-10-05T00:00:00Z"
        })
    }

    async fn list_token_deployment() -> StatusCode {
        StatusCode::FORBIDDEN
    }

    /// Mirrors the platform: sync acquire only accepts deployment-scoped tokens.
    /// A granted acquire finds nothing left to tear down, so the deployment is gone.
    async fn acquire(State(state): State<Shared>, headers: HeaderMap) -> Response {
        let authorization = headers
            .get("authorization")
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_string();
        let mut state = state.lock().unwrap();
        state.acquire_authorizations.push(authorization.clone());
        if authorization != format!("Bearer {DEPLOYMENT_TOKEN}") {
            return StatusCode::FORBIDDEN.into_response();
        }
        state.deleted = true;
        Json(serde_json::json!({ "deployments": [] })).into_response()
    }

    async fn whoami() -> Json<serde_json::Value> {
        Json(serde_json::json!({
            "kind": "serviceAccount", "id": "sa_test", "workspaceId": "ws_test",
            "role": "deployment.manager",
            "scope": { "type": "deployment", "deploymentId": "dep_test", "projectId": "proj_test" }
        }))
    }

    #[cfg(feature = "platform")]
    #[tokio::test]
    async fn logged_in_destroy_mints_a_scoped_token_without_local_tracking() {
        const ID: &str = "dep_0000000000000000000000000000";
        const PROJECT: &str = "prj_0000000000000000000000000000";
        async fn project() -> Json<serde_json::Value> {
            Json(
                serde_json::json!({ "id": PROJECT, "name": "example", "workspaceId": "ws_000000000000000000000000", "createdAt": "2026-01-01T00:00:00Z" }),
            )
        }
        async fn deployment(State(state): State<Shared>) -> Response {
            let state = state.lock().unwrap();
            if state.deleted {
                return StatusCode::NOT_FOUND.into_response();
            }
            let status = if state.delete_authorizations.is_empty() {
                "running"
            } else {
                "teardown-required"
            };
            Json(
                serde_json::json!({ "id": ID, "name": "example", "platform": "test", "status": status, "deploymentGroupId": "dg_0000000000000000000000000000", "deploymentProtocolVersion": 1, "projectId": PROJECT, "workspaceId": "ws_000000000000000000000000", "managerId":"mgr_0000000000000000000000000000", "purpose":"application", "releaseChannel":"stable", "stackSettings":{}, "updatedAt":"2026-01-01T00:00:00Z", "retryRequested": false, "createdAt": "2026-01-01T00:00:00Z" }),
            ).into_response()
        }
        async fn request_delete(
            State(state): State<Shared>,
            headers: HeaderMap,
            Json(body): Json<serde_json::Value>,
        ) -> (StatusCode, Json<serde_json::Value>) {
            assert_eq!(headers["authorization"], "Bearer user-session");
            assert_eq!(body["action"], "cleanup");
            state
                .lock()
                .unwrap()
                .delete_authorizations
                .push(headers["authorization"].to_str().unwrap().to_string());
            (
                StatusCode::ACCEPTED,
                Json(
                    serde_json::json!({ "action": "cleanup", "cleanupRequired": true, "message": "Deployment deletion accepted" }),
                ),
            )
        }
        async fn token(
            headers: HeaderMap,
            Json(body): Json<serde_json::Value>,
        ) -> (StatusCode, Json<serde_json::Value>) {
            assert_eq!(headers["authorization"], "Bearer user-session");
            assert!(body["expiresAt"].is_string());
            (
                StatusCode::CREATED,
                Json(serde_json::json!({ "deploymentId": ID, "token": DEPLOYMENT_TOKEN })),
            )
        }
        let state = Shared::default();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let server_url = format!("http://{}", listener.local_addr().unwrap());
        let manager_url = server_url.clone();
        let app = Router::new()
            .route("/v1/projects/{id}", get(project))
            // Current project routing is unrelated to the deployment's recorded manager.
            .route("/v1/resolve", get(|| async { StatusCode::BAD_GATEWAY }))
            .route("/v1/managers/{id}", get(move || async move {
                Json(serde_json::json!({
                    "id":"mgr_0000000000000000000000000000", "name":"original", "url":manager_url,
                    "workspaceId":"ws_000000000000000000000000", "createdAt":"2026-01-01T00:00:00Z",
                    "defaultProjectCount":0, "managedDeploymentCount":1, "managementConfigs":{},
                    "isSystem":false, "status":"healthy", "targets":["test"]
                }))
            }))
            .route("/v1/deployments/{id}", get(deployment))
            .route("/v1/deployments/{id}/delete", post(request_delete))
            .route("/v1/sync/acquire", post(acquire))
            .route("/v1/deployments/{id}/token", post(token))
            .with_state(state.clone());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let ctx = ExecutionMode::Platform {
            base_url: server_url,
            api_key: Some("user-session".to_string()),
            workspace: Some("example".to_string()),
            project: Some("example".to_string()),
            no_browser: true,
        };
        let args = DestroyArgs {
            token: None,
            name: ID.to_string(),
            platform: Some("test".to_string()),
            force: false,
        };
        let (target, manager) = resolve_destroy_target(&args, &ctx, "test", None)
            .await
            .unwrap();
        assert_eq!(target.deployment_id, ID);
        assert_eq!(target.api_key, DEPLOYMENT_TOKEN);
        assert_eq!(manager.auth_token.as_deref(), Some("user-session"));
        destroy_tracked_deployment(
            &args,
            Platform::Test,
            &target,
            manager,
            FixedSteps::new(&["Resolve deployment", "Resolve manager", "Delete resources"]),
        )
        .await
        .unwrap();
        let state = state.lock().unwrap();
        assert!(state.deleted);
        assert_eq!(state.delete_authorizations, vec!["Bearer user-session"]);
        assert_eq!(
            state.acquire_authorizations,
            vec![format!("Bearer {DEPLOYMENT_TOKEN}")]
        );
    }

    #[tokio::test]
    async fn remote_names_are_project_scoped_and_ambiguity_is_rejected() {
        async fn list() -> Json<serde_json::Value> {
            let item = |id: &str, project: &str, name: &str| serde_json::json!({ "id": id, "name": name, "platform": "test", "status": "running", "deploymentGroupId": "dg_test", "deploymentProtocolVersion": 1, "projectId": project, "workspaceId": "ws_test", "retryRequested": false, "createdAt": "2026-01-01T00:00:00Z" });
            Json(
                serde_json::json!({ "items": [item("dep_test", "proj_test", "test"), item("dep_other", "proj_other", "test"), item("dep_a", "proj_test", "duplicate"), item("dep_b", "proj_test", "duplicate")] }),
            )
        }
        let state = Shared::default();
        let app = Router::new()
            .route("/v1/deployments", get(list))
            .route("/v1/deployments/{id}", get(get_deployment))
            .with_state(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let server_url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let manager = alien_manager_api::Client::new(&server_url);
        let ctx = ExecutionMode::Standalone {
            server_url,
            api_key: "operator".to_string(),
        };
        let target = resolve_untracked_name(&ctx, &manager, "test", "proj_test")
            .await
            .unwrap();
        assert_eq!(target.id, "dep_test");
        let error = resolve_untracked_name(&ctx, &manager, "duplicate", "proj_test")
            .await
            .unwrap_err();
        assert!(error.message.contains("Multiple deployments"));
        assert!(
            resolve_untracked_name(&ctx, &manager, "test", "missing-project")
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn explicit_token_destroys_without_a_local_tracking_entry() {
        let state = Shared::default();
        let app = Router::new()
            .route("/v1/whoami", get(whoami))
            .route("/v1/deployments", get(list_token_deployment))
            .route("/v1/deployments/{id}", get(get_deployment))
            .route("/v1/sync/acquire", post(acquire))
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let server_url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let ctx = ExecutionMode::Standalone {
            server_url,
            api_key: "wrong-session".to_string(),
        };
        let args = DestroyArgs {
            token: Some(DEPLOYMENT_TOKEN.to_string()),
            name: "dep_test".to_string(),
            platform: Some("test".to_string()),
            force: false,
        };
        let (target, manager) = resolve_destroy_target(&args, &ctx, "test", None)
            .await
            .unwrap();
        assert_eq!(target.deployment_id, "dep_test");
        assert_eq!(manager.auth_token.as_deref(), Some(DEPLOYMENT_TOKEN));
        destroy_tracked_deployment(
            &args,
            Platform::Test,
            &target,
            manager,
            FixedSteps::new(&["Resolve deployment", "Resolve manager", "Delete resources"]),
        )
        .await
        .unwrap();
        assert!(state.lock().unwrap().deleted);
        assert_eq!(
            state.lock().unwrap().acquire_authorizations,
            vec![format!("Bearer {DEPLOYMENT_TOKEN}")]
        );
    }

    #[tokio::test]
    async fn explicit_token_rejects_mismatched_name_and_platform() {
        let state = Shared::default();
        let app = Router::new()
            .route("/v1/whoami", get(whoami))
            .route("/v1/deployments", get(list_token_deployment))
            .route("/v1/deployments/{id}", get(get_deployment))
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let server_url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let ctx = ExecutionMode::Standalone {
            server_url,
            api_key: "operator".to_string(),
        };
        for (name, platform, field) in [
            ("another", "test", "name"),
            ("other/test", "test", "name"),
            ("group/test", "test", "name"),
            ("dep_test", "aws", "platform"),
        ] {
            let args = DestroyArgs {
                token: Some(DEPLOYMENT_TOKEN.to_string()),
                name: name.to_string(),
                platform: Some(platform.to_string()),
                force: false,
            };
            let error = resolve_destroy_target(&args, &ctx, platform, None)
                .await
                .err()
                .expect("mismatched target must fail before mutation");
            assert_eq!(error.code, "VALIDATION_ERROR");
            assert!(error.message.contains(field));
            assert!(!state.lock().unwrap().deleted);
            assert!(state.lock().unwrap().acquire_authorizations.is_empty());
        }
    }

    #[tokio::test]
    async fn force_target_rejects_deployment_outside_selected_project() {
        let state = Shared::default();
        let app = Router::new()
            .route("/v1/deployments/{id}", get(get_deployment))
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let server_url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let ctx = ExecutionMode::Standalone {
            server_url,
            api_key: "operator".to_string(),
        };
        let args = DestroyArgs {
            token: None,
            name: "dep_test".to_string(),
            platform: Some("test".to_string()),
            force: true,
        };
        let error = resolve_destroy_target(&args, &ctx, "test", None)
            .await
            .err()
            .expect("cross-project force target must be rejected before mutation");
        assert_eq!(error.code, "VALIDATION_ERROR");
        assert!(error.message.contains("project"));
        let state = state.lock().unwrap();
        assert!(!state.deleted);
        assert!(state.delete_authorizations.is_empty());
        assert!(state.acquire_authorizations.is_empty());
    }

    #[tokio::test]
    async fn remote_local_teardown_acquires_with_the_deployment_token() {
        let state = Shared::default();
        let app = Router::new()
            .route("/v1/deployments/{id}", get(get_deployment))
            .route("/v1/sync/acquire", post(acquire))
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind manager");
        let manager_url = format!("http://{}", listener.local_addr().expect("manager address"));
        tokio::spawn(async move { axum::serve(listener, app).await.expect("serve manager") });

        // Discovery authenticated as the user, as `alien destroy` does with a CLI login.
        let user_http_client =
            crate::auth::client_with_auth_and_workspace("Bearer user-session", "ws-name")
                .expect("user client");
        let manager_ctx = ManagerContext {
            manager_url: manager_url.clone(),
            manager_name: None,
            manager_is_system: None,
            manager_cloud: None,
            client: alien_manager_api::Client::new_with_client(
                &manager_url,
                user_http_client.clone(),
            ),
            http_client: user_http_client,
            auth_token: Some("user-session".to_string()),
            repository_name: None,
            repository_uri: None,
            workspace: Some("ws-name".to_string()),
        };
        let tracked = TrackedDeployment {
            name: "test".to_string(),
            deployment_id: "dep_test".to_string(),
            api_key: DEPLOYMENT_TOKEN.to_string(),
            workspace_id: "ws_test".to_string(),
            project_id: "proj_test".to_string(),
        };
        let args = DestroyArgs {
            token: None,
            name: "test".to_string(),
            platform: Some("local".to_string()),
            force: false,
        };

        destroy_tracked_deployment(
            &args,
            Platform::Local,
            &tracked,
            manager_ctx,
            FixedSteps::new(&["Resolve deployment", "Resolve manager", "Delete resources"]),
        )
        .await
        .expect("destroy should finish once the deployment is gone");

        assert_eq!(
            state.lock().unwrap().acquire_authorizations,
            vec![format!("Bearer {DEPLOYMENT_TOKEN}")]
        );
    }
}
