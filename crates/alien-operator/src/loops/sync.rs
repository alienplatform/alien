//! Sync loop - syncs state with Manager
//!
//! This loop exchanges state with the Manager:
//! - Reports: current deployment state (includes current_release and target_release)
//! - Receives: target deployment (release info + config)
//!
//! When approval_mode is Manual, new targets create approval records
//! that must be approved before deployment proceeds.

use super::observed_release::{observed_application, single_observed_version};
use crate::db::{Approval, ApprovalStatus};
use crate::OperatorState;
use alien_core::{
    sync::{
        OperatorCapabilityReport, OperatorCapabilityState, OperatorImageReport, SyncInput,
        SyncRequest, SyncResponse,
    },
    DeploymentStatus, ObservedInventoryBatch, Platform,
};
use alien_deployment::run_observe_pass;
use alien_error::{Context, IntoAlienError};
use chrono::Utc;
use reqwest::Client;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, error, info};
use uuid::Uuid;

/// Run the sync loop
///
/// This loop:
/// 1. Gets current state from local database
/// 2. Sends state to Management server
/// 3. Receives target release and config
/// 4. Stores in local database for deployment loop
/// 5. Creates approval record if manual approval is required
pub async fn run_sync_loop(state: Arc<OperatorState>) {
    run_sync_loop_with_command_address_support(state, false, None).await;
}

/// Run the sync loop with capabilities declared by injected runtime loops.
///
/// Kept crate-private so the public `run_sync_loop` API remains compatible;
/// the full operator entry point derives this value from the actual operations
/// receiver before moving it into its task.
pub(crate) async fn run_sync_loop_with_command_address_support(
    state: Arc<OperatorState>,
    operations_command_address_v1: bool,
    operator_image: Option<OperatorImageReport>,
) {
    let interval = Duration::from_secs(state.config.sync_interval_seconds);

    let sync_config = match &state.config.sync {
        Some(config) => config,
        None => {
            error!("Sync configuration not provided, sync loop cannot run");
            return;
        }
    };

    // Create authenticated client
    let client = match create_authenticated_client(&sync_config.token) {
        Ok(c) => c,
        Err(e) => {
            error!(error = %e, "Failed to create authenticated client");
            return;
        }
    };

    info!(
        interval_seconds = state.config.sync_interval_seconds,
        "Starting sync loop"
    );

    loop {
        match sync_with_manager(
            &state,
            &client,
            sync_config.url.as_str(),
            operations_command_address_v1,
            operator_image.as_ref(),
        )
        .await
        {
            Ok(has_update) => {
                if has_update {
                    info!("Received update from manager");
                } else {
                    debug!("Sync complete, no updates");
                }
            }
            Err(e) => {
                error!(error = %e, "Sync failed");
            }
        }

        tokio::select! {
            _ = tokio::time::sleep(interval) => {},
            _ = state.cancel.cancelled() => {
                info!("Sync loop shutting down");
                return;
            }
        }
    }
}

/// Create an authenticated HTTP client
fn create_authenticated_client(token: &str) -> crate::error::Result<Client> {
    use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, USER_AGENT};

    let mut headers = HeaderMap::new();
    let auth_value = format!("Bearer {}", token);
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&auth_value)
            .into_alien_error()
            .context(crate::error::ErrorData::SyncFailed {
                message: "Invalid auth token".to_string(),
            })?,
    );
    headers.insert(USER_AGENT, HeaderValue::from_static("operator"));

    Client::builder()
        .default_headers(headers)
        .build()
        .into_alien_error()
        .context(crate::error::ErrorData::SyncFailed {
            message: "Failed to build HTTP client".to_string(),
        })
}

