//! Global Logs view
//!
//! Displays all logs from all agents with filtering capabilities.
//! Works for both local dev (via log_rx channel) and production (via DeepStore SSE).

use chrono::{Local, TimeZone};
use ratatui::{
    crossterm::event::{KeyCode, KeyEvent},
    prelude::*,
    widgets::{Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState},
};
use std::collections::VecDeque;

use crate::tui::app::config::AppMode;
use crate::tui::state::{Action, AppState, LogLevel, LogLine, LogsConnectionStatus, LogsViewState};

/// Render the logs view
pub fn render(
    frame: &mut Frame,
    area: Rect,
    logs_state: &LogsViewState,
    global_logs: &VecDeque<LogLine>,
    app: &AppState,
    mode: AppMode,
    deployment_name_cache: &std::collections::HashMap<String, String>,
) {
    // Layout: status bar (1 line) | search bar (1 line if searching) | logs panel
    let search_height = if logs_state.is_searching { 1 } else { 0 };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),             // Status bar
            Constraint::Length(search_height), // Search bar (when active)
            Constraint::Min(5),                // Logs
        ])
        .split(area);

    // Render status bar
    render_status_bar(
        frame,
        chunks[0],
        logs_state,
        global_logs.len(),
        mode,
        deployment_name_cache,
    );

    // Render search bar (when active)
    if logs_state.is_searching {
        render_search_bar(frame, chunks[1], logs_state, mode);
    }

    // Filter logs (for local dev, this is text filtering; for platform, server already filtered)
    let filtered_logs = logs_state.filter_logs(global_logs);
    let total_logs = filtered_logs.len();

    // Calculate visible area (leave space for scrollbar)
    let logs_chunk = chunks[2];
    let log_area = Rect {
        x: logs_chunk.x,
        y: logs_chunk.y,
        width: logs_chunk.width.saturating_sub(1),
        height: logs_chunk.height,
    };

    // Render logs
    render_logs(frame, log_area, &filtered_logs, logs_state, app);

    // Render scrollbar
    let scrollbar_area = Rect {
        x: logs_chunk.x + logs_chunk.width.saturating_sub(1),
        y: logs_chunk.y,
        width: 1,
        height: logs_chunk.height,
    };

    if total_logs > 0 {
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight);
        let mut scrollbar_state = ScrollbarState::new(total_logs)
            .position(total_logs.saturating_sub(logs_state.scroll_offset + 1));
        frame.render_stateful_widget(scrollbar, scrollbar_area, &mut scrollbar_state);
    }
}

fn render_search_bar(frame: &mut Frame, area: Rect, logs_state: &LogsViewState, mode: AppMode) {
    let hint = match mode {
        AppMode::Dev => " (text search)",
        AppMode::Platform => " (DeepStore query - Enter to search)",
    };

    let query_display = if logs_state.search_query.is_empty() {
        "Type to search...".to_string()
    } else {
        logs_state.search_query.clone()
    };

    let line = Line::from(vec![
        Span::styled(
            " Search: ",
            Style::default().fg(Color::Rgb(96, 165, 250)).bold(),
        ),
        Span::styled(
            &query_display,
            if logs_state.search_query.is_empty() {
                Style::default().fg(Color::Rgb(107, 114, 128)).italic()
            } else {
                Style::default().fg(Color::Rgb(229, 231, 235))
            },
        ),
        Span::styled(
            "█", // Cursor
            Style::default().fg(Color::Rgb(96, 165, 250)),
        ),
        Span::styled(hint, Style::default().fg(Color::Rgb(107, 114, 128))),
    ]);

    let paragraph = Paragraph::new(line).style(Style::default().bg(Color::Rgb(31, 41, 55))); // Darker background
    frame.render_widget(paragraph, area);
}

