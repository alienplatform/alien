use crate::error::{ErrorData, Result};
use alien_error::{AlienError, Context, IntoAlienError};
use async_trait::async_trait;
use std::{fmt::Debug, sync::Arc};

/// Minimal SSM operations required by the Parameter Store vault binding.
#[async_trait]
pub trait SsmParameterStore: Debug + Send + Sync {
    /// Get a decrypted parameter value.
    async fn get_parameter_value(&self, name: &str) -> Result<String>;

    /// Create or update a SecureString parameter.
    async fn put_secure_parameter(&self, name: &str, value: &str, description: &str) -> Result<()>;

    /// Delete a parameter.
    async fn delete_parameter(&self, name: &str) -> Result<()>;
}

#[async_trait]
impl SsmParameterStore for aws_sdk_ssm::Client {
    async fn get_parameter_value(&self, name: &str) -> Result<String> {
        let response = self
            .get_parameter()
            .name(name)
            .with_decryption(true)
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to get parameter '{}'", name),
                resource_id: None,
            })?;

        response
            .parameter()
            .and_then(|parameter| parameter.value())
            .map(ToString::to_string)
            .ok_or_else(|| {
                AlienError::new(ErrorData::CloudPlatformError {
                    message: format!("Parameter '{}' has no value", name),
                    resource_id: None,
                })
            })
    }

    async fn put_secure_parameter(&self, name: &str, value: &str, description: &str) -> Result<()> {
        self.put_parameter()
            .name(name)
            .value(value)
            .r#type(aws_sdk_ssm::types::ParameterType::SecureString)
            .description(description)
            .overwrite(true)
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to put parameter '{}'", name),
                resource_id: None,
            })?;

        Ok(())
    }

    async fn delete_parameter(&self, name: &str) -> Result<()> {
        self.delete_parameter()
            .name(name)
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to delete parameter '{}'", name),
                resource_id: None,
            })?;

        Ok(())
    }
}

/// AWS SSM Parameter Store vault binding implementation.
#[derive(Debug)]
pub struct AwsParameterStoreVault {
    client: Arc<dyn SsmParameterStore>,
    vault_prefix: String,
}

impl AwsParameterStoreVault {
    /// Create a new AWS SSM Parameter Store vault binding.
    pub fn new(client: Arc<dyn SsmParameterStore>, vault_prefix: String) -> Self {
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

        self.client.get_parameter_value(&full_name).await
    }

    /// Set a secret value using SecureString parameters.
    async fn set_secret(&self, secret_name: &str, value: &str) -> Result<()> {
        let full_name = self.full_parameter_name(secret_name);

        let description = format!("Secret managed by Alien vault {}", self.vault_prefix);
        self.client
            .put_secure_parameter(&full_name, value, &description)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to set secret '{}'", secret_name),
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
                message: format!("Failed to delete secret '{}'", secret_name),
                resource_id: None,
            })?;

        Ok(())
    }
}
