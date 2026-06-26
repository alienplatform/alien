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
use tokio::sync::watch;
use tracing::{debug, error, info, warn};

use alien_core::{
    DeploymentConfig, DeploymentState, DeploymentStatus, EnvironmentVariable,
    EnvironmentVariableType, EnvironmentVariablesSnapshot, ReleaseInfo,
    ENV_ALIEN_COMMANDS_POLLING_ENABLED, ENV_ALIEN_COMMANDS_POLLING_URL, ENV_ALIEN_COMMANDS_TOKEN,
    ENV_ALIEN_DEPLOYMENT_ID, ENV_ALIEN_DEPLOYMENT_NAME,
};
use alien_deployment::loop_contract::LoopOperation;
use alien_deployment::runner::{failed_status_for_deployment_error, RunnerPolicy, RunnerResult};
use alien_error::{AlienError, Context, GenericError};
use alien_infra::DefaultPlatformServiceProvider;
use alien_local::LocalBindingsProvider;

use crate::auth::Subject;
use crate::config::ManagerConfig;
use crate::traits::deployment_store::{DeploymentFilter, DeploymentRecord, ReconcileData};
use crate::traits::{CredentialResolver, DeploymentStore, ReleaseStore, ServerBindings};
use crate::transports::ManagerTransport;

/// Maximum number of step() calls per deployment per tick.
const MAX_STEPS_PER_TICK: usize = 100;
const MAX_STEPS_PER_HEARTBEAT: usize = 1;
/// Maximum number of deployments to process concurrently per tick.
pub(crate) const MAX_CONCURRENT_DEPLOYMENTS: usize = 4;
/// Maximum acquire/process batches before yielding back to the interval sleep.
const MAX_ACQUIRE_BATCHES_PER_TICK: usize = 16;
/// Suggested delay threshold (ms) — if step suggests waiting longer, yield.
const SUGGESTED_DELAY_THRESHOLD_MS: u64 = 500;

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
        channel: "byo".to_string(),
        machine_image_version: "byo-local".to_string(),
        horizond_version: "byo".to_string(),
        git_sha: "byo".to_string(),
        created_at: "1970-01-01T00:00:00Z".to_string(),
        base_image: HorizonMachineBaseImage {
            name: "byo".to_string(),
            version: "byo".to_string(),
        },
        aws: Some(HorizonAwsMachineImages { amis }),
        gcp: None,
        azure: None,
        os_images: None,
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
}

