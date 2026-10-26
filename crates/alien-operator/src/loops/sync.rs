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
use crate::{OperatorConfig, OperatorState};
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
use tracing::{debug, error, info, warn};
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
            _ = state.db.wait_for_sync() => {},
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

/// Refresh only established secret slots; completed deployments no longer retain
/// the input values and credentials from their installation configuration.
pub(super) async fn refresh_running_secrets(
    state: &OperatorState,
    current: &mut alien_core::DeploymentState,
) -> crate::error::Result<()> {
    if current.status != DeploymentStatus::Running || current.target_release.is_some() {
        return Ok(());
    }
    let Some(metadata) = &current.runtime_metadata else {
        return Ok(());
    };
    if metadata.deployer_secrets.is_empty() {
        return Ok(());
    }
    let (Some(prepared), Some(stack_state)) = (&metadata.prepared_stack, &current.stack_state)
    else {
        return Ok(());
    };
    let mut stack = prepared.clone();
    stack.inputs.retain(|input| {
        metadata
            .deployer_secrets
            .iter()
            .any(|report| report.input_id == input.id)
    });
    // Setup records the effective cloud here, including a manager-supplied base
    // platform. That observation survives clearing the installation config.
    let base_platform = current
        .environment_info
        .as_ref()
        .map(alien_core::EnvironmentInfo::platform)
        .or(state.config.base_platform);
    let client_config = super::deployment::resolve_client_config(
        current.platform,
        base_platform,
        &state.config.data_dir,
        state.config.namespace.clone(),
        state.config.sync.as_ref(),
    )
    .await?;
    let config = alien_core::DeploymentConfig::builder()
        .maybe_deployment_name(state.config.agent_name.clone())
        .stack_settings(Default::default())
        .environment_variables(alien_core::EnvironmentVariablesSnapshot {
            variables: Vec::new(),
            hash: String::new(),
            created_at: String::new(),
        })
        .stored_secret_input_ids(Vec::new())
        .allow_frozen_changes(false)
        .external_bindings(Default::default())
        .build();
    let reports = alien_deployment::check_deployer_secrets(
        &stack,
        stack_state,
        &client_config,
        &config,
        current.platform,
    )
    .await
    .context(crate::error::ErrorData::DeploymentFailed {
        message: "Failed to refresh deployer secrets".to_string(),
    })?;
    if reports == metadata.deployer_secrets {
        return Ok(());
    }
    let mut next = current.clone();
    next.runtime_metadata
        .as_mut()
        .expect("metadata checked above")
        .deployer_secrets = reports;
    if state
        .db
        .compare_and_set_deployment_state(current, &next)
        .await?
    {
        *current = next;
    } else if let Some(latest) = state.db.get_deployment_state().await? {
        *current = latest;
    }
    Ok(())
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
                protocol_version: alien_core::deployment_protocol_version_for_platform(
                    state.config.platform,
                    state.config.base_platform,
                ),
            });

    // A vault outage must not prevent receiving a new target or deletion request.
    // Preserve the last observation and surface the refresh error after syncing.
    let secret_refresh = refresh_running_secrets(state, &mut deployment_state).await;

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
    let dynamic_reports =
        if dynamic_containers_capability(&state.config).state == OperatorCapabilityState::Granted {
            match super::dynamic_containers::reconcile_saved(state, &deployment_id).await {
                Ok(reports) => reports,
                Err(error) => {
                    error!(error = %error, "Dynamic container reconciliation failed");
                    None
                }
            }
        } else {
            None
        };

    // Propose a protocol-only upgrade through the manager first. The deployment
    // loop cannot persist or execute the upgrade locally until its acknowledgement.
    // Legacy GCP federation needs a setup upgrade and must not be inferred here.
    let effective_platform = state
        .config
        .base_platform
        .unwrap_or(deployment_state.platform);
    if deployment_state.protocol_version < alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION
        && (effective_platform != Platform::Gcp
            || deployment_state.protocol_version
                >= alien_core::GCP_FEDERATION_DEPLOYMENT_PROTOCOL_VERSION)
    {
        deployment_state.protocol_version = alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION;
    }
    let sync_request = SyncRequest {
        deployment_id: deployment_id.clone(),
        session: sync_session.clone(),
        supports_execution_claims: true,
        supports_tunnels: true,
        execution_claim,
        current_state: Some(deployment_state),
        heartbeats,
        observed_inventory_batches,
        capabilities: report_operator_capabilities(state, operations_command_address_v1),
        operator_version: Some(env!("CARGO_PKG_VERSION").to_string()),
        operations_report,
        operations_config: Some(reported_operations_config(state.config.operations.as_ref())),
    };
    let mut sync_input = SyncInput::builder(sync_request);
    if let Some(operator_image) = operator_image.cloned() {
        sync_input = sync_input.operator_image(operator_image);
    }
    if let Some(application) = application {
        sync_input = sync_input.application(application);
    }
    if let Some(reports) = dynamic_reports {
        sync_input = sync_input.dynamic_containers(reports);
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
            if durable_execution.claim.is_some()
                && matches!(
                    alien_error.code.as_str(),
                    "DEPLOYMENT_LEASE_LOST" | "DEPLOYMENT_UPDATE_CLAIM_LOST"
                )
            {
                // This receipt can no longer authorize a report. A new session
                // lets the manager expire the abandoned attempt and grant fresh
                // work; keep the deployment checkpoint and pending observations.
                let session = format!("pull-operator-{}", Uuid::new_v4());
                state.db.set_sync_execution(&session, None, None).await?;
                warn!(
                    deployment_id = %deployment_id,
                    error = %alien_error,
                    "Discarded rejected execution claim; requesting a fresh claim on the next sync"
                );
            }
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

    // The manager advertises its tunnel endpoint on every sync; an absent URL
    // (older manager, or tunnels disabled) stops the tunnel loop from dialing.
    if let Err(e) = state
        .db
        .set_tunnel_url(sync_response.tunnel_url.as_deref())
        .await
    {
        error!(error = %e, "Failed to persist tunnel_url");
    }

    if let Some(image) = sync_response.target_operator_image.as_deref() {
        if let Err(e) = crate::self_update::apply(state, image).await {
            warn!(error = %e, image, "Operator self-update failed; will retry on the next sync");
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
    if let Some(ref targets) = sync_response.target_dynamic_containers {
        state.db.set_target_dynamic_containers(targets).await?;
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
            // Platform claims maintenance on the operation whose apply this
            // Operator completed last (its applied operation). Any other
            // claim is new work: an update, a corrective target or a retry.
            let maintenance = match sync_response.execution_claim.as_ref() {
                Some(claim) => {
                    state.db.get_applied_operation_id().await?.as_deref()
                        == Some(claim.operation_id.as_str())
                }
                None => false,
            };
            accept_target(state, &target_deployment, maintenance).await?;
        }
    }

    secret_refresh?;
    Ok(has_update || state_hydrated)
}

/// Make `target_deployment` the Operator's target: record the release as the
/// target release, store its configuration, and open an approval when
/// updates need one. Sync and air-gapped bundles both deliver targets here.
///
/// A `maintenance` target observes the release this Operator already applied.
/// It keeps the deployment's status, so the deployment loop runs the Running
/// refresh (every Ready handler) instead of re-applying the release.
pub(crate) async fn accept_target(
    state: &OperatorState,
    target_deployment: &alien_core::sync::TargetDeployment,
    maintenance: bool,
) -> crate::error::Result<()> {
    let now = Utc::now().to_rfc3339();

    // Get current deployment state (or create default)
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
                protocol_version: alien_core::deployment_protocol_version_for_platform(
                    state.config.platform,
                    state.config.base_platform,
                ),
            });

    // Online upgrades come only from the manager's persisted acknowledgement.
    // Air-gapped state has no remote manager to fence.
    if state.config.sync.is_none() {
        deployment_state.protocol_version = alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION;
    }

    let refresh = maintenance
        && refreshes_installed_release(&deployment_state, &target_deployment.release_info);
    if refresh {
        deployment_state.target_release = Some(target_deployment.release_info.clone());
    } else {
        accept_target_release(
            &mut deployment_state,
            target_deployment.release_info.clone(),
        );
    }

    // Save state and config
    state.db.set_deployment_state(&deployment_state).await?;
    state
        .db
        .set_deployment_config(&target_deployment.config)
        .await?;

    // Handle deployment approval if required. A refresh changes nothing that
    // was not already approved.
    if !refresh && state.config.requires_deployment_approval() {
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
    Ok(())
}

