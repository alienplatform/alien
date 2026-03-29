use crate::error::{ErrorData, Result};
use crate::execution_context::ExecutionMode;
use crate::get_current_dir;
use crate::interaction::{ConfirmationMode, InteractionMode};
use crate::output::prompt_confirm;
use crate::ui::{
    command, contextual_heading, deployment_resource_detail, dim_label, format_resource_status,
    heading, render_human_error, success_line,
};
use alien_cli_common::network::{self, NetworkArgs};
use alien_error::{AlienError, Context, IntoAlienError};
use clap::{Parser, Subcommand, ValueEnum};
use serde::{de::DeserializeOwned, Deserialize, Serialize};

/// Telemetry (monitoring) mode for a deployment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum MonitoringMode {
    /// Automatically use the best available OTLP config: the parent AM's built-in DeepStore
    /// endpoint, or the AM's external OTLP integration (e.g. Axiom, Datadog).
    Auto,
    /// Disable all monitoring — no OTLP logs for containers or horizond VMs.
    Off,
}
use alien_platform_api::types::{
    CreateDeploymentTokenId, CreateDeploymentTokenRequest, CreateDeploymentTokenWorkspace,
    DeleteDeploymentId, DeleteDeploymentWorkspace, GetDeploymentId, GetDeploymentWorkspace,
    ListDeploymentsWorkspace, PinDeploymentReleaseId, PinDeploymentReleaseWorkspace,
    PinReleaseRequest, PinReleaseRequestReleaseId, RedeployDeploymentId,
    RedeployDeploymentWorkspace, RetryDeploymentId, RetryDeploymentWorkspace,
};
use alien_platform_api::SdkResultExt as _;
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

        /// Platform (aws, gcp, azure, kubernetes, or local)
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
        /// Deployment ID
        id: String,
    },
    /// Delete a deployment
    Delete {
        /// Deployment ID
        id: String,

        /// Skip confirmation prompt
        #[arg(long)]
        yes: bool,
    },
    /// Retry a deployment
    Retry {
        /// Deployment ID
        id: String,
    },
    /// Redeploy a deployment with the same release
    Redeploy {
        /// Deployment ID
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

    // Ensure server is ready (dev mode only)
    ctx.ensure_ready().await?;

    if let ExecutionMode::Dev { port } = ctx {
        return deployments_task_dev(args, port).await;
    }

    // Get client
    let client = ctx.sdk_client().await?;

    // Resolve workspace (flag override -> profile.json -> interactive prompt)
    let workspace_name = ctx.resolve_workspace().await?;

    match args.cmd {
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
            // Dev mode validation
            if ctx.is_dev() && platform != "local" {
                return Err(AlienError::new(ErrorData::ValidationError {
                    field: "platform".to_string(),
                    message: "Dev mode only supports 'local' platform. Use platform mode to deploy to AWS, GCP, Azure, or Kubernetes.".to_string(),
                }));
            }

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
            .await?;
        }
        DeploymentsCmd::Ls { project } => {
            // Merge subcommand --project with global --project
            let effective_project = project.or_else(|| ctx.project_override().map(String::from));
            list_deployments_task(&client, &workspace_name, effective_project).await?;
        }
        DeploymentsCmd::Get { id } => {
            get_deployment_task(&client, &workspace_name, &id).await?;
        }
        DeploymentsCmd::Delete { id, yes } => {
            delete_deployment_task(&client, &workspace_name, &id, yes).await?;
        }
        DeploymentsCmd::Retry { id } => {
            retry_deployment_task(&client, &workspace_name, &id).await?;
        }
        DeploymentsCmd::Redeploy { id } => {
            redeploy_deployment_task(&client, &workspace_name, &id).await?;
        }
        DeploymentsCmd::Pin { id, release_id } => {
            pin_deployment_task(&client, &workspace_name, &id, release_id).await?;
        }
        DeploymentsCmd::Token { id } => {
            token_deployment_task(&client, &workspace_name, &id).await?;
        }
    }

    Ok(())
}

