use crate::error::{ErrorData, Result};
use crate::get_current_dir;
use crate::project_link::{get_project_link_status, remove_project_link, ProjectLinkStatus};
use alien_error::{AlienError, Context, IntoAlienError};
use clap::Parser;

#[derive(Parser, Debug, Clone)]
#[command(
    about = "Unlink directory from an Alien project",
    long_about = "Remove the link between the current directory and an Alien project. This removes the .alien/project.json file."
)]
pub struct UnlinkArgs {
    /// Force unlink without confirmation
    #[arg(long)]
    pub force: bool,
}

pub async fn unlink_task(args: UnlinkArgs) -> Result<()> {
    let current_dir = get_current_dir()?;
    unlink_project(&current_dir, args.force)
}

/// Unlink the current directory from a project
fn unlink_project(dir: &std::path::Path, force: bool) -> Result<()> {
    match get_project_link_status(dir) {
        ProjectLinkStatus::Linked(link) => {
            if !force {
                // Confirm unlinking
                print!(
                    "Unlink directory from project '{}'? [y/N] ",
                    link.project_name
                );
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

                if input.trim().to_lowercase() != "y" && input.trim().to_lowercase() != "yes" {
                    println!("Unlink cancelled");
                    return Ok(());
                }
            }

            remove_project_link(dir)?;
            println!("✅ Directory unlinked from project '{}'", link.project_name);
        }
        ProjectLinkStatus::NotLinked => {
            println!("❌ Directory is not linked to any project");
        }
        ProjectLinkStatus::Error(err) => {
            return Err(AlienError::new(ErrorData::GenericError {
                message: format!("Error reading link status: {}", err),
            }));
        }
    }

    Ok(())
}
