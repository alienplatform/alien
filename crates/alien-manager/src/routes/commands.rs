//! Authenticated command endpoints.
//!
//! All command protocol routes are defined here with per-handler auth checks,
//! following the same pattern as `sync.rs` and `deployments.rs`.

use axum::{
    extract::{Json, Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post, put},
    Router,
};
use serde::Deserialize;
use tracing::error;

use alien_commands::server::{CommandPayloadResponse, StorePayloadRequest};
use alien_commands::types::*;

use crate::error::ErrorData;

use super::{auth, AppState};

// --- Helpers ---

/// Look up the deployment_group_id for a deployment, for DG-scoped auth checks.
async fn get_deployment_group_id(state: &AppState, deployment_id: &str) -> Result<String, Response> {
    let deployment = state
        .deployment_store
        .get_deployment(deployment_id)
        .await
        .map_err(|e| e.into_response())?
        .ok_or_else(|| ErrorData::not_found_deployment(deployment_id).into_response())?;
    Ok(deployment.deployment_group_id.clone())
}

/// Look up the deployment_id that owns a command, for per-command auth checks.
async fn get_command_owner(state: &AppState, command_id: &str) -> Result<String, Response> {
    state
        .command_server
        .get_command_deployment_id(command_id)
        .await
        .map_err(|e| e.into_response())?
        .ok_or_else(|| {
            alien_error::AlienError::new(alien_commands::error::ErrorData::CommandNotFound {
                command_id: command_id.to_string(),
            })
            .into_response()
        })
}

/// Authorize the caller against the deployment that owns a command.
/// Loads the deployment then defers to `Authz::can_read_command`.
async fn require_command_access(
    state: &AppState,
    subject: &crate::auth::Subject,
    deployment_id: &str,
) -> Result<(), Response> {
    let deployment = state
        .deployment_store
        .get_deployment(deployment_id)
        .await
        .map_err(|e| e.into_response())?
        .ok_or_else(|| ErrorData::not_found_deployment(deployment_id).into_response())?;
    if state.authz.can_read_command(subject, &deployment) {
        Ok(())
    } else {
        Err(ErrorData::forbidden("Access denied").into_response())
    }
}

// --- Router ---

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/v1/commands", post(create_command))
        .route(
            "/v1/commands/{command_id}",
            get(get_command_status),
        )
        .route(
            "/v1/commands/{command_id}/upload-complete",
            post(upload_complete),
        )
        .route(
            "/v1/commands/{command_id}/response",
            put(submit_response),
        )
        .route(
            "/v1/commands/{command_id}/payload",
            get(get_command_payload).put(store_command_payload),
        )
        .route("/v1/commands/leases", post(acquire_leases))
        .route(
            "/v1/commands/leases/{lease_id}/release",
            post(release_lease),
        )
}

// --- Handlers ---

/// Create a new command.
///
/// Auth: Admin or DeploymentGroup token (must own the target deployment's group).
async fn create_command(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CreateCommandRequest>,
) -> Response {
    let subject = match auth::require_auth(&state, &headers).await {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };
    let deployment = match state.deployment_store.get_deployment(&request.deployment_id).await {
        Ok(Some(d)) => d,
        Ok(None) => return ErrorData::not_found_deployment(&request.deployment_id).into_response(),
        Err(e) => return e.into_response(),
    };

    if !state.authz.can_dispatch_command(&subject, &deployment) {
        return ErrorData::forbidden("Cannot dispatch command for this deployment")
            .into_response();
    }

    match state.command_server.create_command(request).await {
        Ok(response) => Json(response).into_response(),
        Err(e) => e.into_response(),
    }
}

/// Get command status.
///
/// Auth: Admin, DG (group), or own Deployment token.
async fn get_command_status(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(command_id): Path<String>,
) -> Response {
    let subject = match auth::require_auth(&state, &headers).await {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };
    let deployment_id = match get_command_owner(&state, &command_id).await {
        Ok(id) => id,
        Err(e) => return e,
    };

    if let Err(e) = require_command_access(&state, &subject, &deployment_id).await {
        return e;
    }

    match state.command_server.get_command_status(&command_id).await {
        Ok(response) => Json(response).into_response(),
        Err(e) => e.into_response(),
    }
}

