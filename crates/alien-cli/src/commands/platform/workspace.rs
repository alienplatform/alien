use crate::auth::{load_workspace, save_workspace};
use crate::error::{ErrorData, Result};
use crate::execution_context::ExecutionMode;
use alien_error::{Context, IntoAlienError};
use alien_platform_api::SdkResultExt;
use clap::{Parser, Subcommand};
use ratatui::{prelude::*, widgets::Paragraph, TerminalOptions, Viewport};
use std::io::IsTerminal;
use std::time::Duration;

#[derive(Parser, Debug, Clone)]
#[command(
    about = "Workspace commands",
    long_about = "Manage workspaces in the Alien platform."
)]
pub struct WorkspaceArgs {
    #[command(subcommand)]
    pub cmd: WorkspaceCmd,
}

#[derive(Subcommand, Debug, Clone)]
pub enum WorkspaceCmd {
    /// Print effective current workspace
    Current,
    /// Set default workspace
    Set {
        /// Workspace name (optional - if not provided, shows interactive selection)
        name: Option<String>,
    },
    /// List all available workspaces
    #[command(alias = "list")]
    Ls,
}

pub async fn workspace_task(args: WorkspaceArgs, ctx: ExecutionMode) -> Result<()> {
    match &args.cmd {
        WorkspaceCmd::Current => match load_workspace() {
            Some(name) => println!("{name}"),
            None => println!("<none>  (use `alien workspace set <name>` or run `alien login`)"),
        },
        WorkspaceCmd::Set { .. } | WorkspaceCmd::Ls => {
            // Initialize HTTP client once for commands that need it
            let http = ctx.auth_http().await?;
            let client = http.sdk_client();

            match args.cmd {
                WorkspaceCmd::Set { name } => {
                    let workspace_name = match name {
                        Some(provided_name) => {
                            // Validate the provided workspace name exists
                            let response = client
                                .list_workspaces()
                                .send()
                                .await
                                .into_sdk_error()
                                .context(ErrorData::ApiRequestFailed {
                                message: "Failed to list workspaces".to_string(),
                                url: None,
                            })?;
                            let workspaces_response = response.into_inner();
                            let all: Vec<String> = workspaces_response
                                .items
                                .into_iter()
                                .map(|w| (*w.name).clone())
                                .collect();
                            if !all.iter().any(|ws| *ws == provided_name) {
                                return Err(alien_error::AlienError::new(
                                    ErrorData::ConfigurationError {
                                        message: format!(
                                            "Workspace '{}' not found in your memberships",
                                            provided_name
                                        ),
                                    },
                                ));
                            }
                            provided_name
                        }
                        None => {
                            // No name provided, show interactive selection
                            prompt_workspace_with_tui(&http).await?
                        }
                    };
                    save_workspace(&workspace_name)?;
                    println!("✅ Default workspace set to: {}", workspace_name);
                }
                WorkspaceCmd::Ls => {
                    let response = client
                        .list_workspaces()
                        .send()
                        .await
                        .into_sdk_error()
                        .context(ErrorData::ApiRequestFailed {
                            message: "Failed to list workspaces".to_string(),
                            url: None,
                        })?;
                    let workspaces_response = response.into_inner();
                    let workspaces: Vec<String> = workspaces_response
                        .items
                        .into_iter()
                        .map(|w| (*w.name).clone())
                        .collect();
                    if workspaces.is_empty() {
                        println!("(no workspaces)");
                    } else {
                        for workspace in workspaces {
                            println!("{}", workspace);
                        }
                    }
                }
                WorkspaceCmd::Current => unreachable!(), // Already handled above
            }
        }
    }
    Ok(())
}

pub async fn prompt_workspace_with_tui(http: &crate::auth::AuthHttp) -> Result<String> {
    let client = http.sdk_client();
    let response = client
        .list_workspaces()
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: "Failed to list workspaces".to_string(),
            url: None,
        })?;
    let workspaces_response = response.into_inner();
    let choices: Vec<String> = workspaces_response
        .items
        .into_iter()
        .map(|w| (*w.name).clone())
        .collect();
    if choices.is_empty() {
        return Err(alien_error::AlienError::new(
            ErrorData::ConfigurationError {
                message: "No workspaces found".to_string(),
            },
        ));
    }

    // If only one workspace, return it directly
    if choices.len() == 1 {
        return Ok(choices[0].clone());
    }

    // Check if we can use TUI (TTY available)
    if !std::io::stderr().is_terminal() || !std::io::stdout().is_terminal() {
        return prompt_workspace_console(&choices);
    }

    // Use inline TUI selection like build TUI
    let mut terminal = ratatui::init_with_options(TerminalOptions {
        viewport: Viewport::Inline(choices.len() as u16 + 3), // Title + empty line + options + bottom margin
    });
    let result = workspace_selection_tui(&mut terminal, choices).await;
    ratatui::restore();
    result
}

