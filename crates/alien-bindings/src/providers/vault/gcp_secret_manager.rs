use std::future::Future;
use std::time::Duration;

use alien_core::{GcpClientConfig, GcpCredentials, GcpImpersonationConfig};
use alien_error::{AlienError, Context, ContextError, IntoAlienError, IntoAlienErrorDirect};
use async_trait::async_trait;
use google_cloud_auth::credentials::{
    self, CacheableResource, Credentials, CredentialsProvider, EntityTag,
};
use google_cloud_auth::errors::CredentialsError;
use google_cloud_secretmanager_v1::{
    client::SecretManagerService,
    model::{replication, Replication, Secret, SecretPayload},
};
use http::{header::AUTHORIZATION, Extensions, HeaderMap, HeaderValue};
use serde_json::{json, Value};
use tracing::debug;

use crate::error::{ErrorData, Result};

const CLOUD_PLATFORM_SCOPE: &str = "https://www.googleapis.com/auth/cloud-platform";
const SECRET_MANAGER_ENDPOINT: &str = "https://secretmanager.googleapis.com";

/// GCP Secret Manager vault binding implementation.
#[derive(Debug)]
pub struct GcpSecretManagerVault {
    client: SecretManagerService,
    vault_prefix: String,
    project_id: String,
}

impl GcpSecretManagerVault {
    /// Create a new GCP Secret Manager vault binding.
    pub async fn new(gcp_config: GcpClientConfig, vault_prefix: String) -> Result<Self> {
        let endpoint = gcp_config
            .service_overrides
            .as_ref()
            .and_then(|overrides| overrides.endpoints.get("secretmanager"))
            .cloned()
            .unwrap_or_else(|| SECRET_MANAGER_ENDPOINT.to_string());

        let client = SecretManagerService::builder()
            .with_endpoint(endpoint)
            .with_credentials(credentials_from_gcp_config(&gcp_config)?)
            .build()
            .await
            .into_alien_error()
            .context(ErrorData::BindingSetupFailed {
                binding_type: "vault.secretManager".to_string(),
                reason: "Failed to build official GCP Secret Manager client".to_string(),
            })?;

        Ok(Self {
            client,
            vault_prefix,
            project_id: gcp_config.project_id,
        })
    }

    /// Generate the full secret name with vault prefix.
    fn full_secret_name(&self, secret_name: &str) -> String {
        format!("{}-{}", self.vault_prefix, secret_name)
    }

    /// Generate the secret resource name for GCP API.
    fn secret_resource_name(&self, secret_name: &str) -> String {
        format!(
            "projects/{}/secrets/{}",
            self.project_id,
            self.full_secret_name(secret_name)
        )
    }

    fn project_resource_name(&self) -> String {
        format!("projects/{}", self.project_id)
    }

    async fn add_secret_version(
        &self,
        secret_resource: &str,
        value: &str,
    ) -> google_cloud_secretmanager_v1::Result<()> {
        self.client
            .add_secret_version()
            .set_parent(secret_resource)
            .set_payload(SecretPayload::new().set_data(value.as_bytes().to_vec()))
            .send()
            .await?;

        Ok(())
    }
}

#[async_trait]
impl crate::traits::Binding for GcpSecretManagerVault {}

