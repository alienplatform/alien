//! Commands list view demos

use clap::Subcommand;
use color_eyre::Result;

use super::super::mock_data;
use super::super::runner::run_demo;
use alien_cli::tui::state::{AppState, CommandItem, ListState};
use alien_cli::tui::views::commands_view;

#[derive(Subcommand, Debug, Clone)]
pub enum CommandsDemo {
    /// Empty list
    Empty,
    /// Loading state
    Loading,
    /// Error state
    Error,
    /// Commands with all states
    #[command(name = "states")]
    AllStates,
    /// Many commands
    #[command(name = "many")]
    Many,
}

impl CommandsDemo {
    pub fn run(self) -> Result<()> {
        let (title, state) = match self {
            Self::Empty => ("Commands - Empty", ListState::new()),
            Self::Loading => ("Commands - Loading", ListState::loading()),
            Self::Error => (
                "Commands - Error",
                ListState::with_error("Failed to load commands"),
            ),
            Self::AllStates => (
                "Commands - All States",
                ListState::with_items(mock_data::mock_commands_all_states()),
            ),
            Self::Many => (
                "Commands - Many",
                ListState::with_items(mock_data::mock_commands(50)),
            ),
        };

        let app = AppState::new();

        run_demo(
            title,
            state,
            app,
            |frame, area, state, app| {
                // Pass empty filter and all commands
                let commands_ref: Vec<&CommandItem> = state.items.iter().collect();
                commands_view::render(frame, area, state, app, None, commands_ref)
            },
            |key, state| commands_view::handle_key(key, state),
        )
    }
}
