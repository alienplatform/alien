use crate::error::{ErrorData, Result};
use crate::execution_context::ExecutionMode;
use crate::get_current_dir;
use crate::project_link::{
    get_project_by_name, get_project_link_status, interactive_project_selection, save_project_link,
    suggest_project_name, ProjectLink, ProjectLinkStatus,
};
use alien_error::{Context, IntoAlienError};
use clap::Parser;

#[derive(Parser, Debug, Clone)]
#[command(
    about = "Link local directory to an Alien project",
    long_about = "Link the current directory to an Alien project. This creates a .alien directory with project information."
)]
pub struct LinkArgs {
    /// Project name (for creating new projects)
    #[arg(long)]
    pub name: Option<String>,

    /// Force re-linking even if already linked
    #[arg(long)]
    pub force: bool,
}

pub async fn link_task(args: LinkArgs, ctx: ExecutionMode) -> Result<()> {
    let current_dir = get_current_dir()?;
    link_project(args, &ctx, &current_dir).await
}

/// Link the directory to a project
async fn link_project(args: LinkArgs, ctx: &ExecutionMode, dir: &std::path::Path) -> Result<()> {
    // Check current link status
    match get_project_link_status(dir) {
        ProjectLinkStatus::Linked(link) if !args.force => {
            println!("🔗 Directory is already linked to:");
            println!("   Workspace: {}", link.workspace);
            println!("   Project: {} ({})", link.project_name, link.project_id);
            if let Some(ref root_dir) = link.root_directory {
                println!("   Root Directory: {}", root_dir);
            }
            println!("   Use --force to re-link or `alien unlink` to unlink first");
            return Ok(());
        }
        ProjectLinkStatus::Linked(link) if args.force => {
            println!(
                "🔄 Re-linking directory (currently linked to '{}')",
                link.project_name
            );
        }
        _ => {}
    }

    let http = ctx.auth_http().await?;
    let workspace_name = ctx.resolve_workspace().await?;

    // If global --project is set, resolve it and skip interactive selection
    if let Some(project_name) = ctx.project_override() {
        let project_link = get_project_by_name(&http, &workspace_name, project_name).await?;

        let link = ProjectLink::new(
            workspace_name.clone(),
            project_link.project_id.clone(),
            project_link.project_name.clone(),
        );

        save_project_link(dir, &link)?;

        println!(
            "🔗 Linked to {}/{} (created .alien and added it to .gitignore)",
            workspace_name, project_link.project_name
        );

        return Ok(());
    }

    // Get suggested project name
    let suggested_name = args
        .name
        .as_deref()
        .map(|s| s.to_string())
        .unwrap_or_else(|| suggest_project_name(dir));

    // Show setup prompt
    let dir_display = dir.display();
    print!("Set up and link \"{}\"? [Y/n] ", dir_display);
    use std::io::{self, Write};
    io::stdout().flush().ok();

    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "read".to_string(),
            file_path: "stdin".to_string(),
            reason: "Failed to read user input".to_string(),
        })?;

    if input.trim().to_lowercase() == "n" || input.trim().to_lowercase() == "no" {
        println!("Cancelled. Directory not linked.");
        return Ok(());
    }

    // Show workspace selection
    println!(
        "Which scope should contain your project? {}",
        workspace_name
    );

    // Interactive project selection
    let project =
        interactive_project_selection(&http, &workspace_name, Some(&suggested_name)).await?;

    // Create and save project link
    let link = ProjectLink::new(
        workspace_name.clone(),
        (*project.id).clone(),
        (*project.name).clone(),
    );

    save_project_link(dir, &link)?;

    println!(
        "🔗 Linked to {}/{} (created .alien and added it to .gitignore)",
        workspace_name, *project.name
    );

    Ok(())
}
