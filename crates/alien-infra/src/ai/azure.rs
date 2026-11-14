use tracing::info;

use crate::azure_utils::get_resource_group_name;
use crate::core::{ResourceControllerContext, ResourcePermissionsHelper};
use crate::error::{ErrorData, Result};
use alien_azure_clients::azure::cognitive_services::{
    CognitiveServicesAccountCreateParameters, CognitiveServicesAccountCreateProperties,
    CognitiveServicesDeployment, CognitiveServicesDeploymentCreateParameters,
    CognitiveServicesDeploymentCreateProperties, CognitiveServicesDeploymentModel,
    CognitiveServicesDeploymentSku, CognitiveServicesSku,
};
use alien_azure_clients::long_running_operation::OperationResult;
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::{
    ai_catalog::{self, ClientApi},
    bindings::AiBinding,
    Ai, AiAccessTest, AiAvailabilityBlocker, AiAvailabilityObservation, AiAvailabilitySource,
    AiHeartbeatData, AiHeartbeatStatus, AiModelAvailability, AiModelAvailabilityObservation,
    AiOutputs, AzureFoundryAiHeartbeatData, HeartbeatBackend, ObservedHealth, Platform,
    ProviderLifecycleState, ResourceHeartbeat, ResourceHeartbeatData, ResourceOutputs,
    ResourceStatus,
};
use alien_error::{AlienError, Context, ContextError, IntoAlienError};
use alien_macros::controller;
use chrono::Utc;

const AVAILABILITY_REFRESH_INTERVAL: chrono::Duration = chrono::Duration::minutes(15);

fn azure_availability(
    location: Option<String>,
    deployments: &[CognitiveServicesDeployment],
) -> AiAvailabilityObservation {
    let models = ai_catalog::models_for(Platform::Azure)
        .into_iter()
        .map(|model| {
            let deployment = deployments
                .iter()
                .find(|deployment| deployment.name.as_deref() == Some(model.upstream_id));
            let running = deployment
                .and_then(|deployment| deployment.properties.as_ref())
                .and_then(|properties| properties.provisioning_state.as_deref())
                == Some("Succeeded");
            AiModelAvailabilityObservation {
                public_model_id: model.public_id.to_string(),
                client_apis: ClientApi::ALL.to_vec(),
                availability: if running {
                    AiModelAvailability::Available
                } else {
                    AiModelAvailability::Blocked
                },
                blockers: if running {
                    vec![]
                } else {
                    vec![AiAvailabilityBlocker::DeploymentRequired]
                },
                // A Succeeded deployment is generally available, but this
                // read-only heartbeat does not spend quota to invoke it.
                access_test: AiAccessTest::NotChecked,
                tested_at: Some(Utc::now()),
                error_code: None,
                quota_warnings: Vec::new(),
                throttled_last_hour: None,
            }
        })
        .collect();
    AiAvailabilityObservation {
        source: AiAvailabilitySource::AzureFoundry,
        catalog_revision: ai_catalog::AI_CATALOG_REVISION.to_string(),
        location,
        models,
    }
}

/// The quota counter that limits a model's GlobalStandard deployments.
///
/// Azure names the counter after the model's family, which is not always the model name:
/// `gpt-4.1` is `gpt4.1` and `model-router` is `ModelRouter`.
fn usage_name(model_name: &str) -> String {
    let counter = match model_name {
        "gpt-4.1" => "gpt4.1",
        "model-router" => "ModelRouter",
        other => other,
    };
    format!("OpenAI.GlobalStandard.{counter}")
}

/// Capacity to request for one deployment, given the units still free in the
/// subscription's regional quota (`None` when the quota could not be read or lists
/// no counter for the model; Azure then decides when the deployment is created).
/// `Err` means no whole unit is free.
fn deployment_capacity(target: i32, free_units: Option<f64>) -> std::result::Result<i32, ()> {
    match free_units {
        None => Ok(target),
        Some(free) if free >= 1.0 => Ok(target.min(free.floor() as i32)),
        Some(_) => Err(()),
    }
}

/// Derives a globally-unique AIServices account name from the stack prefix and
/// resource id. Azure account names must be 2-64 chars, alphanumeric + hyphens,
/// start with a letter, and end with a letter or digit.
fn make_account_name(prefix: &str, id: &str) -> String {
    let raw = format!("{}-{}", prefix, id);
    // Keep only alphanumeric chars and hyphens; collapse leading/trailing hyphens.
    let cleaned: String = raw
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let trimmed = cleaned.trim_matches('-').to_string();
    if trimmed.len() > 64 {
        trimmed[..64].trim_end_matches('-').to_string()
    } else {
        trimmed
    }
}

#[controller]
pub struct AzureAiController {
    /// The name of the Azure AIServices account.
    pub(crate) account_name: Option<String>,
    /// The endpoint URL returned by the AIServices account once provisioned.
    pub(crate) endpoint: Option<String>,
    /// The Azure resource group containing the account.
    pub(crate) resource_group: Option<String>,
    /// The Azure region where the account is created.
    pub(crate) location: Option<String>,
    #[serde(default)]
    pub(crate) availability: Option<AiAvailabilityObservation>,
    #[serde(default)]
    pub(crate) availability_observed_at: Option<chrono::DateTime<Utc>>,
    /// One line for each deployment created with less capacity than it asked for.
    #[serde(default)]
    pub(crate) capacity_warnings: Vec<String>,
}

#[controller]
impl AzureAiController {
    // ─────────────── CREATE FLOW ──────────────────────────────

    #[flow_entry(Create)]
    #[handler(
        state = CreateStart,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let azure_config = ctx.get_azure_config()?;
        let config = ctx.desired_resource_config::<Ai>()?;

