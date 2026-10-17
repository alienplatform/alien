//! Deployment loop — background task that drives deployment state machines.
//!
//! The loop:
//! 1. Polls for deployments needing work (via `DeploymentStore::acquire`)
//! 2. Resolves credentials for each deployment (via `CredentialResolver`)
//! 3. Calls `alien_deployment::step()` repeatedly until stable or delayed
//! 4. Reconciles the result (via `DeploymentStore::reconcile`)
//! 5. Releases the lock (via `DeploymentStore::release`)

use std::collections::HashMap;
use std::panic::AssertUnwindSafe;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::{stream, FutureExt, StreamExt};
use sha2::{Digest, Sha256};
use tokio::sync::watch;
use tracing::{debug, error, info, warn};

use alien_bindings::BindingsProviderApi;
use alien_core::{
    DeploymentConfig, DeploymentModel, DeploymentState, DeploymentStatus, EnvironmentVariable,
    EnvironmentVariableType, EnvironmentVariablesSnapshot, Platform, ReleaseInfo, Stack,
    ENV_ALIEN_DEPLOYMENT_ID, ENV_ALIEN_DEPLOYMENT_NAME,
};
use alien_deployment::loop_contract::LoopOperation;
use alien_deployment::runner::{failed_status_for_deployment_error, RunnerPolicy, RunnerResult};
use alien_error::{AlienError, Context, GenericError};
use alien_infra::DefaultPlatformServiceProvider;
use alien_local::{LocalBindingsProvider, LocalContainerManager};

use crate::auth::Subject;
use crate::config::ManagerConfig;
use crate::error::REMOTE_CREDENTIAL_HANDOFF_FAILED_CODE;
use crate::traits::deployment_store::{DeploymentFilter, DeploymentRecord, ReconcileData};
use crate::traits::{CredentialResolver, DeploymentStore, ReleaseStore, ServerBindings};
use crate::transports::ManagerTransport;

/// Maximum number of step() calls per deployment per tick.
const MAX_STEPS_PER_TICK: usize = 100;
const MAX_STEPS_PER_HEARTBEAT: usize = 1;
/// Maximum number of deployments to process concurrently per tick.
///
/// This is also the acquire batch size. A deployment's lease is only renewed
/// once it is actually being processed, so acquiring more than can be processed
/// at once would leave the surplus holding leases that nothing is extending.
pub(crate) const MAX_CONCURRENT_DEPLOYMENTS: usize = 4;
/// Maximum acquire/process batches before yielding back to the interval sleep.
const MAX_ACQUIRE_BATCHES_PER_TICK: usize = 16;
/// How long the loop leaves alone a deployment whose phase its credentials
/// cannot provision, unless the deployment's status changes first. Only a
/// setup-capable driver (the CLI) can move such a deployment, and it acquires
/// it directly, so re-acquiring it every tick only rewrites its row.
const UNPROVISIONABLE_RECHECK_INTERVAL: chrono::Duration = chrono::Duration::minutes(5);
/// Google documents IAM policy changes as typically propagating within two
/// minutes, but potentially taking seven minutes or longer. Keep the first
/// target-side token exchange retryable for a bounded window after setup hands
/// the deployment to Provisioning.
const GCP_CREDENTIAL_HANDOFF_GRACE_PERIOD: Duration = Duration::from_secs(10 * 60);

/// Build a `HorizonMachineImage` from `ALIEN_BYO_HORIZON_AMI_AMD64`/`_ARM64`
/// + `AWS_REGION` env vars. Returns `None` when no AMI env vars are set so
/// the production resolver stays in charge.
fn synthesize_byo_horizon_machine_image() -> Option<alien_core::HorizonMachineImage> {
    use alien_core::{
        HorizonAwsMachineImages, HorizonMachineArchitecture, HorizonMachineBaseImage,
        HorizonMachineImage,
    };

    let amd64 = std::env::var("ALIEN_BYO_HORIZON_AMI_AMD64").ok();
    let arm64 = std::env::var("ALIEN_BYO_HORIZON_AMI_ARM64").ok();
    if amd64.as_deref().unwrap_or("").is_empty() && arm64.as_deref().unwrap_or("").is_empty() {
        return None;
    }

    let region = std::env::var("AWS_REGION")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "us-east-1".to_string());

    let mut amis: HashMap<HorizonMachineArchitecture, HashMap<String, String>> = HashMap::new();
    if let Some(ami) = amd64.filter(|s| !s.is_empty()) {
        let mut by_region = HashMap::new();
        by_region.insert(region.clone(), ami);
        amis.insert(HorizonMachineArchitecture::Amd64, by_region);
    }
    if let Some(ami) = arm64.filter(|s| !s.is_empty()) {
        let mut by_region = HashMap::new();
        by_region.insert(region.clone(), ami);
        amis.insert(HorizonMachineArchitecture::Arm64, by_region);
    }

    Some(HorizonMachineImage {
        reader_capabilities: None,
        runtime_isolation_generation: 0,
        channel: "byo".to_string(),
        machine_image_version: "byo-local".to_string(),
        horizond_version: "byo".to_string(),
        git_sha: "byo".to_string(),
        created_at: "1970-01-01T00:00:00Z".to_string(),
        base_image: HorizonMachineBaseImage {
            name: "byo".to_string(),
            version: "byo".to_string(),
        },
        horizond_artifacts: HashMap::new(),
        aws: Some(HorizonAwsMachineImages { amis }),
        gcp: None,
        azure: None,
    })
}

#[derive(Debug, Clone, Copy)]
struct ProcessOptions {
    max_steps: usize,
    require_heartbeats_enabled: bool,
}

impl ProcessOptions {
    fn deployment_tick() -> Self {
        Self {
            max_steps: MAX_STEPS_PER_TICK,
            require_heartbeats_enabled: false,
        }
    }

    fn heartbeat_tick() -> Self {
        Self {
            max_steps: MAX_STEPS_PER_HEARTBEAT,
            require_heartbeats_enabled: true,
        }
    }
}
fn panic_payload_message(payload: &(dyn std::any::Any + Send)) -> &str {
    if let Some(message) = payload.downcast_ref::<&'static str>() {
        message
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.as_str()
    } else {
        "<non-string panic payload>"
    }
}

fn active_work_statuses() -> Vec<String> {
    vec![
        "pending",
        "initial-setup",
        "provisioning",
        "waiting-for-machines",
        "waiting-for-secrets",
        "update-pending",
        "updating",
        "delete-pending",
        "deleting",
    ]
    .into_iter()
    .map(String::from)
    .collect()
}

fn retryable_failed_statuses() -> Vec<String> {
    vec![
        "preflights-failed",
        "initial-setup-failed",
        "provisioning-failed",
        "refresh-failed",
        "update-failed",
        "delete-failed",
    ]
    .into_iter()
    .map(String::from)
    .collect()
}

fn manager_candidate_statuses() -> Vec<String> {
    let mut statuses = active_work_statuses();
    statuses.extend(retryable_failed_statuses());
    statuses
}

fn get_or_create_local_bindings_provider(
    cache: &Mutex<HashMap<String, Arc<LocalBindingsProvider>>>,
    state_dir: &Path,
    deployment_id: &str,
) -> Result<Arc<LocalBindingsProvider>, AlienError> {
    if let Some(existing) = cache
        .lock()
        .expect("local_bindings_cache poisoned")
        .get(deployment_id)
        .cloned()
    {
        return Ok(existing);
    }

    let local_state_dir = state_dir.join(deployment_id);
    let provider = LocalBindingsProvider::new(&local_state_dir).context(GenericError {
        message: format!(
            "Failed to create LocalBindingsProvider for {}",
            deployment_id
        ),
    })?;

    let mut cache = cache.lock().expect("local_bindings_cache poisoned");
    Ok(cache
        .entry(deployment_id.to_string())
        .or_insert_with(|| provider)
        .clone())
}

pub struct DeploymentLoop {
    config: Arc<ManagerConfig>,
    deployment_store: Arc<dyn DeploymentStore>,
    release_store: Arc<dyn ReleaseStore>,
    credential_resolver: Arc<dyn CredentialResolver>,
    server_bindings: Arc<ServerBindings>,
    /// Signal dev mode watchers when deployment state changes.
    dev_status_tx: Option<watch::Sender<()>>,
    /// Keep local providers alive across ticks so runtime managers retain in-memory state.
    local_bindings_cache: Mutex<HashMap<String, Arc<LocalBindingsProvider>>>,
    /// The settled release's stack, per deployment. An update re-reads it on
    /// every tick until it finishes, and a release never changes once created,
    /// so hold the last one instead of refetching and re-deserializing it.
    /// One entry per deployment, replaced when it moves to another release.
    settled_stack_cache: Mutex<HashMap<String, (String, Option<Stack>)>>,
    /// Deployments this loop skipped because its credentials cannot provision
    /// their phase, keyed by ID. The loop does not acquire them again until
    /// their status changes or the recheck time passes.
    unprovisionable: Mutex<HashMap<String, UnprovisionableDeployment>>,
}

/// A deployment the loop skipped for lack of provisioning credentials.
#[derive(Debug, Clone)]
struct UnprovisionableDeployment {
    /// The status it was skipped in.
    status: String,
    /// When the loop may acquire it again in that status.
    recheck_after: chrono::DateTime<chrono::Utc>,
}

impl DeploymentLoop {
    /// The settled release's stack for a deployment, fetched once and then
    /// held. Releases are immutable, so the only reason to refetch is the
    /// deployment moving to a different one.
    async fn settled_stack(
        &self,
        subject: &crate::auth::Subject,
        deployment_id: &str,
        release_id: &str,
        platform: Platform,
    ) -> Result<Option<Stack>, AlienError> {
        if let Some((cached_release_id, stack)) = self
            .settled_stack_cache
            .lock()
            .expect("settled_stack_cache poisoned")
            .get(deployment_id)
        {
            if cached_release_id == release_id {
                return Ok(stack.clone());
            }
        }

        let stack = self
            .release_store
            .get_release(subject, release_id)
            .await?
            .and_then(|release| release.stacks.get(&platform).cloned());
        self.settled_stack_cache
            .lock()
            .expect("settled_stack_cache poisoned")
            .insert(
                deployment_id.to_string(),
                (release_id.to_string(), stack.clone()),
            );
        Ok(stack)
    }

    pub fn new(
        config: Arc<ManagerConfig>,
        deployment_store: Arc<dyn DeploymentStore>,
        release_store: Arc<dyn ReleaseStore>,
        credential_resolver: Arc<dyn CredentialResolver>,
        server_bindings: Arc<ServerBindings>,
        dev_status_tx: Option<watch::Sender<()>>,
    ) -> Self {
        Self {
            config,
            deployment_store,
            release_store,
            credential_resolver,
            server_bindings,
            dev_status_tx,
            local_bindings_cache: Mutex::new(HashMap::new()),
            settled_stack_cache: Mutex::new(HashMap::new()),
            unprovisionable: Mutex::new(HashMap::new()),
        }
    }

    /// Restrict `filter` to current candidates, minus deployments this loop
    /// skipped for lack of provisioning credentials and that are still in the
    /// status they were skipped in. Forgets entries that are due for a recheck,
    /// changed status, or are no longer candidates. Returns `false` when no
    /// candidate is left.
    async fn exclude_unprovisionable(
        &self,
        caller: &Subject,
        filter: &mut DeploymentFilter,
    ) -> Result<bool, AlienError> {
        let now = chrono::Utc::now();
        let mut parked = self
            .unprovisionable
            .lock()
            .expect("unprovisionable poisoned")
            .clone();
        parked.retain(|_, entry| entry.recheck_after > now);
        if parked.is_empty() {
            self.unprovisionable
                .lock()
                .expect("unprovisionable poisoned")
                .clear();
            return Ok(true);
        }

        let candidates = self
            .deployment_store
            .list_deployments(caller, filter)
            .await?;
        let mut still_parked = HashMap::new();
        let mut ids = Vec::with_capacity(candidates.len());
        for candidate in candidates {
            match parked.remove(&candidate.id) {
                Some(entry) if entry.status == candidate.status => {
                    still_parked.insert(candidate.id, entry);
                }
                _ => ids.push(candidate.id),
            }
        }
        *self
            .unprovisionable
            .lock()
            .expect("unprovisionable poisoned") = still_parked;
        let has_candidates = !ids.is_empty();
        filter.deployment_ids = Some(ids);
        Ok(has_candidates)
    }

    /// Run the deployment loop forever.
    pub async fn run(&self) {
        let (_sender, shutdown) = tokio::sync::watch::channel(false);
        self.run_until_shutdown(shutdown).await;
    }

    pub(crate) async fn run_until_shutdown(
        &self,
        mut shutdown: tokio::sync::watch::Receiver<bool>,
    ) {
        info!(
            interval_secs = self.config.deployment_interval_secs,
            "Starting deployment loop"
        );

        while !*shutdown.borrow() {
            if let Err(payload) = AssertUnwindSafe(self.tick()).catch_unwind().await {
                error!(
                    panic = panic_payload_message(payload.as_ref()),
                    "Deployment loop tick panicked"
                );
            }
            tokio::select! {
                _ = shutdown.changed() => break,
                _ = tokio::time::sleep(Duration::from_secs(self.config.deployment_interval_secs)) => {}
            }
        }
    }

