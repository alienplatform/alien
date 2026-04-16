//! Status command — shows current deployment status.

use crate::deployment_tracking::DeploymentTracker;
use crate::error::{ErrorData, Result};
use crate::output;
use alien_error::{AlienError, Context, IntoAlienError};
use clap::Parser;

#[derive(Parser, Debug, Clone)]
#[command(
    about = "Show deployment status",
    after_help = "EXAMPLES:
    # Show status of a tracked deployment
    alien-deploy status --name production"
)]
pub struct StatusArgs {
    /// Deployment name
    #[arg(long)]
    pub name: String,

    /// Authentication token (optional if deployment is tracked)
    #[arg(long, env = "ALIEN_TOKEN")]
    pub token: Option<String>,

    /// Manager URL (optional if deployment is tracked)
    #[arg(long, env = "ALIEN_MANAGER_URL")]
    pub manager_url: Option<String>,
}

pub async fn status_command(args: StatusArgs) -> Result<()> {
    let tracker = DeploymentTracker::new()?;

    let tracked = tracker.get(&args.name).ok_or_else(|| {
        AlienError::new(ErrorData::ValidationError {
            field: "name".to_string(),
            message: format!(
                "Deployment '{}' is not tracked. Use 'alien-deploy up' first.",
                args.name
            ),
        })
    })?;

    let token = args.token.unwrap_or_else(|| tracked.token.clone());
    let manager_url = args
        .manager_url
        .unwrap_or_else(|| tracked.manager_url.clone());

    let client = crate::commands::up::create_manager_client(&token, &manager_url)?;

    let deployment = client
        .get_deployment()
        .id(&tracked.deployment_id)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to get deployment from manager".to_string(),
        })?
        .into_inner();

    output::header(&format!("Deployment: {}", args.name));
    output::status("ID:", &tracked.deployment_id);
    output::status("Platform:", &tracked.platform);
    output::status("Manager:", &manager_url);
    output::status("Status:", &deployment.status);

    if let Some(ref release_id) = deployment.current_release_id {
        output::status("Current Release:", release_id);
    }
    if let Some(ref release_id) = deployment.desired_release_id {
        output::status("Desired Release:", release_id);
    }
    output::status("Created:", &deployment.created_at);

    Ok(())
}
