//! Explicit conversion from the generated manager SDK into binding domain types.
//!
//! Keep this boundary exhaustive: a manager schema change must fail compilation
//! until every new response or credential variant has an intentional mapping.

use alien_error::{AlienError, Context, IntoAlienError};
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
                    expires_at: parse_manager_expiry(expires_at, &resource_id)?,
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
                    expires_at: parse_manager_expiry(expires_at, &resource_id)?,
                }
            }
            manager_types::ResolveBindingResponse::Kms {
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
                Self::Kms {
                    binding: alien_core::AwsKmsKeyBinding {
                        key_arn: binding.key_arn.into(),
                        region: binding.region.map(Into::into),
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
                    expires_at: parse_manager_expiry(expires_at, &resource_id)?,
                }
            }
            manager_types::ResolveBindingResponse::CloudKms {
                binding,
                client_config,
                expires_at,
            } => {
                let manager_types::RemoteGcpCredentials::AccessToken(token) =
                    client_config.credentials;
                Self::CloudKms {
                    binding: alien_core::GcpCloudKmsKeyBinding {
                        crypto_key_name: binding.crypto_key_name.into(),
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
            manager_types::ResolveBindingResponse::KeyVaultKey {
                binding,
                client_config,
                expires_at,
            } => {
                let manager_types::RemoteAzureCredentials::AccessToken(token) =
                    client_config.credentials;
                Self::KeyVaultKey {
                    binding: alien_core::AzureKeyVaultKeyBinding {
                        key_id: binding.key_id.into(),
                    },
                    client_config: Box::new(alien_core::AzureClientConfig {
                        subscription_id: client_config.subscription_id,
                        tenant_id: client_config.tenant_id,
                        region: client_config.region,
                        credentials: alien_core::AzureCredentials::AccessToken { token },
                        service_overrides: None,
                    }),
                    expires_at: parse_manager_expiry(expires_at, resource_id)?,
                }
            }
            manager_types::ResolveBindingResponse::Bedrock {
                resource_id,
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
                let expires_at = parse_manager_expiry(expires_at, &resource_id)?;
                Self::Bedrock {
                    resource_id,
                    binding: alien_core::BedrockAiBinding {
                        region: binding.region,
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
                    expires_at,
                }
            }
            manager_types::ResolveBindingResponse::Vertex {
                resource_id,
                binding,
                client_config,
                expires_at,
            } => {
                let manager_types::RemoteGcpCredentials::AccessToken(token) =
                    client_config.credentials;
                let expires_at = parse_manager_expiry(expires_at, &resource_id)?;
                Self::Vertex {
                    resource_id,
                    binding: alien_core::VertexAiBinding {
                        project: binding.project,
                        location: binding.location,
                    },
                    client_config: Box::new(alien_core::GcpClientConfig {
                        project_id: client_config.project_id,
                        region: client_config.region,
                        credentials: alien_core::GcpCredentials::AccessToken { token },
                        service_overrides: None,
                        project_number: client_config.project_number,
                    }),
                    expires_at,
                }
            }
            manager_types::ResolveBindingResponse::Foundry {
                resource_id,
                binding,
                client_config,
                expires_at,
            } => {
                let manager_types::RemoteAzureCredentials::AccessToken(token) =
                    client_config.credentials;
                let expires_at = parse_manager_expiry(expires_at, &resource_id)?;
                Self::Foundry {
                    resource_id,
                    binding: alien_core::FoundryAiBinding {
                        endpoint: binding.endpoint,
                        account: binding.account,
                    },
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
            manager_types::ResolveBindingResponse::SandboxAws {
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
                let preview_ports = binding
                    .preview_ports
                    .into_iter()
                    .map(|port| narrow_manager_number(port, "previewPorts", resource_id))
                    .collect::<Result<Vec<u16>>>()?;
                Self::SandboxAws {
                    binding: Box::new(alien_core::AwsSandboxBinding {
                        image_arn: alien_core::BindingValue::Value(binding.image_arn),
                        image_version: alien_core::BindingValue::Value(binding.image_version),
                        region: alien_core::BindingValue::Value(binding.region),
                        // The remote contract carries neither: the provider refuses an execution
                        // role outright, and reads egress from `allow_egress` against this list.
                        execution_role_arn: None,
                        egress_connector_arns: Vec::new(),
                        preview_ports,
                        idle_suspend_seconds: binding
                            .idle_suspend_seconds
                            .map(|seconds| {
                                narrow_manager_number(seconds, "idleSuspendSeconds", resource_id)
                            })
                            .transpose()?,
                        max_lifetime_seconds: binding
                            .max_lifetime_seconds
                            .map(|seconds| {
                                narrow_manager_number(seconds, "maxLifetimeSeconds", resource_id)
                            })
                            .transpose()?,
                        allow_egress: binding.allow_egress,
                    }),
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
        };
        Ok(lease)
    }
}

/// Narrows one of the manager schema's `i32` fields, refusing the whole lease when it does not
/// fit. A truncated preview port would mint ingress for a port the declaration never named.
fn narrow_manager_number<T: TryFrom<i32>>(value: i32, field: &str, resource_id: &str) -> Result<T> {
    T::try_from(value).map_err(|_| {
        AlienError::new(ErrorData::RemoteAccessFailed {
            operation: format!(
                "read {field} value {value} for remote binding '{resource_id}': out of range"
            ),
        })
    })
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
            operation: format!("parse {field} for remote binding '{resource_id}'"),
        })
        .map(|expires_at| expires_at.with_timezone(&Utc))
}
