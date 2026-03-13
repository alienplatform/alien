//! Search overlay component activated by `/`

use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Clear, Paragraph},
};

/// Search overlay widget
pub struct SearchOverlay;

impl SearchOverlay {
    /// Render the search overlay at the bottom of the screen
    pub fn render(frame: &mut Frame, area: Rect, search: &crate::tui::state::SearchState) {
        let query = search.query().unwrap_or("");
        Self::render_with_query(frame, area, query);
    }

    /// Render the search overlay with a raw query string
    pub fn render_with_query(frame: &mut Frame, area: Rect, query: &str) {
        // Position at bottom of screen
        let overlay_height = 3;
        let overlay_area = Rect {
            x: area.x + 2,
            y: area.height.saturating_sub(overlay_height + 1),
            width: area.width.saturating_sub(4).min(60),
            height: overlay_height,
        };

        // Clear the area
        frame.render_widget(Clear, overlay_area);

        // Render the search box
        let block = Block::default()
            .title(" Search ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Rgb(34, 197, 94)));

        let inner = block.inner(overlay_area);
        frame.render_widget(block, overlay_area);

        // Render the input with cursor
        let cursor = "│";
        let input_text = format!("/{}{}", query, cursor);
        let input =
            Paragraph::new(input_text).style(Style::default().fg(Color::Rgb(229, 231, 235)));

        frame.render_widget(input, inner);
    }
}
