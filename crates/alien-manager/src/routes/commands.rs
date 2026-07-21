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

use alien_commands::server::{CommandPayloadResponse, StorePayloadRequest};
use alien_commands::types::*;

use crate::error::ErrorData;

use super::{auth, AppState};

// --- Helpers ---

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
        .get_deployment(subject, deployment_id)
        .await
        .map_err(|e| e.into_response())?
        .ok_or_else(|| ErrorData::not_found_deployment(deployment_id).into_response())?;
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
    if !matches!(subject.scope, crate::auth::Scope::DeploymentGroup { .. }) {
        return if state.authz.can_read_command_context(subject, command) {
            Ok(())
        } else {
            Err(ErrorData::forbidden("Access denied").into_response())
        };
    }

    require_command_access(state, subject, &command.deployment_id).await
}

async fn require_command_mutation_access(
    state: &AppState,
    subject: &crate::auth::Subject,
    deployment_id: &str,
) -> Result<(), Response> {
    let deployment = state
        .deployment_store
        .get_deployment(subject, deployment_id)
        .await
        .map_err(|e| e.into_response())?
        .ok_or_else(|| ErrorData::not_found_deployment(deployment_id).into_response())?;
    if state.authz.can_dispatch_command(subject, &deployment) {
        Ok(())
    } else {
        Err(ErrorData::forbidden("Access denied").into_response())
    }
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
/// Auth: Admin or DeploymentGroup token (must own the target deployment's group).
///
/// Authorization is intentionally deployment-scoped. There is no
/// per-resource auth primitive (the finest auth grain is the deployment), so
/// naming a `targetResourceId` grants no extra access. Target selection is
/// validated server-side by the registry as an EXISTENCE/CAPABILITY check
/// (does this deployment have such a command-capable resource?), not as an
/// authorization boundary — resolution failures surface as
/// `COMMAND_TARGET_NOT_FOUND` (404), `COMMAND_TARGET_AMBIGUOUS` (409), or
/// `NO_COMMAND_TARGETS` (422), which map to HTTP via each error's
/// `http_status_code`.
async fn create_command(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CreateCommandRequest>,
) -> Response {
    let subject = match auth::require_auth(&state, &headers).await {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };
    let deployment = match state
        .deployment_store
        .get_deployment(&subject, &request.deployment_id)
        .await
    {
        Ok(Some(d)) => d,
        Ok(None) => return ErrorData::not_found_deployment(&request.deployment_id).into_response(),
        Err(e) => return e.into_response(),
    };

    if !state.authz.can_dispatch_command(&subject, &deployment) {
        return ErrorData::forbidden("Cannot dispatch command for this deployment").into_response();
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
            .into_response()
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

    let deployment = match state
        .deployment_store
        .get_deployment(&subject, &deployment_id)
        .await
    {
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
        let deployment = match state
            .deployment_store
            .get_deployment(&subject, &deployment_id)
            .await
        {
            Ok(Some(d)) => d,
            Ok(None) => return ErrorData::not_found_deployment(&deployment_id).into_response(),
            Err(e) => return e.into_response(),
        };
        if !state.authz.can_dispatch_command(&subject, &deployment) {
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
    // A platform-issued browser token can carry an exact command capability.
    // It was authorized against the control-plane command row before minting,
    // so it neither needs nor receives broader deployment access. All other
    // callers follow the entity-backed policy below.
    if !state.authz.can_read_command_payload(&subject, &command_id) {
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
    }; // Storing payload with no entity context is workspace-write only.
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
    let deployment = match state
        .deployment_store
        .get_deployment(&subject, &lease_request.deployment_id)
        .await
    {
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
/// Auth: Admin, DG (group), or own Deployment token — the caller must be able
/// to act on the deployment that HOLDS the lease (the lease owner), otherwise
/// any authenticated tenant that learned a lease_id could force-release
/// another deployment's lease and trigger a spurious redelivery.
async fn release_lease(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(lease_id): Path<String>,
) -> Response {
    let subject = match auth::require_auth(&state, &headers).await {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };

    let owner = match state.command_server.get_lease_owner(&lease_id).await {
        Ok(Some((_command_id, owner))) => owner,
        Ok(None) => {
            return alien_error::AlienError::new(alien_commands::error::ErrorData::LeaseNotFound {
                lease_id: lease_id.clone(),
            })
            .into_response()
        }
        Err(e) => return e.into_response(),
    };
    if let Err(e) = require_command_mutation_access(&state, &subject, &owner).await {
        return e;
    }

    match state.command_server.release_lease_by_id(&lease_id).await {
        Ok(()) => StatusCode::OK.into_response(),
        Err(e) => e.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, sync::Arc};

    use alien_bindings::providers::{kv::local::LocalKv, storage::local::LocalStorage};
    use alien_commands::{
        dispatchers::NullCommandDispatcher,
        server::{CommandDispatcher, CommandRegistry, CommandServer},
        types::BodySpec,
        InMemoryCommandRegistry,
    };
    use async_trait::async_trait;
    use axum::{body::Body, http::Request, http::StatusCode};
    use tower::ServiceExt;

    use crate::{
        auth::{Role, Scope, Subject, SubjectKind},
        config::ManagerConfig,
        providers::{local_credentials::LocalCredentialResolver, NullTelemetryBackend, OssAuthz},
        routes::{
            registry_proxy::{CredentialCache, PullValidationCache, RegistryRoutingTable},
            AppState,
        },
        stores::sqlite::{
            SqliteDatabase, SqliteDeploymentStore, SqliteReleaseStore, SqliteTokenStore,
        },
        traits::{
            AuthValidator, CredentialResolver, DeploymentStore, ReleaseStore, TelemetryBackend,
            TokenStore,
        },
    };

    use super::router;

    #[derive(Clone)]
    struct FixedSubjectValidator(Subject);

    #[async_trait]
    impl AuthValidator for FixedSubjectValidator {
        async fn validate(
            &self,
            _headers: &http::HeaderMap,
        ) -> Result<Option<Subject>, alien_error::AlienError> {
            Ok(Some(self.0.clone()))
        }
    }

    async fn command_capability_state(command_id: &str) -> (AppState, tempfile::TempDir) {
        let temp = tempfile::tempdir().expect("create command route test directory");
        let db = Arc::new(
            SqliteDatabase::new(&temp.path().join("manager.db").to_string_lossy())
                .await
                .expect("create test database"),
        );
        let deployment_store: Arc<dyn DeploymentStore> =
            Arc::new(SqliteDeploymentStore::new(db.clone()));
        let release_store: Arc<dyn ReleaseStore> = Arc::new(SqliteReleaseStore::new(db.clone()));
        let token_store: Arc<dyn TokenStore> = Arc::new(SqliteTokenStore::new(db));
        let kv: Arc<dyn alien_bindings::traits::Kv> = Arc::new(
            LocalKv::new(temp.path().join("kv"))
                .await
                .expect("create command KV"),
        );
        let storage: Arc<dyn alien_bindings::traits::Storage> = Arc::new(
            LocalStorage::new(temp.path().join("storage").to_string_lossy().to_string())
                .expect("create command storage"),
        );
        let dispatcher: Arc<dyn CommandDispatcher> = Arc::new(NullCommandDispatcher);
        let registry: Arc<dyn CommandRegistry> = Arc::new(InMemoryCommandRegistry::default());
        let command_server = Arc::new(CommandServer::new(
            kv.clone(),
            storage,
            dispatcher,
            registry,
            "http://localhost:0/v1".to_string(),
            b"test-signing-key".to_vec(),
        ));
        command_server
            .store_params(command_id, &BodySpec::inline(br#"{"safe":true}"#))
            .await
            .expect("store exact command payload");

        let subject = Subject {
            kind: SubjectKind::ServiceAccount {
                id: "platform-command-reader".to_string(),
            },
            workspace_id: "default".to_string(),
            scope: Scope::Command {
                project_id: "default".to_string(),
                deployment_id: "dep-1".to_string(),
                command_id: command_id.to_string(),
            },
            role: Role::CommandPayloadReader,
            bearer_token: "command-token".to_string(),
        };
        let credential_resolver: Arc<dyn CredentialResolver> = Arc::new(
            LocalCredentialResolver::new(temp.path().join("local-credentials")),
        );
        let telemetry_backend: Arc<dyn TelemetryBackend> = Arc::new(NullTelemetryBackend);

        (
            AppState {
                deployment_store,
                release_store,
                token_store,
                auth_validator: Arc::new(FixedSubjectValidator(subject)),
                authz: Arc::new(OssAuthz),
                telemetry_backend,
                credential_resolver,
                command_server,
                config: Arc::new(ManagerConfig::default()),
                bindings_provider: None,
                target_bindings_providers: HashMap::new(),
                kv,
                http_client: reqwest::Client::new(),
                credential_cache: Arc::new(CredentialCache::new()),
                pull_validation_cache: Arc::new(PullValidationCache::new()),
                registry_routing_table: Arc::new(
                    RegistryRoutingTable::new(vec![]).expect("empty registry routing table"),
                ),
                import_registry: Arc::new(alien_infra::ImporterRegistry::built_in()),
            },
            temp,
        )
    }

    fn request(method: &str, uri: &str, body: Body) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(uri)
            .header(http::header::AUTHORIZATION, "Bearer command-token")
            .header(http::header::CONTENT_TYPE, "application/json")
            .body(body)
            .expect("build command route request")
    }

    #[tokio::test]
    async fn exact_command_capability_only_reads_its_payload() {
        let command_id = "cmd-allowed";
        let (state, _temp) = command_capability_state(command_id).await;
        let app = router().with_state(state);

        let allowed = app
            .clone()
            .oneshot(request(
                "GET",
                &format!("/v1/commands/{command_id}/payload"),
                Body::empty(),
            ))
            .await
            .expect("exact payload request should complete");
        assert_eq!(allowed.status(), StatusCode::OK);

        let different_command = app
            .clone()
            .oneshot(request(
                "GET",
                "/v1/commands/cmd-denied/payload",
                Body::empty(),
            ))
            .await
            .expect("cross-command payload request should complete");
        assert_eq!(different_command.status(), StatusCode::FORBIDDEN);

        let store = app
            .oneshot(request(
                "PUT",
                &format!("/v1/commands/{command_id}/payload"),
                Body::from(r#"{"params":{"mode":"inline","inlineBase64":"e30="}}"#),
            ))
            .await
            .expect("payload store request should complete");
        assert_eq!(store.status(), StatusCode::FORBIDDEN);
    }
}
