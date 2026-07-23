use std::time::Duration;

use tracing::info;

use crate::core::{ResourceControllerContext, ResourcePermissionsHelper};
use crate::error::{ErrorData, Result};
use crate::storage::GcpStorageController;
use alien_core::{
    bindings::{AiBinding, FinetuneCapability},
    Ai, AiHeartbeatData, AiHeartbeatStatus, AiOutputs, GcpVertexAiHeartbeatData, HeartbeatBackend,
    Platform, ResourceHeartbeat, ResourceHeartbeatData, ResourceOutputs, ResourceRef,
    ResourceStatus, Storage,
};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_gcp_clients::iam::IamPolicy;
use alien_gcp_clients::resource_manager::GetPolicyOptions;
use alien_macros::controller;
use chrono::Utc;

#[controller]
pub struct GcpAiController {
    /// GCP project ID. None until create_start runs.
    pub(crate) project: Option<String>,
    /// GCP region (location) for the Vertex AI endpoint. None until create_start runs.
    pub(crate) location: Option<String>,
    /// The fine-tuning capability this gateway advertises, resolved during the
    /// create flow when the resource declares a `finetune` spec. The gateway
    /// submits and rediscovers the tuning job at runtime from this capability;
    /// the controller never starts a job itself. Captured into state (rather than
    /// rebuilt in `get_binding_params`, which has no `ctx`) so the binding can
    /// carry it. `None` for a pure-inference gateway.
    #[serde(default)]
    pub(crate) finetune: Option<FinetuneCapability>,
}

#[controller]
impl GcpAiController {
    // ─────────────── CREATE FLOW ──────────────────────────────

    #[flow_entry(Create)]
    #[handler(
        state = CreateStart,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_start(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let gcp_config = ctx.get_gcp_config()?;
        let config = ctx.desired_resource_config::<Ai>()?;

        self.project = Some(gcp_config.project_id.clone());
        self.location = Some(gcp_config.region.clone());

        info!(
            id = %config.id,
            project = %gcp_config.project_id,
            location = %gcp_config.region,
            "GCP AI (Vertex AI) controller: enabling API"
        );

        Ok(HandlerAction::Continue {
            state: EnablingApi,
            suggested_delay: None,
        })
    }

    #[handler(
        state = EnablingApi,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn enabling_api(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let gcp_config = ctx.get_gcp_config()?;
        let config = ctx.desired_resource_config::<Ai>()?;
        let client = ctx
            .service_provider
            .get_gcp_service_usage_client(gcp_config)?;

        info!(
            id = %config.id,
            project = %gcp_config.project_id,
            "Enabling aiplatform.googleapis.com API"
        );

        client
            .enable_service("aiplatform.googleapis.com".to_string())
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to enable aiplatform.googleapis.com".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        info!(
            id = %config.id,
            "aiplatform.googleapis.com enabled (or already enabled)"
        );

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
        let gcp_config = ctx.get_gcp_config()?;
        let config = ctx.desired_resource_config::<Ai>()?;
        let rm_client = ctx
            .service_provider
            .get_gcp_resource_manager_client(gcp_config)?;
        let project_id = gcp_config.project_id.clone();
        let config_id = config.id.clone();

        info!(
            id = %config.id,
            project = %project_id,
            "Applying Vertex AI resource-scoped permissions (custom predict-only role)"
        );

        ResourcePermissionsHelper::apply_gcp_resource_scoped_permissions(
            ctx,
            &config.id,
            &config.id,
            "GCP AI",
            "ai",
            rm_client,
            |rm_client, desired_policy| async move {
                // Project-level IAM requires read-modify-write to avoid clobbering
                // bindings owned by other controllers.
                let current_policy = rm_client
                    .get_project_iam_policy(
                        project_id.clone(),
                        Some(GetPolicyOptions {
                            requested_policy_version: Some(3),
                        }),
                    )
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: "Failed to get project IAM policy before applying AI permissions"
                            .to_string(),
                        resource_id: Some(config_id.clone()),
                    })?;

                let owned_exact_roles =
                    ResourcePermissionsHelper::gcp_predefined_role_names(&desired_policy.bindings);
                let mut all_bindings = current_policy.bindings;

