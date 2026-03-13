//! Error dialog demos

use clap::Subcommand;
use color_eyre::Result;
use ratatui::{
    crossterm::{
        event::{self, Event, KeyCode, KeyModifiers},
        execute,
        terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    },
    prelude::*,
    widgets::Paragraph,
};
use std::io;
use std::time::Duration;

use alien_cli::tui::dialogs::ErrorDialog;
use alien_error::{AlienError, GenericError};

#[derive(Subcommand, Debug, Clone)]
pub enum ErrorDialogDemo {
    /// Simple error (no context, no source)
    Simple,
    /// Error with context fields
    WithContext,
    /// Error with source chain
    WithChain,
    /// Complex error (context + chain + retryable)
    Complex,
}

impl ErrorDialogDemo {
    pub fn run(self) -> Result<()> {
        let (title, error) = match self {
            Self::Simple => {
                let error = AlienError::new(GenericError {
                    message: "Failed to connect to database".to_string(),
                });
                ("Error Dialog - Simple", error)
            }

            Self::WithContext => {
                let mut context = serde_json::Map::new();
                context.insert(
                    "resource_id".to_string(),
                    serde_json::Value::String("api-handler".to_string()),
                );
                context.insert(
                    "operation".to_string(),
                    serde_json::Value::String("provision".to_string()),
                );
                context.insert(
                    "timeout_seconds".to_string(),
                    serde_json::Value::Number(30.into()),
                );

                let mut error = AlienError::new(GenericError {
                    message: "Resource provisioning timeout".to_string(),
                });
                error.code = "PROVISION_TIMEOUT".to_string();
                error.context = Some(serde_json::Value::Object(context));
                error.retryable = true;

                ("Error Dialog - With Context", error)
            }

            Self::WithChain => {
                // Build error chain: bottom -> middle -> top
                let bottom_error = AlienError::new(GenericError {
                    message: "Connection refused on port 5432".to_string(),
                });

                let mut middle_error = AlienError::new(GenericError {
                    message: "Failed to connect to PostgreSQL".to_string(),
                });
                middle_error.code = "DATABASE_CONNECTION_FAILED".to_string();
                middle_error.source = Some(Box::new(bottom_error));

                let mut top_error = AlienError::new(GenericError {
                    message: "Failed to provision database resource".to_string(),
                });
                top_error.code = "RESOURCE_PROVISION_FAILED".to_string();
                top_error.source = Some(Box::new(middle_error));

                ("Error Dialog - With Chain", top_error)
            }

            Self::Complex => {
                // Realistic complex error
                let network_error = AlienError::new(GenericError {
                    message: "Name resolution failed: Could not resolve host 'example.com'"
                        .to_string(),
                });

                let mut http_error = AlienError::new(GenericError {
                    message: "HTTP request failed after 3 retries".to_string(),
                });
                http_error.code = "HTTP_REQUEST_FAILED".to_string();
                http_error.source = Some(Box::new(network_error));

                let mut context = serde_json::Map::new();
                context.insert(
                    "function_name".to_string(),
                    serde_json::Value::String("api-gateway".to_string()),
                );
                context.insert(
                    "platform".to_string(),
                    serde_json::Value::String("aws".to_string()),
                );
                context.insert(
                    "region".to_string(),
                    serde_json::Value::String("us-east-1".to_string()),
                );
                context.insert(
                    "retry_count".to_string(),
                    serde_json::Value::Number(3.into()),
                );

                let mut deployment_error = AlienError::new(GenericError {
                    message: "Function deployment failed during provisioning phase".to_string(),
                });
                deployment_error.code = "DEPLOYMENT_FAILED".to_string();
                deployment_error.context = Some(serde_json::Value::Object(context));
                deployment_error.retryable = true;
                deployment_error.http_status_code = Some(500);
                deployment_error.source = Some(Box::new(http_error));

                ("Error Dialog - Complex (Full Example)", deployment_error)
            }
        };

        // Set up terminal
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        let mut dialog = ErrorDialog::new(error);
        let tick_rate = Duration::from_millis(100);

        loop {
            terminal.draw(|frame| {
                let area = frame.area();

                // Background info
                let info_chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(1), // Title
                        Constraint::Min(5),    // Info
                        Constraint::Length(1), // Help
                    ])
                    .split(area);

                // Title bar
                let title_widget = Paragraph::new(format!(" 📖 Storybook: {} ", title))
                    .style(Style::default().fg(Color::Rgb(34, 197, 94)).bold());
                frame.render_widget(title_widget, info_chunks[0]);

                // Info text
                let info = Paragraph::new(vec![
                    Line::from(""),
                    Line::from(vec![Span::styled(
                        "Error Dialog Component",
                        Style::default().fg(Color::Rgb(239, 68, 68)).bold(),
                    )]),
                    Line::from(""),
                    Line::from(vec![Span::styled(
                        "Reusable modal for displaying AlienError details:",
                        Style::default().fg(Color::Rgb(107, 114, 128)),
                    )]),
                    Line::from(vec![Span::styled(
                        "  • Error code and message",
                        Style::default().fg(Color::Rgb(107, 114, 128)),
                    )]),
                    Line::from(vec![Span::styled(
                        "  • Context fields (formatted nicely)",
                        Style::default().fg(Color::Rgb(107, 114, 128)),
                    )]),
                    Line::from(vec![Span::styled(
                        "  • Error chain (source errors)",
                        Style::default().fg(Color::Rgb(107, 114, 128)),
                    )]),
                    Line::from(vec![Span::styled(
                        "  • Retry hint (if retryable)",
                        Style::default().fg(Color::Rgb(107, 114, 128)),
                    )]),
                    Line::from(""),
                    Line::from(vec![Span::styled(
                        "Can be triggered from anywhere:",
                        Style::default().fg(Color::Rgb(107, 114, 128)),
                    )]),
                    Line::from(vec![Span::styled(
                        "  • Deployment failed? Press E",
                        Style::default().fg(Color::Rgb(107, 114, 128)),
                    )]),
                    Line::from(vec![Span::styled(
                        "  • Resource failed? Press E",
                        Style::default().fg(Color::Rgb(107, 114, 128)),
                    )]),
                    Line::from(vec![Span::styled(
                        "  • API error? Show dialog",
                        Style::default().fg(Color::Rgb(107, 114, 128)),
                    )]),
                ]);
                frame.render_widget(info, info_chunks[1]);

                // Help footer
                let help = Paragraph::new(" q: quit | ↑↓: scroll | ESC: close dialog ")
                    .style(Style::default().fg(Color::Rgb(107, 114, 128)));
                frame.render_widget(help, info_chunks[2]);

                // Render error dialog on top
                dialog.render(frame, area);
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

                    // Dialog handling
                    if dialog.handle_key(key) {
                        // Dialog closed (ESC)
                        break;
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
}
