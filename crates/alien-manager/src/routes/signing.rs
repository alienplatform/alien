//! Signer lists, pending changes and signed deployment config.
//!
//! - `GET /v1/projects/{project}/signing`
//! - `GET/POST /v1/projects/{project}/signer-lists`
//! - `GET/POST /v1/projects/{project}/pending-changes`
//! - `GET /v1/pending-changes/{id}`, `POST /v1/pending-changes/{id}/resolution`
//! - `GET/POST /v1/deployments/{id}/config-statements`
//!
//! The manager stores and forwards these statements; it never verifies a
//! signature, because enforcers in each deployment do, and the CLI verifies
//! the signer list from the fingerprint the repository pins. It only checks
//! what it needs to keep its own records straight: the kind, the project,
//! and that signer list versions arrive in order and never change.

use std::sync::Arc;

use alien_cli_common::setup_templates::SetupHostingOptions;
use alien_cloudformation::{ManagementTrustOptions, RegistrationMode};
use alien_core::SignedStatement;
use alien_error::AlienError;
use alien_signing::{Kind, SignerListBody};
use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};

use super::{auth, AppState};
use crate::{
    auth::{Subject, SubjectKind},
    error::ErrorData,
    traits::{
        PendingChangeRecord, PendingChangeStatus, SignerListRecord, SignerListWrite, SigningStore,
    },
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/v1/projects/{project}/signing", get(project_signing))
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
            "/v1/pending-changes/{id}/resolution",
            post(resolve_pending_change),
        )
        .route(
            "/v1/deployments/{id}/config-statements",
            get(list_config_statements).post(add_config_statement),
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

/// Answer to storing a signer list version.
#[derive(Debug, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct SignerListStored {
    /// The version stored.
    pub version: u64,
}

/// Body of `POST /v1/projects/{project}/pending-changes`: the body of the
/// next signer list, for an admin to review and sign.
#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreatePendingChangeRequest {
    /// One line for lists, such as `bob-thinkpad asks to: release`.
    pub summary: String,
    /// Signer list version the body was prepared from.
    pub based_on_version: u64,
    /// The next list's body (`previous`, `adminThreshold`, `signers`,
    /// `deny`, `recovered`).
    #[cfg_attr(feature = "openapi", schema(value_type = Object))]
    pub body: serde_json::Value,
}

/// How a pending change ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub enum ResolutionStatus {
    Approved,
    Rejected,
}

/// Body of `POST /v1/pending-changes/{id}/resolution`.
#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResolvePendingChangeRequest {
    pub status: ResolutionStatus,
    /// The signer list version that carries the change; required when approved.
    #[serde(default)]
    pub signer_list_version: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::IntoParams))]
#[serde(rename_all = "camelCase")]
pub struct PendingChangesQuery {
    /// Only changes with this status.
    pub status: Option<PendingChangeStatus>,
}

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct PendingChangeResponse {
    /// Also the request id the pairing code covers.
    pub id: String,
    pub project_id: String,
    pub status: PendingChangeStatus,
    /// Display only.
    pub summary: String,
    /// Signer list version the body was prepared from.
    pub based_on_version: u64,
    /// The next list's body.
    #[cfg_attr(feature = "openapi", schema(value_type = Object))]
    pub body: serde_json::Value,
    /// Who filed it, display only.
    pub requested_by: Option<String>,
    pub created_at: String,
    pub resolved_at: Option<String>,
    /// Set when approved.
    pub signer_list_version: Option<u64>,
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
            status: record.status,
            summary: record.summary,
            based_on_version: record.based_on_version,
            body: record.body,
            requested_by: record.requested_by,
            created_at: record.created_at.to_rfc3339(),
            resolved_at: record.resolved_at.map(|at| at.to_rfc3339()),
            signer_list_version: record.signer_list_version,
        }
    }
}

type Handler<T> = std::result::Result<T, Response>;

/// The caller's name as people read it: a user's email, or the service
/// account id.
fn display_name(subject: &Subject) -> String {
    match &subject.kind {
        SubjectKind::User { email, .. } => email.clone(),
        SubjectKind::ServiceAccount { id } => id.clone(),
    }
}

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

fn list_conflict(reason: impl Into<String>) -> Response {
    AlienError::new(ErrorData::SignerListConflict {
        reason: reason.into(),
    })
    .into_response()
}

/// `409 SIGNING_ALREADY_ON` with `context: { enabledBy, enabledAt }`.
fn signing_already_on(project: &str, first: &SignerListRecord) -> Response {
    let enabled_at = first.created_at.to_rfc3339();
    let mut error = AlienError::new(ErrorData::SigningAlreadyOn {
        project: project.to_string(),
        enabled_by: first.created_by.clone(),
        enabled_at: enabled_at.clone(),
    });
    error.context = Some(serde_json::json!({
        "enabledBy": first.created_by,
        "enabledAt": enabled_at,
    }));
    error.into_response()
}

