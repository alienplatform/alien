use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use alien_core::{ResourceOutputs, ResourceStatus, Storage, StorageOutputs};
use alien_error::AlienError;
use alien_macros::controller;
use std::sync::Mutex;
use tracing::info;

/// A CORS origin that makes the test controller fail its create right after it recorded the
/// bucket, the way a real create can fail after its first durable mutation. Storage has no
/// free-form settings, so the trigger rides on a config field.
pub const SIMULATE_STORAGE_CREATE_FAILURE_ORIGIN: &str = "https://simulate-create-failure.test";

/// Every bucket delete the test controller issued: the storage id and the config the delete
/// handler saw.
static ISSUED_STORAGE_DELETES: Mutex<Vec<(String, Storage)>> = Mutex::new(Vec::new());

/// The configs seen by each bucket delete issued for the storage with this id, in order.
///
/// Process-wide, so tests that read it should give their storage unique ids.
pub fn test_storage_deletes_issued(storage_id: &str) -> Vec<Storage> {
    ISSUED_STORAGE_DELETES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .iter()
        .filter(|(issued_for, _)| issued_for == storage_id)
        .map(|(_, config)| config.clone())
        .collect()
}

#[controller]
pub struct TestStorageController {
    /// The name of the created storage bucket.
    pub(crate) bucket_name: Option<String>,
    /// Number of Ready checks executed for this controller.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub(crate) ready_checks: u32,
    #[serde(default, skip_serializing_if = "is_false")]
    pub(crate) convergence_reconciliation: bool,
}

fn is_zero(value: &u32) -> bool {
    *value == 0
}

fn is_false(value: &bool) -> bool {
    !value
}

#[controller]
impl TestStorageController {
    fn requires_convergence_reconciliation(&self) -> bool {
        self.convergence_reconciliation
    }

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

        if storage_config
            .cors_allowed_origins
            .iter()
            .any(|origin| origin == SIMULATE_STORAGE_CREATE_FAILURE_ORIGIN)
        {
            return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                message: format!("Simulated failure after creating bucket `{bucket_name}`"),
                resource_id: Some(storage_config.id.clone()),
            }));
        }

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
        self.ready_checks += 1;
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
            ISSUED_STORAGE_DELETES
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push((storage_config.id.clone(), storage_config.clone()));
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
