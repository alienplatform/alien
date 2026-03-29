use crate::error::Result;
use crate::execution_context::ExecutionMode;
use crate::get_current_dir;
use crate::output::{can_prompt, print_json};
use crate::project_link::{
    choose_or_create_project, get_project_by_name, get_project_link_status, save_project_link,
    suggest_project_name, ProjectLink, ProjectLinkStatus,
};
use crate::ui::{accent, contextual_heading, dim_label, success_line};
use clap::Parser;
use serde::Serialize;

#[derive(Parser, Debug, Clone)]
#[command(
    about = "Link the current directory to an Alien project",
    long_about = "Create or reuse a project link for the current directory. In a real terminal, `alien link` can guide first-run setup; in automation, pass `--project` or `--name`.",
    after_help = "EXAMPLES:
    alien link
    alien link --project my-existing-project
    alien link --name my-new-project
    alien --workspace my-workspace --project my-existing-project link --json"
)]
pub struct LinkArgs {
    /// Create a new project with this name
    #[arg(long)]
    pub name: Option<String>,

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
}

pub async fn link_task(args: LinkArgs, ctx: ExecutionMode) -> Result<()> {
    let current_dir = get_current_dir()?;

    match get_project_link_status(&current_dir) {
        ProjectLinkStatus::Linked(link) if !args.force => {
            return print_link_result(&link, &current_dir.display().to_string(), args.json);
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

    let link = if let Some(project_name) = ctx.project_override() {
        get_project_by_name(&http, &workspace_name, project_name).await?
    } else if let Some(name) = args.name.as_deref() {
        let project = crate::project_link::create_new_project(
            http.sdk_client(),
            &workspace_name,
            Some(name),
            &current_dir,
            false,
        )
        .await?;

        ProjectLink::new(
            workspace_name.clone(),
            project.id.as_str().to_string(),
            project.name.as_str().to_string(),
        )
    } else {
        let allow_prompt = !args.json && can_prompt();
        let project = choose_or_create_project(
            &http,
            &workspace_name,
            Some(&suggest_project_name(&current_dir)),
            &current_dir,
            allow_prompt,
        )
        .await?;

        ProjectLink::new(
            workspace_name.clone(),
            project.id.as_str().to_string(),
            project.name.as_str().to_string(),
        )
    };

    save_project_link(&current_dir, &link)?;
    print_link_result(&link, &current_dir.display().to_string(), args.json)
}

fn print_link_result(link: &ProjectLink, path: &str, json: bool) -> Result<()> {
    if json {
        print_json(&LinkOutput {
            workspace: link.workspace.clone(),
            project_id: link.project_id.clone(),
            project_name: link.project_name.clone(),
            path: path.to_string(),
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
    }

    Ok(())
}