fn prompt_workspace_console(choices: &[String]) -> Result<String> {
    println!("Select a workspace:");
    for (i, name) in choices.iter().enumerate() {
        println!("  [{}] {}", i + 1, name);
    }
    print!("Enter number: ");
    use std::io::Write;
    std::io::stdout().flush().ok();
    let mut line = String::new();
    std::io::stdin()
        .read_line(&mut line)
        .into_alien_error()
        .context(ErrorData::TuiOperationFailed {
            message: "Failed to read user input".to_string(),
        })?;
    let idx: usize =
        line.trim()
            .parse()
            .into_alien_error()
            .context(ErrorData::TuiOperationFailed {
                message: "Expected a number".to_string(),
            })?;
    if idx == 0 || idx > choices.len() {
        return Err(alien_error::AlienError::new(ErrorData::UserCancelled));
    }
    Ok(choices[idx - 1].clone())
}

async fn workspace_selection_tui(
    terminal: &mut ratatui::DefaultTerminal,
    choices: Vec<String>,
) -> Result<String> {
    let mut selected = 0;

    loop {
        terminal
            .draw(|frame: &mut ratatui::Frame| {
                let area = frame.area();

                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .margin(0)
                    .constraints([
                        Constraint::Length(1),                    // Title with prompt
                        Constraint::Length(1),                    // Empty line
                        Constraint::Length(choices.len() as u16), // Options
                        Constraint::Length(1),                    // Bottom margin
                    ])
                    .split(area);

                // Combined title and prompt
                let title_prompt = Paragraph::new("Select a workspace:")
                    .style(Style::default().fg(Color::Rgb(34, 197, 94)).bold());
                frame.render_widget(title_prompt, chunks[0]);

                // Simple list without borders
                for (i, workspace) in choices.iter().enumerate() {
                    let prefix = if selected == i { "▶ " } else { "  " };
                    let line = format!("{}{}", prefix, workspace);
                    let style = if selected == i {
                        Style::default().fg(Color::Rgb(34, 197, 94)).bold()
                    } else {
                        Style::default().fg(Color::Rgb(156, 163, 175))
                    };

                    let item = Paragraph::new(line).style(style);
                    let item_area = Rect {
                        x: chunks[2].x,
                        y: chunks[2].y + i as u16,
                        width: chunks[2].width,
                        height: 1,
                    };
                    frame.render_widget(item, item_area);
                }
            })
            .into_alien_error()
            .context(ErrorData::TuiOperationFailed {
                message: "Failed to draw TUI frame".to_string(),
            })?;

        // Handle input
        if crossterm::event::poll(Duration::from_millis(100))
            .into_alien_error()
            .context(ErrorData::TuiOperationFailed {
                message: "Failed to poll for events".to_string(),
            })?
        {
            if let crossterm::event::Event::Key(key) = crossterm::event::read()
                .into_alien_error()
                .context(ErrorData::TuiOperationFailed {
                message: "Failed to read event".to_string(),
            })? {
                match key.code {
                    crossterm::event::KeyCode::Up => {
                        if selected > 0 {
                            selected -= 1;
                        } else {
                            selected = choices.len() - 1;
                        }
                    }
                    crossterm::event::KeyCode::Down => {
                        if selected < choices.len() - 1 {
                            selected += 1;
                        } else {
                            selected = 0;
                        }
                    }
                    crossterm::event::KeyCode::Enter => {
                        return Ok(choices[selected].clone());
                    }
                    crossterm::event::KeyCode::Esc | crossterm::event::KeyCode::Char('q') => {
                        return Err(alien_error::AlienError::new(ErrorData::UserCancelled));
                    }
                    crossterm::event::KeyCode::Char('c')
                        if key
                            .modifiers
                            .contains(crossterm::event::KeyModifiers::CONTROL) =>
                    {
                        return Err(alien_error::AlienError::new(ErrorData::UserCancelled));
                    }
                    _ => {}
                }
            }
        }
    }
}