/// Mark upload as complete.
///
/// Auth: Admin or DG (same as create — the command creator completes the upload).
async fn upload_complete(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(command_id): Path<String>,
    Json(upload_request): Json<UploadCompleteRequest>,
) -> Response {
    let subject = match auth::require_auth(&state, &headers).await {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };
    let deployment_id = match get_command_owner(&state, &command_id).await {
        Ok(id) => id,
        Err(e) => return e,
    };

    let deployment = match state.deployment_store.get_deployment(&deployment_id).await {
        Ok(Some(d)) => d,
        Ok(None) => return ErrorData::not_found_deployment(&deployment_id).into_response(),
        Err(e) => return e.into_response(),
    };

    if !state.authz.can_dispatch_command(&subject, &deployment) {
        return ErrorData::forbidden("Access denied").into_response();
    }

    match state
        .command_server
        .upload_complete(&command_id, upload_request)
        .await
    {
        Ok(response) => Json(response).into_response(),
        Err(e) => e.into_response(),
    }
}

/// Query parameters for submit_response (presigned URL auth).
#[derive(Deserialize, Default)]
struct SubmitResponseQuery {
    response_token: Option<String>,
    expires: Option<i64>,
}

/// Submit response from deployment.
///
/// Auth: Bearer token (Admin/Deployment) OR presigned URL query params
/// (`response_token` + `expires`). The presigned URL approach is used by
/// push-mode runtimes (Lambda, Cloud Run, Container Apps) that have no
/// pre-existing auth token — the signed URL is embedded in the command envelope.
async fn submit_response(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(command_id): Path<String>,
    Query(query): Query<SubmitResponseQuery>,
    Json(request): Json<SubmitResponseRequest>,
) -> Response {
    // Try Bearer token auth first (used by polling-mode runtimes and admin callers).
    let bearer_auth = match auth::extract_auth(&state, &headers).await {
        Ok(subject) => subject,
        Err(e) => return e.into_response(),
    };

    if let Some(subject) = bearer_auth {
        // Standard Bearer auth path — verify deployment access via Authz.
        let deployment_id = match get_command_owner(&state, &command_id).await {
            Ok(id) => id,
            Err(e) => return e,
        };
        let deployment = match state.deployment_store.get_deployment(&deployment_id).await {
            Ok(Some(d)) => d,
            Ok(None) => return ErrorData::not_found_deployment(&deployment_id).into_response(),
            Err(e) => return e.into_response(),
        };        if !state.authz.can_act_on_deployment(&subject, &deployment) {
            return ErrorData::forbidden(
                "Access denied: only the target deployment can submit responses",
            )
            .into_response();
        }
    } else {
        // No Bearer token — try presigned URL auth from query parameters.
        let (Some(token), Some(expires)) = (&query.response_token, query.expires) else {
            return ErrorData::unauthorized("Authorization required: provide a Bearer token or a valid response_token query parameter")
                .into_response();
        };

        if !state
            .command_server
            .verify_response_token(&command_id, token, expires)
        {
            return ErrorData::unauthorized("Invalid or expired response token").into_response();
        }
    }

    match state
        .command_server
        .submit_command_response(&command_id, request.response)
        .await
    {
        Ok(()) => StatusCode::OK.into_response(),
        Err(e) => e.into_response(),
    }
}

