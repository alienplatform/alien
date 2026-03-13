//! Deployment groups list view - pure rendering functions

use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::prelude::*;

use super::table::{render_table, TableCell, TableColumn, TableItem, TableRow};
use crate::tui::state::{Action, AppState, DeploymentGroupItem, ListState};

impl TableItem for DeploymentGroupItem {
    fn columns() -> Vec<TableColumn> {
        vec![
            TableColumn::new("NAME", Constraint::Length(25)),
            TableColumn::new("ID", Constraint::Length(35)),
            TableColumn::new("MAX DEPLOYMENTS", Constraint::Length(17)),
            TableColumn::new("CREATED", Constraint::Length(20)),
        ]
    }

    fn to_row(&self) -> TableRow {
        let max_deployments = self.max_deployments.to_string();
        let created = self.created_at.format("%Y-%m-%d %H:%M").to_string();

        TableRow::new(vec![
            TableCell::new(&self.name),
            TableCell::dim(&self.id),
            TableCell::new(&max_deployments),
            TableCell::dim(&created),
        ])
    }

    fn title() -> &'static str {
        "Deployment Groups"
    }

    fn empty_message() -> &'static str {
        "No deployment groups found. Press 'n' to create one."
    }
}

/// Render the deployment groups list view
pub fn render(
    frame: &mut Frame,
    area: Rect,
    state: &ListState<DeploymentGroupItem>,
    app: &AppState,
) {
    render_table(frame, area, state, app);
}

/// Handle key input for deployment groups list
pub fn handle_key(key: KeyEvent, state: &mut ListState<DeploymentGroupItem>) -> Action {
    match key.code {
        KeyCode::Down | KeyCode::Char('j') => {
            state.select_next();
            Action::None
        }
        KeyCode::Up | KeyCode::Char('k') => {
            state.select_prev();
            Action::None
        }
        KeyCode::Home | KeyCode::Char('g') => {
            state.select_first();
            Action::None
        }
        KeyCode::End | KeyCode::Char('G') => {
            state.select_last();
            Action::None
        }
        KeyCode::Char('r') => Action::Refresh,
        _ => Action::None,
    }
}

/// Get keybinds for deployment groups list
pub fn keybinds() -> Vec<(&'static str, &'static str)> {
    vec![
        ("↑/k", "up"),
        ("↓/j", "down"),
        ("n", "new"),
        ("r", "refresh"),
        ("/", "search"),
        ("q", "quit"),
    ]
}
