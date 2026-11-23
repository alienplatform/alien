use crate::error::{ErrorData, Result};
use crate::traits::SecretPresence;
use alien_aws_clients::ssm::{
    DescribeParametersRequest, GetParameterRequest, ParameterStringFilter, PutParameterRequest,
    SsmApi, SsmClient, SsmTag,
};
use alien_error::{AlienError, Context, ContextError};
use async_trait::async_trait;
use std::sync::Arc;

/// AWS SSM Parameter Store vault binding implementation.
#[derive(Debug)]
pub struct AwsParameterStoreVault {
    client: Arc<SsmClient>,
    vault_prefix: String,
    vault_id: String,
}

impl AwsParameterStoreVault {
    /// Create a new AWS SSM Parameter Store vault binding. `vault_id` is the id of the vault
    /// resource; it marks the values this binding creates.
    pub fn new(client: Arc<SsmClient>, vault_prefix: String, vault_id: String) -> Self {
        Self {
            client,
            vault_prefix,
            vault_id,
        }
    }

    /// Adds the marker tags to a value this call just created (SSM numbers it version 1).
    /// A failure only costs the marker, so it is logged and the write still succeeds: not every
    /// identity that writes values may add tags.
    async fn mark_if_new(&self, full_name: &str, version: Option<i64>) {
        if version != Some(1) {
            return;
        }
        if let Err(error) = self
            .client
            .add_parameter_tags(full_name, self.marker_tags())
            .await
        {
            tracing::warn!(
                parameter = full_name,
                error = %error,
                "could not mark a new vault value; it will appear as unmarked to the erase step"
            );
        }
    }

    /// Tags that mark a value as created by this vault.
    fn marker_tags(&self) -> Vec<SsmTag> {
        use alien_core::vault_naming::{
            vault_marker_value, VAULT_ID_MARKER_KEY, VAULT_PREFIX_MARKER_KEY,
        };
        vec![
            SsmTag::builder()
                .key(VAULT_PREFIX_MARKER_KEY.to_string())
                .value(vault_marker_value(&self.vault_prefix))
                .build(),
            SsmTag::builder()
                .key(VAULT_ID_MARKER_KEY.to_string())
                .value(vault_marker_value(&self.vault_id))
                .build(),
        ]
    }

