//! Deploy command — creates or updates a deployment via the manager.
//!
//! Flow:
//! 1. Resolve/create deployment (via platform API for DG tokens, or from tracker)
//! 2. Discover manager URL (resolve_manager for OAuth, DG endpoint for DG tokens)
//! 3. Run step loop via manager (acquire → step → reconcile → release)

use crate::commands::deployments::MonitoringMode;
use crate::commands::{
    create_initial_deployment, fetch_dev_deployment_live_state,
    wait_for_dev_deployment_ready_with_progress,
};
use crate::deployment_tracking::{validate_token, DeploymentToken, DeploymentTracker};
use crate::error::{ErrorData, Result};
use crate::execution_context::ExecutionMode;
use crate::ui::{command, contextual_heading, dim_label, success_line, FixedSteps};
use alien_cli_common::network::{self, NetworkArgs};
use alien_core::{ClientConfig, DeploymentConfig, DeploymentState, DeploymentStatus, Platform};
use alien_deployment::loop_contract::{LoopOperation, LoopOutcome, LoopStopReason};
use alien_deployment::manager_api_transport::{
    acquire_deployment, final_reconcile, release_deployment, ManagerApiTransport,
};
use alien_deployment::runner::{RunnerPolicy, RunnerResult};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_manager_api::Client as ManagerClient;
use alien_platform_api::Client as SdkClient;
use clap::Parser;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, USER_AGENT};
use std::str::FromStr;
use tracing::info;
use uuid::Uuid;

#[derive(Parser, Debug, Clone)]
#[command(
    about = "Provision and update a customer deployment",
    long_about = "Provision and update a customer deployment in their cloud account.",
    after_help = "EXAMPLES:
    # Set up a new customer deployment
    alien deploy --token dg_abc123... --name production --platform aws

    # Deploy an existing deployment (uses stored API key)
    alien deploy --name production --platform aws

    # Deploy without heartbeat capability
    alien deploy --token ax_deployment_xyz... --name prod --platform aws --no-heartbeat"
)]
pub struct DeployArgs {
    /// Deployment API key for authentication (optional if deployment is already tracked)
    #[arg(long)]
    pub token: Option<String>,

    /// Deployment name for identification in tracking
    #[arg(long)]
    pub name: String,

    /// Target platform for the deployment (aws, gcp, azure)
    #[arg(long)]
    pub platform: String,

    /// Allow experimental platforms (kubernetes, local)
    #[arg(long)]
    pub experimental: bool,

    /// Disable heartbeat capability
    #[arg(long)]
    pub no_heartbeat: bool,

    /// Telemetry / monitoring mode.
    /// "auto" (default) uses the parent AM's built-in DeepStore or external OTLP integration.
    /// "off" disables all monitoring.
    #[arg(long, value_enum, default_value_t = MonitoringMode::Auto)]
    pub monitoring: MonitoringMode,

    /// Manager to use for deployment.
    /// Omit for auto-resolve (platform resolves from deployment record).
    /// Use "none" to deploy without a manager (e.g., bootstrapping the manager itself).
    /// Or pass a specific manager ID.
    #[arg(long)]
    pub manager: Option<String>,

    #[command(flatten)]
    pub network: NetworkArgs,
}

/// Create authenticated platform client
fn create_platform_client(api_key: &str, base_url: &str) -> Result<SdkClient> {
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {}", api_key))
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "Invalid authorization header value".to_string(),
            })?,
    );
    headers.insert(USER_AGENT, HeaderValue::from_static("alien-cli"));

    let http_client = reqwest::Client::builder()
        .default_headers(headers)
        .build()
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to create HTTP client".to_string(),
        })?;

    Ok(SdkClient::new_with_client(base_url, http_client))
}

/// Create authenticated manager client
fn create_manager_client(token: &str, manager_url: &str) -> Result<ManagerClient> {
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {}", token))
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "Invalid token format".to_string(),
            })?,
    );
    headers.insert(USER_AGENT, HeaderValue::from_static("alien-cli"));

    let http_client = reqwest::Client::builder()
        .default_headers(headers)
        .build()
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to create HTTP client".to_string(),
        })?;

    Ok(ManagerClient::new_with_client(manager_url, http_client))
}