        let account_name = make_account_name(&ctx.resource_prefix, &config.id);
        let resource_group_name = get_resource_group_name(&ctx.state)?;
        let location = azure_config.region.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ClientConfigInvalid {
                platform: Platform::Azure,
                message: "Azure region is required but not specified in configuration".to_string(),
            })
        })?;

        info!(
            id = %config.id,
            account_name = %account_name,
            resource_group = %resource_group_name,
            location = %location,
            "Creating Azure AIServices account"
        );

        let cognitive_client = ctx
            .service_provider
            .get_azure_cognitive_services_client(azure_config)?;

        let parameters = CognitiveServicesAccountCreateParameters {
            location: location.clone(),
            kind: "AIServices".to_string(),
            sku: CognitiveServicesSku {
                name: "S0".to_string(),
            },
            properties: CognitiveServicesAccountCreateProperties {
                custom_sub_domain_name: account_name.clone(),
            },
        };

        let operation_result = cognitive_client
            .create_account(&resource_group_name, &account_name, &parameters)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to create Azure AIServices account '{}'",
                    account_name
                ),
                resource_id: Some(config.id.clone()),
            })?;

        // Store these before matching so both branches can reference them.
        self.account_name = Some(account_name.clone());
        self.resource_group = Some(resource_group_name.clone());
        self.location = Some(location);

        match operation_result {
            OperationResult::Completed(account) => {
                self.endpoint = account.properties.and_then(|p| p.endpoint);

                info!(account_name = %account_name, "Azure AIServices account created (synchronous)");

                Ok(HandlerAction::Continue {
                    state: WaitingForAccountCreation,
                    suggested_delay: None,
                })
            }
            OperationResult::LongRunning(_) => {
                info!(account_name = %account_name, "Azure AIServices account creation is in progress");

                Ok(HandlerAction::Continue {
                    state: WaitingForAccountCreation,
                    suggested_delay: Some(std::time::Duration::from_secs(10)),
                })
            }
        }
    }

    #[handler(
        state = WaitingForAccountCreation,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_account_creation(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let azure_config = ctx.get_azure_config()?;
        let config = ctx.desired_resource_config::<Ai>()?;

        let account_name = self.account_name.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Account name not set in state".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;
        let resource_group_name = self.resource_group.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Resource group not set in state".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        info!(account_name = %account_name, "Polling Azure AIServices account provisioning state");

        let cognitive_client = ctx
            .service_provider
            .get_azure_cognitive_services_client(azure_config)?;

        match cognitive_client
            .get_account(resource_group_name, &account_name)
            .await
        {
            Ok(account) => {
                let provisioning_state = account
                    .properties
                    .as_ref()
                    .and_then(|p| p.provisioning_state.as_deref())
                    .unwrap_or("");

                if provisioning_state.eq_ignore_ascii_case("Succeeded") {
                    self.endpoint = account.properties.and_then(|p| p.endpoint);

                    info!(
                        account_name = %account_name,
                        endpoint = ?self.endpoint,
                        "Azure AIServices account provisioned successfully"
                    );

                    Ok(HandlerAction::Continue {
                        state: ApplyingResourcePermissions,
                        suggested_delay: None,
                    })
                } else {
                    info!(
                        account_name = %account_name,
                        provisioning_state = %provisioning_state,
                        "Azure AIServices account not yet ready, retrying"
                    );

                    Ok(HandlerAction::Continue {
                        state: WaitingForAccountCreation,
                        suggested_delay: Some(std::time::Duration::from_secs(10)),
                    })
                }
            }
            Err(e)
                if matches!(
                    &e.error,
                    Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                info!(account_name = %account_name, "Azure AIServices account not yet visible, retrying");
                Ok(HandlerAction::Continue {
                    state: WaitingForAccountCreation,
                    suggested_delay: Some(std::time::Duration::from_secs(10)),
                })
            }
            Err(e) => Err(e.context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to poll Azure AIServices account '{}' provisioning state",
                    account_name
                ),
                resource_id: Some(config.id.clone()),
            })),
        }
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

        info!(id = %config.id, "Applying Cognitive Services User role on AIServices account");

        let account_name = self.account_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Account name not set in state".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;
        let resource_group_name = self.resource_group.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Resource group not set in state".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        use alien_azure_clients::authorization::Scope;

        let resource_scope = Scope::Resource {
            resource_group_name: resource_group_name.clone(),
            resource_provider: "Microsoft.CognitiveServices".to_string(),
            parent_resource_path: None,
            resource_type: "accounts".to_string(),
            resource_name: account_name.clone(),
        };

        ResourcePermissionsHelper::apply_azure_resource_scoped_permissions(
            ctx,
            &config.id,
            account_name,
            resource_scope,
            "AI",
            "ai",
        )
        .await?;

        info!(id = %config.id, "Successfully applied Cognitive Services User role");

        Ok(HandlerAction::Continue {
            state: DeployingModels,
            suggested_delay: None,
        })
    }

    // ─────────────── MODEL DEPLOYMENT ───────────────────────────

    #[handler(
        state = DeployingModels,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn deploying_models(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let azure_config = ctx.get_azure_config()?;
        let config = ctx.desired_resource_config::<Ai>()?;

        let account_name = self.account_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Account name not set in state".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;
        let resource_group_name = self.resource_group.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Resource group not set in state".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        let cognitive_client = ctx
            .service_provider
            .get_azure_cognitive_services_client(azure_config)?;

        let location = self.location.clone();
        let mut free_units: std::collections::HashMap<String, f64> =
            std::collections::HashMap::new();
        match &location {
            Some(location) => match cognitive_client.list_usages(location).await {
                Ok(usages) => {
                    for usage in usages {
                        if let Some(name) = usage.name.value {
                            free_units
                                .insert(name.to_lowercase(), usage.limit - usage.current_value);
                        }
                    }
                }
                Err(error) => {
                    tracing::warn!(id = %config.id, %error, "Azure model quota could not be read; deployments ask for the default capacity");
                }
            },
            None => {}
        }
        // Units held by deployments this account already has were counted as used by
        // Azure. A retry replaces them, so they are free again for this deployment.
        // Without that list a retry could look out of quota, so ignore the quota then.
        let existing: std::collections::HashMap<String, i32> = match cognitive_client
            .list_deployments(resource_group_name, account_name)
            .await
        {
            Ok(deployments) => deployments
                .into_iter()
                .filter_map(|deployment| Some((deployment.name?, deployment.sku?.capacity)))
                .collect(),
            Err(error) => {
                tracing::warn!(id = %config.id, %error, "Azure model deployments could not be listed; deployments ask for the default capacity");
                free_units.clear();
                std::collections::HashMap::new()
            }
        };

        // Decide every capacity before creating anything, so a model with no free
        // quota fails the deploy without leaving some deployments behind.
        let mut plan = Vec::new();
        let mut warnings = Vec::new();
        for (deployment_name, model_name, model_version, target_capacity) in
            alien_core::ai_catalog::azure_deployments()
        {
            let key = usage_name(model_name).to_lowercase();
            let free = free_units
                .get(&key)
                .map(|free| free + f64::from(existing.get(deployment_name).copied().unwrap_or(0)));
            let capacity = deployment_capacity(target_capacity, free).map_err(|()| {
                AlienError::new(ErrorData::CloudPlatformError {
                    message: format!(
                        "The subscription has no free Azure quota for model '{}' ({}) in {}. \
                         Delete unused deployments in that region or request more quota in the Azure portal, then retry.",
                        model_name,
                        usage_name(model_name),
                        location.as_deref().unwrap_or("the account's region"),
                    ),
                    resource_id: Some(config.id.clone()),
                })
            })?;
            if let Some(free) = free {
                free_units.insert(key, free - f64::from(capacity));
            }
            if capacity < target_capacity {
                warnings.push(format!(
                    "Model '{}' was deployed with {} of {} requested capacity units ({} tokens per minute) because the subscription has no more free quota in {}. Request more quota to raise it.",
                    deployment_name,
                    capacity,
                    target_capacity,
                    u64::from(capacity.unsigned_abs()) * 1000,
                    location.as_deref().unwrap_or("this region"),
                ));
            }
            plan.push((deployment_name, model_name, model_version, capacity));
        }
        self.capacity_warnings = warnings;

        // PUT is idempotent, so re-entering this state (e.g. after a retry) re-issues
        // the same deployments harmlessly.
        for (deployment_name, model_name, model_version, capacity) in plan {
            info!(
                account_name = %account_name,
                deployment = %deployment_name,
                capacity,
                "Creating Azure model deployment"
            );

            let parameters = CognitiveServicesDeploymentCreateParameters {
                sku: CognitiveServicesDeploymentSku {
                    name: "GlobalStandard".to_string(),
                    capacity,
                },
                properties: CognitiveServicesDeploymentCreateProperties {
                    model: CognitiveServicesDeploymentModel {
                        format: "OpenAI".to_string(),
                        name: model_name.to_string(),
                        version: model_version.to_string(),
                    },
                },
            };

            cognitive_client
                .create_deployment(
                    resource_group_name,
                    account_name,
                    deployment_name,
                    &parameters,
                )
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to create Azure model deployment '{}'",
                        deployment_name
                    ),
                    resource_id: Some(config.id.clone()),
                })?;
        }

        Ok(HandlerAction::Continue {
            state: WaitingForDeployments,
            suggested_delay: Some(std::time::Duration::from_secs(10)),
        })
    }

    #[handler(
        state = WaitingForDeployments,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_deployments(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let azure_config = ctx.get_azure_config()?;
        let config = ctx.desired_resource_config::<Ai>()?;

        let account_name = self.account_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Account name not set in state".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;
        let resource_group_name = self.resource_group.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Resource group not set in state".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        let cognitive_client = ctx
            .service_provider
            .get_azure_cognitive_services_client(azure_config)?;

        for (deployment_name, ..) in alien_core::ai_catalog::azure_deployments() {
            match cognitive_client
                .get_deployment(resource_group_name, account_name, deployment_name)
                .await
            {
                Ok(deployment) => {
                    let provisioning_state = deployment
                        .properties
                        .as_ref()
                        .and_then(|p| p.provisioning_state.as_deref())
                        .unwrap_or("");

                    // Deployments routinely reach a terminal failure (in-region
                    // capacity/quota, or the model version not offered there). Fail
                    // fast to CreateFailed rather than polling a dead deployment forever.
                    if provisioning_state.eq_ignore_ascii_case("Failed")
                        || provisioning_state.eq_ignore_ascii_case("Canceled")
                    {
                        return Err(AlienError::new(ErrorData::CloudPlatformError {
                            message: format!(
                                "Azure model deployment '{}' entered terminal state '{}'",
                                deployment_name, provisioning_state
                            ),
                            resource_id: Some(config.id.clone()),
                        }));
                    }

                    if !provisioning_state.eq_ignore_ascii_case("Succeeded") {
                        info!(
                            account_name = %account_name,
                            deployment = %deployment_name,
                            provisioning_state = %provisioning_state,
                            "Model deployment not yet ready, retrying"
                        );
                        return Ok(HandlerAction::Continue {
                            state: WaitingForDeployments,
                            suggested_delay: Some(std::time::Duration::from_secs(10)),
                        });
                    }
                }
                Err(e)
                    if matches!(
                        &e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(
                        account_name = %account_name,
                        deployment = %deployment_name,
                        "Model deployment not yet visible, retrying"
                    );
                    return Ok(HandlerAction::Continue {
                        state: WaitingForDeployments,
                        suggested_delay: Some(std::time::Duration::from_secs(10)),
                    });
                }
                Err(e) => {
                    return Err(e.context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to poll Azure model deployment '{}'",
                            deployment_name
                        ),
                        resource_id: Some(config.id.clone()),
                    }))
                }
            }
        }

        info!(account_name = %account_name, "All Azure model deployments provisioned");
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    // ─────────────── READY STATE ────────────────────────────────

    #[handler(
        state = Ready,
        on_failure = RefreshFailed,
        status = ResourceStatus::Running,
    )]
    async fn ready(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Ai>()?;

        let account_name = self.account_name.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                resource_id: Some(config.id.clone()),
                message: "Account name not set in state".to_string(),
            })
        })?;
        let _endpoint = self.endpoint.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                resource_id: Some(config.id.clone()),
                message: "Endpoint not set in state".to_string(),
            })
        })?;

        info!(id = %config.id, "Azure AI heartbeat tick");
        let resource_group = self.resource_group.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                resource_id: Some(config.id.clone()),
                message: "Resource group not set in state".to_string(),
            })
        })?;
        let refresh = self.availability_observed_at.is_none_or(|observed_at| {
            Utc::now().signed_duration_since(observed_at) >= AVAILABILITY_REFRESH_INTERVAL
        });
        if refresh {
            let client = ctx
                .service_provider
                .get_azure_cognitive_services_client(ctx.get_azure_config()?)?;
            self.availability = Some(
                match client
                    .list_deployments(&resource_group, &account_name)
                    .await
                {
                    Ok(deployments) => azure_availability(self.location.clone(), &deployments),
                    Err(error) => {
                        tracing::warn!(id = %config.id, %error, "Azure AI availability observation failed");
                        let mut availability = AiAvailabilityObservation::unobserved(
                            AiAvailabilitySource::AzureFoundry,
                            Platform::Azure,
                            self.location.clone(),
                        );
                        for model in &mut availability.models {
                            model.tested_at = Some(Utc::now());
                            model.error_code = Some("azure-observation-failed".to_string());
                        }
                        availability
                    }
                },
            );
            self.availability_observed_at = Some(Utc::now());
        }
        let availability = self.availability.clone().unwrap_or_else(|| {
            AiAvailabilityObservation::unobserved(
                AiAvailabilitySource::AzureFoundry,
                Platform::Azure,
                self.location.clone(),
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
            message: {
                let mut lines = Vec::new();
                if partial {
                    lines.push("Some model availability could not be observed".to_string());
                }
                lines.extend(self.capacity_warnings.iter().cloned());
                (!lines.is_empty()).then(|| lines.join(" "))
            },
            stale: false,
            partial,
            collection_issues: vec![],
        };

        ctx.emit_heartbeat(ResourceHeartbeat {
            deployment_id: None,
            resource_id: config.id.clone(),
            resource_type: Ai::RESOURCE_TYPE,
            controller_platform: Platform::Azure,
            backend: HeartbeatBackend::Azure,
            observed_at: Utc::now(),
            data: ResourceHeartbeatData::Ai(AiHeartbeatData::AzureFoundry(
                AzureFoundryAiHeartbeatData {
                    status,
                    account_name: account_name.clone(),
                    endpoint: self.endpoint.clone(),
                    resource_group: self.resource_group.clone(),
                    location: self.location.clone(),
                    availability,
                },
            )),
            raw: vec![],
        });

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: Some(std::time::Duration::from_secs(30)),
        })
    }

    // ─────────────── UPDATE FLOW ──────────────────────────────

    #[flow_entry(Update, from = [Ready, RefreshFailed])]
    #[handler(
        state = UpdateStart,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Ai>()?;
        info!(id = %config.id, "Azure AI update (no-op -- no mutable fields)");
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    // ─────────────── DELETE FLOW ──────────────────────────────

    #[flow_entry(Delete)]
    #[handler(
        state = DeleteStart,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn delete_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let azure_config = ctx.get_azure_config()?;
        let config = ctx.desired_resource_config::<Ai>()?;

        let account_name = match &self.account_name {
            Some(name) => name.clone(),
            None => {
                info!(id = %config.id, "No Azure AIServices account to delete");
                return Ok(HandlerAction::Continue {
                    state: Deleted,
                    suggested_delay: None,
                });
            }
        };
        let resource_group_name = self.resource_group.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Resource group not set in state".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        info!(
            id = %config.id,
            account_name = %account_name,
            "Deleting Azure AIServices account"
        );

        let cognitive_client = ctx
            .service_provider
            .get_azure_cognitive_services_client(azure_config)?;

        match cognitive_client
            .delete_account(resource_group_name, &account_name)
            .await
        {
            Ok(()) => {
                info!(account_name = %account_name, "Azure AIServices account deleted, polling for removal");
                Ok(HandlerAction::Continue {
                    state: WaitingForAccountDeletion,
                    suggested_delay: Some(std::time::Duration::from_secs(5)),
                })
            }
            Err(e)
                if matches!(
                    e.error,
                    Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                info!(account_name = %account_name, "Azure AIServices account already deleted");
                self.clear_state();
                Ok(HandlerAction::Continue {
                    state: Deleted,
                    suggested_delay: None,
                })
            }
            Err(e) => Err(e.context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to delete Azure AIServices account '{}'",
                    account_name
                ),
                resource_id: Some(config.id.clone()),
            })),
        }
    }

    #[handler(
        state = WaitingForAccountDeletion,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn waiting_for_account_deletion(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let azure_config = ctx.get_azure_config()?;
        let config = ctx.desired_resource_config::<Ai>()?;

        let account_name = self.account_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Account name not set in state".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;
        let resource_group_name = self.resource_group.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Resource group not set in state".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        info!(account_name = %account_name, "Polling Azure AIServices account deletion");

        let cognitive_client = ctx
            .service_provider
            .get_azure_cognitive_services_client(azure_config)?;

        match cognitive_client
            .get_account(resource_group_name, account_name)
            .await
        {
            Err(e)
                if matches!(
                    &e.error,
                    Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                info!(account_name = %account_name, "Azure AIServices account confirmed deleted");
                self.clear_state();
                Ok(HandlerAction::Continue {
                    state: Deleted,
                    suggested_delay: None,
                })
            }
            Ok(_) => {
                info!(
                    account_name = %account_name,
                    "Azure AIServices account still exists, retrying deletion poll"
                );
                Ok(HandlerAction::Continue {
                    state: WaitingForAccountDeletion,
                    suggested_delay: Some(std::time::Duration::from_secs(10)),
                })
            }
            Err(e) => Err(e.context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to poll Azure AIServices account '{}' deletion state",
                    account_name
                ),
                resource_id: Some(config.id.clone()),
            })),
        }
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
        let endpoint = self.endpoint.clone()?;
        let account_name = self.account_name.clone()?;
        Some(ResourceOutputs::new(AiOutputs {
            provider: "foundry".into(),
            endpoint: Some(endpoint),
            account: Some(account_name),
        }))
    }

    fn get_binding_params(&self) -> Result<Option<serde_json::Value>> {
        let endpoint = match &self.endpoint {
            Some(ep) => ep.clone(),
            None => return Ok(None),
        };
        let account_name = match &self.account_name {
            Some(a) => a.clone(),
            None => return Ok(None),
        };
        Ok(Some(
            serde_json::to_value(AiBinding::foundry(endpoint, account_name))
                .into_alien_error()
                .context(ErrorData::ResourceStateSerializationFailed {
                    resource_id: "binding".to_string(),
                    message: "Failed to serialize Azure AI binding parameters".to_string(),
                })?,
        ))
    }
}

