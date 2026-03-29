use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;

use alien_commands::{
    error::{ErrorData as ArcErrorData, Result as ArcResult},
    server::{
        dispatchers::{
            LambdaCommandDispatcher, PubSubCommandDispatcher, ServiceBusCommandDispatcher,
        },
        CommandDispatcher,
    },
    Envelope,
};

use alien_commands::test_utils::MockDispatcher;
use alien_core::{ClientConfig, Platform};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_infra::RemoteAccessResolver;
use async_trait::async_trait;
use reqwest::{
    header::{HeaderMap, HeaderValue, AUTHORIZATION, USER_AGENT},
    Client as HttpClient,
};
use tracing::{debug, info};

use super::credential_resolver::impersonate_management_service_account;
use super::error::ErrorData as PlatformErrorData;

/// Platform Manager command dispatcher for push-mode deployments.
#[derive(Debug)]
pub struct ManagedCommandDispatcher {
    http_client: HttpClient,
    api_client: alien_platform_api::Client,
    bindings_provider: Arc<dyn alien_bindings::BindingsProviderApi>,
    target_providers: HashMap<Platform, Arc<dyn alien_bindings::BindingsProviderApi>>,
}

impl ManagedCommandDispatcher {
    pub fn new(
        api_base_url: &str,
        api_key: &str,
        bindings_provider: Arc<dyn alien_bindings::BindingsProviderApi>,
        target_providers: HashMap<Platform, Arc<dyn alien_bindings::BindingsProviderApi>>,
    ) -> super::error::Result<Self> {
        let http_client = HttpClient::new();

        let auth_value = format!("Bearer {}", api_key);
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&auth_value)
                .into_alien_error()
                .context(PlatformErrorData::ConfigurationError {
                    message: "Invalid API key format for authorization header".to_string(),
                })?,
        );
        headers.insert(USER_AGENT, HeaderValue::from_static("alien-manager"));

        let reqwest_client = HttpClient::builder()
            .default_headers(headers)
            .build()
            .into_alien_error()
            .context(PlatformErrorData::ConfigurationError {
                message: "Failed to build HTTP client".to_string(),
            })?;

        let api_client = alien_platform_api::Client::new_with_client(api_base_url, reqwest_client);

        Ok(Self {
            http_client,
            api_client,
            bindings_provider,
            target_providers,
        })
    }

    fn provider_for_target(
        &self,
        platform: Platform,
    ) -> &Arc<dyn alien_bindings::BindingsProviderApi> {
        self.target_providers
            .get(&platform)
            .unwrap_or(&self.bindings_provider)
    }

    async fn get_deployment(
        &self,
        deployment_id: &str,
    ) -> ArcResult<alien_platform_api::types::DeploymentDetailResponse> {
        debug!(deployment_id = %deployment_id, "Getting deployment from platform API");

        let response = self
            .api_client
            .get_deployment()
            .id(deployment_id)
            .send()
            .await
            .map_err(|e| {
                AlienError::new(ArcErrorData::HttpOperationFailed {
                    message: format!(
                        "Failed to retrieve deployment {} from platform API: {}",
                        deployment_id, e
                    ),
                    method: Some("GET".to_string()),
                    url: None,
                })
            })?;

        Ok(response.into_inner())
    }

    async fn create_dispatcher_for_deployment(
        &self,
        deployment: &alien_platform_api::types::DeploymentDetailResponse,
    ) -> ArcResult<Arc<dyn CommandDispatcher>> {
        let platform = match deployment.platform {
            alien_platform_api::types::DeploymentDetailResponsePlatform::Aws => Platform::Aws,
            alien_platform_api::types::DeploymentDetailResponsePlatform::Gcp => Platform::Gcp,
            alien_platform_api::types::DeploymentDetailResponsePlatform::Azure => Platform::Azure,
            alien_platform_api::types::DeploymentDetailResponsePlatform::Test => Platform::Test,
            alien_platform_api::types::DeploymentDetailResponsePlatform::Kubernetes => {
                return Err(AlienError::new(ArcErrorData::OperationNotSupported {
                    message: format!(
                        "Deployment {} is on Kubernetes platform which does not support push dispatch",
                        deployment.id.as_str()
                    ),
                    operation: Some("dispatch".to_string()),
                }));
            }
            alien_platform_api::types::DeploymentDetailResponsePlatform::Local => {
                return Err(AlienError::new(ArcErrorData::OperationNotSupported {
                    message: format!(
                        "Deployment {} is on Local platform which does not support push dispatch",
                        deployment.id.as_str()
                    ),
                    operation: Some("dispatch".to_string()),
                }));
            }
        };

        let uses_push = matches!(
            deployment.stack_settings.deployment_model,
            Some(alien_platform_api::types::DeploymentDetailResponseStackSettingsDeploymentModel::Push)
        );

        if !uses_push {
            return Err(AlienError::new(ArcErrorData::OperationNotSupported {
                message: format!(
                    "Deployment {} is configured for pull mode but push dispatch was attempted",
                    deployment.id.as_str()
                ),
                operation: Some("dispatch".to_string()),
            }));
        }

        let sdk_stack_state = &deployment.stack_state;
        let stack_state: alien_core::StackState =
            serde_json::from_value(serde_json::to_value(sdk_stack_state).map_err(|e| {
                AlienError::new(ArcErrorData::Other {
                    message: format!("Failed to serialize deployment stack state: {}", e),
                })
            })?)
            .map_err(|e| {
                AlienError::new(ArcErrorData::Other {
                    message: format!(
                        "Failed to convert deployment stack state to core StackState: {}",
                        e
                    ),
                })
            })?;

        let provider = self.provider_for_target(platform);
        let base_config = impersonate_management_service_account(&**provider, platform)
            .await
            .map_err(|e| {
                AlienError::new(ArcErrorData::Other {
                    message: format!("Failed to impersonate management SA: {}", e),
                })
            })?;

        let resolver = RemoteAccessResolver::new(std::env::vars().collect());
        let client_config = resolver
            .resolve(base_config, &stack_state)
            .await
            .map_err(|e| {
                AlienError::new(ArcErrorData::Other {
                    message: format!("Failed to resolve remote access from stack state: {}", e),
                })
            })?;

        match client_config {
            ClientConfig::Aws(aws_config) => {
                info!(
                    deployment_id = deployment.id.as_str(),
                    "Creating AWS Lambda dispatcher for push deployment"
                );
                let dispatcher = LambdaCommandDispatcher::new(
                    self.http_client.clone(),
                    *aws_config,
                )
                .await
                .map_err(|e| {
                    AlienError::new(ArcErrorData::Other {
                        message: format!("Failed to create Lambda dispatcher: {}", e),
                    })
                })?;
                Ok(Arc::new(dispatcher) as Arc<dyn CommandDispatcher>)
            }
            ClientConfig::Gcp(gcp_config) => {
                info!(
                    deployment_id = deployment.id.as_str(),
                    "Creating GCP Pub/Sub dispatcher for push deployment"
                );
                Ok(Arc::new(PubSubCommandDispatcher::new(
                    self.http_client.clone(),
                    *gcp_config,
                )) as Arc<dyn CommandDispatcher>)
            }
            ClientConfig::Azure(azure_config) => {
                info!(
                    deployment_id = deployment.id.as_str(),
                    "Creating Azure Service Bus dispatcher for push deployment"
                );
                Ok(Arc::new(ServiceBusCommandDispatcher::new(
                    self.http_client.clone(),
                    *azure_config,
                )) as Arc<dyn CommandDispatcher>)
            }
            ClientConfig::Kubernetes(_) | ClientConfig::Local { .. } => {
                Err(AlienError::new(ArcErrorData::OperationNotSupported {
                    message: format!(
                        "Deployment {} has Kubernetes/Local config which does not support push dispatch",
                        deployment.id.as_str()
                    ),
                    operation: Some("dispatch".to_string()),
                }))
            }
            ClientConfig::Test => {
                info!(
                    deployment_id = deployment.id.as_str(),
                    "Creating Mock dispatcher for test push deployment"
                );
                Ok(Arc::new(MockDispatcher::new()) as Arc<dyn CommandDispatcher>)
            }
        }
    }
}

#[async_trait]
impl CommandDispatcher for ManagedCommandDispatcher {
    async fn dispatch(&self, envelope: &Envelope) -> ArcResult<()> {
        let deployment_id = &envelope.deployment_id;

        debug!(
            command_id = %envelope.command_id,
            deployment_id = %deployment_id,
            command = %envelope.command,
            "ManagedCommandDispatcher: dispatching command"
        );

        let deployment = self.get_deployment(deployment_id).await?;
        let dispatcher = self.create_dispatcher_for_deployment(&deployment).await?;

        info!(
            command_id = %envelope.command_id,
            deployment_id = %deployment_id,
            platform = ?deployment.platform,
            "Dispatching command via platform-specific dispatcher"
        );

        dispatcher.dispatch(envelope).await
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