/// Main entry point for deploy command
pub async fn deploy_task(args: DeployArgs, ctx: ExecutionMode) -> Result<()> {
    if let ExecutionMode::Dev { port } = ctx {
        return deploy_local_dev_task(args, port).await;
    }

    // Check for experimental platforms
    if let Ok(platform) = Platform::from_str(&args.platform) {
        if platform.is_experimental() && !args.experimental {
            return Err(AlienError::new(ErrorData::ValidationError {
                field: "platform".to_string(),
                message: format!(
                    "Platform '{}' is experimental and not yet production-ready. Pass --experimental to use it anyway.",
                    args.platform
                ),
            }));
        }
    }

    info!("Starting deploy command");
    println!(
        "{}",
        contextual_heading("Deploying", &args.name, &[("to", &args.platform)])
    );
    let steps = FixedSteps::new(&[
        "Resolve deployment",
        "Connect to manager",
        "Provision resources",
        "Activate",
    ]);
    steps.activate(0, Some(args.name.clone()));

    // Parse platform
    let platform = Platform::from_str(&args.platform).map_err(|e| {
        AlienError::new(ErrorData::ValidationError {
            field: "platform".to_string(),
            message: e,
        })
    })?;

    let base_url = ctx.base_url();

    // Step 1: Load or register the deployment (via platform API)
    let mut tracker = DeploymentTracker::new()?;
    let tracked_deployment = match tracker.get_deployment(&args.name) {
        Some(deployment) => {
            info!("Found tracked deployment '{}'", args.name);

            // If a token was provided, check if it's different from the stored one
            if let Some(ref provided_token) = args.token {
                if deployment.api_key != *provided_token {
                    info!("Updating stored API key for deployment '{}'", args.name);
                    tracker.remove_deployment(&args.name)?;
                    tracker
                        .add_deployment(args.name.clone(), provided_token.clone(), &base_url)
                        .await
                        .context(ErrorData::ConfigurationError {
                            message: "Failed to update deployment API key".to_string(),
                        })?
                } else {
                    deployment.clone()
                }
            } else {
                deployment.clone()
            }
        }
        None => {
            info!("Deployment '{}' not tracked yet, registering...", args.name);

            let token = args.token.as_ref().ok_or_else(|| {
                AlienError::new(ErrorData::ValidationError {
                    field: "token".to_string(),
                    message: format!(
                        "API key is required when deploying to a new deployment '{}'",
                        args.name
                    ),
                })
            })?;

            let token_info = validate_token(token, &base_url).await?;

            match token_info {
                DeploymentToken::Deployment { .. } => {
                    info!("   Using deployment token");
                    tracker
                        .add_deployment(args.name.clone(), token.clone(), &base_url)
                        .await
                        .context(ErrorData::ConfigurationError {
                            message: "Failed to register deployment".to_string(),
                        })?
                }
                DeploymentToken::DeploymentGroup {
                    deployment_group_name,
                    workspace_id,
                    project_id,
                    ..
                } => {
                    info!(
                        "   Using deployment group token for group '{}'",
                        deployment_group_name
                    );
                    info!("   Creating new deployment '{}'...", args.name);

                    let sdk_client = create_platform_client(token, &base_url)?;

                    let network_settings =
                        network::parse_network_settings(&args.network, &args.platform).map_err(
                            |e| {
                                AlienError::new(ErrorData::ValidationError {
                                    field: "network".to_string(),
                                    message: e,
                                })
                            },
                        )?;

                    let sdk_network = network_settings
                        .map(|ns| {
                            let json = serde_json::to_value(&ns).into_alien_error().context(
                                ErrorData::ConfigurationError {
                                    message: "Failed to serialize network settings".to_string(),
                                },
                            )?;
                            serde_json::from_value(json).into_alien_error().context(
                                ErrorData::ConfigurationError {
                                    message: "Failed to convert network settings to SDK type"
                                        .to_string(),
                                },
                            )
                        })
                        .transpose()?;

                    let stack_settings = alien_platform_api::types::NewDeploymentRequestStackSettings {
                        deployment_model: Some(alien_platform_api::types::NewDeploymentRequestStackSettingsDeploymentModel::Pull),
                        heartbeats: Some(if args.no_heartbeat {
                            alien_platform_api::types::NewDeploymentRequestStackSettingsHeartbeats::Off
                        } else {
                            alien_platform_api::types::NewDeploymentRequestStackSettingsHeartbeats::On
                        }),
                        telemetry: Some(match args.monitoring {
                            MonitoringMode::Off => alien_platform_api::types::NewDeploymentRequestStackSettingsTelemetry::Off,
                            MonitoringMode::Auto => alien_platform_api::types::NewDeploymentRequestStackSettingsTelemetry::Auto,
                        }),
                        updates: Some(alien_platform_api::types::NewDeploymentRequestStackSettingsUpdates::Auto),
                        network: sdk_network,
                        domains: None,
                    };

                    let create_response = sdk_client
                        .create_deployment()
                        .workspace(&workspace_id)
                        .body(alien_platform_api::types::NewDeploymentRequest {
                            name: args.name.clone().try_into().into_alien_error().context(
                                ErrorData::ValidationError {
                                    field: "name".to_string(),
                                    message: "Invalid deployment name".to_string(),
                                },
                            )?,
                            platform: args
                                .platform
                                .clone()
                                .as_str()
                                .try_into()
                                .into_alien_error()
                                .context(ErrorData::ValidationError {
                                    field: "platform".to_string(),
                                    message: "Invalid platform value".to_string(),
                                })?,
                            project: project_id.clone().try_into().into_alien_error().context(
                                ErrorData::ValidationError {
                                    field: "project".to_string(),
                                    message: "Invalid project".to_string(),
                                },
                            )?,
                            stack_settings: Some(stack_settings),
                            manager_id: None,
                            pinned_release_id: None,
                            environment_variables: None,
                            deployment_group_id: None,
                            environment_info: None,
                        })
                        .send()
                        .await
                        .into_alien_error()
                        .context(ErrorData::ConfigurationError {
                            message: "Failed to create deployment with deployment group token"
                                .to_string(),
                        })?
                        .into_inner();

                    let response_json = serde_json::to_value(&create_response)
                        .into_alien_error()
                        .context(ErrorData::ConfigurationError {
                        message: "Failed to serialize response".to_string(),
                    })?;

                    let deployment_id = response_json
                        .get("deployment")
                        .and_then(|d| d.get("id"))
                        .and_then(|id| id.as_str())
                        .ok_or_else(|| {
                            AlienError::new(ErrorData::ConfigurationError {
                                message: "Failed to extract deployment ID from response"
                                    .to_string(),
                            })
                        })?
                        .to_string();

                    let deployment_token = response_json
                        .get("token")
                        .and_then(|t| t.as_str())
                        .ok_or_else(|| {
                            AlienError::new(ErrorData::ConfigurationError {
                                message: "Server did not return deployment token".to_string(),
                            })
                        })?
                        .to_string();

                    info!("   Deployment created: {}", deployment_id);

                    tracker
                        .add_deployment(args.name.clone(), deployment_token, &base_url)
                        .await
                        .context(ErrorData::ConfigurationError {
                            message: "Failed to track newly created deployment".to_string(),
                        })?
                }
            }
        }
    };

    steps.complete(
        0,
        Some(format!(
            "{} ({})",
            args.name, tracked_deployment.deployment_id
        )),
    );

    // Step 2: Resolve manager
    steps.activate(1, Some("Discovering manager...".to_string()));

    let manager_ctx = ctx
        .resolve_manager(&tracked_deployment.project_id, &args.platform)
        .await?;
    let manager_client = manager_ctx.client;

    steps.complete(1, Some(format!("Manager: {}", manager_ctx.manager_url)));

    // Step 3: Initialize with manager and run deployment
    steps.activate(2, Some(tracked_deployment.deployment_id.clone()));

    // Get deployment state from manager
    let deployment = manager_client
        .get_deployment()
        .id(&tracked_deployment.deployment_id)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: format!(
                "Failed to get deployment '{}' from manager.",
                tracked_deployment.deployment_id
            ),
        })?
        .into_inner();

    let status: DeploymentStatus =
        serde_json::from_value(serde_json::Value::String(deployment.status.clone()))
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: format!("Unknown deployment status: {}", deployment.status),
            })?;

    // Get cloud credentials from environment
    use alien_infra::ClientConfigExt;
    let client_config =
        ClientConfig::from_std_env(platform)
            .await
            .context(ErrorData::ConfigurationError {
                message: format!("Failed to build client config for platform {:?}", platform),
            })?;

    // Build deployment state
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

    // Running deploy on a failed deployment is an implicit retry request
    if current.status.is_failed() {
        info!(
            "Deployment is in {:?} state, setting retry_requested to proceed",
            current.status
        );
        current.retry_requested = true;
    }

    if let Some(stack_state) = current.stack_state.as_ref() {
        steps.sync_deployment_resources(&stack_state.resources);
    }

    // Build minimal deployment config
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

    // Acquire → step loop → reconcile → release (all via manager)
    let session = format!("cli-deploy-{}", Uuid::new_v4());
    acquire_deployment(&manager_client, &tracked_deployment.deployment_id, &session)
        .await
        .context(ErrorData::ConfigurationError {
            message: "Failed to acquire deployment lock".to_string(),
        })?;

    // Re-fetch under lock (manager may have advanced the state)
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
    current.runtime_metadata = deployment
        .runtime_metadata
        .map(|rm| serde_json::to_value(rm).and_then(serde_json::from_value))
        .transpose()
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to deserialize runtime_metadata".to_string(),
        })?;

    let transport = ManagerApiTransport::new(manager_client.clone(), session.clone());
    let policy = RunnerPolicy {
        max_steps: 400,
        operation: LoopOperation::Deploy,
        delay_threshold: None,
    };

    let runner_result = alien_deployment::runner::run_step_loop(
        &mut current,
        &mut config,
        &client_config,
        &tracked_deployment.deployment_id,
        &policy,
        &transport,
        None,
        None,
    )
    .await;

    // Always reconcile + release, even on error
    final_reconcile(
        &manager_client,
        &tracked_deployment.deployment_id,
        &session,
        &current,
    )
    .await;
    release_deployment(&manager_client, &tracked_deployment.deployment_id, &session).await;

    let RunnerResult {
        loop_result,
        steps_executed,
    } = runner_result.context(ErrorData::GenericError {
        message: "deployment step loop failed".to_string(),
    })?;

    info!(
        steps_executed = steps_executed,
        stop_reason = ?loop_result.stop_reason,
        outcome = ?loop_result.outcome,
        final_status = ?loop_result.final_status,
        "Deployment loop finished"
    );

    // Handle runner outcome
    match loop_result.outcome {
        LoopOutcome::Success => {
            steps.complete(2, Some("Resources ready".to_string()));
            steps.complete(3, Some("Running".to_string()));
        }
        LoopOutcome::Failure => {
            steps.fail(2, Some(format!("{:?}", loop_result.final_status)));
            return Err(AlienError::new(ErrorData::DeploymentFailed {
                message: format!(
                    "{} failed",
                    describe_failed_status(&loop_result.final_status)
                ),
            }));
        }
        LoopOutcome::Neutral if loop_result.stop_reason == LoopStopReason::Handoff => {
            steps.complete(2, Some("Resources ready".to_string()));
            steps.complete(3, Some("Running".to_string()));
        }
        LoopOutcome::Neutral => {
            steps.fail(2, Some(format!("{:?}", loop_result.final_status)));
            return Err(AlienError::new(ErrorData::DeploymentFailed {
                message: format!(
                    "deployment loop ended without resolution (stop_reason: {:?}, status: {:?})",
                    loop_result.stop_reason, loop_result.final_status
                ),
            }));
        }
    }
    drop(steps);

    println!("{}", success_line("Deployment is running."));
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

