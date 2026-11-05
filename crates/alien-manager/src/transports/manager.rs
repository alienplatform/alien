//! Manager-side deployment loop transport.
//!
//! Implements [`DeploymentLoopTransport`] for the manager's internal loop,
//! persisting state via [`DeploymentStore::reconcile`] and handling
//! cross-account registry access with each step.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;

use alien_bindings::BindingsProviderApi;
use alien_core::{DeploymentState, ObservedInventoryBatch, Platform, ResourceHeartbeat};
use alien_deployment::transport::{DeploymentLoopTransport, StepReconcileResult};
use alien_error::AlienError;

use crate::auth::Subject;
use crate::traits::deployment_store::{ExecutionClaim, ReconcileData};
use crate::traits::DeploymentStore;

/// Transport that persists state directly to the deployment store and
/// reconciles cross-account registry access with each step.
pub struct ManagerTransport {
    deployment_store: Arc<dyn DeploymentStore>,
    telemetry_backend: Option<Arc<dyn crate::traits::TelemetryBackend>>,
    bindings_provider: Option<Arc<dyn BindingsProviderApi>>,
    target_bindings_providers: HashMap<Platform, Arc<dyn BindingsProviderApi>>,
    project_id: String,
    session: String,
    execution_claim: Option<ExecutionClaim>,
    /// AWS regions the artifact registry serves; see `reconcile_registry_access_with_proxy`.
    supported_aws_regions: Vec<String>,
}

impl ManagerTransport {
    pub fn new(
        deployment_store: Arc<dyn DeploymentStore>,
        bindings_provider: Option<Arc<dyn BindingsProviderApi>>,
        target_bindings_providers: HashMap<Platform, Arc<dyn BindingsProviderApi>>,
        project_id: String,
        session: String,
        execution_claim: Option<ExecutionClaim>,
        supported_aws_regions: Vec<String>,
    ) -> Self {
        Self {
            deployment_store,
            telemetry_backend: None,
            bindings_provider,
            target_bindings_providers,
            project_id,
            session,
            execution_claim,
            supported_aws_regions,
        }
    }
    /// Attach the manager's trusted ingestion backend for scheduler-owned metrics.
    pub fn with_telemetry_backend(
        mut self,
        backend: Arc<dyn crate::traits::TelemetryBackend>,
    ) -> Self {
        self.telemetry_backend = Some(backend);
        self
    }
}

#[async_trait]
impl DeploymentLoopTransport for ManagerTransport {
    async fn export_metrics(
        &self,
        deployment_id: &str,
        config: &alien_core::DeploymentConfig,
        data: Vec<u8>,
    ) -> Result<(), AlienError> {
        let Some(backend) = &self.telemetry_backend else {
            return alien_deployment::metrics::export_to_endpoint(config, data)
                .await
                .map_err(AlienError::into_generic);
        };
        if !backend.supports_metrics() {
            return Err(crate::error::ErrorData::internal(
                "Infrastructure metric collection needs a configured metrics backend",
            )
            .into_generic());
        }
        backend
            .ingest(
                crate::traits::TelemetrySignal::Metrics,
                &crate::traits::TelemetryCaller {
                    deployment_id: Some(deployment_id.into()),
                    project_id: Some(self.project_id.clone()),
                    workspace_id: None,
                    gateway_log_source: None,
                },
                data.into(),
            )
            .await
    }

    async fn renew_lease(&self, deployment_id: &str) -> Result<(), AlienError> {
        let caller = Subject::system();
        self.deployment_store
            .renew_lease(
                &caller,
                deployment_id,
                &self.session,
                self.execution_claim.clone(),
            )
            .await?;
        Ok(())
    }

    async fn reconcile_step(
        &self,
        deployment_id: &str,
        state: &DeploymentState,
        config: &alien_core::DeploymentConfig,
        update_heartbeat: bool,
        suggested_delay_ms: Option<u64>,
        heartbeats: Vec<ResourceHeartbeat>,
        observed_inventory_batches: Vec<ObservedInventoryBatch>,
    ) -> Result<StepReconcileResult, AlienError> {
        // 1. Reconcile cross-account registry access (best-effort).
        //    This must happen before persisting so the `registry_access_granted`
        //    flag is included in the persisted state.
        //    A pending replica is persisted too, then returned as a retryable error so the runner
        //    holds the step until the replica exists.
        let mut granted_state = state.clone();
        let replica_pending = crate::registry_access::reconcile_registry_access_with_proxy()
            .bindings_provider(&self.bindings_provider)
            .target_bindings_providers(&self.target_bindings_providers)
            .deployment_id(deployment_id)
            .project_id(&self.project_id)
            .state(&mut granted_state)
            .maybe_proxy_url(config.manager_url.as_deref())
            .supported_aws_regions(&self.supported_aws_regions)
            .call()
            .await
            .err();

        // 2. Revoke a deleted AWS deployment's access before persisting the state that hands it
        //    to setup teardown. A denied revoke comes back as a failed delete to persist instead.
        let updated_state = crate::registry_access::revoke_before_persisting(
            self.deployment_store.as_ref(),
            &self.bindings_provider,
            &self.target_bindings_providers,
            deployment_id,
            &self.project_id,
            granted_state,
        )
        .await
        .map_err(|error| error.into_generic())?;

        // 3. Persist the step result (including any registry access changes).
        // Driven from the deployment loop with no inbound caller — use the
        // synthetic system subject (empty bearer signals to embedders that
        // no caller passthrough is available).
        let caller = Subject::system();
        let outcome = self
            .deployment_store
            .reconcile(
                &caller,
                ReconcileData {
                    deployment_id: deployment_id.to_string(),
                    session: self.session.clone(),
                    state: updated_state.clone(),
                    update_heartbeat,
                    suggested_delay_ms,
                    heartbeats,
                    observed_inventory_batches,
                    capabilities: Vec::new(),
                    operator_version: None,
                    execution_claim: self.execution_claim.clone(),
                    operations_report: None,
                },
            )
            .await?;

        crate::registry_access::revoke_after_persisting(
            self.deployment_store.as_ref(),
            &self.bindings_provider,
            &self.target_bindings_providers,
            deployment_id,
            &self.project_id,
            &updated_state,
        )
        .await
        .map_err(|error| error.into_generic())?;

        if let Some(error) = replica_pending.filter(|_| {
            crate::registry_access::holds_step_for_missing_replica(&outcome.record.status)
        }) {
            return Err(error.into_generic());
        }

        // Only return updated state if something actually changed.
        let state_changed = updated_state.runtime_metadata != state.runtime_metadata
            || updated_state.status != state.status;

        Ok(StepReconcileResult {
            state: if state_changed {
                Some(updated_state)
            } else {
                None
            },
            config: None,
        })
    }
}
