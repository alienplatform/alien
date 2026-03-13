//! Alien-Error – minimal clean version with context-based API.
//! Provides:
//!   • `AlienErrorMetadata` trait (implemented by enums via #[derive(AlienError)])
//!   • `AlienError<T>` container (generic over error type)
//!   • `.context()` extension method for AlienError Results
//!   • `.into_alien_error()` for converting std errors
//!   • `Result<T>` alias
//!   • OpenAPI schema generation (with `openapi` feature)
//!   • Axum IntoResponse implementation (with `axum` feature)
//!
//! Use `.context(YourError::Variant { ... })` on AlienError Results to wrap errors.
//! Use `.into_alien_error()` on std::error::Error Results to convert them first.
//!
//! ## OpenAPI Schema Generation
//!
//! When the `openapi` feature is enabled, the `AlienError` struct implements
//! `utoipa::ToSchema`, allowing it to be used in OpenAPI documentation:
//!
//! ```rust,ignore
//! use utoipa::OpenApi;
//! use alien_error::AlienError;
//!
//! #[derive(OpenApi)]
//! #[openapi(components(schemas(AlienError)))]
//! struct ApiDoc;
//! ```
//!
//! ## Axum Integration
//!
//! When the `axum` feature is enabled, `AlienError` implements `axum::response::IntoResponse`,
//! allowing it to be returned directly from Axum handlers. By default, the `IntoResponse`
//! implementation uses external response behavior (sanitizes internal errors).
//!
//! For different use cases, you can choose between:
//!
//! ### External API Responses (Default)
//! ```rust,ignore
//! use axum::response::IntoResponse;
//! use alien_error::{AlienError, AlienErrorData};
//!
//! // Default behavior - sanitizes internal errors for security
//! async fn api_handler() -> Result<String, AlienError<MyError>> {
//!     Err(AlienError::new(MyError::InternalDatabaseError {
//!         credentials: "secret".to_string()
//!     }))
//! }
//! // Returns: HTTP 500 with {"code": "GENERIC_ERROR", "message": "Internal server error"}
//! ```
//!
//! ### Explicit External Responses
//! ```rust,ignore
//! async fn api_handler() -> impl IntoResponse {
//!     let error = AlienError::new(MyError::InternalDatabaseError {
//!         credentials: "secret".to_string()
//!     });
//!     error.into_external_response() // Explicitly sanitize
//! }
//! ```
//!
//! ### Internal Service Communication
//! ```rust,ignore
//! async fn internal_handler() -> impl IntoResponse {
//!     let error = AlienError::new(MyError::InternalDatabaseError {
//!         credentials: "secret".to_string()
//!     });
//!     error.into_internal_response() // Preserve all details
//! }
//! // Returns: HTTP 500 with full error details including sensitive information
//! ```

use std::{error::Error as StdError, fmt};

use serde::{Deserialize, Serialize};

/// Data every public-facing error variant must expose.
pub trait AlienErrorData {
    /// Short machine-readable identifier ("NOT_FOUND", "TIMEOUT", …).
    fn code(&self) -> &'static str;
    /// Whether the failing operation can be retried.
    fn retryable(&self) -> bool;
    /// Whether the error is internal (should not be shown to end users).
    fn internal(&self) -> bool;
    /// Human-readable message (defaults to `Display`).
    fn message(&self) -> String;
    /// HTTP status code for this error (defaults to 500).
    fn http_status_code(&self) -> u16 {
        500
    }
    /// Optional diagnostic payload built from struct/enum fields.
    fn context(&self) -> Option<serde_json::Value> {
        None
    }

    /// Whether to inherit the retryable flag from the source error.
    /// Returns None if this error should inherit from source, Some(value) for explicit value.
    fn retryable_inherit(&self) -> Option<bool> {
        Some(self.retryable())
    }

    /// Whether to inherit the internal flag from the source error.
    /// Returns None if this error should inherit from source, Some(value) for explicit value.
    fn internal_inherit(&self) -> Option<bool> {
        Some(self.internal())
    }

    /// Whether to inherit the HTTP status code from the source error.
    /// Returns None if this error should inherit from source, Some(value) for explicit value.
    fn http_status_code_inherit(&self) -> Option<u16> {
        Some(self.http_status_code())
    }
}

/// A special marker type for generic/standard errors that don't have specific metadata
#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq, Default)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct GenericError {
    pub message: String,
}