impl AzureAiController {
    fn clear_state(&mut self) {
        self.account_name = None;
        self.endpoint = None;
        self.resource_group = None;
        self.location = None;
        self.availability = None;
        self.availability_observed_at = None;
        self.capacity_warnings = vec![];
    }

    /// Creates a controller in the ready state for testing purposes.
    #[cfg(feature = "test-utils")]
    pub fn mock_ready(account_name: &str, endpoint: &str) -> Self {
        Self {
            state: AzureAiState::Ready,
            account_name: Some(account_name.to_string()),
            endpoint: Some(endpoint.to_string()),
            resource_group: Some("mock-rg".to_string()),
            location: Some("eastus".to_string()),
            availability: None,
            availability_observed_at: None,
            capacity_warnings: vec![],
            _internal_stay_count: None,
        }
    }

    /// Creates a controller poised to deploy the predefined model set (account
    /// already provisioned), for testing the model-deployment states.
    #[cfg(feature = "test-utils")]
    pub fn mock_deploying(account_name: &str, endpoint: &str) -> Self {
        Self {
            state: AzureAiState::DeployingModels,
            account_name: Some(account_name.to_string()),
            endpoint: Some(endpoint.to_string()),
            resource_group: Some("mock-rg".to_string()),
            location: Some("eastus".to_string()),
            availability: None,
            availability_observed_at: None,
            capacity_warnings: vec![],
            _internal_stay_count: None,
        }
    }
}

