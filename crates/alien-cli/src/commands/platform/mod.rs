//! Platform-specific CLI commands.
//!
//! These commands require the `platform` feature and interact with the
//! Alien platform API (api.alien.dev) for OAuth login, workspace management,
//! project management, and directory-to-project linking.

pub mod link;
pub mod login;
pub mod logout;
pub mod projects;
pub mod unlink;
pub mod workspace;

pub use link::{link_task, LinkArgs};
pub use login::{login_task, LoginArgs};
pub use logout::{logout_task, LogoutArgs};
pub use projects::{project_task, ProjectArgs};
pub use unlink::{unlink_task, UnlinkArgs};
pub use workspace::{workspace_task, WorkspaceArgs};

use clap::Subcommand;

/// All platform-specific subcommands, flattened into the top-level CLI.
#[derive(Subcommand, Debug, Clone)]
pub enum PlatformCommand {
    /// Perform login & select default workspace
    Login(LoginArgs),
    /// Remove saved tokens & workspace
    Logout(LogoutArgs),
    /// Workspace commands
    #[command(alias = "workspace")]
    Workspaces(WorkspaceArgs),
    /// Project commands
    #[command(alias = "project")]
    Projects(ProjectArgs),
    /// Link directory to an Alien project
    Link(LinkArgs),
    /// Unlink directory from an Alien project
    Unlink(UnlinkArgs),
}
