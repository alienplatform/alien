//! Manager-side deployment loop transport.
//!
//! Implements [`DeploymentLoopTransport`] for the manager's internal loop,
//! persisting state via [`DeploymentStore::reconcile`] and handling
//! cross-account registry access after each step.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;

use alien_bindings::BindingsProviderApi;
use alien_core::{DeploymentState, Platform};
use alien_deployment::transport::{DeploymentLoopTransport, StepReconcileResult};
use alien_error::AlienError;

use crate::traits::deployment_store::ReconcileData;
use crate::traits::DeploymentStore;

/// Transport that persists state directly to the deployment store and
/// reconciles cross-account registry access after each step.
pub struct ManagerTransport {
    deployment_store: Arc<dyn DeploymentStore>,
    bindings_provider: Option<Arc<dyn BindingsProviderApi>>,
    target_bindings_providers: HashMap<Platform, Arc<dyn BindingsProviderApi>>,
    session: String,
}

impl ManagerTransport {
    pub fn new(
        deployment_store: Arc<dyn DeploymentStore>,
        bindings_provider: Option<Arc<dyn BindingsProviderApi>>,
        target_bindings_providers: HashMap<Platform, Arc<dyn BindingsProviderApi>>,
        session: String,
    ) -> Self {
        Self {
            deployment_store,
            bindings_provider,
            target_bindings_providers,
            session,
        }
    }
}

#[async_trait]
impl DeploymentLoopTransport for ManagerTransport {
    async fn reconcile_step(
        &self,
        deployment_id: &str,
        state: &DeploymentState,
        _config: &alien_core::DeploymentConfig,
        step_error: Option<&AlienError>,
        update_heartbeat: bool,
    ) -> Result<StepReconcileResult, AlienError> {
        // 1. Reconcile cross-account registry access (best-effort).
        //    This must happen before persisting so the `registry_access_granted`
        //    flag is included in the persisted state.
        let mut updated_state = state.clone();
        crate::registry_access::reconcile_registry_access(
            &self.bindings_provider,
            &self.target_bindings_providers,
            deployment_id,
            &mut updated_state,
        )
        .await;

        // 2. Persist the step result (including any registry access changes).
        let error_value = step_error.map(|e| serde_json::to_value(e).unwrap_or_default());

        self.deployment_store
            .reconcile(ReconcileData {
                deployment_id: deployment_id.to_string(),
                session: self.session.clone(),
                state: updated_state.clone(),
                update_heartbeat,
                error: error_value,
            })
            .await?;

        // Only return updated state if something actually changed.
        let state_changed = updated_state.runtime_metadata != state.runtime_metadata;

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