#[cfg(all(test, feature = "test-utils"))]
mod tests {
    use super::*;
    use crate::core::controller_test::SingleControllerExecutor;
    use crate::MockPlatformServiceProvider;
    use alien_azure_clients::azure::cognitive_services::{
        CognitiveServicesDeployment, CognitiveServicesDeploymentProperties, CognitiveServicesUsage,
        CognitiveServicesUsageName, MockCognitiveServicesAccountsApi,
    };
    use alien_azure_clients::long_running_operation::OperationResult;
    use alien_client_core::ErrorData as CloudClientErrorData;
    use alien_core::{Ai, AiOutputs, Platform, ResourceHeartbeatData, ResourceStatus};
    use alien_error::AlienError;
    use std::sync::Arc;

    fn basic_ai() -> Ai {
        Ai::new("my-ai".to_string()).build()
    }

    /// A deployment whose model + provisioning state are both reported succeeded.
    fn succeeded_deployment() -> CognitiveServicesDeployment {
        CognitiveServicesDeployment {
            name: Some("gpt-4.1".to_string()),
            sku: None,
            properties: Some(CognitiveServicesDeploymentProperties {
                model: CognitiveServicesDeploymentModel {
                    format: "OpenAI".to_string(),
                    name: "gpt-4.1".to_string(),
                    version: "2025-04-14".to_string(),
                },
                provisioning_state: Some("Succeeded".to_string()),
            }),
        }
    }

