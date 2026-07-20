//! Converts refreshable provider credentials into response-safe short-lived
//! configurations. HTTP routes choose a purpose; this module owns the cloud
//! handoff and expiry rules.

use std::collections::HashMap;

use alien_aws_clients::AwsClientConfigExt;
use alien_azure_clients::AzureClientConfigExt;
use alien_core::{
    AwsClientConfig, AwsCredentials, AzureClientConfig, AzureCredentials, ClientConfig,
    GcpClientConfig, GcpCredentials, Platform,
};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_gcp_clients::GcpClientConfigExt;
use chrono::{DateTime, Utc};

use crate::error::ErrorData;

const GCP_CLOUD_PLATFORM_SCOPE: &str = "https://www.googleapis.com/auth/cloud-platform";
pub(crate) const AZURE_STORAGE_SCOPE: &str = "https://storage.azure.com/.default";
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
    config: ClientConfig,
) -> Result<MaterializedCredentialLease, AlienError<ErrorData>> {
    match config {
        ClientConfig::Aws(config) => {
            let config = config.materialize_session_credentials().await.context(
                ErrorData::CredentialMaterializationFailed {
                    platform: Platform::Aws,
                    purpose: "remote Storage".to_string(),
                },
            )?;
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
        ClientConfig::Gcp(config) => {
            let token = config
                .get_access_token_with_expiry(GCP_CLOUD_PLATFORM_SCOPE)
                .await
                .context(ErrorData::CredentialMaterializationFailed {
                    platform: Platform::Gcp,
                    purpose: "remote Storage".to_string(),
                })?;
            Ok(MaterializedCredentialLease {
                client_config: ClientConfig::Gcp(Box::new(GcpClientConfig {
                    project_id: config.project_id,
                    region: config.region,
                    credentials: GcpCredentials::AccessToken { token: token.token },
                    service_overrides: None,
                    project_number: config.project_number,
                })),
                expires_at: token.expires_at,
            })
        }
        ClientConfig::Azure(config) => {
            if matches!(&config.credentials, AzureCredentials::AccessToken { .. }) {
                return Err(ErrorData::internal(
                    "Remote Azure Storage requires an exact storage-scope token",
                ));
            }
            let token = config
                .get_bearer_token_with_expiry(AZURE_STORAGE_SCOPE)
                .await
                .context(ErrorData::CredentialMaterializationFailed {
                    platform: Platform::Azure,
                    purpose: "remote Storage".to_string(),
                })?;
            Ok(MaterializedCredentialLease {
                client_config: ClientConfig::Azure(Box::new(AzureClientConfig {
                    subscription_id: config.subscription_id,
                    tenant_id: config.tenant_id,
                    region: config.region,
                    credentials: AzureCredentials::ScopedAccessTokens {
                        tokens: HashMap::from([(AZURE_STORAGE_SCOPE.to_string(), token.token)]),
                    },
                    service_overrides: None,
                })),
                expires_at: token.expires_at,
            })
        }
        other => Err(ErrorData::internal(format!(
            "Credential impersonation returned unsupported {} client config",
            other.platform()
        ))),
    }
}

#[cfg(test)]
mod tests {
    use base64::Engine;

    use super::*;

    #[tokio::test]
    async fn remote_storage_keeps_only_the_azure_storage_audience() {
        let expires_at_timestamp = 1_893_456_000;
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(serde_json::json!({ "exp": expires_at_timestamp }).to_string());
        let storage_token = format!("e30.{payload}.signature");
        let config = ClientConfig::Azure(Box::new(AzureClientConfig {
            subscription_id: "subscription".to_string(),
            tenant_id: "tenant".to_string(),
            region: Some("eastus".to_string()),
            credentials: AzureCredentials::ScopedAccessTokens {
                tokens: HashMap::from([
                    (AZURE_STORAGE_SCOPE.to_string(), storage_token.clone()),
                    (
                        "https://management.azure.com/.default".to_string(),
                        "management-token".to_string(),
                    ),
                ]),
            },
            service_overrides: None,
        }));

        let lease = materialize_remote_storage_lease(config)
            .await
            .expect("storage token should materialize");
        let ClientConfig::Azure(config) = lease.client_config else {
            panic!("expected Azure config");
        };
        let AzureCredentials::ScopedAccessTokens { tokens } = config.credentials else {
            panic!("expected scoped Azure tokens");
        };
        assert_eq!(
            tokens,
            HashMap::from([(AZURE_STORAGE_SCOPE.to_string(), storage_token)])
        );
        assert_eq!(lease.expires_at.timestamp(), expires_at_timestamp);
    }

    #[tokio::test]
    async fn remote_gcp_storage_rejects_opaque_access_tokens_without_expiry() {
        let config = ClientConfig::Gcp(Box::new(GcpClientConfig {
            project_id: "project".to_string(),
            region: "us-central1".to_string(),
            credentials: GcpCredentials::AccessToken {
                token: "opaque-token".to_string(),
            },
            service_overrides: None,
            project_number: None,
        }));

        let error = materialize_remote_storage_lease(config)
            .await
            .expect_err("opaque token has no authoritative expiry");
        assert!(!error.retryable);
    }
}
