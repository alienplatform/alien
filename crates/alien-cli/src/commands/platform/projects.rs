use crate::error::{ErrorData, Result};
use crate::execution_context::ExecutionMode;
use crate::output::print_json;
use crate::ui::dim_label;
use alien_error::{Context, IntoAlienError};
use alien_platform_api::types::ListProjectsWorkspace;
use alien_platform_api::SdkResultExt;
use clap::{Parser, Subcommand};

#[derive(Parser, Debug, Clone)]
#[command(
    about = "Project commands",
    long_about = "Manage projects in the Alien platform.",
    after_help = "EXAMPLES:
    alien projects ls
    alien projects ls --json
    alien --workspace my-workspace projects ls"
)]
pub struct ProjectArgs {
    /// Emit structured JSON output
    #[arg(long, global = true)]
    pub json: bool,

    #[command(subcommand)]
    pub cmd: ProjectCmd,
}

#[derive(Subcommand, Debug, Clone)]
pub enum ProjectCmd {
    /// List projects
    #[command(alias = "list")]
    Ls,
}

pub async fn project_task(args: ProjectArgs, ctx: ExecutionMode) -> Result<()> {
    let http = ctx.auth_http().await?;
    let workspace_name = ctx.resolve_workspace_with_bootstrap(!args.json).await?;

    match args.cmd {
        ProjectCmd::Ls => list_projects_task(&http, &workspace_name, args.json).await?,
    }

    Ok(())
}

async fn list_projects_task(
    http: &crate::auth::AuthHttp,
    workspace: &str,
    json: bool,
) -> Result<()> {
    let workspace_param = ListProjectsWorkspace::try_from(workspace)
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Workspace name is not valid".to_string(),
        })?;

    let response = http
        .sdk_client()
        .list_projects()
        .workspace(&workspace_param)
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: "Failed to list projects".to_string(),
            url: None,
        })?;

    let items = response.into_inner().items;
    if json {
        print_json(&items)?;
    } else if items.is_empty() {
        println!("{}", dim_label("No projects found."));
    } else {
        for project in items {
            println!("{} ({})", project.name.as_str(), project.id.as_str());
        }
    }

    Ok(())
}
