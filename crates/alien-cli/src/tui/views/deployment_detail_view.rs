//! Deployment detail view - pure rendering functions
//!
//! Shows resources and metadata for a deployment in a clear, professional layout.

use ratatui::{
    crossterm::event::{KeyCode, KeyEvent},
    prelude::*,
    widgets::{Block, Borders, Cell, Paragraph, Row, Table},
};

use crate::tui::app::config::AppMode;
use crate::tui::state::{
    deployments::DeploymentStatusExt, format_deployment_model, format_environment_info,
    format_heartbeats_mode, format_resource_status, format_telemetry_mode, format_updates_mode,
    Action, DeploymentDetailState, DeploymentStatus, ResourceInfo,
};
use alien_core::{
    ContainerOutputs, FunctionOutputs, KvOutputs, QueueOutputs, ResourceStatus, StorageOutputs,
};

// Color palette
const TEXT: Color = Color::Rgb(229, 231, 235);
const TEXT_BOLD: Color = Color::Rgb(255, 255, 255);
const DIM_TEXT: Color = Color::Rgb(107, 114, 128);
const BLUE: Color = Color::Rgb(59, 130, 246);
const GREEN: Color = Color::Rgb(34, 197, 94);
const YELLOW: Color = Color::Rgb(245, 158, 11);
const RED: Color = Color::Rgb(239, 68, 68);
const FROZEN_COLOR: Color = Color::Rgb(147, 197, 253);
const BORDER: Color = Color::Rgb(75, 85, 99);

/// Render the deployment detail view
pub fn render(frame: &mut Frame, area: Rect, state: &DeploymentDetailState) {
    // Main layout: Info (top) | Resources (bottom)
    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(12), // Deployment info (fixed height)
            Constraint::Min(10),    // Resources table (rest of space)
        ])
        .split(area);

    render_deployment_info(frame, main_chunks[0], state);
    render_resources_table(frame, main_chunks[1], state);
}

fn render_resources_table(frame: &mut Frame, area: Rect, state: &DeploymentDetailState) {
    let block = Block::default()
        .title(" Resources ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if state.resources.is_empty() {
        let empty =
            Paragraph::new("No resources deployed yet...").style(Style::default().fg(DIM_TEXT));
        frame.render_widget(empty, inner);
        return;
    }

    // Table headers
    let header = Row::new(vec!["Name", "Type", "Lifecycle", "Status", "Details"])
        .style(Style::default().fg(DIM_TEXT).bold())
        .bottom_margin(0); // No margin between header and rows

    // Table rows - show all resources (no scrolling)
    let rows: Vec<Row> = state
        .resources
        .iter()
        .map(|resource| {
            let lifecycle_text = if resource.is_frozen() {
                "frozen"
            } else {
                "live"
            };
            let lifecycle_color = if resource.is_frozen() {
                FROZEN_COLOR
            } else {
                DIM_TEXT
            };

            let status_text = format_resource_status(&resource.status);
            let status_color = status_color(&resource.status);

            let details = format_resource_details(resource);

            Row::new(vec![
                // Name
                Cell::from(resource.id.clone()).style(Style::default().fg(TEXT_BOLD)),
                // Type
                Cell::from(resource.resource_type.as_ref()).style(Style::default().fg(DIM_TEXT)),
                // Lifecycle
                Cell::from(lifecycle_text).style(Style::default().fg(lifecycle_color)),
                // Status
                Cell::from(status_text).style(Style::default().fg(status_color)),
                // Details
                Cell::from(details).style(Style::default().fg(BLUE)),
            ])
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Length(25), // Name
            Constraint::Length(13), // Type
            Constraint::Length(10), // Lifecycle
            Constraint::Length(20), // Status (wider for provision-failed, etc.)
            Constraint::Min(30),    // Details
        ],
    )
    .header(header)
    .column_spacing(2);

    frame.render_widget(table, inner);
}