async fn deployments_task_dev(args: DeploymentsArgs, port: u16) -> Result<()> {
    match args.cmd {
        DeploymentsCmd::Create { .. } => Err(AlienError::new(ErrorData::ValidationError {
            field: "command".to_string(),
            message:
                "Use `alien dev deploy --name <deployment> --platform local` to create deployments in dev mode."
                    .to_string(),
        })),
        DeploymentsCmd::Ls { .. } => list_local_deployments_task(port).await,
        DeploymentsCmd::Get { id } => get_local_deployment_task(port, &id).await,
        DeploymentsCmd::Delete { id, yes } => delete_local_deployment_task(port, &id, yes).await,
        DeploymentsCmd::Retry { id } => retry_local_deployment_task(port, &id).await,
        DeploymentsCmd::Redeploy { id } => redeploy_local_deployment_task(port, &id).await,
        DeploymentsCmd::Pin { .. } => Err(AlienError::new(ErrorData::ValidationError {
            field: "command".to_string(),
            message: "`alien dev deployments pin` is not supported in local dev mode.".to_string(),
        })),
        DeploymentsCmd::Token { .. } => Err(AlienError::new(ErrorData::ValidationError {
            field: "command".to_string(),
            message:
                "`alien dev deployments token` is not supported in local dev mode.".to_string(),
        })),
    }
}

// Deployment creation task - creates a new deployment with environment variables
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
    use alien_platform_api::types::{CreateDeploymentWorkspace, NewDeploymentRequest};

    // Get project - in dev mode use constant, in platform mode resolve by name
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

    // Parse platform
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
                    "Unknown platform: {}. Valid values: aws, gcp, azure, kubernetes (k8s), local",
                    platform_str
                ),
            })
            .into());
        }
    };

    // Build environment variables array
    let mut variables: Vec<alien_platform_api::types::EnvironmentVariableConfig> = Vec::new();

    // Parse plain env vars (--env KEY=VALUE)
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

    // Parse secret env vars (--secret KEY=VALUE)
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

    // Parse targeted plain env vars (--env-targeted KEY=VALUE:pattern1,pattern2)
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

    // Parse targeted secret env vars (--secret-targeted KEY=VALUE:pattern1,pattern2)
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

    // Parse network settings from CLI flags
    let network_settings =
        network::parse_network_settings(network_args, platform_str).map_err(|e| {
            AlienError::new(ErrorData::ValidationError {
                field: "network".to_string(),
                message: e,
            })
        })?;

    // Convert alien_core::NetworkSettings to SDK type via JSON roundtrip
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

    // Build stack settings
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
    };

    // Create the deployment request
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

    // Make API call
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

    // Extract deployment and optional token from response
    let deployment = &deployment_response.deployment;
    let token = deployment_response.token.as_ref();

    // Output result
    if format == "json" {
        let json = serde_json::to_string_pretty(&deployment)
            .into_alien_error()
            .context(ErrorData::JsonError {
                operation: "serialization".to_string(),
                reason: "Failed to serialize deployment response".to_string(),
            })?;
        println!("{}", json);
    } else {
        println!("✅ Deployment created successfully!");
        println!("   ID: {}", *deployment.id);
        println!("   Name: {}", *deployment.name);
        println!("   Project: {}", project.project_name);
        println!("   Platform: {:?}", deployment.platform);
        println!("   Deployment Group: {}", *deployment.deployment_group_id);
        println!("   Status: {:?}", deployment.status);
        if let Some(env_vars) = &deployment.environment_variables {
            // Environment variables is a Vec<EnvironmentVariableConfig>
            println!("   Environment Variables: {} configured", env_vars.len());
        }
        if let Some(token) = token {
            println!("   Token: {}", token);
        }
    }

    Ok(())
}

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
    // First split by '=' to get KEY and VALUE:patterns
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

    // Split patterns by ','
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
        // Test URL with https:// (contains colon)
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
        // Test URL with port number (multiple colons)
        let result = parse_targeted_env_var("API_URL=https://example.com:8080:api-*");
        assert!(result.is_some());
        let (key, value, patterns) = result.unwrap();
        assert_eq!(key, "API_URL");
        assert_eq!(value, "https://example.com:8080");
        assert_eq!(patterns, vec!["api-*"]);
    }

    #[test]
    fn test_parse_targeted_env_var_multiple_patterns() {
        // Test multiple patterns
        let result = parse_targeted_env_var("DATABASE_URL=postgres://localhost:api-*,worker");
        assert!(result.is_some());
        let (key, value, patterns) = result.unwrap();
        assert_eq!(key, "DATABASE_URL");
        assert_eq!(value, "postgres://localhost");
        assert_eq!(patterns, vec!["api-*", "worker"]);
    }

    #[test]
    fn test_parse_targeted_env_var_simple() {
        // Test simple value without colons
        let result = parse_targeted_env_var("LOG_LEVEL=info:api-*");
        assert!(result.is_some());
        let (key, value, patterns) = result.unwrap();
        assert_eq!(key, "LOG_LEVEL");
        assert_eq!(value, "info");
        assert_eq!(patterns, vec!["api-*"]);
    }
}

