//! Signer lists, pending changes and signed deployment config.
//!
//! - `GET/POST /v1/projects/{project}/signer-lists`
//! - `GET/POST /v1/projects/{project}/pending-changes`
//! - `GET /v1/pending-changes/{id}`, `POST /v1/pending-changes/{id}/signatures`
//! - `POST /v1/deployments/{id}/config-statements`
//!
//! The manager stores and forwards these statements; it never verifies a
//! signature, because enforcers in each deployment do. It only checks what it
//! needs to keep its own records straight: the kind, the project, and that
//! signer list versions arrive in order and never change.

use std::sync::Arc;

use alien_core::SignedStatement;
use alien_release_trust::{Kind, Signature, Signer};
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};

use super::{auth, AppState};
use crate::{
    auth::Subject,
    error::ErrorData,
    traits::{
        PendingChangeKind, PendingChangeRecord, PendingChangeStatus, SignerListWrite, SigningStore,
    },
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/v1/projects/{project}/signer-lists",
            get(list_signer_lists).post(add_signer_list),
        )
        .route(
            "/v1/projects/{project}/pending-changes",
            get(list_pending_changes).post(create_pending_change),
        )
        .route("/v1/pending-changes/{id}", get(get_pending_change))
        .route(
            "/v1/pending-changes/{id}/signatures",
            post(sign_pending_change),
        )
        .route(
            "/v1/deployments/{id}/config-statements",
            post(add_config_statement),
        )
}

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct SignerListsResponse {
    /// Every stored version, oldest first.
    pub items: Vec<SignedStatement>,
}

/// A body carrying one signed statement.
#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct StatementRequest {
    pub statement: SignedStatement,
}

/// Body of `POST /v1/projects/{project}/pending-changes`.
#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct CreatePendingChangeRequest {
    pub kind: PendingChangeKind,
    pub summary: String,
    #[serde(default)]
    pub deployment_id: Option<String>,
    /// For device changes: the signer asking to be added.
    #[serde(default)]
    #[cfg_attr(feature = "openapi", schema(value_type = Option<Object>))]
    pub device: Option<Signer>,
    /// For statement changes: base64 payload bytes to sign.
    #[serde(default)]
    pub payload: Option<String>,
    #[serde(default)]
    pub signatures: Vec<Signature>,
}

/// Body of `POST /v1/pending-changes/{id}/signatures`.
#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct SignPendingChangeRequest {
    pub signatures: Vec<Signature>,
    /// The complete signed statement, once it has every signature it needs.
    #[serde(default)]
    pub statement: Option<SignedStatement>,
}

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct PendingChangeResponse {
    pub id: String,
    pub project_id: String,
    pub kind: PendingChangeKind,
    pub status: PendingChangeStatus,
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deployment_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "openapi", schema(value_type = Option<Object>))]
    pub device: Option<Signer>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<String>,
    pub signatures: Vec<Signature>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub statement: Option<SignedStatement>,
    pub created_at: String,
}

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct PendingChangesResponse {
    pub items: Vec<PendingChangeResponse>,
}

impl From<PendingChangeRecord> for PendingChangeResponse {
    fn from(record: PendingChangeRecord) -> Self {
        Self {
            id: record.id,
            project_id: record.project_id,
            kind: record.kind,
            status: record.status,
            summary: record.summary,
            deployment_id: record.deployment_id,
            device: record.device,
            payload: record.payload,
            signatures: record.signatures,
            statement: record.statement,
            created_at: record.created_at.to_rfc3339(),
        }
    }
}

type Handler<T> = std::result::Result<T, Response>;

async fn setup(
    state: &AppState,
    headers: &HeaderMap,
    project: &str,
) -> Handler<(Subject, Arc<dyn SigningStore>)> {
    let subject = auth::require_auth(state, headers)
        .await
        .map_err(IntoResponse::into_response)?;
    let Some(store) = state.signing.clone() else {
        return Err(
            ErrorData::bad_request("signing is not available on this manager").into_response(),
        );
    };
    if !state.authz.can_create_release(&subject, project) {
        return Err(ErrorData::forbidden("Cannot manage signing for this project").into_response());
    }
    Ok((subject, store))
}