    /// Drains the native runtimes of local deployments loaded by this process.
    ///
    /// When `stop_local_containers_on_shutdown` is set (`alien dev`), it also stops the
    /// Docker containers of local push deployments, including saved deployments this
    /// process has not reconciled yet. Every stop is attempted before an error returns.
    pub(crate) async fn shutdown_local_runtimes(&self) -> crate::error::Result<()> {
        let providers: Vec<(String, Arc<LocalBindingsProvider>)> = {
            let mut cache = self
                .local_bindings_cache
                .lock()
                .expect("local_bindings_cache poisoned");
            cache.drain().collect()
        };
        futures::future::join_all(
            providers
                .iter()
                .map(|(_, provider)| provider.clone().shutdown()),
        )
        .await;

        if !self.config.stop_local_containers_on_shutdown {
            return Ok(());
        }
        let loaded_results = futures::future::join_all(providers.iter().map(
            |(deployment_id, provider)| async move {
                provider
                    .stop_containers()
                    .await
                    .context(crate::error::ErrorData::InternalError {
                        message: format!(
                            "Failed to stop local containers of deployment '{deployment_id}'"
                        ),
                    })
            },
        ))
        .await;
        let saved_result = self
            .stop_saved_local_containers(
                &providers
                    .iter()
                    .map(|(deployment_id, _)| deployment_id.as_str())
                    .collect::<Vec<_>>(),
            )
            .await;
        for result in loaded_results {
            result?;
        }
        saved_result
    }

    /// Stops containers recorded by saved local push deployments that have no
    /// provider in this process, e.g. when a resumed session stops before its first
    /// reconciliation.
    async fn stop_saved_local_containers(&self, loaded: &[&str]) -> crate::error::Result<()> {
        let Some(state_dir) = &self.config.state_dir else {
            return Ok(());
        };
        let deployments = self
            .deployment_store
            .list_deployments(
                &Subject::system(),
                &DeploymentFilter {
                    platforms: Some(vec![Platform::Local]),
                    deployment_model: Some(DeploymentModel::Push),
                    ..Default::default()
                },
            )
            .await
            .context(crate::error::ErrorData::InternalError {
                message: "Failed to load saved local workloads for shutdown".to_string(),
            })?;
        let results = futures::future::join_all(
            deployments
                .iter()
                .filter(|deployment| !loaded.contains(&deployment.id.as_str()))
                .map(|deployment| (deployment.id.as_str(), state_dir.join(&deployment.id)))
                .filter(|(_, directory)| directory.join("containers").is_dir())
                .map(|(deployment_id, directory)| async move {
                    let manager = LocalContainerManager::new(directory).context(
                        crate::error::ErrorData::InternalError {
                            message: format!(
                                "Failed to load saved containers of deployment '{deployment_id}'"
                            ),
                        },
                    )?;
                    manager
                        .shutdown_all()
                        .await
                        .context(crate::error::ErrorData::InternalError {
                            message: format!(
                                "Failed to stop saved containers of deployment '{deployment_id}'"
                            ),
                        })
                }),
        )
        .await;
        for result in results {
            result?;
        }
        Ok(())
    }

    /// One iteration of the deployment loop.
    async fn tick(&self) {
        let mut filter = DeploymentFilter {
            statuses: Some(manager_candidate_statuses()),
            platforms: if self.config.targets.is_empty() {
                None
            } else {
                Some(self.config.targets.clone())
            },
            deployment_model: Some(DeploymentModel::Push),
            ..Default::default()
        };

        // Internal loop: no inbound caller. `Subject::system()` carries an
        // empty `bearer_token` — the documented signal to embedders that
        // no caller passthrough is available.
        let caller = Subject::system();

        match self.exclude_unprovisionable(&caller, &mut filter).await {
            Ok(true) => {}
            Ok(false) => return,
            Err(error) => {
                error!(%error, "Failed to list deployment candidates");
                return;
            }
        }

        let skipped_ids = Mutex::new(Vec::new());
        for batch_index in 0..MAX_ACQUIRE_BATCHES_PER_TICK {
            let session = uuid::Uuid::new_v4().to_string();

            match self
                .deployment_store
                .acquire(
                    &caller,
                    &session,
                    &filter,
                    MAX_CONCURRENT_DEPLOYMENTS as u32,
                )
                .await
            {
                Ok(acquired) => {
                    if acquired.is_empty() {
                        break;
                    }

                    debug!(
                        count = acquired.len(),
                        session = %session,
                        batch_index,
                        "Acquired deployments"
                    );
                    stream::iter(acquired)
                        .for_each_concurrent(MAX_CONCURRENT_DEPLOYMENTS, |item| async {
                            let deployment_id = item.deployment.id.clone();
                            let status = item.deployment.status.clone();
                            let skipped = self
                                .process_deployment(
                                    item.deployment,
                                    item.execution_claim,
                                    &session,
                                    ProcessOptions::deployment_tick(),
                                )
                                .await;
                            if skipped {
                                self.unprovisionable
                                    .lock()
                                    .expect("unprovisionable poisoned")
                                    .insert(
                                        deployment_id.clone(),
                                        UnprovisionableDeployment {
                                            status,
                                            recheck_after: chrono::Utc::now()
                                                + UNPROVISIONABLE_RECHECK_INTERVAL,
                                        },
                                    );
                                skipped_ids
                                    .lock()
                                    .expect("skipped IDs poisoned")
                                    .push(deployment_id);
                            }
                        })
                        .await;
                    let skipped = skipped_ids.lock().expect("skipped IDs poisoned").clone();
                    if !skipped.is_empty() {
                        // Snapshot remaining candidates only after the first skip.
                        // ID filtering prevents re-acquisition without holding a CLI's lease
                        // across unrelated work. New arrivals can wait for the next tick.
                        if filter.deployment_ids.is_none() {
                            match self
                                .deployment_store
                                .list_deployments(&caller, &filter)
                                .await
                            {
                                Ok(candidates) => {
                                    filter.deployment_ids =
                                        Some(candidates.into_iter().map(|item| item.id).collect());
                                }
                                Err(error) => {
                                    error!(%error, "Failed to list remaining deployment candidates");
                                    break;
                                }
                            }
                        }
                        let ids = filter
                            .deployment_ids
                            .as_mut()
                            .expect("candidate IDs set above");
                        ids.retain(|id| !skipped.contains(id));
                        if ids.is_empty() {
                            break;
                        }
                    }
                }
                Err(e) => {
                    error!(error = %e, "Failed to acquire deployments");
                    break;
                }
            }
        }

        debug!(
            max_batches = MAX_ACQUIRE_BATCHES_PER_TICK,
            "Deployment loop tick yielded"
        );
    }

    pub(crate) async fn process_heartbeat_deployment(
        &self,
        deployment: DeploymentRecord,
        execution_claim: Option<crate::traits::deployment_store::ExecutionClaim>,
        session: &str,
    ) {
        self.process_deployment(
            deployment,
            execution_claim,
            session,
            ProcessOptions::heartbeat_tick(),
        )
        .await;
    }

    /// Process a single deployment: step until stable, reconcile, release.
    ///
    /// Returns `true` when it was skipped because the resolved credentials
    /// cannot provision its phase.
    async fn process_deployment(
        &self,
        deployment: DeploymentRecord,
        execution_claim: Option<crate::traits::deployment_store::ExecutionClaim>,
        session: &str,
        options: ProcessOptions,
    ) -> bool {
        let deployment_id = deployment.id.clone();

        // Always release the lock when we are done, even on error.
        let result = self
            .process_deployment_inner(deployment, execution_claim.clone(), session, options)
            .await;

        if let Err(e) = &result {
            error!(
                deployment_id = %deployment_id,
                error = %e,
                "Error processing deployment"
            );
        }

        // Release lock unconditionally. Loop context, no inbound caller.
        let caller = Subject::system();
        if let Err(e) = self
            .deployment_store
            .release(&caller, &deployment_id, session, execution_claim.clone())
            .await
        {
            error!(
                deployment_id = %deployment_id,
                error = %e,
                "Failed to release deployment lock"
            );
        }
        matches!(result, Ok(true))
    }