                // Reconcile each member/role binding separately so we only touch what
                // belongs to this stack's workload service accounts.
                for desired_binding in &desired_policy.bindings {
                    for member in &desired_binding.members {
                        ResourcePermissionsHelper::reconcile_gcp_project_member_bindings(
                            &mut all_bindings,
                            vec![desired_binding.clone()],
                            member,
                            &[],
                            &owned_exact_roles,
                        );
                    }
                }

                let new_policy = IamPolicy::builder()
                    .version(3)
                    .bindings(all_bindings)
                    .maybe_etag(current_policy.etag)
                    .maybe_kind(current_policy.kind)
                    .maybe_resource_id(current_policy.resource_id)
                    .build();

                rm_client
                    .set_project_iam_policy(project_id.clone(), new_policy, None)
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: "Failed to set project IAM policy for Vertex AI".to_string(),
                        resource_id: Some(config_id.clone()),
                    })?;

                info!(
                    project = %project_id,
                    "Applied the custom predict-only role at project scope for Vertex AI"
                );

                Ok(())
            },
        )
        .await?;

        // Fine-tuning is triggered at runtime through the gateway, not at deploy
        // time, so the resource is Ready as soon as permissions are applied. When
        // the resource declares a `finetune` spec, resolve the capability the
        // gateway needs to submit/rediscover a runtime tuning job and carry it on
        // the binding.
        self.finetune = self.resolve_finetune_capability(ctx, &config)?;

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    // ─────────────── READY STATE ────────────────────────────────
    // Loops as a heartbeat tick; Vertex AI has no per-stack resource to poll.

    #[handler(
        state = Ready,
        on_failure = RefreshFailed,
        status = ResourceStatus::Running,
    )]
    async fn ready(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Ai>()?;
        let project = self.project.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                resource_id: Some(config.id.clone()),
                message: "Project not set in state".to_string(),
            })
        })?;
        let location = self.location.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                resource_id: Some(config.id.clone()),
                message: "Location not set in state".to_string(),
            })
        })?;
        info!(id = %config.id, "GCP AI heartbeat tick");
        ctx.emit_heartbeat(ResourceHeartbeat {
            deployment_id: None,
            resource_id: config.id.clone(),
            resource_type: Ai::RESOURCE_TYPE,
            controller_platform: Platform::Gcp,
            backend: HeartbeatBackend::Gcp,
            observed_at: Utc::now(),
            data: ResourceHeartbeatData::Ai(AiHeartbeatData::GcpVertex(
                GcpVertexAiHeartbeatData {
                    status: AiHeartbeatStatus::default(),
                    project: project.clone(),
                    location: location.clone(),
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
    async fn update_start(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Ai>()?;
        info!(id = %config.id, "GCP AI update (no-op -- no mutable fields)");
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    // ─────────────── DELETE FLOW ──────────────────────────────
    // GCP AI creates no cloud resource; deletion is always a no-op.
    // The shared aiplatform API is not disabled on delete (other stacks may use it).

    #[flow_entry(Delete)]
    #[handler(
        state = DeleteStart,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn delete_start(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Ai>()?;
        info!(
            id = %config.id,
            "GCP AI delete (no-op -- Vertex AI has no per-stack resource to remove)"
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

    terminal_state!(
        state = RefreshFailed,
        status = ResourceStatus::RefreshFailed
    );

    terminal_state!(state = Deleted, status = ResourceStatus::Deleted);

    fn build_outputs(&self) -> Option<ResourceOutputs> {
        let project = self.project.as_ref()?;
        let location = self.location.as_ref()?;
        Some(ResourceOutputs::new(AiOutputs {
            provider: "vertex".into(),
            endpoint: Some(format!(
                "https://{location}-aiplatform.googleapis.com/v1/projects/{project}/locations/{location}/endpoints/openapi"
            )),
            account: None,
        }))
    }

    fn get_binding_params(&self) -> Result<Option<serde_json::Value>> {
        // Not-ready is Ok(None), not Err: the executor calls this on every step
        // commit (including the pre-provision path), and routing a missing
        // project/location through the error channel would corrupt its retry
        // accounting. Err is reserved for a real serialization failure below.
        let (Some(project), Some(location)) = (self.project.as_ref(), self.location.as_ref())
        else {
            return Ok(None);
        };

        let mut binding = AiBinding::vertex(project, location);

        // Carry the fine-tuning capability when the resource declared one, so the
        // gateway can submit/rediscover a runtime tuning job. A pure-inference
        // gateway returns the untuned binding unchanged. The controller never
        // attaches a `tuned_model` — the gateway rediscovers it by convention.
        if let Some(capability) = self.finetune.as_ref() {
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

impl GcpAiController {
    /// Builds the fine-tuning capability the binding carries when the resource
    /// declares a `finetune` spec, or `None` for a pure-inference gateway.
    ///
    /// Resolves the training data's real GCS bucket from the storage dependency's
    /// controller state (rather than re-deriving the prefixed name), keeping it in
    /// lockstep with the storage controller's naming. Vertex submits the runtime
    /// tuning job under the gateway's ambient identity, so no role is passed
    /// (`role_arn` is empty).
    fn resolve_finetune_capability(
        &self,
        ctx: &ResourceControllerContext<'_>,
        config: &Ai,
    ) -> Result<Option<FinetuneCapability>> {
        let Some(spec) = config.finetune.as_ref() else {
            return Ok(None);
        };

        let training_ref = ResourceRef::new(Storage::RESOURCE_TYPE, spec.training_data.clone());
        let storage_state = ctx.require_dependency::<GcpStorageController>(&training_ref)?;
        let training_bucket = storage_state.bucket_name.ok_or_else(|| {
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
            // Vertex submits under the ambient identity; no passed role.
            role_arn: String::new(),
        }))
    }
}

#[cfg(test)]
mod tests {
    //! GCP Vertex AI controller tests.
    //!
    //! Fine-tuning is triggered at runtime through the gateway, not at deploy
    //! time, so the controller reaches Ready immediately and never submits a
    //! Vertex tuning job. These assert that a finetune resource reaches Ready
    //! with no job created and that its binding carries the `FinetuneCapability`
    //! the gateway needs, plus the pure-inference regression (no finetune ->
    //! Ready with a plain binding).

    use std::sync::Arc;

    use super::GcpAiController;
    use alien_core::bindings::AiBinding;
    use alien_core::{Ai, FinetuneMethod, FinetuneSpec, Platform, ResourceStatus, Storage};
    use alien_gcp_clients::iam::IamPolicy;
    use alien_gcp_clients::longrunning::Operation;
    use alien_gcp_clients::resource_manager::MockResourceManagerApi;
    use alien_gcp_clients::service_usage::MockServiceUsageApi;

    use crate::core::controller_test::SingleControllerExecutor;
    use crate::core::{MockPlatformServiceProvider, PlatformServiceProvider, ResourceController};
    use crate::storage::GcpStorageController;

    const TRAINING_STORAGE_ID: &str = "training-set";

    // ─────────────── FIXTURES ──────────────────────────────────

    /// A pure-inference gateway (no finetune).
    fn base_ai() -> Ai {
        Ai::new("llm".to_string()).build()
    }

    /// An Ai that declares a fine-tuning capability over the training storage.
    fn finetune_ai() -> Ai {
        Ai::new("llm".to_string())
            .finetune(FinetuneSpec {
                base_model: "gemini-2.0-flash-001".to_string(),
                training_data: TRAINING_STORAGE_ID.to_string(),
                training_key: "training.jsonl".to_string(),
                served_model_id: None,
                method: FinetuneMethod::Sft,
            })
            .build()
    }

    fn training_storage() -> Storage {
        Storage::new(TRAINING_STORAGE_ID.to_string()).build()
    }

    // ─────────────── MOCK HELPERS ──────────────────────────────

    /// Resource-manager mock that satisfies the read-modify-write IAM step in
    /// `applying_resource_permissions` (the AI resource grants no bindings, but
    /// the closure always gets then sets the project policy).
    fn iam_mock() -> Arc<MockResourceManagerApi> {
        let mut mock = MockResourceManagerApi::new();
        mock.expect_get_project_iam_policy()
            .returning(|_, _| Ok(IamPolicy::default()));
        mock.expect_set_project_iam_policy()
            .returning(|_, policy, _| Ok(policy));
        Arc::new(mock)
    }

    fn service_usage_mock() -> Arc<MockServiceUsageApi> {
        let mut mock = MockServiceUsageApi::new();
        mock.expect_enable_service()
            .returning(|_| Ok(Operation::default()));
        Arc::new(mock)
    }

    /// Wires a service provider with the service-usage and resource-manager
    /// mocks the create flow needs. The controller no longer submits a Vertex
    /// tuning job, so no aiplatform client is wired — a call to it would panic on
    /// the unset expectation, catching any regression that re-introduces
    /// deploy-time tuning.
    fn provider() -> Arc<MockPlatformServiceProvider> {
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_gcp_service_usage_client()
            .returning({
                let m = service_usage_mock();
                move |_| Ok(m.clone())
            });
        provider
            .expect_get_gcp_resource_manager_client()
            .returning({
                let m = iam_mock();
                move |_| Ok(m.clone())
            });
        Arc::new(provider)
    }

    // ─────────────── TESTS ─────────────────────────────────────

    /// A finetune resource reaches Ready immediately (no tuning job submitted at
    /// deploy) and its binding carries the `FinetuneCapability` with the bucket
    /// resolved from the storage dependency, no `tuned_model`, and an empty
    /// `role_arn` (Vertex submits under the ambient identity).
    #[tokio::test]
    async fn finetune_reaches_ready_immediately_with_capability() {
        let mut executor = SingleControllerExecutor::builder()
            .resource(finetune_ai())
            .controller(GcpAiController::default())
            .platform(Platform::Gcp)
            .service_provider(provider())
            .with_dependency(
                training_storage(),
                GcpStorageController::mock_ready(TRAINING_STORAGE_ID),
            )
            .build()
            .await
            .expect("executor should build");

        executor
            .run_until_terminal()
            .await
            .expect("finetune create flow should complete");

        assert_eq!(
            executor.status(),
            ResourceStatus::Running,
            "a finetune resource is Ready immediately; the gateway triggers tuning at runtime"
        );

        let controller = executor
            .internal_state::<GcpAiController>()
            .expect("controller downcast");

        // The binding the gateway consumes carries the finetune capability but no
        // tuned model (the gateway rediscovers the tuned model by convention).
        let params = controller
            .get_binding_params()
            .expect("binding params ok")
            .expect("binding present once project/location are set");
        let binding: AiBinding = serde_json::from_value(params).expect("binding deserializes");
        assert!(
            binding.tuned_model().is_none(),
            "the controller must not attach a tuned model; the gateway rediscovers it"
        );
        let cap = binding
            .finetune()
            .expect("finetune binding must carry the fine-tuning capability");
        assert_eq!(cap.base_model, "gemini-2.0-flash-001");
        // Bucket resolved from the storage dependency (test-stack-<id>, from
        // GcpStorageController::mock_ready), not re-derived here.
        assert_eq!(cap.training_bucket, "test-stack-training-set");
        assert_eq!(cap.training_key, "training.jsonl");
        assert_eq!(cap.served_model_id, "llm-tuned");
        // Deterministic {prefix}-{id}; the executor test harness uses the
        // resource prefix "test" (distinct from the storage mock's "test-stack"
        // bucket-name prefix).
        assert_eq!(cap.job_name, "test-llm");
        assert!(
            cap.role_arn.is_empty(),
            "Vertex submits under the ambient identity; no role is passed"
        );
    }

    /// Regression: an Ai without a finetune spec reaches Ready with a plain
    /// binding carrying neither a tuned model nor a finetune capability.
    #[tokio::test]
    async fn no_finetune_reaches_ready_with_plain_binding() {
        let mut executor = SingleControllerExecutor::builder()
            .resource(base_ai())
            .controller(GcpAiController::default())
            .platform(Platform::Gcp)
            .service_provider(provider())
            .build()
            .await
            .expect("executor should build");

        executor
            .run_until_terminal()
            .await
            .expect("inference create flow should complete");

        assert_eq!(executor.status(), ResourceStatus::Running);

        let controller = executor
            .internal_state::<GcpAiController>()
            .expect("controller downcast");
        assert!(
            controller.finetune.is_none(),
            "a pure-inference gateway advertises no fine-tuning capability"
        );

        let params = controller
            .get_binding_params()
            .expect("binding params ok")
            .expect("binding present");
        let binding: AiBinding = serde_json::from_value(params).expect("binding deserializes");
        assert!(
            binding.tuned_model().is_none(),
            "an inference-only gateway must not carry a tuned model"
        );
        assert!(
            binding.finetune().is_none(),
            "an inference-only gateway must not carry a finetune capability"
        );
    }
}
