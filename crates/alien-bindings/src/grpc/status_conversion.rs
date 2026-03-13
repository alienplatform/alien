//! Common utilities for converting between AlienError and tonic::Status
//!
//! This module provides standardized conversion functions that:
//! - Map HTTP status codes to gRPC codes
//! - Serialize AlienError as JSON in Status details
//! - Support bidirectional conversion

use crate::error::ErrorData;
use alien_error::AlienError;
use bytes::Bytes;
use serde_json;
use tonic::{Code, Status};
use tracing::warn;

/// Convert an HTTP status code to the appropriate gRPC Code
pub fn http_status_to_grpc_code(http_status: u16) -> Code {
    match http_status {
        200..=299 => Code::Ok,
        400 => Code::InvalidArgument,
        401 => Code::Unauthenticated,
        403 => Code::PermissionDenied,
        404 => Code::NotFound,
        408 => Code::DeadlineExceeded,
        409 => Code::AlreadyExists,
        412 => Code::FailedPrecondition,
        429 => Code::ResourceExhausted,
        499 => Code::Cancelled,
        501 => Code::Unimplemented,
        502 => Code::Unavailable,
        503 => Code::Unavailable,
        504 => Code::DeadlineExceeded,
        _ if http_status >= 500 => Code::Internal,
        _ => Code::Unknown,
    }
}

/// Convert a gRPC Code to an appropriate HTTP status code
pub fn grpc_code_to_http_status(code: Code) -> u16 {
    match code {
        Code::Ok => 200,
        Code::Cancelled => 499,
        Code::Unknown => 500,
        Code::InvalidArgument => 400,
        Code::DeadlineExceeded => 504,
        Code::NotFound => 404,
        Code::AlreadyExists => 409,
        Code::PermissionDenied => 403,
        Code::ResourceExhausted => 429,
        Code::FailedPrecondition => 412,
        Code::Aborted => 409,
        Code::OutOfRange => 400,
        Code::Unimplemented => 501,
        Code::Internal => 500,
        Code::Unavailable => 503,
        Code::DataLoss => 500,
        Code::Unauthenticated => 401,
    }
}

/// Convert an AlienError to a tonic::Status
///
/// The AlienError is serialized as JSON and stored in the Status details field.
/// The HTTP status code from the AlienError is used to determine the gRPC code.
pub fn alien_error_to_status(err: AlienError<ErrorData>) -> Status {
    // Determine gRPC code from HTTP status
    let grpc_code = err
        .http_status_code
        .map(http_status_to_grpc_code)
        .unwrap_or(Code::Internal);

    // Serialize the entire AlienError as JSON for the details
    let details = match serde_json::to_vec(&err) {
        Ok(json_bytes) => Bytes::from(json_bytes),
        Err(e) => {
            warn!("Failed to serialize AlienError to JSON: {}", e);
            Bytes::new()
        }
    };

    Status::with_details(grpc_code, err.message.clone(), details)
}

/// Convert a tonic::Status to an AlienError
///
/// Deserializes the AlienError from the Status details field.
/// If deserialization fails or no details are present, returns a fallback error.
pub fn status_to_alien_error(status: Status, context: &str) -> AlienError<ErrorData> {
    // Try to deserialize from details
    if !status.details().is_empty() {
        match serde_json::from_slice::<AlienError<ErrorData>>(status.details()) {
            Ok(alien_error) => return alien_error,
            Err(e) => {
                warn!(
                    "Failed to deserialize AlienError from gRPC status details: {}",
                    e
                );
                // Fall through to create a fallback error
            }
        }
    }

    // If deserialization failed or no details, create a fallback error
    let http_status = grpc_code_to_http_status(status.code());
    let error_data = ErrorData::Other {
        message: format!(
            "gRPC error in {}: {} ({})",
            context,
            status.message(),
            status.code()
        ),
    };

    let mut alien_error = AlienError::new(error_data);
    alien_error.http_status_code = Some(http_status);
    alien_error
}

#[cfg(test)]
mod tests {
    use super::*;
    use tonic::Code;

    #[test]
    fn test_http_status_to_grpc_code() {
        assert_eq!(http_status_to_grpc_code(200), Code::Ok);
        assert_eq!(http_status_to_grpc_code(400), Code::InvalidArgument);
        assert_eq!(http_status_to_grpc_code(401), Code::Unauthenticated);
        assert_eq!(http_status_to_grpc_code(403), Code::PermissionDenied);
        assert_eq!(http_status_to_grpc_code(404), Code::NotFound);
        assert_eq!(http_status_to_grpc_code(409), Code::AlreadyExists);
        assert_eq!(http_status_to_grpc_code(500), Code::Internal);
        assert_eq!(http_status_to_grpc_code(503), Code::Unavailable);
    }

    #[test]
    fn test_grpc_code_to_http_status() {
        assert_eq!(grpc_code_to_http_status(Code::Ok), 200);
        assert_eq!(grpc_code_to_http_status(Code::InvalidArgument), 400);
        assert_eq!(grpc_code_to_http_status(Code::Unauthenticated), 401);
        assert_eq!(grpc_code_to_http_status(Code::PermissionDenied), 403);
        assert_eq!(grpc_code_to_http_status(Code::NotFound), 404);
        assert_eq!(grpc_code_to_http_status(Code::AlreadyExists), 409);
        assert_eq!(grpc_code_to_http_status(Code::Internal), 500);
        assert_eq!(grpc_code_to_http_status(Code::Unavailable), 503);
    }

    #[test]
    fn test_alien_error_to_status_roundtrip() {
        let original_error = AlienError::new(ErrorData::ResourceNotFound {
            resource_id: "test_resource".to_string(),
        });

        let status = alien_error_to_status(original_error.clone());
        let recovered_error = status_to_alien_error(status, "test_context");

        assert_eq!(original_error.code, recovered_error.code);
        assert_eq!(original_error.message, recovered_error.message);
        assert_eq!(original_error.retryable, recovered_error.retryable);
        assert_eq!(original_error.internal, recovered_error.internal);
    }
}
