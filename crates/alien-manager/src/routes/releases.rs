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
use alien_preflights::compile_time::endpoint_host_label_conflicts;

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
    pub machines: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub test: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateReleaseRequest {
    pub stack: StackByPlatform,
    #[serde(default)]
    pub git_metadata: Option<GitMetadata>,
    /// Project this release belongs to. Required. The standalone server
    /// uses the canonical value `"default"`.
    ///
    /// The OSS CLI sends this field as `project` on `alien release`
    /// (see `alien-cli` release flow); the underlying alien-managerx
    /// release endpoint accepts both forms. Accept both here too so
    /// `alien release` against an OSS standalone manager doesn't fail
    /// at the schema layer with a confusing
    /// `unknown field "project", expected "projectId"`.
    #[serde(alias = "project")]
    pub project_id: String,
    /// Channel the release advances; `production` when absent. Deployments
    /// following the channel roll out to it.
    #[serde(default)]
    pub channel: Option<String>,
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
    pub workspace_id: String,
    pub project_id: String,
    pub stack: StackByPlatform,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_metadata: Option<GitMetadataResponse>,
    pub created_at: String,
    /// Setup-step fingerprints — used by `alien release` to short-circuit
    /// re-pushing artifacts that haven't changed across releases. The
    /// platform-API client requires this field to be present (even if
    /// empty). The OSS standalone manager doesn't track per-setup-step
    /// fingerprints, so we return an empty map.
    #[serde(default)]
    pub setup_fingerprints: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ListReleasesResponse {
    /// Releases the caller may read, newest first.
    pub items: Vec<ReleaseResponse>,
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
        .route("/v1/releases", post(create_release).get(list_releases))
        .route("/v1/releases/latest", get(get_latest_release))
        .route("/v1/releases/{id}", get(get_release))
}

// --- Helpers ---

fn record_to_response(
    r: &ReleaseRecord,
) -> std::result::Result<ReleaseResponse, alien_error::AlienError<ErrorData>> {
    let mut sbp = StackByPlatform {
        aws: None,
        gcp: None,
        azure: None,
        kubernetes: None,
        machines: None,
        local: None,
        test: None,
    };

    for (platform, stack) in &r.stacks {
        let v = serde_json::to_value(stack).map_err(|e| {
            ErrorData::internal(format!(
                "Failed to serialize stack for platform {}: {}",
                platform, e
            ))
        })?;
        match platform {
            Platform::Aws => sbp.aws = Some(v),
            Platform::Gcp => sbp.gcp = Some(v),
            Platform::Azure => sbp.azure = Some(v),
            Platform::Kubernetes => sbp.kubernetes = Some(v),
            Platform::Machines => sbp.machines = Some(v),
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

    Ok(ReleaseResponse {
        id: r.id.clone(),
        workspace_id: r.workspace_id.clone(),
        project_id: r.project_id.clone(),
        stack: sbp,
        git_metadata,
        created_at: r.created_at.to_rfc3339(),
        setup_fingerprints: serde_json::Map::new(),
    })
}

/// Parse all non-null platform stacks from the request.
fn parse_stacks_from_request(
    stack: &StackByPlatform,
) -> std::result::Result<HashMap<Platform, Stack>, alien_error::AlienError<ErrorData>> {
    let platforms: [(Platform, &Option<serde_json::Value>); 7] = [
        (Platform::Aws, &stack.aws),
        (Platform::Gcp, &stack.gcp),
        (Platform::Azure, &stack.azure),
        (Platform::Kubernetes, &stack.kubernetes),
        (Platform::Machines, &stack.machines),
        (Platform::Local, &stack.local),
        (Platform::Test, &stack.test),
    ];

    let mut stacks = HashMap::new();
    for (platform, value) in &platforms {
        if let Some(v) = value {
            let parsed: Stack = serde_json::from_value(v.clone()).map_err(|e| {
                ErrorData::bad_request(format!("Invalid stack format for {}: {}", platform, e))
            })?;
            stacks.insert(*platform, parsed);
        }
    }

    if stacks.is_empty() {
        return Err(ErrorData::bad_request("No platform stack provided"));
    }

    Ok(stacks)
}

/// Refuse a new release whose public endpoints would share a generated hostname.
///
/// Only new releases are refused: deployments of releases created before this check keep
/// updating, so it must not move into deployment-time preflights.
fn refuse_endpoint_host_label_conflicts(
    stacks: &HashMap<Platform, Stack>,
) -> std::result::Result<(), alien_error::AlienError<ErrorData>> {
    let mut platforms: Vec<&Platform> = stacks.keys().collect();
    platforms.sort_by_key(|platform| platform.as_str());
    for platform in platforms {
        let conflicts = endpoint_host_label_conflicts(&stacks[platform], *platform);
        if !conflicts.is_empty() {
            return Err(ErrorData::bad_request(format!(
                "Invalid stack for {platform}: {}",
                conflicts.join(" ")
            )));
        }
    }
    Ok(())
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
    let subject = match auth::require_auth(&state, &headers).await {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("Auth failed for create release: {}", e);
            return e.into_response();
        }
    };

    if !state.authz.can_create_release(&subject, &req.project_id) {
        return ErrorData::forbidden("Cannot create release in this project").into_response();
    }

    tracing::info!(project_id = %req.project_id, "Received create release request");

    let channel = req
        .channel
        .clone()
        .unwrap_or_else(|| crate::traits::DEFAULT_CHANNEL.to_string());
    if !super::channels::valid_channel_name(&channel) {
        return ErrorData::bad_request(format!("Invalid channel name '{channel}'")).into_response();
    }
    if let Some(channels) = &state.release_channels {
        match channels.get_channel(&channel).await {
            Ok(None) if channel != crate::traits::DEFAULT_CHANNEL => {
                return ErrorData::bad_request(format!(
                    "No channel named '{channel}'; create it with `alien releases create-channel {channel}`"
                ))
                .into_response()
            }
            Ok(_) => {}
            Err(e) => return e.into_response(),
        }
    } else if channel != crate::traits::DEFAULT_CHANNEL {
        return ErrorData::bad_request("This manager doesn't support release channels")
            .into_response();
    }

    // Parse all platform stacks from request
    let stacks = match parse_stacks_from_request(&req.stack) {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };
    if let Err(e) = refuse_endpoint_host_label_conflicts(&stacks) {
        return e.into_response();
    }

    let (git_sha, git_ref, git_msg) = match &req.git_metadata {
        Some(gm) => (
            gm.commit_sha.clone(),
            gm.commit_ref.clone(),
            gm.commit_message.clone(),
        ),
        None => (None, None, None),
    };

    let release = match state
        .release_store
        .create_release(
            &subject,
            CreateReleaseParams {
                project_id: req.project_id,
                stacks,
                git_commit_sha: git_sha,
                git_commit_ref: git_ref,
                git_commit_message: git_msg,
            },
        )
        .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("Release store create_release failed: {}", e);
            return e.into_response();
        }
    };

    // Advance the release's channel and roll it out to the deployments
    // following it (every deployment on a manager without channels).
    if let Err(e) =
        super::channels::route_new_release(&state, &subject, &release.id, &channel).await
    {
        return e.into_response();
    }

    let response = match record_to_response(&release) {
        Ok(r) => r,
        Err(e) => return e.into_response(),
    };

    (StatusCode::CREATED, Json(response)).into_response()
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
        (status = 404, description = "Release not found", body = alien_error::AlienError)
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
    let subject = match auth::require_auth(&state, &headers).await {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };

    let release = match state.release_store.get_release(&subject, &id).await {
        Ok(Some(r)) => r,
        Ok(None) => return ErrorData::not_found_release(&id).into_response(),
        Err(e) => return e.into_response(),
    };

    if !state.authz.can_read_release(&subject, &release) {
        return ErrorData::forbidden("Cannot read release").into_response();
    }

    match record_to_response(&release) {
        Ok(resp) => Json(resp).into_response(),
        Err(e) => e.into_response(),
    }
}

