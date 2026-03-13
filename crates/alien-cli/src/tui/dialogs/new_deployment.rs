//! New Deployment Dialog
//!
//! A modal dialog for creating new deployments with platform selection.

use ratatui::{
    crossterm::event::{KeyCode, KeyEvent},
    prelude::*,
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
};

/// Platform options for deployment
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Local,
    Aws,
    Gcp,
    Azure,
    Kubernetes,
}

impl Platform {
    /// Get all available platforms
    fn all() -> Vec<Self> {
        vec![
            Self::Local,
            Self::Aws,
            Self::Gcp,
            Self::Azure,
            Self::Kubernetes,
        ]
    }

    /// Get display name
    fn display_name(&self) -> &'static str {
        match self {
            Self::Local => "Local",
            Self::Aws => "AWS",
            Self::Gcp => "GCP",
            Self::Azure => "Azure",
            Self::Kubernetes => "Kubernetes",
        }
    }

    /// Convert to API platform string
    pub fn to_api_string(&self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Aws => "aws",
            Self::Gcp => "gcp",
            Self::Azure => "azure",
            Self::Kubernetes => "kubernetes",
        }
    }
}

impl Default for Platform {
    fn default() -> Self {
        Self::Local
    }
}

/// Which field is currently focused
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusedField {
    Name,
    Platform,
    DeploymentGroup,
    DeployButton,
    CancelButton,
}

impl Default for FocusedField {
    fn default() -> Self {
        Self::Name
    }
}

/// Result from the new deployment dialog
#[derive(Debug, Clone)]
pub struct NewDeploymentResult {
    /// Platform
    pub platform: Platform,
    /// Deployment name
    pub name: String,
    /// Deployment group ID
    pub deployment_group_id: String,
}

/// Deployment group info for display
#[derive(Debug, Clone)]
pub struct DeploymentGroupInfo {
    pub id: String,
    pub name: String,
}

/// New deployment dialog state
pub struct NewDeploymentDialog {
    /// Selected platform
    platform: Platform,
    /// Platform list state
    platform_list_state: ListState,
    /// Deployment name input
    name: String,
    /// Available deployment groups (fetched externally)
    deployment_groups: Vec<DeploymentGroupInfo>,
    /// Selected deployment group index
    deployment_group_list_state: ListState,
    /// Which field is focused
    focused: FocusedField,
    /// Whether dialog is complete
    completed: Option<Result<NewDeploymentResult, ()>>,
    /// Whether we're in dev mode (restricts platform options)
    is_dev_mode: bool,
}

impl NewDeploymentDialog {
    /// Create a new dialog
    pub fn new(is_dev_mode: bool) -> Self {
        let mut platform_list_state = ListState::default();
        platform_list_state.select(Some(0)); // Default to Local

        let mut deployment_group_list_state = ListState::default();
        deployment_group_list_state.select(Some(0));

        Self {
            platform: Platform::Local,
            platform_list_state,
            name: String::new(),
            deployment_groups: Vec::new(),
            deployment_group_list_state,
            focused: FocusedField::Name, // Start with name field
            completed: None,
            is_dev_mode,
        }
    }

    /// Set the available deployment groups
    pub fn with_deployment_groups(mut self, groups: Vec<DeploymentGroupInfo>) -> Self {
        self.deployment_groups = groups;
        if !self.deployment_groups.is_empty() {
            self.deployment_group_list_state.select(Some(0));
        }
        self
    }

    /// Check if dialog is complete
    pub fn is_completed(&self) -> bool {
        self.completed.is_some()
    }

    /// Get the result (if dialog is complete)
    pub fn result(&self) -> Option<Result<NewDeploymentResult, ()>> {
        self.completed.clone()
    }

