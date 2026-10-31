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
    CredentialRetry, DeploymentConfig, DeploymentModel, DeploymentState, DeploymentStatus,
    EnvironmentVariable, EnvironmentVariableType, EnvironmentVariablesSnapshot, ManagementConfig,
    Platform, ReleaseInfo, Stack, ENV_ALIEN_DEPLOYMENT_ID, ENV_ALIEN_DEPLOYMENT_NAME,
};
use alien_deployment::loop_contract::LoopOperation;
use alien_deployment::runner::{failed_status_for_deployment_error, RunnerPolicy, RunnerResult};
use alien_error::{AlienError, Context, GenericError};
use alien_infra::DefaultPlatformServiceProvider;
use alien_local::{LocalBindingsProvider, LocalContainerManager};
use alien_preflights::mutations::remote_stack_management::stack_renders_management_identity;

use crate::auth::Subject;
use crate::config::ManagerConfig;
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
/// Bound credential outages, including GCP's initial IAM propagation window.
/// The first failure records a timestamp in deployment state; acquisition and
/// later failures never move it, even if a store synthesizes record timestamps.
const CREDENTIAL_ERROR_GRACE_PERIOD: Duration = Duration::from_secs(10 * 60);
const CREDENTIAL_RETRY_INTERVAL: chrono::Duration = chrono::Duration::seconds(5);
/// How long the loop leaves alone a deployment whose phase failed before it
/// could start (no credentials, no management identity) and whose failure is
/// saved, unless the deployment changes first.
const PHASE_FAILURE_RECHECK_INTERVAL: chrono::Duration = chrono::Duration::minutes(5);

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
    /// Acquisition backoff shared by deployment and heartbeat ticks. The retry
    /// deadline itself comes from persisted retry state, never from this cache.
    deferred: Mutex<HashMap<String, DeferredDeployment>>,
}

/// A phase that can't start, to be saved as the deployment's failure.
struct PhaseFailure<'a> {
    deployment: &'a DeploymentRecord,
    status: DeploymentStatus,
    deployment_stack: Option<&'a Stack>,
    target_release_id: Option<&'a str>,
    current_release: Option<ReleaseInfo>,
    had_credential_retry: bool,
    error: AlienError,
    session: &'a str,
    execution_claim: Option<crate::traits::deployment_store::ExecutionClaim>,
}

/// The backoff and retry flag left by this processing attempt.
struct AcquisitionBackoff {
    status: DeploymentStatus,
    delay: chrono::Duration,
    retry_requested: bool,
}

