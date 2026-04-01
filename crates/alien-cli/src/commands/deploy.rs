use crate::commands::deployments::MonitoringMode;
use crate::commands::{create_initial_deployment, wait_for_dev_deployment_ready_with_progress};
use crate::deployment_tracking::{validate_token, DeploymentToken, DeploymentTracker};
use crate::error::{ErrorData, Result};
use crate::execution_context::ExecutionMode;
use crate::ui::{command, contextual_heading, dim_label, success_line, FixedSteps};
use alien_cli_common::network::{self, NetworkArgs};
use alien_core::{ClientConfig, DeploymentConfig, DeploymentState, Platform};
use alien_deployment::loop_contract::{LoopOperation, LoopOutcome, LoopStopReason};
use alien_deployment::runner::{RunnerPolicy, RunnerResult};
use alien_deployment::transport::{DeploymentLoopTransport, StepReconcileResult};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_platform_api::Client as SdkClient;
use alien_platform_api::SdkResultExt;
use async_trait::async_trait;
use clap::Parser;
use reqwest::{
    header::{HeaderMap, HeaderValue, AUTHORIZATION, USER_AGENT},
    Client,
};
use std::str::FromStr;
use tracing::info;
use uuid::Uuid;

#[derive(Parser, Debug, Clone)]
#[command(
    about = "Deploy a deployment to a cloud platform",
    long_about = "Deploy a deployment to a cloud platform using the Alien Platform API.",
    after_help = "EXAMPLES:
    # Deploy a new deployment using a deployment API key
    alien deploy --token ax_deployment_1234abcd... --name production --platform aws

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

    /// Target platform for the deployment (aws, gcp, azure, kubernetes, local)
    #[arg(long)]
    pub platform: String,

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

/// Helper for automatic deployment state release
struct DeploymentGuard {
    client: SdkClient,
    workspace_id: String,
    deployment_id: String,
    session_id: String,
}

impl DeploymentGuard {
    fn new(
        client: SdkClient,
        workspace_id: String,
        deployment_id: String,
        session_id: String,
    ) -> Self {
        Self {
            client,
            workspace_id,
            deployment_id,
            session_id,
        }
    }
}

impl Drop for DeploymentGuard {
    fn drop(&mut self) {
        // Release deployment state on drop (even if deployment fails)
        let client = self.client.clone();
        let workspace_id = self.workspace_id.clone();
        let deployment_id = self.deployment_id.clone();
        let session_id = self.session_id.clone();

        let _ = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async move {
                // Convert deployment_id to typed ID
                let deployment_id_typed: std::result::Result<
                    alien_platform_api::types::SyncReleaseRequestDeploymentId,
                    _,
                > = deployment_id.as_str().try_into();

                if let Ok(deployment_id_typed) = deployment_id_typed {
                    let release_request = alien_platform_api::types::SyncReleaseRequest {
                        deployment_id: deployment_id_typed,
                        session: session_id,
                    };

                    let _ = client
                        .sync_release()
                        .workspace(&workspace_id)
                        .body(release_request)
                        .send()
                        .await;
                }
            })
        });
    }
}

/// Transport that reconciles deployment state via the Platform sync API.
///
/// Used by the CLI deploy command to persist state after each step through
/// the shared [`alien_deployment::runner::run_step_loop`] runner.
struct PlatformCliTransport {
    sdk_client: SdkClient,
    workspace_name: String,
    session_id: String,
}

