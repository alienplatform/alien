use axum::{
    extract::{Path, State},
    response::Json,
};
use tracing::info;

use crate::{
    models::{AppState, ServiceAccountTestResponse},
    ErrorData, Result,
};
use alien_error::Context;

/// Test service account operations
pub async fn test_service_account(
    State(app_state): State<AppState>,
    Path(binding_name): Path<String>,
) -> Result<Json<ServiceAccountTestResponse>> {
    info!(%binding_name, "Received service account test request");

    let sa = app_state
        .ctx
        .get_bindings()
        .load_service_account(&binding_name)
        .await
        .context(ErrorData::BindingNotFound {
            binding_name: binding_name.clone(),
        })?;

    // Get service account identity info
    let info = sa
        .get_info()
        .await
        .context(ErrorData::ServiceAccountOperationFailed {
            operation: "get_info".to_string(),
        })?;

    let info_json = serde_json::to_value(&info).unwrap_or(serde_json::json!(null));

    info!(%binding_name, ?info, "Service account info retrieved");

    Ok(Json(ServiceAccountTestResponse {
        binding_name,
        success: true,
        info: info_json,
    }))
}
