use crate::error::{ErrorData, Result};
use alien_aws_clients::ssm::{GetParameterRequest, PutParameterRequest, SsmApi, SsmClient};
use alien_error::Context;
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

    /// Listing is withheld: SSM parameter names are flat (`{vault_prefix}-{name}`),
    /// so a `BeginsWith` filter on `"{vault_prefix}-"` also matches every sibling
    /// vault whose prefix extends this one (vault `"app"` would list vault
    /// `"app-prod"`'s parameters too). There is no separator reserved between
    /// the prefix and the secret name, so this is unfixable without a naming
    /// scheme change. Returns `OperationNotSupported` until the naming scheme
    /// gains unambiguous separation between `vault_prefix` and `secret_name`.
    async fn list_secrets(&self) -> Result<Vec<String>> {
        Err(alien_error::AlienError::new(
            ErrorData::OperationNotSupported {
                operation: "vault.list_secrets".to_string(),
                reason: format!(
                    "AWS Parameter Store names are flat ('{{vault_prefix}}-{{name}}'); a \
                     BeginsWith filter on vault prefix '{}' would also match sibling vaults \
                     whose prefix extends this one (e.g. '{}-prod'). Listing will return once \
                     the naming scheme has unambiguous separation between vault prefix and \
                     secret name.",
                    self.vault_prefix, self.vault_prefix
                ),
            },
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::Vault as _;
    use alien_aws_clients::AwsCredentialProvider;
    use alien_core::{AwsClientConfig, AwsCredentials};

    async fn test_vault() -> AwsParameterStoreVault {
        let config = AwsClientConfig {
            account_id: "000000000000".to_string(),
            region: "us-east-1".to_string(),
            credentials: AwsCredentials::AccessKeys {
                access_key_id: "test".to_string(),
                secret_access_key: "test".to_string(),
                session_token: None,
            },
            service_overrides: None,
        };
        let credentials = AwsCredentialProvider::from_config(config)
            .await
            .expect("static access-key credentials never fail to construct");
        let client = Arc::new(SsmClient::new(reqwest::Client::new(), credentials));
        AwsParameterStoreVault::new(client, "app".to_string())
    }

    /// Listing is withheld rather than implemented with a `BeginsWith` scan,
    /// because a flat `{vault_prefix}-{name}` namespace lets sibling vault
    /// prefixes alias one another (vault "app" would also match "app-prod").
    /// This must not silently start scanning again; assert the typed error.
    #[tokio::test]
    async fn list_secrets_reports_not_supported() {
        let vault = test_vault().await;

        let error = vault
            .list_secrets()
            .await
            .expect_err("list_secrets must stay withheld until namespace isolation lands");

        assert_eq!(error.code, "OPERATION_NOT_SUPPORTED");
        assert!(
            error.to_string().contains("app"),
            "message should name the vault prefix so operators can diagnose which vault hit \
             this, got: {error}"
        );
    }
}
