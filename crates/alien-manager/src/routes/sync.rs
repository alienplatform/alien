//! State sync endpoints for deployment loop coordination.

use axum::{
    extract::{Json, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
    Router,
};
use serde::{Deserialize, Deserializer, Serialize};

/// Deserialize a bool that may be `null` (treat null as false).
fn deserialize_bool_or_null<'de, D: Deserializer<'de>>(deserializer: D) -> Result<bool, D::Error> {
    Option::<bool>::deserialize(deserializer).map(|opt| opt.unwrap_or(false))
}

use alien_core::{
    sync::TargetDeployment, DeploymentConfig, DeploymentState, DeploymentStatus,
    EnvironmentVariable, EnvironmentVariablesSnapshot, Platform, ReleaseInfo, ResourceHeartbeat,
};
use alien_error::AlienError;

use crate::error::ErrorData;
use crate::ids;
use crate::traits::{
    CreateDeploymentParams, CreateTokenParams, DeploymentAcquireMode, DeploymentFilter,
    DeploymentRecord, ReconcileData, ReleaseRecord, TokenType,
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
    #[serde(default)]
    pub setup_method: Option<String>,
    #[serde(default)]
    pub acquire_mode: Option<String>,
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
    pub state: serde_json::Value,
    #[serde(default, deserialize_with = "deserialize_bool_or_null")]
    pub update_heartbeat: bool,
    #[serde(default)]
    pub suggested_delay_ms: Option<u64>,
    #[serde(default)]
    pub heartbeats: Vec<ResourceHeartbeat>,
}

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ReconcileResponse {
    pub success: bool,
    pub current: serde_json::Value,
    /// Native image registry host for Lambda/Cloud Run.
    /// Returned so push clients can set it on their local DeploymentConfig.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub native_image_host: Option<String>,
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
pub struct AgentSyncRequest {
    pub deployment_id: String,
    /// Current deployment state as reported by the agent.
    /// When present, the manager updates the deployment record to reflect
    /// the agent's progress (status, stack_state, etc.).
    #[serde(default)]
    pub current_state: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct AgentSyncResponse {
    /// Authoritative deployment state from the manager.
    ///
    /// Returned when a pull deployment attaches with an empty local state while
    /// the manager already has imported or previously reconciled state.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_state: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<serde_json::Value>,
    /// Public URL for the commands API. Cloud-deployed workers use this
    /// to poll for pending commands instead of the agent's local sync URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commands_url: Option<String>,
}

#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct InitializeRequest {
    pub name: Option<String>,
    pub platform: Option<Platform>,
    /// Optional base cloud platform for Kubernetes setup targets such as
    /// EKS/GKE/AKS. The runtime platform remains Kubernetes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_platform: Option<Platform>,
    pub stack_settings: Option<alien_core::StackSettings>,
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
        .route("/v1/sync", post(agent_sync))
}

/// Router for the `/v1/initialize` endpoint only.
///
/// Separated so embedders can replace it with their own implementation
/// (for example, one that proxies token creation to an upstream API).
pub fn initialize_router() -> Router<AppState> {
    Router::new().route("/v1/initialize", post(initialize))
}

// --- Handlers ---

/// `POST /v1/sync/acquire` — Inbound: workspace / dg / deployment bearer.
/// `caller: &Subject` is threaded into `DeploymentStore::acquire` so
/// embedders can authorize against the inbound caller's scope.
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
    // If the caller named specific deployments, fetch each and run
    // `Authz::can_acquire_deployments` over the slice. Workspace-scoped
    // callers (e.g., legacy admin tokens) can run with no id list — the
    // store-side filter applies.
    if let Some(ids) = req.deployment_ids.as_ref() {
        let mut deployments = Vec::with_capacity(ids.len());
        for id in ids {
            match state.deployment_store.get_deployment(&subject, id).await {
                Ok(Some(d)) => deployments.push(d),
                Ok(None) => return ErrorData::not_found_deployment(id).into_response(),
                Err(e) => return e.into_response(),
            }
        }
        if !state.authz.can_acquire_deployments(&subject, &deployments) {
            return ErrorData::forbidden("Access denied: cannot acquire these deployments")
                .into_response();
        }
    } else if !matches!(subject.scope, crate::auth::Scope::Workspace) {
        return ErrorData::forbidden(
            "Access denied: only workspace-scoped tokens can acquire without an id filter",
        )
        .into_response();
    }

    let acquire_mode = match req.acquire_mode.as_deref() {
        None => None,
        Some("runtime") => Some(DeploymentAcquireMode::Runtime),
        Some("setup-run") => Some(DeploymentAcquireMode::SetupRun),
        Some("setup-teardown") => Some(DeploymentAcquireMode::SetupTeardown),
        Some(other) => {
            return ErrorData::bad_request(format!("Invalid acquireMode: {other}")).into_response();
        }
    };

    let filter = DeploymentFilter {
        deployment_group_id: None,
        deployment_ids: req.deployment_ids,
        statuses: req.statuses,
        platforms: req.platforms,
        setup_method: req.setup_method,
        acquire_mode,
        limit: Some(req.limit),
        ..DeploymentFilter::default()
    };

    let acquired = match state
        .deployment_store
        .acquire(&subject, &req.session, &filter, req.limit)
        .await
    {
        Ok(a) => a,
        Err(e) => return e.into_response(),
    };

    let deployments: Vec<AcquiredDeploymentResponse> = match acquired
        .into_iter()
        .map(|a| {
            let mut deployment = serde_json::to_value(&a.deployment)?;
            if let (Some(config), Some(object)) = (
                a.deployment.deployment_config.as_ref(),
                deployment.as_object_mut(),
            ) {
                object.insert(
                    "deploymentConfig".to_string(),
                    serde_json::to_value(config)?,
                );
            }
            Ok::<_, serde_json::Error>(AcquiredDeploymentResponse { deployment })
        })
        .collect::<Result<Vec<_>, _>>()
    {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!("Failed to serialize deployment: {e}");
            return ErrorData::internal("Failed to serialize deployment").into_response();
        }
    };

    Json(AcquireResponse { deployments }).into_response()
}

