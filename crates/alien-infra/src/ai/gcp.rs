use std::time::Duration;

use tracing::info;

use crate::core::{ResourceControllerContext, ResourcePermissionsHelper};
use crate::error::{ErrorData, Result};
use crate::storage::GcpStorageController;
use alien_core::{
    bindings::AiBinding, Ai, AiHeartbeatData, AiHeartbeatStatus, AiOutputs, FinetuneMethod,
    GcpVertexAiHeartbeatData, HeartbeatBackend, Platform, ResourceHeartbeat, ResourceHeartbeatData,
    ResourceOutputs, ResourceRef, ResourceStatus, Storage,
};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_gcp_clients::aiplatform::{
    CreateTuningJobRequest, JobState, SupervisedTuningSpec,
};
use alien_gcp_clients::iam::IamPolicy;
use alien_gcp_clients::resource_manager::GetPolicyOptions;
use alien_macros::controller;
use chrono::Utc;

/// How often to re-poll a still-running Vertex tuning job.
const TUNING_POLL_INTERVAL: Duration = Duration::from_secs(30);

#[controller]
pub struct GcpAiController {
    /// GCP project ID. None until create_start runs.
    pub(crate) project: Option<String>,
    /// GCP region (location) for the Vertex AI endpoint. None until create_start runs.
    pub(crate) location: Option<String>,
    /// Full resource name of the submitted Vertex tuning job
    /// (`projects/{p}/locations/{l}/tuningJobs/{id}`). Set once
    /// `SubmittingTuningJob` runs; used by `WaitingForTuningJob` to poll.
    /// `None` for a pure-inference gateway (no `finetune` spec).
    pub(crate) tuning_job_name: Option<String>,
    /// The tuned model's upstream artifact id (the serving endpoint / model
    /// resource name) the Vertex OpenAI-compat chat path routes to. Set only
    /// once the tuning job reaches `JOB_STATE_SUCCEEDED`. Attached to the
    /// binding via `.with_tuned_model(..)` when present.
    pub(crate) tuned_model_upstream_id: Option<String>,
    /// The public model id the gateway serves the tuned model under
    /// (`spec.served_model_id_or_default(&config.id)`). Captured alongside the
    /// upstream id at success so `get_binding_params` (which has no ctx) can
    /// build the tuned binding.
    pub(crate) tuned_model_served_id: Option<String>,
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

        // A pure-inference gateway is ready as soon as permissions are applied.
        // A finetune spec first submits and awaits a Vertex tuning job.
        if config.finetune.is_none() {
            return Ok(HandlerAction::Continue {
                state: Ready,
                suggested_delay: None,
            });
        }

