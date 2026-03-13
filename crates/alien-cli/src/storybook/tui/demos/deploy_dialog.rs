//! Deploy deployment dialog demos
//!
//! Demonstrates the NewDeploymentDialog for different scenarios.

use alien_cli::tui::dialogs::{DeploymentGroupInfo, NewDeploymentDialog};
use clap::Subcommand;
use color_eyre::Result;

#[derive(Subcommand, Debug, Clone, Copy)]
pub enum DeployDialogDemo {
    /// Empty form - initial state
    Empty,
    /// Form with deployment groups
    WithGroups,
    /// Dev mode (local platform only)
    DevMode,
}

impl DeployDialogDemo {
    pub fn run(self) -> Result<()> {
        use crossterm::{
            event::{self, KeyCode},
            execute,
            terminal::{
                disable_raw_mode, enable_raw_mode, Clear, ClearType, EnterAlternateScreen,
                LeaveAlternateScreen,
            },
        };
        use ratatui::prelude::*;
        use std::time::Duration;

        enable_raw_mode()?;
        let mut stdout = std::io::stdout();
        execute!(stdout, EnterAlternateScreen, Clear(ClearType::All))?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        let mut dialog = create_dialog_for_scenario(self);

        loop {
            terminal.draw(|frame| {
                let area = frame.area();
                dialog.render(frame, area);
            })?;

            if event::poll(Duration::from_millis(100))? {
                if let event::Event::Key(key) = event::read()? {
                    match key.code {
                        KeyCode::Esc => break,
                        KeyCode::Char('q') => break,
                        _ => {
                            // Handle key in dialog
                            dialog.handle_key(key);
                        }
                    }
                }
            }
        }

        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
        disable_raw_mode()?;

        Ok(())
    }
}

fn create_dialog_for_scenario(scenario: DeployDialogDemo) -> NewDeploymentDialog {
    let deployment_groups = vec![
        DeploymentGroupInfo {
            id: "dg_123".to_string(),
            name: "Production".to_string(),
        },
        DeploymentGroupInfo {
            id: "dg_456".to_string(),
            name: "Staging".to_string(),
        },
        DeploymentGroupInfo {
            id: "dg_789".to_string(),
            name: "Development".to_string(),
        },
    ];

    match scenario {
        DeployDialogDemo::Empty => NewDeploymentDialog::new(false),
        DeployDialogDemo::WithGroups => {
            NewDeploymentDialog::new(false).with_deployment_groups(deployment_groups)
        }
        DeployDialogDemo::DevMode => {
            NewDeploymentDialog::new(true).with_deployment_groups(deployment_groups)
        }
    }
}
