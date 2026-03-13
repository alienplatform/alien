//! Packages list view demos

use clap::Subcommand;
use color_eyre::Result;

use super::super::mock_data;
use super::super::runner::run_demo;
use alien_cli::tui::state::{AppState, ListState};
use alien_cli::tui::views::packages_view;

#[derive(Subcommand, Debug, Clone)]
pub enum PackagesDemo {
    /// Empty list
    Empty,
    /// Loading state
    Loading,
    /// Error state
    Error,
    /// Packages with all statuses
    #[command(name = "statuses")]
    AllStatuses,
    /// Many packages
    #[command(name = "many")]
    Many,
}

impl PackagesDemo {
    pub fn run(self) -> Result<()> {
        let (title, state) = match self {
            Self::Empty => ("Packages - Empty", ListState::new()),
            Self::Loading => ("Packages - Loading", ListState::loading()),
            Self::Error => (
                "Packages - Error",
                ListState::with_error("Failed to load packages"),
            ),
            Self::AllStatuses => (
                "Packages - All Statuses",
                ListState::with_items(mock_data::mock_packages_all_statuses()),
            ),
            Self::Many => (
                "Packages - Many",
                ListState::with_items(mock_data::mock_packages(50)),
            ),
        };

        let app = AppState::new();

        run_demo(
            title,
            state,
            app,
            |frame, area, state, app| packages_view::render(frame, area, state, app),
            |key, state| packages_view::handle_key(key, state),
        )
    }
}
