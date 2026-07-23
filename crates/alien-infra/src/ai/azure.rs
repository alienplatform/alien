use tracing::info;

use crate::azure_utils::get_resource_group_name;
use crate::core::{ResourceControllerContext, ResourcePermissionsHelper};
use crate::error::{ErrorData, Result};
use crate::infra_requirements::azure_utils::get_storage_account_name;
use alien_azure_clients::azure::cognitive_services::{
    CognitiveServicesAccountCreateParameters, CognitiveServicesAccountCreateProperties,
    CognitiveServicesDeploymentCreateParameters, CognitiveServicesDeploymentCreateProperties,
    CognitiveServicesDeploymentModel, CognitiveServicesDeploymentSku, CognitiveServicesSku,
};
use alien_azure_clients::long_running_operation::OperationResult;
use alien_core::FinetuneSpec;
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::{
    bindings::AiBinding, Ai, AiHeartbeatData, AiHeartbeatStatus, AiOutputs,
    AzureFoundryAiHeartbeatData, HeartbeatBackend, Platform, ResourceHeartbeat,
    ResourceHeartbeatData, ResourceOutputs, ResourceStatus,
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
    /// The Foundry fine-tuning job id, set once the job is submitted. Only
    /// populated when the resource declares a `finetune` spec.
    #[serde(default)]
    pub(crate) tuning_job_id: Option<String>,
    /// The deployment name serving the tuned model, set once the tuned model is
    /// deployed. This is the `upstream_id` the gateway forwards to over the
    /// OpenAI chat path. Absent for a pure inference gateway.
    #[serde(default)]
    pub(crate) tuned_deployment_name: Option<String>,
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

        // A pure inference gateway is Ready here. When the resource declares a
        // finetune spec, tune the base model and serve the result before Ready.
        if config.finetune.is_some() {
            return Ok(HandlerAction::Continue {
                state: SubmittingTuningJob,
                suggested_delay: None,
            });
        }

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    // ─────────────── FINE-TUNING FLOW ───────────────────────────

    #[handler(
        state = SubmittingTuningJob,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn submitting_tuning_job(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let azure_config = ctx.get_azure_config()?;
        let config = ctx.desired_resource_config::<Ai>()?;
        let spec = require_finetune_spec(&config)?;

        let endpoint = self.require_endpoint(&config.id)?.to_string();

        // Foundry imports the training dataset directly from the customer's Blob
        // container. We derive the blob URL deterministically the same way the
        // Azure Storage controller names the container
        // (`{prefix}-{training_data}`, lowercased, `_`->`-`) under the shared
        // default storage account, rather than requiring a public storage state
        // type. See `crates/alien-infra/src/storage/azure.rs`.
        //
        // GOTCHA: Foundry's Blob import path requires the AIServices account to
        // allow *public network access*. A private-endpoint-only account rejects
        // the blob URL at job-submit time; the job then surfaces as a terminal
        // failure in WaitingForTuningJob (fail-fast), not a silent hang.
        let storage_account = get_storage_account_name(ctx.state)?;
        let training_file = training_blob_url(
            &storage_account,
            ctx.resource_prefix,
            &spec.training_data,
            &spec.training_key,
        );

        info!(
            id = %config.id,
            endpoint = %endpoint,
            base_model = %spec.base_model,
            training_file = %training_file,
            "Submitting Azure Foundry fine-tuning job"
        );

        let finetuning_client = ctx
            .service_provider
            .get_azure_foundry_finetuning_client(azure_config)?;

        let job = finetuning_client
            .create_fine_tuning_job(&endpoint, &spec.base_model, &training_file)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to submit Azure Foundry fine-tuning job for base model '{}'",
                    spec.base_model
                ),
                resource_id: Some(config.id.clone()),
            })?;

        info!(id = %config.id, job_id = %job.id, status = %job.status, "Fine-tuning job submitted");
        self.tuning_job_id = Some(job.id);

        Ok(HandlerAction::Continue {
            state: WaitingForTuningJob,
            suggested_delay: Some(std::time::Duration::from_secs(30)),
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
        let azure_config = ctx.get_azure_config()?;
        let config = ctx.desired_resource_config::<Ai>()?;

        let endpoint = self.require_endpoint(&config.id)?.to_string();
        let job_id = self.tuning_job_id.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Fine-tuning job id not set in state".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        info!(id = %config.id, job_id = %job_id, "Polling Azure Foundry fine-tuning job");

        let finetuning_client = ctx
            .service_provider
            .get_azure_foundry_finetuning_client(azure_config)?;

        let job = finetuning_client
            .get_fine_tuning_job(&endpoint, job_id)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to poll Azure Foundry fine-tuning job '{}'", job_id),
                resource_id: Some(config.id.clone()),
            })?;

        // Fail fast on a terminal failure rather than polling a dead job forever,
        // mirroring the WaitingForDeployments style.
        if job.is_terminal_failure() {
            return Err(AlienError::new(ErrorData::CloudPlatformError {
                message: format!(
                    "Azure Foundry fine-tuning job '{}' entered terminal state '{}'",
                    job.id, job.status
                ),
                resource_id: Some(config.id.clone()),
            }));
        }

        if !job.is_succeeded() {
            info!(id = %config.id, job_id = %job.id, status = %job.status, "Fine-tuning job not yet complete, retrying");
            return Ok(HandlerAction::Continue {
                state: WaitingForTuningJob,
                suggested_delay: Some(std::time::Duration::from_secs(30)),
            });
        }

        // Succeeded: capture the tuned model name to deploy. Fail fast if the
        // provider reports success without one — we cannot serve a missing model.
        let fine_tuned_model = job.fine_tuned_model.ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: format!(
                    "Azure Foundry fine-tuning job '{}' succeeded but returned no fine_tuned_model",
                    job.id
                ),
                resource_id: Some(config.id.clone()),
            })
        })?;

        info!(id = %config.id, fine_tuned_model = %fine_tuned_model, "Fine-tuning job succeeded");

        // Deploy the tuned model under the served id, so the OpenAI chat path
        // accepts it as a deployment target.
        let spec = require_finetune_spec(&config)?;
        let served_id = spec.served_model_id_or_default(&config.id);
        self.deploy_tuned_model(ctx, &config.id, &served_id, &fine_tuned_model)
            .await?;
        self.tuned_deployment_name = Some(served_id);

        Ok(HandlerAction::Continue {
            state: WaitingForTunedDeployment,
            suggested_delay: Some(std::time::Duration::from_secs(10)),
        })
    }

    #[handler(
        state = WaitingForTunedDeployment,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_tuned_deployment(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let azure_config = ctx.get_azure_config()?;
        let config = ctx.desired_resource_config::<Ai>()?;

        let account_name = self.require_account_name(&config.id)?.to_string();
        let resource_group_name = self.require_resource_group(&config.id)?.to_string();
        let deployment_name = self.tuned_deployment_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Tuned deployment name not set in state".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        let cognitive_client = ctx
            .service_provider
            .get_azure_cognitive_services_client(azure_config)?;

        match cognitive_client
            .get_deployment(&resource_group_name, &account_name, deployment_name)
            .await
        {
            Ok(deployment) => {
                let provisioning_state = deployment
                    .properties
                    .as_ref()
                    .and_then(|p| p.provisioning_state.as_deref())
                    .unwrap_or("");

                if provisioning_state.eq_ignore_ascii_case("Failed")
                    || provisioning_state.eq_ignore_ascii_case("Canceled")
                {
                    return Err(AlienError::new(ErrorData::CloudPlatformError {
                        message: format!(
                            "Azure tuned-model deployment '{}' entered terminal state '{}'",
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
                        "Tuned-model deployment not yet ready, retrying"
                    );
                    return Ok(HandlerAction::Continue {
                        state: WaitingForTunedDeployment,
                        suggested_delay: Some(std::time::Duration::from_secs(10)),
                    });
                }

                info!(account_name = %account_name, deployment = %deployment_name, "Tuned-model deployment provisioned");
                Ok(HandlerAction::Continue {
                    state: Ready,
                    suggested_delay: None,
                })
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
                    "Tuned-model deployment not yet visible, retrying"
                );
                Ok(HandlerAction::Continue {
                    state: WaitingForTunedDeployment,
                    suggested_delay: Some(std::time::Duration::from_secs(10)),
                })
            }
            Err(e) => Err(e.context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to poll Azure tuned-model deployment '{}'",
                    deployment_name
                ),
                resource_id: Some(config.id.clone()),
            })),
        }
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

        // Attach the tuned model only once its deployment exists, so a pure
        // inference gateway keeps the unchanged untuned wire shape. We deploy the
        // tuned model under a deployment named `served_id`, and Foundry's OpenAI
        // chat path takes that same deployment name as the request-body `model`.
        // So served_id and upstream_id are the one stored deployment name.
        if let Some(deployment_name) = &self.tuned_deployment_name {
            binding = binding.with_tuned_model(deployment_name.clone(), deployment_name.clone());
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
        self.tuning_job_id = None;
        self.tuned_deployment_name = None;
    }

    /// Returns the provisioned account endpoint, or a config error if unset.
    fn require_endpoint(&self, resource_id: &str) -> Result<&str> {
        self.endpoint.as_deref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Endpoint not set in state".to_string(),
                resource_id: Some(resource_id.to_string()),
            })
        })
    }

    /// Returns the AIServices account name, or a config error if unset.
    fn require_account_name(&self, resource_id: &str) -> Result<&str> {
        self.account_name.as_deref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Account name not set in state".to_string(),
                resource_id: Some(resource_id.to_string()),
            })
        })
    }

    /// Returns the resource group name, or a config error if unset.
    fn require_resource_group(&self, resource_id: &str) -> Result<&str> {
        self.resource_group.as_deref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Resource group not set in state".to_string(),
                resource_id: Some(resource_id.to_string()),
            })
        })
    }

    /// Deploys the tuned model under `deployment_name`, reusing the same
    /// control-plane `create_deployment` path as the base catalog so the OpenAI
    /// chat endpoint accepts the deployment name as a `model`.
    async fn deploy_tuned_model(
        &self,
        ctx: &ResourceControllerContext<'_>,
        resource_id: &str,
        deployment_name: &str,
        fine_tuned_model: &str,
    ) -> Result<()> {
        let azure_config = ctx.get_azure_config()?;
        let account_name = self.require_account_name(resource_id)?.to_string();
        let resource_group_name = self.require_resource_group(resource_id)?.to_string();

        info!(
            account_name = %account_name,
            deployment = %deployment_name,
            fine_tuned_model = %fine_tuned_model,
            "Deploying tuned model"
        );

        let cognitive_client = ctx
            .service_provider
            .get_azure_cognitive_services_client(azure_config)?;

        // The tuned model's format is OpenAI and its "version" the fine-tuned
        // model id Foundry returned. Capacity mirrors the base deployments.
        let parameters = CognitiveServicesDeploymentCreateParameters {
            sku: CognitiveServicesDeploymentSku {
                name: "GlobalStandard".to_string(),
                capacity: DEFAULT_DEPLOYMENT_CAPACITY,
            },
            properties: CognitiveServicesDeploymentCreateProperties {
                model: CognitiveServicesDeploymentModel {
                    format: "OpenAI".to_string(),
                    name: fine_tuned_model.to_string(),
                    version: "1".to_string(),
                },
            },
        };

        cognitive_client
            .create_deployment(
                &resource_group_name,
                &account_name,
                deployment_name,
                &parameters,
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to deploy tuned model as '{}'", deployment_name),
                resource_id: Some(resource_id.to_string()),
            })?;

        Ok(())
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
            tuning_job_id: None,
            tuned_deployment_name: None,
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
            tuning_job_id: None,
            tuned_deployment_name: None,
            _internal_stay_count: None,
        }
    }

    /// Creates a controller poised to submit a fine-tuning job (account and base
    /// deployments already provisioned), for testing the tuning states.
    #[cfg(feature = "test-utils")]
    pub fn mock_submitting_tuning(account_name: &str, endpoint: &str) -> Self {
        Self {
            state: AzureAiState::SubmittingTuningJob,
            account_name: Some(account_name.to_string()),
            endpoint: Some(endpoint.to_string()),
            resource_group: Some("mock-rg".to_string()),
            location: Some("eastus".to_string()),
            tuning_job_id: None,
            tuned_deployment_name: None,
            _internal_stay_count: None,
        }
    }
}

