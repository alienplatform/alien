use alien_error::{AlienError, Context, IntoAlienError};
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
    // This test app is public. Cleanup must not expose arbitrary binding/object deletion.
    let id = key
        .strip_prefix("storage-event-test-")
        .or_else(|| key.strip_prefix("wait_until_test_"))
        .and_then(|key| key.strip_suffix(".txt"));
    let valid_key =
        id.is_some_and(|id| uuid::Uuid::parse_str(id).is_ok_and(|uuid| uuid.to_string() == id));
    if binding_name != "alien-storage" || !valid_key {
        return Err(AlienError::new(ErrorData::TestValidationFailed {
            reason: "Cleanup only accepts generated storage test keys in alien-storage".to_string(),
        }));
    }
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
