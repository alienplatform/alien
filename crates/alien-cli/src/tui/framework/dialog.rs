//! Modal dialog framework

use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Clear, List, ListItem, ListState as RatatuiListState, Paragraph},
};

/// A button in a dialog
#[derive(Debug, Clone)]
pub struct DialogButton {
    pub label: String,
    pub key: char,
    pub is_primary: bool,
}

impl DialogButton {
    pub fn new(label: impl Into<String>, key: char) -> Self {
        Self {
            label: label.into(),
            key,
            is_primary: false,
        }
    }

    pub fn primary(label: impl Into<String>, key: char) -> Self {
        Self {
            label: label.into(),
            key,
            is_primary: true,
        }
    }
}

/// State for a dialog
#[derive(Debug, Clone)]
pub struct DialogState {
    /// Whether dialog is visible
    pub visible: bool,
    /// Current input value (for input dialogs)
    pub input: String,
    /// Selected list index (for list dialogs)
    pub selected_index: usize,
    /// Error message
    pub error: Option<String>,
}

impl DialogState {
    pub fn new() -> Self {
        Self {
            visible: false,
            input: String::new(),
            selected_index: 0,
            error: None,
        }
    }

    pub fn show(&mut self) {
        self.visible = true;
        self.error = None;
    }

    pub fn hide(&mut self) {
        self.visible = false;
        self.input.clear();
        self.selected_index = 0;
        self.error = None;
    }

    pub fn set_error(&mut self, msg: impl Into<String>) {
        self.error = Some(msg.into());
    }
}

impl Default for DialogState {
    fn default() -> Self {
        Self::new()
    }
}

/// Dialog widget for rendering modal dialogs
pub struct Dialog;

impl Dialog {
    /// Render a confirmation dialog
    pub fn render_confirm(
        frame: &mut Frame,
        area: Rect,
        title: &str,
        message: &str,
        buttons: &[DialogButton],
    ) {
        let dialog_area = Self::center_dialog(area, 50, 7);

        // Clear and draw border
        frame.render_widget(Clear, dialog_area);
        let block = Block::default()
            .title(format!(" {} ", title))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Rgb(34, 197, 94)));

        let inner = block.inner(dialog_area);
        frame.render_widget(block, dialog_area);

        // Layout: message and buttons
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(2),    // Message
                Constraint::Length(1), // Buttons
            ])
            .margin(1)
            .split(inner);

        // Message
        let msg = Paragraph::new(message)
            .style(Style::default().fg(Color::Rgb(229, 231, 235)))
            .alignment(Alignment::Center);
        frame.render_widget(msg, chunks[0]);

        // Buttons
        let button_text: String = buttons
            .iter()
            .map(|b| {
                if b.is_primary {
                    format!("[{}] {} ", b.key, b.label)
                } else {
                    format!("[{}] {} ", b.key, b.label)
                }
            })
            .collect();

        let buttons_widget = Paragraph::new(button_text)
            .style(Style::default().fg(Color::Rgb(156, 163, 175)))
            .alignment(Alignment::Center);
        frame.render_widget(buttons_widget, chunks[1]);
    }

    /// Render an input dialog
    pub fn render_input(
        frame: &mut Frame,
        area: Rect,
        title: &str,
        label: &str,
        state: &DialogState,
    ) {
        let dialog_area = Self::center_dialog(area, 60, 9);

        frame.render_widget(Clear, dialog_area);
        let block = Block::default()
            .title(format!(" {} ", title))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Rgb(34, 197, 94)));

        let inner = block.inner(dialog_area);
        frame.render_widget(block, dialog_area);

        // Layout
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // Label
                Constraint::Length(3), // Input box
                Constraint::Length(1), // Error or help
            ])
            .margin(1)
            .split(inner);

        // Label
        let label_widget =
            Paragraph::new(label).style(Style::default().fg(Color::Rgb(156, 163, 175)));
        frame.render_widget(label_widget, chunks[0]);

        // Input box
        let input_block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Rgb(75, 85, 99)));
        let input_inner = input_block.inner(chunks[1]);
        frame.render_widget(input_block, chunks[1]);

        let cursor = "│";
        let input_text = format!("{}{}", state.input, cursor);
        let input =
            Paragraph::new(input_text).style(Style::default().fg(Color::Rgb(229, 231, 235)));
        frame.render_widget(input, input_inner);

        // Error or help
        if let Some(ref error) = state.error {
            let error_widget =
                Paragraph::new(error.as_str()).style(Style::default().fg(Color::Rgb(239, 68, 68)));
            frame.render_widget(error_widget, chunks[2]);
        } else {
            let help = Paragraph::new("[Enter] confirm  [Esc] cancel")
                .style(Style::default().fg(Color::Rgb(107, 114, 128)))
                .alignment(Alignment::Center);
            frame.render_widget(help, chunks[2]);
        }
    }

    /// Render a list selection dialog
    pub fn render_list(
        frame: &mut Frame,
        area: Rect,
        title: &str,
        items: &[String],
        state: &DialogState,
    ) {
        let height = (items.len() + 4).min(15) as u16;
        let dialog_area = Self::center_dialog(area, 50, height);

        frame.render_widget(Clear, dialog_area);
        let block = Block::default()
            .title(format!(" {} ", title))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Rgb(34, 197, 94)));

        let inner = block.inner(dialog_area);
        frame.render_widget(block, dialog_area);

        // Layout
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(3),    // List
                Constraint::Length(1), // Help
            ])
            .split(inner);

        // List items
        let list_items: Vec<ListItem> = items
            .iter()
            .enumerate()
            .map(|(idx, item)| {
                let style = if idx == state.selected_index {
                    Style::default().fg(Color::Rgb(34, 197, 94)).bold()
                } else {
                    Style::default().fg(Color::Rgb(229, 231, 235))
                };
                let prefix = if idx == state.selected_index {
                    "▶ "
                } else {
                    "  "
                };
                ListItem::new(format!("{}{}", prefix, item)).style(style)
            })
            .collect();

        let list = List::new(list_items);
        let mut list_state = RatatuiListState::default();
        list_state.select(Some(state.selected_index));
        frame.render_stateful_widget(list, chunks[0], &mut list_state);

        // Help
        let help = Paragraph::new("[↑↓] select  [Enter] confirm  [Esc] cancel")
            .style(Style::default().fg(Color::Rgb(107, 114, 128)))
            .alignment(Alignment::Center);
        frame.render_widget(help, chunks[1]);
    }

    /// Center a dialog in the given area
    fn center_dialog(area: Rect, width: u16, height: u16) -> Rect {
        let width = width.min(area.width.saturating_sub(4));
        let height = height.min(area.height.saturating_sub(2));
        let x = area.x + (area.width.saturating_sub(width)) / 2;
        let y = area.y + (area.height.saturating_sub(height)) / 2;
        Rect::new(x, y, width, height)
    }
}
