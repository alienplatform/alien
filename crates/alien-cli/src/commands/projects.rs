use crate::error::{ErrorData, Result};
use crate::execution_context::ExecutionMode;
use alien_error::{Context, IntoAlienError};
use alien_platform_api::types::ListProjectsWorkspace;
use alien_platform_api::SdkResultExt;
use clap::{Parser, Subcommand};

#[derive(Parser, Debug, Clone)]
#[command(
    about = "Project commands",
    long_about = "Manage projects in the Alien platform."
)]
pub struct ProjectArgs {
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
    let workspace_name = ctx.resolve_workspace().await?;

    match args.cmd {
        ProjectCmd::Ls => {
            list_projects_task(&http, &workspace_name).await?;
        }
    }
    Ok(())
}

async fn list_projects_task(http: &crate::auth::AuthHttp, workspace: &str) -> Result<()> {
    let client = http.sdk_client();

    let workspace_param = ListProjectsWorkspace::try_from(workspace)
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Workspace name is not valid".to_string(),
        })?;
    let response = client
        .list_projects()
        .workspace(&workspace_param)
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: "listing projects".to_string(),
            url: None,
        })?;
    let projects_response = response.into_inner();

    if projects_response.items.is_empty() {
        println!("(no projects)");
    } else {
        let json_str = serde_json::to_string_pretty(&projects_response.items)
            .into_alien_error()
            .context(ErrorData::JsonError {
                operation: "serialization".to_string(),
                reason: "Unable to format projects as JSON".to_string(),
            })?;
        println!("{}", json_str);
    }

    Ok(())
}
