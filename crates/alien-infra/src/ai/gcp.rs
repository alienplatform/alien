use std::time::Duration;

use tracing::info;

use crate::core::{ResourceControllerContext, ResourcePermissionsHelper};
use crate::error::{ErrorData, Result};
use alien_core::{
    bindings::AiBinding, Ai, AiHeartbeatData, AiHeartbeatStatus, AiOutputs,
    GcpVertexAiHeartbeatData, HeartbeatBackend, Platform, ResourceHeartbeat, ResourceHeartbeatData,
    ResourceOutputs, ResourceStatus,
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
        Ok(Some(
            serde_json::to_value(AiBinding::vertex(project, location))
                .into_alien_error()
                .context(ErrorData::ResourceStateSerializationFailed {
                    resource_id: "binding".to_string(),
                    message: "Failed to serialize AI binding parameters".to_string(),
                })?,
        ))
    }
}
