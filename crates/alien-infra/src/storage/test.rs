use serde::{Deserialize, Serialize};

use crate::core::{ResourceController, ResourceControllerContext, ResourceControllerStepResult};
use crate::error::Result;
use alien_core::{ResourceOutputs, ResourceStatus, Storage, StorageOutputs};
use alien_macros::{controller, flow_entry, handler, terminal_state};
use tracing::info;

#[controller]
pub struct TestStorageController {
    /// The name of the created storage bucket.
    pub(crate) bucket_name: Option<String>,
}

#[controller]
impl TestStorageController {
    // ─────────────── CREATE FLOW ──────────────────────────────
    #[flow_entry(Create)]
    #[handler(
        state = CreateStart,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let storage_config = ctx.desired_resource_config::<Storage>()?;
        info!(
            "→ [test-storage-create] Starting creation of storage `{}`",
            storage_config.id
        );

        Ok(HandlerAction::Continue {
            state: CreateStorage,
            suggested_delay: None,
        })
    }

    #[handler(
        state = CreateStorage,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_storage(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let storage_config = ctx.desired_resource_config::<Storage>()?;

        // Simulate bucket creation - in real implementation this would call cloud APIs
        let bucket_name = format!("{}-{}", ctx.resource_prefix, storage_config.id);
        self.bucket_name = Some(bucket_name.clone());

        info!(
            "✓ [test-storage-create] Storage bucket `{}` created",
            bucket_name
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
        let storage_config = ctx.desired_resource_config::<Storage>()?;
        info!(
            "→ [test-storage-update] Starting update of storage `{}`",
            storage_config.id
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
        let storage_config = ctx.desired_resource_config::<Storage>()?;

        // Simulate config update
        info!(
            "✓ [test-storage-update] Storage `{}` configuration updated",
            storage_config.id
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
        let storage_config = ctx.desired_resource_config::<Storage>()?;
        info!(
            "→ [test-storage-delete] Starting deletion of storage `{}`",
            storage_config.id
        );

        Ok(HandlerAction::Continue {
            state: DeleteStorage,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeleteStorage,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn delete_storage(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let storage_config = ctx.desired_resource_config::<Storage>()?;

        // Simulate bucket deletion
        if let Some(bucket_name) = &self.bucket_name {
            info!(
                "✓ [test-storage-delete] Storage bucket `{}` deleted",
                bucket_name
            );
        }
        self.bucket_name = None;

        info!(
            "✓ [test-storage-delete] Storage `{}` deletion completed",
            storage_config.id
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
        self.bucket_name.as_ref().map(|bucket_name| {
            ResourceOutputs::new(StorageOutputs {
                bucket_name: bucket_name.clone(),
            })
        })
    }
}
