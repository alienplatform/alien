//! Deployment groups list view demos

use clap::Subcommand;
use color_eyre::Result;

use super::super::mock_data;
use super::super::runner::run_demo;
use alien_cli::tui::state::{AppState, ListState};
use alien_cli::tui::views::deployment_groups_view;

#[derive(Subcommand, Debug, Clone)]
pub enum DeploymentGroupsDemo {
    /// Empty list
    Empty,
    /// Loading state
    Loading,
    /// Error state
    Error,
    /// Few deployment groups
    #[command(name = "few")]
    Few,
    /// Many deployment groups
    #[command(name = "many")]
    Many,
}

impl DeploymentGroupsDemo {
    pub fn run(self) -> Result<()> {
        let (title, state) = match self {
            Self::Empty => ("Deployment Groups - Empty", ListState::new()),
            Self::Loading => ("Deployment Groups - Loading", ListState::loading()),
            Self::Error => (
                "Deployment Groups - Error",
                ListState::with_error("Failed to load deployment groups"),
            ),
            Self::Few => (
                "Deployment Groups - Few",
                ListState::with_items(mock_data::mock_deployment_groups(5)),
            ),
            Self::Many => (
                "Deployment Groups - Many",
                ListState::with_items(mock_data::mock_deployment_groups(50)),
            ),
        };

        let app = AppState::new();

        run_demo(
            title,
            state,
            app,
            |frame, area, state, app| deployment_groups_view::render(frame, area, state, app),
            |key, state| deployment_groups_view::handle_key(key, state),
        )
    }
}
