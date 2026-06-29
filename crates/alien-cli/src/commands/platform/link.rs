use crate::error::{ErrorData, Result};
use crate::execution_context::ExecutionMode;
use crate::get_current_dir;
use crate::output::{can_prompt, print_json};
use crate::project_link::{
    choose_or_create_project, get_project_by_name, get_project_link_status, save_project_link,
    suggest_project_name, ProjectLink, ProjectLinkStatus,
};
use crate::ui::{accent, contextual_heading, dim_label, success_line};
use alien_error::AlienError;
use alien_platform_api::types;
use clap::Parser;
use serde::Serialize;

#[derive(Parser, Debug, Clone)]
#[command(
    about = "Link the current directory to an Alien project",
    long_about = "Create or reuse a project link for the current directory. In a real terminal, `alien link` can guide first-run setup; in automation, pass `--project <name>`.",
    after_help = "EXAMPLES:
    alien link
    alien link --project my-project
    alien --workspace my-workspace link --project my-project --json"
)]
pub struct LinkArgs {
    /// Deprecated alias for `--project`; kept for compatibility
    #[arg(long, hide = true)]
    pub name: Option<String>,

    /// Do not attach detected git repository metadata when creating a project
    #[arg(long)]
    pub no_git: bool,

    /// Emit structured JSON output
    #[arg(long)]
    pub json: bool,

    /// Force re-linking even if this directory is already linked
    #[arg(long)]
    pub force: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LinkOutput {
    workspace: String,
    project_id: String,
    project_name: String,
    path: String,
    git_repository_warning: Option<types::ApiError>,
}

pub async fn link_task(args: LinkArgs, ctx: ExecutionMode) -> Result<()> {
    let current_dir = get_current_dir()?;

    match get_project_link_status(&current_dir) {
        ProjectLinkStatus::Linked(link) if !args.force => {
            return print_link_result(&link, &current_dir.display().to_string(), args.json, None);
        }
        ProjectLinkStatus::Linked(link) if args.force && !args.json => {
            println!(
                "Re-linking directory currently linked to '{}'.",
                link.project_name
            );
        }
        _ => {}
    }

    let http = ctx.auth_http().await?;
    let workspace_name = ctx.resolve_workspace_with_bootstrap(!args.json).await?;

    let explicit_project_name = match (ctx.project_override(), args.name.as_deref()) {
        (Some(_), Some(_)) => {
            return Err(AlienError::new(ErrorData::ValidationError {
                field: "project".to_string(),
                message: "`--name` is deprecated; pass only `--project <name>`.".to_string(),
            }));
        }
        (Some(project_name), None) | (None, Some(project_name)) => Some(project_name),
        (None, None) => None,
    };

    let (link, git_repository_warning) = if let Some(project_name) = explicit_project_name {
        link_or_create_project_by_name(
            &http,
            &workspace_name,
            project_name,
            &current_dir,
            !args.no_git,
        )
        .await?
    } else {
        let allow_prompt = !args.json && can_prompt();
        let selection = choose_or_create_project(
            &http,
            &workspace_name,
            Some(&suggest_project_name(&current_dir)),
            &current_dir,
            allow_prompt,
        )
        .await?;
        let project = selection.project;

        (
            ProjectLink::new(
                workspace_name.clone(),
                project.id.as_str().to_string(),
                project.name.as_str().to_string(),
            ),
            selection.git_repository_warning,
        )
    };

    save_project_link(&current_dir, &link)?;
    print_link_result(
        &link,
        &current_dir.display().to_string(),
        args.json,
        git_repository_warning,
    )
}

async fn link_or_create_project_by_name(
    http: &crate::auth::AuthHttp,
    workspace_name: &str,
    project_name: &str,
    current_dir: &std::path::Path,
    include_git_repository: bool,
) -> Result<(ProjectLink, Option<types::ApiError>)> {
    match get_project_by_name(http, workspace_name, Some(workspace_name), project_name).await {
        Ok(link) => Ok((link, None)),
        Err(error)
            if matches!(
                error.error,
                Some(ErrorData::InvalidProjectName {
                    project_name: _,
                    reason: _
                })
            ) =>
        {
            let selection = crate::project_link::create_new_project(
                http.sdk_client(),
                workspace_name,
                Some(project_name),
                current_dir,
                false,
                include_git_repository,
            )
            .await?;
            let project = selection.project;
            Ok((
                ProjectLink::new(
                    workspace_name.to_string(),
                    project.id.as_str().to_string(),
                    project.name.as_str().to_string(),
                ),
                selection.git_repository_warning,
            ))
        }
        Err(error) => Err(error),
    }
}

fn print_link_result(
    link: &ProjectLink,
    path: &str,
    json: bool,
    git_repository_warning: Option<types::ApiError>,
) -> Result<()> {
    if json {
        print_json(&LinkOutput {
            workspace: link.workspace.clone(),
            project_id: link.project_id.clone(),
            project_name: link.project_name.clone(),
            path: path.to_string(),
            git_repository_warning,
        })?;
    } else {
        println!(
            "{}",
            contextual_heading("Linked", &link.project_name, &[("from", path)])
        );
        println!("{} {}", dim_label("Workspace"), link.workspace);
        println!("{} {}", dim_label("Project ID"), link.project_id);
        println!(
            "{} {}",
            dim_label("Stored in"),
            accent(".alien/project.json")
        );
        println!("{}", success_line("Project link saved."));
        if let Some(warning) = git_repository_warning {
            println!("Warning: {}", warning.message.as_str());
        }
    }

    Ok(())
}
