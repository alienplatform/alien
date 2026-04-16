use axum::{
    extract::{Path, State},
    response::Json,
};
use bytes::Bytes;
use chrono::Utc;
use object_store::PutMultipartOpts;
use std::time::Duration;
use tracing::info;

use crate::{
    models::{AppState, StorageTestResponse},
    ErrorData, Result,
};
use alien_error::{AlienError, Context, IntoAlienError};

/// Test storage operations
#[utoipa::path(
    post,
    path = "/storage-test/{binding_name}",
    tag = "storage",
    params(
        ("binding_name" = String, Path, description = "Name of the storage binding to test")
    ),
    responses(
        (status = 200, description = "Storage test completed", body = StorageTestResponse),
        (status = 400, description = "Binding not found", body = AlienError),
        (status = 500, description = "Storage operation failed", body = AlienError),
    ),
    operation_id = "test_storage",
    summary = "Test storage operations",
    description = "Performs comprehensive testing of storage operations including put, get, delete, head, and multipart upload operations"
)]
pub async fn test_storage(
    State(app_state): State<AppState>,
    Path(binding_name): Path<String>,
) -> Result<Json<StorageTestResponse>> {
    info!(%binding_name, "Received storage test request");

    let test_object_path_str = format!(
        "test_server_storage_test_{}_{}.txt",
        binding_name,
        Utc::now().timestamp_millis()
    );
    let test_object_path = object_store::path::Path::from(test_object_path_str.clone());
    let test_data = Bytes::from_static(b"Hello from alien-runtime storage test endpoint!");

    let storage_instance = app_state
        .ctx
        .get_bindings()
        .load_storage(&binding_name)
        .await
        .context(ErrorData::BindingNotFound {
            binding_name: binding_name.clone(),
        })?;

    // 1. Put operation
    storage_instance
        .put(&test_object_path, test_data.clone().into())
        .await
        .into_alien_error()
        .context(ErrorData::StorageOperationFailed {
            operation: "put".to_string(),
        })?;

    // 2. Get operation & verify
    let get_result = storage_instance
        .get(&test_object_path)
        .await
        .into_alien_error()
        .context(ErrorData::StorageOperationFailed {
            operation: "get".to_string(),
        })?;

    let retrieved_data =
        get_result
            .bytes()
            .await
            .into_alien_error()
            .context(ErrorData::StorageOperationFailed {
                operation: "get_bytes".to_string(),
            })?;

    if retrieved_data != test_data {
        return Err(AlienError::new(ErrorData::StorageOperationFailed {
            operation: "data_verification".to_string(),
        }));
    }

    // 3. Delete operation
    storage_instance
        .delete(&test_object_path)
        .await
        .into_alien_error()
        .context(ErrorData::StorageOperationFailed {
            operation: "delete".to_string(),
        })?;

    // 4. Head after delete - should return NotFound
    match storage_instance.head(&test_object_path).await {
        Err(object_store::Error::NotFound { .. }) => {
            // This is what we expect after deletion
        }
        Ok(_) => {
            return Err(AlienError::new(ErrorData::StorageOperationFailed {
                operation: "delete_verification".to_string(),
            }));
        }
        Err(e) => {
            return Err(e)
                .into_alien_error()
                .context(ErrorData::StorageOperationFailed {
                    operation: "head_after_delete".to_string(),
                })?;
        }
    }

    // 5. Multipart upload test
    let multipart_test_path_str = format!(
        "test_server_multipart_test_{}_{}.txt",
        binding_name,
        Utc::now().timestamp_millis()
    );
    let multipart_test_path = object_store::path::Path::from(multipart_test_path_str);

    // Create test data for multipart upload - use larger parts for AWS S3 compatibility
    // AWS S3 requires each part (except the last) to be at least 5MB
    let pattern1 = b"PART1_TEST_PATTERN_0123456789ABCDEF";
    let pattern2 = b"PART2_TEST_PATTERN_ZYXWVUTSRQPONMLK";

    // Helper function to create test data of specific size
    let create_test_data = |size: usize, pattern: &[u8]| -> Bytes {
        let pattern_len = pattern.len();
        let mut data_vec = Vec::with_capacity(size);
        for i in 0..size {
            data_vec.push(pattern[i % pattern_len]);
        }
        Bytes::from(data_vec)
    };

    // Always use 5MB + 1KB for multipart uploads to ensure compatibility with all backends
    // AWS S3 requires each part (except the last) to be at least 5MB
    let (part1_size, part2_size) = (5 * 1024 * 1024, 1024); // 5MB + 1KB

    let part1_data = create_test_data(part1_size, pattern1);
    let part2_data = create_test_data(part2_size, pattern2);
    let expected_combined_data = {
        let mut combined = Vec::new();
        combined.extend_from_slice(&part1_data);
        combined.extend_from_slice(&part2_data);
        Bytes::from(combined)
    };

    // Initiate multipart upload
    let mut multipart_upload = storage_instance
        .put_multipart_opts(&multipart_test_path, PutMultipartOpts::default())
        .await
        .into_alien_error()
        .context(ErrorData::StorageOperationFailed {
            operation: "put_multipart_init".to_string(),
        })?;

    // Upload part 1
    multipart_upload
        .put_part(part1_data.clone().into())
        .await
        .into_alien_error()
        .context(ErrorData::StorageOperationFailed {
            operation: "put_multipart_part1".to_string(),
        })?;

    // Upload part 2
    multipart_upload
        .put_part(part2_data.clone().into())
        .await
        .into_alien_error()
        .context(ErrorData::StorageOperationFailed {
            operation: "put_multipart_part2".to_string(),
        })?;

    // Complete the multipart upload
    multipart_upload
        .complete()
        .await
        .into_alien_error()
        .context(ErrorData::StorageOperationFailed {
            operation: "put_multipart_complete".to_string(),
        })?;

    // Verify the multipart upload by retrieving and checking the data
    let multipart_get_result = storage_instance
        .get(&multipart_test_path)
        .await
        .into_alien_error()
        .context(ErrorData::StorageOperationFailed {
            operation: "get_multipart".to_string(),
        })?;

    let multipart_retrieved_data = multipart_get_result
        .bytes()
        .await
        .into_alien_error()
        .context(ErrorData::StorageOperationFailed {
            operation: "get_multipart_bytes".to_string(),
        })?;

    if multipart_retrieved_data != expected_combined_data {
        return Err(AlienError::new(ErrorData::StorageOperationFailed {
            operation: "multipart_data_verification".to_string(),
        }));
    }

    // Clean up the multipart test file
    storage_instance
        .delete(&multipart_test_path)
        .await
        .into_alien_error()
        .context(ErrorData::StorageOperationFailed {
            operation: "delete_multipart".to_string(),
        })?;

    // 6. Presigned URL tests
    let presigned_test_path_str = format!(
        "test_server_presigned_test_{}_{}.txt",
        binding_name,
        Utc::now().timestamp_millis()
    );
    let presigned_test_path = object_store::path::Path::from(presigned_test_path_str);
    let presigned_test_data = Bytes::from_static(b"Presigned URL test data from alien");
    let expires_in = Duration::from_secs(300);

    // 6a. Presigned PUT — upload via presigned request
    let put_request = storage_instance
        .presigned_put(&presigned_test_path, expires_in)
        .await
        .context(ErrorData::StorageOperationFailed {
            operation: "presigned_put".to_string(),
        })?;

    put_request
        .execute(Some(presigned_test_data.clone()))
        .await
        .context(ErrorData::StorageOperationFailed {
            operation: "presigned_put_execute".to_string(),
        })?;

    // 6b. Presigned GET — download via presigned request and verify
    let get_request = storage_instance
        .presigned_get(&presigned_test_path, expires_in)
        .await
        .context(ErrorData::StorageOperationFailed {
            operation: "presigned_get".to_string(),
        })?;

    let get_response =
        get_request
            .execute(None)
            .await
            .context(ErrorData::StorageOperationFailed {
                operation: "presigned_get_execute".to_string(),
            })?;

    let presigned_retrieved = get_response.body.ok_or_else(|| {
        AlienError::new(ErrorData::StorageOperationFailed {
            operation: "presigned_get_no_body".to_string(),
        })
    })?;

    if presigned_retrieved != presigned_test_data {
        return Err(AlienError::new(ErrorData::StorageOperationFailed {
            operation: "presigned_data_verification".to_string(),
        }));
    }

    // 6c. Presigned DELETE — delete via presigned request
    let delete_request = storage_instance
        .presigned_delete(&presigned_test_path, expires_in)
        .await
        .context(ErrorData::StorageOperationFailed {
            operation: "presigned_delete".to_string(),
        })?;

    delete_request
        .execute(None)
        .await
        .context(ErrorData::StorageOperationFailed {
            operation: "presigned_delete_execute".to_string(),
        })?;

    // Verify deletion via head
    match storage_instance.head(&presigned_test_path).await {
        Err(object_store::Error::NotFound { .. }) => {}
        Ok(_) => {
            return Err(AlienError::new(ErrorData::StorageOperationFailed {
                operation: "presigned_delete_verification".to_string(),
            }));
        }
        Err(e) => {
            return Err(e)
                .into_alien_error()
                .context(ErrorData::StorageOperationFailed {
                    operation: "presigned_head_after_delete".to_string(),
                })?;
        }
    }

    Ok(Json(StorageTestResponse {
        binding_name,
        success: true,
    }))
}
