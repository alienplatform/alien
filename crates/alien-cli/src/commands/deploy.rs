use crate::commands::deployments::MonitoringMode;
use crate::deployment_tracking::{validate_token, DeploymentToken, DeploymentTracker};
use crate::error::{ErrorData, Result};
use crate::execution_context::ExecutionMode;
use alien_cli_common::network::{self, NetworkArgs};
use alien_core::{ClientConfig, Platform};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_platform_api::Client as SdkClient;
use alien_platform_api::SdkResultExt;
use clap::Parser;
use reqwest::{
    header::{HeaderMap, HeaderValue, AUTHORIZATION, USER_AGENT},
    Client,
};
use std::str::FromStr;
use tokio::time::{sleep, Duration};
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
    info!("Starting deploy command");

    // Parse platform
    let platform = Platform::from_str(&args.platform).map_err(|e| {
        AlienError::new(ErrorData::ValidationError {
            field: "platform".to_string(),
            message: e,
        })
    })?;

    info!("🚀 Deploying application to deployment '{}'", args.name);

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

    info!("   Deployment ID: {}", tracked_deployment.deployment_id);
    info!("   Workspace ID: {}", tracked_deployment.workspace_id);

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
    info!("📊 Current deployment status: {:?}", deployment.status);

    // Generate unique session ID
    let hostname = hostname::get()
        .ok()
        .and_then(|h| h.into_string().ok())
        .unwrap_or_else(|| "unknown".to_string());
    let session_id = format!("cli-{}-{}", hostname, Uuid::new_v4());

    info!("📋 Session ID: {}", session_id);

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

    info!("📊 Deployment acquired for deployment");

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

    // Deployment loop
    let max_steps = 400;
    let mut step_count = 0;

    loop {
        step_count += 1;
        if step_count > max_steps {
            return Err(AlienError::new(ErrorData::GenericError {
                message: format!("Deployment exceeded maximum steps ({})", max_steps),
            }));
        }

        info!(
            "Step {}: Deployment status = {:?}",
            step_count, current.status
        );

        // Call alien_deployment::step() directly
        let step_result = alien_deployment::step(
            current.clone(),
            config.clone(),
            client_config.clone(),
            None, // Use default PlatformServiceProvider
        )
        .await
        .context(ErrorData::GenericError {
            message: "deployment step failed".to_string(),
        })?;

        // Check if deployment is complete
        let is_success_release = step_result.state.status.is_synced()
            && matches!(
                step_result.state.status,
                alien_deployment::DeploymentStatus::Running
            );

        if is_success_release {
            info!("🎉 Deployment complete!");
        }

        // Check for failure states
        let is_failure = matches!(
            step_result.state.status,
            alien_deployment::DeploymentStatus::ProvisioningFailed
                | alien_deployment::DeploymentStatus::UpdateFailed
                | alien_deployment::DeploymentStatus::DeleteFailed
                | alien_deployment::DeploymentStatus::RefreshFailed
                | alien_deployment::DeploymentStatus::InitialSetupFailed
        );

        // Apply update to platform using sync API
        let state_sdk: alien_platform_api::types::SyncReconcileRequestState =
            serde_json::from_value(
                serde_json::to_value(&step_result.state)
                    .into_alien_error()
                    .context(ErrorData::ConfigurationError {
                        message: "Failed to serialize deployment state".to_string(),
                    })?,
            )
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "Failed to convert state to SDK type".to_string(),
            })?;

        let error_sdk = step_result
            .error
            .as_ref()
            .map(|e| {
                serde_json::from_value(serde_json::to_value(e).into_alien_error().context(
                    ErrorData::ConfigurationError {
                        message: "Failed to serialize deployment error".to_string(),
                    },
                )?)
                .into_alien_error()
                .context(ErrorData::ConfigurationError {
                    message: "Failed to convert error to SDK type".to_string(),
                })
            })
            .transpose()?;

        // Convert deployment_id to typed ID
        let deployment_id_typed: alien_platform_api::types::SyncReconcileRequestDeploymentId =
            tracked_deployment
                .deployment_id
                .as_str()
                .try_into()
                .map_err(|_: alien_platform_api::types::error::ConversionError| {
                    AlienError::new(ErrorData::ConfigurationError {
                        message: "Invalid deployment ID format".to_string(),
                    })
                })?;

        let reconcile_request = alien_platform_api::types::SyncReconcileRequest {
            deployment_id: deployment_id_typed,
            session: Some(session_id.clone()),
            state: state_sdk,
            error: error_sdk,
            update_heartbeat: Some(step_result.update_heartbeat),
        };

        let reconcile_response = sdk_client
            .sync_reconcile()
            .workspace(&workspace_name)
            .body(reconcile_request)
            .send()
            .await
            .into_sdk_error()
            .context(ErrorData::ConfigurationError {
                message: "Failed to reconcile deployment state".to_string(),
            })?
            .into_inner();

        // Handle failures AFTER the platform has been updated
        if is_failure {
            let failed_status = step_result.state.status;

            let operation = match failed_status {
                alien_deployment::DeploymentStatus::InitialSetupFailed => "initial setup",
                alien_deployment::DeploymentStatus::ProvisioningFailed => "provisioning",
                alien_deployment::DeploymentStatus::UpdateFailed => "update",
                alien_deployment::DeploymentStatus::DeleteFailed => "deletion",
                _ => "deployment",
            };

            if let Some(error) = step_result.error {
                return Err(AlienError::new(ErrorData::DeploymentFailed {
                    message: format!("{}: {}", operation, error),
                }));
            } else {
                return Err(AlienError::new(ErrorData::DeploymentFailed {
                    message: operation.to_string(),
                }));
            }
        }

        // Handle success
        if is_success_release {
            break;
        }

        // Update current state for next iteration
        current = serde_json::from_value(
            serde_json::to_value(&reconcile_response.current)
                .into_alien_error()
                .context(ErrorData::ConfigurationError {
                    message: "Failed to serialize updated deployment state".to_string(),
                })?,
        )
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to parse updated deployment state".to_string(),
        })?;

        // Refresh config from the platform's latest view so domain_metadata
        // (cert/DNS status, cert chain), env vars, and other platform-side
        // fields stay current across the whole deployment loop.
        if let Some(target) = &reconcile_response.target {
            config = serde_json::from_value(
                serde_json::to_value(&target.config)
                    .into_alien_error()
                    .context(ErrorData::ConfigurationError {
                        message: "Failed to serialize deployment config".to_string(),
                    })?,
            )
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "Failed to parse updated deployment config".to_string(),
            })?;
        }

        // Wait if suggested
        if let Some(delay_ms) = step_result.suggested_delay_ms {
            sleep(Duration::from_millis(delay_ms)).await;
        }
    }

    Ok(())
}