    #[test]
    fn availability_uses_exact_succeeded_deployment_names() {
        let observation = azure_availability(Some("eastus".to_string()), &[succeeded_deployment()]);
        let deployed = observation
            .models
            .iter()
            .find(|model| model.public_model_id == "gpt-4.1")
            .unwrap();
        let missing = observation
            .models
            .iter()
            .find(|model| model.public_model_id != "gpt-4.1")
            .unwrap();

        assert_eq!(deployed.availability, AiModelAvailability::Available);
        assert_eq!(deployed.access_test, AiAccessTest::NotChecked);
        assert_eq!(missing.availability, AiModelAvailability::Blocked);
        assert_eq!(
            missing.blockers,
            vec![AiAvailabilityBlocker::DeploymentRequired]
        );
    }

    fn setup_mock_provider_for_deletion(
        expect_not_found: bool,
    ) -> Arc<MockPlatformServiceProvider> {
        let mut mock_provider = MockPlatformServiceProvider::new();

        mock_provider
            .expect_get_azure_cognitive_services_client()
            .returning(move |_| {
                let mut mock_cognitive = MockCognitiveServicesAccountsApi::new();

                if expect_not_found {
                    // Simulate account already deleted.
                    mock_cognitive.expect_delete_account().returning(|_, _| {
                        Err(AlienError::new(
                            CloudClientErrorData::RemoteResourceNotFound {
                                resource_type: "CognitiveServicesAccount".to_string(),
                                resource_name: "my-ai".to_string(),
                            },
                        ))
                    });
                } else {
                    // Successful delete then polling confirms deletion.
                    mock_cognitive
                        .expect_delete_account()
                        .returning(|_, _| Ok(()));
                    mock_cognitive.expect_get_account().returning(|_, _| {
                        Err(AlienError::new(
                            CloudClientErrorData::RemoteResourceNotFound {
                                resource_type: "CognitiveServicesAccount".to_string(),
                                resource_name: "my-ai".to_string(),
                            },
                        ))
                    });
                }

                Ok(Arc::new(mock_cognitive))
            });

        Arc::new(mock_provider)
    }