    async fn process_deployment_inner(
        &self,
        deployment: DeploymentRecord,
        execution_claim: Option<crate::traits::deployment_store::ExecutionClaim>,
        session: &str,
        options: ProcessOptions,
    ) -> Result<bool, AlienError> {
        let deployment_id = deployment.id.clone();
        let stack_settings = deployment
            .stack_settings
            .as_ref()
            .expect("stored deployment carries stack_settings");

        // Pull-mode deployments are entirely driven by the alien-operator running in the
        // target environment. The manager must not attempt to provision or deploy them.
        if stack_settings.deployment_model == alien_core::DeploymentModel::Pull {
            debug!(
                deployment_id = %deployment_id,
                "Skipping pull-mode deployment — handled by alien-operator"
            );
            return Ok(false);
        }

        let status = parse_status(&deployment.status);

        if options.require_heartbeats_enabled && !stack_settings.heartbeats.is_enabled() {
            debug!(
                deployment_id = %deployment_id,
                "Skipping heartbeat because heartbeats are disabled for this deployment"
            );
            return Ok(false);
        }

        // 1. Get the release for this deployment.
        let prepared_startup_release = if deployment.platform == alien_core::Platform::Local
            && matches!(
                status,
                DeploymentStatus::InitialSetup | DeploymentStatus::Provisioning
            ) {
            deployment
                .runtime_metadata
                .as_ref()
                .filter(|metadata| metadata.pending_prepared_stack.is_none())
                .and_then(|metadata| metadata.pending_prepared_release_id.as_ref())
        } else {
            None
        };
        let target_release_id = prepared_startup_release
            .or(deployment.desired_release_id.as_ref())
            .or(deployment.current_release_id.as_ref())
            .ok_or_else(|| {
                AlienError::new(GenericError {
                    message: format!(
                        "Deployment {} has neither desired_release_id nor current_release_id",
                        deployment_id
                    ),
                })
            })?;

        let system = crate::auth::Subject::system();
        let (_, supplied_current) = supplied_release_stacks(&deployment);
        let deployment_stack =
            target_release_stack(self.release_store.as_ref(), &deployment, target_release_id)
                .await?;

        let recorded_state = state_from_record(
            &deployment,
            status,
            deployment_stack.as_ref(),
            target_release_id,
        );
        if let Some(next_status) = alien_deployment::destroy_without_runtime(&recorded_state) {
            info!(
                deployment_id = %deployment_id,
                next_status = ?next_status,
                "Runtime never started; skipping runtime cleanup"
            );
            let state_after = DeploymentState {
                status: next_status,
                error: None,
                retry_requested: false,
                ..recorded_state
            };
            // AWS revokes before the checkpoint, which hands the deployment to setup teardown.
            let state_after = crate::registry_access::revoke_before_persisting(
                self.deployment_store.as_ref(),
                &self.server_bindings.bindings_provider,
                &self.server_bindings.target_bindings_providers,
                &deployment_id,
                &deployment.project_id,
                state_after,
            )
            .await
            .map_err(|error| error.into_generic())?;
            self.checkpoint_without_step(
                &deployment_id,
                session,
                state_after.clone(),
                execution_claim,
            )
            .await?;
            crate::registry_access::revoke_after_persisting(
                self.deployment_store.as_ref(),
                &self.server_bindings.bindings_provider,
                &self.server_bindings.target_bindings_providers,
                &deployment_id,
                &deployment.project_id,
                &state_after,
            )
            .await
            .map_err(|error| error.into_generic())?;
            if state_after.status == DeploymentStatus::Deleted {
                self.deployment_store
                    .delete_deployment(&system, &deployment_id)
                    .await?;
            }
            return Ok(false);
        }

        // 2. Resolve credentials for the target platform and lifecycle phase.
        let resolved_credentials = match self
            .credential_resolver
            .resolve_with_capability(&deployment)
            .await
        {
            Ok(resolved) => resolved,
            Err(e) => {
                let handoff_retry_remaining = gcp_credential_handoff_retry_remaining(
                    status,
                    &deployment,
                    &e,
                    chrono::Utc::now(),
                );
                if should_wait_for_credential_handoff(status, &deployment)
                    || handoff_retry_remaining.is_some()
                {
                    warn!(
                        deployment_id = %deployment_id,
                        status = ?status,
                        platform = ?deployment.platform,
                        retry_window_remaining_secs = ?handoff_retry_remaining.map(|duration| duration.as_secs()),
                        error = %e,
                        "Credentials unavailable for deployment phase; waiting for another driver or credential handoff"
                    );
                } else {
                    let credential_error = e.into_generic();
                    let failed_state = failed_state_for_credential_error(
                        &deployment,
                        status,
                        deployment_stack.as_ref(),
                        target_release_id,
                        credential_error,
                    );
                    warn!(
                        deployment_id = %deployment_id,
                        status = ?status,
                        failed_status = ?failed_state.status,
                        platform = ?deployment.platform,
                        "Credential resolution failed for manager-owned phase; checkpointing failed deployment state"
                    );
                    self.checkpoint_without_step(
                        &deployment_id,
                        session,
                        failed_state,
                        execution_claim.clone(),
                    )
                    .await?;
                }
                return Ok(false);
            }
        };

        if needs_provision_capability(status) && !resolved_credentials.has_provision_capability {
            debug!(
                deployment_id = %deployment_id,
                status = ?status,
                platform = ?deployment.platform,
                "Skipping bootstrap phase because resolved credentials cannot provision"
            );
            // Acquisition consumes the persisted retry. If we cannot run setup,
            // return it while we still own the lease so a setup-capable driver
            // can acquire the same failed deployment after we release it.
            if deployment.retry_requested {
                self.checkpoint_without_step(
                    &deployment_id,
                    session,
                    DeploymentState {
                        error: deployment_record_error(&deployment.error),
                        ..recorded_state
                    },
                    execution_claim,
                )
                .await?;
            }
            return Ok(true);
        }
        let client_config = resolved_credentials.client_config;

        // 3. Extract the stack for this deployment's platform from the release.
        let deployment_stack = deployment_stack.ok_or_else(|| {
            AlienError::new(GenericError {
                message: format!(
                    "Release {} does not contain a stack for platform {}",
                    target_release_id, deployment.platform
                ),
            })
        })?;

        // 4. Build deployment state from the record.
        let target_release = ReleaseInfo {
            release_id: Some(target_release_id.clone()),
            version: None,
            description: None,
            stack: deployment_stack.clone(),
        };

        // `current_release` is the release currently deployed, so its stack has
        // to come from that release. During an update it differs from the
        // target, and carrying the target's stack here would make anything
        // that asks "what is deployed now?" compare the target against itself.
        // The pull path hydrates it the same way.
        // A supplied current stack can differ from the target even for the
        // same release, when only the control plane's additions change.
        let current_release = match (deployment.current_release_id.as_ref(), supplied_current) {
            (None, _) => None,
            (Some(id), Some(stack)) => Some(ReleaseInfo {
                release_id: Some(id.clone()),
                version: None,
                description: None,
                stack,
            }),
            (Some(id), None) if id == target_release_id => Some(ReleaseInfo {
                release_id: Some(id.clone()),
                version: None,
                description: None,
                stack: deployment_stack.clone(),
            }),
            (Some(id), None) => self
                .settled_stack(&system, &deployment_id, id, deployment.platform)
                .await?
                .map(|stack| ReleaseInfo {
                    release_id: Some(id.clone()),
                    version: None,
                    description: None,
                    stack,
                }),
        };

        let mut state = DeploymentState {
            status: parse_status(&deployment.status),
            platform: deployment.platform.clone(),
            current_release,
            target_release: Some(target_release),
            stack_state: deployment.stack_state.clone(),
            error: deployment_record_error(&deployment.error),
            environment_info: deployment.environment_info.clone(),
            runtime_metadata: deployment.runtime_metadata.clone(),
            retry_requested: deployment.retry_requested,
            protocol_version: deployment.deployment_protocol_version,
        };

        // 4. Build the deployment config. External runners that acquire this
        //    deployment get the same config from the acquire route.
        let mut config = RunnerConfigSources {
            config: &self.config,
            credential_resolver: self.credential_resolver.as_ref(),
            bindings_provider: &self.server_bindings.bindings_provider,
            target_bindings_providers: &self.server_bindings.target_bindings_providers,
        }
        .deployment_config(&deployment, Some(&deployment_stack))
        .await?;

        // Standalone-mode bridge: inject a BYO Horizon backend from env vars
        // when an external control plane did not supply one. It stays out of
        // the shared config: its management token is this manager's own
        // secret, not something to hand to every runner that acquires.
        if config.compute_backend.is_none() {
            if let (Ok(url), Ok(cluster_id), Ok(token)) = (
                std::env::var("ALIEN_BYO_HORIZON_URL"),
                std::env::var("ALIEN_BYO_HORIZON_CLUSTER_ID"),
                std::env::var("ALIEN_BYO_HORIZON_MANAGEMENT_TOKEN"),
            ) {
                if !url.is_empty() && !cluster_id.is_empty() && !token.is_empty() {
                    let mut clusters = std::collections::HashMap::new();
                    clusters.insert(
                        cluster_id.clone(),
                        alien_core::HorizonClusterConfig {
                            cluster_id,
                            management_token: token,
                        },
                    );
                    config.compute_backend = Some(alien_core::ComputeBackend::Horizon(
                        alien_core::HorizonConfig {
                            url,
                            horizon_machine_image: synthesize_byo_horizon_machine_image(),
                            clusters,
                        },
                    ));
                }
            }
        }

        // 6. Build service provider.
        // Use LocalBindingsProvider for local platform, default provider for cloud platforms.
        let service_provider: Arc<dyn alien_infra::PlatformServiceProvider> = match deployment
            .platform
        {
            alien_core::Platform::Local => {
                let state_dir = self.config.state_dir.as_ref().ok_or_else(|| {
                    AlienError::new(GenericError {
                        message: "state_dir is required for Local platform deployments but was not configured".to_string(),
                    })
                })?;
                let local_bindings = get_or_create_local_bindings_provider(
                    &self.local_bindings_cache,
                    state_dir,
                    &deployment_id,
                )?;
                Arc::new(DefaultPlatformServiceProvider::with_local_bindings(
                    local_bindings,
                ))
            }
            _ => Arc::new(DefaultPlatformServiceProvider::default()),
        };

        // 7. Step loop — use the shared runner with ManagerTransport for
        //    per-step reconciliation (state persistence + registry access).
        let operation = if matches!(
            state.status,
            DeploymentStatus::DeletePending
                | DeploymentStatus::Deleting
                | DeploymentStatus::DeleteFailed
        ) {
            LoopOperation::Delete
        } else {
            LoopOperation::Deploy
        };

        let policy = RunnerPolicy {
            max_steps: options.max_steps,
            operation,
            delay_strategy: alien_deployment::runner::DelayStrategy::Yield,
        };

        let transport = ManagerTransport::new(
            self.deployment_store.clone(),
            self.server_bindings.bindings_provider.clone(),
            self.server_bindings.target_bindings_providers.clone(),
            deployment.project_id.clone(),
            session.to_string(),
            execution_claim,
        );

        let mut config = config;
        let runner_result = if options.require_heartbeats_enabled {
            alien_deployment::runner::run_running_refresh_step_loop(
                &mut state,
                &mut config,
                &client_config,
                &deployment_id,
                &policy,
                &transport,
                Some(service_provider),
                None,
            )
            .await
        } else {
            alien_deployment::runner::run_step_loop(
                &mut state,
                &mut config,
                &client_config,
                &deployment_id,
                &policy,
                &transport,
                Some(service_provider),
                None,
            )
            .await
        };

        match &runner_result {
            Ok(RunnerResult {
                loop_result,
                steps_executed,
                ..
            }) => {
                info!(
                    deployment_id = %deployment_id,
                    status = ?loop_result.final_status,
                    stop_reason = ?loop_result.stop_reason,
                    outcome = ?loop_result.outcome,
                    steps = steps_executed,
                    "Deployment step loop completed"
                );
            }
            Err(e) => {
                error!(
                    deployment_id = %deployment_id,
                    error = %e,
                    "Deployment step loop failed"
                );
            }
        }

        // 8. Handle deleted deployments — clean up the record. Loop
        // context — no inbound caller; pass `Subject::system()`.
        if state.status == DeploymentStatus::Deleted {
            info!(deployment_id = %deployment_id, "Deployment deleted, removing record");
            let caller = Subject::system();
            if let Err(e) = self
                .deployment_store
                .delete_deployment(&caller, &deployment_id)
                .await
            {
                error!(
                    deployment_id = %deployment_id,
                    error = %e,
                    "Failed to delete deployment record"
                );
            }
        }

        // 9. Notify dev-mode watchers of state change.
        if let Some(ref tx) = self.dev_status_tx {
            let _ = tx.send(());
        }

        // Propagate step loop errors to the caller (which logs and releases the lock).
        runner_result.map(|_| false).map_err(|e| e.into_generic())
    }

    async fn checkpoint_without_step(
        &self,
        deployment_id: &str,
        session: &str,
        state: DeploymentState,
        execution_claim: Option<crate::traits::deployment_store::ExecutionClaim>,
    ) -> Result<(), AlienError> {
        self.deployment_store
            .reconcile(
                &Subject::system(),
                ReconcileData {
                    deployment_id: deployment_id.to_string(),
                    session: session.to_string(),
                    state,
                    update_heartbeat: false,
                    suggested_delay_ms: None,
                    heartbeats: Vec::new(),
                    observed_inventory_batches: Vec::new(),
                    capabilities: Vec::new(),
                    operator_version: None,
                    execution_claim,
                    operations_report: None,
                },
            )
            .await?;
        Ok(())
    }
}

/// The stack a runner deploys for the deployment's target release: the one a
/// control plane supplied, else the release's stack for the deployment's
/// platform. `None` when the release has no stack for that platform.
pub(crate) async fn target_release_stack(
    release_store: &dyn ReleaseStore,
    deployment: &DeploymentRecord,
    target_release_id: &str,
) -> Result<Option<Stack>, AlienError> {
    // A control plane that supplies stacks may have added resources to the
    // release's stack for this deployment; deploy what it supplied.
    if let (Some(stack), _) = supplied_release_stacks(deployment) {
        return Ok(Some(stack));
    }
    let release = release_store
        .get_release(&Subject::system(), target_release_id)
        .await?
        .ok_or_else(|| {
            AlienError::new(GenericError {
                message: format!("Release {} not found", target_release_id),
            })
        })?;
    Ok(release.stacks.get(&deployment.platform).cloned())
}

/// What the deployment config is built from, besides the deployment itself.
pub(crate) struct RunnerConfigSources<'a> {
    pub config: &'a ManagerConfig,
    pub credential_resolver: &'a dyn CredentialResolver,
    pub bindings_provider: &'a Option<Arc<dyn BindingsProviderApi>>,
    pub target_bindings_providers: &'a HashMap<Platform, Arc<dyn BindingsProviderApi>>,
}

impl RunnerConfigSources<'_> {
    /// The deployment config a runner applies to this deployment.
    ///
    /// The manager's loop runs with it, and the acquire route returns it to
    /// any other runner (such as `alien deploy`). Both must apply the same
    /// config: when they differ, the second runner to apply rolls every
    /// workload again with a different spec.
    ///
    /// `deployment_stack` is the target release's stack; `None` when the
    /// deployment has none yet, so no resource receives command variables.
    pub(crate) async fn deployment_config(
        &self,
        deployment: &DeploymentRecord,
        deployment_stack: Option<&Stack>,
    ) -> Result<DeploymentConfig, AlienError> {
        let environment_variables =
            deployment_environment_variables(self.config, deployment, deployment_stack)?;
        let monitoring = deployment
            .deployment_config
            .as_ref()
            .and_then(|config| config.monitoring.clone())
            .or_else(|| build_monitoring_config(self.config, deployment));

        // Management config resolution:
        //   1. From platform API (per-deployment, on DeploymentRecord — e.g. private managers)
        //   2. From credential resolver (per-platform, derived from target provider's management binding)
        let management_config = if let Some(mc) = deployment.management_config.clone() {
            Some(mc)
        } else {
            match self
                .credential_resolver
                .resolve_management_config(deployment.platform)
                .await
            {
                Ok(mc) => mc,
                Err(e) => {
                    warn!(
                        deployment_id = %deployment.id,
                        error = %e,
                        "Failed to resolve management config from bindings"
                    );
                    None
                }
            }
        };

        let native_image_host = crate::registry_access::derive_native_image_host(
            self.bindings_provider,
            self.target_bindings_providers,
            &deployment.platform,
        )
        .await;

        let config = if let Some(mut config) = deployment.deployment_config.clone() {
            config.environment_variables = environment_variables;
            if config.deployment_name.is_none() {
                config.deployment_name = Some(deployment.name.clone());
            }
            if config.management_config.is_none() {
                config.management_config = management_config;
            }
            if config.deployment_token.is_none() {
                config.deployment_token = deployment.deployment_token.clone();
            }
            if config.base_platform.is_none() {
                config.base_platform = deployment.base_platform;
            }
            if config.monitoring.is_none() {
                config.monitoring = monitoring;
            }
            // A control plane that predates gate answers omits inputValues
            // entirely; resolving gates from declared defaults instead of the
            // stored answers would deprovision an enabled resource. Whenever
            // gates exist their answers are materialized at install, so a
            // legitimately supplied map is never empty.
            if config.input_values.is_empty() {
                config.input_values = deployment.input_values.clone();
            }
            config.manager_url = Some(self.config.base_url());
            config.native_image_host = native_image_host;
            config
        } else {
            let stack_settings = deployment.stack_settings.clone().ok_or_else(|| {
                AlienError::new(GenericError {
                    message: format!("Deployment {} has no stack settings", deployment.id),
                })
            })?;

            DeploymentConfig {
                stored_secret_input_ids: None,
                input_values: deployment.input_values.clone(),
                deployment_name: Some(deployment.name.clone()),
                external_bindings: stack_settings.external_bindings.clone().unwrap_or_default(),
                stack_settings,
                management_config,
                environment_variables,
                allow_frozen_changes: false,
                setup_run: false,
                compute_backend: None,
                base_platform: deployment.base_platform,
                label_domain: None,
                observe_label_selector: None,
                observe_all_namespaces: false,
                public_endpoints: None,
                domain_metadata: None,
                monitoring,
                manager_url: Some(self.config.base_url()),
                deployment_token: deployment.deployment_token.clone(),
                native_image_host,
                volume_restores: Vec::new(),
            }
        };
        Ok(config)
    }
}