async fn sync_with_manager(
    state: &OperatorState,
    client: &Client,
    base_url: &str,
    operations_command_address_v1: bool,
    operator_image: Option<&OperatorImageReport>,
) -> crate::error::Result<bool> {
    // Get current deployment state from local database (or create default if not exists)
    let mut deployment_state =
        state
            .db
            .get_deployment_state()
            .await?
            .unwrap_or_else(|| alien_core::DeploymentState {
                platform: state.config.platform,
                status: alien_core::DeploymentStatus::Pending,
                current_release: None,
                target_release: None,
                stack_state: None,
                error: None,
                environment_info: None,
                runtime_metadata: None,
                retry_requested: false,
                protocol_version: alien_core::DEPLOYMENT_PROTOCOL_VERSION,
            });

    // Build the sync request with full deployment state
    // Deployment ID is stored in SQLite (from initialization)
    let deployment_id = state
        .db
        .get_deployment_id()
        .await?
        .expect("deployment_id must be set in online mode");
    let durable_execution = match state.db.get_sync_execution().await? {
        Some(execution) => execution,
        None => {
            let session = format!("pull-operator-{}", Uuid::new_v4());
            state.db.set_sync_execution(&session, None, None).await?;
            crate::db::DurableSyncExecution {
                session,
                claim: None,
                target: None,
            }
        }
    };
    let sync_session = durable_execution.session.clone();
    let execution_claim = durable_execution.claim.clone();
    let heartbeats = state.db.get_pending_heartbeats().await?;
    let mut observed_inventory_batches = state.db.get_pending_observed_inventory_batches().await?;
    let fresh_inventory_batches = if deployment_state.status == DeploymentStatus::Running
        && state.config.observes_environment()
    {
        observe_running_deployment(state, &deployment_id).await?
    } else {
        Vec::new()
    };
    // Built from this tick's observe pass only, so the report never repeats
    // workloads read on an earlier tick.
    let application = observed_application(&fresh_inventory_batches);
    observed_inventory_batches.extend(fresh_inventory_batches);

    // Remote Operator deployments have no Alien-shipped release, so they report the
    // app version as a version-only `current_release`; the platform resolves it to a
    // stackless release and sets `currentReleaseId` — the same channel greenfield
    // uses (where the deploy loop sets `current_release.release_id`). The version is
    // the vendor-configured override if set, else the single version observed on the
    // workloads (e.g. `app.kubernetes.io/version`), so it tracks upgrades.
    if state.config.reports_application_release() && deployment_state.current_release.is_none() {
        let app_version = state
            .config
            .app_version
            .clone()
            .or_else(|| single_observed_version(&observed_inventory_batches));
        if let Some(version) = app_version {
            deployment_state.current_release = Some(alien_core::ReleaseInfo {
                release_id: None,
                version: Some(version),
                description: None,
                stack: alien_core::Stack::new("observed".to_string()).build(),
            });
        }
    }

    // Report what's currently loaded (as of the last `sync_bundles` call) in
    // this request. `sync_bundles` is called again below with THIS response's
    // target, so the report embedded here always reflects one tick's lag —
    // acceptable since `syncing` is a transient, self-resolving state and the
    // next tick reports again regardless of whether this one changed anything.
    let operations_report = match &state.operations_sync_handler {
        Some(handler) => Some(
            handler
                .sync_bundles(state.db.get_target_operations_bundle_set().await?.as_ref())
                .await,
        ),
        None => None,
    };

    let sync_request = SyncRequest {
        deployment_id: deployment_id.clone(),
        session: sync_session.clone(),
        supports_execution_claims: true,
        execution_claim,
        current_state: Some(deployment_state),
        heartbeats,
        observed_inventory_batches,
        capabilities: report_operator_capabilities(state, operations_command_address_v1),
        operator_version: Some(env!("CARGO_PKG_VERSION").to_string()),
        operations_report,
    };
    let mut sync_input = SyncInput::builder(sync_request);
    if let Some(operator_image) = operator_image.cloned() {
        sync_input = sync_input.operator_image(operator_image);
    }
    if let Some(application) = application {
        sync_input = sync_input.application(application);
    }
    let sync_input = sync_input.build();

    // Call manager with deployment_id in request body.
    //
    // NOTE: We use raw reqwest here instead of the alien-manager-api SDK because
    // the SDK's generated `OperatorSyncRequest.currentState` and
    // `OperatorSyncResponse.target` fields are `serde_json::Value` (the OpenAPI spec
    // defines them as free-form objects). Using `alien_core::sync::{SyncRequest,
    // SyncResponse}` directly gives us proper typed serialization/deserialization.
    // If the OpenAPI spec is updated to use $ref'd schemas for these fields,
    // this code should be migrated to the SDK's `operator_sync()` method.
    let base_url = base_url.trim_end_matches('/');
    let url = format!("{}/v1/sync", base_url);

    debug!(url = %url, deployment_id = %deployment_id, "Sending sync request to manager");

    let response = client
        .post(&url)
        .json(&sync_input)
        .send()
        .await
        .into_alien_error()
        .context(crate::error::ErrorData::SyncFailed {
            message: "Failed to send sync request".to_string(),
        })?;

    let status = response.status();
    if !status.is_success() {
        let error_body = response
            .text()
            .await
            .unwrap_or_else(|_| "<unable to read error>".to_string());

        // Attempt to deserialize as AlienError
        if let Ok(alien_error) =
            serde_json::from_str::<alien_error::AlienError<crate::error::ErrorData>>(&error_body)
        {
            return Err(alien_error);
        }

        return Err(alien_error::AlienError::new(
            crate::error::ErrorData::SyncFailed {
                message: format!("Manager returned error {}: {}", status, error_body),
            },
        ));
    }

    let sync_response: SyncResponse =
        response
            .json()
            .await
            .into_alien_error()
            .context(crate::error::ErrorData::SyncFailed {
                message: "Failed to parse sync response".to_string(),
            })?;

    let durable_target = sync_response.target.clone().or_else(|| {
        (sync_response.execution_claim == durable_execution.claim)
            .then(|| durable_execution.target.clone())
            .flatten()
    });
    // A claim and its target are one durable receipt. Persist them in one
    // SQLite value before changing deployment state/config so a crash can
    // resume the exact immutable target rather than asking for "latest".
    state
        .db
        .set_sync_execution(
            &sync_session,
            sync_response.execution_claim.as_ref(),
            durable_target.as_ref(),
        )
        .await?;

    // Persist the commands URL so the deployment loop can inject it into
    // deployed functions. This is the public URL cloud functions use to poll
    // for pending commands (vs. the operator's local sync URL which is only
    // reachable from the machine running the operator).
    if let Some(ref commands_url) = sync_response.commands_url {
        if let Err(e) = state.db.set_commands_url(commands_url).await {
            error!(error = %e, "Failed to persist commands_url");
        }
    }

    // Persist the target bundle set so a restart doesn't lose it for a full
    // extra tick — the sync_bundles() call above (built from THIS response,
    // used on the NEXT request) reads it back via get_target_operations_bundle_set.
    if let Some(ref target_bundle_set) = sync_response.target_operations_bundle_set {
        if let Err(e) = state
            .db
            .set_target_operations_bundle_set(target_bundle_set)
            .await
        {
            error!(error = %e, "Failed to persist target_operations_bundle_set");
        }
    }

    let mut state_hydrated = false;
    if let Some(manager_state) = sync_response.current_state {
        let local_state = state.db.get_deployment_state().await?;
        if local_state
            .as_ref()
            .is_none_or(is_uninitialized_deployment_state)
        {
            state.db.set_deployment_state(&manager_state).await?;
            state_hydrated = true;
            info!("Hydrated deployment state from manager");
        } else if let Some(mut local_state) = local_state {
            if apply_manager_control_state(&mut local_state, &manager_state) {
                state.db.set_deployment_state(&local_state).await?;
                state_hydrated = true;
                info!("Applied deployment control state from manager");
            } else {
                debug!("Manager returned deployment state, but local state is already initialized");
            }
        } else {
            debug!("Manager returned deployment state, but local state is already initialized");
        }
    }

    // Check if there's a new target
    let has_update = durable_target.is_some();

    if has_update {
        if let Some(target_deployment) = durable_target {
            let now = Utc::now().to_rfc3339();

            // Get current deployment state (or create default)
            let mut deployment_state =
                state.db.get_deployment_state().await?.unwrap_or_else(|| {
                    alien_core::DeploymentState {
                        platform: state.config.platform,
                        status: alien_core::DeploymentStatus::Pending,
                        current_release: None,
                        target_release: None,
                        stack_state: None,
                        error: None,
                        environment_info: None,
                        runtime_metadata: None,
                        retry_requested: false,
                        protocol_version: alien_core::DEPLOYMENT_PROTOCOL_VERSION,
                    }
                });

            // Update target_release in state
            let target_release_id = target_deployment.release_info.release_id.clone();
            let current_release_id = deployment_state
                .current_release
                .as_ref()
                .and_then(|release| release.release_id.clone());
            deployment_state.target_release = Some(target_deployment.release_info.clone());
            if deployment_state.status == alien_core::DeploymentStatus::Running
                && current_release_id != target_release_id
            {
                deployment_state.status = alien_core::DeploymentStatus::UpdatePending;
            }

            // Save state and config
            state.db.set_deployment_state(&deployment_state).await?;
            state
                .db
                .set_deployment_config(&target_deployment.config)
                .await?;

            // Handle deployment approval if required
            if state.config.requires_deployment_approval() {
                let apr_id = format!("apr_{}", Uuid::new_v4().simple());
                let approval = Approval {
                    id: apr_id.clone(),
                    release_info: Some(target_deployment.release_info.clone()),
                    deployment_config: target_deployment.config.clone(),
                    status: ApprovalStatus::Pending,
                    reason: None,
                    created_at: now,
                    decided_at: None,
                    decided_by: None,
                };
                state.db.create_approval(&approval).await?;
                info!(
                    approval_id = %apr_id,
                    release_id = %target_deployment.release_info.release_id.as_deref().unwrap_or_default(),
                    "Created approval for new target release"
                );
            }
        }
    }

    Ok(has_update || state_hydrated)
}