async fn list_deployments_task(
    client: &alien_platform_api::Client,
    workspace: &str,
    project: Option<String>,
) -> Result<()> {
    // Use provided project or fall back to linked project name
    let project_filter = match project {
        Some(p) => Some(p),
        None => {
            let current_dir = get_current_dir()?;
            match crate::project_link::get_project_link_status(&current_dir) {
                crate::project_link::ProjectLinkStatus::Linked(link) => Some(link.project_name),
                _ => None,
            }
        }
    };

    let workspace_param = ListDeploymentsWorkspace::try_from(workspace)
        .into_alien_error()
        .context(ErrorData::ValidationError {
            field: "workspace".to_string(),
            message: "workspace name format is invalid".to_string(),
        })?;

    // Pass directly to API - no ID resolution needed
    let mut list_builder = client.list_deployments();
    if let Some(ref project) = project_filter {
        list_builder = list_builder.project(project);
    }
    let response = list_builder
        .workspace(&workspace_param)
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: "listing deployments".to_string(),
            url: None,
        })?;
    let deployments_response = response.into_inner();

    if deployments_response.items.is_empty() {
        println!("(no deployments)");
    } else {
        // Pretty print deployments list
        for deployment in &deployments_response.items {
            println!("Deployment ID: {}", deployment.id.as_str());
            println!("  Name: {}", deployment.name.as_str());
            println!("  Status: {:?}", deployment.status);
            println!("  Platform: {:?}", deployment.platform);

            if let Some(current_release_id) = &deployment.current_release_id {
                println!("  Current Release: {}", current_release_id.as_str());
            }

            if let Some(last_heartbeat) = &deployment.last_heartbeat_at {
                println!("  Last Heartbeat: {}", last_heartbeat);
            }

            println!("  Created: {}", deployment.created_at);
            println!();
        }
    }

    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LocalDeploymentsResponse {
    items: Vec<LocalDeploymentResponse>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LocalDeploymentResponse {
    id: String,
    name: String,
    platform: String,
    status: String,
    current_release_id: Option<String>,
    created_at: String,
    updated_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LocalActionResponse {
    #[allow(dead_code)]
    success: Option<bool>,
    #[allow(dead_code)]
    message: Option<String>,
}

async fn list_local_deployments_task(port: u16) -> Result<()> {
    let response: LocalDeploymentsResponse =
        local_manager_request(port, reqwest::Method::GET, "/v1/deployments", None).await?;

    if response.items.is_empty() {
        println!("(no deployments)");
        return Ok(());
    }

    for deployment in response.items {
        print_local_deployment(&deployment);
        println!();
    }

    Ok(())
}

async fn get_local_deployment_task(port: u16, deployment_id: &str) -> Result<()> {
    let path = format!("/v1/deployments/{deployment_id}");
    let response: LocalDeploymentResponse =
        local_manager_request(port, reqwest::Method::GET, &path, None).await?;
    print_local_deployment(&response);
    Ok(())
}

async fn delete_local_deployment_task(port: u16, deployment_id: &str, yes: bool) -> Result<()> {
    if !yes {
        let confirmed = prompt_confirm(
            &format!("Delete deployment '{deployment_id}' from the local manager?"),
            false,
        )?;
        if !confirmed {
            return Err(AlienError::new(ErrorData::UserCancelled));
        }
    }

    let path = format!("/v1/deployments/{deployment_id}");
    let response: LocalActionResponse =
        local_manager_request(port, reqwest::Method::DELETE, &path, None).await?;

    println!(
        "{}",
        response
            .message
            .as_deref()
            .unwrap_or("Deployment deletion enqueued")
    );

    Ok(())
}

async fn retry_local_deployment_task(port: u16, deployment_id: &str) -> Result<()> {
    let path = format!("/v1/deployments/{deployment_id}/retry");
    let _: LocalActionResponse =
        local_manager_request(port, reqwest::Method::POST, &path, None).await?;
    println!("Retry requested for deployment '{deployment_id}'.");
    Ok(())
}

async fn redeploy_local_deployment_task(port: u16, deployment_id: &str) -> Result<()> {
    let path = format!("/v1/deployments/{deployment_id}/redeploy");
    let _: LocalActionResponse =
        local_manager_request(port, reqwest::Method::POST, &path, None).await?;
    println!("Redeploy requested for deployment '{deployment_id}'.");
    Ok(())
}

fn print_local_deployment(deployment: &LocalDeploymentResponse) {
    println!("Deployment ID: {}", deployment.id);
    println!("  Name: {}", deployment.name);
    println!("  Status: {}", deployment.status);
    println!("  Platform: {}", deployment.platform);

    if let Some(current_release_id) = &deployment.current_release_id {
        println!("  Current Release: {}", current_release_id);
    }

    println!("  Created: {}", deployment.created_at);

    if let Some(updated_at) = &deployment.updated_at {
        println!("  Updated: {}", updated_at);
    }
}

async fn local_manager_request<T: DeserializeOwned>(
    port: u16,
    method: reqwest::Method,
    path: &str,
    body: Option<serde_json::Value>,
) -> Result<T> {
    let url = format!("http://localhost:{port}{path}");
    let client = reqwest::Client::new();
    let request = client.request(method, &url);
    let request = if let Some(body) = body {
        request.json(&body)
    } else {
        request
    };

    let response =
        request
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::ApiRequestFailed {
                message: format!("calling local manager endpoint {path}"),
                url: Some(url.clone()),
            })?;

    let status = response.status();
    let body = response
        .text()
        .await
        .into_alien_error()
        .context(ErrorData::ApiRequestFailed {
            message: format!("reading local manager response from {path}"),
            url: Some(url.clone()),
        })?;

    if !status.is_success() {
        let message = if body.trim().is_empty() {
            format!("local manager returned HTTP {}", status.as_u16())
        } else {
            body
        };

        return Err(AlienError::new(ErrorData::ApiRequestFailed {
            message,
            url: Some(url),
        }));
    }

    serde_json::from_str(&body)
        .into_alien_error()
        .context(ErrorData::JsonError {
            operation: "deserialization".to_string(),
            reason: format!("Failed to parse local manager response from {path}"),
        })
}

async fn get_deployment_task(
    client: &alien_platform_api::Client,
    workspace: &str,
    deployment_id: &str,
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
        contextual_heading("Showing deployment", deployment.name.as_ref(), &[])
    );
    println!("{} {}", dim_label("ID"), *deployment.id);
    println!("{} {:?}", dim_label("Status"), deployment.status);
    println!("{} {:?}", dim_label("Platform"), deployment.platform);
    println!("{} {}", dim_label("Project"), *deployment.project_id);
    println!("{} {}", dim_label("Workspace"), *deployment.workspace_id);
    println!("{} {}", dim_label("Created"), deployment.created_at);

    if let Some(last_heartbeat) = &deployment.last_heartbeat_at {
        println!("{} {}", dim_label("Last heartbeat"), last_heartbeat);
    }

    if let Some(current_release_id) = &deployment.current_release_id {
        println!("{} {}", dim_label("Current release"), **current_release_id);
    }

    if let Some(pinned_release_id) = &deployment.pinned_release_id {
        println!("{} {}", dim_label("Pinned release"), **pinned_release_id);
    }

    if let Some(desired_release_id) = &deployment.desired_release_id {
        println!("{} {}", dim_label("Desired release"), **desired_release_id);
    }

    if let Some(manager_id) = &deployment.manager_id {
        println!("{} {:?}", dim_label("Manager"), manager_id);
    }

    if let Some(error) = &deployment.error {
        let error: alien_error::AlienError = convert_via_json(
            error,
            "deployment error",
            "Failed to convert deployment error",
        )?;
        println!("{}", render_human_error(&error));
    }

    if let Some(stack_state) = &deployment.stack_state {
        let stack_state: alien_core::StackState = convert_via_json(
            stack_state,
            "deployment stack state",
            "Failed to convert deployment stack state",
        )?;
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
    client: &alien_platform_api::Client,
    workspace: &str,
    deployment_id: &str,
    yes: bool,
) -> Result<()> {
    let confirmation_mode = delete_confirmation_mode(yes)?;

    // Get deployment details first for confirmation
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
        contextual_heading("Deleting deployment", deployment.name.as_ref(), &[])
    );
    println!("{} {}", dim_label("ID"), *deployment.id);
    println!("{} {:?}", dim_label("Status"), deployment.status);

    // Confirm deletion
    if matches!(confirmation_mode, ConfirmationMode::Prompt)
        && !prompt_confirm("Are you sure you want to delete this deployment?", false)?
    {
        println!("{}", dim_label("Deletion cancelled."));
        return Ok(());
    }

    let delete_workspace_param = DeleteDeploymentWorkspace::try_from(workspace)
        .into_alien_error()
        .context(ErrorData::ValidationError {
            field: "workspace".to_string(),
            message: "workspace name format is invalid".to_string(),
        })?;
    let delete_deployment_id_param = DeleteDeploymentId::try_from(deployment_id)
        .into_alien_error()
        .context(ErrorData::ValidationError {
            field: "deployment_id".to_string(),
            message: "deployment ID format is invalid".to_string(),
        })?;
    let _response = client
        .delete_deployment()
        .id(&delete_deployment_id_param)
        .workspace(&delete_workspace_param)
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
        command(&format!("alien deployments get {}", deployment_id))
    );

    Ok(())
}