/// The environment variables snapshot injected into the deployment's
/// workloads: `ALIEN_DEPLOYMENT_ID`, `ALIEN_DEPLOYMENT_NAME`, command delivery
/// configuration, and the deployment's own variables.
fn deployment_environment_variables(
    config: &ManagerConfig,
    deployment: &DeploymentRecord,
    deployment_stack: Option<&Stack>,
) -> Result<EnvironmentVariablesSnapshot, AlienError> {
    let mut vars: Vec<EnvironmentVariable> = vec![
        EnvironmentVariable {
            name: ENV_ALIEN_DEPLOYMENT_ID.to_string(),
            value: deployment.id.clone(),
            var_type: EnvironmentVariableType::Plain,
            target_resources: None,
        },
        EnvironmentVariable {
            name: ENV_ALIEN_DEPLOYMENT_NAME.to_string(),
            value: deployment.name.clone(),
            var_type: EnvironmentVariableType::Plain,
            target_resources: None,
        },
    ];

    if let Some(deployment_stack) = deployment_stack {
        // Local/Kubernetes Worker push authentication. Cloud-native Worker
        // transports authenticate through their platform instead.
        if matches!(deployment.platform, Platform::Kubernetes | Platform::Local) {
            vars.extend(
                worker_commands_push_env_vars(
                    deployment.deployment_token.as_deref(),
                    deployment_stack,
                )
                .map_err(|e| e.into_generic())?,
            );
        }

        // Container/Daemon command receiver env — injected on EVERY platform.
        // Unlike workers, Containers and Daemons always deliver commands via
        // Pull, regardless of platform, so the receiver contract must reach
        // them everywhere, not just K8s/Local.
        // The Worker-only gate above is deliberately NOT applied here: its
        // justification (cloud *workers* use platform-native push) does not hold
        // for Container/Daemon receivers. The snapshot reaches Container/Daemon env
        // on any platform through the platform-independent per-resource dispatch in
        // `alien-deployment::inject_into_environment` (Container/Daemon downcast).
        // Empty when the stack declares no command-enabled Container/Daemon targets.
        vars.extend(
            commands_receiver_env_vars(
                config.commands_base_url(),
                deployment.deployment_token.as_deref(),
                deployment_stack,
            )
            .map_err(|e| e.into_generic())?,
        );
    }

    if let Some(ref user_vars) = deployment.user_environment_variables {
        vars.extend(user_vars.iter().cloned());
    }

    // Build deterministic hash from variable contents so the infra executor
    // only sees a change when the actual values change.
    let mut hasher = Sha256::new();
    for v in &vars {
        hasher.update(v.name.as_bytes());
        hasher.update(b"=");
        hasher.update(v.value.as_bytes());
        hasher.update(b"\n");
    }
    let hash = format!("env-{:x}", hasher.finalize());

    Ok(EnvironmentVariablesSnapshot {
        variables: vars,
        hash,
        created_at: chrono::Utc::now().to_rfc3339(),
    })
}

/// Where a deployment without a monitoring config sends its logs: back to this manager, unless
/// the deployment turned telemetry off. The platform leaves `monitoring` empty for `off`, so
/// filling it here must honor the same setting.
fn build_monitoring_config(
    config: &ManagerConfig,
    deployment: &DeploymentRecord,
) -> Option<alien_core::OtlpConfig> {
    let telemetry_off = deployment
        .stack_settings
        .as_ref()
        .is_some_and(|settings| settings.telemetry == alien_core::TelemetryMode::Off);
    let otlp_enabled = config.otlp_endpoint.is_some() || config.enable_local_log_ingest();
    let token = deployment.deployment_token.as_ref()?;

    (otlp_enabled && !telemetry_off).then(|| alien_core::OtlpConfig {
        logs_endpoint: format!("{}/v1/logs", config.base_url()),
        logs_auth_header: format!("authorization=Bearer {}", token),
        metrics_endpoint: None,
        metrics_auth_header: None,
        resource_attributes: std::collections::HashMap::new(),
    })
}

/// Builds the scoped token that authenticates Local/Kubernetes command pushes
/// into each command-enabled Worker runtime. Container/Daemon receiver env is
/// wired separately below.
fn worker_commands_push_env_vars(
    deployment_token: Option<&str>,
    deployment_stack: &alien_core::Stack,
) -> Result<Vec<EnvironmentVariable>, alien_error::AlienError<alien_core::ErrorData>> {
    deployment_stack.worker_command_push_env_vars(deployment_token)
}

/// Builds the command *receiver* environment variables (`ALIEN_COMMANDS_URL`,
/// `ALIEN_COMMANDS_TOKEN`, `ALIEN_COMMANDS_TARGET_RESOURCE_ID`,
/// `ALIEN_COMMANDS_TARGET_RESOURCE_TYPE`), each scoped via `target_resources` to a
/// single command-enabled Container or Daemon. Unlike Worker push auth, this is
/// injected on every platform because Container/Daemon always deliver via Pull.
/// `ALIEN_DEPLOYMENT_ID` is not re-emitted here — it is already
/// injected deployment-wide above. The commands token comes from the deployment
/// record — the manager is the sole injector of `ALIEN_COMMANDS_TOKEN`.
fn commands_receiver_env_vars(
    commands_base_url: String,
    deployment_token: Option<&str>,
    deployment_stack: &alien_core::Stack,
) -> Result<Vec<EnvironmentVariable>, alien_error::AlienError<alien_core::ErrorData>> {
    deployment_stack.receiver_command_env_vars(&commands_base_url, deployment_token)
}

fn should_wait_for_credential_handoff(
    status: DeploymentStatus,
    deployment: &DeploymentRecord,
) -> bool {
    match status {
        DeploymentStatus::Pending => true,
        DeploymentStatus::InitialSetup => {
            deployment.stack_state.as_ref().map_or(true, |stack_state| {
                !has_remote_stack_management_outputs(stack_state)
            })
        }
        _ => false,
    }
}

fn has_remote_stack_management_outputs(stack_state: &alien_core::StackState) -> bool {
    stack_state.resources.values().any(|resource_state| {
        resource_state.resource_type == "remote-stack-management"
            && resource_state.outputs.as_ref().is_some()
    })
}

fn gcp_credential_handoff_retry_remaining(
    status: DeploymentStatus,
    deployment: &DeploymentRecord,
    error: &AlienError,
    now: chrono::DateTime<chrono::Utc>,
) -> Option<Duration> {
    if status != DeploymentStatus::Provisioning
        || deployment.platform != alien_core::Platform::Gcp
        || error.code != REMOTE_CREDENTIAL_HANDOFF_FAILED_CODE
        || !deployment
            .stack_state
            .as_ref()
            .is_some_and(has_remote_stack_management_outputs)
    {
        return None;
    }

    // Provisioning is persisted by the setup-to-runtime transition, so
    // updated_at is the durable start of this handoff window. created_at keeps
    // older/imported records bounded if they lack an update timestamp.
    let handoff_started_at = deployment.updated_at.unwrap_or(deployment.created_at);
    let elapsed = now
        .signed_duration_since(handoff_started_at)
        .to_std()
        .unwrap_or(Duration::ZERO);

    GCP_CREDENTIAL_HANDOFF_GRACE_PERIOD
        .checked_sub(elapsed)
        .filter(|remaining| !remaining.is_zero())
}

fn failed_state_for_credential_error(
    deployment: &DeploymentRecord,
    status: DeploymentStatus,
    deployment_stack: Option<&alien_core::Stack>,
    target_release_id: &str,
    error: AlienError,
) -> DeploymentState {
    DeploymentState {
        status: failed_status_for_deployment_error(status),
        error: Some(error),
        retry_requested: false,
        ..state_from_record(deployment, status, deployment_stack, target_release_id)
    }
}

fn state_from_record(
    deployment: &DeploymentRecord,
    status: DeploymentStatus,
    deployment_stack: Option<&alien_core::Stack>,
    target_release_id: &str,
) -> DeploymentState {
    DeploymentState {
        status,
        platform: deployment.platform,
        current_release: deployment_stack.and_then(|stack| {
            deployment
                .current_release_id
                .as_ref()
                .map(|id| ReleaseInfo {
                    release_id: Some(id.clone()),
                    version: None,
                    description: None,
                    stack: stack.clone(),
                })
        }),
        target_release: deployment_stack.map(|stack| ReleaseInfo {
            release_id: Some(target_release_id.to_string()),
            version: None,
            description: None,
            stack: stack.clone(),
        }),
        stack_state: deployment.stack_state.clone(),
        error: None,
        environment_info: deployment.environment_info.clone(),
        runtime_metadata: deployment.runtime_metadata.clone(),
        retry_requested: deployment.retry_requested,
        protocol_version: deployment.deployment_protocol_version,
    }
}

// Test ownership: Manager-specific behavior tests (status parsing, skip logic,
// active statuses, local bindings caching). Loop contract correctness is tested
// in alien-deployment::loop_contract — not duplicated here.

/// The stacks a control plane supplied for this step: the target (the
/// desired release, or the current one when nothing is desired) and the
/// installed current stack. `None` means read the release instead.
fn supplied_release_stacks(deployment: &DeploymentRecord) -> (Option<Stack>, Option<Stack>) {
    let Some(stacks) = deployment.supplied_stacks.as_ref() else {
        return (None, None);
    };
    let target = if deployment.desired_release_id.is_some() {
        stacks.desired.clone()
    } else {
        stacks.current.clone()
    };
    (target, stacks.current.clone())
}

#[cfg(test)]
mod tests {
    use super::{
        active_work_statuses, build_monitoring_config, commands_receiver_env_vars,
        gcp_credential_handoff_retry_remaining, get_or_create_local_bindings_provider,
        has_remote_stack_management_outputs, manager_candidate_statuses,
        needs_provision_capability, parse_status, retryable_failed_statuses,
        should_wait_for_credential_handoff, supplied_release_stacks, worker_commands_push_env_vars,
        DeploymentLoop, ProcessOptions, RunnerConfigSources, GCP_CREDENTIAL_HANDOFF_GRACE_PERIOD,
    };
    use alien_core::{
        Container, ContainerCode, Daemon, DaemonCode, DeploymentConfig, DeploymentStatus,
        EnvironmentVariable, EnvironmentVariableType, EnvironmentVariablesSnapshot, Platform,
        RemoteStackManagement, RemoteStackManagementOutputs, Resource, ResourceLifecycle,
        ResourceOutputs, ResourceSpec, ResourceStatus, Stack, StackResourceState, StackSettings,
        StackState, Worker, WorkerCode, ENV_ALIEN_COMMANDS_TARGET_RESOURCE_ID,
        ENV_ALIEN_COMMANDS_TARGET_RESOURCE_TYPE, ENV_ALIEN_COMMANDS_TOKEN, ENV_ALIEN_COMMANDS_URL,
        ENV_ALIEN_DEPLOYMENT_ID, ENV_ALIEN_DEPLOYMENT_NAME,
    };
    use alien_deployment::loop_contract::{classify_status, LoopOperation};
    use alien_error::AlienError;
    use alien_local::{ContainerConfig, LocalContainerManager};
    use chrono::Utc;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use tempfile::TempDir;

    use crate::auth::Subject;
    use crate::error::ErrorData;
    #[cfg(feature = "sqlite")]
    use crate::stores::sqlite::{SqliteDatabase, SqliteDeploymentStore, SqliteReleaseStore};
    use crate::traits::deployment_store::{
        CreateDeploymentParams, DeploymentFilter, DeploymentRecord, SuppliedStacks,
    };
    use crate::traits::{
        CredentialResolver, DeploymentStore, ReleaseStore, ResolvedCredentials, ServerBindings,
    };
    use alien_bindings::providers::{kv::local::LocalKv, storage::local::LocalStorage};
    use alien_commands::{dispatchers::NullCommandDispatcher, InMemoryCommandRegistry};
    use async_trait::async_trait;

    fn deployment_record(
        status: DeploymentStatus,
        stack_state: Option<StackState>,
    ) -> DeploymentRecord {
        DeploymentRecord {
            supplied_stacks: None,
            id: "dep_test".to_string(),
            workspace_id: "default".to_string(),
            project_id: "default".to_string(),
            name: "test".to_string(),
            deployment_group_id: "dg_test".to_string(),
            platform: Platform::Aws,
            deployment_protocol_version: alien_core::DEPLOYMENT_PROTOCOL_VERSION,
            base_platform: None,
            status: deployment_status_str(status).to_string(),
            stack_settings: Some(StackSettings::default()),
            stack_state,
            environment_info: None,
            runtime_metadata: None,
            current_release_id: None,
            desired_release_id: Some("rel_test".to_string()),
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
            created_at: Utc::now(),
            updated_at: None,
            error: None,
        }
    }

    #[derive(Default)]
    struct RuntimeOnlyCredentials {
        visits: Mutex<Vec<String>>,
        fifth_visit_gate: Option<(Arc<tokio::sync::Notify>, Arc<tokio::sync::Notify>)>,
    }

    #[async_trait]
    impl CredentialResolver for RuntimeOnlyCredentials {
        async fn resolve(
            &self,
            _: &DeploymentRecord,
        ) -> Result<alien_core::ClientConfig, AlienError> {
            Ok(alien_core::ClientConfig::Local {
                state_directory: "unused".to_string(),
            })
        }

        async fn resolve_with_capability(
            &self,
            deployment: &DeploymentRecord,
        ) -> Result<ResolvedCredentials, AlienError> {
            let visit_count = {
                let mut visits = self.visits.lock().unwrap();
                visits.push(deployment.id.clone());
                visits.len()
            };
            if visit_count == 5 {
                if let Some((entered, resume)) = &self.fifth_visit_gate {
                    entered.notify_one();
                    resume.notified().await;
                }
            }
            Ok(ResolvedCredentials {
                client_config: self.resolve(deployment).await?,
                has_provision_capability: false,
            })
        }
    }

