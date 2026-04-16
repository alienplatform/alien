use axum::response::Json;
use chrono::Utc;
use tracing::info;
use utoipa::path;

use crate::{models::HealthResponse, Result};

/// Health check endpoint
#[utoipa::path(
    get,
    path = "/health",
    tag = "health",
    responses(
        (status = 200, description = "Service is healthy", body = HealthResponse),
    ),
    operation_id = "health_check",
    summary = "Health check",
    description = "Returns the health status of the test server"
)]
pub async fn health_check() -> Result<Json<HealthResponse>> {
    info!("Health check requested");

    Ok(Json(HealthResponse {
        status: "ok".to_string(),
        timestamp: Utc::now(),
    }))
}

/// Simple hello endpoint for compatibility
#[utoipa::path(
    get,
    path = "/hello",
    tag = "health",
    responses(
        (status = 200, description = "Hello message", body = serde_json::Value),
    ),
    operation_id = "hello",
    summary = "Hello endpoint",
    description = "Returns a simple hello message for compatibility"
)]
pub async fn hello() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "message": "Hello from Rust!",
        "timestamp": Utc::now().to_rfc3339(),
    }))
}
