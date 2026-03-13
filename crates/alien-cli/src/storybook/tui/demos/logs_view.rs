//! Logs view demos

use alien_cli::tui::app::config::AppMode;
use alien_cli::tui::state::managers::LogsConnectionStatus;
use alien_cli::tui::state::{AppState, LogFilter, LogLevel, LogLine, LogsViewState};
use chrono::Utc;
use clap::Subcommand;
use color_eyre::Result;
use std::collections::VecDeque;

#[derive(Subcommand, Debug, Clone, Copy)]
pub enum LogsViewDemo {
    /// No logs - fresh deployment
    NoLogs,
    /// Few logs (10-20) with mixed levels
    FewLogs,
    /// Many logs (200+) with scrolling
    ManyLogs,
    /// Simulated real-time streaming
    Streaming,
    /// Logs with highlighted errors
    WithErrors,
    /// Filtered logs by level
    Filtered,
}

impl LogsViewDemo {
    pub fn run(self) -> Result<()> {
        use ratatui::{
            crossterm::{
                event::{self, Event, KeyCode},
                execute,
                terminal::{
                    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
                },
            },
            prelude::*,
            widgets::Paragraph,
        };
        use std::io;
        use std::time::Duration;

        let logs = match self {
            Self::NoLogs => create_no_logs(),
            Self::FewLogs => create_few_logs(),
            Self::ManyLogs => create_many_logs(),
            Self::Streaming => create_streaming_logs(),
            Self::WithErrors => create_logs_with_errors(),
            Self::Filtered => create_filtered_logs(),
        };

        let logs_state = LogsViewState {
            filter: LogFilter::default(),
            connection_status: LogsConnectionStatus::Connected,
            is_searching: false,
            search_query: String::new(),
            scroll_offset: 0,
            auto_scroll: true,
            managers: vec![],
            selected_manager_idx: None,
            initializing: false,
        };

        let app_state = AppState::default();
        let mode = AppMode::Dev;
        let title = match self {
            Self::NoLogs => "Logs View - No Logs",
            Self::FewLogs => "Logs View - Few Logs",
            Self::ManyLogs => "Logs View - Many Logs (200+)",
            Self::Streaming => "Logs View - Streaming",
            Self::WithErrors => "Logs View - With Errors",
            Self::Filtered => "Logs View - Filtered",
        };

        // Set up terminal
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

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

                let title_widget = Paragraph::new(format!(" Storybook: {} ", title))
                    .style(Style::default().fg(Color::Rgb(34, 197, 94)).bold());
                frame.render_widget(title_widget, chunks[0]);

                // Create empty deployment name cache for demo
                let deployment_name_cache = std::collections::HashMap::new();
                alien_cli::tui::views::logs_view::render(
                    frame,
                    chunks[1],
                    &logs_state,
                    &logs,
                    &app_state,
                    mode,
                    &deployment_name_cache,
                );

                let help = Paragraph::new(" Press 'q' to exit ")
                    .style(Style::default().fg(Color::Rgb(107, 114, 128)));
                frame.render_widget(help, chunks[2]);
            })?;

            if event::poll(Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
                    if key.code == KeyCode::Char('q') || key.code == KeyCode::Esc {
                        break;
                    }
                }
            }
        }

        disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
        terminal.show_cursor()?;

        Ok(())
    }
}

fn create_no_logs() -> VecDeque<LogLine> {
    VecDeque::new()
}

fn create_few_logs() -> VecDeque<LogLine> {
    let mut logs = VecDeque::new();
    let messages = vec![
        ("[INFO] Starting function server...", LogLevel::Info),
        ("[DEBUG] Loading configuration", LogLevel::Debug),
        ("[INFO] Connecting to database", LogLevel::Info),
        ("[INFO] Database connection established", LogLevel::Info),
        ("[INFO] Server listening on port 8080", LogLevel::Info),
        ("[DEBUG] Received request GET /health", LogLevel::Debug),
        ("[DEBUG] Health check passed", LogLevel::Debug),
        ("[INFO] Processing user registration", LogLevel::Info),
        (
            "[WARN] Validation warning: email already exists",
            LogLevel::Warn,
        ),
        ("[INFO] User created successfully", LogLevel::Info),
    ];

    let messages_len = messages.len();
    for (i, (msg, level)) in messages.into_iter().enumerate() {
        let timestamp = Utc::now() - chrono::Duration::seconds((messages_len - i) as i64);
        logs.push_back(LogLine {
            deployment_id: "dpl_test123".to_string(),
            deployment_name: None,
            deployment_group_id: None,
            deployment_group_name: None,
            resource_id: "api-handler".to_string(),
            content: msg.to_string(),
            timestamp,
            level,
        });
    }

    logs
}

