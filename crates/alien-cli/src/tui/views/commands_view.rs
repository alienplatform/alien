//! Commands (ARC) list view - pure rendering functions

use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::prelude::*;

use super::table::{render_table, TableCell, TableColumn, TableItem, TableRow};
use crate::tui::state::{Action, AppState, CommandItem, CommandState, ListState};

impl TableItem for CommandItem {
    fn columns() -> Vec<TableColumn> {
        vec![
            TableColumn::new("ID", Constraint::Length(35)),
            TableColumn::new("NAME", Constraint::Length(20)),
            TableColumn::new("STATE", Constraint::Length(12)),
            TableColumn::new("DEPLOYMENT", Constraint::Length(20)),
            TableColumn::new("DEPLOYMENT GROUP", Constraint::Length(20)),
            TableColumn::new("CREATED", Constraint::Length(20)),
        ]
    }

    fn to_row(&self) -> TableRow {
        let state_str = format!("{:?}", self.state);
        let state_cell = match self.state {
            CommandState::Succeeded => TableCell::success(&state_str),
            CommandState::Pending | CommandState::PendingUpload | CommandState::Dispatched => {
                TableCell::warning(&state_str)
            }
            CommandState::Failed | CommandState::Expired => TableCell::error(&state_str),
        };

        let created = self.created_at.format("%Y-%m-%d %H:%M").to_string();

        // Display deployment name if available, otherwise show ID
        let deployment_display = self
            .deployment_name
            .as_deref()
            .unwrap_or(&self.deployment_id);

        // Display deployment group name if available, otherwise show ID or N/A
        let dg_display = self
            .deployment_group_name
            .as_deref()
            .or(self.deployment_group_id.as_deref())
            .unwrap_or("N/A");

        TableRow::new(vec![
            TableCell::dim(&self.id),
            TableCell::new(&self.name),
            state_cell,
            TableCell::dim(deployment_display),
            TableCell::dim(dg_display),
            TableCell::dim(&created),
        ])
    }

    fn title() -> &'static str {
        "Commands"
    }

    fn empty_message() -> &'static str {
        "No commands found. Press 'n' to create one."
    }
}

/// Render the commands list view
pub fn render(
    frame: &mut Frame,
    area: Rect,
    all_commands: &ListState<CommandItem>,
    app: &AppState,
    filter_display: Option<&str>,
    filtered_commands: Vec<&CommandItem>,
) {
    // If we have a filter, add a filter bar at the top
    if let Some(filter_text) = filter_display {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // Filter bar
                Constraint::Min(5),    // Table
            ])
            .split(area);

        // Render filter bar
        render_filter_bar(frame, chunks[0], filter_text);

        // Create a temporary state with filtered items for rendering
        let mut temp_state = all_commands.clone();
        temp_state.items = filtered_commands.into_iter().cloned().collect();

        // Render table with filtered items
        render_table(frame, chunks[1], &temp_state, app);
    } else {
        // No filter, just render table with all items
        render_table(frame, area, all_commands, app);
    }
}

/// Render the filter bar
fn render_filter_bar(frame: &mut Frame, area: Rect, filter_text: &str) {
    use ratatui::style::{Color, Style};
    use ratatui::text::{Line, Span};
    use ratatui::widgets::Paragraph;

    let line = Line::from(vec![
        Span::styled(" Filter: ", Style::default().fg(Color::Rgb(107, 114, 128))),
        Span::styled(
            filter_text,
            Style::default().fg(Color::Rgb(245, 158, 11)).bold(),
        ),
        Span::styled(
            " [x to clear]",
            Style::default().fg(Color::Rgb(107, 114, 128)),
        ),
    ]);

    let paragraph = Paragraph::new(line);
    frame.render_widget(paragraph, area);
}

/// Handle key input for commands list
pub fn handle_key(key: KeyEvent, state: &mut ListState<CommandItem>) -> Action {
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
        KeyCode::Char('x') | KeyCode::Char('X') => Action::ClearFilters,
        KeyCode::Char('r') => Action::Refresh,
        _ => Action::None,
    }
}

/// Get keybinds for commands list
pub fn keybinds() -> Vec<(&'static str, &'static str)> {
    vec![
        ("↑/k", "up"),
        ("↓/j", "down"),
        ("n", "new"),
        ("x", "clear filters"),
        ("r", "refresh"),
        ("/", "search"),
        ("q", "quit"),
    ]
}
