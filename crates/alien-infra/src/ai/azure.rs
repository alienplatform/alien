use tracing::info;

use crate::azure_utils::get_resource_group_name;
use crate::core::{ResourceControllerContext, ResourcePermissionsHelper};
use crate::error::{ErrorData, Result};
use crate::storage::AzureStorageController;
use alien_azure_clients::azure::cognitive_services::{
    CognitiveServicesAccountCreateParameters, CognitiveServicesAccountCreateProperties,
    CognitiveServicesDeploymentCreateParameters, CognitiveServicesDeploymentCreateProperties,
    CognitiveServicesDeploymentModel, CognitiveServicesDeploymentSku, CognitiveServicesSku,
};
use alien_azure_clients::long_running_operation::OperationResult;
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::{
    bindings::{AiBinding, FinetuneCapability},
    Ai, AiHeartbeatData, AiHeartbeatStatus, AiOutputs, AzureFoundryAiHeartbeatData,
    HeartbeatBackend, Platform, ResourceHeartbeat, ResourceHeartbeatData, ResourceOutputs,
    ResourceRef, ResourceStatus, Storage,
};
use alien_error::{AlienError, Context, ContextError, IntoAlienError};
use alien_macros::controller;
use chrono::Utc;

/// Provisioned throughput (in GlobalStandard units) for each predefined model
/// deployment. A conservative default; per-region quota tuning is a deploy-time
/// concern verified against the target subscription.
const DEFAULT_DEPLOYMENT_CAPACITY: i32 = 1;