        info!(id = %config.id, "Finetune requested; submitting Vertex tuning job");
        Ok(HandlerAction::Continue {
            state: SubmittingTuningJob,
            suggested_delay: None,
        })
    }

    // ─────────────── FINETUNE FLOW ─────────────────────────────
    // Only reached when the Ai declares a `finetune` spec. Submits a Vertex
    // supervised tuning job reading the customer's training data from GCS, then
    // polls it to completion before serving the tuned model through the gateway.

    #[handler(
        state = SubmittingTuningJob,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn submitting_tuning_job(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let gcp_config = ctx.get_gcp_config()?;
        let config = ctx.desired_resource_config::<Ai>()?;
        let spec = config.finetune.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                resource_id: Some(config.id.clone()),
                message: "SubmittingTuningJob reached without a finetune spec".to_string(),
            })
        })?;

        // Vertex exposes only supervised tuning for Gemini; there is no
        // user-selectable LoRA/QLoRA or DPO method. Map Sft -> supervised tuning
        // and reject the others loudly rather than silently mis-tuning.
        match spec.method {
            FinetuneMethod::Sft => {}
            other => {
                return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                    resource_id: Some(config.id.clone()),
                    message: format!(
                        "Vertex AI supports only supervised fine-tuning (sft); method {other:?} is not available on Vertex"
                    ),
                }));
            }
        }

        // Resolve the training data's real GCS bucket from the dependency's
        // controller state, rather than re-deriving the prefixed name here. The
        // Ai resource declares the training Storage as a dependency
        // (`Ai::get_dependencies`), and the GCS controller records the exact,
        // prefixed bucket name it created in `bucket_name` — reading it keeps
        // this in lockstep with the storage controller's naming.
        let training_ref = ResourceRef::new(Storage::RESOURCE_TYPE, spec.training_data.clone());
        let storage_state = ctx.require_dependency::<GcpStorageController>(&training_ref)?;
        let bucket_name = storage_state.bucket_name.ok_or_else(|| {
            AlienError::new(ErrorData::DependencyNotReady {
                resource_id: config.id.clone(),
                dependency_id: spec.training_data.clone(),
            })
        })?;

        let training_uri = format!("gs://{bucket_name}/{}", spec.training_key);

        let request = CreateTuningJobRequest::builder()
            .base_model(spec.base_model.clone())
            .supervised_tuning_spec(
                SupervisedTuningSpec::builder()
                    .training_dataset_uri(training_uri.clone())
                    .build(),
            )
            .tuned_model_display_name(spec.served_model_id_or_default(&config.id))
            .build();

        info!(
            id = %config.id,
            base_model = %spec.base_model,
            training_uri = %training_uri,
            "Submitting Vertex supervised tuning job"
        );

        let client = ctx.service_provider.get_gcp_aiplatform_client(gcp_config)?;
        let job = client
            .create_tuning_job(request)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to submit Vertex tuning job".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        let job_name = job.name.ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: "Vertex tuning job creation returned no resource name".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        info!(id = %config.id, job = %job_name, "Vertex tuning job submitted");
        self.tuning_job_name = Some(job_name);

        // Poll once immediately; only *in-progress* re-polls wait a full interval.
        Ok(HandlerAction::Continue {
            state: WaitingForTuningJob,
            suggested_delay: None,
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
        let gcp_config = ctx.get_gcp_config()?;
        let config = ctx.desired_resource_config::<Ai>()?;
        let spec = config.finetune.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                resource_id: Some(config.id.clone()),
                message: "WaitingForTuningJob reached without a finetune spec".to_string(),
            })
        })?;
        let spec_served_id = spec.served_model_id_or_default(&config.id);
        let job_name = self.tuning_job_name.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                resource_id: Some(config.id.clone()),
                message: "WaitingForTuningJob reached without a submitted job name".to_string(),
            })
        })?;

        let client = ctx.service_provider.get_gcp_aiplatform_client(gcp_config)?;
        let job = client
            .get_tuning_job(job_name.clone())
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to poll Vertex tuning job '{job_name}'"),
                resource_id: Some(config.id.clone()),
            })?;

        let state = job.state.unwrap_or(JobState::JobStateUnspecified);

        if state.is_in_progress() {
            info!(id = %config.id, job = %job_name, ?state, "Vertex tuning job still running; re-polling");
            return Ok(HandlerAction::Continue {
                state: WaitingForTuningJob,
                suggested_delay: Some(TUNING_POLL_INTERVAL),
            });
        }

        // Terminal failure states fail loud — a fine-tuning resource whose job
        // failed must not silently degrade to serving the untuned base model.
        if state.is_terminal_failure() {
            let detail = job
                .error
                .map(|e| e.message)
                .unwrap_or_else(|| "no error detail returned".to_string());
            return Err(AlienError::new(ErrorData::CloudPlatformError {
                message: format!(
                    "Vertex tuning job '{job_name}' reached terminal state {state:?}: {detail}"
                ),
                resource_id: Some(config.id.clone()),
            }));
        }

        // Success: record the upstream artifact the gateway routes to.
        if state == JobState::JobStateSucceeded {
            let upstream_id = job
                .tuned_model
                .as_ref()
                .and_then(|m| m.upstream_id())
                .ok_or_else(|| {
                    AlienError::new(ErrorData::CloudPlatformError {
                        message: format!(
                            "Vertex tuning job '{job_name}' succeeded but returned no tuned model endpoint/model"
                        ),
                        resource_id: Some(config.id.clone()),
                    })
                })?
                .to_string();

            info!(
                id = %config.id,
                job = %job_name,
                upstream_id = %upstream_id,
                "Vertex tuning job succeeded; tuned model ready"
            );
            self.tuned_model_upstream_id = Some(upstream_id);
            self.tuned_model_served_id = Some(spec_served_id);

            return Ok(HandlerAction::Continue {
                state: Ready,
                suggested_delay: None,
            });
        }

        // Any other (unspecified/unknown) terminal state is unexpected — fail
        // rather than loop forever or serve an untuned model.
        Err(AlienError::new(ErrorData::CloudPlatformError {
            message: format!("Vertex tuning job '{job_name}' in unexpected state {state:?}"),
            resource_id: Some(config.id.clone()),
        }))
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

        // Attach the tuned model only once the tuning job has completed and we
        // recorded both ids; otherwise the base (untuned) binding is returned
        // unchanged, matching a pure-inference gateway.
        if let (Some(served_id), Some(upstream_id)) = (
            self.tuned_model_served_id.as_ref(),
            self.tuned_model_upstream_id.as_ref(),
        ) {
            binding = binding.with_tuned_model(served_id, upstream_id);
        }

        Ok(Some(serde_json::to_value(binding).into_alien_error().context(
            ErrorData::ResourceStateSerializationFailed {
                resource_id: "binding".to_string(),
                message: "Failed to serialize AI binding parameters".to_string(),
            },
        )?))
    }
}

