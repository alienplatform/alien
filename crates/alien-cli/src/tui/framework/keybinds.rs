//! Footer keybind display widget

use ratatui::{prelude::*, widgets::Paragraph};

/// Footer widget showing available keybinds (alias for backwards compat)
pub type Keybinds = KeybindsFooter;

/// Footer widget showing available keybinds
pub struct KeybindsFooter;

impl KeybindsFooter {
    /// Render the keybinds footer
    pub fn render(frame: &mut Frame, area: Rect, keybinds: &[(&str, &str)]) {
        let spans: Vec<Span> = keybinds
            .iter()
            .enumerate()
            .flat_map(|(idx, (key, desc))| {
                let mut parts = vec![
                    Span::styled(
                        format!("[{}]", key),
                        Style::default().fg(Color::Rgb(34, 197, 94)),
                    ),
                    Span::styled(
                        format!(" {} ", desc),
                        Style::default().fg(Color::Rgb(107, 114, 128)),
                    ),
                ];
                if idx < keybinds.len() - 1 {
                    parts.push(Span::styled(" ", Style::default()));
                }
                parts
            })
            .collect();

        let line = Line::from(spans);
        let footer = Paragraph::new(line);
        frame.render_widget(footer, area);
    }
}
