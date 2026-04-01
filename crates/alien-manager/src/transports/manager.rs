//! Manager-side deployment loop transport.
//!
//! Implements [`DeploymentLoopTransport`] for the manager's internal loop,
//! persisting state via [`DeploymentStore::reconcile`] and handling
//! cross-account registry access after each step.

use std::sync::Arc;

use async_trait::async_trait;
use tracing::warn;

use alien_bindings::BindingsProviderApi;
use alien_core::DeploymentState;
use alien_deployment::transport::{DeploymentLoopTransport, StepReconcileResult};
use alien_error::AlienError;

use crate::traits::deployment_store::ReconcileData;
use crate::traits::DeploymentStore;

/// Transport that persists state directly to the deployment store and
/// reconciles cross-account registry access after each step.
pub struct ManagerTransport {
    deployment_store: Arc<dyn DeploymentStore>,
    bindings_provider: Option<Arc<dyn BindingsProviderApi>>,
    session: String,
}

impl ManagerTransport {
    pub fn new(
        deployment_store: Arc<dyn DeploymentStore>,
        bindings_provider: Option<Arc<dyn BindingsProviderApi>>,
        session: String,
    ) -> Self {
        Self {
            deployment_store,
            bindings_provider,
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
        step_error: Option<&AlienError>,
        update_heartbeat: bool,
    ) -> Result<StepReconcileResult, AlienError> {
        // 1. Persist the step result via the deployment store.
        let error_value = step_error.map(|e| serde_json::to_value(e).unwrap_or_default());

        self.deployment_store
            .reconcile(ReconcileData {
                deployment_id: deployment_id.to_string(),
                session: self.session.clone(),
                state: state.clone(),
                update_heartbeat,
                error: error_value,
            })
            .await?;

        // 2. Reconcile cross-account registry access (best-effort).
        //    This mirrors the logic in the HTTP reconcile endpoint.
        let mut updated_state: Option<DeploymentState> = None;

        if let (Some(ref bindings_provider), Some(ref env_info)) =
            (&self.bindings_provider, &state.environment_info)
        {
            let pull_creds = crate::registry_access::reconcile_registry_access(
                bindings_provider,
                deployment_id,
                env_info,
                &state.status,
                state.stack_state.as_ref(),
            )
            .await;

            // Azure ACR returns pull credentials that must be persisted in
            // runtime_metadata so the deployment config can inject them.
            if pull_creds.is_some() {
                let mut new_state = state.clone();
                let metadata = new_state.runtime_metadata.get_or_insert_default();
                metadata.image_pull_credentials = pull_creds;

                // Persist the updated credentials.
                if let Err(e) = self
                    .deployment_store
                    .reconcile(ReconcileData {
                        deployment_id: deployment_id.to_string(),
                        session: "registry-access".to_string(),
                        state: new_state.clone(),
                        update_heartbeat: false,
                        error: None,
                    })
                    .await
                {
                    warn!(
                        deployment_id = %deployment_id,
                        error = %e,
                        "Failed to persist registry access credentials"
                    );
                }

                updated_state = Some(new_state);
            }
        }

        Ok(StepReconcileResult {
            state: updated_state,
            config: None,
        })
    }
}
