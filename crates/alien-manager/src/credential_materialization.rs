//! Converts refreshable provider credentials into response-safe short-lived
//! configurations. HTTP routes choose a purpose; this module owns the cloud
//! handoff and expiry rules.

use std::collections::HashMap;

use alien_azure_clients::AzureClientConfigExt;
use alien_core::{
    AwsClientConfig, AwsCredentials, AzureClientConfig, AzureCredentials, ClientConfig,
    GcpClientConfig, GcpCredentials, Platform,
};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_gcp_clients::GcpClientConfigExt;
use chrono::{DateTime, Utc};

use crate::error::ErrorData;
use crate::traits::RemoteStorageCredentialSource;

const GCP_CLOUD_PLATFORM_SCOPE: &str = "https://www.googleapis.com/auth/cloud-platform";
pub(crate) const AZURE_STORAGE_SCOPE: &str = "https://storage.azure.com/.default";
const REMOTE_STORAGE_DURATION_SECONDS: i32 = 3600;
const AZURE_MINT_SCOPES: [&str; 4] = [
    "https://management.azure.com/.default",
    AZURE_STORAGE_SCOPE,
    "https://vault.azure.net/.default",
    "https://servicebus.azure.net/.default",
];

pub(crate) struct MaterializedCredentialLease {
    pub client_config: ClientConfig,
    pub expires_at: DateTime<Utc>,
}

/// Exact cloud resource requested by remote binding resolution.
pub(crate) enum RemoteStorageCredentialScope {
    AwsS3,
    GcpGcs,
    AzureBlob,
}

