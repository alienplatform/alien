use crate::error::{ErrorData, Result};
use crate::execution_context::ExecutionMode;
use crate::interaction::{ConfirmationMode, InteractionMode};
use crate::output::prompt_confirm;
use crate::ui::{
    command, contextual_heading, deployment_resource_detail, dim_label, format_resource_status,
    heading, make_table, print_table, render_human_error, status_cell, success_line,
};
use alien_cli_common::network::{self, NetworkArgs};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_manager_api::types::DeploymentResponse;
use alien_manager_api::SdkResultExt as ManagerSdkResultExt;
use alien_platform_api::types::{
    CreateDeploymentTokenId, CreateDeploymentTokenRequest, CreateDeploymentTokenWorkspace,
    CreateDeploymentWorkspace, GetDeploymentId, GetDeploymentWorkspace, NewDeploymentRequest,
    PinDeploymentReleaseId, PinDeploymentReleaseWorkspace, PinReleaseRequest,
    PinReleaseRequestReleaseId,
};
use alien_platform_api::SdkResultExt as _;
use clap::{Parser, Subcommand, ValueEnum};

/// Telemetry (monitoring) mode for a deployment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum MonitoringMode {
    /// Automatically use the best available OTLP config: the parent AM's built-in DeepStore
    /// endpoint, or the AM's external OTLP integration (e.g. Axiom, Datadog).
    Auto,
    /// Disable all monitoring — no OTLP logs for containers or horizond VMs.
    Off,
}

#[derive(Parser, Debug, Clone)]
#[command(
    about = "Deployment commands",
    long_about = "Manage deployments in the Alien platform."
)]
pub struct DeploymentsArgs {
    #[command(subcommand)]
    pub cmd: DeploymentsCmd,
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
        /// "auto" (default) uses the parent AM's built-in DeepStore or external OTLP integration.
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
    #[command(alias = "list")]
    Ls {
        /// Project to list deployments for (optional, uses linked project by default)
        #[arg(long)]
        project: Option<String>,
    },
    /// Get deployment details
    Get {
        /// Deployment name or ID
        id: String,
    },
    /// Delete a deployment
    Delete {
        /// Deployment name or ID
        id: String,

        /// Skip confirmation prompt
        #[arg(long)]
        yes: bool,
    },
    /// Retry a deployment
    Retry {
        /// Deployment name or ID
        id: String,
    },
    /// Redeploy a deployment with the same release
    Redeploy {
        /// Deployment name or ID
        id: String,
    },
    /// Pin a deployment to a specific release
    Pin {
        /// Deployment ID
        id: String,
        /// Release ID to pin to (omit to unpin and use active release)
        release_id: Option<String>,
    },
    /// Create a deployment token (deployment-scoped API key)
    Token {
        /// Deployment ID
        id: String,
    },
}

pub async fn deployments_task(args: DeploymentsArgs, ctx: ExecutionMode) -> Result<()> {
    if let DeploymentsCmd::Delete { yes, .. } = &args.cmd {
        delete_confirmation_mode(*yes)?;
    }

    ctx.ensure_ready().await?;

    match args.cmd {
        // --- Manager API operations (all modes: dev, standalone, platform) ---
        DeploymentsCmd::Ls { project } => {
            let manager = resolve_manager_client(&ctx, project.as_deref()).await?;
            list_deployments_task(&manager).await
        }
        DeploymentsCmd::Get { id } => {
            let manager = resolve_manager_client(&ctx, None).await?;
            get_deployment_task(&manager, &id).await
        }
        DeploymentsCmd::Delete { id, yes } => {
            let manager = resolve_manager_client(&ctx, None).await?;
            delete_deployment_task(&manager, &id, yes).await
        }
        DeploymentsCmd::Retry { id } => {
            let manager = resolve_manager_client(&ctx, None).await?;
            retry_deployment_task(&manager, &id).await
        }
        DeploymentsCmd::Redeploy { id } => {
            let manager = resolve_manager_client(&ctx, None).await?;
            redeploy_deployment_task(&manager, &id).await
        }

        // --- Platform API operations (create, pin, token) ---
        DeploymentsCmd::Create {
            name,
            project,
            deployment_group,
            platform,
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
            let workspace_name = ctx.resolve_workspace().await?;

            create_deployment_task(
                &ctx,
                &client,
                &workspace_name,
                &name,
                &project,
                &deployment_group,
                &platform,
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
        DeploymentsCmd::Pin { id, release_id } => {
            if ctx.is_dev() {
                return Err(AlienError::new(ErrorData::ValidationError {
                    field: "command".to_string(),
                    message: "`alien dev deployments pin` is not supported in local dev mode."
                        .to_string(),
                }));
            }
            let client = ctx.sdk_client().await?;
            let workspace_name = ctx.resolve_workspace().await?;
            pin_deployment_task(&client, &workspace_name, &id, release_id).await
        }
        DeploymentsCmd::Token { id } => {
            if ctx.is_dev() {
                return Err(AlienError::new(ErrorData::ValidationError {
                    field: "command".to_string(),
                    message:
                        "`alien dev deployments token` is not supported in local dev mode."
                            .to_string(),
                }));
            }
            let client = ctx.sdk_client().await?;
            let workspace_name = ctx.resolve_workspace().await?;
            token_deployment_task(&client, &workspace_name, &id).await
        }
    }
}

// ---------------------------------------------------------------------------
// Manager client resolution
// ---------------------------------------------------------------------------

/// Resolve a manager API client for the current execution mode.
///
/// Uses the linked project (or `--project` override) to discover the manager URL
/// in platform mode. In dev/standalone modes, the manager URL is known directly.
async fn resolve_manager_client(
    ctx: &ExecutionMode,
    project_override: Option<&str>,
) -> Result<alien_manager_api::Client> {
    let (_, project_link) = ctx.resolve_project(project_override, true).await?;
    // The platform parameter is only used in platform mode for build-config
    // discovery; the manager URL is the same regardless of platform.
    let manager_ctx = ctx.resolve_manager(&project_link.project_id, "aws").await?;
    Ok(manager_ctx.client)
}

/// Resolve a deployment by name or ID.
async fn resolve_deployment_reference(
    client: &alien_manager_api::Client,
    reference: &str,
) -> Result<DeploymentResponse> {
    let response = client
        .list_deployments()
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: "listing deployments for resolution".to_string(),
            url: None,
        })?
        .into_inner();

    response
        .items
        .into_iter()
        .find(|d| d.id == reference || d.name == reference)
        .ok_or_else(|| {
            AlienError::new(ErrorData::ValidationError {
                field: "deployment".to_string(),
                message: format!("Deployment '{}' was not found.", reference),
            })
        })
}

