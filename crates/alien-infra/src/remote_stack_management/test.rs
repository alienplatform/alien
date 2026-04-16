use serde::{Deserialize, Serialize};

use crate::core::{ResourceController, ResourceControllerContext, ResourceControllerStepResult};
use crate::error::Result;
use alien_core::{
    RemoteStackManagement, RemoteStackManagementOutputs, ResourceOutputs, ResourceStatus,
};
use alien_macros::{controller, flow_entry, handler, terminal_state};
use tracing::info;

#[controller]
pub struct TestRemoteStackManagementController {
    /// The management resource identifier.
    pub(crate) management_resource_id: Option<String>,
    /// The access configuration.
    pub(crate) access_configuration: Option<String>,
}

#[controller]
impl TestRemoteStackManagementController {
    // ─────────────── CREATE FLOW ──────────────────────────────
    #[flow_entry(Create)]
    #[handler(
        state = CreateStart,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<RemoteStackManagement>()?;
        info!(
            "→ [test-remote-stack-mgmt-create] Starting creation of remote stack management `{}`",
            config.id
        );

        Ok(HandlerAction::Continue {
            state: CreateManagement,
            suggested_delay: None,
        })
    }

    #[handler(
        state = CreateManagement,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_management(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<RemoteStackManagement>()?;

        // Simulate remote stack management creation
        let management_resource_id = format!("test:remote-stack-management:{}", config.id);
        let access_configuration = format!("test:access-config:{}", config.id);

        self.management_resource_id = Some(management_resource_id.clone());
        self.access_configuration = Some(access_configuration.clone());

        info!(
            "✓ [test-remote-stack-mgmt-create] Remote stack management `{}` created with resource_id: {}, access_config: {}",
            config.id, management_resource_id, access_configuration
        );

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
        let config = ctx.desired_resource_config::<RemoteStackManagement>()?;
        info!(
            "→ [test-remote-stack-mgmt-update] Starting update of remote stack management `{}`",
            config.id
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
        let config = ctx.desired_resource_config::<RemoteStackManagement>()?;

        // Simulate config update
        info!(
            "✓ [test-remote-stack-mgmt-update] Remote stack management `{}` configuration updated",
            config.id
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
        let config = ctx.desired_resource_config::<RemoteStackManagement>()?;
        info!(
            "→ [test-remote-stack-mgmt-delete] Starting deletion of remote stack management `{}`",
            config.id
        );

        Ok(HandlerAction::Continue {
            state: DeleteManagement,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeleteManagement,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn delete_management(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<RemoteStackManagement>()?;

        // Simulate remote stack management deletion
        if let Some(resource_id) = &self.management_resource_id {
            info!(
                "✓ [test-remote-stack-mgmt-delete] Remote stack management `{}` deleted",
                resource_id
            );
        }
        self.management_resource_id = None;
        self.access_configuration = None;

        info!(
            "✓ [test-remote-stack-mgmt-delete] Remote stack management `{}` deletion completed",
            config.id
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
        match (&self.management_resource_id, &self.access_configuration) {
            (Some(management_resource_id), Some(access_configuration)) => {
                Some(ResourceOutputs::new(RemoteStackManagementOutputs {
                    management_resource_id: management_resource_id.clone(),
                    access_configuration: access_configuration.clone(),
                }))
            }
            _ => None,
        }
    }
}