#[async_trait]
impl DeploymentLoopTransport for PlatformCliTransport {
    async fn reconcile_step(
        &self,
        deployment_id: &str,
        state: &DeploymentState,
        step_error: Option<&AlienError>,
        update_heartbeat: bool,
    ) -> Result<StepReconcileResult, AlienError> {
        // Serialize state to SDK type
        let state_sdk: alien_platform_api::types::SyncReconcileRequestState =
            serde_json::from_value(serde_json::to_value(state).map_err(|e| {
                AlienError::new(ErrorData::ConfigurationError {
                    message: format!("Failed to serialize deployment state: {e}"),
                })
            })?)
            .map_err(|e| {
                AlienError::new(ErrorData::ConfigurationError {
                    message: format!("Failed to convert state to SDK type: {e}"),
                })
            })?;

        // Serialize step error to SDK type if present
        let error_sdk = step_error
            .map(|e| {
                let json = serde_json::to_value(e).map_err(|e| {
                    AlienError::new(ErrorData::ConfigurationError {
                        message: format!("Failed to serialize deployment error: {e}"),
                    })
                })?;
                serde_json::from_value(json).map_err(|e| {
                    AlienError::new(ErrorData::ConfigurationError {
                        message: format!("Failed to convert error to SDK type: {e}"),
                    })
                })
            })
            .transpose()?;

        // Convert deployment_id to typed ID
        let deployment_id_typed: alien_platform_api::types::SyncReconcileRequestDeploymentId =
            deployment_id
                .try_into()
                .map_err(|_: alien_platform_api::types::error::ConversionError| {
                    AlienError::new(ErrorData::ConfigurationError {
                        message: "Invalid deployment ID format".to_string(),
                    })
                })?;

        let reconcile_response = self
            .sdk_client
            .sync_reconcile()
            .workspace(&self.workspace_name)
            .body(alien_platform_api::types::SyncReconcileRequest {
                deployment_id: deployment_id_typed,
                session: Some(self.session_id.clone()),
                state: state_sdk,
                error: error_sdk,
                update_heartbeat: Some(update_heartbeat),
            })
            .send()
            .await
            .into_sdk_error()
            .map_err(|e| {
                AlienError::new(ErrorData::ConfigurationError {
                    message: format!("Failed to reconcile deployment state: {e}"),
                })
            })?
            .into_inner();

        // Parse updated state from reconcile response
        let updated_state: DeploymentState = serde_json::from_value(
            serde_json::to_value(&reconcile_response.current).map_err(|e| {
                AlienError::new(ErrorData::ConfigurationError {
                    message: format!("Failed to serialize updated deployment state: {e}"),
                })
            })?,
        )
        .map_err(|e| {
            AlienError::new(ErrorData::ConfigurationError {
                message: format!("Failed to parse updated deployment state: {e}"),
            })
        })?;

        // Parse updated config from reconcile response if present
        let updated_config: Option<DeploymentConfig> = reconcile_response
            .target
            .as_ref()
            .map(|target| {
                let json = serde_json::to_value(&target.config).map_err(|e| {
                    AlienError::new(ErrorData::ConfigurationError {
                        message: format!("Failed to serialize deployment config: {e}"),
                    })
                })?;
                serde_json::from_value(json).map_err(|e| {
                    AlienError::new(ErrorData::ConfigurationError {
                        message: format!("Failed to parse updated deployment config: {e}"),
                    })
                })
            })
            .transpose()?;

        Ok(StepReconcileResult {
            state: Some(updated_state),
            config: updated_config,
        })
    }
}

/// Create authenticated platform client
fn create_authenticated_client(api_key: &str, base_url: &str) -> Result<SdkClient> {
    let auth_value = format!("Bearer {}", api_key);
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&auth_value)
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "Invalid authorization header value".to_string(),
            })?,
    );
    headers.insert(USER_AGENT, HeaderValue::from_static("alien-cli"));

    let reqwest_client = Client::builder()
        .default_headers(headers)
        .build()
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to create HTTP client".to_string(),
        })?;

    Ok(SdkClient::new_with_client(base_url, reqwest_client))
}

