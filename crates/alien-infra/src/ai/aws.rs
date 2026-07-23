use std::time::Duration;

use tracing::info;

use crate::core::{ResourceControllerContext, ResourcePermissionsHelper};
use crate::error::{ErrorData, Result};
use alien_core::{
    bindings::{AiBinding, FinetuneCapability},
    Ai, AiHeartbeatData, AiHeartbeatStatus, AiOutputs, AwsBedrockAiHeartbeatData, HeartbeatBackend,
    Platform, ResourceHeartbeat, ResourceHeartbeatData, ResourceOutputs, ResourceRef,
    ResourceStatus, Storage,
};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_macros::controller;
use chrono::Utc;

#[controller]
pub struct AwsAiController {
    /// AWS region where Bedrock is accessed. None until create_start runs.
    pub(crate) region: Option<String>,
    /// The fine-tuning capability this resource carries on its binding, resolved
    /// during the create flow from the training-storage dependency and the
    /// deterministic finetune role name. `get_binding_params` is a pure function of
    /// controller state (no `ctx`), so it is captured here rather than resolved
    /// live. None for a pure inference gateway.
    pub(crate) finetune: Option<FinetuneCapability>,
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

        // A finetune-enabled resource carries a `FinetuneCapability` on its binding so
        // the gateway can submit and rediscover a runtime tuning job. Resolve it here
        // (where `ctx` is available) and stash it on the controller; `get_binding_params`
        // is a pure function of state and reads it back. The tuning job itself is NOT
        // submitted here — it is triggered at runtime via the gateway API — so the
        // resource is Ready as soon as permissions are applied, whether or not it
        // declares a `finetune` spec.
        self.finetune = resolve_finetune_capability(ctx).await?;

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
    // Ai has no mutable inference fields, and finetune base/training are immutable
    // (enforced in `Ai::validate_update`), so update is a no-op that also recovers
    // RefreshFailed. The resolved finetune capability persists in state across updates.

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

        let mut binding = AiBinding::bedrock(region);

        // A finetune-enabled resource carries a `FinetuneCapability` so the gateway
        // can submit and rediscover a runtime tuning job. No tuned model is attached
        // here — the gateway rediscovers it by convention once a runtime job
        // succeeds. A pure inference gateway emits the plain binding unchanged.
        if let Some(capability) = &self.finetune {
            binding = binding.with_finetune(capability.clone());
        }

        Ok(Some(serde_json::to_value(binding).into_alien_error().context(
            ErrorData::ResourceStateSerializationFailed {
                resource_id: "binding".to_string(),
                message: "Failed to serialize AI binding parameters".to_string(),
            },
        )?))
    }
}

/// Resolve the fine-tuning capability from the declared `finetune` spec, the
/// training-storage dependency's real bucket name, and the deterministic
/// Bedrock-trusted finetune role. Returns `None` for a pure inference gateway.
async fn resolve_finetune_capability(
    ctx: &ResourceControllerContext<'_>,
) -> Result<Option<FinetuneCapability>> {
    let config = ctx.desired_resource_config::<Ai>()?;
    let Some(spec) = config.finetune.as_ref() else {
        return Ok(None);
    };
    let aws_config = ctx.get_aws_config()?;

    // Resolve the training bucket from the dependency's real state rather than
    // re-deriving the name. The Ai resource declares its training-data Storage as a
    // dependency (see `Ai::get_dependencies`), and the AWS storage controller stores
    // the actual (prefixed) bucket name in `bucket_name`.
    let training_ref = ResourceRef::new(Storage::RESOURCE_TYPE, spec.training_data.clone());
    let storage_state =
        ctx.require_dependency::<crate::storage::AwsStorageController>(&training_ref)?;
    let training_bucket = storage_state.bucket_name.clone().ok_or_else(|| {
        AlienError::new(ErrorData::DependencyNotReady {
            resource_id: config.id.clone(),
            dependency_id: spec.training_data.clone(),
        })
    })?;

    Ok(Some(FinetuneCapability {
        base_model: spec.base_model.clone(),
        training_bucket,
        training_key: spec.training_key.clone(),
        served_model_id: spec.served_model_id_or_default(&config.id),
        job_name: format!("{}-{}", ctx.resource_prefix, config.id),
        // The dedicated Bedrock-trusted finetune role emitted by the AWS emitter
        // (`{prefix}-{id}-finetune`). The gateway passes this ARN as the tuning job's
        // `roleArn` so Bedrock can read training data and write output.
        role_arn: format!(
            "arn:aws:iam::{}:role/{}-{}-finetune",
            aws_config.account_id, ctx.resource_prefix, config.id
        ),
    }))
}

#[cfg(all(test, feature = "test-utils"))]
mod tests {
    use super::*;
    use crate::core::controller_test::SingleControllerExecutor;
    use crate::core::ResourceController;
    use crate::storage::AwsStorageController;
    use crate::MockPlatformServiceProvider;
    use alien_core::bindings::AiBinding;
    use alien_core::{Ai, FinetuneMethod, FinetuneSpec, Platform, ResourceStatus, Storage};
    use std::sync::Arc;

