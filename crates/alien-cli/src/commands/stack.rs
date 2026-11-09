use crate::error::{ErrorData, Result};
use crate::output::print_json;
use alien_error::Context;
use clap::{Parser, Subcommand};
use serde_json::Value;
use std::path::{Path, PathBuf};

#[derive(Parser, Debug, Clone)]
#[command(
    about = "Inspect an alien.toml stack",
    long_about = "Inspect an alien.toml stack without building or deploying anything.",
    after_help = "EXAMPLES:
    # Print the fully resolved stack of ./alien.toml
    alien stack expand

    # Pipe it into jq
    alien stack expand ./services/api | jq '.resources | keys'"
)]
pub struct StackArgs {
    #[command(subcommand)]
    pub action: StackAction,
}

#[derive(Subcommand, Debug, Clone)]
pub enum StackAction {
    /// Print the fully resolved stack of an alien.toml as JSON, with every default and
    /// expansion made visible. Nothing else is written to stdout.
    Expand {
        /// Path to an alien.toml, or a directory containing one. Defaults to the current
        /// directory.
        path: Option<PathBuf>,
    },
}

pub fn stack_task(args: StackArgs) -> Result<()> {
    match args.action {
        StackAction::Expand { path } => {
            let path = path.unwrap_or_else(|| PathBuf::from("."));
            print_json(&expand_stack(&path)?)
        }
    }
}

/// The resolved stack of the alien.toml at `path`, or of `<path>/alien.toml` when `path` is
/// a directory.
fn expand_stack(path: &Path) -> Result<Value> {
    let file = if path.is_dir() {
        path.join("alien.toml")
    } else {
        path.to_path_buf()
    };
    alien_stack_toml::expand(&file).context(ErrorData::ConfigurationError {
        message: format!("Invalid stack in {}", file.display()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_error::AlienError;

    const VALID: &str = r#"
name = "acme-app"
platforms = ["aws"]

[storage.data]
"#;

    fn write_toml(dir: &Path, text: &str) {
        std::fs::write(dir.join("alien.toml"), text).expect("write alien.toml");
    }

    fn rendered(error: &AlienError<ErrorData>) -> String {
        crate::ui::render_human_error(error)
    }

    #[test]
    fn expands_a_directory_and_a_file_to_the_same_stack() {
        let dir = tempfile::tempdir().unwrap();
        write_toml(dir.path(), VALID);

        let from_dir = expand_stack(dir.path()).expect("directory expands");
        let from_file = expand_stack(&dir.path().join("alien.toml")).expect("file expands");

        assert_eq!(from_dir, from_file);
        assert_eq!(from_dir["id"], "acme-app");
        assert_eq!(from_dir["resources"]["data"]["lifecycle"], "frozen");
    }

    #[test]
    fn invalid_toml_names_file_line_and_column() {
        let dir = tempfile::tempdir().unwrap();
        write_toml(dir.path(), "name = \"acme-app\"\nplatforms = [\"aws\"\n");

        let error = expand_stack(dir.path()).expect_err("unterminated array is invalid");

        let file = dir.path().join("alien.toml").display().to_string();
        let message = rendered(&error);
        assert!(
            message.contains(&format!("{file}:")),
            "error should name the file: {message}"
        );
        let located = message
            .split(&format!("{file}:"))
            .nth(1)
            .and_then(|rest| {
                let mut parts = rest.splitn(3, ':');
                let line = parts.next()?.parse::<usize>().ok()?;
                let column = parts.next()?.parse::<usize>().ok()?;
                Some((line, column))
            })
            .unwrap_or_else(|| panic!("error should read file:line:column: {message}"));
        assert!(located.0 >= 1 && located.1 >= 1, "{located:?}");
    }

    #[test]
    fn a_directory_without_alien_toml_is_an_error() {
        let dir = tempfile::tempdir().unwrap();

        let error = expand_stack(dir.path()).expect_err("no alien.toml");

        assert!(rendered(&error).contains("alien.toml"));
    }
}