/// `POST /v1/sync/reconcile` — Inbound: workspace / dg / deployment
/// bearer. `caller: &Subject` is threaded into `DeploymentStore::reconcile`.
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

    // Allow admin tokens (push mode) or deployment tokens (pull mode) per
    // the unified Authz policy.
    let deployment = match state
        .deployment_store
        .get_deployment(&subject, &req.deployment_id)
        .await
    {
        Ok(Some(d)) => d,
        Ok(None) => return ErrorData::not_found_deployment(&req.deployment_id).into_response(),
        Err(e) => return e.into_response(),
    };
    if !state.authz.can_sync_deployment(&subject, &deployment) {
        return ErrorData::forbidden("Access denied").into_response();
    }

    // Deserialize state from opaque JSON to DeploymentState.
    // The API accepts serde_json::Value to avoid data loss in generated SDK clients
    // (Progenitor strips additionalProperties fields from typed structs during roundtrip).
    let deployment_state: DeploymentState = match serde_json::from_value(req.state.clone()) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("Failed to deserialize deployment state: {e}");
            return ErrorData::bad_request("Invalid deployment state").into_response();
        }
    };

    // 1. Run registry access reconciliation (cross-account IAM grants).
    //    This must happen before persisting so the `registry_access_granted`
    //    flag is included in the persisted state (matches ManagerTransport).
    let mut final_state = deployment_state;
    crate::registry_access::reconcile_registry_access(
        &state.bindings_provider,
        &state.target_bindings_providers,
        &req.deployment_id,
        &mut final_state,
    )
    .await;

    // 1a. Inject target_release from the deployment record's
    //     desired_release_id. The CLI's deploy command initializes state with
    //     `target_release: None` and depends on the manager to populate it on
    //     reconcile — without this, handlers like `pending::handle_pending`
    //     fail with "Target release required for deployment" on the first
    //     step.
    if final_state.target_release.is_none() {
        if let Some(ref release_id) = deployment.desired_release_id {
            let system = crate::auth::Subject::system();
            if let Ok(Some(rel)) = state.release_store.get_release(&system, release_id).await {
                let release_stack_platform = release_stack_platform(deployment.platform);
                if let Some(info) = release_info_from_record(&rel, release_stack_platform) {
                    final_state.target_release = Some(info);
                }
            }
        }
    }

    // 2. Persist the step result (including any registry access changes).
    let _result = match state
        .deployment_store
        .reconcile(
            &subject,
            ReconcileData {
                deployment_id: req.deployment_id.clone(),
                session: req.session,
                state: final_state.clone(),
                update_heartbeat: req.update_heartbeat,
                suggested_delay_ms: req.suggested_delay_ms,
                heartbeats: req.heartbeats,
            },
        )
        .await
    {
        Ok(r) => r,
        Err(e) => return e.into_response(),
    };

    // Derive native image host for Lambda/Cloud Run so push clients
    // can set it on their local DeploymentConfig.
    let native_image_host = crate::registry_access::derive_native_image_host(
        &state.bindings_provider,
        &state.target_bindings_providers,
        &final_state.platform,
    )
    .await;

    let current = match serde_json::to_value(&final_state) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("Failed to serialize reconciled state: {e}");
            return ErrorData::internal("Failed to serialize reconciled state").into_response();
        }
    };

    Json(ReconcileResponse {
        success: true,
        current,
        native_image_host,
    })
    .into_response()
}

