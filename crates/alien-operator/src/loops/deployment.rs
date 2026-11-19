//! Deployment loop - runs step() when updates are available
//!
//! This loop checks for pending updates in the local database and runs
//! alien-deployment::step() to deploy changes. Unlike the manager,
//! the Operator uses local credentials since it runs in the target environment.
//!
//! Storage model:
//! - `deployment_state` key: Full DeploymentState (includes target_release and current_release)
//! - `deployment_config` key: DeploymentConfig for the target deployment
//!
//! The loop runs steps continuously, respecting the suggested delay between each step,
//! until the deployment is synced (Running, Failed, or Deleted).

use crate::config::OperatorConfig;
use crate::db::{AutomaticRetry, OperatorDb};
use crate::OperatorState;
use alien_core::{
    commands_types::CommandTargetType, ClientConfig, DeploymentConfig, DeploymentState,
    DeploymentStatus, EnvironmentVariable, EnvironmentVariableType, KubernetesClientConfig,
    ObservedInventoryBatch, Platform, ReceiverCommandCredential, ResourceHeartbeat,
    ENV_ALIEN_DEPLOYMENT_ID, ENV_ALIEN_DEPLOYMENT_NAME,
};
use alien_deployment::loop_contract::{LoopOperation, LoopOutcome, LoopStopReason};
use alien_deployment::runner::{RunnerPolicy, RunnerResult};
use alien_deployment::transport::{DeploymentLoopTransport, StepReconcileResult};
use alien_error::{AlienError, Context};
use alien_infra::ClientConfigExt;
use async_trait::async_trait;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc,
};
use std::time::Duration;
use tracing::{debug, error, info, warn};

use crate::error::{ErrorData, Result};

/// Transport implementation that persists state to the operator's local DB
/// and re-reads config each step to pick up sync loop changes.
/// How an apply checkpoint ended.
#[derive(Debug, PartialEq, Eq)]
enum ApplyCheckpoint {
    /// The step's state is stored.
    Stored,
    /// A newer target was stored on top of the step's state: end the pass.
    Adopted,
    /// The stored state kept changing: nothing was written, end the pass.
    Contended,
}

/// Store an apply step's state by compare and set. When the sync loop changed the
/// stored state since this pass last wrote it, a different target is newer work:
/// it is stored on top of the step's result, so the next pass runs every gate
/// (approval, signature, retry budget) for it. Each retry reads the stored state
/// afresh and writes against exactly that read, so a change landing in between is
/// never overwritten. `after_read` runs between a read and the write that follows
/// it (a seam for tests).
async fn checkpoint_apply(
    db: &OperatorDb,
    written: &DeploymentState,
    state: &DeploymentState,
    mut after_read: impl AsyncFnMut(&DeploymentState),
) -> std::result::Result<ApplyCheckpoint, AlienError> {
    let mut expected = written.clone();
    let mut to_write = state.clone();
    let mut adopted = false;
    for _ in 0..APPLY_CHECKPOINT_ATTEMPTS {
        if db
            .compare_and_set_deployment_state(&expected, &to_write)
            .await
            .map_err(|e| e.into_generic())?
        {
            return Ok(if adopted {
                ApplyCheckpoint::Adopted
            } else {
                ApplyCheckpoint::Stored
            });
        }
        let Some(stored) = db
            .get_deployment_state()
            .await
            .map_err(|e| e.into_generic())?
        else {
            break;
        };
        after_read(&stored).await;
        to_write = state.clone();
        adopted = false;
        if let Some(newer) = stored
            .target_release
            .clone()
            .filter(|stored| written.target_release.as_ref() != Some(stored))
        {
            info!(
                release_id = ?newer.release_id,
                "A newer target arrived while the deployment was applying; ending this pass"
            );
            super::sync::accept_target_release(&mut to_write, newer);
            adopted = true;
        }
        expected = stored;
    }
    Ok(ApplyCheckpoint::Contended)
}

/// Tries an apply checkpoint makes against a stored state that keeps changing.
const APPLY_CHECKPOINT_ATTEMPTS: usize = 5;

struct OperatorTransport {
    db: Arc<OperatorDb>,
    operator_config: OperatorConfig,
    platform: Platform,
    suggested_delay_ms: AtomicU64,
    /// Set for a maintenance refresh: the state its last checkpoint saw. A
    /// refresh is an observation and must not overwrite a newer target the sync
    /// loop accepted meanwhile, so its checkpoints compare and set.
    observed_from: Option<tokio::sync::Mutex<DeploymentState>>,
    /// Whether a compare-and-set checkpoint found newer state.
    superseded: AtomicBool,
    /// Set for an apply: the state this loop last wrote. The sync loop can accept a
    /// newer target while a step runs, so each checkpoint compares and sets; when
    /// it finds a newer target it stores it and ends the pass (`superseded`).
    applying_from: Option<tokio::sync::Mutex<DeploymentState>>,
}

#[async_trait]
impl DeploymentLoopTransport for OperatorTransport {
    async fn reconcile_step(
        &self,
        _deployment_id: &str,
        state: &DeploymentState,
        _config: &alien_core::DeploymentConfig,
        _update_heartbeat: bool,
        suggested_delay_ms: Option<u64>,
        heartbeats: Vec<ResourceHeartbeat>,
        observed_inventory_batches: Vec<ObservedInventoryBatch>,
    ) -> std::result::Result<StepReconcileResult, AlienError> {
        self.suggested_delay_ms
            .store(suggested_delay_ms.unwrap_or(0), Ordering::Relaxed);
        let previous = self
            .db
            .get_deployment_state()
            .await
            .map_err(|e| e.into_generic())?;
        if self.operator_config.sync.is_some()
            && previous
                .as_ref()
                .is_some_and(|previous| previous.protocol_version < state.protocol_version)
        {
            self.db.request_sync();
            return Err(AlienError::new(ErrorData::SyncFailed {
                message: "Waiting for the manager to acknowledge the deployment protocol upgrade"
                    .to_string(),
            })
            .into_generic());
        }
        let reports_changed = previous
            .as_ref()
            .and_then(|state| state.runtime_metadata.as_ref())
            .map(|metadata| &metadata.deployer_secrets)
            != state
                .runtime_metadata
                .as_ref()
                .map(|metadata| &metadata.deployer_secrets);
        // Persist state to local DB after each step
        match &self.observed_from {
            Some(observed_from) => {
                let mut observed_from = observed_from.lock().await;
                if !self
                    .db
                    .compare_and_set_deployment_state(&observed_from, state)
                    .await
                    .map_err(|e| e.into_generic())?
                {
                    self.superseded.store(true, Ordering::Relaxed);
                    return Err(AlienError::new(ErrorData::MaintenanceSuperseded).into_generic());
                }
                *observed_from = state.clone();
            }
            None => match &self.applying_from {
                Some(applying_from) => {
                    let mut written = applying_from.lock().await;
                    // Compare and set against what is stored; a newer target the
                    // sync loop stored meanwhile ends the pass (see `checkpoint_apply`).
                    if checkpoint_apply(&self.db, &written, state, async |_| {}).await?
                        != ApplyCheckpoint::Stored
                    {
                        self.superseded.store(true, Ordering::Relaxed);
                        return Err(
                            AlienError::new(ErrorData::MaintenanceSuperseded).into_generic()
                        );
                    }
                    *written = state.clone();
                }
                None => self
                    .db
                    .set_deployment_state(state)
                    .await
                    .map_err(|e| e.into_generic())?,
            },
        }

        if !heartbeats.is_empty() {
            self.db
                .set_pending_heartbeats(&heartbeats)
                .await
                .map_err(|e| e.into_generic())?;
        }
        if !observed_inventory_batches.is_empty() {
            self.db
                .set_pending_observed_inventory_batches(&observed_inventory_batches)
                .await
                .map_err(|e| e.into_generic())?;
        }

        if reports_changed {
            self.db.request_sync();
        }

        // Re-read config from DB (sync loop may have updated it)
        let config = self
            .db
            .get_deployment_config()
            .await
            .map_err(|e| e.into_generic())?;

        let stack = state
            .target_release
            .as_ref()
            .or(state.current_release.as_ref())
            .map(|release| &release.stack);

        let enriched_config = match config {
            Some(config) => Some(
                enrich_config(
                    config,
                    &self.operator_config,
                    self.platform,
                    &self.db,
                    stack,
                )
                .await
                .map_err(|e| e.into_generic())?,
            ),
            None => None,
        };

        Ok(StepReconcileResult {
            state: None,
            config: enriched_config,
        })
    }
}

/// Drops a setup update authorization minted for a different installed setup.
///
/// The manager mints an authorization when the installed chart reports a setup.
/// If the chart is rolled back afterwards, this Operator restarts with the older
/// fingerprint and must not apply an authorization that belongs to the newer
/// chart. Without a recorded setup (not installed from a chart that carries one)
/// the authorization is left as it is.
fn bind_setup_authorization(
    state: &mut DeploymentState,
    installed: Option<&alien_core::sync::InstalledSetup>,
) {
    let Some(installed) = installed else {
        return;
    };
    let Some(metadata) = state.runtime_metadata.as_mut() else {
        return;
    };
    let Some(authorization) = metadata.setup_update_authorization.as_ref() else {
        return;
    };
    if authorization.setup_fingerprint != installed.setup_fingerprint {
        // The loop runs every second; say it once per authorization.
        static WARNED: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);
        let mut warned = WARNED
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if warned.as_deref() != Some(authorization.nonce.as_str()) {
            warn!(
                "Ignoring a setup update authorization for a different installed setup; upgrade to the chart version the deployment is waiting for"
            );
            *warned = Some(authorization.nonce.clone());
        }
        metadata.setup_update_authorization = None;
    }
}

/// Run the deployment loop
///
/// This loop:
/// 1. Checks local database for pending updates (set by sync loop)
/// 2. Checks approval status if manual approval is required
/// 3. Runs alien-deployment::runner::run_step_loop() with OperatorTransport
/// 4. OperatorTransport persists state and re-reads config after each step
/// 5. Sync loop will pick up changes and report to manager
pub async fn run_deployment_loop(state: Arc<OperatorState>) {
    run_deployment_loop_observed(state, |_| {}).await
}

/// [`run_deployment_loop`], calling `after_pass` with the wait it chose after
/// every pass.
async fn run_deployment_loop_observed(
    state: Arc<OperatorState>,
    mut after_pass: impl FnMut(Duration),
) {
    let interval = Duration::from_secs(state.config.deployment_interval_seconds);

    info!(
        interval_seconds = state.config.deployment_interval_seconds,
        "Starting deployment loop"
    );

    let mut hold = HoldBackoff::default();
    loop {
        let delay = run_deployment_pass(&state, interval, &mut hold).await;
        after_pass(delay);

        tokio::select! {
            _ = tokio::time::sleep(delay) => {},
            // A target, an approval decision or a retry request is a reason to
            // act now, whatever the hold interval.
            _ = state.db.wait_for_deployment() => hold.reset(),
            _ = state.cancel.cancelled() => {
                info!("Deployment loop shutting down");
                return;
            }
        }
    }
}

/// First wait of a deployment that holds in a status with nothing to do.
const HOLD_FLOOR: Duration = Duration::from_secs(5);
/// Longest wait of a held deployment.
const HOLD_CAP: Duration = Duration::from_secs(60);

/// Statuses where the Operator has nothing left to do: the deployment waits for
/// something outside this loop (the manager's setup teardown, a retry request, a
/// new target), or is gone. The Delete operation ends the pass in each at once,
/// without a step (`classify_status`), or with the status's no-op handler.
fn is_hold_status(status: DeploymentStatus) -> bool {
    matches!(
        status,
        DeploymentStatus::Deleted
            | DeploymentStatus::TeardownRequired
            | DeploymentStatus::RetentionRequired
            | DeploymentStatus::Retained
            | DeploymentStatus::TeardownFailed
    )
}

/// How long the loop waits between passes while the deployment holds: 5 s,
/// doubling to 60 s, back to 5 s after a wake.
#[derive(Debug, Default)]
pub(crate) struct HoldBackoff {
    next: Option<Duration>,
}

impl HoldBackoff {
    fn next_delay(&mut self) -> Duration {
        let delay = self.next.unwrap_or(HOLD_FLOOR);
        self.next = Some((delay * 2).min(HOLD_CAP));
        delay
    }

    fn reset(&mut self) {
        self.next = None;
    }
}

/// The last deployment result logged at info. The deployment loop is the only
/// writer, so one value for the process is enough.
static LAST_LOGGED_OUTCOME: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// Whether `outcome` differs from the last one reported, and records it.
fn outcome_changed(outcome: &str) -> bool {
    let mut last = LAST_LOGGED_OUTCOME
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if last.as_deref() == Some(outcome) {
        return false;
    }
    *last = Some(outcome.to_string());
    true
}

/// One pass of the deployment loop. Returns how long the loop waits before the
/// next pass: the step's requested delay, never less than `interval`.
async fn run_deployment_pass(
    state: &OperatorState,
    interval: Duration,
    hold: &mut HoldBackoff,
) -> Duration {
    let delay = match run_deployment_continuously(state).await {
        Ok((steps, suggested_delay)) => {
            if steps > 0 {
                // A running deployment takes a health step every pass; the
                // terminal-state log reports when its outcome changes.
                debug!(steps = steps, "Deployment pass completed");
            }
            interval.max(suggested_delay)
        }
        Err(e) => {
            error!(error = %e, "Deployment failed");
            interval
        }
    };
    match state.db.get_deployment_state().await {
        Ok(Some(current)) if is_hold_status(current.status) => delay.max(hold.next_delay()),
        _ => {
            hold.reset();
            delay
        }
    }
}

