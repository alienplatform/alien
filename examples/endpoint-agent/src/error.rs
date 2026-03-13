use thiserror::Error;

/// Error types for endpoint agent
#[derive(Debug, Error)]
pub enum Error {
    #[error("Database operation failed: {0}")]
    Database(String),

    #[error("Monitoring operation failed: {0}")]
    Monitoring(String),

    #[error("Invalid duration format: {0}")]
    InvalidDuration(String),

    #[error("Invalid path: {0}")]
    InvalidPath(String),

    #[error("{0}")]
    Generic(String),
}

pub type Result<T> = std::result::Result<T, Error>;
