//! Releases list view - pure rendering functions

use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::prelude::*;

use super::table::{render_table, TableCell, TableColumn, TableItem, TableRow};
use crate::tui::state::{Action, AppState, ListState, ReleaseItem};

impl TableItem for ReleaseItem {
    fn columns() -> Vec<TableColumn> {
        vec![
            TableColumn::new("ID", Constraint::Length(35)),
            TableColumn::new("PROJECT", Constraint::Length(35)),
            TableColumn::new("CREATED", Constraint::Length(20)),
        ]
    }

    fn to_row(&self) -> TableRow {
        let created = self.created_at.format("%Y-%m-%d %H:%M").to_string();

        TableRow::new(vec![
            TableCell::new(&self.id),
            TableCell::dim(&self.project_id),
            TableCell::dim(&created),
        ])
    }

    fn title() -> &'static str {
        "Releases"
    }

    fn empty_message() -> &'static str {
        "No releases found."
    }
}

/// Render the releases list view
pub fn render(frame: &mut Frame, area: Rect, state: &ListState<ReleaseItem>, app: &AppState) {
    render_table(frame, area, state, app);
}

/// Handle key input for releases list
pub fn handle_key(key: KeyEvent, state: &mut ListState<ReleaseItem>) -> Action {
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

/// Get keybinds for releases list
pub fn keybinds() -> Vec<(&'static str, &'static str)> {
    vec![
        ("↑/k", "up"),
        ("↓/j", "down"),
        ("r", "refresh"),
        ("/", "search"),
        ("q", "quit"),
    ]
}