/// `GET /v1/releases` — Inbound: workspace / project bearer (or authenticated
/// user). Outbound: caller bearer (passthrough). Returns only releases the
/// caller may read.
#[cfg_attr(feature = "openapi", utoipa::path(
    get,
    path = "/v1/releases",
    tag = "releases",
    responses(
        (status = 200, description = "Releases listed", body = ListReleasesResponse),
        (status = 401, description = "Unauthorized")
    ),
    security(
        ("bearer" = [])
    )
))]
async fn list_releases(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let subject = match auth::require_auth(&state, &headers).await {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };

    let releases = match state.release_store.list_releases(&subject).await {
        Ok(r) => r,
        Err(e) => return e.into_response(),
    };

    // Like get_release, return only the releases the caller may read.
    let mut items = Vec::with_capacity(releases.len());
    for release in &releases {
        if !state.authz.can_read_release(&subject, release) {
            continue;
        }
        match record_to_response(release) {
            Ok(resp) => items.push(resp),
            Err(e) => return e.into_response(),
        }
    }

    Json(ListReleasesResponse { items }).into_response()
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
    let subject = match auth::require_auth(&state, &headers).await {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };

    let release = match state.release_store.get_latest_release(&subject).await {
        Ok(Some(r)) => r,
        Ok(None) => return ErrorData::not_found_release("latest").into_response(),
        Err(e) => return e.into_response(),
    };

    if !state.authz.can_read_release(&subject, &release) {
        return ErrorData::forbidden("Cannot read release").into_response();
    }

    match record_to_response(&release) {
        Ok(resp) => Json(resp).into_response(),
        Err(e) => e.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stack_by_platform(aws: serde_json::Value) -> StackByPlatform {
        serde_json::from_value(serde_json::json!({ "aws": aws })).expect("stack by platform")
    }

    /// A container as `alien release` serializes it.
    fn container(id: &str, endpoint: &str) -> serde_json::Value {
        serde_json::json!({
            "config": {
                "id": id,
                "type": "container",
                "code": { "type": "image", "image": format!("example.test/{id}:latest") },
                "cpu": { "min": "0.25", "desired": "0.25" },
                "memory": { "min": "512Mi", "desired": "512Mi" },
                "links": [],
                "ports": [{ "port": 8080 }],
                "replicas": 1,
                "stateful": false,
                "environment": {},
                "healthCheck": { "path": "/health", "method": "GET", "timeoutSeconds": 2, "failureThreshold": 3 },
                "permissions": "app",
                "commandsEnabled": false,
                "publicEndpoints": [
                    { "name": endpoint, "port": 8080, "protocol": "http", "wildcardSubdomains": false }
                ]
            },
            "lifecycle": "live",
            "dependencies": [],
            "remoteAccess": false
        })
    }

    /// `POST /v1/releases` parses the request with these two steps; a stack built outside
    /// `alien release` reaches the manager without the build-time preflights.
    fn validate(mut aws: serde_json::Value) -> std::result::Result<(), String> {
        aws["permissions"] = serde_json::json!({ "profiles": { "app": {} }, "management": "auto" });
        let stacks = parse_stacks_from_request(&stack_by_platform(aws)).map_err(|e| e.message)?;
        refuse_endpoint_host_label_conflicts(&stacks).map_err(|e| e.message)
    }

    #[test]
    fn new_release_with_shared_endpoint_hostname_is_refused() {
        let error = validate(serde_json::json!({
            "id": "app",
            "resources": { "gateway": container("gateway", "api"), "probe": container("probe", "api") },
        }))
        .expect_err("two 'api' endpoints must be refused");
        assert!(
            error.contains("Invalid stack for aws: Public endpoints 'api' on '"),
            "{error}"
        );
        assert!(
            error.contains("would both get hostname 'api.<deployment domain>'"),
            "{error}"
        );

        validate(serde_json::json!({
            "id": "app",
            "resources": { "gateway": container("gateway", "api"), "probe": container("probe", "probe") },
        }))
        .expect("distinct endpoint names are accepted");
    }
}
