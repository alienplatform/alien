use std::time::Duration;

use tracing::{info, warn};

use crate::core::{ResourceControllerContext, ResourcePermissionsHelper};
use crate::error::{ErrorData, Result};
use alien_aws_clients::bedrock::{
    CreateModelCustomizationJobRequest, ModelCustomizationJobStatus, S3DataConfig,
};
use alien_core::{
    bindings::AiBinding, Ai, AiHeartbeatData, AiHeartbeatStatus, AiOutputs,
    AwsBedrockAiHeartbeatData, FinetuneMethod, HeartbeatBackend, Platform, ResourceHeartbeat,
    ResourceHeartbeatData, ResourceOutputs, ResourceRef, ResourceStatus, Storage,
};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_macros::controller;
use chrono::Utc;

/// Poll interval while a Bedrock model-customization job runs. Matches the
/// heartbeat cadence used elsewhere in this controller.
const TUNING_POLL_INTERVAL: Duration = Duration::from_secs(30);

/// Bedrock `customizationType` for a supervised / preference / LoRA fine-tune.
/// Bedrock's public model-customization API exposes `FINE_TUNING`; the specific
/// technique (SFT vs DPO vs LoRA) is selected per base model via hyperparameters,
/// not a distinct customizationType, so all `FinetuneMethod` variants map here.
fn bedrock_customization_type(_method: FinetuneMethod) -> &'static str {
    "FINE_TUNING"
}

#[controller]
pub struct AwsAiController {
    /// AWS region where Bedrock is accessed. None until create_start runs.
    pub(crate) region: Option<String>,
    /// ARN of the submitted Bedrock model-customization job, set once
    /// `SubmittingTuningJob` succeeds. Used as the poll identifier. Only ever set
    /// for a finetune-enabled resource.
    pub(crate) tuning_job_arn: Option<String>,
    /// The completed custom-model ARN Bedrock returns. This is the `upstream_id`
    /// the OpenAI chat endpoint accepts for the tuned model, and is attached to
    /// the binding via `with_tuned_model`. None until the job completes.
    pub(crate) tuned_model_id: Option<String>,
    /// The gateway-facing served model id the tuned model is exposed under
    /// (`spec.served_model_id_or_default(&config.id)`). Captured when the job
    /// completes so `get_binding_params` (which has no `ctx`) can pair it with
    /// `tuned_model_id`. None for a pure inference gateway.
    pub(crate) served_id: Option<String>,
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

        // A pure inference gateway is Ready as soon as permissions are applied —
        // exactly as before. A finetune-enabled resource additionally submits and
        // polls a Bedrock model-customization job before serving.
        let next_state = if config.finetune.is_some() {
            info!(ai=%config.id, "Finetune requested; submitting Bedrock model-customization job");
            SubmittingTuningJob
        } else {
            Ready
        };

