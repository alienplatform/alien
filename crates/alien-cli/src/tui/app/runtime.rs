//! App runtime - event loop and terminal management

use super::config::AppConfig;
use super::controller::AppController;
use crate::tui::framework::{header::HEADER_HEIGHT, Header, KeybindsFooter, SearchOverlay};
use crate::tui::services::AppServices;
use crate::tui::state::{Action, InputMode, ViewId};
use crate::tui::views::{
    commands_view, deployment_detail_view, deployment_groups_view, deployments_list, logs_view,
    packages_view, releases_view,
};
use ratatui::{
    crossterm::{
        event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
        execute,
        terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    },
    prelude::*,
};
use std::io;
use std::time::{Duration, Instant};

/// How often to poll for data updates
const POLL_INTERVAL: Duration = Duration::from_secs(2);

/// Run the TUI application
///
/// # Arguments
/// * `config` - App configuration (dev or platform mode)
///
/// # Terminal Initialization
/// This function initializes the terminal BEFORE any other operations.
/// In dev mode, it signals `terminal_ready_tx` after raw mode is enabled,
/// allowing the build task to start safely without corrupting terminal state.
pub async fn run_app(mut config: AppConfig) -> io::Result<()> {
    // Step 1: Initialize terminal FIRST - this must happen before any background tasks start
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    // Step 2: Signal that terminal is ready (build task can now safely start)
    if let Some(terminal_ready_tx) = config.terminal_ready_tx.take() {
        terminal_ready_tx.send(()).ok();
    }

    // Step 3: Continue with normal TUI initialization
    let mut build_status_rx = config.build_status_rx.take();
    let services = AppServices::new(config.sdk.clone(), config.project_id.clone());
    let mut controller = AppController::new(config, services);

    controller.initialize().await;
    controller.load_current_view().await;

    let result = run_event_loop(&mut terminal, &mut controller, &mut build_status_rx).await;

    // Cleanup
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

async fn run_event_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    controller: &mut AppController,
    build_status_rx: &mut Option<tokio::sync::mpsc::Receiver<crate::tui::state::BuildState>>,
) -> io::Result<()> {
    let tick_rate = Duration::from_millis(100);
    let mut last_poll = Instant::now();

    loop {
        // Poll build status (dev mode only)
        if let Some(ref mut rx) = build_status_rx {
            while let Ok(build_state) = rx.try_recv() {
                controller.state.build_state = Some(build_state);
            }
        }

        // Render
        terminal.draw(|frame| render(frame, &mut *controller))?;

        // Logs now flow via OTLP -> dev server -> LogBuffer -> deepstore-client
        // No more channel-based log handling needed

        // Periodic data refresh
        if last_poll.elapsed() >= POLL_INTERVAL {
            controller.refresh_current_view().await;
            last_poll = Instant::now();
        }

        // Handle input
        if event::poll(tick_rate)? {
            let ev = event::read()?;
            if let Event::Key(key) = ev {
                let action = handle_key(key, controller);
                if controller.handle_action(action).await {
                    return Ok(());
                }
            }
        }

        // Tick spinner
        controller.state.tick();
    }
}

fn render(frame: &mut Frame, controller: &mut AppController) {
    // Calculate has_error before borrowing state mutably
    let has_error = controller
        .state
        .deployment_detail
        .as_ref()
        .and_then(|d| d.metadata.as_ref())
        .and_then(|m| m.error.as_ref())
        .is_some();

    let state = &mut controller.state;
    let area = frame.area();

    // Check if we're in initializing state - if so, show full-screen overlay
    if let Some(ref build_state) = state.build_state {
        if build_state.is_initializing() {
            render_initializing_overlay(frame, area, state.app.spinner_frame);
            return;
        }
    }

    // Layout: header | content | keybinds
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(HEADER_HEIGHT), // Header (consistent height)
            Constraint::Min(10),               // Content
            Constraint::Length(1),             // Keybinds
        ])
        .split(area);

    // Render header based on current view
    if !matches!(state.current_view, ViewId::DeploymentDetail(_)) {
        // List view - show tabs and build status (dev mode)
        let tabs = state.available_tabs();
        Header::render_list_view(
            frame,
            chunks[0],
            &state.connection,
            &tabs,
            &state.current_view,
            state.build_state.as_ref(),
            state.app.spinner_frame,
        );
    } else {
        // Detail view - show back button, deployment info, and build status (dev mode)
        if let Some(ref detail) = state.deployment_detail {
            Header::render_detail_view(
                frame,
                chunks[0],
                &state.connection,
                detail,
                state.build_state.as_ref(),
                state.app.spinner_frame,
            );
        }
    }

    // Render content based on current view
    match &state.current_view {
        ViewId::Deployments => {
            deployments_list::render(frame, chunks[1], &state.deployments, &state.app);
        }
        ViewId::DeploymentDetail(_) => {
            if let Some(ref detail) = state.deployment_detail {
                deployment_detail_view::render(frame, chunks[1], detail);
            }
        }
        ViewId::DeploymentGroups => {
            deployment_groups_view::render(frame, chunks[1], &state.deployment_groups, &state.app);
        }
        ViewId::Commands => {
            let filter_display = state.get_commands_filter_display();
            let filtered_commands = state.get_filtered_commands();
            commands_view::render(
                frame,
                chunks[1],
                &state.commands,
                &state.app,
                filter_display.as_deref(),
                filtered_commands,
            );
        }
        ViewId::Releases => {
            releases_view::render(frame, chunks[1], &state.releases, &state.app);
        }
        ViewId::Packages => {
            packages_view::render(frame, chunks[1], &state.packages, &state.app);
        }
        ViewId::Logs => {
            logs_view::render(
                frame,
                chunks[1],
                &state.logs_view,
                &state.logs,
                &state.app,
                state.mode,
                &state.deployment_name_cache,
            );
        }
    }

    // Render keybinds
    let mode = controller.config.mode;
    let keybinds = get_keybinds(&state.current_view, mode, has_error);
    KeybindsFooter::render(frame, chunks[2], &keybinds);

    // Render search overlay if active
    if state.app.search.is_input_active() {
        SearchOverlay::render(frame, area, &state.app.search);
    }

    // Render new deployment dialog if open
    if let Some(ref mut dialog) = state.new_deployment_dialog {
        dialog.render(frame, area);
    }

    // Render error dialog if open
    if let Some(ref dialog) = state.error_dialog {
        dialog.render(frame, area);
    }
}

