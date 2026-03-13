//! Demo runner for TUI storybook
//!
//! Provides utilities to run view demos in interactive mode.

use color_eyre::Result;
use ratatui::{
    crossterm::{
        event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
        execute,
        terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    },
    prelude::*,
    widgets::Paragraph,
};
use std::io;
use std::time::Duration;

use alien_cli::tui::state::{Action, AppState, DeploymentDetailState};

/// Run a view demo with the given state and render/handle functions
pub fn run_demo<S, R, H>(
    title: &str,
    mut state: S,
    mut app: AppState,
    render: R,
    handle_key: H,
) -> Result<()>
where
    R: Fn(&mut Frame, Rect, &S, &AppState),
    H: Fn(KeyEvent, &mut S) -> Action,
{
    // Set up terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let tick_rate = Duration::from_millis(100);

    loop {
        // Render
        terminal.draw(|frame| {
            let area = frame.area();

            // Layout: title | content | help
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(1), // Title
                    Constraint::Min(10),   // Content
                    Constraint::Length(1), // Help
                ])
                .split(area);

            // Title bar
            let title_widget = Paragraph::new(format!(" 📖 Storybook: {} ", title))
                .style(Style::default().fg(Color::Rgb(34, 197, 94)).bold());
            frame.render_widget(title_widget, chunks[0]);

            // Main content
            render(frame, chunks[1], &state, &app);

            // Help footer
            let help = Paragraph::new(" q: quit | ↑↓: navigate | Enter: select ")
                .style(Style::default().fg(Color::Rgb(107, 114, 128)));
            frame.render_widget(help, chunks[2]);
        })?;

        // Handle input
        if event::poll(tick_rate)? {
            if let Event::Key(key) = event::read()? {
                // Global quit
                if key.code == KeyCode::Char('q')
                    || (key.code == KeyCode::Char('c')
                        && key.modifiers.contains(KeyModifiers::CONTROL))
                {
                    break;
                }

                // View-specific handling
                let action = handle_key(key, &mut state);
                match action {
                    Action::Quit => break,
                    _ => {}
                }
            }
        }

        // Tick spinner
        app.tick();
    }

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    Ok(())
}

/// Run a simple demo that just renders state (no interaction)
pub fn run_static_demo<S, R>(title: &str, state: &S, render: R) -> Result<()>
where
    R: Fn(&mut Frame, Rect, &S, &AppState),
{
    // Set up terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let app = AppState::new();

    loop {
        terminal.draw(|frame| {
            let area = frame.area();

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(1),
                    Constraint::Min(10),
                    Constraint::Length(1),
                ])
                .split(area);

            let title_widget = Paragraph::new(format!(" 📖 Storybook: {} ", title))
                .style(Style::default().fg(Color::Rgb(34, 197, 94)).bold());
            frame.render_widget(title_widget, chunks[0]);

            render(frame, chunks[1], state, &app);

            let help = Paragraph::new(" Press any key to exit ")
                .style(Style::default().fg(Color::Rgb(107, 114, 128)));
            frame.render_widget(help, chunks[2]);
        })?;

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(_) = event::read()? {
                break;
            }
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    Ok(())
}

/// Run a deployment detail demo
pub fn run_deployment_detail_demo<R, H>(
    title: &str,
    mut state: DeploymentDetailState,
    render: R,
    handle_key: H,
) -> Result<()>
where
    R: Fn(&mut Frame, Rect, &DeploymentDetailState),
    H: Fn(KeyEvent, &mut DeploymentDetailState) -> Action,
{
    // Set up terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let tick_rate = Duration::from_millis(100);

    loop {
        // Render
        terminal.draw(|frame| {
            let area = frame.area();

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(1), // Title
                    Constraint::Min(10),   // Content
                    Constraint::Length(1), // Help
                ])
                .split(area);

            // Title bar
            let title_widget = Paragraph::new(format!(" 📖 Storybook: {} ", title))
                .style(Style::default().fg(Color::Rgb(34, 197, 94)).bold());
            frame.render_widget(title_widget, chunks[0]);

            // Main content
            render(frame, chunks[1], &state);

            // Help footer
            let help = Paragraph::new(" q: quit | ↑↓: scroll resources | ESC: back ")
                .style(Style::default().fg(Color::Rgb(107, 114, 128)));
            frame.render_widget(help, chunks[2]);
        })?;

        // Handle input
        if event::poll(tick_rate)? {
            if let Event::Key(key) = event::read()? {
                // Global quit
                if key.code == KeyCode::Char('q')
                    || (key.code == KeyCode::Char('c')
                        && key.modifiers.contains(KeyModifiers::CONTROL))
                {
                    break;
                }

                // View-specific handling
                let action = handle_key(key, &mut state);
                match action {
                    Action::Quit => break,
                    _ => {}
                }
            }
        }
    }

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    Ok(())
}
