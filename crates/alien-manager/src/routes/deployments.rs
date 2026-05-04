//! Deployment REST API endpoints.

use axum::{
    extract::{Path, Query, State},
    http::{request::Parts, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};

use alien_core::{ContainerOutputs, EnvironmentVariable, FunctionOutputs, Platform, StackSettings};

use crate::error::ErrorData;
use crate::ids;
use crate::traits::{
    CreateDeploymentParams, CreateTokenParams, DeploymentFilter, DeploymentRecord, TokenType,
};

use super::{auth, AppState};

// --- Request / Response types ---

#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct CreateDeploymentRequest {
    pub name: String,
    pub platform: Platform,
    pub deployment_group_id: Option<String>,
    #[serde(default)]
    pub stack_settings: Option<StackSettings>,
    #[serde(default)]
    pub environment_variables: Option<Vec<EnvironmentVariable>>,
}

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct CreateDeploymentResponse {
    pub deployment: DeploymentResponse,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
}

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct DeploymentResponse {
    pub id: String,
    pub name: String,
    pub platform: Platform,
    pub status: String,
    pub deployment_group_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stack_settings: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stack_state: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment_info: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_metadata: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_release_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub desired_release_id: Option<String>,
    pub retry_requested: bool,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deployment_group: Option<DeploymentGroupMinimal>,
}

#[derive(Debug, Serialize, Clone)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct DeploymentGroupMinimal {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ListDeploymentsResponse {
    pub items: Vec<DeploymentResponse>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ListDeploymentsQuery {
    pub deployment_group_id: Option<String>,
    #[serde(default)]
    pub include: Vec<String>,
}

// Custom extractor for repeated query params like ?include[]=a&include[]=b
impl<S: Send + Sync> axum::extract::FromRequestParts<S> for ListDeploymentsQuery {
    type Rejection = (StatusCode, String);

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let query_string = parts.uri.query().unwrap_or("");
        let mut deployment_group_id: Option<String> = None;
        let mut include: Vec<String> = Vec::new();

        for (key, value) in form_urlencoded::parse(query_string.as_bytes()) {
            match key.as_ref() {
                "deploymentGroupId" => deployment_group_id = Some(value.into_owned()),
                "include" | "include[]" => include.push(value.into_owned()),
                _ => {}
            }
        }