async fn observe_running_deployment(
    state: &OperatorState,
    deployment_id: &str,
) -> crate::error::Result<Vec<ObservedInventoryBatch>> {
    let client_config = super::deployment::resolve_client_config(
        state.config.platform,
        state.config.base_platform,
        &state.config.data_dir,
        state.config.namespace.clone(),
        state.config.sync.as_ref(),
    )
    .await?;
    let service_provider = state
        .service_provider
        .clone()
        .unwrap_or_else(|| Arc::new(alien_infra::DefaultPlatformServiceProvider::default()));
    let observe_report = run_observe_pass(
        state.config.platform,
        &client_config,
        &service_provider,
        deployment_id,
        state.config.label_selector.as_deref(),
        state.config.observe_all_namespaces,
    )
    .await
    .map_err(|e| e.into_generic())
    .context(crate::error::ErrorData::SyncFailed {
        message: "Failed to run observe pass before sync".to_string(),
    })?;
    Ok(observe_report.inventory_batches)
}

fn report_operator_capabilities(
    state: &OperatorState,
    operations_command_address_v1: bool,
) -> Vec<OperatorCapabilityReport> {
    let mut capabilities = vec![operation_command_address_capability(
        operations_command_address_v1,
    )];
    capabilities.push(OperatorCapabilityReport {
        key: "credential.rotation-v1".to_string(),
        state: OperatorCapabilityState::Granted,
        detail: Some(
            "Persisted deployment credential replacement with monotonic revisions".to_string(),
        ),
    });

    let workload_state = if state.config.platform == Platform::Kubernetes {
        OperatorCapabilityState::Granted
    } else {
        OperatorCapabilityState::Unavailable
    };
    capabilities.push(OperatorCapabilityReport {
        key: "k8s-workloads".to_string(),
        state: workload_state,
        detail: state
            .config
            .namespace
            .as_ref()
            .map(|namespace| format!("namespace {namespace}")),
    });

    capabilities.push(OperatorCapabilityReport {
        key: "cloud-observe".to_string(),
        state: state
            .config
            .base_platform
            .map(|_| OperatorCapabilityState::Granted)
            .unwrap_or(OperatorCapabilityState::Unavailable),
        detail: state
            .config
            .base_platform
            .map(|platform| format!("base platform {platform:?}")),
    });

    capabilities.push(OperatorCapabilityReport {
        key: "logs".to_string(),
        state: if state.config.collector_token.is_some() {
            OperatorCapabilityState::Granted
        } else {
            OperatorCapabilityState::Unavailable
        },
        detail: None,
    });

    capabilities
}

