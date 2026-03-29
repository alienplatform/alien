//! Deployment loop — background task that drives deployment state machines.
//!
//! The loop:
//! 1. Polls for deployments needing work (via `DeploymentStore::acquire`)
//! 2. Resolves credentials for each deployment (via `CredentialResolver`)
//! 3. Calls `alien_deployment::step()` repeatedly until stable or delayed
//! 4. Reconciles the result (via `DeploymentStore::reconcile`)
//! 5. Releases the lock (via `DeploymentStore::release`)

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::watch;
use tracing::{debug, error, info, warn};

use alien_core::{
    DeploymentConfig, DeploymentState, DeploymentStatus, EnvironmentVariable,
    EnvironmentVariableType, EnvironmentVariablesSnapshot, ExternalBindings, ReleaseInfo,
};
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
            statuses: Some(
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
                .collect(),
            ),
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
        let config = DeploymentConfig {
            stack_settings: deployment.stack_settings.clone(),
            management_config: None,
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
                let local_state_dir = state_dir.join(&deployment_id);
                let local_bindings = LocalBindingsProvider::new(&local_state_dir).map_err(|e| {
                    AlienError::new(GenericError {
                        message: format!(
                            "Failed to create LocalBindingsProvider for {}: {}",
                            deployment_id, e
                        ),
                    })
                })?;
                Arc::new(DefaultPlatformServiceProvider::with_local_bindings(
                    local_bindings,
                ))
            }
            _ => Arc::new(DefaultPlatformServiceProvider::default()),
        };

        // 7. Step loop — call step() repeatedly until stable or delayed.
        // Note: step() takes ownership of state, so we clone before each call
        // to retain the last known state if step() fails.
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

                    // If state is synced, we are done.
                    if state.status.is_synced() {
                        debug!(
                            deployment_id = %deployment_id,
                            status = ?state.status,
                            "Deployment reached synced state"
                        );
                        break;
                    }

                    // If step suggests a delay above threshold, yield to next tick.
                    if let Some(delay_ms) = result.suggested_delay_ms {
                        if delay_ms > SUGGESTED_DELAY_THRESHOLD_MS {
                            debug!(
                                deployment_id = %deployment_id,
                                delay_ms = delay_ms,
                                "Step suggests delay, yielding to next tick"
                            );
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

        // 3. Commands polling configuration.
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
        // Token for commands polling — in dev mode, use deployment_id as token (permissive auth)
        vars.push(EnvironmentVariable {
            name: "ALIEN_COMMANDS_TOKEN".to_string(),
            value: deployment_id.to_string(),
            var_type: EnvironmentVariableType::Secret,
            target_resources: None,
        });

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
