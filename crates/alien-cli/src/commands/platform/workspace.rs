use crate::auth::{load_workspace, save_workspace};
use crate::error::{ErrorData, Result};
use crate::execution_context::ExecutionMode;
use crate::output::{can_prompt, print_json, prompt_select};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_platform_api::SdkResultExt;
use clap::{Parser, Subcommand};
use serde::Serialize;

#[derive(Parser, Debug, Clone)]
#[command(
    about = "Workspace commands",
    long_about = "Manage workspaces in the Alien platform.",
    after_help = "EXAMPLES:
    alien workspaces current
    alien workspaces ls
    alien workspaces set my-workspace
    alien workspaces set --json"
)]
pub struct WorkspaceArgs {
    /// Emit structured JSON output
    #[arg(long, global = true)]
    pub json: bool,

    #[command(subcommand)]
    pub cmd: WorkspaceCmd,
}

#[derive(Subcommand, Debug, Clone)]
pub enum WorkspaceCmd {
    /// Print the effective current workspace
    Current,
    /// Set the default workspace
    Set {
        /// Workspace name. If omitted in a real TTY, prompts for selection.
        name: Option<String>,
    },
    /// List all available workspaces
    #[command(alias = "list")]
    Ls,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceCurrentOutput {
    workspace: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceSetOutput {
    workspace: String,
    saved: bool,
}

pub async fn workspace_task(args: WorkspaceArgs, ctx: ExecutionMode) -> Result<()> {
    match args.cmd {
        WorkspaceCmd::Current => {
            let workspace = load_workspace();
            if args.json {
                print_json(&WorkspaceCurrentOutput { workspace })?;
            } else if let Some(workspace) = workspace {
                println!("{workspace}");
            } else {
                println!("<none>");
                println!("Run `alien workspaces set <name>` or `alien login` to choose one.");
            }
        }
        WorkspaceCmd::Set { name } => {
            let http = ctx.auth_http().await?;
            let workspace_name = match name {
                Some(name) => validate_workspace_name(&http, &name).await?,
                None => prompt_workspace(&http, args.json).await?,
            };

            save_workspace(&workspace_name)?;

            if args.json {
                print_json(&WorkspaceSetOutput {
                    workspace: workspace_name,
                    saved: true,
                })?;
            } else {
                println!("Default workspace set to: {workspace_name}");
            }
        }
        WorkspaceCmd::Ls => {
            let http = ctx.auth_http().await?;
            let workspaces = list_workspace_names(&http).await?;

            if args.json {
                print_json(&workspaces)?;
            } else if workspaces.is_empty() {
                println!("(no workspaces)");
            } else {
                for workspace in workspaces {
                    println!("{workspace}");
                }
            }
        }
    }

    Ok(())
}

pub async fn list_workspace_names(http: &crate::auth::AuthHttp) -> Result<Vec<String>> {
    let client = http.sdk_client();
    let response = client
        .list_workspaces()
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: "Failed to list workspaces".to_string(),
            url: None,
        })?;

    Ok(response
        .into_inner()
        .items
        .into_iter()
        .map(|workspace| (*workspace.name).clone())
        .collect())
}

pub async fn validate_workspace_name(http: &crate::auth::AuthHttp, workspace: &str) -> Result<String> {
    let workspaces = list_workspace_names(http).await?;
    if workspaces.iter().any(|candidate| candidate == workspace) {
        Ok(workspace.to_string())
    } else {
        Err(AlienError::new(ErrorData::ConfigurationError {
            message: format!("Workspace '{workspace}' not found in your memberships."),
        }))
    }
}

pub async fn prompt_workspace(http: &crate::auth::AuthHttp, json_mode: bool) -> Result<String> {
    let workspaces = list_workspace_names(http).await?;
    if workspaces.is_empty() {
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: "No workspaces found for this account.".to_string(),
        }));
    }

    if workspaces.len() == 1 {
        return Ok(workspaces[0].clone());
    }

    if json_mode || !can_prompt() {
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message:
                "Workspace selection requires a real terminal. Pass `--workspace <name>` or run `alien workspaces set <name>` first."
                    .to_string(),
        }));
    }

    prompt_select("Select a workspace:", &workspaces)
}
