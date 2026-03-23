//! List command — shows all tracked deployments.

use crate::deployment_tracking::DeploymentTracker;
use crate::error::Result;
use crate::output;
use clap::Parser;

#[derive(Parser, Debug, Clone)]
#[command(about = "List tracked deployments")]
pub struct ListArgs {}

pub async fn list_command(_args: ListArgs) -> Result<()> {
    let tracker = DeploymentTracker::new()?;
    let deployments = tracker.list();

    if deployments.is_empty() {
        output::info("No tracked deployments. Use 'alien-deploy up' to create one.");
        return Ok(());
    }

    output::header("Tracked Deployments");

    for dep in deployments {
        eprintln!(
            "  \x1b[1m{}\x1b[0m  ({})  {}  {}",
            dep.name, dep.platform, dep.deployment_id, dep.manager_url
        );
    }

    eprintln!();
    Ok(())
}
