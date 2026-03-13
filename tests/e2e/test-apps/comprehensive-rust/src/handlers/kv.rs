use axum::{
    extract::{Path, State},
    response::Json,
};
use chrono::Utc;
use tracing::info;

use crate::{
    models::{AppState, KvTestResponse},
    ErrorData, Result,
};
use alien_bindings::traits::PutOptions;
use alien_error::{AlienError, Context, IntoAlienError};

/// Test KV operations with a full E2E flow
#[utoipa::path(
    post,
    path = "/kv-test/{binding_name}",
    tag = "kv",
    params(
        ("binding_name" = String, Path, description = "Name of the KV binding to test")
    ),
    responses(
        (status = 200, description = "KV test completed", body = KvTestResponse),
        (status = 400, description = "Binding not found", body = AlienError),
        (status = 500, description = "KV operation failed", body = AlienError),
    ),
    operation_id = "test_kv",
    summary = "Test KV operations",
    description = "Performs comprehensive E2E testing of KV operations: put, get, exists, scan_prefix, and delete operations"
)]
pub async fn test_kv(
    State(app_state): State<AppState>,
    Path(binding_name): Path<String>,
) -> Result<Json<KvTestResponse>> {
    info!(%binding_name, "Received KV test request");

    let test_key_prefix = format!(
        "test_server_kv_test_{}_{}",
        binding_name,
        Utc::now().timestamp_millis()
    );
    let test_key1 = format!("{}_key1", test_key_prefix);
    let test_key2 = format!("{}_key2", test_key_prefix);
    let test_value1 = b"test-value-1-from-alien-test-server".to_vec();
    let test_value2 = b"test-value-2-from-alien-test-server".to_vec();

    let kv_instance = app_state
        .ctx
        .get_bindings()
        .load_kv(&binding_name)
        .await
        .context(ErrorData::BindingNotFound {
            binding_name: binding_name.clone(),
        })?;

    // 1. Put the first key-value pair
    info!(%test_key1, "Putting first key-value pair");
    let put1_result = kv_instance
        .put(&test_key1, test_value1.clone(), None)
        .await
        .into_alien_error()
        .context(ErrorData::KvOperationFailed {
            operation: "put".to_string(),
            key: test_key1.clone(),
            reason: "Failed to put first key-value pair".to_string(),
        })?;

    if !put1_result {
        return Err(AlienError::new(ErrorData::KvOperationFailed {
            operation: "put".to_string(),
            key: test_key1.clone(),
            reason: "Put operation returned false unexpectedly".to_string(),
        }));
    }

    // 2. Put the second key-value pair with if_not_exists option
    info!(%test_key2, "Putting second key-value pair with if_not_exists");
    let put_options = PutOptions {
        ttl: None,
        if_not_exists: true,
    };
    let put2_result = kv_instance
        .put(&test_key2, test_value2.clone(), Some(put_options))
        .await
        .into_alien_error()
        .context(ErrorData::KvOperationFailed {
            operation: "put_if_not_exists".to_string(),
            key: test_key2.clone(),
            reason: "Failed to put second key-value pair with if_not_exists".to_string(),
        })?;

    if !put2_result {
        return Err(AlienError::new(ErrorData::KvOperationFailed {
            operation: "put_if_not_exists".to_string(),
            key: test_key2.clone(),
            reason: "Put with if_not_exists returned false unexpectedly".to_string(),
        }));
    }

    // 3. Try to put the second key again with if_not_exists (should return false)
    info!(%test_key2, "Attempting to put existing key with if_not_exists (should fail)");
    let put_options_duplicate = PutOptions {
        ttl: None,
        if_not_exists: true,
    };
    let put_duplicate_result = kv_instance
        .put(
            &test_key2,
            b"should-not-work".to_vec(),
            Some(put_options_duplicate),
        )
        .await
        .into_alien_error()
        .context(ErrorData::KvOperationFailed {
            operation: "put_if_not_exists_duplicate".to_string(),
            key: test_key2.clone(),
            reason: "Failed to test duplicate put with if_not_exists".to_string(),
        })?;

    if put_duplicate_result {
        return Err(AlienError::new(ErrorData::KvOperationFailed {
            operation: "put_if_not_exists_duplicate".to_string(),
            key: test_key2.clone(),
            reason: "Put with if_not_exists should have returned false for existing key"
                .to_string(),
        }));
    }

    // 4. Get the first value and verify
    info!(%test_key1, "Getting first value and verifying");
    let retrieved_value1 = kv_instance
        .get(&test_key1)
        .await
        .into_alien_error()
        .context(ErrorData::KvOperationFailed {
            operation: "get".to_string(),
            key: test_key1.clone(),
            reason: "Failed to get first value".to_string(),
        })?;

    match retrieved_value1 {
        Some(value) if value == test_value1 => {
            info!(%test_key1, "First value retrieved and verified successfully");
        }
        Some(value) => {
            return Err(AlienError::new(ErrorData::KvOperationFailed {
                operation: "get_verification".to_string(),
                key: test_key1.clone(),
                reason: format!(
                    "Retrieved value doesn't match expected. Got: {:?}, Expected: {:?}",
                    value, test_value1
                ),
            }));
        }
        None => {
            return Err(AlienError::new(ErrorData::KvOperationFailed {
                operation: "get".to_string(),
                key: test_key1.clone(),
                reason: "Key not found when it should exist".to_string(),
            }));
        }
    }

    // 5. Check if keys exist
    info!(%test_key1, "Checking if first key exists");
    let exists1 = kv_instance
        .exists(&test_key1)
        .await
        .into_alien_error()
        .context(ErrorData::KvOperationFailed {
            operation: "exists".to_string(),
            key: test_key1.clone(),
            reason: "Failed to check existence of first key".to_string(),
        })?;

    if !exists1 {
        return Err(AlienError::new(ErrorData::KvOperationFailed {
            operation: "exists".to_string(),
            key: test_key1.clone(),
            reason: "Key should exist but exists() returned false".to_string(),
        }));
    }

    // 6. Scan with prefix to find both keys
    info!(%test_key_prefix, "Scanning keys with prefix");
    let scan_result = kv_instance
        .scan_prefix(&test_key_prefix, Some(10), None)
        .await
        .into_alien_error()
        .context(ErrorData::KvOperationFailed {
            operation: "scan_prefix".to_string(),
            key: test_key_prefix.clone(),
            reason: "Failed to scan keys with prefix".to_string(),
        })?;

    if scan_result.items.len() != 2 {
        return Err(AlienError::new(ErrorData::KvOperationFailed {
            operation: "scan_prefix".to_string(),
            key: test_key_prefix.clone(),
            reason: format!(
                "Expected 2 items in scan result, got {}",
                scan_result.items.len()
            ),
        }));
    }

    // Verify the scanned items contain our keys
    let scanned_keys: Vec<String> = scan_result.items.iter().map(|(k, _)| k.clone()).collect();
    if !scanned_keys.contains(&test_key1) || !scanned_keys.contains(&test_key2) {
        return Err(AlienError::new(ErrorData::KvOperationFailed {
            operation: "scan_prefix_verification".to_string(),
            key: test_key_prefix.clone(),
            reason: format!(
                "Scanned keys {:?} don't contain expected keys [{}', '{}']",
                scanned_keys, test_key1, test_key2
            ),
        }));
    }

    // 7. Delete the first key
    info!(%test_key1, "Deleting first key");
    kv_instance
        .delete(&test_key1)
        .await
        .into_alien_error()
        .context(ErrorData::KvOperationFailed {
            operation: "delete".to_string(),
            key: test_key1.clone(),
            reason: "Failed to delete first key".to_string(),
        })?;

    // 8. Verify the first key no longer exists
    info!(%test_key1, "Verifying first key deletion");
    let exists_after_delete = kv_instance
        .exists(&test_key1)
        .await
        .into_alien_error()
        .context(ErrorData::KvOperationFailed {
            operation: "exists_after_delete".to_string(),
            key: test_key1.clone(),
            reason: "Failed to check existence after deletion".to_string(),
        })?;

    if exists_after_delete {
        return Err(AlienError::new(ErrorData::KvOperationFailed {
            operation: "delete_verification".to_string(),
            key: test_key1.clone(),
            reason: "Key should not exist after deletion but exists() returned true".to_string(),
        }));
    }

    // 9. Get the deleted key (should return None)
    info!(%test_key1, "Verifying deleted key returns None");
    let get_after_delete = kv_instance
        .get(&test_key1)
        .await
        .into_alien_error()
        .context(ErrorData::KvOperationFailed {
            operation: "get_after_delete".to_string(),
            key: test_key1.clone(),
            reason: "Failed to get key after deletion".to_string(),
        })?;

    if get_after_delete.is_some() {
        return Err(AlienError::new(ErrorData::KvOperationFailed {
            operation: "get_after_delete_verification".to_string(),
            key: test_key1.clone(),
            reason: "Get after deletion should return None but returned Some".to_string(),
        }));
    }

    // 10. Clean up: delete the second key
    info!(%test_key2, "Cleaning up - deleting second key");
    kv_instance
        .delete(&test_key2)
        .await
        .into_alien_error()
        .context(ErrorData::KvOperationFailed {
            operation: "cleanup_delete".to_string(),
            key: test_key2.clone(),
            reason: "Failed to delete second key during cleanup".to_string(),
        })?;

    info!(%test_key_prefix, "KV test completed successfully");

    Ok(Json(KvTestResponse {
        binding_name,
        success: true,
    }))
}
