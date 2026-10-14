use std::time::Duration;

use alien_core::{
    bindings::KeyBinding,
    import::{data::AwsKeyImportData, ImportContext},
    AwsKmsKeyHeartbeatData, HeartbeatBackend, Key, KeyFingerprint, KeyHeartbeatData,
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
pub struct AwsKeyController {
    // Older installations have only DescribeKey permission until setup is rerun.
    #[serde(default)]
    pub(crate) availability_probe: bool,
    #[serde(default)]
    pub(crate) available: Option<bool>,
    pub(crate) key_arn: Option<String>,
    pub(crate) region: Option<String>,
}

#[controller]
impl AwsKeyController {
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
        // A failed check keeps the last observation unless AWS answered about the
        // key itself; a throttle or 5xx must not pause every dependent resource.
        let config = ctx.desired_resource_config::<Key>()?;
        let key_arn = self.key_arn.as_deref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceStateSerializationFailed {
                resource_id: config.id.clone(),
                message: "Imported AWS key is missing its ARN".to_string(),
            })
        })?;
        let client = ctx
            .service_provider
            .get_aws_kms_client(ctx.get_aws_config()?)
            .await?;
        let metadata = client
            .describe_key(key_arn)
            .await
            .inspect_err(|error| {
                if super::aws_key_revoked(error) {
                    self.available = Some(false);
                }
            })
            .context(ErrorData::CloudPlatformError {
                message: "Failed to read AWS KMS key metadata".to_string(),
                resource_id: Some(config.id.clone()),
            })?
            .key_metadata;
        if metadata.arn != key_arn {
            return Err(AlienError::new(ErrorData::ResourceDrift {
                resource_id: config.id.clone(),
                message: "AWS KMS returned metadata for a different key".to_string(),
            }));
        }
        let mut usable = metadata.key_state == "Enabled" && metadata.enabled;
        if usable
            && self.availability_probe
            && metadata.origin.as_deref() == Some("EXTERNAL_KEY_STORE")
        {
            // DescribeKey cannot observe the external customer's key. Encrypting
            // public probe text exercises the gateway without permitting data reads.
            usable = alien_aws_clients::kms::probe_external_key(client.as_ref(), key_arn)
                .await
                .inspect_err(|error| {
                    if super::aws_key_revoked(error) {
                        self.available = Some(false);
                    }
                })
                .context(ErrorData::CloudPlatformError {
                    message: "Failed to check customer key availability".to_string(),
                    resource_id: Some(config.id.clone()),
                })?;
        }
        self.available = Some(usable);
        let (health, lifecycle) = match metadata.key_state.as_str() {
            "Enabled" if usable => (ObservedHealth::Healthy, ProviderLifecycleState::Running),
            "Enabled" | "Disabled" => (ObservedHealth::Unhealthy, ProviderLifecycleState::Stopped),
            "PendingDeletion" | "PendingReplicaDeletion" => {
                (ObservedHealth::Unhealthy, ProviderLifecycleState::Deleting)
            }
            _ => (ObservedHealth::Degraded, ProviderLifecycleState::Unknown),
        };
        ctx.emit_heartbeat(ResourceHeartbeat {
            deployment_id: None,
            resource_id: config.id.clone(),
            resource_type: Key::RESOURCE_TYPE,
            controller_platform: Platform::Aws,
            backend: HeartbeatBackend::Aws,
            observed_at: Utc::now(),
            data: ResourceHeartbeatData::Key(KeyHeartbeatData::AwsKms(AwsKmsKeyHeartbeatData {
                status: KeyHeartbeatStatus {
                    health,
                    lifecycle,
                    message: (self.available != Some(true))
                        .then(|| "customer key unavailable".to_string()),
                },
                key_arn: metadata.arn,
                key_state: metadata.key_state,
                enabled: metadata.enabled,
                key_spec: metadata.key_spec,
                key_usage: metadata.key_usage,
            })),
            raw: vec![],
        });
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: Some(Duration::from_secs(30)),
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

    fn requires_convergence_reconciliation(&self) -> bool {
        true
    }

    fn build_outputs(&self) -> Option<ResourceOutputs> {
        let key_arn = self.key_arn.clone()?;
        Some(ResourceOutputs::new(KeyOutputs {
            available: Some(self.available.unwrap_or(false)),
            fingerprint: KeyFingerprint::Aws {
                key_arn: key_arn.clone(),
            },
            wrapping_key_id: key_arn,
        }))
    }

    fn get_binding_params(&self) -> Result<Option<serde_json::Value>> {
        let Some(key_arn) = &self.key_arn else {
            return Ok(None);
        };
        Ok(Some(
            serde_json::to_value(KeyBinding::aws_kms(key_arn, self.region.as_deref()))
                .into_alien_error()
                .context(ErrorData::ResourceStateSerializationFailed {
                    resource_id: "binding".to_string(),
                    message: "Failed to serialize Key binding parameters".to_string(),
                })?,
        ))
    }
}

#[derive(Debug, Default)]
pub struct AwsKeyImporter;

#[cfg(all(test, feature = "test"))]
#[path = "aws_tests.rs"]
mod tests;

impl ResourceImporter for AwsKeyImporter {
    type ImportData = AwsKeyImportData;

    fn merge_reimport(
        &self,
        existing: StackResourceState,
        imported: StackResourceState,
        ctx: &ImportContext<'_>,
    ) -> alien_core::Result<StackResourceState> {
        super::merge_key_reimport(existing, imported, ctx)
    }

    fn import(
        &self,
        data: Self::ImportData,
        ctx: &ImportContext<'_>,
    ) -> alien_core::Result<StackResourceState> {
        make_imported_state(
            AwsKeyController {
                state: AwsKeyState::Ready,
                available: None,
                availability_probe: data.availability_probe,
                key_arn: Some(data.key_arn),
                region: Some(ctx.region.to_string()),
                _internal_stay_count: None,
            },
            ctx,
        )
    }
}