/// Run deployment steps continuously until synced or long delay.
/// Returns the number of steps executed and the delay before resuming.
pub(crate) async fn run_deployment_continuously(
    state: &OperatorState,
) -> Result<(usize, Duration)> {
    // Get deployment state and config
    let mut current = match state.db.get_deployment_state().await? {
        Some(s) => s,
        None => return Ok((0, Duration::ZERO)),
    };

    bind_setup_authorization(&mut current, state.config.installed_setup.as_ref());

    // Check if there's a target to deploy
    let target_release = match &current.target_release {
        Some(r) => r,
        None => {
            let delay = if crate::daily_identity::has_runtime_identity(&state.db).await? {
                refresh_installed(state).await?
            } else {
                Duration::ZERO
            };
            return Ok((0, delay));
        }
    };
    // A pushed target always carries an Alien release id (observe deployments never
    // receive a target), so an empty fallback is unreachable in practice.
    let target_release_id = target_release.release_id.as_deref().unwrap_or_default();

    // Check deployment approval status if required
    if state.config.requires_deployment_approval() {
        match state
            .db
            .get_approval_status_for_release(target_release_id)
            .await?
        {
            Some(crate::db::ApprovalStatus::Pending) => {
                debug!("Target release pending approval, skipping deployment");
                renew_installed_credentials_or_warn(state).await;
                return Ok((0, Duration::ZERO));
            }
            Some(crate::db::ApprovalStatus::Rejected) => {
                info!("Target release was rejected, clearing target");
                current.target_release = None;
                state.db.set_deployment_state(&current).await?;
                state.db.clear_deployment_config().await?;
                return Ok((0, Duration::ZERO));
            }
            Some(crate::db::ApprovalStatus::Auto)
            | Some(crate::db::ApprovalStatus::Approved)
            | None => {
                // Proceed with deployment
            }
        }
    }

    debug!(
        release_id = %target_release_id,
        "Found target release to deploy"
    );

    // The delete-side statuses run the Delete operation, which ends the pass at
    // once in the four hold statuses (see `is_hold_status`): as Deploy they are
    // not terminal and the step loop would spin on them.
    let operation = if matches!(
        current.status,
        alien_core::DeploymentStatus::DeletePending
            | alien_core::DeploymentStatus::Deleting
            | alien_core::DeploymentStatus::DeleteFailed
    ) || is_hold_status(current.status)
    {
        LoopOperation::Delete
    } else {
        LoopOperation::Deploy
    };

    // Protected: whatever wrote the stored target, deploy it only if it is
    // still exactly the signed release. Deleting needs no signature.
    if let Some(signing) = &state.config.release_signing {
        if operation == LoopOperation::Deploy {
            let stored =
                state.db.get_signing_trust().await?.unwrap_or_else(|| {
                    crate::protected::OperatorTrust::new(&signing.settings.prefix)
                });
            if let Err(refusal) = signing.verify_release(current.platform, target_release, &stored)
            {
                error!(
                    release_id = %target_release_id,
                    reason = %refusal,
                    "Protected mode: refused to deploy the stored target; nothing was changed"
                );
                return Ok((0, Duration::ZERO));
            }
        }
    }

    // Get deployment ID (used for logging in runner)
    let deployment_id = state
        .db
        .get_deployment_id()
        .await?
        .unwrap_or_else(|| "unknown".to_string());

    // Build initial enriched config
    let base_config = match state.db.get_deployment_config().await? {
        Some(c) => c,
        None => {
            debug!("No deployment config found, skipping");
            return Ok((0, Duration::ZERO));
        }
    };
    if !current.retry_requested
        && matches!(
            current.status,
            DeploymentStatus::ProvisioningFailed | DeploymentStatus::UpdateFailed
        )
    {
        match state
            .db
            .prepare_automatic_retry(&current, &base_config, chrono::Utc::now())
            .await?
        {
            AutomaticRetry::Scheduled {
                attempt,
                delay_seconds,
            } => {
                info!(release_id = %target_release_id, attempt, delay_seconds,
                    "Scheduled automatic retry of failed deployment");
                renew_installed_credentials_or_warn(state).await;
                return Ok((0, Duration::ZERO));
            }
            AutomaticRetry::Requested { attempt } => {
                current.retry_requested = true;
                info!(release_id = %target_release_id, attempt,
                    "Resuming failed deployment from saved checkpoints");
            }
            AutomaticRetry::Idle => {
                renew_installed_credentials_or_warn(state).await;
                return Ok((0, Duration::ZERO));
            }
        }
    }
    // Identifies this apply once it completes; the config itself is moved into
    // enrichment below and cleared after an online apply.
    let applying_target_hash =
        crate::db::approved_target_hash(current.target_release.as_ref(), &base_config)?;
    let mut enriched_config = enrich_config(
        base_config,
        &state.config,
        current.platform,
        &state.db,
        Some(&target_release.stack),
    )
    .await?;

    // Resolve client config once (it doesn't change between steps)
    let client_config = resolve_client_config(
        current.platform,
        enriched_config.base_platform.or(state.config.base_platform),
        &state.config.data_dir,
        state.config.namespace.clone(),
        state.config.sync.as_ref(),
    )
    .await?;

    let policy = RunnerPolicy {
        max_steps: 100,
        operation,
        delay_strategy: alien_deployment::runner::DelayStrategy::Yield,
    };

    // A target held while Running or RefreshFailed on the same release is
    // maintenance of the installed release (see `sync::accept_target`). Run one
    // Running refresh step, as the manager does for each maintenance claim of a
    // push deployment: every Ready handler runs, so health checks and in-place
    // repairs such as a linked-binding refresh happen. A repair a handler starts
    // continues on the next maintenance claim.
    let refresh = operation == LoopOperation::Deploy
        && matches!(
            current.status,
            DeploymentStatus::Running | DeploymentStatus::RefreshFailed
        )
        && current
            .current_release
            .as_ref()
            .and_then(|release| release.release_id.as_deref())
            == Some(target_release_id);

    let service_provider = crate::extension_bundles::provider_for(
        state.service_provider.clone(),
        &crate::extension_bundles::cache(&state.config.data_dir),
        &current,
        &deployment_id,
        &client_config,
    )
    .await?;

    let transport = OperatorTransport {
        db: Arc::clone(&state.db),
        operator_config: state.config.clone(),
        platform: current.platform,
        suggested_delay_ms: AtomicU64::new(0),
        observed_from: refresh.then(|| tokio::sync::Mutex::new(current.clone())),
        superseded: AtomicBool::new(false),
        applying_from: (operation == LoopOperation::Deploy)
            .then(|| tokio::sync::Mutex::new(current.clone())),
    };

    if refresh {
        // Captured before stepping: the sync loop replaces the durable claim
        // once it reports this refresh complete.
        let answers_claim = state
            .db
            .get_sync_execution()
            .await?
            .is_some_and(|execution| execution.claim.is_some());
        let steps = match run_maintenance_refresh(
            service_provider,
            &mut current,
            &mut enriched_config,
            &client_config,
            &deployment_id,
            &policy,
            &transport,
        )
        .await
        {
            Ok(steps) => steps,
            Err(_) if transport.superseded.load(Ordering::Relaxed) => {
                debug!("Maintenance refresh superseded by newer deployment state");
                return Ok((0, Duration::ZERO));
            }
            Err(error) => return Err(error),
        };
        finish_maintenance_refresh(state, &current, answers_claim).await?;
        return Ok((steps, running_check_delay(&transport)));
    }

    // The claim whose target this loop applies. Captured before stepping: the
    // sync loop replaces the durable claim once it reports this apply complete.
    let applying_operation_id = state
        .db
        .get_sync_execution()
        .await?
        .and_then(|execution| execution.claim)
        .map(|claim| claim.operation_id);

    // An update can wait for a long time, for example on a rollout that never
    // becomes ready. The step loop does not reach the workloads it is not
    // changing, so renew the installed credentials on every pass.
    if operation == LoopOperation::Deploy {
        renew_installed_credentials_or_warn(state).await;
    }

    let result = alien_deployment::runner::run_step_loop(
        &mut current,
        &mut enriched_config,
        &client_config,
        &deployment_id,
        &policy,
        &transport,
        service_provider,
        None,
    )
    .await;
    if transport.superseded.load(Ordering::Relaxed) {
        // A newer target was stored mid-pass: nothing of this pass is kept (the
        // applying hash, the operation id, the config built for the old target).
        debug!("Apply superseded by a newer target; the next pass starts it");
        return Ok((0, Duration::ZERO));
    }
    let result = result.context(ErrorData::DeploymentFailed {
        message: "Deployment step loop failed".to_string(),
    })?;

    let RunnerResult {
        loop_result,
        steps_executed,
        ..
    } = result;

    if loop_result.outcome == LoopOutcome::Neutral {
        debug!(
            status = ?loop_result.final_status,
            stop_reason = ?loop_result.stop_reason,
            steps = steps_executed,
            "Deployment step loop yielded"
        );
    } else if loop_result.stop_reason != LoopStopReason::Handoff {
        if loop_result.outcome == LoopOutcome::Success {
            if operation == LoopOperation::Deploy
                && loop_result.final_status == DeploymentStatus::Running
            {
                if let Some(operation_id) = applying_operation_id.as_deref() {
                    state.db.set_applied_operation_id(operation_id).await?;
                }
                state
                    .db
                    .set_installed_target_hash(&applying_target_hash)
                    .await?;
            }
            // A disconnected Operator needs the installed config for health refreshes.
            // A pass that held without a step finished nothing: the sync loop
            // keeps sending the target, so clearing its config would only be
            // undone by the next sync.
            let held_without_step = steps_executed == 0 && is_hold_status(loop_result.final_status);
            if !held_without_step
                && ((state.config.airgap_target_secret.is_none()
                    && enriched_config.runtime_identity_audience.is_none())
                    || loop_result.final_status != DeploymentStatus::Running)
            {
                debug!("Deployment synced, clearing deployment config");
                state.db.clear_deployment_config().await?;
            }
        }

        if steps_executed > 0 {
            // Every pass of a healthy deployment ends here, once per interval:
            // report at info only when the result differs from the last one. The target
            // is part of the result: a new release that ends the same way is new work.
            let level_is_info = outcome_changed(&format!(
                "{:?}/{:?}/{:?}/{}",
                loop_result.final_status,
                loop_result.stop_reason,
                loop_result.outcome,
                applying_target_hash
            ));
            if level_is_info {
                info!(
                    status = ?loop_result.final_status,
                    stop_reason = ?loop_result.stop_reason,
                    outcome = ?loop_result.outcome,
                    steps = steps_executed,
                    "Deployment reached terminal state"
                );
            } else {
                debug!(
                    status = ?loop_result.final_status,
                    stop_reason = ?loop_result.stop_reason,
                    outcome = ?loop_result.outcome,
                    steps = steps_executed,
                    "Deployment reached terminal state"
                );
            }
        }
    }

    // Honor the step's delay for every wait, resource waits included. A
    // controller's poll budget (`Stay { max_times }`) counts steps, so its
    // author's wall-clock budget is `max_times` times the delay it asks for;
    // stepping on a shorter interval spends that budget faster.
    let delay = if loop_result.stop_reason == LoopStopReason::Delayed {
        Duration::from_millis(transport.suggested_delay_ms.load(Ordering::Relaxed))
    } else {
        Duration::ZERO
    };
    Ok((steps_executed, delay))
}

/// Refresh an installed deployment for health reporting and bounded credential renewal.
pub(super) async fn refresh_installed_deployment(state: &OperatorState) -> Result<()> {
    refresh_installed(state).await.map(|_| ())
}

/// How long the loop waits before the next Running check: the controllers' own
/// delay (30 s for a ready cluster or container, 5 s while one needs a closer
/// look), never under [`RUNNING_CHECK_FLOOR`] nor over [`RUNNING_CHECK_CAP`].
fn running_check_delay(transport: &OperatorTransport) -> Duration {
    clamp_running_check_delay(Duration::from_millis(
        transport.suggested_delay_ms.load(Ordering::Relaxed),
    ))
}

fn clamp_running_check_delay(requested: Duration) -> Duration {
    requested.clamp(RUNNING_CHECK_FLOOR, RUNNING_CHECK_CAP)
}

/// No Running check starts sooner than this after the last one.
const RUNNING_CHECK_FLOOR: Duration = Duration::from_secs(5);
/// A Running check never waits longer than this, whatever a controller asks.
const RUNNING_CHECK_CAP: Duration = Duration::from_secs(60);

/// [`refresh_installed_deployment`], returning how long to wait before the next check.
async fn refresh_installed(state: &OperatorState) -> Result<Duration> {
    let Some(mut current) = state.db.get_deployment_state().await? else {
        return Ok(Duration::ZERO);
    };
    if current.target_release.is_some()
        || !matches!(
            current.status,
            DeploymentStatus::Running | DeploymentStatus::RefreshFailed
        )
    {
        return renew_installed_credentials(state)
            .await
            .map(|()| Duration::ZERO);
    }
    let Some(release) = current.current_release.as_ref() else {
        return Ok(Duration::ZERO);
    };
    let Some(config) = state.db.get_deployment_config().await? else {
        if state.db.get_airgap_receipt().await?.is_none()
            && state.db.get_airgap_sequence().await?.is_some()
        {
            // Completed older installations discarded config. Preserve their status
            // until a newer verified target provides enough context to refresh.
            return Ok(Duration::ZERO);
        }
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: "The installed deployment config is unavailable for health refresh"
                .to_string(),
        }));
    };
    let mut config = enrich_config(
        config,
        &state.config,
        current.platform,
        &state.db,
        Some(&release.stack),
    )
    .await?;
    let client_config = resolve_client_config(
        current.platform,
        config.base_platform.or(state.config.base_platform),
        &state.config.data_dir,
        state.config.namespace.clone(),
        state.config.sync.as_ref(),
    )
    .await?;
    let deployment_id = state.db.get_deployment_id().await?.ok_or_else(|| {
        AlienError::new(ErrorData::ConfigurationError {
            message: "The deployment identity is unavailable for health refresh".to_string(),
        })
    })?;
    // An air-gapped Operator gets no downloads, so this is the bundle it already holds.
    let service_provider = crate::extension_bundles::provider_for(
        state.service_provider.clone(),
        &crate::extension_bundles::cache(&state.config.data_dir),
        &current,
        &deployment_id,
        &client_config,
    )
    .await?;
    let transport = OperatorTransport {
        db: Arc::clone(&state.db),
        operator_config: state.config.clone(),
        platform: current.platform,
        suggested_delay_ms: AtomicU64::new(0),
        observed_from: Some(tokio::sync::Mutex::new(current.clone())),
        superseded: AtomicBool::new(false),
        applying_from: None,
    };
    let policy = RunnerPolicy {
        max_steps: 100,
        operation: LoopOperation::Deploy,
        delay_strategy: alien_deployment::runner::DelayStrategy::Yield,
    };
    let result = run_maintenance_refresh(
        service_provider,
        &mut current,
        &mut config,
        &client_config,
        &deployment_id,
        &policy,
        &transport,
    )
    .await;
    match result {
        Err(_) if transport.superseded.load(Ordering::Relaxed) => Ok(Duration::ZERO),
        result => result.map(|_| running_check_delay(&transport)),
    }
}

/// Renew the credential Secrets of the installed workloads when the installed
/// release cannot be refreshed: an update waits for approval, waits to retry,
/// runs, or failed. Workloads keep running on their old revision, so their
/// credentials must not expire meanwhile.
async fn renew_installed_credentials(state: &OperatorState) -> Result<()> {
    if !crate::daily_identity::has_runtime_identity(&state.db).await? {
        return Ok(());
    }
    let Some(current) = state.db.get_deployment_state().await? else {
        return Ok(());
    };
    let (Some(stack_state), Some(release)) = (&current.stack_state, &current.current_release)
    else {
        return Ok(());
    };
    // The stored config belongs to the target while an update is pending, so it
    // is prepared with the target's stack: the installed release may have a
    // command receiver the update replaces, and the update's config carries no
    // key for it. Renewal only rewrites the capability-file and pull Secrets
    // from the installed stack; it never changes a receiver's commands key.
    let Some(config) = state.db.get_deployment_config().await? else {
        return Ok(());
    };
    let config_release = current.target_release.as_ref().unwrap_or(release);
    let config = enrich_config(
        config,
        &state.config,
        current.platform,
        &state.db,
        Some(&config_release.stack),
    )
    .await?;
    let client_config = resolve_client_config(
        current.platform,
        config.base_platform.or(state.config.base_platform),
        &state.config.data_dir,
        state.config.namespace.clone(),
        state.config.sync.as_ref(),
    )
    .await?;
    let service_provider = state
        .service_provider
        .clone()
        .unwrap_or_else(|| Arc::new(alien_infra::DefaultPlatformServiceProvider::default()));
    alien_infra::refresh_installed_credentials(
        stack_state,
        &release.stack,
        &config,
        client_config,
        &service_provider,
    )
    .await
    .context(ErrorData::DeploymentFailed {
        message: "Cannot renew installed workload credentials".to_string(),
    })
}

/// A renewal failure never blocks the update work that follows.
async fn renew_installed_credentials_or_warn(state: &OperatorState) {
    if let Err(error) = renew_installed_credentials(state).await {
        tracing::warn!(error = %error, "Cannot renew installed workload credentials");
    }
}

/// One Running refresh pass over the installed release.
async fn run_maintenance_refresh(
    service_provider: Option<Arc<dyn alien_infra::PlatformServiceProvider>>,
    current: &mut DeploymentState,
    config: &mut DeploymentConfig,
    client_config: &ClientConfig,
    deployment_id: &str,
    policy: &RunnerPolicy,
    transport: &OperatorTransport,
) -> Result<usize> {
    let result = alien_deployment::runner::run_running_refresh_step_loop(
        current,
        config,
        client_config,
        deployment_id,
        policy,
        transport,
        service_provider,
        None,
    )
    .await
    .context(ErrorData::DeploymentFailed {
        message: "Maintenance refresh failed".to_string(),
    })?;
    debug!(
        status = ?result.loop_result.final_status,
        steps = result.steps_executed,
        "Maintenance refresh step completed"
    );
    Ok(result.steps_executed)
}

