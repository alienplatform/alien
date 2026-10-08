use alien_bindings::ErrorData;
use alien_error::{AlienError, AlienErrorData};
use pyo3::exceptions::PyRuntimeError;
use pyo3::PyErr;
use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ErrorEnvelope<'a> {
    code: &'a str,
    message: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    context: Option<&'a serde_json::Value>,
    retryable: bool,
    internal: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    http_status_code: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    hint: Option<&'a str>,
}

pub(crate) fn map_alien_error<T>(error: AlienError<T>) -> PyErr
where
    T: AlienErrorData + Clone + std::fmt::Debug + Serialize,
{
    let envelope = ErrorEnvelope {
        code: &error.code,
        message: &error.message,
        context: error.context.as_ref(),
        retryable: error.retryable,
        internal: error.internal,
        http_status_code: error.http_status_code,
        hint: error.hint.as_deref(),
    };
    let message = serde_json::to_string(&envelope).unwrap_or_else(|_| error.message.clone());
    PyRuntimeError::new_err(message)
}

pub(crate) fn map_object_store_error(
    error: object_store::Error,
    binding_name: &str,
    operation: &str,
) -> PyErr {
    let data = match error {
        object_store::Error::PermissionDenied { .. }
        | object_store::Error::Unauthenticated { .. } => ErrorData::StorageAccessDenied {
            binding_name: binding_name.to_string(),
            operation: operation.to_string(),
        },
        object_store::Error::NotFound { .. } => ErrorData::StorageObjectNotFound {
            binding_name: binding_name.to_string(),
            operation: operation.to_string(),
        },
        object_store::Error::AlreadyExists { .. } => ErrorData::StorageObjectAlreadyExists {
            binding_name: binding_name.to_string(),
            operation: operation.to_string(),
        },
        _ => ErrorData::StorageOperationFailed {
            binding_name: binding_name.to_string(),
            operation: operation.to_string(),
        },
    };
    map_alien_error(AlienError::new(data))
}
