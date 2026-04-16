use axum::response::Json;
use tracing::debug;
use utoipa::path;

use crate::{models::InspectResponse, Result};

/// Inspect incoming request
#[utoipa::path(
    post,
    path = "/inspect",
    tag = "testing",
    request_body = serde_json::Value,
    responses(
        (status = 200, description = "Request body echoed back", body = InspectResponse),
    ),
    operation_id = "inspect_request",
    summary = "Inspect request",
    description = "Echoes back the request body for testing purposes"
)]
pub async fn inspect_request(Json(body): Json<serde_json::Value>) -> Result<Json<InspectResponse>> {
    // Log request details
    debug!(
        body = %body,
        "Received request for /inspect"
    );

    Ok(Json(InspectResponse {
        success: true,
        request_body: body,
    }))
}