#[cfg(test)]
mod tests {
    //! GCP Vertex AI controller tests.
    //!
    //! These drive the finetune state machine end-to-end against a mocked
    //! `AiPlatformApi`: submit -> pending -> succeeded -> Ready, asserting the
    //! resulting binding carries the tuned model. They also cover the fail-fast
    //! path (job FAILED -> ProvisionFailed) and the pure-inference regression
    //! (no finetune -> Ready with an untuned binding).

    use std::sync::{Arc, Mutex};

    use super::GcpAiController;
    use alien_core::bindings::AiBinding;
    use alien_core::{Ai, FinetuneMethod, FinetuneSpec, Platform, ResourceStatus, Storage};
    use alien_gcp_clients::aiplatform::{
        JobState, MockAiPlatformApi, TunedModelRef, TuningJob,
    };
    use alien_gcp_clients::iam::IamPolicy;
    use alien_gcp_clients::longrunning::Operation;
    use alien_gcp_clients::resource_manager::MockResourceManagerApi;
    use alien_gcp_clients::service_usage::MockServiceUsageApi;

    use crate::core::controller_test::SingleControllerExecutor;
    use crate::core::{MockPlatformServiceProvider, PlatformServiceProvider, ResourceController};
    use crate::storage::GcpStorageController;

    const TRAINING_STORAGE_ID: &str = "training-set";
    const TUNED_ENDPOINT: &str = "projects/test-project-123/locations/us-central1/endpoints/9988";

    // ─────────────── FIXTURES ──────────────────────────────────

    /// A pure-inference gateway (no finetune).
    fn base_ai() -> Ai {
        Ai::new("llm".to_string()).build()
    }

    /// An Ai that fine-tunes a Gemini base model from the training storage.
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

