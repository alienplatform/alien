use std::sync::Arc;
use std::time::Duration;

use tracing::{info, warn};

use crate::core::{ResourceControllerContext, ResourcePermissionsHelper};
use crate::error::{ErrorData, Result};
use alien_core::{
    ai_catalog, bindings::AiBinding, Ai, AiAccessTest, AiAvailabilityBlocker,
    AiAvailabilityObservation, AiAvailabilitySource, AiHeartbeatData, AiHeartbeatStatus,
    AiModelAvailability, AiModelAvailabilityObservation, AiOutputs, GcpVertexAiHeartbeatData,
    HeartbeatBackend, ObservedHealth, Platform, ProviderLifecycleState, ResourceHeartbeat,
    ResourceHeartbeatData, ResourceOutputs, ResourceStatus,
};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_gcp_clients::iam::IamPolicy;
use alien_gcp_clients::model_garden::{ModelGardenApi, PublisherModel};
use alien_gcp_clients::resource_manager::GetPolicyOptions;
use alien_macros::controller;
use chrono::Utc;

#[controller]
pub struct GcpAiController {
    /// GCP project ID. None until create_start runs.
    pub(crate) project: Option<String>,
    /// GCP region (location) for the Vertex AI endpoint. None until create_start runs.
    pub(crate) location: Option<String>,
    #[serde(default)]
    pub(crate) availability: Option<AiAvailabilityObservation>,
    #[serde(default)]
    pub(crate) availability_observed_at: Option<chrono::DateTime<Utc>>,
}

const AVAILABILITY_REFRESH_INTERVAL: chrono::Duration = chrono::Duration::minutes(15);

/// Vertex location used for inference. See `create_start` for why this is not
/// the deployment's region.
const VERTEX_LOCATION: &str = "global";

fn publisher_model_present(models: &[PublisherModel], publisher: &str, upstream_id: &str) -> bool {
    let model_id = upstream_id
        .strip_prefix("google/")
        .unwrap_or(upstream_id)
        .split('@')
        .next()
        .unwrap_or(upstream_id);
    let expected = format!("publishers/{publisher}/models/{model_id}");
    models
        .iter()
        .any(|model| model.name.split('@').next() == Some(expected.as_str()))
}

