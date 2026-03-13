//! Tab bar widget for navigation between views

use ratatui::{prelude::*, widgets::Tabs as RatatuiTabs};

use crate::tui::state::ViewId;

/// Tab bar widget
pub struct TabBar;

impl TabBar {
    /// Render the tab bar with view IDs
    pub fn render(frame: &mut Frame, area: Rect, tabs: &[ViewId], current: ViewId) {
        // Create tab titles with shortcuts
        let titles: Vec<Line> = tabs
            .iter()
            .enumerate()
            .map(|(i, view_id)| {
                let shortcut = format!("[{}] ", i + 1);
                let is_active = *view_id == current;

                let style = if is_active {
                    Style::default()
                        .fg(Color::Rgb(34, 197, 94)) // Green for active
                        .bold()
                } else {
                    Style::default().fg(Color::Rgb(156, 163, 175)) // Gray for inactive
                };

                Line::from(vec![
                    Span::styled(
                        shortcut,
                        Style::default().fg(Color::Rgb(107, 114, 128)).dim(),
                    ),
                    Span::styled(view_id.title(), style),
                ])
            })
            .collect();

        // Find active tab index
        let selected = tabs.iter().position(|v| *v == current).unwrap_or(0);

        // Simple tab bar without block - just render tabs directly
        let tabs_widget = RatatuiTabs::new(titles)
            .select(selected)
            .style(Style::default())
            .highlight_style(Style::default())
            .divider(" │ ");

        frame.render_widget(tabs_widget, area);
    }
}
