//! Reusable table rendering for list views
//!
//! Pure rendering functions for tables with selection, search filtering, and scrolling.

use ratatui::{
    prelude::*,
    widgets::{
        Block, Borders, Cell, Paragraph, Row, Scrollbar, ScrollbarOrientation, ScrollbarState,
        Table,
    },
};

use crate::tui::state::{AppState, ListState};

/// Column definition for a table
#[derive(Debug, Clone)]
pub struct TableColumn {
    /// Column header text
    pub header: String,
    /// Column width constraint
    pub width: Constraint,
}

impl TableColumn {
    pub fn new(header: impl Into<String>, width: Constraint) -> Self {
        Self {
            header: header.into(),
            width,
        }
    }
}

/// A cell in a table row
#[derive(Debug, Clone)]
pub struct TableCell {
    /// Cell content
    pub content: String,
    /// Cell style
    pub style: Style,
}

impl TableCell {
    pub fn new(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            style: Style::default().fg(Color::Rgb(229, 231, 235)),
        }
    }

    pub fn styled(content: impl Into<String>, style: Style) -> Self {
        Self {
            content: content.into(),
            style,
        }
    }

    pub fn dim(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            style: Style::default().fg(Color::Rgb(107, 114, 128)),
        }
    }

    pub fn success(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            style: Style::default().fg(Color::Rgb(34, 197, 94)),
        }
    }

    pub fn warning(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            style: Style::default().fg(Color::Rgb(245, 158, 11)),
        }
    }

    pub fn error(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            style: Style::default().fg(Color::Rgb(239, 68, 68)),
        }
    }

    pub fn primary(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            style: Style::default().fg(Color::Rgb(59, 130, 246)),
        }
    }
}

/// A row in the table with styled cells
#[derive(Debug, Clone)]
pub struct TableRow {
    /// Cell values
    pub cells: Vec<TableCell>,
}

impl TableRow {
    pub fn new(cells: Vec<TableCell>) -> Self {
        Self { cells }
    }
}

/// Trait for items that can be rendered in a table
pub trait TableItem {
    /// Column definitions for this item type
    fn columns() -> Vec<TableColumn>;
    /// Convert this item to a table row
    fn to_row(&self) -> TableRow;
    /// Title for the table
    fn title() -> &'static str;
    /// Message shown when list is empty
    fn empty_message() -> &'static str;
}

/// Render a table with the given state
pub fn render_table<T: TableItem>(
    frame: &mut Frame,
    area: Rect,
    state: &ListState<T>,
    app: &AppState,
) {
    // Loading state
    if state.loading && state.items.is_empty() {
        let loading = Paragraph::new(format!("Loading {}...", T::title().to_lowercase()))
            .style(Style::default().fg(Color::Rgb(107, 114, 128)))
            .alignment(Alignment::Center);
        frame.render_widget(loading, area);
        return;
    }

    // Error state
    if let Some(ref error) = state.error {
        let error_widget = Paragraph::new(error.as_str())
            .style(Style::default().fg(Color::Rgb(239, 68, 68)))
            .alignment(Alignment::Center);
        frame.render_widget(error_widget, area);
        return;
    }

    // Empty state
    if state.items.is_empty() {
        render_empty(frame, area, T::title(), T::empty_message());
        return;
    }

    let columns = T::columns();
    let rows: Vec<TableRow> = state.items.iter().map(|item| item.to_row()).collect();
    let search_query = app.search.query();

    // Filter rows by search query
    let filtered_indices: Vec<usize> = if let Some(query) = search_query {
        let query_lower = query.to_lowercase();
        rows.iter()
            .enumerate()
            .filter(|(_, row)| {
                row.cells
                    .iter()
                    .any(|cell| cell.content.to_lowercase().contains(&query_lower))
            })
            .map(|(i, _)| i)
            .collect()
    } else {
        (0..rows.len()).collect()
    };

    // Build header row
    let header_cells: Vec<Cell> = columns
        .iter()
        .map(|col| {
            Cell::from(col.header.as_str())
                .style(Style::default().fg(Color::Rgb(107, 114, 128)).bold())
        })
        .collect();
    let header = Row::new(header_cells).height(1);

    // Build data rows
    let data_rows: Vec<Row> = filtered_indices
        .iter()
        .map(|&idx| {
            let row = &rows[idx];
            let is_selected = state.selected == Some(idx);
            let row_style = if is_selected {
                Style::default().bg(Color::Rgb(55, 65, 81))
            } else {
                Style::default()
            };

            let cells: Vec<Cell> = row
                .cells
                .iter()
                .map(|cell| Cell::from(cell.content.as_str()).style(cell.style))
                .collect();

            Row::new(cells).style(row_style)
        })
        .collect();

    // Build column widths
    let widths: Vec<Constraint> = columns.iter().map(|col| col.width).collect();

    // Create title with count
    let title_text = if let Some(query) = search_query {
        format!(
            " {} ({} of {} matching '{}') ",
            T::title(),
            filtered_indices.len(),
            rows.len(),
            query
        )
    } else {
        format!(" {} ({}) ", T::title(), rows.len())
    };

    // Build table
    let table = Table::new(data_rows, widths)
        .header(header)
        .block(
            Block::default()
                .title(title_text)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Rgb(75, 85, 99))),
        )
        .row_highlight_style(Style::default().bg(Color::Rgb(55, 65, 81)));

    // Render table with selection
    let mut table_state = ratatui::widgets::TableState::default();

    // Map selected index to filtered index
    if let Some(sel) = state.selected {
        if let Some(pos) = filtered_indices.iter().position(|&i| i == sel) {
            table_state.select(Some(pos));
        }
    }

    frame.render_stateful_widget(table, area, &mut table_state);

    // Render scrollbar if needed
    let visible_rows = area.height.saturating_sub(3) as usize;
    if filtered_indices.len() > visible_rows {
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight);
        let pos = state
            .selected
            .and_then(|sel| filtered_indices.iter().position(|&i| i == sel))
            .unwrap_or(0);
        let mut scrollbar_state = ScrollbarState::new(filtered_indices.len()).position(pos);

        frame.render_stateful_widget(
            scrollbar,
            area.inner(Margin {
                vertical: 1,
                horizontal: 0,
            }),
            &mut scrollbar_state,
        );
    }
}

/// Render an empty state message
pub fn render_empty(frame: &mut Frame, area: Rect, title: &str, message: &str) {
    let block = Block::default()
        .title(format!(" {} ", title))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Rgb(75, 85, 99)));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let text = Paragraph::new(message)
        .style(Style::default().fg(Color::Rgb(107, 114, 128)))
        .alignment(Alignment::Center);

    // Center vertically
    let y_offset = inner.height / 2;
    let centered_area = Rect {
        x: inner.x,
        y: inner.y + y_offset,
        width: inner.width,
        height: 1,
    };
    frame.render_widget(text, centered_area);
}