fn describe_failed_status(status: &alien_deployment::DeploymentStatus) -> &'static str {
    match status {
        alien_deployment::DeploymentStatus::InitialSetupFailed => "initial setup",
        alien_deployment::DeploymentStatus::ProvisioningFailed => "provisioning",
        alien_deployment::DeploymentStatus::UpdateFailed => "update",
        alien_deployment::DeploymentStatus::DeleteFailed => "deletion",
        alien_deployment::DeploymentStatus::RefreshFailed => "refresh",
        _ => "deployment",
    }
}

async fn deploy_local_dev_task(args: DeployArgs, port: u16) -> Result<()> {
    if args.platform != "local" {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "platform".to_string(),
            message: "alien dev deploy only supports --platform local".to_string(),
        }));
    }

    println!(
        "{}",
        contextual_heading("Creating local deployment", &args.name, &[])
    );

    let steps = FixedSteps::new(&["Prepare deployment", "Wait for deployment"]);
    steps.activate(0, Some(args.name.clone()));
    let deployment_id = create_initial_deployment(&args.name, port, None).await?;
    steps.complete(0, Some(format!("{} ({})", args.name, deployment_id)));

    steps.activate(1, Some(format!("{} ({})", args.name, "queued")));
    let snapshot = wait_for_dev_deployment_ready_with_progress(port, &args.name, None, |status| {
        steps.activate(
            1,
            Some(format!(
                "{} ({})",
                args.name,
                crate::ui::format_deployment_status(status).to_ascii_lowercase()
            )),
        );
    })
    .await?;
    steps.complete(1, Some(format!("{} ready", args.name)));
    drop(steps);

    println!("{}", success_line("Deployment ready."));
    println!(
        "{} {} ({})",
        dim_label("Deployment"),
        snapshot.deployment_name,
        snapshot.deployment_id
    );
    let live_state = fetch_dev_deployment_live_state(port, &snapshot.deployment_name).await?;
    let stack_state = live_state.as_ref().and_then(|state| state.stack_state.as_ref());
    if snapshot.resources.is_empty() && stack_state.is_none() {
        println!(
            "{}",
            dim_label("No resources were reported yet.")
        );
    } else {
        println!("{}", dim_label("Resources"));
        let mut resource_names = std::collections::BTreeSet::new();
        resource_names.extend(snapshot.resources.keys().cloned());
        if let Some(stack_state) = stack_state {
            resource_names.extend(stack_state.resources.keys().cloned());
        }

        for name in resource_names {
            let public_resource = snapshot.resources.get(&name);
            let stack_resource = stack_state.and_then(|state| state.resources.get(&name));
            let rendered_value = format_local_dev_resource_value(&name, public_resource, stack_resource);
            let resource_type = public_resource
                .and_then(|resource| resource.resource_type.as_ref().map(|value| value.as_str()))
                .or_else(|| stack_resource.map(|resource| resource.resource_type.as_str()));
            println!(
                "  - {}{}{}",
                name,
                resource_type
                    .map(|resource_type| format!(" ({resource_type})"))
                    .unwrap_or_default(),
                format!(": {}", rendered_value)
            );
        }
    }
    println!(
        "{} inspect it with {}",
        dim_label("Next"),
        command(&format!(
            "alien dev deployments get {}",
            snapshot.deployment_name
        ))
    );

    Ok(())
}