fn conflict(reason: impl Into<String>) -> Response {
    alien_error::AlienError::new(ErrorData::SignedStatementConflict {
        reason: reason.into(),
    })
    .into_response()
}

#[cfg_attr(feature = "openapi", utoipa::path(
    get,
    path = "/v1/projects/{project}/signer-lists",
    operation_id = "list_signer_lists",
    tag = "signing",
    params(("project" = String, Path, description = "Project id")),
    responses((status = 200, description = "Stored signer list versions", body = SignerListsResponse)),
    security(("bearer" = []))
))]
async fn list_signer_lists(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(project): Path<String>,
) -> Response {
    let (_, store) = match setup(&state, &headers, &project).await {
        Ok(ok) => ok,
        Err(response) => return response,
    };
    match store.signer_lists(&project).await {
        Ok(items) => Json(SignerListsResponse { items }).into_response(),
        Err(error) => error.into_response(),
    }
}

#[cfg_attr(feature = "openapi", utoipa::path(
    post,
    path = "/v1/projects/{project}/signer-lists",
    operation_id = "add_signer_list",
    tag = "signing",
    params(("project" = String, Path, description = "Project id")),
    request_body = StatementRequest,
    responses(
        (status = 201, description = "Stored"),
        (status = 200, description = "The same version was already stored"),
        (status = 409, description = "The version is out of order or differs from the stored one"),
    ),
    security(("bearer" = []))
))]
async fn add_signer_list(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(project): Path<String>,
    Json(request): Json<StatementRequest>,
) -> Response {
    let (_, store) = match setup(&state, &headers, &project).await {
        Ok(ok) => ok,
        Err(response) => return response,
    };
    let opened = match request.statement.open() {
        Ok(opened) if opened.kind == Kind::SignerList => opened,
        Ok(opened) => {
            return ErrorData::bad_request(format!(
                "A '{}' statement is not a signer list",
                opened.kind
            ))
            .into_response()
        }
        Err(error) => return ErrorData::bad_request(error.message).into_response(),
    };
    if opened.project.as_deref() != Some(project.as_str()) {
        return ErrorData::bad_request("The signer list names another project").into_response();
    }
    let stored = match store.signer_lists(&project).await {
        Ok(stored) => stored,
        Err(error) => return error.into_response(),
    };
    let next = stored.len() as u64 + 1;
    if opened.sequence > next {
        return conflict(format!(
            "Signer list version {} can't be stored before version {next}",
            opened.sequence
        ));
    }
    match store
        .add_signer_list(&project, opened.sequence, &request.statement)
        .await
    {
        Ok(SignerListWrite::Added) => StatusCode::CREATED.into_response(),
        Ok(SignerListWrite::AlreadyStored) => StatusCode::OK.into_response(),
        Ok(SignerListWrite::Conflict) => conflict(format!(
            "A different signer list version {} is already stored",
            opened.sequence
        )),
        Err(error) => error.into_response(),
    }
}

#[cfg_attr(feature = "openapi", utoipa::path(
    get,
    path = "/v1/projects/{project}/pending-changes",
    operation_id = "list_pending_changes",
    tag = "signing",
    params(("project" = String, Path, description = "Project id")),
    responses((status = 200, description = "Pending changes", body = PendingChangesResponse)),
    security(("bearer" = []))
))]
async fn list_pending_changes(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(project): Path<String>,
) -> Response {
    let (_, store) = match setup(&state, &headers, &project).await {
        Ok(ok) => ok,
        Err(response) => return response,
    };
    match store.pending_changes(&project).await {
        Ok(records) => Json(PendingChangesResponse {
            items: records
                .into_iter()
                .filter(|record| record.status == PendingChangeStatus::Pending)
                .map(Into::into)
                .collect(),
        })
        .into_response(),
        Err(error) => error.into_response(),
    }
}

