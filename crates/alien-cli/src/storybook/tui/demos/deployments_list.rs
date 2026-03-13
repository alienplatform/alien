//! Deployments list view demos

use clap::Subcommand;
use color_eyre::Result;

use super::super::mock_data;
use super::super::runner::run_demo;
use alien_cli::tui::state::{AppState, ListState};
use alien_cli::tui::views::deployments_list;

#[derive(Subcommand, Debug, Clone)]
pub enum DeploymentsListDemo {
    /// Empty list
    Empty,
    /// Loading state
    Loading,
    /// Error state
    Error,
    /// Single deployment
    #[command(name = "1")]
    Single,
    /// Five deployments
    #[command(name = "5")]
    Five,
    /// Many deployments (50)
    #[command(name = "many")]
    Many,
    /// Huge list (500 deployments)
    #[command(name = "huge")]
    Huge,
    /// Deployments with various statuses
    #[command(name = "statuses")]
    AllStatuses,
    /// Deployments with long names
    #[command(name = "long")]
    LongNames,
}

impl DeploymentsListDemo {
    pub fn run(self) -> Result<()> {
        let (title, state) = match self {
            Self::Empty => ("Deployments - Empty", ListState::new()),
            Self::Loading => ("Deployments - Loading", ListState::loading()),
            Self::Error => (
                "Deployments - Error",
                ListState::with_error("Failed to load deployments: Connection timeout"),
            ),
            Self::Single => (
                "Deployments - Single",
                ListState::with_items(mock_data::mock_deployments(1)),
            ),
            Self::Five => (
                "Deployments - Five",
                ListState::with_items(mock_data::mock_deployments(5)),
            ),
            Self::Many => (
                "Deployments - Many (50)",
                ListState::with_items(mock_data::mock_deployments(50)),
            ),
            Self::Huge => (
                "Deployments - Huge (500)",
                ListState::with_items(mock_data::mock_deployments(500)),
            ),
            Self::AllStatuses => (
                "Deployments - All Statuses",
                ListState::with_items(mock_data::mock_deployments_various_statuses()),
            ),
            Self::LongNames => (
                "Deployments - Long Names",
                ListState::with_items(mock_data::mock_deployments_long_names()),
            ),
        };

        let app = AppState::new();

        run_demo(
            title,
            state,
            app,
            |frame, area, state, app| deployments_list::render(frame, area, state, app),
            |key, state| deployments_list::handle_key(key, state),
        )
    }
}
