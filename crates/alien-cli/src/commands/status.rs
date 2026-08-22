use clap::Parser;

use crate::commands::deployments::{deployments_task, DeploymentsArgs, DeploymentsCmd};
use crate::error::Result;
use crate::execution_context::ExecutionMode;

#[derive(Parser, Debug, Clone)]
#[command(
    about = "Show deployment status",
    long_about = "Show all deployments in the linked project, or detailed live status for one deployment.",
    after_help = "EXAMPLES:
    alien status
    alien status --json
    alien status production/api
    alien status dep_abc123 --json"
)]
pub struct StatusArgs {
    /// Deployment ID, or <deployment-group-name>/<deployment-name>
    pub deployment: Option<String>,
    /// Project to list when no deployment is provided (defaults to the linked project)
    #[arg(long, conflicts_with = "deployment")]
    pub project: Option<String>,
    /// Emit machine-readable JSON
    #[arg(long)]
    pub json: bool,
}

impl StatusArgs {
    pub fn wants_json_output(&self) -> bool {
        self.json
    }
}

pub async fn status_task(args: StatusArgs, ctx: ExecutionMode) -> Result<()> {
    let cmd = match args.deployment {
        Some(id) => DeploymentsCmd::Get {
            id,
            json: args.json,
        },
        None => DeploymentsCmd::Ls {
            project: args.project,
            json: args.json,
        },
    };
    deployments_task(DeploymentsArgs { cmd }, ctx).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_an_optional_deployment_reference() {
        let list = StatusArgs::try_parse_from(["status", "--json"])
            .expect("linked-project status should parse");
        assert!(list.deployment.is_none());

        let detail = StatusArgs::try_parse_from(["status", "production/api", "--json"])
            .expect("deployment status should parse");
        assert_eq!(detail.deployment.as_deref(), Some("production/api"));
    }

    #[test]
    fn project_and_deployment_are_unambiguous() {
        StatusArgs::try_parse_from(["status", "production/api", "--project", "example"])
            .expect_err("project cannot modify deployment lookup");
    }
}
