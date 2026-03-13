//! Releases list view demos

use clap::Subcommand;
use color_eyre::Result;

use super::super::mock_data;
use super::super::runner::run_demo;
use alien_cli::tui::state::{AppState, ListState};
use alien_cli::tui::views::releases_view;

#[derive(Subcommand, Debug, Clone)]
pub enum ReleasesDemo {
    /// Empty list
    Empty,
    /// Loading state
    Loading,
    /// Few releases
    #[command(name = "few")]
    Few,
    /// Many releases
    #[command(name = "many")]
    Many,
}

impl ReleasesDemo {
    pub fn run(self) -> Result<()> {
        let (title, state) = match self {
            Self::Empty => ("Releases - Empty", ListState::new()),
            Self::Loading => ("Releases - Loading", ListState::loading()),
            Self::Few => (
                "Releases - Few",
                ListState::with_items(mock_data::mock_releases(5)),
            ),
            Self::Many => (
                "Releases - Many",
                ListState::with_items(mock_data::mock_releases(50)),
            ),
        };

        let app = AppState::new();

        run_demo(
            title,
            state,
            app,
            |frame, area, state, app| releases_view::render(frame, area, state, app),
            |key, state| releases_view::handle_key(key, state),
        )
    }
}
