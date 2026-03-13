//! Reusable searchable table component for list views

use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Cell, Row, Scrollbar, ScrollbarOrientation, ScrollbarState, Table},
};

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

/// A row in the table with styled cells
#[derive(Debug, Clone)]
pub struct TableRow {
    /// Cell values
    pub cells: Vec<TableCell>,
    /// Whether this row is selectable
    pub selectable: bool,
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

impl TableRow {
    pub fn new(cells: Vec<TableCell>) -> Self {
        Self {
            cells,
            selectable: true,
        }
    }
}

/// State for a searchable table
#[derive(Debug, Clone)]
pub struct TableState {
    /// Currently selected row index
    selected: Option<usize>,
    /// Total number of rows
    total_rows: usize,
    /// Offset for scrolling
    offset: usize,
}

impl TableState {
    pub fn new() -> Self {
        Self {
            selected: None,
            total_rows: 0,
            offset: 0,
        }
    }

    pub fn with_rows(total: usize) -> Self {
        Self {
            selected: if total > 0 { Some(0) } else { None },
            total_rows: total,
            offset: 0,
        }
    }

    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    pub fn set_total(&mut self, total: usize) {
        self.total_rows = total;
        if let Some(sel) = self.selected {
            if sel >= total {
                self.selected = if total > 0 { Some(total - 1) } else { None };
            }
        } else if total > 0 {
            self.selected = Some(0);
        }
    }

    pub fn select_next(&mut self) {
        if self.total_rows == 0 {
            return;
        }
        let i = match self.selected {
            Some(i) => (i + 1).min(self.total_rows - 1),
            None => 0,
        };
        self.selected = Some(i);
    }

    pub fn select_prev(&mut self) {
        if self.total_rows == 0 {
            return;
        }
        let i = match self.selected {
            Some(i) => i.saturating_sub(1),
            None => 0,
        };
        self.selected = Some(i);
    }

    pub fn select_first(&mut self) {
        if self.total_rows > 0 {
            self.selected = Some(0);
        }
    }

    pub fn select_last(&mut self) {
        if self.total_rows > 0 {
            self.selected = Some(self.total_rows - 1);
        }
    }
}

impl Default for TableState {
    fn default() -> Self {
        Self::new()
    }
}

/// Searchable table widget
pub struct SearchableTable;

impl SearchableTable {
    /// Render a searchable table
    pub fn render(
        frame: &mut Frame,
        area: Rect,
        title: &str,
        columns: &[TableColumn],
        rows: &[TableRow],
        state: &mut TableState,
        search_query: Option<&str>,
    ) {
        // Update state total
        state.set_total(rows.len());

        // Filter rows by search query
        let filtered_rows: Vec<&TableRow> = if let Some(query) = search_query {
            let query_lower = query.to_lowercase();
            rows.iter()
                .filter(|row| {
                    row.cells
                        .iter()
                        .any(|cell| cell.content.to_lowercase().contains(&query_lower))
                })
                .collect()
        } else {
            rows.iter().collect()
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
        let data_rows: Vec<Row> = filtered_rows
            .iter()
            .enumerate()
            .map(|(idx, row)| {
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
                title,
                filtered_rows.len(),
                rows.len(),
                query
            )
        } else {
            format!(" {} ({}) ", title, rows.len())
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

        // Render table
        let mut ratatui_state = ratatui::widgets::TableState::default();
        ratatui_state.select(state.selected);
        frame.render_stateful_widget(table, area, &mut ratatui_state);

        // Render scrollbar if needed
        let visible_rows = area.height.saturating_sub(3) as usize; // Account for borders and header
        if filtered_rows.len() > visible_rows {
            let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight);
            let mut scrollbar_state =
                ScrollbarState::new(filtered_rows.len()).position(state.selected.unwrap_or(0));

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
        use ratatui::widgets::Paragraph;

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
}
