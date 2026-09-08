use alien_error::{Context, IntoAlienError};
use axum::{
    extract::{Path, State},
    http::StatusCode,
};

use crate::{models::AppState, ErrorData, Result};

/// Remove one test object after its content or creation event has been verified.
pub async fn delete_storage_object(
    State(state): State<AppState>,
    Path((binding_name, key)): Path<(String, String)>,
) -> Result<StatusCode> {
    let storage = state
        .ctx
        .bindings()
        .storage(&binding_name)
        .await
        .context(ErrorData::BindingNotFound { binding_name })?;
    let path = object_store::path::Path::from(key);
    match storage.delete(&path).await {
        Ok(()) | Err(object_store::Error::NotFound { .. }) => Ok(StatusCode::NO_CONTENT),
        Err(error) => Err(error)
            .into_alien_error()
            .context(ErrorData::StorageOperationFailed {
                operation: "delete test object".to_string(),
            }),
    }
}