    #[tokio::test]
    async fn test_deploys_predefined_models_then_ready() {
        // Account already provisioned; drive DeployingModels -> WaitingForDeployments
        // -> Ready with create_deployment + get_deployment(Succeeded) mocked.
        let mut mock_provider = MockPlatformServiceProvider::new();
        mock_provider
            .expect_get_azure_cognitive_services_client()
            .returning(move |_| {
                let mut mock_cognitive = MockCognitiveServicesAccountsApi::new();
                mock_cognitive
                    .expect_list_usages()
                    .returning(|_| Ok(vec![]));
                mock_cognitive
                    .expect_list_deployments()
                    .returning(|_, _| Ok(vec![]));
                mock_cognitive
                    .expect_create_deployment()
                    .returning(|_, _, _, _| Ok(OperationResult::Completed(succeeded_deployment())));
                mock_cognitive
                    .expect_get_deployment()
                    .returning(|_, _, _| Ok(succeeded_deployment()));
                Ok(Arc::new(mock_cognitive))
            });

        let mut executor = SingleControllerExecutor::builder()
            .resource(basic_ai())
            .controller(AzureAiController::mock_deploying(
                "my-ai-account",
                "https://my-ai.cognitiveservices.azure.com/",
            ))
            .platform(Platform::Azure)
            .service_provider(Arc::new(mock_provider))
            .build()
            .await
            .unwrap();

        // Step through the deployment states; Ready is not terminal, so stop once
        // the controller reports Running.
        for _ in 0..5 {
            if executor.status() == ResourceStatus::Running {
                break;
            }
            executor.step().await.unwrap();
        }

        assert_eq!(
            executor.status(),
            ResourceStatus::Running,
            "controller should reach Ready after deploying the predefined models"
        );
    }

    /// A fake Azure subscription: its regional quota counters, the deployments the
    /// account already has, and a record of every deployment PUT it receives.
    #[derive(Default)]
    struct FakeAzure {
        /// (usage name, limit, current value)
        usages: Vec<(String, f64, f64)>,
        usages_unreadable: bool,
        existing: Vec<(String, i32)>,
        deployments_unreadable: bool,
        /// The deployment list stops working once the first deployment is created.
        list_fails_after_puts: bool,
        puts: std::sync::Mutex<Vec<(String, i32)>>,
    }

    impl FakeAzure {
        fn with_quota(models: &[(&str, f64, f64)]) -> Self {
            Self {
                usages: models
                    .iter()
                    .map(|(model, limit, current)| {
                        (usage_name(model), *limit, *current)
                    })
                    .collect(),
                ..Self::default()
            }
        }

        /// Every catalog model has `limit` units with `current` already used.
        fn uniform(limit: f64, current: f64) -> Self {
            let models: Vec<(&str, f64, f64)> = alien_core::ai_catalog::azure_deployments()
                .into_iter()
                .map(|(_, model, _, _)| (model, limit, current))
                .collect();
            Self::with_quota(&models)
        }

        fn provider(self: &Arc<Self>) -> Arc<MockPlatformServiceProvider> {
            let fake = self.clone();
            let mut provider = MockPlatformServiceProvider::new();
            provider
                .expect_get_azure_cognitive_services_client()
                .returning(move |_| {
                    let mut api = MockCognitiveServicesAccountsApi::new();
                    let fake_usages = fake.clone();
                    api.expect_list_usages().returning(move |_| {
                        if fake_usages.usages_unreadable {
                            return Err(AlienError::new(
                                CloudClientErrorData::RemoteAccessDenied {
                                    resource_type: "usages".to_string(),
                                    resource_name: "eastus".to_string(),
                                },
                            ));
                        }
                        Ok(fake_usages
                            .usages
                            .iter()
                            .map(|(name, limit, current)| CognitiveServicesUsage {
                                name: CognitiveServicesUsageName {
                                    value: Some(name.clone()),
                                },
                                current_value: *current,
                                limit: *limit,
                            })
                            .collect())
                    });
                    let fake_existing = fake.clone();
                    api.expect_list_deployments().returning(move |_, _| {
                        if fake_existing.deployments_unreadable
                            || (fake_existing.list_fails_after_puts
                                && !fake_existing.puts.lock().unwrap().is_empty())
                        {
                            return Err(AlienError::new(
                                CloudClientErrorData::RemoteAccessDenied {
                                    resource_type: "deployments".to_string(),
                                    resource_name: "my-ai-account".to_string(),
                                },
                            ));
                        }
                        Ok(fake_existing
                            .existing
                            .iter()
                            .map(|(name, capacity)| CognitiveServicesDeployment {
                                name: Some(name.clone()),
                                sku: Some(CognitiveServicesDeploymentSku {
                                    name: "GlobalStandard".to_string(),
                                    capacity: *capacity,
                                }),
                                properties: None,
                            })
                            .collect())
                    });
                    let fake_puts = fake.clone();
                    api.expect_create_deployment()
                        .returning(move |_, _, name, parameters| {
                            fake_puts
                                .puts
                                .lock()
                                .unwrap()
                                .push((name.to_string(), parameters.sku.capacity));
                            Ok(OperationResult::Completed(succeeded_deployment()))
                        });
                    api.expect_get_deployment()
                        .returning(|_, _, _| Ok(succeeded_deployment()));
                    Ok(Arc::new(api))
                });
            Arc::new(provider)
        }

        fn put_capacity(&self, deployment: &str) -> Option<i32> {
            self.puts
                .lock()
                .unwrap()
                .iter()
                .find(|(name, _)| name == deployment)
                .map(|(_, capacity)| *capacity)
        }
    }

