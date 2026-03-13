//! Search overlay demos
//!
//! Note: The SearchState in the current TUI is simple (active: bool, query: String).
//! These demos show a placeholder implementation since search functionality
//! is still being developed.

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
pub enum SearchDemo {
    /// Empty search - no query entered
    Empty,
    /// User typing a query
    Typing,
    /// Search with matching results
    Results,
    /// Search with no matches
    NoResults,
    /// Large result set with scrolling
    LargeSet,
}

impl SearchDemo {
    pub fn run(self) -> Result<()> {
        enable_raw_mode()?;
        let mut stdout = std::io::stdout();
        execute!(stdout, EnterAlternateScreen, Clear(ClearType::All))?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        let (title, description) = match self {
            Self::Empty => ("Empty Search", "No query entered yet"),
            Self::Typing => ("Typing", "User typing: 'api-h'"),
            Self::Results => ("Search Results", "Found 5 matches for 'deployment'"),
            Self::NoResults => ("No Results", "No matches for 'xyz123'"),
            Self::LargeSet => ("Large Result Set", "50+ matches with scrolling"),
        };

        loop {
            terminal.draw(|frame| {
                let area = frame.area();

                let block = Block::default()
                    .title(format!(" Search Demo: {} ", title))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan));

                let inner = block.inner(area);
                frame.render_widget(block, area);

                let text = vec![
                    Line::from(""),
                    Line::from(Span::styled(description, Style::default().fg(Color::White))),
                    Line::from(""),
                    Line::from(Span::styled(
                        "Search functionality placeholder demo.",
                        Style::default().fg(Color::DarkGray),
                    )),
                    Line::from(Span::styled(
                        "The actual search uses SearchState with 'active' and 'query' fields.",
                        Style::default().fg(Color::DarkGray),
                    )),
                    Line::from(""),
                    Line::from(Span::styled(
                        "Press 'q' or Esc to exit",
                        Style::default().fg(Color::Yellow),
                    )),
                ];

                let paragraph = Paragraph::new(text).alignment(Alignment::Center);
                frame.render_widget(paragraph, inner);
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

        Ok(())
    }
}
