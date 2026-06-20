use crate::error::{ErrorData, Result};
use crate::traits::{
    AwsServiceAccountInfo, Binding, ImpersonationRequest, ServiceAccount, ServiceAccountInfo,
};
use alien_core::bindings::AwsServiceAccountBinding;
use alien_core::{AwsClientConfig, AwsCredentials, ClientConfig};
use alien_error::{AlienError, Context, IntoAlienError};
use async_trait::async_trait;
use aws_sdk_sts::primitives::DateTimeFormat;
use std::{fmt::Debug, sync::Arc};

/// Temporary credentials returned by STS AssumeRole.
#[derive(Debug, Clone)]
pub struct AssumedRoleCredentials {
    /// AWS Access Key ID.
    pub access_key_id: String,
    /// AWS Secret Access Key.
    pub secret_access_key: String,
    /// AWS Session Token.
    pub session_token: String,
    /// Credential expiration as an RFC3339 timestamp.
    pub expires_at: String,
}

/// Minimal STS operation required by the AWS service-account binding.
#[async_trait]
pub trait AwsStsClient: Debug + Send + Sync {
    /// Assume an IAM role.
    async fn assume_role(
        &self,
        role_arn: &str,
        session_name: &str,
        duration_seconds: i32,
    ) -> Result<AssumedRoleCredentials>;
}

#[async_trait]
impl AwsStsClient for aws_sdk_sts::Client {
    async fn assume_role(
        &self,
        role_arn: &str,
        session_name: &str,
        duration_seconds: i32,
    ) -> Result<AssumedRoleCredentials> {
        let response = self
            .assume_role()
            .role_arn(role_arn)
            .role_session_name(session_name)
            .duration_seconds(duration_seconds)
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::Other {
                message: format!("Failed to assume IAM role '{}'", role_arn),
            })?;

        let credentials = response.credentials().ok_or_else(|| {
            AlienError::new(ErrorData::Other {
                message: format!("AssumeRole for '{}' returned no credentials", role_arn),
            })
        })?;

        let expires_at = credentials
            .expiration()
            .fmt(DateTimeFormat::DateTime)
            .into_alien_error()
            .context(ErrorData::Other {
                message: format!(
                    "Failed to format AssumeRole credential expiration for '{}'",
                    role_arn
                ),
            })?;

        Ok(AssumedRoleCredentials {
            access_key_id: credentials.access_key_id().to_string(),
            secret_access_key: credentials.secret_access_key().to_string(),
            session_token: credentials.session_token().to_string(),
            expires_at,
        })
    }
}

/// AWS IAM Role service account binding implementation.
#[derive(Debug)]
pub struct AwsIamServiceAccount {
    client: Arc<dyn AwsStsClient>,
    config: AwsClientConfig,
    binding: AwsServiceAccountBinding,
}

impl AwsIamServiceAccount {
    pub fn new(
        client: Arc<dyn AwsStsClient>,
        config: AwsClientConfig,
        binding: AwsServiceAccountBinding,
    ) -> Self {
        Self {
            client,
            config,
            binding,
        }
    }

    /// Get the role ARN from the binding, resolving template expressions if needed.
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

    /// Get the role name from the binding, resolving template expressions if needed.
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

        let credentials = self
            .client
            .assume_role(&role_arn, &session_name, duration)
            .await?;

        let impersonated_config = AwsClientConfig {
            account_id: self.config.account_id.clone(),
            region: self.config.region.clone(),
            credentials: AwsCredentials::SessionCredentials {
                access_key_id: credentials.access_key_id,
                secret_access_key: credentials.secret_access_key,
                session_token: credentials.session_token,
                expires_at: credentials.expires_at,
            },
            service_overrides: self.config.service_overrides.clone(),
        };

        Ok(ClientConfig::Aws(Box::new(impersonated_config)))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