/// Consume a finished maintenance target: the deployment is settled on its
/// installed release again, so passive health reporting resumes and the next
/// tick does not refresh again. A refresh that answers a claim syncs right
/// away, so the claim completes with this refresh's result. A manager that
/// issues no claims replays the stored target on every empty sync, so an
/// immediate sync would start the next refresh at once; its refreshes follow
/// the sync interval instead. A state the sync loop changed meanwhile (for
/// example a newer target) is left, with its config, for the next tick.
async fn finish_maintenance_refresh(
    state: &OperatorState,
    refreshed: &DeploymentState,
    answers_claim: bool,
) -> Result<()> {
    let mut settled = refreshed.clone();
    settled.target_release = None;
    if state
        .db
        .finish_target_if_unchanged(refreshed, &settled)
        .await?
        && answers_claim
    {
        state.db.request_sync();
    }
    Ok(())
}

/// Enrich a deployment config with operator-specific settings.
///
/// Applies public_endpoints and stack_settings from operator config,
/// and injects command delivery env vars for K8s/Local platforms.
/// External bindings also populate DeploymentConfig for executor preflights.
pub(super) async fn enrich_config(
    mut config: DeploymentConfig,
    operator_config: &OperatorConfig,
    platform: Platform,
    db: &OperatorDb,
    stack: Option<&alien_core::Stack>,
) -> Result<DeploymentConfig> {
    // The previous Operator pod accepted this config before it enrolled, so it
    // still carries the deployment token and no manager audience, and the
    // manager does not send it again. Treat it the way a synced target is
    // treated. The deployment token goes to no workload: the manager revokes
    // it after the first rollout, and a command receiver built before
    // capability files gets the commands key the manager supplies instead.
    if config.runtime_identity_audience.is_none() {
        if let Some(identity) = db.read_enrolled_daily_identity().await? {
            super::sync::sanitize_identity_config(
                &mut config,
                &identity.manager_url,
                &identity.audience,
            )?;
        }
    }

    // Pass through public endpoints from operator config.
    if operator_config.public_endpoints.is_some() {
        config.public_endpoints = operator_config.public_endpoints.clone();
    }

    config.image_registry = operator_config.image_registry.clone();
    config.image_pull_secrets = operator_config.image_pull_secrets.clone();

    // Chart-created databases are defaults; explicit bindings always win.
    if let Some(defaults) = &operator_config.chart_default_bindings {
        for (id, binding) in &defaults.0 {
            if !config.external_bindings.has(id) {
                config.external_bindings.insert(id.clone(), binding.clone());
            }
        }
    }
    // Installation-provided bindings take precedence over manager-provided ones.
    if let Some(ref stack_settings) = operator_config.stack_settings {
        if let Some(ref bindings) = stack_settings.external_bindings {
            config.external_bindings.0.extend(bindings.0.clone());
        }
        config.stack_settings = stack_settings.clone();
    }
    if operator_config.stack_settings.is_some() || operator_config.chart_default_bindings.is_some()
    {
        config.stack_settings.external_bindings = Some(config.external_bindings.clone());
    }
    if config.base_platform.is_none() {
        config.base_platform = operator_config.base_platform;
    }
    if config.label_domain.is_none() {
        config.label_domain = operator_config.label_domain.clone();
    }
    if config.deployment_name.is_none() {
        config.deployment_name = operator_config.agent_name.clone();
    }
    if config.observe_label_selector.is_none() {
        config.observe_label_selector = operator_config.label_selector.clone();
    }
    config.observe_all_namespaces = operator_config.observe_all_namespaces;

    // Local/Kubernetes Workers expose an authenticated runtime-owned push
    // endpoint. Cloud-native Worker transports authenticate through their
    // platform instead.
    let needs_http_push_auth = matches!(platform, Platform::Kubernetes | Platform::Local);

    if operator_config.airgap_target_secret.is_some() {
        if let Some(stack) = stack {
            config.environment_variables.variables.extend(
                stack
                    .command_targets()
                    .into_iter()
                    .filter(|target| {
                        target.resource_type
                            != alien_core::commands_types::CommandTargetType::Worker
                    })
                    .map(|target| EnvironmentVariable {
                        name: "ALIEN_COMMANDS_DISABLED".to_string(),
                        value: "true".to_string(),
                        var_type: EnvironmentVariableType::Plain,
                        target_resources: Some(vec![target.resource_id]),
                    }),
            );
        }
    }

    if needs_http_push_auth {
        if let Some(ref sync_config) = operator_config.sync {
            let commands_url = match db.get_commands_url().await {
                Ok(Some(url)) => url,
                _ => format!("{}/v1", sync_config.url.as_str().trim_end_matches('/')),
            };

            let mut vars = config.environment_variables.variables.clone();

            if let (Some(stack), Some(audience)) =
                (stack, config.runtime_identity_audience.as_deref())
            {
                if platform == Platform::Local
                    && stack.resources().any(|(_, resource)| {
                        resource
                            .config
                            .downcast_ref::<alien_core::Container>()
                            .is_some()
                    })
                {
                    return Err(AlienError::new(ErrorData::ConfigurationError {
                        message: "Local container credential file access is not configured".into(),
                    }));
                }
                // A Container or Daemon whose receiver the build did not record as
                // reading the capability file is an older receiver. It cannot
                // start without `ALIEN_COMMANDS_TOKEN`, so that token stays for
                // it and for no one else.
                let unmarked: std::collections::BTreeSet<String> = stack
                    .command_receivers_without_capability_file()
                    .into_iter()
                    .collect();
                vars.retain(|variable| {
                    variable.name != "ALIEN_COMMANDS_TOKEN"
                        || variable.target_resources.as_ref().is_some_and(|targets| {
                            !targets.is_empty()
                                && targets.iter().all(|target| unmarked.contains(target))
                        })
                });
                // Worker runtimes built before capability files accept a push
                // only with `ALIEN_COMMANDS_TOKEN`. Give them the Operator's
                // own push secret, which grants nothing at the manager. Newer
                // runtimes use the capability file and ignore it.
                let targets = stack.command_targets();
                let push_secret = if targets
                    .iter()
                    .any(|target| target.resource_type == CommandTargetType::Worker)
                {
                    Some(db.worker_push_secret().await?)
                } else {
                    None
                };
                for target in targets {
                    if unmarked.contains(&target.resource_id) {
                        // The manager supplied this receiver's commands key
                        // with its other settings. Without it the receiver
                        // cannot start, so fail here where the cause can be
                        // named. A config saved before enrollment has none;
                        // the sync loop asks the manager for a fresh one.
                        let has_token = vars.iter().any(|variable| {
                            variable.name == "ALIEN_COMMANDS_TOKEN"
                                && variable
                                    .target_resources
                                    .as_ref()
                                    .is_some_and(|targets| targets.contains(&target.resource_id))
                        });
                        if !has_token {
                            return Err(AlienError::new(ErrorData::ConfigurationError {
                                message: format!(
                                    "Command receiver '{}' was built before capability files and needs ALIEN_COMMANDS_TOKEN, which the manager did not supply",
                                    target.resource_id
                                ),
                            }));
                        }
                        let values = [
                            ("ALIEN_COMMANDS_URL", commands_url.clone()),
                            (
                                "ALIEN_COMMANDS_TARGET_RESOURCE_ID",
                                target.resource_id.clone(),
                            ),
                            (
                                "ALIEN_COMMANDS_TARGET_RESOURCE_TYPE",
                                target.resource_type.as_str().to_owned(),
                            ),
                        ];
                        vars.extend(values.into_iter().map(|(name, value)| EnvironmentVariable {
                            name: name.into(),
                            value,
                            var_type: EnvironmentVariableType::Plain,
                            target_resources: Some(vec![target.resource_id.clone()]),
                        }));
                        continue;
                    }
                    if let (CommandTargetType::Worker, Some(secret)) =
                        (target.resource_type, push_secret.as_ref())
                    {
                        vars.push(EnvironmentVariable {
                            name: alien_core::ENV_ALIEN_COMMANDS_TOKEN.into(),
                            value: secret.clone(),
                            var_type: EnvironmentVariableType::Secret,
                            target_resources: Some(vec![target.resource_id.clone()]),
                        });
                    }
                    let values = [
                        ("ALIEN_IDENTITY_PROVIDER", "capabilityFile".to_owned()),
                        ("ALIEN_MANAGER_URL", sync_config.url.to_string()),
                        ("ALIEN_MANAGER_AUDIENCE", audience.to_owned()),
                        (
                            "ALIEN_COMMANDS_CAPABILITY_FILE",
                            "/var/run/alien-runtime/commands.json".to_owned(),
                        ),
                        ("ALIEN_COMMANDS_URL", commands_url.clone()),
                        (
                            "ALIEN_COMMANDS_TARGET_RESOURCE_ID",
                            target.resource_id.clone(),
                        ),
                        (
                            "ALIEN_COMMANDS_TARGET_RESOURCE_TYPE",
                            target.resource_type.as_str().to_owned(),
                        ),
                    ];
                    vars.extend(values.into_iter().map(|(name, value)| EnvironmentVariable {
                        name: name.into(),
                        value,
                        var_type: EnvironmentVariableType::Plain,
                        target_resources: Some(vec![target.resource_id.clone()]),
                    }));
                }
            } else {
                // The Manager authenticates command delivery with the deployment's
                // sync token. The Worker runtime keeps it out of the application
                // child environment; Container/Daemon receivers necessarily read it.
                if let Some(stack) = stack {
                    vars.extend(
                        stack
                            .worker_command_push_env_vars(Some(&sync_config.token))
                            .context(ErrorData::ConfigurationError {
                                message: "Failed to build Worker command push auth env".to_string(),
                            })?,
                    );
                    // Container/Daemon command receiver env (ALIEN_COMMANDS_URL/TOKEN/
                    // TARGET_RESOURCE_ID/TARGET_RESOURCE_TYPE), scoped per resource.
                    // Container/Daemon always deliver via Pull; the operator
                    // only ever manages K8s/Local, so this block already covers every
                    // platform it serves. It uses the same deployment-token auth
                    // contract as Worker push handling above.
                    vars.extend(
                        stack
                            .receiver_command_env_vars(&commands_url, |_| {
                                ReceiverCommandCredential::Token(Some(sync_config.token.clone()))
                            })
                            .context(ErrorData::ConfigurationError {
                                message: "Failed to build command receiver env".to_string(),
                            })?,
                    );
                }
            }

            // Ensure ALIEN_DEPLOYMENT_ID is present (should come from manager config,
            // but add defensively in case it's missing)
            if !vars.iter().any(|v| v.name == ENV_ALIEN_DEPLOYMENT_ID) {
                if let Ok(Some(dep_id)) = db.get_deployment_id().await {
                    vars.push(EnvironmentVariable {
                        name: ENV_ALIEN_DEPLOYMENT_ID.to_string(),
                        value: dep_id,
                        var_type: EnvironmentVariableType::Plain,
                        target_resources: None,
                    });
                }
            }

            if !vars.iter().any(|v| v.name == ENV_ALIEN_DEPLOYMENT_NAME) {
                if let Some(name) = config.deployment_name.as_ref() {
                    vars.push(EnvironmentVariable {
                        name: ENV_ALIEN_DEPLOYMENT_NAME.to_string(),
                        value: name.clone(),
                        var_type: EnvironmentVariableType::Plain,
                        target_resources: None,
                    });
                }
            }

            config.environment_variables.variables = vars;

            info!("Injected command delivery configuration for K8s/Local deployment");
        }
    }

    // Image pull credentials are no longer needed here — pull-model operators
    // pull images through the manager's /v2/ registry proxy, which handles
    // upstream authentication using the manager's own credentials.

    Ok(config)
}

/// Resolve client config based on platform
pub(super) async fn resolve_client_config(
    platform: Platform,
    base_platform: Option<Platform>,
    data_dir: &str,
    namespace: Option<String>,
    _sync_config: Option<&crate::config::SyncConfig>,
) -> Result<ClientConfig> {
    match platform {
        Platform::Kubernetes => {
            let kubernetes = KubernetesClientConfig::InCluster {
                    namespace,
                    additional_headers: None,
            };
            if let Some(base_platform) = base_platform {
                let cloud = ClientConfig::from_std_env(base_platform)
                    .await
                    .context(ErrorData::ConfigurationError {
                        message: format!(
                            "Failed to create {} base client config for Kubernetes deployment",
                            base_platform
                        ),
                    })?;
                Ok(ClientConfig::KubernetesCloud {
                    kubernetes: Box::new(kubernetes),
                    cloud: Box::new(cloud),
                })
            } else {
                Ok(ClientConfig::Kubernetes(Box::new(kubernetes)))
            }
        }
        Platform::Local => {
            // No artifact_registry_config needed — the deployment token for proxy
            // pull auth flows through DeploymentConfig.deployment_token (set by
            // the sync handler from the operator's Bearer token).
            Ok(ClientConfig::Local {
                state_directory: data_dir.to_string(),
            })
        }
        Platform::Machines => Err(AlienError::new(ErrorData::ConfigurationError {
            message: "Machines deployments are reconciled by the manager, not by a local operator"
                .to_string(),
        })),
        Platform::Test => Ok(ClientConfig::Test),
        Platform::Aws | Platform::Gcp | Platform::Azure => {
            ClientConfig::from_std_env(platform)
                .await
                .context(ErrorData::ConfigurationError {
                    message: format!(
                        "Failed to create {} client config from environment. Ensure the required environment variables are set.",
                        platform
                    ),
                })
        }
    }
}

#[cfg(test)]
mod outcome_log_tests {
    use super::outcome_changed;

