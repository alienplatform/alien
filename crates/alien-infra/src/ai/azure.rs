use tracing::info;

use crate::azure_utils::get_resource_group_name;
use crate::core::{ResourceControllerContext, ResourcePermissionsHelper};
use crate::error::{ErrorData, Result};
use alien_azure_clients::azure::cognitive_services::{
    CognitiveServicesAccountCreateParameters, CognitiveServicesAccountCreateProperties,
    CognitiveServicesDeploymentCreateParameters, CognitiveServicesDeploymentCreateProperties,
    CognitiveServicesDeploymentModel, CognitiveServicesDeploymentSku, CognitiveServicesSku,
};
use alien_azure_clients::long_running_operation::OperationResult;
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
        CognitiveServicesDeployment, CognitiveServicesDeploymentProperties,
        MockCognitiveServicesAccountsApi,
    };
    use alien_azure_clients::long_running_operation::OperationResult;
    use alien_client_core::ErrorData as CloudClientErrorData;
    use alien_core::{Ai, AiOutputs, Platform, ResourceStatus};
    use alien_error::AlienError;
    use std::sync::Arc;

    fn basic_ai() -> Ai {
        Ai::new("my-ai".to_string()).build()
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
}