/// `POST /v1/sync/release` — Inbound: workspace / dg / deployment bearer.
/// `caller: &Subject` is threaded into `DeploymentStore::release`.
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
    let deployment = match state
        .deployment_store
        .get_deployment(&subject, &req.deployment_id)
        .await
    {
        Ok(Some(d)) => d,
        Ok(None) => return ErrorData::not_found_deployment(&req.deployment_id).into_response(),
        Err(e) => return e.into_response(),
    };
    if !state.authz.can_sync_deployment(&subject, &deployment) {
        return ErrorData::forbidden("Access denied").into_response();
    }

    match state
        .deployment_store
        .release(&subject, &req.deployment_id, &req.session)
        .await
    {
        Ok(()) => Json(serde_json::json!({ "success": true })).into_response(),
        Err(e) => e.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use alien_core::{
        DeploymentConfig, DeploymentState, DeploymentStatus, EnvironmentVariablesSnapshot,
        ExternalBindings, Platform, ResourceHeartbeatData, StackSettings, StackState,
        CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
    };
    use chrono::Utc;
    use serde_json::json;
    use std::collections::HashMap;

    use crate::traits::DeploymentRecord;

    use super::{
        build_target_deployment_config, deployment_needs_target, deployment_state_from_record,
        management_platform, release_stack_platform, should_ignore_agent_state_report,
        should_return_current_state_for_agent_sync, validate_initialize_base_platform,
        ReconcileRequest,
    };

    #[test]
    fn release_stack_platform_keeps_imported_kubernetes_deployment_platform() {
        assert_eq!(
            release_stack_platform(Platform::Kubernetes),
            Platform::Kubernetes
        );
    }

    #[test]
    fn release_stack_platform_defaults_to_deployment_platform() {
        assert_eq!(release_stack_platform(Platform::Gcp), Platform::Gcp);
    }

    #[test]
    fn management_platform_uses_base_platform_for_imported_kubernetes_deployments() {
        assert_eq!(
            management_platform(Platform::Kubernetes, Some(Platform::Aws)),
            Platform::Aws
        );
    }

    #[test]
    fn reconcile_request_defaults_missing_heartbeats_to_empty() {
        let req: ReconcileRequest = serde_json::from_value(json!({
            "deploymentId": "dep_test",
            "session": "session_test",
            "state": { "status": "running" },
            "updateHeartbeat": true
        }))
        .expect("reconcile request without heartbeats should remain compatible");

        assert!(req.heartbeats.is_empty());
        assert!(req.update_heartbeat);
    }

    #[test]
    fn reconcile_request_accepts_typed_heartbeats() {
        let req: ReconcileRequest = serde_json::from_value(json!({
            "deploymentId": "dep_test",
            "session": "session_test",
            "state": { "status": "running" },
            "updateHeartbeat": true,
            "heartbeats": [{
                "deploymentId": "dep_test",
                "resourceId": "api",
                "resourceType": "container",
                "controllerPlatform": "kubernetes",
                "backend": "kubernetes",
                "observedAt": "2026-01-01T00:00:00Z",
                "data": {
                    "resourceType": "container",
                    "data": {
                        "backend": "kubernetes",
                        "status": {
                            "health": "healthy",
                            "lifecycle": "running",
                            "message": null,
                            "stale": false,
                            "partial": true,
                            "collectionIssues": [{
                                "source": "metrics",
                                "reason": "not-installed",
                                "severity": "warning",
                                "message": "metrics API is not installed"
                            }]
                        },
                        "namespace": "default",
                        "name": "api",
                        "workloadKind": "deployment",
                        "replicas": { "desired": 2, "current": 2, "ready": 2, "available": 2, "updated": null, "misscheduled": null },
                        "restarts": 0,
                        "cpu": null,
                        "memory": null,
                        "workload": null,
                        "pods": [],
                        "instances": [],
                        "events": []
                    }
                },
                "raw": []
            }]
        }))
        .expect("reconcile request should accept typed heartbeat envelopes");

        assert_eq!(req.heartbeats.len(), 1);
        assert_eq!(req.heartbeats[0].resource_id, "api");
        assert!(matches!(
            req.heartbeats[0].data,
            ResourceHeartbeatData::Container(_)
        ));
    }

    #[test]
    fn initialize_accepts_cloud_base_platform_for_kubernetes() {
        assert_eq!(
            validate_initialize_base_platform(Platform::Kubernetes, Some(Platform::Gcp)).unwrap(),
            Some(Platform::Gcp)
        );
    }

    #[test]
    fn initialize_rejects_base_platform_for_non_kubernetes_platform() {
        assert!(validate_initialize_base_platform(Platform::Aws, Some(Platform::Gcp)).is_err());
    }

    #[test]
    fn initialize_rejects_non_cloud_base_platform_for_kubernetes() {
        assert!(
            validate_initialize_base_platform(Platform::Kubernetes, Some(Platform::Local)).is_err()
        );
    }

    #[test]
    fn ignores_blank_pull_state_when_manager_has_imported_stack_state() {
        let deployment =
            deployment_record_with_state("initial-setup", Some(StackState::new(Platform::Aws)));
        let agent_state = uninitialized_state();

        assert!(should_ignore_agent_state_report(&deployment, &agent_state));
    }

    #[test]
    fn accepts_blank_pull_state_for_uninitialized_manager_deployment() {
        let deployment = deployment_record_with_state("pending", None);
        let agent_state = uninitialized_state();

        assert!(!should_ignore_agent_state_report(&deployment, &agent_state));
    }

    #[test]
    fn accepts_non_blank_pull_state_even_when_manager_has_state() {
        let deployment =
            deployment_record_with_state("initial-setup", Some(StackState::new(Platform::Aws)));
        let mut agent_state = uninitialized_state();
        agent_state.status = DeploymentStatus::Provisioning;

        assert!(!should_ignore_agent_state_report(&deployment, &agent_state));
    }

    #[test]
    fn returns_current_state_when_retry_is_pending() {
        let mut deployment = deployment_record_with_state(
            "provisioning-failed",
            Some(StackState::new(Platform::Local)),
        );
        deployment.retry_requested = true;

        assert!(should_return_current_state_for_agent_sync(
            false,
            &deployment
        ));
    }

    #[test]
    fn returns_current_state_when_agent_blank_state_was_ignored() {
        let deployment =
            deployment_record_with_state("running", Some(StackState::new(Platform::Local)));

        assert!(should_return_current_state_for_agent_sync(
            true,
            &deployment
        ));
    }

    #[test]
    fn builds_authoritative_state_from_manager_record() {
        let stack_state = StackState::with_resource_prefix(Platform::Aws, "abc123".to_string());
        let deployment = deployment_record_with_state("initial-setup", Some(stack_state.clone()));

        let state = deployment_state_from_record(&deployment, None, None).unwrap();

        assert_eq!(state.status, DeploymentStatus::InitialSetup);
        assert_eq!(state.protocol_version, CURRENT_DEPLOYMENT_PROTOCOL_VERSION);
        assert_eq!(
            state.stack_state.unwrap().resource_prefix,
            stack_state.resource_prefix
        );
    }

    #[test]
    fn provisioning_deployment_needs_target_even_when_current_matches_desired() {
        let mut deployment = deployment_record_with_state("provisioning", None);
        deployment.current_release_id = Some("rel_test".to_string());
        deployment.desired_release_id = Some("rel_test".to_string());

        assert!(deployment_needs_target(&deployment));
    }

    #[test]
    fn running_deployment_does_not_need_target_when_current_matches_desired() {
        let mut deployment = deployment_record_with_state("running", None);
        deployment.current_release_id = Some("rel_test".to_string());
        deployment.desired_release_id = Some("rel_test".to_string());

        assert!(!deployment_needs_target(&deployment));
    }

    #[test]
    fn deployment_needs_target_when_desired_differs_from_current() {
        let mut deployment = deployment_record_with_state("running", None);
        deployment.current_release_id = Some("rel_old".to_string());
        deployment.desired_release_id = Some("rel_new".to_string());

        assert!(deployment_needs_target(&deployment));
    }

    #[test]
    fn target_config_preserves_control_plane_public_endpoints() {
        let mut deployment = deployment_record_with_state("provisioning", None);
        let public_endpoints = HashMap::from([(
            "bear-agent-loader".to_string(),
            HashMap::from([(
                "shares".to_string(),
                "https://shares.byoc.example.test".to_string(),
            )]),
        )]);
        deployment.deployment_config = Some(DeploymentConfig {
            public_endpoints: Some(public_endpoints.clone()),
            ..test_deployment_config()
        });

        let config = build_target_deployment_config(
            &deployment,
            StackSettings::default(),
            None,
            vec![],
            "https://manager.example.test".to_string(),
            Some("ax_dep_test".to_string()),
            None,
        );

        assert_eq!(config.public_endpoints, Some(public_endpoints));
    }

    fn uninitialized_state() -> DeploymentState {
        DeploymentState {
            platform: Platform::Kubernetes,
            status: DeploymentStatus::Pending,
            current_release: None,
            target_release: None,
            stack_state: None,
            error: None,
            environment_info: None,
            runtime_metadata: None,
            retry_requested: false,
            protocol_version: CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
        }
    }

    fn test_deployment_config() -> DeploymentConfig {
        DeploymentConfig {
            deployment_name: None,
            stack_settings: StackSettings::default(),
            management_config: None,
            environment_variables: EnvironmentVariablesSnapshot {
                variables: vec![],
                hash: String::new(),
                created_at: String::new(),
            },
            allow_frozen_changes: false,
            compute_backend: None,
            external_bindings: ExternalBindings::default(),
            base_platform: None,
            public_endpoints: None,
            domain_metadata: None,
            monitoring: None,
            manager_url: None,
            deployment_token: None,
            native_image_host: None,
        }
    }

    fn deployment_record_with_state(
        status: &str,
        stack_state: Option<StackState>,
    ) -> DeploymentRecord {
        let now = Utc::now();
        DeploymentRecord {
            id: "dep_test".to_string(),
            workspace_id: "default".to_string(),
            project_id: "default".to_string(),
            name: "test".to_string(),
            deployment_group_id: "dg_test".to_string(),
            platform: Platform::Kubernetes,
            deployment_protocol_version: CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
            base_platform: Some(Platform::Aws),
            status: status.to_string(),
            stack_settings: Some(StackSettings::default()),
            stack_state,
            environment_info: None,
            runtime_metadata: None,
            current_release_id: None,
            desired_release_id: None,
            import_source: None,
            setup_method: None,
            setup_metadata: None,
            setup_target: None,
            setup_fingerprint: None,
            setup_fingerprint_version: None,
            user_environment_variables: None,
            management_config: None,
            deployment_token: None,
            deployment_config: None,
            retry_requested: false,
            locked_by: None,
            locked_at: None,
            created_at: now,
            updated_at: Some(now),
            error: None,
        }
    }
}

