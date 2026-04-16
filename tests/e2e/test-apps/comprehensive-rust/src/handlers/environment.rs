use axum::{extract::Path, response::Json};
use tracing::info;
use utoipa::path;

use crate::{models::EnvVarResponse, ErrorData, Result};
use alien_error::{AlienError, Context};

/// Get environment variable value
#[utoipa::path(
    get,
    path = "/env-var/{var_name}",
    tag = "environment",
    params(
        ("var_name" = String, Path, description = "Name of the environment variable to retrieve")
    ),
    responses(
        (status = 200, description = "Environment variable retrieved", body = EnvVarResponse),
        (status = 404, description = "Environment variable not found", body = AlienError),
    ),
    operation_id = "get_env_var",
    summary = "Get environment variable",
    description = "Retrieves the value of a specific environment variable"
)]
pub async fn get_env_var(Path(var_name): Path<String>) -> Result<Json<EnvVarResponse>> {
    info!(%var_name, "Request to get environment variable");

    match std::env::var(&var_name) {
        Ok(value) => Ok(Json(EnvVarResponse {
            success: true,
            name: var_name,
            value: Some(value),
            error: None,
        })),
        Err(_) => {
            let response = EnvVarResponse {
                success: false,
                name: var_name.clone(),
                value: None,
                error: Some("Environment variable not found".to_string()),
            };

            // We still return Ok here because the request was processed successfully,
            // but the env var wasn't found. This matches the original behavior.
            Ok(Json(response))
        }
    }
}