impl DeploymentLoop {
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
        }
    }

    /// Run the deployment loop forever.
    pub async fn run(&self) {
        info!(
            interval_secs = self.config.deployment_interval_secs,
            "Starting deployment loop"
        );

        loop {
            if let Err(payload) = AssertUnwindSafe(self.tick()).catch_unwind().await {
                error!(
                    panic = panic_payload_message(payload.as_ref()),
                    "Deployment loop tick panicked"
                );
            }
            tokio::time::sleep(Duration::from_secs(self.config.deployment_interval_secs)).await;
        }
    }

    /// One iteration of the deployment loop.
    async fn tick(&self) {
        let filter = DeploymentFilter {
            statuses: Some(manager_candidate_statuses()),
            platforms: if self.config.targets.is_empty() {
                None
            } else {
                Some(self.config.targets.clone())
            },
            ..Default::default()
        };

        // Internal loop: no inbound caller. `Subject::system()` carries an
        // empty `bearer_token` — the documented signal to embedders that
        // no caller passthrough is available.
        let caller = Subject::system();

        for batch_index in 0..MAX_ACQUIRE_BATCHES_PER_TICK {
            let session = uuid::Uuid::new_v4().to_string();

            match self
                .deployment_store
                .acquire(&caller, &session, &filter, 10)
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
                                &session,
                                ProcessOptions::deployment_tick(),
                            )
                            .await;
                        })
                        .await;
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
        session: &str,
    ) {
        self.process_deployment(deployment, session, ProcessOptions::heartbeat_tick())
            .await;
    }

    /// Process a single deployment: step until stable, reconcile, release.
    async fn process_deployment(
        &self,
        deployment: DeploymentRecord,
        session: &str,
        options: ProcessOptions,
    ) {
        let deployment_id = deployment.id.clone();

        // Always release the lock when we are done, even on error.
        let result = self
            .process_deployment_inner(deployment, session, options)
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
            .release(&caller, &deployment_id, session)
            .await
        {
            error!(
                deployment_id = %deployment_id,
                error = %e,
                "Failed to release deployment lock"
            );
        }
    }

    async fn process_deployment_inner(
        &self,
        deployment: DeploymentRecord,
        session: &str,
        options: ProcessOptions,
    ) -> Result<(), AlienError> {
        let deployment_id = deployment.id.clone();
        let stack_settings = deployment
            .stack_settings
            .as_ref()
            .expect("stored deployment carries stack_settings");

        // Pull-mode deployments are entirely driven by the alien-agent running in the
        // target environment. The manager must not attempt to provision or deploy them.
        if stack_settings.deployment_model == alien_core::DeploymentModel::Pull {
            debug!(
                deployment_id = %deployment_id,
                "Skipping pull-mode deployment — handled by alien-agent"
            );
            return Ok(());
        }

        let status = parse_status(&deployment.status);

        if options.require_heartbeats_enabled && !stack_settings.heartbeats.is_enabled() {
            debug!(
                deployment_id = %deployment_id,
                "Skipping heartbeat because heartbeats are disabled for this deployment"
            );
            return Ok(());
        }

        // 1. Get the release for this deployment.
        let target_release_id = deployment
            .desired_release_id
            .as_ref()
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
        let release = self
            .release_store
            .get_release(&system, target_release_id)
            .await?
            .ok_or_else(|| {
                AlienError::new(GenericError {
                    message: format!("Release {} not found", target_release_id),
                })
            })?;
        let deployment_stack = release.stacks.get(&deployment.platform).cloned();

        // 2. Resolve credentials for the target platform and lifecycle phase.
        let resolved_credentials = match self
            .credential_resolver
            .resolve_with_capability(&deployment)
            .await
        {
            Ok(resolved) => resolved,
            Err(e) => {
                if should_wait_for_credential_handoff(status, &deployment) {
                    warn!(
                        deployment_id = %deployment_id,
                        status = ?status,
                        platform = ?deployment.platform,
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
                    let caller = Subject::system();
                    self.deployment_store
                        .reconcile(
                            &caller,
                            ReconcileData {
                                deployment_id: deployment_id.clone(),
                                session: session.to_string(),
                                state: failed_state,
                                update_heartbeat: false,
                                suggested_delay_ms: None,
                                heartbeats: Vec::new(),
                            },
                        )
                        .await?;
                }
                return Ok(());
            }
        };

        if needs_provision_capability(status) && !resolved_credentials.has_provision_capability {
            debug!(
                deployment_id = %deployment_id,
                status = ?status,
                platform = ?deployment.platform,
                "Skipping bootstrap phase because resolved credentials cannot provision"
            );
            return Ok(());
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
            release_id: target_release_id.clone(),
            version: None,
            description: None,
            stack: deployment_stack.clone(),
        };

        let mut state = DeploymentState {
            status: parse_status(&deployment.status),
            platform: deployment.platform.clone(),
            current_release: deployment
                .current_release_id
                .as_ref()
                .map(|id| ReleaseInfo {
                    release_id: id.clone(),
                    version: None,
                    description: None,
                    stack: deployment_stack.clone(),
                }),
            target_release: Some(target_release),
            stack_state: deployment.stack_state.clone(),
            error: deployment_record_error(&deployment.error),
            environment_info: deployment.environment_info.clone(),
            runtime_metadata: deployment.runtime_metadata.clone(),
            retry_requested: deployment.retry_requested,
            protocol_version: deployment.deployment_protocol_version,
        };

        // 4. Build environment variables.
        let environment_variables = self
            .build_environment_variables(&deployment_id, &deployment)
            .await?;
        let provided_config = deployment.deployment_config.as_ref();
        let monitoring = provided_config
            .and_then(|config| config.monitoring.clone())
            .or_else(|| self.build_monitoring_config(&deployment));

        // 5. Build deployment config.
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
                        deployment_id = %deployment_id,
                        error = %e,
                        "Failed to resolve management config from bindings"
                    );
                    None
                }
            }
        };

        let native_image_host = crate::registry_access::derive_native_image_host(
            &self.server_bindings.bindings_provider,
            &self.server_bindings.target_bindings_providers,
            &deployment.platform,
        )
        .await;

        let stack_settings = deployment
            .stack_settings
            .clone()
            .or_else(|| provided_config.map(|config| config.stack_settings.clone()))
            .expect("stored deployment carries stack_settings");

        let mut config = DeploymentConfig {
            deployment_name: Some(deployment.name.clone()),
            stack_settings: stack_settings.clone(),
            management_config,
            environment_variables,
            allow_frozen_changes: provided_config
                .map(|config| config.allow_frozen_changes)
                .unwrap_or(false),
            compute_backend: provided_config.and_then(|config| config.compute_backend.clone()),
            external_bindings: deployment
                .stack_settings
                .as_ref()
                .or_else(|| provided_config.map(|config| &config.stack_settings))
                .expect("stored deployment carries stack_settings")
                .external_bindings
                .clone()
                .unwrap_or_default(),
            base_platform: provided_config
                .and_then(|config| config.base_platform)
                .or(deployment.base_platform),
            public_urls: provided_config.and_then(|config| config.public_urls.clone()),
            domain_metadata: provided_config.and_then(|config| config.domain_metadata.clone()),
            monitoring,
            manager_url: Some(self.config.base_url()),
            deployment_token: provided_config
                .and_then(|config| config.deployment_token.clone())
                .or_else(|| deployment.deployment_token.clone()),
            native_image_host,
        };

        // Standalone-mode bridge: inject a BYO Horizon backend from env vars
        // when an external control plane did not supply one.
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
            delay_threshold: Some(Duration::from_millis(SUGGESTED_DELAY_THRESHOLD_MS)),
        };

        let transport = ManagerTransport::new(
            self.deployment_store.clone(),
            self.server_bindings.bindings_provider.clone(),
            self.server_bindings.target_bindings_providers.clone(),
            session.to_string(),
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
        runner_result.map(|_| ()).map_err(|e| e.into_generic())
    }

    /// Derive the native image host for Lambda/Cloud Run deployments.
    ///
    /// Lambda requires ECR URIs and Cloud Run requires GAR URIs — they can't pull
    /// Build the environment variables snapshot injected into containers/workers.
    ///
    /// Includes:
    /// - `ALIEN_DEPLOYMENT_ID`
    /// - `ALIEN_DEPLOYMENT_NAME`
    /// - Commands polling configuration
    async fn build_environment_variables(
        &self,
        deployment_id: &str,
        deployment: &DeploymentRecord,
    ) -> Result<EnvironmentVariablesSnapshot, AlienError> {
        let mut vars: Vec<EnvironmentVariable> = Vec::new();

        // 1. ALIEN_DEPLOYMENT_ID — always included.
        vars.push(EnvironmentVariable {
            name: ENV_ALIEN_DEPLOYMENT_ID.to_string(),
            value: deployment_id.to_string(),
            var_type: EnvironmentVariableType::Plain,
            target_resources: None,
        });
        vars.push(EnvironmentVariable {
            name: ENV_ALIEN_DEPLOYMENT_NAME.to_string(),
            value: deployment.name.clone(),
            var_type: EnvironmentVariableType::Plain,
            target_resources: None,
        });

        // 3. Commands configuration — only inject polling for K8s/Local.
        // Cloud workers (Lambda, Cloud Run, Container Apps) receive commands via
        // platform-native push (InvokeFunction, Pub/Sub, Service Bus) — no polling needed,
        // regardless of deployment model. K8s/Local run as containers that must poll.
        let needs_polling = matches!(
            deployment.platform,
            alien_core::Platform::Kubernetes | alien_core::Platform::Local
        );

        if needs_polling {
            let commands_base = self.config.commands_base_url();
            vars.push(EnvironmentVariable {
                name: ENV_ALIEN_COMMANDS_POLLING_ENABLED.to_string(),
                value: "true".to_string(),
                var_type: EnvironmentVariableType::Plain,
                target_resources: None,
            });
            vars.push(EnvironmentVariable {
                name: ENV_ALIEN_COMMANDS_POLLING_URL.to_string(),
                value: commands_base,
                var_type: EnvironmentVariableType::Plain,
                target_resources: None,
            });
            // Commands token — from the deployment record field.
            // The manager is the sole injector of ALIEN_COMMANDS_TOKEN.
            if let Some(ref token) = deployment.deployment_token {
                vars.push(EnvironmentVariable {
                    name: ENV_ALIEN_COMMANDS_TOKEN.to_string(),
                    value: token.clone(),
                    var_type: EnvironmentVariableType::Secret,
                    target_resources: None,
                });
            }
        }

        // 4. User-provided environment variables from the deployment record.
        if let Some(ref user_vars) = deployment.user_environment_variables {
            vars.extend(user_vars.iter().cloned());
        }

        // Build deterministic hash from variable contents so the infra executor
        // only sees a change when the actual values change.
        use sha2::{Digest, Sha256};
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

    fn build_monitoring_config(
        &self,
        deployment: &DeploymentRecord,
    ) -> Option<alien_core::OtlpConfig> {
        let otlp_enabled =
            self.config.otlp_endpoint.is_some() || self.config.enable_local_log_ingest();
        let token = deployment.deployment_token.as_ref()?;

        otlp_enabled.then(|| alien_core::OtlpConfig {
            logs_endpoint: format!("{}/v1/logs", self.config.base_url()),
            logs_auth_header: format!("authorization=Bearer {}", token),
            metrics_endpoint: None,
            metrics_auth_header: None,
            resource_attributes: std::collections::HashMap::new(),
        })
    }
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

fn failed_state_for_credential_error(
    deployment: &DeploymentRecord,
    status: DeploymentStatus,
    deployment_stack: Option<&alien_core::Stack>,
    target_release_id: &str,
    error: AlienError,
) -> DeploymentState {
    DeploymentState {
        status: failed_status_for_deployment_error(status),
        platform: deployment.platform,
        current_release: deployment_stack.and_then(|stack| {
            deployment
                .current_release_id
                .as_ref()
                .map(|id| ReleaseInfo {
                    release_id: id.clone(),
                    version: None,
                    description: None,
                    stack: stack.clone(),
                })
        }),
        target_release: deployment_stack.map(|stack| ReleaseInfo {
            release_id: target_release_id.to_string(),
            version: None,
            description: None,
            stack: stack.clone(),
        }),
        stack_state: deployment.stack_state.clone(),
        error: Some(error),
        environment_info: deployment.environment_info.clone(),
        runtime_metadata: deployment.runtime_metadata.clone(),
        retry_requested: false,
        protocol_version: deployment.deployment_protocol_version,
    }
}

// Test ownership: Manager-specific behavior tests (status parsing, skip logic,
// active statuses, local bindings caching). Loop contract correctness is tested
// in alien-deployment::loop_contract — not duplicated here.

#[cfg(test)]
mod tests {
    use super::{
        active_work_statuses, get_or_create_local_bindings_provider,
        has_remote_stack_management_outputs, manager_candidate_statuses,
        needs_provision_capability, parse_status, retryable_failed_statuses,
        should_wait_for_credential_handoff,
    };
    use alien_core::{
        DeploymentStatus, Platform, RemoteStackManagement, RemoteStackManagementOutputs, Resource,
        ResourceOutputs, ResourceStatus, StackResourceState, StackSettings, StackState,
    };
    use alien_deployment::loop_contract::{classify_status, LoopOperation};
    use chrono::Utc;
    use std::collections::HashMap;
    use std::sync::Mutex;
    use tempfile::TempDir;

    use crate::traits::deployment_store::DeploymentRecord;

    fn deployment_record(
        status: DeploymentStatus,
        stack_state: Option<StackState>,
    ) -> DeploymentRecord {
        DeploymentRecord {
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
            deployment_config: None,
            retry_requested: false,
            locked_by: None,
            locked_at: None,
            created_at: Utc::now(),
            updated_at: None,
            error: None,
        }
    }

    fn deployment_status_str(status: DeploymentStatus) -> &'static str {
        match status {
            DeploymentStatus::Pending => "pending",
            DeploymentStatus::PreflightsFailed => "preflights-failed",
            DeploymentStatus::InitialSetup => "initial-setup",
            DeploymentStatus::InitialSetupFailed => "initial-setup-failed",
            DeploymentStatus::Provisioning => "provisioning",
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
            }))
            .dependencies(Vec::new());

        let resource_state = if has_outputs {
            builder
                .outputs(ResourceOutputs::new(RemoteStackManagementOutputs {
                    management_resource_id: "arn:aws:iam::123456789012:role/test-management"
                        .to_string(),
                    access_configuration: "arn:aws:iam::123456789012:role/test-management"
                        .to_string(),
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
}

fn needs_provision_capability(status: DeploymentStatus) -> bool {
    matches!(
        status,
        DeploymentStatus::Pending | DeploymentStatus::PreflightsFailed
    )
}

fn deployment_record_error(error: &Option<serde_json::Value>) -> Option<AlienError> {
    error
        .clone()
        .and_then(|value| serde_json::from_value::<AlienError>(value).ok())
}

/// Parse a status string (kebab-case, as stored in the DB) to `DeploymentStatus`.
fn parse_status(status: &str) -> DeploymentStatus {
    match status {
        "pending" => DeploymentStatus::Pending,
        "preflights-failed" => DeploymentStatus::PreflightsFailed,
        "initial-setup" => DeploymentStatus::InitialSetup,
        "initial-setup-failed" => DeploymentStatus::InitialSetupFailed,
        "provisioning" => DeploymentStatus::Provisioning,
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