        Ok(ListDeploymentsQuery {
            deployment_group_id,
            include,
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteQuery {
    #[serde(default)]
    pub force: bool,
}

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct DeploymentInfoResponse {
    pub commands: CommandsInfo,
    pub resources: std::collections::HashMap<String, ResourceEntry>,
    pub status: String,
    pub platform: Platform,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct CommandsInfo {
    pub url: String,
    pub deployment_id: String,
}

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ResourceEntry {
    pub resource_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub public_url: Option<String>,
}

// --- Router ---

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/v1/deployments",
            post(create_deployment).get(list_deployments),
        )
        .route(
            "/v1/deployments/{id}",
            get(get_deployment).delete(delete_deployment),
        )
        .route("/v1/deployments/{id}/info", get(get_deployment_info))
        .route("/v1/deployments/{id}/retry", post(retry_deployment))
        .route("/v1/deployments/{id}/redeploy", post(redeploy))
}

// --- Helpers ---

fn record_to_response(
    r: &DeploymentRecord,
    deployment_group: Option<DeploymentGroupMinimal>,
) -> DeploymentResponse {
    DeploymentResponse {
        id: r.id.clone(),
        name: r.name.clone(),
        platform: r.platform.clone(),
        status: r.status.clone(),
        deployment_group_id: r.deployment_group_id.clone(),
        stack_settings: Some(serde_json::to_value(&r.stack_settings).unwrap_or_default()),
        stack_state: r
            .stack_state
            .as_ref()
            .map(|s| serde_json::to_value(s).unwrap_or_default()),
        environment_info: r
            .environment_info
            .as_ref()
            .map(|e| serde_json::to_value(e).unwrap_or_default()),
        runtime_metadata: r
            .runtime_metadata
            .as_ref()
            .map(|m| serde_json::to_value(m).unwrap_or_default()),
        current_release_id: r.current_release_id.clone(),
        desired_release_id: r.desired_release_id.clone(),
        retry_requested: r.retry_requested,
        created_at: r.created_at.to_rfc3339(),
        updated_at: r.updated_at.map(|u| u.to_rfc3339()),
        error: r.error.clone(),
        deployment_group,
    }
}

// --- Handlers ---

/// Every handler in this file runs `auth::require_auth(&state, &headers)`
/// and then threads `&subject` into the `DeploymentStore` calls. Embedders
/// that proxy to an upstream API can use the subject's `bearer_token` for
/// passthrough; single-tenant impls ignore it. See the trait doc on
/// [`DeploymentStore`] for the full convention.
///
/// `POST /v1/deployments` — Inbound: workspace / project / dg bearer (or
/// authenticated user). Deployment-scoped tokens cannot create deployments.
#[cfg_attr(feature = "openapi", utoipa::path(
    post,
    path = "/v1/deployments",
    tag = "deployments",
    request_body = CreateDeploymentRequest,
    responses(
        (status = 201, description = "Deployment created", body = CreateDeploymentResponse),
        (status = 400, description = "Bad request"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden"),
    )
))]
async fn create_deployment(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<CreateDeploymentRequest>,
) -> Response {
    let subject = match auth::require_auth(&state, &headers).await {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };

    // Determine deployment group from the subject's scope or the explicit
    // request body. DG-scoped subjects use their own group; workspace/project
    // scopes accept the body's `deployment_group_id` (or auto-select the
    // first available one); deployment-scoped subjects can't create.
    let deployment_group_id = match &subject.scope {
        crate::auth::Scope::DeploymentGroup {
            deployment_group_id,
            ..
        } => deployment_group_id.clone(),
        crate::auth::Scope::Workspace | crate::auth::Scope::Project { .. } => {
            match &req.deployment_group_id {
                Some(id) => id.clone(),
                None => match state.deployment_store.list_deployment_groups(&subject).await {
                    Ok(groups) if !groups.is_empty() => groups[0].id.clone(),
                    Ok(_) => {
                        return ErrorData::bad_request(
                            "No deployment groups exist. Create one first.",
                        )
                        .into_response()
                    }
                    Err(e) => return e.into_response(),
                },
            }
        }
        crate::auth::Scope::Deployment { .. } => {
            return ErrorData::forbidden("Deployment tokens cannot create deployments")
                .into_response()
        }
    };

    // Verify deployment group exists
    let dg = match state
        .deployment_store
        .get_deployment_group(&subject, &deployment_group_id)
        .await
    {
        Ok(Some(dg)) => dg,
        Ok(None) => return ErrorData::not_found_group(&deployment_group_id).into_response(),
        Err(e) => return e.into_response(),
    };

    let create_ctx = crate::auth::DeploymentCreateCtx {
        workspace_id: &dg.workspace_id,
        project_id: &dg.project_id,
        deployment_group_id: Some(&deployment_group_id),
    };
    if !state.authz.can_create_deployment(&subject, create_ctx) {
        return ErrorData::forbidden("Cannot create deployment in this group").into_response();
    }

    if dg.deployment_count >= dg.max_deployments {
        return alien_error::AlienError::new(ErrorData::MaxDeploymentsReached {
            deployment_group_id: deployment_group_id.clone(),
            max_deployments: dg.max_deployments,
        })
        .into_response();
    }

    // Auto-assign latest release if available
    let desired_release_id = match state.release_store.get_latest_release(&subject).await {
        Ok(Some(release)) => Some(release.id),
        Ok(None) => None,
        Err(e) => return e.into_response(),
    };

    // Create the deployment first (token is set after).
    let (raw_token, key_prefix, key_hash) = ids::generate_token(TokenType::Deployment.prefix());

    let mut deployment = match state
        .deployment_store
        .create_deployment(
            &subject,
            CreateDeploymentParams {
                name: req.name,
                deployment_group_id: deployment_group_id.clone(),
                platform: req.platform,
                stack_settings: req.stack_settings.unwrap_or_default(),
                environment_variables: req.environment_variables,
                deployment_token: Some(raw_token.clone()),
            },
        )
        .await
    {
        Ok(d) => d,
        Err(e) => return e.into_response(),
    };

    // Create the deployment token with the deployment_id so the proxy can
    // scope pull access to this specific deployment's release images.
    match state
        .token_store
        .create_token(CreateTokenParams {
            token_type: TokenType::Deployment,
            key_prefix,
            key_hash,
            deployment_group_id: Some(deployment_group_id.clone()),
            deployment_id: Some(deployment.id.clone()),
        })
        .await
    {
        Ok(_) => {}
        Err(e) => return e.into_response(),
    };

    // Set the desired release if we found one
    if let Some(ref release_id) = desired_release_id {
        if let Err(e) = state
            .deployment_store
            .set_deployment_desired_release(&subject, &deployment.id, release_id)
            .await
        {
            return e.into_response();
        }
        deployment.desired_release_id = desired_release_id;
    }

    let token = Some(raw_token);

    (
        StatusCode::CREATED,
        Json(CreateDeploymentResponse {
            deployment: record_to_response(&deployment, None),
            token,
        }),
    )
        .into_response()
}