impl std::fmt::Debug for MaterializedCredentialLease {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MaterializedCredentialLease")
            .field("client_config", &"[REDACTED]")
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

/// Convert provider impersonation output into a response-safe credential form.
/// Refreshable sources and internal service overrides never cross the API.
pub(crate) async fn materialize_minted_client_config(
    config: ClientConfig,
) -> Result<ClientConfig, AlienError<ErrorData>> {
    match config {
        ClientConfig::Aws(config)
            if matches!(
                &config.credentials,
                AwsCredentials::SessionCredentials { .. }
            ) =>
        {
            Ok(ClientConfig::Aws(Box::new(AwsClientConfig {
                account_id: config.account_id,
                region: config.region,
                credentials: config.credentials,
                service_overrides: None,
            })))
        }
        ClientConfig::Aws(_) => Err(ErrorData::internal(
            "AWS impersonation did not return short-lived session credentials",
        )),
        ClientConfig::Gcp(config) => {
            let token = config
                .get_bearer_token(GCP_CLOUD_PLATFORM_SCOPE)
                .await
                .context(ErrorData::CredentialMaterializationFailed {
                    platform: Platform::Gcp,
                    purpose: "credential minting".to_string(),
                })?;
            Ok(ClientConfig::Gcp(Box::new(GcpClientConfig {
                project_id: config.project_id,
                region: config.region,
                credentials: GcpCredentials::AccessToken { token },
                service_overrides: None,
                project_number: config.project_number,
            })))
        }
        ClientConfig::Azure(config) => {
            if matches!(&config.credentials, AzureCredentials::AccessToken { .. }) {
                return Err(ErrorData::internal(
                    "Azure impersonation returned a single-scope access token; exact per-scope tokens are required",
                ));
            }
            let mut tokens = HashMap::with_capacity(AZURE_MINT_SCOPES.len());
            for scope in AZURE_MINT_SCOPES {
                let token = config.get_bearer_token_with_scope(scope).await.context(
                    ErrorData::CredentialMaterializationFailed {
                        platform: Platform::Azure,
                        purpose: format!("credential minting scope '{scope}'"),
                    },
                )?;
                tokens.insert(scope.to_string(), token);
            }
            Ok(ClientConfig::Azure(Box::new(AzureClientConfig {
                subscription_id: config.subscription_id,
                tenant_id: config.tenant_id,
                region: config.region,
                credentials: AzureCredentials::ScopedAccessTokens { tokens },
                service_overrides: None,
            })))
        }
        other => Err(ErrorData::internal(format!(
            "Credential impersonation returned unsupported {} client config",
            other.platform()
        ))),
    }
}

/// Materialize the one short-lived credential needed by remote Storage and
/// preserve the cloud provider's authoritative expiry.
pub(crate) async fn materialize_remote_storage_lease(
    source: RemoteStorageCredentialSource,
    scope: RemoteStorageCredentialScope,
) -> Result<MaterializedCredentialLease, AlienError<ErrorData>> {
    let RemoteStorageCredentialSource::Direct(config) = source;
    if config.platform() != remote_scope_platform(&scope) {
        return Err(ErrorData::internal(
            "Remote Bindings credential provider does not match the resolved resource",
        ));
    }
    match config {
        ClientConfig::Aws(config) => aws_remote_storage_lease(*config),
        ClientConfig::Gcp(config) => {
            let token = config
                .get_bearer_token(GCP_CLOUD_PLATFORM_SCOPE)
                .await
                .context(ErrorData::CredentialMaterializationFailed {
                    platform: Platform::Gcp,
                    purpose: "Remote Bindings".to_string(),
                })?;
            Ok(MaterializedCredentialLease {
                client_config: ClientConfig::Gcp(Box::new(GcpClientConfig {
                    project_id: config.project_id,
                    region: config.region,
                    credentials: GcpCredentials::AccessToken { token },
                    service_overrides: None,
                    project_number: config.project_number,
                })),
                expires_at: Utc::now()
                    + chrono::Duration::seconds(i64::from(REMOTE_STORAGE_DURATION_SECONDS)),
            })
        }
        ClientConfig::Azure(config) => {
            let token = config
                .get_bearer_token_with_scope(AZURE_STORAGE_SCOPE)
                .await
                .context(ErrorData::CredentialMaterializationFailed {
                    platform: Platform::Azure,
                    purpose: "Remote Bindings".to_string(),
                })?;
            Ok(MaterializedCredentialLease {
                client_config: ClientConfig::Azure(Box::new(AzureClientConfig {
                    subscription_id: config.subscription_id,
                    tenant_id: config.tenant_id,
                    region: config.region,
                    credentials: AzureCredentials::AccessToken { token },
                    service_overrides: None,
                })),
                expires_at: Utc::now()
                    + chrono::Duration::seconds(i64::from(REMOTE_STORAGE_DURATION_SECONDS)),
            })
        }
        other => Err(ErrorData::internal(format!(
            "Remote Bindings returned unsupported {} credentials",
            other.platform()
        ))),
    }
}

fn aws_remote_storage_lease(
    config: AwsClientConfig,
) -> Result<MaterializedCredentialLease, AlienError<ErrorData>> {
    let AwsCredentials::SessionCredentials { expires_at, .. } = &config.credentials else {
        return Err(ErrorData::internal(
            "Remote AWS Storage credentials are not a short-lived session",
        ));
    };
    let expires_at = DateTime::parse_from_rfc3339(expires_at)
        .into_alien_error()
        .context(ErrorData::InternalError {
            message: "AWS returned an invalid session credential expiry".to_string(),
        })?
        .with_timezone(&Utc);
    Ok(MaterializedCredentialLease {
        client_config: ClientConfig::Aws(Box::new(AwsClientConfig {
            account_id: config.account_id,
            region: config.region,
            credentials: config.credentials,
            service_overrides: None,
        })),
        expires_at,
    })
}

fn remote_scope_platform(scope: &RemoteStorageCredentialScope) -> Platform {
    match scope {
        RemoteStorageCredentialScope::AwsS3 => Platform::Aws,
        RemoteStorageCredentialScope::GcpGcs => Platform::Gcp,
        RemoteStorageCredentialScope::AzureBlob => Platform::Azure,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn remote_bindings_rejects_provider_mismatch() {
        let config = ClientConfig::Gcp(Box::new(GcpClientConfig {
            project_id: "project".to_string(),
            region: "us-central1".to_string(),
            credentials: GcpCredentials::AccessToken {
                token: "opaque-token".to_string(),
            },
            service_overrides: None,
            project_number: None,
        }));

        let error = materialize_remote_storage_lease(
            RemoteStorageCredentialSource::Direct(config),
            RemoteStorageCredentialScope::AwsS3,
        )
        .await
        .expect_err("provider mismatch must fail closed");
        assert!(!error.retryable);
    }
}
