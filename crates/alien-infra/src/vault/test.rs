use serde::{Deserialize, Serialize};

use crate::core::{ResourceController, ResourceControllerContext, ResourceControllerStepResult};
use crate::error::{ErrorData, Result};
use alien_core::{ResourceOutputs, ResourceStatus, Vault, VaultOutputs};
use alien_error::{Context, IntoAlienError};
use alien_macros::{controller, flow_entry, handler, terminal_state};
use tracing::info;

#[controller]
pub struct TestVaultController {
    /// The identifier of the created vault.
    pub(crate) vault_id: Option<String>,
    /// The data directory for storing vault secrets (for binding params).
    pub(crate) data_dir: Option<String>,
}

#[controller]
impl TestVaultController {
    // ─────────────── CREATE FLOW ──────────────────────────────
    #[flow_entry(Create)]
    #[handler(
        state = CreateStart,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let vault_config = ctx.desired_resource_config::<Vault>()?;
        info!(
            "→ [test-vault-create] Starting creation of vault `{}`",
            vault_config.id
        );

        Ok(HandlerAction::Continue {
            state: CreateVault,
            suggested_delay: None,
        })
    }

    #[handler(
        state = CreateVault,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_vault(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let vault_config = ctx.desired_resource_config::<Vault>()?;

        // Simulate vault creation - generate a mock vault identifier
        let vault_id = format!("test:vault:{}", vault_config.id);
        self.vault_id = Some(vault_id.clone());

        // Generate a unique data directory for this vault instance
        // Use a temp directory pattern that can be overridden in tests
        let data_dir = std::env::var("TEST_VAULT_DATA_DIR")
            .unwrap_or_else(|_| format!("/tmp/test-vault-{}", uuid::Uuid::new_v4().simple()));
        self.data_dir = Some(data_dir);

        info!("✓ [test-vault-create] Vault `{}` created", vault_id);

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
        let vault_config = ctx.desired_resource_config::<Vault>()?;
        info!(
            "→ [test-vault-update] Starting update of vault `{}`",
            vault_config.id
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
        let vault_config = ctx.desired_resource_config::<Vault>()?;

        // Simulate config update
        info!(
            "✓ [test-vault-update] Vault `{}` configuration updated",
            vault_config.id
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
        let vault_config = ctx.desired_resource_config::<Vault>()?;
        info!(
            "→ [test-vault-delete] Starting deletion of vault `{}`",
            vault_config.id
        );

        Ok(HandlerAction::Continue {
            state: DeleteVault,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeleteVault,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn delete_vault(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let vault_config = ctx.desired_resource_config::<Vault>()?;

        // Simulate vault deletion
        if let Some(vault_id) = &self.vault_id {
            info!("✓ [test-vault-delete] Vault `{}` deleted", vault_id);
        }
        self.vault_id = None;

        info!(
            "✓ [test-vault-delete] Vault `{}` deletion completed",
            vault_config.id
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
        self.vault_id.as_ref().map(|vault_id| {
            ResourceOutputs::new(VaultOutputs {
                vault_id: vault_id.clone(),
            })
        })
    }

    fn get_binding_params(&self) -> Result<Option<serde_json::Value>> {
        // Return VaultBinding::Local for test platform
        // This enables BindingsProvider::from_stack_state(...).load_vault("secrets") to work
        if let (Some(vault_id), Some(data_dir)) = (&self.vault_id, &self.data_dir) {
            let binding =
                alien_core::bindings::VaultBinding::local(vault_id.clone(), data_dir.clone());
            Ok(Some(
                serde_json::to_value(binding).into_alien_error().context(
                    ErrorData::ResourceStateSerializationFailed {
                        resource_id: "binding".to_string(),
                        message: "Failed to serialize binding parameters".to_string(),
                    },
                )?,
            ))
        } else {
            Ok(None)
        }
    }
}