    /// Generate the full parameter name with vault prefix.
    fn full_parameter_name(&self, secret_name: &str) -> String {
        alien_core::vault_naming::parameter_store_parameter_name(&self.vault_prefix, secret_name)
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

        let response = self.client.get_parameter(request).await.map_err(|error| {
            super::secret_read_error(
                error,
                &self.vault_prefix,
                secret_name,
                format!("Failed to get parameter '{full_name}'"),
            )
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

    /// Looks the parameter up with `DescribeParameters`, which returns metadata
    /// only, and requires a `SecureString`: a plain `String` parameter would
    /// keep the secret unencrypted.
    async fn secret_presence(&self, secret_name: &str) -> Result<SecretPresence> {
        let full_name = self.full_parameter_name(secret_name);

        let request = DescribeParametersRequest::builder()
            .parameter_filters(vec![ParameterStringFilter::builder()
                .key("Name".to_string())
                .option("Equals".to_string())
                .values(vec![full_name.clone()])
                .build()])
            .build();

        let response = self.client.describe_parameters(request).await.context(
            ErrorData::CloudPlatformError {
                message: format!("Failed to describe parameter '{full_name}'"),
                resource_id: None,
            },
        )?;

        let Some(parameter) = response.parameters.unwrap_or_default().into_iter().next() else {
            return Ok(SecretPresence::Missing);
        };
        match parameter.parameter_type.as_deref() {
            Some("SecureString") => Ok(SecretPresence::Present {
                version: parameter.version.map(|version| version.to_string()),
            }),
            other => Ok(SecretPresence::Invalid {
                reason: format!(
                    "parameter '{full_name}' is a {} parameter; it must be a SecureString",
                    other.unwrap_or("untyped")
                ),
            }),
        }
    }

    /// Set a secret value using SecureString parameters. `PutParameter` does not accept tags
    /// together with `Overwrite`, so the value is written first. SSM numbers a new parameter 1,
    /// and only then are the vault's marker tags added; an existing value keeps the tags it has.
    /// The markers are best effort: an identity without `ssm:AddTagsToResource` still writes.
    async fn set_secret(&self, secret_name: &str, value: &str) -> Result<()> {
        let full_name = self.full_parameter_name(secret_name);

        let request = PutParameterRequest::builder()
            .name(full_name.clone())
            .value(value.to_string())
            .parameter_type("SecureString".to_string())
            .description(format!(
                "Secret managed by deployment vault {}",
                self.vault_prefix
            ))
            .overwrite(true)
            .build();

        let response =
            self.client
                .put_parameter(request)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!("Failed to put parameter '{}'", full_name),
                    resource_id: None,
                })?;

        self.mark_if_new(&full_name, response.version).await;

        Ok(())
    }

    /// Create a secret only if no parameter by its name exists (`Overwrite: false`).
    async fn create_secret(&self, secret_name: &str, value: &str) -> Result<()> {
        let full_name = self.full_parameter_name(secret_name);

        let request = PutParameterRequest::builder()
            .name(full_name.clone())
            .value(value.to_string())
            .parameter_type("SecureString".to_string())
            .description(format!(
                "Secret managed by deployment vault {}",
                self.vault_prefix
            ))
            .overwrite(false)
            .build();

        match self.client.put_parameter(request).await {
            Ok(response) => {
                self.mark_if_new(&full_name, response.version).await;
                Ok(())
            }
            Err(error)
                if matches!(
                    error.error,
                    Some(alien_client_core::ErrorData::RemoteResourceConflict { .. })
                ) =>
            {
                Err(AlienError::new(ErrorData::VaultSecretAlreadyExists {
                    vault: self.vault_prefix.clone(),
                    secret_name: secret_name.to_string(),
                }))
            }
            Err(error) => Err(error).context(ErrorData::CloudPlatformError {
                message: format!("Failed to create parameter '{}'", full_name),
                resource_id: None,
            }),
        }
    }

    /// Delete a secret.
    async fn delete_secret(&self, secret_name: &str) -> Result<()> {
        let full_name = self.full_parameter_name(secret_name);

        match self.client.delete_parameter(&full_name).await {
            Ok(_) => Ok(()),
            Err(error)
                if matches!(
                    error.error,
                    Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                Ok(())
            }
            Err(error) => Err(error.context(ErrorData::CloudPlatformError {
                message: format!("Failed to delete parameter '{full_name}'"),
                resource_id: None,
            })),
        }
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
        AwsParameterStoreVault::new(client, "app".to_string(), "secrets".to_string())
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

    fn vault_for(server: &httpmock::MockServer, prefix: &str, id: &str) -> AwsParameterStoreVault {
        let config = AwsClientConfig {
            account_id: "000000000000".to_string(),
            region: "us-east-1".to_string(),
            credentials: AwsCredentials::AccessKeys {
                access_key_id: "test".to_string(),
                secret_access_key: "test".to_string(),
                session_token: None,
            },
            service_overrides: Some(alien_aws_clients::ServiceOverrides {
                endpoints: std::collections::HashMap::from([(
                    "ssm".to_string(),
                    server.base_url(),
                )]),
            }),
        };
        let client = Arc::new(SsmClient::new(
            reqwest::Client::new(),
            AwsCredentialProvider::from_config_sync(config),
        ));
        AwsParameterStoreVault::new(client, prefix.to_string(), id.to_string())
    }

    /// A value created through `set_secret` is tagged right after it is written.
    #[tokio::test]
    async fn a_value_set_for_the_first_time_is_tagged_with_its_vault_prefix_and_id() {
        let server = httpmock::MockServer::start_async().await;
        let put = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::POST)
                    .header("X-Amz-Target", "AmazonSSM.PutParameter")
                    .json_body_partial(
                        serde_json::json!({"Name": "acme-1a2b-secrets-api-key", "Overwrite": true})
                            .to_string(),
                    );
                then.status(200)
                    .json_body(serde_json::json!({"Version": 1}));
            })
            .await;
        let tag = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::POST)
                    .header("X-Amz-Target", "AmazonSSM.AddTagsToResource")
                    .json_body_partial(
                        serde_json::json!({
                            "ResourceType": "Parameter",
                            "ResourceId": "acme-1a2b-secrets-api-key",
                            "Tags": [
                                {"Key": "alien-vault-prefix", "Value": "acme-1a2b-secrets"},
                                {"Key": "alien-vault", "Value": "secrets"}
                            ]
                        })
                        .to_string(),
                    );
                then.status(200).json_body(serde_json::json!({}));
            })
            .await;
        let vault = vault_for(&server, "acme-1a2b-secrets", "secrets");

        vault.set_secret("api-key", "v").await.unwrap();

        assert_eq!(put.hits_async().await, 1);
        assert_eq!(tag.hits_async().await, 1);
    }

    /// An update (version above 1) is a single overwrite: no tag call, so the tags recorded
    /// when the value was created stay as they are.
    #[tokio::test]
    async fn updating_an_existing_value_does_not_touch_its_tags() {
        let server = httpmock::MockServer::start_async().await;
        let put = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::POST)
                    .header("X-Amz-Target", "AmazonSSM.PutParameter")
                    .json_body_partial(serde_json::json!({"Overwrite": true}).to_string());
                then.status(200)
                    .json_body(serde_json::json!({"Version": 2}));
            })
            .await;
        let other = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::POST).matches(|request| {
                    !request.headers.iter().flatten().any(|(name, value)| {
                        name.eq_ignore_ascii_case("x-amz-target")
                            && value == "AmazonSSM.PutParameter"
                    })
                });
                then.status(500);
            })
            .await;
        let vault = vault_for(&server, "acme-1a2b-secrets", "secrets");

        vault.set_secret("api-key", "v2").await.unwrap();

        assert_eq!(put.hits_async().await, 1);
        assert_eq!(other.hits_async().await, 0);
    }

    /// A refused tag call costs the marker, not the write.
    #[tokio::test]
    async fn a_value_is_written_even_when_the_identity_may_not_add_tags() {
        let server = httpmock::MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(httpmock::Method::POST)
                    .header("X-Amz-Target", "AmazonSSM.PutParameter");
                then.status(200)
                    .json_body(serde_json::json!({"Version": 1}));
            })
            .await;
        let denied = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::POST)
                    .header("X-Amz-Target", "AmazonSSM.AddTagsToResource");
                then.status(400).json_body(serde_json::json!({
                    "__type": "AccessDeniedException",
                    "message": "not authorized to perform: ssm:AddTagsToResource"
                }));
            })
            .await;
        let vault = vault_for(&server, "acme-1a2b-secrets", "secrets");

        vault.create_secret("api-key", "v").await.unwrap();
        vault.set_secret("api-key", "v").await.unwrap();

        assert_eq!(
            denied.hits_async().await,
            2,
            "one refused attempt per created value, no retry loop"
        );
    }

    #[tokio::test]
    async fn markers_that_do_not_fit_a_label_are_encoded() {
        let server = httpmock::MockServer::start_async().await;
        let long_prefix = format!("{}-secrets", "a".repeat(60));
        let expected = alien_core::vault_naming::vault_marker_value(&long_prefix);
        assert!(expected.starts_with("h-") && expected.len() == 42);
        server
            .mock_async(|when, then| {
                when.method(httpmock::Method::POST)
                    .header("X-Amz-Target", "AmazonSSM.PutParameter");
                then.status(200)
                    .json_body(serde_json::json!({"Version": 1}));
            })
            .await;
        let put = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::POST)
                    .header("X-Amz-Target", "AmazonSSM.AddTagsToResource")
                    .json_body_partial(
                        serde_json::json!({"Tags": [
                            {"Key": "alien-vault-prefix", "Value": expected}
                        ]})
                        .to_string(),
                    );
                then.status(200).json_body(serde_json::json!({}));
            })
            .await;
        let vault = vault_for(&server, &long_prefix, "secrets");

        vault.create_secret("k", "v").await.unwrap();

        assert_eq!(put.hits_async().await, 1);
    }
}
