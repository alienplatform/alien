use alien_error::AlienErrorData;
use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};

/// Infrastructure and startup errors that benefit from Alien's retry and
/// internal-detail metadata.
#[derive(Debug, Clone, AlienErrorData, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorData {
    /// The process configuration is incomplete or invalid.
    #[error(
        code = "CONFIGURATION_INVALID",
        message = "Invalid configuration: {message}",
        retryable = "false",
        internal = "false"
    )]
    ConfigurationInvalid {
        /// Configuration failure.
        message: String,
    },

    /// An Alien binding operation failed.
    #[error(
        code = "BINDING_OPERATION_FAILED",
        message = "Binding operation '{operation}' failed",
        retryable = "inherit",
        internal = "true"
    )]
    BindingOperationFailed {
        /// Operation being performed.
        operation: String,
    },

    /// An object-storage operation failed.
    #[error(
        code = "STORAGE_OPERATION_FAILED",
        message = "Storage operation '{operation}' failed for '{path}'",
        retryable = "inherit",
        internal = "true"
    )]
    StorageOperationFailed {
        /// Operation being performed.
        operation: String,
        /// Object path involved.
        path: String,
    },

    /// A queue operation failed.
    #[error(
        code = "QUEUE_OPERATION_FAILED",
        message = "Queue operation '{operation}' failed",
        retryable = "inherit",
        internal = "true"
    )]
    QueueOperationFailed {
        /// Operation being performed.
        operation: String,
    },

    /// A SlateDB operation failed.
    #[error(
        code = "DATABASE_OPERATION_FAILED",
        message = "Database operation '{operation}' failed",
        retryable = "inherit",
        internal = "true"
    )]
    DatabaseOperationFailed {
        /// Operation being performed.
        operation: String,
    },

    /// JSON encoding or decoding failed inside the service.
    #[error(
        code = "SERIALIZATION_FAILED",
        message = "Serialization operation '{operation}' failed",
        retryable = "false",
        internal = "true"
    )]
    SerializationFailed {
        /// Operation being performed.
        operation: String,
    },
}

pub type Error = alien_error::AlienError<ErrorData>;
pub type Result<T> = alien_error::Result<T, ErrorData>;

/// Small, application-owned errors for expected HTTP outcomes.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiError {
    pub code: &'static str,
    pub message: String,
    #[serde(skip)]
    pub status: StatusCode,
}

impl ApiError {
    pub fn invalid(message: impl Into<String>) -> Self {
        Self {
            code: "TRACE_INVALID",
            message: message.into(),
            status: StatusCode::BAD_REQUEST,
        }
    }

    pub fn not_found(trace_id: &str) -> Self {
        Self {
            code: "TRACE_NOT_FOUND",
            message: format!("Trace '{trace_id}' was not found"),
            status: StatusCode::NOT_FOUND,
        }
    }

    pub fn conflict(trace_id: &str) -> Self {
        Self {
            code: "TRACE_CONFLICT",
            message: format!("Trace '{trace_id}' already exists with different content"),
            status: StatusCode::CONFLICT,
        }
    }

    pub fn invalid_cursor() -> Self {
        Self {
            code: "CURSOR_INVALID",
            message: "Invalid pagination cursor".to_string(),
            status: StatusCode::BAD_REQUEST,
        }
    }

    pub fn not_ready() -> Self {
        Self {
            code: "TRACE_STORE_NOT_READY",
            message: "The trace store is starting; try again shortly".to_string(),
            status: StatusCode::SERVICE_UNAVAILABLE,
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(self)).into_response()
    }
}

#[derive(Debug)]
pub enum AppError {
    Api(ApiError),
    Internal(Error),
}

impl From<ApiError> for AppError {
    fn from(error: ApiError) -> Self {
        Self::Api(error)
    }
}

impl From<Error> for AppError {
    fn from(error: Error) -> Self {
        Self::Internal(error)
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        match self {
            Self::Api(error) => error.into_response(),
            Self::Internal(error) => error.into_response(),
        }
    }
}

pub type AppResult<T> = std::result::Result<T, AppError>;