#[cfg_attr(feature = "openapi", utoipa::path(
    get,
    path = "/v1/deployments",
    tag = "deployments",
    params(
        ("deploymentGroupId" = Option<String>, Query, description = "Filter by deployment group ID"),
        ("include" = Option<Vec<String>>, Query, description = "Include related resources (e.g. deploymentGroup)"),
    ),
    responses(
        (status = 200, description = "List of deployments", body = ListDeploymentsResponse),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden"),
    )
))]
async fn list_deployments(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: ListDeploymentsQuery,
) -> Response {
    let subject = match auth::require_auth(&state, &headers).await {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };

    // Determine filter based on the subject's scope.
    let deployment_group_id = match &subject.scope {
        crate::auth::Scope::DeploymentGroup {
            deployment_group_id,
            ..
        } => Some(deployment_group_id.clone()),
        crate::auth::Scope::Workspace | crate::auth::Scope::Project { .. } => {
            query.deployment_group_id.clone()
        }
        crate::auth::Scope::Deployment { .. } => {
            return ErrorData::forbidden("Deployment tokens cannot list deployments")
                .into_response()
        }
    };

    let filter = DeploymentFilter {
        deployment_group_id,
        ..Default::default()
    };

    let deployments = match state.deployment_store.list_deployments(&subject, &filter).await {
        Ok(d) => d,
        Err(e) => return e.into_response(),
    };

    let include_dg = query.include.iter().any(|i| i == "deploymentGroup");

    let mut dg_cache: std::collections::HashMap<String, Option<DeploymentGroupMinimal>> =
        std::collections::HashMap::new();

    let mut items = Vec::with_capacity(deployments.len());
    for d in &deployments {
        let dg_minimal = if include_dg {
            if let Some(cached) = dg_cache.get(&d.deployment_group_id) {
                cached.clone()
            } else {
                let minimal = match state
                    .deployment_store
                    .get_deployment_group(&subject, &d.deployment_group_id)
                    .await
                {
                    Ok(Some(dg)) => Some(DeploymentGroupMinimal {
                        id: dg.id,
                        name: dg.name,
                    }),
                    _ => None,
                };
                dg_cache.insert(d.deployment_group_id.clone(), minimal.clone());
                minimal
            }
        } else {
            None
        };
        items.push(record_to_response(d, dg_minimal));
    }

    Json(ListDeploymentsResponse {
        items,
        next_cursor: None,
    })
    .into_response()
}

#[cfg_attr(feature = "openapi", utoipa::path(
    get,
    path = "/v1/deployments/{id}",
    tag = "deployments",
    params(
        ("id" = String, Path, description = "Deployment ID"),
    ),
    responses(
        (status = 200, description = "Deployment details", body = DeploymentResponse),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden"),
        (status = 404, description = "Not found"),
    )
))]
async fn get_deployment(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    let subject = match auth::require_auth(&state, &headers).await {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };

    let deployment = match state.deployment_store.get_deployment(&subject, &id).await {
        Ok(Some(d)) => d,
        Ok(None) => return ErrorData::not_found_deployment(&id).into_response(),
        Err(e) => return e.into_response(),
    };    if !state.authz.can_read_deployment(&subject, &deployment) {
        return ErrorData::forbidden("Access denied").into_response();
    }

    Json(record_to_response(&deployment, None)).into_response()
}