#[cfg_attr(feature = "openapi", utoipa::path(
    post,
    path = "/v1/projects/{project}/pending-changes",
    operation_id = "create_pending_change",
    tag = "signing",
    params(("project" = String, Path, description = "Project id")),
    request_body = CreatePendingChangeRequest,
    responses((status = 201, description = "Created", body = PendingChangeResponse)),
    security(("bearer" = []))
))]
async fn create_pending_change(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(project): Path<String>,
    Json(request): Json<CreatePendingChangeRequest>,
) -> Response {
    let (_, store) = match setup(&state, &headers, &project).await {
        Ok(ok) => ok,
        Err(response) => return response,
    };
    let well_formed = match request.kind {
        PendingChangeKind::Device => request.device.is_some() && request.payload.is_none(),
        PendingChangeKind::Statement => request.payload.is_some() && request.device.is_none(),
    };
    if !well_formed {
        return ErrorData::bad_request(
            "A device change carries a device, and a statement change carries a payload",
        )
        .into_response();
    }
    if let Some(payload) = &request.payload {
        let unsigned = SignedStatement {
            payload: payload.clone(),
            signatures: Vec::new(),
        };
        match unsigned.open() {
            Ok(opened) if opened.project.as_deref() == Some(project.as_str()) => {}
            Ok(_) => {
                return ErrorData::bad_request("The statement names another project")
                    .into_response()
            }
            Err(error) => return ErrorData::bad_request(error.message).into_response(),
        }
    }
    let record = PendingChangeRecord {
        id: format!("pch_{}", uuid::Uuid::new_v4().simple()),
        project_id: project,
        kind: request.kind,
        status: PendingChangeStatus::Pending,
        summary: request.summary,
        deployment_id: request.deployment_id,
        device: request.device,
        payload: request.payload,
        signatures: request.signatures,
        statement: None,
        created_at: chrono::Utc::now(),
    };
    match store.create_pending_change(&record).await {
        Ok(()) => (
            StatusCode::CREATED,
            Json(PendingChangeResponse::from(record)),
        )
            .into_response(),
        Err(error) => error.into_response(),
    }
}

async fn load_pending(
    state: &AppState,
    headers: &HeaderMap,
    id: &str,
) -> Handler<(Arc<dyn SigningStore>, PendingChangeRecord)> {
    let subject = auth::require_auth(state, headers)
        .await
        .map_err(IntoResponse::into_response)?;
    let Some(store) = state.signing.clone() else {
        return Err(
            ErrorData::bad_request("signing is not available on this manager").into_response(),
        );
    };
    let record = store
        .get_pending_change(id)
        .await
        .map_err(IntoResponse::into_response)?
        .ok_or_else(|| {
            alien_error::AlienError::new(ErrorData::PendingChangeNotFound { id: id.to_string() })
                .into_response()
        })?;
    if !state.authz.can_create_release(&subject, &record.project_id) {
        return Err(ErrorData::forbidden("Cannot manage signing for this project").into_response());
    }
    Ok((store, record))
}

#[cfg_attr(feature = "openapi", utoipa::path(
    get,
    path = "/v1/pending-changes/{id}",
    operation_id = "get_pending_change",
    tag = "signing",
    params(("id" = String, Path, description = "Pending change id")),
    responses((status = 200, description = "The change", body = PendingChangeResponse)),
    security(("bearer" = []))
))]
async fn get_pending_change(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    match load_pending(&state, &headers, &id).await {
        Ok((_, record)) => Json(PendingChangeResponse::from(record)).into_response(),
        Err(response) => response,
    }
}