impl std::fmt::Display for GenericError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl StdError for GenericError {}

impl AlienErrorData for GenericError {
    fn code(&self) -> &'static str {
        "GENERIC_ERROR"
    }

    fn retryable(&self) -> bool {
        false
    }

    fn internal(&self) -> bool {
        false
    }

    fn message(&self) -> String {
        self.message.clone()
    }

    fn http_status_code(&self) -> u16 {
        500
    }
}

/// Canonical error container that provides a structured way to represent errors
/// with rich metadata including error codes, human-readable messages, context,
/// and chaining capabilities for error propagation.
///
/// This struct is designed to be both machine-readable and user-friendly,
/// supporting serialization for API responses and detailed error reporting
/// in distributed systems.
#[derive(Debug, Serialize, Deserialize, Clone, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AlienError<T = GenericError>
where
    T: AlienErrorData + Clone + std::fmt::Debug + Serialize,
{
    /// A unique identifier for the type of error.
    ///
    /// This should be a short, machine-readable string that can be used
    /// by clients to programmatically handle different error types.
    /// Examples: "NOT_FOUND", "VALIDATION_ERROR", "TIMEOUT"
    #[cfg_attr(feature = "openapi", schema(example = "NOT_FOUND", max_length = 128))]
    pub code: String,

    /// Human-readable error message.
    ///
    /// This message should be clear and actionable for developers or end-users,
    /// providing context about what went wrong and potentially how to fix it.
    #[cfg_attr(
        feature = "openapi",
        schema(example = "Item not found.", max_length = 16384)
    )]
    pub message: String,

    /// Additional diagnostic information about the error context.
    ///
    /// This optional field can contain structured data providing more details
    /// about the error, such as validation errors, request parameters that
    /// caused the issue, or other relevant context information.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "openapi", schema(nullable = true))]
    pub context: Option<serde_json::Value>,

    /// Indicates whether the operation that caused the error should be retried.
    ///
    /// When `true`, the error is transient and the operation might succeed
    /// if attempted again. When `false`, retrying the same operation is
    /// unlikely to succeed without changes.
    #[cfg_attr(feature = "openapi", schema(default = false))]
    pub retryable: bool,

    /// Indicates if this is an internal error that should not be exposed to users.
    ///
    /// When `true`, this error contains sensitive information or implementation
    /// details that should not be shown to end-users. Such errors should be
    /// logged for debugging but replaced with generic error messages in responses.
    pub internal: bool,

    /// HTTP status code for this error.
    ///
    /// Used when converting the error to an HTTP response. If None, falls back to
    /// the error type's default status code or 500.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "openapi", schema(minimum = 100, maximum = 599))]
    pub http_status_code: Option<u16>,

    /// The underlying error that caused this error, creating an error chain.
    ///
    /// This allows for proper error propagation and debugging by maintaining
    /// the full context of how an error occurred through multiple layers
    /// of an application.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "openapi", schema(value_type = Option<serde_json::Value>))]
    pub source: Option<Box<AlienError<GenericError>>>,

    /// The original error for pattern matching
    #[serde(
        rename = "_error_for_pattern_matching",
        skip_serializing_if = "Option::is_none"
    )]
    #[cfg_attr(feature = "openapi", schema(ignore))]
    pub error: Option<T>,
}

impl<T> AlienError<T>
where
    T: AlienErrorData + Clone + std::fmt::Debug + Serialize,
{
    /// Create an AlienError from an AlienErrorData implementor
    pub fn new(meta: T) -> Self {
        AlienError {
            code: meta.code().to_string(),
            message: meta.message(),
            context: meta.context(),
            retryable: meta.retryable(),
            internal: meta.internal(),
            http_status_code: Some(meta.http_status_code()),
            source: None,
            error: Some(meta),
        }
    }
}

impl AlienError<GenericError> {
    /// Create an AlienError from a standard error
    pub fn from_std(err: &(dyn StdError + 'static)) -> Self {
        let generic = GenericError {
            message: err.to_string(),
        };

        // Recursively build the source chain
        let source = err.source().map(|src| Box::new(Self::from_std(src)));

        AlienError {
            code: generic.code().to_string(),
            message: generic.message(),
            context: generic.context(),
            retryable: generic.retryable(),
            internal: generic.internal(),
            http_status_code: Some(generic.http_status_code()),
            source,
            error: Some(generic),
        }
    }
}

impl<T> fmt::Display for AlienError<T>
where
    T: AlienErrorData + Clone + std::fmt::Debug + Serialize,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)?;
        fn recurse(
            e: &AlienError<GenericError>,
            indent: &str,
            f: &mut fmt::Formatter<'_>,
        ) -> fmt::Result {
            writeln!(f, "{}├─▶ {}: {}", indent, e.code, e.message)?;
            if let Some(ref src) = e.source {
                recurse(src, &format!("{}│   ", indent), f)?;
            }
            Ok(())
        }
        if let Some(ref src) = self.source {
            writeln!(f)?;
            recurse(src, "", f)?;
        }
        Ok(())
    }
}

