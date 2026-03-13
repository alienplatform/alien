//! Deployments list view - pure rendering functions
//!
//! Displays a list of deployments with status and platform information.

use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::prelude::*;

use super::table::{render_table, TableCell, TableColumn, TableItem, TableRow};
use crate::tui::state::deployments::{DeploymentPlatform, DeploymentStatusExt};
use crate::tui::state::{Action, AppState, DeploymentItem, DeploymentStatus, ListState};

impl TableItem for DeploymentItem {
    fn columns() -> Vec<TableColumn> {
        vec![
            TableColumn::new("NAME", Constraint::Length(20)),
            TableColumn::new("DEPLOYMENT GROUP", Constraint::Length(35)),
            TableColumn::new("STATUS", Constraint::Length(18)),
            TableColumn::new("PLATFORM", Constraint::Length(12)),
        ]
    }

    fn to_row(&self) -> TableRow {
        let status_str = self.status.display();
        let status_cell = match self.status {
            DeploymentStatus::Running => TableCell::success(status_str),
            DeploymentStatus::Pending
            | DeploymentStatus::Provisioning
            | DeploymentStatus::InitialSetup
            | DeploymentStatus::Updating
            | DeploymentStatus::UpdatePending => TableCell::warning(status_str),
            DeploymentStatus::InitialSetupFailed
            | DeploymentStatus::ProvisioningFailed
            | DeploymentStatus::RefreshFailed
            | DeploymentStatus::UpdateFailed
            | DeploymentStatus::DeleteFailed => TableCell::error(status_str),
            _ => TableCell::dim(status_str),
        };

        let platform = match self.platform {
            DeploymentPlatform::Aws => "Aws",
            DeploymentPlatform::Gcp => "Gcp",
            DeploymentPlatform::Azure => "Azure",
            DeploymentPlatform::Local => "Local",
            DeploymentPlatform::Kubernetes => "Kubernetes",
            DeploymentPlatform::Test => "Test",
        };

        // Display deployment group name if available, otherwise show ID
        let dg_display = self
            .deployment_group_name
            .as_deref()
            .unwrap_or(&self.deployment_group_id);

        TableRow::new(vec![
            TableCell::new(&self.name),
            TableCell::dim(dg_display),
            status_cell,
            TableCell::dim(platform),
        ])
    }

    fn title() -> &'static str {
        "Deployments"
    }

    fn empty_message() -> &'static str {
        "No deployments found. Press 'n' to create one."
    }
}

/// Render the deployments list view
pub fn render(frame: &mut Frame, area: Rect, state: &ListState<DeploymentItem>, app: &AppState) {
    render_table(frame, area, state, app);
}

/// Handle key input for deployments list
pub fn handle_key(key: KeyEvent, state: &mut ListState<DeploymentItem>) -> Action {
    match key.code {
        KeyCode::Down | KeyCode::Char('j') => {
            state.select_next();
            Action::None
        }
        KeyCode::Up | KeyCode::Char('k') => {
            state.select_prev();
            Action::None
        }
        KeyCode::Home | KeyCode::Char('g') => {
            state.select_first();
            Action::None
        }
        KeyCode::End | KeyCode::Char('G') => {
            state.select_last();
            Action::None
        }
        KeyCode::Enter => {
            if let Some(deployment) = state.selected_item() {
                Action::NavigateToDeployment(deployment.id.clone())
            } else {
                Action::None
            }
        }
        KeyCode::Char('l') | KeyCode::Char('L') => {
            if let Some(deployment) = state.selected_item() {
                Action::NavigateToLogsFilteredByDeployment {
                    deployment_id: deployment.id.clone(),
                    deployment_name: deployment.name.clone(),
                }
            } else {
                Action::None
            }
        }
        KeyCode::Char('c') | KeyCode::Char('C') => {
            if let Some(deployment) = state.selected_item() {
                Action::NavigateToCommandsFilteredByDeployment {
                    deployment_id: deployment.id.clone(),
                    deployment_name: deployment.name.clone(),
                }
            } else {
                Action::None
            }
        }
        KeyCode::Char('n') => Action::OpenNewDeploymentDialog,
        KeyCode::Char('d') | KeyCode::Delete => {
            if let Some(deployment) = state.selected_item() {
                Action::DeleteDeployment(deployment.id.clone())
            } else {
                Action::None
            }
        }
        KeyCode::Char('r') => Action::Refresh,
        _ => Action::None,
    }
}

/// Get keybinds for deployments list
pub fn keybinds() -> Vec<(&'static str, &'static str)> {
    vec![
        ("↑/k", "up"),
        ("↓/j", "down"),
        ("Enter", "view"),
        ("l", "logs"),
        ("c", "commands"),
        ("n", "deploy"),
        ("d", "delete"),
        ("r", "refresh"),
        ("/", "search"),
        ("q", "quit"),
    ]
}
