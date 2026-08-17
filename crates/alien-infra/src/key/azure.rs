use std::time::Duration;

use alien_core::{
    bindings::KeyBinding,
    import::{data::AzureKeyImportData, ImportContext},
    AzureKeyVaultKeyHeartbeatData, HeartbeatBackend, Key, KeyFingerprint, KeyHeartbeatData,
    KeyHeartbeatStatus, KeyOutputs, ObservedHealth, Platform, ProviderLifecycleState,
    ResourceHeartbeat, ResourceHeartbeatData, ResourceOutputs, ResourceStatus, StackResourceState,
};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_macros::controller;
use chrono::Utc;

use crate::{
    core::ResourceControllerContext,
    error::{ErrorData, Result},
    import::ResourceImporter,
    import_helpers::make_imported_state,
};

#[controller]
pub struct AzureKeyController {
    pub(crate) vault_resource_id: Option<String>,
    pub(crate) key_name: Option<String>,
    pub(crate) lineage_version_id: Option<String>,
    pub(crate) key_id: Option<String>,
}

#[controller]
impl AzureKeyController {
    #[flow_entry(Create)]
    #[handler(state = CreateStart, on_failure = CreateFailed, status = ResourceStatus::Provisioning)]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let key = ctx.desired_resource_config::<Key>()?;
        Err(AlienError::new(ErrorData::ResourceConfigInvalid {
            resource_id: Some(key.id.clone()),
            message: "key resources are setup-owned and enter a deployment through stack import"
                .to_string(),
        }))
    }

    #[handler(state = Ready, on_failure = RefreshFailed, status = ResourceStatus::Running)]
    async fn ready(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Key>()?;
        let versioned_key_id = self.key_id.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceStateSerializationFailed {
                resource_id: config.id.clone(),
                message: "Imported Azure key is missing its data-plane key ID".to_string(),
            })
        })?;
        let current_key_id = versioned_key_id
            .rsplit_once('/')
            .map(|(id, _)| id)
            .ok_or_else(|| {
                AlienError::new(ErrorData::ResourceStateSerializationFailed {
                    resource_id: config.id.clone(),
                    message: "Imported Azure key ID does not contain a version".to_string(),
                })
            })?;
        let key = ctx
            .service_provider
            .get_azure_key_vault_keys_client(ctx.get_azure_config()?)?
            .get_key(current_key_id)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to read Azure Key Vault key metadata".to_string(),
                resource_id: Some(config.id.clone()),
            })?;
        if !key.key.kid.starts_with(&format!("{current_key_id}/")) {
            return Err(AlienError::new(ErrorData::ResourceDrift {
                resource_id: config.id.clone(),
                message: "Azure Key Vault returned metadata for a different key".to_string(),
            }));
        }
        self.key_id = Some(key.key.kid.clone());
        let (health, lifecycle) = match key.attributes.enabled {
            Some(true) => (ObservedHealth::Healthy, ProviderLifecycleState::Running),
            Some(false) => (ObservedHealth::Unhealthy, ProviderLifecycleState::Stopped),
            None => (ObservedHealth::Degraded, ProviderLifecycleState::Unknown),
        };
        ctx.emit_heartbeat(ResourceHeartbeat {
            deployment_id: None,
            resource_id: config.id.clone(),
            resource_type: Key::RESOURCE_TYPE,
            controller_platform: Platform::Azure,
            backend: HeartbeatBackend::Azure,
            observed_at: Utc::now(),
            data: ResourceHeartbeatData::Key(KeyHeartbeatData::AzureKeyVault(
                AzureKeyVaultKeyHeartbeatData {
                    status: KeyHeartbeatStatus {
                        health,
                        lifecycle,
                        message: None,
                    },
                    key_id: key.key.kid,
                    enabled: key.attributes.enabled,
                    key_type: key.key.kty,
                    key_operations: key.key.key_ops,
                    recovery_level: key.attributes.recovery_level,
                },
            )),
            raw: vec![],
        });
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: Some(Duration::from_secs(300)),
        })
    }

    terminal_state!(
        state = CreateFailed,
        status = ResourceStatus::ProvisionFailed
    );
    terminal_state!(
        state = RefreshFailed,
        status = ResourceStatus::RefreshFailed
    );
    terminal_state!(state = Deleted, status = ResourceStatus::Deleted);

    fn build_outputs(&self) -> Option<ResourceOutputs> {
        Some(ResourceOutputs::new(KeyOutputs {
            fingerprint: KeyFingerprint::Azure {
                vault_resource_id: self.vault_resource_id.clone()?,
                key_name: self.key_name.clone()?,
                lineage_version_id: self.lineage_version_id.clone()?,
            },
            wrapping_key_id: self.key_id.clone()?,
        }))
    }

    fn get_binding_params(&self) -> Result<Option<serde_json::Value>> {
        let Some(key_id) = &self.key_id else {
            return Ok(None);
        };
        Ok(Some(
            serde_json::to_value(KeyBinding::azure_key_vault(key_id))
                .into_alien_error()
                .context(ErrorData::ResourceStateSerializationFailed {
                    resource_id: "binding".to_string(),
                    message: "Failed to serialize Key binding parameters".to_string(),
                })?,
        ))
    }
}

#[derive(Debug, Default)]
pub struct AzureKeyImporter;

impl ResourceImporter for AzureKeyImporter {
    type ImportData = AzureKeyImportData;

    fn import(
        &self,
        data: Self::ImportData,
        ctx: &ImportContext<'_>,
    ) -> alien_core::Result<StackResourceState> {
        make_imported_state(
            AzureKeyController {
                state: AzureKeyState::Ready,
                vault_resource_id: Some(data.vault_resource_id),
                key_name: Some(data.key_name),
                lineage_version_id: Some(data.lineage_version_id),
                key_id: Some(data.key_id),
                _internal_stay_count: None,
            },
            ctx,
        )
    }
}
