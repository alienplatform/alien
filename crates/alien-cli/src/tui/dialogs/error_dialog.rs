//! Error dialog - displays AlienError details in a modal
//!
//! **Reusable component** for showing any AlienError anywhere in the TUI.
//!
//! AlienError is used throughout the system (alien-core, alien-infra, alien-deployment,
//! alien-build, etc.), so this dialog can display errors from any source:
//! - Deployment failures
//! - Resource provisioning errors
//! - API request failures
//! - Build/release errors
//! - Any error that implements AlienErrorData
//!
//! Usage:
//! ```ignore
//! // Press E on failed deployment
//! Action::ShowErrorDialog(deployment.error.clone())
//!
//! // API error
//! Action::ShowErrorDialog(api_error.into_generic())
//!
//! // Any AlienError<T>
//! state.open_error_dialog(error.into_generic())
//! ```

use alien_error::{AlienError, GenericError};
use ratatui::{
    crossterm::event::{KeyCode, KeyEvent},
    prelude::*,
    widgets::{
        Block, Borders, Clear, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap,
    },
};

// Colors
const RED: Color = Color::Rgb(239, 68, 68);
const YELLOW: Color = Color::Rgb(245, 158, 11);
const TEXT: Color = Color::Rgb(229, 231, 235);
const DIM_TEXT: Color = Color::Rgb(107, 114, 128);

/// Error dialog state
pub struct ErrorDialog {
    error: AlienError<GenericError>,
    scroll_offset: usize,
}

impl ErrorDialog {
    /// Create a new error dialog
    pub fn new(error: AlienError<GenericError>) -> Self {
        Self {
            error,
            scroll_offset: 0,
        }
    }

    /// Render the error dialog as a modal overlay
    pub fn render(&self, frame: &mut Frame, area: Rect) {
        // Center the dialog (70% width, 80% height for better readability)
        let dialog_width = (area.width as f32 * 0.7).min(120.0) as u16;
        let dialog_height = (area.height as f32 * 0.8).min(35.0) as u16;

        let dialog_area = Rect {
            x: (area.width.saturating_sub(dialog_width)) / 2,
            y: (area.height.saturating_sub(dialog_height)) / 2,
            width: dialog_width,
            height: dialog_height,
        };

        // Clear the dialog area (important!)
        frame.render_widget(Clear, dialog_area);

        // Dialog block with dark background
        let block = Block::default()
            .title(" Error Details ")
            .title_bottom(" [ESC] Close  [↑↓/j/k] Scroll  [Home/End] Top/Bottom ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(RED).bold())
            .style(Style::default().bg(Color::Rgb(17, 24, 39))); // Dark background

        let inner = block.inner(dialog_area);
        frame.render_widget(block, dialog_area);

        // Build error content
        let lines = self.build_error_lines();
        let total_lines = lines.len();

        // Calculate visible range with bounds checking
        let visible_height = inner.height as usize;
        let max_scroll = total_lines.saturating_sub(visible_height);
        let start_idx = self.scroll_offset.min(max_scroll);
        let end_idx = (start_idx + visible_height).min(total_lines);

        let visible_lines: Vec<Line> = if start_idx < total_lines {
            lines[start_idx..end_idx].to_vec()
        } else {
            vec![]
        };

        let paragraph = Paragraph::new(visible_lines).wrap(Wrap { trim: false });
        frame.render_widget(paragraph, inner);

        // Scrollbar
        if total_lines > visible_height {
            let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight);
            let mut scrollbar_state = ScrollbarState::new(total_lines).position(self.scroll_offset);
            frame.render_stateful_widget(
                scrollbar,
                dialog_area.inner(Margin {
                    vertical: 1,
                    horizontal: 0,
                }),
                &mut scrollbar_state,
            );
        }
    }

    /// Build the error display lines
    fn build_error_lines(&self) -> Vec<Line<'static>> {
        let mut lines = vec![];

