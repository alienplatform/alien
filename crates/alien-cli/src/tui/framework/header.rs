//! Application header widget
//!
//! Displays Alien branding, connection info, and view-specific navigation.

use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph},
};

use crate::tui::common::SPINNER_FRAMES;
use crate::tui::state::{BuildState, ConnectionInfo, DeploymentDetailState, ViewId};

/// Primary brand color - Alien green
pub const ALIEN_GREEN: Color = Color::Rgb(34, 197, 94);
/// Muted green for secondary elements  
pub const ALIEN_GREEN_MUTED: Color = Color::Rgb(22, 163, 74);
/// Dim text color
pub const DIM_TEXT: Color = Color::Rgb(107, 114, 128);
/// Normal text color  
pub const TEXT: Color = Color::Rgb(229, 231, 235);
/// Border color
pub const BORDER: Color = Color::Rgb(75, 85, 99);
/// Amber for build status
pub const AMBER: Color = Color::Rgb(245, 158, 11);

/// Header height (consistent across all views)
pub const HEADER_HEIGHT: u16 = 4;

/// Header widget for the TUI
pub struct Header;

impl Header {
    /// Render the header for list views (with tabs)
    pub fn render_list_view(
        frame: &mut Frame,
        area: Rect,
        connection: &ConnectionInfo,
        tabs: &[ViewId],
        current_view: &ViewId,
        build_state: Option<&BuildState>,
        spinner_frame: usize,
    ) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(ALIEN_GREEN_MUTED));

        let inner = block.inner(area);
        frame.render_widget(block, area);

        // 2-line layout
        let lines = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // Line 1: Logo + Connection
                Constraint::Length(1), // Line 2: Tabs + Build status
            ])
            .split(inner);

        // Line 1: Logo (left) and Connection (right)
        let line1_layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(10), // Logo
                Constraint::Min(10),    // Spacer
            ])
            .split(lines[0]);

        // Logo
        let logo = Paragraph::new("🪐 ALIEN").style(Style::default().fg(ALIEN_GREEN).bold());
        frame.render_widget(logo, line1_layout[0]);

        // Connection (right-aligned)
        let (label, label_color) = match connection {
            ConnectionInfo::Dev { .. } => ("LOCAL", AMBER),
            ConnectionInfo::Platform { .. } => ("PROD", ALIEN_GREEN),
        };

        let connection_line = Line::from(vec![
            Span::styled(
                format!("[{}]", label),
                Style::default().fg(label_color).bold(),
            ),
            Span::raw(" "),
            Span::styled(connection.display_text(), Style::default().fg(DIM_TEXT)),
        ]);
        let connection_widget = Paragraph::new(connection_line).alignment(Alignment::Right);
        frame.render_widget(connection_widget, lines[0]);

        // Line 2: Tabs (left) and Build status (right)
        let line2_layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Min(40), // Tabs
                Constraint::Min(30), // Build status
            ])
            .split(lines[1]);

        // Tabs
        Self::render_tabs(frame, line2_layout[0], tabs, current_view);

        // Build status (right-aligned)
        if let Some(build_state) = build_state {
            Self::render_build_status(frame, line2_layout[1], build_state, spinner_frame);
        }
    }

    /// Render the header for detail views (with back button)
    pub fn render_detail_view(
        frame: &mut Frame,
        area: Rect,
        connection: &ConnectionInfo,
        detail: &DeploymentDetailState,
        build_state: Option<&BuildState>,
        spinner_frame: usize,
    ) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(ALIEN_GREEN_MUTED));

        let inner = block.inner(area);
        frame.render_widget(block, area);

        // 2-line layout
        let lines = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // Line 1: Logo + Connection
                Constraint::Length(1), // Line 2: Breadcrumb + Build status
            ])
            .split(inner);

        // Line 1: Logo (left) and Connection (right)
        let line1_layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(10), // Logo
                Constraint::Min(10),    // Spacer
            ])
            .split(lines[0]);

        // Logo
        let logo = Paragraph::new("🪐 ALIEN").style(Style::default().fg(ALIEN_GREEN).bold());
        frame.render_widget(logo, line1_layout[0]);

        // Connection (right-aligned)
        let (label, label_color) = match connection {
            ConnectionInfo::Dev { .. } => ("LOCAL", AMBER),
            ConnectionInfo::Platform { .. } => ("PROD", ALIEN_GREEN),
        };

        let connection_line = Line::from(vec![
            Span::styled(
                format!("[{}]", label),
                Style::default().fg(label_color).bold(),
            ),
            Span::raw(" "),
            Span::styled(connection.display_text(), Style::default().fg(DIM_TEXT)),
        ]);
        let connection_widget = Paragraph::new(connection_line).alignment(Alignment::Right);
        frame.render_widget(connection_widget, lines[0]);

        // Line 2: Breadcrumb (left) and Build status (right)
        let line2_layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Min(30), // Breadcrumb
                Constraint::Min(30), // Build status
            ])
            .split(lines[1]);

        // Breadcrumb
        Self::render_breadcrumb(frame, line2_layout[0], detail);

        // Build status (right-aligned)
        if let Some(build_state) = build_state {
            Self::render_build_status(frame, line2_layout[1], build_state, spinner_frame);
        }
    }

    fn render_tabs(frame: &mut Frame, area: Rect, tabs: &[ViewId], current: &ViewId) {
        let mut spans = Vec::new();

        for (i, tab) in tabs.iter().enumerate() {
            let is_active = tab == current;

            // Tab with shortcut
            if is_active {
                spans.push(Span::styled(
                    format!("[{}] {}", i + 1, tab.title()),
                    Style::default().fg(ALIEN_GREEN).bold(),
                ));
            } else {
                spans.push(Span::styled(
                    format!("[{}] {}", i + 1, tab.title()),
                    Style::default().fg(DIM_TEXT),
                ));
            }

            // Separator between tabs
            if i < tabs.len() - 1 {
                spans.push(Span::styled("  ", Style::default()));
            }
        }

        let line = Line::from(spans);
        let tabs_widget = Paragraph::new(line);
        frame.render_widget(tabs_widget, area);
    }

    fn render_breadcrumb(frame: &mut Frame, area: Rect, detail: &DeploymentDetailState) {
        let line = Line::from(vec![
            Span::styled("← ", Style::default().fg(DIM_TEXT)),
            Span::styled("[ESC]", Style::default().fg(ALIEN_GREEN)),
            Span::styled(" back", Style::default().fg(DIM_TEXT)),
            Span::raw("   "),
            Span::styled("Deployment: ", Style::default().fg(DIM_TEXT)),
            Span::styled(&detail.deployment_name, Style::default().fg(TEXT).bold()),
        ]);

        let widget = Paragraph::new(line);
        frame.render_widget(widget, area);
    }

    fn render_build_status(
        frame: &mut Frame,
        area: Rect,
        build_state: &BuildState,
        spinner_frame: usize,
    ) {
        let Some(status_text) = build_state.header_display() else {
            return;
        };

        let (color, icon): (Color, String) = match build_state {
            BuildState::Initializing | BuildState::Building => {
                let spinner = SPINNER_FRAMES[spinner_frame % SPINNER_FRAMES.len()];
                (AMBER, spinner.to_string())
            }
            BuildState::Built { .. } => (ALIEN_GREEN, "✓".to_string()),
            BuildState::Failed { .. } => (Color::Rgb(239, 68, 68), "✗".to_string()),
            BuildState::Idle => return,
        };

        let line = Line::from(vec![Span::styled(
            format!("{} {}", icon, status_text),
            Style::default().fg(color),
        )]);

        let widget = Paragraph::new(line).alignment(Alignment::Right);
        frame.render_widget(widget, area);
    }
}