/// `POST /v1/sync` — Inbound: deployment bearer. The agent-driven sync
/// path; `caller: &Subject` is threaded into the store so embedders see
/// the agent's own scope.
#[cfg_attr(feature = "openapi", utoipa::path(
    post,
    path = "/v1/sync",
    tag = "sync",
    request_body = AgentSyncRequest,
    responses(
        (status = 200, description = "Agent sync response with optional target state", body = AgentSyncResponse)
    ),
    security(
        ("bearer" = [])
    )
))]
async fn agent_sync(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<AgentSyncRequest>,
) -> Response {
    let subject = match auth::require_auth(&state, &headers).await {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };

    // Must be a deployment token matching this deployment (workspace-scoped
    // tokens are accepted by `Authz::can_sync_deployment` for system flows).
    let deployment = match state
        .deployment_store
        .get_deployment(&subject, &req.deployment_id)
        .await
    {
        Ok(Some(d)) => d,
        Ok(None) => return ErrorData::not_found_deployment(&req.deployment_id).into_response(),
        Err(e) => return e.into_response(),
    };
    if !state.authz.can_sync_deployment(&subject, &deployment) {
        return ErrorData::forbidden("Access denied").into_response();
    }

    // If the agent reported its current state, persist it to the deployment record.
    // This is how pull-mode agents propagate status changes (e.g. Pending → Running)
    // back to the manager so that API consumers can observe deployment progress.
    let mut ignored_agent_state_report = false;
    if let Some(ref current_state_value) = req.current_state {
        match serde_json::from_value::<DeploymentState>(current_state_value.clone()) {
            Ok(mut agent_state) => {
                if should_ignore_agent_state_report(&deployment, &agent_state) {
                    ignored_agent_state_report = true;
                    tracing::info!(
                        deployment_id = %req.deployment_id,
                        "Ignoring empty pull sync state because manager already has deployment state"
                    );
                } else {
                    // Reconcile registry access before persisting so the flag
                    // is saved in the same DB write.
                    crate::registry_access::reconcile_registry_access(
                        &state.bindings_provider,
                        &state.target_bindings_providers,
                        &req.deployment_id,
                        &mut agent_state,
                    )
                    .await;

                    if let Err(e) = state
                        .deployment_store
                        .reconcile(
                            &subject,
                            ReconcileData {
                                deployment_id: req.deployment_id.clone(),
                                session: "agent-sync".to_string(),
                                state: agent_state.clone(),
                                update_heartbeat: true,
                                heartbeats: vec![],
                                suggested_delay_ms: None,
                            },
                        )
                        .await
                    {
                        tracing::warn!(
                            deployment_id = %req.deployment_id,
                            error = %e,
                            "Failed to reconcile agent-reported state"
                        );
                    }
                }
            }
            Err(e) => {
                tracing::warn!(
                    deployment_id = %req.deployment_id,
                    error = %e,
                    "Failed to deserialize agent current_state"
                );
            }
        }
    }

    let deployment = match state
        .deployment_store
        .get_deployment(&subject, &req.deployment_id)
        .await
    {
        Ok(Some(d)) => d,
        Ok(None) => return ErrorData::not_found_deployment(&req.deployment_id).into_response(),
        Err(e) => return e.into_response(),
    };

    // Return target state if deployment needs updating
    let target = if deployment_needs_target(&deployment) {
        let release = if let Some(ref release_id) = deployment.desired_release_id {
            let system = crate::auth::Subject::system();
            match state.release_store.get_release(&system, release_id).await {
                Ok(Some(r)) => Some(r),
                _ => None,
            }
        } else {
            None
        };

        let release_stack_platform = release_stack_platform(deployment.platform);
        let management_platform =
            management_platform(deployment.platform, deployment.base_platform);

        // Resolve management config (same pattern as push-mode deployment loop).
        // 1. From deployment record (platform API / private managers)
        // 2. From credential resolver (derived from management binding env vars)
        let management_config = if let Some(mc) = deployment.management_config.clone() {
            Some(mc)
        } else {
            state
                .credential_resolver
                .resolve_management_config(management_platform)
                .await
                .unwrap_or(None)
        };

        // Image pull credentials are no longer passed through the sync response.
        // Pull-model agents pull images through the manager's /v2/ registry proxy.
        // Push-model Azure Container Apps also use the proxy (they support any registry).
        // Only AWS Lambda and GCP Cloud Run pull directly from native registries.

        // Extract the agent's deployment token from the Authorization header.
        // This is reused for pull auth — no new tokens created.
        let agent_token = headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.strip_prefix("Bearer "))
            .map(|t| t.to_string());

        let manager_url = state.config.base_url();

        // Derive native image host for Lambda/Cloud Run so controllers
        // can resolve proxy URIs to native ECR/GAR URIs.
        let native_image_host = crate::registry_access::derive_native_image_host(
            &state.bindings_provider,
            &state.target_bindings_providers,
            &management_platform,
        )
        .await;

        match release {
            Some(r) => {
                let stack = match r.stacks.get(&release_stack_platform) {
                    Some(s) => s.clone(),
                    None => {
                        return ErrorData::internal(format!(
                            "Release {} does not contain a stack for platform {}",
                            r.id, release_stack_platform
                        ))
                        .into_response();
                    }
                };

                let env_vars: Vec<EnvironmentVariable> = deployment
                    .user_environment_variables
                    .clone()
                    .unwrap_or_default();

                // Records loaded for sync always carry stack settings; in a
                // handler, answer with a 500 rather than panic-dropping the
                // connection if that invariant is ever broken.
                let stack_settings = match deployment.stack_settings.clone() {
                    Some(settings) => settings,
                    None => {
                        return ErrorData::internal("synced deployment is missing stack_settings")
                            .into_response();
                    }
                };
                let config = build_target_deployment_config(
                    &deployment,
                    stack_settings,
                    management_config,
                    env_vars,
                    manager_url,
                    agent_token,
                    native_image_host,
                );

                Some(TargetDeployment {
                    release_info: ReleaseInfo {
                        release_id: r.id,
                        version: None,
                        description: None,
                        stack,
                    },
                    config,
                })
            }
            None => None,
        }
    } else {
        None
    };

    let should_return_current_state =
        should_return_current_state_for_agent_sync(ignored_agent_state_report, &deployment);
    let current_state = if should_return_current_state {
        let release_stack_platform = release_stack_platform(deployment.platform);
        let current_release = if let Some(ref release_id) = deployment.current_release_id {
            let system = crate::auth::Subject::system();
            match state.release_store.get_release(&system, release_id).await {
                Ok(Some(release)) => release_info_from_record(&release, release_stack_platform),
                Ok(None) => None,
                Err(e) => {
                    tracing::warn!(
                        deployment_id = %req.deployment_id,
                        release_id = %release_id,
                        error = %e,
                        "Failed to load current release for state hydration"
                    );
                    None
                }
            }
        } else {
            None
        };
        let target_release = target.as_ref().map(|t| t.release_info.clone());
        match deployment_state_from_record(&deployment, current_release, target_release) {
            Some(deployment_state) => match serde_json::to_value(deployment_state) {
                Ok(v) => Some(v),
                Err(e) => {
                    tracing::warn!("Failed to serialize deployment state: {e}");
                    return ErrorData::internal("Failed to serialize deployment state")
                        .into_response();
                }
            },
            None => None,
        }
    } else {
        None
    };

    Json(AgentSyncResponse {
        current_state,
        target: match target.map(|t| serde_json::to_value(&t)).transpose() {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("Failed to serialize target deployment: {e}");
                return ErrorData::internal("Failed to serialize target deployment")
                    .into_response();
            }
        },
        commands_url: Some(state.config.commands_base_url()),
    })
    .into_response()
}

