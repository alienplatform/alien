//! Explicit conversion from the generated manager SDK into binding domain types.
//!
//! Keep this boundary exhaustive: a manager schema change must fail compilation
//! until every new response or credential variant has an intentional mapping.

use std::collections::HashMap;

use alien_error::{Context, IntoAlienError};
use alien_manager_api::types as manager_types;
use chrono::{DateTime, Utc};

use super::{invalid_remote_lease, ResolvedRemoteBinding};
use crate::error::{ErrorData, Result};

const AZURE_REMOTE_STORAGE_PERMISSIONS: &str = "rcwdl";

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
                let manager_types::RemoteAzureCredentials::ContainerSas(sas) =
                    client_config.credentials;
                let expires_at = parse_manager_expiry(expires_at, resource_id)?;
                let query_parameters = azure_sas_query_parameters(
                    sas,
                    &binding.account_name,
                    &binding.container_name,
                    expires_at,
                    resource_id,
                )?;
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
                        credentials: alien_core::AzureCredentials::SasToken { query_parameters },
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

fn azure_sas_query_parameters(
    sas: manager_types::RemoteAzureContainerSas,
    account_name: &str,
    container_name: &str,
    lease_expires_at: DateTime<Utc>,
    resource_id: &str,
) -> Result<HashMap<String, String>> {
    if sas.account_name != account_name || sas.container_name != container_name {
        return Err(invalid_remote_lease(
            "Azure",
            "SAS scope does not match the resolved Blob container",
        ));
    }
    if sas.permissions != AZURE_REMOTE_STORAGE_PERMISSIONS
        || sas.protocol != "https"
        || sas.signed_resource != "c"
        || sas.signed_key_service != "b"
    {
        return Err(invalid_remote_lease(
            "Azure",
            "SAS permissions or signed scope are not exact",
        ));
    }
    if [
        &sas.signed_object_id,
        &sas.signed_tenant_id,
        &sas.signed_key_version,
        &sas.service_version,
        &sas.signature,
    ]
    .into_iter()
    .any(|value| value.is_empty())
    {
        return Err(invalid_remote_lease(
            "Azure",
            "SAS contains an empty required signed field",
        ));
    }

    let starts_at = parse_manager_timestamp(&sas.starts_at, "Azure SAS start", resource_id)?;
    let expires_at = parse_manager_timestamp(&sas.expires_at, "Azure SAS expiry", resource_id)?;
    let signed_key_start = parse_manager_timestamp(
        &sas.signed_key_start,
        "Azure SAS signed-key start",
        resource_id,
    )?;
    let signed_key_expiry = parse_manager_timestamp(
        &sas.signed_key_expiry,
        "Azure SAS signed-key expiry",
        resource_id,
    )?;
    if starts_at >= expires_at
        || expires_at < lease_expires_at
        || signed_key_start > starts_at
        || signed_key_expiry < expires_at
    {
        return Err(invalid_remote_lease(
            "Azure",
            "SAS or signed-key lifetime does not cover the credential lease",
        ));
    }

    Ok(HashMap::from([
        ("sp".to_string(), sas.permissions),
        ("st".to_string(), sas.starts_at),
        ("se".to_string(), sas.expires_at),
        ("skoid".to_string(), sas.signed_object_id),
        ("sktid".to_string(), sas.signed_tenant_id),
        ("skt".to_string(), sas.signed_key_start),
        ("ske".to_string(), sas.signed_key_expiry),
        ("sks".to_string(), sas.signed_key_service),
        ("skv".to_string(), sas.signed_key_version),
        ("spr".to_string(), sas.protocol),
        ("sv".to_string(), sas.service_version),
        ("sr".to_string(), sas.signed_resource),
        ("sig".to_string(), sas.signature),
    ]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn azure_sas_conversion_rejects_scope_and_lifetime_drift_without_leaking_signature() {
        let lease_expires_at = DateTime::parse_from_rfc3339("2030-01-01T01:00:00Z")
            .unwrap()
            .with_timezone(&Utc);

        let mut wrong_container = valid_azure_sas();
        wrong_container.container_name = "another-container".to_string();
        assert_rejected_sas(wrong_container, lease_expires_at);

        let mut wrong_permissions = valid_azure_sas();
        wrong_permissions.permissions = "rl".to_string();
        assert_rejected_sas(wrong_permissions, lease_expires_at);

        let mut short_lived = valid_azure_sas();
        short_lived.expires_at = "2030-01-01T00:59:59Z".to_string();
        assert_rejected_sas(short_lived, lease_expires_at);
    }

    fn assert_rejected_sas(
        sas: manager_types::RemoteAzureContainerSas,
        lease_expires_at: DateTime<Utc>,
    ) {
        let error =
            azure_sas_query_parameters(sas, "account", "container", lease_expires_at, "files")
                .expect_err("invalid Azure SAS must fail closed");
        assert!(!format!("{error:?}").contains("SENTINEL_SAS_SIGNATURE"));
    }

    fn valid_azure_sas() -> manager_types::RemoteAzureContainerSas {
        manager_types::RemoteAzureContainerSas {
            account_name: "account".to_string(),
            container_name: "container".to_string(),
            expires_at: "2030-01-01T01:00:00Z".to_string(),
            permissions: "rcwdl".to_string(),
            protocol: "https".to_string(),
            service_version: "2023-11-03".to_string(),
            signature: "SENTINEL_SAS_SIGNATURE".to_string(),
            signed_key_expiry: "2030-01-01T02:00:00Z".to_string(),
            signed_key_service: "b".to_string(),
            signed_key_start: "2029-12-31T23:50:00Z".to_string(),
            signed_key_version: "2023-11-03".to_string(),
            signed_object_id: "object-id".to_string(),
            signed_resource: "c".to_string(),
            signed_tenant_id: "tenant-id".to_string(),
            starts_at: "2029-12-31T23:55:00Z".to_string(),
        }
    }
}
