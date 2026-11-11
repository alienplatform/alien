use std::sync::Arc;
use std::time::Duration;

use tracing::{info, warn};

use crate::core::{ResourceControllerContext, ResourcePermissionsHelper};
use crate::error::{ErrorData, Result};
use alien_aws_clients::bedrock::{
    BedrockApi, FoundationModelAvailability, InferenceProfileSummary,
};
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
        quota_warnings: Vec::new(),
        throttled_last_hour: None,
    }
}

/// Claude is invoked through a geo inference profile (`us.anthropic.…`), not through the
/// foundation model id that `GetFoundationModelAvailability` describes. A Claude model
/// whose profile for this region isn't offered or isn't active can't be invoked, whatever
/// the foundation-model record says.
fn apply_inference_profile(
    model: &ai_catalog::CatalogModel,
    region: &str,
    profiles: &[InferenceProfileSummary],
    mut observed: AiModelAvailabilityObservation,
) -> AiModelAvailabilityObservation {
    let profile_id = ai_catalog::bedrock_inference_profile_id(region, model.upstream_id);
    let invocable = profiles.iter().any(|profile| {
        profile.inference_profile_id == profile_id && profile.status.as_deref() == Some("ACTIVE")
    });
    if invocable || observed.availability == AiModelAvailability::Unknown {
        return observed;
    }
    observed.availability = AiModelAvailability::Blocked;
    if !observed
        .blockers
        .contains(&AiAvailabilityBlocker::RegionUnavailable)
    {
        observed
            .blockers
            .push(AiAvailabilityBlocker::RegionUnavailable);
    }
    observed
}

/// Health is `Degraded` only when Alien could not look at a model. A model whose
/// availability the provider's control plane cannot report (the bedrock-mantle GPT-5
/// family) is not a gap in what Alien can see, so it doesn't count.
fn observation_incomplete(availability: &AiAvailabilityObservation) -> bool {
    availability.models.iter().any(|model| {
        model
            .blockers
            .contains(&AiAvailabilityBlocker::ObservationFailed)
    })
}