fn release_stack_platform(platform: Platform) -> Platform {
    platform
}

fn build_target_deployment_config(
    deployment: &DeploymentRecord,
    stack_settings: alien_core::StackSettings,
    management_config: Option<alien_core::ManagementConfig>,
    env_vars: Vec<EnvironmentVariable>,
    manager_url: String,
    agent_token: Option<String>,
    native_image_host: Option<String>,
) -> DeploymentConfig {
    let deployment_config = deployment.deployment_config.as_ref();

    DeploymentConfig::builder()
        .deployment_name(deployment.name.clone())
        .stack_settings(stack_settings.clone())
        .maybe_management_config(management_config)
        .environment_variables(EnvironmentVariablesSnapshot {
            variables: env_vars,
            hash: String::new(),
            created_at: String::new(),
        })
        .allow_frozen_changes(false)
        .maybe_compute_backend(deployment_config.and_then(|config| config.compute_backend.clone()))
        .external_bindings(stack_settings.external_bindings.clone().unwrap_or_default())
        .maybe_base_platform(deployment.base_platform)
        .maybe_public_endpoints(
            deployment_config.and_then(|config| config.public_endpoints.clone()),
        )
        .maybe_domain_metadata(deployment_config.and_then(|config| config.domain_metadata.clone()))
        .maybe_monitoring(deployment_config.and_then(|config| config.monitoring.clone()))
        .maybe_manager_url(Some(manager_url))
        .maybe_deployment_token(agent_token)
        .maybe_native_image_host(native_image_host)
        .build()
}