fn create_many_logs() -> VecDeque<LogLine> {
    let mut logs = VecDeque::new();

    let patterns = vec![
        ("[DEBUG] Received request GET /api/users", LogLevel::Debug),
        ("[DEBUG] Query executed in 45ms", LogLevel::Debug),
        ("[INFO] Response sent: 200 OK", LogLevel::Info),
        ("[DEBUG] Cache hit for key: user:123", LogLevel::Debug),
        ("[INFO] Background job started", LogLevel::Info),
        ("[INFO] Processing batch of 100 items", LogLevel::Info),
        ("[DEBUG] Item processed successfully", LogLevel::Debug),
        ("[INFO] Checkpoint reached", LogLevel::Info),
    ];

    for i in 0..200 {
        let (msg, level) = &patterns[i % patterns.len()];
        let timestamp = Utc::now() - chrono::Duration::seconds((200 - i) as i64);
        logs.push_back(LogLine {
            deployment_id: "dpl_test123".to_string(),
            deployment_name: None,
            deployment_group_id: None,
            deployment_group_name: None,
            resource_id: if i % 3 == 0 {
                "api-handler".to_string()
            } else {
                "worker".to_string()
            },
            content: format!("{} (#{})", msg, i),
            timestamp,
            level: *level,
        });
    }

    logs
}

fn create_streaming_logs() -> VecDeque<LogLine> {
    let mut logs = VecDeque::new();

    // Simulate a streaming scenario with timestamps very close together
    for i in 0..50 {
        let timestamp = Utc::now() - chrono::Duration::milliseconds((50 - i) * 100);
        logs.push_back(LogLine {
            deployment_id: "dpl_test123".to_string(),
            deployment_name: None,
            deployment_group_id: None,
            deployment_group_name: None,
            resource_id: "stream-processor".to_string(),
            content: format!(
                "[{}] Processing stream event {}",
                if i % 10 == 0 { "INFO" } else { "DEBUG" },
                i
            ),
            timestamp,
            level: if i % 10 == 0 {
                LogLevel::Info
            } else {
                LogLevel::Debug
            },
        });
    }

    logs
}

fn create_logs_with_errors() -> VecDeque<LogLine> {
    let mut logs = VecDeque::new();

    let sequence = vec![
        ("[INFO] Application started", LogLevel::Info),
        ("[INFO] Connecting to database", LogLevel::Info),
        ("[INFO] Database connected", LogLevel::Info),
        ("[INFO] Starting HTTP server", LogLevel::Info),
        ("[INFO] Server listening on :8080", LogLevel::Info),
        ("[INFO] Received request: POST /api/process", LogLevel::Info),
        ("[DEBUG] Validating request payload", LogLevel::Debug),
        ("[DEBUG] Validation passed", LogLevel::Debug),
        ("[INFO] Starting background job", LogLevel::Info),
        (
            "[ERROR] Connection timeout to external API",
            LogLevel::Error,
        ),
        ("[WARN] Retrying request (attempt 1/3)", LogLevel::Warn),
        (
            "[ERROR] Connection timeout to external API",
            LogLevel::Error,
        ),
        ("[WARN] Retrying request (attempt 2/3)", LogLevel::Warn),
        (
            "[ERROR] Connection timeout to external API",
            LogLevel::Error,
        ),
        ("[ERROR] Max retries exceeded", LogLevel::Error),
        (
            "[ERROR] Job failed with error: ExternalAPITimeout",
            LogLevel::Error,
        ),
        ("[WARN] Sending error response to client", LogLevel::Warn),
        ("[WARN] Request completed with status 503", LogLevel::Warn),
        ("[INFO] Next request: GET /api/status", LogLevel::Info),
        ("[INFO] Status check: operational", LogLevel::Info),
    ];

    let sequence_len = sequence.len();
    for (i, (msg, level)) in sequence.into_iter().enumerate() {
        let timestamp = Utc::now() - chrono::Duration::seconds((sequence_len - i) as i64);
        logs.push_back(LogLine {
            deployment_id: "dpl_test123".to_string(),
            deployment_name: None,
            deployment_group_id: None,
            deployment_group_name: None,
            resource_id: "api-handler".to_string(),
            content: msg.to_string(),
            timestamp,
            level,
        });
    }

    logs
}

fn create_filtered_logs() -> VecDeque<LogLine> {
    let mut logs = VecDeque::new();

    // Only error and warn logs (as if filtered)
    let filtered_sequence = vec![
        ("[WARN] Database query slow (2.5s)", LogLevel::Warn),
        ("[WARN] Rate limit approaching (90%)", LogLevel::Warn),
        ("[ERROR] Failed to connect to cache", LogLevel::Error),
        ("[WARN] Cache unavailable, using fallback", LogLevel::Warn),
        ("[WARN] Authentication failed for user", LogLevel::Warn),
        ("[ERROR] Invalid API key provided", LogLevel::Error),
        ("[ERROR] Resource quota exceeded", LogLevel::Error),
        ("[WARN] Memory usage high (85%)", LogLevel::Warn),
    ];

    let filtered_len = filtered_sequence.len();
    for (i, (msg, level)) in filtered_sequence.into_iter().enumerate() {
        let timestamp = Utc::now() - chrono::Duration::seconds((filtered_len - i) as i64 * 5);
        logs.push_back(LogLine {
            deployment_id: "dpl_test123".to_string(),
            deployment_name: None,
            deployment_group_id: None,
            deployment_group_name: None,
            resource_id: "api-handler".to_string(),
            content: msg.to_string(),
            timestamp,
            level,
        });
    }

    logs
}