impl<T> StdError for AlienError<T>
where
    T: AlienErrorData + Clone + std::fmt::Debug + Serialize,
{
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        self.source
            .as_ref()
            .map(|e| e.as_ref() as &(dyn StdError + 'static))
    }
}

/// Extension trait for adding context to AlienError Results
pub trait Context<T, E> {
    /// Add context to an AlienError result, wrapping it with a new error
    fn context<M: AlienErrorData + Clone + std::fmt::Debug + Serialize>(
        self,
        meta: M,
    ) -> std::result::Result<T, AlienError<M>>;
}

// Implementation for AlienError results
impl<T, E> Context<T, E> for std::result::Result<T, AlienError<E>>
where
    E: AlienErrorData + Clone + std::fmt::Debug + Serialize + 'static + Send + Sync,
{
    fn context<M: AlienErrorData + Clone + std::fmt::Debug + Serialize>(
        self,
        meta: M,
    ) -> std::result::Result<T, AlienError<M>> {
        self.map_err(|err| {
            let mut new_err = AlienError::new(meta.clone());

            // Check for inheritance and apply source error properties
            // SAFETY: err.retryable, err.internal, and err.http_status_code are always valid
            // as they are primitive types (bool, Option<u16>) that cannot be in an invalid state
            if meta.retryable_inherit().is_none() {
                new_err.retryable = err.retryable;
            }
            if meta.internal_inherit().is_none() {
                new_err.internal = err.internal;
            }
            if meta.http_status_code_inherit().is_none() {
                new_err.http_status_code = err.http_status_code;
            }

            // Convert the original typed error to a generic error to maintain the chain
            let generic_err = AlienError {
                code: err.code.clone(),
                message: err.message.clone(),
                context: err.context.clone(),
                retryable: err.retryable,
                internal: err.internal,
                source: err.source,
                error: None,
                http_status_code: err.http_status_code,
            };
            new_err.source = Some(Box::new(generic_err));
            new_err
        })
    }
}

/// Extension trait for adding context directly to AlienError instances
pub trait ContextError<E> {
    /// Add context to an AlienError, wrapping it with a new error
    fn context<M: AlienErrorData + Clone + std::fmt::Debug + Serialize>(
        self,
        meta: M,
    ) -> AlienError<M>;
}

// Implementation for AlienError instances
impl<E> ContextError<E> for AlienError<E>
where
    E: AlienErrorData + Clone + std::fmt::Debug + Serialize + 'static + Send + Sync,
{
    fn context<M: AlienErrorData + Clone + std::fmt::Debug + Serialize>(
        self,
        meta: M,
    ) -> AlienError<M> {
        let mut new_err = AlienError::new(meta.clone());

        // Check for inheritance and apply source error properties
        // SAFETY: self.retryable, self.internal, and self.http_status_code are always valid
        // as they are primitive types (bool, Option<u16>) that cannot be in an invalid state
        if meta.retryable_inherit().is_none() {
            new_err.retryable = self.retryable;
        }
        if meta.internal_inherit().is_none() {
            new_err.internal = self.internal;
        }
        if meta.http_status_code_inherit().is_none() {
            new_err.http_status_code = self.http_status_code;
        }

        // Convert the original typed error to a generic error to maintain the chain
        let generic_err = AlienError {
            code: self.code.clone(),
            message: self.message.clone(),
            context: self.context.clone(),
            retryable: self.retryable,
            internal: self.internal,
            source: self.source,
            error: None,
            http_status_code: self.http_status_code,
        };
        new_err.source = Some(Box::new(generic_err));
        new_err
    }
}

/// Extension trait for converting standard errors to AlienError
pub trait IntoAlienError<T> {
    /// Convert a standard error result into an AlienError result
    fn into_alien_error(self) -> std::result::Result<T, AlienError<GenericError>>;
}

impl<T, E> IntoAlienError<T> for std::result::Result<T, E>
where
    E: StdError + 'static,
{
    fn into_alien_error(self) -> std::result::Result<T, AlienError<GenericError>> {
        self.map_err(|err| AlienError::from_std(&err as &dyn StdError))
    }
}

/// Extension trait for converting standard errors directly to AlienError
pub trait IntoAlienErrorDirect {
    /// Convert a standard error into an AlienError
    fn into_alien_error(self) -> AlienError<GenericError>;
}

impl<E> IntoAlienErrorDirect for E
where
    E: StdError + 'static,
{
    fn into_alien_error(self) -> AlienError<GenericError> {
        AlienError::from_std(&self as &dyn StdError)
    }
}

/// Alias for the common `Result` type used throughout an application.
/// This is now generic over the error type for better type safety.
pub type Result<T, E = GenericError> = std::result::Result<T, AlienError<E>>;

impl<T> AlienError<T>
where
    T: AlienErrorData + Clone + std::fmt::Debug + Serialize,
{
    /// Convert this AlienError<T> to AlienError<GenericError> without losing data
    pub fn into_generic(self) -> AlienError<GenericError> {
        AlienError {
            code: self.code,
            message: self.message,
            context: self.context,
            retryable: self.retryable,
            internal: self.internal,
            source: self.source,
            error: None,
            http_status_code: self.http_status_code,
        }
    }
}

// Re-export the derive macro so users only depend on this crate.
pub use alien_error_derive::AlienErrorData;

// Conversions for anyhow interoperability
#[cfg(feature = "anyhow")]
impl From<anyhow::Error> for AlienError<GenericError> {
    fn from(err: anyhow::Error) -> AlienError<GenericError> {
        AlienError::new(GenericError {
            message: err.to_string(),
        })
    }
}

#[cfg(feature = "anyhow")]
pub trait IntoAnyhow<T> {
    /// Convert an AlienError result into an anyhow result
    fn into_anyhow(self) -> anyhow::Result<T>;
}

#[cfg(feature = "anyhow")]
impl<T, E> IntoAnyhow<T> for std::result::Result<T, AlienError<E>>
where
    E: AlienErrorData + Clone + std::fmt::Debug + Serialize + Send + Sync + 'static,
{
    fn into_anyhow(self) -> anyhow::Result<T> {
        self.map_err(|err| anyhow::Error::new(err))
    }
}

// Axum IntoResponse implementation
#[cfg(feature = "axum")]
impl<T> axum::response::IntoResponse for AlienError<T>
where
    T: AlienErrorData + Clone + std::fmt::Debug + Serialize + Send + Sync + 'static,
{
    fn into_response(self) -> axum::response::Response {
        // Default behavior: external response (sanitizes internal errors)
        self.into_external_response()
    }
}

#[cfg(feature = "axum")]
impl<T> AlienError<T>
where
    T: AlienErrorData + Clone + std::fmt::Debug + Serialize + Send + Sync + 'static,
{
    /// Convert to an Axum response suitable for internal microservice communication.
    /// Preserves all error details including sensitive information from internal errors.
    pub fn into_internal_response(self) -> axum::response::Response {
        use axum::http::StatusCode;
        use axum::response::{IntoResponse, Json};

        // For internal responses, preserve all error details regardless of internal flag
        let response_error = self.into_generic();

        // Convert HTTP status code to StatusCode
        let status_code = response_error
            .http_status_code
            .and_then(|code| StatusCode::from_u16(code).ok())
            .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);

        // Return JSON response with the error
        (status_code, Json(response_error)).into_response()
    }

    /// Convert to an Axum response suitable for external API responses.
    /// Sanitizes internal errors to prevent information leakage.
    pub fn into_external_response(self) -> axum::response::Response {
        use axum::http::StatusCode;
        use axum::response::{IntoResponse, Json};

        // For external responses, sanitize internal errors
        let response_error = if self.internal {
            // For internal errors, return a generic error message with 500 status code
            AlienError::new(GenericError {
                message: "Internal server error".to_string(),
            })
        } else {
            self.into_generic()
        };

        // Convert HTTP status code to StatusCode
        let status_code = response_error
            .http_status_code
            .and_then(|code| StatusCode::from_u16(code).ok())
            .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);

        // Return JSON response with the error
        (status_code, Json(response_error)).into_response()
    }
}