#[cfg_attr(feature = "openapi", utoipa::path(
    get,
    path = "/v1/deployments/{id}/info",
    tag = "deployments",
    params(
        ("id" = String, Path, description = "Deployment ID"),
    ),
    responses(
        (status = 200, description = "Deployment info", body = DeploymentInfoResponse),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden"),
        (status = 404, description = "Not found"),
    )
))]
async fn get_deployment_info(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    let subject = match auth::require_auth(&state, &headers).await {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };

    let deployment = match state.deployment_store.get_deployment(&subject, &id).await {
        Ok(Some(d)) => d,
        Ok(None) => return ErrorData::not_found_deployment(&id).into_response(),
        Err(e) => return e.into_response(),
    };    if !state.authz.can_read_deployment(&subject, &deployment) {
        return ErrorData::forbidden("Access denied").into_response();
    }

    // Extract resources from stack_state
    let mut resources = std::collections::HashMap::new();
    if let Some(stack_state) = &deployment.stack_state {
        for (resource_id, resource_state) in &stack_state.resources {
            let public_url = match resource_state.resource_type.as_str() {
                "function" => stack_state
                    .get_resource_outputs::<FunctionOutputs>(resource_id)
                    .ok()
                    .and_then(|o| o.url.clone()),
                "container" => stack_state
                    .get_resource_outputs::<ContainerOutputs>(resource_id)
                    .ok()
                    .and_then(|o| o.url.clone()),
                _ => None,
            };
            resources.insert(
                resource_id.clone(),
                ResourceEntry {
                    resource_type: resource_state.resource_type.clone(),
                    public_url,
                },
            );
        }
    }

    Json(DeploymentInfoResponse {
        commands: CommandsInfo {
            url: state.config.base_url(),
            deployment_id: deployment.id.clone(),
        },
        resources,
        status: deployment.status,
        platform: deployment.platform,
        error: deployment.error,
    })
    .into_response()
}

#[cfg_attr(feature = "openapi", utoipa::path(
    delete,
    path = "/v1/deployments/{id}",
    tag = "deployments",
    params(
        ("id" = String, Path, description = "Deployment ID"),
        ("force" = Option<bool>, Query, description = "Force delete without running cleanup (immediately removes record)"),
    ),
    responses(
        (status = 202, description = "Deployment deletion enqueued"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden"),
        (status = 404, description = "Not found"),
    )
))]
async fn delete_deployment(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(query): Query<DeleteQuery>,
) -> Response {
    let subject = match auth::require_auth(&state, &headers).await {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };

    let deployment = match state.deployment_store.get_deployment(&subject, &id).await {
        Ok(Some(d)) => d,
        Ok(None) => return ErrorData::not_found_deployment(&id).into_response(),
        Err(e) => return e.into_response(),
    };    if !state.authz.can_delete_deployment(&subject, &deployment) {
        return ErrorData::forbidden("Cannot delete deployment").into_response();
    }

    if query.force {
        if let Err(e) = state.deployment_store.delete_deployment(&subject, &id).await {
            return e.into_response();
        }
    } else {
        if let Err(e) = state.deployment_store.set_delete_pending(&subject, &id).await {
            return e.into_response();
        }
    }

    (
        StatusCode::ACCEPTED,
        Json(serde_json::json!({
            "message": "Deployment deletion enqueued"
        })),
    )
        .into_response()
}

#[cfg_attr(feature = "openapi", utoipa::path(
    post,
    path = "/v1/deployments/{id}/retry",
    tag = "deployments",
    params(
        ("id" = String, Path, description = "Deployment ID"),
    ),
    responses(
        (status = 200, description = "Retry requested"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden"),
        (status = 404, description = "Not found"),
    )
))]
async fn retry_deployment(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    let subject = match auth::require_auth(&state, &headers).await {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };

    let deployment = match state.deployment_store.get_deployment(&subject, &id).await {
        Ok(Some(d)) => d,
        Ok(None) => return ErrorData::not_found_deployment(&id).into_response(),
        Err(e) => return e.into_response(),
    };    if !state.authz.can_update_deployment(&subject, &deployment) {
        return ErrorData::forbidden("Cannot retry deployment").into_response();
    }

    if let Err(e) = state.deployment_store.set_retry_requested(&subject, &id).await {
        return e.into_response();
    }

    Json(serde_json::json!({ "success": true })).into_response()
}

#[cfg_attr(feature = "openapi", utoipa::path(
    post,
    path = "/v1/deployments/{id}/redeploy",
    tag = "deployments",
    params(
        ("id" = String, Path, description = "Deployment ID"),
    ),
    responses(
        (status = 200, description = "Redeploy requested"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden"),
        (status = 404, description = "Not found"),
    )
))]
async fn redeploy(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    let subject = match auth::require_auth(&state, &headers).await {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };

    let deployment = match state.deployment_store.get_deployment(&subject, &id).await {
        Ok(Some(d)) => d,
        Ok(None) => return ErrorData::not_found_deployment(&id).into_response(),
        Err(e) => return e.into_response(),
    };    if !state.authz.can_update_deployment(&subject, &deployment) {
        return ErrorData::forbidden("Cannot redeploy").into_response();
    }

    if let Err(e) = state.deployment_store.set_redeploy(&subject, &id).await {
        return e.into_response();
    }

    Json(serde_json::json!({ "success": true })).into_response()
}