fn format_resource_details(resource: &ResourceInfo) -> String {
    let Some(ref outputs) = resource.outputs else {
        return String::new();
    };

    match resource.resource_type.as_ref() {
        "function" => {
            if let Some(func) = outputs.downcast_ref::<FunctionOutputs>() {
                func.url
                    .clone()
                    .unwrap_or_else(|| func.function_name.clone())
            } else {
                String::new()
            }
        }
        "container" => {
            if let Some(cont) = outputs.downcast_ref::<ContainerOutputs>() {
                if let Some(ref url) = cont.url {
                    format!(
                        "{} ({}/{})",
                        url, cont.current_replicas, cont.desired_replicas
                    )
                } else {
                    format!(
                        "{}/{}  replicas",
                        cont.current_replicas, cont.desired_replicas
                    )
                }
            } else {
                String::new()
            }
        }
        "storage" => {
            if let Some(stor) = outputs.downcast_ref::<StorageOutputs>() {
                stor.bucket_name.clone()
            } else {
                String::new()
            }
        }
        "kv" => {
            if let Some(kv) = outputs.downcast_ref::<KvOutputs>() {
                kv.store_name.clone()
            } else {
                String::new()
            }
        }
        "queue" => {
            if let Some(q) = outputs.downcast_ref::<QueueOutputs>() {
                q.queue_name.clone()
            } else {
                String::new()
            }
        }
        _ => String::new(),
    }
}

fn render_deployment_info(frame: &mut Frame, area: Rect, state: &DeploymentDetailState) {
    let block = Block::default()
        .title(" Deployment Info ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let Some(ref metadata) = state.metadata else {
        let loading =
            Paragraph::new("Loading deployment details...").style(Style::default().fg(DIM_TEXT));
        frame.render_widget(loading, inner);
        return;
    };

    // Split into two columns for better space usage
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(50), // Left column
            Constraint::Percentage(50), // Right column
        ])
        .split(inner);

    // Left column: Status, Platform, Created, Release
    let mut left_lines = vec![];

    // Deployment status (prominent)
    let status_text = state.status.display();
    let status_color = match state.status {
        DeploymentStatus::Running => GREEN,
        DeploymentStatus::Provisioning
        | DeploymentStatus::Updating
        | DeploymentStatus::UpdatePending
        | DeploymentStatus::InitialSetup => YELLOW,
        DeploymentStatus::Pending => DIM_TEXT,
        _ => RED,
    };
    left_lines.push(Line::from(vec![
        Span::styled("Status:      ", Style::default().fg(DIM_TEXT)),
        Span::styled(status_text, Style::default().fg(status_color).bold()),
    ]));

    left_lines.push(Line::from(vec![
        Span::styled("Platform:    ", Style::default().fg(DIM_TEXT)),
        Span::styled(
            format!("{:?}", metadata.platform),
            Style::default().fg(TEXT),
        ),
    ]));

    // Show deployment group name
    let dg_display = state
        .deployment_group_name
        .as_deref()
        .unwrap_or(&state.deployment_group_id);
    if !dg_display.is_empty() {
        left_lines.push(Line::from(vec![
            Span::styled("Group:       ", Style::default().fg(DIM_TEXT)),
            Span::styled(dg_display, Style::default().fg(TEXT)),
        ]));
    }

    left_lines.push(Line::from(vec![
        Span::styled("Created:     ", Style::default().fg(DIM_TEXT)),
        Span::styled(&metadata.created_at, Style::default().fg(TEXT)),
    ]));

    if let Some(ref release_id) = metadata.current_release_id {
        left_lines.push(Line::from(vec![
            Span::styled("Release:     ", Style::default().fg(DIM_TEXT)),
            Span::styled(release_id, Style::default().fg(TEXT)),
        ]));
    }

    left_lines.push(Line::from(""));

    // Deployment settings
    left_lines.push(Line::from(vec![Span::styled(
        "Deployment",
        Style::default().fg(GREEN).bold(),
    )]));

    left_lines.push(Line::from(vec![
        Span::styled("  Model:     ", Style::default().fg(DIM_TEXT)),
        Span::styled(
            format_deployment_model(&metadata.stack_settings.deployment_model),
            Style::default().fg(TEXT),
        ),
    ]));

    left_lines.push(Line::from(vec![
        Span::styled("  Updates:   ", Style::default().fg(DIM_TEXT)),
        Span::styled(
            format_updates_mode(&metadata.stack_settings.updates),
            Style::default().fg(TEXT),
        ),
    ]));

    let left_para = Paragraph::new(left_lines);
    frame.render_widget(left_para, columns[0]);

    // Right column: Environment info, Telemetry, Heartbeats
    let mut right_lines = vec![];

    if let Some(ref env_info) = metadata.environment_info {
        right_lines.push(Line::from(vec![Span::styled(
            "Environment",
            Style::default().fg(GREEN).bold(),
        )]));
        right_lines.push(Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(format_environment_info(env_info), Style::default().fg(TEXT)),
        ]));
        right_lines.push(Line::from(""));
    }

    right_lines.push(Line::from(vec![Span::styled(
        "Observability",
        Style::default().fg(GREEN).bold(),
    )]));

    right_lines.push(Line::from(vec![
        Span::styled("  Telemetry: ", Style::default().fg(DIM_TEXT)),
        Span::styled(
            format_telemetry_mode(&metadata.stack_settings.telemetry),
            Style::default().fg(TEXT),
        ),
    ]));

    right_lines.push(Line::from(vec![
        Span::styled("  Heartbeats:", Style::default().fg(DIM_TEXT)),
        Span::styled(
            format_heartbeats_mode(&metadata.stack_settings.heartbeats),
            Style::default().fg(TEXT),
        ),
    ]));

    let right_para = Paragraph::new(right_lines);
    frame.render_widget(right_para, columns[1]);
}