fn delete_confirmation_mode(yes: bool) -> Result<ConfirmationMode> {
    InteractionMode::current(false).confirmation_mode(
        yes,
        "Deployment deletion requires a real terminal. Re-run with `--yes`.",
    )
}

async fn retry_deployment_task(
    client: &alien_platform_api::Client,
    workspace: &str,
    deployment_id: &str,
) -> Result<()> {
    // Get deployment details first to show what we're retrying
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
        contextual_heading("Retrying deployment", deployment.name.as_ref(), &[])
    );
    println!("{} {}", dim_label("ID"), *deployment.id);
    println!("{} {:?}", dim_label("Status"), deployment.status);
    let retry_workspace_param = RetryDeploymentWorkspace::try_from(workspace)
        .into_alien_error()
        .context(ErrorData::ValidationError {
            field: "workspace".to_string(),
            message: "workspace name format is invalid".to_string(),
        })?;
    let retry_deployment_id_param = RetryDeploymentId::try_from(deployment_id)
        .into_alien_error()
        .context(ErrorData::ValidationError {
            field: "deployment_id".to_string(),
            message: "deployment ID format is invalid".to_string(),
        })?;
    let response: alien_platform_api::ResponseValue<
        alien_platform_api::types::RetryDeploymentResponse,
    > = client
        .retry_deployment()
        .id(&retry_deployment_id_param)
        .workspace(&retry_workspace_param)
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: "retrying deployment".to_string(),
            url: None,
        })?;
    let retry_response = response.into_inner();

    println!("{}", success_line("Retry requested."));
    println!("{} {}", dim_label("Status"), retry_response.message);
    println!(
        "{} {}",
        dim_label("Next"),
        command(&format!("alien deployments get {}", deployment_id))
    );

    Ok(())
}

