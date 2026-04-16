//! Destroy command — tears down a deployment.

use crate::deployment_tracking::DeploymentTracker;
use crate::error::{ErrorData, Result};
use crate::output;
use alien_core::{ClientConfig, Platform};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_infra::ClientConfigExt;
use clap::Parser;
use std::str::FromStr;

use super::up::{create_manager_client, push_deletion};

#[derive(Parser, Debug, Clone)]
#[command(
    about = "Destroy a deployment and its resources",
    after_help = "EXAMPLES:
    # Destroy a tracked deployment
    alien-deploy down --name production

    # Force-delete (skip resource teardown, just remove the record)
    alien-deploy down --name production --force

    # Destroy using explicit token
    alien-deploy down --name production --token ax_dg_abc123... --manager-url https://manager.example.com"
)]
pub struct DownArgs {
    /// Deployment name
    #[arg(long)]
    pub name: String,

    /// Authentication token (optional if deployment is tracked)
    #[arg(long, env = "ALIEN_TOKEN")]
    pub token: Option<String>,

    /// Manager URL (optional if deployment is tracked)
    #[arg(long, env = "ALIEN_MANAGER_URL")]
    pub manager_url: Option<String>,

    /// Force deletion — skip resource teardown, just remove the deployment record
    #[arg(long)]
    pub force: bool,

    /// Skip confirmation prompt
    #[arg(long, short = 'y')]
    pub yes: bool,
}

pub async fn down_command(args: DownArgs) -> Result<()> {
    let tracker = DeploymentTracker::new()?;

    let (token, manager_url, platform_str) = match tracker.get(&args.name) {
        Some(tracked) => {
            let token = args.token.unwrap_or_else(|| tracked.token.clone());
            let url = args
                .manager_url
                .unwrap_or_else(|| tracked.manager_url.clone());
            let platform = tracked.platform.clone();
            (token, url, platform)
        }
        None => {
            let token = args.token.ok_or_else(|| {
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

    if args.force {
        output::step(1, 2, "Force-deleting deployment...");

        client
            .delete_deployment()
            .id(&deployment_id)
            .force(true)
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::DeploymentFailed {
                operation: "force deletion".to_string(),
            })?;

        output::step(2, 2, "Done!");
        output::success("Deployment force-deleted. No resource teardown was performed.");
        return Ok(());
    }

    let total_steps = 3;
    output::step(1, total_steps, "Requesting deployment deletion...");

    client
        .delete_deployment()
        .id(&deployment_id)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::DeploymentFailed {
            operation: "request deletion".to_string(),
        })?;

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