fn status_color(status: &ResourceStatus) -> Color {
    match status {
        ResourceStatus::Pending => DIM_TEXT,
        ResourceStatus::Provisioning | ResourceStatus::Updating => YELLOW,
        ResourceStatus::Running => GREEN,
        ResourceStatus::Deleting => YELLOW,
        ResourceStatus::Deleted => DIM_TEXT,
        ResourceStatus::ProvisionFailed
        | ResourceStatus::UpdateFailed
        | ResourceStatus::DeleteFailed
        | ResourceStatus::RefreshFailed => RED,
    }
}

/// Handle key input for deployment detail view
pub fn handle_key(key: KeyEvent, state: &DeploymentDetailState, _mode: AppMode) -> Action {
    match key.code {
        // Show error dialog if deployment has an error
        KeyCode::Char('e') | KeyCode::Char('E') => {
            if let Some(ref metadata) = state.metadata {
                if let Some(ref error) = metadata.error {
                    return Action::ShowErrorDialog(error.clone());
                }
            }
            Action::None
        }
        // Navigate to logs filtered by this deployment
        KeyCode::Char('l') | KeyCode::Char('L') => Action::NavigateToLogsFilteredByDeployment {
            deployment_id: state.deployment_id.clone(),
            deployment_name: state.deployment_name.clone(),
        },
        // Navigate to commands filtered by this deployment
        KeyCode::Char('c') | KeyCode::Char('C') => Action::NavigateToCommandsFilteredByDeployment {
            deployment_id: state.deployment_id.clone(),
            deployment_name: state.deployment_name.clone(),
        },
        _ => Action::None,
    }
}

/// Get keybinds for deployment detail view
pub fn keybinds(mode: AppMode, has_error: bool) -> Vec<(&'static str, &'static str)> {
    let mut binds = vec![("ESC", "back"), ("l", "logs"), ("c", "commands")];

    // Add rebuild keybind in dev mode
    if mode == AppMode::Dev {
        binds.push(("b", "rebuild"));
    }

    // Add view error keybind if there's an error
    if has_error {
        binds.push(("e", "view error"));
    }

    binds.push(("q", "quit"));

    binds
}
