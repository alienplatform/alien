use crate::auth::{load_workspace, save_workspace};
use crate::error::{ErrorData, Result};
use crate::execution_context::ExecutionMode;
use crate::interaction::InteractionMode;
use crate::output::{print_json, prompt_select};
use crate::ui::{command, dim_label, make_table, print_table, success_line};
use alien_error::{AlienError, Context};
use alien_platform_api::SdkResultExt;
use clap::{Parser, Subcommand};
use serde::Serialize;

#[derive(Parser, Debug, Clone)]
#[command(
    about = "Workspace commands",
    long_about = "Manage workspaces in the Alien platform.",
    after_help = "EXAMPLES:
    alien workspaces current
    alien workspaces create my-workspace
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
    /// Create a workspace and select it as the default
    Create {
        /// Permanent workspace name used in URLs and CLI commands.
        name: String,
    },
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceCreateOutput {
    id: String,
    workspace: String,
    role: String,
    selected: bool,
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
                println!(
                    "{} run {} or {} to choose one.",
                    dim_label("Next"),
                    command("alien workspaces set <name>"),
                    command("alien login")
                );
            }
        }
        WorkspaceCmd::Create { name } => {
            let http = ctx.auth_http().await?;
            let response = http
                .sdk_client()
                .create_workspace()
                .body_map(|body| body.name(name.as_str()))
                .send()
                .await
                .into_sdk_error()
                .context(ErrorData::ApiRequestFailed {
                    message: "Failed to create workspace".to_string(),
                    url: None,
                })?
                .into_inner();
            let workspace = (*response.name).clone();
            save_workspace(&workspace).context(ErrorData::WorkspaceCreatedSelectionFailed {
                workspace: workspace.clone(),
            })?;

            if args.json {
                print_json(&WorkspaceCreateOutput {
                    id: response.id.to_string(),
                    workspace,
                    role: response.role.to_string(),
                    selected: true,
                })?;
            } else {
                println!(
                    "{}",
                    success_line(&format!("Created workspace {workspace} and selected it."))
                );
                println!("{}", dim_label("Workspace names are permanent."));
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
                println!(
                    "{}",
                    success_line(&format!("Using workspace {}.", workspace_name))
                );
            }
        }
        WorkspaceCmd::Ls => {
            let http = ctx.auth_http().await?;
            let workspaces = list_workspace_names(&http).await?;
            let current = load_workspace();

            if args.json {
                print_json(&workspaces)?;
            } else if workspaces.is_empty() {
                println!("(no workspaces)");
            } else {
                let mut table = make_table(&["Workspace", "Selected"]);
                for workspace in workspaces {
                    let selected = if current.as_deref() == Some(workspace.as_str()) {
                        "Yes"
                    } else {
                        ""
                    };
                    table.add_row(vec![workspace, selected.to_string()]);
                }
                print_table(table);
            }
        }
    }

    Ok(())
}

pub async fn list_workspace_names(http: &crate::auth::AuthHttp) -> Result<Vec<String>> {
    let client = http.sdk_client();
    let response = client
        .list_memberships()
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
        .map(|membership| (*membership.name).clone())
        .collect())
}

pub async fn validate_workspace_name(
    http: &crate::auth::AuthHttp,
    workspace: &str,
) -> Result<String> {
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
            message: "No workspaces found for this account. Run `alien workspaces create <name>`."
                .to_string(),
        }));
    }

    if workspaces.len() == 1 {
        return Ok(workspaces[0].clone());
    }

    if InteractionMode::current(json_mode).is_machine() {
        return Err(AlienError::new(ErrorData::WorkspaceSelectionRequired {
            workspaces,
        }));
    }

    prompt_select("Select a workspace:", &workspaces)
}

#[cfg(test)]
mod tests {
    use super::{WorkspaceArgs, WorkspaceCmd};
    use clap::Parser;

    #[test]
    fn parses_workspace_create_name() {
        let args = WorkspaceArgs::try_parse_from(["workspaces", "create", "acme-prod"])
            .expect("create command should parse");

        assert!(matches!(
            args.cmd,
            WorkspaceCmd::Create { name } if name == "acme-prod"
        ));
    }
}