fn format_local_dev_resource_value(
    name: &str,
    public_resource: Option<&alien_core::DevResourceInfo>,
    stack_resource: Option<&alien_core::StackResourceState>,
) -> String {
    if let Some(public_resource) = public_resource {
        if is_local_private_url(&public_resource.url) {
            if name == "worker"
                || public_resource
                    .resource_type
                    .as_deref()
                    .is_some_and(|resource_type| resource_type.eq_ignore_ascii_case("function"))
            {
                return "running (private)".to_string();
            }
            if public_resource
                .resource_type
                .as_deref()
                .is_some_and(|resource_type| resource_type.eq_ignore_ascii_case("storage"))
            {
                return "local filesystem".to_string();
            }
        }
        return public_resource.url.clone();
    }

    let Some(stack_resource) = stack_resource else {
        return "running".to_string();
    };

    match stack_resource.status {
        alien_core::ResourceStatus::Running
            if stack_resource.resource_type.eq_ignore_ascii_case("storage") =>
        {
            "local filesystem".to_string()
        }
        alien_core::ResourceStatus::Running => "running (private)".to_string(),
        _ => crate::ui::format_resource_status(stack_resource.status)
            .to_ascii_lowercase()
            .replace(' ', "-"),
    }
}

fn is_local_private_url(url: &str) -> bool {
    url.starts_with("http://localhost:")
        || url.starts_with("https://localhost:")
        || url.starts_with("http://127.0.0.1:")
        || url.starts_with("https://127.0.0.1:")
}
