use axum::{
    extract::{Path, State},
    response::Json,
};
use tracing::info;

use crate::{
    models::{AppState, QueueMessageRetrievalResponse},
    ErrorData, Result,
};
use alien_error::{AlienError, Context};

/// Retrieve stored queue messages from KV storage
#[utoipa::path(
    get,
    path = "/queue-messages-received/{kv_binding_name}",
    tag = "queue",
    params(
        ("kv_binding_name" = String, Path, description = "KV binding name to retrieve from")
    ),
    responses(
        (status = 200, description = "Queue messages retrieved successfully", body = QueueMessageRetrievalResponse),
        (status = 400, description = "Binding not found", body = AlienError),
        (status = 500, description = "Failed to retrieve queue messages", body = AlienError),
    ),
    operation_id = "get_received_queue_messages",
    summary = "Get received queue messages",
    description = "Retrieves queue messages that were previously processed and stored in KV storage by the queue message handler."
)]
pub async fn get_received_queue_messages(
    State(app_state): State<AppState>,
    Path(kv_binding_name): Path<String>,
) -> Result<Json<QueueMessageRetrievalResponse>> {
    info!(kv_binding_name = %kv_binding_name, "Retrieving stored queue messages");

    // Get KV binding
    let kv = app_state
        .ctx
        .get_bindings()
        .load_kv(&kv_binding_name)
        .await
        .context(ErrorData::BindingNotFound {
            binding_name: kv_binding_name.clone(),
        })?;

    // Use the proper scan_prefix operation to find all queue messages
    let prefix = "queue_message:";
    let limit = 100;

    let mut retrieved_messages = Vec::new();

    match kv.scan_prefix(prefix, Some(limit), None).await {
        Ok(scan_result) => {
            info!(
                found_keys = scan_result.items.len(),
                "Found keys with scan_prefix"
            );

            for (key, value) in scan_result.items {
                match serde_json::from_slice::<serde_json::Value>(&value) {
                    Ok(parsed_message) => {
                        retrieved_messages.push(parsed_message);
                        info!(key = %key, "Found stored queue message");
                    }
                    Err(e) => {
                        info!(key = %key, error = %e, "Failed to parse stored message");
                    }
                }
            }
        }
        Err(e) => {
            info!(error = %e, "Failed to scan prefix for queue messages");
        }
    }

    info!(
        retrieved_count = retrieved_messages.len(),
        "Retrieved stored queue messages"
    );

    Ok(Json(QueueMessageRetrievalResponse {
        success: true,
        kv_binding_name,
        retrieved_count: retrieved_messages.len(),
        messages: retrieved_messages,
    }))
}