fn get_keybinds(
    view: &ViewId,
    mode: super::config::AppMode,
    has_error: bool,
) -> Vec<(&'static str, &'static str)> {
    match view {
        ViewId::Deployments => deployments_list::keybinds(),
        ViewId::DeploymentDetail(_) => deployment_detail_view::keybinds(mode, has_error),
        ViewId::DeploymentGroups => deployment_groups_view::keybinds(),
        ViewId::Commands => commands_view::keybinds(),
        ViewId::Releases => releases_view::keybinds(),
        ViewId::Packages => packages_view::keybinds(),
        ViewId::Logs => logs_view::keybinds(),
    }
}

fn handle_key(key: KeyEvent, controller: &mut AppController) -> Action {
    let state = &mut controller.state;

    // If initializing (first build), only allow Ctrl+C to quit
    if let Some(ref build_state) = state.build_state {
        if build_state.is_initializing() {
            // Only allow Ctrl+C during initialization
            if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                return Action::Quit;
            }
            // Ignore all other keys during initialization
            return Action::None;
        }
    }

    // Global shortcuts
    match key.code {
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            return Action::Quit;
        }
        KeyCode::Char('q') if state.app.input_mode == InputMode::Normal => {
            return Action::Quit;
        }
        // Global rebuild in dev mode (works everywhere)
        KeyCode::Char('b') | KeyCode::Char('B')
            if state.app.input_mode == InputMode::Normal
                && controller.config.mode == super::config::AppMode::Dev =>
        {
            return Action::TriggerRebuild;
        }
        // ESC to navigate back from detail views
        KeyCode::Esc
            if state.app.input_mode == InputMode::Normal
                && matches!(state.current_view, ViewId::DeploymentDetail(_)) =>
        {
            return Action::NavigateBack;
        }
        _ => {}
    }

    // Search mode handling
    if state.app.search.is_input_active() {
        match key.code {
            KeyCode::Esc => {
                state.app.search.deactivate();
                return Action::None;
            }
            KeyCode::Enter => {
                state.app.search.deactivate();
                return Action::None;
            }
            KeyCode::Backspace => {
                state.app.search.backspace();
                return Action::None;
            }
            KeyCode::Char(c) => {
                state.app.search.input(c);
                return Action::None;
            }
            _ => return Action::None,
        }
    }

    // Dialog mode - route keys to dialogs
    if state.app.input_mode == InputMode::Dialog {
        // Error dialog
        if let Some(ref mut dialog) = state.error_dialog {
            if dialog.handle_key(key) {
                // Dialog wants to close
                state.close_error_dialog();
            }
            return Action::None;
        }

        // New deployment dialog
        if let Some(ref mut dialog) = state.new_deployment_dialog {
            dialog.handle_key(key);

            // Check if dialog completed
            if let Some(result) = dialog.result() {
                match result {
                    Ok(deploy_result) => {
                        // Close dialog and create deployment
                        state.close_new_deployment_dialog();
                        return Action::CreateDeployment {
                            platform: deploy_result.platform.to_api_string().to_string(),
                            name: deploy_result.name,
                            deployment_group_id: deploy_result.deployment_group_id,
                        };
                    }
                    Err(()) => {
                        // Dialog cancelled
                        state.close_new_deployment_dialog();
                        return Action::None;
                    }
                }
            }
            return Action::None;
        }

        // No dialog but in dialog mode - this is a bug, force reset
        state.app.input_mode = InputMode::Normal;
        return Action::None;
    }

    // Defensive: If a dialog is open but mode isn't Dialog, close it to prevent state corruption
    // This shouldn't happen in normal flow, but catches bugs where dialogs aren't properly closed
    if state.new_deployment_dialog.is_some() && state.app.input_mode != InputMode::Dialog {
        state.close_new_deployment_dialog();
    }
    if state.error_dialog.is_some() && state.app.input_mode != InputMode::Dialog {
        state.close_error_dialog();
    }

    // Normal mode - activate search
    if key.code == KeyCode::Char('/') && state.app.input_mode == InputMode::Normal {
        state.app.search.activate();
        return Action::None;
    }

    // Clear search with Escape
    if key.code == KeyCode::Esc && state.app.search.is_active() {
        state.app.search.clear();
        return Action::None;
    }

    // Tab navigation (only in non-detail views)
    if state.app.input_mode == InputMode::Normal
        && !matches!(state.current_view, ViewId::DeploymentDetail(_))
    {
        let tabs = state.available_tabs();
        let current_idx = tabs.iter().position(|t| *t == state.current_view);

        match key.code {
            KeyCode::Tab => {
                if let Some(idx) = current_idx {
                    let next_idx = (idx + 1) % tabs.len();
                    return Action::NavigateToView(tabs[next_idx].clone());
                }
            }
            KeyCode::BackTab => {
                if let Some(idx) = current_idx {
                    let prev_idx = if idx == 0 { tabs.len() - 1 } else { idx - 1 };
                    return Action::NavigateToView(tabs[prev_idx].clone());
                }
            }
            // Number shortcuts - only navigate to tabs that are available
            KeyCode::Char(c @ '1'..='9') => {
                let idx = c.to_digit(10).unwrap_or(0) as usize - 1;
                if idx < tabs.len() {
                    return Action::NavigateToView(tabs[idx].clone());
                }
            }
            _ => {}
        }
    }

    // View-specific handling
    match &state.current_view {
        ViewId::Deployments => deployments_list::handle_key(key, &mut state.deployments),
        ViewId::DeploymentDetail(_) => {
            let mode = controller.config.mode;
            if let Some(ref mut detail) = state.deployment_detail {
                deployment_detail_view::handle_key(key, detail, mode)
            } else {
                Action::None
            }
        }
        ViewId::DeploymentGroups => {
            deployment_groups_view::handle_key(key, &mut state.deployment_groups)
        }
        ViewId::Commands => commands_view::handle_key(key, &mut state.commands),
        ViewId::Releases => releases_view::handle_key(key, &mut state.releases),
        ViewId::Packages => packages_view::handle_key(key, &mut state.packages),
        ViewId::Logs => {
            let total_logs = state.logs.len();
            let mode = state.mode;
            logs_view::handle_key(key, &mut state.logs_view, total_logs, mode)
        }
    }
}

