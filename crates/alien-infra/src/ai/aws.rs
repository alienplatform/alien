use std::time::Duration;

use tracing::info;

use crate::core::{ResourceControllerContext, ResourcePermissionsHelper};
use crate::error::{ErrorData, Result};
use alien_core::{
    bindings::AiBinding, Ai, AiHeartbeatData, AiHeartbeatStatus, AiOutputs,
    AwsBedrockAiHeartbeatData, HeartbeatBackend, Platform, ResourceHeartbeat,
    ResourceHeartbeatData, ResourceOutputs, ResourceStatus,
};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_macros::controller;
use chrono::Utc;

#[controller]
pub struct AwsAiController {
    /// AWS region where Bedrock is accessed. None until create_start runs.
    pub(crate) region: Option<String>,
}

#[controller]
impl AwsAiController {
    // ─────────────── CREATE FLOW ──────────────────────────────

    #[flow_entry(Create)]
    #[handler(
        state = CreateStart,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Ai>()?;
        let aws_config = ctx.get_aws_config()?;
        self.region = Some(aws_config.region.clone());

        info!(id=%config.id, region=%aws_config.region, "AWS AI (Bedrock) controller: no resource to create, applying permissions");

        Ok(HandlerAction::Continue {
            state: ApplyingResourcePermissions,
            suggested_delay: None,
        })
    }

    #[handler(
        state = ApplyingResourcePermissions,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn applying_resource_permissions(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Ai>()?;

        info!(ai=%config.id, "Applying resource-scoped permissions for Bedrock AI gateway");

        // Bedrock invoke grants use foundation-model/* ARNs (not per-resource ARNs).
        // config.id is passed as resource_name; the ai/invoke permission set binding
        // uses `arn:aws:bedrock:*::foundation-model/*` which is region/account-wide.
        ResourcePermissionsHelper::apply_aws_resource_scoped_permissions(
            ctx, &config.id, &config.id, "ai",
        )
        .await?;

        info!(ai=%config.id, "Successfully applied resource-scoped permissions");

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    // ─────────────── READY STATE ────────────────────────────────
    // Loops as a heartbeat tick; Bedrock has no per-stack resource to poll.

    #[handler(
        state = Ready,
        on_failure = RefreshFailed,
        status = ResourceStatus::Running,
    )]
    async fn ready(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Ai>()?;
        let region = self.region.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                resource_id: Some(config.id.clone()),
                message: "Region not set in state".to_string(),
            })
        })?;
        info!(id=%config.id, "AWS AI heartbeat tick");
        ctx.emit_heartbeat(ResourceHeartbeat {
            deployment_id: None,
            resource_id: config.id.clone(),
            resource_type: Ai::RESOURCE_TYPE,
            controller_platform: Platform::Aws,
            backend: HeartbeatBackend::Aws,
            observed_at: Utc::now(),
            data: ResourceHeartbeatData::Ai(AiHeartbeatData::AwsBedrock(
                AwsBedrockAiHeartbeatData {
                    status: AiHeartbeatStatus::default(),
                    region: region.clone(),
                },
            )),
            raw: vec![],
        });
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: Some(Duration::from_secs(30)),
        })
    }

    // ─────────────── UPDATE FLOW ──────────────────────────────
    // Ai has no mutable fields -- update is a no-op that also recovers RefreshFailed.

    #[flow_entry(Update, from = [Ready, RefreshFailed])]
    #[handler(
        state = UpdateStart,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Ai>()?;
        info!(id=%config.id, "AWS AI update (no-op -- no mutable fields)");
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    // ─────────────── DELETE FLOW ──────────────────────────────
    // AWS AI creates no cloud resource; deletion is always a no-op.

    #[flow_entry(Delete)]
    #[handler(
        state = DeleteStart,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn delete_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Ai>()?;
        info!(id=%config.id, "AWS AI delete (no-op -- Bedrock has no per-stack resource to remove)");
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

    terminal_state!(
        state = RefreshFailed,
        status = ResourceStatus::RefreshFailed
    );

    terminal_state!(state = Deleted, status = ResourceStatus::Deleted);

    fn build_outputs(&self) -> Option<ResourceOutputs> {
        let region = self.region.as_ref()?;
        Some(ResourceOutputs::new(AiOutputs {
            provider: "bedrock".into(),
            endpoint: Some(format!("https://bedrock-mantle.{}.api.aws/v1", region)),
            account: None,
        }))
    }

    fn get_binding_params(&self) -> Result<Option<serde_json::Value>> {
        let region = match &self.region {
            Some(r) => r,
            None => return Ok(None),
        };
        Ok(Some(
            serde_json::to_value(AiBinding::bedrock(region))
                .into_alien_error()
                .context(ErrorData::ResourceStateSerializationFailed {
                    resource_id: "binding".to_string(),
                    message: "Failed to serialize AI binding parameters".to_string(),
                })?,
        ))
    }
}