    #[cfg(feature = "sqlite")]
    #[tokio::test]
    async fn skipped_bootstrap_preserves_retry_for_cli_acquisition() {
        let temp = tempfile::tempdir().unwrap();
        let db = Arc::new(
            SqliteDatabase::new(temp.path().join("manager.db").to_str().unwrap())
                .await
                .unwrap(),
        );
        let store = Arc::new(SqliteDeploymentStore::new(db.clone()));
        let caller = Subject::system();
        let deployment = store
            .create_deployment(
                &caller,
                CreateDeploymentParams {
                    name: "retry-race".to_string(),
                    deployment_group_id: "dg_test".to_string(),
                    platform: Platform::Aws,
                    deployment_protocol_version: alien_core::DEPLOYMENT_PROTOCOL_VERSION,
                    base_platform: None,
                    stack_settings: StackSettings {
                        deployment_model: alien_core::DeploymentModel::Push,
                        ..Default::default()
                    },
                    stack_state: None,
                    environment_variables: None,
                    public_subdomain: None,
                    input_values: Default::default(),
                    setup_item: None,
                    deployment_token: None,
                },
            )
            .await
            .unwrap();
        db.execute(&format!(
            "UPDATE deployments SET status = 'preflights-failed' WHERE id = '{}'",
            deployment.id
        ))
        .await
        .unwrap();
        store
            .set_retry_requested(&caller, &deployment.id)
            .await
            .unwrap();
        let filter = DeploymentFilter {
            deployment_ids: Some(vec![deployment.id.clone()]),
            statuses: Some(vec!["preflights-failed".to_string()]),
            deployment_model: Some(alien_core::DeploymentModel::Push),
            ..Default::default()
        };
        let mut acquired = store
            .acquire(&caller, "manager-session", &filter, 1)
            .await
            .unwrap();
        assert_eq!(acquired.len(), 1);
        let mut item = acquired.remove(0);
        assert!(item.deployment.retry_requested);
        let saved_error =
            serde_json::to_value(alien_error::AlienError::new(alien_error::GenericError {
                message: "Saved quota failure".to_string(),
            }))
            .unwrap();
        item.deployment.error = Some(saved_error.clone());
        item.deployment.desired_release_id = Some("rel_test".to_string());
        item.deployment.supplied_stacks = Some(SuppliedStacks {
            desired: Some(
                serde_json::from_value(serde_json::json!({"id": "retry", "resources": {}}))
                    .unwrap(),
            ),
            current: None,
        });
        let bindings = Arc::new(ServerBindings {
            kv: Arc::new(LocalKv::new(temp.path().join("kv")).await.unwrap()),
            command_storage: Arc::new(
                LocalStorage::new(temp.path().join("storage").to_string_lossy().to_string())
                    .unwrap(),
            ),
            command_dispatcher: Arc::new(NullCommandDispatcher),
            command_registry: Arc::new(InMemoryCommandRegistry::default()),
            artifact_registry: None,
            bindings_provider: None,
            target_bindings_providers: Default::default(),
            registry_routing_table: Arc::new(
                crate::routes::registry_proxy::RegistryRoutingTable::new(vec![]).unwrap(),
            ),
        });
        let runner = DeploymentLoop::new(
            Arc::new(crate::config::ManagerConfig::default()),
            store.clone(),
            Arc::new(SqliteReleaseStore::new(db.clone())),
            Arc::new(RuntimeOnlyCredentials::default()),
            bindings.clone(),
            None,
        );
        runner
            .process_deployment(
                item.deployment,
                item.execution_claim,
                "manager-session",
                ProcessOptions::deployment_tick(),
            )
            .await;
        let stored = store
            .get_deployment(&caller, &deployment.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.status, "preflights-failed");
        assert_eq!(stored.error, Some(saved_error));
        assert!(
            stored.retry_requested,
            "skipping setup must not consume the CLI's retry"
        );
        assert!(stored.locked_by.is_none());
        assert!(stored.stack_state.is_none());
        let cli = store
            .acquire(&caller, "cli-session", &filter, 1)
            .await
            .unwrap();
        assert_eq!(
            cli.len(),
            1,
            "CLI must acquire after the incapable manager releases"
        );
        assert!(cli[0].deployment.retry_requested);
        store
            .release(
                &caller,
                &deployment.id,
                "cli-session",
                cli[0].execution_claim.clone(),
            )
            .await
            .unwrap();
        let releases = Arc::new(SqliteReleaseStore::new(db.clone()));
        let release = releases
            .create_release(
                &caller,
                crate::traits::release_store::CreateReleaseParams {
                    project_id: "default".to_string(),
                    stacks: HashMap::from([(
                        Platform::Aws,
                        serde_json::from_value(serde_json::json!({"id":"retry", "resources":{}}))
                            .unwrap(),
                    )]),
                    git_commit_sha: None,
                    git_commit_ref: None,
                    git_commit_message: None,
                },
            )
            .await
            .unwrap();
        let mut ids = vec![deployment.id.clone()];
        for index in 0..4 {
            let other = store
                .create_deployment(
                    &caller,
                    CreateDeploymentParams {
                        name: format!("retry-race-{index}"),
                        deployment_group_id: "dg_test".to_string(),
                        platform: Platform::Aws,
                        deployment_protocol_version: alien_core::DEPLOYMENT_PROTOCOL_VERSION,
                        base_platform: None,
                        stack_settings: StackSettings {
                            deployment_model: alien_core::DeploymentModel::Push,
                            ..Default::default()
                        },
                        stack_state: None,
                        environment_variables: None,
                        public_subdomain: None,
                        input_values: Default::default(),
                        setup_item: None,
                        deployment_token: None,
                    },
                )
                .await
                .unwrap();
            ids.push(other.id);
        }
        for id in &ids {
            db.execute(&format!("UPDATE deployments SET status = 'preflights-failed', desired_release_id = '{}' WHERE id = '{}'", release.id, id)).await.unwrap();
            store.set_retry_requested(&caller, id).await.unwrap();
        }
        let entered = Arc::new(tokio::sync::Notify::new());
        let resume = Arc::new(tokio::sync::Notify::new());
        let credentials = Arc::new(RuntimeOnlyCredentials {
            fifth_visit_gate: Some((entered.clone(), resume.clone())),
            ..Default::default()
        });
        let runner = DeploymentLoop::new(
            Arc::new(crate::config::ManagerConfig::default()),
            store.clone(),
            releases,
            credentials.clone(),
            bindings,
            None,
        );
        let tick = tokio::spawn(async move { runner.tick().await });
        tokio::time::timeout(Duration::from_secs(5), entered.notified())
            .await
            .expect("fifth deployment must be reached");
        // The fifth credential lookup stands in for unrelated slow cloud work.
        // Earlier skipped retries must already be available to the CLI.
        let earlier_ids = credentials.visits.lock().unwrap()[..4].to_vec();
        let earlier_filter = DeploymentFilter {
            deployment_ids: Some(earlier_ids),
            statuses: Some(vec!["preflights-failed".to_string()]),
            ..Default::default()
        };
        let earlier = store
            .acquire(&caller, "cli-during-tick", &earlier_filter, 4)
            .await
            .unwrap();
        let earlier_count = earlier.len();
        for item in earlier {
            assert!(item.deployment.retry_requested);
            store
                .set_retry_requested(&caller, &item.deployment.id)
                .await
                .unwrap();
            store
                .release(
                    &caller,
                    &item.deployment.id,
                    "cli-during-tick",
                    item.execution_claim,
                )
                .await
                .unwrap();
        }
        resume.notify_one();
        tick.await.unwrap();
        assert_eq!(
            earlier_count, 4,
            "skipped retries must be acquirable while unrelated work is still blocked"
        );
        let mut visits = credentials.visits.lock().unwrap().clone();
        visits.sort();
        ids.sort();
        assert_eq!(
            visits, ids,
            "each queued retry must be visited once without starving the fifth deployment"
        );
        let filter = DeploymentFilter {
            deployment_ids: Some(ids.clone()),
            statuses: Some(vec!["preflights-failed".to_string()]),
            ..Default::default()
        };
        let cli = store
            .acquire(&caller, "cli-after-tick", &filter, 5)
            .await
            .unwrap();
        assert_eq!(
            cli.len(),
            5,
            "all skipped leases must be returned before tick exits"
        );
        assert!(cli.iter().all(|item| item.deployment.retry_requested));
    }

    /// A manager whose credentials cannot provision a deployment's phase must
    /// not re-acquire it on every tick: each acquisition rewrites the row
    /// (lease, retry checkpoint, release) and nothing can change until a
    /// setup-capable driver acts. Covers an abandoned retry on a failed
    /// preflight and a pending deployment whose CLI never acquired it.
    #[cfg(feature = "sqlite")]
    #[tokio::test]
    async fn unprovisionable_deployment_is_not_reacquired_every_tick() {
        for (status, retry) in [("preflights-failed", true), ("pending", false)] {
            let temp = tempfile::tempdir().unwrap();
            let db = Arc::new(
                SqliteDatabase::new(temp.path().join("manager.db").to_str().unwrap())
                    .await
                    .unwrap(),
            );
            let store = Arc::new(SqliteDeploymentStore::new(db.clone()));
            let releases = Arc::new(SqliteReleaseStore::new(db.clone()));
            let caller = Subject::system();
            let release = releases
                .create_release(
                    &caller,
                    crate::traits::release_store::CreateReleaseParams {
                        project_id: "default".to_string(),
                        stacks: HashMap::from([(
                            Platform::Aws,
                            serde_json::from_value(
                                serde_json::json!({"id":"retry", "resources":{}}),
                            )
                            .unwrap(),
                        )]),
                        git_commit_sha: None,
                        git_commit_ref: None,
                        git_commit_message: None,
                    },
                )
                .await
                .unwrap();
            let deployment = store
                .create_deployment(
                    &caller,
                    CreateDeploymentParams {
                        name: "abandoned".to_string(),
                        deployment_group_id: "dg_test".to_string(),
                        platform: Platform::Aws,
                        deployment_protocol_version: alien_core::DEPLOYMENT_PROTOCOL_VERSION,
                        base_platform: None,
                        stack_settings: StackSettings {
                            deployment_model: alien_core::DeploymentModel::Push,
                            ..Default::default()
                        },
                        stack_state: None,
                        environment_variables: None,
                        public_subdomain: None,
                        input_values: Default::default(),
                        setup_item: None,
                        deployment_token: None,
                    },
                )
                .await
                .unwrap();
            db.execute(&format!(
                "UPDATE deployments SET status = '{status}', desired_release_id = '{}' WHERE id = '{}'",
                release.id, deployment.id
            ))
            .await
            .unwrap();
            // The retry a CLI requested before it died, or one requested with
            // no CLI around.
            if retry {
                store
                    .set_retry_requested(&caller, &deployment.id)
                    .await
                    .unwrap();
            }
            db.execute("CREATE TABLE row_writes (n INTEGER)")
                .await
                .unwrap();
            db.execute(
                "CREATE TRIGGER count_row_writes AFTER UPDATE ON deployments BEGIN INSERT INTO row_writes VALUES (1); END",
            )
            .await
            .unwrap();
            let row_writes = || async {
                let conn = db.conn().lock().await;
                let mut rows = conn
                    .query("SELECT count(*) FROM row_writes", ())
                    .await
                    .unwrap();
                let row = rows.next().await.unwrap().unwrap();
                row.get::<i64>(0).unwrap()
            };
            let bindings = Arc::new(ServerBindings {
                kv: Arc::new(LocalKv::new(temp.path().join("kv")).await.unwrap()),
                command_storage: Arc::new(
                    LocalStorage::new(temp.path().join("storage").to_string_lossy().to_string())
                        .unwrap(),
                ),
                command_dispatcher: Arc::new(NullCommandDispatcher),
                command_registry: Arc::new(InMemoryCommandRegistry::default()),
                artifact_registry: None,
                bindings_provider: None,
                target_bindings_providers: Default::default(),
                registry_routing_table: Arc::new(
                    crate::routes::registry_proxy::RegistryRoutingTable::new(vec![]).unwrap(),
                ),
            });
            let credentials = Arc::new(RuntimeOnlyCredentials::default());
            let runner = DeploymentLoop::new(
                Arc::new(crate::config::ManagerConfig::default()),
                store.clone(),
                releases,
                credentials.clone(),
                bindings,
                None,
            );
            let visits = || credentials.visits.lock().unwrap().len();

            runner.tick().await;
            assert_eq!(visits(), 1, "{status}: the first tick visits it once");
            let writes_after_first_tick = row_writes().await;
            assert!(
                writes_after_first_tick > 0,
                "{status}: the first tick leases it"
            );
            for _ in 0..20 {
                runner.tick().await;
            }
            assert_eq!(
                visits(),
                1,
                "{status}: later ticks must not acquire it again"
            );
            assert_eq!(
                row_writes().await,
                writes_after_first_tick,
                "{status}: later ticks must not write its row"
            );

            let stored = store
                .get_deployment(&caller, &deployment.id)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(stored.status, status);
            assert_eq!(stored.retry_requested, retry, "{status}: the retry is kept");
            assert!(stored.locked_by.is_none());

            // A setup-capable CLI still acquires it right away.
            let cli_filter = DeploymentFilter {
                deployment_ids: Some(vec![deployment.id.clone()]),
                statuses: Some(vec![status.to_string()]),
                deployment_model: Some(alien_core::DeploymentModel::Push),
                ..Default::default()
            };
            let cli = store
                .acquire(&caller, "cli-session", &cli_filter, 1)
                .await
                .unwrap();
            assert_eq!(cli.len(), 1, "{status}: the CLI must acquire it");
            assert_eq!(cli[0].deployment.retry_requested, retry);
            store
                .release_unused_acquisition(&caller, &cli[0], "cli-session")
                .await
                .unwrap();

            // Leaving it alone does not hold back other work: a deployment that
            // arrives later is acquired on the next tick.
            let newcomer = store
                .create_deployment(
                    &caller,
                    CreateDeploymentParams {
                        name: "newcomer".to_string(),
                        deployment_group_id: "dg_test".to_string(),
                        platform: Platform::Aws,
                        deployment_protocol_version: alien_core::DEPLOYMENT_PROTOCOL_VERSION,
                        base_platform: None,
                        stack_settings: StackSettings {
                            deployment_model: alien_core::DeploymentModel::Push,
                            ..Default::default()
                        },
                        stack_state: None,
                        environment_variables: None,
                        public_subdomain: None,
                        input_values: Default::default(),
                        setup_item: None,
                        deployment_token: None,
                    },
                )
                .await
                .unwrap();
            db.execute(&format!(
                "UPDATE deployments SET desired_release_id = '{}' WHERE id = '{}'",
                release.id, newcomer.id
            ))
            .await
            .unwrap();
            let parked_writes = row_writes().await;
            runner.tick().await;
            assert_eq!(
                credentials.visits.lock().unwrap()[1..],
                [newcomer.id.clone()],
                "{status}: the newcomer is acquired and the waiting deployment is not"
            );
            let parked_row = store
                .get_deployment(&caller, &deployment.id)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(parked_row.status, status);
            assert!(
                row_writes().await > parked_writes,
                "{status}: the newcomer was leased"
            );

            // The wait ends when its status changes (to another status these
            // credentials cannot provision either)...
            let other_status = if status == "pending" {
                "preflights-failed"
            } else {
                "pending"
            };
            db.execute(&format!(
                "UPDATE deployments SET status = '{other_status}', retry_requested = 1 WHERE id = '{}'",
                deployment.id
            ))
            .await
            .unwrap();
            runner.tick().await;
            assert_eq!(visits(), 3, "{status}: a status change ends the wait");
            assert_eq!(credentials.visits.lock().unwrap()[2], deployment.id);
            runner.tick().await;
            assert_eq!(visits(), 3, "{status}: the new status waits again");

            // ...or when the recheck time passes.
            for entry in runner
                .unprovisionable
                .lock()
                .expect("unprovisionable poisoned")
                .values_mut()
            {
                entry.recheck_after = chrono::Utc::now() - chrono::Duration::seconds(1);
            }
            runner.tick().await;
            assert_eq!(visits(), 5, "{status}: expired waits are rechecked");
        }
    }