#[async_trait]
impl crate::traits::Vault for GcpSecretManagerVault {
    /// Get a secret value by name.
    async fn get_secret(&self, secret_name: &str) -> Result<String> {
        let secret_resource = self.secret_resource_name(secret_name);
        let response = self
            .client
            .access_secret_version()
            .set_name(format!("{}/versions/latest", secret_resource))
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to access secret version '{}'", secret_resource),
                resource_id: None,
            })?;

        let payload = response.payload.ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: format!("Secret '{}' has no payload", secret_resource),
                resource_id: None,
            })
        })?;

        String::from_utf8(payload.data.to_vec())
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: format!("Secret '{}' contains invalid UTF-8 data", secret_resource),
                resource_id: None,
            })
    }

    /// Set a secret value.
    async fn set_secret(&self, secret_name: &str, value: &str) -> Result<()> {
        let secret_resource = self.secret_resource_name(secret_name);

        match self.add_secret_version(&secret_resource, value).await {
            Ok(()) => Ok(()),
            Err(e) if gcp_error_is_not_found(&e) => {
                let full_secret_name = self.full_secret_name(secret_name);
                let replication =
                    Replication::new().set_automatic(replication::Automatic::default());
                let secret = Secret::new().set_replication(replication);

                self.client
                    .create_secret()
                    .set_parent(self.project_resource_name())
                    .set_secret_id(full_secret_name)
                    .set_secret(secret)
                    .send()
                    .await
                    .into_alien_error()
                    .context(ErrorData::CloudPlatformError {
                        message: format!("Failed to create secret '{}'", secret_resource),
                        resource_id: None,
                    })?;

                self.add_secret_version(&secret_resource, value)
                    .await
                    .into_alien_error()
                    .context(ErrorData::CloudPlatformError {
                        message: format!("Failed to add version to secret '{}'", secret_resource),
                        resource_id: None,
                    })
            }
            Err(e) => Err(e.into_alien_error().context(ErrorData::CloudPlatformError {
                message: format!("Failed to add version to secret '{}'", secret_resource),
                resource_id: None,
            })),
        }
    }

    /// Delete a secret.
    async fn delete_secret(&self, secret_name: &str) -> Result<()> {
        let secret_resource = self.secret_resource_name(secret_name);

        match self
            .client
            .delete_secret()
            .set_name(secret_resource.clone())
            .send()
            .await
        {
            Ok(_) => Ok(()),
            Err(e) if gcp_error_is_not_found(&e) => {
                debug!(
                    "Secret '{}' was not found during deletion - treating as success",
                    secret_resource
                );
                Ok(())
            }
            Err(e) => Err(e.into_alien_error().context(ErrorData::CloudPlatformError {
                message: format!("Failed to delete secret '{}'", secret_resource),
                resource_id: None,
            })),
        }
    }
}

#[derive(Debug, Clone)]
struct StaticAccessTokenCredentials {
    token: String,
    entity_tag: EntityTag,
}

impl StaticAccessTokenCredentials {
    fn new(token: String) -> Self {
        Self {
            token,
            entity_tag: EntityTag::new(),
        }
    }
}

impl CredentialsProvider for StaticAccessTokenCredentials {
    fn headers(
        &self,
        _extensions: Extensions,
    ) -> impl Future<Output = std::result::Result<CacheableResource<HeaderMap>, CredentialsError>> + Send
    {
        let token = self.token.clone();
        let entity_tag = self.entity_tag.clone();
        async move {
            let mut value = HeaderValue::from_str(&format!("Bearer {token}"))
                .map_err(|error| CredentialsError::from_source(false, error))?;
            value.set_sensitive(true);

            let mut headers = HeaderMap::new();
            headers.insert(AUTHORIZATION, value);

            Ok(CacheableResource::New {
                entity_tag,
                data: headers,
            })
        }
    }

    fn universe_domain(&self) -> impl Future<Output = Option<String>> + Send {
        async { None }
    }
}

fn credentials_from_gcp_config(config: &GcpClientConfig) -> Result<Credentials> {
    credentials_from_gcp_credentials(&config.credentials)
}