/// Whether a maintenance target can be served by refreshing what is installed:
/// the deployment is settled on exactly the target's release. Anything else
/// (an Operator that restarted mid-update, a different release) applies it.
fn refreshes_installed_release(
    deployment_state: &alien_core::DeploymentState,
    release: &alien_core::ReleaseInfo,
) -> bool {
    matches!(
        deployment_state.status,
        alien_core::DeploymentStatus::Running | alien_core::DeploymentStatus::RefreshFailed
    ) && release.release_id.is_some()
        && deployment_state
            .current_release
            .as_ref()
            .and_then(|current| current.release_id.as_deref())
            == release.release_id.as_deref()
}

fn accept_target_release(
    deployment_state: &mut alien_core::DeploymentState,
    release: alien_core::ReleaseInfo,
) {
    deployment_state.target_release = Some(release);
    // A target can change runtime configuration while retaining the same
    // release. A corrective target must also restart a failed update from
    // preflights, rather than reusing the failed target's prepared stack.
    // A failed health check does not block new work either.
    if matches!(
        deployment_state.status,
        alien_core::DeploymentStatus::Running
            | alien_core::DeploymentStatus::ProvisioningFailed
            | alien_core::DeploymentStatus::UpdateFailed
            | alien_core::DeploymentStatus::RefreshFailed
    ) {
        deployment_state.status = if deployment_state.status
            == alien_core::DeploymentStatus::ProvisioningFailed
            && deployment_state.stack_state.is_none()
        {
            alien_core::DeploymentStatus::Pending
        } else {
            alien_core::DeploymentStatus::UpdatePending
        };
        if let Some(metadata) = deployment_state.runtime_metadata.as_mut() {
            metadata.pending_prepared_stack = None;
            metadata.pending_prepared_release_id = None;
        }
        deployment_state.retry_requested = false;
    }
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

    capabilities.push(dynamic_containers_capability(&state.config));

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

    capabilities.extend(logs_capability(&state.config, state.pod_log_collection));

    capabilities
}

/// Reports whether the Kubernetes log collector sends Pod logs. Logs reach the
/// Operator either from a node collector, which authenticates with the
/// collector token, or from the Operator's own Pod log stream, which needs no
/// token. Either one means this installation sends logs.
///
/// Only the Kubernetes chart has a log collector, so other platforms report
/// nothing. Their logs don't depend on a collector: workers export them over
/// OTLP to the endpoint in the deployment's monitoring config, and the cloud's
/// container runtime ships container logs. An `unavailable` report there would
/// tell clients that a deployment whose logs work sends none.
///
/// Every state carries a detail. Earlier Operators reported this capability
/// with no detail and marked it unavailable whenever the collector token was
/// missing, including in podApi mode where logs do flow, so clients trust an
/// unavailable report only when it has a detail.
fn logs_capability(
    config: &OperatorConfig,
    pod_log_collection: bool,
) -> Option<OperatorCapabilityReport> {
    if config.platform != Platform::Kubernetes {
        return None;
    }
    let (state, detail) = if pod_log_collection {
        (
            OperatorCapabilityState::Granted,
            "Pod logs read through the Kubernetes API",
        )
    } else if config.collector_token.is_some() {
        (
            OperatorCapabilityState::Granted,
            "Pod logs sent by the node log collector",
        )
    } else {
        (
            OperatorCapabilityState::Unavailable,
            "No log collector configured",
        )
    };
    Some(OperatorCapabilityReport {
        key: "logs".to_string(),
        state,
        detail: Some(detail.to_string()),
    })
}

/// Granted only when the chart bound the dynamic container Role: the manager
/// sends container targets to Operators that report this, so a namespace-only
/// install without that access must not claim it.
/// The operations this Operator declares locally, without setting values. An
/// Operator that declares none reports an empty declaration, so removing its
/// last plugin clears what it reported before; only an Operator too old to
/// report the field omits it.
fn reported_operations_config(
    local: Option<&alien_core::OperationsConfig>,
) -> alien_core::OperationsConfig {
    local
        .map(alien_core::OperationsConfig::without_settings)
        .unwrap_or_default()
}

fn dynamic_containers_capability(config: &OperatorConfig) -> OperatorCapabilityReport {
    let granted = config.platform == Platform::Kubernetes
        && config.namespace.is_some()
        && config.dynamic_containers;
    OperatorCapabilityReport {
        key: "dynamic-containers-v1".to_string(),
        state: if granted {
            OperatorCapabilityState::Granted
        } else {
            OperatorCapabilityState::Unavailable
        },
        detail: None,
    }
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
    // Setup can remove identities and complete retention after the runtime has
    // stopped. Adopt its whole checkpoint, including cleaned resource state,
    // rather than repeatedly reporting the old runtime handoff over it.
    if alien_core::accepts_deployment_protocol_report(
        local_state.protocol_version,
        manager_state.protocol_version,
    ) && matches!(
        manager_state.status,
        alien_core::DeploymentStatus::RetentionRequired
            | alien_core::DeploymentStatus::Retained
            | alien_core::DeploymentStatus::TeardownRequired
            | alien_core::DeploymentStatus::TeardownFailed
            | alien_core::DeploymentStatus::Deleted
    ) {
        *local_state = manager_state.clone();
        return true;
    }
    let protocol_changed = manager_state.protocol_version > local_state.protocol_version
        && alien_core::accepts_deployment_protocol_report(
            local_state.protocol_version,
            manager_state.protocol_version,
        );
    if protocol_changed {
        local_state.protocol_version = manager_state.protocol_version;
    }
    let policy_changed = apply_manager_deletion_policy(local_state, manager_state);
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

    protocol_changed || policy_changed
}

