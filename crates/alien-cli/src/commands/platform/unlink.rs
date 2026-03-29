use crate::error::{ErrorData, Result};
use crate::get_current_dir;
use crate::interaction::{ConfirmationMode, InteractionMode};
use crate::output::prompt_confirm;
use crate::project_link::{get_project_link_status, remove_project_link, ProjectLinkStatus};
use crate::ui::{dim_label, success_line};
use alien_error::AlienError;
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
            let confirmation_mode = unlink_confirmation_mode(force)?;
            if matches!(confirmation_mode, ConfirmationMode::Prompt)
                && !prompt_confirm(
                    &format!("Unlink directory from project '{}'?", link.project_name),
                    false,
                )?
            {
                println!("{}", dim_label("Unlink cancelled."));
                return Ok(());
            }

            remove_project_link(dir)?;
            println!(
                "{}",
                success_line(&format!("Unlinked from {}.", link.project_name))
            );
        }
        ProjectLinkStatus::NotLinked => {
            println!(
                "{}",
                dim_label("This directory is not linked to a project.")
            );
        }
        ProjectLinkStatus::Error(err) => {
            return Err(AlienError::new(ErrorData::GenericError {
                message: format!("Error reading link status: {}", err),
            }));
        }
    }

    Ok(())
}

fn unlink_confirmation_mode(force: bool) -> Result<ConfirmationMode> {
    InteractionMode::current(false).confirmation_mode(
        force,
        "Unlink requires a real terminal. Re-run with `--force`.",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project_link::{save_project_link, ProjectLink};
    use tempfile::TempDir;

    #[test]
    fn unlink_project_force_removes_link_file() {
        let temp_dir = TempDir::new().unwrap();
        let link = ProjectLink::new(
            "workspace".to_string(),
            "project_123".to_string(),
            "demo".to_string(),
        );
        save_project_link(temp_dir.path(), &link).unwrap();

        unlink_project(temp_dir.path(), true).unwrap();

        match get_project_link_status(temp_dir.path()) {
            ProjectLinkStatus::NotLinked => {}
            status => panic!("expected directory to be unlinked, got {status:?}"),
        }
    }

    #[test]
    fn unlink_confirmation_mode_requires_force_in_machine_mode() {
        let err = InteractionMode::new(false, false)
            .confirmation_mode(
                false,
                "Unlink requires a real terminal. Re-run with `--force`.",
            )
            .unwrap_err();
        assert!(err.to_string().contains("Re-run with `--force`"));
    }
}