fn render_status_bar(
    frame: &mut Frame,
    area: Rect,
    logs_state: &LogsViewState,
    total_logs: usize,
    mode: AppMode,
    deployment_name_cache: &std::collections::HashMap<String, String>,
) {
    let mut spans = Vec::new();

    // Deployment filter (if active)
    if let Some(filter_display) = logs_state.get_filter_display(deployment_name_cache) {
        spans.push(Span::styled(
            " Filter: ",
            Style::default().fg(Color::Rgb(107, 114, 128)),
        ));
        spans.push(Span::styled(
            filter_display,
            Style::default().fg(Color::Rgb(245, 158, 11)).bold(),
        ));
        spans.push(Span::styled(
            " [x to clear]",
            Style::default().fg(Color::Rgb(107, 114, 128)),
        ));
        spans.push(Span::raw("  │  "));
    }

    // Source indicator
    let source_text = match mode {
        AppMode::Dev => "Local Dev".to_string(),
        AppMode::Platform => {
            if let Some(am) = logs_state.selected_manager() {
                am.name.clone()
            } else if logs_state.managers.is_empty() {
                "No Managers".to_string()
            } else {
                "Select Manager".to_string()
            }
        }
    };

    spans.push(Span::styled(
        " Source: ",
        Style::default().fg(Color::Rgb(107, 114, 128)),
    ));
    spans.push(Span::styled(
        source_text,
        Style::default().fg(Color::Rgb(96, 165, 250)).bold(),
    ));

    // Connection status (only for platform mode)
    if mode == AppMode::Platform {
        let (status_text, status_color) = match &logs_state.connection_status {
            LogsConnectionStatus::Disconnected => ("●", Color::Rgb(107, 114, 128)),
            LogsConnectionStatus::Connecting => ("●", Color::Rgb(245, 158, 11)),
            LogsConnectionStatus::Connected => ("●", Color::Rgb(34, 197, 94)),
            LogsConnectionStatus::Error(_) => ("●", Color::Rgb(239, 68, 68)),
        };

        spans.push(Span::raw("  "));
        spans.push(Span::styled(status_text, Style::default().fg(status_color)));
        spans.push(Span::styled(
            format!(" {}", logs_state.connection_status.display_text()),
            Style::default().fg(Color::Rgb(156, 163, 175)),
        ));
    }

    // Log count
    spans.push(Span::raw("  │  "));
    spans.push(Span::styled(
        format!("{} logs", total_logs),
        Style::default().fg(Color::Rgb(156, 163, 175)),
    ));

    // Manager navigation hint (platform mode only)
    if mode == AppMode::Platform && !logs_state.managers.is_empty() {
        // Add to right side
        let hint = " [←/→ Switch Source]";
        let hint_width = hint.len();
        let used_width: usize = spans.iter().map(|s| s.content.len()).sum();
        let padding = area.width as usize - used_width - hint_width;
        if padding > 0 {
            spans.push(Span::raw(" ".repeat(padding)));
            spans.push(Span::styled(
                hint,
                Style::default().fg(Color::Rgb(75, 85, 99)),
            ));
        }
    }

    let line = Line::from(spans);
    let paragraph = Paragraph::new(line);
    frame.render_widget(paragraph, area);
}

fn render_logs(
    frame: &mut Frame,
    area: Rect,
    filtered_logs: &[&LogLine],
    logs_state: &LogsViewState,
    _app: &AppState,
) {
    let total_logs = filtered_logs.len();
    let visible_height = area.height as usize;

    // Calculate which logs to show (newest at bottom)
    let start_idx = if logs_state.auto_scroll {
        total_logs.saturating_sub(visible_height)
    } else {
        total_logs.saturating_sub(visible_height + logs_state.scroll_offset)
    };
    let end_idx = (start_idx + visible_height).min(total_logs);

    let visible_logs = &filtered_logs[start_idx..end_idx];

    // Build log lines with formatting
    let mut lines: Vec<Line> = Vec::with_capacity(visible_logs.len());

    for log in visible_logs {
        let line = format_log_line(log, area.width as usize);
        lines.push(line);
    }

    // Show empty state if no logs
    if lines.is_empty() {
        let (empty_text, color) = match &logs_state.connection_status {
            LogsConnectionStatus::Disconnected if logs_state.initializing => {
                ("Loading logs...", Color::Rgb(245, 158, 11)) // Amber for loading
            }
            LogsConnectionStatus::Connecting => {
                ("Connecting to log stream...", Color::Rgb(245, 158, 11)) // Amber for loading
            }
            LogsConnectionStatus::Disconnected => {
                ("Disconnected from log stream", Color::Rgb(107, 114, 128))
            }
            LogsConnectionStatus::Error(e) => {
                // Show error in a more noticeable way
                let err_msg = format!("Error connecting to logs: {}", e);
                let paragraph = Paragraph::new(err_msg)
                    .style(Style::default().fg(Color::Rgb(239, 68, 68)))
                    .alignment(Alignment::Center)
                    .block(Block::default().borders(Borders::NONE));
                frame.render_widget(paragraph, area);
                return;
            }
            LogsConnectionStatus::Connected if logs_state.initializing => {
                ("Loading logs...", Color::Rgb(245, 158, 11)) // Still initializing even though connected
            }
            LogsConnectionStatus::Connected => {
                if logs_state.filter.filter_active || !logs_state.search_query.is_empty() {
                    (
                        "No logs match the current filter",
                        Color::Rgb(107, 114, 128),
                    )
                } else {
                    (
                        "No logs yet. Logs will appear here when your deployments start running.",
                        Color::Rgb(107, 114, 128),
                    )
                }
            }
        };

        let paragraph = Paragraph::new(empty_text)
            .style(Style::default().fg(color))
            .alignment(Alignment::Center)
            .block(Block::default().borders(Borders::NONE));

        frame.render_widget(paragraph, area);
        return;
    }

    let paragraph = Paragraph::new(lines).block(Block::default().borders(Borders::NONE));

    frame.render_widget(paragraph, area);
}