    /// Handle a key event
    pub fn handle_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.completed = Some(Err(()));
            }
            KeyCode::Tab => {
                self.next_field();
            }
            KeyCode::BackTab => {
                self.prev_field();
            }
            KeyCode::Enter => {
                match self.focused {
                    FocusedField::DeployButton => {
                        self.try_submit();
                    }
                    FocusedField::CancelButton => {
                        self.completed = Some(Err(()));
                    }
                    FocusedField::Platform => {
                        // Check if non-local platform in dev mode
                        if self.is_dev_mode && self.platform != Platform::Local {
                            // Don't proceed - show message in UI
                        } else {
                            self.next_field();
                        }
                    }
                    _ => {
                        self.next_field();
                    }
                }
            }
            KeyCode::Up => match self.focused {
                FocusedField::Platform => {
                    self.select_prev_platform();
                }
                FocusedField::DeploymentGroup => {
                    self.select_prev_deployment_group();
                }
                _ => {}
            },
            KeyCode::Down => match self.focused {
                FocusedField::Platform => {
                    self.select_next_platform();
                }
                FocusedField::DeploymentGroup => {
                    self.select_next_deployment_group();
                }
                _ => {}
            },
            KeyCode::Char(c) => {
                if self.focused == FocusedField::Name {
                    self.name.push(c);
                }
            }
            KeyCode::Backspace => {
                if self.focused == FocusedField::Name {
                    self.name.pop();
                }
            }
            _ => {}
        }
    }

    fn next_field(&mut self) {
        self.focused = match self.focused {
            FocusedField::Name => FocusedField::Platform,
            FocusedField::Platform => FocusedField::DeploymentGroup,
            FocusedField::DeploymentGroup => FocusedField::DeployButton,
            FocusedField::DeployButton => FocusedField::CancelButton,
            FocusedField::CancelButton => FocusedField::Name,
        };
    }

    fn prev_field(&mut self) {
        self.focused = match self.focused {
            FocusedField::Name => FocusedField::CancelButton,
            FocusedField::Platform => FocusedField::Name,
            FocusedField::DeploymentGroup => FocusedField::Platform,
            FocusedField::DeployButton => FocusedField::DeploymentGroup,
            FocusedField::CancelButton => FocusedField::DeployButton,
        };
    }

    /// Check if the current form is valid for deployment
    fn is_valid(&self) -> bool {
        // Deployment name must not be empty
        if self.name.trim().is_empty() {
            return false;
        }

        // In dev mode, only Local platform is allowed
        if self.is_dev_mode && self.platform != Platform::Local {
            return false;
        }

        // Deployment group must be selected
        if self.deployment_group_list_state.selected().is_none() {
            return false;
        }

        true
    }

    fn select_next_platform(&mut self) {
        let platforms = Platform::all();
        let current_idx = platforms
            .iter()
            .position(|p| *p == self.platform)
            .unwrap_or(0);
        let next_idx = if current_idx >= platforms.len() - 1 {
            0
        } else {
            current_idx + 1
        };
        self.platform = platforms[next_idx];
        self.platform_list_state.select(Some(next_idx));
    }

    fn select_prev_platform(&mut self) {
        let platforms = Platform::all();
        let current_idx = platforms
            .iter()
            .position(|p| *p == self.platform)
            .unwrap_or(0);
        let prev_idx = if current_idx == 0 {
            platforms.len() - 1
        } else {
            current_idx - 1
        };
        self.platform = platforms[prev_idx];
        self.platform_list_state.select(Some(prev_idx));
    }

    fn select_next_deployment_group(&mut self) {
        if self.deployment_groups.is_empty() {
            return;
        }
        let i = match self.deployment_group_list_state.selected() {
            Some(i) => {
                if i >= self.deployment_groups.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.deployment_group_list_state.select(Some(i));
    }

    fn select_prev_deployment_group(&mut self) {
        if self.deployment_groups.is_empty() {
            return;
        }
        let i = match self.deployment_group_list_state.selected() {
            Some(i) => {
                if i == 0 {
                    self.deployment_groups.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.deployment_group_list_state.select(Some(i));
    }

    fn try_submit(&mut self) {
        // Validate name
        if self.name.is_empty() {
            return;
        }

        // In dev mode, only allow Local platform
        if self.is_dev_mode && self.platform != Platform::Local {
            return;
        }

        // Get selected deployment group
        if let Some(idx) = self.deployment_group_list_state.selected() {
            if let Some(group) = self.deployment_groups.get(idx) {
                self.completed = Some(Ok(NewDeploymentResult {
                    platform: self.platform,
                    name: self.name.clone(),
                    deployment_group_id: group.id.clone(),
                }));
            }
        }
    }

    /// Render the dialog
    pub fn render(&mut self, frame: &mut Frame, area: Rect) {
        // Dialog dimensions - spacious for comfortable reading
        let dialog_width = 80;
        let dialog_height = 26;

        // Center the dialog
        let x = area.x + (area.width.saturating_sub(dialog_width)) / 2;
        let y = area.y + (area.height.saturating_sub(dialog_height)) / 2;
        let dialog_area = Rect::new(
            x,
            y,
            dialog_width.min(area.width),
            dialog_height.min(area.height),
        );

        // Clear background
        frame.render_widget(Clear, dialog_area);

        // Dialog border - use blue instead of green
        let block = Block::default()
            .title(" New Deployment ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Rgb(96, 165, 250))); // Blue

        let inner = block.inner(dialog_area);
        frame.render_widget(block, dialog_area);

        // Layout inside dialog - Name first, then Platform
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([
                Constraint::Length(2), // Info message
                Constraint::Length(1), // Spacer
                Constraint::Length(1), // Name label
                Constraint::Length(1), // Name field
                Constraint::Length(1), // Spacer
                Constraint::Length(1), // Platform label
                Constraint::Length(6), // Platform list
                Constraint::Length(1), // Spacer
                Constraint::Length(1), // Deployment group label
                Constraint::Length(3), // Deployment group list
                Constraint::Length(1), // Spacer
                Constraint::Length(2), // Help message area (for dev mode warning)
                Constraint::Length(1), // Spacer
                Constraint::Length(1), // Buttons
            ])
            .split(inner);

        // Info message at top
        let info_text = if self.is_dev_mode {
            vec![
                Line::from(vec![
                    Span::styled("i ", Style::default().fg(Color::Rgb(96, 165, 250))),
                    Span::styled(
                        "For testing with local credentials. ",
                        Style::default().fg(Color::Rgb(156, 163, 175)),
                    ),
                    Span::styled(
                        "To deploy to customers, run ",
                        Style::default().fg(Color::Rgb(156, 163, 175)),
                    ),
                ]),
                Line::from(vec![
                    Span::styled("  ", Style::default()),
                    Span::styled(
                        "alien onboard <customer-name>",
                        Style::default().fg(Color::Rgb(156, 163, 175)).italic(),
                    ),
                    Span::styled(
                        " to get a deployment link.",
                        Style::default().fg(Color::Rgb(156, 163, 175)),
                    ),
                ]),
            ]
        } else {
            vec![
                Line::from(vec![Span::styled(
                    "Create a new deployment to a remote environment. ",
                    Style::default().fg(Color::Rgb(156, 163, 175)),
                )]),
                Line::from(vec![
                    Span::styled(
                        "For customers, use deployment links from ",
                        Style::default().fg(Color::Rgb(156, 163, 175)),
                    ),
                    Span::styled(
                        "alien onboard",
                        Style::default().fg(Color::Rgb(156, 163, 175)).italic(),
                    ),
                    Span::styled(".", Style::default().fg(Color::Rgb(156, 163, 175))),
                ]),
            ]
        };
        let info = Paragraph::new(info_text);
        frame.render_widget(info, chunks[0]);

        // Name label
        let name_label_style = if self.focused == FocusedField::Name {
            Style::default()
                .fg(Color::Rgb(96, 165, 250))
                .add_modifier(Modifier::BOLD) // Blue
        } else {
            Style::default().fg(Color::Rgb(156, 163, 175))
        };
        let name_label = Paragraph::new("Deployment Name:").style(name_label_style);
        frame.render_widget(name_label, chunks[2]);

        // Name field with placeholder
        let name_display = if self.name.is_empty() && self.focused != FocusedField::Name {
            "my-deployment" // Placeholder
        } else {
            &self.name
        };

        let name_field_style = if self.focused == FocusedField::Name {
            Style::default()
                .fg(Color::Rgb(229, 231, 235))
                .add_modifier(Modifier::BOLD)
        } else if self.name.is_empty() {
            Style::default().fg(Color::Rgb(107, 114, 128)).italic() // Dim for placeholder
        } else {
            Style::default().fg(Color::Rgb(229, 231, 235))
        };

        let cursor = if self.focused == FocusedField::Name {
            "|"
        } else {
            ""
        };
        let name_field =
            Paragraph::new(format!("  {}{}", name_display, cursor)).style(name_field_style);
        frame.render_widget(name_field, chunks[3]);

        // Platform label
        let platform_label_style = if self.focused == FocusedField::Platform {
            Style::default()
                .fg(Color::Rgb(96, 165, 250))
                .add_modifier(Modifier::BOLD) // Blue
        } else {
            Style::default().fg(Color::Rgb(156, 163, 175))
        };
        let platform_label = Paragraph::new("Platform (use up/down):").style(platform_label_style);
        frame.render_widget(platform_label, chunks[5]);

        // Platform list
        let platform_items: Vec<ListItem> = Platform::all()
            .iter()
            .map(|p| {
                let name = p.display_name();
                ListItem::new(format!("  {}", name))
            })
            .collect();

        let platform_list_style = if self.focused == FocusedField::Platform {
            Style::default().fg(Color::Rgb(96, 165, 250)) // Blue
        } else {
            Style::default().fg(Color::Rgb(107, 114, 128))
        };

        let platform_list = List::new(platform_items)
            .style(platform_list_style)
            .highlight_style(
                Style::default()
                    .fg(Color::Rgb(96, 165, 250))
                    .add_modifier(Modifier::BOLD),
            ) // Blue
            .highlight_symbol("> ");

        frame.render_stateful_widget(platform_list, chunks[6], &mut self.platform_list_state);

        // Deployment group label
        let dg_label_style = if self.focused == FocusedField::DeploymentGroup {
            Style::default()
                .fg(Color::Rgb(96, 165, 250))
                .add_modifier(Modifier::BOLD) // Blue
        } else {
            Style::default().fg(Color::Rgb(156, 163, 175))
        };
        let dg_label = Paragraph::new("Deployment Group (use up/down):").style(dg_label_style);
        frame.render_widget(dg_label, chunks[8]);

        // Deployment group list
        let dg_list_style = if self.focused == FocusedField::DeploymentGroup {
            Style::default().fg(Color::Rgb(96, 165, 250)) // Blue
        } else {
            Style::default().fg(Color::Rgb(107, 114, 128))
        };

        if self.deployment_groups.is_empty() {
            let empty = Paragraph::new("  No deployment groups available")
                .style(Style::default().fg(Color::Rgb(107, 114, 128)));
            frame.render_widget(empty, chunks[9]);
        } else {
            let dg_items: Vec<ListItem> = self
                .deployment_groups
                .iter()
                .map(|g| ListItem::new(format!("  {}", g.name)))
                .collect();

            let dg_list = List::new(dg_items)
                .style(dg_list_style)
                .highlight_style(
                    Style::default()
                        .fg(Color::Rgb(96, 165, 250))
                        .add_modifier(Modifier::BOLD),
                ) // Blue
                .highlight_symbol("> ");

            frame.render_stateful_widget(dg_list, chunks[9], &mut self.deployment_group_list_state);
        }

        // Help message area - show warning in dev mode for non-local platforms
        if self.is_dev_mode && self.platform != Platform::Local {
            let warning_text = vec![
                Line::from(vec![
                    Span::styled("! ", Style::default().fg(Color::Rgb(251, 191, 36))),
                    Span::styled(
                        "Local dev only supports Local platform. Connect to the platform API",
                        Style::default().fg(Color::Rgb(156, 163, 175)),
                    ),
                ]),
                Line::from(vec![
                    Span::styled("  ", Style::default()),
                    Span::styled(
                        "to deploy to AWS, GCP, Azure, or Kubernetes.",
                        Style::default().fg(Color::Rgb(156, 163, 175)),
                    ),
                ]),
            ];
            let warning = Paragraph::new(warning_text);
            frame.render_widget(warning, chunks[11]);
        }

        // Buttons
        let button_area = chunks[13];
        let button_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(button_area);

        // Deploy button - disabled if form is invalid
        let is_valid = self.is_valid();
        let deploy_style = if !is_valid {
            // Disabled state - dim and strikethrough
            Style::default()
                .fg(Color::Rgb(75, 85, 99))
                .add_modifier(Modifier::DIM)
        } else if self.focused == FocusedField::DeployButton {
            // Focused and valid - blue (not green)
            Style::default()
                .fg(Color::Rgb(96, 165, 250))
                .add_modifier(Modifier::BOLD)
        } else {
            // Not focused but valid
            Style::default().fg(Color::Rgb(107, 114, 128))
        };
        let deploy_btn = Paragraph::new("  [Deploy]").style(deploy_style);
        frame.render_widget(deploy_btn, button_chunks[0]);

        let cancel_style = if self.focused == FocusedField::CancelButton {
            Style::default()
                .fg(Color::Rgb(239, 68, 68))
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Rgb(107, 114, 128))
        };
        let cancel_btn = Paragraph::new("  [Cancel]").style(cancel_style);
        frame.render_widget(cancel_btn, button_chunks[1]);
    }
}

impl Default for NewDeploymentDialog {
    fn default() -> Self {
        Self::new(false) // Default to platform mode
    }
}
