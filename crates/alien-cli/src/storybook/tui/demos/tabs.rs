//! Tabs navigation demos
//!
//! Demonstrates the TabBar widget which uses ViewId for navigation.

use alien_cli::tui::{framework::tabs::TabBar, state::ViewId};
use clap::Subcommand;
use color_eyre::Result;
use crossterm::{
    event::{self, KeyCode},
    execute,
    terminal::{
        disable_raw_mode, enable_raw_mode, Clear, ClearType, EnterAlternateScreen,
        LeaveAlternateScreen,
    },
};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph},
};
use std::time::Duration;

#[derive(Subcommand, Debug, Clone, Copy)]
pub enum TabsDemo {
    /// Few tabs (3-4) with basic navigation
    FewTabs,
    /// Many tabs (8+)
    ManyTabs,
    /// All available views
    AllViews,
}

impl TabsDemo {
    pub fn run(self) -> Result<()> {
        enable_raw_mode()?;
        let mut stdout = std::io::stdout();
        execute!(stdout, EnterAlternateScreen, Clear(ClearType::All))?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        let (tabs, mut current_idx) = match self {
            Self::FewTabs => (create_few_tabs(), 0),
            Self::ManyTabs => (create_many_tabs(), 0),
            Self::AllViews => (create_all_views(), 0),
        };

        loop {
            terminal.draw(|frame| {
                let area = frame.area();

                // Title
                let title_area = Rect {
                    x: area.x,
                    y: area.y,
                    width: area.width,
                    height: 1,
                };
                let title = Paragraph::new(" Alien TUI - Tabs Demo ")
                    .style(Style::default().fg(Color::Black).bg(Color::Cyan));
                frame.render_widget(title, title_area);

                // Tabs using TabBar widget
                let tabs_area = Rect {
                    x: area.x,
                    y: area.y + 1,
                    width: area.width,
                    height: 1,
                };
                TabBar::render(frame, tabs_area, &tabs, tabs[current_idx].clone());

                // Content
                let content_area = Rect {
                    x: area.x,
                    y: area.y + 3,
                    width: area.width,
                    height: area.height.saturating_sub(5),
                };
                let block = Block::default()
                    .title(format!(" {} ", tabs[current_idx].title()))
                    .borders(Borders::ALL);
                let inner = block.inner(content_area);
                frame.render_widget(block, content_area);

                let content =
                    Paragraph::new(format!("Content for {} view", tabs[current_idx].title()))
                        .alignment(Alignment::Center);
                frame.render_widget(content, inner);

                // Footer
                let footer_area = Rect {
                    x: area.x,
                    y: area.height.saturating_sub(1),
                    width: area.width,
                    height: 1,
                };
                let footer = Paragraph::new(" <-/->: Navigate | 1-9: Jump to tab | q: Quit ")
                    .style(Style::default().fg(Color::DarkGray));
                frame.render_widget(footer, footer_area);
            })?;

            if event::poll(Duration::from_millis(100))? {
                if let event::Event::Key(key) = event::read()? {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => break,
                        KeyCode::Left | KeyCode::Char('h') => {
                            if current_idx > 0 {
                                current_idx -= 1;
                            }
                        }
                        KeyCode::Right | KeyCode::Char('l') => {
                            if current_idx < tabs.len() - 1 {
                                current_idx += 1;
                            }
                        }
                        KeyCode::Tab => {
                            current_idx = (current_idx + 1) % tabs.len();
                        }
                        KeyCode::Char(c) if c.is_ascii_digit() => {
                            let idx = c.to_digit(10).unwrap() as usize;
                            if idx > 0 && idx <= tabs.len() {
                                current_idx = idx - 1;
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
        disable_raw_mode()?;

        Ok(())
    }
}

fn create_few_tabs() -> Vec<ViewId> {
    vec![
        ViewId::Deployments,
        ViewId::DeploymentGroups,
        ViewId::Commands,
        ViewId::Releases,
    ]
}

fn create_many_tabs() -> Vec<ViewId> {
    vec![
        ViewId::Deployments,
        ViewId::DeploymentGroups,
        ViewId::Commands,
        ViewId::Releases,
        ViewId::Packages,
        ViewId::Logs,
        ViewId::DeploymentDetail("dpl_demo123".to_string()),
    ]
}

fn create_all_views() -> Vec<ViewId> {
    vec![
        ViewId::Deployments,
        ViewId::DeploymentGroups,
        ViewId::Commands,
        ViewId::Releases,
        ViewId::Packages,
        ViewId::Logs,
    ]
}
