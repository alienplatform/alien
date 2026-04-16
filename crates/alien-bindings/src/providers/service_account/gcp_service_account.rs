use crate::error::{ErrorData, Result};
use crate::traits::{
    Binding, GcpServiceAccountInfo, ImpersonationRequest, ServiceAccount, ServiceAccountInfo,
};
use alien_core::bindings::GcpServiceAccountBinding;
use alien_core::{ClientConfig, GcpClientConfig as CoreGcpClientConfig, GcpCredentials};
use alien_error::Context;
use alien_gcp_clients::{
    iam::{GenerateAccessTokenRequest, IamApi, IamClient},
    GcpClientConfig,
};
use async_trait::async_trait;
use reqwest::Client;

/// GCP Service Account binding implementation
#[derive(Debug)]
pub struct GcpServiceAccount {
    client: IamClient,
    config: GcpClientConfig,
    binding: GcpServiceAccountBinding,
}

impl GcpServiceAccount {
    pub fn new(
        http_client: Client,
        config: GcpClientConfig,
        binding: GcpServiceAccountBinding,
    ) -> Self {
        let iam_client = IamClient::new(http_client, config.clone());
        Self {
            client: iam_client,
            config,
            binding,
        }
    }

    /// Get the service account email from the binding, resolving template expressions if needed
    fn get_email(&self) -> Result<String> {
        self.binding
            .email
            .clone()
            .into_value("service-account", "email")
            .context(ErrorData::BindingConfigInvalid {
                binding_name: "service-account".to_string(),
                reason: "Failed to resolve email from binding".to_string(),
            })
    }

    /// Get the unique ID from the binding, resolving template expressions if needed
    fn get_unique_id(&self) -> Result<String> {
        self.binding
            .unique_id
            .clone()
            .into_value("service-account", "unique_id")
            .context(ErrorData::BindingConfigInvalid {
                binding_name: "service-account".to_string(),
                reason: "Failed to resolve unique_id from binding".to_string(),
            })
    }
}

impl Binding for GcpServiceAccount {}

#[async_trait]
impl ServiceAccount for GcpServiceAccount {
    async fn get_info(&self) -> Result<ServiceAccountInfo> {
        let email = self.get_email()?;
        let unique_id = self.get_unique_id()?;

        Ok(ServiceAccountInfo::Gcp(GcpServiceAccountInfo {
            email,
            unique_id,
        }))
    }

    async fn impersonate(&self, request: ImpersonationRequest) -> Result<ClientConfig> {
        let email = self.get_email()?;
        let scopes = request
            .scopes
            .unwrap_or_else(|| vec!["https://www.googleapis.com/auth/cloud-platform".to_string()]);

        let token_request = GenerateAccessTokenRequest::builder().scope(scopes).build();

        let response = self
            .client
            .generate_access_token(email.clone(), token_request)
            .await
            .context(ErrorData::Other {
                message: format!(
                    "Failed to generate access token for service account '{}'",
                    email
                ),
            })?;

        // Create new GCP client config with the impersonated access token
        let impersonated_config = CoreGcpClientConfig {
            project_id: self.config.project_id.clone(),
            region: self.config.region.clone(),
            credentials: GcpCredentials::AccessToken {
                token: response.access_token,
            },
            service_overrides: self.config.service_overrides.clone(),
            project_number: self.config.project_number.clone(),
        };

        Ok(ClientConfig::Gcp(Box::new(impersonated_config)))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