fn operation_command_address_capability(
    operations_command_address_v1: bool,
) -> OperatorCapabilityReport {
    OperatorCapabilityReport {
        key: "operations.command-address-v1".to_string(),
        state: if operations_command_address_v1 {
            OperatorCapabilityState::Granted
        } else {
            OperatorCapabilityState::Unavailable
        },
        detail: operations_command_address_v1
            .then(|| "Version-qualified operation command registration and execution".to_string()),
    }
}

fn is_uninitialized_deployment_state(state: &alien_core::DeploymentState) -> bool {
    state.status == alien_core::DeploymentStatus::Pending
        && state.current_release.is_none()
        && state.target_release.is_none()
        && state.stack_state.is_none()
        && state.environment_info.is_none()
        && state.runtime_metadata.is_none()
}

fn apply_manager_control_state(
    local_state: &mut alien_core::DeploymentState,
    manager_state: &alien_core::DeploymentState,
) -> bool {
    if manager_state_is_delete_command(manager_state)
        && !local_state_is_delete_or_deleted(local_state)
    {
        local_state.status = manager_state.status;
        if local_state.target_release.is_none() {
            local_state.target_release = manager_state.target_release.clone();
        }
        if local_state.stack_state.is_none() {
            local_state.stack_state = manager_state.stack_state.clone();
        }
        if local_state.environment_info.is_none() {
            local_state.environment_info = manager_state.environment_info.clone();
        }
        if local_state.runtime_metadata.is_none() {
            local_state.runtime_metadata = manager_state.runtime_metadata.clone();
        }
        local_state.retry_requested = manager_state.retry_requested;
        return true;
    }

    if manager_state.retry_requested
        && !local_state.retry_requested
        && local_state.status.is_failed()
    {
        local_state.retry_requested = true;
        return true;
    }

    false
}

