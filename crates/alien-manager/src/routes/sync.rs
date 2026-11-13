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
    sync::{
        ObservedApplicationReport, OperationsReport, OperatorCapabilityReport, OperatorImageReport,
        TargetDeployment, TargetOperationsBundleSet,
    },
    DeploymentConfig, DeploymentModel, DeploymentState, DeploymentStatus, EnvironmentVariable,
    EnvironmentVariablesSnapshot, KeyOutputs, ObservedInventoryBatch, Platform, ReleaseInfo,
    ResourceHeartbeat, ResourceOutputs, StackState,
};
use alien_error::{AlienError, Context, GenericError, IntoAlienError};

use crate::error::ErrorData;
use crate::ids;
use crate::loops::deployment::{target_release_stack, ManagementRenderer, RunnerConfigSources};
use crate::traits::{
    CreateDeploymentParams, CreateTokenParams, DeploymentAcquireMode, DeploymentFilter,
    DeploymentRecord, ReconcileData, ReconcileInput, ReleaseRecord, TokenType,
    UnacquiredDeployment,
};

use super::{auth, AppState};

// --- Request / Response types ---

#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct AcquireRequest {
    pub session: String,
    /// Missing on protocol1 actors. Advertise the reader ceiling before taking ownership.
    #[serde(default)]
    pub max_supported_deployment_protocol_version: Option<u32>,
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
    pub deployment_model: DeploymentModel,
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
    /// Bounded reasons for explicitly requested deployments that were not acquired.
    /// Empty for discovery-style batch acquisition.
    #[serde(default)]
    #[cfg_attr(feature = "openapi", schema(required = false))]
    pub not_acquired: Vec<UnacquiredDeployment>,
}

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct AcquiredDeploymentResponse {
    pub deployment: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution_claim: Option<crate::traits::deployment_store::ExecutionClaim>,
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
    #[serde(default, rename = "resourceHeartbeats")]
    pub heartbeats: Vec<ResourceHeartbeat>,
    #[serde(default, rename = "observedInventoryBatches")]
    pub observed_inventory_batches: Vec<ObservedInventoryBatch>,
    #[serde(default)]
    pub capabilities: Vec<OperatorCapabilityReport>,
    #[serde(default, rename = "operatorVersion")]
    pub operator_version: Option<String>,
    #[serde(default)]
    pub execution_claim: Option<crate::traits::deployment_store::ExecutionClaim>,
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
    #[serde(default)]
    pub execution_claim: Option<crate::traits::deployment_store::ExecutionClaim>,
}

#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct RenewRequest {
    pub deployment_id: String,
    pub session: String,
    #[serde(default)]
    pub execution_claim: Option<crate::traits::deployment_store::ExecutionClaim>,
}

#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "openapi", schema(as = AgentSyncRequestBase))]
#[serde(rename_all = "camelCase")]
pub struct AgentSyncRequest {
    pub deployment_id: String,
    #[serde(default)]
    pub session: String,
    #[serde(default)]
    pub supports_execution_claims: bool,
    /// Absent for Operators that predate the pod security profile settings.
    #[serde(default)]
    pub supports_pod_security: bool,
    #[serde(default)]
    pub execution_claim: Option<crate::traits::deployment_store::ExecutionClaim>,
    /// Current deployment state as reported by the agent.
    /// When present, the manager updates the deployment record to reflect
    /// the agent's progress (status, stack_state, etc.).
    #[serde(default)]
    pub current_state: Option<serde_json::Value>,
    /// Managed resource status samples emitted by pull-mode deployment steps.
    #[serde(default, rename = "resourceHeartbeats")]
    pub heartbeats: Vec<ResourceHeartbeat>,
    #[serde(default, rename = "observedInventoryBatches")]
    pub observed_inventory_batches: Vec<ObservedInventoryBatch>,
    #[serde(default)]
    pub capabilities: Vec<OperatorCapabilityReport>,
    #[serde(default, rename = "operatorVersion")]
    pub operator_version: Option<String>,
    /// The Operator's self-reported operations-plugin catalog and loaded
    /// bundle hash. Opaque to OSS beyond forwarding it to `reconcile()`.
    #[serde(default)]
    pub operations_report: Option<OperationsReport>,
}

// Adds optional receipts without expanding the public [`AgentSyncRequest`]
// struct literal. The API documents it as `AgentSyncRequest`, the body
// clients send.
/// Body of `POST /v1/sync`.
#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "openapi", schema(as = AgentSyncRequest))]
#[serde(rename_all = "camelCase")]
struct AgentSyncWireRequest {
    #[serde(flatten)]
    #[cfg_attr(feature = "openapi", schema(inline))]
    request: AgentSyncRequest,
    /// Missing on actors that only understand protocol1.
    #[serde(default)]
    max_supported_deployment_protocol_version: Option<u32>,
    /// Exact immutable Operator image identity reported by the running process.
    #[serde(default)]
    operator_image: Option<OperatorImageReport>,
    /// Application release the Operator observed in its environment.
    /// Opaque to OSS beyond forwarding it to `reconcile_request()`.
    #[serde(default)]
    application: Option<ObservedApplicationReport>,
    /// Absent for older Operators. This report has no secret values.
    #[serde(default)]
    dynamic_containers: Option<Vec<alien_core::sync::DynamicContainerReport>>,
    /// Operations an Operator installed without a release declares, without
    /// setting values. Opaque to OSS beyond forwarding it.
    #[serde(default)]
    operations_config: Option<alien_core::OperationsConfig>,
    /// The Operator runs a settled release but lost its config (it cleared
    /// it in token mode, then enrolled). Answered with the current release.
    #[serde(default)]
    installed_config_missing: bool,
    /// The Operator rewrote the pull Secret its Deployment references from
    /// its own registry pull credential. Sent on every sync while that holds.
    /// The manager keeps the deployment token until it sees this.
    #[serde(default)]
    pull_secret_refreshed: bool,
    /// The setup the installed chart carries. An observation the manager may
    /// act on under its own rules; never a grant.
    #[serde(default)]
    installed_setup: Option<alien_core::sync::InstalledSetup>,
}

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct AgentSyncResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution_claim: Option<crate::traits::deployment_store::ExecutionClaim>,
    /// Authoritative deployment state from the manager.
    ///
    /// Returned when a pull deployment attaches with an empty local state while
    /// the manager already has imported or previously reconciled state.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_state: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<serde_json::Value>,
    /// Public URL for the commands API. Operators and app-owned receivers use
    /// it to lease pending commands instead of the agent's local sync URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commands_url: Option<String>,
    /// Bundles the Operator should download to catch up to the target
    /// enabled-plugin-set hash. Opaque passthrough from the embedder's
    /// `DeploymentStore::reconcile`; `None` in OSS.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_operations_bundle_set: Option<TargetOperationsBundleSet>,
    /// Complete release-independent target set. Older embedders omit it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_dynamic_containers: Option<Vec<alien_core::sync::TargetDynamicContainer>>,
    /// Operator image this manager's charts install. Operators that manage
    /// their own workload update to it. Absent when the manager serves no
    /// charts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_operator_image: Option<String>,
    /// The deployment's newest signed config, on every sync, so a protected
    /// Operator applies new developer values without a new release.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signed_config: Option<alien_core::SignedStatement>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "kebab-case")]
pub enum InitialDesiredRelease {
    /// Start from the project's active release.
    Active,
    /// Register the environment without expressing desired release state.
    None,
}

#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct InitializeRequest {
    pub name: Option<String>,
    /// Stable prefix for resources owned by this deployment.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource_prefix: Option<String>,
    /// Subdomain to request under the generated domain, used by stores that assign one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub public_subdomain: Option<String>,
    pub platform: Option<Platform>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission: Option<String>,
    /// Setup method that is registering this deployment, such as `manual` for
    /// rendered Operator manifests or `helm` for generated Helm installs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub setup_method: Option<String>,
    /// Helm release that installed this deployment, for `helm` setups. Lets
    /// the manager tell the deployment owner the exact `helm uninstall`
    /// command. Older clients omit it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub helm_release: Option<HelmRelease>,
    /// Optional base cloud platform for Kubernetes setup targets such as
    /// EKS/GKE/AKS. The runtime platform remains Kubernetes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_platform: Option<Platform>,
    /// Desired-release selection for a newly registered deployment. This is
    /// creation intent, not a permanent deployment mode: a later update can
    /// assign a desired release to a deployment initialized with `none`.
    pub initial_desired_release: InitialDesiredRelease,
    /// Customer setup item selected from the deployment-group contract.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub setup_item: Option<String>,
    pub stack_settings: Option<alien_core::StackSettings>,
    /// Deployer-provided stack inputs. Embedded platform managers resolve
    /// these before creating the deployment; standalone managers accept the
    /// field so generated setup clients have one stable initialize contract.
    #[serde(default)]
    pub input_values: std::collections::HashMap<String, serde_json::Value>,
}

/// A Helm release, identified by its name and namespace.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct HelmRelease {
    /// Release name, as passed to `helm install`.
    pub name: String,
    /// Kubernetes namespace the release is installed in.
    pub namespace: String,
}

pub const HELM_RELEASE_ERROR_MESSAGE: &str = "helmRelease needs a Helm release name (lowercase letters, digits, '-' and '.', at most 53 characters) and a Kubernetes namespace (lowercase letters, digits and '-', at most 63 characters)";

impl HelmRelease {
    /// Whether the name and namespace are ones Helm and Kubernetes accept.
    /// Valid values contain no characters a shell treats specially, so they
    /// can be shown in a command without quoting.
    pub fn is_valid(&self) -> bool {
        self.name.len() <= 53
            && self.name.split('.').all(is_dns_label)
            && self.namespace.len() <= 63
            && is_dns_label(&self.namespace)
    }
}

/// A Kubernetes DNS-1123 label, without the length limit.
fn is_dns_label(value: &str) -> bool {
    let bytes = value.as_bytes();
    match (bytes.first(), bytes.last()) {
        (Some(first), Some(last)) => {
            first.is_ascii_alphanumeric()
                && last.is_ascii_alphanumeric()
                && bytes
                    .iter()
                    .all(|byte| matches!(byte, b'a'..=b'z' | b'0'..=b'9' | b'-'))
        }
        _ => false,
    }
}

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct InitializeResponse {
    pub deployment_id: String,
    pub deployment_model: DeploymentModel,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
}

// --- Router ---

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/v1/sync/acquire", post(acquire))
        .route("/v1/sync/reconcile", post(reconcile))
        .route("/v1/sync/renew", post(renew))
        .route("/v1/sync/release", post(release))
        .route("/v1/sync", post(agent_sync))
}

/// Renew an acquired deployment lease without writing deployment state.
#[cfg_attr(feature = "openapi", utoipa::path(
    post,
    path = "/v1/sync/renew",
    tag = "sync",
    request_body = RenewRequest,
    responses(
        (status = 200, description = "Deployment lease renewed")
    ),
    security(("bearer" = []))
))]
async fn renew(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<RenewRequest>,
) -> Response {
    let subject = match auth::require_auth(&state, &headers).await {
        Ok(subject) => subject,
        Err(error) => return error.into_response(),
    };
    let deployment = match state
        .deployment_store
        .get_deployment(&subject, &req.deployment_id)
        .await
    {
        Ok(Some(deployment)) => deployment,
        Ok(None) => return ErrorData::not_found_deployment(&req.deployment_id).into_response(),
        Err(error) => return error.into_response(),
    };
    if !state.authz.can_sync_deployment(&subject, &deployment) {
        return ErrorData::forbidden("Access denied").into_response();
    }

    match state
        .deployment_store
        .renew_lease(
            &subject,
            &req.deployment_id,
            &req.session,
            req.execution_claim,
        )
        .await
    {
        Ok(()) => Json(serde_json::json!({ "success": true })).into_response(),
        Err(error) => error.into_response(),
    }
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
    let supported_protocol = req
        .max_supported_deployment_protocol_version
        .unwrap_or(1)
        .min(alien_core::MAX_SUPPORTED_DEPLOYMENT_PROTOCOL_VERSION);
    if supported_protocol == 0 {
        return ErrorData::bad_request("maxSupportedDeploymentProtocolVersion must be positive")
            .into_response();
    }
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
        if deployments.iter().any(|deployment| {
            deployment.required_deployment_protocol_version() > supported_protocol
        }) {
            return ErrorData::bad_request(
                "This deployment requires a newer deployment protocol. Update the deployment CLI or manager before retrying.",
            ).into_response();
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
        max_deployment_protocol_version: Some(supported_protocol),
        statuses: req.statuses,
        platforms: req.platforms,
        setup_method: req.setup_method,
        acquire_mode,
        deployment_model: Some(req.deployment_model),
        limit: Some(req.limit),
        ..DeploymentFilter::default()
    };

    let acquire_result = match state
        .deployment_store
        .acquire_with_reasons(&subject, &req.session, &filter, req.limit)
        .await
    {
        Ok(a) => a,
        Err(e) => return e.into_response(),
    };

    let sources = RunnerConfigSources {
        config: &state.config,
        credential_resolver: state.credential_resolver.as_ref(),
        bindings_provider: &state.bindings_provider,
        target_bindings_providers: &state.target_bindings_providers,
        commands_keys: Some(crate::auth::commands_key::CommandsKeyStores::from_state(
            &state,
        )),
    };
    let mut deployments = Vec::with_capacity(acquire_result.deployments.len());
    for acquired in &acquire_result.deployments {
        let response = acquired_deployment_response(
            state.release_store.as_ref(),
            state.values.as_deref(),
            &sources,
            acquired,
        )
        .await
        .and_then(|mut deployment| {
            if subject.role == crate::auth::Role::OperatorSync {
                deployment.deployment["deploymentToken"] = serde_json::Value::Null;
                if let Some(config) = deployment.deployment.get_mut("deploymentConfig") {
                    remove_legacy_runtime_credentials(config, &state.config.base_url())?;
                }
            }
            Ok(deployment)
        });
        match response {
            Ok(deployment) => deployments.push(deployment),
            Err(error) => {
                // The caller never learns it holds these leases, so give them
                // back instead of blocking every runner until they expire.
                for acquired in &acquire_result.deployments {
                    if let Err(give_back_error) = state
                        .deployment_store
                        .release_unused_acquisition(&subject, acquired, &req.session)
                        .await
                    {
                        tracing::warn!(
                            deployment_id = %acquired.deployment.id,
                            error = %give_back_error,
                            "Failed to give back a lease after the acquire response failed"
                        );
                    }
                }
                return error.into_response();
            }
        }
    }

    Json(AcquireResponse {
        deployments,
        not_acquired: acquire_result.not_acquired,
    })
    .into_response()
}

/// Bounded operators receive endpoints and external monitoring configuration, never
/// a legacy deployment bearer or a manager monitoring bearer to persist.
fn remove_legacy_runtime_credentials(
    config: &mut serde_json::Value,
    audience: &str,
) -> Result<(), AlienError> {
    if config
        .pointer("/computeBackend/clusters")
        .is_some_and(|clusters| {
            !clusters.is_null()
                && clusters
                    .as_object()
                    .is_none_or(|clusters| !clusters.is_empty())
        })
    {
        return Err(ErrorData::forbidden(
            "Bounded operators do not support external compute clusters",
        )
        .into_generic());
    }
    let Some(config) = config.as_object_mut() else {
        return Ok(());
    };
    config.insert("deploymentToken".into(), serde_json::Value::Null);
    config.insert("runtimeIdentityAudience".into(), audience.into());
    let manager_origin = reqwest::Url::parse(audience).ok().map(|url| url.origin());
    if let Some(monitoring) = config
        .get_mut("monitoring")
        .and_then(serde_json::Value::as_object_mut)
    {
        for (endpoint, authorization) in [
            ("logsEndpoint", "logsAuthHeader"),
            ("metricsEndpoint", "metricsAuthHeader"),
        ] {
            let same_origin = monitoring
                .get(endpoint)
                .and_then(serde_json::Value::as_str)
                .and_then(|value| reqwest::Url::parse(value).ok())
                .is_some_and(|url| Some(url.origin()) == manager_origin);
            if same_origin {
                monitoring.insert(authorization.into(), "".into());
            }
        }
    }
    Ok(())
}

/// Keeps the values this manager holds for `deployment` out of a state that a
/// runner or an operator reported, before it is saved.
async fn redact_reported_values(
    state: &AppState,
    deployment: &DeploymentRecord,
    stack_state: Option<&mut alien_core::StackState>,
) -> Result<(), AlienError> {
    crate::values::HeldValues::current(
        state.values.as_deref(),
        &state.config,
        state.release_store.as_ref(),
        deployment,
        stack_state.as_deref(),
    )
    .await
    .and_then(|held| held.redact(stack_state))
    .map_err(|e| e.into_generic())
}

/// An acquired deployment as the runner that acquired it receives it.
///
/// `deploymentConfig` is the config the manager's own loop would run with, so
/// a runner that takes over an operation applies the same desired state. It can
/// carry deployment secrets (the commands token, secret variables): the caller
/// is authorized to run this deployment, and running it writes them into the
/// workloads anyway.
async fn acquired_deployment_response(
    release_store: &dyn crate::traits::ReleaseStore,
    values: Option<&crate::values::ValueStore>,
    sources: &RunnerConfigSources<'_>,
    acquired: &crate::traits::deployment_store::AcquiredDeployment,
) -> Result<AcquiredDeploymentResponse, AlienError> {
    let record = &acquired.deployment;
    let target_release_id = record
        .desired_release_id
        .as_deref()
        .or(record.current_release_id.as_deref());
    let deployment_stack = match target_release_id {
        Some(release_id) => target_release_stack(release_store, record, release_id).await?,
        None => None,
    };
    let management_config = crate::loops::deployment::resolve_deployment_management_config(
        sources.credential_resolver,
        record,
        record.platform,
        ManagementRenderer::Runner,
    )
    .await?;
    let held = crate::values::hydrate_held_values(values, record)
        .await
        .map_err(|e| e.into_generic())?;
    let (held, _) = crate::values::with_generated_inputs(values, deployment_stack.as_ref(), held)
        .await
        .map_err(|e| e.into_generic())?;
    let config = sources
        .deployment_config(&held, deployment_stack.as_ref(), management_config)
        .await
        .context(GenericError {
            message: format!("Failed to build the config for deployment {}", record.id),
        })?;

    // The record's saved state carries markers for held values; the runner's
    // executor compares it with the config above, which carries the values.
    let mut served = record.clone();
    crate::values::HeldValues::of_step(
        values,
        sources.config,
        record,
        &held,
        deployment_stack.as_ref(),
    )
    .await
    .and_then(|step_values| step_values.restore(served.stack_state.as_mut()))
    .map_err(|e| e.into_generic())?;
    let mut deployment =
        serde_json::to_value(&served)
            .into_alien_error()
            .context(GenericError {
                message: format!("Failed to serialize deployment {}", record.id),
            })?;
    let config = serde_json::to_value(&config)
        .into_alien_error()
        .context(GenericError {
            message: format!(
                "Failed to serialize the config for deployment {}",
                record.id
            ),
        })?;
    if let Some(object) = deployment.as_object_mut() {
        object.insert("deploymentConfig".to_string(), config);
    }
    Ok(AcquiredDeploymentResponse {
        deployment,
        execution_claim: acquired.execution_claim.clone(),
    })
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

    if !alien_core::accepts_deployment_protocol_report(
        deployment.required_deployment_protocol_version(),
        deployment_state.protocol_version,
    ) || (deployment.base_platform.unwrap_or(deployment.platform) == alien_core::Platform::Gcp
        && deployment.deployment_protocol_version
            < alien_core::GCP_FEDERATION_DEPLOYMENT_PROTOCOL_VERSION
        && deployment_state.protocol_version != deployment.deployment_protocol_version)
    {
        return ErrorData::bad_request(
            "A deployment report must preserve its registered protocol version. Update the deployment actor or manager before retrying.",
        ).into_response();
    }

    // 1. Run registry access reconciliation (cross-account IAM grants).
    //    This must happen before persisting so the `registry_access_granted`
    //    flag is included in the persisted state (matches ManagerTransport).
    let mut final_state = deployment_state;
    // Only a setup run may turn a recorded decline into a yes. What counts is the
    // stored record, never the report: the caller is outside the deployment, holds
    // the deployment's lock for this session, and the deployment was set up with
    // direct setup authority. The report must be a setup handoff, and its stack
    // state must hold every resource the target release puts behind the gate as
    // delivered, the way an import must deliver them. The lock row does not record
    // how it was acquired, so delivery is what separates a setup run from a runtime
    // lock holder.
    let setup_gate_resources = if !matches!(subject.scope, crate::auth::Scope::Deployment { .. })
        && matches!(
            final_state.status,
            alien_core::DeploymentStatus::InitialSetup
                | alien_core::DeploymentStatus::InitialSetupFailed
        )
        && deployment.locked_by.as_deref() == Some(req.session.as_str())
        && deployment
            .runtime_metadata
            .as_ref()
            .is_some_and(|metadata| {
                metadata.initial_setup_authority == alien_core::InitialSetupAuthority::DirectSetup
            }) {
        let release_id = deployment
            .desired_release_id
            .as_deref()
            .or(deployment.current_release_id.as_deref());
        match release_id {
            Some(release_id) => {
                match target_release_stack(state.release_store.as_ref(), &deployment, release_id)
                    .await
                {
                    Ok(stack) => stack.map(|stack| gate_resources(&stack)),
                    Err(error) => return error.into_response(),
                }
            }
            None => None,
        }
    } else {
        None
    };
    // The stored answer flips only once every gated resource is delivered. Until then
    // the setup run's choice goes back to it in the response alone, so it keeps creating
    // the resources; it is never stored, and every report is checked again from scratch.
    let pending_setup_yes = preserve_recorded_gate_answers(
        &mut final_state,
        &deployment,
        setup_gate_resources.as_ref(),
    );
    let legacy_cli = headers
        .get("user-agent")
        .and_then(|value| value.to_str().ok())
        == Some("alien-cli");
    if legacy_cli {
        preserve_legacy_key_observations(&mut final_state, &deployment);
    }
    // A pending replica still persists below — the state records what was granted — and then
    // answers the step with a retryable error, so the runner holds it until the replica exists.
    let replica_pending = crate::registry_access::reconcile_registry_access_with_proxy()
        .bindings_provider(&state.bindings_provider)
        .target_bindings_providers(&state.target_bindings_providers)
        .deployment_id(&req.deployment_id)
        .project_id(&deployment.project_id)
        .state(&mut final_state)
        .proxy_url(&state.config.base_url())
        .supported_aws_regions(&state.config.supported_aws_regions)
        .protected(is_protected(&deployment))
        .call()
        .await
        .err();

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

    // 1b. Revoke a deleted AWS deployment's access before persisting the state that hands it to
    //     setup teardown. A denied revoke comes back as a failed delete to persist instead.
    let final_state = match crate::registry_access::revoke_before_persisting(
        state.deployment_store.as_ref(),
        &state.bindings_provider,
        &state.target_bindings_providers,
        &req.deployment_id,
        &deployment.project_id,
        final_state,
    )
    .await
    {
        Ok(final_state) => final_state,
        Err(error) => return error.into_response(),
    };

    // 2. Persist the step result (including any registry access changes).
    //    Values this manager holds for the deployment stay out of what is saved.
    let mut saved_state = final_state.clone();
    if let Err(error) =
        redact_reported_values(&state, &deployment, saved_state.stack_state.as_mut()).await
    {
        return error.into_response();
    }
    let generated_slots = match generated_slot_report(&state, &deployment).await {
        Ok(slots) => slots,
        Err(response) => return response,
    };
    let result = match reconcile_agent_report(
        &state,
        &subject,
        ReconcileData {
            deployment_id: req.deployment_id.clone(),
            session: req.session,
            state: saved_state,
            update_heartbeat: req.update_heartbeat,
            suggested_delay_ms: req.suggested_delay_ms,
            heartbeats: req.heartbeats,
            observed_inventory_batches: req.observed_inventory_batches,
            capabilities: req.capabilities,
            operator_version: req.operator_version,
            execution_claim: req.execution_claim,
            // Push-mode reconcile never carries an operations report;
            // that's a pull-mode-only concept reported via `/v1/sync`.
            operations_report: None,
        },
        AgentReportExtras {
            operator_image: None,
            application: None,
            dynamic_containers: None,
            operations_config: None,
            deployment_token_revocable: false,
            generated_slots,
        },
    )
    .await
    {
        Ok(r) => r,
        Err(e) => return e.into_response(),
    };

    if let Err(error) = crate::registry_access::revoke_after_persisting(
        state.deployment_store.as_ref(),
        &state.bindings_provider,
        &state.target_bindings_providers,
        &req.deployment_id,
        &deployment.project_id,
        &final_state,
    )
    .await
    {
        // The state is already persisted, and for a deletion the record may be gone, so the
        // caller cannot repeat this cleanup. Leave the unrevoked grant in the manager's log.
        tracing::error!(
            deployment_id = %req.deployment_id,
            status = ?final_state.status,
            error = %error,
            "Registry access cleanup failed after the deployment state was persisted"
        );
        return error.into_response();
    }

    if let Some(error) = replica_pending
        .filter(|_| crate::registry_access::holds_step_for_missing_replica(&result.record.status))
    {
        return error.into_response();
    }

    // Derive native image host for Lambda/Cloud Run so push clients
    // can set it on their local DeploymentConfig.
    let native_image_host = crate::registry_access::derive_native_image_host(
        &state.bindings_provider,
        &state.target_bindings_providers,
        &final_state.platform,
    )
    .await;

    let mut returned_state = final_state.clone();
    if let Some(metadata) = returned_state.runtime_metadata.as_mut() {
        for input_id in pending_setup_yes {
            metadata.persisted_gate_answers.insert(input_id, true);
        }
    }
    let mut current = match serde_json::to_value(&returned_state) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("Failed to serialize reconciled state: {e}");
            return ErrorData::internal("Failed to serialize reconciled state").into_response();
        }
    };

    if legacy_cli {
        if let Some(stack) = current.get_mut("stackState") {
            super::deployments::remove_legacy_key_observations(stack);
        }
    }

    Json(ReconcileResponse {
        success: true,
        current,
        native_image_host,
    })
    .into_response()
}