/// Render initializing overlay - blocks all interaction except Ctrl+C
fn render_initializing_overlay(frame: &mut Frame, area: Rect, spinner_frame: usize) {
    use ratatui::text::{Line, Span};
    use ratatui::widgets::{Block, Borders, Clear, Paragraph};

    // Clear the entire screen
    frame.render_widget(Clear, area);

    // Center a message box
    let overlay_width = 60;
    let overlay_height = 7;
    let overlay_area = Rect {
        x: (area.width.saturating_sub(overlay_width)) / 2,
        y: (area.height.saturating_sub(overlay_height)) / 2,
        width: overlay_width,
        height: overlay_height,
    };

    // Render the message
    let spinner = crate::tui::common::SPINNER_FRAMES
        [spinner_frame % crate::tui::common::SPINNER_FRAMES.len()];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Rgb(34, 197, 94))); // ALIEN_GREEN

    let lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled(
                format!("  {}  ", spinner),
                Style::default().fg(Color::Rgb(245, 158, 11)).bold(), // AMBER
            ),
            Span::styled(
                "Initializing Alien...",
                Style::default().fg(Color::Rgb(229, 231, 235)).bold(),
            ),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "  Building your deployment for the first time",
            Style::default().fg(Color::Rgb(156, 163, 175)),
        )),
        Line::from(Span::styled(
            "  Press Ctrl+C to cancel",
            Style::default().fg(Color::Rgb(107, 114, 128)),
        )),
    ];

    let paragraph = Paragraph::new(lines)
        .block(block)
        .alignment(Alignment::Left);

    frame.render_widget(paragraph, overlay_area);
}
