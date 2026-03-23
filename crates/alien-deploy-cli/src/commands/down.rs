//! Destroy command — tears down a deployment.

use crate::deployment_tracking::DeploymentTracker;
use crate::error::{ErrorData, Result};
use crate::output;
use alien_error::{AlienError, Context, IntoAlienError};
use clap::Parser;

#[derive(Parser, Debug, Clone)]
#[command(
    about = "Destroy a deployment and its resources",
    after_help = "EXAMPLES:
    # Destroy a tracked deployment
    alien-deploy down --name production

    # Destroy using explicit token
    alien-deploy down --name production --token dg_abc123... --manager-url https://manager.example.com"
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

    /// Skip confirmation prompt
    #[arg(long, short = 'y')]
    pub yes: bool,
}

pub async fn down_command(args: DownArgs) -> Result<()> {
    let tracker = DeploymentTracker::new()?;

    let (token, manager_url) = match tracker.get(&args.name) {
        Some(tracked) => {
            let token = args.token.unwrap_or_else(|| tracked.token.clone());
            let url = args
                .manager_url
                .unwrap_or_else(|| tracked.manager_url.clone());
            (token, url)
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
            (token, url)
        }
    };

    output::header("Alien Deploy — Destroy");
    output::status("Name:", &args.name);
    output::status("Manager:", &manager_url);

    let client = crate::commands::up::create_manager_client(&token, &manager_url)?;

    // Get deployment to find its ID
    let tracked = tracker.get(&args.name).ok_or_else(|| {
        AlienError::new(ErrorData::ValidationError {
            field: "name".to_string(),
            message: format!("Deployment '{}' not found in tracker", args.name),
        })
    })?;

    output::step(1, 2, "Requesting deployment destruction...");

    client
        .delete_deployment()
        .id(&tracked.deployment_id)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::DeploymentFailed {
            operation: "destruction".to_string(),
        })?;

    output::step(2, 2, "Done!");
    output::success("Deployment deletion initiated. Resources will be cleaned up by the manager.");

    Ok(())
}