    /// A deployment that turned telemetry off gets no log endpoint from this manager, as the
    /// platform already does; one that left it on gets this manager's.
    #[test]
    fn telemetry_off_gets_no_log_endpoint_from_the_manager() {
        let config = crate::config::ManagerConfig {
            enable_local_log_ingest: true,
            ..Default::default()
        };
        let deployment_with = |telemetry: alien_core::TelemetryMode| DeploymentRecord {
            deployment_token: Some("dep-token".to_string()),
            stack_settings: Some(StackSettings {
                telemetry,
                ..Default::default()
            }),
            ..deployment_record(DeploymentStatus::Running, None)
        };

        assert!(
            build_monitoring_config(&config, &deployment_with(alien_core::TelemetryMode::Off))
                .is_none()
        );
        let monitoring =
            build_monitoring_config(&config, &deployment_with(alien_core::TelemetryMode::Auto))
                .expect("telemetry left on ships logs to the manager");
        assert_eq!(
            monitoring.logs_endpoint,
            format!("{}/v1/logs", config.base_url())
        );
    }

    fn deployment_status_str(status: DeploymentStatus) -> &'static str {
        match status {
            DeploymentStatus::Pending => "pending",
            DeploymentStatus::PreflightsFailed => "preflights-failed",
            DeploymentStatus::InitialSetup => "initial-setup",
            DeploymentStatus::InitialSetupFailed => "initial-setup-failed",
            DeploymentStatus::Provisioning => "provisioning",
            DeploymentStatus::WaitingForMachines => "waiting-for-machines",
            DeploymentStatus::WaitingForSecrets => "waiting-for-secrets",
            DeploymentStatus::ProvisioningFailed => "provisioning-failed",
            DeploymentStatus::Running => "running",
            DeploymentStatus::RefreshFailed => "refresh-failed",
            DeploymentStatus::UpdatePending => "update-pending",
            DeploymentStatus::Updating => "updating",
            DeploymentStatus::UpdateFailed => "update-failed",
            DeploymentStatus::DeletePending => "delete-pending",
            DeploymentStatus::Deleting => "deleting",
            DeploymentStatus::DeleteFailed => "delete-failed",
            DeploymentStatus::TeardownRequired => "teardown-required",
            DeploymentStatus::TeardownFailed => "teardown-failed",
            DeploymentStatus::Deleted => "deleted",
            DeploymentStatus::Error => "error",
        }
    }

    fn stack_state_with_remote_management_outputs(has_outputs: bool) -> StackState {
        let mut state = StackState::with_resource_prefix(Platform::Aws, "test".to_string());
        let builder = StackResourceState::builder()
            .resource_type("remote-stack-management".to_string())
            .status(ResourceStatus::Running)
            .config(Resource::new(RemoteStackManagement {
                id: "remote-stack-management".to_string(),
                management_permission_sets: None,
            }))
            .dependencies(Vec::new());

        let resource_state = if has_outputs {
            builder
                .outputs(ResourceOutputs::new(RemoteStackManagementOutputs {
                    management_resource_id: "arn:aws:iam::123456789012:role/test-management"
                        .to_string(),
                    access_configuration: "arn:aws:iam::123456789012:role/test-management"
                        .to_string(),
                    legacy_remote_bindings_access: None,
                }))
                .build()
        } else {
            builder.build()
        };

        state
            .resources
            .insert("remote-stack-management".to_string(), resource_state);
        state
    }

    #[test]
    fn active_work_statuses_include_new_deployment_phases() {
        let statuses = active_work_statuses();
        assert!(statuses.iter().any(|status| status == "pending"));
        assert!(statuses.iter().any(|status| status == "initial-setup"));
        assert!(statuses.iter().any(|status| status == "provisioning"));
    }

    #[test]
    fn active_work_statuses_include_deletion_phases() {
        let statuses = active_work_statuses();
        assert!(
            statuses.iter().any(|s| s == "delete-pending"),
            "delete-pending must be in active statuses"
        );
        assert!(
            statuses.iter().any(|s| s == "deleting"),
            "deleting must be in active statuses"
        );
    }

    #[test]
    fn active_work_statuses_exclude_terminal_and_failed() {
        let statuses = active_work_statuses();
        for excluded in [
            "running",
            "deleted",
            "initial-setup-failed",
            "provisioning-failed",
            "refresh-failed",
            "update-failed",
            "delete-failed",
        ] {
            assert!(
                !statuses.iter().any(|s| s == excluded),
                "{excluded} should NOT be in active_work_statuses"
            );
        }
    }

    #[test]
    fn retryable_failed_statuses_include_manual_retry_candidates() {
        let statuses = retryable_failed_statuses();
        for included in [
            "preflights-failed",
            "initial-setup-failed",
            "provisioning-failed",
            "refresh-failed",
            "update-failed",
            "delete-failed",
        ] {
            assert!(
                statuses.iter().any(|s| s == included),
                "{included} should be a retryable failed status"
            );
        }
    }

    #[test]
    fn credential_failure_waits_during_expected_handoff_windows() {
        let pending = deployment_record(DeploymentStatus::Pending, None);
        assert!(should_wait_for_credential_handoff(
            DeploymentStatus::Pending,
            &pending
        ));

        let initial_setup_without_stack = deployment_record(DeploymentStatus::InitialSetup, None);
        assert!(should_wait_for_credential_handoff(
            DeploymentStatus::InitialSetup,
            &initial_setup_without_stack
        ));

        let initial_setup_without_outputs = deployment_record(
            DeploymentStatus::InitialSetup,
            Some(stack_state_with_remote_management_outputs(false)),
        );
        assert!(should_wait_for_credential_handoff(
            DeploymentStatus::InitialSetup,
            &initial_setup_without_outputs
        ));
    }

    #[test]
    fn credential_failure_marks_manager_owned_phases_failed() {
        let initial_setup_with_outputs = deployment_record(
            DeploymentStatus::InitialSetup,
            Some(stack_state_with_remote_management_outputs(true)),
        );
        assert!(!should_wait_for_credential_handoff(
            DeploymentStatus::InitialSetup,
            &initial_setup_with_outputs
        ));

        for status in [
            DeploymentStatus::Provisioning,
            DeploymentStatus::Running,
            DeploymentStatus::UpdatePending,
            DeploymentStatus::Updating,
            DeploymentStatus::DeletePending,
            DeploymentStatus::Deleting,
        ] {
            let deployment = deployment_record(status, Some(StackState::new(Platform::Aws)));
            assert!(
                !should_wait_for_credential_handoff(status, &deployment),
                "{status:?} should be classified as manager-owned"
            );
        }
    }

    #[test]
    fn gcp_provisioning_retries_target_credential_handoff_during_grace_period() {
        let now = Utc::now();
        let mut deployment = deployment_record(
            DeploymentStatus::Provisioning,
            Some(stack_state_with_remote_management_outputs(true)),
        );
        deployment.platform = Platform::Gcp;
        deployment.updated_at = Some(now - chrono::Duration::minutes(2));
        let error = AlienError::new(ErrorData::RemoteCredentialHandoffFailed {
            deployment_id: deployment.id.clone(),
            platform: Platform::Gcp,
        })
        .into_generic();

        let remaining = gcp_credential_handoff_retry_remaining(
            DeploymentStatus::Provisioning,
            &deployment,
            &error,
            now,
        )
        .expect("fresh GCP handoff failure should remain retryable");

        assert_eq!(
            remaining,
            GCP_CREDENTIAL_HANDOFF_GRACE_PERIOD - Duration::from_secs(2 * 60)
        );
    }

    #[test]
    fn gcp_provisioning_handoff_becomes_hard_failure_after_grace_period() {
        let now = Utc::now();
        let mut deployment = deployment_record(
            DeploymentStatus::Provisioning,
            Some(stack_state_with_remote_management_outputs(true)),
        );
        deployment.platform = Platform::Gcp;
        deployment.updated_at = Some(
            now - chrono::Duration::from_std(GCP_CREDENTIAL_HANDOFF_GRACE_PERIOD)
                .expect("grace period should fit chrono duration"),
        );
        let error = AlienError::new(ErrorData::RemoteCredentialHandoffFailed {
            deployment_id: deployment.id.clone(),
            platform: Platform::Gcp,
        })
        .into_generic();

        assert_eq!(
            gcp_credential_handoff_retry_remaining(
                DeploymentStatus::Provisioning,
                &deployment,
                &error,
                now,
            ),
            None,
            "handoff retry must be bounded"
        );

        let failed = super::failed_state_for_credential_error(
            &deployment,
            DeploymentStatus::Provisioning,
            None,
            "release",
            error,
        );
        assert_eq!(failed.status, DeploymentStatus::ProvisioningFailed);
        assert_eq!(
            failed.error.expect("hard failure should retain error").code,
            "REMOTE_CREDENTIAL_HANDOFF_FAILED"
        );
    }

    #[test]
    fn provisioning_does_not_retry_unrelated_credential_failures() {
        let now = Utc::now();
        let mut deployment = deployment_record(
            DeploymentStatus::Provisioning,
            Some(stack_state_with_remote_management_outputs(true)),
        );
        deployment.platform = Platform::Gcp;
        deployment.updated_at = Some(now);
        let error = AlienError::new(alien_error::GenericError {
            message: "management binding is missing".to_string(),
        });

        assert_eq!(
            gcp_credential_handoff_retry_remaining(
                DeploymentStatus::Provisioning,
                &deployment,
                &error,
                now,
            ),
            None
        );
    }

    #[test]
    fn remote_stack_management_output_detection_is_explicit() {
        assert!(!has_remote_stack_management_outputs(
            &stack_state_with_remote_management_outputs(false)
        ));
        assert!(has_remote_stack_management_outputs(
            &stack_state_with_remote_management_outputs(true)
        ));
    }

    #[test]
    fn manager_candidate_statuses_are_active_plus_retryable_failed() {
        let active = active_work_statuses();
        let retryable = retryable_failed_statuses();
        let candidates = manager_candidate_statuses();

        for status in active.iter().chain(retryable.iter()) {
            assert!(
                candidates.iter().any(|candidate| candidate == status),
                "{status} should be a manager acquisition candidate"
            );
        }
    }

    #[test]
    fn parse_status_roundtrips_all_known_statuses() {
        let cases = [
            ("pending", DeploymentStatus::Pending),
            ("preflights-failed", DeploymentStatus::PreflightsFailed),
            ("initial-setup", DeploymentStatus::InitialSetup),
            ("initial-setup-failed", DeploymentStatus::InitialSetupFailed),
            ("provisioning", DeploymentStatus::Provisioning),
            ("provisioning-failed", DeploymentStatus::ProvisioningFailed),
            ("running", DeploymentStatus::Running),
            ("refresh-failed", DeploymentStatus::RefreshFailed),
            ("update-pending", DeploymentStatus::UpdatePending),
            ("updating", DeploymentStatus::Updating),
            ("update-failed", DeploymentStatus::UpdateFailed),
            ("delete-pending", DeploymentStatus::DeletePending),
            ("deleting", DeploymentStatus::Deleting),
            ("delete-failed", DeploymentStatus::DeleteFailed),
            ("deleted", DeploymentStatus::Deleted),
            ("error", DeploymentStatus::Error),
        ];

        for (input, expected) in cases {
            assert_eq!(
                parse_status(input),
                expected,
                "parse_status({input:?}) mismatch"
            );
        }
    }

    #[test]
    fn only_bootstrap_statuses_need_provision_capability() {
        assert!(needs_provision_capability(DeploymentStatus::Pending));
        assert!(needs_provision_capability(
            DeploymentStatus::PreflightsFailed
        ));

        for status in [
            DeploymentStatus::InitialSetup,
            DeploymentStatus::Provisioning,
            DeploymentStatus::UpdatePending,
            DeploymentStatus::Updating,
            DeploymentStatus::DeletePending,
            DeploymentStatus::Deleting,
            DeploymentStatus::DeleteFailed,
        ] {
            assert!(
                !needs_provision_capability(status),
                "{status:?} should run with management credentials"
            );
        }
    }

    #[test]
    fn manager_skip_logic_matches_contract() {
        let skipped_for_push = [
            DeploymentStatus::Pending,
            DeploymentStatus::InitialSetup,
            DeploymentStatus::DeletePending,
            DeploymentStatus::Deleting,
            DeploymentStatus::DeleteFailed,
        ];

        for status in skipped_for_push {
            let deploy_result = classify_status(&status, LoopOperation::Deploy);
            let delete_result = classify_status(&status, LoopOperation::Delete);

            if status == DeploymentStatus::DeleteFailed {
                assert!(
                    deploy_result.is_some(),
                    "DeleteFailed should be terminal in contract"
                );
            } else {
                let _ = (deploy_result, delete_result);
            }
        }
    }

    fn stack_with_workers(worker_ids: &[&str]) -> Stack {
        worker_ids
            .iter()
            .fold(Stack::new("supplied".to_string()), |stack, id| {
                stack.add(
                    Worker::new((*id).to_string())
                        .code(WorkerCode::Image {
                            image: "worker:latest".to_string(),
                        })
                        .permissions("execution".to_string())
                        .build(),
                    ResourceLifecycle::Live,
                )
            })
            .build()
    }

    fn resource_ids(stack: &Stack) -> Vec<String> {
        let mut ids: Vec<String> = stack.resources().map(|(id, _)| id.clone()).collect();
        ids.sort();
        ids
    }

    #[test]
    fn supplied_stacks_can_add_resources_within_one_release() {
        // Same release installed and desired; the control plane adds a
        // resource to the target only. Both stacks must be kept apart so the
        // update creates the new resource.
        let installed = stack_with_workers(&["app"]);
        let desired = stack_with_workers(&["app", "extra"]);
        let mut record = deployment_record(DeploymentStatus::UpdatePending, None);
        record.current_release_id = Some("rel_test".to_string());
        record.supplied_stacks = Some(SuppliedStacks {
            current: Some(installed.clone()),
            desired: Some(desired.clone()),
        });

        let (target, current) = supplied_release_stacks(&record);

        assert_eq!(
            resource_ids(&target.expect("target supplied")),
            vec!["app", "extra"]
        );
        assert_eq!(
            resource_ids(&current.expect("current supplied")),
            vec!["app"]
        );
    }

    #[test]
    fn supplied_current_stack_is_the_target_when_nothing_is_desired() {
        let installed = stack_with_workers(&["app", "extra"]);
        let mut record = deployment_record(DeploymentStatus::Running, None);
        record.current_release_id = Some("rel_test".to_string());
        record.desired_release_id = None;
        record.supplied_stacks = Some(SuppliedStacks {
            current: Some(installed.clone()),
            desired: None,
        });

        let (target, _) = supplied_release_stacks(&record);

        assert_eq!(
            resource_ids(&target.expect("current used as target")),
            vec!["app", "extra"]
        );
    }

    #[test]
    fn records_without_supplied_stacks_read_the_release() {
        let record = deployment_record(DeploymentStatus::UpdatePending, None);
        let (target, current) = supplied_release_stacks(&record);
        assert!(target.is_none());
        assert!(current.is_none());
    }

    #[cfg(feature = "sqlite")]
    struct NoRuntimeCredentials;

    #[cfg(feature = "sqlite")]
    #[async_trait::async_trait]
    impl crate::traits::CredentialResolver for NoRuntimeCredentials {
        async fn resolve(
            &self,
            _: &DeploymentRecord,
        ) -> Result<alien_core::ClientConfig, AlienError> {
            panic!("a rejected first install must not resolve runtime credentials");
        }
    }

    #[cfg(feature = "sqlite")]
    #[tokio::test]
    async fn credential_free_destroy_removes_sqlite_deployment() {
        let directory = TempDir::new().unwrap();
        let manager = crate::server::AlienManager::builder(crate::config::ManagerConfig {
            db_path: Some(directory.path().join("manager.db")),
            state_dir: Some(directory.path().to_path_buf()),
            ..Default::default()
        })
        .credential_resolver(std::sync::Arc::new(NoRuntimeCredentials))
        .with_standalone_defaults(&crate::standalone_config::ManagerTomlConfig::default())
        .await
        .unwrap()
        .build()
        .await
        .unwrap();
        let subject = crate::auth::Subject::system();
        let group = manager
            .deployment_store
            .create_deployment_group(
                &subject,
                crate::traits::CreateDeploymentGroupParams {
                    name: "rejected-installs".to_string(),
                    max_deployments: 10,
                    setup: Default::default(),
                },
            )
            .await
            .unwrap();
        let processor = super::DeploymentLoop::new(
            manager.config.clone(),
            manager.deployment_store.clone(),
            manager.release_store.clone(),
            manager.credential_resolver.clone(),
            manager.server_bindings.clone(),
            None,
        );
        for platform in [Platform::Aws, Platform::Gcp, Platform::Azure] {
            let created = manager
                .deployment_store
                .create_with_state(
                    &subject,
                    crate::traits::CreateImportedDeploymentParams {
                        name: format!("rejected-{platform}"),
                        deployment_group_id: group.id.clone(),
                        platform,
                        deployment_protocol_version:
                            alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
                        base_platform: None,
                        stack_settings: StackSettings::default(),
                        stack_state: StackState::new(platform),
                        environment_info: None,
                        runtime_metadata: Default::default(),
                        status: "delete-pending".to_string(),
                        current_release_id: None,
                        desired_release_id: Some("rejected-release".to_string()),
                        import_source: None,
                        setup_metadata: None,
                        setup_target: "test".to_string(),
                        setup_fingerprint: format!("test-{platform}"),
                        setup_fingerprint_version: 1,
                        deployment_token: None,
                        management_config: None,
                        input_values: Default::default(),
                    },
                )
                .await
                .unwrap();
            let mut acquired = manager
                .deployment_store
                .acquire(
                    &subject,
                    "destroy-test",
                    &crate::traits::DeploymentFilter {
                        statuses: Some(vec!["delete-pending".to_string()]),
                        ..Default::default()
                    },
                    1,
                )
                .await
                .unwrap();
            assert_eq!(acquired.len(), 1);
            let mut acquired = acquired.remove(0);
            assert_eq!(acquired.deployment.id, created.id);
            acquired.deployment.supplied_stacks = Some(SuppliedStacks {
                current: None,
                desired: Some(Stack::new("rejected-stack".to_string()).build()),
            });
            processor
                .process_deployment_inner(
                    acquired.deployment,
                    acquired.execution_claim,
                    "destroy-test",
                    super::ProcessOptions::deployment_tick(),
                )
                .await
                .expect("destroy without management credentials");
            assert!(
                manager
                    .deployment_store
                    .get_deployment(&subject, &created.id)
                    .await
                    .unwrap()
                    .is_none(),
                "{platform}: deleted record must be removed"
            );
        }
    }

    #[cfg(feature = "sqlite")]
    #[tokio::test]
    async fn new_release_waits_for_the_prepared_local_startup_to_converge() {
        let directory = TempDir::new().unwrap();
        let manager = crate::server::AlienManager::builder(crate::config::ManagerConfig {
            db_path: Some(directory.path().join("manager.db")),
            state_dir: Some(directory.path().to_path_buf()),
            ..Default::default()
        })
        .credential_resolver(Arc::new(
            crate::providers::local_credentials::LocalCredentialResolver::new(
                directory.path().to_path_buf(),
            ),
        ))
        .with_standalone_defaults(&crate::standalone_config::ManagerTomlConfig::default())
        .await
        .unwrap()
        .build()
        .await
        .unwrap();
        let subject = crate::auth::Subject::system();
        let group = manager
            .deployment_store
            .create_deployment_group(
                &subject,
                crate::traits::CreateDeploymentGroupParams {
                    name: "local".to_string(),
                    max_deployments: 10,
                    setup: Default::default(),
                },
            )
            .await
            .unwrap();
        let mut releases = Vec::new();
        for added in [false, true] {
            let builder = Stack::new("startup".to_string()).add(
                alien_core::Storage::new("retained".to_string()).build(),
                ResourceLifecycle::Frozen,
            );
            let stack = if added {
                builder
                    .add(
                        alien_core::Storage::new("added".to_string()).build(),
                        ResourceLifecycle::Frozen,
                    )
                    .build()
            } else {
                builder.build()
            };
            releases.push(
                manager
                    .release_store
                    .create_release(
                        &subject,
                        crate::traits::CreateReleaseParams {
                            project_id: "default".to_string(),
                            stacks: HashMap::from([(Platform::Local, stack)]),
                            git_commit_sha: None,
                            git_commit_ref: None,
                            git_commit_message: None,
                        },
                    )
                    .await
                    .unwrap()
                    .id,
            );
        }
        let deployment = manager
            .deployment_store
            .create_deployment(
                &subject,
                crate::traits::CreateDeploymentParams {
                    name: "startup".to_string(),
                    deployment_group_id: group.id,
                    platform: Platform::Local,
                    deployment_protocol_version: alien_core::DEPLOYMENT_PROTOCOL_VERSION,
                    base_platform: None,
                    stack_settings: StackSettings::default(),
                    stack_state: None,
                    environment_variables: None,
                    public_subdomain: None,
                    input_values: Default::default(),
                    setup_item: None,
                    deployment_token: None,
                },
            )
            .await
            .unwrap();
        manager
            .deployment_store
            .set_deployment_desired_release(&subject, &deployment.id, &releases[0])
            .await
            .unwrap();
        let runner = DeploymentLoop::new(
            manager.config.clone(),
            manager.deployment_store.clone(),
            manager.release_store.clone(),
            manager.credential_resolver.clone(),
            manager.server_bindings.clone(),
            None,
        );
        let record = manager
            .deployment_store
            .get_deployment(&subject, &deployment.id)
            .await
            .unwrap()
            .unwrap();
        runner
            .process_deployment_inner(
                record,
                None,
                "startup",
                ProcessOptions {
                    max_steps: 1,
                    require_heartbeats_enabled: false,
                },
            )
            .await
            .unwrap();
        let prepared = manager
            .deployment_store
            .get_deployment(&subject, &deployment.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(prepared.status, "initial-setup");
        // A publication races the retry's commit: desired changes after preparation.
        manager
            .deployment_store
            .set_deployment_desired_release(&subject, &deployment.id, &releases[1])
            .await
            .unwrap();
        let data = directory.path().join(&deployment.id).join("storage");
        let mut saw_old_release = false;
        for _ in 0..40 {
            let record = manager
                .deployment_store
                .get_deployment(&subject, &deployment.id)
                .await
                .unwrap()
                .unwrap();
            if record.current_release_id.as_deref() == Some(releases[0].as_str())
                && !saw_old_release
            {
                assert!(
                    !data.join("added").exists(),
                    "the old operation must not claim the new stack"
                );
                assert_eq!(record.status, "update-pending");
                std::fs::write(data.join("retained/value"), "keep me").unwrap();
                saw_old_release = true;
            }
            if record.status == "running"
                && record.current_release_id.as_deref() == Some(releases[1].as_str())
            {
                assert!(saw_old_release);
                assert!(data.join("added").is_dir());
                assert_eq!(
                    std::fs::read_to_string(data.join("retained/value")).unwrap(),
                    "keep me"
                );
                runner.shutdown_local_runtimes().await.unwrap();
                return;
            }
            runner
                .process_deployment_inner(
                    record,
                    None,
                    "startup",
                    ProcessOptions {
                        max_steps: 1,
                        require_heartbeats_enabled: false,
                    },
                )
                .await
                .unwrap();
        }
        panic!("the prepared release and its queued successor must converge");
    }

    /// Only `alien dev` ties container lifetime to the manager. A standalone manager
    /// restart must leave a saved deployment's container running.
    #[cfg(feature = "sqlite")]
    #[tokio::test]
    #[ignore = "requires Docker and registry access"]
    async fn only_dev_managers_stop_saved_local_containers_on_shutdown() {
        for stop_containers in [false, true] {
            let directory = TempDir::new().unwrap();
            let manager = crate::server::AlienManager::builder(crate::config::ManagerConfig {
                db_path: Some(directory.path().join("manager.db")),
                state_dir: Some(directory.path().to_path_buf()),
                stop_local_containers_on_shutdown: stop_containers,
                ..Default::default()
            })
            .credential_resolver(Arc::new(
                crate::providers::local_credentials::LocalCredentialResolver::new(
                    directory.path().to_path_buf(),
                ),
            ))
            .with_standalone_defaults(&crate::standalone_config::ManagerTomlConfig::default())
            .await
            .unwrap()
            .build()
            .await
            .unwrap();
            let subject = Subject::system();
            let group = manager
                .deployment_store
                .create_deployment_group(
                    &subject,
                    crate::traits::CreateDeploymentGroupParams {
                        name: "local".to_string(),
                        max_deployments: 10,
                        setup: Default::default(),
                    },
                )
                .await
                .unwrap();
            let deployment = manager
                .deployment_store
                .create_deployment(
                    &subject,
                    CreateDeploymentParams {
                        name: "saved".to_string(),
                        deployment_group_id: group.id,
                        platform: Platform::Local,
                        deployment_protocol_version: alien_core::DEPLOYMENT_PROTOCOL_VERSION,
                        base_platform: None,
                        stack_settings: StackSettings::default(),
                        stack_state: None,
                        environment_variables: None,
                        public_subdomain: None,
                        input_values: Default::default(),
                        setup_item: None,
                        deployment_token: None,
                    },
                )
                .await
                .unwrap();
            // A container recorded by an earlier session; this process never loaded it.
            let containers =
                LocalContainerManager::new(directory.path().join(&deployment.id)).unwrap();
            let id = format!("shutdown-gate-{}-{stop_containers}", std::process::id());
            containers
                .start_container(
                    &id,
                    ContainerConfig {
                        image: "busybox:1.37.0".to_string(),
                        command: Some(vec!["sleep".to_string(), "300".to_string()]),
                        ports: vec![],
                        public_endpoint: None,
                        health_check_port: None,
                        env_vars: HashMap::new(),
                        stateful: false,
                        ordinal: None,
                        volume_mount: None,
                        volume_size: None,
                        bind_mounts: vec![],
                        proxy_token: None,
                    },
                )
                .await
                .unwrap();
            let runner = DeploymentLoop::new(
                manager.config.clone(),
                manager.deployment_store.clone(),
                manager.release_store.clone(),
                manager.credential_resolver.clone(),
                manager.server_bindings.clone(),
                None,
            );

            let result = runner.shutdown_local_runtimes().await;
            let running = containers.is_running(&id).await;
            containers.delete_container(&id).await.unwrap();

            result.unwrap();
            assert_eq!(
                running, !stop_containers,
                "stop_local_containers_on_shutdown={stop_containers}"
            );
        }
    }

    #[tokio::test]
    async fn local_bindings_provider_is_reused_per_deployment() {
        let temp_dir = TempDir::new().unwrap();
        let cache = Mutex::new(HashMap::new());

        let provider_a =
            get_or_create_local_bindings_provider(&cache, temp_dir.path(), "dep_test").unwrap();
        let provider_b =
            get_or_create_local_bindings_provider(&cache, temp_dir.path(), "dep_test").unwrap();

        assert!(std::sync::Arc::ptr_eq(&provider_a, &provider_b));

        provider_a.shutdown().await;
    }

    #[tokio::test]
    async fn local_bindings_provider_different_per_deployment() {
        let temp_dir = TempDir::new().unwrap();
        let cache = Mutex::new(HashMap::new());

        let provider_a =
            get_or_create_local_bindings_provider(&cache, temp_dir.path(), "dep_a").unwrap();
        let provider_b =
            get_or_create_local_bindings_provider(&cache, temp_dir.path(), "dep_b").unwrap();

        assert!(
            !std::sync::Arc::ptr_eq(&provider_a, &provider_b),
            "Different deployment IDs should get different providers"
        );

        provider_a.shutdown().await;
        provider_b.shutdown().await;
    }

    #[test]
    fn worker_commands_push_env_vars_scopes_token_per_command_enabled_worker() {
        let worker_a = Worker::new("worker-a".to_string())
            .code(WorkerCode::Image {
                image: "worker:latest".to_string(),
            })
            .permissions("execution".to_string())
            .commands_enabled(true)
            .build();
        let worker_b = Worker::new("worker-b".to_string())
            .code(WorkerCode::Image {
                image: "worker:latest".to_string(),
            })
            .permissions("execution".to_string())
            .commands_enabled(true)
            .build();
        // Commands-disabled Worker must not expose a command push endpoint.
        let worker_off = Worker::new("worker-off".to_string())
            .code(WorkerCode::Image {
                image: "worker:latest".to_string(),
            })
            .permissions("execution".to_string())
            .build();
        let stack = Stack::new("manager-command-target-stack".to_string())
            .add(worker_a, ResourceLifecycle::Live)
            .add(worker_b, ResourceLifecycle::Live)
            .add(worker_off, ResourceLifecycle::Live)
            .build();

        let vars =
            worker_commands_push_env_vars(Some("ax_dep_test"), &stack).expect("token present");

        // Nothing deployment-wide; nothing scoped to the disabled worker.
        assert!(vars.iter().all(|v| {
            v.target_resources == Some(vec!["worker-a".to_string()])
                || v.target_resources == Some(vec!["worker-b".to_string()])
        }));

        // One secret token per command-enabled worker.
        for worker_id in ["worker-a", "worker-b"] {
            let scoped: Vec<_> = vars
                .iter()
                .filter(|v| v.target_resources == Some(vec![worker_id.to_string()]))
                .collect();
            assert_eq!(scoped.len(), 1, "expected one token for {worker_id}");
            assert!(scoped.iter().any(
                |v| v.name == alien_core::ENV_ALIEN_COMMANDS_TOKEN && v.value == "ax_dep_test"
            ));
        }
    }

    fn command_container(id: &str, commands_enabled: bool) -> Container {
        let builder = Container::new(id.to_string())
            .code(ContainerCode::Image {
                image: "container:latest".to_string(),
            })
            .cpu(ResourceSpec {
                min: "0.5".to_string(),
                desired: "1".to_string(),
            })
            .memory(ResourceSpec {
                min: "512Mi".to_string(),
                desired: "1Gi".to_string(),
            })
            .port(8080)
            .permissions("container-execution".to_string());
        if commands_enabled {
            builder.commands_enabled(true).build()
        } else {
            builder.build()
        }
    }

    /// A control plane's config carries its own copy of the deployment's
    /// variables. The runner config replaces it with the manager's snapshot:
    /// the identity and command variables plus the deployment's own.
    #[tokio::test]
    async fn runner_config_replaces_control_plane_snapshot_with_manager_variables() {
        let daemon = Daemon::new("debug-daemon".to_string())
            .code(DaemonCode::Image {
                image: "daemon:latest".to_string(),
            })
            .permissions("daemon-execution".to_string())
            .commands_enabled(true)
            .build();
        let stack = Stack::new("manager-platform-config-stack".to_string())
            .add(daemon, ResourceLifecycle::Live)
            .build();
        let user_variable = EnvironmentVariable {
            name: "LOG_LEVEL".to_string(),
            value: "debug".to_string(),
            var_type: EnvironmentVariableType::Plain,
            target_resources: None,
        };
        let provided_config = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(EnvironmentVariablesSnapshot {
                variables: vec![user_variable.clone()],
                hash: "control-plane-snapshot".to_string(),
                created_at: "2026-01-01T00:00:00Z".to_string(),
            })
            .allow_frozen_changes(false)
            .external_bindings(Default::default())
            .build();
        let mut deployment = deployment_record(DeploymentStatus::UpdatePending, None);
        deployment.deployment_token = Some("ax_dep_test".to_string());
        deployment.user_environment_variables = Some(vec![user_variable.clone()]);
        deployment.deployment_config = Some(provided_config);

        let state_dir = TempDir::new().expect("temp dir");
        let manager_config = crate::config::ManagerConfig {
            base_url: Some("https://manager.example.test".to_string()),
            ..Default::default()
        };
        let credential_resolver = crate::providers::local_credentials::LocalCredentialResolver::new(
            state_dir.path().to_path_buf(),
        );
        let config = RunnerConfigSources {
            config: &manager_config,
            credential_resolver: &credential_resolver,
            bindings_provider: &None,
            target_bindings_providers: &HashMap::new(),
        }
        .deployment_config(&deployment, Some(&stack))
        .await
        .expect("runner config");

        let scoped = Some(vec!["debug-daemon".to_string()]);
        let variable =
            |name: &str, value: &str, var_type, target_resources: &Option<Vec<String>>| {
                EnvironmentVariable {
                    name: name.to_string(),
                    value: value.to_string(),
                    var_type,
                    target_resources: target_resources.clone(),
                }
            };
        assert_eq!(
            config.environment_variables.variables,
            vec![
                variable(
                    ENV_ALIEN_DEPLOYMENT_ID,
                    "dep_test",
                    EnvironmentVariableType::Plain,
                    &None
                ),
                variable(
                    ENV_ALIEN_DEPLOYMENT_NAME,
                    "test",
                    EnvironmentVariableType::Plain,
                    &None
                ),
                variable(
                    ENV_ALIEN_COMMANDS_URL,
                    "https://manager.example.test/v1",
                    EnvironmentVariableType::Plain,
                    &scoped,
                ),
                variable(
                    ENV_ALIEN_COMMANDS_TOKEN,
                    "ax_dep_test",
                    EnvironmentVariableType::Secret,
                    &scoped,
                ),
                variable(
                    ENV_ALIEN_COMMANDS_TARGET_RESOURCE_TYPE,
                    "daemon",
                    EnvironmentVariableType::Plain,
                    &scoped,
                ),
                variable(
                    ENV_ALIEN_COMMANDS_TARGET_RESOURCE_ID,
                    "debug-daemon",
                    EnvironmentVariableType::Plain,
                    &scoped,
                ),
                user_variable,
            ]
        );
        assert_ne!(config.environment_variables.hash, "control-plane-snapshot");
        assert_eq!(config.deployment_name.as_deref(), Some("test"));
        assert_eq!(config.deployment_token.as_deref(), Some("ax_dep_test"));
        assert_eq!(
            config.manager_url.as_deref(),
            Some("https://manager.example.test")
        );
    }

    #[test]
    fn commands_receiver_env_vars_scopes_contract_per_command_enabled_container_and_daemon() {
        let container = command_container("container-a", true);
        let daemon = Daemon::new("daemon-b".to_string())
            .code(DaemonCode::Image {
                image: "daemon:latest".to_string(),
            })
            .permissions("daemon-execution".to_string())
            .commands_enabled(true)
            .build();
        // Commands-disabled container: must get NOTHING.
        let container_off = command_container("container-off", false);
        // Commands-enabled Worker uses runtime push, never the receiver
        // contract, and must not be a receiver scope target.
        let worker = Worker::new("worker-c".to_string())
            .code(WorkerCode::Image {
                image: "worker:latest".to_string(),
            })
            .permissions("execution".to_string())
            .commands_enabled(true)
            .build();
        let stack = Stack::new("manager-receiver-stack".to_string())
            .add(container, ResourceLifecycle::Live)
            .add(daemon, ResourceLifecycle::Live)
            .add(container_off, ResourceLifecycle::Live)
            .add(worker, ResourceLifecycle::Live)
            .build();

        let vars = commands_receiver_env_vars(
            "https://manager.example.test/v1".to_string(),
            Some("ax_dep_test"),
            &stack,
        )
        .expect("token present");

        // Every var scoped to exactly one command-enabled Container/Daemon; the
        // disabled container and the worker are never scope targets.
        assert!(vars.iter().all(|v| {
            v.target_resources == Some(vec!["container-a".to_string()])
                || v.target_resources == Some(vec!["daemon-b".to_string()])
        }));
        // No deployment-wide DEPLOYMENT_ID is re-emitted here.
        assert!(!vars
            .iter()
            .any(|v| v.name == alien_core::ENV_ALIEN_DEPLOYMENT_ID));

        for (resource_id, expected_type) in [("container-a", "container"), ("daemon-b", "daemon")] {
            let scoped: Vec<_> = vars
                .iter()
                .filter(|v| v.target_resources == Some(vec![resource_id.to_string()]))
                .collect();
            assert_eq!(
                scoped.len(),
                4,
                "expected 4 receiver vars for {resource_id}"
            );
            assert!(scoped.iter().any(|v| {
                v.name == alien_core::ENV_ALIEN_COMMANDS_URL
                    && v.value == "https://manager.example.test/v1"
            }));
            assert!(scoped
                .iter()
                .any(|v| v.name == alien_core::ENV_ALIEN_COMMANDS_TOKEN
                    && v.value == "ax_dep_test"
                    && v.var_type == alien_core::EnvironmentVariableType::Secret));
            assert!(scoped.iter().any(|v| {
                v.name == alien_core::ENV_ALIEN_COMMANDS_TARGET_RESOURCE_ID
                    && v.value == resource_id
            }));
            assert!(scoped.iter().any(|v| {
                v.name == alien_core::ENV_ALIEN_COMMANDS_TARGET_RESOURCE_TYPE
                    && v.value == expected_type
            }));
        }
    }
}

