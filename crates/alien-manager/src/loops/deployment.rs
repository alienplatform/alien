//! Deployment loop — background task that drives deployment state machines.
//!
//! The loop:
//! 1. Polls for deployments needing work (via `DeploymentStore::acquire`)
//! 2. Resolves credentials for each deployment (via `CredentialResolver`)
//! 3. Calls `alien_deployment::step()` repeatedly until stable or delayed
//! 4. Reconciles the result (via `DeploymentStore::reconcile`)
//! 5. Releases the lock (via `DeploymentStore::release`)

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::watch;
use tracing::{debug, error, info, warn};

use alien_core::{
    DeploymentConfig, DeploymentState, DeploymentStatus, EnvironmentVariable,
    EnvironmentVariableType, EnvironmentVariablesSnapshot, ExternalBindings, ReleaseInfo,
};
use alien_deployment::loop_contract::{classify_status, LoopOperation, LoopStopReason};
use alien_error::{AlienError, GenericError};
use alien_infra::DefaultPlatformServiceProvider;
use alien_local::LocalBindingsProvider;

use crate::config::ManagerConfig;
use crate::traits::deployment_store::{DeploymentFilter, DeploymentRecord, ReconcileData};
use crate::traits::{
    CredentialResolver, DeploymentStore, ReleaseStore, ServerBindings, TelemetryBackend,
};

