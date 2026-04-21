//! Releases REST API endpoints.

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};

use std::collections::HashMap;

use alien_core::{Platform, Stack};

use crate::error::ErrorData;
use crate::traits::{CreateReleaseParams, ReleaseRecord};

use super::{auth, AppState};

// --- Request / Response types ---

/// The release API accepts stacks keyed by platform.
/// Only one platform stack needs to be present.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct StackByPlatform {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aws: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gcp: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub azure: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kubernetes: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub test: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct CreateReleaseRequest {
    pub stack: StackByPlatform,
    #[serde(default)]
    pub git_metadata: Option<GitMetadata>,
    #[serde(default)]
    pub project: Option<String>,
}

#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct GitMetadata {
    pub commit_sha: Option<String>,
    pub commit_ref: Option<String>,
    pub commit_message: Option<String>,
}

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ReleaseResponse {
    pub id: String,
    pub stack: StackByPlatform,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_metadata: Option<GitMetadataResponse>,
    pub created_at: String,
}

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct GitMetadataResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit_sha: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit_ref: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit_message: Option<String>,
}

// --- Router ---

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/v1/releases", post(create_release))
        .route("/v1/releases/latest", get(get_latest_release))
        .route("/v1/releases/{id}", get(get_release))
}

// --- Helpers ---

fn record_to_response(r: &ReleaseRecord) -> ReleaseResponse {
    let mut sbp = StackByPlatform {
        aws: None,
        gcp: None,
        azure: None,
        kubernetes: None,
        local: None,
        test: None,
    };

    for (platform, stack) in &r.stacks {
        let v = serde_json::to_value(stack).unwrap_or_default();
        match platform {
            Platform::Aws => sbp.aws = Some(v),
            Platform::Gcp => sbp.gcp = Some(v),
            Platform::Azure => sbp.azure = Some(v),
            Platform::Kubernetes => sbp.kubernetes = Some(v),
            Platform::Local => sbp.local = Some(v),
            Platform::Test => sbp.test = Some(v),
        }
    }

    let git_metadata = if r.git_commit_sha.is_some()
        || r.git_commit_ref.is_some()
        || r.git_commit_message.is_some()
    {
        Some(GitMetadataResponse {
            commit_sha: r.git_commit_sha.clone(),
            commit_ref: r.git_commit_ref.clone(),
            commit_message: r.git_commit_message.clone(),
        })
    } else {
        None
    };

    ReleaseResponse {
        id: r.id.clone(),
        stack: sbp,
        git_metadata,
        created_at: r.created_at.to_rfc3339(),
    }
}

/// Parse all non-null platform stacks from the request.
fn parse_stacks_from_request(
    stack: &StackByPlatform,
) -> std::result::Result<HashMap<Platform, Stack>, alien_error::AlienError<ErrorData>> {
    let platforms: [(Platform, &Option<serde_json::Value>); 6] = [
        (Platform::Aws, &stack.aws),
        (Platform::Gcp, &stack.gcp),
        (Platform::Azure, &stack.azure),
        (Platform::Kubernetes, &stack.kubernetes),
        (Platform::Local, &stack.local),
        (Platform::Test, &stack.test),
    ];

    let mut stacks = HashMap::new();
    for (platform, value) in &platforms {
        if let Some(v) = value {
            let parsed: Stack = serde_json::from_value(v.clone())
                .map_err(|e| ErrorData::bad_request(format!("Invalid stack format for {}: {}", platform, e)))?;
            stacks.insert(*platform, parsed);
        }
    }

    if stacks.is_empty() {
        return Err(ErrorData::bad_request("No platform stack provided"));
    }

    Ok(stacks)
}

// --- Handlers ---

#[cfg_attr(feature = "openapi", utoipa::path(
    post,
    path = "/v1/releases",
    tag = "releases",
    request_body = CreateReleaseRequest,
    responses(
        (status = 201, description = "Release created successfully", body = ReleaseResponse)
    ),
    security(
        ("bearer" = [])
    )
))]
async fn create_release(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<CreateReleaseRequest>,
) -> Response {
    tracing::info!(project = ?req.project, "Received create release request");

    let subject = match auth::require_auth(&state, &headers).await {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("Auth failed for create release: {}", e);
            return e.into_response();
        }
    };

    if let Err(e) = auth::require_admin(&subject) {
        return e.into_response();
    }

    // Parse all platform stacks from request
    let stacks = match parse_stacks_from_request(&req.stack) {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };

    let (git_sha, git_ref, git_msg) = match &req.git_metadata {
        Some(gm) => (
            gm.commit_sha.clone(),
            gm.commit_ref.clone(),
            gm.commit_message.clone(),
        ),
        None => (None, None, None),
    };

    // Extract bearer token for forwarding (platform mode token passthrough)
    let caller_token = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|s| s.to_string());

    let release = match state
        .release_store
        .create_release(CreateReleaseParams {
            project: req.project,
            caller_token,
            stacks,
            git_commit_sha: git_sha,
            git_commit_ref: git_ref,
            git_commit_message: git_msg,
        })
        .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("Release store create_release failed: {}", e);
            return e.into_response();
        }
    };

    // Set desired_release_id on eligible deployments
    if let Err(e) = state
        .deployment_store
        .set_desired_release(&release.id, None)
        .await
    {
        tracing::warn!(error = %e, "Failed to set desired release on deployments");
    }

    (
        StatusCode::CREATED,
        Json(record_to_response(&release)),
    )
        .into_response()
}

#[cfg_attr(feature = "openapi", utoipa::path(
    get,
    path = "/v1/releases/{id}",
    tag = "releases",
    params(
        ("id" = String, Path, description = "Release ID")
    ),
    responses(
        (status = 200, description = "Release found", body = ReleaseResponse),
        (status = 404, description = "Release not found")
    ),
    security(
        ("bearer" = [])
    )
))]
async fn get_release(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    let _subject = match auth::require_auth(&state, &headers).await {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };

    match state.release_store.get_release(&id).await {
        Ok(Some(r)) => Json(record_to_response(&r)).into_response(),
        Ok(None) => ErrorData::not_found_release(&id).into_response(),
        Err(e) => e.into_response(),
    }
}

#[cfg_attr(feature = "openapi", utoipa::path(
    get,
    path = "/v1/releases/latest",
    tag = "releases",
    responses(
        (status = 200, description = "Latest release found", body = ReleaseResponse),
        (status = 404, description = "No releases found")
    ),
    security(
        ("bearer" = [])
    )
))]
async fn get_latest_release(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let _subject = match auth::require_auth(&state, &headers).await {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };

    match state.release_store.get_latest_release().await {
        Ok(Some(r)) => Json(record_to_response(&r)).into_response(),
        Ok(None) => ErrorData::not_found_release("latest").into_response(),
        Err(e) => e.into_response(),
    }
}