async fn redeploy_deployment_task(
    client: &alien_platform_api::Client,
    workspace: &str,
    deployment_id: &str,
) -> Result<()> {
    // Get deployment details first to show what we're redeploying
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
        contextual_heading("Redeploying deployment", deployment.name.as_ref(), &[])
    );
    println!("{} {}", dim_label("ID"), *deployment.id);
    println!("{} {:?}", dim_label("Status"), deployment.status);
    if let Some(current_release_id) = &deployment.current_release_id {
        println!("{} {}", dim_label("Current release"), **current_release_id);
    }
    let redeploy_workspace_param = RedeployDeploymentWorkspace::try_from(workspace)
        .into_alien_error()
        .context(ErrorData::ValidationError {
            field: "workspace".to_string(),
            message: "workspace name format is invalid".to_string(),
        })?;
    let redeploy_deployment_id_param = RedeployDeploymentId::try_from(deployment_id)
        .into_alien_error()
        .context(ErrorData::ValidationError {
            field: "deployment_id".to_string(),
            message: "deployment ID format is invalid".to_string(),
        })?;
    let response = client
        .redeploy_deployment()
        .id(&redeploy_deployment_id_param)
        .workspace(&redeploy_workspace_param)
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: "redeploying deployment".to_string(),
            url: None,
        })?;
    let redeploy_response = response.into_inner();

    println!("{}", success_line("Redeploy requested."));
    println!("{} {}", dim_label("Status"), redeploy_response.message);
    println!(
        "{} {}",
        dim_label("Next"),
        command(&format!("alien deployments get {}", deployment_id))
    );

    Ok(())
}