#[cfg_attr(feature = "openapi", utoipa::path(
    post,
    path = "/v1/pending-changes/{id}/signatures",
    operation_id = "sign_pending_change",
    tag = "signing",
    params(("id" = String, Path, description = "Pending change id")),
    request_body = SignPendingChangeRequest,
    responses((status = 200, description = "Updated", body = PendingChangeResponse)),
    security(("bearer" = []))
))]
async fn sign_pending_change(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(request): Json<SignPendingChangeRequest>,
) -> Response {
    let (store, mut record) = match load_pending(&state, &headers, &id).await {
        Ok(ok) => ok,
        Err(response) => return response,
    };
    if record.status != PendingChangeStatus::Pending {
        return conflict(format!("Pending change '{id}' is already resolved"));
    }
    for signature in request.signatures {
        record
            .signatures
            .retain(|existing| existing.signer != signature.signer);
        record.signatures.push(signature);
    }
    if let Some(statement) = request.statement {
        // A statement change resolves only with the bytes it asked to sign.
        if let Some(payload) = &record.payload {
            if &statement.payload != payload {
                return ErrorData::bad_request("The statement is not the pending payload")
                    .into_response();
            }
        }
        let opened = match statement.open() {
            Ok(opened) => opened,
            Err(error) => return ErrorData::bad_request(error.message).into_response(),
        };
        if opened.kind == Kind::Config {
            let Some(deployment) = opened.deployment.clone() else {
                return ErrorData::bad_request("A config statement names its deployment")
                    .into_response();
            };
            if let Err(error) = store
                .add_config_statement(&deployment, opened.sequence, &statement)
                .await
            {
                return error.into_response();
            }
        }
        record.status = PendingChangeStatus::Approved;
        record.statement = Some(statement);
    }
    match store.update_pending_change(&record).await {
        Ok(()) => Json(PendingChangeResponse::from(record)).into_response(),
        Err(error) => error.into_response(),
    }
}

#[cfg_attr(feature = "openapi", utoipa::path(
    post,
    path = "/v1/deployments/{id}/config-statements",
    operation_id = "add_config_statement",
    tag = "signing",
    params(("id" = String, Path, description = "Deployment id")),
    request_body = StatementRequest,
    responses((status = 201, description = "Stored")),
    security(("bearer" = []))
))]
async fn add_config_statement(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(request): Json<StatementRequest>,
) -> Response {
    let subject = match auth::require_auth(&state, &headers).await {
        Ok(subject) => subject,
        Err(error) => return error.into_response(),
    };
    let Some(store) = state.signing.clone() else {
        return ErrorData::bad_request("signing is not available on this manager").into_response();
    };
    let deployment = match state.deployment_store.get_deployment(&subject, &id).await {
        Ok(Some(deployment)) if state.authz.can_update_deployment(&subject, &deployment) => {
            deployment
        }
        Ok(Some(_)) => {
            return ErrorData::forbidden("Cannot configure this deployment").into_response()
        }
        Ok(None) => return ErrorData::not_found_deployment(&id).into_response(),
        Err(error) => return error.into_response(),
    };
    let opened = match request.statement.open() {
        Ok(opened) if opened.kind == Kind::Config => opened,
        Ok(opened) => {
            return ErrorData::bad_request(format!(
                "A '{}' statement is not deployment config",
                opened.kind
            ))
            .into_response()
        }
        Err(error) => return ErrorData::bad_request(error.message).into_response(),
    };
    if opened.deployment.as_deref() != Some(deployment.id.as_str()) {
        return ErrorData::bad_request("The config statement names another deployment")
            .into_response();
    }
    if let Err(error) = store
        .add_config_statement(&deployment.id, opened.sequence, &request.statement)
        .await
    {
        return error.into_response();
    }
    // The deployment's enforcers take developer values only from this
    // statement, so it is delivered with the next target: the release it
    // runs, again.
    if deployment.desired_release_id.is_none() {
        if let Some(current) = deployment.current_release_id.as_deref() {
            if let Err(error) = state
                .deployment_store
                .set_deployment_desired_release(&subject, &deployment.id, current)
                .await
            {
                return error.into_response();
            }
        }
    }
    match state.deployment_store.set_redeploy(&subject, &deployment.id).await {
        Ok(()) => StatusCode::CREATED.into_response(),
        Err(error) => error.into_response(),
    }
}