fn format_log_line(log: &LogLine, max_width: usize) -> Line<'static> {
    // Format: [HH:MM:SS] [LEVEL] deployment-group › deployment › resource | message
    let timestamp = Local
        .from_utc_datetime(&log.timestamp.naive_utc())
        .format("%H:%M:%S")
        .to_string();

    let level_str = match log.level {
        LogLevel::Debug => "DEBUG",
        LogLevel::Info => " INFO",
        LogLevel::Warn => " WARN",
        LogLevel::Error => "ERROR",
    };

    // Build deployment display: prefer deployment_name, fallback to deployment_id
    let deployment_display = log.deployment_name.as_deref().unwrap_or(&log.deployment_id);
    let deployment_short = if deployment_display.len() > 15 {
        format!("{}...", &deployment_display[..14])
    } else {
        deployment_display.to_string()
    };

    // Build deployment group display if available
    let dg_display = log
        .deployment_group_name
        .as_deref()
        .or(log.deployment_group_id.as_deref());
    let dg_short_opt = dg_display.map(|dg| {
        if dg.len() > 12 {
            format!("{}…", &dg[..11])
        } else {
            dg.to_string()
        }
    });

    let resource_short = if log.resource_id.len() > 15 {
        format!("{}…", &log.resource_id[..14])
    } else {
        log.resource_id.clone()
    };

    // Build prefix: deployment-group > deployment > resource
    let prefix = if let Some(ref dg) = dg_short_opt {
        format!(
            "{} {} {} > {} > {}",
            timestamp, level_str, dg, deployment_short, resource_short
        )
    } else {
        format!(
            "{} {} {} > {}",
            timestamp, level_str, deployment_short, resource_short
        )
    };
    let prefix_len = prefix.len();

    // Calculate remaining space for message
    let separator = " │ ";
    let message_space = max_width.saturating_sub(prefix_len + separator.len());
    let message = if log.content.len() > message_space && message_space > 3 {
        format!("{}…", &log.content[..message_space - 1])
    } else {
        log.content.clone()
    };

    // Build spans: deployment-group > deployment > resource
    let mut spans = vec![
        Span::styled(
            timestamp,
            Style::default().fg(Color::Rgb(107, 114, 128)), // gray
        ),
        Span::raw(" "),
        Span::styled(level_str, Style::default().fg(log.level.color()).bold()),
        Span::raw(" "),
    ];

    if let Some(dg_short) = dg_short_opt {
        spans.push(Span::styled(
            dg_short,
            Style::default().fg(Color::Rgb(34, 197, 94)), // green for deployment group
        ));
        spans.push(Span::styled(
            " › ",
            Style::default().fg(Color::Rgb(75, 85, 99)), // dimmed separator
        ));
    }

    spans.extend(vec![
        Span::styled(
            deployment_short,
            Style::default().fg(Color::Rgb(96, 165, 250)), // blue
        ),
        Span::styled(
            " › ",
            Style::default().fg(Color::Rgb(75, 85, 99)), // dimmed separator
        ),
        Span::styled(
            resource_short,
            Style::default().fg(Color::Rgb(167, 139, 250)), // purple
        ),
        Span::styled(separator, Style::default().fg(Color::Rgb(55, 65, 81))),
        Span::styled(
            message,
            Style::default().fg(Color::Rgb(229, 231, 235)), // light gray
        ),
    ]);

    Line::from(spans)
}