fn print_stack_resources(stack_state: &alien_core::StackState) {
    println!("{}", heading("Resources"));
    let mut resources: Vec<_> = stack_state.resources.iter().collect();
    resources.sort_by(|(left_name, _), (right_name, _)| left_name.cmp(right_name));

    for (resource_name, resource) in resources {
        println!(
            "  - {} ({}): {}",
            resource_name,
            resource.resource_type,
            format_resource_status(resource.status)
        );
        if let Some(detail) = deployment_resource_detail(resource) {
            println!("    {}", detail);
        }
    }
}

fn convert_via_json<T, U>(value: &T, operation_target: &str, reason: &str) -> Result<U>
where
    T: Serialize,
    U: serde::de::DeserializeOwned,
{
    serde_json::from_value(serde_json::to_value(value).into_alien_error().context(
        ErrorData::JsonError {
            operation: "serialization".to_string(),
            reason: format!("Failed to serialize {operation_target}"),
        },
    )?)
    .into_alien_error()
    .context(ErrorData::JsonError {
        operation: "deserialization".to_string(),
        reason: reason.to_string(),
    })
}

async fn pin_deployment_task(
    client: &alien_platform_api::Client,
    workspace: &str,
    deployment_id: &str,
    release_id: Option<String>,
) -> Result<()> {
    // Get deployment details first to show what we're pinning
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

    // Show what we're about to pin
    println!("📌 About to update deployment:");
    println!("   ID: {}", *deployment.id);
    println!("   Name: {}", *deployment.name);
    println!("   Status: {:?}", deployment.status);

    // Pin/unpin the deployment
    if let Some(ref release_id_str) = release_id {
        println!("📌 Pinning deployment to release: {}", release_id_str);
    } else {
        println!("📌 Unpinning deployment (will use active release)");
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

    // Create pin request
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

    println!("✅ Deployment pin updated successfully!");
    println!("   Message: {}", pin_response.message);

    Ok(())
}

// Create a deployment-scoped API key for an existing deployment
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