        Ok(HandlerAction::Continue {
            state: next_state,
            suggested_delay: None,
        })
    }

    // ─────────────── FINE-TUNING FLOW ──────────────────────────
    // Only entered when the resource declares a `finetune` spec.

    #[handler(
        state = SubmittingTuningJob,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn submitting_tuning_job(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Ai>()?;
        let spec = config.finetune.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                resource_id: Some(config.id.clone()),
                message: "SubmittingTuningJob reached without a finetune spec".to_string(),
            })
        })?;
        let aws_config = ctx.get_aws_config()?;

        // Resolve the training bucket from the dependency's real state rather than
        // re-deriving the name. The Ai resource declares its training-data Storage
        // as a dependency (see `Ai::get_dependencies`), and the AWS storage
        // controller stores the actual (prefixed) bucket name in `bucket_name`.
        // Reading it here keeps this controller correct even if the storage naming
        // scheme changes.
        let training_ref = ResourceRef::new(Storage::RESOURCE_TYPE, spec.training_data.clone());
        let storage_state = ctx
            .require_dependency::<crate::storage::AwsStorageController>(&training_ref)?;
        let bucket = storage_state.bucket_name.clone().ok_or_else(|| {
            AlienError::new(ErrorData::DependencyNotReady {
                resource_id: config.id.clone(),
                dependency_id: spec.training_data.clone(),
            })
        })?;

        // Bedrock assumes an IAM role to read the training data and write output.
        // The Ai resource carries no dedicated service account, so derive the
        // execution role ARN deterministically from the deployment account id and
        // the stack's resource-role naming convention (`{prefix}-{id}`, matching
        // `service_account::aws::get_aws_role_name`). The `ai/finetune` permission
        // set grants this role the Bedrock job + S3 read actions it needs.
        let role_arn = format!(
            "arn:aws:iam::{}:role/{}-{}",
            aws_config.account_id, ctx.resource_prefix, config.id
        );

        let training_key = &spec.training_key;
        let training_uri = format!("s3://{}/{}", bucket, training_key);
        let output_uri = format!("s3://{}/alien-finetune-output/{}/", bucket, config.id);

        // Bedrock job + custom-model names must match `([0-9a-zA-Z][_-]?){1,63}`;
        // the resource id is already constrained to `[A-Za-z0-9-_]{1,64}`.
        let job_name = format!("{}-{}", ctx.resource_prefix, config.id);
        let custom_model_name = format!("{}-{}", ctx.resource_prefix, config.id);

        let request = CreateModelCustomizationJobRequest::builder()
            .job_name(job_name)
            .custom_model_name(custom_model_name)
            .role_arn(role_arn)
            .base_model_identifier(spec.base_model.clone())
            .customization_type(bedrock_customization_type(spec.method).to_string())
            .training_data_config(S3DataConfig::builder().s3_uri(training_uri).build())
            .output_data_config(S3DataConfig::builder().s3_uri(output_uri).build())
            .build();

        let client = ctx.service_provider.get_aws_bedrock_client(aws_config).await?;
        let response = client
            .create_model_customization_job(&request)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to submit Bedrock model-customization job for AI '{}'",
                    config.id
                ),
                resource_id: Some(config.id.clone()),
            })?;

        info!(ai=%config.id, job_arn=%response.job_arn, "Submitted Bedrock model-customization job");
        self.tuning_job_arn = Some(response.job_arn);

        Ok(HandlerAction::Continue {
            state: WaitingForTuningJob,
            suggested_delay: Some(TUNING_POLL_INTERVAL),
        })
    }

    #[handler(
        state = WaitingForTuningJob,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_tuning_job(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Ai>()?;
        let aws_config = ctx.get_aws_config()?;

        let job_arn = self.tuning_job_arn.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                resource_id: Some(config.id.clone()),
                message: "Tuning job ARN not set in state".to_string(),
            })
        })?;

        let client = ctx.service_provider.get_aws_bedrock_client(aws_config).await?;
        let job = client
            .get_model_customization_job(job_arn)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to poll Bedrock model-customization job '{}' for AI '{}'",
                    job_arn, config.id
                ),
                resource_id: Some(config.id.clone()),
            })?;

        match job.status() {
            ModelCustomizationJobStatus::Completed => {
                // The custom-model ARN is the artifact the gateway forwards to.
                let output_model_arn = job.output_model_arn.clone().ok_or_else(|| {
                    AlienError::new(ErrorData::CloudPlatformError {
                        message: format!(
                            "Bedrock job '{}' completed but returned no outputModelArn",
                            job_arn
                        ),
                        resource_id: Some(config.id.clone()),
                    })
                })?;

                let spec = config.finetune.as_ref().ok_or_else(|| {
                    AlienError::new(ErrorData::ResourceConfigInvalid {
                        resource_id: Some(config.id.clone()),
                        message: "WaitingForTuningJob completed without a finetune spec".to_string(),
                    })
                })?;

                info!(ai=%config.id, custom_model=%output_model_arn, "Bedrock model-customization job completed");
                self.tuned_model_id = Some(output_model_arn);
                self.served_id = Some(spec.served_model_id_or_default(&config.id));

                Ok(HandlerAction::Continue {
                    state: Ready,
                    suggested_delay: None,
                })
            }
            // Fail loud on any terminal failure, mirroring the Azure controller's
            // WaitingForDeployments fail-fast style (don't poll a dead job forever).
            status @ (ModelCustomizationJobStatus::Failed
            | ModelCustomizationJobStatus::Stopped) => {
                let reason = job
                    .failure_message
                    .clone()
                    .unwrap_or_else(|| "no failure message reported".to_string());
                Err(AlienError::new(ErrorData::CloudPlatformError {
                    message: format!(
                        "Bedrock model-customization job '{}' entered terminal state '{:?}': {}",
                        job_arn, status, reason
                    ),
                    resource_id: Some(config.id.clone()),
                }))
            }
            // InProgress / Stopping / any unrecognized status: keep polling.
            other => {
                info!(ai=%config.id, ?other, "Bedrock model-customization job not yet complete, re-polling");
                Ok(HandlerAction::Continue {
                    state: WaitingForTuningJob,
                    suggested_delay: Some(TUNING_POLL_INTERVAL),
                })
            }
        }
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
    // RefreshFailed. The tuned model id persists in controller state across updates.

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

        // Attach the tuned model only once the job has completed and produced both
        // a custom-model id and its served id (both are set together in
        // WaitingForTuningJob). Until then (and for a pure inference gateway) the
        // base binding is emitted unchanged.
        match (&self.tuned_model_id, &self.served_id) {
            (Some(upstream_id), Some(served_id)) => {
                binding = binding.with_tuned_model(served_id.clone(), upstream_id.clone());
            }
            (Some(_), None) => {
                // A tuned artifact without a served id is a controller-state
                // inconsistency (they are set together). Emit the untuned binding
                // rather than serve under an unknown id, and warn loudly.
                warn!("Tuned model id present but served id missing; emitting untuned binding");
            }
            _ => {}
        }

        Ok(Some(serde_json::to_value(binding).into_alien_error().context(
            ErrorData::ResourceStateSerializationFailed {
                resource_id: "binding".to_string(),
                message: "Failed to serialize AI binding parameters".to_string(),
            },
        )?))
    }
}

