//! Authenticated command endpoints.
//!
//! All command protocol routes are defined here with per-handler auth checks,
//! following the same pattern as `sync.rs` and `deployments.rs`.

use axum::{
    Router,
    extract::{Json, Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post, put},
};
use serde::Deserialize;

use alien_commands::server::command_registry::{CommandStatus, OPERATOR_COMMAND_TARGET_ID};
use alien_commands::server::{CommandPayloadResponse, StorePayloadRequest};
use alien_commands::types::*;

use crate::error::ErrorData;

use super::{AppState, auth};

// --- Helpers ---

async fn require_upload_complete_access(
    deployment_store: &dyn crate::traits::DeploymentStore,
    authz: &dyn crate::auth::Authz,
    subject: &crate::auth::Subject,
    command: &CommandStatus,
) -> Result<(), Response> {
    if command.target.resource_id == OPERATOR_COMMAND_TARGET_ID {
        return if crate::auth::command_capability::operator_upload_complete_allowed(
            subject, command,
        ) {
            Ok(())
        } else {
            Err(ErrorData::forbidden("Access denied").into_response())
        };
    }

    require_command_dispatch_access(deployment_store, authz, subject, &command.deployment_id).await
}

fn require_direct_payload_access(
    subject: &crate::auth::Subject,
    command: Option<&alien_commands::server::CommandAccessContext>,
) -> Result<(), &'static str> {
    if command.is_some_and(|command| command.target.resource_id == OPERATOR_COMMAND_TARGET_ID) {
        return Err("Access denied");
    }

    if matches!(subject.scope, crate::auth::Scope::Workspace)
        && matches!(
            subject.role,
            crate::auth::Role::WorkspaceAdmin | crate::auth::Role::WorkspaceMember
        )
    {
        Ok(())
    } else {
        Err("Workspace-write access required")
    }
}

async fn get_command_deployment(
    deployment_store: &dyn crate::traits::DeploymentStore,
    subject: &crate::auth::Subject,
    deployment_id: &str,
) -> Result<crate::traits::DeploymentRecord, Response> {
    match deployment_store
        .get_deployment(subject, deployment_id)
        .await
        .map_err(|e| e.into_response())?
    {
        Some(deployment) => Ok(deployment),
        None => Err(ErrorData::not_found_deployment(deployment_id).into_response()),
    }
}

/// Authorize the caller against the deployment that owns a command.
/// Loads the deployment then defers to `Authz::can_read_command`.
async fn require_command_access(
    state: &AppState,
    subject: &crate::auth::Subject,
    deployment_id: &str,
) -> Result<(), Response> {
    let deployment =
        get_command_deployment(state.deployment_store.as_ref(), subject, deployment_id).await?;
    if state.authz.can_read_command(subject, &deployment) {
        Ok(())
    } else {
        Err(ErrorData::forbidden("Access denied").into_response())
    }
}

/// Authorize from the command record when it contains everything required by
/// the caller's scope. Deployment-group tokens still load the deployment,
/// because group ownership is not duplicated on command records.
async fn require_command_read_access(
    state: &AppState,
    subject: &crate::auth::Subject,
    command: &alien_commands::server::CommandAccessContext,
) -> Result<(), Response> {
    if !matches!(&subject.scope, crate::auth::Scope::DeploymentGroup { .. }) {
        return if state.authz.can_read_command_context(subject, command) {
            Ok(())
        } else {
            Err(ErrorData::forbidden("Access denied").into_response())
        };
    }

    require_command_access(state, subject, &command.deployment_id).await
}