fn management_platform(platform: Platform, base_platform: Option<Platform>) -> Platform {
    base_platform.unwrap_or(platform)
}

fn validate_initialize_base_platform(
    platform: Platform,
    base_platform: Option<Platform>,
) -> std::result::Result<Option<Platform>, alien_error::AlienError<ErrorData>> {
    let Some(base_platform) = base_platform else {
        return Ok(None);
    };

    if platform != Platform::Kubernetes {
        return Err(ErrorData::bad_request(
            "basePlatform is only supported when platform is kubernetes",
        ));
    }

    match base_platform {
        Platform::Aws | Platform::Gcp | Platform::Azure => Ok(Some(base_platform)),
        Platform::Kubernetes | Platform::Local | Platform::Test => Err(ErrorData::bad_request(
            "basePlatform for kubernetes must be one of aws, gcp, or azure",
        )),
    }
}

fn should_ignore_agent_state_report(
    deployment: &DeploymentRecord,
    agent_state: &DeploymentState,
) -> bool {
    agent_state_is_uninitialized(agent_state) && deployment_has_authoritative_state(deployment)
}

fn agent_state_is_uninitialized(state: &DeploymentState) -> bool {
    state.status == DeploymentStatus::Pending
        && state.current_release.is_none()
        && state.target_release.is_none()
        && state.stack_state.is_none()
        && state.environment_info.is_none()
        && state.runtime_metadata.is_none()
}

