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
use crate::traits::deployment_store::{ExecutionClaim, ReconcileData, ReconcileInput};
use crate::traits::DeploymentStore;

/// Transport that persists state directly to the deployment store and
/// reconciles cross-account registry access with each step.
pub struct ManagerTransport {
    deployment_store: Arc<dyn DeploymentStore>,
    bindings_provider: Option<Arc<dyn BindingsProviderApi>>,
    target_bindings_providers: HashMap<Platform, Arc<dyn BindingsProviderApi>>,
    project_id: String,
    session: String,
    execution_claim: Option<ExecutionClaim>,
    /// AWS regions the artifact registry serves; see `reconcile_registry_access_with_proxy`.
    supported_aws_regions: Vec<String>,
    held_values: crate::values::HeldValues,
    generated_slots: Vec<crate::values::ValueMeta>,
    /// Where a deployment that finishes Running goes next; `None` queues nothing.
    release_targeting: Option<crate::release_targeting::ReleaseTargeting>,
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
            bindings_provider,
            target_bindings_providers,
            project_id,
            session,
            execution_claim,
            supported_aws_regions,
            held_values: crate::values::HeldValues::default(),
            generated_slots: Vec::new(),
            release_targeting: None,
        }
    }

    /// Keep the values the step loaded out of the state this transport saves.
    pub fn with_held_values(mut self, held_values: crate::values::HeldValues) -> Self {
        self.held_values = held_values;
        self
    }
}

impl ManagerTransport {
    /// Queue an update when a step leaves the deployment Running on a release
    /// other than the one it should run.
    pub fn with_release_targeting(
        mut self,
        targeting: Option<crate::release_targeting::ReleaseTargeting>,
    ) -> Self {
        self.release_targeting = targeting;
        self
    }

    /// Report these generated secret inputs with every step.
    pub fn with_generated_slots(mut self, slots: Vec<crate::values::ValueMeta>) -> Self {
        self.generated_slots = slots;
        self
    }
}

#[async_trait]
impl DeploymentLoopTransport for ManagerTransport {
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
        let mut saved_state = updated_state.clone();
        self.held_values
            .redact(saved_state.stack_state.as_mut())
            .map_err(|error| error.into_generic())?;
        let data = ReconcileData {
            deployment_id: deployment_id.to_string(),
            session: self.session.clone(),
            state: saved_state,
            update_heartbeat,
            suggested_delay_ms,
            heartbeats,
            observed_inventory_batches,
            capabilities: Vec::new(),
            operator_version: None,
            execution_claim: self.execution_claim.clone(),
            operations_report: None,
        };
        let mut outcome = if self.generated_slots.is_empty() {
            self.deployment_store.reconcile(&caller, data).await?
        } else {
            let request = ReconcileInput::builder(data)
                .generated_slots(self.generated_slots.clone())
                .build();
            self.deployment_store
                .reconcile_request(&caller, request)
                .await?
        };

        if let Some(targeting) = &self.release_targeting {
            targeting
                .retarget_if_settled(self.deployment_store.as_ref(), &caller, &mut outcome.record)
                .await?;
        }

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

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use alien_core::{
        DeploymentConfig, DeploymentStatus, EnvironmentVariablesSnapshot, Platform,
        CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
    };
    use chrono::Utc;

    use super::*;
    use crate::traits::deployment_store::{MockDeploymentStore, ReconcileOutcome};
    use crate::traits::DeploymentRecord;
    use crate::values::{ValueKind, ValueMeta};

    const CANARY: &str = "generated-canary-7c1d";

    fn state() -> DeploymentState {
        DeploymentState {
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
        }
    }

    fn config() -> alien_core::DeploymentConfig {
        let mut config = DeploymentConfig::builder()
            .deployment_name("dep".to_string())
            .stack_settings(Default::default())
            .input_values(Default::default())
            .environment_variables(EnvironmentVariablesSnapshot {
                variables: Vec::new(),
                hash: String::new(),
                created_at: String::new(),
            })
            .allow_frozen_changes(false)
            .external_bindings(Default::default())
            .build();
        // The step's config carries the generated value; it is never reported.
        config
            .input_values
            .insert("signingKey".to_string(), serde_json::json!(CANARY));
        config
    }

    fn record() -> DeploymentRecord {
        DeploymentRecord {
            supplied_stacks: None,
            id: "dep_1".to_string(),
            workspace_id: "default".to_string(),
            project_id: "project".to_string(),
            name: "dep".to_string(),
            deployment_group_id: "group".to_string(),
            platform: Platform::Local,
            deployment_protocol_version: CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
            base_platform: None,
            status: "running".to_string(),
            stack_settings: Some(Default::default()),
            stack_state: None,
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
            deployment_config: None,
            deployment_token: None,
            input_values: Default::default(),
            retry_requested: false,
            locked_by: None,
            locked_at: None,
            created_at: Utc::now(),
            updated_at: None,
            error: None,
        }
    }

    fn meta() -> ValueMeta {
        ValueMeta {
            slot: "input:signingKey".parse().unwrap(),
            kind: ValueKind::Secret,
            digest: "ab".repeat(32),
            updated_at: Utc::now(),
        }
    }

    async fn steps(store: MockDeploymentStore, slots: Vec<ValueMeta>, count: usize) {
        let transport = ManagerTransport::new(
            Arc::new(store),
            None,
            HashMap::new(),
            "project".to_string(),
            "session".to_string(),
            None,
            Vec::new(),
        )
        .with_generated_slots(slots);
        for _ in 0..count {
            transport
                .reconcile_step("dep_1", &state(), &config(), false, None, vec![], vec![])
                .await
                .expect("the step is reconciled");
        }
    }

    fn outcome() -> ReconcileOutcome {
        ReconcileOutcome {
            record: record(),
            target_operations_bundle_set: None,
            target_dynamic_containers: None,
        }
    }

    #[tokio::test]
    async fn every_step_reports_the_generated_slots_and_not_their_values() {
        let reported = Arc::new(Mutex::new(Vec::new()));
        let mut store = MockDeploymentStore::new();
        let log = reported.clone();
        store
            .expect_reconcile_request()
            .times(2)
            .returning(move |_, request| {
                log.lock()
                    .unwrap()
                    .push((request.generated_slots().to_vec(), format!("{request:?}")));
                Ok(outcome())
            });
        let slots = vec![meta()];

        steps(store, slots.clone(), 2).await;

        let reported = reported.lock().unwrap();
        assert_eq!(reported.len(), 2);
        for (generated, debug) in reported.iter() {
            assert_eq!(generated, &slots);
            assert!(!debug.contains(CANARY), "{debug}");
        }
    }

    #[tokio::test]
    async fn a_deployment_without_generated_slots_reconciles_as_before() {
        let mut store = MockDeploymentStore::new();
        store
            .expect_reconcile()
            .times(1)
            .returning(|_, _| Ok(outcome()));
        store.expect_reconcile_request().never();

        steps(store, Vec::new(), 1).await;
    }
}
