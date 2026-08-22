use std::sync::Arc;
use std::time::Duration;

use tracing::{info, warn};

use crate::core::{ResourceControllerContext, ResourcePermissionsHelper};
use crate::error::{ErrorData, Result};
use alien_aws_clients::bedrock::{BedrockApi, FoundationModelAvailability};
use alien_core::{
    ai_catalog::{self, ClientApi},
    bindings::AiBinding,
    Ai, AiAccessTest, AiAvailabilityBlocker, AiAvailabilityObservation, AiAvailabilitySource,
    AiHeartbeatData, AiHeartbeatStatus, AiModelAvailability, AiModelAvailabilityObservation,
    AiOutputs, AwsBedrockAiHeartbeatData, HeartbeatBackend, ObservedHealth, Platform,
    ProviderLifecycleState, ResourceHeartbeat, ResourceHeartbeatData, ResourceOutputs,
    ResourceStatus,
};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_macros::controller;
use chrono::Utc;

#[controller]
pub struct AwsAiController {
    /// AWS region where Bedrock is accessed. None until create_start runs.
    pub(crate) region: Option<String>,
    #[serde(default)]
    pub(crate) availability: Option<AiAvailabilityObservation>,
    #[serde(default)]
    pub(crate) availability_observed_at: Option<chrono::DateTime<Utc>>,
}

const AVAILABILITY_REFRESH_INTERVAL: chrono::Duration = chrono::Duration::minutes(15);

fn classify_bedrock_availability(
    model: &ai_catalog::CatalogModel,
    response: &FoundationModelAvailability,
    tested_at: chrono::DateTime<Utc>,
) -> AiModelAvailabilityObservation {
    let agreement = response
        .agreement_availability
        .as_ref()
        .and_then(|value| value.status.as_deref());
    let entitlement = response.entitlement_availability.as_deref();
    let authorization = response.authorization_status.as_deref();
    let region = response.region_availability.as_deref();
    let complete =
        agreement.is_some() && entitlement.is_some() && authorization.is_some() && region.is_some();
    let mut blockers = Vec::new();
    if agreement != Some("AVAILABLE") {
        blockers.push(AiAvailabilityBlocker::AgreementRequired);
    }
    if entitlement != Some("AVAILABLE") {
        blockers.push(AiAvailabilityBlocker::EntitlementRequired);
    }
    if authorization != Some("AUTHORIZED") {
        blockers.push(AiAvailabilityBlocker::AccessDenied);
    }
    if region != Some("AVAILABLE") {
        blockers.push(AiAvailabilityBlocker::RegionUnavailable);
    }
    AiModelAvailabilityObservation {
        public_model_id: model.public_id.to_string(),
        client_apis: ClientApi::ALL.to_vec(),
        availability: if !complete {
            AiModelAvailability::Unknown
        } else if blockers.is_empty() {
            AiModelAvailability::Available
        } else {
            AiModelAvailability::Blocked
        },
        blockers: if complete {
            blockers
        } else {
            vec![AiAvailabilityBlocker::ObservationFailed]
        },
        // This heartbeat only reads the provider control plane. It deliberately
        // does not invoke a model, consume quota, or claim a successful request.
        access_test: AiAccessTest::NotChecked,
        tested_at: Some(tested_at),
        error_code: None,
    }
}