fn manager_state_is_delete_command(state: &alien_core::DeploymentState) -> bool {
    matches!(
        state.status,
        alien_core::DeploymentStatus::DeletePending
            | alien_core::DeploymentStatus::Deleting
            | alien_core::DeploymentStatus::DeleteFailed
    )
}

fn local_state_is_delete_or_deleted(state: &alien_core::DeploymentState) -> bool {
    matches!(
        state.status,
        alien_core::DeploymentStatus::DeletePending
            | alien_core::DeploymentStatus::Deleting
            | alien_core::DeploymentStatus::DeleteFailed
            | alien_core::DeploymentStatus::TeardownRequired
            | alien_core::DeploymentStatus::TeardownFailed
            | alien_core::DeploymentStatus::Deleted
    )
}

#[cfg(test)]
mod tests {
    use std::{
        collections::{BTreeMap, HashMap},
        sync::{Arc, Mutex},
    };

    use alien_core::{
        sync::OperatorCapabilityState, ContainerImageIdentity, DeploymentState, DeploymentStatus,
        HeartbeatBackend, ObservedHealth, ObservedInventoryBatch, ObservedResourceSample, Platform,
        ProviderLifecycleState, CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
    };
    use alien_infra::MockPlatformServiceProvider;
    use alien_k8s_clients::{
        DeploymentApi, EventApi, KubernetesClient, KubernetesClientConfig, MetricsApi, PodApi,
    };
    use axum::{
        extract::State,
        routing::{get, post},
        Json, Router,
    };
    use serde_json::{json, Value};
    use tokio_util::sync::CancellationToken;

    use super::{
        apply_manager_control_state, create_authenticated_client,
        is_uninitialized_deployment_state, operation_command_address_capability, sync_with_manager,
    };
    use crate::{db::OperatorDb, OperatorConfig, OperatorState, SyncConfig};

