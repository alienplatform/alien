//! `env-file`: environment variables kept in a plain `KEY=value` file.
//!
//! The format is deliberately small:
//! - one `KEY=value` per line; blank lines and lines starting with `#` are ignored
//! - whitespace around the key and the value is trimmed
//! - a value in double quotes may use `\"`, `\\` and `\n`; a value in single quotes is literal
//! - anything else is taken verbatim, including `#` (there are no inline comments)

use indexmap::IndexMap;

use crate::error::Result;
use crate::source::SourceFile;

/// Parses an env file, in file order. Errors point at the offending line of the file.
pub fn parse(file: &SourceFile<'_>) -> Result<IndexMap<String, String>> {
    let mut variables = IndexMap::new();
    let mut offset = 0;
    for raw_line in file.text.split_inclusive('\n') {
        let line_offset = offset;
        offset += raw_line.len();
        let line = raw_line.trim_end_matches(['\n', '\r']);
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let indent = line.len() - trimmed.len();
        let span_of = |start: usize, len: usize| {
            line_offset + indent + start..line_offset + indent + start + len
        };

        let Some((key, value)) = trimmed.split_once('=') else {
            return Err(file.error_hiding_values(
                span_of(0, trimmed.len()),
                "expected KEY=value; comments start with #",
            ));
        };
        let key = key.trim_end();
        if !is_variable_name(key) {
            return Err(file.error_hiding_values(
                span_of(0, key.len()),
                format!("'{key}' is not a variable name; use letters, digits and underscores, not starting with a digit"),
            ));
        }
        let value_start = trimmed.len() - value.len();
        let value_text = value.trim();
        let leading = value.len() - value.trim_start().len();
        let value = unquote(value_text).map_err(|reason| {
            file.error_hiding_values(span_of(value_start + leading, value_text.len()), reason)
        })?;
        if variables.insert(key.to_string(), value).is_some() {
            return Err(file.error_hiding_values(
                span_of(0, key.len()),
                format!("'{key}' is set more than once"),
            ));
        }
    }
    Ok(variables)
}

fn is_variable_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn unquote(value: &str) -> std::result::Result<String, String> {
    if let Some(inner) = value.strip_prefix('\'') {
        return inner
            .strip_suffix('\'')
            .filter(|inner| !inner.contains('\''))
            .map(str::to_string)
            .ok_or_else(|| "unterminated single-quoted value".to_string());
    }
    let Some(inner) = value.strip_prefix('"') else {
        return Ok(value.to_string());
    };

    let mut result = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                return if chars.as_str().is_empty() {
                    Ok(result)
                } else {
                    Err("unexpected text after the closing quote".to_string())
                };
            }
            '\\' => match chars.next() {
                Some('"') => result.push('"'),
                Some('\\') => result.push('\\'),
                Some('n') => result.push('\n'),
                Some(_) => return Err("unknown escape sequence; use \\\", \\\\ or \\n".to_string()),
                None => break,
            },
            c => result.push(c),
        }
    }
    Err("unterminated double-quoted value".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorData;

    fn parse_text(text: &str) -> Result<IndexMap<String, String>> {
        parse(&SourceFile {
            name: "app.env".to_string(),
            text,
        })
    }

    #[test]
    fn reads_variables_in_order() {
        let variables = parse_text(
            "# settings\n\nRUST_LOG=info\n  PORT = 8080 \nGREETING=\"hello \\\"you\\\"\\nbye\"\nRAW='a \"b\" #c'\nURL=http://x/#frag\nEMPTY=\n",
        )
        .unwrap();
        let expected: Vec<(&str, &str)> = vec![
            ("RUST_LOG", "info"),
            ("PORT", "8080"),
            ("GREETING", "hello \"you\"\nbye"),
            ("RAW", "a \"b\" #c"),
            ("URL", "http://x/#frag"),
            ("EMPTY", ""),
        ];
        let actual: Vec<(&str, &str)> = variables
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        assert_eq!(actual, expected);
    }

    fn location(error: alien_error::AlienError<ErrorData>) -> (usize, usize, String) {
        match error.error.unwrap() {
            ErrorData::InvalidDocument {
                line,
                column,
                message,
                ..
            } => (line, column, message),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn errors_point_at_the_line() {
        let cases = [
            ("A=1\nnot a pair\n", 2, 1, "expected KEY=value"),
            ("A=1\n  9X=1\n", 2, 3, "'9X' is not a variable name"),
            (
                "A=1\nB = \"open\n",
                2,
                5,
                "unterminated double-quoted value",
            ),
            ("A=1\nB='x\n", 2, 3, "unterminated single-quoted value"),
            (
                "A=\"x\" y\n",
                1,
                3,
                "unexpected text after the closing quote",
            ),
            ("A=\"\\t\"\n", 1, 3, "unknown escape"),
            ("A=1\nA=2\n", 2, 1, "'A' is set more than once"),
        ];
        for (text, line, column, message) in cases {
            let (actual_line, actual_column, actual_message) =
                location(parse_text(text).unwrap_err());
            assert_eq!((actual_line, actual_column), (line, column), "{text:?}");
            assert!(
                actual_message.contains(message),
                "{text:?}: {actual_message}"
            );
        }
    }
}
