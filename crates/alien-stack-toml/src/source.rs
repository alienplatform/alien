//! Turning byte spans into `file:line:column` errors with a source excerpt.

use std::ops::Range;

use alien_error::AlienError;

use crate::error::ErrorData;

/// The text of one TOML file, used to locate errors in it.
pub struct SourceFile<'a> {
    /// Path shown in errors.
    pub name: String,
    pub text: &'a str,
}

impl SourceFile<'_> {
    /// An error pointing at `span`.
    pub fn error(&self, span: Range<usize>, message: impl Into<String>) -> AlienError<ErrorData> {
        let start = span.start.min(self.text.len());
        let line_start = self.text[..start].rfind('\n').map_or(0, |index| index + 1);
        let line_end = self.text[start..]
            .find('\n')
            .map_or(self.text.len(), |index| start + index);
        let line_text = self.text[line_start..line_end].trim_end_matches('\r');
        let line = self.text[..start].matches('\n').count() + 1;
        let column = self.text[line_start..start].chars().count() + 1;

        // Underline the span, clipped to its first line; at least one caret.
        let end = span.end.clamp(start, line_start + line_text.len());
        let width = self.text[start..end].chars().count().max(1);
        let gutter = " ".repeat(line.to_string().len());
        let snippet = format!(
            "{gutter} |\n{line} | {line_text}\n{gutter} | {}{}",
            " ".repeat(column - 1),
            "^".repeat(width)
        );

        AlienError::new(ErrorData::InvalidDocument {
            file: self.name.clone(),
            line,
            column,
            message: message.into(),
            snippet,
        })
    }
}