// ---------------------------------------------------------------------------
// Manager API operations (unified for all modes)
// ---------------------------------------------------------------------------

async fn list_deployments_task(client: &alien_manager_api::Client) -> Result<()> {
    let response = client
        .list_deployments()
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: "listing deployments".to_string(),
            url: None,
        })?
        .into_inner();

    if response.items.is_empty() {
        println!("(no deployments)");
        return Ok(());
    }

    let mut table = make_table(&["Name", "ID", "Status", "Platform", "Release", "Updated"]);
    for deployment in &response.items {
        table.add_row(vec![
            deployment.name.clone().into(),
            deployment.id.clone().into(),
            status_cell(&deployment.status),
            deployment.platform.to_string().into(),
            deployment
                .current_release_id
                .clone()
                .unwrap_or_else(|| "—".to_string())
                .into(),
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

async fn get_deployment_task(
    client: &alien_manager_api::Client,
    reference: &str,
) -> Result<()> {
    let deployment = resolve_deployment_reference(client, reference).await?;

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

    if let Some(stack_state) = &deployment.stack_state {
        let stack_state: alien_core::StackState = serde_json::from_value(stack_state.clone())
            .into_alien_error()
            .context(ErrorData::JsonError {
                operation: "deserialization".to_string(),
                reason: "Failed to convert deployment stack state".to_string(),
            })?;
        print_stack_resources(&stack_state);
    }

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

async fn delete_deployment_task(
    client: &alien_manager_api::Client,
    reference: &str,
    yes: bool,
) -> Result<()> {
    let confirmation_mode = delete_confirmation_mode(yes)?;
    let deployment = resolve_deployment_reference(client, reference).await?;

    println!(
        "{}",
        contextual_heading("Deleting deployment", &deployment.name, &[])
    );
    println!("{} {}", dim_label("ID"), deployment.id);
    println!("{} {}", dim_label("Status"), deployment.status);

    if matches!(confirmation_mode, ConfirmationMode::Prompt)
        && !prompt_confirm("Are you sure you want to delete this deployment?", false)?
    {
        println!("{}", dim_label("Deletion cancelled."));
        return Ok(());
    }

    client
        .delete_deployment()
        .id(&deployment.id)
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: "deleting deployment".to_string(),
            url: None,
        })?;

    println!("{}", success_line("Delete requested."));
    println!(
        "{} {}",
        dim_label("Next"),
        command(&format!("alien deployments get {}", deployment.id))
    );

    Ok(())
}

async fn retry_deployment_task(
    client: &alien_manager_api::Client,
    reference: &str,
) -> Result<()> {
    let deployment = resolve_deployment_reference(client, reference).await?;

    println!(
        "{}",
        contextual_heading("Retrying deployment", &deployment.name, &[])
    );
    println!("{} {}", dim_label("ID"), deployment.id);
    println!("{} {}", dim_label("Status"), deployment.status);

    client
        .retry_deployment()
        .id(&deployment.id)
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: "retrying deployment".to_string(),
            url: None,
        })?;

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
) -> Result<()> {
    let deployment = resolve_deployment_reference(client, reference).await?;

    println!(
        "{}",
        contextual_heading("Redeploying deployment", &deployment.name, &[])
    );
    println!("{} {}", dim_label("ID"), deployment.id);
    println!("{} {}", dim_label("Status"), deployment.status);
    if let Some(current_release_id) = &deployment.current_release_id {
        println!("{} {}", dim_label("Current release"), current_release_id);
    }

    client
        .redeploy()
        .id(&deployment.id)
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: "redeploying deployment".to_string(),
            url: None,
        })?;

    println!("{}", success_line("Redeploy requested."));
    println!(
        "{} {}",
        dim_label("Next"),
        command(&format!("alien deployments get {}", deployment.id))
    );

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
        crate::project_link::get_project_by_name(&http, workspace, project_name).await?
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
    };

    let request = NewDeploymentRequest {
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
        stack_settings: Some(stack_settings),
        environment_variables: if variables.is_empty() {
            None
        } else {
            Some(variables)
        },
        manager_id: None,
        pinned_release_id: None,
        environment_info: None,
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
        if let Some(env_vars) = &deployment.environment_variables {
            println!("   Environment Variables: {} configured", env_vars.len());
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
