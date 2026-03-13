use crate::error::{ErrorData, Result};
use alien_aws_clients::ssm::{GetParameterRequest, PutParameterRequest, SsmApi, SsmClient};
use alien_error::{Context, ContextError};
use async_trait::async_trait;
use std::sync::Arc;

/// AWS SSM Parameter Store vault binding implementation.
#[derive(Debug)]
pub struct AwsParameterStoreVault {
    client: Arc<SsmClient>,
    vault_prefix: String,
}

impl AwsParameterStoreVault {
    /// Create a new AWS SSM Parameter Store vault binding.
    pub fn new(client: Arc<SsmClient>, vault_prefix: String) -> Self {
        Self {
            client,
            vault_prefix,
        }
    }

    /// Generate the full parameter name with vault prefix.
    fn full_parameter_name(&self, secret_name: &str) -> String {
        format!("{}-{}", self.vault_prefix, secret_name)
    }
}

#[async_trait]
impl crate::traits::Binding for AwsParameterStoreVault {}

#[async_trait]
impl crate::traits::Vault for AwsParameterStoreVault {
    /// Get a secret value by name.
    async fn get_secret(&self, secret_name: &str) -> Result<String> {
        let full_name = self.full_parameter_name(secret_name);

        let request = GetParameterRequest::builder()
            .name(full_name.clone())
            .with_decryption(true)
            .build();

        let response =
            self.client
                .get_parameter(request)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!("Failed to get parameter '{}'", full_name),
                    resource_id: None,
                })?;

        let parameter = response.parameter.ok_or_else(|| {
            alien_error::AlienError::new(ErrorData::CloudPlatformError {
                message: format!("Parameter '{}' missing in response", full_name),
                resource_id: None,
            })
        })?;

        parameter.value.ok_or_else(|| {
            alien_error::AlienError::new(ErrorData::CloudPlatformError {
                message: format!("Parameter '{}' has no value", full_name),
                resource_id: None,
            })
        })
    }

    /// Set a secret value using SecureString parameters.
    async fn set_secret(&self, secret_name: &str, value: &str) -> Result<()> {
        let full_name = self.full_parameter_name(secret_name);

        let request = PutParameterRequest::builder()
            .name(full_name.clone())
            .value(value.to_string())
            .parameter_type("SecureString".to_string())
            .description(format!(
                "Secret managed by Alien vault {}",
                self.vault_prefix
            ))
            .overwrite(true)
            .build();

        self.client
            .put_parameter(request)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to put parameter '{}'", full_name),
                resource_id: None,
            })?;

        Ok(())
    }

    /// Delete a secret.
    async fn delete_secret(&self, secret_name: &str) -> Result<()> {
        let full_name = self.full_parameter_name(secret_name);

        self.client
            .delete_parameter(&full_name)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to delete parameter '{}'", full_name),
                resource_id: None,
            })?;

        Ok(())
    }
}
