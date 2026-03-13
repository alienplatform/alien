use serde::{Deserialize, Serialize};

use crate::core::{ResourceController, ResourceControllerContext, ResourceControllerStepResult};
use crate::error::Result;
use alien_core::{ResourceOutputs, ResourceStatus, ServiceAccount, ServiceAccountOutputs};
use alien_macros::{controller, flow_entry, handler, terminal_state};
use tracing::info;

#[controller]
pub struct TestServiceAccountController {
    /// The identity of the created service account.
    pub(crate) identity: Option<String>,
}

#[controller]
impl TestServiceAccountController {
    // ─────────────── CREATE FLOW ──────────────────────────────
    #[flow_entry(Create)]
    #[handler(
        state = CreateStart,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let sa_config = ctx.desired_resource_config::<ServiceAccount>()?;
        info!(
            "→ [test-sa-create] Starting creation of service account `{}`",
            sa_config.id
        );

        Ok(HandlerAction::Continue {
            state: CreateServiceAccount,
            suggested_delay: None,
        })
    }

    #[handler(
        state = CreateServiceAccount,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_service_account(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let sa_config = ctx.desired_resource_config::<ServiceAccount>()?;

        // Simulate service account creation - generate a mock identity
        let identity = format!("test-sa-{}", sa_config.id);
        self.identity = Some(identity.clone());

        info!("✓ [test-sa-create] Service account `{}` created", identity);

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    // ─────────────── READY STATE ────────────────────────────────
    #[handler(
        state = Ready,
        on_failure = CreateFailed,
        status = ResourceStatus::Running,
    )]
    async fn ready(&mut self, _ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        // The system will automatically know if config changed and transition to update
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    // ─────────────── UPDATE FLOW ──────────────────────────────
    #[flow_entry(Update, from = [Ready])]
    #[handler(
        state = UpdateStart,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let sa_config = ctx.desired_resource_config::<ServiceAccount>()?;
        info!(
            "→ [test-sa-update] Starting update of service account `{}`",
            sa_config.id
        );

        Ok(HandlerAction::Continue {
            state: UpdateConfig,
            suggested_delay: None,
        })
    }

    #[handler(
        state = UpdateConfig,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_config(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let sa_config = ctx.desired_resource_config::<ServiceAccount>()?;

        // Simulate config update
        info!(
            "✓ [test-sa-update] Service account `{}` configuration updated",
            sa_config.id
        );

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    // ─────────────── DELETE FLOW ──────────────────────────────
    #[flow_entry(Delete)]
    #[handler(
        state = DeleteStart,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn delete_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let sa_config = ctx.desired_resource_config::<ServiceAccount>()?;
        info!(
            "→ [test-sa-delete] Starting deletion of service account `{}`",
            sa_config.id
        );

        Ok(HandlerAction::Continue {
            state: DeleteServiceAccount,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeleteServiceAccount,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn delete_service_account(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let sa_config = ctx.desired_resource_config::<ServiceAccount>()?;

        // Simulate service account deletion
        if let Some(identity) = &self.identity {
            info!("✓ [test-sa-delete] Service account `{}` deleted", identity);
        }
        self.identity = None;

        info!(
            "✓ [test-sa-delete] Service account `{}` deletion completed",
            sa_config.id
        );

        Ok(HandlerAction::Continue {
            state: Deleted,
            suggested_delay: None,
        })
    }

    // ─────────────── TERMINALS ────────────────────────────────
    terminal_state!(
        state = CreateFailed,
        status = ResourceStatus::ProvisionFailed
    );

    terminal_state!(state = UpdateFailed, status = ResourceStatus::UpdateFailed);

    terminal_state!(state = DeleteFailed, status = ResourceStatus::DeleteFailed);

    terminal_state!(state = Deleted, status = ResourceStatus::Deleted);

    fn build_outputs(&self) -> Option<ResourceOutputs> {
        self.identity.as_ref().map(|identity| {
            ResourceOutputs::new(ServiceAccountOutputs {
                identity: identity.clone(),
                resource_id: identity.clone(),
            })
        })
    }

    fn get_binding_params(&self) -> Option<serde_json::Value> {
        // Test platform doesn't use service account bindings
        // All resources are accessible without authentication
        None
    }
}

impl TestServiceAccountController {
    /// Creates a controller in a ready state with mock values for testing purposes.
    pub fn mock_ready(service_account_id: &str) -> Self {
        Self {
            state: TestServiceAccountState::Ready,
            identity: Some(format!("test-sa-{}", service_account_id)),
            _internal_stay_count: None,
        }
    }
}
