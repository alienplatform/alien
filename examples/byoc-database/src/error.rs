use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use thiserror::Error;

/// Error types for BYOC database
#[derive(Debug, Error)]
pub enum Error {
    #[error("Storage operation failed: {0}")]
    Storage(String),

    #[error("Namespace '{0}' not found")]
    NamespaceNotFound(String),

    #[error("Invalid vector: {0}")]
    InvalidVector(String),

    #[error("Serialization failed: {0}")]
    Serialization(String),

    #[error("Configuration error: {0}")]
    Configuration(String),

    #[error("{0}")]
    Generic(String),
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            Error::Storage(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg.as_str()),
            Error::NamespaceNotFound(msg) => (StatusCode::NOT_FOUND, msg.as_str()),
            Error::InvalidVector(msg) => (StatusCode::BAD_REQUEST, msg.as_str()),
            Error::Serialization(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg.as_str()),
            Error::Configuration(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg.as_str()),
            Error::Generic(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg.as_str()),
        };

        let body = Json(json!({
            "error": message,
        }));

        (status, body).into_response()
    }
}

pub type Result<T> = std::result::Result<T, Error>;