/// Fill `quota_warnings` and `throttled_last_hour` for the declared models.
async fn add_declared_model_signals(
    observed: &mut AiAvailabilityObservation,
    declared: &[alien_core::AiModel],
    quotas: &dyn alien_aws_clients::service_quotas::ServiceQuotasApi,
    cloudwatch: &dyn alien_aws_clients::cloudwatch::CloudWatchApi,
) {
    let models: Vec<&ai_catalog::CatalogModel> = declared
        .iter()
        .filter_map(|model| ai_catalog::resolve_for(&model.id, Platform::Aws))
        .collect();
    // Two spellings of one model (`claude-sonnet-4-6`, `claude-sonnet-4.6`) resolve to the
    // same catalog entry: read its quota and throttles once, or the count doubles.
    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
    let models: Vec<&ai_catalog::CatalogModel> = models
        .into_iter()
        .filter(|model| seen.insert(model.upstream_id))
        .collect();
    let public_ids: Vec<&str> = models.iter().map(|model| model.public_id).collect();
    match super::aws_quotas::low_quotas_for(quotas, &public_ids).await {
        Ok(mut low) => {
            for model in observed.models.iter_mut() {
                if let Some(warnings) = low.remove(&model.public_model_id) {
                    model.quota_warnings = warnings;
                }
            }
        }
        Err(error) => warn!(%error, "Bedrock quota read failed; no quota warnings reported"),
    }
    let upstream_ids: Vec<&str> = models.iter().map(|model| model.upstream_id).collect();
    match super::aws_traffic::throttles_for(cloudwatch, &upstream_ids).await {
        Ok(totals) => {
            for model in &models {
                if let Some(total) = totals.get(model.upstream_id) {
                    if let Some(entry) = observed
                        .models
                        .iter_mut()
                        .find(|entry| entry.public_model_id == model.public_id)
                    {
                        entry.throttled_last_hour = Some(*total);
                    }
                }
            }
        }
        Err(error) => warn!(%error, "Bedrock throttle read failed; throttles not reported"),
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
    // Claude models are invoked through inference profiles. If the profile list can't be
    // read (for example a management role set up before it was granted), their result
    // stays what the foundation-model record says.
    let profiles = match client.list_inference_profiles().await {
        Ok(profiles) => Some(profiles),
        Err(error) => {
            warn!(%error, "Bedrock inference profile listing failed; Claude availability uses the foundation-model record only");
            None
        }
    };
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
            quota_warnings: Vec::new(),
            throttled_last_hour: None,
        })
        .collect();
    models.extend(responses.into_iter().map(|(model, result)| match result {
        Ok(response) => {
            let observed = classify_bedrock_availability(model, &response, tested_at);
            match &profiles {
                Some(profiles) if model.provider_api == ai_catalog::ProviderApi::Anthropic => {
                    apply_inference_profile(model, region, profiles, observed)
                }
                _ => observed,
            }
        }
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
                quota_warnings: Vec::new(),
                throttled_last_hour: None,
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
            let mut observed = observe_bedrock_availability(client, &region).await;
            // Quota and throttle reads cover only the models the app declares. They are
            // best effort: a failed read leaves the fields empty, never a guess.
            if !config.models.is_empty() {
                let aws_config = ctx.get_aws_config()?;
                let quotas = ctx
                    .service_provider
                    .get_aws_service_quotas_client(aws_config)
                    .await?;
                let cloudwatch = ctx
                    .service_provider
                    .get_aws_cloudwatch_client(aws_config)
                    .await?;
                add_declared_model_signals(
                    &mut observed,
                    &config.models,
                    quotas.as_ref(),
                    cloudwatch.as_ref(),
                )
                .await;
            }
            self.availability = Some(observed);
            self.availability_observed_at = Some(Utc::now());
        }
        let availability = self.availability.clone().unwrap_or_else(|| {
            AiAvailabilityObservation::unobserved(
                AiAvailabilitySource::AwsBedrock,
                Platform::Aws,
                Some(region.clone()),
            )
        });
        let partial = observation_incomplete(&availability);
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
    use crate::core::{
        controller_test::SingleControllerExecutor, MockPlatformServiceProvider, StackExecutor,
        StackResourceStateExt,
    };
    use alien_aws_clients::bedrock::{BedrockAvailabilityStatus, MockBedrockApi};
    use alien_aws_clients::{AwsClientConfig, AwsClientConfigExt};

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

    #[tokio::test]
    async fn declared_models_get_quota_warnings_and_throttle_counts() {
        use alien_aws_clients::cloudwatch::{
            Dimension, GetMetricDataResponse, ListMetricsResponse, Metric, MetricDataResult,
            MockCloudWatchApi,
        };
        use alien_aws_clients::service_quotas::{
            ListServiceQuotasResponse, MockServiceQuotasApi, ServiceQuota,
        };
        let quota = |value: f64| ServiceQuota {
            quota_code: Some("L-00FF3314".into()),
            service_code: Some("bedrock".into()),
            quota_name: Some(
                "Cross-region model inference requests per minute for Anthropic Claude Sonnet 4.6"
                    .into(),
            ),
            value: Some(value),
        };
        let mut quotas = MockServiceQuotasApi::new();
        quotas.expect_list_service_quotas().returning(move |_, _| {
            Ok(ListServiceQuotasResponse {
                quotas: vec![quota(10.0)],
                next_token: None,
            })
        });
        quotas
            .expect_list_aws_default_service_quotas()
            .returning(move |_, _| {
                Ok(ListServiceQuotasResponse {
                    quotas: vec![quota(10_000.0)],
                    next_token: None,
                })
            });
        let mut cloudwatch = MockCloudWatchApi::new();
        cloudwatch.expect_list_metrics().returning(|_| {
            Ok(ListMetricsResponse {
                metrics: vec![Metric::builder()
                    .namespace("AWS/Bedrock".to_string())
                    .metric_name("InvocationThrottles".to_string())
                    .dimensions(vec![Dimension::builder()
                        .name("ModelId".to_string())
                        .value("us.anthropic.claude-sonnet-4-6".to_string())
                        .build()])
                    .build()],
                next_token: None,
                owning_accounts: vec![],
            })
        });
        cloudwatch.expect_get_metric_data().returning(|_| {
            Ok(GetMetricDataResponse {
                metric_data_results: vec![MetricDataResult {
                    id: "q0".into(),
                    label: None,
                    status_code: Some("Complete".into()),
                    timestamps: vec![1],
                    values: vec![7.0],
                    messages: vec![],
                }],
                messages: vec![],
                next_token: None,
            })
        });
        let model = ai_catalog::resolve_for("claude-sonnet-4-6", Platform::Aws).unwrap();
        let other = ai_catalog::models_for(Platform::Aws)
            .into_iter()
            .find(|m| m.public_id != model.public_id)
            .unwrap();
        let entry = |m: &ai_catalog::CatalogModel| {
            classify_bedrock_availability(
                m,
                &response(
                    Some("AVAILABLE"),
                    Some("AVAILABLE"),
                    Some("AUTHORIZED"),
                    Some("AVAILABLE"),
                ),
                Utc::now(),
            )
        };
        let mut observed = AiAvailabilityObservation {
            source: AiAvailabilitySource::AwsBedrock,
            catalog_revision: ai_catalog::AI_CATALOG_REVISION.to_string(),
            location: Some("us-west-2".into()),
            models: vec![entry(model), entry(other)],
        };
        add_declared_model_signals(
            &mut observed,
            &[alien_core::AiModel {
                id: "claude-sonnet-4-6".into(),
                required: true,
            }],
            &quotas,
            &cloudwatch,
        )
        .await;
        let declared = &observed.models[0];
        assert_eq!(declared.quota_warnings.len(), 1);
        assert_eq!(declared.quota_warnings[0].applied, 10.0);
        assert_eq!(declared.throttled_last_hour, Some(7));
        // Models the app does not declare are not read.
        assert!(observed.models[1].quota_warnings.is_empty());
        assert_eq!(observed.models[1].throttled_last_hour, None);
    }

    #[tokio::test]
    async fn two_aliases_of_one_model_are_read_once() {
        use alien_aws_clients::cloudwatch::{
            Dimension, GetMetricDataResponse, ListMetricsResponse, Metric, MetricDataResult,
            MockCloudWatchApi,
        };
        use alien_aws_clients::service_quotas::{
            ListServiceQuotasResponse, MockServiceQuotasApi, ServiceQuota,
        };
        let quota = |value: f64| ServiceQuota {
            quota_code: Some("L-00FF3314".into()),
            service_code: Some("bedrock".into()),
            quota_name: Some(
                "Cross-region model inference requests per minute for Anthropic Claude Sonnet 4.6"
                    .into(),
            ),
            value: Some(value),
        };
        let mut quotas = MockServiceQuotasApi::new();
        quotas.expect_list_service_quotas().returning(move |_, _| {
            Ok(ListServiceQuotasResponse {
                quotas: vec![quota(10.0)],
                next_token: None,
            })
        });
        quotas
            .expect_list_aws_default_service_quotas()
            .returning(move |_, _| {
                Ok(ListServiceQuotasResponse {
                    quotas: vec![quota(10_000.0)],
                    next_token: None,
                })
            });
        let mut cloudwatch = MockCloudWatchApi::new();
        cloudwatch.expect_list_metrics().returning(|_| {
            Ok(ListMetricsResponse {
                metrics: vec![Metric::builder()
                    .namespace("AWS/Bedrock".to_string())
                    .metric_name("InvocationThrottles".to_string())
                    .dimensions(vec![Dimension::builder()
                        .name("ModelId".to_string())
                        .value("us.anthropic.claude-sonnet-4-6".to_string())
                        .build()])
                    .build()],
                next_token: None,
                owning_accounts: vec![],
            })
        });
        cloudwatch.expect_get_metric_data().returning(|request| {
            // One answer per query sent, like CloudWatch: a duplicated query doubles the sum.
            Ok(GetMetricDataResponse {
                metric_data_results: request
                    .metric_data_queries
                    .iter()
                    .map(|query| MetricDataResult {
                        id: query.id.clone(),
                        label: None,
                        status_code: Some("Complete".into()),
                        timestamps: vec![1],
                        values: vec![7.0],
                        messages: vec![],
                    })
                    .collect(),
                messages: vec![],
                next_token: None,
            })
        });
        let model = ai_catalog::resolve_for("claude-sonnet-4-6", Platform::Aws).unwrap();
        let other = ai_catalog::models_for(Platform::Aws)
            .into_iter()
            .find(|m| m.public_id != model.public_id)
            .unwrap();
        let entry = |m: &ai_catalog::CatalogModel| {
            classify_bedrock_availability(
                m,
                &response(
                    Some("AVAILABLE"),
                    Some("AVAILABLE"),
                    Some("AUTHORIZED"),
                    Some("AVAILABLE"),
                ),
                Utc::now(),
            )
        };
        let mut observed = AiAvailabilityObservation {
            source: AiAvailabilitySource::AwsBedrock,
            catalog_revision: ai_catalog::AI_CATALOG_REVISION.to_string(),
            location: Some("us-west-2".into()),
            models: vec![entry(model), entry(other)],
        };
        add_declared_model_signals(
            &mut observed,
            &[
                alien_core::AiModel {
                    id: "claude-sonnet-4-6".into(),
                    required: true,
                },
                alien_core::AiModel {
                    id: "claude-sonnet-4.6".into(),
                    required: false,
                },
            ],
            &quotas,
            &cloudwatch,
        )
        .await;
        let declared = &observed.models[0];
        assert_eq!(declared.quota_warnings.len(), 1);
        assert_eq!(declared.quota_warnings[0].applied, 10.0);
        assert_eq!(declared.throttled_last_hour, Some(7));
        // Models the app does not declare are not read.
        assert!(observed.models[1].quota_warnings.is_empty());
        assert_eq!(observed.models[1].throttled_last_hour, None);
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

    fn available() -> FoundationModelAvailability {
        response(
            Some("AVAILABLE"),
            Some("AVAILABLE"),
            Some("AUTHORIZED"),
            Some("AVAILABLE"),
        )
    }

    /// Every Claude model's profile for `us-west-2`, active.
    fn all_claude_profiles() -> Vec<InferenceProfileSummary> {
        claude_profiles(|_| true)
    }

    fn claude_profiles(
        include: impl Fn(&ai_catalog::CatalogModel) -> bool,
    ) -> Vec<InferenceProfileSummary> {
        ai_catalog::models_for(Platform::Aws)
            .into_iter()
            .filter(|model| model.provider_api == ai_catalog::ProviderApi::Anthropic)
            .filter(|model| include(model))
            .map(|model| InferenceProfileSummary {
                inference_profile_id: ai_catalog::bedrock_inference_profile_id(
                    "us-west-2",
                    model.upstream_id,
                ),
                status: Some("ACTIVE".to_string()),
            })
            .collect()
    }

    /// Steps the real `Ready` handler once against `bedrock` and returns its heartbeat.
    async fn ready_heartbeat(bedrock: MockBedrockApi) -> AwsBedrockAiHeartbeatData {
        let bedrock = Arc::new(bedrock);
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_aws_bedrock_client()
            .returning(move |_| Ok(bedrock.clone()));
        let mut executor = SingleControllerExecutor::builder()
            .resource(Ai::new("llm".to_string()).build())
            .controller(AwsAiController {
                state: AwsAiState::Ready,
                region: Some("us-west-2".to_string()),
                ..Default::default()
            })
            .platform(Platform::Aws)
            .service_provider(Arc::new(provider))
            .build()
            .await
            .unwrap();
        executor.step().await.unwrap();
        let [heartbeat] = executor.last_heartbeats() else {
            panic!("Ready should emit one heartbeat");
        };
        let ResourceHeartbeatData::Ai(AiHeartbeatData::AwsBedrock(data)) = &heartbeat.data else {
            panic!("unexpected heartbeat data");
        };
        data.clone()
    }

    /// GPT-5 models are served by bedrock-mantle and have no foundation-model record, so
    /// their availability is always unknown. That must not make an account whose other
    /// models all answer `Degraded`.
    #[tokio::test]
    async fn unknown_mantle_models_do_not_degrade_health() {
        let mut bedrock = MockBedrockApi::new();
        bedrock
            .expect_get_foundation_model_availability()
            .returning(|_| Ok(available()));
        bedrock
            .expect_list_inference_profiles()
            .returning(|| Ok(all_claude_profiles()));

        let data = ready_heartbeat(bedrock).await;

        let mantle: Vec<_> = data
            .availability
            .models
            .iter()
            .filter(|model| model.availability == AiModelAvailability::Unknown)
            .collect();
        assert!(!mantle.is_empty(), "the catalog has mantle-served models");
        assert!(mantle.iter().all(|model| model.blockers.is_empty()));
        assert_eq!(data.status.health, ObservedHealth::Healthy);
        assert!(!data.status.partial);
        assert_eq!(data.status.message, None);
    }

    /// A model whose availability call failed is a real gap in what Alien can see.
    #[tokio::test]
    async fn a_failed_observation_still_degrades_health() {
        let mut bedrock = MockBedrockApi::new();
        bedrock
            .expect_get_foundation_model_availability()
            .returning(|_| {
                Err(AlienError::new(
                    alien_client_core::ErrorData::RemoteResourceNotFound {
                        resource_type: "foundation model".to_string(),
                        resource_name: "any".to_string(),
                    },
                ))
            });
        bedrock
            .expect_list_inference_profiles()
            .returning(|| Ok(all_claude_profiles()));

        let data = ready_heartbeat(bedrock).await;

        assert_eq!(data.status.health, ObservedHealth::Degraded);
        assert!(data.status.partial);
        assert_eq!(
            data.status.message.as_deref(),
            Some("Some model availability could not be observed")
        );
    }

    fn model<'a>(
        data: &'a AwsBedrockAiHeartbeatData,
        id: &str,
    ) -> &'a AiModelAvailabilityObservation {
        data.availability
            .models
            .iter()
            .find(|model| model.public_model_id == id)
            .unwrap_or_else(|| panic!("{id} is observed"))
    }

    /// Claude is invoked through its inference profile. A foundation-model record that says
    /// AVAILABLE must not make a model look usable when the region offers no profile for it.
    #[tokio::test]
    async fn claude_without_an_invocable_profile_is_blocked() {
        let mut bedrock = MockBedrockApi::new();
        bedrock
            .expect_get_foundation_model_availability()
            .returning(|_| Ok(available()));
        bedrock.expect_list_inference_profiles().returning(|| {
            // No profile for Sonnet 5, and Opus 5's profile isn't active.
            let mut profiles = claude_profiles(|model| {
                !matches!(model.public_id, "claude-sonnet-5" | "claude-opus-5")
            });
            profiles.push(InferenceProfileSummary {
                inference_profile_id: ai_catalog::bedrock_inference_profile_id(
                    "us-west-2",
                    "anthropic.claude-opus-5",
                ),
                status: Some("LEGACY".to_string()),
            });
            Ok(profiles)
        });

        let data = ready_heartbeat(bedrock).await;

        for blocked in ["claude-sonnet-5", "claude-opus-5"] {
            let observed = model(&data, blocked);
            assert_eq!(
                observed.availability,
                AiModelAvailability::Blocked,
                "{blocked}"
            );
            assert_eq!(
                observed.blockers,
                vec![AiAvailabilityBlocker::RegionUnavailable]
            );
        }
        let haiku = model(&data, "claude-haiku-4.5");
        assert_eq!(haiku.availability, AiModelAvailability::Available);
        assert!(haiku.blockers.is_empty());
        // A blocked model is reported, not a gap in what Alien can see.
        assert_eq!(data.status.health, ObservedHealth::Healthy);
    }

    /// An account that has not accepted the Claude agreement keeps its agreement blocker
    /// next to the profile result.
    #[tokio::test]
    async fn claude_keeps_the_agreement_blocker_alongside_the_profile_check() {
        let mut bedrock = MockBedrockApi::new();
        bedrock
            .expect_get_foundation_model_availability()
            .returning(|id| {
                if id.starts_with("anthropic.") {
                    Ok(response(
                        Some("NOT_AVAILABLE"),
                        Some("AVAILABLE"),
                        Some("AUTHORIZED"),
                        Some("AVAILABLE"),
                    ))
                } else {
                    Ok(available())
                }
            });
        bedrock
            .expect_list_inference_profiles()
            .returning(|| Ok(all_claude_profiles()));

        let data = ready_heartbeat(bedrock).await;

        let sonnet = model(&data, "claude-sonnet-5");
        assert_eq!(sonnet.availability, AiModelAvailability::Blocked);
        assert_eq!(
            sonnet.blockers,
            vec![AiAvailabilityBlocker::AgreementRequired]
        );
    }

    /// A management role set up before it could list inference profiles still gets the
    /// foundation-model result, and the resource is not reported degraded for it.
    #[tokio::test]
    async fn claude_falls_back_to_the_foundation_model_record_when_profiles_cannot_be_listed() {
        let mut bedrock = MockBedrockApi::new();
        bedrock
            .expect_get_foundation_model_availability()
            .returning(|_| Ok(available()));
        bedrock.expect_list_inference_profiles().returning(|| {
            Err(AlienError::new(
                alien_client_core::ErrorData::RemoteAccessDenied {
                    resource_type: "inference profiles".to_string(),
                    resource_name: "list".to_string(),
                },
            ))
        });

        let data = ready_heartbeat(bedrock).await;

        assert_eq!(
            model(&data, "claude-sonnet-5").availability,
            AiModelAvailability::Available
        );
        assert_eq!(data.status.health, ObservedHealth::Healthy);
    }

    /// A Running Bedrock AI resource in a stack, as the state an update carries.
    fn running_ai_stack(
        controller: AwsAiController,
    ) -> (alien_core::Stack, alien_core::StackState) {
        let ai = Ai::new("llm".to_string()).build();
        let stack = alien_core::Stack::new("ai".to_string())
            .add(ai.clone(), alien_core::ResourceLifecycle::Live)
            .build();
        let mut saved = alien_core::StackResourceState::new_pending(
            "ai".to_string(),
            alien_core::Resource::new(ai),
            Some(alien_core::ResourceLifecycle::Live),
            vec![],
        );
        saved.status = ResourceStatus::Running;
        saved
            .set_internal_controller(Some(Box::new(controller)))
            .unwrap();
        let mut state = alien_core::StackState::new(Platform::Aws);
        state.resources.insert("llm".to_string(), saved);
        (stack, state)
    }

    fn observing_executor(
        stack: &alien_core::Stack,
        provider: MockPlatformServiceProvider,
    ) -> StackExecutor {
        let config = alien_core::DeploymentConfig::builder()
            .stack_settings(alien_core::StackSettings::default())
            .environment_variables(alien_core::EnvironmentVariablesSnapshot {
                variables: vec![],
                hash: String::new(),
                created_at: String::new(),
            })
            .external_bindings(alien_core::ExternalBindings::default())
            .allow_frozen_changes(false)
            .build();
        StackExecutor::builder(
            stack,
            alien_core::ClientConfig::Aws(Box::new(AwsClientConfig::mock())),
        )
        .deployment_config(&config)
        .initial_setup_authority(alien_core::InitialSetupAuthority::DirectSetup)
        .service_provider(Arc::new(provider))
        .build()
        .unwrap()
    }

    /// An update observes its untouched resources every 30 seconds. The model lookup is spaced
    /// 15 minutes apart, so the time of the last lookup must survive from one observation to
    /// the next or every observation repeats it.
    #[tokio::test]
    async fn an_observation_remembers_when_models_were_last_looked_up() {
        let mut bedrock = MockBedrockApi::new();
        bedrock
            .expect_get_foundation_model_availability()
            .returning(|_| Ok(available()));
        bedrock
            .expect_list_inference_profiles()
            .returning(|| Ok(all_claude_profiles()));
        let bedrock = Arc::new(bedrock);
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_aws_bedrock_client()
            .times(1)
            .returning(move |_| Ok(bedrock.clone()));
        let (stack, state) = running_ai_stack(AwsAiController {
            state: AwsAiState::Ready,
            region: Some("us-west-2".to_string()),
            ..Default::default()
        });
        let executor = observing_executor(&stack, provider);

        let first = executor.observe_running(state, &[]).await.unwrap();
        let second = executor
            .observe_running(first.state.clone(), &[])
            .await
            .unwrap();

        assert_eq!(first.heartbeats.len(), 1);
        assert_eq!(
            second.heartbeats.len(),
            1,
            "the second observation still reports, from what it remembers"
        );
        let remembered: AwsAiController = first.state.resources["llm"]
            .get_internal_controller_typed()
            .unwrap();
        assert!(remembered.availability_observed_at.is_some());
        assert_eq!(
            second.state.resources["llm"].status,
            ResourceStatus::Running
        );
    }

    /// An observation that fails must leave the resource as it was: Running, with no error,
    /// so a check that cannot run does not fail a resource or an update.
    #[tokio::test]
    async fn a_failed_observation_leaves_the_state_as_it_was() {
        // No region: the Ready handler fails without a retry.
        let (stack, state) = running_ai_stack(AwsAiController {
            state: AwsAiState::Ready,
            region: None,
            ..Default::default()
        });
        let before = serde_json::to_value(&state).unwrap();
        let executor = observing_executor(&stack, MockPlatformServiceProvider::new());

        let observation = executor.observe_running(state, &[]).await.unwrap();

        assert!(observation.heartbeats.is_empty());
        assert_eq!(serde_json::to_value(&observation.state).unwrap(), before);
        assert_eq!(
            observation.state.resources["llm"].status,
            ResourceStatus::Running
        );
    }

    /// An update's own step already ran the resources it changed. Observing them again in the
    /// same pass would repeat their cloud calls and replace the controller state the step saved.
    #[tokio::test]
    async fn an_observation_skips_resources_the_step_already_ran() {
        let mut bedrock = MockBedrockApi::new();
        bedrock.expect_get_foundation_model_availability().times(0);
        bedrock.expect_list_inference_profiles().times(0);
        let bedrock = Arc::new(bedrock);
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_aws_bedrock_client()
            .times(0)
            .returning(move |_| Ok(bedrock.clone()));
        let (stack, state) = running_ai_stack(AwsAiController {
            state: AwsAiState::Ready,
            region: Some("us-west-2".to_string()),
            ..Default::default()
        });
        let before = serde_json::to_value(&state).unwrap();
        let executor = observing_executor(&stack, provider);

        let observation = executor
            .observe_running(state, &["llm".to_string()])
            .await
            .unwrap();

        assert!(observation.heartbeats.is_empty());
        assert_eq!(serde_json::to_value(&observation.state).unwrap(), before);
    }
}
