//! State sync endpoints for deployment loop coordination.

use axum::{
    extract::{Json, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
    Router,
};
use serde::{Deserialize, Serialize};

use alien_core::{DeploymentState, Platform};

use crate::error::ErrorData;
use crate::ids;
use crate::traits::{
    CreateDeploymentParams, CreateTokenParams, DeploymentFilter, ReconcileData, TokenType,
};

use super::{auth, AppState};

// --- Request / Response types ---

#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct AcquireRequest {
    pub session: String,
    #[serde(default)]
    pub deployment_ids: Option<Vec<String>>,
    #[serde(default)]
    pub platforms: Option<Vec<Platform>>,
    #[serde(default)]
    pub statuses: Option<Vec<String>>,
    #[serde(default = "default_limit")]
    pub limit: u32,
}

fn default_limit() -> u32 {
    10
}

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct AcquireResponse {
    pub deployments: Vec<AcquiredDeploymentResponse>,
}

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct AcquiredDeploymentResponse {
    pub deployment: serde_json::Value,
}

#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ReconcileRequest {
    pub deployment_id: String,
    pub session: String,
    pub state: DeploymentState,
    #[serde(default)]
    pub update_heartbeat: bool,
}

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ReconcileResponse {
    pub success: bool,
    pub current: DeploymentState,
}

#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ReleaseRequest {
    pub deployment_id: String,
    pub session: String,
}

#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct OperatorSyncRequest {
    pub deployment_id: String,
}

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct OperatorSyncResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct InitializeRequest {
    pub name: Option<String>,
    pub platform: Option<Platform>,
}

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct InitializeResponse {
    pub deployment_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
}

// --- Router ---

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/v1/sync/acquire", post(acquire))
        .route("/v1/sync/reconcile", post(reconcile))
        .route("/v1/sync/release", post(release))
        .route("/v1/sync", post(operator_sync))
}

/// Router for the `/v1/initialize` endpoint only.
///
/// Separated so embedders (e.g. alien-platform-manager) can replace it with a
/// platform-specific implementation that proxies token creation to the Platform API.
pub fn initialize_router() -> Router<AppState> {
    Router::new().route("/v1/initialize", post(initialize))
}

// --- Handlers ---

#[cfg_attr(feature = "openapi", utoipa::path(
    post,
    path = "/v1/sync/acquire",
    tag = "sync",
    request_body = AcquireRequest,
    responses(
        (status = 200, description = "Deployments acquired for reconciliation", body = AcquireResponse)
    ),
    security(
        ("bearer" = [])
    )
))]
async fn acquire(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<AcquireRequest>,
) -> Response {
    let subject = match auth::require_auth(&state, &headers).await {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };

    if let Err(e) = auth::require_admin(&subject) {
        return e.into_response();
    }

    let filter = DeploymentFilter {
        deployment_group_id: None,
        statuses: req.statuses,
        platforms: req.platforms,
        limit: Some(req.limit),
    };

    let acquired = match state
        .deployment_store
        .acquire(&req.session, &filter, req.limit)
        .await
    {
        Ok(a) => a,
        Err(e) => return e.into_response(),
    };

    let deployments = acquired
        .into_iter()
        .map(|a| AcquiredDeploymentResponse {
            deployment: serde_json::to_value(&a.deployment).unwrap_or_default(),
        })
        .collect();

    Json(AcquireResponse { deployments }).into_response()
}

#[cfg_attr(feature = "openapi", utoipa::path(
    post,
    path = "/v1/sync/reconcile",
    tag = "sync",
    request_body = ReconcileRequest,
    responses(
        (status = 200, description = "Deployment state reconciled", body = ReconcileResponse)
    ),
    security(
        ("bearer" = [])
    )
))]
async fn reconcile(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<ReconcileRequest>,
) -> Response {
    let subject = match auth::require_auth(&state, &headers).await {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };

    // Allow admin tokens (push mode) or deployment tokens (pull mode)
    if !subject.is_admin() && !subject.can_access_deployment(&req.deployment_id) {
        return ErrorData::forbidden("Access denied").into_response();
    }

    let _result = match state
        .deployment_store
        .reconcile(ReconcileData {
            deployment_id: req.deployment_id,
            session: req.session,
            state: req.state.clone(),
            update_heartbeat: req.update_heartbeat,
        })
        .await
    {
        Ok(r) => r,
        Err(e) => return e.into_response(),
    };

    Json(ReconcileResponse {
        success: true,
        current: req.state,
    })
    .into_response()
}

