//! Dev TUI storybook demos
//!
//! Demos for the multi-deployment TUI dashboard. These demos showcase
//! the new unified TUI framework components.

use clap::Subcommand;
use ratatui::{
    crossterm::{
        event::{self, KeyCode},
        execute,
        terminal::{
            disable_raw_mode, enable_raw_mode, Clear, ClearType, EnterAlternateScreen,
            LeaveAlternateScreen,
        },
    },
    prelude::*,
    widgets::{Block, Borders, Paragraph},
    Terminal,
};
use std::{io::Write, time::Duration};

#[derive(Subcommand)]
pub enum DevDemo {
    /// Deployments list view
    #[command(name = "deployments")]
    Deployments,
    /// Deployment detail view with logs
    #[command(name = "detail")]
    DeploymentDetail,
    /// Deployment groups list
    #[command(name = "deployment-groups")]
    DeploymentGroups,
    /// Commands list
    #[command(name = "commands")]
    Commands,
    /// Releases list
    #[command(name = "releases")]
    Releases,
    /// Packages list
    #[command(name = "packages")]
    Packages,
    /// Search overlay demo
    #[command(name = "search")]
    Search,
    /// Tab navigation demo
    #[command(name = "tabs")]
    Tabs,
}

impl DevDemo {
    pub fn run(self) -> color_eyre::Result<()> {
        match self {
            Self::Deployments => {
                run_placeholder_demo("Deployments View", "Lists all deployments in the project")
            }
            Self::DeploymentDetail => run_placeholder_demo(
                "Deployment Detail View",
                "Shows deployment resources and streaming logs",
            ),
            Self::DeploymentGroups => {
                run_placeholder_demo("Deployment Groups View", "Lists all deployment groups")
            }
            Self::Commands => run_placeholder_demo("Commands View", "Lists all commands"),
            Self::Releases => run_placeholder_demo("Releases View", "Lists all releases"),
            Self::Packages => run_placeholder_demo("Packages View", "Lists all packages"),
            Self::Search => {
                run_placeholder_demo("Search Overlay", "Press / to search, Esc to close")
            }
            Self::Tabs => run_tabs_demo(),
        }
    }
}

/// Run a simple placeholder demo showing the view name
fn run_placeholder_demo(title: &str, description: &str) -> color_eyre::Result<()> {
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen, Clear(ClearType::All))?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let title = title.to_string();
    let description = description.to_string();

    loop {
        terminal.draw(|frame| {
            let area = frame.area();

            let block = Block::default()
                .title(format!(" {} ", title))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan));

            let inner = block.inner(area);
            frame.render_widget(block, area);

            let text = vec![
                Line::from(""),
                Line::from(Span::styled(
                    &description,
                    Style::default().fg(Color::White),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    "This is a placeholder demo.",
                    Style::default().fg(Color::DarkGray),
                )),
                Line::from(Span::styled(
                    "The actual view uses the new TUI framework.",
                    Style::default().fg(Color::DarkGray),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    "Press 'q' or Esc to exit",
                    Style::default().fg(Color::Yellow),
                )),
            ];

            let paragraph = Paragraph::new(text).alignment(Alignment::Center);

            // Center vertically
            let y_offset = inner.height.saturating_sub(7) / 2;
            let centered_area = Rect {
                x: inner.x,
                y: inner.y + y_offset,
                width: inner.width,
                height: 7,
            };

            frame.render_widget(paragraph, centered_area);
        })?;

        if event::poll(Duration::from_millis(100))? {
            if let event::Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => break,
                    _ => {}
                }
            }
        }
    }

    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    disable_raw_mode()?;
    let _ = std::io::stdout().flush();

    Ok(())
}

/// Demo showing tab navigation
fn run_tabs_demo() -> color_eyre::Result<()> {
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen, Clear(ClearType::All))?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let tabs = [
        "Deployments",
        "Deployment Groups",
        "Commands",
        "Releases",
        "Packages",
    ];
    let mut selected = 0usize;

    loop {
        terminal.draw(|frame| {
            let area = frame.area();

            // Title bar
            let title_area = Rect {
                x: area.x,
                y: area.y,
                width: area.width,
                height: 1,
            };
            let title = Paragraph::new(" Alien TUI - Tab Navigation Demo ")
                .style(Style::default().fg(Color::Black).bg(Color::Cyan));
            frame.render_widget(title, title_area);

            // Tab bar
            let tab_area = Rect {
                x: area.x,
                y: area.y + 1,
                width: area.width,
                height: 1,
            };
            let tab_spans: Vec<Span> = tabs
                .iter()
                .enumerate()
                .map(|(i, name)| {
                    if i == selected {
                        Span::styled(
                            format!(" {} ", name),
                            Style::default().fg(Color::Black).bg(Color::White),
                        )
                    } else {
                        Span::styled(format!(" {} ", name), Style::default().fg(Color::Gray))
                    }
                })
                .collect();
            let tab_line = Line::from(tab_spans);
            frame.render_widget(Paragraph::new(tab_line), tab_area);

            // Content area
            let content_area = Rect {
                x: area.x,
                y: area.y + 3,
                width: area.width,
                height: area.height.saturating_sub(5),
            };
            let block = Block::default()
                .title(format!(" {} ", tabs[selected]))
                .borders(Borders::ALL);
            let inner = block.inner(content_area);
            frame.render_widget(block, content_area);

            let content = Paragraph::new(format!("Content for {} tab", tabs[selected]))
                .alignment(Alignment::Center);
            frame.render_widget(content, inner);

            // Footer
            let footer_area = Rect {
                x: area.x,
                y: area.height - 1,
                width: area.width,
                height: 1,
            };
            let footer = Paragraph::new(" <-/->: Switch tabs | q: Quit ")
                .style(Style::default().fg(Color::DarkGray));
            frame.render_widget(footer, footer_area);
        })?;

        if event::poll(Duration::from_millis(100))? {
            if let event::Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => break,
                    KeyCode::Left | KeyCode::Char('h') => {
                        selected = selected.saturating_sub(1);
                    }
                    KeyCode::Right | KeyCode::Char('l') => {
                        selected = (selected + 1).min(tabs.len() - 1);
                    }
                    KeyCode::Tab => {
                        selected = (selected + 1) % tabs.len();
                    }
                    _ => {}
                }
            }
        }
    }

    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    disable_raw_mode()?;
    let _ = std::io::stdout().flush();

    Ok(())
}
