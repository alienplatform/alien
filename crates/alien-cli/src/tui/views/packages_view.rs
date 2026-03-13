//! Packages list view - pure rendering functions

use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::prelude::*;

use super::table::{render_table, TableCell, TableColumn, TableItem, TableRow};
use crate::tui::state::{Action, AppState, ListState, PackageItem, PackageStatus};

impl TableItem for PackageItem {
    fn columns() -> Vec<TableColumn> {
        vec![
            TableColumn::new("ID", Constraint::Length(35)),
            TableColumn::new("TYPE", Constraint::Length(15)),
            TableColumn::new("VERSION", Constraint::Length(15)),
            TableColumn::new("STATUS", Constraint::Length(12)),
            TableColumn::new("CREATED", Constraint::Length(20)),
        ]
    }

    fn to_row(&self) -> TableRow {
        let status_str = format!("{:?}", self.status);
        let status_cell = match self.status {
            PackageStatus::Ready => TableCell::success(&status_str),
            PackageStatus::Pending | PackageStatus::Building => TableCell::warning(&status_str),
            PackageStatus::Failed | PackageStatus::Canceled => TableCell::error(&status_str),
        };
        let created = self.created_at.format("%Y-%m-%d %H:%M").to_string();

        TableRow::new(vec![
            TableCell::dim(&self.id),
            TableCell::primary(&self.type_display),
            TableCell::new(&self.version),
            status_cell,
            TableCell::dim(&created),
        ])
    }

    fn title() -> &'static str {
        "Packages"
    }

    fn empty_message() -> &'static str {
        "No packages found."
    }
}

/// Render the packages list view
pub fn render(frame: &mut Frame, area: Rect, state: &ListState<PackageItem>, app: &AppState) {
    render_table(frame, area, state, app);
}

/// Handle key input for packages list
pub fn handle_key(key: KeyEvent, state: &mut ListState<PackageItem>) -> Action {
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

/// Get keybinds for packages list
pub fn keybinds() -> Vec<(&'static str, &'static str)> {
    vec![
        ("↑/k", "up"),
        ("↓/j", "down"),
        ("r", "refresh"),
        ("/", "search"),
        ("q", "quit"),
    ]
}
