//! Header widget demos

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

use alien_cli::tui::framework::{header::HEADER_HEIGHT, Header};
use alien_cli::tui::state::{
    BuildState, ConnectionInfo, DeploymentDetailState, DeploymentStatus, ViewId,
};

#[derive(Subcommand, Debug, Clone)]
pub enum HeaderDemo {
    /// List view with dev connection (localhost)
    #[command(name = "list-dev")]
    ListDev,
    /// List view with platform connection
    #[command(name = "list-platform")]
    ListPlatform,
    /// Detail view with dev connection
    #[command(name = "detail-dev")]
    DetailDev,
    /// Detail view with platform connection
    #[command(name = "detail-platform")]
    DetailPlatform,
    /// Detail view with building status
    #[command(name = "detail-building")]
    DetailBuilding,
    /// Detail view with failed status
    #[command(name = "detail-failed")]
    DetailFailed,
}

impl HeaderDemo {
    pub fn run(self) -> Result<()> {
        // Set up terminal
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        let tick_rate = Duration::from_millis(100);
        let mut spinner_frame: usize = 0;

        loop {
            terminal.draw(|frame| {
                let area = frame.area();

                // Show header at top of screen
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(HEADER_HEIGHT), // Header (consistent height)
                        Constraint::Min(5),                 // Info
                        Constraint::Length(1),              // Help
                    ])
                    .split(area);

                match &self {
                    HeaderDemo::ListDev => {
                        let connection = ConnectionInfo::dev_with_url("http://localhost:9090".to_string());
                        let tabs = vec![ViewId::Deployments, ViewId::DeploymentGroups, ViewId::Commands, ViewId::Logs];
                        let build_state = Some(BuildState::Built {
                            duration: Duration::from_secs_f64(8.4)
                        });
                        Header::render_list_view(frame, chunks[0], &connection, &tabs, &ViewId::Deployments, build_state.as_ref(), spinner_frame);
                        render_info(frame, chunks[1], "List view (dev) - 2 lines: branding + URL + build status, tabs");
                    }
                    HeaderDemo::ListPlatform => {
                        let connection = ConnectionInfo::platform_with_url("https://api.alien.dev".to_string());
                        let tabs = vec![
                            ViewId::Deployments,
                            ViewId::DeploymentGroups,
                            ViewId::Commands,
                            ViewId::Releases,
                            ViewId::Packages,
                            ViewId::Logs,
                        ];
                        Header::render_list_view(frame, chunks[0], &connection, &tabs, &ViewId::Deployments, None, spinner_frame);
                        render_info(frame, chunks[1], "List view (platform) - 2 lines: branding + URL, tabs");
                    }
                    HeaderDemo::DetailDev => {
                        let connection = ConnectionInfo::dev_with_url("http://localhost:9090".to_string());
                        let detail = DeploymentDetailState::new("dpl_123".to_string(), "my-deployment".to_string(), DeploymentStatus::Running);
                        let build_state = Some(BuildState::Built {
                            duration: Duration::from_secs_f64(12.3)
                        });
                        Header::render_detail_view(frame, chunks[0], &connection, &detail, build_state.as_ref(), spinner_frame);
                        render_info(frame, chunks[1], "Detail view (dev) - 2 lines: branding + URL + build status, breadcrumb");
                    }
                    HeaderDemo::DetailPlatform => {
                        let connection = ConnectionInfo::platform_with_url("https://api.alien.dev".to_string());
                        let detail = DeploymentDetailState::new("dpl_456".to_string(), "prod-worker".to_string(), DeploymentStatus::Running);
                        Header::render_detail_view(frame, chunks[0], &connection, &detail, None, spinner_frame);
                        render_info(frame, chunks[1], "Detail view (platform) - 2 lines: branding + URL, breadcrumb");
                    }
                    HeaderDemo::DetailBuilding => {
                        let connection = ConnectionInfo::dev_with_url("http://localhost:9090".to_string());
                        let detail = DeploymentDetailState::new("dpl_789".to_string(), "building-deployment".to_string(), DeploymentStatus::Provisioning);
                        let build_state = Some(BuildState::Building);
                        Header::render_detail_view(frame, chunks[0], &connection, &detail, build_state.as_ref(), spinner_frame);
                        render_info(frame, chunks[1], "Detail view (building) - 2 lines with animated spinner");
                    }
                    HeaderDemo::DetailFailed => {
                        let connection = ConnectionInfo::dev_with_url("http://localhost:9090".to_string());
                        let detail = DeploymentDetailState::new("dpl_fail".to_string(), "failed-deployment".to_string(), DeploymentStatus::ProvisioningFailed);
                        let build_state = Some(BuildState::Failed {
                            error: "Connection refused".to_string(),
                        });
                        Header::render_detail_view(frame, chunks[0], &connection, &detail, build_state.as_ref(), spinner_frame);
                        render_info(frame, chunks[1], "Detail view (failed) - 2 lines with error status");
                    }
                }

                // Help footer
                let help = Paragraph::new(" q: quit ")
                    .style(Style::default().fg(Color::Rgb(107, 114, 128)));
                frame.render_widget(help, chunks[2]);
            })?;

            // Handle input
            if event::poll(tick_rate)? {
                if let Event::Key(key) = event::read()? {
                    if key.code == KeyCode::Char('q')
                        || (key.code == KeyCode::Char('c')
                            && key.modifiers.contains(KeyModifiers::CONTROL))
                    {
                        break;
                    }
                }
            }

            spinner_frame = spinner_frame.wrapping_add(1);
        }

        // Restore terminal
        disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
        terminal.show_cursor()?;

        Ok(())
    }
}

fn render_info(frame: &mut Frame, area: Rect, description: &str) {
    let text = Paragraph::new(vec![
        Line::from(""),
        Line::from(vec![
            Span::styled(
                "Demo: ",
                Style::default().fg(Color::Rgb(34, 197, 94)).bold(),
            ),
            Span::styled(description, Style::default().fg(Color::Rgb(229, 231, 235))),
        ]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "Multi-line header design:",
            Style::default().fg(Color::Rgb(107, 114, 128)),
        )]),
        Line::from(vec![
            Span::styled("  Line 1: ", Style::default().fg(Color::Rgb(107, 114, 128))),
            Span::styled("ALIEN", Style::default().fg(Color::Rgb(34, 197, 94)).bold()),
            Span::styled(
                " logo + Connection info (LOCAL/PROD)",
                Style::default().fg(Color::Rgb(107, 114, 128)),
            ),
        ]),
        Line::from(vec![
            Span::styled("  Line 2: ", Style::default().fg(Color::Rgb(107, 114, 128))),
            Span::styled(
                "Navigation tabs or breadcrumb (< back)",
                Style::default().fg(Color::Rgb(107, 114, 128)),
            ),
        ]),
        Line::from(vec![
            Span::styled("  Line 3: ", Style::default().fg(Color::Rgb(107, 114, 128))),
            Span::styled(
                "Build status (dev mode only)",
                Style::default().fg(Color::Rgb(107, 114, 128)),
            ),
        ]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "Clean, spacious, professional.",
            Style::default().fg(Color::Rgb(107, 114, 128)).italic(),
        )]),
    ]);
    frame.render_widget(text, area);
}
