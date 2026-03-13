use crate::error::{ErrorData, Result};
use crate::traits::{
    AwsServiceAccountInfo, Binding, ImpersonationRequest, ServiceAccount, ServiceAccountInfo,
};
use alien_aws_clients::{
    sts::{AssumeRoleRequest, StsApi, StsClient},
    AwsClientConfig,
};
use alien_core::bindings::AwsServiceAccountBinding;
use alien_core::{AwsClientConfig as CoreAwsClientConfig, AwsCredentials, ClientConfig};
use alien_error::Context;
use async_trait::async_trait;
use reqwest::Client;

/// AWS IAM Role service account binding implementation
#[derive(Debug)]
pub struct AwsIamServiceAccount {
    client: StsClient,
    config: AwsClientConfig,
    binding: AwsServiceAccountBinding,
}

impl AwsIamServiceAccount {
    pub fn new(
        http_client: Client,
        config: AwsClientConfig,
        binding: AwsServiceAccountBinding,
    ) -> Self {
        let sts_client = StsClient::new(http_client, config.clone());
        Self {
            client: sts_client,
            config,
            binding,
        }
    }

    /// Get the role ARN from the binding, resolving template expressions if needed
    fn get_role_arn(&self) -> Result<String> {
        self.binding
            .role_arn
            .clone()
            .into_value("service-account", "role_arn")
            .context(ErrorData::BindingConfigInvalid {
                binding_name: "service-account".to_string(),
                reason: "Failed to resolve role_arn from binding".to_string(),
            })
    }

    /// Get the role name from the binding, resolving template expressions if needed
    fn get_role_name(&self) -> Result<String> {
        self.binding
            .role_name
            .clone()
            .into_value("service-account", "role_name")
            .context(ErrorData::BindingConfigInvalid {
                binding_name: "service-account".to_string(),
                reason: "Failed to resolve role_name from binding".to_string(),
            })
    }
}

impl Binding for AwsIamServiceAccount {}

#[async_trait]
impl ServiceAccount for AwsIamServiceAccount {
    async fn get_info(&self) -> Result<ServiceAccountInfo> {
        let role_name = self.get_role_name()?;
        let role_arn = self.get_role_arn()?;

        Ok(ServiceAccountInfo::Aws(AwsServiceAccountInfo {
            role_name,
            role_arn,
        }))
    }

    async fn impersonate(&self, request: ImpersonationRequest) -> Result<ClientConfig> {
        let role_arn = self.get_role_arn()?;
        let session_name = request
            .session_name
            .unwrap_or_else(|| "alien-impersonation".to_string());
        let duration = request.duration_seconds.unwrap_or(3600);

        let assume_role_request = AssumeRoleRequest::builder()
            .role_arn(role_arn.clone())
            .role_session_name(session_name)
            .duration_seconds(duration)
            .build();

        let response =
            self.client
                .assume_role(assume_role_request)
                .await
                .context(ErrorData::Other {
                    message: format!("Failed to assume IAM role '{}'", role_arn),
                })?;

        let credentials = response.assume_role_result.credentials;

        // Create new AWS client config with the temporary credentials
        let impersonated_config = CoreAwsClientConfig {
            account_id: self.config.account_id.clone(),
            region: self.config.region.clone(),
            credentials: AwsCredentials::AccessKeys {
                access_key_id: credentials.access_key_id,
                secret_access_key: credentials.secret_access_key,
                session_token: Some(credentials.session_token),
            },
            service_overrides: self.config.service_overrides.clone(),
        };

        Ok(ClientConfig::Aws(Box::new(impersonated_config)))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
