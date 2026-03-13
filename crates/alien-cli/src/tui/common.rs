//! Common TUI infrastructure shared between all UI components
//!
//! This module provides the shared foundation for all TUI components including:
//! - Common constants and utilities
//! - Shared UI widgets and rendering helpers
//! - Common data structures

use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Padding, Paragraph},
};
use std::time::Duration;

/// Common spinner frames used across all UI components
pub const SPINNER_FRAMES: &[char] = &['⠇', '⠏', '⠋', '⠙', '⠸', '⠴', '⠦', '⠇'];

/// Viewport height constraints
pub const MIN_VIEWPORT_HEIGHT: u16 = 6;
pub const MAX_VIEWPORT_HEIGHT: u16 = 60;

/// Common widgets and rendering utilities
pub mod widgets {
    use super::*;

    /// Render a header with platform information
    pub fn render_header(frame: &mut Frame, area: Rect, title: &str, platform: &str, color: Color) {
        let platform_upper = platform.to_uppercase();
        let header_text = format!("👾 {} {}", title, platform_upper);

        let header = Paragraph::new(header_text).style(Style::default().fg(color).bold());
        frame.render_widget(header, area);
    }

    /// Render a success box with operation-specific messaging
    pub fn render_success_box(
        frame: &mut Frame,
        area: Rect,
        success_message: &str,
        next_steps: Vec<&str>,
        elapsed: Duration,
        color: Color,
    ) {
        let elapsed_text = format!("{:.1}s", elapsed.as_secs_f64());

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(color))
            .title(" Success ")
            .padding(Padding::new(1, 1, 1, 1)); // left, right, top, bottom

        let inner = block.inner(area);
        frame.render_widget(block, area);

        let mut lines = vec![
            Line::from(vec![
                Span::styled(success_message, Style::default().fg(color).bold()),
                Span::raw("  "),
                Span::styled(
                    format!("Time: {}", elapsed_text),
                    Style::default().fg(Color::Rgb(156, 163, 175)),
                ),
            ]),
            Line::from(""),
            Line::from(vec![Span::styled("Next steps:", Style::default().bold())]),
        ];

        for step in next_steps {
            lines.push(Line::from(vec![Span::styled(
                step,
                Style::default().fg(Color::Rgb(156, 163, 175)),
            )]));
        }

        let paragraph = Paragraph::new(lines);
        frame.render_widget(paragraph, inner);
    }

    /// Get step status symbol and color
    pub fn get_step_status_display(status: &StepStatus, spinner_char: char) -> (String, Color) {
        match status {
            StepStatus::Pending => ("⏳".to_string(), Color::Rgb(107, 114, 128)), // Gray
            StepStatus::InProgress => {
                (spinner_char.to_string(), Color::Rgb(245, 158, 11)) // Amber - use actual spinner char
            }
            StepStatus::Completed => ("✓".to_string(), Color::Rgb(34, 197, 94)), // Green
            StepStatus::Failed(_) => ("✗".to_string(), Color::Rgb(239, 68, 68)), // Red
        }
    }
}

/// Common step status enum used across UI components
#[derive(Debug, Clone, PartialEq)]
pub enum StepStatus {
    Pending,
    InProgress,
    Completed,
    Failed(String),
}

/// Common step state used across UI components
#[derive(Debug, Clone)]
pub struct StepState {
    /// Step display name
    pub name: String,
    /// Current status
    pub status: StepStatus,
    /// Optional warning count for completed steps
    pub warning_count: Option<usize>,
}