    #[test]
    fn a_repeated_outcome_is_reported_once() {
        assert!(outcome_changed("Running/Synced/Success"));
        assert!(!outcome_changed("Running/Synced/Success"));
        assert!(!outcome_changed("Running/Synced/Success"));
        assert!(outcome_changed("Failed/Synced/Failure"));
        assert!(outcome_changed("Running/Synced/Success"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{OperatorConfig, SyncConfig};
    use crate::db::OperatorDb;
    use alien_core::{
        DeploymentConfig, EnvironmentVariablesSnapshot, ExternalBinding, ExternalBindings,
        StackSettings, StorageBinding,
    };
    use std::collections::HashMap;

    fn held_state(status: DeploymentStatus) -> DeploymentState {
        let release = alien_core::ReleaseInfo {
            release_id: Some("rel_held".into()),
            version: None,
            description: None,
            stack: alien_core::Stack::new("held".into()).build(),
            signed: None,
            signer_lists: Vec::new(),
        };
        DeploymentState {
            platform: Platform::Local,
            status,
            current_release: Some(release.clone()),
            target_release: Some(release),
            stack_state: None,
            runtime_metadata: None,
            error: None,
            environment_info: None,
            retry_requested: false,
            protocol_version: alien_core::DEPLOYMENT_PROTOCOL_VERSION,
        }
    }

    async fn held_operator(status: DeploymentStatus) -> (tempfile::TempDir, Arc<OperatorState>) {
        let directory = tempfile::tempdir().unwrap();
        let db = Arc::new(
            OperatorDb::new(
                directory.path().to_str().unwrap(),
                "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
            )
            .await
            .unwrap(),
        );
        db.set_deployment_id("dep_held").await.unwrap();
        db.set_deployment_state(&held_state(status)).await.unwrap();
        db.set_deployment_config(&test_deployment_config())
            .await
            .unwrap();
        let state = Arc::new(OperatorState {
            config: OperatorConfig::builder()
                .platform(Platform::Local)
                .data_dir(directory.path().to_str().unwrap())
                .encryption_key("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef")
                .build(),
            db,
            service_provider: None,
            operations_sync_handler: None,
            pod_log_collection: false,
            cancel: tokio_util::sync::CancellationToken::new(),
        });
        (directory, state)
    }

    const HOLD_STATUSES: [DeploymentStatus; 5] = [
        DeploymentStatus::Deleted,
        DeploymentStatus::TeardownRequired,
        DeploymentStatus::RetentionRequired,
        DeploymentStatus::Retained,
        DeploymentStatus::TeardownFailed,
    ];

    #[tokio::test]
    async fn a_hold_status_pass_runs_no_step_and_changes_nothing() {
        for status in HOLD_STATUSES {
            let (_directory, state) = held_operator(status).await;
            let before = state.db.get_deployment_state().await.unwrap().unwrap();
            let config_before = state.db.get_deployment_config().await.unwrap();
            let (steps, _) = run_deployment_continuously(&state).await.unwrap();
            assert_eq!(steps, 0, "{status:?} must not step");
            let after = state.db.get_deployment_state().await.unwrap().unwrap();
            assert_eq!(
                serde_json::to_value(&before).unwrap(),
                serde_json::to_value(&after).unwrap(),
                "{status:?} must not change"
            );
            // The sync loop keeps sending the target, so a hold pass must not clear its config.
            assert_eq!(
                serde_json::to_value(state.db.get_deployment_config().await.unwrap()).unwrap(),
                serde_json::to_value(config_before).unwrap(),
                "{status:?} must keep the stored config"
            );
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_held_deployment_backs_off_for_ten_minutes_and_a_wake_ends_the_wait() {
        for status in HOLD_STATUSES {
            let (_directory, state) = held_operator(status).await;
            let waits = Arc::new(std::sync::Mutex::new(Vec::<Duration>::new()));
            let observed = Arc::clone(&waits);
            let task = tokio::spawn(run_deployment_loop_observed(
                Arc::clone(&state),
                move |delay| observed.lock().unwrap().push(delay),
            ));

            tokio::time::sleep(Duration::from_secs(600)).await;
            let passes = waits.lock().unwrap().len();
            // 5, 10, 20, 40 s then 60 s: about 13 passes in ten minutes (a 1 s
            // interval ran 600, each with up to 100 steps).
            assert!(
                (10..=15).contains(&passes),
                "{status:?}: {passes} passes in ten simulated minutes"
            );
            assert_eq!(
                waits.lock().unwrap()[..5],
                [5, 10, 20, 40, 60].map(Duration::from_secs),
                "{status:?}"
            );

            // A wake is handled at once, and the wait starts again at 5 s.
            state.db.request_deployment();
            tokio::time::sleep(Duration::from_millis(1)).await;
            let waits = waits.lock().unwrap();
            assert_eq!(waits.len(), passes + 1, "{status:?}: wake runs a pass");
            assert_eq!(*waits.last().unwrap(), Duration::from_secs(5), "{status:?}");
            state.cancel.cancel();
            task.await.unwrap();
        }
    }

    fn test_deployment_config() -> DeploymentConfig {
        DeploymentConfig {
            management_external_id: None,
            image_registry: None,
            image_pull_secrets: Vec::new(),
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
            deployment_id: None,
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
            runtime_identity_audience: None,
            deployment_token: None,
            native_image_host: None,
            volume_restores: Vec::new(),
            value_slots: Vec::new(),
            deletion_policy: Default::default(),
            release_statements: None,
            signed_config: None,
        }
    }

    fn state_with_authorization(fingerprint: &str) -> DeploymentState {
        DeploymentState {
            platform: Platform::Test,
            status: DeploymentStatus::UpdatePending,
            current_release: None,
            target_release: None,
            stack_state: None,
            error: None,
            environment_info: None,
            runtime_metadata: Some(alien_core::RuntimeMetadata {
                setup_update_authorization: Some(alien_core::SetupUpdateAuthorization {
                    nonce: "n".into(),
                    baseline_frozen_digest: "a".into(),
                    target_frozen_digest: "b".into(),
                    release_id: "rel_2".into(),
                    setup_target: "helm".into(),
                    setup_fingerprint: fingerprint.into(),
                    setup_fingerprint_version: 1,
                }),
                ..alien_core::RuntimeMetadata::default()
            }),
            retry_requested: false,
            protocol_version: alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
        }
    }

    fn installed(fingerprint: &str) -> alien_core::sync::InstalledSetup {
        alien_core::sync::InstalledSetup {
            release_id: "rel_1".into(),
            setup_target: "helm".into(),
            setup_fingerprint: fingerprint.into(),
            setup_fingerprint_version: 1,
            release_name: None,
            namespace: None,
        }
    }

    /// After a rollback the Operator restarts with the older fingerprint, so an
    /// authorization minted for the newer chart must go unused.
    #[test]
    fn an_authorization_for_another_installed_setup_goes_unused() {
        let has_authorization = |state: &DeploymentState| {
            state
                .runtime_metadata
                .as_ref()
                .unwrap()
                .setup_update_authorization
                .is_some()
        };

        let mut matching = state_with_authorization("fp_new");
        bind_setup_authorization(&mut matching, Some(&installed("fp_new")));
        assert!(has_authorization(&matching));

        let mut rolled_back = state_with_authorization("fp_new");
        bind_setup_authorization(&mut rolled_back, Some(&installed("fp_old")));
        assert!(!has_authorization(&rolled_back));

        let mut no_record = state_with_authorization("fp_new");
        bind_setup_authorization(&mut no_record, None);
        assert!(has_authorization(&no_record));
    }

    /// The sync loop accepts release B while the loop is still applying release A. The
    /// loop's own state holds A and must not write it back over B: the first failure of
    /// A would then be retried automatically instead of B being prepared.
    #[tokio::test]
    async fn an_apply_checkpoint_never_overwrites_a_target_accepted_meanwhile() {
        let data_dir = tempfile::tempdir().unwrap();
        let encryption_key = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let db = Arc::new(
            OperatorDb::new(data_dir.path().to_str().unwrap(), encryption_key)
                .await
                .unwrap(),
        );
        let release = |id: &str| alien_core::ReleaseInfo {
            release_id: Some(id.to_string()),
            version: None,
            description: None,
            stack: alien_core::Stack::new("apply".to_string()).build(),
            signed: None,
            signer_lists: Vec::new(),
        };
        let state = |status, target: &str| DeploymentState {
            platform: Platform::Test,
            status,
            current_release: Some(release("rel_installed")),
            target_release: Some(release(target)),
            stack_state: None,
            error: None,
            environment_info: None,
            runtime_metadata: Some(alien_core::RuntimeMetadata {
                pending_prepared_release_id: Some("rel_a".to_string()),
                ..Default::default()
            }),
            retry_requested: false,
            protocol_version: alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
        };
        let updating = state(DeploymentStatus::Updating, "rel_a");
        db.set_deployment_state(&updating).await.unwrap();
        let transport = OperatorTransport {
            db: Arc::clone(&db),
            operator_config: OperatorConfig::builder()
                .platform(Platform::Test)
                .data_dir(data_dir.path().to_str().unwrap())
                .encryption_key(encryption_key)
                .build(),
            platform: Platform::Test,
            suggested_delay_ms: AtomicU64::new(0),
            observed_from: None,
            superseded: AtomicBool::new(false),
            applying_from: Some(tokio::sync::Mutex::new(updating.clone())),
        };
        let config = test_deployment_config();

        // Nothing newer: the checkpoint is written as is and the pass goes on.
        let step = transport
            .reconcile_step("dep", &updating, &config, false, None, vec![], vec![])
            .await
            .unwrap();
        assert!(step.state.is_none());
        assert!(!transport.superseded.load(Ordering::Relaxed));

        // The sync loop accepts B while the update is Updating (status is kept).
        let mut accepted = updating.clone();
        accepted.target_release = Some(release("rel_b"));
        db.set_deployment_state(&accepted).await.unwrap();

        // A step of A ends in failure and checkpoints with A as its target. B is
        // stored as the target and the pass ends, so the next pass runs every gate
        // for B (approval, signature) before anything of B is applied.
        let failed = state(DeploymentStatus::UpdateFailed, "rel_a");
        let Err(error) = transport
            .reconcile_step("dep", &failed, &config, false, None, vec![], vec![])
            .await
        else {
            panic!("the pass must end when a newer target is stored");
        };
        assert_eq!(error.code, "MAINTENANCE_SUPERSEDED");
        assert!(transport.superseded.load(Ordering::Relaxed));
        let stored = db.get_deployment_state().await.unwrap().unwrap();
        assert_eq!(
            stored.target_release.unwrap().release_id.as_deref(),
            Some("rel_b")
        );
        // B is prepared from the start, not A retried, and nothing of it ran.
        assert_eq!(stored.status, DeploymentStatus::UpdatePending);
        assert!(!stored.retry_requested);
        assert!(stored
            .runtime_metadata
            .unwrap()
            .pending_prepared_release_id
            .is_none());
    }

    /// With updates set to approval-required, a release adopted mid-apply is not
    /// applied until it is approved: the adopting checkpoint ends the pass, and the
    /// next pass holds at its approval gate.
    #[tokio::test]
    async fn a_release_adopted_mid_apply_waits_for_approval() {
        let (directory, mut state) = held_operator(DeploymentStatus::Updating).await;
        let data_dir = directory.path().to_str().unwrap().to_string();
        Arc::get_mut(&mut state).unwrap().config = OperatorConfig::builder()
            .platform(Platform::Local)
            .data_dir(&data_dir)
            .encryption_key("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef")
            .maybe_stack_settings(Some(alien_core::StackSettings {
                updates: alien_core::UpdatesMode::ApprovalRequired,
                ..Default::default()
            }))
            .build();
        let release = |id: &str| alien_core::ReleaseInfo {
            release_id: Some(id.to_string()),
            ..held_state(DeploymentStatus::Updating)
                .target_release
                .unwrap()
        };
        let mut updating = held_state(DeploymentStatus::Updating);
        updating.target_release = Some(release("rel_b"));
        state.db.set_deployment_state(&updating).await.unwrap();
        let approval = |id: &str, release_id: &str, status| crate::db::Approval {
            id: id.into(),
            release_info: Some(release(release_id)),
            deployment_config: test_deployment_config(),
            status,
            reason: None,
            created_at: chrono::Utc::now().to_rfc3339(),
            decided_at: None,
            decided_by: None,
        };
        state
            .db
            .create_approval(&approval(
                "apr_b",
                "rel_b",
                crate::db::ApprovalStatus::Approved,
            ))
            .await
            .unwrap();
        state
            .db
            .create_approval(&approval(
                "apr_c",
                "rel_c",
                crate::db::ApprovalStatus::Pending,
            ))
            .await
            .unwrap();
        let transport = OperatorTransport {
            db: Arc::clone(&state.db),
            operator_config: state.config.clone(),
            platform: Platform::Local,
            suggested_delay_ms: AtomicU64::new(0),
            observed_from: None,
            superseded: AtomicBool::new(false),
            applying_from: Some(tokio::sync::Mutex::new(updating.clone())),
        };

        // C is accepted while B applies; B then fails.
        let mut accepted = updating.clone();
        accepted.target_release = Some(release("rel_c"));
        state.db.set_deployment_state(&accepted).await.unwrap();
        let mut failed = updating.clone();
        failed.status = DeploymentStatus::UpdateFailed;
        transport
            .reconcile_step(
                "dep",
                &failed,
                &test_deployment_config(),
                false,
                None,
                vec![],
                vec![],
            )
            .await
            .err()
            .expect("the pass ends");
        let held = state.db.get_deployment_state().await.unwrap().unwrap();
        assert_eq!(held.status, DeploymentStatus::UpdatePending);

        // The next pass stops at the approval gate and changes nothing.
        let (steps, _) = run_deployment_continuously(&state).await.unwrap();
        assert_eq!(steps, 0);
        let still_held = state.db.get_deployment_state().await.unwrap().unwrap();
        assert_eq!(
            serde_json::to_value(&held).unwrap(),
            serde_json::to_value(&still_held).unwrap()
        );

        // Once approved, the pass applies it.
        state
            .db
            .decide_approval("apr_c", crate::db::ApprovalStatus::Approved, None, None)
            .await
            .unwrap();
        let _ = run_deployment_continuously(&state).await;
        let moved = state.db.get_deployment_state().await.unwrap().unwrap();
        assert_ne!(moved.status, DeploymentStatus::UpdatePending);
    }

    /// The sync loop can store a newer target between the checkpoint's read of the
    /// stored state and its write. The write compares against that read, so the
    /// change is seen on the next try, never overwritten.
    #[tokio::test]
    async fn an_apply_checkpoint_never_overwrites_a_change_landing_after_its_read() {
        let data_dir = tempfile::tempdir().unwrap();
        let db = OperatorDb::new(
            data_dir.path().to_str().unwrap(),
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        )
        .await
        .unwrap();
        let release = |id: &str| alien_core::ReleaseInfo {
            release_id: Some(id.to_string()),
            version: None,
            description: None,
            stack: alien_core::Stack::new("apply".to_string()).build(),
            signed: None,
            signer_lists: Vec::new(),
        };
        let state = |status, target: &str| DeploymentState {
            platform: Platform::Test,
            status,
            current_release: Some(release("rel_installed")),
            target_release: Some(release(target)),
            stack_state: None,
            error: None,
            environment_info: None,
            runtime_metadata: None,
            retry_requested: false,
            protocol_version: alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
        };
        let written = state(DeploymentStatus::Updating, "rel_a");
        db.set_deployment_state(&written).await.unwrap();
        // Sync accepts B; the checkpoint's compare fails and it reads B.
        db.set_deployment_state(&state(DeploymentStatus::Updating, "rel_b"))
            .await
            .unwrap();
        // Right after that read, sync accepts C.
        let mut landed = false;
        let outcome = checkpoint_apply(
            &db,
            &written,
            &state(DeploymentStatus::UpdateFailed, "rel_a"),
            async |_| {
                if !std::mem::replace(&mut landed, true) {
                    db.set_deployment_state(&state(DeploymentStatus::Updating, "rel_c"))
                        .await
                        .unwrap();
                }
            },
        )
        .await
        .unwrap();
        assert_eq!(outcome, ApplyCheckpoint::Adopted);
        let stored = db.get_deployment_state().await.unwrap().unwrap();
        assert_eq!(
            stored.target_release.unwrap().release_id.as_deref(),
            Some("rel_c")
        );
        assert_eq!(stored.status, DeploymentStatus::UpdatePending);
    }

    /// The last step of an apply clears the target. A target the sync loop accepted
    /// meanwhile must survive that checkpoint too.
    #[tokio::test]
    async fn the_last_apply_checkpoint_keeps_a_target_accepted_meanwhile() {
        let data_dir = tempfile::tempdir().unwrap();
        let encryption_key = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let db = Arc::new(
            OperatorDb::new(data_dir.path().to_str().unwrap(), encryption_key)
                .await
                .unwrap(),
        );
        let release = |id: &str| alien_core::ReleaseInfo {
            release_id: Some(id.to_string()),
            version: None,
            description: None,
            stack: alien_core::Stack::new("apply".to_string()).build(),
            signed: None,
            signer_lists: Vec::new(),
        };
        let state = |status, current: &str, target: Option<&str>| DeploymentState {
            platform: Platform::Test,
            status,
            current_release: Some(release(current)),
            target_release: target.map(release),
            stack_state: None,
            error: None,
            environment_info: None,
            runtime_metadata: None,
            retry_requested: false,
            protocol_version: alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
        };
        let updating = state(DeploymentStatus::Updating, "rel_installed", Some("rel_a"));
        db.set_deployment_state(&updating).await.unwrap();
        let transport = OperatorTransport {
            db: Arc::clone(&db),
            operator_config: OperatorConfig::builder()
                .platform(Platform::Test)
                .data_dir(data_dir.path().to_str().unwrap())
                .encryption_key(encryption_key)
                .build(),
            platform: Platform::Test,
            suggested_delay_ms: AtomicU64::new(0),
            observed_from: None,
            superseded: AtomicBool::new(false),
            applying_from: Some(tokio::sync::Mutex::new(updating.clone())),
        };
        let config = test_deployment_config();

        let mut accepted = updating.clone();
        accepted.target_release = Some(release("rel_c"));
        db.set_deployment_state(&accepted).await.unwrap();

        // A finishes: Running on A, target cleared.
        let done = state(DeploymentStatus::Running, "rel_a", None);
        assert!(transport
            .reconcile_step("dep", &done, &config, false, None, vec![], vec![])
            .await
            .is_err());
        let stored = db.get_deployment_state().await.unwrap().unwrap();
        assert_eq!(
            stored.target_release.unwrap().release_id.as_deref(),
            Some("rel_c")
        );
        assert_eq!(stored.status, DeploymentStatus::UpdatePending);
        assert_eq!(
            stored.current_release.unwrap().release_id.as_deref(),
            Some("rel_a")
        );
    }

    /// A maintenance refresh is an observation. If the sync loop accepted newer
    /// work while it ran (the claim may already be complete), its checkpoint must
    /// not overwrite that target.
    #[tokio::test]
    async fn a_maintenance_checkpoint_never_overwrites_a_newer_target() {
        let data_dir = tempfile::tempdir().unwrap();
        let encryption_key = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let db = Arc::new(
            OperatorDb::new(data_dir.path().to_str().unwrap(), encryption_key)
                .await
                .unwrap(),
        );
        let release = |id: &str| alien_core::ReleaseInfo {
            release_id: Some(id.to_string()),
            version: None,
            description: None,
            stack: alien_core::Stack::new("observe".to_string()).build(),
            signed: None,
            signer_lists: Vec::new(),
        };
        let state = |status, target: &str| DeploymentState {
            platform: Platform::Test,
            status,
            current_release: Some(release("rel_installed")),
            target_release: Some(release(target)),
            stack_state: None,
            error: None,
            environment_info: None,
            runtime_metadata: None,
            retry_requested: false,
            protocol_version: alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
        };
        let loaded = state(DeploymentStatus::Running, "rel_installed");
        db.set_deployment_state(&loaded).await.unwrap();
        let transport = OperatorTransport {
            db: Arc::clone(&db),
            operator_config: OperatorConfig::builder()
                .platform(Platform::Test)
                .data_dir(data_dir.path().to_str().unwrap())
                .encryption_key(encryption_key)
                .build(),
            platform: Platform::Test,
            suggested_delay_ms: AtomicU64::new(0),
            observed_from: Some(tokio::sync::Mutex::new(loaded.clone())),
            superseded: AtomicBool::new(false),
            applying_from: None,
        };
        let config = test_deployment_config();

        // Unchanged: the refresh checkpoints normally, twice.
        let mut observed = loaded.clone();
        observed.retry_requested = true;
        for _ in 0..2 {
            transport
                .reconcile_step("dep", &observed, &config, true, None, vec![], vec![])
                .await
                .expect("checkpoint over the loaded state");
            assert_eq!(
                serde_json::to_value(db.get_deployment_state().await.unwrap()).unwrap(),
                serde_json::to_value(Some(&observed)).unwrap()
            );
        }

        // The sync loop accepts new work for the same release.
        let newer = state(DeploymentStatus::UpdatePending, "rel_installed");
        db.set_deployment_state(&newer).await.unwrap();
        let Err(error) = transport
            .reconcile_step("dep", &loaded, &config, true, None, vec![], vec![])
            .await
        else {
            panic!("a stale observation must not checkpoint");
        };
        assert_eq!(error.code, "MAINTENANCE_SUPERSEDED");
        assert!(!error.retryable);
        assert!(transport.superseded.load(Ordering::Relaxed));
        assert_eq!(
            serde_json::to_value(db.get_deployment_state().await.unwrap()).unwrap(),
            serde_json::to_value(Some(&newer)).unwrap()
        );
    }

    #[tokio::test]
    async fn secret_wait_yields_for_the_requested_delay_and_wakes_sync_only_on_change() {
        alien_deployment::set_ephemeral_secrets_digest_key();
        use alien_bindings::{BindingsProvider, BindingsProviderApi};
        use alien_core::{
            DeploymentStatus, Resource, ResourceLifecycle, ResourceStatus, RuntimeMetadata, Stack,
            StackResourceState, StackState, Vault, VaultBinding,
        };
        let directory = tempfile::tempdir().unwrap();
        let data_dir = directory.path().to_str().unwrap();
        let db = Arc::new(
            OperatorDb::new(
                data_dir,
                "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
            )
            .await
            .unwrap(),
        );
        // One Worker reads the secret; only it is held while the key is missing.
        let mut stack = Stack::new("secret-test".into())
            .add(
                alien_core::Worker::new("reader".into())
                    .code(alien_core::WorkerCode::Image {
                        image: "test:latest".into(),
                    })
                    .permissions("default".into())
                    .build(),
                ResourceLifecycle::Live,
            )
            .build();
        stack.inputs = vec![serde_json::from_value(serde_json::json!({
            "id": "key", "kind": "secret", "providedBy": ["deployer"],
            "required": true, "label": "Test key", "description": "",
            "env": [{"name": "KEY", "targetResources": ["reader"]}]
        }))
        .unwrap()];
        let mut vault = StackResourceState::new_pending(
            "vault".into(),
            Resource::new(Vault::new("deployer-secrets".into()).build()),
            Some(ResourceLifecycle::Frozen),
            Vec::new(),
        );
        vault.status = ResourceStatus::Running;
        vault.remote_binding_params =
            Some(serde_json::to_value(VaultBinding::local("deployer-secrets", data_dir)).unwrap());
        let mut stack_state = StackState::new(Platform::Local);
        stack_state
            .resources
            .insert("deployer-secrets".into(), vault);
        let provider = BindingsProvider::from_stack_state(
            &stack_state,
            ClientConfig::Local {
                state_directory: data_dir.into(),
            },
        )
        .unwrap();
        let vault = provider.load_vault("deployer-secrets").await.unwrap();
        let release = alien_core::ReleaseInfo {
            release_id: Some("rel_test".into()),
            version: None,
            description: None,
            stack: stack.clone(),
            signed: None,
            signer_lists: Vec::new(),
        };
        let current = DeploymentState {
            platform: Platform::Local,
            status: DeploymentStatus::WaitingForSecrets,
            current_release: None,
            target_release: Some(release.clone()),
            stack_state: Some(stack_state),
            runtime_metadata: Some(alien_core::RuntimeMetadata {
                prepared_stack: Some(stack),
                ..Default::default()
            }),
            error: None,
            environment_info: None,
            retry_requested: false,
            protocol_version: alien_core::DEPLOYMENT_PROTOCOL_VERSION,
        };
        db.set_deployment_state(&current).await.unwrap();
        db.set_deployment_config(&test_deployment_config())
            .await
            .unwrap();
        let state = OperatorState {
            config: OperatorConfig::builder()
                .platform(Platform::Local)
                .data_dir(data_dir)
                .encryption_key("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef")
                .build(),
            db,
            service_provider: None,
            operations_sync_handler: None,
            pod_log_collection: false,
            cancel: tokio_util::sync::CancellationToken::new(),
        };
        let (steps, delay) = run_deployment_continuously(&state).await.unwrap();
        assert_eq!(steps, 1);
        assert_eq!(delay, Duration::from_secs(30));
        tokio::time::timeout(Duration::from_secs(1), state.db.wait_for_sync())
            .await
            .unwrap();
        let (_, delay) = run_deployment_continuously(&state).await.unwrap();
        assert_eq!(delay, Duration::from_secs(30));
        assert!(
            tokio::time::timeout(Duration::from_millis(10), state.db.wait_for_sync())
                .await
                .is_err()
        );

        // A completed deployment drops its installation config. Refresh still
        // observes a changed slot without starting any resource controllers.
        let mut running = state.db.get_deployment_state().await.unwrap().unwrap();
        running.status = DeploymentStatus::Running;
        running.error = None;
        running.target_release = None;
        running.current_release = Some(release);
        // An input supplied at install time must not become a new deployer slot.
        let metadata = running.runtime_metadata.as_mut().unwrap();
        let mut supplied = metadata.prepared_stack.as_ref().unwrap().inputs[0].clone();
        supplied.id = "supplied".into();
        supplied
            .provided_by
            .push(alien_core::StackInputProvider::Developer);
        metadata
            .prepared_stack
            .as_mut()
            .unwrap()
            .inputs
            .push(supplied);
        // Read-back can reorder resource HashMaps without changing the checkpoint.
        let resources = &mut running.stack_state.as_mut().unwrap().resources;
        let vault_resource = resources["deployer-secrets"].clone();
        for index in 0..32 {
            resources.insert(format!("unchanged-{index}"), vault_resource.clone());
        }
        state.db.set_deployment_state(&running).await.unwrap();
        running = state.db.get_deployment_state().await.unwrap().unwrap();
        state.db.clear_deployment_config().await.unwrap();
        vault
            .set_secret("key", "fake-test-value-1566")
            .await
            .unwrap();
        super::super::sync::refresh_running_secrets(&state, &mut running)
            .await
            .unwrap();
        let reports = &running.runtime_metadata.as_ref().unwrap().deployer_secrets;
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].status, alien_core::DeployerSecretStatus::Present);
        let stale = running.clone();
        vault.delete_secret("key").await.unwrap();
        super::super::sync::refresh_running_secrets(&state, &mut running)
            .await
            .unwrap();
        assert_eq!(
            running.runtime_metadata.as_ref().unwrap().deployer_secrets[0].status,
            alien_core::DeployerSecretStatus::Missing
        );
        let persisted = state.db.get_deployment_state().await.unwrap().unwrap();
        assert_eq!(persisted.runtime_metadata, running.runtime_metadata);
        // An older observation cannot overwrite a newer deployment checkpoint.
        assert!(!state
            .db
            .compare_and_set_deployment_state(&stale, &current)
            .await
            .unwrap());
        assert_eq!(
            state
                .db
                .get_deployment_state()
                .await
                .unwrap()
                .unwrap()
                .status,
            DeploymentStatus::Running
        );
    }

    #[tokio::test]
    async fn enrich_config_passes_operator_bindings_to_executor() {
        let temp_dir = tempfile::tempdir().unwrap();
        let encryption_key = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let db = OperatorDb::new(temp_dir.path().to_str().unwrap(), encryption_key)
            .await
            .unwrap();
        let mut bindings = ExternalBindings::new();
        bindings.insert(
            "data",
            ExternalBinding::Storage(StorageBinding::s3("existing-bucket")),
        );
        let operator_config = OperatorConfig::builder()
            .platform(Platform::Kubernetes)
            .stack_settings(StackSettings {
                external_bindings: Some(bindings.clone()),
                ..StackSettings::default()
            })
            .encryption_key(encryption_key)
            .build();

        let enriched = enrich_config(
            test_deployment_config(),
            &operator_config,
            Platform::Kubernetes,
            &db,
            None,
        )
        .await
        .unwrap();

        assert_eq!(enriched.external_bindings, bindings);
    }

    /// The deployment config the Operator runs with when the chart's `external-bindings.json`,
    /// the `toJson` of its `infrastructure` values, is `chart_json`: read at startup, then merged
    /// into the deployment's config.
    async fn config_from_chart(chart_json: &str) -> DeploymentConfig {
        let bindings = crate::cli::parse_json_opt::<ExternalBindings>(
            Some(chart_json.to_string()),
            "external bindings",
        )
        .unwrap();
        let settings = crate::cli::stack_settings_with_bindings(StackSettings::default(), bindings);
        let temp_dir = tempfile::tempdir().unwrap();
        let encryption_key = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let db = OperatorDb::new(temp_dir.path().to_str().unwrap(), encryption_key)
            .await
            .unwrap();
        let operator_config = OperatorConfig::builder()
            .platform(Platform::Kubernetes)
            .stack_settings(settings)
            .encryption_key(encryption_key)
            .build();
        enrich_config(
            test_deployment_config(),
            &operator_config,
            Platform::Kubernetes,
            &db,
            None,
        )
        .await
        .unwrap()
    }

    /// The full example's stack on a cluster in no cloud: its Frozen zone, whose type the `acme`
    /// extension declares as the example's build does, and the service provider that loaded the
    /// extension's hooks module.
    fn zone_stack() -> (
        alien_core::Stack,
        Arc<dyn alien_infra::PlatformServiceProvider>,
    ) {
        use alien_infra::extension_bundle::{testing::TestBundle, ExtensionBundle, Runtime};

        // Its link sets the zone's id, as the example's link does.
        let bundle = TestBundle::with_hooks_answering(
            "acme",
            "1.0.0",
            r#"[{"name":"link-zone","kind":"link","resourceType":"acme/dns-zone"}]"#,
            r#"{"env":{"ACME_ZONE_ID":"Z0123"},"permissionSets":[]}"#,
            &[],
        );
        let mut manifest = bundle.manifest.clone();
        manifest.resource_types.insert(
            "acme/dns-zone".into(),
            serde_json::from_value(serde_json::json!({
                "lifecycles": ["frozen", "live"],
                "holdsData": false,
                "setup": {
                    "cloudformation": ["aws"],
                    "terraform": ["aws", "gcp", "azure"],
                    "helm": ["kubernetes"],
                    "importers": ["aws", "gcp", "azure"]
                },
                "controllerPlatforms": ["aws", "gcp", "azure", "local"],
                "permissionSets": [],
                "link": { "permissionSets": [], "env": ["ACME_ZONE_ID"] },
                "schemas": {
                    "config": {},
                    "outputs": {
                        "type": "object",
                        "required": ["zoneId"],
                        "properties": { "zoneId": { "type": "string" } }
                    }
                }
            }))
            .unwrap(),
        );
        let stack: alien_core::Stack = serde_json::from_value(serde_json::json!({
            "id": "my-app",
            "resources": {
                "zone": {
                    "config": { "type": "acme/dns-zone", "id": "zone", "domain": "internal.acme.dev" },
                    "lifecycle": "frozen",
                    "dependencies": []
                }
            },
            "permissions": { "profiles": {}, "management": "auto" },
            "extensions": [manifest]
        }))
        .unwrap();
        let hooks = ExtensionBundle::open(&bundle.manifest, &bundle.zip)
            .unwrap()
            .load_hooks(&Runtime::new().unwrap())
            .unwrap();
        let provider: Arc<dyn alien_infra::PlatformServiceProvider> = Arc::new(
            alien_infra::DefaultPlatformServiceProvider::default()
                .with_extensions(
                    &stack,
                    Arc::new(alien_infra::LoadedExtensions::default().with_hooks("acme", hooks)),
                )
                .unwrap(),
        );
        (stack, provider)
    }

    fn kubernetes() -> alien_core::ClientConfig {
        alien_core::ClientConfig::Kubernetes(Box::new(alien_core::KubernetesClientConfig::Manual {
            server_url: "https://kubernetes.test".to_string(),
            certificate_authority_data: None,
            insecure_skip_tls_verify: Some(true),
            client_certificate_data: None,
            client_key_data: None,
            token: None,
            username: None,
            password: None,
            namespace: Some("app".to_string()),
            additional_headers: Default::default(),
        }))
    }

    /// The chart's binding for the full example's zone: the Operator reads it at startup, the
    /// zone binds with no controller on Kubernetes, and a workload that links it gets its outputs.
    #[tokio::test]
    async fn a_custom_frozen_resource_binds_from_the_chart_s_values() {
        use alien_core::{ResourceRef, ResourceStatus};

        let config = config_from_chart(
            r#"{"zone":{"type":"acme/dns-zone","service":"route53","zoneId":"Z0123"}}"#,
        )
        .await;
        let (stack, provider) = zone_stack();
        let executor = alien_infra::StackExecutor::builder(&stack, kubernetes())
            .deployment_config(&config)
            .service_provider(provider.clone())
            .build()
            .unwrap_or_else(|error| panic!("the executor refused the bound zone: {error:?}"));
        let stepped = executor
            .step(alien_core::StackState::new(Platform::Kubernetes))
            .await
            .unwrap()
            .next_state;
        let zone = &stepped.resources["zone"];
        assert_eq!(zone.status, ResourceStatus::Running);
        assert!(zone.external_binding);
        assert!(zone.internal_state.is_none(), "no controller ran");
        assert_eq!(
            serde_json::to_value(zone.outputs.as_ref().unwrap()).unwrap(),
            serde_json::json!({ "type": "acme/dns-zone", "zoneId": "Z0123" })
        );

        // A workload that links the zone, as a Kubernetes controller renders its environment.
        let registry = Arc::new(alien_infra::ResourceRegistry::with_built_ins());
        let worker = alien_core::Resource::new(
            alien_core::Worker::new("api".to_string())
                .code(alien_core::WorkerCode::Image {
                    image: "registry.example.com/api:1".to_string(),
                })
                .permissions("default".to_string())
                .build(),
        );
        let ctx = alien_infra::ResourceControllerContext {
            desired_config: &worker,
            platform: Platform::Kubernetes,
            client_config: kubernetes(),
            state: &stepped,
            resource_prefix: "app",
            registry: &registry,
            desired_stack: &stack,
            service_provider: &provider,
            deployment_config: &config,
            initial_setup_authority: alien_core::InitialSetupAuthority::DirectSetup,
            heartbeat_collector: Default::default(),
        };
        let env = HashMap::new();
        let (env_vars, _) = alien_infra::EnvironmentVariableBuilder::new(&env)
            .add_linked_resources(
                &[ResourceRef::new(
                    alien_core::ResourceType::from("acme/dns-zone"),
                    "zone".to_string(),
                )],
                &ctx,
                "api",
            )
            .await
            .unwrap()
            .build_with_bindings();
        let binding: serde_json::Value =
            serde_json::from_str(&env_vars["ALIEN_ZONE_BINDING"]).unwrap();
        assert_eq!(
            binding,
            serde_json::json!({ "service": "route53", "zoneId": "Z0123" })
        );
        assert_eq!(env_vars["ACME_ZONE_ID"], "Z0123", "the link's variable");
    }

    /// A value the type's outputs don't describe, such as a misspelled field, stops the
    /// deployment before anything runs.
    #[tokio::test]
    async fn a_chart_binding_the_zone_s_outputs_do_not_describe_is_refused() {
        let config = config_from_chart(
            r#"{"zone":{"type":"acme/dns-zone","service":"route53","zoneID":"Z0123"}}"#,
        )
        .await;
        let (stack, provider) = zone_stack();
        let Err(error) = alien_infra::StackExecutor::builder(&stack, kubernetes())
            .deployment_config(&config)
            .service_provider(provider)
            .build()
        else {
            panic!("the executor accepted a binding with no zoneId");
        };
        assert!(
            error.message.contains("the binding has no 'zoneId'")
                && error
                    .message
                    .contains("'zoneID' is not one of the outputs its schema names"),
            "{error:?}"
        );
    }

    #[tokio::test]
    async fn enrich_config_preserves_manager_bindings_not_overridden_by_installation() {
        let temp_dir = tempfile::tempdir().unwrap();
        let encryption_key = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let db = OperatorDb::new(temp_dir.path().to_str().unwrap(), encryption_key)
            .await
            .unwrap();
        let mut manager_bindings = ExternalBindings::new();
        manager_bindings.insert(
            "manager-only",
            ExternalBinding::Storage(StorageBinding::s3("manager-bucket")),
        );
        manager_bindings.insert(
            "shared",
            ExternalBinding::Storage(StorageBinding::s3("old-bucket")),
        );
        let mut installation_bindings = ExternalBindings::new();
        installation_bindings.insert(
            "shared",
            ExternalBinding::Storage(StorageBinding::s3("new-bucket")),
        );
        let operator_config = OperatorConfig::builder()
            .platform(Platform::Kubernetes)
            .stack_settings(StackSettings {
                external_bindings: Some(installation_bindings.clone()),
                ..StackSettings::default()
            })
            .encryption_key(encryption_key)
            .build();
        let mut config = test_deployment_config();
        config.external_bindings = manager_bindings;

        let enriched = enrich_config(config, &operator_config, Platform::Kubernetes, &db, None)
            .await
            .unwrap();

        assert!(enriched.external_bindings.has("manager-only"));
        assert_eq!(
            enriched.external_bindings.get("shared"),
            installation_bindings.get("shared")
        );
    }

    #[tokio::test]
    async fn enrich_config_keeps_supplied_database_ahead_of_chart_defaults() {
        let temp = tempfile::tempdir().unwrap();
        let encryption_key = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let db = OperatorDb::new(temp.path().to_str().unwrap(), encryption_key)
            .await
            .unwrap();
        let supplied: ExternalBindings = serde_json::from_value(serde_json::json!({
            "database":{"type":"postgres","service":"external","host":"db.example.com","port":5432,"database":"app","username":"app","password":"existing-password"},
            "bucket":{"type":"storage","service":"s3","bucketName":"existing-bucket"}
        })).unwrap();
        let defaults: ExternalBindings = serde_json::from_value(serde_json::json!({
            "database":{"type":"postgres","service":"external","host":"new-db.site.svc","port":5432,"database":"app","username":"postgres","password":{"secretKeyRef":{"name":"new-db","key":"password"}}}
        })).unwrap();
        let operator = OperatorConfig::builder()
            .platform(Platform::Kubernetes)
            .chart_default_bindings(defaults.clone())
            .encryption_key(encryption_key)
            .build();
        let mut config = test_deployment_config();
        config.external_bindings = supplied.clone();
        let enriched = enrich_config(config, &operator, Platform::Kubernetes, &db, None)
            .await
            .unwrap();
        assert_eq!(enriched.external_bindings, supplied);
        assert_eq!(enriched.stack_settings.external_bindings, Some(supplied));
        let enriched = enrich_config(
            test_deployment_config(),
            &operator,
            Platform::Kubernetes,
            &db,
            None,
        )
        .await
        .unwrap();
        assert_eq!(
            enriched.external_bindings.get("database"),
            defaults.get("database")
        );
        assert_eq!(
            enriched.stack_settings.external_bindings,
            Some(enriched.external_bindings.clone())
        );
    }

    #[tokio::test]
    async fn enrich_config_honors_customer_secret_key_reference() {
        let temp = tempfile::tempdir().unwrap();
        let encryption_key = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let db = OperatorDb::new(temp.path().to_str().unwrap(), encryption_key)
            .await
            .unwrap();
        let supplied: ExternalBindings = serde_json::from_value(serde_json::json!({
            "database":{"type":"postgres","service":"external","host":"db.example.com","port":5432,"database":"app","username":"app","password":"existing-password"},
            "bucket":{"type":"storage","service":"s3","bucketName":"existing-bucket"}
        })).unwrap();
        let defaults: ExternalBindings = serde_json::from_value(serde_json::json!({
            "database":{"type":"postgres","service":"external","host":"new-db.site.svc","port":5432,"database":"app","username":"postgres","password":{"secretKeyRef":{"name":"new-db","key":"password"}}}
        })).unwrap();
        let operator = OperatorConfig::builder()
            .platform(Platform::Kubernetes)
            .stack_settings(StackSettings {
                external_bindings: Some(defaults.clone()),
                ..Default::default()
            })
            .encryption_key(encryption_key)
            .build();
        let mut config = test_deployment_config();
        config.external_bindings = supplied.clone();
        let enriched = enrich_config(config, &operator, Platform::Kubernetes, &db, None)
            .await
            .unwrap();
        assert_eq!(
            enriched.external_bindings.get("database"),
            defaults.get("database")
        );
        assert_eq!(
            enriched.external_bindings.get("bucket"),
            supplied.get("bucket")
        );
    }

    #[tokio::test]
    async fn enrich_config_uses_operator_name_for_runtime_deployment_name() {
        let temp_dir = tempfile::tempdir().unwrap();
        let encryption_key = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let db = OperatorDb::new(temp_dir.path().to_str().unwrap(), encryption_key)
            .await
            .unwrap();
        db.set_deployment_id("dep_local").await.unwrap();

        let config = test_deployment_config();
        let operator_config = OperatorConfig::builder()
            .platform(Platform::Local)
            .agent_name("local-runner")
            .maybe_sync(Some(SyncConfig {
                url: "https://manager.example.com".parse().unwrap(),
                token: "ax_dep_test".to_string(),
            }))
            .encryption_key(encryption_key)
            .build();

        let enriched = enrich_config(config, &operator_config, Platform::Local, &db, None)
            .await
            .unwrap();

        assert_eq!(enriched.deployment_name.as_deref(), Some("local-runner"));
        assert!(enriched
            .environment_variables
            .variables
            .iter()
            .any(|var| { var.name == ENV_ALIEN_DEPLOYMENT_NAME && var.value == "local-runner" }));
        assert!(enriched
            .environment_variables
            .variables
            .iter()
            .any(|var| var.name == ENV_ALIEN_DEPLOYMENT_ID && var.value == "dep_local"));
    }

    #[tokio::test]
    async fn enrich_config_applies_operator_public_endpoints() {
        let temp_dir = tempfile::tempdir().unwrap();
        let encryption_key = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let db = OperatorDb::new(temp_dir.path().to_str().unwrap(), encryption_key)
            .await
            .unwrap();

        let config = test_deployment_config();
        let public_endpoints = HashMap::from([(
            "gateway".to_string(),
            HashMap::from([(
                "api".to_string(),
                "https://api.gateway.example.test".to_string(),
            )]),
        )]);
        let operator_config = OperatorConfig::builder()
            .platform(Platform::Local)
            .agent_name("local-runner")
            .maybe_public_endpoints(Some(public_endpoints.clone()))
            .encryption_key(encryption_key)
            .build();

        let enriched = enrich_config(config, &operator_config, Platform::Local, &db, None)
            .await
            .unwrap();

        assert_eq!(enriched.public_endpoints, Some(public_endpoints));
    }

    #[tokio::test]
    async fn enrich_config_scopes_push_token_per_command_enabled_worker() {
        let temp_dir = tempfile::tempdir().unwrap();
        let encryption_key = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let db = OperatorDb::new(temp_dir.path().to_str().unwrap(), encryption_key)
            .await
            .unwrap();
        db.set_deployment_id("dep_local").await.unwrap();

        let config = test_deployment_config();
        let operator_config = OperatorConfig::builder()
            .platform(Platform::Local)
            .agent_name("local-runner")
            .maybe_sync(Some(SyncConfig {
                url: "https://manager.example.com".parse().unwrap(),
                token: "ax_dep_test".to_string(),
            }))
            .encryption_key(encryption_key)
            .build();

        let worker_a = alien_core::Worker::new("worker-a".to_string())
            .code(alien_core::WorkerCode::Image {
                image: "worker:latest".to_string(),
            })
            .permissions("execution".to_string())
            .commands_enabled(true)
            .build();
        let worker_b = alien_core::Worker::new("worker-b".to_string())
            .code(alien_core::WorkerCode::Image {
                image: "worker:latest".to_string(),
            })
            .permissions("execution".to_string())
            .commands_enabled(true)
            .build();
        // Commands-disabled Worker must not expose a command push endpoint.
        let worker_off = alien_core::Worker::new("worker-off".to_string())
            .code(alien_core::WorkerCode::Image {
                image: "worker:latest".to_string(),
            })
            .permissions("execution".to_string())
            .build();
        let stack = alien_core::Stack::new("operator-command-target-stack".to_string())
            .add(worker_a, alien_core::ResourceLifecycle::Live)
            .add(worker_b, alien_core::ResourceLifecycle::Live)
            .add(worker_off, alien_core::ResourceLifecycle::Live)
            .build();

        let enriched = enrich_config(config, &operator_config, Platform::Local, &db, Some(&stack))
            .await
            .unwrap();

        let push_vars: Vec<_> = enriched
            .environment_variables
            .variables
            .iter()
            .filter(|var| var.name == alien_core::ENV_ALIEN_COMMANDS_TOKEN)
            .collect();

        // Every push token is scoped to exactly one command-enabled Worker —
        // nothing deployment-wide, nothing scoped to the disabled Worker.
        assert!(push_vars.iter().all(|var| {
            var.target_resources == Some(vec!["worker-a".to_string()])
                || var.target_resources == Some(vec!["worker-b".to_string()])
        }));

        // One token per command-enabled Worker.
        for worker_id in ["worker-a", "worker-b"] {
            let scoped: Vec<_> = push_vars
                .iter()
                .filter(|var| var.target_resources == Some(vec![worker_id.to_string()]))
                .collect();
            assert_eq!(scoped.len(), 1, "expected one token for {worker_id}");
            assert!(scoped.iter().any(|var| {
                var.name == alien_core::ENV_ALIEN_COMMANDS_TOKEN && var.value == "ax_dep_test"
            }));
        }
    }

    #[tokio::test]
    async fn enrich_config_scopes_receiver_env_per_command_enabled_container_and_daemon() {
        let temp_dir = tempfile::tempdir().unwrap();
        let encryption_key = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let db = OperatorDb::new(temp_dir.path().to_str().unwrap(), encryption_key)
            .await
            .unwrap();
        db.set_deployment_id("dep_local").await.unwrap();

        let config = test_deployment_config();
        let operator_config = OperatorConfig::builder()
            .platform(Platform::Local)
            .agent_name("local-runner")
            .maybe_sync(Some(SyncConfig {
                url: "https://manager.example.com".parse().unwrap(),
                token: "ax_dep_test".to_string(),
            }))
            .encryption_key(encryption_key)
            .build();

        let container = alien_core::Container::new("container-a".to_string())
            .code(alien_core::ContainerCode::Image {
                image: "container:latest".to_string(),
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
            .permissions("container-execution".to_string())
            .commands_enabled(true)
            .build();
        let daemon = alien_core::Daemon::new("daemon-b".to_string())
            .code(alien_core::DaemonCode::Image {
                image: "daemon:latest".to_string(),
            })
            .permissions("daemon-execution".to_string())
            .commands_enabled(true)
            .build();
        let stack = alien_core::Stack::new("operator-receiver-stack".to_string())
            .add(container, alien_core::ResourceLifecycle::Live)
            .add(daemon, alien_core::ResourceLifecycle::Live)
            .build();

        let enriched = enrich_config(config, &operator_config, Platform::Local, &db, Some(&stack))
            .await
            .unwrap();

        let receiver_names = [
            alien_core::ENV_ALIEN_COMMANDS_URL,
            alien_core::ENV_ALIEN_COMMANDS_TOKEN,
            alien_core::ENV_ALIEN_COMMANDS_TARGET_RESOURCE_ID,
            alien_core::ENV_ALIEN_COMMANDS_TARGET_RESOURCE_TYPE,
        ];
        let receiver_vars: Vec<_> = enriched
            .environment_variables
            .variables
            .iter()
            .filter(|var| receiver_names.contains(&var.name.as_str()))
            .collect();

        assert!(receiver_vars.iter().all(|var| {
            var.target_resources == Some(vec!["container-a".to_string()])
                || var.target_resources == Some(vec!["daemon-b".to_string()])
        }));

        for (resource_id, expected_type) in [("container-a", "container"), ("daemon-b", "daemon")] {
            let scoped: Vec<_> = receiver_vars
                .iter()
                .filter(|var| var.target_resources == Some(vec![resource_id.to_string()]))
                .collect();
            assert_eq!(
                scoped.len(),
                4,
                "expected 4 receiver vars for {resource_id}"
            );
            // Same normalized commands URL the operator derives for receivers.
            assert!(scoped
                .iter()
                .any(|var| var.name == alien_core::ENV_ALIEN_COMMANDS_URL
                    && var.value == "https://manager.example.com/v1"));
            assert!(scoped.iter().any(|var| {
                var.name == alien_core::ENV_ALIEN_COMMANDS_TOKEN && var.value == "ax_dep_test"
            }));
            assert!(scoped.iter().any(|var| {
                var.name == alien_core::ENV_ALIEN_COMMANDS_TARGET_RESOURCE_TYPE
                    && var.value == expected_type
            }));
            assert!(scoped.iter().any(|var| {
                var.name == alien_core::ENV_ALIEN_COMMANDS_TARGET_RESOURCE_ID
                    && var.value == resource_id
            }));
        }
    }
    fn receiver_container(id: &str, reads_capability_file: bool) -> alien_core::Container {
        let mut container = alien_core::Container::new(id.to_string())
            .code(alien_core::ContainerCode::Image {
                image: "container:latest".to_string(),
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
            .permissions("container-execution".to_string())
            .commands_enabled(true)
            .build();
        container.commands_capability_file = reads_capability_file;
        container
    }

    fn manager_receiver_token(resource_id: &str) -> EnvironmentVariable {
        EnvironmentVariable {
            name: alien_core::ENV_ALIEN_COMMANDS_TOKEN.to_string(),
            value: "ax_dep_manager".to_string(),
            var_type: EnvironmentVariableType::Secret,
            target_resources: Some(vec![resource_id.to_string()]),
        }
    }

    async fn enrich_enrolled_kubernetes(
        stack: &alien_core::Stack,
        manager_tokens: &[&str],
    ) -> Result<DeploymentConfig> {
        let temp_dir = tempfile::tempdir().unwrap();
        let encryption_key = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let db = OperatorDb::new(temp_dir.path().to_str().unwrap(), encryption_key)
            .await
            .unwrap();
        db.set_deployment_id("dep_k8s").await.unwrap();
        let mut config = test_deployment_config();
        config.runtime_identity_audience = Some("https://manager.example.com".to_string());
        config.environment_variables.variables = manager_tokens
            .iter()
            .map(|id| manager_receiver_token(id))
            .collect();
        // After enrollment the operator's own token is a short session token.
        let operator_config = OperatorConfig::builder()
            .platform(Platform::Kubernetes)
            .agent_name("k8s-runner")
            .maybe_sync(Some(SyncConfig {
                url: "https://manager.example.com".parse().unwrap(),
                token: "ax_session_not_for_workloads".to_string(),
            }))
            .encryption_key(encryption_key)
            .build();
        enrich_config(
            config,
            &operator_config,
            Platform::Kubernetes,
            &db,
            Some(stack),
        )
        .await
    }

    fn scoped_values<'a>(
        config: &'a DeploymentConfig,
        resource_id: &str,
        name: &str,
    ) -> Vec<&'a str> {
        config
            .environment_variables
            .variables
            .iter()
            .filter(|var| {
                var.name == name
                    && var.target_resources.as_deref() == Some(&[resource_id.to_string()][..])
            })
            .map(|var| var.value.as_str())
            .collect()
    }

    async fn enrolled_db(temp_dir: &tempfile::TempDir, encryption_key: &str) -> OperatorDb {
        let db = OperatorDb::new(temp_dir.path().to_str().unwrap(), encryption_key)
            .await
            .unwrap();
        db.set_deployment_id("dep_k8s").await.unwrap();
        db.write_daily_identity(&crate::daily_identity::DailyIdentityState {
            manager_url: "https://manager.example.com".into(),
            audience: "https://manager.example.com".into(),
            deployment_id: "dep_k8s".into(),
            current: "ax_day_v1_fixture".into(),
            pending: None,
            pending_started_at: None,
            expires_at: chrono::Utc::now() + chrono::Duration::hours(24),
            enrolled_code_hash: None,
            pending_enrollment: None,
            previous: None,
        })
        .await
        .unwrap();
        db
    }

    fn enrolled_operator_config(encryption_key: &str) -> OperatorConfig {
        OperatorConfig::builder()
            .platform(Platform::Kubernetes)
            .agent_name("k8s-runner")
            .maybe_sync(Some(SyncConfig {
                url: "https://manager.example.com".parse().unwrap(),
                token: "ax_session_not_for_workloads".to_string(),
            }))
            .encryption_key(encryption_key)
            .build()
    }

    /// The previous Operator pod accepted this config before it enrolled, so it
    /// has no manager audience and still carries the deployment token. That
    /// token is not a workload credential and the manager has since revoked
    /// it, so no receiver gets it: a receiver built before capability files has
    /// no key in this config and the Operator says so, until the manager sends
    /// the release again with the commands key. Receivers that read the
    /// capability file need nothing.
    #[tokio::test]
    async fn a_stale_deployment_token_is_never_handed_to_a_receiver() {
        let temp_dir = tempfile::tempdir().unwrap();
        let encryption_key = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let db = enrolled_db(&temp_dir, encryption_key).await;
        let operator_config = enrolled_operator_config(encryption_key);
        let mixed = alien_core::Stack::new("mixed".to_string())
            .add(
                receiver_container("old-api", false),
                alien_core::ResourceLifecycle::Live,
            )
            .add(
                receiver_container("new-api", true),
                alien_core::ResourceLifecycle::Live,
            )
            .build();
        let mut config = test_deployment_config();
        config.deployment_token = Some("ax_dep_before_enrollment".to_string());

        let error = enrich_config(
            config,
            &operator_config,
            Platform::Kubernetes,
            &db,
            Some(&mixed),
        )
        .await
        .expect_err("the receiver has no credential in a config saved before enrollment");
        let message = error.to_string();
        assert!(message.contains("old-api"), "{message}");
        assert!(message.contains("ALIEN_COMMANDS_TOKEN"), "{message}");
        assert!(!message.contains("ax_dep_before_enrollment"), "{message}");

        let marked = alien_core::Stack::new("marked".to_string())
            .add(
                receiver_container("new-api", true),
                alien_core::ResourceLifecycle::Live,
            )
            .build();
        let mut config = test_deployment_config();
        config.deployment_token = Some("ax_dep_before_enrollment".to_string());
        let enriched = enrich_config(
            config,
            &operator_config,
            Platform::Kubernetes,
            &db,
            Some(&marked),
        )
        .await
        .unwrap();
        assert!(enriched.deployment_token.is_none());
        assert!(scoped_values(&enriched, "new-api", "ALIEN_COMMANDS_TOKEN").is_empty());
        assert_eq!(
            scoped_values(&enriched, "new-api", "ALIEN_COMMANDS_CAPABILITY_FILE"),
            ["/var/run/alien-runtime/commands.json"]
        );
        assert!(enriched.environment_variables.variables.iter().all(|var| {
            var.value != "ax_session_not_for_workloads" && var.value != "ax_dep_before_enrollment"
        }));
    }

    /// A receiver built before capability files runs on the commands key the
    /// manager supplied with the config. The Operator passes that key on as it
    /// is, to that receiver only, and never swaps in another credential.
    #[tokio::test]
    async fn the_commands_key_the_manager_supplied_reaches_only_its_receiver() {
        let temp_dir = tempfile::tempdir().unwrap();
        let encryption_key = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let db = enrolled_db(&temp_dir, encryption_key).await;
        let operator_config = enrolled_operator_config(encryption_key);
        let stack = alien_core::Stack::new("mixed".to_string())
            .add(
                receiver_container("old-api", false),
                alien_core::ResourceLifecycle::Live,
            )
            .add(
                receiver_container("new-api", true),
                alien_core::ResourceLifecycle::Live,
            )
            .build();
        let mut config = test_deployment_config();
        config.runtime_identity_audience = Some("https://manager.example.com".to_string());
        config
            .environment_variables
            .variables
            .push(EnvironmentVariable {
                name: "ALIEN_COMMANDS_TOKEN".to_string(),
                value: "ax_cmds_supplied_by_the_manager".to_string(),
                var_type: EnvironmentVariableType::Secret,
                target_resources: Some(vec!["old-api".to_string()]),
            });

        let enriched = enrich_config(
            config,
            &operator_config,
            Platform::Kubernetes,
            &db,
            Some(&stack),
        )
        .await
        .unwrap();

        assert_eq!(
            scoped_values(&enriched, "old-api", "ALIEN_COMMANDS_TOKEN"),
            ["ax_cmds_supplied_by_the_manager"]
        );
        assert!(scoped_values(&enriched, "new-api", "ALIEN_COMMANDS_TOKEN").is_empty());
        assert!(enriched
            .environment_variables
            .variables
            .iter()
            .all(|var| var.value != "ax_session_not_for_workloads"));
    }

    /// While an update to a release whose receivers read the capability file
    /// is waiting or running, the installed release still has a receiver built
    /// before capability files, and the stored config belongs to the update
    /// and has no commands key. Renewing the installed workloads' credentials
    /// takes that config with the update's stack, so it neither fails nor
    /// logs a warning for a receiver the update replaces.
    #[tokio::test]
    async fn renewing_credentials_during_a_marked_update_needs_no_commands_key() {
        let temp_dir = tempfile::tempdir().unwrap();
        let encryption_key = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let db = enrolled_db(&temp_dir, encryption_key).await;
        let release = |id: &str, marked: bool| alien_core::ReleaseInfo {
            release_id: Some(id.to_string()),
            version: None,
            description: None,
            stack: alien_core::Stack::new("app".to_string())
                .add(
                    receiver_container("agent", marked),
                    alien_core::ResourceLifecycle::Live,
                )
                .build(),
            signed: None,
            signer_lists: Vec::new(),
        };
        db.set_deployment_state(&DeploymentState {
            status: DeploymentStatus::UpdatePending,
            platform: Platform::Kubernetes,
            current_release: Some(release("rel_old", false)),
            target_release: Some(release("rel_new", true)),
            stack_state: Some(alien_core::StackState::new(Platform::Kubernetes)),
            error: None,
            environment_info: None,
            runtime_metadata: None,
            retry_requested: false,
            protocol_version: alien_core::DEPLOYMENT_PROTOCOL_VERSION,
        })
        .await
        .unwrap();
        let mut config = test_deployment_config();
        config.runtime_identity_audience = Some("https://manager.example.com".to_string());
        db.set_deployment_config(&config).await.unwrap();
        let state = OperatorState {
            config: enrolled_operator_config(encryption_key),
            db: Arc::new(db),
            service_provider: None,
            operations_sync_handler: None,
            pod_log_collection: false,
            cancel: tokio_util::sync::CancellationToken::new(),
        };

        renew_installed_credentials(&state)
            .await
            .expect("the update's config pairs with the update's stack");
    }

    #[tokio::test]
    async fn enrolled_receivers_get_the_capability_file_only_when_the_build_marked_them() {
        let mut daemon = alien_core::Daemon::new("old-agent".to_string())
            .code(alien_core::DaemonCode::Image {
                image: "daemon:latest".to_string(),
            })
            .permissions("daemon-execution".to_string())
            .commands_enabled(true)
            .build();
        daemon.commands_capability_file = false;
        let stack = alien_core::Stack::new("operator-marker-stack".to_string())
            .add(
                receiver_container("new-web", true),
                alien_core::ResourceLifecycle::Live,
            )
            .add(
                receiver_container("old-web", false),
                alien_core::ResourceLifecycle::Live,
            )
            .add(daemon, alien_core::ResourceLifecycle::Live)
            .build();

        let enriched = enrich_enrolled_kubernetes(&stack, &["new-web", "old-web", "old-agent"])
            .await
            .unwrap();

        // Marked: only the capability file, no token.
        assert!(scoped_values(&enriched, "new-web", "ALIEN_COMMANDS_TOKEN").is_empty());
        assert_eq!(
            scoped_values(&enriched, "new-web", "ALIEN_COMMANDS_CAPABILITY_FILE"),
            ["/var/run/alien-runtime/commands.json"]
        );
        // Unmarked (older receiver): the token the manager supplied, no file.
        for id in ["old-web", "old-agent"] {
            assert_eq!(
                scoped_values(&enriched, id, "ALIEN_COMMANDS_TOKEN"),
                ["ax_dep_manager"],
                "{id} keeps its token"
            );
            assert!(
                scoped_values(&enriched, id, "ALIEN_COMMANDS_CAPABILITY_FILE").is_empty(),
                "{id} gets no capability file"
            );
            assert_eq!(
                scoped_values(&enriched, id, "ALIEN_COMMANDS_URL"),
                ["https://manager.example.com/v1"]
            );
        }
        // The session token is never given to a workload.
        assert!(!enriched
            .environment_variables
            .variables
            .iter()
            .any(|var| var.value == "ax_session_not_for_workloads"));
    }

    #[tokio::test]
    async fn an_older_receiver_without_a_manager_token_fails_the_deploy_with_its_name() {
        let stack = alien_core::Stack::new("operator-marker-stack".to_string())
            .add(
                receiver_container("old-web", false),
                alien_core::ResourceLifecycle::Live,
            )
            .build();

        let error = enrich_enrolled_kubernetes(&stack, &[]).await.unwrap_err();

        assert!(
            error.to_string().contains("old-web"),
            "the error names the receiver: {error}"
        );
    }

    #[tokio::test]
    async fn disconnected_commands_are_disabled_only_for_command_targets() {
        let directory = tempfile::tempdir().unwrap();
        let key = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let db = OperatorDb::new(directory.path().to_str().unwrap(), key)
            .await
            .unwrap();
        let operator = OperatorConfig::builder()
            .platform(Platform::Kubernetes)
            .airgap_target_secret("site-target")
            .encryption_key(key)
            .build();
        let mut stack = alien_core::Stack::new("app".into()).build();
        for (id, enabled) in [("setup", true), ("ordinary", false)] {
            let daemon = alien_core::Daemon::new(id.into())
                .code(alien_core::DaemonCode::Image {
                    image: "app:1".into(),
                })
                .commands_enabled(enabled)
                .build();
            stack.resources.insert(
                id.into(),
                alien_core::ResourceEntry {
                    config: alien_core::Resource::new(daemon),
                    display_name: None,
                    description: None,
                    lifecycle: alien_core::ResourceLifecycle::Live,
                    dependencies: vec![],
                    remote_access: false,
                    enabled_when: None,
                },
            );
        }
        let config = enrich_config(
            test_deployment_config(),
            &operator,
            Platform::Kubernetes,
            &db,
            Some(&stack),
        )
        .await
        .unwrap();
        let variables = config.environment_variables.variables;
        let disabled: Vec<_> = variables
            .iter()
            .filter(|v| v.name == "ALIEN_COMMANDS_DISABLED")
            .collect();
        assert_eq!(disabled.len(), 1);
        assert_eq!(disabled[0].value, "true");
        assert_eq!(disabled[0].target_resources, Some(vec!["setup".into()]));
        assert!(!variables
            .iter()
            .any(|v| v.name == alien_core::ENV_ALIEN_COMMANDS_TOKEN
                || v.name == alien_core::ENV_ALIEN_COMMANDS_URL));
    }

    /// An installed Test deployment whose `worker` fails its first health refresh.
    async fn installed_refresh_fixture() -> (OperatorState, tempfile::TempDir) {
        installed_refresh_fixture_with_ready_delay(None).await
    }

    /// `ready_delay_secs`: the heartbeat delay every worker's Ready handler returns
    /// (real Kubernetes handlers return 30 s); `None` returns no delay.
    async fn installed_refresh_fixture_with_ready_delay(
        ready_delay_secs: Option<u64>,
    ) -> (OperatorState, tempfile::TempDir) {
        alien_deployment::set_ephemeral_secrets_digest_key();
        let directory = tempfile::tempdir().unwrap();
        let key = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let db = OperatorDb::new(directory.path().to_str().unwrap(), key)
            .await
            .unwrap();
        db.set_deployment_id("dep_site").await.unwrap();
        let worker = alien_core::Worker::new("worker".into())
            .code(alien_core::WorkerCode::Image {
                image: "test:1".into(),
            })
            .permissions("default".into())
            .environment(HashMap::from([
                ("SIMULATE_OBSERVED_URL_REFRESH".into(), "true".into()),
                ("SIMULATE_OBSERVED_REFRESH_FAIL_ONCE".into(), "true".into()),
            ]))
            .build();
        let ready_delay_env: HashMap<String, String> = ready_delay_secs
            .map(|secs| ("SIMULATE_READY_DELAY_SECS".to_string(), secs.to_string()))
            .into_iter()
            .collect();
        let mut worker = worker;
        worker.environment.extend(ready_delay_env.clone());
        let mut stack =
            alien_core::Stack::new("site".into()).add(worker, alien_core::ResourceLifecycle::Live);
        for index in 0..8 {
            let worker = alien_core::Worker::new(format!("healthy-{index}"))
                .code(alien_core::WorkerCode::Image {
                    image: "test:1".into(),
                })
                .permissions("default".into())
                .environment(ready_delay_env.clone())
                .build();
            stack = stack.add(worker, alien_core::ResourceLifecycle::Live);
        }
        let mut stack = stack.build();
        stack
            .permissions
            .profiles
            .insert("default".into(), alien_core::PermissionProfile::new());
        let release = alien_core::ReleaseInfo {
            release_id: Some("rel_1".into()),
            version: Some("1.0.0".into()),
            description: None,
            stack,
            signed: None,
            signer_lists: Vec::new(),
        };
        db.set_deployment_state(&DeploymentState {
            status: DeploymentStatus::Pending,
            platform: Platform::Test,
            current_release: None,
            target_release: Some(release),
            stack_state: None,
            error: None,
            environment_info: None,
            runtime_metadata: None,
            retry_requested: false,
            protocol_version: alien_core::DEPLOYMENT_PROTOCOL_VERSION,
        })
        .await
        .unwrap();
        db.set_deployment_config(&test_deployment_config())
            .await
            .unwrap();
        let state = OperatorState {
            config: OperatorConfig::builder()
                .platform(Platform::Test)
                .data_dir(directory.path().to_str().unwrap())
                .airgap_target_secret("site-target")
                .encryption_key(key)
                .build(),
            db: Arc::new(db),
            service_provider: None,
            operations_sync_handler: None,
            pod_log_collection: false,
            cancel: tokio_util::sync::CancellationToken::new(),
        };
        for _ in 0..20 {
            run_deployment_continuously(&state).await.unwrap();
            if state
                .db
                .get_deployment_state()
                .await
                .unwrap()
                .unwrap()
                .status
                == DeploymentStatus::Running
            {
                break;
            }
        }
        let installed = state.db.get_deployment_state().await.unwrap().unwrap();
        assert_eq!(installed.status, DeploymentStatus::Running);
        (state, directory)
    }

    #[test]
    fn the_running_check_follows_the_controller_delay_within_the_floor_and_cap() {
        let delay = |ms: u64| clamp_running_check_delay(Duration::from_millis(ms));
        assert_eq!(delay(0), Duration::from_secs(5));
        assert_eq!(delay(2_000), Duration::from_secs(5));
        assert_eq!(delay(30_000), Duration::from_secs(30));
        assert_eq!(delay(300_000), Duration::from_secs(60));
    }

    /// Controllers that ask for 30 s (the Kubernetes Ready handlers do) give a check
    /// every 30 s: about 20 in ten minutes, not 120.
    #[tokio::test(start_paused = true)]
    async fn controllers_asking_for_thirty_seconds_give_about_twenty_checks_in_ten_minutes() {
        let (state, _directory) = installed_refresh_fixture_with_ready_delay(Some(30)).await;
        state
            .db
            .write_daily_identity(&crate::daily_identity::DailyIdentityState {
                manager_url: "https://manager.example".into(),
                audience: "https://manager.example".into(),
                deployment_id: "dep_site".into(),
                current: "ax_day_v1_fixture".into(),
                pending: None,
                pending_started_at: None,
                expires_at: chrono::Utc::now() + chrono::Duration::hours(24),
                enrolled_code_hash: None,
                pending_enrollment: None,
                previous: None,
            })
            .await
            .unwrap();
        let state = Arc::new(state);
        let waits = Arc::new(std::sync::Mutex::new(Vec::<Duration>::new()));
        let observed = Arc::clone(&waits);
        let task = tokio::spawn(run_deployment_loop_observed(
            Arc::clone(&state),
            move |delay| observed.lock().unwrap().push(delay),
        ));
        tokio::time::sleep(Duration::from_secs(602)).await;
        let waits_now = waits.lock().unwrap().clone();
        state.cancel.cancel();
        task.await.unwrap();
        assert!(
            (19..=22).contains(&waits_now.len()),
            "{} checks in ten simulated minutes: {waits_now:?}",
            waits_now.len()
        );
        assert!(
            // The fixture's first worker dips into RefreshFailed once: two closer checks.
            waits_now
                .iter()
                .skip(2)
                .all(|wait| *wait == Duration::from_secs(30)),
            "{waits_now:?}"
        );
    }

    /// A healthy installed deployment: the loop checks it at the controllers'
    /// pace, not every interval (a 1 s interval ran a check every second).
    #[tokio::test(start_paused = true)]
    async fn a_healthy_running_deployment_is_checked_at_most_every_five_seconds() {
        let (state, _directory) = installed_refresh_fixture().await;
        state
            .db
            .write_daily_identity(&crate::daily_identity::DailyIdentityState {
                manager_url: "https://manager.example".into(),
                audience: "https://manager.example".into(),
                deployment_id: "dep_site".into(),
                current: "ax_day_v1_fixture".into(),
                pending: None,
                pending_started_at: None,
                expires_at: chrono::Utc::now() + chrono::Duration::hours(24),
                enrolled_code_hash: None,
                pending_enrollment: None,
                previous: None,
            })
            .await
            .unwrap();
        let state = Arc::new(state);
        let waits = Arc::new(std::sync::Mutex::new(Vec::<Duration>::new()));
        let observed = Arc::clone(&waits);
        let task = tokio::spawn(run_deployment_loop_observed(
            Arc::clone(&state),
            move |delay| observed.lock().unwrap().push(delay),
        ));
        tokio::time::sleep(Duration::from_secs(602)).await;
        let waits_now = waits.lock().unwrap().clone();
        assert!(
            waits_now.len() <= 600 / 5 + 2,
            "{} checks in ten simulated minutes",
            waits_now.len()
        );
        assert!(waits_now.iter().all(|wait| *wait >= RUNNING_CHECK_FLOOR));

        // A wake (a new target) is handled at once, mid-wait.
        let before = waits_now.len();
        state.db.request_deployment();
        tokio::time::sleep(Duration::from_millis(1)).await;
        assert_eq!(waits.lock().unwrap().len(), before + 1);
        state.cancel.cancel();
        task.await.unwrap();
    }

    /// The manager replays the same target on every sync. That must not cut a wait
    /// short (a hold's backoff, a step's requested delay); a changed target does.
    #[tokio::test(start_paused = true)]
    async fn replayed_targets_do_not_wake_the_loop_and_a_changed_one_does() {
        let (_directory, state) = held_operator(DeploymentStatus::TeardownRequired).await;
        let stored = state.db.get_deployment_state().await.unwrap().unwrap();
        let target = alien_core::sync::TargetDeployment {
            release_info: stored.target_release.clone().unwrap(),
            config: test_deployment_config(),
            signed_config: None,
        };
        let waits = Arc::new(std::sync::Mutex::new(Vec::<Duration>::new()));
        let observed = Arc::clone(&waits);
        let task = tokio::spawn(run_deployment_loop_observed(
            Arc::clone(&state),
            move |delay| observed.lock().unwrap().push(delay),
        ));
        // The first sync stores the target the Operator did not hold yet.
        crate::loops::sync::accept_target(&state, &target, false)
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(1)).await;
        let after_first = waits.lock().unwrap().len();

        // Ten minutes of syncs, one every 30 s, each replaying the same target.
        for _ in 0..20 {
            tokio::time::sleep(Duration::from_secs(30)).await;
            crate::loops::sync::accept_target(&state, &target, false)
                .await
                .unwrap();
        }
        let passes = waits.lock().unwrap().len() - after_first;
        assert!(
            passes <= 14,
            "{passes} passes in ten minutes of replayed targets: a replay woke the loop"
        );
        assert!(
            waits.lock().unwrap().contains(&HOLD_CAP),
            "the backoff reached its cap"
        );

        // A different release is a real change: the loop runs a pass at once.
        let before = waits.lock().unwrap().len();
        let mut changed = target.clone();
        changed.release_info.release_id = Some("rel_changed".into());
        crate::loops::sync::accept_target(&state, &changed, false)
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(1)).await;
        assert_eq!(waits.lock().unwrap().len(), before + 1);
        state.cancel.cancel();
        task.await.unwrap();
    }

    #[tokio::test]
    async fn an_approval_decision_wakes_the_deployment_loop() {
        let (_directory, state) = held_operator(DeploymentStatus::Running).await;
        let approval = crate::db::Approval {
            id: "apr_1".into(),
            release_info: None,
            deployment_config: test_deployment_config(),
            status: crate::db::ApprovalStatus::Pending,
            reason: None,
            created_at: chrono::Utc::now().to_rfc3339(),
            decided_at: None,
            decided_by: None,
        };
        state.db.create_approval(&approval).await.unwrap();
        let woken = |db: Arc<OperatorDb>| async move {
            tokio::time::timeout(Duration::from_millis(50), db.wait_for_deployment())
                .await
                .is_ok()
        };
        assert!(!woken(Arc::clone(&state.db)).await);
        state
            .db
            .decide_approval("apr_1", crate::db::ApprovalStatus::Approved, None, None)
            .await
            .unwrap();
        assert!(woken(Arc::clone(&state.db)).await);
    }

    #[tokio::test]
    async fn disconnected_health_refresh_observes_failure_and_recovery() {
        let (state, _directory) = installed_refresh_fixture().await;
        let installed = state.db.get_deployment_state().await.unwrap().unwrap();
        assert!(state.db.get_deployment_config().await.unwrap().is_some());
        refresh_installed_deployment(&state).await.unwrap();
        let failed = state.db.get_deployment_state().await.unwrap().unwrap();
        assert_eq!(failed.status, DeploymentStatus::RefreshFailed);
        assert_eq!(
            failed.stack_state.as_ref().unwrap().resources["worker"].status,
            alien_core::ResourceStatus::RefreshFailed
        );
        assert_eq!(failed.current_release, installed.current_release);
        refresh_installed_deployment(&state).await.unwrap();
        let recovered = state.db.get_deployment_state().await.unwrap().unwrap();
        assert_eq!(recovered.status, DeploymentStatus::Running);
        let resources = &recovered.stack_state.as_ref().unwrap().resources;
        // Preflights add the shared secrets vault and default service account.
        assert_eq!(resources.len(), 11);
        assert!(resources.contains_key("secrets"));
        assert!(resources.contains_key("default-sa"));
        assert!(resources.contains_key("worker"));
        for index in 0..8 {
            assert!(resources.contains_key(&format!("healthy-{index}")));
        }
        assert!(resources
            .values()
            .all(|resource| resource.status == alien_core::ResourceStatus::Running));
        assert_eq!(recovered.current_release, installed.current_release);
        assert!(recovered.target_release.is_none());
        assert!(state.db.get_deployment_config().await.unwrap().is_some());
    }

    /// An enrolled Operator refreshes its installed deployment on every idle
    /// pass. That pass runs each workload's ready handler, which rewrites the
    /// workload's credential Secret before the previous leaf expires.
    #[tokio::test]
    async fn enrolled_operator_refreshes_installed_workloads_on_every_idle_pass() {
        let (state, _directory) = installed_refresh_fixture().await;
        // A legacy Operator leaves an idle installed deployment alone.
        run_deployment_continuously(&state).await.unwrap();
        assert_eq!(
            state
                .db
                .get_deployment_state()
                .await
                .unwrap()
                .unwrap()
                .status,
            DeploymentStatus::Running
        );

        state
            .db
            .write_daily_identity(&crate::daily_identity::DailyIdentityState {
                manager_url: "https://manager.example".into(),
                audience: "https://manager.example".into(),
                deployment_id: "dep_site".into(),
                current: "ax_day_v1_fixture".into(),
                pending: None,
                pending_started_at: None,
                expires_at: chrono::Utc::now() + chrono::Duration::hours(24),
                enrolled_code_hash: None,
                pending_enrollment: None,
                previous: None,
            })
            .await
            .unwrap();
        run_deployment_continuously(&state).await.unwrap();
        let refreshed = state.db.get_deployment_state().await.unwrap().unwrap();
        assert_eq!(refreshed.status, DeploymentStatus::RefreshFailed);
        assert_eq!(
            refreshed.stack_state.as_ref().unwrap().resources["worker"].status,
            alien_core::ResourceStatus::RefreshFailed
        );
        run_deployment_continuously(&state).await.unwrap();
        assert_eq!(
            state
                .db
                .get_deployment_state()
                .await
                .unwrap()
                .unwrap()
                .status,
            DeploymentStatus::Running
        );
    }

    /// A resource wait's poll budget is `max_times` steps, one per requested
    /// delay. The Kubernetes public endpoint wait asks for 60 polls 10s apart,
    /// so it must not time out before about ten minutes. The loop is driven
    /// pass by pass and its waits are added up instead of slept.
    #[tokio::test]
    async fn resource_poll_budget_lasts_its_requested_delays() {
        alien_deployment::set_ephemeral_secrets_digest_key();
        const MAX_POLLS: u32 = 60;
        const POLL_DELAY: Duration = Duration::from_secs(10);

        let directory = tempfile::tempdir().unwrap();
        let key = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let db = OperatorDb::new(directory.path().to_str().unwrap(), key)
            .await
            .unwrap();
        db.set_deployment_id("dep_poll").await.unwrap();
        let worker = alien_core::Worker::new("worker".into())
            .code(alien_core::WorkerCode::Image {
                image: "test:1".into(),
            })
            .permissions("default".into())
            .environment(HashMap::from([
                ("SIMULATE_STAY_EXHAUSTION".into(), MAX_POLLS.to_string()),
                (
                    "SIMULATE_STAY_DELAY_MS".into(),
                    POLL_DELAY.as_millis().to_string(),
                ),
            ]))
            .build();
        let mut stack = alien_core::Stack::new("poll".into())
            .add(worker, alien_core::ResourceLifecycle::Live)
            .build();
        stack
            .permissions
            .profiles
            .insert("default".into(), alien_core::PermissionProfile::new());
        db.set_deployment_state(&DeploymentState {
            status: DeploymentStatus::Pending,
            platform: Platform::Test,
            current_release: None,
            target_release: Some(alien_core::ReleaseInfo {
                release_id: Some("rel_1".into()),
                version: Some("1.0.0".into()),
                description: None,
                stack,
                signed: None,
                signer_lists: Vec::new(),
            }),
            stack_state: None,
            error: None,
            environment_info: None,
            runtime_metadata: None,
            retry_requested: false,
            protocol_version: alien_core::DEPLOYMENT_PROTOCOL_VERSION,
        })
        .await
        .unwrap();
        db.set_deployment_config(&test_deployment_config())
            .await
            .unwrap();
        let state = OperatorState {
            config: OperatorConfig::builder()
                .platform(Platform::Test)
                .data_dir(directory.path().to_str().unwrap())
                .encryption_key(key)
                .build(),
            db: Arc::new(db),
            service_provider: None,
            operations_sync_handler: None,
            pod_log_collection: false,
            cancel: tokio_util::sync::CancellationToken::new(),
        };
        let interval = Duration::from_secs(state.config.deployment_interval_seconds);
        assert!(
            interval < POLL_DELAY,
            "the loop interval must be shorter than the poll delay for this test to mean anything"
        );

        let mut hold = HoldBackoff::default();
        let mut waited = Duration::ZERO;
        let mut passes = 0;
        let failed = loop {
            let current = state.db.get_deployment_state().await.unwrap().unwrap();
            if current.status.is_failed() {
                break current;
            }
            assert!(
                passes < 4 * MAX_POLLS,
                "deployment never failed; status {:?} after {passes} passes",
                current.status
            );
            waited += run_deployment_pass(&state, interval, &mut hold).await;
            passes += 1;
        };

        assert_eq!(failed.status, DeploymentStatus::ProvisioningFailed);
        let worker = &failed.stack_state.as_ref().unwrap().resources["worker"];
        assert_eq!(worker.status, alien_core::ResourceStatus::ProvisionFailed);
        assert_eq!(
            worker.error.as_ref().map(|error| error.code.as_str()),
            Some("RESOURCE_POLLING_TIMEOUT")
        );
        // The 60th poll times out, after 59 waits of the requested delay.
        let budget = POLL_DELAY * (MAX_POLLS - 1);
        assert!(
            waited >= budget,
            "poll budget spent after {waited:?}; the controller asked for {budget:?}"
        );
        // Setup passes before the wait add only a few short waits.
        assert!(
            waited <= budget + POLL_DELAY * 6,
            "loop waited {waited:?}, far longer than the {budget:?} budget"
        );
    }
}