/// Get command payload (params and response) from KV.
///
/// Auth: Admin, DG (group), or own Deployment token.
async fn get_command_payload(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(command_id): Path<String>,
) -> Response {
    let subject = match auth::require_auth(&state, &headers).await {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };
    // Verify the caller has access to this command's deployment via Authz.
    // If the command isn't in the local registry (e.g. when command metadata
    // is managed externally), fall back to requiring workspace-write
    // authority. A *registry lookup error* must NOT trigger that fallback —
    // a transient store error for a deployment-owned command would otherwise
    // expose its payload to any workspace-admin/member token.
    match state
        .command_server
        .get_command_deployment_id(&command_id)
        .await
    {
        Ok(Some(deployment_id)) => {
            if let Err(e) = require_command_access(&state, &subject, &deployment_id).await {
                return e;
            }
        }
        Ok(None) => {
            // No canonical owner in the local registry — only workspace-wide
            // writers may inspect such payloads.
            if !matches!(subject.scope, crate::auth::Scope::Workspace)
                || !matches!(
                    subject.role,
                    crate::auth::Role::WorkspaceAdmin | crate::auth::Role::WorkspaceMember
                )
            {
                return ErrorData::forbidden("Workspace-write access required").into_response();
            }
        }
        Err(e) => return e.into_response(),
    }

    let params = match state.command_server.get_params(&command_id).await {
        Ok(p) => p,
        Err(e) => return e.into_response(),
    };
    let response = match state.command_server.get_response(&command_id).await {
        Ok(r) => r,
        Err(e) => return e.into_response(),
    };

    if params.is_none() && response.is_none() {
        return alien_error::AlienError::new(alien_commands::error::ErrorData::CommandNotFound {
            command_id: command_id.clone(),
        })
        .into_response();
    }

    Json(CommandPayloadResponse {
        command_id,
        params,
        response,
    })
    .into_response()
}

/// Store command payload directly (bypasses command lifecycle).
///
/// Auth: Admin only.
async fn store_command_payload(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(command_id): Path<String>,
    Json(request): Json<StorePayloadRequest>,
) -> Response {
    let subject = match auth::require_auth(&state, &headers).await {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };    // Storing payload with no entity context is workspace-write only.
    if !matches!(subject.scope, crate::auth::Scope::Workspace)
        || !matches!(
            subject.role,
            crate::auth::Role::WorkspaceAdmin | crate::auth::Role::WorkspaceMember
        )
    {
        return ErrorData::forbidden("Workspace-write access required").into_response();
    }

    if let Some(params) = &request.params {
        if let Err(e) = state.command_server.store_params(&command_id, params).await {
            return e.into_response();
        }
    }

    if let Some(response) = &request.response {
        if let Err(e) = state
            .command_server
            .store_response(&command_id, response)
            .await
        {
            return e.into_response();
        }
    }

    StatusCode::OK.into_response()
}

/// Acquire leases for polling deployments.
///
/// Auth: Admin or Deployment token for the requested deployment_id.
async fn acquire_leases(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(lease_request): Json<LeaseRequest>,
) -> Response {
    let subject = match auth::require_auth(&state, &headers).await {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };
    let deployment = match state.deployment_store.get_deployment(&lease_request.deployment_id).await {
        Ok(Some(d)) => d,
        Ok(None) => {
            return ErrorData::not_found_deployment(&lease_request.deployment_id).into_response()
        }
        Err(e) => return e.into_response(),
    };

    if !state.authz.can_act_on_deployment(&subject, &deployment) {
        return ErrorData::forbidden("Access denied: can only acquire leases for own deployment")
            .into_response();
    }

    match state
        .command_server
        .acquire_lease(&lease_request.deployment_id, &lease_request)
        .await
    {
        Ok(response) => Json(response).into_response(),
        Err(e) => e.into_response(),
    }
}

/// Release a lease.
///
/// Auth: Any authenticated caller.
async fn release_lease(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(lease_id): Path<String>,
) -> Response {
    if let Err(e) = auth::require_auth(&state, &headers).await {
        return e.into_response();
    }

    match state
        .command_server
        .release_lease_by_id(&lease_id)
        .await
    {
        Ok(()) => StatusCode::OK.into_response(),
        Err(e) => e.into_response(),
    }
}