    async fn deploy_models(
        fake: &Arc<FakeAzure>,
    ) -> (
        SingleControllerExecutor,
        std::result::Result<(), AlienError<ErrorData>>,
    ) {
        let mut executor = SingleControllerExecutor::builder()
            .resource(basic_ai())
            .controller(AzureAiController::mock_deploying(
                "my-ai-account",
                "https://my-ai.cognitiveservices.azure.com/",
            ))
            .platform(Platform::Azure)
            .service_provider(fake.provider())
            .build()
            .await
            .unwrap();
        for _ in 0..8 {
            if executor.status() == ResourceStatus::Running {
                break;
            }
            if let Err(error) = executor.step().await {
                return (executor, Err(error));
            }
        }
        // One more step so the Ready state emits its heartbeat.
        let result = executor.step().await.map(|_| ());
        (executor, result)
    }

    #[tokio::test]
    async fn deployments_ask_for_50_units_when_the_quota_allows_it() {
        let fake = Arc::new(FakeAzure::uniform(1000.0, 100.0));
        let (executor, result) = deploy_models(&fake).await;
        result.unwrap();

        assert_eq!(executor.status(), ResourceStatus::Running);
        let puts = fake.puts.lock().unwrap().clone();
        assert_eq!(
            puts.len(),
            alien_core::ai_catalog::azure_deployments().len()
        );
        assert!(
            puts.iter().all(|(_, capacity)| *capacity == 50),
            "every deployment should get 50 units (50,000 tokens per minute), got {puts:?}"
        );
        let heartbeat = executor.last_heartbeats().last().unwrap();
        let ResourceHeartbeatData::Ai(AiHeartbeatData::AzureFoundry(data)) = &heartbeat.data else {
            panic!("expected an Azure AI heartbeat");
        };
        assert_eq!(data.status.message, None);
    }

    /// The counter names Azure lists in eastus2 (read from a live subscription).
    #[test]
    fn counter_names_match_what_azure_lists() {
        let live = [
            "OpenAI.GlobalStandard.gpt4.1",
            "OpenAI.GlobalStandard.ModelRouter",
            "OpenAI.GlobalStandard.gpt-5.5",
            "OpenAI.GlobalStandard.gpt-5.6-sol",
            "OpenAI.GlobalStandard.text-embedding-3-large",
        ];
        for (_, model, _, _) in alien_core::ai_catalog::azure_deployments() {
            assert!(
                live.iter().any(|l| l.eq_ignore_ascii_case(&usage_name(model))),
                "no live counter for {model}: {}",
                usage_name(model)
            );
        }
    }

    #[tokio::test]
    async fn a_deployment_is_capped_to_the_free_quota_and_reports_a_warning() {
        let mut fake = FakeAzure::uniform(1000.0, 0.0);
        for usage in &mut fake.usages {
            if usage.0 == "OpenAI.GlobalStandard.gpt4.1" {
                *usage = (usage.0.clone(), 100.0, 90.0);
            }
        }
        let fake = Arc::new(fake);
        let (executor, result) = deploy_models(&fake).await;
        result.unwrap();

        assert_eq!(executor.status(), ResourceStatus::Running);
        assert_eq!(fake.put_capacity("gpt-4.1"), Some(10));
        assert_eq!(fake.put_capacity("model-router"), Some(50));
        let heartbeat = executor.last_heartbeats().last().unwrap();
        let ResourceHeartbeatData::Ai(AiHeartbeatData::AzureFoundry(data)) = &heartbeat.data else {
            panic!("expected an Azure AI heartbeat");
        };
        let message = data.status.message.as_deref().expect("a capacity warning");
        assert!(message.contains("gpt-4.1"), "{message}");
        assert!(message.contains("10 of 50"), "{message}");
        assert!(!message.contains("model-router"), "{message}");
    }

    #[tokio::test]
    async fn a_capacity_warning_survives_a_failed_availability_read() {
        let mut fake = FakeAzure::uniform(1000.0, 0.0);
        for usage in &mut fake.usages {
            if usage.0 == "OpenAI.GlobalStandard.gpt4.1" {
                *usage = (usage.0.clone(), 100.0, 90.0);
            }
        }
        fake.list_fails_after_puts = true;
        let fake = Arc::new(fake);
        let (executor, result) = deploy_models(&fake).await;
        result.unwrap();

        let heartbeat = executor.last_heartbeats().last().unwrap();
        let ResourceHeartbeatData::Ai(AiHeartbeatData::AzureFoundry(data)) = &heartbeat.data else {
            panic!("expected an Azure AI heartbeat");
        };
        assert!(data.status.partial);
        let message = data.status.message.as_deref().unwrap();
        assert!(message.contains("could not be observed"), "{message}");
        assert!(message.contains("10 of 50"), "{message}");
    }

    #[tokio::test]
    async fn no_free_quota_fails_before_creating_any_deployment() {
        let mut fake = FakeAzure::uniform(1000.0, 0.0);
        for usage in &mut fake.usages {
            if usage.0 == "OpenAI.GlobalStandard.ModelRouter" {
                *usage = (usage.0.clone(), 30.0, 30.0);
            }
        }
        let fake = Arc::new(fake);
        let (_executor, result) = deploy_models(&fake).await;

        let error = result.expect_err("a model with no free quota must fail the deploy");
        let text = error.to_string();
        assert!(text.contains("model-router"), "{text}");
        assert!(text.to_lowercase().contains("quota"), "{text}");
        assert!(
            fake.puts.lock().unwrap().is_empty(),
            "no deployment should be created when one model has no quota"
        );
    }

    #[tokio::test]
    async fn a_retry_does_not_count_its_own_deployments_against_the_quota() {
        // The first attempt already created gpt-4.1 with 50 units, which now shows as
        // used. Re-running must not see that as "no quota left".
        let mut fake = FakeAzure::uniform(1000.0, 0.0);
        for usage in &mut fake.usages {
            if usage.0 == "OpenAI.GlobalStandard.gpt4.1" {
                *usage = (usage.0.clone(), 50.0, 50.0);
            }
        }
        fake.existing = vec![("gpt-4.1".to_string(), 50)];
        let fake = Arc::new(fake);
        let (executor, result) = deploy_models(&fake).await;
        result.unwrap();

        assert_eq!(executor.status(), ResourceStatus::Running);
        assert_eq!(fake.put_capacity("gpt-4.1"), Some(50));
    }