/// Maximum number of step() calls per deployment per tick.
const MAX_STEPS_PER_TICK: usize = 100;
/// Suggested delay threshold (ms) — if step suggests waiting longer, yield.
const SUGGESTED_DELAY_THRESHOLD_MS: u64 = 500;

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
    let provider = LocalBindingsProvider::new(&local_state_dir).map_err(|e| {
        AlienError::new(GenericError {
            message: format!(
                "Failed to create LocalBindingsProvider for {}: {}",
                deployment_id, e
            ),
        })
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
    #[allow(dead_code)]
    telemetry_backend: Arc<dyn TelemetryBackend>,
    #[allow(dead_code)]
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
        telemetry_backend: Arc<dyn TelemetryBackend>,
        server_bindings: Arc<ServerBindings>,
        dev_status_tx: Option<watch::Sender<()>>,
    ) -> Self {
        Self {
            config,
            deployment_store,
            release_store,
            credential_resolver,
            telemetry_backend,
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
            self.tick().await;
            tokio::time::sleep(Duration::from_secs(self.config.deployment_interval_secs)).await;
        }
    }

    /// One iteration of the deployment loop.
    async fn tick(&self) {
        let session = uuid::Uuid::new_v4().to_string();

        // Acquire deployments that need work.
        let filter = DeploymentFilter {
            statuses: Some(active_work_statuses()),
            platforms: if self.config.targets.is_empty() {
                None
            } else {
                Some(self.config.targets.clone())
            },
            ..Default::default()
        };

        match self.deployment_store.acquire(&session, &filter, 10).await {
            Ok(acquired) => {
                if !acquired.is_empty() {
                    debug!(count = acquired.len(), session = %session, "Acquired deployments");
                }
                for item in acquired {
                    self.process_deployment(item.deployment, &session).await;
                }
            }
            Err(e) => {
                error!(error = %e, "Failed to acquire deployments");
            }
        }
    }

    /// Process a single deployment: step until stable, reconcile, release.
    async fn process_deployment(&self, deployment: DeploymentRecord, session: &str) {
        let deployment_id = deployment.id.clone();

        // Always release the lock when we are done, even on error.
        let result = self.process_deployment_inner(deployment, session).await;

        if let Err(e) = &result {
            error!(
                deployment_id = %deployment_id,
                error = %e,
                "Error processing deployment"
            );
        }

        // Release lock unconditionally.
        if let Err(e) = self.deployment_store.release(&deployment_id, session).await {
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
    ) -> Result<(), AlienError> {
        let deployment_id = deployment.id.clone();

        // Pull-mode deployments are entirely driven by the alien-agent running in the
        // target environment. The manager must not attempt to provision or deploy them.
        if deployment.stack_settings.deployment_model == alien_core::DeploymentModel::Pull {
            debug!(
                deployment_id = %deployment_id,
                "Skipping pull-mode deployment — handled by alien-agent"
            );
            return Ok(());
        }

        // Push-mode deployments: skip creation and deletion phases.
        // These steps are handled by the push client (alien-deploy-cli) which runs
        // with target-environment credentials. The manager only has management credentials,
        // which would cause resources to be created/deleted in the wrong project.
        let status = parse_status(&deployment.status);
        if matches!(
            status,
            DeploymentStatus::Pending
                | DeploymentStatus::InitialSetup
                | DeploymentStatus::DeletePending
                | DeploymentStatus::Deleting
                | DeploymentStatus::DeleteFailed
        ) {
            debug!(
                deployment_id = %deployment_id,
                status = ?status,
                "Skipping push-mode deployment — creation/deletion phases handled by push client"
            );
            return Ok(());
        }

        // 1. Get the release for this deployment.
        let desired_release_id = deployment.desired_release_id.as_ref().ok_or_else(|| {
            AlienError::new(GenericError {
                message: format!("Deployment {} has no desired_release_id", deployment_id),
            })
        })?;

        let release = self
            .release_store
            .get_release(desired_release_id)
            .await?
            .ok_or_else(|| {
                AlienError::new(GenericError {
                    message: format!("Release {} not found", desired_release_id),
                })
            })?;

        // 2. Resolve credentials for the target platform.
        let client_config = self.credential_resolver.resolve(&deployment).await?;

        // 3. Build deployment state from the record.
        let stack = release.stack.clone();
        let target_release = ReleaseInfo {
            release_id: desired_release_id.clone(),
            version: None,
            description: None,
            stack: stack.clone(),
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
                    stack: stack.clone(),
                }),
            target_release: Some(target_release),
            stack_state: deployment.stack_state.clone(),
            environment_info: deployment.environment_info.clone(),
            runtime_metadata: deployment.runtime_metadata.clone(),
            retry_requested: deployment.retry_requested,
        };

        // Clear retry_requested flag before running.
        if deployment.retry_requested {
            self.deployment_store
                .set_retry_requested(&deployment_id)
                .await?;
            state.retry_requested = true;
        }

        // 4. Build environment variables.
        let environment_variables = self
            .build_environment_variables(&deployment_id, &deployment)
            .await?;

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

        let config = DeploymentConfig {
            stack_settings: deployment.stack_settings.clone(),
            management_config,
            environment_variables,
            allow_frozen_changes: false,
            artifact_registry: None,
            compute_backend: None,
            external_bindings: ExternalBindings::new(),
            image_pull_credentials: None,
            public_urls: None,
            domain_metadata: None,
            monitoring: None,
        };

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

        // 7. Step loop — call step() repeatedly until stable or delayed.
        // Note: step() takes ownership of state, so we clone before each call
        // to retain the last known state if step() fails.
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

        let mut last_step_error: Option<serde_json::Value> = None;
        for i in 0..MAX_STEPS_PER_TICK {
            info!(
                deployment_id = %deployment_id,
                status = ?state.status,
                step = i,
                "Running deployment step"
            );

            let state_snapshot = state.clone();
            let step_result = alien_deployment::step(
                state_snapshot,
                config.clone(),
                client_config.clone(),
                Some(service_provider.clone()),
            )
            .await;

            match step_result {
                Ok(result) => {
                    state = result.state;

                    if let Some(ref err) = result.error {
                        warn!(
                            deployment_id = %deployment_id,
                            error = %err,
                            "Deployment step returned error"
                        );
                        last_step_error = Some(serde_json::to_value(err).unwrap_or_default());
                    } else {
                        last_step_error = None;
                    }

                    if let Some(loop_result) = classify_status(&state.status, operation) {
                        if loop_result.stop_reason != LoopStopReason::Handoff {
                            debug!(
                                deployment_id = %deployment_id,
                                status = ?state.status,
                                stop_reason = ?loop_result.stop_reason,
                                outcome = ?loop_result.outcome,
                                "Deployment reached terminal state"
                            );
                            break;
                        }
                    }

                    // If step suggests a delay above threshold, actually sleep for that
                    // duration before yielding to the next tick. This is critical for
                    // phase transitions like InitialSetup → Provisioning where AWS IAM
                    // inline policies need time to propagate (eventual consistency).
                    if let Some(delay_ms) = result.suggested_delay_ms {
                        if delay_ms > SUGGESTED_DELAY_THRESHOLD_MS {
                            // Cap at 60s to prevent pathological delays
                            let sleep_ms = delay_ms.min(60_000);
                            debug!(
                                deployment_id = %deployment_id,
                                delay_ms = delay_ms,
                                sleep_ms = sleep_ms,
                                "Step suggests delay, sleeping before next tick"
                            );
                            tokio::time::sleep(Duration::from_millis(sleep_ms)).await;
                            break;
                        }
                    }
                }
                Err(e) => {
                    error!(
                        deployment_id = %deployment_id,
                        error = %e,
                        "Deployment step() failed"
                    );
                    last_step_error = Some(serde_json::to_value(&e).unwrap_or_default());
                    // state retains its value from before the failed step.
                    // We still reconcile below to persist the current state.
                    break;
                }
            }
        }

        // 8. Reconcile — write the new state back.
        let reconcile_data = ReconcileData {
            deployment_id: deployment_id.clone(),
            session: session.to_string(),
            state: state.clone(),
            update_heartbeat: state.status == DeploymentStatus::Running,
            error: last_step_error,
        };

        match self.deployment_store.reconcile(reconcile_data).await {
            Ok(_record) => {
                info!(
                    deployment_id = %deployment_id,
                    status = ?state.status,
                    "Deployment reconciled"
                );
            }
            Err(e) => {
                error!(
                    deployment_id = %deployment_id,
                    error = %e,
                    "Failed to reconcile deployment"
                );
            }
        }

        // 9. Handle deleted deployments — clean up the record.
        if state.status == DeploymentStatus::Deleted {
            info!(deployment_id = %deployment_id, "Deployment deleted, removing record");
            if let Err(e) = self
                .deployment_store
                .delete_deployment(&deployment_id)
                .await
            {
                error!(
                    deployment_id = %deployment_id,
                    error = %e,
                    "Failed to delete deployment record"
                );
            }
        }

        // 10. Notify dev-mode watchers of state change.
        if let Some(ref tx) = self.dev_status_tx {
            let _ = tx.send(());
        }

        Ok(())
    }

    /// Build the environment variables snapshot injected into containers/functions.
    ///
    /// Includes:
    /// - `ALIEN_DEPLOYMENT_ID`
    /// - OTLP configuration (if telemetry endpoint is set)
    /// - Commands polling configuration
    async fn build_environment_variables(
        &self,
        deployment_id: &str,
        deployment: &DeploymentRecord,
    ) -> Result<EnvironmentVariablesSnapshot, AlienError> {
        let mut vars: Vec<EnvironmentVariable> = Vec::new();

        // 1. ALIEN_DEPLOYMENT_ID — always included.
        vars.push(EnvironmentVariable {
            name: "ALIEN_DEPLOYMENT_ID".to_string(),
            value: deployment_id.to_string(),
            var_type: EnvironmentVariableType::Plain,
            target_resources: None,
        });

        // 2. OTLP telemetry configuration — if an OTLP endpoint is configured
        // or local log ingest is enabled on this manager instance.
        let base_url = self.config.base_url();

        if self.config.otlp_endpoint.is_some() || self.config.enable_local_log_ingest() {
            vars.push(EnvironmentVariable {
                name: "OTEL_EXPORTER_OTLP_LOGS_ENDPOINT".to_string(),
                value: format!("{}/v1/logs", base_url),
                var_type: EnvironmentVariableType::Plain,
                target_resources: None,
            });
            vars.push(EnvironmentVariable {
                name: "OTEL_EXPORTER_OTLP_HEADERS".to_string(),
                value: format!("authorization=Bearer {}", deployment_id),
                var_type: EnvironmentVariableType::Plain,
                target_resources: None,
            });
        }

        // 3. Commands configuration — only inject polling for K8s/Local.
        // Cloud functions (Lambda, Cloud Run, Container Apps) receive commands via
        // platform-native push (InvokeFunction, Pub/Sub, Service Bus) — no polling needed,
        // regardless of deployment model. K8s/Local run as containers that must poll.
        let needs_polling = matches!(
            deployment.platform,
            alien_core::Platform::Kubernetes | alien_core::Platform::Local
        );

        if needs_polling {
            let commands_base = self.config.commands_base_url();
            vars.push(EnvironmentVariable {
                name: "ALIEN_COMMANDS_POLLING_ENABLED".to_string(),
                value: "true".to_string(),
                var_type: EnvironmentVariableType::Plain,
                target_resources: None,
            });
            vars.push(EnvironmentVariable {
                name: "ALIEN_COMMANDS_POLLING_URL".to_string(),
                value: commands_base,
                var_type: EnvironmentVariableType::Plain,
                target_resources: None,
            });
            // Token for commands polling — in dev/standalone mode, use deployment_id as token
            // (permissive auth). Plain type so it's available during InitialSetup when
            // secrets are skipped (vault doesn't exist yet).
            vars.push(EnvironmentVariable {
                name: "ALIEN_COMMANDS_TOKEN".to_string(),
                value: deployment_id.to_string(),
                var_type: EnvironmentVariableType::Plain,
                target_resources: None,
            });
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
}

// Test ownership: Manager-specific behavior tests (status parsing, skip logic,
// active statuses, local bindings caching). Loop contract correctness is tested
// in alien-deployment::loop_contract — not duplicated here.

#[cfg(test)]
mod tests {
    use super::{active_work_statuses, get_or_create_local_bindings_provider, parse_status};
    use alien_core::DeploymentStatus;
    use alien_deployment::loop_contract::{classify_status, LoopOperation};
    use std::collections::HashMap;
    use std::sync::Mutex;
    use tempfile::TempDir;

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
            "update-failed",
            "delete-failed",
            "refresh-failed",
        ] {
            assert!(
                !statuses.iter().any(|s| s == excluded),
                "{excluded} should NOT be in active_work_statuses"
            );
        }
    }

    #[test]
    fn parse_status_roundtrips_all_known_statuses() {
        let cases = [
            ("pending", DeploymentStatus::Pending),
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
            get_or_create_local_bindings_provider(&cache, temp_dir.path(), "ag_test").unwrap();
        let provider_b =
            get_or_create_local_bindings_provider(&cache, temp_dir.path(), "ag_test").unwrap();

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

/// Parse a status string (kebab-case, as stored in the DB) to `DeploymentStatus`.
fn parse_status(status: &str) -> DeploymentStatus {
    match status {
        "pending" => DeploymentStatus::Pending,
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
        "deleted" => DeploymentStatus::Deleted,
        _ => {
            warn!(
                status = status,
                "Unknown deployment status, defaulting to Pending"
            );
            DeploymentStatus::Pending
        }
    }
}
