use crate::error::{ErrorData, Result};
use alien_error::{AlienError, Context, IntoAlienError};
use console::{style, Key, Term};
use serde::Serialize;
use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::Path;

pub fn can_prompt() -> bool {
    io::stdin().is_terminal() && io::stdout().is_terminal()
}

pub fn print_json<T: Serialize>(value: &T) -> Result<()> {
    let json = serde_json::to_string_pretty(value)
        .into_alien_error()
        .context(ErrorData::JsonError {
            operation: "serialize".to_string(),
            reason: "Failed to serialize JSON output".to_string(),
        })?;
    println!("{json}");
    Ok(())
}

pub fn write_json_file<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let parent = path.parent().ok_or_else(|| {
        AlienError::new(ErrorData::FileOperationFailed {
            operation: "resolve parent".to_string(),
            file_path: path.display().to_string(),
            reason: "Status file path has no parent directory".to_string(),
        })
    })?;

    fs::create_dir_all(parent)
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "create directory".to_string(),
            file_path: parent.display().to_string(),
            reason: "Failed to create output directory".to_string(),
        })?;

    let contents = serde_json::to_vec_pretty(value)
        .into_alien_error()
        .context(ErrorData::JsonError {
            operation: "serialize".to_string(),
            reason: "Failed to serialize JSON file contents".to_string(),
        })?;

    let temp_path = path.with_extension("tmp");
    fs::write(&temp_path, contents)
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "write".to_string(),
            file_path: temp_path.display().to_string(),
            reason: "Failed to write temporary JSON file".to_string(),
        })?;

    fs::rename(&temp_path, path)
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "rename".to_string(),
            file_path: path.display().to_string(),
            reason: "Failed to move temporary JSON file into place".to_string(),
        })?;

    Ok(())
}

pub fn prompt_confirm(prompt: &str, default_yes: bool) -> Result<bool> {
    let suffix = if default_yes { "[Y/n]" } else { "[y/N]" };
    print!("{prompt} {suffix} ");
    io::stdout()
        .flush()
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "flush".to_string(),
            file_path: "stdout".to_string(),
            reason: "Failed to flush prompt".to_string(),
        })?;

    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "read".to_string(),
            file_path: "stdin".to_string(),
            reason: "Failed to read user input".to_string(),
        })?;

    Ok(parse_confirm_response(&input, default_yes))
}

pub fn prompt_select(prompt: &str, choices: &[String]) -> Result<String> {
    let index = prompt_select_index(prompt, choices)?;
    Ok(choices[index].clone())
}

/// Interactive select with arrow-key navigation. Returns the index of the selected item.
///
/// Shows a scrollable viewport when there are more items than `MAX_VISIBLE`.
pub fn prompt_select_index(prompt: &str, items: &[String]) -> Result<usize> {
    const MAX_VISIBLE: usize = 15;

    let term = Term::stdout();
    let mut cursor: usize = 0;
    let mut offset: usize = 0;
    let page_size = items.len().min(MAX_VISIBLE);
    let needs_scroll = items.len() > page_size;

    term.hide_cursor()
        .into_alien_error()
        .context(prompt_io_context())?;

    let prompt_line = format!("{} {}", style("?").cyan().bold(), style(prompt).bold());
    term.write_line(&prompt_line)
        .into_alien_error()
        .context(prompt_io_context())?;

    let visible_lines = page_size + if needs_scroll { 1 } else { 0 };
    write_select_page(&term, items, cursor, offset, page_size, needs_scroll)?;

    let result = loop {
        match term
            .read_key()
            .into_alien_error()
            .context(prompt_io_context())?
        {
            Key::ArrowUp | Key::Char('k') => {
                if cursor > 0 {
                    cursor -= 1;
                    if cursor < offset {
                        offset = cursor;
                    }
                    term.clear_last_lines(visible_lines)
                        .into_alien_error()
                        .context(prompt_io_context())?;
                    write_select_page(&term, items, cursor, offset, page_size, needs_scroll)?;
                }
            }
            Key::ArrowDown | Key::Char('j') => {
                if cursor < items.len() - 1 {
                    cursor += 1;
                    if cursor >= offset + page_size {
                        offset = cursor - page_size + 1;
                    }
                    term.clear_last_lines(visible_lines)
                        .into_alien_error()
                        .context(prompt_io_context())?;
                    write_select_page(&term, items, cursor, offset, page_size, needs_scroll)?;
                }
            }
            Key::Enter => {
                term.clear_last_lines(visible_lines + 1)
                    .into_alien_error()
                    .context(prompt_io_context())?;
                break Ok(cursor);
            }
            Key::Escape => {
                term.clear_last_lines(visible_lines + 1)
                    .into_alien_error()
                    .context(prompt_io_context())?;
                break Err(AlienError::new(ErrorData::UserCancelled));
            }
            _ => {}
        }
    };

    let _ = term.show_cursor();
    result
}

