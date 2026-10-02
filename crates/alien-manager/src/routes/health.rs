//! Health check endpoint.

use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};

use super::AppState;

#[derive(Deserialize, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct HealthResponse {
    pub status: String,
    /// True when operation result contracts are persisted before commands
    /// become executable.
    #[serde(default)]
    pub operation_result_contract: bool,
    /// True when this manager recognizes the compute-planner capability role.
    #[serde(default)]
    pub compute_plan_capability: bool,
}

#[cfg_attr(feature = "openapi", utoipa::path(
    get,
    path = "/health",
    tag = "health",
    responses(
        (status = 200, description = "Server is healthy", body = HealthResponse)
    )
))]
pub async fn health(State(state): State<AppState>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "healthy".to_string(),
        operation_result_contract: state.command_server.persists_operation_result_contract(),
        compute_plan_capability: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compute_plan_capability_has_a_stable_wire_name() {
        let json = serde_json::to_value(HealthResponse {
            status: "healthy".to_string(),
            operation_result_contract: false,
            compute_plan_capability: true,
        })
        .expect("serialize health");
        assert_eq!(json["computePlanCapability"], serde_json::json!(true));

        let older: HealthResponse =
            serde_json::from_value(serde_json::json!({ "status": "healthy" }))
                .expect("a manager without the field still deserializes");
        assert!(!older.compute_plan_capability);
    }
}
