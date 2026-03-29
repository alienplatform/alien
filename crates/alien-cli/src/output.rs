use crate::error::{ErrorData, Result};
use alien_error::{AlienError, Context, IntoAlienError};
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

    let value = input.trim().to_ascii_lowercase();
    if value.is_empty() {
        return Ok(default_yes);
    }

    Ok(matches!(value.as_str(), "y" | "yes"))
}

pub fn prompt_select(prompt: &str, choices: &[String]) -> Result<String> {
    println!("{prompt}");
    for (index, choice) in choices.iter().enumerate() {
        println!("  [{}] {}", index + 1, choice);
    }

    print!("Enter number: ");
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

    let index: usize =
        input
            .trim()
            .parse()
            .into_alien_error()
            .context(ErrorData::ValidationError {
                field: "selection".to_string(),
                message: "Expected a number".to_string(),
            })?;

    if index == 0 || index > choices.len() {
        return Err(AlienError::new(ErrorData::UserCancelled));
    }

    Ok(choices[index - 1].clone())
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

    let value = input.trim();
    if value.is_empty() {
        return Ok(default_value.unwrap_or_default().to_string());
    }

    Ok(value.to_string())
}