async fn observe_vertex_availability(
    client: Arc<dyn ModelGardenApi>,
    location: &str,
) -> AiAvailabilityObservation {
    let (google, anthropic) = tokio::join!(
        client.list_publisher_models("google"),
        client.list_publisher_models("anthropic")
    );
    if let Err(error) = &google {
        warn!(%error, "Google Model Garden catalog observation failed");
    }
    if let Err(error) = &anthropic {
        warn!(%error, "Anthropic Model Garden catalog observation failed");
    }
    let tested_at = Utc::now();
    let mut models = Vec::new();
    for model in ai_catalog::models_for(Platform::Gcp) {
        let is_anthropic = model.public_id.starts_with("claude");
        let (publisher, listed) = if is_anthropic {
            ("anthropic", anthropic.as_ref())
        } else {
            ("google", google.as_ref())
        };
        let listed = match listed {
            Ok(catalog) => publisher_model_present(catalog, publisher, model.upstream_id),
            Err(_) => {
                models.push(AiModelAvailabilityObservation {
                    public_model_id: model.public_id.to_string(),
                    client_apis: model.client_apis.to_vec(),
                    availability: AiModelAvailability::Unknown,
                    blockers: vec![AiAvailabilityBlocker::ObservationFailed],
                    access_test: AiAccessTest::NotChecked,
                    tested_at: Some(tested_at),
                    error_code: Some("model-garden-list-failed".to_string()),
                });
                continue;
            }
        };
        if !listed {
            models.push(AiModelAvailabilityObservation {
                public_model_id: model.public_id.to_string(),
                client_apis: model.client_apis.to_vec(),
                availability: AiModelAvailability::Blocked,
                blockers: vec![AiAvailabilityBlocker::RegionUnavailable],
                access_test: AiAccessTest::NotChecked,
                tested_at: Some(tested_at),
                error_code: None,
            });
            continue;
        }
        if !is_anthropic {
            models.push(AiModelAvailabilityObservation {
                public_model_id: model.public_id.to_string(),
                client_apis: model.client_apis.to_vec(),
                availability: AiModelAvailability::Available,
                blockers: vec![],
                access_test: AiAccessTest::NotChecked,
                tested_at: Some(tested_at),
                error_code: None,
            });
            continue;
        }
        let base_id = model
            .upstream_id
            .split('@')
            .next()
            .unwrap_or(model.upstream_id);
        let resource = format!("publishers/anthropic/models/{base_id}");
        match client.check_publisher_model_eula(&resource).await {
            Ok(acceptance) => models.push(AiModelAvailabilityObservation {
                public_model_id: model.public_id.to_string(),
                client_apis: model.client_apis.to_vec(),
                availability: if acceptance.publisher_model_eula_acked {
                    AiModelAvailability::Available
                } else {
                    AiModelAvailability::Blocked
                },
                blockers: if acceptance.publisher_model_eula_acked {
                    vec![]
                } else {
                    vec![AiAvailabilityBlocker::AgreementRequired]
                },
                access_test: AiAccessTest::NotChecked,
                tested_at: Some(tested_at),
                error_code: None,
            }),
            Err(error) => {
                warn!(model = model.public_id, %error, "Vertex EULA observation failed");
                models.push(AiModelAvailabilityObservation {
                    public_model_id: model.public_id.to_string(),
                    client_apis: model.client_apis.to_vec(),
                    availability: AiModelAvailability::Unknown,
                    blockers: vec![AiAvailabilityBlocker::ObservationFailed],
                    access_test: AiAccessTest::NotChecked,
                    tested_at: Some(tested_at),
                    error_code: Some("model-garden-eula-check-failed".to_string()),
                });
            }
        }
    }
    AiAvailabilityObservation {
        source: AiAvailabilitySource::GcpVertex,
        catalog_revision: ai_catalog::AI_CATALOG_REVISION.to_string(),
        location: Some(location.to_string()),
        models,
    }
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
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let gcp_config = ctx.get_gcp_config()?;
        let config = ctx.desired_resource_config::<Ai>()?;

        self.project = Some(gcp_config.project_id.clone());
        // Vertex serves models per location, and the deployment's own region is
        // usually the wrong one: verified live 2026-08-09 against
        // alien-test-target, `claude-haiku-4.5` resolves in `us-central1` but
        // 404s in `us-east4`, `us-east5`, and `europe-west1`, while every
        // catalog model — including Gemini 3.x, which Google serves nowhere
        // else — resolves on `global`. Availability is observed against the
        // global Model Garden, so routing anywhere else reports models as
        // available and then fails the request.
        //
        // `global` is also Google's recommended endpoint and carries no pricing
        // premium; regional and multi-region endpoints add 10%. A deployment
        // that needs single-region data residency for inference will need an
        // explicit opt-in here rather than silently losing most of the catalog.
        self.location = Some(VERTEX_LOCATION.to_string());

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
    async fn enabling_api(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
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
        let project = self.project.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                resource_id: Some(config.id.clone()),
                message: "Project not set in state".to_string(),
            })
        })?;
        let location = self.location.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                resource_id: Some(config.id.clone()),
                message: "Location not set in state".to_string(),
            })
        })?;
        info!(id = %config.id, "GCP AI heartbeat tick");
        let refresh = self.availability_observed_at.is_none_or(|observed_at| {
            Utc::now().signed_duration_since(observed_at) >= AVAILABILITY_REFRESH_INTERVAL
        });
        if refresh {
            let client = ctx
                .service_provider
                .get_gcp_model_garden_client(ctx.get_gcp_config()?)?;
            self.availability = Some(observe_vertex_availability(client, &location).await);
            self.availability_observed_at = Some(Utc::now());
        }
        let availability = self.availability.clone().unwrap_or_else(|| {
            AiAvailabilityObservation::unobserved(
                AiAvailabilitySource::GcpVertex,
                Platform::Gcp,
                Some(location.clone()),
            )
        });
        let partial = availability
            .models
            .iter()
            .any(|model| model.availability == AiModelAvailability::Unknown);
        let status = AiHeartbeatStatus {
            health: if partial {
                ObservedHealth::Degraded
            } else {
                ObservedHealth::Healthy
            },
            lifecycle: ProviderLifecycleState::Running,
            message: partial.then(|| "Some model availability could not be observed".to_string()),
            stale: false,
            partial,
            collection_issues: vec![],
        };
        ctx.emit_heartbeat(ResourceHeartbeat {
            deployment_id: None,
            resource_id: config.id.clone(),
            resource_type: Ai::RESOURCE_TYPE,
            controller_platform: Platform::Gcp,
            backend: HeartbeatBackend::Gcp,
            observed_at: Utc::now(),
            data: ResourceHeartbeatData::Ai(AiHeartbeatData::GcpVertex(GcpVertexAiHeartbeatData {
                status,
                project: project.clone(),
                location: location.clone(),
                availability,
            })),
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
    async fn delete_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
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

#[cfg(all(test, feature = "test-utils"))]
mod tests {
    use super::*;
    use alien_gcp_clients::model_garden::{MockModelGardenApi, PublisherModelEulaAcceptance};

    #[tokio::test]
    async fn distinguishes_first_party_models_from_unaccepted_publisher_terms() {
        let mut client = MockModelGardenApi::new();
        client
            .expect_list_publisher_models()
            .returning(|publisher| {
                let models = ai_catalog::models_for(Platform::Gcp)
                    .into_iter()
                    .filter(|model| {
                        model.public_id.starts_with("claude") == (publisher == "anthropic")
                    })
                    .map(|model| {
                        let model_id = model
                            .upstream_id
                            .strip_prefix("google/")
                            .unwrap_or(model.upstream_id)
                            .split('@')
                            .next()
                            .unwrap();
                        PublisherModel {
                            name: format!("publishers/{publisher}/models/{model_id}"),
                        }
                    })
                    .collect();
                Ok(models)
            });
        client
            .expect_check_publisher_model_eula()
            .returning(|publisher_model| {
                Ok(PublisherModelEulaAcceptance {
                    publisher_model: Some(publisher_model.to_string()),
                    publisher_model_eula_acked: false,
                })
            });

        let observation = observe_vertex_availability(Arc::new(client), "us-central1").await;
        let gemini = observation
            .models
            .iter()
            .find(|model| model.public_model_id.starts_with("gemini"))
            .unwrap();
        let claude = observation
            .models
            .iter()
            .find(|model| model.public_model_id.starts_with("claude"))
            .unwrap();

        assert_eq!(gemini.availability, AiModelAvailability::Available);
        assert_eq!(gemini.access_test, AiAccessTest::NotChecked);
        assert_eq!(claude.availability, AiModelAvailability::Blocked);
        assert_eq!(
            claude.blockers,
            vec![AiAvailabilityBlocker::AgreementRequired]
        );
    }
}