async fn require_command_auth(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<crate::auth::Subject, Response> {
    auth::require_auth(state, headers)
        .await
        .map_err(IntoResponse::into_response)
}

async fn require_command_create_access(
    deployment_store: &dyn crate::traits::DeploymentStore,
    authz: &dyn crate::auth::Authz,
    subject: &crate::auth::Subject,
    deployment_id: &str,
    command: &str,
    requested_target: Option<&str>,
) -> Result<(), Response> {
    if let Some(allowed) = crate::auth::command_capability::create_request_decision(
        subject,
        deployment_id,
        command,
        requested_target,
    ) {
        return if allowed {
            Ok(())
        } else {
            Err(ErrorData::forbidden("Access denied").into_response())
        };
    }

    if requested_target == Some(OPERATOR_COMMAND_TARGET_ID) {
        return Err(ErrorData::forbidden("Access denied").into_response());
    }

    let deployment = get_command_deployment(deployment_store, subject, deployment_id).await?;
    if authz.can_dispatch_command(subject, &deployment) {
        Ok(())
    } else {
        Err(ErrorData::forbidden("Access denied").into_response())
    }
}

async fn require_command_dispatch_access(
    deployment_store: &dyn crate::traits::DeploymentStore,
    authz: &dyn crate::auth::Authz,
    subject: &crate::auth::Subject,
    deployment_id: &str,
) -> Result<(), Response> {
    if let Some(allowed) =
        crate::auth::command_capability::sender_request_decision(subject, deployment_id)
    {
        return if allowed {
            Ok(())
        } else {
            Err(ErrorData::forbidden("Access denied").into_response())
        };
    }

    let deployment = get_command_deployment(deployment_store, subject, deployment_id).await?;
    if authz.can_dispatch_command(subject, &deployment) {
        Ok(())
    } else {
        Err(ErrorData::forbidden("Access denied").into_response())
    }
}

async fn require_command_execution_access(
    state: &AppState,
    subject: &crate::auth::Subject,
    deployment_id: &str,
) -> Result<(), Response> {
    let deployment =
        get_command_deployment(state.deployment_store.as_ref(), subject, deployment_id).await?;
    if state.authz.can_execute_command(subject, &deployment) {
        Ok(())
    } else {
        Err(ErrorData::forbidden("Access denied").into_response())
    }
}

async fn require_command_receive_access(
    deployment_store: &dyn crate::traits::DeploymentStore,
    authz: &dyn crate::auth::Authz,
    subject: &crate::auth::Subject,
    deployment_id: &str,
    target: &alien_core::CommandTarget,
) -> Result<(), Response> {
    if let Some(allowed) =
        crate::auth::command_capability::receiver_request_decision(subject, deployment_id, target)
    {
        return if allowed {
            Ok(())
        } else {
            Err(ErrorData::forbidden("Access denied").into_response())
        };
    }

    let deployment = get_command_deployment(deployment_store, subject, deployment_id).await?;
    if authz.can_receive_command(subject, &deployment, target) {
        Ok(())
    } else {
        Err(
            ErrorData::forbidden("Access denied: can only acquire leases for own deployment")
                .into_response(),
        )
    }
}

/// Authorize response/release execution from canonical command ownership.
/// Commands-only receiver credentials are checked against the exact target;
/// existing admin/deployment/group callers retain the deployment-level path.
async fn require_command_execution_context(
    state: &AppState,
    subject: &crate::auth::Subject,
    command: &alien_commands::server::CommandAccessContext,
) -> Result<(), Response> {
    if matches!(subject.scope, crate::auth::Scope::Commands { .. }) {
        return if state.authz.can_execute_command_context(subject, command) {
            Ok(())
        } else {
            Err(ErrorData::forbidden("Access denied").into_response())
        };
    }

    require_command_execution_access(state, subject, &command.deployment_id).await
}

// --- Router ---

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/v1/commands", post(create_command))
        .route("/v1/commands/{command_id}", get(get_command_status))
        .route(
            "/v1/commands/{command_id}/upload-complete",
            post(upload_complete),
        )
        .route("/v1/commands/{command_id}/response", put(submit_response))
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
/// Auth: deployment-authorized callers may target stack resources. The reserved
/// operations target additionally requires an exact commands capability bound
/// to the deployment and command name.
async fn create_command(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CreateCommandRequest>,
) -> Response {
    let subject = match auth::require_auth(&state, &headers).await {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };
    if let Err(response) = require_command_create_access(
        state.deployment_store.as_ref(),
        state.authz.as_ref(),
        &subject,
        &request.deployment_id,
        &request.command,
        request.target_resource_id.as_deref(),
    )
    .await
    {
        return response;
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
    let subject = match require_command_auth(&state, &headers).await {
        Ok(subject) => subject,
        Err(response) => return response,
    };
    let command = match state
        .command_server
        .get_command_access_context(&command_id)
        .await
    {
        Ok(Some(command)) => command,
        Ok(None) => {
            return alien_error::AlienError::new(
                alien_commands::error::ErrorData::CommandNotFound {
                    command_id: command_id.clone(),
                },
            )
            .into_response();
        }
        Err(e) => return e.into_response(),
    };

    if let Err(e) = require_command_read_access(&state, &subject, &command).await {
        return e;
    }

    match state.command_server.get_command_status(&command_id).await {
        Ok(response) => Json(response).into_response(),
        Err(e) => e.into_response(),
    }
}

/// Mark upload as complete.
///
/// Auth: Same deployment-level rules as create. Reserved operator commands
/// require the exact operations capability that created the command.
async fn upload_complete(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(command_id): Path<String>,
    Json(upload_request): Json<UploadCompleteRequest>,
) -> Response {
    let subject = match require_command_auth(&state, &headers).await {
        Ok(subject) => subject,
        Err(response) => return response,
    };
    let command = match state
        .command_server
        .get_command_status_record(&command_id)
        .await
    {
        Ok(Some(command)) => command,
        Ok(None) => {
            return alien_error::AlienError::new(
                alien_commands::error::ErrorData::CommandNotFound {
                    command_id: command_id.clone(),
                },
            )
            .into_response();
        }
        Err(e) => return e.into_response(),
    };

    if let Err(response) = require_upload_complete_access(
        state.deployment_store.as_ref(),
        state.authz.as_ref(),
        &subject,
        &command,
    )
    .await
    {
        return response;
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
        // Standard Bearer auth path — commands-only receiver credentials are
        // bound to the exact command target, while legacy/admin credentials
        // retain deployment-level authorization.
        let command = match state
            .command_server
            .get_command_access_context(&command_id)
            .await
        {
            Ok(Some(command)) => command,
            Ok(None) => {
                return alien_error::AlienError::new(
                    alien_commands::error::ErrorData::CommandNotFound {
                        command_id: command_id.clone(),
                    },
                )
                .into_response();
            }
            Err(e) => return e.into_response(),
        };
        if let Err(e) = require_command_execution_context(&state, &subject, &command).await {
            return e;
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
    let subject = match require_command_auth(&state, &headers).await {
        Ok(subject) => subject,
        Err(response) => return response,
    };
    // Verify the caller has access to this command's deployment via Authz.
    // If the command isn't in the local registry (e.g. when command metadata
    // is managed externally), fall back to requiring workspace-write
    // authority. A *registry lookup error* must NOT trigger that fallback —
    // a transient store error for a deployment-owned command would otherwise
    // expose its payload to any workspace-admin/member token.
    match state
        .command_server
        .get_command_access_context(&command_id)
        .await
    {
        Ok(Some(command)) => {
            if let Err(e) = require_command_read_access(&state, &subject, &command).await {
                return e;
            }
        }
        Ok(None) => {
            // No canonical owner in the local registry — only workspace-wide
            // writers may inspect such payloads.
            if !matches!(&subject.scope, crate::auth::Scope::Workspace)
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
    };
    let command = match state
        .command_server
        .get_command_access_context(&command_id)
        .await
    {
        Ok(command) => command,
        Err(e) => return e.into_response(),
    };

    if let Err(message) = require_direct_payload_access(&subject, command.as_ref()) {
        return ErrorData::forbidden(message).into_response();
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
    if let Err(response) = require_command_receive_access(
        state.deployment_store.as_ref(),
        state.authz.as_ref(),
        &subject,
        &lease_request.deployment_id,
        &lease_request.target,
    )
    .await
    {
        return response;
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
/// Auth: Admin, DG (group), or own Deployment token — the caller must be able
/// to act on the deployment that HOLDS the lease (the lease owner), otherwise
/// any authenticated tenant that learned a lease_id could force-release
/// another deployment's lease and trigger a spurious redelivery.
async fn release_lease(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(lease_id): Path<String>,
) -> Response {
    let subject = match require_command_auth(&state, &headers).await {
        Ok(subject) => subject,
        Err(response) => return response,
    };

    let (command_id, owner) = match state.command_server.get_lease_owner(&lease_id).await {
        Ok(Some((command_id, owner))) => (command_id, owner),
        Ok(None) => {
            return alien_error::AlienError::new(alien_commands::error::ErrorData::LeaseNotFound {
                lease_id: lease_id.clone(),
            })
            .into_response();
        }
        Err(e) => return e.into_response(),
    };
    if matches!(subject.scope, crate::auth::Scope::Commands { .. }) {
        let command = match state
            .command_server
            .get_command_access_context(&command_id)
            .await
        {
            Ok(Some(command)) => command,
            Ok(None) => {
                return alien_error::AlienError::new(
                    alien_commands::error::ErrorData::CommandNotFound { command_id },
                )
                .into_response();
            }
            Err(e) => return e.into_response(),
        };
        if let Err(e) = require_command_execution_context(&state, &subject, &command).await {
            return e;
        }
    } else if let Err(e) = require_command_execution_access(&state, &subject, &owner).await {
        return e;
    }

    match state.command_server.release_lease_by_id(&lease_id).await {
        Ok(()) => StatusCode::OK.into_response(),
        Err(e) => e.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_commands::test_utils::{
        TestCommandServer, test_storage_create_command, test_upload_complete_request,
    };
    use chrono::Utc;

    use crate::auth::{CommandCapability, Role, Scope, Subject, SubjectKind};
    use crate::providers::OssAuthz;
    use crate::traits::deployment_store::MockDeploymentStore;

    fn commands_sender_subject() -> Subject {
        Subject {
            kind: SubjectKind::ServiceAccount {
                id: "commands-sender".to_string(),
            },
            workspace_id: "workspace-1".to_string(),
            scope: Scope::Commands {
                project_id: "project-1".to_string(),
                deployment_id: "deployment-1".to_string(),
                capability: CommandCapability::Send,
            },
            role: Role::CommandCapability,
            bearer_token: "bearer".to_string(),
        }
    }

    fn operations_subject(command: &str) -> Subject {
        operations_subject_for("workspace-1", "project-1", "deployment-1", command)
    }

    fn operations_subject_for(
        workspace_id: &str,
        project_id: &str,
        deployment_id: &str,
        command: &str,
    ) -> Subject {
        Subject {
            kind: SubjectKind::ServiceAccount {
                id: "operations-dispatcher".to_string(),
            },
            workspace_id: workspace_id.to_string(),
            scope: Scope::Commands {
                project_id: project_id.to_string(),
                deployment_id: deployment_id.to_string(),
                capability: CommandCapability::Operations {
                    command: command.to_string(),
                },
            },
            role: Role::CommandCapability,
            bearer_token: "bearer".to_string(),
        }
    }

    fn commands_receiver_subject() -> Subject {
        Subject {
            kind: SubjectKind::ServiceAccount {
                id: "commands-receiver".to_string(),
            },
            workspace_id: "workspace-1".to_string(),
            scope: Scope::Commands {
                project_id: "project-1".to_string(),
                deployment_id: "deployment-1".to_string(),
                capability: CommandCapability::Receive {
                    target: alien_core::CommandTarget::new(
                        OPERATOR_COMMAND_TARGET_ID,
                        alien_core::CommandTargetType::Daemon,
                    ),
                },
            },
            role: Role::CommandCapability,
            bearer_token: "bearer".to_string(),
        }
    }

    fn workspace_admin_subject() -> Subject {
        Subject {
            kind: SubjectKind::ServiceAccount {
                id: "workspace-admin".to_string(),
            },
            workspace_id: "workspace-1".to_string(),
            scope: Scope::Workspace,
            role: Role::WorkspaceAdmin,
            bearer_token: "bearer".to_string(),
        }
    }

    fn operator_command_status(command: &str) -> CommandStatus {
        CommandStatus {
            command_id: "command-1".to_string(),
            workspace_id: "workspace-1".to_string(),
            project_id: "project-1".to_string(),
            deployment_id: "deployment-1".to_string(),
            command: command.to_string(),
            state: CommandState::PendingUpload,
            attempt: 0,
            deadline: None,
            created_at: Utc::now(),
            dispatched_at: None,
            completed_at: None,
            error: None,
            request_size_bytes: Some(200_000),
            response_size_bytes: None,
            target: alien_core::CommandTarget::new(
                OPERATOR_COMMAND_TARGET_ID,
                alien_core::CommandTargetType::Daemon,
            ),
        }
    }

    #[tokio::test]
    async fn commands_dispatch_authorization_does_not_preflight_the_deployment() {
        let mut deployment_store = MockDeploymentStore::new();
        require_command_dispatch_access(
            &deployment_store,
            &OssAuthz,
            &commands_sender_subject(),
            "deployment-1",
        )
        .await
        .expect("signed deployment scope should authorize the sender");

        deployment_store.checkpoint();
    }

    #[tokio::test]
    async fn operations_capability_creates_only_its_exact_operator_command() {
        let mut deployment_store = MockDeploymentStore::new();
        let subject = operations_subject("postgres/health");

        require_command_create_access(
            &deployment_store,
            &OssAuthz,
            &subject,
            "deployment-1",
            "postgres/health",
            Some(OPERATOR_COMMAND_TARGET_ID),
        )
        .await
        .expect("exact operations capability should authorize create");

        for (deployment_id, command, target) in [
            (
                "deployment-2",
                "postgres/health",
                Some(OPERATOR_COMMAND_TARGET_ID),
            ),
            (
                "deployment-1",
                "postgres/drop",
                Some(OPERATOR_COMMAND_TARGET_ID),
            ),
            ("deployment-1", "postgres/health", Some("daemon-1")),
            ("deployment-1", "postgres/health", None),
        ] {
            assert_eq!(
                require_command_create_access(
                    &deployment_store,
                    &OssAuthz,
                    &subject,
                    deployment_id,
                    command,
                    target,
                )
                .await
                .expect_err("operations capability must stay bound to its exact request")
                .status(),
                StatusCode::FORBIDDEN,
            );
        }

        deployment_store.checkpoint();
    }

    #[tokio::test]
    async fn ordinary_sender_cannot_create_operator_commands() {
        let mut deployment_store = MockDeploymentStore::new();

        assert_eq!(
            require_command_create_access(
                &deployment_store,
                &OssAuthz,
                &commands_sender_subject(),
                "deployment-1",
                "postgres/health",
                Some(OPERATOR_COMMAND_TARGET_ID),
            )
            .await
            .expect_err("sender capability must not address the operator")
            .status(),
            StatusCode::FORBIDDEN,
        );

        deployment_store.checkpoint();
    }

    #[tokio::test]
    async fn deployment_level_subjects_cannot_create_operator_commands() {
        let mut deployment_store = MockDeploymentStore::new();
        let subjects = [
            Subject {
                kind: SubjectKind::User {
                    id: "user-1".to_string(),
                    email: "user@example.com".to_string(),
                    workspace_name: Some("workspace-1".to_string()),
                },
                workspace_id: "workspace-1".to_string(),
                scope: Scope::Workspace,
                role: Role::WorkspaceAdmin,
                bearer_token: "bearer".to_string(),
            },
            Subject {
                kind: SubjectKind::ServiceAccount {
                    id: "deployment-group-1".to_string(),
                },
                workspace_id: "workspace-1".to_string(),
                scope: Scope::DeploymentGroup {
                    project_id: "project-1".to_string(),
                    deployment_group_id: "group-1".to_string(),
                },
                role: Role::DeploymentGroupDeployer,
                bearer_token: "bearer".to_string(),
            },
            Subject {
                kind: SubjectKind::ServiceAccount {
                    id: "deployment-manager-1".to_string(),
                },
                workspace_id: "workspace-1".to_string(),
                scope: Scope::Deployment {
                    project_id: "project-1".to_string(),
                    deployment_id: "deployment-1".to_string(),
                },
                role: Role::DeploymentManager,
                bearer_token: "bearer".to_string(),
            },
        ];

        for subject in &subjects {
            assert_eq!(
                require_command_create_access(
                    &deployment_store,
                    &OssAuthz,
                    subject,
                    "deployment-1",
                    "postgres/health",
                    Some(OPERATOR_COMMAND_TARGET_ID),
                )
                .await
                .expect_err("deployment-level authorization must not reach the operator")
                .status(),
                StatusCode::FORBIDDEN,
            );
        }

        deployment_store.checkpoint();
    }

    #[tokio::test]
    async fn operations_capability_cannot_use_sender_or_receiver_paths() {
        let mut deployment_store = MockDeploymentStore::new();
        let subject = operations_subject("postgres/health");

        assert_eq!(
            require_command_dispatch_access(
                &deployment_store,
                &OssAuthz,
                &subject,
                "deployment-1",
            )
            .await
            .expect_err("operations capability must not gain sender mutation access")
            .status(),
            StatusCode::FORBIDDEN,
        );
        assert_eq!(
            require_command_receive_access(
                &deployment_store,
                &OssAuthz,
                &subject,
                "deployment-1",
                &alien_core::CommandTarget::new(
                    OPERATOR_COMMAND_TARGET_ID,
                    alien_core::CommandTargetType::Daemon,
                ),
            )
            .await
            .expect_err("operations capability must not gain receiver access")
            .status(),
            StatusCode::FORBIDDEN,
        );

        deployment_store.checkpoint();
    }

    #[tokio::test]
    async fn operator_upload_complete_rejects_non_operations_subjects() {
        let mut deployment_store = MockDeploymentStore::new();
        let command = operator_command_status("postgres/health");
        let subjects = [
            commands_sender_subject(),
            commands_receiver_subject(),
            workspace_admin_subject(),
        ];

        for subject in &subjects {
            assert_eq!(
                require_upload_complete_access(&deployment_store, &OssAuthz, subject, &command,)
                    .await
                    .expect_err("only an operations capability may complete an operator upload")
                    .status(),
                StatusCode::FORBIDDEN,
            );
        }

        deployment_store.checkpoint();
    }

    #[tokio::test]
    async fn operator_upload_complete_rejects_a_different_operation_command() {
        let mut deployment_store = MockDeploymentStore::new();

        assert_eq!(
            require_upload_complete_access(
                &deployment_store,
                &OssAuthz,
                &operations_subject("postgres/version"),
                &operator_command_status("postgres/health"),
            )
            .await
            .expect_err("operations capability must match the canonical command name")
            .status(),
            StatusCode::FORBIDDEN,
        );

        deployment_store.checkpoint();
    }

    #[test]
    fn direct_operator_payload_put_remains_denied() {
        let command = alien_commands::server::CommandAccessContext {
            workspace_id: "workspace-1".to_string(),
            project_id: "project-1".to_string(),
            deployment_id: "deployment-1".to_string(),
            target: alien_core::CommandTarget::new(
                OPERATOR_COMMAND_TARGET_ID,
                alien_core::CommandTargetType::Daemon,
            ),
        };

        for subject in [
            operations_subject("postgres/health"),
            workspace_admin_subject(),
        ] {
            assert_eq!(
                require_direct_payload_access(&subject, Some(&command)),
                Err("Access denied"),
            );
        }
    }

    #[tokio::test]
    async fn oversized_operator_upload_becomes_leasable_for_exact_operations_capability() {
        let mut server = TestCommandServer::builder().with_pull_mode().build().await;
        let deployment_id = "deployment-1";
        let command_name = "postgres/health";
        let mut request = test_storage_create_command(deployment_id, command_name, 200_000);
        request.target_resource_id = Some(OPERATOR_COMMAND_TARGET_ID.to_string());

        let created = server
            .create_command(request)
            .await
            .expect("create oversized operator command");
        assert_eq!(created.state, CommandState::PendingUpload);

        let command = server
            .command_server
            .get_command_status_record(&created.command_id)
            .await
            .expect("read canonical command status")
            .expect("created command status");
        let mut deployment_store = MockDeploymentStore::new();
        require_upload_complete_access(
            &deployment_store,
            &OssAuthz,
            &operations_subject_for("default", "default", deployment_id, command_name),
            &command,
        )
        .await
        .expect("exact operations capability should complete its params upload");

        let completed = server
            .upload_complete(&created.command_id, test_upload_complete_request(200_000))
            .await
            .expect("complete operator params upload");
        assert_eq!(completed.state, CommandState::Pending);

        let replay = server
            .upload_complete(&created.command_id, test_upload_complete_request(200_000))
            .await
            .expect_err("upload completion must not replay after leaving PendingUpload");
        assert_eq!(replay.code, "INVALID_STATE_TRANSITION");

        let leases = server
            .acquire_lease(
                deployment_id,
                LeaseRequest {
                    deployment_id: deployment_id.to_string(),
                    target: alien_core::CommandTarget::new(
                        OPERATOR_COMMAND_TARGET_ID,
                        alien_core::CommandTargetType::Daemon,
                    ),
                    max_leases: 1,
                    lease_seconds: 60,
                },
            )
            .await
            .expect("lease uploaded operator command");
        assert_eq!(leases.leases[0].command_id, created.command_id);

        deployment_store.checkpoint();
        server.shutdown().await;
    }

    #[tokio::test]
    async fn commands_receive_authorization_does_not_preflight_the_deployment() {
        let mut deployment_store = MockDeploymentStore::new();
        let target = alien_core::CommandTarget::new(
            "container-1".to_string(),
            alien_core::CommandTargetType::Container,
        );
        let subject = Subject {
            kind: SubjectKind::ServiceAccount {
                id: "commands-receiver".to_string(),
            },
            workspace_id: "workspace-1".to_string(),
            scope: Scope::Commands {
                project_id: "project-1".to_string(),
                deployment_id: "deployment-1".to_string(),
                capability: CommandCapability::Receive {
                    target: target.clone(),
                },
            },
            role: Role::CommandCapability,
            bearer_token: "bearer".to_string(),
        };

        require_command_receive_access(
            &deployment_store,
            &OssAuthz,
            &subject,
            "deployment-1",
            &target,
        )
        .await
        .expect("signed deployment and target should authorize the receiver");

        let wrong_target = alien_core::CommandTarget::new(
            "container-2".to_string(),
            alien_core::CommandTargetType::Container,
        );
        assert_eq!(
            require_command_receive_access(
                &deployment_store,
                &OssAuthz,
                &subject,
                "deployment-1",
                &wrong_target,
            )
            .await
            .expect_err("receiver capability must stay bound to its exact target")
            .status(),
            StatusCode::FORBIDDEN,
        );
        assert_eq!(
            require_command_receive_access(
                &deployment_store,
                &OssAuthz,
                &subject,
                "deployment-2",
                &target,
            )
            .await
            .expect_err("receiver capability must stay bound to its deployment")
            .status(),
            StatusCode::FORBIDDEN,
        );

        deployment_store.checkpoint();
    }
}