/// Old clients cannot carry availability observations through a read/save round trip.
/// Keep the stored observation only for the same provider key and wrapping version;
/// a removed or replaced key must never inherit another key's observation.
fn preserve_legacy_key_observations(state: &mut DeploymentState, stored: &DeploymentRecord) {
    let (Some(incoming), Some(previous)) =
        (state.stack_state.as_mut(), stored.stack_state.as_ref())
    else {
        return;
    };
    for (id, resource) in &mut incoming.resources {
        let Some(key) = resource
            .outputs
            .as_ref()
            .and_then(|outputs| outputs.downcast_ref::<KeyOutputs>())
        else {
            continue;
        };
        let Some(old_key) = previous
            .resources
            .get(id)
            .and_then(|resource| resource.outputs.as_ref())
            .and_then(|outputs| outputs.downcast_ref::<KeyOutputs>())
        else {
            continue;
        };
        if key.available.is_none()
            && key.fingerprint == old_key.fingerprint
            && key.wrapping_key_id == old_key.wrapping_key_id
        {
            let mut key = key.clone();
            key.available = old_key.available;
            resource.outputs = Some(ResourceOutputs::new(key));
        }
    }
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
        .release(
            &subject,
            &req.deployment_id,
            &req.session,
            req.execution_claim,
        )
        .await
    {
        Ok(()) => Json(serde_json::json!({ "success": true })).into_response(),
        Err(e) => e.into_response(),
    }
}

#[cfg(test)]
pub(in crate::routes) mod tests {
    use alien_bindings::providers::{kv::local::LocalKv, storage::local::LocalStorage};
    use alien_commands::{
        dispatchers::NullCommandDispatcher,
        server::{CommandDispatcher, CommandRegistry, CommandServer},
        InMemoryCommandRegistry,
    };
    use alien_core::{
        sync::{
            ObservedApplicationImage, ObservedApplicationReport, ObservedApplicationSource,
            SyncInput, SyncRequest,
        },
        ContainerHeartbeatData, DeploymentConfig, DeploymentState, DeploymentStatus,
        EnvironmentVariable, EnvironmentVariablesSnapshot, ExternalBindings, Platform, ReleaseInfo,
        ResourceHeartbeatData, RuntimeMetadata, Stack, StackSettings, StackState,
        CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
    };
    use alien_error::{AlienError, GenericError};
    use axum::{
        body::{to_bytes, Body},
        http::{Request, StatusCode},
    };
    use chrono::Utc;
    use serde_json::{json, Value};
    use std::{
        collections::HashMap,
        sync::{Arc, Mutex},
    };
    use tower::ServiceExt;

    use crate::{
        config::ManagerConfig,
        providers::{
            local_credentials::LocalCredentialResolver, permissive_auth::PermissiveAuthValidator,
            NullTelemetryBackend, OssAuthz, SingleWorkspaceProjects,
        },
        routes::{
            registry_proxy::{CredentialCache, PullValidationCache, RegistryRoutingTable},
            AppState,
        },
        stores::sqlite::{SqliteDatabase, SqliteReleaseStore, SqliteTokenStore},
        traits::{
            deployment_store::{
                ExecutionClaim, MockDeploymentStore, ReconcileData, ReconcileOutcome,
            },
            CreateReleaseParams, DeploymentRecord, ReleaseStore,
        },
    };

    use super::{
        acquired_deployment_response, agent_needs_initial_state_hydration,
        agent_report_completes_claim, build_target_deployment_config, deployment_needs_target,
        deployment_state_from_record, deployment_target_release_id, management_platform,
        may_deliver_agent_target, preserve_recorded_gate_answers,
        preserve_setup_update_authorization, release_stack_platform,
        should_ignore_agent_state_report, should_return_current_state_for_agent_sync,
        validate_kubernetes_base_platform, AgentSyncRequest, AgentSyncWireRequest, HelmRelease,
        InitialDesiredRelease, InitializeRequest, ReconcileRequest, RunnerConfigSources,
    };

    #[tokio::test]
    async fn completed_receipt_recovery_returns_only_an_acknowledgment() {
        let mut store = crate::traits::deployment_store::MockDeploymentStore::new();
        store
            .expect_acknowledge_completed_execution()
            .times(2)
            .withf(|_, deployment, session, claim| {
                deployment == "dep_test"
                    && session == "session_test"
                    && claim.operation_id == "operation_test"
                    && claim.attempt_id == "attempt_test"
            })
            .returning(|_, _, _, _| Ok(true));
        let request: AgentSyncRequest = serde_json::from_value(json!({
            "deploymentId": "dep_test", "session": "session_test",
            "supportsExecutionClaims": true,
            "executionClaim": { "operationId": "operation_test", "attemptId": "attempt_test" },
            "currentState": { "status": "running" },
        }))
        .expect("valid replay request");
        for error in ["DEPLOYMENT_UPDATE_CLAIM_LOST", "DEPLOYMENT_LEASE_LOST"] {
            let response = super::acknowledge_completed_claim(
                &store,
                &crate::auth::Subject::system(),
                &request,
                error,
            )
            .await
            .expect("acknowledge completed receipt")
            .expect("successful recovery");
            assert_eq!(
                serde_json::to_value(response).expect("serialize acknowledgment"),
                json!({})
            );
        }
    }

    #[tokio::test]
    async fn unrelated_errors_and_uncompleted_claims_are_not_acknowledged() {
        let mut store = crate::traits::deployment_store::MockDeploymentStore::new();
        store
            .expect_acknowledge_completed_execution()
            .times(1)
            .returning(|_, _, _, _| Ok(false));
        let request: AgentSyncRequest = serde_json::from_value(json!({
            "deploymentId": "dep_test", "session": "session_test",
            "executionClaim": { "operationId": "operation_test", "attemptId": "attempt_test" },
        }))
        .expect("valid request");
        assert!(super::acknowledge_completed_claim(
            &store,
            &crate::auth::Subject::system(),
            &request,
            "SYNC_FAILED",
        )
        .await
        .expect("unrelated error")
        .is_none());
        assert!(super::acknowledge_completed_claim(
            &store,
            &crate::auth::Subject::system(),
            &request,
            "DEPLOYMENT_UPDATE_CLAIM_LOST",
        )
        .await
        .expect("live claim is not recovered")
        .is_none());
    }

    /// A pull Operator that syncs while its deployment has an update it holds
    /// no claim for: the control plane did not grant one (the target cannot run
    /// yet, or it failed). Its report must not acknowledge that target, but it
    /// still proves the Operator is connected.
    ///
    /// Returns the response and every reconcile the store received.
    async fn sync_operator_without_claim(record: DeploymentRecord) -> (Value, Vec<ReconcileData>) {
        sync_operator_without_claim_reporting(record, false).await
    }

    async fn sync_operator_without_claim_reporting(
        record: DeploymentRecord,
        installed_config_missing: bool,
    ) -> (Value, Vec<ReconcileData>) {
        sync_operator_with_reconciled_image(record, installed_config_missing, None).await
    }

    /// Like [`sync_operator_without_claim_reporting`], where the store's
    /// reconcile answers with `reconciled_image` as the Operator's target image.
    async fn sync_operator_with_reconciled_image(
        record: DeploymentRecord,
        installed_config_missing: bool,
        reconciled_image: Option<&str>,
    ) -> (Value, Vec<ReconcileData>) {
        let (response, reconciled, _) = sync_operator_full(
            record,
            None,
            None,
            Stack::new("app".to_string()).build(),
            None,
            false,
            SyncReport {
                installed_config_missing,
                reconciled_image: reconciled_image.map(str::to_owned),
                status: None,
            },
        )
        .await;
        (response, reconciled)
    }

    /// Like [`sync_operator_without_claim`], with the control plane answering
    /// every reconcile with `targets` and the manager holding `values`.
    async fn sync_operator_with_targets(
        record: DeploymentRecord,
        targets: Option<Vec<alien_core::sync::TargetDynamicContainer>>,
        values: Option<Arc<crate::values::ValueStore>>,
    ) -> (Value, Vec<ReconcileData>) {
        let (response, reconciled, _) = sync_operator_report(
            record,
            targets,
            values,
            Stack::new("app".to_string()).build(),
        )
        .await;
        (response, reconciled)
    }

    /// Like [`sync_operator_with_targets`], with the current release holding
    /// `stack`; also returns the generated slots each reconcile carried.
    async fn sync_operator_report(
        record: DeploymentRecord,
        targets: Option<Vec<alien_core::sync::TargetDynamicContainer>>,
        values: Option<Arc<crate::values::ValueStore>>,
        stack: Stack,
    ) -> (
        Value,
        Vec<ReconcileData>,
        Vec<Vec<crate::values::ValueMeta>>,
    ) {
        sync_operator_reporting(record, targets, values, stack, None).await
    }

    /// Like [`sync_operator_report`], with the Operator's report carrying
    /// `stack_state`.
    async fn sync_operator_reporting(
        record: DeploymentRecord,
        targets: Option<Vec<alien_core::sync::TargetDynamicContainer>>,
        values: Option<Arc<crate::values::ValueStore>>,
        stack: Stack,
        stack_state: Option<Value>,
    ) -> (
        Value,
        Vec<ReconcileData>,
        Vec<Vec<crate::values::ValueMeta>>,
    ) {
        sync_operator_reporting_for(record, targets, values, stack, stack_state, false).await
    }

    /// With `release_gone`, the deployment's release is one the manager no
    /// longer has.
    async fn sync_operator_reporting_for(
        record: DeploymentRecord,
        targets: Option<Vec<alien_core::sync::TargetDynamicContainer>>,
        values: Option<Arc<crate::values::ValueStore>>,
        stack: Stack,
        stack_state: Option<Value>,
        release_gone: bool,
    ) -> (
        Value,
        Vec<ReconcileData>,
        Vec<Vec<crate::values::ValueMeta>>,
    ) {
        sync_operator_full(
            record,
            targets,
            values,
            stack,
            stack_state,
            release_gone,
            SyncReport::default(),
        )
        .await
    }

    /// What the Operator reports, and the image the control plane answers with.
    #[derive(Default)]
    struct SyncReport {
        installed_config_missing: bool,
        reconciled_image: Option<String>,
        /// The status the Operator reports; `running` when unset. Any other
        /// status reports the release as the target it is applying.
        status: Option<&'static str>,
    }