    /// An Ai requesting a method Vertex does not expose (LoRA).
    fn lora_ai() -> Ai {
        Ai::new("llm".to_string())
            .finetune(FinetuneSpec {
                base_model: "gemini-2.0-flash-001".to_string(),
                training_data: TRAINING_STORAGE_ID.to_string(),
                training_key: "training.jsonl".to_string(),
                served_model_id: None,
                method: FinetuneMethod::Lora,
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

    /// Wires a service provider with the given aiplatform mock plus the
    /// service-usage and resource-manager mocks the create flow needs.
    fn provider_with(aiplatform: MockAiPlatformApi) -> Arc<MockPlatformServiceProvider> {
        let aiplatform = Arc::new(aiplatform);
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
        provider
            .expect_get_gcp_aiplatform_client()
            .returning(move |_| Ok(aiplatform.clone()));
        Arc::new(provider)
    }

    fn succeeded_job() -> TuningJob {
        TuningJob::builder()
            .name("projects/test-project-123/locations/us-central1/tuningJobs/42".to_string())
            .state(JobState::JobStateSucceeded)
            .tuned_model(
                TunedModelRef::builder()
                    .endpoint(TUNED_ENDPOINT.to_string())
                    .build(),
            )
            .build()
    }

    // ─────────────── TESTS ─────────────────────────────────────

    /// Submit -> pending -> running -> succeeded -> Ready, and the binding
    /// carries the tuned model with the right served/upstream ids.
    #[tokio::test]
    async fn finetune_reaches_ready_with_tuned_binding() {
        let mut aiplatform = MockAiPlatformApi::new();

        // create returns a job with a name to poll.
        aiplatform
            .expect_create_tuning_job()
            .withf(|req| {
                req.base_model == "gemini-2.0-flash-001"
                    // The gs:// URI is built from the dependency bucket
                    // (test-stack-<id>, from GcpStorageController::mock_ready) + training_key.
                    && req.supervised_tuning_spec.training_dataset_uri
                        == "gs://test-stack-training-set/training.jsonl"
                    && req.tuned_model_display_name.as_deref() == Some("llm-tuned")
            })
            .times(1)
            .returning(|_| {
                Ok(TuningJob::builder()
                    .name(
                        "projects/test-project-123/locations/us-central1/tuningJobs/42"
                            .to_string(),
                    )
                    .state(JobState::JobStatePending)
                    .build())
            });

        // Poll: still-running once (proving the re-poll loop), then succeeded.
        let poll_count = Arc::new(Mutex::new(0u32));
        aiplatform.expect_get_tuning_job().returning(move |name| {
            assert_eq!(
                name,
                "projects/test-project-123/locations/us-central1/tuningJobs/42"
            );
            let mut n = poll_count.lock().unwrap();
            *n += 1;
            match *n {
                1 => Ok(TuningJob::builder().state(JobState::JobStateRunning).build()),
                _ => Ok(succeeded_job()),
            }
        });

        let provider = provider_with(aiplatform);

        let mut executor = SingleControllerExecutor::builder()
            .resource(finetune_ai())
            .controller(GcpAiController::default())
            .platform(Platform::Gcp)
            .service_provider(provider)
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
            "a completed tuning job leaves the gateway Running"
        );

        // The controller recorded the tuned artifact.
        let controller = executor
            .internal_state::<GcpAiController>()
            .expect("controller downcast");
        assert_eq!(
            controller.tuned_model_upstream_id.as_deref(),
            Some(TUNED_ENDPOINT)
        );
        assert_eq!(controller.tuned_model_served_id.as_deref(), Some("llm-tuned"));

        // The binding the gateway consumes carries the tuned model.
        let params = controller
            .get_binding_params()
            .expect("binding params ok")
            .expect("binding present once project/location are set");
        let binding: AiBinding =
            serde_json::from_value(params).expect("binding deserializes");
        let tuned = binding
            .tuned_model()
            .expect("binding must carry the tuned model");
        assert_eq!(tuned.served_id, "llm-tuned");
        assert_eq!(tuned.upstream_id, TUNED_ENDPOINT);
    }

    /// A terminal FAILED job fails the resource loud (no silent fall-back to base).
    #[tokio::test]
    async fn finetune_failed_job_reaches_provision_failed() {
        let mut aiplatform = MockAiPlatformApi::new();
        aiplatform.expect_create_tuning_job().returning(|_| {
            Ok(TuningJob::builder()
                .name("projects/test-project-123/locations/us-central1/tuningJobs/7".to_string())
                .state(JobState::JobStatePending)
                .build())
        });
        aiplatform.expect_get_tuning_job().returning(|_| {
            Ok(TuningJob::builder()
                .state(JobState::JobStateFailed)
                .error(
                    alien_gcp_clients::longrunning::Status::builder()
                        .code(3)
                        .message("training data malformed".to_string())
                        .build(),
                )
                .build())
        });

        let provider = provider_with(aiplatform);

        let mut executor = SingleControllerExecutor::builder()
            .resource(finetune_ai())
            .controller(GcpAiController::default())
            .platform(Platform::Gcp)
            .service_provider(provider)
            .with_dependency(
                training_storage(),
                GcpStorageController::mock_ready(TRAINING_STORAGE_ID),
            )
            .build()
            .await
            .expect("executor should build");

        // Fail-fast: the handler surfaces the terminal-failure as an error rather
        // than silently reaching Ready on the untuned base model. In production the
        // executor catches this and applies `on_failure = CreateFailed`
        // (ProvisionFailed); the test harness surfaces the raw error, which we
        // assert on directly.
        let err = executor
            .run_until_terminal()
            .await
            .expect_err("a failed tuning job must error out, not reach Ready");
        assert_eq!(err.code, "CLOUD_PLATFORM_ERROR");
        assert!(
            err.to_string().contains("JobStateFailed")
                && err.to_string().contains("training data malformed"),
            "error must name the terminal state and carry the provider detail: {err}"
        );

        // The resource never reached Running and recorded no tuned model, so the
        // gateway would not serve an untuned model as if it were tuned.
        assert_ne!(executor.status(), ResourceStatus::Running);
        let controller = executor
            .internal_state::<GcpAiController>()
            .expect("controller downcast");
        assert!(controller.tuned_model_upstream_id.is_none());
    }

    /// Regression: an Ai without a finetune spec reaches Ready with an untuned
    /// binding and never touches the aiplatform client.
    #[tokio::test]
    async fn no_finetune_reaches_ready_with_untuned_binding() {
        // A create-tuning-job expectation would fail if the controller called it.
        let aiplatform = MockAiPlatformApi::new();
        let provider = provider_with(aiplatform);

        let mut executor = SingleControllerExecutor::builder()
            .resource(base_ai())
            .controller(GcpAiController::default())
            .platform(Platform::Gcp)
            .service_provider(provider)
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
            controller.tuning_job_name.is_none(),
            "a pure-inference gateway never submits a tuning job"
        );

        let params = controller
            .get_binding_params()
            .expect("binding params ok")
            .expect("binding present");
        let binding: AiBinding =
            serde_json::from_value(params).expect("binding deserializes");
        assert!(
            binding.tuned_model().is_none(),
            "an untuned gateway must not carry a tuned model"
        );
    }

    /// Vertex does not expose LoRA for Gemini supervised tuning, so submit fails
    /// loud rather than silently mapping it to something else.
    #[tokio::test]
    async fn unsupported_method_reaches_provision_failed() {
        // No create call should happen — the method check rejects before submit.
        let aiplatform = MockAiPlatformApi::new();
        let provider = provider_with(aiplatform);

        let mut executor = SingleControllerExecutor::builder()
            .resource(lora_ai())
            .controller(GcpAiController::default())
            .platform(Platform::Gcp)
            .service_provider(provider)
            .with_dependency(
                training_storage(),
                GcpStorageController::mock_ready(TRAINING_STORAGE_ID),
            )
            .build()
            .await
            .expect("executor should build");

        // The method check rejects before any client call; the handler errors out
        // (mapped to `on_failure = CreateFailed` by the executor in production).
        let err = executor
            .run_until_terminal()
            .await
            .expect_err("an unsupported tuning method must fail rather than mis-tune");
        assert_eq!(err.code, "RESOURCE_CONFIG_INVALID");
        assert!(
            err.to_string().to_lowercase().contains("supervised"),
            "error should explain Vertex only supports supervised tuning: {err}"
        );
        assert_ne!(executor.status(), ResourceStatus::Running);
    }
}