#[cfg(all(test, feature = "test-utils"))]
mod tests {
    use super::*;
    use crate::core::controller_test::SingleControllerExecutor;
    use crate::core::ResourceController;
    use crate::storage::AwsStorageController;
    use crate::MockPlatformServiceProvider;
    use alien_aws_clients::bedrock::{
        CreateModelCustomizationJobResponse, GetModelCustomizationJobResponse, MockBedrockApi,
    };
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

    /// The completed custom-model ARN the gateway forwards tuned requests to.
    const CUSTOM_MODEL_ARN: &str =
        "arn:aws:bedrock:us-east-1:123456789012:custom-model/amazon.nova-lite-v1:0/abcdef012345";
    const JOB_ARN: &str =
        "arn:aws:bedrock:us-east-1:123456789012:model-customization-job/amazon.nova-lite-v1:0/abcdef012345";

    fn completed_job() -> GetModelCustomizationJobResponse {
        serde_json::from_value(serde_json::json!({
            "status": "Completed",
            "jobArn": JOB_ARN,
            "outputModelArn": CUSTOM_MODEL_ARN,
            "outputModelName": "test-my-ai",
        }))
        .expect("valid completed job json")
    }

    fn in_progress_job() -> GetModelCustomizationJobResponse {
        serde_json::from_value(serde_json::json!({ "status": "InProgress" }))
            .expect("valid in-progress job json")
    }

    fn failed_job() -> GetModelCustomizationJobResponse {
        serde_json::from_value(serde_json::json!({
            "status": "Failed",
            "failureMessage": "training data validation failed",
        }))
        .expect("valid failed job json")
    }

    /// Build a mock Bedrock client that submits the job then returns the given
    /// sequence of poll responses (one per `get_model_customization_job` call).
    fn mock_bedrock(polls: Vec<GetModelCustomizationJobResponse>) -> Arc<MockBedrockApi> {
        let mut mock = MockBedrockApi::new();
        mock.expect_create_model_customization_job().returning(|_| {
            Ok(CreateModelCustomizationJobResponse {
                job_arn: JOB_ARN.to_string(),
            })
        });
        let responses = std::sync::Mutex::new(polls.into_iter());
        mock.expect_get_model_customization_job()
            .returning(move |_| {
                responses
                    .lock()
                    .unwrap()
                    .next()
                    .map(Ok)
                    .unwrap_or_else(|| Ok(completed_job()))
            });
        Arc::new(mock)
    }