    async fn sync_operator_full(
        record: DeploymentRecord,
        targets: Option<Vec<alien_core::sync::TargetDynamicContainer>>,
        values: Option<Arc<crate::values::ValueStore>>,
        stack: Stack,
        stack_state: Option<Value>,
        release_gone: bool,
        report: SyncReport,
    ) -> (
        Value,
        Vec<ReconcileData>,
        Vec<Vec<crate::values::ValueMeta>>,
    ) {
        let SyncReport {
            installed_config_missing,
            reconciled_image,
            status,
        } = report;
        let temp = tempfile::tempdir().expect("test directory");
        let db = Arc::new(
            SqliteDatabase::new(&temp.path().join("manager.db").to_string_lossy())
                .await
                .expect("database"),
        );
        let release_store = Arc::new(SqliteReleaseStore::new(db.clone()));
        let current_release = release_store
            .create_release(
                &crate::auth::Subject::system(),
                CreateReleaseParams {
                    project_id: record.project_id.clone(),
                    stacks: HashMap::from([(Platform::Kubernetes, stack)]),
                    git_commit_sha: None,
                    git_commit_ref: None,
                    git_commit_message: None,
                    signed_stacks: Default::default(),
                    setup_templates: Default::default(),
                },
            )
            .await
            .expect("current release");
        let mut record = record;
        record.current_release_id = Some(if release_gone {
            "rel_gone".to_string()
        } else {
            current_release.id.clone()
        });

        let reconciled = Arc::new(Mutex::new(Vec::new()));
        let mut store = MockDeploymentStore::new();
        // A control plane with durable operations, like Platform.
        store.expect_requires_execution_claims().return_const(true);
        let stored = record.clone();
        store
            .expect_get_deployment()
            .returning(move |_, _| Ok(Some(stored.clone())));
        // The control plane hands the claim back: nothing to execute.
        store
            .expect_acquire()
            .times(1)
            .returning(|_, _, _, _| Ok(Vec::new()));
        let received = reconciled.clone();
        let reconciled_record = record.clone();
        let outcome_targets = targets.clone();
        let request_image = reconciled_image.clone();
        // These tests are about the report, not where a settled deployment goes next.
        store
            .expect_retarget_settled_deployment()
            .returning(|_, _, _, _, _| Ok(false));
        store.expect_reconcile().returning(move |_, data| {
            received.lock().expect("reconcile log").push(data);
            Ok(ReconcileOutcome {
                record: reconciled_record.clone(),
                target_operations_bundle_set: None,
                target_dynamic_containers: outcome_targets.clone(),
                target_operator_image: reconciled_image.clone(),
            })
        });
        let generated = Arc::new(Mutex::new(Vec::new()));
        let received = reconciled.clone();
        let received_slots = generated.clone();
        let reconciled_record = record.clone();
        store
            .expect_reconcile_request()
            .returning(move |_, request| {
                received_slots
                    .lock()
                    .expect("generated slot log")
                    .push(request.generated_slots().to_vec());
                let (data, _) = request.into_parts();
                received.lock().expect("reconcile log").push(data);
                Ok(ReconcileOutcome {
                    record: reconciled_record.clone(),
                    target_operations_bundle_set: None,
                    target_dynamic_containers: targets.clone(),
                    target_operator_image: request_image.clone(),
                })
            });

        let mut state = test_app_state(
            temp.path(),
            Arc::new(store),
            release_store,
            Arc::new(LocalCredentialResolver::new(temp.path().to_path_buf())),
            db,
        )
        .await;
        state.values = values;

        // The Operator's report claims the pending target is already running.
        // Accepting it would acknowledge work the Operator never received.
        let mut body = sync_body_with_container_heartbeat(Value::Null);
        body["session"] = json!("operator-session");
        body["supportsExecutionClaims"] = json!(true);
        body["maxSupportedDeploymentProtocolVersion"] =
            json!(alien_core::MAX_SUPPORTED_DEPLOYMENT_PROTOCOL_VERSION);
        body["operatorVersion"] = json!("1.2.3");
        body["currentState"] = json!({
            "platform": "kubernetes",
            "status": "running",
            "currentRelease": { "releaseId": "rel_target", "stack": { "id": "app", "resources": {} } },
            "protocolVersion": CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
        });
        if let Some(status) = status {
            body["currentState"]["status"] = json!(status);
            body["currentState"]["targetRelease"] = body["currentState"]["currentRelease"].clone();
        }
        if installed_config_missing {
            body["installedConfigMissing"] = json!(true);
        }
        if let Some(stack_state) = stack_state {
            body["currentState"]["stackState"] = stack_state;
        }
        let response = super::router()
            .with_state(state)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/sync")
                    .header("authorization", "Bearer operator-token")
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .expect("request"),
            )
            .await
            .expect("sync response");
        assert_eq!(response.status(), StatusCode::OK);
        let response: Value = serde_json::from_slice(
            &to_bytes(response.into_body(), 1024 * 1024)
                .await
                .expect("response body"),
        )
        .expect("response json");
        let reconciled = reconciled.lock().expect("reconcile log").clone();
        let generated = generated.lock().expect("generated slot log").clone();
        (response, reconciled, generated)
    }

    /// The sync routes' state over `deployment_store`, with local command storage.
    pub(in crate::routes) async fn test_app_state(
        directory: &std::path::Path,
        deployment_store: Arc<dyn crate::traits::DeploymentStore>,
        release_store: Arc<SqliteReleaseStore>,
        credential_resolver: Arc<dyn crate::traits::CredentialResolver>,
        db: Arc<SqliteDatabase>,
    ) -> AppState {
        let kv: Arc<dyn alien_bindings::traits::Kv> =
            Arc::new(LocalKv::new(directory.join("kv")).await.expect("kv"));
        let command_storage: Arc<dyn alien_bindings::traits::Storage> = Arc::new(
            LocalStorage::new(directory.join("storage").to_string_lossy().to_string())
                .expect("command storage"),
        );
        let command_dispatcher: Arc<dyn CommandDispatcher> = Arc::new(NullCommandDispatcher);
        let command_registry: Arc<dyn CommandRegistry> =
            Arc::new(InMemoryCommandRegistry::default());
        AppState {
            deployment_store,
            release_store,
            project_store: Arc::new(SingleWorkspaceProjects),
            token_store: Arc::new(SqliteTokenStore::new(db)),
            auth_validator: Arc::new(PermissiveAuthValidator::new()),
            authz: Arc::new(OssAuthz),
            telemetry_backend: Arc::new(NullTelemetryBackend),
            credential_resolver,
            command_server: Arc::new(CommandServer::new(
                kv.clone(),
                command_storage,
                command_dispatcher,
                command_registry,
                "http://localhost:0/v1".to_string(),
                b"test-signing-key".to_vec(),
            )),
            config: Arc::new(ManagerConfig::default()),
            bindings_provider: None,
            target_bindings_providers: HashMap::new(),
            kv,
            http_client: reqwest::Client::new(),
            credential_cache: Arc::new(CredentialCache::new()),
            pull_validation_cache: Arc::new(PullValidationCache::new()),
            registry_routing_table: Arc::new(
                RegistryRoutingTable::new(vec![]).expect("empty routing table"),
            ),
            import_registry: Arc::new(alien_infra::ImporterRegistry::built_in()),
            charts: None,
            release_channels: None,
            signing: None,
            bundle_signing_key: None,
            bundle_sources: None,
            log_buffer: Arc::new(crate::dev::LogBuffer::new()),
            values: None,
            value_access: None,
            verify_limiter: Arc::new(crate::values::VerifyLimiter::default()),
        }
    }

    struct UnresolvableManagementIdentity;

    #[async_trait::async_trait]
    impl crate::traits::CredentialResolver for UnresolvableManagementIdentity {
        async fn resolve(
            &self,
            _: &DeploymentRecord,
        ) -> Result<alien_core::ClientConfig, AlienError> {
            Ok(alien_core::ClientConfig::Test)
        }

        async fn resolve_management_config(
            &self,
            _: Platform,
        ) -> Result<Option<alien_core::ManagementConfig>, AlienError> {
            Err(AlienError::new(GenericError {
                message: "[impersonation.azure] needs AZURE_MANAGEMENT_OIDC_ISSUER".to_string(),
            }))
        }
    }

    /// The Azure management identity a complete `[impersonation.azure]` resolves to.
    fn azure_management_identity() -> alien_core::ManagementConfig {
        alien_core::ManagementConfig::Azure(alien_core::AzureManagementConfig {
            managing_tenant_id: "managing-tenant".to_string(),
            oidc_issuer: "https://issuer.example.com".to_string(),
            oidc_subject: "manager".to_string(),
        })
    }

    struct ResolvedManagementIdentity;

    #[async_trait::async_trait]
    impl crate::traits::CredentialResolver for ResolvedManagementIdentity {
        async fn resolve(
            &self,
            _: &DeploymentRecord,
        ) -> Result<alien_core::ClientConfig, AlienError> {
            Ok(alien_core::ClientConfig::Test)
        }

        async fn resolve_management_config(
            &self,
            _: Platform,
        ) -> Result<Option<alien_core::ManagementConfig>, AlienError> {
            Ok(Some(azure_management_identity()))
        }
    }

    /// What a pull sync did on a manager whose management identity doesn't resolve.
    struct UnresolvableManagementSync {
        status: StatusCode,
        response: Value,
        reconciled: Vec<ReconcileData>,
        released: Vec<Option<ExecutionClaim>>,
    }

    /// An Operator that supports execution claims syncs `record` (whose target is
    /// the release this creates) and the control plane grants it a claim.
    async fn sync_without_management_identity(
        record: DeploymentRecord,
    ) -> UnresolvableManagementSync {
        sync_with_management_resolver(record, Arc::new(UnresolvableManagementIdentity)).await
    }

    async fn sync_with_management_resolver(
        record: DeploymentRecord,
        credential_resolver: Arc<dyn crate::traits::CredentialResolver>,
    ) -> UnresolvableManagementSync {
        sync_with_values(record, credential_resolver, None).await
    }

    async fn sync_with_values(
        mut record: DeploymentRecord,
        credential_resolver: Arc<dyn crate::traits::CredentialResolver>,
        values: Option<Arc<crate::values::ValueStore>>,
    ) -> UnresolvableManagementSync {
        let temp = tempfile::tempdir().expect("test directory");
        let db = Arc::new(
            SqliteDatabase::new(&temp.path().join("manager.db").to_string_lossy())
                .await
                .expect("database"),
        );
        let release_store = Arc::new(SqliteReleaseStore::new(db.clone()));
        let release = release_store
            .create_release(
                &crate::auth::Subject::system(),
                CreateReleaseParams {
                    project_id: record.project_id.clone(),
                    stacks: HashMap::from([(
                        Platform::Kubernetes,
                        Stack::new("app".to_string()).build(),
                    )]),
                    git_commit_sha: None,
                    git_commit_ref: None,
                    git_commit_message: None,
                    signed_stacks: Default::default(),
                    setup_templates: Default::default(),
                },
            )
            .await
            .expect("release");
        record.base_platform = Some(Platform::Azure);
        if record.status == "delete-pending" {
            record.current_release_id = Some(release.id.clone());
        } else {
            record.desired_release_id = Some(release.id.clone());
        }

        let claim = ExecutionClaim {
            operation_id: "operation_test".to_string(),
            attempt_id: "attempt_test".to_string(),
        };
        let reconciled = Arc::new(Mutex::new(Vec::new()));
        let released = Arc::new(Mutex::new(Vec::new()));
        let mut store = MockDeploymentStore::new();
        store.expect_requires_execution_claims().return_const(true);
        let stored = record.clone();
        store
            .expect_get_deployment()
            .returning(move |_, _| Ok(Some(stored.clone())));
        let acquired = record.clone();
        let granted = claim.clone();
        store
            .expect_acquire()
            .times(1)
            .returning(move |_, _, _, _| {
                Ok(vec![crate::traits::deployment_store::AcquiredDeployment {
                    deployment: acquired.clone(),
                    execution_claim: Some(granted.clone()),
                }])
            });
        store.expect_renew_lease().returning(|_, _, _, _| Ok(()));
        let received = reconciled.clone();
        let reconciled_record = record.clone();
        // These tests are about the report, not where a settled deployment goes next.
        store
            .expect_retarget_settled_deployment()
            .returning(|_, _, _, _, _| Ok(false));
        store.expect_reconcile().returning(move |_, data| {
            received.lock().expect("reconcile log").push(data);
            Ok(ReconcileOutcome {
                record: reconciled_record.clone(),
                target_operations_bundle_set: None,
                target_dynamic_containers: None,
                target_operator_image: None,
            })
        });
        let gave_back = released.clone();
        store.expect_release().returning(move |_, _, _, claim| {
            gave_back.lock().expect("release log").push(claim);
            Ok(())
        });
        let mut state = test_app_state(
            temp.path(),
            Arc::new(store),
            release_store,
            credential_resolver,
            db,
        )
        .await;
        state.values = values;

        let body = json!({
            "deploymentId": record.id,
            "session": "operator-session",
            "supportsExecutionClaims": true,
            "maxSupportedDeploymentProtocolVersion":
                alien_core::MAX_SUPPORTED_DEPLOYMENT_PROTOCOL_VERSION,
        });
        let response = super::router()
            .with_state(state)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/sync")
                    .header("authorization", "Bearer operator-token")
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .expect("request"),
            )
            .await
            .expect("sync response");
        let status = response.status();
        let response: Value = serde_json::from_slice(
            &to_bytes(response.into_body(), 1024 * 1024)
                .await
                .expect("response body"),
        )
        .expect("response json");
        let reconciled = reconciled.lock().expect("reconcile log").clone();
        let released = released.lock().expect("release log").clone();
        UnresolvableManagementSync {
            status,
            response,
            reconciled,
            released,
        }
    }

    /// A pull update that can't render without the management identity is saved as failed
    /// with its cause under the claim the sync acquired, and the claim is given back, so
    /// the failure shows on the deployment and the target isn't offered again until a retry.
    #[tokio::test]
    async fn a_pull_update_without_its_management_identity_saves_the_failure() {
        let sync =
            sync_without_management_identity(deployment_record_with_state("update-pending", None))
                .await;

        assert_eq!(
            sync.status,
            StatusCode::INTERNAL_SERVER_ERROR,
            "{}",
            sync.response
        );
        assert!(sync.response.get("target").is_none(), "{}", sync.response);
        assert_eq!(sync.reconciled.len(), 1, "one failure checkpoint");
        let saved = &sync.reconciled[0];
        assert_eq!(saved.state.status, DeploymentStatus::UpdateFailed);
        assert_eq!(saved.session, "operator-session");
        let claim = saved
            .execution_claim
            .as_ref()
            .expect("saved under the claim");
        assert_eq!(claim.operation_id, "operation_test");
        let error = saved.state.error.as_ref().expect("saved cause");
        assert_eq!(
            error.message,
            "Failed to resolve the azure management identity for deployment dep_test"
        );
        assert_eq!(
            error.source.as_ref().expect("resolver error").message,
            "[impersonation.azure] needs AZURE_MANAGEMENT_OIDC_ISSUER"
        );
        assert_eq!(sync.released.len(), 1, "the claim is given back");
        assert_eq!(
            sync.released[0]
                .as_ref()
                .map(|claim| claim.attempt_id.as_str()),
            Some("attempt_test")
        );
    }

    fn pull_record_in_cluster(
        status: &str,
        ownership: alien_core::KubernetesClusterOwnership,
    ) -> DeploymentRecord {
        let mut record = deployment_record_with_state(status, None);
        record.stack_settings = Some(StackSettings {
            deployment_model: alien_core::DeploymentModel::Pull,
            kubernetes: Some(alien_core::KubernetesSettings {
                cluster: Some(alien_core::KubernetesClusterSettings {
                    ownership,
                    namespace: Some("application".to_string()),
                    cloud: None,
                }),
                exposure: None,
            }),
            ..Default::default()
        });
        record
    }

    /// An Operator installed into an existing cluster gets no setup-owned management
    /// resource, so its update renders without the identity the manager can't resolve.
    /// An Operator whose cluster the setup manages still needs it.
    #[tokio::test]
    async fn an_existing_cluster_pull_update_renders_without_the_management_identity() {
        let sync = sync_without_management_identity(pull_record_in_cluster(
            "update-pending",
            alien_core::KubernetesClusterOwnership::Existing,
        ))
        .await;
        assert_eq!(sync.status, StatusCode::OK, "{}", sync.response);
        let target = sync.response.get("target").expect("update target");
        assert!(
            target["config"].get("managementConfig").is_none(),
            "{target}"
        );
        assert!(
            sync.reconciled
                .iter()
                .all(|data| data.state.status == DeploymentStatus::UpdatePending),
            "no failure saved"
        );

        let sync = sync_without_management_identity(pull_record_in_cluster(
            "update-pending",
            alien_core::KubernetesClusterOwnership::Managed,
        ))
        .await;
        assert_eq!(
            sync.status,
            StatusCode::INTERNAL_SERVER_ERROR,
            "{}",
            sync.response
        );
        assert_eq!(sync.reconciled.len(), 1);
        assert_eq!(
            sync.reconciled[0].state.status,
            DeploymentStatus::UpdateFailed
        );
    }

    /// A target delivered to a Running or RefreshFailed Operator carries the management
    /// identity, as it did before the identity became required: the Operator turns the target
    /// into an update (`accept_target_release`), whose RemoteStackManagement mutation renders
    /// the management resource from it, and keeps its config for later refreshes. Without the
    /// identity the sync saves the failure instead of delivering a target that drops it.
    #[tokio::test]
    async fn a_running_pull_target_carries_the_management_identity() {
        for (status, recorded, failed) in [
            (
                "running",
                DeploymentStatus::Running,
                DeploymentStatus::RefreshFailed,
            ),
            (
                "refresh-failed",
                DeploymentStatus::RefreshFailed,
                DeploymentStatus::RefreshFailed,
            ),
        ] {
            let sync = sync_with_management_resolver(
                deployment_record_with_state(status, None),
                Arc::new(ResolvedManagementIdentity),
            )
            .await;
            assert_eq!(sync.status, StatusCode::OK, "{status}: {}", sync.response);
            let target = sync.response.get("target").expect("target");
            assert_eq!(
                target["config"]["managementConfig"],
                serde_json::to_value(azure_management_identity()).unwrap(),
                "{status}: {target}"
            );
            assert!(
                sync.reconciled
                    .iter()
                    .all(|data| data.state.status == recorded && data.state.error.is_none()),
                "{status}: no failure saved"
            );

            let sync =
                sync_without_management_identity(deployment_record_with_state(status, None)).await;
            assert_eq!(
                sync.status,
                StatusCode::INTERNAL_SERVER_ERROR,
                "{status}: {}",
                sync.response
            );
            assert!(sync.response.get("target").is_none(), "{}", sync.response);
            assert_eq!(sync.reconciled.len(), 1, "{status}: one failure checkpoint");
            let saved = &sync.reconciled[0].state;
            assert_eq!(saved.status, failed, "{status}");
            assert_eq!(
                saved.error.as_ref().expect("saved cause").message,
                "Failed to resolve the azure management identity for deployment dep_test"
            );
            assert_eq!(sync.released.len(), 1, "{status}: the claim is given back");
        }
    }

    /// Deleting a pull deployment never renders setup, so the Operator gets its delete
    /// target without the management identity.
    #[tokio::test]
    async fn a_pull_deletion_needs_no_management_identity() {
        let sync =
            sync_without_management_identity(deployment_record_with_state("delete-pending", None))
                .await;

        assert_eq!(sync.status, StatusCode::OK, "{}", sync.response);
        let target = sync.response.get("target").expect("delete target");
        assert!(
            target["config"].get("managementConfig").is_none(),
            "{target}"
        );
        assert!(sync
            .reconciled
            .iter()
            .all(|data| data.state.status == DeploymentStatus::DeletePending));
    }

    /// Assert the one reconcile recorded the Operator as connected, with its
    /// observations, against the stored state rather than its report.
    fn assert_heartbeat_against_stored_state(
        response: &Value,
        reconciled: &[ReconcileData],
        expected_status: DeploymentStatus,
    ) -> DeploymentState {
        assert!(
            response.get("target").is_none(),
            "no target without a claim: {response}"
        );
        assert!(response.get("executionClaim").is_none(), "{response}");
        assert_eq!(reconciled.len(), 1, "one heartbeat reconcile per sync");
        let data = &reconciled[0];
        assert!(data.update_heartbeat);
        assert!(data.execution_claim.is_none());
        assert_eq!(data.operator_version.as_deref(), Some("1.2.3"));
        assert_eq!(data.heartbeats.len(), 1);
        assert_eq!(data.heartbeats[0].resource_id, "api");
        assert_eq!(data.state.status, expected_status);
        let current = data
            .state
            .current_release
            .as_ref()
            .and_then(|release| release.release_id.as_deref());
        assert!(
            current.is_some_and(|id| id != "rel_target"),
            "the stored current release, not the unaccepted target: {current:?}"
        );
        assert!(data.state.target_release.is_none());
        data.state.clone()
    }

    #[tokio::test]
    async fn operator_heartbeat_is_recorded_while_an_update_waits_for_a_claim() {
        let mut record = deployment_record_with_state("running", None);
        record.desired_release_id = Some("rel_target".to_string());

        let (response, reconciled) = sync_operator_without_claim(record).await;

        let state = assert_heartbeat_against_stored_state(
            &response,
            &reconciled,
            DeploymentStatus::Running,
        );
        assert!(state.error.is_none());
    }

    const TOKEN_ENROLLMENT_ADMIN: &str = "ax_admin_for_enrollment";
    const TOKEN_ENROLLMENT_DEPLOYMENT_TOKEN: &str = "ax_deploy_before_enrollment";
    const TOKEN_ENROLLMENT_GROUP_TOKEN: &str = "ax_group_for_enrollment";

    /// A manager with the real router, the real standalone identity store and
    /// two real tokens: a deployment token for `dep_test`, and an admin token.
    struct TokenEnrollment {
        _temp: tempfile::TempDir,
        db: Arc<SqliteDatabase>,
        state: AppState,
        verifier: Arc<dyn crate::auth::identity_exchange::RuntimeIdentityVerifier>,
        credentials: Arc<crate::auth::runtime_capability::RuntimeCredentials>,
        identity: Arc<crate::stores::sqlite::SqliteRuntimeIdentityStore>,
        stored: Arc<Mutex<DeploymentRecord>>,
        /// The `deployment_token_revocable` value of every sync that reached
        /// the store as a reconcile request, in order.
        revocable_reports: Arc<Mutex<Vec<bool>>>,
    }

    impl TokenEnrollment {
        async fn new() -> Self {
            use crate::auth::runtime_capability::{RuntimeAuthValidator, RuntimeCredentials};
            use crate::auth::standalone_identity::StandaloneRuntimeIdentity;
            use crate::providers::token_db_validator::TokenDbValidator;
            use crate::stores::sqlite::SqliteRuntimeIdentityStore;
            use crate::traits::token_store::{CreateTokenParams, TokenStore, TokenType};

            let temp = tempfile::tempdir().expect("test directory");
            let db = Arc::new(
                SqliteDatabase::new(&temp.path().join("manager.db").to_string_lossy())
                    .await
                    .expect("database"),
            );
            let tokens = Arc::new(SqliteTokenStore::new(db.clone()));
            for (token_type, bearer, deployment_id) in [
                (
                    TokenType::Deployment,
                    TOKEN_ENROLLMENT_DEPLOYMENT_TOKEN,
                    Some("dep_test".to_string()),
                ),
                (TokenType::Admin, TOKEN_ENROLLMENT_ADMIN, None),
            ] {
                tokens
                    .create_token(CreateTokenParams {
                        token_type,
                        key_prefix: bearer[..12].to_string(),
                        key_hash: crate::ids::sha256_hash(bearer),
                        deployment_group_id: deployment_id.as_ref().map(|_| "dg_test".into()),
                        deployment_id,
                    })
                    .await
                    .expect("token");
            }

            tokens
                .create_token(CreateTokenParams {
                    token_type: TokenType::DeploymentGroup,
                    key_prefix: TOKEN_ENROLLMENT_GROUP_TOKEN[..12].to_string(),
                    key_hash: crate::ids::sha256_hash(TOKEN_ENROLLMENT_GROUP_TOKEN),
                    deployment_group_id: Some("dg_test".into()),
                    deployment_id: None,
                })
                .await
                .expect("group token");

            let stored = Arc::new(Mutex::new(deployment_record_with_state("running", None)));
            let revocable_reports = Arc::new(Mutex::new(Vec::new()));
            let mut store = MockDeploymentStore::new();
            store.expect_requires_execution_claims().return_const(false);
            let current = stored.clone();
            // Any deployment of the group answers with the same record under
            // the id asked for, so siblings can be told apart by key.
            store.expect_get_deployment().returning(move |_, id| {
                let mut record = current.lock().unwrap().clone();
                record.id = id.to_string();
                Ok(Some(record))
            });
            let apply = |current: &Mutex<DeploymentRecord>, data: ReconcileData| {
                let mut record = current.lock().unwrap().clone();
                record.status = serde_json::to_value(data.state.status)
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .to_owned();
                *current.lock().unwrap() = record.clone();
                Ok(ReconcileOutcome {
                    record,
                    target_operations_bundle_set: None,
                    target_dynamic_containers: None,
                    target_operator_image: None,
                })
            };
            let current = stored.clone();
            store
                .expect_reconcile()
                .returning(move |_, data| apply(&current, data));
            // A sync that carries a receipt reaches the store as a request.
            let current = stored.clone();
            let received = revocable_reports.clone();
            store
                .expect_reconcile_request()
                .returning(move |_, request| {
                    received
                        .lock()
                        .unwrap()
                        .push(request.deployment_token_revocable());
                    let (data, _) = request.into_parts();
                    apply(&current, data)
                });
            let store: Arc<dyn crate::traits::DeploymentStore> = Arc::new(store);
            let resolver = Arc::new(LocalCredentialResolver::new(temp.path().to_path_buf()));
            let identity = Arc::new(SqliteRuntimeIdentityStore::new(db.clone()));
            let verifier: Arc<dyn crate::auth::identity_exchange::RuntimeIdentityVerifier> =
                Arc::new(
                    StandaloneRuntimeIdentity::new(
                        identity.clone(),
                        store.clone(),
                        resolver.clone(),
                    )
                    .unwrap(),
                );
            let mut state = test_app_state(
                temp.path(),
                store,
                Arc::new(SqliteReleaseStore::new(db.clone())),
                resolver,
                db.clone(),
            )
            .await;
            let credentials =
                Arc::new(RuntimeCredentials::new("https://manager.test".into(), [9; 32]).unwrap());
            state.config = Arc::new(crate::config::ManagerConfig {
                response_signing_key: b"token-enrollment-signing-key".to_vec(),
                ..(*state.config).clone()
            });
            state.token_store = tokens.clone();
            state.auth_validator = Arc::new(RuntimeAuthValidator::new(
                credentials.clone(),
                Arc::new(TokenDbValidator::new(tokens)),
            ));
            Self {
                _temp: temp,
                db,
                state,
                verifier,
                credentials,
                identity,
                stored,
                revocable_reports,
            }
        }

        fn identity_router(&self) -> axum::Router {
            crate::auth::identity_exchange::router(self.credentials.clone(), self.verifier.clone())
                .with_state(self.state.clone())
        }

        async fn post(
            router: axum::Router,
            uri: &str,
            bearer: Option<&str>,
            body: Value,
        ) -> (StatusCode, Value) {
            let mut request = Request::builder()
                .method("POST")
                .uri(uri)
                .header("content-type", "application/json");
            if let Some(bearer) = bearer {
                request = request.header("authorization", format!("Bearer {bearer}"));
            }
            let response = router
                .oneshot(request.body(Body::from(body.to_string())).expect("request"))
                .await
                .expect("response");
            let status = response.status();
            let body = to_bytes(response.into_body(), 1 << 20).await.unwrap();
            (
                status,
                serde_json::from_slice::<Value>(&body).unwrap_or(Value::Null),
            )
        }

        /// `POST /v1/deployments/dep_test/enrollment-code` with `bearer`.
        async fn enrollment_code(&self, bearer: &str) -> (StatusCode, Value) {
            self.enrollment_code_for("dep_test", bearer).await
        }

        async fn enrollment_code_for(
            &self,
            deployment_id: &str,
            bearer: &str,
        ) -> (StatusCode, Value) {
            Self::post(
                self.identity_router(),
                &format!("/v1/deployments/{deployment_id}/enrollment-code"),
                Some(bearer),
                Value::Null,
            )
            .await
        }

        /// `POST /v1/identity/enroll`: the operator enrolls `key` with `code`.
        async fn enroll(&self, code: &str, key: &str) -> StatusCode {
            Self::post(
                self.identity_router(),
                "/v1/identity/enroll",
                None,
                json!({ "code": code, "initialKeyHash": crate::ids::sha256_hash(key) }),
            )
            .await
            .0
        }

        async fn mint(&self, bearer: &str) -> String {
            let (status, issued) = self.enrollment_code(bearer).await;
            assert_eq!(status, StatusCode::OK, "{issued}");
            issued["code"].as_str().expect("code").to_owned()
        }

        async fn key_is_current(&self, key: &str) -> bool {
            self.key_is_current_for("dep_test", key).await
        }

        async fn key_is_current_for(&self, deployment_id: &str, key: &str) -> bool {
            self.identity
                .verify_key(deployment_id, key, Utc::now())
                .await
                .unwrap()
                .is_some()
        }

        /// Warn-level entries the manager shows for the deployment.
        async fn refusals(&self) -> Vec<crate::dev::LogEntry> {
            self.state
                .log_buffer
                .get_entries(Some("dep_test"), usize::MAX)
                .await
                .into_iter()
                .filter(|entry| {
                    entry.severity == "WARN"
                        && entry
                            .attributes
                            .contains(&("event".to_owned(), "enrollment-code-refused".to_owned()))
                })
                .collect()
        }

        /// A capability for the enrolled operator, as the exchange issues it.
        fn operator_capability(&self) -> String {
            use crate::auth::runtime_capability::{RuntimeGrant, RuntimePurpose};
            self.credentials
                .issue(
                    RuntimeGrant {
                        workspace_id: "default".into(),
                        project_id: "default".into(),
                        deployment_id: "dep_test".into(),
                        component_id: "operator".into(),
                        purpose: RuntimePurpose::OperatorSync,
                    },
                    Utc::now() + chrono::Duration::hours(1),
                )
                .unwrap()
                .token
        }

        /// Make a release whose command-enabled Containers are `(id, marked)`
        /// (`marked`: built with a receiver that reads the capability file) the
        /// deployment's current release.
        async fn run_release_with_receivers(&self, receivers: &[(&str, bool)]) {
            let release_id = self.create_release_with_receivers(receivers).await;
            self.stored.lock().unwrap().current_release_id = Some(release_id);
        }

        /// A release whose command-enabled Containers are `(id, marked)`.
        async fn create_release_with_receivers(&self, receivers: &[(&str, bool)]) -> String {
            let mut stack = Stack::new("app".to_string());
            for (id, marked) in receivers {
                let container = alien_core::Container::new((*id).to_string())
                    .code(alien_core::ContainerCode::Image {
                        image: "app:latest".to_string(),
                    })
                    .cpu(alien_core::ResourceSpec {
                        min: "0.5".to_string(),
                        desired: "1".to_string(),
                    })
                    .memory(alien_core::ResourceSpec {
                        min: "512Mi".to_string(),
                        desired: "1Gi".to_string(),
                    })
                    .port(8080)
                    .permissions("execution".to_string())
                    .commands_enabled(true)
                    .commands_capability_file(*marked)
                    .build();
                stack = stack.add(container, alien_core::ResourceLifecycle::Live);
            }
            let release = self
                .state
                .release_store
                .create_release(
                    &crate::auth::Subject::system(),
                    CreateReleaseParams {
                        project_id: "default".to_string(),
                        stacks: HashMap::from([(Platform::Kubernetes, stack.build())]),
                        git_commit_sha: None,
                        git_commit_ref: None,
                        git_commit_message: None,
                        setup_templates: Default::default(),
                        signed_stacks: Default::default(),
                    },
                )
                .await
                .expect("release");
            release.id
        }

        /// Issue the deployment's commands key the way a pull target does.
        async fn issue_commands_key(&self) -> String {
            crate::auth::commands_key::ensure(&self.state, "dep_test", &["old-api".to_string()])
                .await
                .expect("commands key")
        }

        async fn commands_key_is_accepted(&self) -> bool {
            crate::auth::commands_key::exists(&self.state, "dep_test")
                .await
                .unwrap()
        }

        /// A release with no stack for the deployment's platform.
        async fn create_stackless_release(&self) -> String {
            self.state
                .release_store
                .create_release(
                    &crate::auth::Subject::system(),
                    CreateReleaseParams {
                        project_id: "default".to_string(),
                        stacks: HashMap::new(),
                        git_commit_sha: None,
                        git_commit_ref: None,
                        git_commit_message: None,
                        setup_templates: Default::default(),
                        signed_stacks: Default::default(),
                    },
                )
                .await
                .expect("release")
                .id
        }

        /// `POST /v1/sync` as `bearer`, reporting the deployment running.
        async fn sync(&self, bearer: &str) -> StatusCode {
            self.sync_reporting(bearer, false).await
        }

        /// The same, with the Operator reporting that its pull Secret holds
        /// its own credential.
        async fn sync_with_refreshed_pull_secret(&self, bearer: &str) -> StatusCode {
            self.sync_reporting(bearer, true).await
        }

        async fn sync_reporting(&self, bearer: &str, pull_secret_refreshed: bool) -> StatusCode {
            self.sync_reporting_release(bearer, pull_secret_refreshed, None)
                .await
        }

        /// The same, with the Operator naming the release it runs.
        async fn sync_reporting_release(
            &self,
            bearer: &str,
            pull_secret_refreshed: bool,
            running_release_id: Option<&str>,
        ) -> StatusCode {
            let mut body = json!({
                "deploymentId": "dep_test",
                "maxSupportedDeploymentProtocolVersion":
                    alien_core::MAX_SUPPORTED_DEPLOYMENT_PROTOCOL_VERSION,
                "currentState": {
                    "platform": "kubernetes",
                    "status": "running",
                    "protocolVersion": CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
                },
            });
            if pull_secret_refreshed {
                body["pullSecretRefreshed"] = json!(true);
            }
            if let Some(release_id) = running_release_id {
                body["currentState"]["currentRelease"] = json!({
                    "releaseId": release_id,
                    "stack": Stack::new("app".to_string()).build(),
                });
            }
            let response = super::router()
                .with_state(self.state.clone())
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/v1/sync")
                        .header("authorization", format!("Bearer {bearer}"))
                        .header("content-type", "application/json")
                        .body(Body::from(body.to_string()))
                        .expect("request"),
                )
                .await
                .expect("sync response");
            let status = response.status();
            if status == StatusCode::BAD_REQUEST {
                let body = to_bytes(response.into_body(), 1 << 20).await.unwrap();
                panic!("{}", String::from_utf8_lossy(&body));
            }
            status
        }
    }

    /// An Operator on a deployment token (a self-updated old install) asks
    /// for an enrollment code with that token and enrolls with it. The token
    /// keeps working until the first rollout the enrolled Operator finishes.
    /// Then every workload uses capability files and it is revoked, and it
    /// can no longer get a code.
    #[tokio::test]
    async fn the_deployment_token_is_revoked_after_the_first_enrolled_rollout() {
        let manager = TokenEnrollment::new().await;
        let old_token = TOKEN_ENROLLMENT_DEPLOYMENT_TOKEN;

        // The old token gets a one-use code for its own deployment, and the
        // code enrolls the Operator's key for that deployment.
        let (status, issued) = manager.enrollment_code(old_token).await;
        assert_eq!(status, StatusCode::OK, "{issued}");
        assert_eq!(issued["deploymentId"], "dep_test");
        assert_eq!(issued["managerAudience"], "https://manager.test");
        assert_eq!(
            manager
                .enroll(issued["code"].as_str().expect("code"), "ax_day_v1_operator")
                .await,
            StatusCode::OK
        );
        assert!(manager.key_is_current("ax_day_v1_operator").await);
        let operator = manager.operator_capability();

        // Enrolled, but the workloads still run the revision from before.
        // Health refreshes do not end the token.
        assert_eq!(manager.sync(&operator).await, StatusCode::OK);
        manager.stored.lock().unwrap().status = "refresh-failed".into();
        assert_eq!(manager.sync(&operator).await, StatusCode::OK);
        assert_eq!(manager.sync(old_token).await, StatusCode::OK);

        // The enrolled Operator finishes a rollout and reports that its pull
        // Secret no longer holds the token.
        manager.stored.lock().unwrap().status = "update-pending".into();
        assert_eq!(
            manager.sync_with_refreshed_pull_secret(&operator).await,
            StatusCode::OK
        );
        assert_eq!(manager.sync(old_token).await, StatusCode::UNAUTHORIZED);
        assert_eq!(manager.sync(&operator).await, StatusCode::OK);
        assert_eq!(
            manager.enrollment_code(old_token).await.0,
            StatusCode::UNAUTHORIZED
        );
    }

    /// Once the real Operator has enrolled, a copy of the deployment token
    /// cannot get a code that replaces its key. The refusal shows in the
    /// deployment's logs. An administrator's code still re-enrolls.
    #[tokio::test]
    async fn a_deployment_token_cannot_mint_a_code_once_the_operator_has_a_key() {
        let manager = TokenEnrollment::new().await;
        let token = TOKEN_ENROLLMENT_DEPLOYMENT_TOKEN;

        let code = manager.mint(token).await;
        assert_eq!(
            manager.enroll(&code, "ax_day_v1_real").await,
            StatusCode::OK
        );
        assert!(manager.refusals().await.is_empty());

        let (status, refused) = manager.enrollment_code(token).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{refused}");
        assert_eq!(refused["code"], "ENROLLMENT_KEY_EXISTS", "{refused}");
        assert!(
            refused["message"]
                .as_str()
                .is_some_and(|message| message.contains("administrator")),
            "{refused}"
        );
        assert!(manager.key_is_current("ax_day_v1_real").await);
        let refusals = manager.refusals().await;
        assert_eq!(refusals.len(), 1, "{refusals:?}");
        assert!(refusals[0]
            .attributes
            .contains(&("stage".to_owned(), "issue".to_owned())));

        // The administrator can always enroll the Operator again.
        let admin_code = manager.mint(TOKEN_ENROLLMENT_ADMIN).await;
        assert_eq!(
            manager.enroll(&admin_code, "ax_day_v1_replacement").await,
            StatusCode::OK
        );
        assert!(manager.key_is_current("ax_day_v1_replacement").await);
        assert!(!manager.key_is_current("ax_day_v1_real").await);
        assert_eq!(
            manager.refusals().await.len(),
            1,
            "an admin code is not a refusal"
        );
    }

    /// A setup-link token covers every deployment of its group. Once a
    /// deployment's operator holds a key it cannot be used to get a code that
    /// replaces that key; before that it can, and an administrator always can.
    #[tokio::test]
    async fn a_group_token_cannot_mint_a_code_once_the_operator_has_a_key() {
        let manager = TokenEnrollment::new().await;
        let group = TOKEN_ENROLLMENT_GROUP_TOKEN;
        let sibling = "dep_sibling";

        // Before the sibling enrolled, the group token gets a code.
        let (status, issued) = manager.enrollment_code_for(sibling, group).await;
        assert_eq!(status, StatusCode::OK, "{issued}");
        assert_eq!(
            manager
                .enroll(issued["code"].as_str().expect("code"), "ax_day_v1_sibling")
                .await,
            StatusCode::OK
        );
        assert!(
            manager
                .key_is_current_for(sibling, "ax_day_v1_sibling")
                .await
        );

        // After it enrolled, the group token is refused and the key is kept.
        let (status, refused) = manager.enrollment_code_for(sibling, group).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{refused}");
        assert_eq!(refused["code"], "ENROLLMENT_KEY_EXISTS", "{refused}");
        assert!(refused.get("deploymentCode").is_none());
        assert!(
            manager
                .key_is_current_for(sibling, "ax_day_v1_sibling")
                .await
        );

        // An administrator can still enroll the operator again.
        let (status, issued) = manager
            .enrollment_code_for(sibling, TOKEN_ENROLLMENT_ADMIN)
            .await;
        assert_eq!(status, StatusCode::OK, "{issued}");
    }

    /// Sets when the deployment's operator key expires. Keys last 24 hours, so
    /// this also sets when it was last rotated.
    async fn set_key_expiry(manager: &TokenEnrollment, deployment_id: &str, expires_at: i64) {
        let conn = manager.db.conn().lock().await;
        conn.execute(
            "UPDATE runtime_daily_keys SET expires_at = ?1 WHERE deployment_id = ?2",
            (expires_at, deployment_id.to_owned()),
        )
        .await
        .unwrap();
    }

    /// A group credential can replace an operator key only when the key has
    /// not been rotated within the live window, and never when the key time
    /// cannot be read.
    #[tokio::test]
    async fn a_group_token_can_replace_only_a_key_that_is_not_live() {
        let manager = TokenEnrollment::new().await;
        let group = TOKEN_ENROLLMENT_GROUP_TOKEN;
        let sibling = "dep_sibling";
        let code = manager.mint(TOKEN_ENROLLMENT_ADMIN).await;
        assert_eq!(manager.enroll(&code, "ax_day_v1_old").await, StatusCode::OK);

        // Rotated within the window, even just before it ends: refused.
        let now = Utc::now().timestamp();
        let day = 24 * 3600;
        for hours_ago in [0, 24] {
            set_key_expiry(&manager, "dep_test", now + day - hours_ago * 3600).await;
            let (status, refused) = manager.enrollment_code(group).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{hours_ago}h: {refused}");
            assert_eq!(refused["code"], "ENROLLMENT_KEY_EXISTS");
        }

        // An unreadable key time counts as live.
        set_key_expiry(&manager, "dep_test", i64::MAX).await;
        let (status, refused) = manager.enrollment_code(group).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{refused}");

        // Rotated more than 25 hours ago: the group credential gets a code
        // that replaces the key.
        set_key_expiry(&manager, "dep_test", now + day - 26 * 3600).await;
        let (status, issued) = manager.enrollment_code(group).await;
        assert_eq!(status, StatusCode::OK, "{issued}");
        assert_eq!(
            manager
                .enroll(issued["code"].as_str().expect("code"), "ax_day_v1_new")
                .await,
            StatusCode::OK
        );
        assert!(
            manager
                .key_is_current_for("dep_test", "ax_day_v1_new")
                .await
        );
        assert!(
            !manager
                .key_is_current_for("dep_test", "ax_day_v1_old")
                .await
        );

        // The sibling in the group never enrolled, and the group credential
        // still gets its first code.
        let (status, issued) = manager.enrollment_code_for(sibling, group).await;
        assert_eq!(status, StatusCode::OK, "{issued}");
    }

    /// An administrator gets a code whatever the key's age.
    #[tokio::test]
    async fn an_admin_token_gets_a_code_while_the_key_is_live() {
        let manager = TokenEnrollment::new().await;
        let code = manager.mint(TOKEN_ENROLLMENT_ADMIN).await;
        assert_eq!(
            manager.enroll(&code, "ax_day_v1_live").await,
            StatusCode::OK
        );
        let (status, issued) = manager.enrollment_code(TOKEN_ENROLLMENT_ADMIN).await;
        assert_eq!(status, StatusCode::OK, "{issued}");
    }

    /// A token-made code that is still unused when the deployment has a key
    /// must not replace the key. (A new enrollment normally deletes the
    /// deployment's other unused codes; this is the state that must stay
    /// harmless if one is left over.) The real key keeps working, and the
    /// refusal shows in the deployment's logs.
    #[tokio::test]
    async fn a_token_made_code_left_over_from_before_enrollment_cannot_replace_the_key() {
        let manager = TokenEnrollment::new().await;
        let code = manager.mint(TOKEN_ENROLLMENT_DEPLOYMENT_TOKEN).await;
        assert_eq!(
            manager.enroll(&code, "ax_day_v1_real").await,
            StatusCode::OK
        );

        let (leftover, _, hash) = crate::ids::generate_token("ax_enroll_");
        manager
            .db
            .conn()
            .lock()
            .await
            .execute(
                "INSERT INTO runtime_enrollment_codes \
                 (code_hash, deployment_id, expires_at, from_token) VALUES (?1, 'dep_test', ?2, 1)",
                (hash, (Utc::now() + chrono::Duration::hours(1)).timestamp()),
            )
            .await
            .expect("leftover token-made code");

        assert_eq!(
            manager.enroll(&leftover, "ax_day_v1_copier").await,
            StatusCode::UNAUTHORIZED
        );
        assert!(manager.key_is_current("ax_day_v1_real").await);
        assert!(!manager.key_is_current("ax_day_v1_copier").await);
        let refusals = manager.refusals().await;
        assert_eq!(refusals.len(), 1, "{refusals:?}");
        assert!(refusals[0]
            .attributes
            .contains(&("stage".to_owned(), "redeem".to_owned())));
        // The operator on its real key still syncs.
        assert_eq!(
            manager.sync(&manager.operator_capability()).await,
            StatusCode::OK
        );
    }

    /// The rollout that revokes the deployment token also retires the
    /// deployment's unused token-made codes, but not an admin's code.
    #[tokio::test]
    async fn revoking_the_deployment_token_retires_its_unused_codes() {
        let manager = TokenEnrollment::new().await;
        let token_code = manager.mint(TOKEN_ENROLLMENT_DEPLOYMENT_TOKEN).await;
        let admin_code = manager.mint(TOKEN_ENROLLMENT_ADMIN).await;
        let operator = manager.operator_capability();

        manager.stored.lock().unwrap().status = "update-pending".into();
        assert_eq!(
            manager.sync_with_refreshed_pull_secret(&operator).await,
            StatusCode::OK
        );
        assert_eq!(
            manager.sync(TOKEN_ENROLLMENT_DEPLOYMENT_TOKEN).await,
            StatusCode::UNAUTHORIZED,
            "the rollout revoked the token"
        );

        assert_eq!(
            manager.enroll(&token_code, "ax_day_v1_late").await,
            StatusCode::UNAUTHORIZED
        );
        assert!(!manager.key_is_current("ax_day_v1_late").await);
        assert_eq!(
            manager.enroll(&admin_code, "ax_day_v1_admin").await,
            StatusCode::OK
        );
    }

    /// The pull Secret of an Operator installed with the deployment token
    /// still holds that token. Revoking it first would leave the Operator pod
    /// unable to pull its image on a new node, so the token stays until the
    /// Operator reports that it rewrote the Secret, even when that report
    /// comes after the rollout, or after the manager restarted.
    #[tokio::test]
    async fn the_deployment_token_is_kept_until_the_operator_reports_its_pull_secret() {
        let manager = TokenEnrollment::new().await;
        let old_token = TOKEN_ENROLLMENT_DEPLOYMENT_TOKEN;
        let operator = manager.operator_capability();

        // The rollout finishes, and the Operator has not reported.
        manager.stored.lock().unwrap().status = "update-pending".into();
        assert_eq!(manager.sync(&operator).await, StatusCode::OK);
        assert_eq!(manager.sync(old_token).await, StatusCode::OK);
        assert_eq!(manager.sync(&operator).await, StatusCode::OK);
        assert_eq!(manager.sync(old_token).await, StatusCode::OK);
        assert!(manager
            .state
            .kv
            .exists(&super::token_hold_key("dep_test"))
            .await
            .unwrap());

        // The report on a later sync, with no rollout in it, revokes.
        assert_eq!(
            manager.sync_with_refreshed_pull_secret(&operator).await,
            StatusCode::OK
        );
        assert_eq!(manager.sync(old_token).await, StatusCode::UNAUTHORIZED);
        assert!(!manager
            .state
            .kv
            .exists(&super::token_hold_key("dep_test"))
            .await
            .unwrap());
    }

    /// The Operator's pull-Secret report reaches the deployment store with the
    /// reconcile as `deployment_token_revocable`, so a store that revokes the
    /// deployment's tokens elsewhere waits for it too. A sync without the
    /// report does not send one.
    #[tokio::test]
    async fn a_refreshed_pull_secret_tells_the_store_the_token_is_revocable() {
        let manager = TokenEnrollment::new().await;
        let operator = manager.operator_capability();

        assert_eq!(manager.sync(&operator).await, StatusCode::OK);
        assert!(manager.revocable_reports.lock().unwrap().is_empty());

        assert_eq!(
            manager.sync_with_refreshed_pull_secret(&operator).await,
            StatusCode::OK
        );
        assert_eq!(*manager.revocable_reports.lock().unwrap(), vec![true]);
    }

    /// A report alone does not end the token. Workloads deployed before the
    /// Operator enrolled may still use it until the first rollout is done.
    #[tokio::test]
    async fn the_pull_secret_report_does_not_revoke_before_the_first_rollout() {
        let manager = TokenEnrollment::new().await;
        let operator = manager.operator_capability();

        assert_eq!(
            manager.sync_with_refreshed_pull_secret(&operator).await,
            StatusCode::OK
        );
        assert_eq!(
            manager.sync(TOKEN_ENROLLMENT_DEPLOYMENT_TOKEN).await,
            StatusCode::OK
        );
    }

    /// A receiver built before capability files cannot start without a
    /// credential in `ALIEN_COMMANDS_TOKEN`, but that credential is not the
    /// deployment token. The deployment token ends once the Operator reports
    /// its pull Secret, whatever receivers the release has.
    #[tokio::test]
    async fn the_deployment_token_does_not_wait_for_an_unmarked_receiver() {
        let manager = TokenEnrollment::new().await;
        let old_token = TOKEN_ENROLLMENT_DEPLOYMENT_TOKEN;
        let operator = manager.operator_capability();
        manager
            .run_release_with_receivers(&[("old-api", false), ("new-api", true)])
            .await;

        manager.stored.lock().unwrap().status = "update-pending".into();
        assert_eq!(
            manager.sync_with_refreshed_pull_secret(&operator).await,
            StatusCode::OK
        );
        assert_eq!(manager.sync(old_token).await, StatusCode::UNAUTHORIZED);
        assert_eq!(*manager.revocable_reports.lock().unwrap(), vec![true]);
        assert!(!manager
            .state
            .kv
            .exists(&super::token_hold_key("dep_test"))
            .await
            .unwrap());
    }

    /// An enrolled Operator holds a session credential it must not give to
    /// workloads, so the manager hands a receiver built before capability
    /// files the deployment's commands-only key. A marked receiver gets none,
    /// and every target carries the same key.
    #[tokio::test]
    async fn the_pull_target_carries_the_commands_key_only_for_unmarked_receivers() {
        let manager = TokenEnrollment::new().await;
        manager
            .run_release_with_receivers(&[("old-api", false), ("new-api", true)])
            .await;
        let release_id = manager
            .stored
            .lock()
            .unwrap()
            .current_release_id
            .clone()
            .expect("release");
        let release = manager
            .state
            .release_store
            .get_release(&crate::auth::Subject::system(), &release_id)
            .await
            .unwrap()
            .expect("stored release");
        let mut deployment = manager.stored.lock().unwrap().clone();
        deployment.deployment_token = Some(TOKEN_ENROLLMENT_DEPLOYMENT_TOKEN.to_string());

        let mut handed_out = Vec::new();
        for _ in 0..2 {
            // The Operator asked with its own capability, not the deployment token.
            let target = super::build_pull_target(
                &manager.state,
                &deployment,
                release.clone(),
                None,
                Some(manager.operator_capability()),
                true,
            )
            .await
            .unwrap_or_else(|_| panic!("pull target"));
            let tokens: Vec<_> = target
                .config
                .environment_variables
                .variables
                .iter()
                .filter(|variable| variable.name == alien_core::ENV_ALIEN_COMMANDS_TOKEN)
                .map(|variable| (variable.target_resources.clone(), variable.value.clone()))
                .collect();
            assert_eq!(tokens.len(), 1, "{tokens:?}");
            assert_eq!(tokens[0].0, Some(vec!["old-api".to_string()]));
            handed_out.push(tokens[0].1.clone());
        }
        let key = handed_out[0].clone();
        assert_eq!(handed_out[1], key, "every target carries the same key");
        assert!(key.starts_with("ax_cmds_"), "{key}");
        assert_ne!(key, TOKEN_ENROLLMENT_DEPLOYMENT_TOKEN);

        // The vendor sees the long-lived credential once, naming the receiver.
        let warnings: Vec<_> = manager
            .state
            .log_buffer
            .get_entries(Some("dep_test"), usize::MAX)
            .await
            .into_iter()
            .filter(|entry| {
                entry
                    .attributes
                    .contains(&("event".to_owned(), "legacy-command-receivers".to_owned()))
            })
            .collect();
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert_eq!(warnings[0].severity, "WARN");
        assert!(warnings[0].body.contains("old-api"), "{}", warnings[0].body);
        assert!(
            !warnings[0].body.contains("new-api"),
            "{}",
            warnings[0].body
        );
        assert!(!warnings[0].body.contains(&key));

        // It is a real credential, but it syncs nothing.
        assert_eq!(manager.sync(&key).await, StatusCode::FORBIDDEN);
        let issued = manager.state.token_store.list_tokens().await.unwrap();
        assert_eq!(
            issued
                .iter()
                .filter(|token| token.token_type
                    == crate::traits::token_store::TokenType::CommandsReceiver
                    && token.deployment_id.as_deref() == Some("dep_test"))
                .count(),
            1
        );
    }

    /// The key ends when neither the current release nor the one being rolled
    /// out has a receiver built before capability files.
    #[tokio::test]
    async fn the_commands_key_is_revoked_once_no_release_has_an_unmarked_receiver() {
        let manager = TokenEnrollment::new().await;
        let operator = manager.operator_capability();
        manager
            .run_release_with_receivers(&[("old-api", false)])
            .await;
        let key = manager.issue_commands_key().await;

        // A deployment that never redeploys keeps its key, sync after sync.
        for _ in 0..2 {
            assert_eq!(
                manager.sync_with_refreshed_pull_secret(&operator).await,
                StatusCode::OK
            );
            assert!(manager.commands_key_is_accepted().await);
        }

        manager
            .run_release_with_receivers(&[("old-api", true)])
            .await;
        assert_eq!(manager.sync(&operator).await, StatusCode::OK);
        assert!(!manager.commands_key_is_accepted().await);
        assert_eq!(manager.sync(&key).await, StatusCode::UNAUTHORIZED);
    }

    /// A revoked commands key never comes back: a later release with an
    /// unmarked receiver gets a different key, and the old one stays refused.
    #[tokio::test]
    async fn a_revoked_commands_key_is_not_issued_again() {
        let manager = TokenEnrollment::new().await;
        let operator = manager.operator_capability();
        manager
            .run_release_with_receivers(&[("old-api", false)])
            .await;
        let old_key = manager.issue_commands_key().await;

        manager
            .run_release_with_receivers(&[("old-api", true)])
            .await;
        assert_eq!(manager.sync(&operator).await, StatusCode::OK);
        assert!(!manager.commands_key_is_accepted().await);

        manager
            .run_release_with_receivers(&[("old-api", false)])
            .await;
        let new_key = manager.issue_commands_key().await;

        assert_ne!(new_key, old_key, "a revoked key must not be derived again");
        assert_eq!(manager.sync(&old_key).await, StatusCode::UNAUTHORIZED);
        assert!(manager.commands_key_is_accepted().await);
        assert_ne!(manager.sync(&new_key).await, StatusCode::UNAUTHORIZED);
    }

    /// The Operator names the release it runs, but the manager knows which
    /// release it assigned. A report of an older, fully marked release, or of a
    /// release with no stack, does not end the commands key while the
    /// manager's own current release has an unmarked receiver.
    #[tokio::test]
    async fn a_reported_release_does_not_decide_whether_the_commands_key_is_revoked() {
        let manager = TokenEnrollment::new().await;
        let operator = manager.operator_capability();
        let older_marked = manager
            .create_release_with_receivers(&[("old-api", true)])
            .await;
        let stackless = manager.create_stackless_release().await;
        manager
            .run_release_with_receivers(&[("old-api", false)])
            .await;
        manager.issue_commands_key().await;

        for reported in [older_marked, stackless] {
            manager.stored.lock().unwrap().status = "update-pending".into();
            assert_eq!(
                manager
                    .sync_reporting_release(&operator, true, Some(&reported))
                    .await,
                StatusCode::OK
            );
            assert!(
                manager.commands_key_is_accepted().await,
                "reported release {reported} must not revoke the key"
            );
        }
    }

    /// Mid-rollout the current release is fully marked, but the release being
    /// rolled out adds an unmarked receiver. The key stays until neither has one.
    #[tokio::test]
    async fn the_commands_key_is_kept_while_the_release_being_rolled_out_has_an_unmarked_receiver()
    {
        let manager = TokenEnrollment::new().await;
        let operator = manager.operator_capability();
        manager
            .run_release_with_receivers(&[("old-api", true)])
            .await;
        let incoming = manager
            .create_release_with_receivers(&[("old-api", true), ("new-api", false)])
            .await;
        manager.issue_commands_key().await;
        manager.stored.lock().unwrap().desired_release_id = Some(incoming);
        manager.stored.lock().unwrap().status = "update-pending".into();

        assert_eq!(
            manager.sync_with_refreshed_pull_secret(&operator).await,
            StatusCode::OK
        );
        assert!(manager.commands_key_is_accepted().await);
    }

    /// A release of another project is not one the deployment runs, so the key
    /// stays even when that release is fully marked.
    #[tokio::test]
    async fn the_commands_key_is_kept_when_the_release_belongs_to_another_project() {
        let manager = TokenEnrollment::new().await;
        let operator = manager.operator_capability();
        let foreign = manager
            .state
            .release_store
            .create_release(
                &crate::auth::Subject::system(),
                CreateReleaseParams {
                    project_id: "other_project".to_string(),
                    stacks: HashMap::from([(
                        Platform::Kubernetes,
                        Stack::new("app".into()).build(),
                    )]),
                    git_commit_sha: None,
                    git_commit_ref: None,
                    git_commit_message: None,
                    setup_templates: Default::default(),
                    signed_stacks: Default::default(),
                },
            )
            .await
            .expect("release")
            .id;
        manager.stored.lock().unwrap().current_release_id = Some(foreign);
        manager.issue_commands_key().await;

        assert_eq!(
            manager.sync_with_refreshed_pull_secret(&operator).await,
            StatusCode::OK
        );
        assert!(manager.commands_key_is_accepted().await);
    }

    /// Without a signing key the manager cannot derive the key, and it must
    /// not hand out a guessable one: the receiver gets no credential and fails
    /// to start with a named cause.
    #[tokio::test]
    async fn a_manager_without_a_signing_key_hands_out_no_commands_key() {
        let mut manager = TokenEnrollment::new().await;
        manager.state.config = Arc::new(crate::config::ManagerConfig {
            response_signing_key: Vec::new(),
            ..(*manager.state.config).clone()
        });
        manager
            .run_release_with_receivers(&[("old-api", false)])
            .await;
        let release_id = manager
            .stored
            .lock()
            .unwrap()
            .current_release_id
            .clone()
            .expect("release");
        let release = manager
            .state
            .release_store
            .get_release(&crate::auth::Subject::system(), &release_id)
            .await
            .unwrap()
            .expect("stored release");
        let deployment = manager.stored.lock().unwrap().clone();

        let target = super::build_pull_target(
            &manager.state,
            &deployment,
            release,
            None,
            Some(manager.operator_capability()),
            true,
        )
        .await
        .unwrap_or_else(|_| panic!("pull target"));
        assert!(target
            .config
            .environment_variables
            .variables
            .iter()
            .all(|variable| variable.name != alien_core::ENV_ALIEN_COMMANDS_TOKEN));
        assert!(!manager.commands_key_is_accepted().await);
    }

    /// A release the manager cannot read could hold an unmarked receiver, so
    /// the key stays, and the warning names the deployment.
    #[tokio::test]
    async fn the_commands_key_is_kept_when_the_current_release_cannot_be_read() {
        #[derive(Clone, Default)]
        struct Captured(Arc<Mutex<Vec<u8>>>);
        impl std::io::Write for Captured {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(buf);
                Ok(buf.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        let manager = TokenEnrollment::new().await;
        let operator = manager.operator_capability();
        manager.stored.lock().unwrap().current_release_id = Some("rel_gone".into());
        manager.stored.lock().unwrap().status = "update-pending".into();
        manager.issue_commands_key().await;

        let captured = Captured::default();
        let writer = captured.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::WARN)
            .with_ansi(false)
            .with_writer(move || writer.clone())
            .finish();
        let guard = tracing::subscriber::set_default(subscriber);
        let status = manager.sync_with_refreshed_pull_secret(&operator).await;
        drop(guard);

        assert_eq!(status, StatusCode::OK);
        assert!(manager.commands_key_is_accepted().await);
        let logs = String::from_utf8(captured.0.lock().unwrap().clone()).unwrap();
        assert!(
            logs.contains("Release not found; keeping the commands key")
                && logs.contains("dep_test"),
            "{logs}"
        );
    }

    /// An Operator that cleared its config in token mode and then enrolled
    /// asks for it. The manager sends the running release again, and only then.
    #[tokio::test]
    async fn an_operator_without_its_installed_config_gets_the_running_release() {
        let record =
            pull_record_in_cluster("running", alien_core::KubernetesClusterOwnership::Existing);
        let (response, _) = sync_operator_without_claim_reporting(record.clone(), false).await;
        assert!(response.get("target").is_none(), "{response}");

        let (response, _) = sync_operator_without_claim_reporting(record, true).await;
        let target = response.get("target").expect("the running release again");
        let current = response["currentState"]["currentRelease"]["releaseId"].clone();
        assert!(current.is_string(), "{response}");
        assert_eq!(target["releaseInfo"]["releaseId"], current, "{response}");

        let mut updating = pull_record_in_cluster(
            "update-failed",
            alien_core::KubernetesClusterOwnership::Existing,
        );
        updating.desired_release_id = Some("rel_target".to_string());
        let (response, _) = sync_operator_without_claim_reporting(updating, true).await;
        assert!(response.get("target").is_none(), "{response}");
    }

    /// A new Operator pod can start while its predecessor was applying the
    /// install-time target, whose config holds no commands key. It reports the
    /// update as pending and asks for its config. The manager has no work of
    /// its own to send, so it sends the release it runs again.
    #[tokio::test]
    async fn an_operator_stuck_applying_a_config_it_cannot_use_gets_its_release_again() {
        let mut record = pull_record_in_cluster(
            "update-pending",
            alien_core::KubernetesClusterOwnership::Existing,
        );
        record.desired_release_id = None;
        let (response, _, _) = sync_operator_full(
            record,
            None,
            None,
            Stack::new("app".to_string()).build(),
            None,
            false,
            SyncReport {
                installed_config_missing: true,
                status: Some("update-pending"),
                ..SyncReport::default()
            },
        )
        .await;
        let target = response
            .get("target")
            .unwrap_or_else(|| panic!("the release again: {response}"));
        let current = response["currentState"]["currentRelease"]["releaseId"].clone();
        assert!(current.is_string(), "{response}");
        assert_eq!(target["releaseInfo"]["releaseId"], current, "{response}");
    }

    /// A manager that serves no charts names no Operator image itself. The
    /// image its store reports for the deployment reaches the Operator, and a
    /// store that reports none sends none.
    #[tokio::test]
    async fn a_manager_without_charts_sends_the_image_its_store_reports() {
        let record =
            pull_record_in_cluster("running", alien_core::KubernetesClusterOwnership::Existing);

        let (response, _) = sync_operator_with_reconciled_image(
            record.clone(),
            false,
            Some("registry.example.com/operator:2.0.0"),
        )
        .await;
        assert_eq!(
            response["targetOperatorImage"], "registry.example.com/operator:2.0.0",
            "{response}"
        );

        let (response, _) = sync_operator_with_reconciled_image(record, false, None).await;
        assert!(response.get("targetOperatorImage").is_none(), "{response}");
    }

    fn dynamic_container(name: &str) -> alien_core::sync::TargetDynamicContainer {
        alien_core::sync::TargetDynamicContainer {
            name: name.to_string(),
            generation: 1,
            image: "registry.example.com/app@sha256:abc".to_string(),
            cpu: "0.5".to_string(),
            memory: "512Mi".to_string(),
            replicas: 1,
            ports: vec![8080],
            deleted: false,
            env: Default::default(),
            secret_env: Default::default(),
            value_slots: None,
            health_check: None,
            suspended_reason: None,
        }
    }

    /// The Operator gets a dynamic container's stored variables in its sync
    /// response, and the control plane's reconcile never sees them.
    #[tokio::test]
    async fn dynamic_container_values_reach_the_operator_and_not_the_control_plane() {
        use crate::values::{ValueKind, ValueScope, ValueSlot};
        const CANARY: &str = "canary-3b9e7d51-not-a-real-secret";

        let dir = tempfile::tempdir().expect("values directory");
        let values = Arc::new(crate::values::fixtures::kv_store(dir.path()).await);
        let scope = ValueScope::deployment("dep_test").expect("scope");
        for (container, name, kind, value) in [
            ("api", "TOKEN", ValueKind::Secret, CANARY),
            ("api", "LEVEL", ValueKind::Plain, "debug"),
            ("api.worker", "TOKEN", ValueKind::Secret, "the-worker-token"),
        ] {
            values
                .put(
                    &scope,
                    &ValueSlot::container_env(container, name).expect("slot"),
                    kind,
                    value.to_string(),
                )
                .await
                .expect("stored");
        }
        let mut record = deployment_record_with_state("running", None);
        record.desired_release_id = Some("rel_target".to_string());
        let mut api = dynamic_container("api");
        api.env.insert("KEPT".to_string(), "kept".to_string());
        let mut deleted = dynamic_container("api.worker");
        deleted.deleted = true;
        let targets = vec![api, dynamic_container("web"), deleted];

        let (response, reconciled) =
            sync_operator_with_targets(record, Some(targets), Some(values)).await;

        let targets = response["targetDynamicContainers"]
            .as_array()
            .expect("targets in the response");
        let by_name = |name: &str| {
            targets
                .iter()
                .find(|t| t["name"] == name)
                .expect("target present")
        };
        assert_eq!(by_name("api")["secretEnv"], json!({ "TOKEN": CANARY }));
        assert_eq!(
            by_name("api")["env"],
            json!({ "KEPT": "kept", "LEVEL": "debug" })
        );
        assert_eq!(by_name("web")["env"], json!({}));
        assert_eq!(by_name("web")["secretEnv"], json!({}));
        assert_eq!(by_name("api.worker")["secretEnv"], json!({}));

        assert!(!reconciled.is_empty(), "the control plane was reconciled");
        for data in &reconciled {
            let seen = format!("{data:?}{}", serde_json::to_string(&data.state).unwrap());
            assert!(!seen.contains(CANARY), "{seen}");
            assert!(!seen.contains("the-worker-token"), "{seen}");
        }
    }

    /// A sync creates a deployment's generated secret input on the manager,
    /// reports its metadata, and keeps the value out of everything the control
    /// plane receives.
    #[tokio::test]
    async fn a_sync_reports_generated_slots_and_never_their_values() {
        use crate::values::{ValueKind, ValueScope, ValueSlot};

        let dir = tempfile::tempdir().expect("values directory");
        let values = Arc::new(crate::values::fixtures::kv_store(dir.path()).await);
        let mut stack = Stack::new("app".to_string()).build();
        stack.inputs = vec![alien_core::StackInputDefinition {
            id: "signingKey".to_string(),
            kind: alien_core::StackInputKind::Secret,
            when: None,
            provided_by: vec![alien_core::StackInputProvider::Developer],
            required: true,
            label: "Signing key".to_string(),
            description: "Key the service signs internal tokens with.".to_string(),
            placeholder: None,
            default: None,
            platforms: None,
            validation: None,
            generate: Some(alien_core::StackInputGenerate { length: 40 }),
            env: vec![],
        }];
        let record = deployment_record_with_state("running", None);

        let (response, reconciled, generated) =
            sync_operator_report(record, None, Some(values.clone()), stack).await;

        let scope = ValueScope::deployment("dep_test").expect("scope");
        let slot: ValueSlot = "input:signingKey".parse().expect("slot");
        let stored = values
            .get(&scope, &slot)
            .await
            .expect("store read")
            .expect("the sync generated the input");
        assert_eq!(stored.kind, ValueKind::Secret);
        let value: String = serde_json::from_str(&stored.value).expect("JSON string");
        assert_eq!(value.len(), 40);

        assert!(!generated.is_empty(), "the reconcile carried a report");
        for slots in &generated {
            assert_eq!(slots, &vec![stored.meta(&scope).expect("meta")]);
        }
        let report = serde_json::to_value(&generated[0]).expect("report json");
        assert_eq!(report[0]["slot"], "input:signingKey");
        assert_eq!(report[0]["kind"], "secret");
        assert_eq!(report[0]["digest"].as_str().map(str::len), Some(64));
        assert!(report[0]["updatedAt"].is_string());
        let mut seen = response.to_string();
        for data in &reconciled {
            seen.push_str(&format!("{data:?}"));
            seen.push_str(&serde_json::to_string(&data.state).unwrap());
        }
        seen.push_str(&serde_json::to_string(&generated).unwrap());
        assert!(!seen.contains(&value), "{seen}");
    }

    /// A deployment config that plans `slots` and nothing else.
    fn planning_config(slots: Vec<alien_core::ValueSlotRef>) -> alien_core::DeploymentConfig {
        let mut config = alien_core::DeploymentConfig::builder()
            .deployment_name("test".to_string())
            .stack_settings(StackSettings::default())
            .input_values(Default::default())
            .environment_variables(alien_core::EnvironmentVariablesSnapshot {
                variables: Vec::new(),
                hash: String::new(),
                created_at: String::new(),
            })
            .allow_frozen_changes(false)
            .external_bindings(Default::default())
            .build();
        config.value_slots = slots;
        config
    }

    fn plain_slot_ref(slot: &str, digest: String) -> alien_core::ValueSlotRef {
        alien_core::ValueSlotRef {
            slot: slot.to_string(),
            kind: alien_core::EnvironmentVariableType::Plain,
            digest,
            env_name: None,
            targets: None,
        }
    }

    /// The state an Operator reports for one running worker with `env`.
    fn reported_state_with_env(env: &[(&str, &str)]) -> StackState {
        let mut worker = alien_core::Worker::new("api".to_string())
            .code(alien_core::WorkerCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("default".to_string())
            .build();
        worker.environment = env
            .iter()
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect();
        let resource = alien_core::Resource::new(worker);
        let mut reported = StackState::new(Platform::Kubernetes);
        reported.resources.insert(
            "api".to_string(),
            alien_core::StackResourceState {
                progress: None,
                display_name: None,
                description: None,
                resource_type: resource.resource_type().as_ref().to_string(),
                internal_state: None,
                status: alien_core::ResourceStatus::Running,
                outputs: None,
                previous_config: None,
                config: resource,
                retry_attempt: 0,
                error: None,
                lifecycle: Some(alien_core::ResourceLifecycle::Live),
                controller_platform: None,
                dependencies: Vec::new(),
                last_failed_state: None,
                remote_binding_params: None,
                external_binding: false,
            },
        );
        reported
    }

    /// An Operator reports a stack state that still carries a value the
    /// manager holds, which the user replaced after the deployment loaded it.
    /// Neither the old nor the new value reaches the control plane.
    #[tokio::test]
    async fn an_operator_report_is_redacted_of_a_replaced_held_value() {
        use crate::values::{ValueKind, ValueScope, ValueSlot};
        const OLD: &str = "canary-old-6c13f0";
        const NEW: &str = "canary-new-a94e27";

        let dir = tempfile::tempdir().expect("values directory");
        let values = Arc::new(crate::values::fixtures::kv_store(dir.path()).await);
        let scope = ValueScope::deployment("dep_test").expect("scope");
        let slot: ValueSlot = "env:API_URL".parse().expect("slot");
        let planned = values
            .put(&scope, &slot, ValueKind::Plain, OLD.to_string())
            .await
            .expect("stored")
            .digest;
        let mut record = deployment_record_with_state("running", None);
        record.deployment_config = Some(planning_config(vec![plain_slot_ref(
            "env:API_URL",
            planned,
        )]));
        values
            .put(&scope, &slot, ValueKind::Plain, NEW.to_string())
            .await
            .expect("replaced");
        let reported = reported_state_with_env(&[("API_URL", OLD)]);

        let (_, reconciled, _) = sync_operator_reporting(
            record,
            None,
            Some(values),
            Stack::new("app".to_string()).build(),
            Some(serde_json::to_value(&reported).expect("state json")),
        )
        .await;

        assert!(!reconciled.is_empty(), "the control plane was reconciled");
        let seen = reconciled
            .iter()
            .map(|data| serde_json::to_string(&data.state).unwrap())
            .collect::<String>();
        assert!(
            seen.contains("alien-held:"),
            "the report was redacted: {seen}"
        );
        assert!(!seen.contains(OLD), "{seen}");
        assert!(!seen.contains(NEW), "{seen}");
    }

    /// An Operator was served a held value, which the user then replaced and
    /// deleted. Its next report still carries the value it was served: it must
    /// not reach the control plane, though the manager no longer holds it.
    #[tokio::test]
    async fn an_operator_report_is_redacted_of_a_value_served_then_changed_then_deleted() {
        use crate::values::{ValueKind, ValueScope, ValueSlot};
        const SERVED: &str = "canary-served-3f7a1c";
        const CHANGED: &str = "canary-changed-82d0e5";

        let dir = tempfile::tempdir().expect("values directory");
        let values = Arc::new(crate::values::fixtures::kv_store(dir.path()).await);
        let scope = ValueScope::deployment("dep_test").expect("scope");
        let slot: ValueSlot = "env:API_URL".parse().expect("slot");
        let digest = values
            .put(&scope, &slot, ValueKind::Plain, SERVED.to_string())
            .await
            .expect("stored")
            .digest;
        let mut pull = pull_record_in_cluster(
            "update-pending",
            alien_core::KubernetesClusterOwnership::Existing,
        );
        pull.deployment_config = Some(planning_config(vec![plain_slot_ref("env:API_URL", digest)]));

        let served = sync_with_values(
            pull,
            Arc::new(UnresolvableManagementIdentity),
            Some(values.clone()),
        )
        .await;
        assert_eq!(served.status, StatusCode::OK, "{}", served.response);
        let target = serde_json::to_string(&served.response["target"]).expect("target json");
        assert!(target.contains(SERVED), "the Operator was served the value");

        values
            .put(&scope, &slot, ValueKind::Plain, CHANGED.to_string())
            .await
            .expect("changed");
        values.delete(&scope, &slot).await.expect("deleted");

        let (_, reconciled, _) = sync_operator_reporting(
            deployment_record_with_state("running", None),
            None,
            Some(values),
            Stack::new("app".to_string()).build(),
            Some(
                serde_json::to_value(reported_state_with_env(&[("API_URL", SERVED)]))
                    .expect("state json"),
            ),
        )
        .await;

        assert!(!reconciled.is_empty(), "the control plane was reconciled");
        let seen = reconciled
            .iter()
            .map(|data| serde_json::to_string(&data.state).unwrap())
            .collect::<String>();
        assert!(
            seen.contains("alien-held:"),
            "the report was redacted: {seen}"
        );
        assert!(!seen.contains(SERVED), "{seen}");
        assert!(!seen.contains(CHANGED), "{seen}");
    }

    /// A report for a deployment whose release is gone can't map held stack
    /// inputs to variables. It is still accepted, with the input's value
    /// redacted by its content.
    #[tokio::test]
    async fn an_operator_report_with_a_gone_release_is_accepted_and_redacted_by_content() {
        use crate::values::{ValueKind, ValueScope, ValueSlot};
        const INPUT: &str = "canary-input-5e0a17";

        let dir = tempfile::tempdir().expect("values directory");
        let values = Arc::new(crate::values::fixtures::kv_store(dir.path()).await);
        values
            .put(
                &ValueScope::deployment("dep_test").expect("scope"),
                &"input:endpoint".parse::<ValueSlot>().expect("slot"),
                ValueKind::Plain,
                format!("\"{INPUT}\""),
            )
            .await
            .expect("stored");

        let (_, reconciled, _) = sync_operator_reporting_for(
            deployment_record_with_state("running", None),
            None,
            Some(values),
            Stack::new("app".to_string()).build(),
            Some(
                serde_json::to_value(reported_state_with_env(&[("ENDPOINT", INPUT)]))
                    .expect("state json"),
            ),
            true,
        )
        .await;

        assert!(!reconciled.is_empty(), "the control plane was reconciled");
        let seen = reconciled
            .iter()
            .map(|data| serde_json::to_string(&data.state).unwrap())
            .collect::<String>();
        assert!(
            seen.contains("alien-held:"),
            "the report was redacted: {seen}"
        );
        assert!(!seen.contains(INPUT), "{seen}");
    }

    #[tokio::test]
    async fn operator_heartbeat_keeps_a_failed_update_failed() {
        // A failed operation keeps its desired release for diagnosis. The
        // Operator's healthy report must not turn the failure back into running.
        let mut record = deployment_record_with_state("update-failed", None);
        record.desired_release_id = Some("rel_target".to_string());
        record.error = Some(json!({
            "code": "RELEASE_NOT_AVAILABLE_FOR_PLATFORM",
            "message": "Release 'rel_target' does not contain platform 'kubernetes'",
            "retryable": false,
            "internal": false,
            "httpStatusCode": 400,
        }));

        let (response, reconciled) = sync_operator_without_claim(record).await;

        let state = assert_heartbeat_against_stored_state(
            &response,
            &reconciled,
            DeploymentStatus::UpdateFailed,
        );
        assert_eq!(
            state.error.map(|error| error.code),
            Some("RELEASE_NOT_AVAILABLE_FOR_PLATFORM".to_string())
        );
    }

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
        assert!(req.observed_inventory_batches.is_empty());
        assert!(req.capabilities.is_empty());
        assert!(req.operator_version.is_none());
        assert!(req.update_heartbeat);
    }

    #[test]
    fn reconcile_request_accepts_typed_heartbeats() {
        let req: ReconcileRequest = serde_json::from_value(json!({
            "deploymentId": "dep_test",
            "session": "session_test",
            "state": { "status": "running" },
            "updateHeartbeat": true,
            "resourceHeartbeats": [{
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
    fn agent_sync_request_defaults_missing_heartbeats_to_empty() {
        let req: AgentSyncRequest = serde_json::from_value(json!({
            "deploymentId": "dep_test",
            "currentState": null
        }))
        .expect("agent sync request without heartbeats should remain compatible");

        assert!(req.heartbeats.is_empty());
        assert!(req.observed_inventory_batches.is_empty());
        assert!(req.capabilities.is_empty());
        assert!(req.operator_version.is_none());
        assert!(!req.supports_execution_claims);
    }

    #[test]
    fn agent_sync_wire_request_accepts_operator_image_receipt() {
        let req: AgentSyncWireRequest = serde_json::from_value(json!({
            "deploymentId": "dep_test",
            "operatorImage": {
                "source": "package",
                "packageId": "pkg_operator_123",
                "packageVersion": "7",
                "image": "registry.example.com/alien/operator@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "digest": "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            }
        }))
        .expect("agent sync wire request should accept an immutable Operator image receipt");

        req.operator_image
            .expect("operator image receipt should be present")
            .validate()
            .expect("operator image receipt should be valid");
        assert_eq!(req.request.deployment_id, "dep_test");
    }

    #[test]
    fn agent_sync_wire_request_receives_the_operator_application_report() {
        let application = ObservedApplicationReport {
            source: ObservedApplicationSource::Kubernetes,
            chart_name: Some("shop".to_string()),
            chart_version: Some("1.4.0".to_string()),
            images: vec![ObservedApplicationImage {
                workload: "apps/v1:Deployment:shop:api".to_string(),
                container: "api".to_string(),
                image: "registry.example.com/shop/api:1.4.0".to_string(),
                digest: Some(format!("sha256:{}", "a".repeat(64))),
            }],
            complete: true,
            observed_at: "2026-09-24T10:00:00Z".parse().unwrap(),
        };
        let operator_request: SyncRequest =
            serde_json::from_value(json!({ "deploymentId": "dep_test" })).unwrap();
        let wire = serde_json::to_value(
            SyncInput::builder(operator_request)
                .application(application.clone())
                .build(),
        )
        .unwrap();

        let req: AgentSyncWireRequest = serde_json::from_value(wire).unwrap();

        assert_eq!(req.request.deployment_id, "dep_test");
        assert_eq!(req.application, Some(application));
        assert_eq!(req.operator_image, None);
    }

    fn sync_body_with_container_heartbeat(cpu: Value) -> Value {
        json!({
            "deploymentId": "dep_test",
            "currentState": null,
            "resourceHeartbeats": [{
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
                        "cpu": cpu,
                        "memory": null,
                        "workload": null,
                        "pods": [],
                        "instances": [],
                        "events": []
                    }
                },
                "raw": []
            }]
        })
    }

    #[test]
    fn agent_sync_request_accepts_typed_heartbeats() {
        let req: AgentSyncRequest =
            serde_json::from_value(sync_body_with_container_heartbeat(Value::Null))
                .expect("agent sync request should accept typed heartbeat envelopes");

        assert_eq!(req.heartbeats.len(), 1);
        assert_eq!(req.heartbeats[0].resource_id, "api");
        assert!(matches!(
            req.heartbeats[0].data,
            ResourceHeartbeatData::Container(_)
        ));
    }

    #[test]
    fn sync_body_text_with_fractional_metrics_decodes() {
        // Operators send fractional CPU samples. The body is decoded from text
        // through a flattened request type, which buffers every number. A
        // dependency that enables serde_json's `arbitrary_precision` feature
        // makes this decode fail for the whole build.
        let body = sync_body_with_container_heartbeat(json!({ "value": 0.25, "unit": "cores" }))
            .to_string();

        let req: AgentSyncWireRequest =
            serde_json::from_str(&body).expect("sync body with fractional metrics must decode");

        let ResourceHeartbeatData::Container(ContainerHeartbeatData::Kubernetes(container)) =
            &req.request.heartbeats[0].data
        else {
            panic!("expected a Kubernetes container heartbeat");
        };
        assert_eq!(container.cpu.as_ref().map(|cpu| cpu.value), Some(0.25));
    }

    #[test]
    fn initialize_accepts_cloud_base_platform_for_kubernetes() {
        assert_eq!(
            validate_kubernetes_base_platform(Platform::Kubernetes, Some(Platform::Gcp)).unwrap(),
            Some(Platform::Gcp)
        );
    }

    #[test]
    fn initialize_rejects_base_platform_for_non_kubernetes_platform() {
        assert!(validate_kubernetes_base_platform(Platform::Aws, Some(Platform::Gcp)).is_err());
    }

    #[test]
    fn initialize_rejects_non_cloud_base_platform_for_kubernetes() {
        assert!(
            validate_kubernetes_base_platform(Platform::Kubernetes, Some(Platform::Local)).is_err()
        );
    }

    #[test]
    fn helm_release_accepts_only_names_helm_and_kubernetes_accept() {
        let release = |name: &str, namespace: &str| HelmRelease {
            name: name.to_string(),
            namespace: namespace.to_string(),
        };
        assert!(release("alien-operator", "alien-orders").is_valid());
        assert!(release("orders.v2", "default").is_valid());
        assert!(release(&"a".repeat(53), &"b".repeat(63)).is_valid());

        for (name, namespace) in [
            ("", "default"),
            ("orders", ""),
            ("Orders", "default"),
            ("-orders", "default"),
            ("orders.", "default"),
            ("orders", "team.orders"),
            ("orders; helm list", "default"),
            ("orders", "default $(id)"),
            (&"a".repeat(54), "default"),
            ("orders", &"b".repeat(64)),
        ] {
            assert!(
                !release(name, namespace).is_valid(),
                "{name:?} in {namespace:?} should be refused"
            );
        }
    }

    #[test]
    fn initialize_requires_explicit_initial_desired_release() {
        let request: InitializeRequest = serde_json::from_value(json!({
            "platform": "kubernetes",
            "permission": "observe",
            "initialDesiredRelease": "none",
            "setupItem": "deployment"
        }))
        .expect("explicit no-release initialization should deserialize");
        assert_eq!(request.initial_desired_release, InitialDesiredRelease::None);
        assert_eq!(request.setup_item.as_deref(), Some("deployment"));

        let missing_selection = serde_json::from_value::<InitializeRequest>(json!({
            "platform": "kubernetes",
            "permission": "observe"
        }));
        assert!(missing_selection.is_err());
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
    fn ignores_running_agent_state_after_delete_was_requested() {
        let deployment =
            deployment_record_with_state("delete-pending", Some(StackState::new(Platform::Local)));
        let mut agent_state = uninitialized_state();
        agent_state.status = DeploymentStatus::Running;

        assert!(should_ignore_agent_state_report(&deployment, &agent_state));
    }

    #[test]
    fn accepts_agent_delete_progress_after_delete_was_requested() {
        let deployment =
            deployment_record_with_state("delete-pending", Some(StackState::new(Platform::Local)));
        let mut agent_state = uninitialized_state();
        agent_state.status = DeploymentStatus::Deleting;

        assert!(!should_ignore_agent_state_report(&deployment, &agent_state));
    }

    #[test]
    fn returns_setup_owned_cleanup_instead_of_accepting_stale_pull_progress() {
        let mut reported = uninitialized_state();
        reported.status = DeploymentStatus::RetentionRequired;
        for status in [
            "retention-required",
            "teardown-required",
            "teardown-failed",
            "retained",
            "deleted",
        ] {
            let saved =
                deployment_record_with_state(status, Some(StackState::new(Platform::Local)));
            let ignored = should_ignore_agent_state_report(&saved, &reported);
            assert!(ignored, "setup owns {status}");
            assert!(should_return_current_state_for_agent_sync(
                ignored, false, &saved
            ));
        }
    }

    #[test]
    fn accepts_observe_only_running_state_without_desired_release() {
        let deployment = deployment_record_with_state("pending", None);
        let mut agent_state = uninitialized_state();
        agent_state.status = DeploymentStatus::Running;

        assert!(!agent_state.has_desired());
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
            true,
            &deployment
        ));
    }

    #[test]
    fn hydrates_seeded_resource_prefix_before_first_execution_claim() {
        let deployment = deployment_record_with_state(
            "pending",
            Some(StackState::with_resource_prefix(
                Platform::Kubernetes,
                "chart-release".to_string(),
            )),
        );
        let report = serde_json::to_value(uninitialized_state()).unwrap();

        assert!(agent_needs_initial_state_hydration(
            &deployment,
            Some(&report),
            false,
        ));
        assert!(!agent_needs_initial_state_hydration(
            &deployment,
            Some(&report),
            true,
        ));
    }

    #[test]
    fn returns_current_state_when_delete_was_requested() {
        let deployment =
            deployment_record_with_state("delete-pending", Some(StackState::new(Platform::Local)));

        assert!(should_return_current_state_for_agent_sync(
            false,
            false,
            &deployment
        ));
    }

    #[test]
    fn returns_imported_state_before_an_unclaimed_initial_target() {
        let deployment = deployment_record_with_state(
            "pending",
            Some(StackState::with_resource_prefix(
                Platform::Kubernetes,
                "helm-release".to_string(),
            )),
        );

        assert!(should_return_current_state_for_agent_sync(
            false,
            true,
            &deployment,
        ));
        assert!(!should_return_current_state_for_agent_sync(
            false,
            false,
            &deployment,
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
    fn deleting_deployment_needs_target_even_when_current_matches_desired() {
        let mut deployment = deployment_record_with_state("delete-pending", None);
        deployment.current_release_id = Some("rel_test".to_string());
        deployment.desired_release_id = Some("rel_test".to_string());

        assert!(deployment_needs_target(&deployment));
    }

    #[test]
    fn delete_failed_deployment_needs_target_for_retry() {
        let mut deployment = deployment_record_with_state("delete-failed", None);
        deployment.current_release_id = Some("rel_test".to_string());
        deployment.desired_release_id = Some("rel_test".to_string());

        assert!(deployment_needs_target(&deployment));
    }

    #[test]
    fn deleting_deployment_uses_current_release_when_desired_was_cleared() {
        let mut deployment = deployment_record_with_state("delete-pending", None);
        deployment.current_release_id = Some("rel_test".to_string());
        deployment.desired_release_id = None;

        assert_eq!(deployment_target_release_id(&deployment), Some("rel_test"));
        assert!(deployment_needs_target(&deployment));
    }

    #[test]
    fn observe_only_deployment_does_not_need_target() {
        let deployment = deployment_record_with_state("running", None);

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
    fn desired_pull_target_is_never_delivered_without_its_claim() {
        let mut deployment = deployment_record_with_state("update-failed", None);
        deployment.current_release_id = Some("rel_old".to_string());
        deployment.desired_release_id = Some("rel_new".to_string());

        assert!(!may_deliver_agent_target(&deployment, true, false));
        assert!(may_deliver_agent_target(&deployment, true, true));
    }

    #[test]
    fn standalone_pull_target_does_not_require_a_control_plane_claim() {
        let mut deployment = deployment_record_with_state("update-pending", None);
        deployment.current_release_id = Some("rel_old".to_string());
        deployment.desired_release_id = Some("rel_new".to_string());

        assert!(may_deliver_agent_target(&deployment, false, false));
    }

    #[test]
    fn active_claim_only_completes_on_terminal_agent_state() {
        let deployment = deployment_record_with_state("running", None);
        let mut state = uninitialized_state();
        state.status = DeploymentStatus::Updating;
        state.target_release = Some(ReleaseInfo {
            release_id: Some("rel_a".to_string()),
            version: None,
            description: None,
            stack: Stack::new("test".to_string()).build(),
            signed: None,
            signer_lists: Vec::new(),
        });
        assert!(!agent_report_completes_claim(&deployment, &state));

        state.status = DeploymentStatus::Running;
        state.current_release = state.target_release.clone();
        assert!(agent_report_completes_claim(&deployment, &state));

        state.status = DeploymentStatus::UpdateFailed;
        assert!(agent_report_completes_claim(&deployment, &state));
    }

    #[test]
    fn settled_pull_deployment_still_needs_no_claim() {
        let deployment = deployment_record_with_state("running", None);

        assert!(!may_deliver_agent_target(&deployment, true, false));
    }

    #[test]
    fn target_config_preserves_control_plane_public_endpoints() {
        let mut deployment = deployment_record_with_state("provisioning", None);
        let public_endpoints = HashMap::from([(
            "host-loader".to_string(),
            HashMap::from([(
                "wildcard".to_string(),
                "https://wildcard.byoc.example.test".to_string(),
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

    /// Remote runners resolve gated live resources from the acquired config,
    /// so the stored deployer answers must reach it — an empty map here would
    /// flip gated resources back to their declared defaults.
    #[test]
    fn target_config_carries_stored_input_values() {
        let stored_values =
            HashMap::from([("enableAnalytics".to_string(), serde_json::json!(true))]);
        let mut deployment = deployment_record_with_state("provisioning", None);
        deployment.input_values = stored_values.clone();

        let config = build_target_deployment_config(
            &deployment,
            StackSettings::default(),
            None,
            vec![],
            "https://manager.example.test".to_string(),
            None,
            None,
        );
        assert_eq!(config.input_values, stored_values);
        assert!(config.stored_secret_input_ids.is_none());

        // A control-plane-supplied config owns the values when present.
        let control_plane_values =
            HashMap::from([("enableAnalytics".to_string(), serde_json::json!(false))]);
        deployment.deployment_config = Some(DeploymentConfig {
            stored_secret_input_ids: Some(vec!["apiKey".to_string()]),

            input_values: control_plane_values.clone(),
            ..test_deployment_config()
        });

        let config = build_target_deployment_config(
            &deployment,
            StackSettings::default(),
            None,
            vec![],
            "https://manager.example.test".to_string(),
            None,
            None,
        );
        assert_eq!(config.input_values, control_plane_values);
        assert_eq!(
            config.stored_secret_input_ids,
            Some(vec!["apiKey".to_string()])
        );

        deployment
            .deployment_config
            .as_mut()
            .unwrap()
            .stored_secret_input_ids = Some(Vec::new());
        let empty = build_target_deployment_config(
            &deployment,
            StackSettings::default(),
            None,
            vec![],
            "https://manager.example.test".to_string(),
            None,
            None,
        );
        assert_eq!(empty.stored_secret_input_ids, Some(Vec::new()));

        // A config from a control plane that predates gate answers has an
        // empty map; the stored answers must stand in, not the defaults.
        deployment.deployment_config = Some(test_deployment_config());

        let config = build_target_deployment_config(
            &deployment,
            StackSettings::default(),
            None,
            vec![],
            "https://manager.example.test".to_string(),
            None,
            None,
        );
        assert_eq!(config.input_values, stored_values);
        assert!(config.stored_secret_input_ids.is_none());
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

    #[test]
    fn security_settings_old_operators_cannot_read_are_removed() {
        use alien_core::{ContainerSecurity, ContainerSecurityProfile};
        let container = |id: &str, security: Option<ContainerSecurity>| {
            let mut container = alien_core::Container::new(id.to_string())
                .code(alien_core::ContainerCode::Image {
                    image: "api:latest".to_string(),
                })
                .cpu(alien_core::ResourceSpec {
                    min: "0.5".to_string(),
                    desired: "1".to_string(),
                })
                .memory(alien_core::ResourceSpec {
                    min: "512Mi".to_string(),
                    desired: "1Gi".to_string(),
                })
                .port(8080)
                .permissions("execution".to_string())
                .build();
            container.security = security;
            container
        };
        let old_form = ContainerSecurity {
            profile: ContainerSecurityProfile::Restricted,
            run_as_user: Some(1000),
            run_as_group: Some(1000),
            read_only_root_filesystem: Some(true),
            writable_paths: vec![],
        };
        let mut stack = alien_core::Stack::new("app".to_string())
            .add(
                container(
                    "optout",
                    Some(ContainerSecurity {
                        profile: ContainerSecurityProfile::Unrestricted,
                        run_as_user: None,
                        run_as_group: None,
                        read_only_root_filesystem: None,
                        writable_paths: vec![],
                    }),
                ),
                alien_core::ResourceLifecycle::Live,
            )
            .add(
                container(
                    "partial",
                    Some(ContainerSecurity {
                        run_as_user: None,
                        ..old_form.clone()
                    }),
                ),
                alien_core::ResourceLifecycle::Live,
            )
            .add(
                container("complete", Some(old_form.clone())),
                alien_core::ResourceLifecycle::Live,
            )
            .build();

        assert!(super::downgrade_container_security(&mut stack));
        let security = |id: &str| {
            stack
                .resources()
                .find(|(rid, _)| rid.as_str() == id)
                .and_then(|(_, entry)| entry.config.downcast_ref::<alien_core::Container>())
                .expect("container")
                .security
                .clone()
        };
        assert_eq!(security("optout"), None);
        assert_eq!(security("partial"), None);
        assert_eq!(security("complete"), Some(old_form));
        assert!(!super::downgrade_container_security(&mut stack));
    }

    #[test]
    fn older_operators_get_daemons_and_workers_without_security() {
        use alien_core::{ContainerSecurity, Daemon, DaemonCode, Worker, WorkerCode};
        let security = ContainerSecurity {
            profile: alien_core::ContainerSecurityProfile::Unrestricted,
            run_as_user: None,
            run_as_group: None,
            read_only_root_filesystem: None,
            writable_paths: vec![],
        };
        let mut daemon = Daemon::new("d".to_string())
            .permissions("p".to_string())
            .code(DaemonCode::Image {
                image: "img".to_string(),
            })
            .build();
        daemon.security = Some(security.clone());
        let mut worker = Worker::new("w".to_string())
            .permissions("p".to_string())
            .code(WorkerCode::Image {
                image: "img".to_string(),
            })
            .build();
        worker.security = Some(security);
        let mut stack = alien_core::Stack::new("app".to_string())
            .add(daemon, alien_core::ResourceLifecycle::Live)
            .add(worker, alien_core::ResourceLifecycle::Live)
            .build();
        assert!(super::downgrade_container_security(&mut stack));
        for (_, entry) in stack.resources() {
            if let Some(d) = entry.config.downcast_ref::<Daemon>() {
                assert!(d.security.is_none());
            }
            if let Some(w) = entry.config.downcast_ref::<Worker>() {
                assert!(w.security.is_none());
            }
        }
        assert!(!super::downgrade_container_security(&mut stack));
    }

    fn test_deployment_config() -> DeploymentConfig {
        DeploymentConfig {
            image_registry: None,
            image_pull_secrets: Vec::new(),
            runtime_identity_audience: None,
            gcp_federation_scope: None,
            stored_secret_input_ids: None,
            input_values: Default::default(),
            deployment_name: None,
            stack_settings: StackSettings::default(),
            management_config: None,
            environment_variables: EnvironmentVariablesSnapshot {
                variables: vec![],
                hash: String::new(),
                created_at: String::new(),
            },
            allow_frozen_changes: false,
            setup_run: false,
            setup_run_id: None,
            compute_backend: None,
            external_bindings: ExternalBindings::default(),
            base_platform: None,
            label_domain: None,
            observe_label_selector: None,
            observe_all_namespaces: false,
            public_endpoints: None,
            domain_metadata: None,
            monitoring: None,
            manager_url: None,
            deployment_token: None,
            native_image_host: None,
            volume_restores: Vec::new(),
            value_slots: Vec::new(),
            deletion_policy: Default::default(),
            release_statements: None,
            signed_config: None,
        }
    }

    fn deployment_record_with_state(
        status: &str,
        stack_state: Option<StackState>,
    ) -> DeploymentRecord {
        let now = Utc::now();
        DeploymentRecord {
            supplied_stacks: None,
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
            configuration_revision: None,
            import_source: None,
            setup_method: None,
            setup_metadata: None,
            setup_target: None,
            setup_fingerprint: None,
            setup_fingerprint_version: None,
            user_environment_variables: None,
            management_config: None,
            deployment_token: None,
            input_values: Default::default(),
            deployment_config: None,
            retry_requested: false,
            locked_by: None,
            locked_at: None,
            created_at: now,
            updated_at: Some(now),
            error: None,
        }
    }

    #[test]
    fn off_platform_inputs_do_not_remove_independent_environment_values() {
        let input = serde_json::from_value(serde_json::json!({
            "id": "logLevel", "kind": "string", "providedBy": ["deployer"],
            "required": true, "label": "Log level", "description": "Logging verbosity",
            "platforms": ["aws"], "env": [{"name": "LOG_LEVEL"}]
        }))
        .unwrap();
        let stack = Stack::new("logging".into()).inputs(vec![input]).build();
        let mut deployment = deployment_record_with_state("running", None);
        let variable = EnvironmentVariable {
            name: "LOG_LEVEL".into(),
            value: "debug".into(),
            var_type: alien_core::EnvironmentVariableType::Plain,
            target_resources: None,
        };
        deployment.user_environment_variables = Some(vec![variable.clone()]);
        let resolved =
            crate::loops::deployment::resolve_input_environment(&stack, &deployment).unwrap();
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].name, variable.name);
        assert_eq!(resolved[0].value, variable.value);
    }

    /// The reported case: a control plane supplies the config and stacks, the
    /// manager's loop starts the update, and `alien deploy` acquires it. The
    /// acquired config must be the one the loop runs with, or the second
    /// runner rolls every workload again without the manager's variables.
    #[tokio::test]
    async fn acquired_config_is_the_config_the_manager_loop_runs_with() {
        alien_deployment::set_ephemeral_secrets_digest_key();
        let api = alien_core::Container::new("api".to_string())
            .code(alien_core::ContainerCode::Image {
                image: "api:latest".to_string(),
            })
            .cpu(alien_core::ResourceSpec {
                min: "0.5".to_string(),
                desired: "1".to_string(),
            })
            .memory(alien_core::ResourceSpec {
                min: "512Mi".to_string(),
                desired: "1Gi".to_string(),
            })
            .port(8080)
            .permissions("execution".to_string())
            .commands_enabled(true)
            .build();
        let stack = Stack::new("app".to_string())
            .add(api, alien_core::ResourceLifecycle::Live)
            .build();
        let user_variable = alien_core::EnvironmentVariable {
            name: "LOG_LEVEL".to_string(),
            value: "debug".to_string(),
            var_type: alien_core::EnvironmentVariableType::Plain,
            target_resources: None,
        };
        // The control plane's own config: its copy of the user variables only.
        let mut control_plane_config = test_deployment_config();
        control_plane_config.environment_variables.variables = vec![user_variable.clone()];
        let mut record = deployment_record_with_state("update-pending", None);
        record.desired_release_id = Some("rel_next".to_string());
        record.deployment_token = Some("ax_dep_test".to_string());
        record.user_environment_variables = Some(vec![user_variable]);
        record.deployment_config = Some(control_plane_config);
        record.supplied_stacks = Some(crate::traits::deployment_store::SuppliedStacks {
            current: None,
            desired: Some(stack.clone()),
        });

        let dir = tempfile::tempdir().expect("tempdir");
        let db = std::sync::Arc::new(
            crate::stores::sqlite::SqliteDatabase::new(
                &dir.path().join("manager.db").to_string_lossy(),
            )
            .await
            .expect("database"),
        );
        let token_store = crate::stores::sqlite::SqliteTokenStore::new(db.clone());
        let log_buffer = crate::dev::LogBuffer::new();
        let release_store = crate::stores::sqlite::SqliteReleaseStore::new(db);
        let manager_config = crate::config::ManagerConfig {
            base_url: Some("https://manager.example.test".to_string()),
            response_signing_key: b"acquire-signing-key".to_vec(),
            ..Default::default()
        };
        let credential_resolver = crate::providers::local_credentials::LocalCredentialResolver::new(
            dir.path().to_path_buf(),
        );
        let sources = RunnerConfigSources {
            config: &manager_config,
            credential_resolver: &credential_resolver,
            bindings_provider: &None,
            target_bindings_providers: &HashMap::new(),
            commands_keys: Some(crate::auth::commands_key::CommandsKeyStores {
                signing_key: &manager_config.response_signing_key,
                token_store: &token_store,
                log_buffer: &log_buffer,
            }),
        };

        let response = acquired_deployment_response(
            &release_store,
            None,
            &sources,
            &crate::traits::deployment_store::AcquiredDeployment {
                deployment: record.clone(),
                execution_claim: None,
            },
        )
        .await
        .expect("acquire response");
        let loop_config = sources
            .deployment_config(&record, Some(&stack), None)
            .await
            .expect("loop config");

        // The snapshot's creation time is the only field allowed to differ.
        let without_created_at = |mut config: Value| {
            config["environmentVariables"]["createdAt"] = Value::Null;
            config
        };
        let acquired = without_created_at(response.deployment["deploymentConfig"].clone());
        assert_eq!(
            acquired,
            without_created_at(serde_json::to_value(&loop_config).expect("loop config json"))
        );
        let names: Vec<&str> = acquired["environmentVariables"]["variables"]
            .as_array()
            .expect("variables")
            .iter()
            .map(|variable| variable["name"].as_str().expect("name"))
            .collect();
        assert_eq!(
            names,
            vec![
                alien_core::ENV_ALIEN_DEPLOYMENT_ID,
                alien_core::ENV_ALIEN_DEPLOYMENT_NAME,
                alien_core::ENV_ALIEN_COMMANDS_URL,
                alien_core::ENV_ALIEN_COMMANDS_TOKEN,
                alien_core::ENV_ALIEN_COMMANDS_TARGET_RESOURCE_TYPE,
                alien_core::ENV_ALIEN_COMMANDS_TARGET_RESOURCE_ID,
                "LOG_LEVEL",
            ]
        );
    }

    /// A runner that acquires a deployment runs with its generated secret
    /// input: created on the manager when none exists, the same one every time.
    #[tokio::test]
    async fn an_acquired_config_carries_the_generated_secret_input() {
        let mut stack = Stack::new("app".to_string()).build();
        stack.inputs = vec![alien_core::StackInputDefinition {
            id: "signingKey".to_string(),
            kind: alien_core::StackInputKind::Secret,
            when: None,
            provided_by: vec![alien_core::StackInputProvider::Developer],
            required: true,
            label: "Signing key".to_string(),
            description: "Key the service signs internal tokens with.".to_string(),
            placeholder: None,
            default: None,
            platforms: None,
            validation: None,
            generate: Some(alien_core::StackInputGenerate { length: 32 }),
            env: vec![],
        }];
        let mut record = deployment_record_with_state("update-pending", None);
        record.desired_release_id = Some("rel_next".to_string());
        record.deployment_config = Some(test_deployment_config());
        record.supplied_stacks = Some(crate::traits::deployment_store::SuppliedStacks {
            current: None,
            desired: Some(stack),
        });
        let dir = tempfile::tempdir().expect("tempdir");
        let db = std::sync::Arc::new(
            crate::stores::sqlite::SqliteDatabase::new(
                &dir.path().join("manager.db").to_string_lossy(),
            )
            .await
            .expect("database"),
        );
        let release_store = crate::stores::sqlite::SqliteReleaseStore::new(db);
        let values = crate::values::fixtures::kv_store(&dir.path().join("values")).await;
        let manager_config = crate::config::ManagerConfig::default();
        let credential_resolver = crate::providers::local_credentials::LocalCredentialResolver::new(
            dir.path().to_path_buf(),
        );
        let sources = RunnerConfigSources {
            config: &manager_config,
            credential_resolver: &credential_resolver,
            bindings_provider: &None,
            target_bindings_providers: &HashMap::new(),
            commands_keys: None,
        };
        let acquired = crate::traits::deployment_store::AcquiredDeployment {
            deployment: record,
            execution_claim: None,
        };

        let first =
            acquired_deployment_response(&release_store, Some(&values), &sources, &acquired)
                .await
                .expect("first acquire");
        let second =
            acquired_deployment_response(&release_store, Some(&values), &sources, &acquired)
                .await
                .expect("second acquire");

        let key = &first.deployment["deploymentConfig"]["inputValues"]["signingKey"];
        assert_eq!(key.as_str().map(str::len), Some(32), "{key}");
        assert_eq!(
            key, &second.deployment["deploymentConfig"]["inputValues"]["signingKey"],
            "the same value on every acquire"
        );
        let scope = crate::values::ValueScope::deployment("dep_test").expect("scope");
        let stored = values
            .get(&scope, &"input:signingKey".parse().expect("slot"))
            .await
            .expect("store read")
            .expect("generated on the manager");
        assert_eq!(serde_json::from_str::<Value>(&stored.value).unwrap(), *key);
    }

    #[test]
    fn a_report_from_the_operator_does_not_erase_the_recorded_setup_refusal() {
        let mut stored = deployment_record_with_state("update-failed", None);
        stored.runtime_metadata = Some(RuntimeMetadata {
            setup_report_refusal: Some(alien_core::SetupReportRefusal {
                reason: "stale chart".into(),
                reported_release_id: "rel_1".into(),
                at: chrono::Utc::now(),
            }),
            ..RuntimeMetadata::default()
        });
        let mut incoming = uninitialized_state();
        incoming.runtime_metadata = Some(RuntimeMetadata::default());
        preserve_setup_update_authorization(&mut incoming, &stored);
        assert_eq!(
            incoming
                .runtime_metadata
                .unwrap()
                .setup_report_refusal
                .map(|refusal| refusal.reason),
            Some("stale chart".to_string())
        );
    }

    #[test]
    fn a_cleared_refusal_and_a_newer_release_are_not_restored_from_the_operator() {
        let stored_release = alien_core::InstalledHelmRelease {
            name: "shop".into(),
            namespace: "apps".into(),
        };
        let mut stored = deployment_record_with_state("running", None);
        stored.runtime_metadata = Some(RuntimeMetadata {
            installed_helm_release: Some(stored_release.clone()),
            ..RuntimeMetadata::default()
        });
        // The Operator saved an earlier checkpoint: it still carries a refusal
        // the manager has cleared and an older release name.
        let mut incoming = uninitialized_state();
        incoming.runtime_metadata = Some(RuntimeMetadata {
            setup_report_refusal: Some(alien_core::SetupReportRefusal {
                reason: "stale chart".into(),
                reported_release_id: "rel_1".into(),
                at: chrono::Utc::now(),
            }),
            installed_helm_release: Some(alien_core::InstalledHelmRelease {
                name: "old".into(),
                namespace: "apps".into(),
            }),
            ..RuntimeMetadata::default()
        });
        preserve_setup_update_authorization(&mut incoming, &stored);
        let metadata = incoming.runtime_metadata.unwrap();
        assert!(metadata.setup_report_refusal.is_none());
        assert_eq!(
            metadata.installed_helm_release.map(|release| release.name),
            Some(stored_release.name)
        );
    }

    #[test]
    fn a_report_from_the_operator_does_not_erase_the_recorded_helm_release() {
        let mut stored = deployment_record_with_state("running", None);
        stored.runtime_metadata = Some(RuntimeMetadata {
            installed_helm_release: Some(alien_core::InstalledHelmRelease {
                name: "shop".into(),
                namespace: "apps".into(),
            }),
            ..RuntimeMetadata::default()
        });
        let mut incoming = uninitialized_state();
        incoming.runtime_metadata = Some(RuntimeMetadata::default());
        preserve_setup_update_authorization(&mut incoming, &stored);
        assert_eq!(
            incoming
                .runtime_metadata
                .unwrap()
                .installed_helm_release
                .map(|release| (release.name, release.namespace)),
            Some(("shop".to_string(), "apps".to_string()))
        );
    }

    #[test]
    fn a_report_without_the_minted_authorization_does_not_erase_it_until_the_update_ran() {
        let authorization = alien_core::SetupUpdateAuthorization {
            nonce: "n".into(),
            baseline_frozen_digest: "a".into(),
            target_frozen_digest: "b".into(),
            release_id: "rel_2".into(),
            setup_target: "helm".into(),
            setup_fingerprint: "fp".into(),
            setup_fingerprint_version: 1,
        };
        let mut stored = deployment_record_with_state("update-failed", None);
        stored.runtime_metadata = Some(RuntimeMetadata {
            setup_update_authorization: Some(authorization.clone()),
            ..RuntimeMetadata::default()
        });
        let release = |id: &str| alien_core::ReleaseInfo {
            release_id: Some(id.to_string()),
            version: None,
            description: None,
            stack: alien_core::Stack::new("s".to_string()).build(),
            signed: None,
            signer_lists: Vec::new(),
        };

        let mut waiting = uninitialized_state();
        waiting.runtime_metadata = Some(RuntimeMetadata::default());
        waiting.current_release = Some(release("rel_1"));
        preserve_setup_update_authorization(&mut waiting, &stored);
        assert_eq!(
            waiting.runtime_metadata.unwrap().setup_update_authorization,
            Some(authorization)
        );

        let mut done = uninitialized_state();
        done.runtime_metadata = Some(RuntimeMetadata::default());
        done.current_release = Some(release("rel_2"));
        preserve_setup_update_authorization(&mut done, &stored);
        assert!(done
            .runtime_metadata
            .unwrap()
            .setup_update_authorization
            .is_none());
    }

    #[test]
    fn preserving_gate_answers_folds_key_by_key_with_the_record_winning() {
        let mut recorded = RuntimeMetadata::default();
        recorded
            .persisted_gate_answers
            .insert("a".to_string(), false);
        recorded
            .persisted_gate_answers
            .insert("b".to_string(), true);
        let mut stored = deployment_record_with_state("running", None);
        stored.runtime_metadata = Some(recorded);

        let mut incoming_metadata = RuntimeMetadata::default();
        incoming_metadata
            .persisted_gate_answers
            .insert("a".to_string(), true);
        incoming_metadata
            .persisted_gate_answers
            .insert("c".to_string(), false);
        let mut incoming = uninitialized_state();
        incoming.runtime_metadata = Some(incoming_metadata);

        let _ = preserve_recorded_gate_answers(&mut incoming, &stored, None);

        let answers = &incoming
            .runtime_metadata
            .expect("metadata must survive")
            .persisted_gate_answers;
        assert_eq!(answers.get("a"), Some(&false), "the record wins a flip");
        assert_eq!(answers.get("b"), Some(&true), "a dropped key is restored");
        assert_eq!(
            answers.get("c"),
            Some(&false),
            "an incoming-only key survives the fold"
        );
        assert_eq!(answers.len(), 3);
    }
}

/// The generated secret inputs of the deployment's target release, created if
/// a first step has not made them yet, as the metadata a report carries.
/// Empty when this manager keeps no values, the deployment has no release (or
/// its release is gone), or it is being deleted.
async fn generated_slot_report(
    state: &AppState,
    deployment: &DeploymentRecord,
) -> Result<Vec<crate::values::ValueMeta>, Response> {
    let Some(values) = state.values.as_deref() else {
        return Ok(Vec::new());
    };
    if deployment_is_deleting(deployment) {
        return Ok(Vec::new());
    }
    let Some(release_id) = deployment
        .desired_release_id
        .as_deref()
        .or(deployment.current_release_id.as_deref())
    else {
        return Ok(Vec::new());
    };
    // A report does not depend on its release: one that is gone has nothing to generate.
    let Some(release) = state
        .release_store
        .get_release(&crate::auth::Subject::system(), release_id)
        .await
        .map_err(|e| e.into_response())?
    else {
        return Ok(Vec::new());
    };
    let (_, generated) = crate::values::with_generated_inputs(
        Some(values),
        release.stacks.get(&deployment.platform),
        std::borrow::Cow::Borrowed(deployment),
    )
    .await
    .map_err(|e| e.into_response())?;
    Ok(generated.slots)
}

/// Optional metadata an agent's report carries to the store.
struct AgentReportExtras {
    operator_image: Option<OperatorImageReport>,
    application: Option<ObservedApplicationReport>,
    dynamic_containers: Option<Vec<alien_core::sync::DynamicContainerReport>>,
    operations_config: Option<alien_core::OperationsConfig>,
    deployment_token_revocable: bool,
    generated_slots: Vec<crate::values::ValueMeta>,
}

/// Persist an agent's report. A report that leaves the deployment Running with
/// nothing queued also queues the update to the release it should run now.
async fn reconcile_agent_report(
    state: &AppState,
    subject: &crate::auth::Subject,
    data: ReconcileData,
    extras: AgentReportExtras,
) -> Result<crate::traits::ReconcileOutcome, AlienError> {
    let mut outcome =
        persist_agent_report(state.deployment_store.as_ref(), subject, data, extras).await?;
    crate::release_targeting::retarget_if_settled(
        state.deployment_store.as_ref(),
        state.release_store.as_ref(),
        state.release_channels.as_deref(),
        subject,
        &mut outcome.record,
    )
    .await?;
    Ok(outcome)
}

async fn persist_agent_report(
    store: &dyn crate::traits::DeploymentStore,
    subject: &crate::auth::Subject,
    data: ReconcileData,
    extras: AgentReportExtras,
) -> Result<crate::traits::ReconcileOutcome, AlienError> {
    let AgentReportExtras {
        operator_image,
        application,
        dynamic_containers,
        operations_config,
        deployment_token_revocable,
        generated_slots,
    } = extras;
    if operator_image.is_none()
        && application.is_none()
        && dynamic_containers.is_none()
        && operations_config.is_none()
        && !deployment_token_revocable
        && generated_slots.is_empty()
    {
        return store.reconcile(subject, data).await;
    }
    let mut request = ReconcileInput::builder(data).generated_slots(generated_slots);
    if let Some(operator_image) = operator_image {
        request = request.operator_image(operator_image);
    }
    if let Some(application) = application {
        request = request.application(application);
    }
    if let Some(reports) = dynamic_containers {
        request = request.dynamic_containers(reports);
    }
    if let Some(config) = operations_config {
        request = request.operations_config(config);
    }
    store
        .reconcile_request(
            subject,
            request
                .deployment_token_revocable(deployment_token_revocable)
                .build(),
        )
        .await
}

/// A completed receipt grants no authority to replay a report or take new work.
async fn acknowledge_completed_claim(
    store: &dyn crate::traits::DeploymentStore,
    subject: &crate::auth::Subject,
    request: &AgentSyncRequest,
    error_code: &str,
) -> Result<Option<AgentSyncResponse>, alien_error::AlienError> {
    if !matches!(
        error_code,
        "DEPLOYMENT_LEASE_LOST" | "DEPLOYMENT_UPDATE_CLAIM_LOST"
    ) {
        return Ok(None);
    }
    let Some(claim) = request.execution_claim.as_ref() else {
        return Ok(None);
    };
    if !store
        .acknowledge_completed_execution(subject, &request.deployment_id, &request.session, claim)
        .await?
    {
        return Ok(None);
    }
    Ok(Some(AgentSyncResponse {
        execution_claim: None,
        current_state: None,
        target: None,
        commands_url: None,
        target_operations_bundle_set: None,
        target_dynamic_containers: None,
        target_operator_image: None,
        signed_config: None,
    }))
}

/// Whether the deployment's setup pinned a signing fingerprint.
pub(super) fn is_protected(deployment: &DeploymentRecord) -> bool {
    deployment
        .stack_settings
        .as_ref()
        .is_some_and(|settings| settings.signing.is_some())
}

/// `POST /v1/sync` — Inbound: deployment bearer. The agent-driven sync
/// path; `caller: &Subject` is threaded into the store so embedders see
/// the agent's own scope.
#[cfg_attr(feature = "openapi", utoipa::path(
    post,
    path = "/v1/sync",
    tag = "sync",
    request_body = AgentSyncWireRequest,
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
    Json(AgentSyncWireRequest {
        request: req,
        max_supported_deployment_protocol_version,
        operator_image,
        application,
        dynamic_containers,
        operations_config,
        installed_config_missing,
        pull_secret_refreshed,
        installed_setup,
    }): Json<AgentSyncWireRequest>,
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

    // Handled before the state report below, which needs the record as the
    // report left it. Re-read only when the report changed it.
    let deployment = match installed_setup.as_ref() {
        Some(report)
            if super::installed_setup::handle_installed_setup(
                &state,
                &subject,
                &deployment,
                report,
            )
            .await =>
        {
            match state
                .deployment_store
                .get_deployment(&subject, &req.deployment_id)
                .await
            {
                Ok(Some(d)) => d,
                Ok(None) => {
                    return ErrorData::not_found_deployment(&req.deployment_id).into_response()
                }
                Err(e) => return e.into_response(),
            }
        }
        _ => deployment,
    };

    let supported_protocol = max_supported_deployment_protocol_version
        .unwrap_or(1)
        .min(alien_core::MAX_SUPPORTED_DEPLOYMENT_PROTOCOL_VERSION);
    if deployment.required_deployment_protocol_version() > supported_protocol {
        // The sync is refused, but the deployment still records which Operator
        // version is stuck so the CLI can show it.
        if let Some(operator_version) = req.operator_version.as_deref() {
            if let Err(e) = state
                .deployment_store
                .record_operator_version(&subject, &req.deployment_id, operator_version)
                .await
            {
                tracing::warn!(deployment_id = %req.deployment_id, error = %e, "Failed to record the Operator version of a rejected sync");
            }
        }
        // An Operator that reports its version is told which Operator and chart
        // version to move to; other actors keep the generic text.
        let message = match (&state.charts, req.operator_version.as_deref()) {
            (Some(charts), Some(operator_version)) => format!(
                "This deployment requires a newer deployment protocol. {}",
                charts.operator_upgrade_message(Some(operator_version))
            ),
            _ => "This deployment requires a newer deployment protocol. Update the deployment actor or manager before retrying.".to_string(),
        };
        return ErrorData::bad_request(message).into_response();
    }
    if let Some(reported) = req.current_state.as_ref() {
        let version = reported
            .get("protocolVersion")
            .and_then(serde_json::Value::as_u64);
        if !version
            .and_then(|version| u32::try_from(version).ok())
            .is_some_and(|version| {
                version <= supported_protocol
                    && !(deployment.base_platform.unwrap_or(deployment.platform)
                        == alien_core::Platform::Gcp
                        && deployment.deployment_protocol_version
                            < alien_core::GCP_FEDERATION_DEPLOYMENT_PROTOCOL_VERSION
                        && version != deployment.deployment_protocol_version)
                    && alien_core::accepts_deployment_protocol_report(
                        deployment.required_deployment_protocol_version(),
                        version,
                    )
            })
            && !(version.is_none() && deployment.required_deployment_protocol_version() == 1)
        {
            return ErrorData::bad_request(
                "A deployment report must preserve its registered protocol version",
            )
            .into_response();
        }
    }

    if let Some(operator_image) = &operator_image {
        if let Err(reason) = operator_image.validate() {
            return ErrorData::bad_request(reason).into_response();
        }
    }

    let requires_execution_claims = state.deployment_store.requires_execution_claims();
    let report_has_claim = req.execution_claim.is_some();
    if report_has_claim {
        if req.session.is_empty() {
            return ErrorData::bad_request("session is required with executionClaim")
                .into_response();
        }
        if let Err(error) = state
            .deployment_store
            .renew_lease(
                &subject,
                &req.deployment_id,
                &req.session,
                req.execution_claim.clone(),
            )
            .await
        {
            match acknowledge_completed_claim(
                state.deployment_store.as_ref(),
                &subject,
                &req,
                &error.code,
            )
            .await
            {
                Ok(Some(response)) => return Json(response).into_response(),
                Ok(None) => {}
                Err(ack_error) => return ack_error.into_response(),
            }
            return error.into_response();
        }
    }

    // If the agent reported its current state, persist it to the deployment record.
    // This is how pull-mode agents propagate status changes (e.g. Pending → Running)
    // back to the manager so that API consumers can observe deployment progress.
    let mut ignored_agent_state_report = false;
    let mut reported_claim_is_terminal = false;
    let mut target_operations_bundle_set = None;
    let mut target_dynamic_containers = None;
    let mut reconciled_operator_image = None;
    // A target-bearing row without an echoed claim belongs to work the agent
    // has not accepted yet. Do not let its idle state acknowledge that target.
    let reported_state_owns_target =
        report_has_claim || !requires_execution_claims || deployment.desired_release_id.is_none();
    // The skipped report still proves the agent is connected. Its heartbeat and
    // observations are recorded below against the manager's own state, so an
    // update the agent cannot take yet does not make it look disconnected.
    let unaccepted_target_report = req.current_state.is_some() && !reported_state_owns_target;
    if let Some(current_state_value) = req
        .current_state
        .as_ref()
        .filter(|_| reported_state_owns_target)
    {
        match serde_json::from_value::<DeploymentState>(current_state_value.clone()) {
            Ok(mut agent_state) => {
                reported_claim_is_terminal =
                    report_has_claim && agent_report_completes_claim(&deployment, &agent_state);
                if should_ignore_agent_state_report(&deployment, &agent_state) {
                    ignored_agent_state_report = true;
                    tracing::info!(
                        deployment_id = %req.deployment_id,
                        "Ignoring pull sync state because manager owns the authoritative checkpoint"
                    );
                } else {
                    // Stays below the ignore check: restoring metadata here
                    // would make an uninitialized report look initialized.
                    let _ = preserve_recorded_gate_answers(&mut agent_state, &deployment, None);
                    preserve_setup_update_authorization(&mut agent_state, &deployment);
                    // Reconcile registry access before persisting so the flag
                    // is saved in the same DB write.
                    // Pull sync is the agent's only channel to the manager, so a pending
                    // replica does not refuse it. The record stays incomplete, and the next
                    // sync grants again.
                    if let Err(error) =
                        crate::registry_access::reconcile_registry_access_with_proxy()
                            .bindings_provider(&state.bindings_provider)
                            .target_bindings_providers(&state.target_bindings_providers)
                            .deployment_id(&req.deployment_id)
                            .project_id(&deployment.project_id)
                            .state(&mut agent_state)
                            .proxy_url(&state.config.base_url())
                            .supported_aws_regions(&state.config.supported_aws_regions)
                            .protected(is_protected(&deployment))
                            .call()
                            .await
                    {
                        tracing::warn!(
                            deployment_id = %req.deployment_id,
                            error = %error,
                            "Registry replica not ready; the next sync grants again"
                        );
                    }

                    let mut saved_state = agent_state.clone();
                    if let Err(error) = redact_reported_values(
                        &state,
                        &deployment,
                        saved_state.stack_state.as_mut(),
                    )
                    .await
                    {
                        return error.into_response();
                    }
                    let generated_slots = match generated_slot_report(&state, &deployment).await {
                        Ok(slots) => slots,
                        Err(response) => return response,
                    };
                    let deployment_token_revocable = pull_secret_refreshed;
                    let reconcile_data = ReconcileData {
                        deployment_id: req.deployment_id.clone(),
                        session: req.session.clone(),
                        state: saved_state,
                        update_heartbeat: true,
                        heartbeats: req.heartbeats.clone(),
                        observed_inventory_batches: req.observed_inventory_batches.clone(),
                        capabilities: req.capabilities.clone(),
                        operator_version: req.operator_version.clone(),
                        suggested_delay_ms: None,
                        execution_claim: req.execution_claim.clone(),
                        operations_report: req.operations_report.clone(),
                    };
                    let reconcile_result = reconcile_agent_report(
                        &state,
                        &subject,
                        reconcile_data,
                        AgentReportExtras {
                            operator_image: operator_image.clone(),
                            application: application.clone(),
                            dynamic_containers: dynamic_containers.clone(),
                            operations_config: operations_config.clone(),
                            deployment_token_revocable,
                            generated_slots,
                        },
                    )
                    .await;

                    match reconcile_result {
                        Err(e) => {
                            if report_has_claim {
                                match acknowledge_completed_claim(
                                    state.deployment_store.as_ref(),
                                    &subject,
                                    &req,
                                    &e.code,
                                )
                                .await
                                {
                                    Ok(Some(response)) => return Json(response).into_response(),
                                    Ok(None) => {}
                                    Err(ack_error) => return ack_error.into_response(),
                                }
                                return e.into_response();
                            }
                            tracing::warn!(deployment_id = %req.deployment_id, error = %e, "Failed to reconcile agent-reported state");
                        }
                        Ok(outcome) => {
                            settle_deployment_token(
                                &state,
                                &req.deployment_id,
                                &subject,
                                &deployment,
                                &outcome.record,
                                deployment_token_revocable,
                            )
                            .await;
                            settle_commands_key(&state, &deployment).await;
                            target_operations_bundle_set = outcome.target_operations_bundle_set;
                            target_dynamic_containers = outcome.target_dynamic_containers;
                            reconciled_operator_image = outcome.target_operator_image;
                            if let Err(error) =
                                crate::registry_access::cleanup_deleted_registry_access(
                                    state.deployment_store.as_ref(),
                                    &state.bindings_provider,
                                    &state.target_bindings_providers,
                                    &req.deployment_id,
                                    &deployment.project_id,
                                    &agent_state,
                                )
                                .await
                            {
                                return error.into_response();
                            }
                        }
                    }
                }
            }
            Err(e) => {
                if report_has_claim {
                    return ErrorData::bad_request(format!(
                        "currentState is invalid for the active execution claim: {e}"
                    ))
                    .into_response();
                }
                tracing::warn!(
                    deployment_id = %req.deployment_id,
                    error = %e,
                    "Failed to deserialize agent current_state"
                );
            }
        }
    }

    if reported_claim_is_terminal {
        if let Err(error) = state
            .deployment_store
            .release(
                &subject,
                &req.deployment_id,
                &req.session,
                req.execution_claim.clone(),
            )
            .await
        {
            return error.into_response();
        }
    }

    let mut deployment = match state
        .deployment_store
        .get_deployment(&subject, &req.deployment_id)
        .await
    {
        Ok(Some(d)) => d,
        Ok(None) => return ErrorData::not_found_deployment(&req.deployment_id).into_response(),
        Err(e) => return e.into_response(),
    };

    // A nonterminal claim remains attached to the same agent session across
    // polls. Releasing it on every heartbeat would turn one execution into a
    // chain of handed-off attempts and briefly make the immutable target
    // available to another executor.
    let mut execution_claim = if report_has_claim && !reported_claim_is_terminal {
        req.execution_claim.clone()
    } else {
        None
    };
    if execution_claim.is_none() && req.supports_execution_claims && !req.session.is_empty() {
        // Ask the store even when the legacy desired-release projection is
        // empty. The authoritative operation may be a same-release config
        // update, deletion, or maintenance attempt.
        let filter = DeploymentFilter {
            deployment_group_id: None,
            name: None,
            deployment_ids: Some(vec![req.deployment_id.clone()]),
            excluded_deployment_ids: Vec::new(),
            max_deployment_protocol_version: Some(supported_protocol),
            statuses: Some(vec![deployment.status.clone()]),
            platforms: Some(vec![deployment.platform]),
            setup_method: None,
            acquire_mode: Some(DeploymentAcquireMode::Runtime),
            deployment_model: Some(DeploymentModel::Pull),
            limit: Some(1),
        };
        match state
            .deployment_store
            .acquire(&subject, &req.session, &filter, 1)
            .await
        {
            Ok(mut acquired) => {
                if let Some(acquired) = acquired.pop() {
                    if let Err(error) = state
                        .deployment_store
                        .renew_lease(
                            &subject,
                            &req.deployment_id,
                            &req.session,
                            acquired.execution_claim.clone(),
                        )
                        .await
                    {
                        return error.into_response();
                    }
                    execution_claim = acquired.execution_claim;
                    deployment = acquired.deployment;
                }
            }
            Err(error) => return error.into_response(),
        }
    } else if requires_execution_claims
        && execution_claim.is_none()
        && deployment.desired_release_id.is_some()
    {
        return ErrorData::bad_request(
            "This deployment has update work; upgrade the Operator to a version that supports execution claims.",
        )
        .into_response();
    }

    // Return target state if deployment needs updating
    // A desired target is executable only when Platform granted the exact
    // operation claim. In particular, a failed operation keeps its desired
    // release for diagnosis but must not be silently retried by a pull agent.
    let may_deliver_target = may_deliver_agent_target(
        &deployment,
        requires_execution_claims,
        execution_claim.is_some() && (!report_has_claim || reported_claim_is_terminal),
    );
    // An Operator that lost the config of its release, or holds one without
    // the commands key its receivers need, gets that release again.
    let resend_installed =
        !may_deliver_target && installed_config_missing && release_to_resend(&deployment).is_some();
    let target = if may_deliver_target || resend_installed {
        let release_id = if resend_installed {
            release_to_resend(&deployment)
        } else {
            deployment_target_release_id(&deployment)
        };
        let release = if let Some(release_id) = release_id {
            let system = crate::auth::Subject::system();
            match state.release_store.get_release(&system, release_id).await {
                Ok(Some(r)) => Some(r),
                _ => None,
            }
        } else {
            None
        };

        // Reused for image pull auth: no new tokens created.
        let agent_token = headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.strip_prefix("Bearer "))
            .map(|t| t.to_string());
        match release {
            Some(r) => {
                let management_config = match pull_management_config(&state, &deployment).await {
                    Ok(management_config) => management_config,
                    Err(error) => {
                        return save_pull_render_failure(
                            &state,
                            &subject,
                            &deployment,
                            &r,
                            &req.session,
                            execution_claim,
                            error,
                        )
                        .await
                    }
                };
                match build_pull_target(
                    &state,
                    &deployment,
                    r,
                    management_config,
                    agent_token,
                    req.supports_pod_security,
                )
                .await
                {
                    Ok(target) => Some(target),
                    Err(response) => return response,
                }
            }
            None => None,
        }
    } else {
        None
    };

    // Return the seeded state before an unclaimed first target starts, and
    // restore authoritative state when an agent reports an empty snapshot.
    let unclaimed_initial_state = agent_needs_initial_state_hydration(
        &deployment,
        req.current_state.as_ref(),
        report_has_claim,
    );
    let agent_state_uninitialized = req
        .current_state
        .as_ref()
        .and_then(|value| serde_json::from_value::<DeploymentState>(value.clone()).ok())
        .is_some_and(|state| agent_state_is_uninitialized(&state));
    // A new pull actor must observe the persisted protocol before executing new
    // controller states locally. Also acknowledge retries after a lost response.
    let should_return_current_state = (supported_protocol
        >= alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION
        && deployment.deployment_protocol_version
            == alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION)
        || should_return_current_state_for_agent_sync(
            ignored_agent_state_report || unclaimed_initial_state,
            agent_state_uninitialized,
            &deployment,
        );
    // Without a claim, the agent's liveness and observations are recorded
    // against the manager's own state, never the agent's report.
    let record_unclaimed_observations = execution_claim.is_none()
        && !runtime_cleanup_has_finished(&deployment)
        && (unaccepted_target_report
            || (should_return_current_state
                && (!req.heartbeats.is_empty()
                    || !req.observed_inventory_batches.is_empty()
                    || !req.capabilities.is_empty()
                    || req.operator_version.is_some()
                    || operator_image.is_some()
                    || application.is_some()
                    || operations_config.is_some()
                    || req.operations_report.is_some())));
    let current_state = if should_return_current_state || record_unclaimed_observations {
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
            Some(deployment_state) => {
                if record_unclaimed_observations {
                    let generated_slots = match generated_slot_report(&state, &deployment).await {
                        Ok(slots) => slots,
                        Err(response) => return response,
                    };
                    let deployment_token_revocable = pull_secret_refreshed;
                    let reconcile_data = ReconcileData {
                        deployment_id: req.deployment_id.clone(),
                        session: "agent-sync".to_string(),
                        state: deployment_state.clone(),
                        update_heartbeat: true,
                        heartbeats: req.heartbeats.clone(),
                        observed_inventory_batches: req.observed_inventory_batches.clone(),
                        capabilities: req.capabilities.clone(),
                        operator_version: req.operator_version.clone(),
                        suggested_delay_ms: None,
                        execution_claim: None,
                        operations_report: req.operations_report.clone(),
                    };
                    let reconcile_result = reconcile_agent_report(
                        &state,
                        &subject,
                        reconcile_data,
                        AgentReportExtras {
                            operator_image: operator_image.clone(),
                            application: application.clone(),
                            dynamic_containers: dynamic_containers.clone(),
                            operations_config: operations_config.clone(),
                            deployment_token_revocable,
                            generated_slots,
                        },
                    )
                    .await;

                    match reconcile_result {
                        Err(e) => {
                            tracing::warn!(
                                deployment_id = %req.deployment_id,
                                error = %e,
                                "Failed to reconcile agent-reported heartbeats"
                            );
                        }
                        Ok(outcome) => {
                            target_operations_bundle_set = outcome.target_operations_bundle_set;
                            target_dynamic_containers = outcome.target_dynamic_containers;
                            reconciled_operator_image = outcome.target_operator_image;
                        }
                    }
                }

                if !should_return_current_state {
                    None
                } else {
                    match serde_json::to_value(deployment_state) {
                        Ok(v) => Some(v),
                        Err(e) => {
                            tracing::warn!("Failed to serialize deployment state: {e}");
                            return ErrorData::internal("Failed to serialize deployment state")
                                .into_response();
                        }
                    }
                }
            }
            None => None,
        }
    } else {
        None
    };

    // A protected deployment's newest signed config travels with every
    // sync. Unprotected deployments don't touch the signing store at all.
    let signed_config = match &state.signing {
        Some(signing) if is_protected(&deployment) => {
            match signing.latest_config_statement(&deployment.id).await {
                Ok(config) => config,
                Err(error) => return error.into_response(),
            }
        }
        _ => None,
    };
    // A protected install runs no dynamic containers, so their environment is
    // not loaded for it; the field naming the values never reaches the Operator.
    if let (Some(targets), true) = (&mut target_dynamic_containers, is_protected(&deployment)) {
        for target in targets.iter_mut() {
            target.value_slots = None;
        }
    } else if let Some(targets) = &mut target_dynamic_containers {
        match state.values.as_deref() {
            Some(values) => {
                if let Err(error) =
                    crate::values::load_container_env(values, &req.deployment_id, targets).await
                {
                    return error.into_response();
                }
            }
            // Without a store nothing can supply named values, so a container that names any
            // would start without them. The field is never passed on to the Operator.
            None => {
                for target in targets.iter_mut() {
                    let named = target.value_slots.take();
                    if !target.deleted && named.is_some_and(|slots| !slots.is_empty()) {
                        return ErrorData::internal(format!(
                            "Container '{}' names stored values, but this manager has no value store",
                            target.name
                        ))
                        .into_response();
                    }
                }
            }
        }
    }

    Json(AgentSyncResponse {
        execution_claim,
        current_state,
        target: match target.map(|t| serde_json::to_value(&t)).transpose() {
            Ok(mut value) => {
                if subject.role == crate::auth::Role::OperatorSync {
                    if let Some(config) = value.as_mut().and_then(|target| target.get_mut("config"))
                    {
                        if let Err(error) =
                            remove_legacy_runtime_credentials(config, &state.config.base_url())
                        {
                            return error.into_response();
                        }
                    }
                }
                value
            }
            Err(e) => {
                tracing::warn!("Failed to serialize target deployment: {e}");
                return ErrorData::internal("Failed to serialize target deployment")
                    .into_response();
            }
        },
        commands_url: Some(state.config.commands_base_url()),
        target_operations_bundle_set,
        target_dynamic_containers,
        // Charts this manager serves name the image. A manager without charts
        // uses the one its store reported for this deployment.
        target_operator_image: state
            .charts
            .as_ref()
            .map(|charts| charts.deployed_operator_image(&state.config.base_url()))
            .or(reconciled_operator_image),
        signed_config,
    })
    .into_response()
}

/// Answers a sync whose target can't render because the management identity
/// doesn't resolve. Under the execution claim this sync acquired, the failure
/// is saved in the phase's failed status, so it shows on the deployment and the
/// target isn't offered again until a retry; without one (an Operator that
/// predates claims) the Operator gets the error on each sync.
async fn save_pull_render_failure(
    state: &AppState,
    subject: &crate::auth::Subject,
    deployment: &DeploymentRecord,
    release: &crate::traits::ReleaseRecord,
    session: &str,
    execution_claim: Option<crate::traits::deployment_store::ExecutionClaim>,
    error: AlienError,
) -> Response {
    let Some(execution_claim) = execution_claim else {
        return error.into_response();
    };
    let Some(status) = deployment_status_from_record(&deployment.status) else {
        return error.into_response();
    };
    let failed_state = crate::loops::deployment::failed_state_for_phase_error(
        deployment,
        status,
        release
            .stacks
            .get(&release_stack_platform(deployment.platform)),
        Some(&release.id),
        error.clone(),
    );
    let saved = state
        .deployment_store
        .reconcile(
            subject,
            ReconcileData {
                deployment_id: deployment.id.clone(),
                session: session.to_string(),
                state: failed_state,
                update_heartbeat: false,
                suggested_delay_ms: None,
                heartbeats: Vec::new(),
                observed_inventory_batches: Vec::new(),
                capabilities: Vec::new(),
                operator_version: None,
                execution_claim: Some(execution_claim.clone()),
                operations_report: None,
            },
        )
        .await;
    if let Err(save_error) = saved {
        tracing::error!(
            deployment_id = %deployment.id,
            render_error = %error,
            error = %save_error,
            "Failed to save a pull deployment's render failure"
        );
        return save_error.into_response();
    }
    if let Err(release_error) = state
        .deployment_store
        .release(subject, &deployment.id, session, Some(execution_claim))
        .await
    {
        return release_error.into_response();
    }
    error.into_response()
}

/// The target a pull deployment converges to for `release`: the release's
/// stack for the deployment's platform and the deployment's configuration.
/// Sync delivers it to connected Operators; air-gapped bundles carry it.
/// Drop `security` settings an Operator that predates pod security profiles
/// cannot parse (it would reject the whole target, including its own
/// self-update). Such Operators already apply no security context unless a
/// container declares the complete old form, so removing the rest keeps their
/// behavior as it was. Returns whether anything was removed.
fn downgrade_container_security(stack: &mut alien_core::Stack) -> bool {
    use alien_core::ContainerSecurityProfile;
    let mut removed = false;
    for (_, entry) in stack.resources_mut() {
        if let Some(container) = entry.config.downcast_mut::<alien_core::Container>() {
            let readable = container.security.as_ref().is_none_or(|security| {
                security.profile == ContainerSecurityProfile::Restricted
                    && security.run_as_user.is_some()
                    && security.run_as_group.is_some()
                    && security.read_only_root_filesystem.is_some()
            });
            if !readable {
                container.security = None;
                removed = true;
            }
        } else if let Some(daemon) = entry.config.downcast_mut::<alien_core::Daemon>() {
            // Older Operators have no `security` field on daemons or workers.
            removed |= daemon.security.take().is_some();
        } else if let Some(worker) = entry.config.downcast_mut::<alien_core::Worker>() {
            removed |= worker.security.take().is_some();
        }
    }
    removed
}

/// The management identity a pull deployment's target renders with; see
/// [`crate::loops::deployment::resolve_deployment_management_config`] and
/// [`ManagementRenderer::PullTarget`].
pub(crate) async fn pull_management_config(
    state: &AppState,
    deployment: &DeploymentRecord,
) -> Result<Option<alien_core::ManagementConfig>, AlienError> {
    crate::loops::deployment::resolve_deployment_management_config(
        state.credential_resolver.as_ref(),
        deployment,
        management_platform(deployment.platform, deployment.base_platform),
        ManagementRenderer::PullTarget,
    )
    .await
}

/// `management_config` comes from [`pull_management_config`].
pub(crate) async fn build_pull_target(
    state: &AppState,
    deployment: &DeploymentRecord,
    r: crate::traits::ReleaseRecord,
    management_config: Option<alien_core::ManagementConfig>,
    agent_token: Option<String>,
    operator_supports_pod_security: bool,
) -> Result<TargetDeployment, Response> {
    let release_stack_platform = release_stack_platform(deployment.platform);
    let management_platform = management_platform(deployment.platform, deployment.base_platform);

    // Image pull credentials are no longer passed through the sync response.
    // Pull-model agents pull images through the manager's /v2/ registry proxy.
    // Push-model Azure Container Apps also use the proxy (they support any registry).
    // Only AWS Lambda and GCP Cloud Run pull directly from native registries.

    let manager_url = state.config.base_url();

    // A protected install takes its variables and inputs from its signed
    // config and its own inputs, so nothing from the value store is loaded
    // for it.
    let protected = is_protected(deployment);
    let stored_record = deployment;
    let held = if protected {
        std::borrow::Cow::Borrowed(deployment)
    } else {
        crate::values::hydrate_held_values(state.values.as_deref(), deployment)
            .await
            .map_err(|e| e.into_response())?
    };
    let deployment: &DeploymentRecord = &held;

    // Derive native image host for Lambda/Cloud Run so controllers
    // can resolve proxy URIs to native ECR/GAR URIs.
    let native_image_host = crate::registry_access::derive_native_image_host(
        &state.bindings_provider,
        &state.target_bindings_providers,
        &management_platform,
    )
    .await;

    let mut stack = match r.stacks.get(&release_stack_platform) {
        Some(s) => s.clone(),
        None => {
            return Err(ErrorData::internal(format!(
                "Release {} does not contain a stack for platform {}",
                r.id, release_stack_platform
            ))
            .into_response());
        }
    };

    if !operator_supports_pod_security && downgrade_container_security(&mut stack) {
        tracing::info!(
            deployment_id = %deployment.id,
            release_id = %r.id,
            "Operator predates pod security profiles; delivering the release without the settings it cannot read. Upgrade the Operator to apply them."
        );
    }

    let held = if protected {
        held
    } else {
        crate::values::with_generated_inputs(state.values.as_deref(), Some(&stack), held)
            .await
            .map_err(|e| e.into_response())?
            .0
    };
    let deployment: &DeploymentRecord = &held;

    if !protected {
        crate::values::HeldValues::of_step(
            state.values.as_deref(),
            &state.config,
            stored_record,
            deployment,
            Some(&stack),
        )
        .await
        .map_err(|e| e.into_response())?;
    }

    let mut env_vars = match crate::loops::deployment::resolve_input_environment(&stack, deployment)
    {
        Ok(variables) => variables,
        Err(error) => {
            return Err(ErrorData::bad_request(format!(
                "Deployment input values do not resolve for release {}: {}",
                r.id, error.message
            ))
            .into_response())
        }
    };
    // A command receiver built before capability files cannot read the
    // capability file, so it starts only with `ALIEN_COMMANDS_TOKEN`. An
    // enrolled Operator holds a short-lived session credential that must not
    // reach workloads, and the deployment token ends after the first rollout.
    // Those receivers get the deployment's commands-only key, which does
    // nothing but lease and answer the deployment's commands. The manager
    // revokes it once no release of the deployment has such a receiver.
    let unmarked_receivers = stack.command_receivers_without_capability_file();
    if !unmarked_receivers.is_empty() {
        if let Some(key) =
            crate::auth::commands_key::ensure(state, &deployment.id, &unmarked_receivers).await
        {
            env_vars.extend(unmarked_receivers.into_iter().map(|resource_id| {
                EnvironmentVariable {
                    name: alien_core::ENV_ALIEN_COMMANDS_TOKEN.to_string(),
                    value: key.clone(),
                    var_type: alien_core::EnvironmentVariableType::Secret,
                    target_resources: Some(vec![resource_id]),
                }
            }));
        }
    }

    // Records loaded for sync always carry stack settings; in a
    // handler, answer with a 500 rather than panic-dropping the
    // connection if that invariant is ever broken.
    let stack_settings = match deployment.stack_settings.clone() {
        Some(settings) => settings,
        None => {
            return Err(
                ErrorData::internal("synced deployment is missing stack_settings").into_response(),
            );
        }
    };
    let mut config = build_target_deployment_config(
        &deployment,
        stack_settings,
        management_config,
        env_vars,
        manager_url,
        agent_token,
        native_image_host,
    );
    // Workloads export OTLP to this manager when it forwards
    // telemetry, the same wiring push deployments get. Stored
    // monitoring config from an embedder takes precedence.
    if config.monitoring.is_none() {
        config.monitoring = default_monitoring(&state.config, &deployment);
    }

    // A signed release travels with the project's signer list chain, which
    // the deployment verifies from the fingerprint its setup pinned.
    let signed = r.signed_stacks.get(&release_stack_platform).cloned();
    let signer_lists = match (&signed, &state.signing) {
        (Some(_), Some(signing)) => signing
            .signer_lists(&r.project_id)
            .await
            .map_err(|error| error.into_response())?
            .into_iter()
            .map(|record| record.statement)
            .collect(),
        _ => Vec::new(),
    };
    // The deployment's newest signed config: a protected deployment takes
    // developer inputs and configurable env only from it. Unprotected
    // deployments don't touch the signing store.
    let signed_config = match (&signed, &state.signing) {
        (Some(_), Some(signing)) if is_protected(deployment) => signing
            .latest_config_statement(&deployment.id)
            .await
            .map_err(|error| error.into_response())?,
        _ => None,
    };
    Ok(TargetDeployment {
        signed_config,
        release_info: ReleaseInfo {
            signed,
            signer_lists,
            release_id: Some(r.id),
            version: None,
            description: None,
            stack,
        },
        config,
    })
}

fn default_monitoring(
    config: &crate::config::ManagerConfig,
    deployment: &DeploymentRecord,
) -> Option<alien_core::OtlpConfig> {
    let forwards = config.otlp_endpoint.is_some() || config.enable_local_log_ingest();
    let token = deployment.deployment_token.as_ref()?;
    forwards.then(|| alien_core::OtlpConfig {
        logs_endpoint: format!("{}/v1/logs", config.base_url()),
        logs_auth_header: format!("authorization=Bearer {token}"),
        metrics_endpoint: Some(format!("{}/v1/metrics", config.base_url())),
        metrics_auth_header: Some(format!("authorization=Bearer {token}")),
        resource_attributes: std::collections::HashMap::from([(
            "alien.deployment_id".to_string(),
            deployment.id.clone(),
        )]),
    })
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
        .input_values(
            // A stored config from a control plane that predates gate answers
            // has an empty map; the record's persisted answers stand in so
            // gates don't fall back to their declared defaults.
            deployment_config
                .map(|config| config.input_values.clone())
                .filter(|values| !values.is_empty())
                .unwrap_or_else(|| deployment.input_values.clone()),
        )
        .maybe_stored_secret_input_ids(
            deployment_config.and_then(|config| config.stored_secret_input_ids.clone()),
        )
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
        .volume_restores(
            deployment_config
                .map(|config| config.volume_restores.clone())
                .unwrap_or_default(),
        )
        .build()
}

fn management_platform(platform: Platform, base_platform: Option<Platform>) -> Platform {
    base_platform.unwrap_or(platform)
}

pub(crate) fn validate_kubernetes_base_platform(
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
        Platform::Kubernetes | Platform::Machines | Platform::Local | Platform::Test => Err(
            ErrorData::bad_request("basePlatform for kubernetes must be one of aws, gcp, or azure"),
        ),
    }
}

/// Gate answers are lifetime-fixed: fold the stored map into a write arriving
/// through these routes so an actor that predates the field, omits its
/// metadata, or holds a stale copy cannot shrink or flip the record.
/// The resources a stack puts behind each frozen gate, by input id.
fn gate_resources(stack: &alien_core::Stack) -> std::collections::HashMap<String, Vec<String>> {
    let mut gates: std::collections::HashMap<String, Vec<String>> = Default::default();
    for (resource_id, input_id) in alien_core::frozen_gated(stack) {
        gates
            .entry(input_id.to_string())
            .or_default()
            .push(resource_id.clone());
    }
    gates
}

/// Keeps the setup update authorization the manager minted until the Operator
/// has run it. An Operator that has not received the authorization yet reports
/// state without it, and saving that report must not erase the grant. Once the
/// reported current release is the authorized one, the update has run and the
/// grant is dropped.
fn preserve_setup_update_authorization(incoming: &mut DeploymentState, stored: &DeploymentRecord) {
    // The reason a setup report was declined and the Helm release the install
    // reported are the manager's own records; the Operator's copy of the
    // metadata never carries them. The stored values win even when they are
    // empty, so a refusal the manager cleared does not come back from an
    // Operator that saved an earlier checkpoint.
    if let Some(metadata) = incoming.runtime_metadata.as_mut() {
        let stored_metadata = stored.runtime_metadata.as_ref();
        metadata.setup_report_refusal =
            stored_metadata.and_then(|stored| stored.setup_report_refusal.clone());
        metadata.installed_helm_release =
            stored_metadata.and_then(|stored| stored.installed_helm_release.clone());
        metadata.cloud_setup_required =
            stored_metadata.and_then(|stored| stored.cloud_setup_required.clone());
    }
    let Some(authorization) = stored
        .runtime_metadata
        .as_ref()
        .and_then(|metadata| metadata.setup_update_authorization.as_ref())
    else {
        return;
    };
    let Some(metadata) = incoming.runtime_metadata.as_mut() else {
        return;
    };
    let already_ran = incoming
        .current_release
        .as_ref()
        .and_then(|release| release.release_id.as_deref())
        == Some(authorization.release_id.as_str());
    if metadata.setup_update_authorization.is_none() && !already_ran {
        metadata.setup_update_authorization = Some(authorization.clone());
    }
}

fn preserve_recorded_gate_answers(
    incoming: &mut DeploymentState,
    stored: &DeploymentRecord,
    setup_gate_resources: Option<&std::collections::HashMap<String, Vec<String>>>,
) -> Vec<String> {
    // An installed ongoing-management grant permits setup choices to change.
    // Only the stored grant counts: a runtime report cannot confer authority.
    if stored
        .runtime_metadata
        .as_ref()
        .is_some_and(|metadata| metadata.automatic_setup)
    {
        return Vec::new();
    }
    let Some(recorded) = stored
        .runtime_metadata
        .as_ref()
        .map(|metadata| &metadata.persisted_gate_answers)
        .filter(|answers| !answers.is_empty())
    else {
        return Vec::new();
    };
    // Resources the report shows as installed: created, with state recorded.
    let delivered: std::collections::HashSet<String> = incoming
        .stack_state
        .iter()
        .flat_map(|state| state.resources.iter())
        .filter(|(_, resource)| {
            !matches!(
                resource.status,
                alien_core::ResourceStatus::Pending | alien_core::ResourceStatus::Deleted
            ) && resource.internal_state.is_some()
        })
        .map(|(id, _)| id.clone())
        .collect();
    let metadata = incoming.runtime_metadata.get_or_insert_default();
    let mut restored = 0usize;
    let mut pending_setup_yes = Vec::new();
    for (input_id, recorded_answer) in recorded {
        match metadata.persisted_gate_answers.get(input_id) {
            None => {
                metadata
                    .persisted_gate_answers
                    .insert(input_id.clone(), *recorded_answer);
                restored += 1;
            }
            // A setup run's yes replaces a recorded decline, once the report's stack
            // state holds every resource behind the gate as delivered; nothing else does.
            Some(true)
                if !*recorded_answer
                    && setup_gate_resources
                        .and_then(|gates| gates.get(input_id))
                        .is_some_and(|resource_ids| {
                            !resource_ids.is_empty()
                                && resource_ids.iter().all(|id| delivered.contains(id))
                        }) => {}
            Some(incoming_answer) if incoming_answer != recorded_answer => {
                // A setup run asking for yes with delivery incomplete: keep the record, and
                // tell the caller the choice to hand back in the response.
                if *incoming_answer
                    && setup_gate_resources
                        .and_then(|gates| gates.get(input_id))
                        .is_some_and(|resource_ids| !resource_ids.is_empty())
                {
                    pending_setup_yes.push(input_id.clone());
                }
                tracing::warn!(
                    %input_id,
                    deployment_id = %stored.id,
                    recorded = *recorded_answer,
                    incoming = *incoming_answer,
                    "state write-back contradicts a recorded gate answer; keeping the record"
                );
                metadata
                    .persisted_gate_answers
                    .insert(input_id.clone(), *recorded_answer);
            }
            Some(_) => {}
        }
    }
    if restored > 0 {
        tracing::debug!(
            restored,
            deployment_id = %stored.id,
            "restored recorded gate answers a state write-back dropped"
        );
    }
    pending_setup_yes
}

/// Once runtime cleanup hands off, setup owns the remaining identities and
/// checkpoint. Completed retention/deletion also cannot be revived by reports.
fn runtime_cleanup_has_finished(deployment: &DeploymentRecord) -> bool {
    matches!(
        deployment.status.as_str(),
        "retention-required" | "retained" | "teardown-required" | "teardown-failed" | "deleted"
    )
}

fn should_ignore_agent_state_report(
    deployment: &DeploymentRecord,
    agent_state: &DeploymentState,
) -> bool {
    if runtime_cleanup_has_finished(deployment) {
        return true;
    }
    if deployment_is_deleting(deployment) && !agent_state_is_delete_progress(agent_state) {
        return true;
    }

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

fn agent_needs_initial_state_hydration(
    deployment: &DeploymentRecord,
    reported_state: Option<&serde_json::Value>,
    has_execution_claim: bool,
) -> bool {
    !has_execution_claim
        && deployment_has_authoritative_state(deployment)
        && reported_state
            .and_then(|value| serde_json::from_value::<DeploymentState>(value.clone()).ok())
            .as_ref()
            .is_some_and(agent_state_is_uninitialized)
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
    agent_state_uninitialized: bool,
    deployment: &DeploymentRecord,
) -> bool {
    ignored_agent_state_report
        || (agent_state_uninitialized && deployment_has_authoritative_state(deployment))
        || deployment.retry_requested
        || deployment_is_deleting(deployment)
}

fn deployment_is_deleting(deployment: &DeploymentRecord) -> bool {
    matches!(
        deployment_status_from_record(&deployment.status),
        Some(
            DeploymentStatus::DeletePending
                | DeploymentStatus::Deleting
                | DeploymentStatus::DeleteFailed
        )
    )
}

fn agent_state_is_delete_progress(state: &DeploymentState) -> bool {
    matches!(
        state.status,
        DeploymentStatus::DeletePending
            | DeploymentStatus::Deleting
            | DeploymentStatus::DeleteFailed
            | DeploymentStatus::TeardownRequired
            | DeploymentStatus::RetentionRequired
            | DeploymentStatus::Retained
            | DeploymentStatus::TeardownFailed
            | DeploymentStatus::Deleted
    )
}

fn agent_report_completes_claim(_deployment: &DeploymentRecord, state: &DeploymentState) -> bool {
    if matches!(
        state.status,
        DeploymentStatus::PreflightsFailed
            | DeploymentStatus::InitialSetupFailed
            | DeploymentStatus::ProvisioningFailed
            | DeploymentStatus::UpdateFailed
            | DeploymentStatus::RefreshFailed
            | DeploymentStatus::DeleteFailed
            | DeploymentStatus::TeardownFailed
            | DeploymentStatus::Deleted
    ) {
        return true;
    }

    if state.status != DeploymentStatus::Running {
        return false;
    }

    let Some(target_release_id) = state
        .target_release
        .as_ref()
        .and_then(|release| release.release_id.as_deref())
    else {
        // Maintenance has no new release target. Reaching Running completes
        // that claimed refresh attempt.
        return true;
    };
    state
        .current_release
        .as_ref()
        .and_then(|release| release.release_id.as_deref())
        == Some(target_release_id)
}

fn deployment_target_release_id(deployment: &DeploymentRecord) -> Option<&str> {
    if deployment_is_deleting(deployment) {
        deployment
            .desired_release_id
            .as_deref()
            .or(deployment.current_release_id.as_deref())
    } else {
        deployment.desired_release_id.as_deref()
    }
}

fn deployment_needs_target(deployment: &DeploymentRecord) -> bool {
    let Some(target_release_id) = deployment_target_release_id(deployment) else {
        return false;
    };

    if deployment_is_deleting(deployment) {
        return true;
    }

    if deployment.current_release_id.as_deref() != Some(target_release_id) {
        return true;
    }

    !matches!(
        deployment_status_from_record(&deployment.status),
        Some(DeploymentStatus::Running)
    )
}

/// An enrolled Operator reported the end of a rollout. Every workload now
/// runs a revision it built with capability files, so nothing needs the
/// deployment token to run its workloads any more. A refresh (from Running or
/// RefreshFailed) is not a rollout: workloads deployed before enrollment may
/// still use it.
fn rollout_by_enrolled_operator_finished(
    subject: &crate::auth::Subject,
    before: &DeploymentRecord,
    after: &DeploymentRecord,
) -> bool {
    matches!(subject.role, crate::auth::Role::OperatorSync)
        && crate::auth::runtime_capability::is_runtime_capability(&subject.bearer_token)
        && !matches!(before.status.as_str(), "running" | "refresh-failed")
        && after.status == "running"
}

/// Durable mark that an enrolled Operator's first rollout finished while its
/// deployment token waits for the pull-Secret report. A running Operator
/// repeats the report on every sync, so the report itself needs no storage.
fn token_hold_key(deployment_id: &str) -> String {
    format!("deployment-token-held:{deployment_id}")
}

/// Whether the releases the manager itself assigned to the deployment, the
/// current one and the one being rolled out, have a command-enabled Container
/// or Daemon that `alien build` did not record as reading the capability file.
/// Such a receiver starts only with `ALIEN_COMMANDS_TOKEN`, so the
/// deployment's commands key must outlive it.
///
/// The gate never reads the release an Operator reports: whoever holds the
/// Operator's credential could report an older, fully marked release to end
/// the key early. A release that cannot be read, belongs to another project
/// or has no stack for the deployment's platform counts as having one. A
/// deployment with no release has no receiver.
async fn command_receivers_need_commands_key(
    state: &AppState,
    deployment: &DeploymentRecord,
) -> bool {
    let system = crate::auth::Subject::system();
    let current = deployment.current_release_id.as_deref();
    let desired = deployment
        .desired_release_id
        .as_deref()
        .filter(|desired| Some(*desired) != current);
    for release_id in current.into_iter().chain(desired) {
        let release = match state.release_store.get_release(&system, release_id).await {
            Ok(Some(release)) => release,
            Ok(None) => {
                tracing::warn!(deployment_id = %deployment.id, %release_id, "Release not found; keeping the commands key");
                return true;
            }
            Err(error) => {
                tracing::warn!(deployment_id = %deployment.id, %release_id, %error, "Cannot read the release; keeping the commands key");
                return true;
            }
        };
        if release.project_id != deployment.project_id {
            tracing::warn!(deployment_id = %deployment.id, %release_id, "Release belongs to another project; keeping the commands key");
            return true;
        }
        let Some(stack) = release
            .stacks
            .get(&release_stack_platform(deployment.platform))
        else {
            tracing::warn!(deployment_id = %deployment.id, %release_id, "Release has no stack for the deployment's platform; keeping the commands key");
            return true;
        };
        let unmarked = stack.command_receivers_without_capability_file();
        if !unmarked.is_empty() {
            tracing::debug!(
                deployment_id = %deployment.id,
                %release_id,
                resources = ?unmarked,
                "Command receivers built before capability files still need the commands key"
            );
            return true;
        }
    }
    false
}

/// Revoke the deployment's commands key once its current and desired releases
/// have no command receiver built before capability files. Revoking does not
/// stop a receiver that is running: a running receiver is in one of those
/// releases, and none of them needs the key. A deployment that never
/// issued the key costs one store lookup per sync.
async fn settle_commands_key(state: &AppState, deployment: &DeploymentRecord) {
    match crate::auth::commands_key::exists(state, &deployment.id).await {
        Ok(false) => return,
        Ok(true) => {}
        Err(error) => {
            tracing::warn!(deployment_id = %deployment.id, %error, "Cannot look up the commands key");
            return;
        }
    }
    if !command_receivers_need_commands_key(state, deployment).await {
        crate::auth::commands_key::revoke(state, &deployment.id).await;
    }
}

/// Revoke the deployment token once the enrolled Operator's first rollout is
/// done and it has reported that its pull Secret holds its own credential. A
/// Secret that still holds the token would stop the Operator pod from pulling
/// its image on a new node, so the token stays until the report arrives. A
/// command receiver built before capability files starts only with the token,
/// so `deployment_token_revocable` here is already false while the current
/// release has one. An Operator that never reports keeps its token; that is logged
/// once, with the deployment id. Neither case fails the sync.
async fn settle_deployment_token(
    state: &AppState,
    deployment_id: &str,
    subject: &crate::auth::Subject,
    before: &DeploymentRecord,
    after: &DeploymentRecord,
    deployment_token_revocable: bool,
) {
    if !matches!(subject.role, crate::auth::Role::OperatorSync)
        || !crate::auth::runtime_capability::is_runtime_capability(&subject.bearer_token)
    {
        return;
    }
    let key = token_hold_key(deployment_id);
    let rolled_out = rollout_by_enrolled_operator_finished(subject, before, after);
    if !deployment_token_revocable {
        if rolled_out {
            if let Err(error) = state
                .kv
                .put(
                    &key,
                    chrono::Utc::now().timestamp().to_string().into_bytes(),
                    None,
                )
                .await
            {
                tracing::error!(%deployment_id, %error, "Cannot record the held deployment token");
            }
            tracing::warn!(
                %deployment_id,
                "Kept the deployment token: the Operator has not reported a refreshed pull Secret"
            );
        }
        return;
    }
    let held = rolled_out
        || match state.kv.exists(&key).await {
            Ok(held) => held,
            Err(error) => {
                tracing::warn!(%deployment_id, %error, "Cannot read the held deployment token mark");
                false
            }
        };
    if held && revoke_deployment_tokens(state, deployment_id).await {
        if let Err(error) = state.kv.delete(&key, None).await {
            tracing::warn!(%deployment_id, %error, "Cannot clear the held deployment token mark");
        }
    }
}

/// Delete the deployment's long-lived tokens. A failure is logged, returns
/// `false` and is retried on the next sync that reports the pull Secret; it
/// never fails the sync. Deleting a deployment token also retires the unused
/// enrollment codes made from it, in the same transaction (see
/// `SqliteTokenStore::delete_token`).
async fn revoke_deployment_tokens(state: &AppState, deployment_id: &str) -> bool {
    let tokens = match state.token_store.list_tokens().await {
        Ok(tokens) => tokens,
        Err(error) => {
            tracing::warn!(%deployment_id, %error, "Cannot list deployment tokens to revoke");
            return false;
        }
    };
    let mut revoked_all = true;
    for token in tokens.into_iter().filter(|token| {
        token.token_type == crate::traits::token_store::TokenType::Deployment
            && token.deployment_id.as_deref() == Some(deployment_id)
    }) {
        match state.token_store.delete_token(&token.id).await {
            Ok(()) => tracing::info!(
                %deployment_id,
                key_prefix = %token.key_prefix,
                "Revoked the deployment token after the first rollout of the enrolled Operator"
            ),
            Err(error) => {
                revoked_all = false;
                tracing::warn!(%deployment_id, %error, "Cannot revoke a deployment token")
            }
        }
    }
    revoked_all
}

/// The release a deployment runs, when no other work is pending. An Operator
/// that restarted while applying that release (`UpdatePending`, `Updating`)
/// asks for it again like a settled one does: its saved config is the one it
/// cannot use.
fn release_to_resend(deployment: &DeploymentRecord) -> Option<&str> {
    let current = deployment.current_release_id.as_deref()?;
    let holds_release = matches!(
        deployment_status_from_record(&deployment.status),
        Some(
            DeploymentStatus::Running
                | DeploymentStatus::RefreshFailed
                | DeploymentStatus::UpdatePending
                | DeploymentStatus::Updating
        )
    );
    let no_other_target = deployment
        .desired_release_id
        .as_deref()
        .is_none_or(|desired| desired == current);
    (holds_release && no_other_target).then_some(current)
}

fn may_deliver_agent_target(
    deployment: &DeploymentRecord,
    requires_execution_claim: bool,
    has_execution_claim: bool,
) -> bool {
    if requires_execution_claim && deployment.desired_release_id.is_some() {
        has_execution_claim
    } else {
        deployment_needs_target(deployment)
    }
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
    let error = error
        .clone()
        .and_then(|value| serde_json::from_value::<AlienError>(value).ok());
    alien_deployment::deployment_state_error_from_headline(error)
}

fn release_info_from_record(
    release: &ReleaseRecord,
    release_stack_platform: Platform,
) -> Option<ReleaseInfo> {
    Some(ReleaseInfo {
        release_id: Some(release.id.clone()),
        version: None,
        description: None,
        stack: release.stacks.get(&release_stack_platform)?.clone(),
        signed: release.signed_stacks.get(&release_stack_platform).cloned(),
        signer_lists: Vec::new(),
    })
}

/// The signing settings an install reports when it registers. A protected Operator reports
/// the ones it pinned, so the manager sends it the signed config and signer lists. The manager
/// only ever learns protection this way, never gives it up, and the install enforces its own
/// pinned settings whatever is recorded here.
fn reported_signing(
    state: &AppState,
    stack_settings: Option<&alien_core::StackSettings>,
    helm_release: Option<&HelmRelease>,
) -> Result<Option<alien_core::StackSigning>, AlienError<ErrorData>> {
    let Some(signing) = stack_settings.and_then(|settings| settings.signing.as_ref()) else {
        return Ok(None);
    };
    if state.signing.is_none() {
        return Err(ErrorData::bad_request(
            "This manager stores no signer lists, so it can't serve a protected install",
        ));
    }
    let Some(release) = helm_release else {
        return Err(ErrorData::bad_request(
            "An install that reports signing settings names the Helm release they were found for",
        ));
    };
    signing
        .settings(&release.name)
        .map_err(|error| ErrorData::bad_request(error.message))?;
    Ok(Some(signing.clone()))
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

    if req
        .helm_release
        .as_ref()
        .is_some_and(|release| !release.is_valid())
    {
        return ErrorData::bad_request(HELM_RELEASE_ERROR_MESSAGE).into_response();
    }

    match subject.scope.clone() {
        crate::auth::Scope::Deployment { deployment_id, .. } => {
            let deployment = match state
                .deployment_store
                .get_deployment(&subject, &deployment_id)
                .await
            {
                Ok(Some(deployment)) => deployment,
                Ok(None) => return ErrorData::not_found_deployment(&deployment_id).into_response(),
                Err(e) => return e.into_response(),
            };

            Json(InitializeResponse {
                deployment_id,
                deployment_model: super::deployments::deployment_model_for_record(&deployment),
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
            let stack_state = match req.resource_prefix {
                Some(resource_prefix) => {
                    if !alien_core::is_valid_resource_prefix(&resource_prefix) {
                        return ErrorData::bad_request(alien_core::RESOURCE_PREFIX_ERROR_MESSAGE)
                            .into_response();
                    }
                    Some(StackState::with_resource_prefix(platform, resource_prefix))
                }
                None => None,
            };
            let base_platform = match validate_kubernetes_base_platform(platform, req.base_platform)
            {
                Ok(base_platform) => base_platform,
                Err(e) => return e.into_response(),
            };
            let reported_signing = match reported_signing(
                &state,
                req.stack_settings.as_ref(),
                req.helm_release.as_ref(),
            ) {
                Ok(signing) => signing,
                Err(e) => return e.into_response(),
            };

            // Idempotency: if a deployment with this name already exists in the
            // group, issue a fresh deployment token and return the existing ID.
            if let Ok(Some(existing)) = state
                .deployment_store
                .get_deployment_by_name(&subject, &dg_id, &name)
                .await
            {
                // Protection is learned once, when the deployment is created. A later
                // registration that reports other signing settings, or none, doesn't change it.
                if existing
                    .stack_settings
                    .as_ref()
                    .and_then(|settings| settings.signing.as_ref())
                    != reported_signing.as_ref()
                {
                    return ErrorData::bad_request(
                        "Deployment already uses different signing settings",
                    )
                    .into_response();
                }
                if let Some(requested) = &stack_state {
                    if existing
                        .stack_state
                        .as_ref()
                        .map(|state| &state.resource_prefix)
                        != Some(&requested.resource_prefix)
                    {
                        return ErrorData::bad_request(
                            "Deployment already uses a different resource prefix",
                        )
                        .into_response();
                    }
                }
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
                        deployment_model: super::deployments::deployment_model_for_record(
                            &existing,
                        ),
                        deployment_id: existing.id,
                        token: Some(raw_token),
                    })
                    .into_response(),
                    Err(e) => e.into_response(),
                };
            }

            let settings =
                match super::deployments::stack_settings_for_platform(platform, req.stack_settings)
                {
                    Ok(settings) => settings,
                    Err(e) => return e.into_response(),
                };

            // The deployment's own token: what its workloads use for image
            // pulls and telemetry. Never the group token the Operator
            // registered with, which can create deployments.
            let (raw_token, key_prefix, key_hash) =
                ids::generate_token(TokenType::Deployment.prefix());
            let dep_token = Some(raw_token.clone());

            // The release a deployment created here starts on. Install values
            // come from the deployer, who may not choose a generated secret:
            // refuse them before anything is created.
            let starting_release =
                match super::channels::release_for_deployment(&state, &subject, None).await {
                    Ok(release) => release,
                    Err(e) => return e.into_response(),
                };

            // Developer-provided setup on the group applies to every deployment
            // it creates; values supplied by the deployer at install win.
            let group_setup = match state
                .deployment_store
                .get_deployment_group(&subject, &dg_id)
                .await
            {
                Ok(Some(group)) => group.setup,
                Ok(None) => {
                    return AlienError::new(ErrorData::DeploymentGroupNotFound {
                        deployment_group_id: dg_id.clone(),
                    })
                    .into_response();
                }
                Err(e) => return e.into_response(),
            };
            let mut input_values = group_setup.input_values;
            input_values.extend(req.input_values.clone());
            if let Some(stack) = starting_release
                .as_ref()
                .and_then(|release| release.stacks.get(&platform))
            {
                let supplied_values = req
                    .input_values
                    .into_iter()
                    .filter(|(id, _)| {
                        stack
                            .inputs
                            .iter()
                            .find(|input| input.id == *id)
                            .is_none_or(|input| {
                                input.when.is_none()
                                    || input.applies(&stack.inputs, &input_values, platform)
                            })
                    })
                    .collect();
                if let Err(error) = crate::generated_inputs::reject_generated_input_values(
                    &stack.inputs,
                    &supplied_values,
                ) {
                    return error.into_response();
                }
                if let Err(error) =
                    alien_core::resolve_stack_input_values(&stack.inputs, &input_values, platform)
                {
                    return error.into_response();
                }
                let condition_values = input_values.clone();
                input_values.retain(|id, _| {
                    stack
                        .inputs
                        .iter()
                        .find(|input| input.id == *id)
                        .is_none_or(|input| {
                            input.when.is_none()
                                || input.applies(&stack.inputs, &condition_values, platform)
                        })
                });
            }
            let environment_variables = (!group_setup.environment_variables.is_empty())
                .then_some(group_setup.environment_variables);

            // Generated secret inputs get their value from the starting
            // release's stack now, once, and keep it in the stored input
            // values for every later update.
            let initial_release = (req.initial_desired_release == InitialDesiredRelease::Active)
                .then_some(starting_release)
                .flatten();
            if let Some(stack) = initial_release
                .as_ref()
                .and_then(|release| release.stacks.get(&platform))
            {
                crate::generated_inputs::generate_missing_input_values(
                    &stack.inputs,
                    platform,
                    &mut input_values,
                );
            }

            let deployment = match state
                .deployment_store
                .create_deployment(
                    &subject,
                    CreateDeploymentParams {
                        deployment_protocol_version:
                            alien_core::deployment_protocol_version_for_platform(
                                platform,
                                base_platform,
                            ),
                        name,
                        deployment_group_id: dg_id.clone(),
                        platform,
                        base_platform,
                        stack_settings: settings,
                        stack_state,
                        environment_variables,
                        public_subdomain: req.public_subdomain,
                        input_values,
                        setup_item: req.setup_item,
                        deployment_token: dep_token,
                    },
                )
                .await
            {
                Ok(d) => d,
                Err(e) => return e.into_response(),
            };

            // Initialize is the agent's own bootstrap: keep the caller's
            // subject for reads and writes so embedders can authorize
            // against the agent's scope rather than a service credential.
            if let Some(release) = &initial_release {
                if let Err(e) = state
                    .deployment_store
                    .set_deployment_desired_release(&subject, &deployment.id, &release.id)
                    .await
                {
                    return e.into_response();
                }
            }

            // Record the deployment token minted above.
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
                        deployment_model: super::deployments::deployment_model_for_record(
                            &deployment,
                        ),
                        deployment_id: deployment.id,
                        token: Some(raw_token),
                    }),
                )
                    .into_response(),
                Err(e) => e.into_response(),
            }
        }
        crate::auth::Scope::Workspace | crate::auth::Scope::Project { .. } => {
            // Self-hosted flow: the operator creates a deployment, then starts an agent with a
            // workspace or project token. Being listed is not a sync grant, so the agent gets
            // the newest deployment `Authz` lets the caller sync.
            let filter = DeploymentFilter::default();
            let deployments = match state
                .deployment_store
                .list_deployments(&subject, &filter)
                .await
            {
                Ok(deployments) => deployments,
                Err(e) => return e.into_response(),
            };
            // Only deployments the caller may read count, so a 400 reveals nothing it could not
            // list itself.
            if !deployments
                .iter()
                .any(|deployment| state.authz.can_read_deployment(&subject, deployment))
            {
                return ErrorData::bad_request(
                    "No deployments found. Create a deployment before initializing an agent.",
                )
                .into_response();
            }
            let Some(deployment) = deployments
                .iter()
                .find(|deployment| state.authz.can_sync_deployment(&subject, deployment))
            else {
                return ErrorData::forbidden("Caller cannot assign an agent to any deployment")
                    .into_response();
            };
            let deployment_id = deployment.id.clone();
            tracing::info!(
                %deployment_id,
                "Workspace or project token: assigning agent to existing deployment"
            );
            Json(InitializeResponse {
                deployment_model: super::deployments::deployment_model_for_record(deployment),
                deployment_id,
                token: None,
            })
            .into_response()
        }
        crate::auth::Scope::Commands { .. } => {
            ErrorData::forbidden("Command credentials cannot initialize deployments")
                .into_response()
        }
        crate::auth::Scope::RemoteBindings { .. } => {
            ErrorData::forbidden("Remote bindings credentials cannot initialize deployments")
                .into_response()
        }
        crate::auth::Scope::Telemetry { .. } => {
            ErrorData::forbidden("Telemetry credentials cannot initialize deployments")
                .into_response()
        }
    }
}