/// Derives a globally-unique AIServices account name from the stack prefix and
/// resource id. Azure account names must be 2-64 chars, alphanumeric + hyphens,
/// start with a letter, and end with a letter or digit.
fn make_account_name(prefix: &str, id: &str) -> String {
    let raw = format!("{}-{}", prefix, id);
    // Keep only alphanumeric chars and hyphens; collapse leading/trailing hyphens.
    let cleaned: String = raw
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' { c } else { '-' })
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
    /// The fine-tuning capability this gateway advertises, resolved during the
    /// create flow when the resource declares a `finetune` spec. The gateway
    /// submits and rediscovers the Foundry fine-tuning job at runtime from this
    /// capability; the controller never starts a job itself. Captured into state
    /// (rather than rebuilt in `get_binding_params`, which has no `ctx`) so the
    /// binding can carry it. `None` for a pure-inference gateway.
    #[serde(default)]
    pub(crate) finetune: Option<FinetuneCapability>,
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
                message: format!("Failed to create Azure AIServices account '{}'", account_name),
                resource_id: Some(config.id.clone()),
            })?;

        // Store these before matching so both branches can reference them.
        self.account_name = Some(account_name.clone());
        self.resource_group = Some(resource_group_name.clone());
        self.location = Some(location);

        match operation_result {
            OperationResult::Completed(account) => {
                self.endpoint = account
                    .properties
                    .and_then(|p| p.endpoint);

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

        info!(account_name = %account_name, "Polling Azure AIServices account provisioning state");

        let cognitive_client = ctx
            .service_provider
            .get_azure_cognitive_services_client(azure_config)?;

        match cognitive_client
            .get_account(resource_group_name, account_name)
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

        info!(id = %config.id, "Applying Cognitive Services OpenAI User role on AIServices account");

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

        info!(id = %config.id, "Successfully applied Cognitive Services OpenAI User role");

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

        // PUT is idempotent, so re-entering this state (e.g. after a retry) re-issues
        // the same deployments harmlessly.
        for (deployment_name, model_name, model_version) in alien_core::ai_catalog::azure_deployments()
        {
            info!(
                account_name = %account_name,
                deployment = %deployment_name,
                "Creating Azure model deployment"
            );

            let parameters = CognitiveServicesDeploymentCreateParameters {
                sku: CognitiveServicesDeploymentSku {
                    name: "GlobalStandard".to_string(),
                    capacity: DEFAULT_DEPLOYMENT_CAPACITY,
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

        for (deployment_name, _, _) in alien_core::ai_catalog::azure_deployments() {
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

        // Fine-tuning is triggered at runtime through the gateway, not at deploy
        // time, so the resource is Ready as soon as the base deployments succeed.
        // When the resource declares a `finetune` spec, resolve the capability the
        // gateway needs to submit/rediscover a runtime tuning job and carry it on
        // the binding.
        self.finetune = self.resolve_finetune_capability(ctx, &config)?;

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

        let account_name = self.account_name.as_ref().ok_or_else(|| {
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

        ctx.emit_heartbeat(ResourceHeartbeat {
            deployment_id: None,
            resource_id: config.id.clone(),
            resource_type: Ai::RESOURCE_TYPE,
            controller_platform: Platform::Azure,
            backend: HeartbeatBackend::Azure,
            observed_at: Utc::now(),
            data: ResourceHeartbeatData::Ai(AiHeartbeatData::AzureFoundry(
                AzureFoundryAiHeartbeatData {
                    status: AiHeartbeatStatus::default(),
                    account_name: account_name.clone(),
                    endpoint: self.endpoint.clone(),
                    resource_group: self.resource_group.clone(),
                    location: self.location.clone(),
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
                message: format!("Failed to delete Azure AIServices account '{}'", account_name),
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

        let mut binding = AiBinding::foundry(endpoint, account_name);

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
                message: "Failed to serialize Azure AI binding parameters".to_string(),
            },
        )?))
    }
}

impl AzureAiController {
    fn clear_state(&mut self) {
        self.account_name = None;
        self.endpoint = None;
        self.resource_group = None;
        self.location = None;
        self.finetune = None;
    }

    /// Builds the fine-tuning capability the binding carries when the resource
    /// declares a `finetune` spec, or `None` for a pure-inference gateway.
    ///
    /// Resolves the training data's real Blob container from the storage
    /// dependency's controller state (rather than re-deriving the prefixed name),
    /// keeping it in lockstep with the storage controller's naming. Foundry
    /// submits the runtime tuning job under the gateway's ambient identity, so no
    /// role is passed (`role_arn` is empty).
    fn resolve_finetune_capability(
        &self,
        ctx: &ResourceControllerContext<'_>,
        config: &Ai,
    ) -> Result<Option<FinetuneCapability>> {
        let Some(spec) = config.finetune.as_ref() else {
            return Ok(None);
        };

        let training_ref = ResourceRef::new(Storage::RESOURCE_TYPE, spec.training_data.clone());
        let storage_state = ctx.require_dependency::<AzureStorageController>(&training_ref)?;
        let training_bucket = storage_state.container_name.ok_or_else(|| {
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
            // Foundry submits under the ambient identity; no passed role.
            role_arn: String::new(),
        }))
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
            finetune: None,
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
            finetune: None,
            _internal_stay_count: None,
        }
    }
}

#[cfg(all(test, feature = "test-utils"))]
mod tests {
    use super::*;
    use crate::core::controller_test::SingleControllerExecutor;
    use crate::core::ResourceController;
    use crate::MockPlatformServiceProvider;
    use alien_azure_clients::azure::cognitive_services::{
        CognitiveServicesDeployment, CognitiveServicesDeploymentProperties,
        MockCognitiveServicesAccountsApi,
    };
    use alien_azure_clients::long_running_operation::OperationResult;
    use alien_client_core::ErrorData as CloudClientErrorData;
    use crate::storage::AzureStorageController;
    use alien_core::bindings::AiBinding;
    use alien_core::{
        Ai, AiOutputs, FinetuneMethod, FinetuneSpec, Platform, ResourceStatus, Storage,
    };
    use alien_error::AlienError;
    use std::sync::Arc;

    fn basic_ai() -> Ai {
        Ai::new("my-ai".to_string()).build()
    }

    /// The training-data storage id the tuned resource depends on. Matches the
    /// Azure test storage account dependency wired by `with_test_dependencies`.
    const TRAINING_STORAGE_ID: &str = "training-set";

    fn tuned_ai() -> Ai {
        Ai::new("my-ai".to_string())
            .finetune(FinetuneSpec {
                base_model: "gpt-4o-mini".to_string(),
                training_data: TRAINING_STORAGE_ID.to_string(),
                training_key: "training.jsonl".to_string(),
                served_model_id: None,
                method: FinetuneMethod::Sft,
            })
            .build()
    }

    /// The training-data Storage the tuned resource declares as a dependency
    /// (via `Ai::get_dependencies`). Must be wired Ready or the executor won't
    /// run the AI controller.
    fn training_storage() -> Storage {
        Storage::new(TRAINING_STORAGE_ID.to_string()).build()
    }

    /// A cognitive-services mock that succeeds all base deployments (create +
    /// get both report Succeeded).
    fn mock_cognitive_all_succeed() -> MockCognitiveServicesAccountsApi {
        let mut mock = MockCognitiveServicesAccountsApi::new();
        mock.expect_create_deployment()
            .returning(|_, _, _, _| Ok(OperationResult::Completed(succeeded_deployment())));
        mock.expect_get_deployment()
            .returning(|_, _, _| Ok(succeeded_deployment()));
        mock
    }

    /// Read the controller's current binding via the ResourceController trait.
    fn current_binding(executor: &SingleControllerExecutor) -> AiBinding {
        let controller = executor
            .internal_state::<AzureAiController>()
            .expect("controller is AzureAiController");
        let value = controller
            .get_binding_params()
            .expect("binding params resolve")
            .expect("binding present once endpoint + account are set");
        serde_json::from_value(value).expect("binding deserializes")
    }

    /// A deployment whose model + provisioning state are both reported succeeded.
    fn succeeded_deployment() -> CognitiveServicesDeployment {
        CognitiveServicesDeployment {
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

    fn setup_mock_provider_for_deletion(expect_not_found: bool) -> Arc<MockPlatformServiceProvider> {
        let mut mock_provider = MockPlatformServiceProvider::new();

        mock_provider
            .expect_get_azure_cognitive_services_client()
            .returning(move |_| {
                let mut mock_cognitive = MockCognitiveServicesAccountsApi::new();

                if expect_not_found {
                    // Simulate account already deleted.
                    mock_cognitive
                        .expect_delete_account()
                        .returning(|_, _| {
                            Err(AlienError::new(CloudClientErrorData::RemoteResourceNotFound {
                                resource_type: "CognitiveServicesAccount".to_string(),
                                resource_name: "my-ai".to_string(),
                            }))
                        });
                } else {
                    // Successful delete then polling confirms deletion.
                    mock_cognitive
                        .expect_delete_account()
                        .returning(|_, _| Ok(()));
                    mock_cognitive
                        .expect_get_account()
                        .returning(|_, _| {
                            Err(AlienError::new(CloudClientErrorData::RemoteResourceNotFound {
                                resource_type: "CognitiveServicesAccount".to_string(),
                                resource_name: "my-ai".to_string(),
                            }))
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
                    .expect_create_deployment()
                    .returning(|_, _, _, _| Ok(OperationResult::Completed(succeeded_deployment())));
                mock_cognitive.expect_get_deployment().returning(|_, _, _| {
                    Ok(CognitiveServicesDeployment {
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

    // ─────────────── FINE-TUNING TESTS ──────────────────────────

    #[tokio::test]
    async fn test_finetune_reaches_ready_immediately_with_capability() {
        // Fine-tuning is triggered at runtime through the gateway, so a finetune
        // resource reaches Ready as soon as the base deployments succeed — no
        // tuning job or tuned deployment is created at deploy time. The Foundry
        // finetuning client is never wired, so a call to it would panic on the
        // unset expectation, catching any regression that re-introduces
        // deploy-time tuning. The binding carries the FinetuneCapability instead.
        let mut mock_provider = MockPlatformServiceProvider::new();
        mock_provider
            .expect_get_azure_cognitive_services_client()
            .returning(|_| Ok(Arc::new(mock_cognitive_all_succeed())));

        let mut executor = SingleControllerExecutor::builder()
            .resource(tuned_ai())
            .controller(AzureAiController::mock_deploying(
                "default-storage-account",
                "https://my-ai.cognitiveservices.azure.com/",
            ))
            .platform(Platform::Azure)
            .service_provider(Arc::new(mock_provider))
            .with_test_dependencies()
            .with_dependency(
                training_storage(),
                AzureStorageController::mock_ready(TRAINING_STORAGE_ID),
            )
            .build()
            .await
            .expect("executor builds");

        // Ready is a heartbeat loop (not terminal), so step until Running.
        for _ in 0..8 {
            if executor.status() == ResourceStatus::Running {
                break;
            }
            executor.step().await.expect("step should not error");
        }
        assert_eq!(
            executor.status(),
            ResourceStatus::Running,
            "a finetune resource is Ready immediately; the gateway triggers tuning at runtime"
        );

        let binding = current_binding(&executor);
        assert!(
            binding.tuned_model().is_none(),
            "the controller must not attach a tuned model; the gateway rediscovers it"
        );
        let cap = binding
            .finetune()
            .expect("finetune binding must carry the fine-tuning capability");
        assert_eq!(cap.base_model, "gpt-4o-mini");
        // Bucket resolved from the storage dependency's container name
        // (test-stack-<id>, from AzureStorageController::mock_ready), not
        // re-derived here.
        assert_eq!(cap.training_bucket, "test-stack-training-set");
        assert_eq!(cap.training_key, "training.jsonl");
        assert_eq!(cap.served_model_id, "my-ai-tuned");
        // Deterministic {prefix}-{id}; the executor test harness uses the
        // resource prefix "test" (distinct from the storage mock's "test-stack"
        // container-name prefix).
        assert_eq!(cap.job_name, "test-my-ai");
        assert!(
            cap.role_arn.is_empty(),
            "Foundry submits under the ambient identity; no role is passed"
        );
    }

    #[tokio::test]
    async fn test_no_finetune_reaches_ready_with_plain_binding() {
        // Regression: an inference-only resource reaches Ready with a plain
        // binding carrying neither a tuned model nor a finetune capability.
        let mut mock_provider = MockPlatformServiceProvider::new();
        mock_provider
            .expect_get_azure_cognitive_services_client()
            .returning(|_| Ok(Arc::new(mock_cognitive_all_succeed())));

        let mut executor = SingleControllerExecutor::builder()
            .resource(basic_ai())
            .controller(AzureAiController::mock_deploying(
                "my-ai-account",
                "https://my-ai.cognitiveservices.azure.com/",
            ))
            .platform(Platform::Azure)
            .service_provider(Arc::new(mock_provider))
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
        assert!(
            binding.finetune().is_none(),
            "inference-only binding must omit the finetune capability"
        );
    }
}