    const TEST_ENCRYPTION_KEY: &str =
        "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    struct SyncFixture {
        config: OperatorConfig,
        status: DeploymentStatus,
        pending_inventory: Vec<ObservedInventoryBatch>,
        service_provider: Option<Arc<dyn alien_infra::PlatformServiceProvider>>,
    }

    /// Runs one sync against a stub manager and returns the request body it
    /// received.
    async fn captured_sync_request(
        config: impl FnOnce(String, SyncConfig) -> SyncFixture,
    ) -> Value {
        type Captured = Arc<Mutex<Option<Value>>>;
        async fn capture_sync(
            State(captured): State<Captured>,
            Json(body): Json<Value>,
        ) -> Json<Value> {
            *captured.lock().unwrap() = Some(body);
            Json(json!({}))
        }

        let captured: Captured = Arc::new(Mutex::new(None));
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("bind stub manager");
        let address = listener.local_addr().expect("stub manager address");
        let app = Router::new()
            .route("/v1/sync", post(capture_sync))
            .with_state(captured.clone());
        let server = tokio::spawn(async move { axum::serve(listener, app).await });

        let data_dir = tempfile::tempdir().expect("create data dir");
        let sync_config = SyncConfig {
            url: format!("http://{address}").parse().unwrap(),
            token: "ax_dep_test".to_string(),
        };
        let fixture = config(
            data_dir.path().to_string_lossy().to_string(),
            sync_config.clone(),
        );
        let db = OperatorDb::new(&fixture.config.data_dir, TEST_ENCRYPTION_KEY)
            .await
            .expect("open operator db");
        db.set_deployment_id("dep_remote").await.unwrap();
        db.set_deployment_state(&DeploymentState {
            platform: fixture.config.platform,
            status: fixture.status,
            current_release: None,
            target_release: None,
            stack_state: None,
            error: None,
            environment_info: None,
            runtime_metadata: None,
            retry_requested: false,
            protocol_version: CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
        })
        .await
        .unwrap();
        db.set_pending_observed_inventory_batches(&fixture.pending_inventory)
            .await
            .unwrap();
        let state = OperatorState {
            config: fixture.config,
            db: Arc::new(db),
            service_provider: fixture.service_provider,
            operations_sync_handler: None,
            cancel: CancellationToken::new(),
        };

        let client = create_authenticated_client(&sync_config.token).unwrap();
        sync_with_manager(&state, &client, sync_config.url.as_str(), false, None)
            .await
            .expect("sync with stub manager");
        server.abort();

        let body = captured.lock().unwrap().take();
        body.expect("stub manager received a sync request")
    }

    /// Serves the Kubernetes list calls the observe pass makes for one
    /// Helm-installed Deployment in namespace `shop`, and returns a service
    /// provider whose Kubernetes clients talk to it.
    async fn stub_kubernetes_api(image_id: &str) -> Arc<dyn alien_infra::PlatformServiceProvider> {
        fn list(api_version: &str, kind: &str, items: Value) -> Value {
            json!({ "apiVersion": api_version, "kind": kind, "metadata": {}, "items": items })
        }
        let deployments = list(
            "apps/v1",
            "DeploymentList",
            json!([{
                "metadata": {
                    "name": "api",
                    "namespace": "shop",
                    "labels": {
                        "helm.sh/chart": "shop-1.4.0",
                        "app.kubernetes.io/version": "1.4.0"
                    }
                },
                "spec": {
                    "selector": { "matchLabels": { "app": "api" } },
                    "template": {
                        "metadata": { "labels": { "app": "api" } },
                        "spec": { "containers": [{ "name": "api" }] }
                    }
                },
                "status": { "replicas": 1, "readyReplicas": 1 }
            }]),
        );
        let pods = list(
            "v1",
            "PodList",
            json!([{
                "metadata": { "name": "api-7d9f", "namespace": "shop" },
                "spec": { "containers": [{ "name": "api" }] },
                "status": {
                    "phase": "Running",
                    "containerStatuses": [{
                        "name": "api",
                        "ready": true,
                        "restartCount": 0,
                        "image": "registry.example.com/shop/api:1.4.0",
                        "imageID": image_id
                    }]
                }
            }]),
        );
        let statefulsets = list("apps/v1", "StatefulSetList", json!([]));
        let daemonsets = list("apps/v1", "DaemonSetList", json!([]));
        let events = list("v1", "EventList", json!([]));
        let metrics = list("metrics.k8s.io/v1beta1", "PodMetricsList", json!([]));
        let app = Router::new()
            .route(
                "/apis/apps/v1/namespaces/shop/deployments",
                get(move || async move { Json(deployments) }),
            )
            .route(
                "/apis/apps/v1/namespaces/shop/statefulsets",
                get(move || async move { Json(statefulsets) }),
            )
            .route(
                "/apis/apps/v1/namespaces/shop/daemonsets",
                get(move || async move { Json(daemonsets) }),
            )
            .route(
                "/api/v1/namespaces/shop/pods",
                get(move || async move { Json(pods) }),
            )
            .route(
                "/api/v1/namespaces/shop/events",
                get(move || async move { Json(events) }),
            )
            .route(
                "/apis/metrics.k8s.io/v1beta1/namespaces/shop/pods",
                get(move || async move { Json(metrics) }),
            );
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("bind stub Kubernetes API");
        let address = listener.local_addr().expect("stub Kubernetes address");
        tokio::spawn(async move { axum::serve(listener, app).await });

        let client = Arc::new(
            KubernetesClient::new(KubernetesClientConfig::Manual {
                server_url: format!("http://{address}"),
                certificate_authority_data: None,
                insecure_skip_tls_verify: None,
                client_certificate_data: None,
                client_key_data: None,
                token: Some("test-token".to_string()),
                username: None,
                password: None,
                namespace: Some("shop".to_string()),
                additional_headers: HashMap::new(),
            })
            .await
            .expect("create stub Kubernetes client"),
        );
        let mut provider = MockPlatformServiceProvider::new();
        let deployment_client: Arc<dyn DeploymentApi> = client.clone();
        let pod_client: Arc<dyn PodApi> = client.clone();
        let event_client: Arc<dyn EventApi> = client.clone();
        let metrics_client: Arc<dyn MetricsApi> = client;
        provider
            .expect_get_kubernetes_deployment_client()
            .returning(move |_| Ok(deployment_client.clone()));
        provider
            .expect_get_kubernetes_pod_client()
            .returning(move |_| Ok(pod_client.clone()));
        provider
            .expect_get_kubernetes_event_client()
            .returning(move |_| Ok(event_client.clone()));
        provider
            .expect_get_kubernetes_metrics_client()
            .returning(move |_| Ok(metrics_client.clone()));
        Arc::new(provider)
    }

    fn earlier_tick_inventory() -> Vec<ObservedInventoryBatch> {
        vec![ObservedInventoryBatch {
            source_kind: "operator".to_string(),
            inventory_scope: "kubernetes:apps/v1:Deployment:shop".to_string(),
            controller_platform: Platform::Kubernetes,
            backend: HeartbeatBackend::Kubernetes,
            observed_at: "2026-09-23T10:00:00Z".parse().unwrap(),
            complete: true,
            resources: vec![ObservedResourceSample {
                deployment_id: Some("dep_remote".to_string()),
                raw_identity: "apps/v1:Deployment:shop:api".to_string(),
                provider_kind: "apps/v1/Deployment".to_string(),
                display_name: "api".to_string(),
                namespace: Some("shop".to_string()),
                region: None,
                scope: None,
                resource_type_hint: None,
                version: None,
                alien_resource_id: None,
                health: ObservedHealth::Healthy,
                lifecycle: ProviderLifecycleState::Running,
                message: None,
                partial: false,
                provider_stale: false,
                counts: None,
                collection_issues: vec![],
                labels: BTreeMap::from([("helm.sh/chart".to_string(), "shop-1.3.0".to_string())]),
                attributes: BTreeMap::new(),
                raw: vec![],
                images: vec![ContainerImageIdentity {
                    name: "api".to_string(),
                    image: "registry.example.com/shop/api:1.3.0".to_string(),
                    digest: None,
                }],
            }],
        }]
    }

    #[tokio::test]
    async fn ecs_diagnostics_operator_reports_the_configured_application_release() {
        let body = captured_sync_request(|data_dir, sync| SyncFixture {
            config: OperatorConfig::builder()
                .platform(Platform::Aws)
                .operator_permission("diagnostics")
                .app_version("2026.09.1")
                .sync(sync)
                .data_dir(data_dir)
                .encryption_key(TEST_ENCRYPTION_KEY)
                .build(),
            status: DeploymentStatus::Running,
            pending_inventory: vec![],
            service_provider: None,
        })
        .await;

        assert_eq!(body["deploymentId"], "dep_remote");
        assert_eq!(
            body["currentState"]["currentRelease"]["version"],
            "2026.09.1"
        );
        assert_eq!(
            body["currentState"]["currentRelease"]["releaseId"],
            Value::Null
        );
        assert!(body.get("observedInventoryBatches").is_none());
        assert!(body.get("application").is_none());
    }

    #[tokio::test]
    async fn kubernetes_remote_operator_reports_chart_and_image_digests_it_observes() {
        let digest = format!("sha256:{}", "a".repeat(64));
        let provider =
            stub_kubernetes_api(&format!("registry.example.com/shop/api@{digest}")).await;

        let body = captured_sync_request(|data_dir, sync| SyncFixture {
            config: OperatorConfig::builder()
                .platform(Platform::Kubernetes)
                .operator_permission("observe")
                .namespace("shop")
                .sync(sync)
                .data_dir(data_dir)
                .encryption_key(TEST_ENCRYPTION_KEY)
                .build(),
            status: DeploymentStatus::Running,
            pending_inventory: earlier_tick_inventory(),
            service_provider: Some(provider),
        })
        .await;

        let mut application = body["application"].clone();
        let observed_at = application
            .as_object_mut()
            .and_then(|report| report.remove("observedAt"))
            .expect("application report carries observedAt");
        assert!(observed_at.as_str().is_some_and(|at| at.starts_with("20")));
        assert_eq!(
            application,
            json!({
                "source": "kubernetes",
                "chartName": "shop",
                "chartVersion": "1.4.0",
                "images": [{
                    "workload": "apps/v1:Deployment:shop:api",
                    "container": "api",
                    "image": "registry.example.com/shop/api:1.4.0",
                    "digest": digest,
                }],
                "complete": true,
            })
        );
    }

    #[test]
    fn reports_versioned_operation_command_support_declared_by_the_receiver() {
        let capability = operation_command_address_capability(true);
        assert_eq!(capability.key, "operations.command-address-v1");
        assert_eq!(capability.state, OperatorCapabilityState::Granted);
    }

    #[test]
    fn does_not_report_versioned_command_support_without_receiver_opt_in() {
        let capability = operation_command_address_capability(false);
        assert_eq!(capability.key, "operations.command-address-v1");
        assert_eq!(capability.state, OperatorCapabilityState::Unavailable);
    }

    #[test]
    fn recognizes_empty_pending_state_as_uninitialized() {
        let state = DeploymentState {
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
        };

        assert!(is_uninitialized_deployment_state(&state));
    }

    #[test]
    fn recognizes_non_pending_state_as_initialized() {
        let state = DeploymentState {
            platform: Platform::Kubernetes,
            status: DeploymentStatus::Provisioning,
            current_release: None,
            target_release: None,
            stack_state: None,
            error: None,
            environment_info: None,
            runtime_metadata: None,
            retry_requested: false,
            protocol_version: CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
        };

        assert!(!is_uninitialized_deployment_state(&state));
    }

    #[test]
    fn applies_manager_retry_request_to_failed_local_state() {
        let mut local_state = DeploymentState {
            platform: Platform::Local,
            status: DeploymentStatus::ProvisioningFailed,
            current_release: None,
            target_release: None,
            stack_state: None,
            error: None,
            environment_info: None,
            runtime_metadata: None,
            retry_requested: false,
            protocol_version: CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
        };
        let manager_state = DeploymentState {
            retry_requested: true,
            ..local_state.clone()
        };

        assert!(apply_manager_control_state(
            &mut local_state,
            &manager_state
        ));
        assert!(local_state.retry_requested);
    }

    #[test]
    fn ignores_manager_retry_request_for_active_local_state() {
        let mut local_state = DeploymentState {
            platform: Platform::Local,
            status: DeploymentStatus::Provisioning,
            current_release: None,
            target_release: None,
            stack_state: None,
            error: None,
            environment_info: None,
            runtime_metadata: None,
            retry_requested: false,
            protocol_version: CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
        };
        let manager_state = DeploymentState {
            status: DeploymentStatus::ProvisioningFailed,
            retry_requested: true,
            ..local_state.clone()
        };

        assert!(!apply_manager_control_state(
            &mut local_state,
            &manager_state
        ));
        assert!(!local_state.retry_requested);
    }

    #[test]
    fn applies_manager_delete_request_to_running_local_state() {
        let mut local_state = DeploymentState {
            platform: Platform::Local,
            status: DeploymentStatus::Running,
            current_release: None,
            target_release: None,
            stack_state: None,
            error: None,
            environment_info: None,
            runtime_metadata: None,
            retry_requested: false,
            protocol_version: CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
        };
        let manager_state = DeploymentState {
            status: DeploymentStatus::DeletePending,
            target_release: None,
            ..local_state.clone()
        };

        assert!(apply_manager_control_state(
            &mut local_state,
            &manager_state
        ));
        assert_eq!(local_state.status, DeploymentStatus::DeletePending);
    }

    #[test]
    fn does_not_rewind_local_delete_progress_from_manager_state() {
        let mut local_state = DeploymentState {
            platform: Platform::Local,
            status: DeploymentStatus::Deleting,
            current_release: None,
            target_release: None,
            stack_state: None,
            error: None,
            environment_info: None,
            runtime_metadata: None,
            retry_requested: false,
            protocol_version: CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
        };
        let manager_state = DeploymentState {
            status: DeploymentStatus::DeletePending,
            ..local_state.clone()
        };

        assert!(!apply_manager_control_state(
            &mut local_state,
            &manager_state
        ));
        assert_eq!(local_state.status, DeploymentStatus::Deleting);
    }
}