/// Handle key events for the logs view
pub fn handle_key(
    key: KeyEvent,
    logs_state: &mut LogsViewState,
    total_logs: usize,
    mode: AppMode,
) -> Action {
    // If in search mode, handle search input
    if logs_state.is_searching {
        return handle_search_input(key, logs_state, mode);
    }

    match key.code {
        // Start search
        KeyCode::Char('/') | KeyCode::Char('s') => {
            logs_state.is_searching = true;
            Action::None
        }
        // Clear filters
        KeyCode::Char('x') | KeyCode::Char('X') => Action::ClearFilters,
        // Scroll up (older logs)
        KeyCode::Up | KeyCode::Char('k') => {
            logs_state.scroll_up(1, total_logs);
            Action::None
        }
        // Scroll down (newer logs)
        KeyCode::Down | KeyCode::Char('j') => {
            logs_state.scroll_down(1);
            Action::None
        }
        // Page up
        KeyCode::PageUp => {
            logs_state.scroll_up(20, total_logs);
            Action::None
        }
        // Page down
        KeyCode::PageDown => {
            logs_state.scroll_down(20);
            Action::None
        }
        // Jump to bottom (newest)
        KeyCode::Char('G') | KeyCode::End => {
            logs_state.scroll_to_bottom();
            Action::None
        }
        // Jump to top (oldest)
        KeyCode::Char('g') | KeyCode::Home => {
            logs_state.scroll_up(total_logs, total_logs);
            Action::None
        }
        // Clear filters
        KeyCode::Char('c') => {
            logs_state.filter.show_all();
            logs_state.search_query.clear();
            Action::None
        }
        // Switch manager (platform mode only)
        KeyCode::Left | KeyCode::Char('h') if mode == AppMode::Platform => {
            logs_state.select_prev_manager();
            Action::SwitchLogSource
        }
        KeyCode::Right | KeyCode::Char('l') if mode == AppMode::Platform => {
            logs_state.select_next_manager();
            Action::SwitchLogSource
        }
        // Refresh
        KeyCode::Char('r') => Action::Refresh,
        _ => Action::None,
    }
}

/// Handle key events while in search mode
fn handle_search_input(key: KeyEvent, logs_state: &mut LogsViewState, mode: AppMode) -> Action {
    match key.code {
        // Exit search mode
        KeyCode::Esc => {
            logs_state.is_searching = false;
            Action::None
        }
        // Submit search (for platform mode: triggers DeepStore search)
        KeyCode::Enter => {
            logs_state.is_searching = false;
            if mode == AppMode::Platform && !logs_state.search_query.is_empty() {
                // Trigger server-side search for platform mode
                Action::SearchLogs(logs_state.search_query.clone())
            } else {
                // For local dev, search is client-side (already applied via filter_logs)
                Action::None
            }
        }
        // Backspace
        KeyCode::Backspace => {
            logs_state.search_query.pop();
            Action::None
        }
        // Clear search
        KeyCode::Delete if logs_state.search_query.is_empty() => {
            logs_state.is_searching = false;
            Action::None
        }
        // Type character
        KeyCode::Char(c) => {
            logs_state.search_query.push(c);
            Action::None
        }
        _ => Action::None,
    }
}

/// Get available keybinds for display
pub fn keybinds() -> Vec<(&'static str, &'static str)> {
    vec![
        ("/", "Search"),
        ("↑/k", "Scroll up"),
        ("↓/j", "Scroll down"),
        ("←/→", "Switch source"),
        ("G", "Newest"),
        ("g", "Oldest"),
        ("x", "Clear filters"),
        ("r", "Refresh"),
    ]
}