fn needs_provision_capability(status: DeploymentStatus) -> bool {
    matches!(
        status,
        DeploymentStatus::Pending | DeploymentStatus::PreflightsFailed
    )
}

fn deployment_record_error(error: &Option<serde_json::Value>) -> Option<AlienError> {
    let error = error
        .clone()
        .and_then(|value| serde_json::from_value::<AlienError>(value).ok());
    alien_deployment::deployment_state_error_from_headline(error)
}

/// Parse a status string (kebab-case, as stored in the DB) to `DeploymentStatus`.
fn parse_status(status: &str) -> DeploymentStatus {
    match status {
        "pending" => DeploymentStatus::Pending,
        "preflights-failed" => DeploymentStatus::PreflightsFailed,
        "initial-setup" => DeploymentStatus::InitialSetup,
        "initial-setup-failed" => DeploymentStatus::InitialSetupFailed,
        "provisioning" => DeploymentStatus::Provisioning,
        "waiting-for-machines" => DeploymentStatus::WaitingForMachines,
        "waiting-for-secrets" => DeploymentStatus::WaitingForSecrets,
        "provisioning-failed" => DeploymentStatus::ProvisioningFailed,
        "running" => DeploymentStatus::Running,
        "refresh-failed" => DeploymentStatus::RefreshFailed,
        "update-pending" => DeploymentStatus::UpdatePending,
        "updating" => DeploymentStatus::Updating,
        "update-failed" => DeploymentStatus::UpdateFailed,
        "delete-pending" => DeploymentStatus::DeletePending,
        "deleting" => DeploymentStatus::Deleting,
        "delete-failed" => DeploymentStatus::DeleteFailed,
        "teardown-required" => DeploymentStatus::TeardownRequired,
        "teardown-failed" => DeploymentStatus::TeardownFailed,
        "deleted" => DeploymentStatus::Deleted,
        "error" => DeploymentStatus::Error,
        _ => {
            error!(
                status = status,
                "Unknown deployment status, treating as error to prevent unintended redeployment"
            );
            DeploymentStatus::Error
        }
    }
}