    // The training-data storage the finetune resource depends on. Its AWS storage
    // controller mock is wired with a real bucket name so the AI controller can
    // read it via `require_dependency`.
    const TRAINING_STORAGE_ID: &str = "training-set";

    fn training_storage() -> Storage {
        Storage::new(TRAINING_STORAGE_ID.to_string()).build()
    }

    fn tuned_ai() -> Ai {
        Ai::new("my-ai".to_string())
            .finetune(FinetuneSpec {
                base_model: "amazon.nova-lite-v1:0".to_string(),
                training_data: TRAINING_STORAGE_ID.to_string(),
                training_key: "training.jsonl".to_string(),
                served_model_id: None,
                method: FinetuneMethod::Sft,
            })
            .build()
    }

    fn untuned_ai() -> Ai {
        Ai::new("my-ai".to_string()).build()
    }

    /// A provider that must never touch Bedrock: the runtime model submits tuning
    /// jobs from the gateway, never from the controller, so the controller must not
    /// construct a Bedrock client at deploy time.
    fn provider_without_bedrock() -> Arc<MockPlatformServiceProvider> {
        let mut provider = MockPlatformServiceProvider::new();
        provider.expect_get_aws_bedrock_client().never();
        Arc::new(provider)
    }

    /// The real (prefixed) bucket name the storage dependency mock resolves to.
    /// `AwsStorageController::mock_ready` prefixes with "test-stack".
    fn training_bucket_name() -> String {
        format!("test-stack-{}", TRAINING_STORAGE_ID)
    }

    /// Read the controller's current binding via the ResourceController trait.
    fn current_binding(executor: &SingleControllerExecutor) -> AiBinding {
        let controller = executor
            .internal_state::<AwsAiController>()
            .expect("controller is AwsAiController");
        let value = controller
            .get_binding_params()
            .expect("binding params resolve")
            .expect("binding present once region is set");
        serde_json::from_value(value).expect("binding deserializes")
    }

    /// Step the executor until it reaches Running (Ready is a heartbeat loop, not a
    /// terminal state) or the bound is exhausted.
    async fn drive_to_running(executor: &mut SingleControllerExecutor, steps: usize) {
        for _ in 0..steps {
            if executor.status() == ResourceStatus::Running {
                return;
            }
            executor.step().await.expect("step should not error");
        }
    }

    #[tokio::test]
    async fn test_finetune_reaches_ready_immediately_with_capability_and_no_job() {
        // The runtime model: a finetune resource reaches Ready as soon as
        // permissions are applied — NO Bedrock job is submitted at deploy time — and
        // its binding carries a FinetuneCapability with the resolved training bucket
        // and the dedicated `-finetune` role ARN.
        let provider = provider_without_bedrock();

        let mut executor = SingleControllerExecutor::builder()
            .resource(tuned_ai())
            .controller(AwsAiController::default())
            .platform(Platform::Aws)
            .service_provider(provider)
            .with_dependency(
                training_storage(),
                AwsStorageController::mock_ready(TRAINING_STORAGE_ID),
            )
            .build()
            .await
            .expect("executor builds");

        drive_to_running(&mut executor, 6).await;
        assert_eq!(
            executor.status(),
            ResourceStatus::Running,
            "a finetune resource must reach Ready immediately (no deploy-time job)"
        );

        let binding = current_binding(&executor);
        assert!(
            binding.tuned_model().is_none(),
            "no tuned model is attached at deploy time; the gateway rediscovers it"
        );

        let capability = binding
            .finetune()
            .expect("finetune resource must carry a FinetuneCapability");
        assert_eq!(capability.base_model, "amazon.nova-lite-v1:0");
        assert_eq!(
            capability.training_bucket,
            training_bucket_name(),
            "training bucket must come from the storage dependency's real state"
        );
        assert_eq!(capability.training_key, "training.jsonl");
        assert_eq!(
            capability.served_model_id, "my-ai-tuned",
            "served id must default to <ai-id>-tuned"
        );
        assert_eq!(
            capability.job_name, "test-my-ai",
            "job name must be the deterministic {{prefix}}-{{id}}"
        );
        assert_eq!(
            capability.role_arn, "arn:aws:iam::123456789012:role/test-my-ai-finetune",
            "role ARN must name the dedicated Bedrock-trusted `-finetune` role"
        );
    }

    #[tokio::test]
    async fn test_no_finetune_reaches_ready_with_plain_binding() {
        // Regression: an inference-only resource never touches Bedrock tuning and
        // emits a plain binding with neither a tuned model nor a capability.
        let provider = provider_without_bedrock();

        let mut executor = SingleControllerExecutor::builder()
            .resource(untuned_ai())
            .controller(AwsAiController::default())
            .platform(Platform::Aws)
            .service_provider(provider)
            .with_test_dependencies()
            .build()
            .await
            .expect("executor builds");

        drive_to_running(&mut executor, 6).await;
        assert_eq!(
            executor.status(),
            ResourceStatus::Running,
            "inference-only resource must reach Ready"
        );

        let binding = current_binding(&executor);
        assert!(
            binding.tuned_model().is_none(),
            "inference-only binding must omit tuned_model"
        );
        assert!(
            binding.finetune().is_none(),
            "inference-only binding must omit the finetune capability"
        );
    }
}