fn credentials_from_gcp_credentials(credentials: &GcpCredentials) -> Result<Credentials> {
    match credentials {
        GcpCredentials::AccessToken { token } => {
            Ok(Credentials::from(StaticAccessTokenCredentials::new(token.clone())))
        }
        GcpCredentials::ServiceAccountKey { json } => {
            let key = serde_json::from_str::<Value>(json).into_alien_error().context(
                ErrorData::BindingSetupFailed {
                    binding_type: "vault.secretManager".to_string(),
                    reason: "Failed to parse GCP service account key JSON".to_string(),
                },
            )?;
            credentials::service_account::Builder::new(key)
                .with_access_specifier(credentials::service_account::AccessSpecifier::from_scopes(
                    [CLOUD_PLATFORM_SCOPE],
                ))
                .build()
                .into_alien_error()
                .context(ErrorData::BindingSetupFailed {
                    binding_type: "vault.secretManager".to_string(),
                    reason: "Failed to build official GCP service account credentials".to_string(),
                })
        }
        GcpCredentials::ServiceMetadata => credentials::mds::Builder::default()
            .with_scopes([CLOUD_PLATFORM_SCOPE])
            .build()
            .into_alien_error()
            .context(ErrorData::BindingSetupFailed {
                binding_type: "vault.secretManager".to_string(),
                reason: "Failed to build official GCP metadata credentials".to_string(),
            }),
        GcpCredentials::ExternalAccount {
            audience,
            subject_token_type,
            token_url,
            credential_source_file,
            service_account_impersonation_url,
        } => {
            let external_account = external_account_json(
                audience,
                subject_token_type,
                token_url,
                credential_source_file,
                service_account_impersonation_url.as_deref(),
            );
            credentials::external_account::Builder::new(external_account)
                .build()
                .into_alien_error()
                .context(ErrorData::BindingSetupFailed {
                    binding_type: "vault.secretManager".to_string(),
                    reason: "Failed to build official GCP external account credentials".to_string(),
                })
        }
        GcpCredentials::AuthorizedUser {
            client_id,
            client_secret,
            refresh_token,
        } => {
            let authorized_user = json!({
                "type": "authorized_user",
                "client_id": client_id,
                "client_secret": client_secret,
                "refresh_token": refresh_token,
            });
            credentials::user_account::Builder::new(authorized_user)
                .with_scopes([CLOUD_PLATFORM_SCOPE])
                .build()
                .into_alien_error()
                .context(ErrorData::BindingSetupFailed {
                    binding_type: "vault.secretManager".to_string(),
                    reason: "Failed to build official GCP authorized user credentials".to_string(),
                })
        }
        GcpCredentials::ImpersonatedServiceAccount { source, config } => {
            impersonated_credentials_from_gcp_config(source, config)
        }
        GcpCredentials::ProjectedServiceAccount { .. } => Err(AlienError::new(
            ErrorData::BindingSetupFailed {
                binding_type: "vault.secretManager".to_string(),
                reason: "Projected service account token files are not a complete official Google auth credential configuration; use external_account credentials with an audience and credential source instead".to_string(),
            },
        )),
    }
}

fn impersonated_credentials_from_gcp_config(
    source: &GcpClientConfig,
    config: &GcpImpersonationConfig,
) -> Result<Credentials> {
    let source_credentials = credentials_from_gcp_config(source)?;
    let mut builder =
        credentials::impersonated::Builder::from_source_credentials(source_credentials)
            .with_target_principal(config.service_account_email.clone())
            .with_scopes(config.scopes.clone());

    if let Some(delegates) = &config.delegates {
        builder = builder.with_delegates(delegates.clone());
    }

    if let Some(lifetime) = &config.lifetime {
        builder = builder.with_lifetime(parse_google_duration(lifetime)?);
    }

    builder
        .build()
        .into_alien_error()
        .context(ErrorData::BindingSetupFailed {
            binding_type: "vault.secretManager".to_string(),
            reason: "Failed to build official GCP impersonated credentials".to_string(),
        })
}

fn external_account_json(
    audience: &str,
    subject_token_type: &str,
    token_url: &str,
    credential_source_file: &str,
    service_account_impersonation_url: Option<&str>,
) -> Value {
    let mut value = json!({
        "type": "external_account",
        "audience": audience,
        "subject_token_type": subject_token_type,
        "token_url": token_url,
        "credential_source": {
            "file": credential_source_file,
        },
        "scopes": [CLOUD_PLATFORM_SCOPE],
    });

    if let Some(url) = service_account_impersonation_url {
        value["service_account_impersonation_url"] = Value::String(url.to_string());
    }

    value
}

fn parse_google_duration(value: &str) -> Result<Duration> {
    let seconds = value
        .strip_suffix('s')
        .ok_or_else(|| {
            AlienError::new(ErrorData::BindingSetupFailed {
                binding_type: "vault.secretManager".to_string(),
                reason: format!("Invalid Google duration '{}': missing 's' suffix", value),
            })
        })?
        .parse::<u64>()
        .into_alien_error()
        .context(ErrorData::BindingSetupFailed {
            binding_type: "vault.secretManager".to_string(),
            reason: format!("Invalid Google duration '{}'", value),
        })?;

    Ok(Duration::from_secs(seconds))
}

fn gcp_error_is_not_found(error: &google_cloud_secretmanager_v1::Error) -> bool {
    error.http_status_code() == Some(404)
}
