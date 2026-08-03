use std::time::Duration;

use alien_core::{
    bindings::KeyBinding,
    import::{data::AwsKeyImportData, ImportContext},
    Key, KeyFingerprint, KeyOutputs, ResourceOutputs, ResourceStatus, StackResourceState,
};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_macros::controller;

use crate::{
    core::ResourceControllerContext,
    error::{ErrorData, Result},
    import::ResourceImporter,
    import_helpers::make_imported_state,
};

#[controller]
pub struct AwsKeyController {
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
    async fn ready(&mut self, _ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
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
        let key_arn = self.key_arn.clone()?;
        Some(ResourceOutputs::new(KeyOutputs {
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

impl ResourceImporter for AwsKeyImporter {
    type ImportData = AwsKeyImportData;

    fn import(
        &self,
        data: Self::ImportData,
        ctx: &ImportContext<'_>,
    ) -> alien_core::Result<StackResourceState> {
        make_imported_state(
            AwsKeyController {
                state: AwsKeyState::Ready,
                key_arn: Some(data.key_arn),
                region: Some(ctx.region.to_string()),
                _internal_stay_count: None,
            },
            ctx,
        )
    }
}
