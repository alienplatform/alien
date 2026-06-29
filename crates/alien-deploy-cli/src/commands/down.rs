//! Destroy command — tears down a deployment.

use crate::deployment_tracking::DeploymentTracker;
use crate::error::{ErrorData, Result};
use crate::output;
use alien_core::embedded_config::DeployCliConfig;
use alien_core::{ClientConfig, Platform};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_infra::ClientConfigExt;
use clap::Parser;
use std::{path::PathBuf, str::FromStr};

use super::up::{create_manager_client, push_deletion, read_token_file};

#[derive(Parser, Debug, Clone)]
#[command(
    about = "Destroy a deployment and its resources",
    after_help = "EXAMPLES:
    # Destroy a tracked deployment
    alien-deploy destroy --name production

    # Force-delete an imported deployment record
    alien-deploy destroy --name production --force-delete-record

    # Destroy using explicit token
    alien-deploy destroy --name production --token ax_dg_abc123... --manager-url https://manager.example.com"
)]
pub struct DownArgs {
    /// Deployment name
    #[arg(long)]
    pub name: String,

    /// Authentication token (optional if deployment is tracked)
    #[arg(long, env = "ALIEN_TOKEN")]
    pub token: Option<String>,

    /// Read authentication token from a file.
    #[arg(long, conflicts_with = "token")]
    pub token_file: Option<PathBuf>,

    /// Manager URL (optional if deployment is tracked)
    #[arg(long, env = "ALIEN_MANAGER_URL")]
    pub manager_url: Option<String>,

    /// Force deletion — skip resource teardown, just remove the deployment record
    #[arg(long = "force-delete-record", alias = "force")]
    pub force_delete_record: bool,

    /// Skip confirmation prompt
    #[arg(long, short = 'y')]
    pub yes: bool,
}

pub async fn down_command(args: DownArgs, embedded_config: Option<&DeployCliConfig>) -> Result<()> {
    let tracker = DeploymentTracker::new()?;

    let (token, manager_url, platform_str) = match tracker.get(&args.name) {
        Some(tracked) => {
            let token = resolve_token(
                args.token.clone(),
                args.token_file.as_ref(),
                embedded_config,
            )?
            .unwrap_or_else(|| tracked.token.clone());
            let url = args
                .manager_url
                .unwrap_or_else(|| tracked.manager_url.clone());
            let platform = tracked.platform.clone();
            (token, url, platform)
        }
        None => {
            let token = resolve_token(
                args.token.clone(),
                args.token_file.as_ref(),
                embedded_config,
            )?
            .ok_or_else(|| {
                AlienError::new(ErrorData::ValidationError {
                    field: "token".to_string(),
                    message: format!(
                        "Deployment '{}' is not tracked. Provide --token and --manager-url.",
                        args.name
                    ),
                })
            })?;
            let url = args.manager_url.ok_or_else(|| {
                AlienError::new(ErrorData::ValidationError {
                    field: "manager_url".to_string(),
                    message: "--manager-url is required for untracked deployments".to_string(),
                })
            })?;
            (token, url, "aws".to_string())
        }
    };

    output::header("Alien Deploy — Destroy");
    output::status("Name:", &args.name);
    output::status("Manager:", &manager_url);

    let client = create_manager_client(&token, &manager_url)?;

    let tracked = tracker.get(&args.name).ok_or_else(|| {
        AlienError::new(ErrorData::ValidationError {
            field: "name".to_string(),
            message: format!("Deployment '{}' not found in tracker", args.name),
        })
    })?;

    let deployment_id = tracked.deployment_id.clone();

    let deployment = client
        .get_deployment()
        .id(&deployment_id)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::DeploymentFailed {
            operation: "fetch deployment".to_string(),
        })?;
    let deployment_json = serde_json::to_value(&*deployment)
        .into_alien_error()
        .context(ErrorData::DeploymentFailed {
            operation: "decode deployment".to_string(),
        })?;
    let import_source = deployment_json
        .get("importSource")
        .and_then(|value| value.as_str())
        .map(ToOwned::to_owned);
    let deployment_status = deployment_json
        .get("status")
        .and_then(|value| value.as_str())
        .unwrap_or_default();

    if let Some(source) = &import_source {
        if !args.force_delete_record {
            return Err(AlienError::new(ErrorData::ValidationError {
                field: "force_delete_record".to_string(),
                message: format!(
                    "Deployment '{}' was imported from {}. Refusing to tear down customer-owned IaC resources; rerun with --force-delete-record to remove only the manager record.",
                    args.name, source
                ),
            }));
        }
    }

    if args.force_delete_record {
        output::step(1, 2, "Force-deleting deployment...");

        client
            .delete_deployment()
            .id(&deployment_id)
            .body(alien_manager_api::types::DeleteDeploymentRequest {
                action: alien_manager_api::types::DeleteDeploymentAction::Forget,
            })
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::DeploymentFailed {
                operation: "force deletion".to_string(),
            })?;

        output::step(2, 2, "Done!");
        if import_source.is_some() {
            output::success(
                "Imported deployment record removed. No resource teardown was performed.",
            );
        } else {
            output::success("Deployment force-deleted. No resource teardown was performed.");
        }
        return Ok(());
    }

    let total_steps = 3;
    if matches!(deployment_status, "teardown-required" | "teardown-failed") {
        output::step(
            1,
            total_steps,
            "Deletion already requested; continuing setup teardown...",
        );
    } else {
        output::step(1, total_steps, "Requesting deployment deletion...");

        client
            .delete_deployment()
            .id(&deployment_id)
            .body(alien_manager_api::types::DeleteDeploymentRequest {
                action: alien_manager_api::types::DeleteDeploymentAction::Cleanup,
            })
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::DeploymentFailed {
                operation: "request deletion".to_string(),
            })?;
    }

    output::step(
        2,
        total_steps,
        "Loading target credentials and running deletion...",
    );

    let platform = Platform::from_str(&platform_str).map_err(|e| {
        AlienError::new(ErrorData::ValidationError {
            field: "platform".to_string(),
            message: e,
        })
    })?;

    let client_config = ClientConfig::from_std_env(platform)
        .await
        .context(ErrorData::ConfigurationError {
            message: format!(
                "Failed to load {} credentials from environment. Ensure the required environment variables are set.",
                platform
            ),
        })?;

    push_deletion(&client, &deployment_id, platform, client_config).await?;

    output::step(total_steps, total_steps, "Done!");
    output::success("Deployment destroyed successfully.");

    Ok(())
}

fn resolve_token(
    explicit_token: Option<String>,
    token_file: Option<&PathBuf>,
    embedded_config: Option<&DeployCliConfig>,
) -> Result<Option<String>> {
    Ok(explicit_token
        .map(Ok)
        .or_else(|| token_file.map(|path| read_token_file(path)))
        .transpose()?
        .or_else(|| {
            embedded_config
                .and_then(|c| c.token_env_var.as_ref())
                .and_then(|env_var| std::env::var(env_var).ok())
        })
        .or_else(|| embedded_config.and_then(|c| c.token.clone())))
}