    fn provider_with_bedrock(mock: Arc<MockBedrockApi>) -> Arc<MockPlatformServiceProvider> {
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_aws_bedrock_client()
            .returning(move |_| Ok(mock.clone()));
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

    #[tokio::test]
    async fn test_finetune_submit_poll_succeeds_and_binds_tuned_model() {
        // Submit -> InProgress -> Completed -> Ready, then the binding must carry
        // the tuned model with the right served + upstream ids.
        let mock = mock_bedrock(vec![in_progress_job(), completed_job()]);
        let provider = provider_with_bedrock(mock);

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

        // Ready is not terminal (heartbeat loop), so step until Running.
        for _ in 0..8 {
            if executor.status() == ResourceStatus::Running {
                break;
            }
            executor.step().await.expect("step should not error");
        }
        assert_eq!(
            executor.status(),
            ResourceStatus::Running,
            "finetune resource must reach Ready after the job completes"
        );

        let binding = current_binding(&executor);
        let tuned = binding
            .tuned_model()
            .expect("completed finetune must attach a tuned model");
        assert_eq!(
            tuned.served_id, "my-ai-tuned",
            "served id must default to <ai-id>-tuned"
        );
        assert_eq!(
            tuned.upstream_id, CUSTOM_MODEL_ARN,
            "upstream id must be the completed custom-model ARN"
        );
    }

    #[tokio::test]
    async fn test_finetune_job_failed_fails_fast_to_create_failed() {
        // A terminal Failed status must route to CreateFailed, not poll forever.
        let mock = mock_bedrock(vec![failed_job()]);
        let provider = provider_with_bedrock(mock);

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

        // A terminal Failed job must surface as a handler error (which the real
        // executor routes to CreateFailed via `on_failure`), NOT an endless poll.
        // The test harness's `step()` returns that error rather than applying the
        // failure transition, so assert the error surfaces. Bounded so a
        // poll-forever regression fails the test instead of hanging.
        let mut surfaced_error = false;
        for _ in 0..10 {
            if executor.step().await.is_err() {
                surfaced_error = true;
                break;
            }
        }
        assert!(
            surfaced_error,
            "a terminal Failed job must surface as an error, not a silent retry"
        );
    }

    #[tokio::test]
    async fn test_no_finetune_reaches_ready_with_untuned_binding() {
        // Regression: an inference-only resource must never touch Bedrock tuning
        // and must emit an untuned binding.
        let mut provider = MockPlatformServiceProvider::new();
        provider.expect_get_aws_bedrock_client().never();
        let provider = Arc::new(provider);

        let mut executor = SingleControllerExecutor::builder()
            .resource(untuned_ai())
            .controller(AwsAiController::default())
            .platform(Platform::Aws)
            .service_provider(provider)
            .with_test_dependencies()
            .build()
            .await
            .expect("executor builds");

        for _ in 0..6 {
            if executor.status() == ResourceStatus::Running {
                break;
            }
            executor.step().await.expect("step should not error");
        }
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
    }

    #[tokio::test]
    async fn test_finetune_uses_training_bucket_and_customization_type() {
        // Assert the submitted job carries the dependency's real bucket in both the
        // training and output S3 URIs, and FINE_TUNING as the customization type.
        let bucket = training_bucket_name();
        let expected_training_uri = format!("s3://{}/training.jsonl", bucket);
        let expected_output_prefix = format!("s3://{}/alien-finetune-output/my-ai/", bucket);

        let mut mock = MockBedrockApi::new();
        mock.expect_create_model_customization_job()
            .withf(move |req| {
                req.customization_type == "FINE_TUNING"
                    && req.base_model_identifier == "amazon.nova-lite-v1:0"
                    && req.training_data_config.s3_uri == expected_training_uri
                    && req.output_data_config.s3_uri == expected_output_prefix
                    && req.role_arn == "arn:aws:iam::123456789012:role/test-my-ai"
            })
            .returning(|_| {
                Ok(CreateModelCustomizationJobResponse {
                    job_arn: JOB_ARN.to_string(),
                })
            });
        mock.expect_get_model_customization_job()
            .returning(|_| Ok(completed_job()));

        let provider = provider_with_bedrock(Arc::new(mock));

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

        // Reaching Running proves the withf predicate matched (a mismatch panics
        // the mock, which surfaces as a step error and never reaches Running).
        for _ in 0..8 {
            if executor.status() == ResourceStatus::Running {
                break;
            }
            executor.step().await.expect("step should not error");
        }
        assert_eq!(executor.status(), ResourceStatus::Running);
    }
}