        // Error code and message (main error)
        lines.push(Line::from(vec![
            Span::styled("● ", Style::default().fg(RED).bold()),
            Span::styled(self.error.code.clone(), Style::default().fg(RED).bold()),
        ]));

        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(self.error.message.clone(), Style::default().fg(TEXT)),
        ]));

        lines.push(Line::from(""));
        lines.push(Line::from(""));

        // Context (if present)
        if let Some(ref context) = self.error.context {
            lines.push(Line::from(vec![Span::styled(
                "Context:",
                Style::default().fg(DIM_TEXT).bold(),
            )]));

            if let Some(obj) = context.as_object() {
                for (key, value) in obj.iter() {
                    let value_str = format_context_value(value);

                    if !value_str.is_empty() {
                        // Format key (snake_case to Title Case)
                        let formatted_key = format_field_name(key);

                        // Handle multi-line values
                        if value_str.contains('\n') {
                            lines.push(Line::from(vec![
                                Span::raw("  • "),
                                Span::styled(
                                    format!("{}:", formatted_key),
                                    Style::default().fg(DIM_TEXT),
                                ),
                            ]));
                            for line in value_str.lines() {
                                lines.push(Line::from(vec![
                                    Span::raw("      "),
                                    Span::styled(line.to_string(), Style::default().fg(TEXT)),
                                ]));
                            }
                        } else {
                            lines.push(Line::from(vec![
                                Span::raw("  • "),
                                Span::styled(
                                    format!("{}: ", formatted_key),
                                    Style::default().fg(DIM_TEXT),
                                ),
                                Span::styled(value_str, Style::default().fg(TEXT)),
                            ]));
                        }
                    }
                }
            }

            lines.push(Line::from(""));
        }

        // Error chain (source errors)
        let mut current_source = self.error.source.as_deref();
        let mut depth = 0;

        if current_source.is_some() {
            lines.push(Line::from(vec![Span::styled(
                "Error Chain:",
                Style::default().fg(DIM_TEXT).bold(),
            )]));
        }

        while let Some(source) = current_source {
            if depth >= 5 {
                // Limit depth
                lines.push(Line::from(vec![
                    Span::raw("  "),
                    Span::styled(
                        "... (more errors in chain)",
                        Style::default().fg(DIM_TEXT).italic(),
                    ),
                ]));
                break;
            }

            let indent = "  ".repeat(depth + 1);
            lines.push(Line::from(vec![
                Span::raw(format!("{}└─ ", indent)),
                Span::styled(source.code.clone(), Style::default().fg(YELLOW)),
            ]));

            lines.push(Line::from(vec![
                Span::raw(format!("{}   ", indent)),
                Span::styled(source.message.clone(), Style::default().fg(DIM_TEXT)),
            ]));

            current_source = source.source.as_deref();
            depth += 1;
        }

        // Metadata section
        lines.push(Line::from(""));
        lines.push(Line::from(vec![Span::styled(
            "Metadata:",
            Style::default().fg(DIM_TEXT).bold(),
        )]));

        lines.push(Line::from(vec![
            Span::raw("  • "),
            Span::styled("Retryable: ", Style::default().fg(DIM_TEXT)),
            Span::styled(
                if self.error.retryable { "Yes" } else { "No" },
                Style::default().fg(if self.error.retryable {
                    YELLOW
                } else {
                    DIM_TEXT
                }),
            ),
        ]));

        if let Some(status_code) = self.error.http_status_code {
            lines.push(Line::from(vec![
                Span::raw("  • "),
                Span::styled("HTTP Status: ", Style::default().fg(DIM_TEXT)),
                Span::styled(status_code.to_string(), Style::default().fg(TEXT)),
            ]));
        }

        // Hint for retry
        if self.error.retryable {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled("💡 ", Style::default()),
                Span::styled(
                    "This error is retryable. Try rebuilding (B) or retrying the operation.",
                    Style::default().fg(YELLOW),
                ),
            ]));
        }

        lines
    }

    /// Handle key input
    /// Returns true if dialog should close
    pub fn handle_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => true, // Close dialog
            KeyCode::Up | KeyCode::Char('k') => {
                self.scroll_offset = self.scroll_offset.saturating_sub(1);
                false
            }
            KeyCode::Down | KeyCode::Char('j') => {
                // Bounds checking done in render, but keep reasonable
                self.scroll_offset = self.scroll_offset.saturating_add(1).min(1000);
                false
            }
            KeyCode::PageUp => {
                self.scroll_offset = self.scroll_offset.saturating_sub(10);
                false
            }
            KeyCode::PageDown => {
                // Bounds checking done in render, but keep reasonable
                self.scroll_offset = self.scroll_offset.saturating_add(10).min(1000);
                false
            }
            KeyCode::Home => {
                self.scroll_offset = 0;
                false
            }
            KeyCode::End => {
                // Scroll to bottom (bounds checked in render)
                self.scroll_offset = 1000;
                false
            }
            _ => false,
        }
    }
}

/// Format context value for display
fn format_context_value(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Array(arr) => {
            let items: Vec<String> = arr
                .iter()
                .filter_map(|v| match v {
                    serde_json::Value::String(s) => Some(s.clone()),
                    serde_json::Value::Number(n) => Some(n.to_string()),
                    _ => None,
                })
                .collect();

            if items.is_empty() {
                return String::new();
            }

            if items.len() <= 5 {
                items.join(", ")
            } else {
                format!("{}, ... (+{} more)", items[..5].join(", "), items.len() - 5)
            }
        }
        _ => String::new(), // Skip complex objects
    }
}

/// Format field name from snake_case to Title Case
fn format_field_name(field_name: &str) -> String {
    field_name
        .split('_')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}