    #[tokio::test]
    async fn an_unreadable_deployment_list_does_not_fail_a_retry_for_quota() {
        // The account holds all 50 units in gpt-4.1 itself, but the list that would
        // show that is unreadable. The quota must not be used to refuse the deploy.
        let mut fake = FakeAzure::uniform(1000.0, 0.0);
        for usage in &mut fake.usages {
            if usage.0 == "OpenAI.GlobalStandard.gpt4.1" {
                *usage = (usage.0.clone(), 50.0, 50.0);
            }
        }
        fake.deployments_unreadable = true;
        let fake = Arc::new(fake);
        let (executor, result) = deploy_models(&fake).await;
        result.unwrap();

        assert_eq!(executor.status(), ResourceStatus::Running);
        assert_eq!(fake.put_capacity("gpt-4.1"), Some(50));
    }

    #[tokio::test]
    async fn unreadable_quota_still_deploys_at_the_default() {
        let mut fake = FakeAzure::default();
        fake.usages_unreadable = true;
        let fake = Arc::new(fake);
        let (executor, result) = deploy_models(&fake).await;
        result.unwrap();

        assert_eq!(executor.status(), ResourceStatus::Running);
        assert_eq!(fake.put_capacity("gpt-4.1"), Some(50));
    }

    #[tokio::test]
    async fn test_deployment_terminal_failure_fails_fast() {
        // A deployment that reports a terminal "Failed" state must route to
        // CreateFailed (ProvisionFailed), not poll forever.
        let mut mock_provider = MockPlatformServiceProvider::new();
        mock_provider
            .expect_get_azure_cognitive_services_client()
            .returning(move |_| {
                let mut mock_cognitive = MockCognitiveServicesAccountsApi::new();
                mock_cognitive
                    .expect_list_usages()
                    .returning(|_| Ok(vec![]));
                mock_cognitive
                    .expect_list_deployments()
                    .returning(|_, _| Ok(vec![]));
                mock_cognitive
                    .expect_create_deployment()
                    .returning(|_, _, _, _| Ok(OperationResult::Completed(succeeded_deployment())));
                mock_cognitive.expect_get_deployment().returning(|_, _, _| {
                    Ok(CognitiveServicesDeployment {
                        name: Some("gpt-4.1".to_string()),
                        sku: None,
                        properties: Some(CognitiveServicesDeploymentProperties {
                            model: CognitiveServicesDeploymentModel {
                                format: "OpenAI".to_string(),
                                name: "gpt-4.1".to_string(),
                                version: "2025-04-14".to_string(),
                            },
                            provisioning_state: Some("Failed".to_string()),
                        }),
                    })
                });
                Ok(Arc::new(mock_cognitive))
            });

        let mut executor = SingleControllerExecutor::builder()
            .resource(basic_ai())
            .controller(AzureAiController::mock_deploying(
                "my-ai-account",
                "https://my-ai.cognitiveservices.azure.com/",
            ))
            .platform(Platform::Azure)
            .service_provider(Arc::new(mock_provider))
            .build()
            .await
            .unwrap();

        // A "Failed" deployment must surface as a handler error (which the executor
        // routes to CreateFailed), not an endless poll. Bounded so a regression
        // (poll-forever) fails the test instead of hanging.
        let mut surfaced_error = false;
        for _ in 0..5 {
            if executor.step().await.is_err() {
                surfaced_error = true;
                break;
            }
        }

        assert!(
            surfaced_error,
            "a terminal deployment failure must surface as an error, not a silent retry"
        );
    }

    #[tokio::test]
    async fn test_ready_controller_has_correct_outputs() {
        let account_name = "my-ai-account";
        let endpoint = "https://my-ai.cognitiveservices.azure.com/";

        let mock_provider = setup_mock_provider_for_deletion(false);
        let executor = SingleControllerExecutor::builder()
            .resource(basic_ai())
            .controller(AzureAiController::mock_ready(account_name, endpoint))
            .platform(Platform::Azure)
            .service_provider(mock_provider)
            .build()
            .await
            .unwrap();

        assert_eq!(executor.status(), ResourceStatus::Running);

        let outputs = executor.outputs().unwrap();
        let ai_outputs = outputs.downcast_ref::<AiOutputs>().unwrap();
        assert_eq!(ai_outputs.provider, "foundry");
        assert_eq!(ai_outputs.endpoint.as_deref(), Some(endpoint));
        assert_eq!(ai_outputs.account.as_deref(), Some(account_name));
    }

    #[tokio::test]
    async fn test_delete_flow_succeeds() {
        let account_name = "my-ai-account";
        let endpoint = "https://my-ai.cognitiveservices.azure.com/";

        let mock_provider = setup_mock_provider_for_deletion(false);
        let mut executor = SingleControllerExecutor::builder()
            .resource(basic_ai())
            .controller(AzureAiController::mock_ready(account_name, endpoint))
            .platform(Platform::Azure)
            .service_provider(mock_provider)
            .build()
            .await
            .unwrap();

        executor.delete().unwrap();
        executor.run_until_terminal().await.unwrap();

        assert_eq!(executor.status(), ResourceStatus::Deleted);
    }

    #[tokio::test]
    async fn test_delete_already_gone_succeeds() {
        // Deleting when the account is already gone (RemoteResourceNotFound on delete_account)
        // must succeed, not fail. This is the best-effort-delete path.
        let account_name = "my-ai-account";
        let endpoint = "https://my-ai.cognitiveservices.azure.com/";

        let mock_provider = setup_mock_provider_for_deletion(true);
        let mut executor = SingleControllerExecutor::builder()
            .resource(basic_ai())
            .controller(AzureAiController::mock_ready(account_name, endpoint))
            .platform(Platform::Azure)
            .service_provider(mock_provider)
            .build()
            .await
            .unwrap();

        executor.delete().unwrap();
        executor.run_until_terminal().await.unwrap();

        assert_eq!(executor.status(), ResourceStatus::Deleted);
    }
}