/// Main entry point for deploy command
pub async fn deploy_task(args: DeployArgs, ctx: ExecutionMode) -> Result<()> {
    if let ExecutionMode::Dev { port } = ctx {
        return deploy_local_dev_task(args, port).await;
    }

    info!("Starting deploy command");
    println!(
        "{}",
        contextual_heading("Deploying", &args.name, &[("to", &args.platform)])
    );
    let steps = FixedSteps::new(&[
        "Resolve deployment",
        "Acquire deployment",
        "Apply deployment",
        "Finalize",
    ]);
    steps.activate(0, Some(format!("Deployment {}", args.name)));

    // Parse platform
    let platform = Platform::from_str(&args.platform).map_err(|e| {
        AlienError::new(ErrorData::ValidationError {
            field: "platform".to_string(),
            message: e,
        })
    })?;

    let base_url = ctx.base_url();

    // Step 1: Load or register the deployment
    let mut tracker = DeploymentTracker::new()?;
    let tracked_deployment = match tracker.get_deployment(&args.name) {
        Some(deployment) => {
            info!("✅ Found tracked deployment '{}'", args.name);

            // If a token was provided, check if it's different from the stored one
            if let Some(ref provided_token) = args.token {
                if deployment.api_key != *provided_token {
                    info!("🔄 Updating stored API key for deployment '{}'", args.name);
                    // Remove old deployment and add with new token
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
                // No token provided, use stored token
                deployment.clone()
            }
        }
        None => {
            info!(
                "📝 Deployment '{}' not tracked yet, registering...",
                args.name
            );

            // Token is required for new deployments
            let token = args.token.as_ref().ok_or_else(|| {
                AlienError::new(ErrorData::ValidationError {
                    field: "token".to_string(),
                    message: format!(
                        "API key is required when deploying to a new deployment '{}'",
                        args.name
                    ),
                })
            })?;

            // Validate token type (deployment or deployment-group)
            let token_info = validate_token(token, &base_url).await?;

            match token_info {
                DeploymentToken::Deployment { .. } => {
                    // Deployment token - register directly
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
                    // Deployment group token - create deployment first
                    info!(
                        "   Using deployment group token for group '{}'",
                        deployment_group_name
                    );
                    info!("   Creating new deployment '{}'...", args.name);

                    // Create authenticated client with DG token
                    let sdk_client = create_authenticated_client(token, &base_url)?;

                    // Parse network settings from CLI flags
                    let network_settings =
                        network::parse_network_settings(&args.network, &args.platform).map_err(
                            |e| {
                                AlienError::new(ErrorData::ValidationError {
                                    field: "network".to_string(),
                                    message: e,
                                })
                            },
                        )?;

                    // Convert alien_core::NetworkSettings to SDK type via JSON roundtrip
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

                    // Build stack settings from args
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

                    // Create deployment via POST /deployments with DG token
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
                            deployment_group_id: None, // Auto-filled from DG token
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

                    // When using DG token, the API returns CreateDeploymentWithTokenResponse: { deployment, token }
                    // The SDK represents this as a union type
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
                                message: "Failed to extract deployment ID from response (expected 'deployment.id')".to_string(),
                            })
                        })?
                        .to_string();

                    let deployment_token = response_json
                        .get("token")
                        .and_then(|t| t.as_str())
                        .ok_or_else(|| {
                            AlienError::new(ErrorData::ConfigurationError {
                                message: "Server did not return deployment token (expected 'token' field)".to_string(),
                            })
                        })?
                        .to_string();

                    info!("   ✅ Deployment created: {}", deployment_id);
                    info!("   Tracking deployment with deployment token");

                    // Track the deployment with the returned deployment token (not the DG token)
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

    // Resolve workspace name for API calls that require it.
    // tracked_deployment.workspace_id is an ID (e.g. "ws_..."), but sync APIs expect a workspace name.
    let workspace_name = ctx.resolve_workspace().await?;

    // Create SDK client for subsequent operations
    let sdk_client = create_authenticated_client(&tracked_deployment.api_key, &base_url)?;

    // Get deployment
    let deployment_response = sdk_client
        .get_deployment()
        .id(&tracked_deployment.deployment_id)
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ConfigurationError {
            message: format!(
                "Failed to get deployment '{}' from platform.",
                tracked_deployment.deployment_id
            ),
        })?;

    let deployment = deployment_response.into_inner();
    // Generate unique session ID
    let hostname = hostname::get()
        .ok()
        .and_then(|h| h.into_string().ok())
        .unwrap_or_else(|| "unknown".to_string());
    let session_id = format!("cli-{}-{}", hostname, Uuid::new_v4());

    // Resolve manager ID from --manager flag
    // None (omitted): platform auto-resolves from deployment record
    // "none": explicitly no manager (e.g., bootstrapping the manager itself)
    // <id>: specific manager ID
    let manager_id = match args.manager.as_deref() {
        None | Some("none") => None,
        Some(id) => Some(id.try_into().map_err(
            |_: alien_platform_api::types::error::ConversionError| {
                AlienError::new(ErrorData::ConfigurationError {
                    message: "Invalid manager ID".to_string(),
                })
            },
        )?),
    };

    // Acquire deployment for deployment using sync API
    let acquire_request = alien_platform_api::types::SyncAcquireRequest {
        manager_id,
        session: session_id.clone(),
        deployment_ids: vec![tracked_deployment
            .deployment_id
            .as_str()
            .try_into()
            .map_err(|_: alien_platform_api::types::error::ConversionError| {
                AlienError::new(ErrorData::ConfigurationError {
                    message: "Invalid deployment ID format".to_string(),
                })
            })?],
        statuses: vec![],
        platforms: vec![],
        deployment_model: None,
        limit: std::num::NonZeroU64::new(1),
    };

    let acquire_response = sdk_client
        .sync_acquire()
        .workspace(&workspace_name)
        .body(acquire_request)
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to acquire deployment for deployment".to_string(),
        })?
        .into_inner();

    // Find this deployment in the response
    let deployment_context = acquire_response
        .deployments
        .iter()
        .find(|d| d.deployment_id.as_str() == tracked_deployment.deployment_id)
        .ok_or_else(|| {
            AlienError::new(ErrorData::ConfigurationError {
                message: format!(
                    "Deployment '{}' not found in acquire response",
                    tracked_deployment.deployment_id
                ),
            })
        })?;

    // Ensure state is released on exit
    let _guard = DeploymentGuard::new(
        sdk_client.clone(),
        workspace_name.clone(),
        tracked_deployment.deployment_id.clone(),
        session_id.clone(),
    );

    steps.activate(1, Some(tracked_deployment.deployment_id.clone()));

    // Extract deployment config
    let mut config: alien_deployment::DeploymentConfig = serde_json::from_value(
        serde_json::to_value(&deployment_context.config)
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "Failed to serialize deployment config".to_string(),
            })?,
    )
    .into_alien_error()
    .context(ErrorData::ConfigurationError {
        message: "Failed to parse deployment config".to_string(),
    })?;

    // Get cloud credentials from environment
    use alien_infra::ClientConfigExt;
    let client_config =
        ClientConfig::from_std_env(platform)
            .await
            .context(ErrorData::ConfigurationError {
                message: format!("Failed to build client config for platform {:?}", platform),
            })?;

    // Track current state
    let mut current: alien_deployment::DeploymentState = serde_json::from_value(
        serde_json::to_value(&deployment_context.current)
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "Failed to serialize initial deployment state".to_string(),
            })?,
    )
    .into_alien_error()
    .context(ErrorData::ConfigurationError {
        message: "Failed to parse initial deployment state".to_string(),
    })?;

    // Running deploy on a failed deployment is an implicit retry request
    if current.status.is_failed() {
        info!(
            "Deployment is in {:?} state, setting retry_requested to proceed",
            current.status
        );
        current.retry_requested = true;
    }

    steps.complete(1, Some(format!("{:?}", deployment.status)));
    steps.activate(2, Some(format!("{:?}", current.status)));
    if let Some(stack_state) = current.stack_state.as_ref() {
        steps.sync_deployment_resources(&stack_state.resources);
    }

    // Deployment loop — delegated to the shared runner with platform transport.
    let transport = PlatformCliTransport {
        sdk_client: sdk_client.clone(),
        workspace_name: workspace_name.clone(),
        session_id: session_id.clone(),
    };

    let policy = RunnerPolicy {
        max_steps: 400,
        operation: LoopOperation::Deploy,
        delay_threshold: None,
    };

    let RunnerResult {
        loop_result,
        steps_executed,
    } = alien_deployment::runner::run_step_loop(
        &mut current,
        &mut config,
        &client_config,
        &tracked_deployment.deployment_id,
        &policy,
        &transport,
        None,
    )
    .await
    .context(ErrorData::GenericError {
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
            steps.activate(3, Some("Promoting release".to_string()));
            steps.complete(3, Some("Deployment is running".to_string()));
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
            // Handoff to manager — deployment continues on the manager side.
            steps.complete(2, Some("Resources ready".to_string()));
            steps.activate(3, Some("Promoting release".to_string()));
            steps.complete(3, Some("Deployment is running".to_string()));
        }
        LoopOutcome::Neutral => {
            // Unexpected neutral without handoff — treat as failure.
            steps.fail(2, Some(format!("{:?}", loop_result.final_status)));
            return Err(AlienError::new(ErrorData::DeploymentFailed {
                message: format!(
                    "deployment loop ended without resolution (stop_reason: {:?}, status: {:?})",
                    loop_result.stop_reason, loop_result.final_status
                ),
            }));
        }
    }

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

    println!("{}", success_line("Deployment ready."));
    println!("{} {} ({})", dim_label("Deployment"), snapshot.deployment_name, snapshot.deployment_id);
    if snapshot.resources.is_empty() {
        println!("{}", dim_label("No public resource URLs were reported yet."));
    } else {
        println!("{}", dim_label("Resources"));
        for (name, resource) in snapshot.resources.iter() {
            println!(
                "  - {}{}{}",
                name,
                resource
                    .resource_type
                    .as_ref()
                    .map(|resource_type| format!(" ({resource_type})"))
                    .unwrap_or_default(),
                format!(": {}", resource.url)
            );
        }
    }
    println!(
        "{} inspect it with {}",
        dim_label("Next"),
        command(&format!("alien dev deployments get {}", snapshot.deployment_name))
    );

    Ok(())
}
