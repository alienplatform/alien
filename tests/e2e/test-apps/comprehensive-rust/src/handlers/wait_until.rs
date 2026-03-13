use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::Json,
};
use bytes::Bytes;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};
use uuid::Uuid;

use crate::models::AppState;

/// Request body for wait_until test
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WaitUntilTestRequest {
    /// Storage binding name to use for the test
    pub storage_binding_name: String,
    /// Test data to write in the background task
    pub test_data: String,
    /// How long to wait before writing the file (in milliseconds)
    pub delay_ms: Option<u64>,
}

/// Response for wait_until test
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WaitUntilTestResponse {
    /// Success indicator
    pub success: bool,
    /// Test ID for verification
    pub test_id: String,
    /// Message describing the operation
    pub message: String,
    /// Any error that occurred
    pub error: Option<String>,
}

/// Response for wait_until verification
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WaitUntilVerifyResponse {
    /// Success indicator
    pub success: bool,
    /// Test ID that was verified
    pub test_id: String,
    /// Whether the background task completed (file exists)
    pub background_task_completed: bool,
    /// Content of the test file (if found)
    pub file_content: Option<String>,
    /// Any error that occurred
    pub error: Option<String>,
    /// Message describing the verification result
    pub message: String,
}

/// Test the wait_until functionality by registering a background task
pub async fn test_wait_until(
    State(state): State<AppState>,
    Json(request): Json<WaitUntilTestRequest>,
) -> Result<Json<WaitUntilTestResponse>, StatusCode> {
    let test_id = Uuid::new_v4().to_string();
    info!(test_id = %test_id, "Starting wait_until test");

    // Get the storage binding for the background task
    let storage_binding = match state
        .ctx
        .get_bindings()
        .load_storage(&request.storage_binding_name)
        .await
    {
        Ok(storage) => storage,
        Err(e) => {
            warn!(error = %e, "Failed to load storage binding");
            return Ok(Json(WaitUntilTestResponse {
                success: false,
                test_id,
                message: "Failed to load storage binding".to_string(),
                error: Some(e.to_string()),
            }));
        }
    };

    // Register background task with wait_until
    let test_data = request.test_data.clone();
    let delay_ms = request.delay_ms.unwrap_or(1000);
    let test_id_clone = test_id.clone();

    let result = state.ctx.wait_until(move || async move {
        info!(test_id = %test_id_clone, delay_ms = delay_ms, "Background task starting");

        // Wait for the specified delay
        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;

        // Write test file to storage
        let file_path = format!("wait_until_test_{}.txt", test_id_clone);
        let path = object_store::path::Path::from(file_path);

        match storage_binding.put(&path, test_data.into()).await {
            Ok(_) => {
                info!(test_id = %test_id_clone, "Background task completed successfully");
            }
            Err(e) => {
                warn!(test_id = %test_id_clone, error = %e, "Background task failed");
            }
        }
    });

    match result {
        Ok(_) => {
            info!(test_id = %test_id, "Successfully registered wait_until task");
            Ok(Json(WaitUntilTestResponse {
                success: true,
                test_id,
                message: "Background task registered successfully".to_string(),
                error: None,
            }))
        }
        Err(e) => {
            warn!(test_id = %test_id, error = %e, "Failed to register wait_until task");
            Ok(Json(WaitUntilTestResponse {
                success: false,
                test_id,
                message: "Failed to register background task".to_string(),
                error: Some(e.to_string()),
            }))
        }
    }
}

/// Verify that the wait_until background task completed by checking if the file exists
pub async fn verify_wait_until(
    State(app_state): State<AppState>,
    Path((test_id, storage_binding_name)): Path<(String, String)>,
) -> Result<Json<WaitUntilVerifyResponse>, StatusCode> {
    info!(test_id = %test_id, storage_binding = %storage_binding_name, "Verifying wait_until test");

    let test_file_path = format!("wait_until_test_{}.txt", test_id);
    let test_object_path = object_store::path::Path::from(test_file_path);

    let storage_instance = match app_state
        .ctx
        .get_bindings()
        .load_storage(&storage_binding_name)
        .await
    {
        Ok(storage) => storage,
        Err(e) => {
            warn!(error = %e, "Failed to load storage binding");
            return Ok(Json(WaitUntilVerifyResponse {
                success: false,
                test_id,
                background_task_completed: false,
                file_content: None,
                error: Some(e.to_string()),
                message: "Failed to load storage binding".to_string(),
            }));
        }
    };

    // Try to read the test file to see if background task completed
    match storage_instance.get(&test_object_path).await {
        Ok(get_result) => {
            // File exists, let's verify its content
            match get_result.bytes().await {
                Ok(content) => {
                    let content_str = String::from_utf8_lossy(&content).to_string();

                    info!(test_id = %test_id, "Verification successful - background task completed");
                    Ok(Json(WaitUntilVerifyResponse {
                        success: true,
                        test_id,
                        background_task_completed: true,
                        file_content: Some(content_str),
                        error: None,
                        message:
                            "Background task completed successfully - file exists with content"
                                .to_string(),
                    }))
                }
                Err(e) => {
                    warn!(test_id = %test_id, error = %e, "File exists but failed to read content");
                    Ok(Json(WaitUntilVerifyResponse {
                        success: false,
                        test_id,
                        background_task_completed: true,
                        file_content: None,
                        error: Some(e.to_string()),
                        message: "File exists but failed to read content".to_string(),
                    }))
                }
            }
        }
        Err(object_store::Error::NotFound { .. }) => {
            info!(test_id = %test_id, "Test file not found - background task may not have completed yet");
            Ok(Json(WaitUntilVerifyResponse {
                success: false,
                test_id,
                background_task_completed: false,
                file_content: None,
                error: None,
                message:
                    "Test file not found - background task may not have completed yet or failed"
                        .to_string(),
            }))
        }
        Err(e) => {
            warn!(test_id = %test_id, error = %e, "Failed to check for test file");
            Ok(Json(WaitUntilVerifyResponse {
                success: false,
                test_id,
                background_task_completed: false,
                file_content: None,
                error: Some(e.to_string()),
                message: "Failed to check for test file".to_string(),
            }))
        }
    }
}
