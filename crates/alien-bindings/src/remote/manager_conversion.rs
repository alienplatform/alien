//! Explicit conversion from the generated manager SDK into binding domain types.
//!
//! Keep this boundary exhaustive: a manager schema change must fail compilation
//! until every new response or credential variant has an intentional mapping.

use alien_error::{Context, IntoAlienError};
use alien_manager_api::types as manager_types;
use chrono::{DateTime, Utc};

use super::ResolvedRemoteBinding;
use crate::error::{ErrorData, Result};

impl ResolvedRemoteBinding {
    pub(super) fn from_manager_response(
        response: manager_types::ResolveBindingResponse,
        resource_id: &str,
    ) -> Result<Self> {
        let lease = match response {
            manager_types::ResolveBindingResponse::S3 {
                binding,
                client_config,
                expires_at,
            } => {
                let manager_types::RemoteAwsCredentials::SessionCredentials {
                    access_key_id,
                    secret_access_key,
                    session_token,
                    expires_at: credential_expires_at,
                } = client_config.credentials;
                Self::S3 {
                    binding: alien_core::S3StorageBinding {
                        bucket_name: binding.bucket_name.into(),
                    },
                    client_config: Box::new(alien_core::AwsClientConfig {
                        account_id: client_config.account_id,
                        region: client_config.region,
                        credentials: alien_core::AwsCredentials::SessionCredentials {
                            access_key_id,
                            secret_access_key,
                            session_token,
                            expires_at: credential_expires_at,
                        },
                        service_overrides: None,
                    }),
                    expires_at: parse_manager_expiry(expires_at, resource_id)?,
                }
            }
            manager_types::ResolveBindingResponse::Blob {
                binding,
                client_config,
                expires_at,
            } => {
                let manager_types::RemoteAzureCredentials::AccessToken(token) =
                    client_config.credentials;
                let expires_at = parse_manager_expiry(expires_at, resource_id)?;
                let binding = alien_core::BlobStorageBinding {
                    account_name: binding.account_name.into(),
                    container_name: binding.container_name.into(),
                };
                Self::Blob {
                    binding,
                    client_config: Box::new(alien_core::AzureClientConfig {
                        subscription_id: client_config.subscription_id,
                        tenant_id: client_config.tenant_id,
                        region: client_config.region,
                        credentials: alien_core::AzureCredentials::AccessToken { token },
                        service_overrides: None,
                    }),
                    expires_at,
                }
            }
            manager_types::ResolveBindingResponse::Gcs {
                binding,
                client_config,
                expires_at,
            } => {
                let manager_types::RemoteGcpCredentials::AccessToken(token) =
                    client_config.credentials;
                Self::Gcs {
                    binding: alien_core::GcsStorageBinding {
                        bucket_name: binding.bucket_name.into(),
                    },
                    client_config: Box::new(alien_core::GcpClientConfig {
                        project_id: client_config.project_id,
                        region: client_config.region,
                        credentials: alien_core::GcpCredentials::AccessToken { token },
                        service_overrides: None,
                        project_number: client_config.project_number,
                    }),
                    expires_at: parse_manager_expiry(expires_at, resource_id)?,
                }
            }
        };
        Ok(lease)
    }
}

fn parse_manager_expiry(expires_at: String, resource_id: &str) -> Result<DateTime<Utc>> {
    parse_manager_timestamp(&expires_at, "credential lease expiry", resource_id)
}

fn parse_manager_timestamp(
    timestamp: &str,
    field: &str,
    resource_id: &str,
) -> Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(timestamp)
        .into_alien_error()
        .context(ErrorData::RemoteAccessFailed {
            operation: format!("parse {field} for remote Storage binding '{resource_id}'"),
        })
        .map(|expires_at| expires_at.with_timezone(&Utc))
}