/// Extracts the finetune spec from an `Ai` config, erroring if absent. Called
/// only from tuning states, which are unreachable without a spec.
fn require_finetune_spec(config: &Ai) -> Result<&FinetuneSpec> {
    config.finetune.as_ref().ok_or_else(|| {
        AlienError::new(ErrorData::ResourceConfigInvalid {
            message: "Reached a fine-tuning state without a finetune spec".to_string(),
            resource_id: Some(config.id.clone()),
        })
    })
}

/// Builds the Blob URL Foundry imports the training dataset from.
///
/// Derived deterministically to match the Azure Storage controller's container
/// naming (`{prefix}-{name}` lowercased with `_`->`-`) under the shared default
/// storage account, so we don't need a public storage state type. Kept as a
/// pure function so it is unit-testable.
fn training_blob_url(
    storage_account: &str,
    resource_prefix: &str,
    training_data: &str,
    training_key: &str,
) -> String {
    let container = format!("{}-{}", resource_prefix, training_data)
        .to_lowercase()
        .replace('_', "-");
    format!(
        "https://{}.blob.core.windows.net/{}/{}",
        storage_account, container, training_key
    )
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
    use alien_azure_clients::azure::openai_finetuning::{FineTuningJob, MockFoundryFineTuningApi};
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

    /// A fine-tuning job in the given status, carrying `fine_tuned_model` only
    /// when succeeded.
    fn job(status: &str, fine_tuned_model: Option<&str>) -> FineTuningJob {
        FineTuningJob {
            id: "ftjob-abc".to_string(),
            status: status.to_string(),
            fine_tuned_model: fine_tuned_model.map(|s| s.to_string()),
        }
    }

    /// A cognitive-services mock that succeeds base + tuned deployments (create +
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

    #[test]
    fn training_blob_url_matches_storage_container_naming() {
        // Must match the Azure Storage controller's container naming so Foundry
        // imports from the container the storage controller actually created.
        let url = training_blob_url("myacct", "test", "training_set", "training.jsonl");
        assert_eq!(
            url,
            "https://myacct.blob.core.windows.net/test-training-set/training.jsonl",
            "underscores must become hyphens and the prefix must be applied, lowercased"
        );
    }

    #[tokio::test]
    async fn test_finetune_submit_poll_succeeds_and_binds_tuned_model() {
        // Full flow from the base deployments (mock_deploying) through submit ->
        // pending -> succeeded -> deploy tuned -> Ready, then assert the tuned
        // binding. Drives the real WaitingForDeployments -> SubmittingTuningJob
        // branch (config.finetune is Some).
        // One shared finetuning mock (cloned per getter call) so its poll
        // sequence persists across steps: first `running`, then `succeeded`.
        let mut finetuning_mock = MockFoundryFineTuningApi::new();
        finetuning_mock
            .expect_create_fine_tuning_job()
            .returning(|_, _, _| Ok(job("pending", None)));
        let polls = std::sync::Mutex::new(
            vec![
                job("running", None),
                job("succeeded", Some("gpt-4o-mini.ft-abc")),
            ]
            .into_iter(),
        );
        finetuning_mock
            .expect_get_fine_tuning_job()
            .returning(move |_, _| {
                Ok(polls
                    .lock()
                    .unwrap()
                    .next()
                    .unwrap_or_else(|| job("succeeded", Some("gpt-4o-mini.ft-abc"))))
            });
        let finetuning_mock = Arc::new(finetuning_mock);

        let mut mock_provider = MockPlatformServiceProvider::new();
        mock_provider
            .expect_get_azure_cognitive_services_client()
            .returning(|_| Ok(Arc::new(mock_cognitive_all_succeed())));
        mock_provider
            .expect_get_azure_foundry_finetuning_client()
            .returning(move |_| Ok(finetuning_mock.clone()));

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
        for _ in 0..12 {
            if executor.status() == ResourceStatus::Running {
                break;
            }
            executor.step().await.expect("step should not error");
        }
        assert_eq!(
            executor.status(),
            ResourceStatus::Running,
            "finetune resource must reach Ready after the job completes and the tuned model deploys"
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
            tuned.upstream_id, "my-ai-tuned",
            "upstream id is the tuned deployment name, which equals served_id"
        );
    }

    #[tokio::test]
    async fn test_finetune_job_failed_fails_fast_to_create_failed() {
        // A terminal Failed job status must route to CreateFailed, not poll forever.
        let mut mock_provider = MockPlatformServiceProvider::new();
        mock_provider
            .expect_get_azure_cognitive_services_client()
            .returning(|_| Ok(Arc::new(mock_cognitive_all_succeed())));
        mock_provider
            .expect_get_azure_foundry_finetuning_client()
            .returning(|_| {
                let mut mock = MockFoundryFineTuningApi::new();
                mock.expect_create_fine_tuning_job()
                    .returning(|_, _, _| Ok(job("pending", None)));
                mock.expect_get_fine_tuning_job()
                    .returning(|_, _| Ok(job("failed", None)));
                Ok(Arc::new(mock))
            });

        let mut executor = SingleControllerExecutor::builder()
            .resource(tuned_ai())
            .controller(AzureAiController::mock_submitting_tuning(
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

        // A terminal Failed job must surface as a handler error (which the
        // executor's on_failure routing turns into CreateFailed), not an endless
        // poll. Bounded so a poll-forever regression fails instead of hanging.
        let mut surfaced_error = false;
        for _ in 0..8 {
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
        // Regression: an inference-only resource must never call the fine-tuning
        // client and must emit an untuned binding.
        let mut mock_provider = MockPlatformServiceProvider::new();
        mock_provider
            .expect_get_azure_cognitive_services_client()
            .returning(|_| Ok(Arc::new(mock_cognitive_all_succeed())));
        mock_provider
            .expect_get_azure_foundry_finetuning_client()
            .never();

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
    }
}