#[cfg_attr(feature = "openapi", utoipa::path(
    post,
    path = "/v1/sync/release",
    tag = "sync",
    request_body = ReleaseRequest,
    responses(
        (status = 200, description = "Deployment lock released")
    ),
    security(
        ("bearer" = [])
    )
))]
async fn release(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<ReleaseRequest>,
) -> Response {
    let subject = match auth::require_auth(&state, &headers).await {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };

    if !subject.is_admin() && !subject.can_access_deployment(&req.deployment_id) {
        return ErrorData::forbidden("Access denied").into_response();
    }

    match state
        .deployment_store
        .release(&req.deployment_id, &req.session)
        .await
    {
        Ok(()) => Json(serde_json::json!({ "success": true })).into_response(),
        Err(e) => e.into_response(),
    }
}

#[cfg_attr(feature = "openapi", utoipa::path(
    post,
    path = "/v1/sync",
    tag = "sync",
    request_body = OperatorSyncRequest,
    responses(
        (status = 200, description = "Operator sync response with optional target state", body = OperatorSyncResponse)
    ),
    security(
        ("bearer" = [])
    )
))]
async fn operator_sync(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<OperatorSyncRequest>,
) -> Response {
    let subject = match auth::require_auth(&state, &headers).await {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };

    // Must be a deployment token matching this deployment
    if !subject.can_access_deployment(&req.deployment_id) && !subject.is_admin() {
        return ErrorData::forbidden("Access denied").into_response();
    }

    let deployment = match state
        .deployment_store
        .get_deployment(&req.deployment_id)
        .await
    {
        Ok(Some(d)) => d,
        Ok(None) => return ErrorData::not_found_deployment(&req.deployment_id).into_response(),
        Err(e) => return e.into_response(),
    };

    // Return target state if deployment needs updating
    let target = if deployment.desired_release_id.is_some()
        && deployment.desired_release_id != deployment.current_release_id
    {
        // Build target info for the operator
        let release_info = if let Some(ref release_id) = deployment.desired_release_id {
            match state.release_store.get_release(release_id).await {
                Ok(Some(r)) => Some(serde_json::json!({
                    "releaseId": r.id,
                    "stack": serde_json::to_value(&r.stack).unwrap_or_default(),
                })),
                _ => None,
            }
        } else {
            None
        };

        release_info.map(|ri| {
            serde_json::json!({
                "releaseInfo": ri,
                "config": serde_json::to_value(&deployment.stack_settings).unwrap_or_default(),
            })
        })
    } else {
        None
    };

    Json(OperatorSyncResponse { target }).into_response()
}

#[cfg_attr(feature = "openapi", utoipa::path(
    post,
    path = "/v1/initialize",
    tag = "sync",
    request_body = InitializeRequest,
    responses(
        (status = 200, description = "Existing deployment returned", body = InitializeResponse),
        (status = 201, description = "New deployment created with token", body = InitializeResponse)
    ),
    security(
        ("bearer" = [])
    )
))]
async fn initialize(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<InitializeRequest>,
) -> Response {
    let subject = match auth::require_auth(&state, &headers).await {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };

    match subject.scope.token_type {
        TokenType::Deployment => {
            // Already has a deployment - return its ID
            let deployment_id = subject.scope.deployment_id.unwrap_or_default();
            Json(InitializeResponse {
                deployment_id,
                token: None,
            })
            .into_response()
        }
        TokenType::DeploymentGroup => {
            // Create a new pull-mode deployment
            let dg_id = subject.scope.deployment_group_id.unwrap_or_default();

            let name = req
                .name
                .unwrap_or_else(|| format!("operator-{}", &ids::deployment_id()[3..9]));
            let platform = req.platform.unwrap_or(Platform::Kubernetes);

            let mut settings = alien_core::StackSettings::default();
            settings.deployment_model = alien_core::DeploymentModel::Pull;

            let deployment = match state
                .deployment_store
                .create_deployment(CreateDeploymentParams {
                    name,
                    deployment_group_id: dg_id.clone(),
                    platform,
                    stack_settings: settings,
                    environment_variables: None,
                })
                .await
            {
                Ok(d) => d,
                Err(e) => return e.into_response(),
            };

            // Create a deployment token for the new deployment
            let (raw_token, key_prefix, key_hash) =
                ids::generate_token(TokenType::Deployment.prefix());
            match state
                .token_store
                .create_token(CreateTokenParams {
                    token_type: TokenType::Deployment,
                    key_prefix,
                    key_hash,
                    deployment_group_id: Some(dg_id),
                    deployment_id: Some(deployment.id.clone()),
                })
                .await
            {
                Ok(_) => (
                    StatusCode::CREATED,
                    Json(InitializeResponse {
                        deployment_id: deployment.id,
                        token: Some(raw_token),
                    }),
                )
                    .into_response(),
                Err(e) => e.into_response(),
            }
        }
        TokenType::Admin => {
            ErrorData::bad_request("Initialize requires a deployment or deployment group token")
                .into_response()
        }
    }
}