/// A deployment waiting for credentials or another driver.
#[derive(Debug, Clone)]
struct DeferredDeployment {
    /// The status it was skipped in.
    status: DeploymentStatus,
    updated_at: Option<chrono::DateTime<chrono::Utc>>,
    retry_requested: bool,
    /// When the loop may acquire it again if the record has not changed.
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
            deferred: Mutex::new(HashMap::new()),
        }
    }

    /// Add acquisition exclusions without enumerating a possibly capped candidate list.
    /// Refresh cached revisions once per tick; later batches only add newly deferred IDs.
    pub(crate) async fn exclude_deferred(
        &self,
        caller: &Subject,
        filter: &mut DeploymentFilter,
        refresh: bool,
    ) -> Result<bool, AlienError> {
        let now = chrono::Utc::now();
        let cached = {
            let mut deferred = self.deferred.lock().expect("deferred poisoned");
            deferred.retain(|_, entry| entry.recheck_after > now);
            deferred.clone()
        };
        if refresh {
            let mut revisions = stream::iter(cached.into_iter().filter(|(_, entry)| {
                filter.statuses.as_ref().is_none_or(|statuses| {
                    statuses
                        .iter()
                        .any(|status| parse_status(status) == entry.status)
                })
            }))
            .map(|(id, entry)| async move {
                let record = self.deployment_store.get_deployment(caller, &id).await;
                (id, entry, record)
            })
            .buffer_unordered(MAX_CONCURRENT_DEPLOYMENTS);
            while let Some((id, entry, record)) = revisions.next().await {
                let unchanged = match record {
                    Ok(Some(record)) => {
                        entry.status == parse_status(&record.status)
                            && entry.updated_at == record.updated_at
                            && entry.retry_requested == record.retry_requested
                    }
                    Ok(None) => false,
                    Err(error) => {
                        warn!(deployment_id = %id, %error, "Could not refresh acquisition backoff; allowing a recheck");
                        false
                    }
                };
                if !unchanged {
                    let mut deferred = self.deferred.lock().expect("deferred poisoned");
                    if deferred
                        .get(&id)
                        .is_some_and(|current| current.recheck_after == entry.recheck_after)
                    {
                        deferred.remove(&id);
                    }
                }
            }
        }
        let deferred_ids: std::collections::HashSet<String> = self
            .deferred
            .lock()
            .expect("deferred poisoned")
            .iter()
            .filter(|(_, entry)| {
                filter.statuses.as_ref().is_none_or(|statuses| {
                    statuses
                        .iter()
                        .any(|status| parse_status(status) == entry.status)
                })
            })
            .map(|(id, _)| id.clone())
            .collect();
        if self.deployment_store.supports_acquire_exclusions() {
            filter.excluded_deployment_ids.extend(deferred_ids);
            filter.excluded_deployment_ids.sort();
            filter.excluded_deployment_ids.dedup();
        } else if !deferred_ids.is_empty() {
            // Preserve the candidate snapshot fallback for stores that cannot
            // exclude before acquisition. Snapshot at most once per tick; later
            // batches remove newly deferred IDs from the same inclusion list.
            if filter.deployment_ids.is_none() {
                filter.deployment_ids = Some(
                    self.deployment_store
                        .list_deployment_ids(caller, filter)
                        .await?,
                );
            }
            let ids = filter
                .deployment_ids
                .as_mut()
                .expect("candidate IDs set above");
            ids.retain(|id| !deferred_ids.contains(id));
            return Ok(!ids.is_empty());
        }
        Ok(true)
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

        for (deployment_id, _) in &providers {
            self.server_bindings
                .local_queues
                .unregister(deployment_id)
                .await;
        }

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

        match self.exclude_deferred(&caller, &mut filter, true).await {
            Ok(true) => {}
            Ok(false) => return,
            Err(error) => {
                error!(%error, "Failed to list deployment candidates");
                return;
            }
        }

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
                            self.process_deployment(
                                item.deployment,
                                item.execution_claim,
                                &session,
                                ProcessOptions::deployment_tick(),
                            )
                            .await;
                        })
                        .await;
                    match self.exclude_deferred(&caller, &mut filter, false).await {
                        Ok(true) => {}
                        Ok(false) => break,
                        Err(error) => {
                            error!(%error, "Failed to filter remaining deployment candidates");
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
    /// Returns `true` when acquisition should wait before trying this deployment again.
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

        // Capture the revision while we still own the lease.
        let caller = Subject::system();
        let deferred = if let Ok(Some(backoff)) = result {
            // A checkpoint may have changed the status and timestamp. Remember
            // the persisted record so later user actions can end the backoff.
            match self
                .deployment_store
                .get_deployment(&caller, &deployment_id)
                .await
            {
                Ok(Some(record)) => {
                    self.deferred.lock().expect("deferred poisoned").insert(
                        deployment_id.clone(),
                        DeferredDeployment {
                            status: backoff.status,
                            updated_at: record.updated_at,
                            // The reread can already contain a new user retry. Comparing
                            // against our own checkpoint must let that request through.
                            retry_requested: backoff.retry_requested,
                            recheck_after: chrono::Utc::now() + backoff.delay,
                        },
                    );
                }
                Ok(None) => {}
                Err(error) => {
                    error!(%deployment_id, %error, "Failed to remember deployment backoff")
                }
            }
            true
        } else {
            false
        };
        // Release lock unconditionally, including when reading the revision failed.
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
        deferred
    }

    async fn process_deployment_inner(
        &self,
        mut deployment: DeploymentRecord,
        execution_claim: Option<crate::traits::deployment_store::ExecutionClaim>,
        session: &str,
        options: ProcessOptions,
    ) -> Result<Option<AcquisitionBackoff>, AlienError> {
        let deployment_id = deployment.id.clone();
        let stack_settings = deployment
            .stack_settings
            .as_ref()
            .expect("stored deployment carries stack_settings");

        let skip_credentials = stack_settings.deployment_model == alien_core::DeploymentModel::Pull
            || (options.require_heartbeats_enabled && !stack_settings.heartbeats.is_enabled());

        let status = parse_status(&deployment.status);
        let had_credential_retry = deployment
            .runtime_metadata
            .as_ref()
            .is_some_and(|metadata| metadata.credential_retry.is_some());
        if let Some(metadata) = deployment.runtime_metadata.as_mut() {
            if metadata
                .credential_retry
                .as_ref()
                .is_some_and(|retry| retry.status != status)
            {
                metadata.credential_retry = None;
            }
        }

        if skip_credentials {
            if had_credential_retry {
                self.clear_skipped_credential_retry(&deployment, session, execution_claim)
                    .await?;
            }
            debug!(deployment_id = %deployment_id, "Skipping credentials for pull-mode or disabled heartbeat work");
            return Ok(None);
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
                .and_then(|metadata| metadata.pending_prepared_release_id.clone())
        } else {
            None
        };
        let target_release_id = prepared_startup_release
            .as_deref()
            .or(deployment.desired_release_id.as_deref())
            .or(deployment.current_release_id.as_deref());
        // Retention intentionally clears release references. A later explicit erase
        // operates on the saved resource controllers, without needing a build release.
        let deleting = matches!(
            status,
            DeploymentStatus::DeletePending
                | DeploymentStatus::Deleting
                | DeploymentStatus::DeleteFailed
        );
        if target_release_id.is_none() && !deleting {
            return Err(AlienError::new(GenericError {
                message: format!(
                    "Deployment {} has neither desired_release_id nor current_release_id",
                    deployment_id
                ),
            }));
        }

        let system = crate::auth::Subject::system();
        let (_, supplied_current) = supplied_release_stacks(&deployment);
        let deployment_stack = match target_release_id {
            Some(id) => target_release_stack(self.release_store.as_ref(), &deployment, id).await?,
            None => None,
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
            (Some(id), None) if Some(id.as_str()) == target_release_id => {
                deployment_stack.as_ref().map(|stack| ReleaseInfo {
                    release_id: Some(id.clone()),
                    version: None,
                    description: None,
                    stack: stack.clone(),
                })
            }
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

        let mut recorded_state = state_from_record(
            &deployment,
            status,
            deployment_stack.as_ref(),
            target_release_id,
        );
        recorded_state.current_release = current_release.clone();
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
                self.server_bindings
                    .local_queues
                    .unregister(&deployment_id)
                    .await;
                self.local_bindings_cache
                    .lock()
                    .expect("local_bindings_cache poisoned")
                    .remove(&deployment_id);
                self.deployment_store
                    .delete_deployment(&system, &deployment_id)
                    .await?;
            }
            return Ok(None);
        }

        // 2. Resolve credentials for the target platform and lifecycle phase.
        let resolved_credentials = match self
            .credential_resolver
            .resolve_with_capability(&deployment)
            .await
        {
            Ok(resolved) => resolved,
            Err(e) => {
                let e = crate::providers::credential_errors::classify_credential_error(e);
                let now = chrono::Utc::now();
                let retry_remaining = credential_error_retry_remaining(&deployment, &e, now);
                let waiting_for_driver = should_wait_for_credential_handoff(status, &deployment);
                if waiting_for_driver || retry_remaining.is_some() {
                    let retry = deployment
                        .runtime_metadata
                        .as_ref()
                        .and_then(|metadata| metadata.credential_retry.as_ref());
                    // One checkpoint starts the window; subsequent failures only release
                    // the lease. Expected setup-driver waits retain their existing semantics.
                    let checkpoint_retry_state = (!waiting_for_driver && retry.is_none())
                        || (waiting_for_driver && had_credential_retry);
                    let mut retry_state_saved = true;
                    if checkpoint_retry_state {
                        recorded_state
                            .runtime_metadata
                            .get_or_insert_default()
                            .credential_retry = (!waiting_for_driver).then_some(CredentialRetry {
                            started_at: now,
                            status,
                        });
                        recorded_state.error = deployment_record_error(&deployment.error);
                        let expected_retry = recorded_state
                            .runtime_metadata
                            .as_ref()
                            .and_then(|metadata| metadata.credential_retry.clone());
                        let saved = self
                            .checkpoint_without_step(
                                &deployment_id,
                                session,
                                recorded_state,
                                execution_claim.clone(),
                            )
                            .await;
                        retry_state_saved = match saved {
                            Ok(record)
                                if record
                                    .runtime_metadata
                                    .as_ref()
                                    .and_then(|metadata| metadata.credential_retry.as_ref())
                                    == expected_retry.as_ref() =>
                            {
                                true
                            }
                            Ok(_) => {
                                warn!(%deployment_id, "Store dropped credential retry state; failing with the credential error");
                                false
                            }
                            Err(error) => {
                                warn!(%deployment_id, %error, "Store rejected credential retry state; failing with the credential error");
                                false
                            }
                        };
                    }
                    if retry_state_saved {
                        warn!(
                            deployment_id = %deployment_id,
                            status = ?status,
                            platform = ?deployment.platform,
                            retry_window_remaining_secs = ?retry_remaining.map(|duration| duration.as_secs()),
                            error_code = %e.code,
                            error = %e,
                            "Credentials unavailable for deployment phase; waiting to retry or for credential handoff"
                        );
                        return Ok(Some(AcquisitionBackoff {
                            status,
                            delay: CREDENTIAL_RETRY_INTERVAL,
                            retry_requested: checkpoint_retry_state && deployment.retry_requested,
                        }));
                    }
                }
                warn!(
                    deployment_id = %deployment_id,
                    status = ?status,
                    platform = ?deployment.platform,
                    retryable = e.retryable,
                    error_code = %e.code,
                    error = %e,
                    "Credential resolution failed for manager-owned phase"
                );
                return self
                    .checkpoint_phase_failure(PhaseFailure {
                        deployment: &deployment,
                        status,
                        deployment_stack: deployment_stack.as_ref(),
                        target_release_id,
                        current_release,
                        had_credential_retry,
                        error: e.into_generic(),
                        session,
                        execution_claim,
                    })
                    .await;
            }
        };

        if had_credential_retry {
            deployment
                .runtime_metadata
                .as_mut()
                .expect("retry metadata exists")
                .credential_retry = None;
            recorded_state.runtime_metadata = deployment.runtime_metadata.clone();
            recorded_state.error = deployment_record_error(&deployment.error);
            // The cleared retry window is persisted by exactly one checkpoint: the
            // runner's own step checkpoint, or the one below when no step runs.
            // A separate checkpoint before the step would be a second write under
            // the same claim, and a checkpoint that settles a Running or
            // RefreshFailed heartbeat ends that claim, so the step's checkpoint
            // would then be refused as a lost claim.
        }

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
            if deployment.retry_requested || had_credential_retry {
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
            return Ok(Some(AcquisitionBackoff {
                status,
                delay: UNPROVISIONABLE_RECHECK_INTERVAL,
                retry_requested: deployment.retry_requested,
            }));
        }
        let client_config = resolved_credentials.client_config;

        if let Some(id) = target_release_id {
            if deployment_stack.is_none() {
                return Err(AlienError::new(GenericError {
                    message: format!(
                        "Release {} does not contain a stack for platform {}",
                        id, deployment.platform
                    ),
                }));
            }
        }
        let mut state = recorded_state.clone();
        state.error = deployment_record_error(&deployment.error);

        // 4. Build the deployment config. External runners that acquire this
        //    deployment get the same config from the acquire route. Without
        //    its management identity a setup render can't proceed; that is
        //    saved as the deployment's failure, like a credential failure.
        let management_config = match resolve_deployment_management_config(
            self.credential_resolver.as_ref(),
            &deployment,
            deployment.platform,
            ManagementIdentityUse::for_runner(status),
        )
        .await
        {
            Ok(management_config) => management_config,
            Err(error) => {
                warn!(
                    deployment_id = %deployment_id,
                    status = ?status,
                    platform = ?deployment.platform,
                    error = %error,
                    "Management identity unavailable for a setup render"
                );
                return self
                    .checkpoint_phase_failure(PhaseFailure {
                        deployment: &deployment,
                        status,
                        deployment_stack: deployment_stack.as_ref(),
                        target_release_id,
                        current_release,
                        had_credential_retry,
                        error,
                        session,
                        execution_claim,
                    })
                    .await;
            }
        };
        let mut config = RunnerConfigSources {
            config: &self.config,
            credential_resolver: self.credential_resolver.as_ref(),
            bindings_provider: &self.server_bindings.bindings_provider,
            target_bindings_providers: &self.server_bindings.target_bindings_providers,
        }
        .deployment_config(&deployment, deployment_stack.as_ref(), management_config)
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
                self.server_bindings.local_queues.register(
                    &deployment_id,
                    local_bindings.queue_manager().clone(),
                    &self.config.base_url(),
                );
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
            execution_claim.clone(),
            self.config.supported_aws_regions.clone(),
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

        // A runner that ran no step wrote no checkpoint, so the cleared credential
        // retry window still needs its single write.
        if had_credential_retry
            && matches!(&runner_result, Ok(result) if result.steps_executed == 0)
        {
            self.checkpoint_without_step(&deployment_id, session, recorded_state, execution_claim)
                .await?;
        }

        // 8. Handle deleted deployments — clean up the record. Loop
        // context — no inbound caller; pass `Subject::system()`.
        if state.status == DeploymentStatus::Deleted {
            self.server_bindings
                .local_queues
                .unregister(&deployment_id)
                .await;
            self.local_bindings_cache
                .lock()
                .expect("local_bindings_cache poisoned")
                .remove(&deployment_id);
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
        runner_result.map(|_| None).map_err(|e| e.into_generic())
    }

    /// Saves `failure` as the deployment's error in its phase's failed status and backs off
    /// acquisition. A failure already saved unchanged is not written again.
    async fn checkpoint_phase_failure(
        &self,
        failure: PhaseFailure<'_>,
    ) -> Result<Option<AcquisitionBackoff>, AlienError> {
        let PhaseFailure {
            deployment,
            status,
            deployment_stack,
            target_release_id,
            current_release,
            had_credential_retry,
            error,
            session,
            execution_claim,
        } = failure;
        let failed_status = failed_status_for_deployment_error(status);
        let backoff = AcquisitionBackoff {
            status: failed_status,
            delay: PHASE_FAILURE_RECHECK_INTERVAL,
            retry_requested: false,
        };
        let unchanged = status == failed_status
            && !deployment.retry_requested
            && !had_credential_retry
            && deployment_record_error(&deployment.error).as_ref() == Some(&error);
        if unchanged {
            warn!(
                deployment_id = %deployment.id,
                status = ?status,
                error_code = %error.code,
                error = %error,
                "Phase still failing with its saved error; backing off"
            );
            return Ok(Some(backoff));
        }
        warn!(
            deployment_id = %deployment.id,
            status = ?status,
            failed_status = ?failed_status,
            error_code = %error.code,
            "Checkpointing failed deployment state"
        );
        let mut failed_state = failed_state_for_phase_error(
            deployment,
            status,
            deployment_stack,
            target_release_id,
            error,
        );
        failed_state.current_release = current_release;
        self.checkpoint_without_step(&deployment.id, session, failed_state, execution_claim)
            .await?;
        Ok(Some(backoff))
    }

    async fn clear_skipped_credential_retry(
        &self,
        deployment: &DeploymentRecord,
        session: &str,
        execution_claim: Option<crate::traits::deployment_store::ExecutionClaim>,
    ) -> Result<(), AlienError> {
        let mut state = state_from_record(deployment, parse_status(&deployment.status), None, None);
        state.error = deployment_record_error(&deployment.error);
        if let Some(metadata) = state.runtime_metadata.as_mut() {
            metadata.credential_retry = None;
        }
        // A metadata-only checkpoint must preserve both release snapshots, including
        // a pending target different from the currently installed release.
        let (target_stack, current_stack) = supplied_release_stacks(deployment);
        for (is_current, id, supplied) in [
            (true, deployment.current_release_id.as_ref(), current_stack),
            (
                false,
                deployment
                    .desired_release_id
                    .as_ref()
                    .or(deployment.current_release_id.as_ref()),
                target_stack,
            ),
        ] {
            let Some(id) = id else { continue };
            let stack = if let Some(stack) = supplied {
                stack
            } else {
                self.release_store
                    .get_release(&Subject::system(), id)
                    .await?
                    .and_then(|release| release.stacks.get(&deployment.platform).cloned())
                    .ok_or_else(|| {
                        AlienError::new(GenericError {
                            message: format!(
                                "Release {id} is unavailable while clearing credential retry state"
                            ),
                        })
                    })?
            };
            let release = Some(ReleaseInfo {
                release_id: Some(id.clone()),
                version: None,
                description: None,
                stack,
            });
            if is_current {
                state.current_release = release;
            } else {
                state.target_release = release;
            }
        }
        self.checkpoint_without_step(&deployment.id, session, state, execution_claim)
            .await?;
        Ok(())
    }

    async fn checkpoint_without_step(
        &self,
        deployment_id: &str,
        session: &str,
        state: DeploymentState,
        execution_claim: Option<crate::traits::deployment_store::ExecutionClaim>,
    ) -> Result<DeploymentRecord, AlienError> {
        let outcome = self
            .deployment_store
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
        Ok(outcome.record)
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

/// What the next operation of a deployment does with its management identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ManagementIdentityUse {
    /// The operation renders the stack and creates or updates its resources. Without the
    /// identity the stack renders without its setup-owned 'management' resource, so a
    /// resolution failure fails the operation.
    RendersSetup,
    /// A Running refresh: it observes the prepared stack and renders no setup, but its
    /// executor still reads the identity (the AWS permissions-boundary check), so it gets it as
    /// it always did. A resolution failure is logged and the refresh runs without it, as before.
    Refresh,
    /// Deletion and teardown remove what is installed; no handler they run reads the identity,
    /// so it isn't resolved and a manager that can't render setup can still delete.
    Unused,
}

impl ManagementIdentityUse {
    /// The use for the operation this manager (or a runner acquiring from it) runs next.
    pub(crate) fn for_runner(status: DeploymentStatus) -> Self {
        match status {
            DeploymentStatus::Pending
            | DeploymentStatus::PreflightsFailed
            | DeploymentStatus::InitialSetup
            | DeploymentStatus::InitialSetupFailed
            | DeploymentStatus::Provisioning
            | DeploymentStatus::WaitingForMachines
            | DeploymentStatus::WaitingForSecrets
            | DeploymentStatus::ProvisioningFailed
            | DeploymentStatus::UpdatePending
            | DeploymentStatus::Updating
            | DeploymentStatus::UpdateFailed => Self::RendersSetup,
            DeploymentStatus::Running | DeploymentStatus::RefreshFailed => Self::Refresh,
            DeploymentStatus::DeletePending
            | DeploymentStatus::Deleting
            | DeploymentStatus::DeleteFailed
            | DeploymentStatus::TeardownRequired
            | DeploymentStatus::TeardownFailed
            | DeploymentStatus::RetentionRequired
            | DeploymentStatus::Retained
            | DeploymentStatus::Deleted
            | DeploymentStatus::Error => Self::Unused,
        }
    }

    /// The use for a target delivered to a pull Operator (sync or air-gapped bundle). An
    /// Operator that is Running or RefreshFailed turns a delivered target into an update
    /// (`accept_target_release` in alien-operator's sync loop) and keeps its config for later
    /// refreshes, so such a target renders setup like any update.
    pub(crate) fn for_pull_target(status: DeploymentStatus) -> Self {
        match Self::for_runner(status) {
            Self::Refresh => Self::RendersSetup,
            other => other,
        }
    }
}

/// The management identity a deployment's stack renders with: the deployment record's own (set
/// by a control plane, e.g. for private managers), else the one the credential resolver derives
/// from `platform`'s management binding. How a resolution failure is handled follows `use_`
/// (see [`ManagementIdentityUse`]). A stack with no setup-owned management resource, e.g. an
/// Operator's in an existing cluster, renders the same without the identity, so for it a failure
/// is logged and the render goes on without it, as before. Which stacks have one is
/// [`stack_renders_management_identity`], the predicate the RemoteStackManagement mutation uses.
pub(crate) async fn resolve_deployment_management_config(
    credential_resolver: &dyn CredentialResolver,
    deployment: &DeploymentRecord,
    platform: Platform,
    use_: ManagementIdentityUse,
) -> Result<Option<ManagementConfig>, AlienError> {
    if let Some(config) = &deployment.management_config {
        return Ok(Some(config.clone()));
    }
    if use_ == ManagementIdentityUse::Unused {
        return Ok(None);
    }
    let resolved = credential_resolver
        .resolve_management_config(platform)
        .await
        .context(GenericError {
            message: format!(
                "Failed to resolve the {platform} management identity for deployment {}",
                deployment.id
            ),
        });
    match resolved {
        Err(error) if use_ == ManagementIdentityUse::Refresh => {
            warn!(
                deployment_id = %deployment.id,
                %error,
                "Refreshing without the management identity; the refresh renders no setup"
            );
            Ok(None)
        }
        Err(error)
            if deployment.stack_settings.as_ref().is_some_and(|settings| {
                !stack_renders_management_identity(
                    deployment.platform,
                    deployment.base_platform,
                    settings,
                )
            }) =>
        {
            warn!(
                deployment_id = %deployment.id,
                %error,
                "Rendering without the management identity, which this deployment's stack doesn't use"
            );
            Ok(None)
        }
        resolved => resolved,
    }
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
    /// `management_config` comes from [`resolve_deployment_management_config`].
    pub(crate) async fn deployment_config(
        &self,
        deployment: &DeploymentRecord,
        deployment_stack: Option<&Stack>,
        management_config: Option<ManagementConfig>,
    ) -> Result<DeploymentConfig, AlienError> {
        let environment_variables =
            deployment_environment_variables(self.config, deployment, deployment_stack)?;
        let monitoring = deployment
            .deployment_config
            .as_ref()
            .and_then(|config| config.monitoring.clone())
            .or_else(|| build_monitoring_config(self.config, deployment));

        let native_image_host = crate::registry_access::derive_native_image_host(
            self.bindings_provider,
            self.target_bindings_providers,
            &deployment.platform,
        )
        .await;

        let gcp_federation_scope = (deployment.base_platform.unwrap_or(deployment.platform)
            == alien_core::Platform::Gcp)
            .then(|| deployment.gcp_federation_scope());
        let config = if let Some(mut config) = deployment.deployment_config.clone() {
            // Stored installer configuration cannot choose the broker's authorization namespace.
            config.gcp_federation_scope = gcp_federation_scope;
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
                gcp_federation_scope,
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

/// Newly granted GCP target-SA permissions can take minutes to propagate after setup.
/// Limit this exception to the first provisioning and the target impersonation hop.
fn is_initial_gcp_iam_propagation(deployment: &DeploymentRecord, error: &AlienError) -> bool {
    if deployment.platform != Platform::Gcp
        || parse_status(&deployment.status) != DeploymentStatus::Provisioning
        || deployment.current_release_id.is_some()
    {
        return false;
    }
    let target_email = deployment.stack_state.as_ref().and_then(|stack| {
        stack
            .resources
            .values()
            .filter(|resource| resource.resource_type == "remote-stack-management")
            .find_map(|resource| {
                resource
                    .outputs
                    .as_ref()?
                    .downcast_ref::<alien_core::RemoteStackManagementOutputs>()
            })
            // The email, under both protocols; a federated access configuration is a descriptor.
            .map(|outputs| outputs.management_resource_id.as_str())
    });
    let Some(target_email) = target_email else {
        return false;
    };
    let target_operation = format!("{target_email}:generateAccessToken");
    let mut handoff = false;
    let mut cause = Some(error);
    while let Some(error) = cause {
        handoff |= error.code == "REMOTE_CREDENTIAL_HANDOFF_FAILED";
        if handoff && error.code == "HTTP_RESPONSE_ERROR" {
            if let Some(context) = error.context.as_ref() {
                let is_target_request = context
                    .get("url")
                    .and_then(serde_json::Value::as_str)
                    .and_then(|url| url::Url::parse(url).ok())
                    .is_some_and(|url| {
                        url.path().rsplit('/').next().is_some_and(|operation| {
                            urlencoding::decode(operation)
                                .is_ok_and(|operation| operation == target_operation)
                        })
                    });
                if context
                    .get("http_status")
                    .and_then(serde_json::Value::as_u64)
                    == Some(403)
                    && is_target_request
                {
                    return true;
                }
            }
        }
        cause = error.source.as_deref();
    }
    false
}

fn credential_error_retry_remaining(
    deployment: &DeploymentRecord,
    error: &AlienError,
    now: chrono::DateTime<chrono::Utc>,
) -> Option<Duration> {
    if parse_status(&deployment.status).is_failed()
        || (!error.retryable && !is_initial_gcp_iam_propagation(deployment, error))
    {
        return None;
    }
    credential_retry_remaining(deployment, now)
}

fn credential_retry_remaining(
    deployment: &DeploymentRecord,
    now: chrono::DateTime<chrono::Utc>,
) -> Option<Duration> {
    let started_at = deployment
        .runtime_metadata
        .as_ref()
        .and_then(|metadata| metadata.credential_retry.as_ref())
        .filter(|retry| retry.status == parse_status(&deployment.status))
        .map_or(now, |retry| retry.started_at);
    let elapsed = now
        .signed_duration_since(started_at)
        .to_std()
        .unwrap_or(Duration::ZERO);
    CREDENTIAL_ERROR_GRACE_PERIOD
        .checked_sub(elapsed)
        .filter(|remaining| !remaining.is_zero())
}

pub(crate) fn failed_state_for_phase_error(
    deployment: &DeploymentRecord,
    status: DeploymentStatus,
    deployment_stack: Option<&alien_core::Stack>,
    target_release_id: Option<&str>,
    error: AlienError,
) -> DeploymentState {
    let mut state = DeploymentState {
        status: failed_status_for_deployment_error(status),
        error: Some(error),
        retry_requested: false,
        ..state_from_record(deployment, status, deployment_stack, target_release_id)
    };
    if let Some(metadata) = state.runtime_metadata.as_mut() {
        metadata.credential_retry = None;
    }
    state
}

fn state_from_record(
    deployment: &DeploymentRecord,
    status: DeploymentStatus,
    deployment_stack: Option<&alien_core::Stack>,
    target_release_id: Option<&str>,
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
            release_id: target_release_id.map(str::to_owned),
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
        credential_error_retry_remaining, get_or_create_local_bindings_provider,
        has_remote_stack_management_outputs, manager_candidate_statuses,
        needs_provision_capability, parse_status, resolve_deployment_management_config,
        retryable_failed_statuses, should_wait_for_credential_handoff, supplied_release_stacks,
        worker_commands_push_env_vars, DeploymentLoop, ManagementIdentityUse,
        ManagementIdentityUse::{Refresh, RendersSetup, Unused},
        ProcessOptions, RunnerConfigSources, CREDENTIAL_ERROR_GRACE_PERIOD,
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
    use alien_error::{AlienError, GenericError};
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
            local_queues: Arc::new(alien_local::LocalQueueService::default()),
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
    async fn deferred_deployment_is_not_reacquired_every_tick() {
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
                local_queues: Arc::new(alien_local::LocalQueueService::default()),
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
                .deferred
                .lock()
                .expect("deferred poisoned")
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
            DeploymentStatus::RetentionRequired => "retention-required",
            DeploymentStatus::Retained => "retained",
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

    #[cfg(feature = "sqlite")]
    #[test]
    fn sqlite_store_can_acquire_every_active_work_status() {
        for status in active_work_statuses() {
            assert!(
                SqliteDeploymentStore::WORK_STATUSES.contains(&status.as_str()),
                "The deployment loop polls {status}, so SQLite must acquire it without a retry request"
            );
        }
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

    #[cfg(feature = "sqlite")]
    mod credential_retry_tests {
        use super::*;
        use alien_core::Storage;
        use alien_infra::{state_utils::StackResourceStateExt, LocalStorageController};
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

        struct Credentials {
            unavailable: AtomicBool,
            visits: AtomicUsize,
            error: AlienError,
            state_directory: String,
            /// When set, the management identity fails to resolve with this error.
            management_error: Mutex<Option<AlienError>>,
            management_visits: AtomicUsize,
        }

        #[async_trait]
        impl CredentialResolver for Credentials {
            async fn resolve(
                &self,
                _: &DeploymentRecord,
            ) -> Result<alien_core::ClientConfig, AlienError> {
                self.visits.fetch_add(1, Ordering::SeqCst);
                if self.unavailable.load(Ordering::SeqCst) {
                    Err(self.error.clone())
                } else {
                    Ok(alien_core::ClientConfig::Local {
                        state_directory: self.state_directory.clone(),
                    })
                }
            }

            async fn resolve_management_config(
                &self,
                _: Platform,
            ) -> Result<Option<alien_core::ManagementConfig>, AlienError> {
                self.management_visits.fetch_add(1, Ordering::SeqCst);
                match self.management_error.lock().unwrap().clone() {
                    Some(error) => Err(error),
                    None => Ok(None),
                }
            }
        }

        struct Fixture {
            manager: crate::server::AlienManager,
            db: Arc<SqliteDatabase>,
            credentials: Arc<Credentials>,
            id: String,
            _directory: TempDir,
        }

        impl Fixture {
            async fn new(status: DeploymentStatus, error: AlienError) -> Self {
                Self::new_for_platform(status, error, Platform::Local).await
            }

            async fn new_for_platform(
                status: DeploymentStatus,
                error: AlienError,
                platform: Platform,
            ) -> Self {
                let directory = TempDir::new().unwrap();
                let db_path = directory.path().join("manager.db");
                let credentials = Arc::new(Credentials {
                    unavailable: AtomicBool::new(true),
                    visits: AtomicUsize::new(0),
                    error,
                    state_directory: directory.path().to_string_lossy().to_string(),
                    management_error: Mutex::new(None),
                    management_visits: AtomicUsize::new(0),
                });
                let manager = crate::server::AlienManager::builder(crate::config::ManagerConfig {
                    db_path: Some(db_path.clone()),
                    state_dir: Some(directory.path().to_path_buf()),
                    ..Default::default()
                })
                .credential_resolver(credentials.clone())
                .with_standalone_defaults(&crate::standalone_config::ManagerTomlConfig::default())
                .await
                .unwrap()
                .build()
                .await
                .unwrap();
                let db = Arc::new(
                    SqliteDatabase::new(db_path.to_str().unwrap())
                        .await
                        .unwrap(),
                );
                let caller = Subject::system();
                let release = manager
                    .release_store
                    .create_release(
                        &caller,
                        crate::traits::release_store::CreateReleaseParams {
                            project_id: "default".to_string(),
                            stacks: HashMap::from([(
                                platform,
                                Stack::new("test".to_string()).build(),
                            )]),
                            git_commit_sha: None,
                            git_commit_ref: None,
                            git_commit_message: None,
                        },
                    )
                    .await
                    .unwrap();
                let deployment = manager
                    .deployment_store
                    .create_deployment(
                        &caller,
                        CreateDeploymentParams {
                            name: "credential-retry".to_string(),
                            deployment_group_id: "dg_test".to_string(),
                            platform,
                            deployment_protocol_version: alien_core::DEPLOYMENT_PROTOCOL_VERSION,
                            base_platform: None,
                            stack_settings: StackSettings {
                                deployment_model: alien_core::DeploymentModel::Push,
                                ..Default::default()
                            },
                            stack_state: Some(StackState::new(platform)),
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
                    "UPDATE deployments SET status = '{}', current_release_id = '{}', desired_release_id = '{}' WHERE id = '{}'",
                    deployment_status_str(status), release.id, release.id, deployment.id,
                )).await.unwrap();
                let metadata = serde_json::to_string(&alien_core::RuntimeMetadata {
                    prepared_stack: Some(Stack::new("test".to_string()).build()),
                    ..Default::default()
                })
                .unwrap();
                db.execute(&format!(
                    "UPDATE deployments SET runtime_metadata = '{}'",
                    metadata.replace('\'', "''")
                ))
                .await
                .unwrap();
                db.execute("CREATE TABLE row_writes (n INTEGER)")
                    .await
                    .unwrap();
                db.execute("CREATE TRIGGER count_row_writes AFTER UPDATE ON deployments BEGIN INSERT INTO row_writes VALUES (1); END").await.unwrap();
                Self {
                    manager,
                    db,
                    credentials,
                    id: deployment.id,
                    _directory: directory,
                }
            }

            fn runner(&self) -> DeploymentLoop {
                DeploymentLoop::new(
                    self.manager.config.clone(),
                    self.manager.deployment_store.clone(),
                    self.manager.release_store.clone(),
                    self.credentials.clone(),
                    self.manager.server_bindings.clone(),
                    None,
                )
            }

            async fn record(&self) -> DeploymentRecord {
                self.manager
                    .deployment_store
                    .get_deployment(&Subject::system(), &self.id)
                    .await
                    .unwrap()
                    .unwrap()
            }

            async fn process(&self, runner: &DeploymentLoop) {
                let acquired = self
                    .manager
                    .deployment_store
                    .acquire(
                        &Subject::system(),
                        "test-session",
                        &DeploymentFilter {
                            deployment_ids: Some(vec![self.id.clone()]),
                            statuses: Some(vec![self.record().await.status, "running".to_string()]),
                            ..Default::default()
                        },
                        1,
                    )
                    .await
                    .unwrap();
                assert_eq!(acquired.len(), 1);
                let mut item = acquired.into_iter().next().unwrap();
                // Some adapters synthesize record timestamps on every acquisition.
                // Only the deployment state itself is a durable clock for retries.
                item.deployment.created_at = Utc::now();
                item.deployment.updated_at = None;
                runner
                    .process_deployment(
                        item.deployment,
                        item.execution_claim,
                        "test-session",
                        ProcessOptions {
                            max_steps: 1,
                            require_heartbeats_enabled: matches!(
                                self.record().await.status.as_str(),
                                "running" | "refresh-failed"
                            ),
                        },
                    )
                    .await;
            }

            async fn writes(&self) -> i64 {
                let conn = self.db.conn().lock().await;
                let mut rows = conn
                    .query("SELECT count(*) FROM row_writes", ())
                    .await
                    .unwrap();
                rows.next().await.unwrap().unwrap().get::<i64>(0).unwrap()
            }
        }

        fn transient_error() -> AlienError {
            AlienError::new(alien_gcp_clients::ErrorData::HttpResponseError {
                message: "External account token exchange failed with status 503".to_string(),
                url: "http://localhost/token".to_string(),
                http_status: 503,
                http_request_text: None,
                http_response_text: None,
            })
            .into_generic()
        }

        /// An Azure manager missing its management configuration, with working cloud credentials.
        fn without_azure_management(fixture: &Fixture) -> AlienError {
            let error = AlienError::new(GenericError {
                message: "[impersonation.azure] needs AZURE_MANAGEMENT_OIDC_ISSUER in the manager's environment".to_string(),
            });
            fixture
                .credentials
                .unavailable
                .store(false, Ordering::SeqCst);
            *fixture.credentials.management_error.lock().unwrap() = Some(error.clone());
            error
        }

        /// An update that can't render without the management identity ends failed with the
        /// cause saved, and the loop leaves it alone until someone retries.
        #[tokio::test]
        async fn an_unresolvable_management_identity_fails_the_update_with_its_cause() {
            let fixture = Fixture::new_for_platform(
                DeploymentStatus::UpdatePending,
                transient_error(),
                Platform::Azure,
            )
            .await;
            let cause = without_azure_management(&fixture);
            let runner = fixture.runner();

            runner.tick().await;
            let record = fixture.record().await;
            assert_eq!(record.status, "update-failed");
            assert!(!record.retry_requested);
            let saved: AlienError = serde_json::from_value(record.error.expect("saved error"))
                .expect("saved error decodes");
            assert_eq!(
                saved.message,
                format!(
                    "Failed to resolve the azure management identity for deployment {}",
                    fixture.id
                )
            );
            assert_eq!(saved.source.expect("resolver error").message, cause.message);
            assert_eq!(
                fixture.credentials.management_visits.load(Ordering::SeqCst),
                1
            );

            // Not re-picked: no lease writes, no further resolution.
            let writes = fixture.writes().await;
            for _ in 0..5 {
                runner.tick().await;
            }
            assert_eq!(fixture.writes().await, writes);
            assert_eq!(
                fixture.credentials.management_visits.load(Ordering::SeqCst),
                1
            );

            // A retry renders again, and fails again with the same saved cause.
            fixture
                .manager
                .deployment_store
                .set_retry_requested(&Subject::system(), &fixture.id)
                .await
                .unwrap();
            runner.tick().await;
            let record = fixture.record().await;
            assert_eq!(
                fixture.credentials.management_visits.load(Ordering::SeqCst),
                2
            );
            assert_eq!(record.status, "update-failed");
            assert!(!record.retry_requested);
            let saved: AlienError =
                serde_json::from_value(record.error.expect("saved error")).unwrap();
            assert_eq!(saved.source.expect("resolver error").message, cause.message);
        }

        /// Deletion removes what is installed and never renders setup, so it needs no
        /// management identity.
        #[tokio::test]
        async fn an_azure_deployment_deletes_without_its_management_identity() {
            let fixture = Fixture::new_for_platform(
                DeploymentStatus::DeletePending,
                transient_error(),
                Platform::Azure,
            )
            .await;
            without_azure_management(&fixture);

            fixture.runner().tick().await;

            assert!(
                fixture
                    .manager
                    .deployment_store
                    .get_deployment(&Subject::system(), &fixture.id)
                    .await
                    .unwrap()
                    .is_none(),
                "the deleted deployment's record is removed"
            );
            assert_eq!(
                fixture.credentials.management_visits.load(Ordering::SeqCst),
                0
            );
        }

        #[tokio::test]
        async fn unsupported_exclusions_preserve_batches_without_reacquiring_deferred_work() {
            for status in [
                DeploymentStatus::UpdatePending,
                DeploymentStatus::Provisioning,
            ] {
                let fixture = Fixture::new(status, transient_error()).await;
                let record = fixture.record().await;
                let records = Arc::new(Mutex::new(
                    (0..8)
                        .map(|index| {
                            let mut record = record.clone();
                            record.id = format!("candidate-{index}");
                            (record.id.clone(), record)
                        })
                        .collect::<HashMap<_, _>>(),
                ));
                let mut store = crate::traits::deployment_store::MockDeploymentStore::new();
                store
                    .expect_supports_acquire_exclusions()
                    .return_const(false);
                let available = records.clone();
                store
                    .expect_acquire()
                    .times(2)
                    .returning(move |_, _, filter, limit| {
                        assert!(filter.excluded_deployment_ids.is_empty());
                        Ok(available
                            .lock()
                            .unwrap()
                            .values()
                            .filter(|record| {
                                filter
                                    .deployment_ids
                                    .as_ref()
                                    .is_none_or(|ids| ids.contains(&record.id))
                            })
                            .take(limit as usize)
                            .map(
                                |record| crate::traits::deployment_store::AcquiredDeployment {
                                    deployment: record.clone(),
                                    execution_claim: None,
                                },
                            )
                            .collect())
                    });
                let listed = records.clone();
                store
                    .expect_list_deployment_ids()
                    .times(1)
                    .returning(move |_, _| Ok(listed.lock().unwrap().keys().cloned().collect()));
                let saved = records.clone();
                store.expect_reconcile().times(8).returning(move |_, data| {
                    let mut records = saved.lock().unwrap();
                    let record = records.get_mut(&data.deployment_id).unwrap();
                    record.runtime_metadata = data.state.runtime_metadata;
                    Ok(crate::traits::deployment_store::ReconcileOutcome {
                        record: record.clone(),
                        target_operations_bundle_set: None,
                        target_dynamic_containers: None,
                    })
                });
                store
                    .expect_get_deployment()
                    .returning(move |_, id| Ok(records.lock().unwrap().get(id).cloned()));
                store
                    .expect_release()
                    .times(8)
                    .returning(|_, _, _, _| Ok(()));
                let store = Arc::new(store);
                let mut runner = fixture.runner();
                runner.deployment_store = store.clone();
                runner.tick().await;
                assert_eq!(fixture.credentials.visits.load(Ordering::SeqCst), 8);
            }
        }

        fn defer_pending(runner: &DeploymentLoop) {
            runner.deferred.lock().unwrap().insert(
                "deferred-pending".to_string(),
                super::super::DeferredDeployment {
                    status: DeploymentStatus::Pending,
                    updated_at: None,
                    retry_requested: false,
                    recheck_after: Utc::now() + chrono::Duration::minutes(5),
                },
            );
        }

        /// A store that decodes whole records fails its list when one record holds stack
        /// state the current types reject. The candidate snapshot needs only IDs, so the
        /// other candidates must still be acquired and processed.
        #[tokio::test]
        async fn undecodable_candidate_does_not_stop_other_deployments() {
            let fixture = Fixture::new(DeploymentStatus::UpdatePending, transient_error()).await;
            let good = fixture.record().await;
            let good_id = good.id.clone();
            let mut store = crate::traits::deployment_store::MockDeploymentStore::new();
            store
                .expect_supports_acquire_exclusions()
                .return_const(false);
            store.expect_list_deployments().returning(|_, _| {
                Err(AlienError::new(alien_error::GenericError {
                    message: "JSON deserialize failed: unknown field `ingress`".to_string(),
                }))
            });
            let listed = vec![
                "deferred-pending".to_string(),
                "undecodable".to_string(),
                good_id.clone(),
            ];
            store
                .expect_list_deployment_ids()
                .times(1)
                .returning(move |_, _| Ok(listed.clone()));
            let acquirable = Mutex::new(Some(good.clone()));
            store.expect_acquire().returning(move |_, _, filter, _| {
                let ids = filter
                    .deployment_ids
                    .as_ref()
                    .expect("acquisition is narrowed to the candidate snapshot");
                assert!(!ids.contains(&"deferred-pending".to_string()));
                // A store releases a record it cannot decode instead of returning it, so only
                // the decodable candidate comes back, once.
                Ok(acquirable
                    .lock()
                    .unwrap()
                    .take()
                    .filter(|record| ids.contains(&record.id))
                    .map(
                        |record| crate::traits::deployment_store::AcquiredDeployment {
                            deployment: record,
                            execution_claim: None,
                        },
                    )
                    .into_iter()
                    .collect())
            });
            let saved = Arc::new(Mutex::new(good));
            let reconciled = saved.clone();
            store.expect_reconcile().times(1).returning(move |_, data| {
                let mut record = reconciled.lock().unwrap();
                assert_eq!(data.deployment_id, record.id);
                record.runtime_metadata = data.state.runtime_metadata;
                Ok(crate::traits::deployment_store::ReconcileOutcome {
                    record: record.clone(),
                    target_operations_bundle_set: None,
                    target_dynamic_containers: None,
                })
            });
            // The deferred deployment is unchanged, so it stays in backoff.
            store.expect_get_deployment().returning(move |_, id| {
                let mut record = saved.lock().unwrap().clone();
                if id == "deferred-pending" {
                    record.id = id.to_string();
                    record.status = "pending".to_string();
                    record.updated_at = None;
                    record.retry_requested = false;
                }
                Ok(Some(record).filter(|record| record.id == id))
            });
            store
                .expect_release()
                .times(1)
                .returning(|_, _, _, _| Ok(()));
            let mut runner = fixture.runner();
            runner.deployment_store = Arc::new(store);
            defer_pending(&runner);

            runner.tick().await;

            assert_eq!(
                fixture.credentials.visits.load(Ordering::SeqCst),
                1,
                "the decodable candidate is processed"
            );
        }

        #[tokio::test]
        async fn unrelated_backoff_does_not_snapshot_unsupported_deployment_candidates() {
            let fixture = Fixture::new(DeploymentStatus::UpdatePending, transient_error()).await;
            let mut runner = fixture.runner();
            defer_pending(&runner);
            let mut store = crate::traits::deployment_store::MockDeploymentStore::new();
            store
                .expect_supports_acquire_exclusions()
                .return_const(false);
            store.expect_list_deployments().times(0);
            runner.deployment_store = Arc::new(store);
            let mut filter = DeploymentFilter {
                statuses: Some(vec!["update-pending".to_string()]),
                ..Default::default()
            };
            assert!(runner
                .exclude_deferred(&Subject::system(), &mut filter, true)
                .await
                .unwrap());
            assert!(filter.deployment_ids.is_none());
        }

        #[tokio::test]
        async fn unsupported_heartbeats_reach_all_candidates_beyond_the_listing_cap() {
            let fixture = Fixture::new(DeploymentStatus::Running, transient_error()).await;
            let record = fixture.record().await;
            let records = Arc::new(Mutex::new(
                (0..150)
                    .map(|index| {
                        let mut record = record.clone();
                        record.id = format!("candidate-{index:03}");
                        (record.id.clone(), record)
                    })
                    .collect::<std::collections::BTreeMap<_, _>>(),
            ));
            let queue = Arc::new(Mutex::new(
                records.lock().unwrap().keys().cloned().collect::<Vec<_>>(),
            ));
            let listed_count = Arc::new(AtomicUsize::new(0));
            let released = Arc::new(Mutex::new(std::collections::HashSet::new()));
            let mut store = crate::traits::deployment_store::MockDeploymentStore::new();
            store
                .expect_supports_acquire_exclusions()
                .return_const(false);
            let available = records.clone();
            store
                .expect_acquire()
                .returning(move |_, _, filter, limit| {
                    let mut queue = queue.lock().unwrap();
                    let ids: Vec<_> = queue
                        .iter()
                        .filter(|id| {
                            filter
                                .deployment_ids
                                .as_ref()
                                .is_none_or(|ids| ids.contains(id))
                        })
                        .take(limit as usize)
                        .cloned()
                        .collect();
                    queue.retain(|id| !ids.contains(id));
                    let available = available.lock().unwrap();
                    Ok(ids
                        .into_iter()
                        .map(|id| crate::traits::deployment_store::AcquiredDeployment {
                            deployment: available[&id].clone(),
                            execution_claim: None,
                        })
                        .collect())
                });
            let listed = records.clone();
            let calls = listed_count.clone();
            store.expect_list_deployments().returning(move |_, _| {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(listed.lock().unwrap().values().take(100).cloned().collect())
            });
            let saved = records.clone();
            store.expect_reconcile().returning(move |_, data| {
                let mut records = saved.lock().unwrap();
                let record = records.get_mut(&data.deployment_id).unwrap();
                record.runtime_metadata = data.state.runtime_metadata;
                Ok(crate::traits::deployment_store::ReconcileOutcome {
                    record: record.clone(),
                    target_operations_bundle_set: None,
                    target_dynamic_containers: None,
                })
            });
            store
                .expect_get_deployment()
                .returning(move |_, id| Ok(records.lock().unwrap().get(id).cloned()));
            let completed = released.clone();
            store.expect_release().returning(move |_, id, _, _| {
                completed.lock().unwrap().insert(id.to_string());
                Ok(())
            });
            let store = Arc::new(store);
            let mut runner = fixture.runner();
            runner.deployment_store = store.clone();
            // A CLI-owned pending deployment parked by the deployment loop must
            // not make heartbeats enumerate a capped list of running deployments.
            defer_pending(&runner);
            let heartbeat = crate::loops::heartbeat::HeartbeatLoop::new(
                fixture.manager.config.clone(),
                store,
                Arc::new(runner),
            );
            heartbeat.tick().await;
            assert_eq!(
                released.lock().unwrap().len(),
                100,
                "preserve the existing per-tick budget"
            );
            heartbeat.tick().await;
            assert_eq!(
                released.lock().unwrap().len(),
                150,
                "newer candidates must also receive heartbeats"
            );
            assert_eq!(fixture.credentials.visits.load(Ordering::SeqCst), 150);
            assert_eq!(
                listed_count.load(Ordering::SeqCst),
                0,
                "unsupported heartbeat stores retain direct acquisition"
            );
        }

        #[tokio::test]
        async fn first_gcp_provisioning_waits_for_target_iam_but_other_phases_fail_fast() {
            let error = gcp_target_iam_denial().await;
            for (status, installed) in [
                (DeploymentStatus::Provisioning, false),
                (DeploymentStatus::Provisioning, true),
                (DeploymentStatus::UpdatePending, false),
                (DeploymentStatus::Running, true),
            ] {
                let fixture = Fixture::new_for_platform(status, error.clone(), Platform::Gcp).await;
                let stack = gcp_stack_with_target_identity();
                let encoded = serde_json::to_string(&stack).unwrap().replace('\'', "''");
                fixture
                    .db
                    .execute(&format!(
                        "UPDATE deployments SET stack_state = '{encoded}'{}",
                        if installed {
                            ""
                        } else {
                            ", current_release_id = NULL"
                        }
                    ))
                    .await
                    .unwrap();
                fixture.process(&fixture.runner()).await;
                if status == DeploymentStatus::Provisioning && !installed {
                    assert_eq!(fixture.record().await.status, "provisioning");
                    for minutes in [8, 11] {
                        let mut metadata = fixture.record().await.runtime_metadata.unwrap();
                        metadata.credential_retry.as_mut().unwrap().started_at =
                            Utc::now() - chrono::Duration::minutes(minutes);
                        let encoded = serde_json::to_string(&metadata)
                            .unwrap()
                            .replace('\'', "''");
                        fixture
                            .db
                            .execute(&format!(
                                "UPDATE deployments SET runtime_metadata = '{encoded}'"
                            ))
                            .await
                            .unwrap();
                        fixture.process(&fixture.runner()).await;
                        assert_eq!(
                            fixture.record().await.status,
                            if minutes == 8 {
                                "provisioning"
                            } else {
                                "provisioning-failed"
                            }
                        );
                    }
                } else {
                    assert!(parse_status(&fixture.record().await.status).is_failed());
                }
                assert_eq!(
                    fixture.record().await.error.unwrap(),
                    serde_json::to_value(&error).unwrap()
                );
            }
        }

        #[tokio::test]
        async fn azure_token_endpoint_failures_reach_the_real_loop() {
            use alien_azure_clients::AzureClientConfigExt;
            let server = httpmock::MockServer::start_async().await;
            let mut config = alien_core::AzureClientConfig {
                subscription_id: "synthetic-subscription".to_string(),
                tenant_id: "synthetic-tenant".to_string(),
                region: None,
                service_overrides: None,
                credentials: alien_core::AzureCredentials::ManagedIdentity {
                    client_id: "synthetic-client".to_string(),
                    identity_endpoint: server.url("/token"),
                    identity_header: "synthetic-header".to_string(),
                },
            };
            for (status, retryable) in [
                (400, false),
                (401, false),
                (403, false),
                (429, true),
                (503, true),
                (0, true),
            ] {
                let response = server
                    .mock_async(|when, then| {
                        when.method(httpmock::Method::GET).path("/token");
                        then.status(if status == 0 { 503 } else { status })
                            .json_body(serde_json::json!({"error": "synthetic rejection"}));
                    })
                    .await;
                if status == 0 {
                    config.credentials = alien_core::AzureCredentials::ManagedIdentity {
                        client_id: "synthetic-client".to_string(),
                        identity_endpoint: "http://127.0.0.1:0/token".to_string(),
                        identity_header: "synthetic-header".to_string(),
                    };
                }
                let error = config.get_bearer_token().await.unwrap_err().into_generic();
                assert_eq!(error.code, "AUTHENTICATION_ERROR");
                let fixture = Fixture::new_for_platform(
                    DeploymentStatus::UpdatePending,
                    error.clone(),
                    Platform::Azure,
                )
                .await;
                fixture.process(&fixture.runner()).await;
                let record = fixture.record().await;
                assert_eq!(
                    record.status,
                    if retryable {
                        "update-pending"
                    } else {
                        "update-failed"
                    },
                    "HTTP {status}: {error}"
                );
                assert_eq!(
                    record.runtime_metadata.unwrap().credential_retry.is_some(),
                    retryable
                );
                if status != 0 {
                    response.assert_hits_async(1).await;
                }
                response.delete_async().await;
            }
        }

        /// A control plane ends a heartbeat's execution claim on the first
        /// checkpoint that settles it (Running or RefreshFailed) and refuses any
        /// later checkpoint under that claim. Recovering from a credential retry
        /// must therefore persist the cleared window in that one checkpoint.
        #[tokio::test]
        async fn credential_recovery_checkpoints_a_heartbeat_claim_once() {
            for (status, options, steps) in [
                (
                    DeploymentStatus::Running,
                    ProcessOptions::heartbeat_tick(),
                    true,
                ),
                (
                    DeploymentStatus::RefreshFailed,
                    ProcessOptions::heartbeat_tick(),
                    true,
                ),
                (
                    DeploymentStatus::Running,
                    ProcessOptions::deployment_tick(),
                    false,
                ),
            ] {
                let fixture = Fixture::new(status, transient_error()).await;
                fixture
                    .credentials
                    .unavailable
                    .store(false, Ordering::SeqCst);
                let mut record = fixture.record().await;
                record.runtime_metadata.as_mut().unwrap().credential_retry =
                    Some(alien_core::CredentialRetry {
                        started_at: Utc::now() - chrono::Duration::minutes(1),
                        status,
                    });
                let claim = crate::traits::deployment_store::ExecutionClaim {
                    operation_id: "operation_test".to_string(),
                    attempt_id: "attempt_test".to_string(),
                };
                let accepted = Arc::new(Mutex::new(Vec::<
                    crate::traits::deployment_store::ReconcileData,
                >::new()));
                let refused = Arc::new(AtomicUsize::new(0));
                let claim_live = Arc::new(AtomicBool::new(true));
                let mut store = crate::traits::deployment_store::MockDeploymentStore::new();
                let returned = record.clone();
                let (saved, rejected, live) =
                    (accepted.clone(), refused.clone(), claim_live.clone());
                store.expect_reconcile().returning(move |_, data| {
                    assert_eq!(
                        data.execution_claim
                            .as_ref()
                            .map(|claim| claim.attempt_id.as_str()),
                        Some("attempt_test")
                    );
                    if !live.load(Ordering::SeqCst) {
                        rejected.fetch_add(1, Ordering::SeqCst);
                        let mut error = AlienError::new(alien_error::GenericError {
                            message: "The execution attempt is no longer live.".to_string(),
                        });
                        error.code = "DEPLOYMENT_UPDATE_CLAIM_LOST".to_string();
                        error.retryable = false;
                        return Err(error);
                    }
                    if matches!(
                        data.state.status,
                        DeploymentStatus::Running | DeploymentStatus::RefreshFailed
                    ) {
                        live.store(false, Ordering::SeqCst);
                    }
                    let mut record = returned.clone();
                    record.status = deployment_status_str(data.state.status).to_string();
                    record.runtime_metadata = data.state.runtime_metadata.clone();
                    saved.lock().unwrap().push(data);
                    Ok(crate::traits::deployment_store::ReconcileOutcome {
                        record,
                        target_operations_bundle_set: None,
                        target_dynamic_containers: None,
                    })
                });
                let mut runner = fixture.runner();
                runner.deployment_store = Arc::new(store);

                let result = runner
                    .process_deployment_inner(record, Some(claim), "test-session", options)
                    .await;

                assert!(
                    result.is_ok(),
                    "{status:?} recovery failed: {:?}",
                    result.err()
                );
                assert_eq!(
                    refused.load(Ordering::SeqCst),
                    0,
                    "{status:?}: a checkpoint reused a claim the control plane had ended"
                );
                let accepted = accepted.lock().unwrap();
                assert_eq!(accepted.len(), 1, "{status:?}: one checkpoint per claim");
                let checkpoint = &accepted[0];
                assert!(checkpoint
                    .state
                    .runtime_metadata
                    .as_ref()
                    .unwrap()
                    .credential_retry
                    .is_none());
                assert_eq!(
                    checkpoint.update_heartbeat, steps,
                    "{status:?}: the checkpoint must be the step's own when a step runs"
                );
                assert!(
                    !claim_live.load(Ordering::SeqCst),
                    "{status:?}: the single checkpoint must settle the claim"
                );
            }
        }

        #[tokio::test]
        async fn credential_checkpoints_preserve_distinct_release_stacks() {
            for retryable in [true, false] {
                let error = if retryable {
                    transient_error()
                } else {
                    AlienError::new(alien_error::GenericError {
                        message: "missing credentials".to_string(),
                    })
                };
                let fixture = Fixture::new(DeploymentStatus::UpdatePending, error).await;
                let mut record = fixture.record().await;
                let installed = stack_with_workers(&["installed"]);
                let target = stack_with_workers(&["replacement"]);
                record.supplied_stacks = Some(SuppliedStacks {
                    current: Some(installed.clone()),
                    desired: Some(target.clone()),
                });
                record.current_release_id = Some("old-release".to_string());
                record.desired_release_id = Some("new-release".to_string());
                let mut returned = record.clone();
                let mut store = crate::traits::deployment_store::MockDeploymentStore::new();
                store
                    .expect_reconcile()
                    .times(1)
                    .return_once(move |_, data| {
                        let current = data.state.current_release.as_ref().unwrap();
                        let desired = data.state.target_release.as_ref().unwrap();
                        assert_eq!(current.release_id.as_deref(), Some("old-release"));
                        assert_eq!(desired.release_id.as_deref(), Some("new-release"));
                        assert_eq!(current.stack, installed);
                        assert_eq!(desired.stack, target);
                        assert_eq!(
                            data.state.status,
                            if retryable {
                                DeploymentStatus::UpdatePending
                            } else {
                                DeploymentStatus::UpdateFailed
                            }
                        );
                        returned.runtime_metadata = data.state.runtime_metadata;
                        Ok(crate::traits::deployment_store::ReconcileOutcome {
                            record: returned,
                            target_operations_bundle_set: None,
                            target_dynamic_containers: None,
                        })
                    });
                let mut runner = fixture.runner();
                runner.deployment_store = Arc::new(store);
                runner
                    .process_deployment_inner(
                        record,
                        None,
                        "test-session",
                        ProcessOptions::deployment_tick(),
                    )
                    .await
                    .unwrap();
            }
        }

        async fn retries_then_recovers(status: DeploymentStatus) {
            let fixture = Fixture::new(status, transient_error()).await;
            let before = fixture.record().await;
            fixture.process(&fixture.runner()).await;
            let waiting = fixture.record().await;
            assert_eq!(
                waiting.status, before.status,
                "transient errors must retain the operation"
            );
            assert!(waiting.error.is_none());
            let retry = waiting
                .runtime_metadata
                .as_ref()
                .unwrap()
                .credential_retry
                .as_ref()
                .unwrap();
            assert_eq!(retry.status, status);
            assert!(retry.started_at <= Utc::now());
            let writes = fixture.writes().await;
            fixture.process(&fixture.runner()).await;
            let still_waiting = fixture.record().await;
            assert_eq!(still_waiting.runtime_metadata, waiting.runtime_metadata);
            assert_eq!(still_waiting.updated_at, waiting.updated_at);
            assert_eq!(
                fixture.writes().await - writes,
                2,
                "another replica only acquires and releases"
            );
            assert!(waiting.locked_by.is_none());
            fixture
                .credentials
                .unavailable
                .store(false, Ordering::SeqCst);
            // A new loop instance models a manager restart or another replica.
            fixture.process(&fixture.runner()).await;
            let recovered = fixture.record().await;
            assert!(
                recovered.error.is_none(),
                "recovery error: {:?}",
                recovered.error
            );
            assert!(!parse_status(&recovered.status).is_failed());
            assert_ne!(
                recovered.updated_at, before.updated_at,
                "resolved credentials must reach the runner checkpoint"
            );
            assert!(recovered
                .runtime_metadata
                .unwrap()
                .credential_retry
                .is_none());
            assert_eq!(fixture.credentials.visits.load(Ordering::SeqCst), 3);
        }

        #[tokio::test]
        async fn update_pending_retries_then_recovers() {
            retries_then_recovers(DeploymentStatus::UpdatePending).await;
        }
        #[tokio::test]
        async fn erase_after_retention_does_not_require_a_release_reference() {
            for status in [
                DeploymentStatus::DeletePending,
                DeploymentStatus::Deleting,
                DeploymentStatus::DeleteFailed,
            ] {
                let fixture = Fixture::new(status, transient_error()).await;
                fixture
                    .credentials
                    .unavailable
                    .store(false, Ordering::SeqCst);
                let storage_path = fixture
                    ._directory
                    .path()
                    .join(&fixture.id)
                    .join("storage/data");
                tokio::fs::create_dir_all(&storage_path).await.unwrap();
                let object = storage_path.join("retained.txt");
                tokio::fs::write(&object, b"retained object").await.unwrap();
                let controller: LocalStorageController =
                    serde_json::from_value(serde_json::json!({
                        "state": "ready", "storagePath": storage_path,
                    }))
                    .unwrap();
                let mut resource = StackResourceState::builder()
                    .resource_type("storage".into())
                    .status(ResourceStatus::Running)
                    .config(Resource::new(Storage::new("data".into()).build()))
                    .lifecycle(ResourceLifecycle::Frozen)
                    .dependencies(vec![])
                    .build();
                resource
                    .set_internal_controller(Some(Box::new(controller)))
                    .unwrap();
                let mut saved = fixture.record().await.stack_state.unwrap();
                saved.resources.insert("data".into(), resource);
                let encoded = serde_json::to_string(&saved).unwrap().replace('\'', "''");
                fixture
                    .db
                    .execute(&format!("UPDATE deployments SET stack_state = '{encoded}'"))
                    .await
                    .unwrap();
                fixture.db.execute("UPDATE deployments SET current_release_id = NULL, desired_release_id = NULL, retry_requested = 1, runtime_metadata = json_set(runtime_metadata, '$.deletionPolicy', 'erase')").await.unwrap();
                assert_eq!(tokio::fs::read(&object).await.unwrap(), b"retained object");
                let runner = fixture.runner();
                for _ in 0..4 {
                    runner.tick().await;
                    if fixture
                        .manager
                        .deployment_store
                        .get_deployment(&Subject::system(), &fixture.id)
                        .await
                        .unwrap()
                        .is_none()
                    {
                        assert!(
                            !storage_path.exists(),
                            "record must not disappear before its saved storage controller erases data"
                        );
                    }
                }
                assert!(
                    !object.exists(),
                    "retained object must be erased ({status:?})"
                );
                assert!(
                    !storage_path.exists(),
                    "storage directory must be erased ({status:?})"
                );
                assert!(
                    fixture
                        .manager
                        .deployment_store
                        .get_deployment(&Subject::system(), &fixture.id)
                        .await
                        .unwrap()
                        .is_none(),
                    "saved deletion must finish even after retention cleared releases ({status:?})"
                );
                assert!(fixture.credentials.visits.load(Ordering::SeqCst) > 0);
            }
        }

        #[tokio::test]
        async fn delete_pending_retries_then_recovers() {
            retries_then_recovers(DeploymentStatus::DeletePending).await;
        }
        #[tokio::test]
        async fn provisioning_retries_then_recovers() {
            retries_then_recovers(DeploymentStatus::Provisioning).await;
        }
        #[tokio::test]
        async fn running_retries_then_recovers() {
            retries_then_recovers(DeploymentStatus::Running).await;
        }

        #[tokio::test]
        async fn permanent_error_fails_immediately_with_real_error() {
            let error = AlienError::new(alien_gcp_clients::ErrorData::InvalidClientConfig {
                message: "management credentials are missing".to_string(),
                errors: None,
            })
            .into_generic();
            assert!(!error.retryable);
            for status in [
                DeploymentStatus::UpdatePending,
                DeploymentStatus::DeletePending,
                DeploymentStatus::Provisioning,
                DeploymentStatus::Running,
            ] {
                let fixture = Fixture::new(status, error.clone()).await;
                fixture.process(&fixture.runner()).await;
                let record = fixture.record().await;
                assert_eq!(
                    parse_status(&record.status),
                    alien_deployment::runner::failed_status_for_deployment_error(status)
                );
                assert_eq!(record.error.unwrap(), serde_json::to_value(&error).unwrap());
            }
        }

        #[tokio::test]
        async fn expired_transient_error_fails_with_real_error_after_restart() {
            let error = transient_error();
            let fixture = Fixture::new(DeploymentStatus::UpdatePending, error.clone()).await;
            fixture.process(&fixture.runner()).await;
            assert_eq!(fixture.record().await.status, "update-pending");
            let mut metadata = fixture.record().await.runtime_metadata.unwrap();
            metadata.credential_retry.as_mut().unwrap().started_at =
                Utc::now() - chrono::Duration::minutes(11);
            let serialized = serde_json::to_string(&metadata)
                .unwrap()
                .replace('\'', "''");
            fixture
                .db
                .execute(&format!(
                    "UPDATE deployments SET runtime_metadata = '{serialized}'"
                ))
                .await
                .unwrap();
            fixture.process(&fixture.runner()).await;
            let record = fixture.record().await;
            assert_eq!(record.status, "update-failed");
            assert!(record.runtime_metadata.unwrap().credential_retry.is_none());
            assert_eq!(record.error.unwrap(), serde_json::to_value(error).unwrap());
        }

        #[test]
        fn legacy_runtime_metadata_has_no_credential_retry_window() {
            let legacy: alien_core::RuntimeMetadata = serde_json::from_str("{}").unwrap();
            assert!(legacy.credential_retry.is_none());
            assert!(serde_json::to_value(&legacy)
                .unwrap()
                .get("credentialRetry")
                .is_none());
        }

        #[tokio::test]
        async fn changed_status_starts_a_new_credential_retry_window() {
            let fixture = Fixture::new(DeploymentStatus::UpdatePending, transient_error()).await;
            fixture.process(&fixture.runner()).await;
            let mut metadata = fixture.record().await.runtime_metadata.unwrap();
            let old = Utc::now() - chrono::Duration::minutes(11);
            metadata.credential_retry.as_mut().unwrap().started_at = old;
            let serialized = serde_json::to_string(&metadata)
                .unwrap()
                .replace('\'', "''");
            fixture.db.execute(&format!("UPDATE deployments SET status = 'delete-pending', runtime_metadata = '{serialized}'")).await.unwrap();
            fixture.process(&fixture.runner()).await;
            let record = fixture.record().await;
            assert_eq!(record.status, "delete-pending");
            let retry = record.runtime_metadata.unwrap().credential_retry.unwrap();
            assert_eq!(retry.status, DeploymentStatus::DeletePending);
            assert!(retry.started_at > old);
        }

        #[tokio::test]
        async fn credential_failure_backs_off_heartbeat_acquisition_without_writes() {
            let error = AlienError::new(alien_gcp_clients::ErrorData::InvalidClientConfig {
                message: "management credentials are missing".to_string(),
                errors: None,
            })
            .into_generic();
            let fixture = Fixture::new(DeploymentStatus::Running, error).await;
            let runner = Arc::new(fixture.runner());
            let heartbeat = crate::loops::heartbeat::HeartbeatLoop::new(
                fixture.manager.config.clone(),
                fixture.manager.deployment_store.clone(),
                runner.clone(),
            );
            heartbeat.tick().await;
            assert_eq!(fixture.record().await.status, "refresh-failed");
            assert_eq!(fixture.credentials.visits.load(Ordering::SeqCst), 1);
            let writes = fixture.writes().await;
            for _ in 0..20 {
                runner.tick().await;
                heartbeat.tick().await;
            }
            assert_eq!(
                fixture.writes().await,
                writes,
                "backoff must avoid even lease writes"
            );
            assert_eq!(fixture.credentials.visits.load(Ordering::SeqCst), 1);

            // A periodic recheck acquires/releases, but does not repeat the failure checkpoint.
            for entry in runner.deferred.lock().unwrap().values_mut() {
                entry.recheck_after = Utc::now() - chrono::Duration::seconds(1);
            }
            let before = fixture.record().await;
            heartbeat.tick().await;
            assert_eq!(fixture.credentials.visits.load(Ordering::SeqCst), 2);
            assert_eq!(fixture.record().await.updated_at, before.updated_at);
            let after_recheck = fixture.writes().await;
            assert_eq!(
                after_recheck - writes,
                2,
                "only acquire and release may write"
            );
            heartbeat.tick().await;
            assert_eq!(fixture.writes().await, after_recheck);

            // An explicit retry ends the backoff, including when the status stays failed.
            fixture
                .manager
                .deployment_store
                .set_retry_requested(&Subject::system(), &fixture.id)
                .await
                .unwrap();
            heartbeat.tick().await;
            assert_eq!(fixture.credentials.visits.load(Ordering::SeqCst), 3);
        }

        async fn incompatible_retry_metadata_store_fails_safely(trigger: &str) {
            let error = transient_error();
            let fixture = Fixture::new(DeploymentStatus::UpdatePending, error.clone()).await;
            fixture.db.execute(trigger).await.unwrap();
            let runner = fixture.runner();
            runner.tick().await;
            let record = fixture.record().await;
            assert_eq!(record.status, "update-failed");
            assert_eq!(record.error, Some(serde_json::to_value(error).unwrap()));
            assert!(record.runtime_metadata.unwrap().credential_retry.is_none());
            let writes = fixture.writes().await;
            runner.tick().await;
            assert_eq!(fixture.writes().await, writes);
        }

        #[tokio::test]
        async fn rejected_retry_metadata_falls_back_to_credential_failure() {
            incompatible_retry_metadata_store_fails_safely(
                "CREATE TRIGGER reject_retry BEFORE UPDATE OF runtime_metadata ON deployments WHEN json_extract(NEW.runtime_metadata, '$.credentialRetry') IS NOT NULL BEGIN SELECT RAISE(ABORT, 'unsupported credential retry field'); END"
            ).await;
        }

        #[tokio::test]
        async fn stripped_retry_metadata_falls_back_to_credential_failure() {
            incompatible_retry_metadata_store_fails_safely(
                "CREATE TRIGGER strip_retry AFTER UPDATE OF runtime_metadata ON deployments WHEN json_extract(NEW.runtime_metadata, '$.credentialRetry') IS NOT NULL BEGIN UPDATE deployments SET runtime_metadata = json_remove(NEW.runtime_metadata, '$.credentialRetry') WHERE id = NEW.id; END"
            ).await;
        }

        #[tokio::test]
        async fn skipped_credentials_clear_an_old_retry_window() {
            for pull in [true, false] {
                let fixture = Fixture::new(DeploymentStatus::Running, transient_error()).await;
                fixture
                    .db
                    .execute("UPDATE deployments SET desired_release_id = NULL")
                    .await
                    .unwrap();
                let before = fixture.record().await;
                let mut metadata = before.runtime_metadata.clone().unwrap();
                metadata.credential_retry = Some(alien_core::CredentialRetry {
                    started_at: Utc::now() - chrono::Duration::minutes(11),
                    status: DeploymentStatus::Running,
                });
                let mut settings = before.stack_settings.clone().unwrap();
                if pull {
                    settings.deployment_model = alien_core::DeploymentModel::Pull;
                } else {
                    settings.heartbeats = alien_core::HeartbeatsMode::Off;
                }
                fixture
                    .db
                    .execute(&format!(
                        "UPDATE deployments SET runtime_metadata = '{}', stack_settings = '{}'",
                        serde_json::to_string(&metadata)
                            .unwrap()
                            .replace('\'', "''"),
                        serde_json::to_string(&settings).unwrap()
                    ))
                    .await
                    .unwrap();
                fixture.process(&fixture.runner()).await;
                let cleared = fixture.record().await;
                assert!(cleared.runtime_metadata.unwrap().credential_retry.is_none());
                assert_eq!(cleared.status, "running");
                assert_eq!(cleared.current_release_id, before.current_release_id);
                assert_eq!(cleared.desired_release_id, before.desired_release_id);
                assert_eq!(fixture.credentials.visits.load(Ordering::SeqCst), 0);
                fixture
                    .db
                    .execute(&format!(
                        "UPDATE deployments SET stack_settings = '{}'",
                        serde_json::to_string(&before.stack_settings).unwrap()
                    ))
                    .await
                    .unwrap();
                fixture.process(&fixture.runner()).await;
                assert_eq!(fixture.record().await.status, "running");
                assert!(
                    fixture
                        .record()
                        .await
                        .runtime_metadata
                        .unwrap()
                        .credential_retry
                        .unwrap()
                        .started_at
                        > metadata.credential_retry.unwrap().started_at
                );
            }
        }

        #[tokio::test]
        async fn capped_list_does_not_limit_heartbeat_acquisition() {
            let fixture = Fixture::new(
                DeploymentStatus::Running,
                AlienError::new(alien_error::GenericError {
                    message: "missing credentials".to_string(),
                }),
            )
            .await;
            let runner = fixture.runner();
            fixture.process(&runner).await;
            fixture.db.execute(&format!("WITH RECURSIVE numbers(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM numbers WHERE n < 150) INSERT INTO deployments (id, name, deployment_group_id, platform, deployment_protocol_version, status, stack_settings, retry_requested, created_at, input_values) SELECT 'candidate-' || n, 'candidate-' || n, deployment_group_id, platform, deployment_protocol_version, 'running', stack_settings, 0, created_at, input_values FROM numbers CROSS JOIN deployments WHERE id = '{}'", fixture.id)).await.unwrap();
            let caller = Subject::system();
            let mut filter = DeploymentFilter {
                statuses: Some(vec!["running".to_string(), "refresh-failed".to_string()]),
                limit: Some(100),
                ..Default::default()
            };
            assert_eq!(
                fixture
                    .manager
                    .deployment_store
                    .list_deployments(&caller, &filter)
                    .await
                    .unwrap()
                    .len(),
                100
            );
            runner
                .exclude_deferred(&caller, &mut filter, true)
                .await
                .unwrap();
            let acquired = fixture
                .manager
                .deployment_store
                .acquire(&caller, "beyond-page", &filter, 200)
                .await
                .unwrap();
            assert_eq!(
                acquired.len(),
                150,
                "a capped listing must not constrain acquisition"
            );
            assert!(acquired.iter().all(|item| item.deployment.id != fixture.id));
            assert!(acquired
                .iter()
                .any(|item| item.deployment.id == "candidate-150"));
        }

        #[tokio::test]
        async fn delete_requested_during_failure_checkpoint_bypasses_backoff() {
            let fixture = Fixture::new(
                DeploymentStatus::Running,
                AlienError::new(alien_error::GenericError {
                    message: "missing credentials".to_string(),
                }),
            )
            .await;
            let runner = fixture.runner();
            fixture.db.execute("CREATE TRIGGER delete_after_failure AFTER UPDATE OF status ON deployments WHEN OLD.status = 'running' AND NEW.status = 'refresh-failed' BEGIN UPDATE deployments SET status = 'delete-pending' WHERE id = NEW.id; END").await.unwrap();
            fixture.process(&runner).await;
            assert_eq!(fixture.record().await.status, "delete-pending");
            runner.tick().await;
            assert_eq!(fixture.credentials.visits.load(Ordering::SeqCst), 2);
            assert_eq!(fixture.record().await.status, "delete-failed");
        }

        #[tokio::test]
        async fn retry_requested_during_failure_checkpoint_bypasses_backoff() {
            let error = AlienError::new(alien_gcp_clients::ErrorData::InvalidClientConfig {
                message: "management credentials are missing".to_string(),
                errors: None,
            })
            .into_generic();
            let fixture = Fixture::new(DeploymentStatus::Running, error).await;
            let runner = Arc::new(fixture.runner());
            let heartbeat = crate::loops::heartbeat::HeartbeatLoop::new(
                fixture.manager.config.clone(),
                fixture.manager.deployment_store.clone(),
                runner,
            );
            // Deterministically request a retry after the failure is persisted but
            // before process_deployment rereads the record to cache its revision.
            fixture.db.execute("CREATE TRIGGER retry_after_failure AFTER UPDATE OF status ON deployments WHEN OLD.status = 'running' AND NEW.status = 'refresh-failed' BEGIN UPDATE deployments SET retry_requested = 1 WHERE id = NEW.id; END").await.unwrap();
            heartbeat.tick().await;
            heartbeat.tick().await;
            assert_eq!(fixture.credentials.visits.load(Ordering::SeqCst), 2);
            assert!(!fixture.record().await.retry_requested);
        }

        #[tokio::test]
        async fn transient_credentials_are_attempted_once_per_tick_and_keep_deadline() {
            let fixture = Fixture::new(DeploymentStatus::UpdatePending, transient_error()).await;
            let runner = fixture.runner();
            runner.tick().await;
            let waiting = fixture.record().await;
            assert_eq!(fixture.credentials.visits.load(Ordering::SeqCst), 1);
            let writes = fixture.writes().await;
            runner.tick().await;
            assert_eq!(fixture.credentials.visits.load(Ordering::SeqCst), 1);
            assert_eq!(fixture.writes().await, writes);
            assert_eq!(fixture.record().await.updated_at, waiting.updated_at);
            // Another replica may retry, but must use the original persisted deadline.
            fixture.runner().tick().await;
            assert_eq!(fixture.credentials.visits.load(Ordering::SeqCst), 2);
            assert_eq!(fixture.record().await.updated_at, waiting.updated_at);
        }

        #[tokio::test]
        async fn identical_refresh_failure_does_not_checkpoint_again() {
            let error = transient_error();
            let fixture = Fixture::new(DeploymentStatus::RefreshFailed, error.clone()).await;
            let serialized = serde_json::to_string(&error).unwrap().replace('\'', "''");
            let old = (Utc::now() - chrono::Duration::minutes(11)).to_rfc3339();
            fixture
                .db
                .execute(&format!(
                    "UPDATE deployments SET error = '{serialized}', updated_at = '{old}'"
                ))
                .await
                .unwrap();
            let before = fixture.record().await;
            let writes_before = fixture.writes().await;
            fixture
                .runner()
                .process_deployment_inner(
                    before.clone(),
                    None,
                    "test-session",
                    ProcessOptions::heartbeat_tick(),
                )
                .await
                .unwrap();
            assert_eq!(
                fixture.writes().await,
                writes_before,
                "an unchanged failure must not be checkpointed"
            );
            assert_eq!(fixture.record().await.updated_at, before.updated_at);
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

    fn gcp_stack_with_target_identity() -> StackState {
        let mut stack = stack_state_with_remote_management_outputs(true);
        stack.platform = Platform::Gcp;
        stack
            .resources
            .get_mut("remote-stack-management")
            .unwrap()
            .outputs = Some(ResourceOutputs::new(RemoteStackManagementOutputs {
            management_resource_id: "target@example.invalid".to_string(),
            access_configuration: "target@example.invalid".to_string(),
            legacy_remote_bindings_access: None,
        }));
        stack
    }

    async fn gcp_target_iam_denial() -> AlienError {
        use alien_error::Context;
        let server = httpmock::MockServer::start_async().await;
        let response = server.mock_async(|when, then| {
            when.method(httpmock::Method::POST);
            then.status(403).json_body(serde_json::json!({"error": {"code": 403, "message": "synthetic target IAM propagation"}}));
        }).await;
        let config = alien_core::ClientConfig::Gcp(Box::new(alien_core::GcpClientConfig {
            project_id: "synthetic-project".to_string(),
            region: "us-central1".to_string(),
            project_number: None,
            credentials: alien_core::GcpCredentials::AccessToken {
                token: "synthetic-token".to_string(),
            },
            service_overrides: Some(alien_core::GcpServiceOverrides {
                endpoints: HashMap::from([("iamcredentials".to_string(), server.base_url())]),
            }),
        }));
        use alien_gcp_clients::iam::{GenerateAccessTokenRequest, IamApi, IamClient};
        let error = IamClient::new(reqwest::Client::new(), config.gcp_config().unwrap().clone())
            .generate_access_token(
                "target@example.invalid".to_string(),
                GenerateAccessTokenRequest::builder()
                    .scope(vec![
                        "https://www.googleapis.com/auth/cloud-platform".to_string()
                    ])
                    .build(),
            )
            .await
            .context(ErrorData::RemoteCredentialHandoffFailed {
                deployment_id: "dep_test".to_string(),
                platform: Platform::Gcp,
            })
            .unwrap_err()
            .into_generic();
        assert!(response.hits_async().await > 0);
        crate::providers::credential_errors::classify_credential_error(error)
    }

    #[tokio::test]
    async fn gcp_provisioning_retries_target_credential_handoff_during_grace_period() {
        let now = Utc::now();
        let mut deployment = deployment_record(
            DeploymentStatus::Provisioning,
            Some(gcp_stack_with_target_identity()),
        );
        deployment.platform = Platform::Gcp;
        deployment.runtime_metadata = Some(alien_core::RuntimeMetadata {
            credential_retry: Some(alien_core::CredentialRetry {
                started_at: now - chrono::Duration::minutes(2),
                status: DeploymentStatus::Provisioning,
            }),
            ..Default::default()
        });
        let error = gcp_target_iam_denial().await;
        assert!(
            !error.retryable,
            "only the first provisioning gets propagation grace"
        );

        let remaining = credential_error_retry_remaining(&deployment, &error, now)
            .expect("fresh GCP handoff failure should remain retryable");

        assert_eq!(
            remaining,
            CREDENTIAL_ERROR_GRACE_PERIOD - Duration::from_secs(2 * 60)
        );

        deployment
            .stack_state
            .as_mut()
            .unwrap()
            .resources
            .get_mut("remote-stack-management")
            .unwrap()
            .outputs = Some(ResourceOutputs::new(RemoteStackManagementOutputs {
            management_resource_id: "other@example.invalid".to_string(),
            access_configuration: "other@example.invalid".to_string(),
            legacy_remote_bindings_access: None,
        }));
        assert!(
            credential_error_retry_remaining(&deployment, &error, now).is_none(),
            "only the freshly granted target identity gets propagation grace"
        );
    }

    /// A federated deployment stores its federation descriptor as the access configuration; the
    /// freshly granted target identity is still the management email.
    #[tokio::test]
    async fn federated_gcp_provisioning_retries_target_credential_handoff_during_grace_period() {
        let now = Utc::now();
        let mut stack = gcp_stack_with_target_identity();
        stack
            .resources
            .get_mut("remote-stack-management")
            .unwrap()
            .outputs = Some(ResourceOutputs::new(RemoteStackManagementOutputs {
            management_resource_id: "target@example.invalid".to_string(),
            access_configuration: serde_json::json!({
                "assertionAudience": "urn:deployment-access:v1:0123456789abcdef",
                "poolId": "deployment-0123456789abcdef0123",
                "projectNumber": "123456789012",
                "providerId": "manager",
                "serviceAccountEmail": "target@example.invalid",
                "trust": {
                    "issuer": "https://issuer.example.com",
                    "subject": "system:serviceaccount:platform:manager"
                }
            })
            .to_string(),
            legacy_remote_bindings_access: None,
        }));
        let mut deployment = deployment_record(DeploymentStatus::Provisioning, Some(stack));
        deployment.platform = Platform::Gcp;
        deployment.runtime_metadata = Some(alien_core::RuntimeMetadata {
            credential_retry: Some(alien_core::CredentialRetry {
                started_at: now - chrono::Duration::minutes(2),
                status: DeploymentStatus::Provisioning,
            }),
            ..Default::default()
        });
        let error = gcp_target_iam_denial().await;

        assert_eq!(
            credential_error_retry_remaining(&deployment, &error, now),
            Some(CREDENTIAL_ERROR_GRACE_PERIOD - Duration::from_secs(2 * 60)),
            "the freshly granted target identity gets propagation grace"
        );
    }

    #[tokio::test]
    async fn gcp_provisioning_handoff_becomes_hard_failure_after_grace_period() {
        let now = Utc::now();
        let mut deployment = deployment_record(
            DeploymentStatus::Provisioning,
            Some(gcp_stack_with_target_identity()),
        );
        deployment.platform = Platform::Gcp;
        deployment.runtime_metadata = Some(alien_core::RuntimeMetadata {
            credential_retry: Some(alien_core::CredentialRetry {
                started_at: now
                    - chrono::Duration::from_std(CREDENTIAL_ERROR_GRACE_PERIOD).unwrap(),
                status: DeploymentStatus::Provisioning,
            }),
            ..Default::default()
        });
        let error = gcp_target_iam_denial().await;
        assert!(
            !error.retryable,
            "only the first provisioning gets propagation grace"
        );

        assert_eq!(
            credential_error_retry_remaining(&deployment, &error, now,),
            None,
            "handoff retry must be bounded"
        );

        let failed = super::failed_state_for_phase_error(
            &deployment,
            DeploymentStatus::Provisioning,
            None,
            Some("release"),
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
            credential_error_retry_remaining(&deployment, &error, now,),
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

    struct UnresolvableManagementIdentity;

    #[async_trait]
    impl CredentialResolver for UnresolvableManagementIdentity {
        async fn resolve(
            &self,
            _: &DeploymentRecord,
        ) -> Result<alien_core::ClientConfig, AlienError> {
            Ok(alien_core::ClientConfig::Test)
        }

        async fn resolve_management_config(
            &self,
            _: alien_core::Platform,
        ) -> Result<Option<alien_core::ManagementConfig>, AlienError> {
            Err(AlienError::new(GenericError {
                message: "AZURE_MANAGEMENT_OIDC_ISSUER is not set".to_string(),
            }))
        }
    }

    struct ResolvedManagementIdentity;

    #[async_trait]
    impl CredentialResolver for ResolvedManagementIdentity {
        async fn resolve(
            &self,
            _: &DeploymentRecord,
        ) -> Result<alien_core::ClientConfig, AlienError> {
            Ok(alien_core::ClientConfig::Test)
        }

        async fn resolve_management_config(
            &self,
            _: alien_core::Platform,
        ) -> Result<Option<alien_core::ManagementConfig>, AlienError> {
            Ok(Some(alien_core::ManagementConfig::Azure(
                alien_core::AzureManagementConfig {
                    managing_tenant_id: "resolved-tenant".to_string(),
                    oidc_issuer: "https://resolved.example.com".to_string(),
                    oidc_subject: "manager".to_string(),
                },
            )))
        }
    }

    /// Rendering without the management identity would drop the setup-owned 'management'
    /// resource, so a failure to resolve it stops a setup render instead of rendering without it.
    /// A Running refresh gets the identity as before (its executor reads it), and runs without it
    /// when it doesn't resolve. Deletion and teardown never read it. A pull Operator turns any
    /// target it receives while Running into an update, so a pull target always renders setup.
    #[tokio::test]
    async fn the_management_identity_is_required_only_to_render_setup() {
        for (status, runner, pull_target) in [
            (DeploymentStatus::Pending, RendersSetup, RendersSetup),
            (
                DeploymentStatus::PreflightsFailed,
                RendersSetup,
                RendersSetup,
            ),
            (DeploymentStatus::InitialSetup, RendersSetup, RendersSetup),
            (
                DeploymentStatus::InitialSetupFailed,
                RendersSetup,
                RendersSetup,
            ),
            (DeploymentStatus::Provisioning, RendersSetup, RendersSetup),
            (
                DeploymentStatus::WaitingForMachines,
                RendersSetup,
                RendersSetup,
            ),
            (
                DeploymentStatus::WaitingForSecrets,
                RendersSetup,
                RendersSetup,
            ),
            (
                DeploymentStatus::ProvisioningFailed,
                RendersSetup,
                RendersSetup,
            ),
            (DeploymentStatus::UpdatePending, RendersSetup, RendersSetup),
            (DeploymentStatus::Updating, RendersSetup, RendersSetup),
            (DeploymentStatus::UpdateFailed, RendersSetup, RendersSetup),
            (DeploymentStatus::Running, Refresh, RendersSetup),
            (DeploymentStatus::RefreshFailed, Refresh, RendersSetup),
            (DeploymentStatus::DeletePending, Unused, Unused),
            (DeploymentStatus::Deleting, Unused, Unused),
            (DeploymentStatus::DeleteFailed, Unused, Unused),
            (DeploymentStatus::TeardownRequired, Unused, Unused),
            (DeploymentStatus::TeardownFailed, Unused, Unused),
            (DeploymentStatus::RetentionRequired, Unused, Unused),
            (DeploymentStatus::Retained, Unused, Unused),
            (DeploymentStatus::Deleted, Unused, Unused),
            (DeploymentStatus::Error, Unused, Unused),
        ] {
            assert_eq!(
                ManagementIdentityUse::for_runner(status),
                runner,
                "{status:?}"
            );
            assert_eq!(
                ManagementIdentityUse::for_pull_target(status),
                pull_target,
                "{status:?}"
            );
        }

        let deployment = deployment_record(DeploymentStatus::Running, None);
        for use_ in [RendersSetup, Refresh, Unused] {
            let resolved = resolve_deployment_management_config(
                &ResolvedManagementIdentity,
                &deployment,
                deployment.platform,
                use_,
            )
            .await
            .expect("resolves");
            assert_eq!(
                resolved.map(
                    |config| serde_json::to_value(config).unwrap()["managingTenantId"].clone()
                ),
                (use_ != Unused).then(|| serde_json::json!("resolved-tenant")),
                "{use_:?}"
            );

            let resolved = resolve_deployment_management_config(
                &UnresolvableManagementIdentity,
                &deployment,
                deployment.platform,
                use_,
            )
            .await;
            if use_ == RendersSetup {
                let error = resolved.expect_err("a setup render needs the management identity");
                assert_eq!(
                    error.message,
                    format!(
                        "Failed to resolve the {} management identity for deployment {}",
                        deployment.platform, deployment.id
                    )
                );
                assert_eq!(
                    error.source.as_ref().expect("resolver error").message,
                    "AZURE_MANAGEMENT_OIDC_ISSUER is not set"
                );
            } else {
                assert!(
                    resolved.expect("not fatal").is_none(),
                    "{use_:?} runs without the identity"
                );
            }
        }

        // A control plane's per-deployment identity wins; the resolver isn't consulted.
        let recorded = alien_core::ManagementConfig::Azure(alien_core::AzureManagementConfig {
            managing_tenant_id: "managing-tenant".to_string(),
            oidc_issuer: "https://issuer.example.com".to_string(),
            oidc_subject: "manager".to_string(),
        });
        let mut deployment = deployment_record(DeploymentStatus::UpdatePending, None);
        deployment.management_config = Some(recorded.clone());
        let resolved = resolve_deployment_management_config(
            &UnresolvableManagementIdentity,
            &deployment,
            deployment.platform,
            RendersSetup,
        )
        .await
        .expect("the recorded identity needs no resolver");
        assert_eq!(
            serde_json::to_value(resolved).unwrap(),
            serde_json::to_value(Some(recorded)).unwrap()
        );
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
        .deployment_config(&deployment, Some(&stack), None)
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
pub(crate) fn parse_status(status: &str) -> DeploymentStatus {
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
        "retention-required" => DeploymentStatus::RetentionRequired,
        "retained" => DeploymentStatus::Retained,
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