/// What a release of the project needs to know to sign its setup templates.
#[derive(Debug, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ProjectSigningResponse {
    /// What this manager renders setup templates with, so the digests a
    /// release signs are of the templates installs get.
    #[cfg_attr(feature = "openapi", schema(value_type = Object))]
    pub setup_hosting: SetupHostingOptions,
}

/// The setup hosting of this manager. It hosts no templates itself: vendors
/// render them with `alien render`, which registers through stack outputs.
fn manager_setup_hosting() -> SetupHostingOptions {
    SetupHostingOptions {
        registration: RegistrationMode::OutputsFallback,
        description: None,
        management_trust: ManagementTrustOptions::default(),
        supported_aws_regions: Vec::new(),
        terraform_registration: None,
    }
}

#[cfg_attr(feature = "openapi", utoipa::path(
    get,
    path = "/v1/projects/{project}/signing",
    operation_id = "get_project_signing",
    tag = "signing",
    params(("project" = String, Path, description = "Project id")),
    responses((status = 200, description = "Setup hosting for signed releases", body = ProjectSigningResponse)),
    security(("bearer" = []))
))]
async fn project_signing(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(project): Path<String>,
) -> Response {
    if let Err(response) = setup(&state, &headers, &project).await {
        return response;
    }
    Json(ProjectSigningResponse {
        setup_hosting: manager_setup_hosting(),
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
        Ok(records) => Json(SignerListsResponse {
            items: records.into_iter().map(|record| record.statement).collect(),
        })
        .into_response(),
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
        (status = 201, description = "Stored", body = SignerListStored),
        (status = 200, description = "The same version was already stored", body = SignerListStored),
        (status = 409, description = "SIGNING_ALREADY_ON: another version 1 is stored; SIGNER_LIST_CONFLICT: the version skips one or differs from the stored one"),
    ),
    security(("bearer" = []))
))]
async fn add_signer_list(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(project): Path<String>,
    Json(request): Json<StatementRequest>,
) -> Response {
    let (subject, store) = match setup(&state, &headers, &project).await {
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
    if opened.project != project {
        return ErrorData::bad_request(format!(
            "The signer list is for project '{}', not '{project}'",
            opened.project
        ))
        .into_response();
    }
    let version = opened.sequence;
    if version == 0 {
        return ErrorData::bad_request("Signer list versions start at 1").into_response();
    }
    let stored = match store.signer_lists(&project).await {
        Ok(stored) => stored,
        Err(error) => return error.into_response(),
    };
    let next = stored.len() as u64 + 1;
    if version > next {
        return list_conflict(format!(
            "Signer list version {version} can't be stored before version {next}"
        ));
    }
    let record = SignerListRecord {
        version,
        statement: request.statement,
        created_by: display_name(&subject),
        created_at: Utc::now(),
    };
    match store.add_signer_list(&project, &record).await {
        Ok(SignerListWrite::Added) => {
            (StatusCode::CREATED, Json(SignerListStored { version })).into_response()
        }
        Ok(SignerListWrite::AlreadyStored) => {
            (StatusCode::OK, Json(SignerListStored { version })).into_response()
        }
        Ok(SignerListWrite::Conflict(first)) if version == 1 => {
            signing_already_on(&project, &first)
        }
        Ok(SignerListWrite::Conflict(_)) => list_conflict(format!(
            "A different signer list version {version} is already stored"
        )),
        Err(error) => error.into_response(),
    }
}

#[cfg_attr(feature = "openapi", utoipa::path(
    get,
    path = "/v1/projects/{project}/pending-changes",
    operation_id = "list_pending_changes",
    tag = "signing",
    params(("project" = String, Path, description = "Project id"), PendingChangesQuery),
    responses((status = 200, description = "Pending changes, newest first", body = PendingChangesResponse)),
    security(("bearer" = []))
))]
async fn list_pending_changes(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(project): Path<String>,
    Query(query): Query<PendingChangesQuery>,
) -> Response {
    let (_, store) = match setup(&state, &headers, &project).await {
        Ok(ok) => ok,
        Err(response) => return response,
    };
    match store.pending_changes(&project).await {
        Ok(records) => Json(PendingChangesResponse {
            items: records
                .into_iter()
                .filter(|record| query.status.is_none_or(|status| record.status == status))
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
    let (subject, store) = match setup(&state, &headers, &project).await {
        Ok(ok) => ok,
        Err(response) => return response,
    };
    // The admin's CLI reads the body as the next list's body; refuse one it
    // could never sign.
    if let Err(error) = serde_json::from_value::<SignerListBody>(request.body.clone()) {
        return ErrorData::bad_request(format!("The body is not a signer list body: {error}"))
            .into_response();
    }
    let record = PendingChangeRecord {
        id: format!("pch_{}", uuid::Uuid::new_v4().simple()),
        project_id: project,
        status: PendingChangeStatus::Pending,
        summary: request.summary,
        based_on_version: request.based_on_version,
        body: request.body,
        requested_by: Some(display_name(&subject)),
        created_at: Utc::now(),
        resolved_at: None,
        signer_list_version: None,
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
            AlienError::new(ErrorData::PendingChangeNotFound { id: id.to_string() }).into_response()
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
    path = "/v1/pending-changes/{id}/resolution",
    operation_id = "resolve_pending_change",
    tag = "signing",
    params(("id" = String, Path, description = "Pending change id")),
    request_body = ResolvePendingChangeRequest,
    responses(
        (status = 200, description = "Resolved", body = PendingChangeResponse),
        (status = 409, description = "The change is already resolved"),
    ),
    security(("bearer" = []))
))]
async fn resolve_pending_change(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(request): Json<ResolvePendingChangeRequest>,
) -> Response {
    let (store, mut record) = match load_pending(&state, &headers, &id).await {
        Ok(ok) => ok,
        Err(response) => return response,
    };
    if record.status != PendingChangeStatus::Pending {
        return AlienError::new(ErrorData::SignedStatementConflict {
            reason: format!("Pending change '{id}' is already resolved"),
        })
        .into_response();
    }
    match (request.status, request.signer_list_version) {
        (ResolutionStatus::Approved, Some(version)) => {
            // Presence only: the CLI that signed the list verified it.
            let lists = match store.signer_lists(&record.project_id).await {
                Ok(lists) => lists,
                Err(error) => return error.into_response(),
            };
            if !lists.iter().any(|list| list.version == version) {
                return ErrorData::bad_request(format!(
                    "Signer list version {version} is not stored; store it before approving"
                ))
                .into_response();
            }
            record.status = PendingChangeStatus::Approved;
            record.signer_list_version = Some(version);
        }
        (ResolutionStatus::Approved, None) => {
            return ErrorData::bad_request(
                "An approved change names the signer list version that carries it",
            )
            .into_response()
        }
        (ResolutionStatus::Rejected, None) => record.status = PendingChangeStatus::Rejected,
        (ResolutionStatus::Rejected, Some(_)) => {
            return ErrorData::bad_request("A rejected change names no signer list version")
                .into_response()
        }
    }
    record.resolved_at = Some(Utc::now());
    match store.update_pending_change(&record).await {
        Ok(()) => Json(PendingChangeResponse::from(record)).into_response(),
        Err(error) => error.into_response(),
    }
}

/// Config statements stored for a deployment.
#[derive(Debug, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ConfigStatementsResponse {
    /// Every stored statement, lowest sequence first.
    pub items: Vec<SignedStatement>,
}

#[cfg_attr(feature = "openapi", utoipa::path(
    get,
    path = "/v1/deployments/{id}/config-statements",
    operation_id = "list_config_statements",
    tag = "signing",
    params(("id" = String, Path, description = "Deployment id")),
    responses((status = 200, description = "Stored config statements", body = ConfigStatementsResponse)),
    security(("bearer" = []))
))]
async fn list_config_statements(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    let subject = match auth::require_auth(&state, &headers).await {
        Ok(subject) => subject,
        Err(error) => return error.into_response(),
    };
    let Some(store) = state.signing.clone() else {
        return ErrorData::bad_request("signing is not available on this manager").into_response();
    };
    match state.deployment_store.get_deployment(&subject, &id).await {
        Ok(Some(deployment)) if state.authz.can_read_deployment(&subject, &deployment) => {}
        Ok(Some(_)) => return ErrorData::forbidden("Cannot read this deployment").into_response(),
        Ok(None) => return ErrorData::not_found_deployment(&id).into_response(),
        Err(error) => return error.into_response(),
    }
    match store.config_statements(&id).await {
        Ok(items) => Json(ConfigStatementsResponse { items }).into_response(),
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
    if opened.project != deployment.project_id {
        return ErrorData::bad_request(format!(
            "The config statement is for project '{}', not this deployment's",
            opened.project
        ))
        .into_response();
    }
    // The statement names the install it is for; the install checks that
    // itself, so the manager only stores it.
    if opened.install.is_none() {
        return ErrorData::bad_request("A config statement names the install it is for")
            .into_response();
    }
    // Sync hands the newest config to the deployment on its next pass.
    match store
        .add_config_statement(&deployment.id, opened.sequence, &request.statement)
        .await
    {
        Ok(()) => StatusCode::CREATED.into_response(),
        Err(error) => error.into_response(),
    }
}
