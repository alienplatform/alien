use alien_error::AlienErrorData;
use serde::{Deserialize, Serialize};

/// Errors loading an `alien.toml` stack.
#[derive(Debug, Clone, AlienErrorData, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorData {
    /// A value in alien.toml is malformed or does not resolve to a valid stack.
    #[error(
        code = "ALIEN_TOML_INVALID",
        message = "{file}:{line}:{column}: {message}\n{snippet}",
        retryable = "false",
        internal = "false",
        http_status_code = 400
    )]
    InvalidDocument {
        /// Path of the alien.toml file as given.
        file: String,
        /// 1-based line of the offending value.
        line: usize,
        /// 1-based column of the offending value, in characters.
        column: usize,
        /// What is wrong and how to fix it.
        message: String,
        /// The offending source line with the value underlined.
        snippet: String,
    },

    /// A file alien.toml refers to cannot be read.
    #[error(
        code = "ALIEN_TOML_FILE_UNREADABLE",
        message = "Cannot read {file}",
        retryable = "false",
        internal = "false",
        http_status_code = 400
    )]
    FileUnreadable {
        /// Path of the file that cannot be read.
        file: String,
    },
}

pub type Result<T> = alien_error::Result<T, ErrorData>;