fn deployment_has_authoritative_state(deployment: &DeploymentRecord) -> bool {
    deployment.stack_state.is_some()
        || deployment.environment_info.is_some()
        || deployment.runtime_metadata.is_some()
        || deployment.current_release_id.is_some()
        || deployment.status != "pending"
}

fn should_return_current_state_for_agent_sync(
    ignored_agent_state_report: bool,
    deployment: &DeploymentRecord,
) -> bool {
    ignored_agent_state_report || deployment.retry_requested
}

fn deployment_needs_target(deployment: &DeploymentRecord) -> bool {
    let Some(desired_release_id) = deployment.desired_release_id.as_ref() else {
        return false;
    };

    if deployment.current_release_id.as_ref() != Some(desired_release_id) {
        return true;
    }

    !matches!(
        deployment_status_from_record(&deployment.status),
        Some(DeploymentStatus::Running)
    )
}

fn deployment_state_from_record(
    deployment: &DeploymentRecord,
    current_release: Option<ReleaseInfo>,
    target_release: Option<ReleaseInfo>,
) -> Option<DeploymentState> {
    let status = deployment_status_from_record(&deployment.status)?;
    Some(DeploymentState {
        platform: deployment.platform,
        status,
        current_release,
        target_release,
        stack_state: deployment.stack_state.clone(),
        error: deployment_record_error(&deployment.error),
        environment_info: deployment.environment_info.clone(),
        runtime_metadata: deployment.runtime_metadata.clone(),
        retry_requested: deployment.retry_requested,
        protocol_version: deployment.deployment_protocol_version,
    })
}

fn deployment_status_from_record(status: &str) -> Option<DeploymentStatus> {
    serde_json::from_value(serde_json::Value::String(status.to_string())).ok()
}

fn deployment_record_error(error: &Option<serde_json::Value>) -> Option<AlienError> {
    error
        .clone()
        .and_then(|value| serde_json::from_value::<AlienError>(value).ok())
}