async fn observe_bedrock_availability(
    client: Arc<dyn BedrockApi>,
    region: &str,
) -> AiAvailabilityObservation {
    let tested_at = Utc::now();
    // The GPT-5 family is served by bedrock-mantle, which has no foundation-model
    // record: `GetFoundationModelAvailability` rejects those ids outright
    // (verified live 2026-08-09 — `openai.gpt-5.6-sol` returns
    // ValidationException while `openai.gpt-oss-20b-1:0` returns AVAILABLE).
    // Probing them anyway reported every one as a failed observation on every
    // account, so they are reported unknown-but-not-broken instead.
    let (mantle_only, candidates): (Vec<_>, Vec<_>) = ai_catalog::models_for(Platform::Aws)
        .into_iter()
        .partition(|model| model.provider_api == ai_catalog::ProviderApi::OpenAiResponses);
    // The qualified catalog contains many models. Keep the observation fast but
    // avoid turning one heartbeat into an unbounded provider request burst.
    let mut responses = Vec::with_capacity(candidates.len());
    for chunk in candidates.chunks(8) {
        let requests = chunk.iter().copied().map(|model| {
            let client = Arc::clone(&client);
            async move {
                (
                    model,
                    client
                        .get_foundation_model_availability(model.upstream_id)
                        .await,
                )
            }
        });
        responses.extend(futures::future::join_all(requests).await);
    }
    let mut models: Vec<AiModelAvailabilityObservation> = mantle_only
        .into_iter()
        .map(|model| AiModelAvailabilityObservation {
            public_model_id: model.public_id.to_string(),
            client_apis: ClientApi::ALL.to_vec(),
            availability: AiModelAvailability::Unknown,
            // No blocker: nothing is wrong with the account, the control plane
            // simply cannot answer for a mantle-served model. The gateway's own
            // call is the first authoritative signal.
            blockers: Vec::new(),
            access_test: AiAccessTest::NotChecked,
            tested_at: Some(tested_at),
            error_code: None,
        })
        .collect();
    models.extend(responses.into_iter().map(|(model, result)| match result {
        Ok(response) => classify_bedrock_availability(model, &response, tested_at),
        Err(error) => {
            warn!(model = model.public_id, %error, "Bedrock availability observation failed");
            AiModelAvailabilityObservation {
                public_model_id: model.public_id.to_string(),
                client_apis: ClientApi::ALL.to_vec(),
                availability: AiModelAvailability::Unknown,
                blockers: vec![AiAvailabilityBlocker::ObservationFailed],
                access_test: AiAccessTest::NotChecked,
                tested_at: Some(tested_at),
                error_code: Some("bedrock-observation-failed".to_string()),
            }
        }
    }));
    AiAvailabilityObservation {
        source: AiAvailabilitySource::AwsBedrock,
        catalog_revision: ai_catalog::AI_CATALOG_REVISION.to_string(),
        location: Some(region.to_string()),
        models,
    }
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
        let region = self.region.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                resource_id: Some(config.id.clone()),
                message: "Region not set in state".to_string(),
            })
        })?;
        info!(id=%config.id, "AWS AI heartbeat tick");
        let refresh = self.availability_observed_at.is_none_or(|observed_at| {
            Utc::now().signed_duration_since(observed_at) >= AVAILABILITY_REFRESH_INTERVAL
        });
        if refresh {
            let client = ctx
                .service_provider
                .get_aws_bedrock_client(ctx.get_aws_config()?)
                .await?;
            self.availability = Some(observe_bedrock_availability(client, &region).await);
            self.availability_observed_at = Some(Utc::now());
        }
        let availability = self.availability.clone().unwrap_or_else(|| {
            AiAvailabilityObservation::unobserved(
                AiAvailabilitySource::AwsBedrock,
                Platform::Aws,
                Some(region.clone()),
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
            controller_platform: Platform::Aws,
            backend: HeartbeatBackend::Aws,
            observed_at: Utc::now(),
            data: ResourceHeartbeatData::Ai(AiHeartbeatData::AwsBedrock(
                AwsBedrockAiHeartbeatData {
                    status,
                    region: region.clone(),
                    availability,
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

#[cfg(test)]
mod tests {
    use super::*;
    use alien_aws_clients::bedrock::BedrockAvailabilityStatus;

    fn response(
        agreement: Option<&str>,
        entitlement: Option<&str>,
        authorization: Option<&str>,
        region: Option<&str>,
    ) -> FoundationModelAvailability {
        FoundationModelAvailability {
            agreement_availability: agreement.map(|status| BedrockAvailabilityStatus {
                status: Some(status.to_string()),
            }),
            entitlement_availability: entitlement.map(str::to_string),
            authorization_status: authorization.map(str::to_string),
            region_availability: region.map(str::to_string),
        }
    }

    #[test]
    fn classifies_complete_bedrock_control_plane_response() {
        let model = ai_catalog::models_for(Platform::Aws)[0];
        let observed = classify_bedrock_availability(
            model,
            &response(
                Some("AVAILABLE"),
                Some("AVAILABLE"),
                Some("AUTHORIZED"),
                Some("AVAILABLE"),
            ),
            Utc::now(),
        );

        assert_eq!(observed.availability, AiModelAvailability::Available);
        assert!(observed.blockers.is_empty());
        assert_eq!(observed.access_test, AiAccessTest::NotChecked);
    }

    #[test]
    fn incomplete_response_is_unknown_instead_of_overstating_access() {
        let model = ai_catalog::models_for(Platform::Aws)[0];
        let observed = classify_bedrock_availability(
            model,
            &response(
                Some("AVAILABLE"),
                None,
                Some("AUTHORIZED"),
                Some("AVAILABLE"),
            ),
            Utc::now(),
        );

        assert_eq!(observed.availability, AiModelAvailability::Unknown);
        assert_eq!(
            observed.blockers,
            vec![AiAvailabilityBlocker::ObservationFailed]
        );
    }

    #[test]
    fn reports_each_bedrock_activation_blocker() {
        let model = ai_catalog::models_for(Platform::Aws)[0];
        let observed = classify_bedrock_availability(
            model,
            &response(
                Some("NOT_AVAILABLE"),
                Some("NOT_AVAILABLE"),
                Some("NOT_AUTHORIZED"),
                Some("NOT_AVAILABLE"),
            ),
            Utc::now(),
        );

        assert_eq!(observed.availability, AiModelAvailability::Blocked);
        assert_eq!(
            observed.blockers,
            vec![
                AiAvailabilityBlocker::AgreementRequired,
                AiAvailabilityBlocker::EntitlementRequired,
                AiAvailabilityBlocker::AccessDenied,
                AiAvailabilityBlocker::RegionUnavailable,
            ]
        );
    }
}
