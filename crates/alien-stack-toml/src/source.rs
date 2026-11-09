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
        let end = span.end.min(line_start + line_text.len()).max(start);
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

    /// Like [`SourceFile::error`], for a file whose values are secrets: the excerpt shows the
    /// text before the first `=` and hides everything after it, so a log never carries a value.
    pub fn error_hiding_values(
        &self,
        span: Range<usize>,
        message: impl Into<String>,
    ) -> AlienError<ErrorData> {
        const HIDDEN_VALUE: &str = "<value hidden>";
        const HIDDEN_LINE: &str = "<line hidden>";

        let start = span.start.min(self.text.len());
        let line_start = self.text[..start].rfind('\n').map_or(0, |index| index + 1);
        let line_end = self.text[start..]
            .find('\n')
            .map_or(self.text.len(), |index| start + index);
        let line_text = self.text[line_start..line_end].trim_end_matches('\r');
        let line = self.text[..start].matches('\n').count() + 1;
        let column = self.text[line_start..start].chars().count() + 1;

        let (shown, caret_column, width) = match line_text.find('=') {
            Some(equals) if start - line_start > equals => {
                let key = line_text[..equals].trim_end();
                (
                    format!("{key}={HIDDEN_VALUE}"),
                    key.chars().count() + 2,
                    HIDDEN_VALUE.len(),
                )
            }
            Some(equals) => {
                let end = span.end.min(line_start + equals).max(start);
                (
                    format!("{}={HIDDEN_VALUE}", line_text[..equals].trim_end()),
                    column,
                    self.text[start..end].chars().count().max(1),
                )
            }
            None => (HIDDEN_LINE.to_string(), 1, HIDDEN_LINE.len()),
        };
        let gutter = " ".repeat(line.to_string().len());
        let snippet = format!(
            "{gutter} |\n{line} | {shown}\n{gutter} | {}{}",
            " ".repeat(caret_column - 1),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn located(error: AlienError<ErrorData>) -> (usize, usize, String) {
        match error.error.expect("error data") {
            ErrorData::InvalidDocument {
                line,
                column,
                snippet,
                ..
            } => (line, column, snippet),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn a_span_inside_a_carriage_return_line_ending_does_not_panic() {
        let file = SourceFile {
            name: "alien.toml".to_string(),
            text: "a = 1\r\r\n",
        };
        // The span starts on the stray `\r`, past the end of the trimmed line text.
        let (line, column, snippet) = located(file.error(5..7, "bad"));
        assert_eq!((line, column), (1, 6));
        assert_eq!(snippet, "  |\n1 | a = 1\n  |      ^");
    }

    #[test]
    fn hiding_values_shows_the_key_only() {
        let file = SourceFile {
            name: "app.env".to_string(),
            text: "A=1\nTOKEN = s3cret-value\n",
        };
        let value_start = "A=1\nTOKEN = ".len();
        let (line, column, snippet) =
            located(file.error_hiding_values(value_start..value_start + 12, "bad value"));
        assert_eq!((line, column), (2, 9));
        assert_eq!(
            snippet,
            "  |\n2 | TOKEN=<value hidden>\n  |       ^^^^^^^^^^^^^^"
        );
        assert!(!snippet.contains("s3cret"));

        let key_start = "A=1\n".len();
        let (line, column, snippet) =
            located(file.error_hiding_values(key_start..key_start + 5, "bad key"));
        assert_eq!((line, column), (2, 1));
        assert_eq!(snippet, "  |\n2 | TOKEN=<value hidden>\n  | ^^^^^");
    }

    #[test]
    fn hiding_values_hides_a_line_without_a_key() {
        let file = SourceFile {
            name: "app.env".to_string(),
            text: "A=1\nsupersecrettoken\n",
        };
        let (line, column, snippet) = located(file.error_hiding_values(4..20, "bad line"));
        assert_eq!((line, column), (2, 1));
        assert_eq!(snippet, "  |\n2 | <line hidden>\n  | ^^^^^^^^^^^^^");
        assert!(!snippet.contains("secret"));
    }
}