/// The manager authorizes the deletion policy. The operator keeps its own resource
/// checkpoints but must act on the manager's retain or erase choice.
fn apply_manager_deletion_policy(
    local_state: &mut alien_core::DeploymentState,
    manager_state: &alien_core::DeploymentState,
) -> bool {
    let (Some(local), Some(manager)) = (
        local_state.runtime_metadata.as_mut(),
        manager_state.runtime_metadata.as_ref(),
    ) else {
        return false;
    };
    if local.deletion_policy == manager.deletion_policy {
        return false;
    }
    local.deletion_policy = manager.deletion_policy;
    true
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
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc, Mutex,
        },
    };

    use alien_core::{
        sync::OperatorCapabilityState, ContainerImageIdentity, DeploymentState, DeploymentStatus,
        HeartbeatBackend, ObservedHealth, ObservedInventoryBatch, ObservedResourceSample, Platform,
        ProviderLifecycleState, ReleaseInfo, Stack, CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
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
        accept_target_release, apply_manager_control_state, create_authenticated_client,
        dynamic_containers_capability, is_uninitialized_deployment_state,
        operation_command_address_capability, report_operator_capabilities,
        reported_operations_config, sync_with_manager,
    };
    use crate::{db::OperatorDb, OperatorConfig, OperatorState, SyncConfig};

    const TEST_ENCRYPTION_KEY: &str =
        "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    /// Removing an Operator's last local plugin must clear what it reported:
    /// it sends an empty declaration (`{}` on the wire), which the platform
    /// tells apart from an older Operator that omits the field.
    #[test]
    fn an_operator_without_local_operations_reports_an_empty_declaration() {
        let declared: alien_core::OperationsConfig = serde_json::from_value(serde_json::json!({
            "plugins": {
                "postgres": {
                    "settings": { "password": { "env": "PG_PASSWORD" } },
                    "approval": { "*": "manual" }
                }
            }
        }))
        .expect("operations config");
        assert_eq!(
            serde_json::to_value(reported_operations_config(Some(&declared))).unwrap(),
            serde_json::json!({ "plugins": { "postgres": { "approval": { "*": "manual" } } } })
        );
        assert_eq!(
            serde_json::to_value(reported_operations_config(None)).unwrap(),
            serde_json::json!({})
        );
    }

    #[test]
    fn same_release_target_reconciles_runtime_configuration() {
        let release = ReleaseInfo {
            release_id: Some("rel_existing".to_string()),
            version: Some("1.0.0".to_string()),
            description: None,
            stack: Stack::new("test".to_string()).build(),
        };
        let mut state = DeploymentState {
            platform: Platform::Kubernetes,
            status: DeploymentStatus::Running,
            current_release: Some(release.clone()),
            target_release: None,
            stack_state: None,
            error: None,
            environment_info: None,
            runtime_metadata: None,
            retry_requested: false,
            protocol_version: CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
        };

        accept_target_release(&mut state, release.clone());

        assert_eq!(state.status, DeploymentStatus::UpdatePending);
        assert_eq!(state.target_release, Some(release.clone()));

        // New work also reaches a deployment whose health check failed.
        for status in [
            DeploymentStatus::RefreshFailed,
            DeploymentStatus::UpdateFailed,
        ] {
            state.status = status;
            state.target_release = None;
            accept_target_release(&mut state, release.clone());
            assert_eq!(state.status, DeploymentStatus::UpdatePending);
        }
    }

    #[test]
    fn corrective_target_without_provisioned_state_restarts_initial_preflights() {
        let release = ReleaseInfo {
            release_id: Some("rel_corrected".into()),
            version: None,
            description: None,
            stack: Stack::new("corrected".into()).build(),
        };
        let mut state = DeploymentState {
            platform: Platform::Kubernetes,
            status: DeploymentStatus::ProvisioningFailed,
            current_release: None,
            target_release: None,
            stack_state: None,
            error: None,
            environment_info: None,
            runtime_metadata: None,
            retry_requested: true,
            protocol_version: CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
        };
        accept_target_release(&mut state, release.clone());
        assert_eq!(state.status, DeploymentStatus::Pending);
        assert_eq!(state.target_release, Some(release.clone()));
        assert!(!state.retry_requested);
        state.status = DeploymentStatus::ProvisioningFailed;
        state.stack_state = Some(alien_core::StackState::new(Platform::Kubernetes));
        accept_target_release(&mut state, release);
        assert_eq!(state.status, DeploymentStatus::UpdatePending);
        assert!(state.stack_state.is_some());
    }

    #[tokio::test]
    async fn corrective_target_with_partial_state_and_no_current_release_converges() {
        let storage = alien_core::Storage::new("files".into()).build();
        let mut partial = alien_core::StackState::new(Platform::Test);
        let mut resource = alien_core::StackResourceState::new_pending(
            "storage".into(),
            alien_core::Resource::new(storage.clone()),
            None,
            vec![],
        );
        resource.lifecycle = Some(alien_core::ResourceLifecycle::Live);
        partial.resources.insert("files".into(), resource);
        let release = ReleaseInfo {
            release_id: Some("rel_corrected".into()),
            version: None,
            description: None,
            stack: Stack::new("corrected".into())
                .add(storage, alien_core::ResourceLifecycle::Live)
                .build(),
        };
        let mut state = DeploymentState {
            platform: Platform::Test,
            status: DeploymentStatus::ProvisioningFailed,
            current_release: None,
            target_release: None,
            stack_state: Some(partial),
            error: None,
            environment_info: None,
            runtime_metadata: Some(alien_core::RuntimeMetadata::default()),
            retry_requested: false,
            protocol_version: CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
        };
        accept_target_release(&mut state, release.clone());
        assert_eq!(state.status, DeploymentStatus::UpdatePending);
        let config = alien_core::DeploymentConfig::builder()
            .stack_settings(Default::default())
            .environment_variables(alien_core::EnvironmentVariablesSnapshot {
                variables: vec![],
                hash: String::new(),
                created_at: String::new(),
            })
            .allow_frozen_changes(false)
            .external_bindings(Default::default())
            .build();
        for _ in 0..30 {
            state =
                alien_deployment::step(state, config.clone(), alien_core::ClientConfig::Test, None)
                    .await
                    .expect("corrective deployment step")
                    .state;
            if state.status == DeploymentStatus::Running {
                break;
            }
            assert!(
                !matches!(
                    state.status,
                    DeploymentStatus::UpdateFailed | DeploymentStatus::ProvisioningFailed
                ),
                "{:?}",
                state.error
            );
        }
        assert_eq!(state.status, DeploymentStatus::Running);
        assert_eq!(
            state.current_release.unwrap().release_id,
            release.release_id
        );
        assert_eq!(
            state.stack_state.unwrap().resources["files"].status,
            alien_core::ResourceStatus::Running
        );
    }

    #[test]
    fn corrective_target_restarts_failed_update_from_preflights() {
        let release = ReleaseInfo {
            release_id: Some("rel_corrected".to_string()),
            version: None,
            description: None,
            stack: Stack::new("corrected".to_string()).build(),
        };
        for status in [
            DeploymentStatus::UpdateFailed,
            DeploymentStatus::ProvisioningFailed,
        ] {
            let mut metadata = alien_core::RuntimeMetadata::default();
            metadata.pending_prepared_stack = Some(Stack::new("failed-target".to_string()).build());
            let mut state = DeploymentState {
                platform: Platform::Kubernetes,
                status,
                current_release: None,
                target_release: None,
                stack_state: Some(alien_core::StackState::new(Platform::Kubernetes)),
                error: None,
                environment_info: None,
                runtime_metadata: Some(metadata),
                retry_requested: true,
                protocol_version: CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
            };

            accept_target_release(&mut state, release.clone());

            assert_eq!(state.status, DeploymentStatus::UpdatePending);
            assert_eq!(state.target_release, Some(release.clone()));
            assert!(!state.retry_requested);
            assert!(state
                .runtime_metadata
                .as_ref()
                .is_some_and(|metadata| metadata.pending_prepared_stack.is_none()));
        }
    }

    #[tokio::test]
    async fn running_cloud_secret_refresh_survives_config_cleanup_and_restart() {
        const CHILD: &str = "ALIEN_TEST_RUNNING_CLOUD_SECRET_REFRESH";
        if std::env::var_os(CHILD).is_none() {
            // Isolate credential environment changes from other tests. Only the
            // real metadata client in this child can reach this localhost stub.
            let calls = Arc::new(AtomicUsize::new(0));
            let request_calls = calls.clone();
            let app = Router::new().route("/", post(move |headers: axum::http::HeaderMap, body: axum::body::Bytes| {
                let body: Value = serde_json::from_slice(&body).unwrap();
                assert_eq!(headers["x-amz-target"], "AmazonSSM.DescribeParameters");
                assert_eq!(body["ParameterFilters"][0]["Values"][0], "test-deployer-secrets-key");
                let call = request_calls.fetch_add(1, Ordering::SeqCst);
                async move { Json(if call == 0 {
                    json!({"Parameters": [{"Name": "test-deployer-secrets-key", "Type": "SecureString", "Version": 7}]})
                } else { json!({"Parameters": []}) }) }
            }));
            let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
                .await
                .unwrap();
            let endpoint = format!("http://{}", listener.local_addr().unwrap());
            let server = tokio::spawn(async move { axum::serve(listener, app).await });
            let result = tokio::task::spawn_blocking(move || { std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "loops::sync::tests::running_cloud_secret_refresh_survives_config_cleanup_and_restart", "--nocapture"])
                .env_clear()
                .env(CHILD, "1")
                .env("RUST_MIN_STACK", "8388608")
                .env("AWS_ACCOUNT_ID", "123456789012")
                .env("AWS_REGION", "us-east-1")
                .env("AWS_ACCESS_KEY_ID", "fake-test-access-key")
                .env("AWS_SECRET_ACCESS_KEY", "fake-test-secret-key")
                .env("AWS_SERVICE_OVERRIDES_ENDPOINTS", json!({"ssm": endpoint}).to_string())
                .output().unwrap() }).await.unwrap();
            server.abort();
            assert!(
                result.status.success(),
                "{}\n{}",
                String::from_utf8_lossy(&result.stdout),
                String::from_utf8_lossy(&result.stderr)
            );
            assert_eq!(calls.load(Ordering::SeqCst), 2);
            return;
        }

        let data_dir = tempfile::tempdir().unwrap();
        let directory = data_dir.path().to_str().unwrap();
        let db = OperatorDb::new(directory, TEST_ENCRYPTION_KEY)
            .await
            .unwrap();
        let config = alien_core::DeploymentConfig::builder()
            .base_platform(Platform::Aws)
            .stack_settings(Default::default())
            .environment_variables(alien_core::EnvironmentVariablesSnapshot {
                variables: Vec::new(),
                hash: String::new(),
                created_at: String::new(),
            })
            .allow_frozen_changes(false)
            .external_bindings(Default::default())
            .build();
        db.set_deployment_config(&config).await.unwrap();
        let mut stack = Stack::new("cloud-secrets".into()).build();
        stack.inputs = vec![serde_json::from_value(json!({
            "id": "key", "kind": "secret", "providedBy": ["deployer"],
            "required": true, "label": "Test key", "description": ""
        }))
        .unwrap()];
        let mut vault = alien_core::StackResourceState::new_pending(
            "vault".into(),
            alien_core::Resource::new(alien_core::Vault::new("deployer-secrets".into()).build()),
            Some(alien_core::ResourceLifecycle::Frozen),
            Vec::new(),
        );
        vault.status = alien_core::ResourceStatus::Running;
        vault.remote_binding_params = Some(
            serde_json::to_value(alien_core::VaultBinding::parameter_store(
                "test-deployer-secrets",
            ))
            .unwrap(),
        );
        let mut stack_state = alien_core::StackState::new(Platform::Kubernetes);
        stack_state
            .resources
            .insert("deployer-secrets".into(), vault);
        let running = DeploymentState {
            platform: Platform::Kubernetes, status: DeploymentStatus::Running,
            current_release: None, target_release: None, stack_state: Some(stack_state),
            environment_info: Some(alien_core::EnvironmentInfo::Aws(alien_core::AwsEnvironmentInfo {
                account_id: "123456789012".into(), region: "us-east-1".into(),
            })),
            runtime_metadata: Some(alien_core::RuntimeMetadata {
                prepared_stack: Some(stack),
                deployer_secrets: vec![serde_json::from_value(json!({
                    "inputId": "key", "label": "Test key", "required": true, "status": "missing",
                    "location": {"store": "aws-parameter-store", "name": "test-deployer-secrets-key", "cliCommand": "unused"}
                })).unwrap()],
                ..Default::default()
            }),
            error: None, retry_requested: false, protocol_version: CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
        };
        db.set_deployment_state(&running).await.unwrap();
        db.clear_deployment_config().await.unwrap();
        drop(db);
        let db = Arc::new(
            OperatorDb::new(directory, TEST_ENCRYPTION_KEY)
                .await
                .unwrap(),
        );
        assert!(db.get_deployment_config().await.unwrap().is_none());
        let mut running = db.get_deployment_state().await.unwrap().unwrap();
        let state = OperatorState {
            config: OperatorConfig::builder()
                .platform(Platform::Kubernetes)
                .data_dir(directory)
                .encryption_key(TEST_ENCRYPTION_KEY)
                .build(),
            db,
            service_provider: None,
            operations_sync_handler: None,
            pod_log_collection: false,
            cancel: CancellationToken::new(),
        };
        assert!(state.config.base_platform.is_none());
        for expected in [
            alien_core::DeployerSecretStatus::Present,
            alien_core::DeployerSecretStatus::Missing,
        ] {
            super::refresh_running_secrets(&state, &mut running)
                .await
                .unwrap();
            let persisted = state.db.get_deployment_state().await.unwrap().unwrap();
            assert_eq!(
                persisted
                    .runtime_metadata
                    .as_ref()
                    .unwrap()
                    .deployer_secrets[0]
                    .status,
                expected
            );
            if expected == alien_core::DeployerSecretStatus::Present {
                assert_eq!(
                    persisted
                        .runtime_metadata
                        .as_ref()
                        .unwrap()
                        .deployer_secrets[0]
                        .version
                        .as_deref(),
                    Some("7")
                );
            }
        }
    }

    #[tokio::test]
    async fn secret_refresh_failure_still_receives_delete_and_reports_the_error() {
        let data_dir = tempfile::tempdir().unwrap();
        let db = Arc::new(
            OperatorDb::new(data_dir.path().to_str().unwrap(), TEST_ENCRYPTION_KEY)
                .await
                .unwrap(),
        );
        db.set_deployment_id("dep_secret_failure").await.unwrap();
        let mut stack = Stack::new("secret-check".into()).build();
        stack.inputs = vec![serde_json::from_value(json!({
            "id": "key", "kind": "secret", "providedBy": ["deployer"],
            "required": true, "label": "Test key", "description": ""
        }))
        .unwrap()];
        let report = serde_json::from_value(json!({
            "inputId": "key", "label": "Test key", "required": true, "status": "present",
            "location": {"store": "local-vault", "name": "input-key", "cliCommand": "unused"}
        }))
        .unwrap();
        let mut stack_state = alien_core::StackState::new(Platform::Local);
        stack_state.resources.insert(
            "deployer-secrets".into(),
            alien_core::StackResourceState::new_pending(
                "vault".into(),
                alien_core::Resource::new(
                    alien_core::Vault::new("deployer-secrets".into()).build(),
                ),
                Some(alien_core::ResourceLifecycle::Frozen),
                Vec::new(),
            ),
        );
        let running = DeploymentState {
            platform: Platform::Local,
            status: DeploymentStatus::Running,
            current_release: None,
            target_release: None,
            // The vault binding is unavailable: refreshing this established slot must fail.
            stack_state: Some(stack_state),
            runtime_metadata: Some(alien_core::RuntimeMetadata {
                prepared_stack: Some(stack),
                deployer_secrets: vec![report],
                ..Default::default()
            }),
            error: None,
            environment_info: None,
            retry_requested: false,
            protocol_version: CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
        };
        db.set_deployment_state(&running).await.unwrap();
        let deletion = DeploymentState {
            status: DeploymentStatus::DeletePending,
            ..running.clone()
        };
        let captured = Arc::new(Mutex::new(None));
        let request_capture = captured.clone();
        let app = Router::new().route(
            "/v1/sync",
            post(move |Json(body): Json<Value>| {
                *request_capture.lock().unwrap() = Some(body);
                let response = json!({"currentState": deletion});
                async move { Json(response) }
            }),
        );
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await });
        let state = OperatorState {
            config: OperatorConfig::builder()
                .platform(Platform::Local)
                .data_dir(data_dir.path().to_str().unwrap())
                .encryption_key(TEST_ENCRYPTION_KEY)
                .build(),
            db,
            service_provider: None,
            operations_sync_handler: None,
            pod_log_collection: false,
            cancel: CancellationToken::new(),
        };
        let error = sync_with_manager(
            &state,
            &create_authenticated_client("fake-token").unwrap(),
            &url,
            false,
            None,
        )
        .await
        .expect_err("failed secret observation remains an error");
        assert!(error
            .to_string()
            .contains("Failed to refresh deployer secrets"));
        let request = captured
            .lock()
            .unwrap()
            .take()
            .expect("manager was contacted");
        assert_eq!(
            request["currentState"]["runtimeMetadata"]["deployerSecrets"],
            serde_json::to_value(&running.runtime_metadata.as_ref().unwrap().deployer_secrets)
                .unwrap()
        );
        assert_eq!(
            state
                .db
                .get_deployment_state()
                .await
                .unwrap()
                .unwrap()
                .status,
            DeploymentStatus::DeletePending
        );
        server.abort();
    }

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
        captured_sync_request_with_receipt(config, None).await
    }

    async fn captured_sync_request_with_receipt(
        config: impl FnOnce(String, SyncConfig) -> SyncFixture,
        receipt: Option<alien_core::sync::SyncExecutionClaim>,
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
        if let Some(claim) = &receipt {
            db.set_sync_execution("receipt-session", Some(claim), None)
                .await
                .unwrap();
        }
        db.set_pending_observed_inventory_batches(&fixture.pending_inventory)
            .await
            .unwrap();
        let state = OperatorState {
            config: fixture.config,
            db: Arc::new(db),
            service_provider: fixture.service_provider,
            operations_sync_handler: None,
            pod_log_collection: false,
            cancel: CancellationToken::new(),
        };

        let client = create_authenticated_client(&sync_config.token).unwrap();
        sync_with_manager(&state, &client, sync_config.url.as_str(), false, None)
            .await
            .expect("sync with stub manager");
        let body = captured
            .lock()
            .unwrap()
            .take()
            .expect("stub manager received a sync request");
        if receipt.is_some() {
            // Reopen SQLite to prove the acknowledgment cleared the durable
            // receipt, then verify the next sync carries no execution claim.
            let reopened = OperatorDb::new(&state.config.data_dir, TEST_ENCRYPTION_KEY)
                .await
                .unwrap();
            let persisted = reopened.get_sync_execution().await.unwrap().unwrap();
            assert_eq!(persisted.session, "receipt-session");
            assert!(persisted.claim.is_none());
            assert!(persisted.target.is_none());
            sync_with_manager(&state, &client, sync_config.url.as_str(), false, None)
                .await
                .unwrap();
            let next = captured.lock().unwrap().take().unwrap();
            assert!(next.get("executionClaim").is_none());
            assert_eq!(next["session"], "receipt-session");
        }
        server.abort();
        body
    }

    /// How many times the test Storage controller's Ready handler has run.
    fn storage_ready_checks(state: &DeploymentState) -> u64 {
        state
            .stack_state
            .as_ref()
            .and_then(|stack| stack.resources.get("files"))
            .and_then(|resource| resource.internal_state.as_ref())
            .and_then(|internal| {
                internal
                    .get("readyChecks")
                    .or_else(|| internal.get("ready_checks"))
            })
            .and_then(Value::as_u64)
            .unwrap_or(0)
    }

    /// Runs the Operator's deployment loop until it has nothing left to do.
    async fn run_deployment_until_idle(state: &OperatorState) {
        for _ in 0..20 {
            let (steps, _) = super::super::deployment::run_deployment_continuously(state)
                .await
                .expect("deployment loop tick");
            if steps == 0 {
                return;
            }
        }
        panic!("the deployment loop kept stepping");
    }

    /// A pull Operator never ran Ready on a settled deployment: every maintenance
    /// claim was re-applied as an update, whose executor skips Running resources.
    /// Ready-time repairs (linked-binding drift, Kubernetes endpoint and health
    /// checks) therefore never ran on Operator deployments. This drives the real
    /// sync and deployment loops against a stub manager on the test platform.
    #[tokio::test]
    async fn a_maintenance_claim_runs_ready_on_the_installed_release() {
        type Response = Arc<Mutex<Value>>;
        async fn serve(State(response): State<Response>) -> Json<Value> {
            Json(response.lock().unwrap().clone())
        }

        let vault_dir = tempfile::tempdir().unwrap();
        std::env::set_var("TEST_VAULT_DATA_DIR", vault_dir.path());
        let response: Response = Arc::new(Mutex::new(json!({})));
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let address = listener.local_addr().unwrap();
        let app = Router::new()
            .route("/v1/sync", post(serve))
            .with_state(response.clone());
        let server = tokio::spawn(async move { axum::serve(listener, app).await });

        let data_dir = tempfile::tempdir().unwrap();
        let directory = data_dir.path().to_str().unwrap();
        let db = OperatorDb::new(directory, TEST_ENCRYPTION_KEY)
            .await
            .unwrap();
        db.set_deployment_id("dep_maintenance").await.unwrap();
        let state = OperatorState {
            config: OperatorConfig::builder()
                .platform(Platform::Test)
                .data_dir(directory)
                .encryption_key(TEST_ENCRYPTION_KEY)
                .build(),
            db: Arc::new(db),
            service_provider: None,
            operations_sync_handler: None,
            pod_log_collection: false,
            cancel: CancellationToken::new(),
        };
        let client = create_authenticated_client("ax_dep_test").unwrap();
        let url = format!("http://{address}");
        let release = ReleaseInfo {
            release_id: Some("rel_installed".to_string()),
            version: Some("1.0.0".to_string()),
            description: None,
            stack: Stack::new("maintenance".to_string())
                .add(
                    alien_core::Storage::new("files".to_string()).build(),
                    alien_core::ResourceLifecycle::Frozen,
                )
                .build(),
        };
        let config = alien_core::DeploymentConfig::builder()
            .stack_settings(Default::default())
            .environment_variables(alien_core::EnvironmentVariablesSnapshot {
                variables: Vec::new(),
                hash: String::new(),
                created_at: String::new(),
            })
            .allow_frozen_changes(false)
            .external_bindings(Default::default())
            .build();
        let target = json!({ "releaseInfo": release, "config": config });
        let claim = |operation: &str, attempt: &str| json!({ "operationId": operation, "attemptId": attempt });

        // The first apply installs the release and records its operation.
        *response.lock().unwrap() =
            json!({ "executionClaim": claim("duop_install", "att_1"), "target": target });
        sync_with_manager(&state, &client, &url, false, None)
            .await
            .unwrap();
        run_deployment_until_idle(&state).await;
        let installed = state.db.get_deployment_state().await.unwrap().unwrap();
        assert_eq!(installed.status, DeploymentStatus::Running);
        assert_eq!(
            state
                .db
                .get_applied_operation_id()
                .await
                .unwrap()
                .as_deref(),
            Some("duop_install")
        );
        let ready_before = storage_ready_checks(&installed);

        // Platform claims maintenance on the applied operation. The Operator keeps
        // the deployment Running and runs every Ready handler once.
        *response.lock().unwrap() =
            json!({ "executionClaim": claim("duop_install", "att_2"), "target": target });
        sync_with_manager(&state, &client, &url, false, None)
            .await
            .unwrap();
        let accepted = state.db.get_deployment_state().await.unwrap().unwrap();
        assert_eq!(accepted.status, DeploymentStatus::Running);
        run_deployment_until_idle(&state).await;
        let refreshed = state.db.get_deployment_state().await.unwrap().unwrap();
        assert_eq!(refreshed.status, DeploymentStatus::Running);
        assert!(
            storage_ready_checks(&refreshed) > ready_before,
            "maintenance must run Ready on the Running storage ({} -> {})",
            ready_before,
            storage_ready_checks(&refreshed)
        );
        assert!(refreshed.target_release.is_none());
        assert!(state.db.get_deployment_config().await.unwrap().is_none());
        assert_eq!(
            state
                .db
                .get_applied_operation_id()
                .await
                .unwrap()
                .as_deref(),
            Some("duop_install")
        );

        // New work under another operation, even for the same release (a config
        // change), is still applied as an update.
        *response.lock().unwrap() =
            json!({ "executionClaim": claim("duop_config", "att_1"), "target": target });
        sync_with_manager(&state, &client, &url, false, None)
            .await
            .unwrap();
        let update = state.db.get_deployment_state().await.unwrap().unwrap();
        assert_eq!(update.status, DeploymentStatus::UpdatePending);
        run_deployment_until_idle(&state).await;
        assert_eq!(
            state
                .db
                .get_applied_operation_id()
                .await
                .unwrap()
                .as_deref(),
            Some("duop_config")
        );

        server.abort();
        std::env::remove_var("TEST_VAULT_DATA_DIR");
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
    async fn empty_sync_acknowledgment_clears_the_durable_completed_receipt() {
        let body = captured_sync_request_with_receipt(
            |data_dir, sync| SyncFixture {
                config: OperatorConfig::builder()
                    .platform(Platform::Aws)
                    .operator_permission("diagnostics")
                    .sync(sync)
                    .data_dir(data_dir)
                    .encryption_key(TEST_ENCRYPTION_KEY)
                    .build(),
                status: DeploymentStatus::Running,
                pending_inventory: vec![],
                service_provider: None,
            },
            Some(alien_core::sync::SyncExecutionClaim {
                operation_id: "duop_receipt".into(),
                attempt_id: "duat_receipt".into(),
            }),
        )
        .await;
        assert_eq!(body["executionClaim"]["operationId"], "duop_receipt");
        assert_eq!(body["executionClaim"]["attemptId"], "duat_receipt");
    }

    #[tokio::test]
    async fn rejected_execution_claim_is_discarded_before_requesting_a_fresh_claim() {
        type Requests = Arc<Mutex<Vec<Value>>>;
        async fn serve(
            State((code, requests)): State<(String, Requests)>,
            Json(body): Json<Value>,
        ) -> (axum::http::StatusCode, Json<Value>) {
            requests.lock().unwrap().push(body.clone());
            if body
                .get("executionClaim")
                .is_some_and(|claim| claim["attemptId"] == "attempt_expired")
            {
                return (
                    axum::http::StatusCode::CONFLICT,
                    Json(json!({
                        "code": code, "message": "Execution claim rejected", "retryable": true,
                        "internal": false, "httpStatusCode": 409
                    })),
                );
            }
            (
                axum::http::StatusCode::OK,
                Json(json!({"executionClaim": {
                    "operationId": "operation_current", "attemptId": "attempt_fresh"
                }})),
            )
        }
        for code in [
            "DEPLOYMENT_LEASE_LOST",
            "DEPLOYMENT_UPDATE_CLAIM_LOST",
            "FORBIDDEN",
            "UNAVAILABLE",
        ] {
            let requests: Requests = Arc::new(Mutex::new(Vec::new()));
            let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
                .await
                .unwrap();
            let url = format!("http://{}", listener.local_addr().unwrap());
            let app = Router::new()
                .route("/v1/sync", post(serve))
                .with_state((code.to_string(), requests.clone()));
            let server = tokio::spawn(async move { axum::serve(listener, app).await });
            let directory = tempfile::tempdir().unwrap();
            let db = OperatorDb::new(directory.path().to_str().unwrap(), TEST_ENCRYPTION_KEY)
                .await
                .unwrap();
            db.set_deployment_id("dep_receipt").await.unwrap();
            let current: DeploymentState = serde_json::from_value(json!({
                "platform": "aws", "status": "running", "protocolVersion": CURRENT_DEPLOYMENT_PROTOCOL_VERSION
            })).unwrap();
            db.set_deployment_state(&current).await.unwrap();
            let claim = alien_core::sync::SyncExecutionClaim {
                operation_id: "operation_current".into(),
                attempt_id: "attempt_expired".into(),
            };
            let target: alien_core::sync::TargetDeployment = serde_json::from_value(json!({
                "releaseInfo": {"releaseId": "rel_current", "stack": {"id": "receipt", "resources": {}}},
                "config": {"deploymentName": "receipt", "environmentVariables": {"variables": [], "hash": "", "createdAt": "2026-01-01T00:00:00Z"}, "externalBindings": {}, "stackSettings": {"deploymentModel": "pull"}, "managerUrl": "https://manager.example.com"}
            })).unwrap();
            db.set_sync_execution("session_expired", Some(&claim), Some(&target))
                .await
                .unwrap();
            let state = OperatorState {
                config: OperatorConfig::builder()
                    .platform(Platform::Aws)
                    .operator_permission("diagnostics")
                    .data_dir(directory.path().to_str().unwrap())
                    .encryption_key(TEST_ENCRYPTION_KEY)
                    .build(),
                db: Arc::new(db),
                service_provider: None,
                operations_sync_handler: None,
                pod_log_collection: false,
                cancel: CancellationToken::new(),
            };
            let client = create_authenticated_client("ax_dep_test").unwrap();
            let error = sync_with_manager(&state, &client, &url, false, None)
                .await
                .unwrap_err();
            assert_eq!(error.code, code);
            let reopened = OperatorDb::new(directory.path().to_str().unwrap(), TEST_ENCRYPTION_KEY)
                .await
                .unwrap();
            let saved = reopened.get_sync_execution().await.unwrap().unwrap();
            assert_eq!(
                serde_json::to_value(reopened.get_deployment_state().await.unwrap().unwrap())
                    .unwrap(),
                serde_json::to_value(&current).unwrap()
            );
            if matches!(
                code,
                "DEPLOYMENT_LEASE_LOST" | "DEPLOYMENT_UPDATE_CLAIM_LOST"
            ) {
                assert!(
                    saved.claim.is_none(),
                    "Rejected claim must be discarded: {code}"
                );
                assert!(saved.target.is_none());
                assert_ne!(saved.session, "session_expired");
                sync_with_manager(&state, &client, &url, false, None)
                    .await
                    .unwrap();
                let requests = requests.lock().unwrap();
                assert_eq!(requests.len(), 2);
                assert!(requests[1].get("executionClaim").is_none());
                assert_eq!(requests[1]["session"], saved.session);
                drop(requests);
                assert_eq!(
                    reopened
                        .get_sync_execution()
                        .await
                        .unwrap()
                        .unwrap()
                        .claim
                        .unwrap()
                        .attempt_id,
                    "attempt_fresh"
                );
            } else {
                assert_eq!(saved.session, "session_expired");
                assert_eq!(saved.claim, Some(claim));
                assert_eq!(
                    serde_json::to_value(saved.target).unwrap(),
                    serde_json::to_value(Some(target)).unwrap()
                );
            }
            server.abort();
        }
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
    fn claims_dynamic_containers_only_when_the_chart_bound_their_role() {
        let config = |platform: Platform, namespace: Option<&str>, dynamic_containers: bool| {
            OperatorConfig::builder()
                .platform(platform)
                .maybe_namespace(namespace.map(str::to_string))
                .dynamic_containers(dynamic_containers)
                .data_dir("unused")
                .encryption_key(TEST_ENCRYPTION_KEY)
                .build()
        };
        let state = |config: OperatorConfig| {
            let capability = dynamic_containers_capability(&config);
            assert_eq!(capability.key, "dynamic-containers-v1");
            capability.state
        };

        assert_eq!(
            state(config(Platform::Kubernetes, Some("customer"), true)),
            OperatorCapabilityState::Granted
        );
        // A namespace install without the Role must not receive container targets.
        assert_eq!(
            state(config(Platform::Kubernetes, Some("customer"), false)),
            OperatorCapabilityState::Unavailable
        );
        assert_eq!(
            state(config(Platform::Kubernetes, None, true)),
            OperatorCapabilityState::Unavailable
        );
        assert_eq!(
            state(config(Platform::Aws, Some("customer"), true)),
            OperatorCapabilityState::Unavailable
        );
    }

    #[tokio::test]
    async fn completed_retention_survives_operator_restart_and_later_erase() {
        type Exchange = Arc<Mutex<(Value, Vec<Value>)>>;
        async fn serve(
            State(exchange): State<Exchange>,
            Json(request): Json<Value>,
        ) -> Json<Value> {
            let mut exchange = exchange.lock().unwrap();
            exchange.1.push(request["currentState"].clone());
            Json(exchange.0.clone())
        }
        let local = DeploymentState {
            platform: Platform::Test,
            status: DeploymentStatus::RetentionRequired,
            current_release: None,
            target_release: None,
            stack_state: None,
            environment_info: None,
            runtime_metadata: Some(alien_core::RuntimeMetadata::default()),
            error: None,
            retry_requested: false,
            protocol_version: CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
        };
        let mut completed = local.clone();
        completed.status = DeploymentStatus::Retained;
        completed
            .runtime_metadata
            .as_mut()
            .unwrap()
            .vault_credentials_revoked = true;
        let exchange: Exchange =
            Arc::new(Mutex::new((json!({"currentState": completed}), Vec::new())));
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let address = listener.local_addr().unwrap();
        let app = Router::new()
            .route("/v1/sync", post(serve))
            .with_state(exchange.clone());
        let server = tokio::spawn(async move { axum::serve(listener, app).await });
        let dir = tempfile::tempdir().unwrap();
        let sync = SyncConfig {
            url: format!("http://{address}").parse().unwrap(),
            token: "ax_dep_test".into(),
        };
        let config = OperatorConfig::builder()
            .platform(Platform::Test)
            .sync(sync.clone())
            .data_dir(dir.path().to_str().unwrap())
            .encryption_key(TEST_ENCRYPTION_KEY)
            .build();
        let db = Arc::new(
            OperatorDb::new(&config.data_dir, TEST_ENCRYPTION_KEY)
                .await
                .unwrap(),
        );
        db.set_deployment_id("dep_retained").await.unwrap();
        db.set_deployment_state(&local).await.unwrap();
        let state = OperatorState {
            config,
            db,
            service_provider: None,
            operations_sync_handler: None,
            pod_log_collection: false,
            cancel: CancellationToken::new(),
        };
        let client = create_authenticated_client(&sync.token).unwrap();
        sync_with_manager(&state, &client, sync.url.as_str(), false, None)
            .await
            .unwrap();
        let reopened = OperatorDb::new(&state.config.data_dir, TEST_ENCRYPTION_KEY)
            .await
            .unwrap();
        assert_eq!(
            serde_json::to_value(reopened.get_deployment_state().await.unwrap().unwrap()).unwrap(),
            serde_json::to_value(&completed).unwrap()
        );
        let restarted = OperatorState {
            db: Arc::new(reopened),
            ..state
        };
        sync_with_manager(&restarted, &client, sync.url.as_str(), false, None)
            .await
            .unwrap();
        {
            let exchange = exchange.lock().unwrap();
            assert_eq!(exchange.1.len(), 2);
            assert_eq!(exchange.1[0]["status"], "retention-required");
            assert_eq!(exchange.1[1], serde_json::to_value(&completed).unwrap());
        }
        let mut erase = completed;
        erase.status = DeploymentStatus::DeletePending;
        erase.runtime_metadata.as_mut().unwrap().deletion_policy =
            alien_core::DeletionPolicy::Erase;
        exchange.lock().unwrap().0 = json!({"currentState": erase});
        sync_with_manager(&restarted, &client, sync.url.as_str(), false, None)
            .await
            .unwrap();
        let current = restarted.db.get_deployment_state().await.unwrap().unwrap();
        assert_eq!(current.status, DeploymentStatus::DeletePending);
        let metadata = current.runtime_metadata.unwrap();
        assert_eq!(metadata.deletion_policy, alien_core::DeletionPolicy::Erase);
        assert!(metadata.vault_credentials_revoked);
        server.abort();
    }

    #[tokio::test]
    async fn protocol_upgrade_waits_for_persisted_manager_acknowledgement_across_restart() {
        type Response = Arc<Mutex<Value>>;
        async fn serve(
            State(response): State<Response>,
            Json(request): Json<Value>,
        ) -> Json<Value> {
            assert_eq!(
                request["currentState"]["protocolVersion"],
                alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION
            );
            assert_eq!(
                request["maxSupportedDeploymentProtocolVersion"],
                alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION
            );
            Json(response.lock().unwrap().clone())
        }
        let response: Response = Arc::new(Mutex::new(json!({})));
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let address = listener.local_addr().unwrap();
        let app = Router::new()
            .route("/v1/sync", post(serve))
            .with_state(response.clone());
        let server = tokio::spawn(async move { axum::serve(listener, app).await });
        let dir = tempfile::tempdir().unwrap();
        let sync = SyncConfig {
            url: format!("http://{address}").parse().unwrap(),
            token: "ax_dep_test".into(),
        };
        let config = OperatorConfig::builder()
            .platform(Platform::Test)
            .sync(sync.clone())
            .data_dir(dir.path().to_str().unwrap())
            .encryption_key(TEST_ENCRYPTION_KEY)
            .build();
        let db = Arc::new(
            OperatorDb::new(&config.data_dir, TEST_ENCRYPTION_KEY)
                .await
                .unwrap(),
        );
        db.set_deployment_id("dep_protocol").await.unwrap();
        let mut checkpoint = DeploymentState {
            platform: Platform::Test,
            status: DeploymentStatus::Provisioning,
            current_release: None,
            target_release: None,
            stack_state: None,
            error: None,
            environment_info: None,
            runtime_metadata: None,
            retry_requested: false,
            protocol_version: 1,
        };
        db.set_deployment_state(&checkpoint).await.unwrap();
        let state = OperatorState {
            config,
            db: db.clone(),
            service_provider: None,
            operations_sync_handler: None,
            pod_log_collection: false,
            cancel: CancellationToken::new(),
        };
        let client = create_authenticated_client(&sync.token).unwrap();
        // A successful HTTP response with no acknowledgement cannot enable writers.
        sync_with_manager(&state, &client, sync.url.as_str(), false, None)
            .await
            .unwrap();
        assert_eq!(
            db.get_deployment_state()
                .await
                .unwrap()
                .unwrap()
                .protocol_version,
            1
        );
        checkpoint.protocol_version = alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION;
        *response.lock().unwrap() = json!({"currentState": checkpoint});
        sync_with_manager(&state, &client, sync.url.as_str(), false, None)
            .await
            .unwrap();
        let reopened = OperatorDb::new(&state.config.data_dir, TEST_ENCRYPTION_KEY)
            .await
            .unwrap();
        assert_eq!(
            reopened
                .get_deployment_state()
                .await
                .unwrap()
                .unwrap()
                .protocol_version,
            alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION
        );
        // A delayed reply from an old manager cannot lower the durable version.
        checkpoint.protocol_version = 1;
        *response.lock().unwrap() = json!({"currentState": checkpoint});
        sync_with_manager(&state, &client, sync.url.as_str(), false, None)
            .await
            .unwrap();
        assert_eq!(
            db.get_deployment_state()
                .await
                .unwrap()
                .unwrap()
                .protocol_version,
            alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION
        );
        server.abort();
    }

    /// The `logs` capability describes the Kubernetes log collector. Operators
    /// on other platforms have none, and their workloads send logs without the
    /// Operator, so they must not report it: clients read `unavailable` as "this
    /// deployment sends no logs".
    #[tokio::test]
    async fn reports_logs_only_for_the_kubernetes_log_collector() {
        let data_dir = tempfile::tempdir().unwrap();
        let directory = data_dir.path().to_str().unwrap();
        let db = Arc::new(
            OperatorDb::new(directory, TEST_ENCRYPTION_KEY)
                .await
                .unwrap(),
        );
        let reported_logs =
            |platform: Platform, collector_token: Option<&str>, pod_log_collection: bool| {
                let state = OperatorState {
                    config: OperatorConfig::builder()
                        .platform(platform)
                        .namespace("customer".to_string())
                        .maybe_collector_token(collector_token.map(str::to_string))
                        .data_dir(directory)
                        .encryption_key(TEST_ENCRYPTION_KEY)
                        .build(),
                    db: db.clone(),
                    service_provider: None,
                    operations_sync_handler: None,
                    pod_log_collection,
                    cancel: CancellationToken::new(),
                };
                report_operator_capabilities(&state, false)
                    .into_iter()
                    .find(|capability| capability.key == "logs")
                    .map(|capability| (capability.state, capability.detail))
            };

        // Collector off: no node collector token and no Pod log stream.
        assert_eq!(
            reported_logs(Platform::Kubernetes, None, false),
            Some((
                OperatorCapabilityState::Unavailable,
                Some("No log collector configured".to_string())
            ))
        );
        // nodeAgent mode: the chart mounts the collector token.
        assert_eq!(
            reported_logs(Platform::Kubernetes, Some("collector-secret"), false),
            Some((
                OperatorCapabilityState::Granted,
                Some("Pod logs sent by the node log collector".to_string())
            ))
        );
        // podApi mode: the chart sets the Pod log label and mounts no token.
        assert_eq!(
            reported_logs(Platform::Kubernetes, None, true),
            Some((
                OperatorCapabilityState::Granted,
                Some("Pod logs read through the Kubernetes API".to_string())
            ))
        );

        // Cloud and local Operators have no log collector to report on.
        for platform in [
            Platform::Aws,
            Platform::Gcp,
            Platform::Azure,
            Platform::Local,
        ] {
            assert_eq!(reported_logs(platform, None, false), None, "{platform}");
        }
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
    fn copies_manager_erase_policy_into_existing_runtime_metadata() {
        let local_metadata = alien_core::RuntimeMetadata {
            last_synced_env_vars_hash: Some("local-checkpoint".to_string()),
            ..Default::default()
        };
        let mut local_state = DeploymentState {
            platform: Platform::Local,
            status: DeploymentStatus::Running,
            current_release: None,
            target_release: None,
            stack_state: None,
            error: None,
            environment_info: None,
            runtime_metadata: Some(local_metadata),
            retry_requested: false,
            protocol_version: CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
        };
        let manager_state = DeploymentState {
            status: DeploymentStatus::DeletePending,
            runtime_metadata: Some(alien_core::RuntimeMetadata {
                deletion_policy: alien_core::DeletionPolicy::Erase,
                ..Default::default()
            }),
            ..local_state.clone()
        };

        assert!(apply_manager_control_state(
            &mut local_state,
            &manager_state
        ));
        assert_eq!(local_state.status, DeploymentStatus::DeletePending);
        let metadata = local_state.runtime_metadata.unwrap();
        assert_eq!(metadata.deletion_policy, alien_core::DeletionPolicy::Erase);
        assert_eq!(
            metadata.last_synced_env_vars_hash.as_deref(),
            Some("local-checkpoint")
        );
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
