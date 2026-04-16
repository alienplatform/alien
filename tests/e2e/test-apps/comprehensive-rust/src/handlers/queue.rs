use crate::{models::AppState, ErrorData, Result};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_sdk::traits::MessagePayload;
use axum::{
    extract::{Path, State},
    response::Json,
};
use tracing::info;

#[derive(serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct QueueTestResponse {
    pub binding_name: String,
    pub success: bool,
}

#[cfg_attr(feature = "openapi", utoipa::path(
    post,
    path = "/queue-test/{binding_name}",
    tag = "queue",
    params(("binding_name" = String, Path, description = "Queue binding name")),
    responses(
        (status = 200, description = "Queue test completed", body = QueueTestResponse),
        (status = 400, description = "Binding not found", body = AlienError),
        (status = 500, description = "Queue operation failed", body = AlienError),
    ),
    operation_id = "test_queue",
    summary = "Test Queue operations",
    description = "Sends a message, receives it back, and acks"
))]
#[cfg_attr(not(feature = "openapi"), allow(unused))]
pub async fn test_queue(
    State(app_state): State<AppState>,
    Path(binding_name): Path<String>,
) -> Result<Json<QueueTestResponse>> {
    info!(%binding_name, "Received Queue test request");

    let queue = app_state
        .ctx
        .get_bindings()
        .load_queue(&binding_name)
        .await
        .context(ErrorData::BindingNotFound {
            binding_name: binding_name.clone(),
        })?;

    // Send a test message
    let payload =
        MessagePayload::Json(serde_json::json!({ "hello": "world", "binding": binding_name }));
    queue
        .send(&binding_name, payload)
        .await
        .into_alien_error()
        .context(ErrorData::QueueOperationFailed {
            operation: "Failed to send test message".to_string(),
        })?;

    // Receive up to 1 message
    let messages = queue
        .receive(&binding_name, 1)
        .await
        .into_alien_error()
        .context(ErrorData::QueueOperationFailed {
            operation: "Failed to receive message".to_string(),
        })?;
    if let Some(msg) = messages.into_iter().next() {
        queue
            .ack(&binding_name, &msg.receipt_handle)
            .await
            .into_alien_error()
            .context(ErrorData::QueueOperationFailed {
                operation: "Failed to ack message".to_string(),
            })?;
    }

    Ok(Json(QueueTestResponse {
        binding_name,
        success: true,
    }))
}