fn release_info_from_record(
    release: &ReleaseRecord,
    release_stack_platform: Platform,
) -> Option<ReleaseInfo> {
    Some(ReleaseInfo {
        release_id: release.id.clone(),
        version: None,
        description: None,
        stack: release.stacks.get(&release_stack_platform)?.clone(),
    })
}

/// `POST /v1/initialize` — Inbound: deployment-group bearer (typical),
/// or workspace bearer for self-hosted operator workflows. New deployments
/// are created via `DeploymentStore::create_deployment(caller, …)` so
/// embedders that proxy to an upstream API write the row in the dg's
/// workspace, not the manager's.
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

    match subject.scope.clone() {
        crate::auth::Scope::Deployment { deployment_id, .. } => {
            // Already has a deployment - return its ID
            Json(InitializeResponse {
                deployment_id,
                token: None,
            })
            .into_response()
        }
        crate::auth::Scope::DeploymentGroup {
            deployment_group_id: dg_id,
            ..
        } => {
            let name = req
                .name
                .unwrap_or_else(|| format!("agent-{}", &ids::deployment_id()[3..9]));
            let platform = req.platform.unwrap_or(Platform::Kubernetes);
            let base_platform = match validate_initialize_base_platform(platform, req.base_platform)
            {
                Ok(base_platform) => base_platform,
                Err(e) => return e.into_response(),
            };

            // Idempotency: if a deployment with this name already exists in the
            // group, issue a fresh deployment token and return the existing ID.
            if let Ok(Some(existing)) = state
                .deployment_store
                .get_deployment_by_name(&subject, &dg_id, &name)
                .await
            {
                let (raw_token, key_prefix, key_hash) =
                    ids::generate_token(TokenType::Deployment.prefix());
                return match state
                    .token_store
                    .create_token(CreateTokenParams {
                        token_type: TokenType::Deployment,
                        key_prefix,
                        key_hash,
                        deployment_group_id: Some(dg_id),
                        deployment_id: Some(existing.id.clone()),
                    })
                    .await
                {
                    Ok(_) => Json(InitializeResponse {
                        deployment_id: existing.id,
                        token: Some(raw_token),
                    })
                    .into_response(),
                    Err(e) => e.into_response(),
                };
            }

            let settings = req.stack_settings.unwrap_or_else(|| {
                let mut settings = alien_core::StackSettings::default();
                settings.deployment_model = match platform {
                    Platform::Aws | Platform::Gcp | Platform::Azure | Platform::Test => {
                        alien_core::DeploymentModel::Push
                    }
                    Platform::Kubernetes | Platform::Local => alien_core::DeploymentModel::Pull,
                };
                settings
            });

            // Create deployment with a token (reuse the agent's Bearer token)
            let dep_token = headers
                .get("authorization")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.strip_prefix("Bearer "))
                .map(|t| t.to_string());

            let deployment = match state
                .deployment_store
                .create_deployment(
                    &subject,
                    CreateDeploymentParams {
                        deployment_protocol_version:
                            alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
                        name,
                        deployment_group_id: dg_id.clone(),
                        platform,
                        base_platform,
                        stack_settings: settings,
                        stack_state: None,
                        environment_variables: None,
                        deployment_token: dep_token,
                    },
                )
                .await
            {
                Ok(d) => d,
                Err(e) => return e.into_response(),
            };

            // Auto-assign latest release if available. Initialize is the
            // agent's own bootstrap: keep the caller's subject for both
            // reads and writes so embedders can authorize against the
            // agent's scope rather than a service credential.
            if let Ok(Some(release)) = state.release_store.get_latest_release(&subject).await {
                let _ = state
                    .deployment_store
                    .set_deployment_desired_release(&subject, &deployment.id, &release.id)
                    .await;
            }

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
        crate::auth::Scope::Workspace | crate::auth::Scope::Project { .. } => {
            // Admin / workspace tokens on standalone managers: find the most
            // recent deployment and assign the agent to it. Self-hosted
            // workflow where the operator creates a deployment via the API
            // and then starts an agent with the admin token.
            let filter = DeploymentFilter {
                deployment_group_id: None,
                deployment_ids: None,
                statuses: None,
                platforms: None,
                limit: Some(1),
                ..DeploymentFilter::default()
            };
            match state
                .deployment_store
                .list_deployments(&subject, &filter)
                .await
            {
                Ok(deployments) if !deployments.is_empty() => {
                    let deployment_id = deployments[0].id.clone();
                    tracing::info!(
                        %deployment_id,
                        "Admin token: assigning agent to existing deployment"
                    );
                    Json(InitializeResponse {
                        deployment_id,
                        token: None,
                    })
                    .into_response()
                }
                Ok(_) => ErrorData::bad_request(
                    "No deployments found. Create a deployment before initializing an agent.",
                )
                .into_response(),
                Err(e) => e.into_response(),
            }
        }
    }
}
