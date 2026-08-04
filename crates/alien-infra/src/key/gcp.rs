use std::time::Duration;

use alien_core::{
    bindings::KeyBinding,
    import::{data::GcpKeyImportData, ImportContext},
    GcpCloudKmsKeyHeartbeatData, HeartbeatBackend, Key, KeyFingerprint, KeyHeartbeatData,
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
pub struct GcpKeyController {
    pub(crate) crypto_key_name: Option<String>,
    pub(crate) primary_version: Option<String>,
}

#[controller]
impl GcpKeyController {
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
        let crypto_key_name = self.crypto_key_name.as_deref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceStateSerializationFailed {
                resource_id: config.id.clone(),
                message: "Imported GCP key is missing its CryptoKey name".to_string(),
            })
        })?;
        let key = ctx
            .service_provider
            .get_gcp_cloud_kms_client(ctx.get_gcp_config()?)?
            .get_crypto_key(crypto_key_name)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to read GCP Cloud KMS key metadata".to_string(),
                resource_id: Some(config.id.clone()),
            })?;
        if key.name != crypto_key_name {
            return Err(AlienError::new(ErrorData::ResourceDrift {
                resource_id: config.id.clone(),
                message: "GCP Cloud KMS returned metadata for a different key".to_string(),
            }));
        }
        let primary_state = key.primary.as_ref().map(|version| version.state.as_str());
        let (health, lifecycle) = match primary_state {
            Some("ENABLED") => (ObservedHealth::Healthy, ProviderLifecycleState::Running),
            Some("DISABLED") => (ObservedHealth::Unhealthy, ProviderLifecycleState::Stopped),
            Some("DESTROY_SCHEDULED") | Some("DESTROYED") => {
                (ObservedHealth::Unhealthy, ProviderLifecycleState::Deleting)
            }
            _ => (ObservedHealth::Degraded, ProviderLifecycleState::Unknown),
        };
        self.primary_version = key.primary.as_ref().map(|version| version.name.clone());
        ctx.emit_heartbeat(ResourceHeartbeat {
            deployment_id: None,
            resource_id: config.id.clone(),
            resource_type: Key::RESOURCE_TYPE,
            controller_platform: Platform::Gcp,
            backend: HeartbeatBackend::Gcp,
            observed_at: Utc::now(),
            data: ResourceHeartbeatData::Key(KeyHeartbeatData::GcpCloudKms(
                GcpCloudKmsKeyHeartbeatData {
                    status: KeyHeartbeatStatus {
                        health,
                        lifecycle,
                        message: None,
                    },
                    crypto_key_name: key.name,
                    purpose: key.purpose,
                    primary_version: key.primary.as_ref().map(|version| version.name.clone()),
                    primary_state: key.primary.as_ref().map(|version| version.state.clone()),
                    algorithm: key.primary.map(|version| version.algorithm),
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
            fingerprint: KeyFingerprint::Gcp {
                crypto_key_name: self.crypto_key_name.clone()?,
            },
            wrapping_key_id: self.primary_version.clone()?,
        }))
    }

    fn get_binding_params(&self) -> Result<Option<serde_json::Value>> {
        let Some(crypto_key_name) = &self.crypto_key_name else {
            return Ok(None);
        };
        Ok(Some(
            serde_json::to_value(KeyBinding::gcp_cloud_kms(crypto_key_name))
                .into_alien_error()
                .context(ErrorData::ResourceStateSerializationFailed {
                    resource_id: "binding".to_string(),
                    message: "Failed to serialize Key binding parameters".to_string(),
                })?,
        ))
    }
}

#[derive(Debug, Default)]
pub struct GcpKeyImporter;

impl ResourceImporter for GcpKeyImporter {
    type ImportData = GcpKeyImportData;

    fn import(
        &self,
        data: Self::ImportData,
        ctx: &ImportContext<'_>,
    ) -> alien_core::Result<StackResourceState> {
        make_imported_state(
            GcpKeyController {
                state: GcpKeyState::Ready,
                crypto_key_name: Some(data.crypto_key_name),
                primary_version: Some(data.primary_version),
                _internal_stay_count: None,
            },
            ctx,
        )
    }
}