fn write_select_page(
    term: &Term,
    items: &[String],
    cursor: usize,
    offset: usize,
    page_size: usize,
    needs_scroll: bool,
) -> Result<()> {
    let end = (offset + page_size).min(items.len());
    for i in offset..end {
        let line = if i == cursor {
            format!("{} {}", style("❯").cyan().bold(), style(&items[i]).cyan())
        } else {
            format!("  {}", &items[i])
        };
        term.write_line(&line)
            .into_alien_error()
            .context(prompt_io_context())?;
    }
    if needs_scroll {
        term.write_line(
            &style("(Use arrow keys to reveal more choices)")
                .dim()
                .to_string(),
        )
        .into_alien_error()
        .context(prompt_io_context())?;
    }
    Ok(())
}

fn prompt_io_context() -> ErrorData {
    ErrorData::FileOperationFailed {
        operation: "prompt".to_string(),
        file_path: "terminal".to_string(),
        reason: "Terminal I/O failed during interactive prompt".to_string(),
    }
}

pub fn prompt_text(prompt: &str, default_value: Option<&str>) -> Result<String> {
    match default_value {
        Some(default_value) => {
            print!("{prompt} [{default_value}]: ");
        }
        None => {
            print!("{prompt}: ");
        }
    }

    io::stdout()
        .flush()
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "flush".to_string(),
            file_path: "stdout".to_string(),
            reason: "Failed to flush prompt".to_string(),
        })?;

    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "read".to_string(),
            file_path: "stdin".to_string(),
            reason: "Failed to read user input".to_string(),
        })?;

    Ok(parse_text_response(&input, default_value))
}

pub(crate) fn parse_confirm_response(input: &str, default_yes: bool) -> bool {
    let value = input.trim().to_ascii_lowercase();
    if value.is_empty() {
        return default_yes;
    }

    matches!(value.as_str(), "y" | "yes")
}

pub(crate) fn parse_text_response(input: &str, default_value: Option<&str>) -> String {
    let value = input.trim();
    if value.is_empty() {
        return default_value.unwrap_or_default().to_string();
    }

    value.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Serialize;
    use tempfile::TempDir;

    #[derive(Serialize)]
    struct TestPayload {
        hello: &'static str,
    }

    #[test]
    fn parse_confirm_response_honors_default() {
        assert!(parse_confirm_response("", true));
        assert!(!parse_confirm_response("", false));
        assert!(parse_confirm_response("yes", false));
        assert!(!parse_confirm_response("n", true));
    }

    #[test]
    fn parse_text_response_uses_default() {
        assert_eq!(parse_text_response("", Some("default")), "default");
        assert_eq!(parse_text_response(" value ", None), "value");
    }

    #[test]
    fn write_json_file_creates_parent_and_replaces_existing_file() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("nested").join("status.json");

        write_json_file(&path, &TestPayload { hello: "world" }).unwrap();
        assert!(path.exists());
        assert!(!path.with_extension("tmp").exists());

        write_json_file(&path, &TestPayload { hello: "again" }).unwrap();

        let contents = fs::read_to_string(&path).unwrap();
        assert!(contents.contains("\"hello\": \"again\""));
    }
}
