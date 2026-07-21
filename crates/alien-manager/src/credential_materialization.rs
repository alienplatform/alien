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
use crate::traits::RemoteStorageCredentialSource;

const GCP_CLOUD_PLATFORM_SCOPE: &str = "https://www.googleapis.com/auth/cloud-platform";
pub(crate) const AZURE_STORAGE_SCOPE: &str = "https://storage.azure.com/.default";
pub(crate) const AZURE_REMOTE_STORAGE_PERMISSIONS: &str = "rcwdl";
const GCP_REMOTE_STORAGE_ROLE: &str = "roles/storage.objectAdmin";
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
    AwsS3 {
        bucket_name: String,
    },
    GcpGcs {
        bucket_name: String,
    },
    AzureBlob {
        account_name: String,
        container_name: String,
    },
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
    match (source, scope) {
        (
            RemoteStorageCredentialSource::Direct(ClientConfig::Aws(config)),
            RemoteStorageCredentialScope::AwsS3 { bucket_name },
        ) => {
            let policy = aws_s3_session_policy(&bucket_name)?;
            let config = config
                .materialize_web_identity_session_with_policy(
                    &format!("alien-remote-storage-{}", uuid::Uuid::new_v4().simple()),
                    REMOTE_STORAGE_DURATION_SECONDS,
                    &policy,
                )
                .await
                .context(ErrorData::CredentialMaterializationFailed {
                    platform: Platform::Aws,
                    purpose: format!("remote Storage bucket '{bucket_name}'"),
                })?;
            aws_remote_storage_lease(config)
        }
        (
            RemoteStorageCredentialSource::AwsAssumeRole {
                source,
                role_arn,
                role_session_name,
                target_account_id,
                target_region,
            },
            RemoteStorageCredentialScope::AwsS3 { bucket_name },
        ) => {
            if !role_arn.starts_with(&format!("arn:aws:iam::{target_account_id}:role/")) {
                return Err(ErrorData::internal(
                    "AWS remote Storage target role does not match the deployment account",
                ));
            }
            let policy = aws_s3_session_policy(&bucket_name)?;
            let config = source
                .assume_role_with_session_policy(
                    &role_arn,
                    &role_session_name,
                    REMOTE_STORAGE_DURATION_SECONDS,
                    &policy,
                    &target_account_id,
                    &target_region,
                )
                .await
                .context(ErrorData::CredentialMaterializationFailed {
                    platform: Platform::Aws,
                    purpose: format!("remote Storage bucket '{bucket_name}'"),
                })?;
            aws_remote_storage_lease(config)
        }
        (
            RemoteStorageCredentialSource::Direct(ClientConfig::Gcp(config)),
            RemoteStorageCredentialScope::GcpGcs { bucket_name },
        ) => {
            let token = config
                .downscope_access_token_for_bucket(&bucket_name, GCP_REMOTE_STORAGE_ROLE)
                .await
                .context(ErrorData::CredentialMaterializationFailed {
                    platform: Platform::Gcp,
                    purpose: format!("remote Storage bucket '{bucket_name}'"),
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
        (
            RemoteStorageCredentialSource::Direct(ClientConfig::Azure(config)),
            RemoteStorageCredentialScope::AzureBlob {
                account_name,
                container_name,
            },
        ) => {
            if matches!(&config.credentials, AzureCredentials::AccessToken { .. }) {
                return Err(ErrorData::internal(
                    "Remote Azure Storage requires an exact storage-audience token source",
                ));
            }
            let desired_expiry =
                Utc::now() + chrono::Duration::seconds(i64::from(REMOTE_STORAGE_DURATION_SECONDS));
            let sas = config
                .create_container_user_delegation_sas(
                    &account_name,
                    &container_name,
                    AZURE_REMOTE_STORAGE_PERMISSIONS,
                    desired_expiry,
                )
                .await
                .context(ErrorData::CredentialMaterializationFailed {
                    platform: Platform::Azure,
                    purpose: format!("remote Storage container '{account_name}/{container_name}'"),
                })?;
            let signed_expiry = sas
                .expires_at
                .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
            if sas.account_name != account_name
                || sas.container_name != container_name
                || sas.permissions != AZURE_REMOTE_STORAGE_PERMISSIONS
                || sas.query_parameters.get("sp") != Some(&sas.permissions)
                || sas.query_parameters.get("se") != Some(&signed_expiry)
                || sas.query_parameters.get("sr").map(String::as_str) != Some("c")
                || sas.query_parameters.get("spr").map(String::as_str) != Some("https")
            {
                return Err(ErrorData::internal(
                    "Azure returned a SAS that does not prove the requested container scope",
                ));
            }
            Ok(MaterializedCredentialLease {
                client_config: ClientConfig::Azure(Box::new(AzureClientConfig {
                    subscription_id: config.subscription_id,
                    tenant_id: config.tenant_id,
                    region: config.region,
                    credentials: AzureCredentials::SasToken {
                        query_parameters: sas.query_parameters,
                    },
                    service_overrides: None,
                })),
                expires_at: sas.expires_at,
            })
        }
        (source, scope) => Err(ErrorData::internal(format!(
            "Remote Storage credential source and scope do not match (source {source:?}, scope {})",
            remote_scope_platform(&scope)
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

fn aws_s3_session_policy(bucket_name: &str) -> Result<String, AlienError<ErrorData>> {
    let valid_bucket_name = (3..=63).contains(&bucket_name.len())
        && bucket_name.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b".-".contains(&byte)
        })
        && bucket_name
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
        && bucket_name
            .as_bytes()
            .last()
            .is_some_and(u8::is_ascii_alphanumeric)
        && !bucket_name.contains("..")
        && !bucket_name.contains(".-")
        && !bucket_name.contains("-.");
    if !valid_bucket_name {
        return Err(ErrorData::bad_request(
            "Remote S3 binding contains an invalid bucket name",
        ));
    }
    serde_json::to_string(&serde_json::json!({
        "Version": "2012-10-17",
        "Statement": [
            {
                "Sid": "RemoteStorageBucket",
                "Effect": "Allow",
                "Action": ["s3:ListBucket"],
                "Resource": [format!("arn:aws:s3:::{bucket_name}")]
            },
            {
                "Sid": "RemoteStorageObjects",
                "Effect": "Allow",
                "Action": [
                    "s3:GetObject",
                    "s3:PutObject",
                    "s3:DeleteObject",
                    "s3:AbortMultipartUpload"
                ],
                "Resource": [format!("arn:aws:s3:::{bucket_name}/*")]
            }
        ]
    }))
    .into_alien_error()
    .context(ErrorData::InternalError {
        message: "Failed to serialize the remote S3 session policy".to_string(),
    })
}

fn remote_scope_platform(scope: &RemoteStorageCredentialScope) -> Platform {
    match scope {
        RemoteStorageCredentialScope::AwsS3 { .. } => Platform::Aws,
        RemoteStorageCredentialScope::GcpGcs { .. } => Platform::Gcp,
        RemoteStorageCredentialScope::AzureBlob { .. } => Platform::Azure,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aws_remote_storage_policy_has_exact_bucket_and_object_resources() {
        let policy = aws_s3_session_policy("requested-bucket").expect("policy should serialize");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&policy).unwrap(),
            serde_json::json!({
                "Version": "2012-10-17",
                "Statement": [
                    {
                        "Sid": "RemoteStorageBucket",
                        "Effect": "Allow",
                        "Action": ["s3:ListBucket"],
                        "Resource": ["arn:aws:s3:::requested-bucket"]
                    },
                    {
                        "Sid": "RemoteStorageObjects",
                        "Effect": "Allow",
                        "Action": [
                            "s3:GetObject",
                            "s3:PutObject",
                            "s3:DeleteObject",
                            "s3:AbortMultipartUpload"
                        ],
                        "Resource": ["arn:aws:s3:::requested-bucket/*"]
                    }
                ]
            })
        );
    }

    #[test]
    fn aws_remote_storage_policy_rejects_wildcard_or_malformed_buckets() {
        assert!(aws_s3_session_policy("*").is_err());
        assert!(aws_s3_session_policy("bucket/*").is_err());
        assert!(aws_s3_session_policy("bucket..name").is_err());
        assert!(aws_s3_session_policy("valid-bucket-123").is_ok());
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

        let error = materialize_remote_storage_lease(
            RemoteStorageCredentialSource::Direct(config),
            RemoteStorageCredentialScope::GcpGcs {
                bucket_name: "bucket".to_string(),
            },
        )
        .await
        .expect_err("opaque token has no authoritative expiry");
        assert!(!error.retryable);
    }

    #[tokio::test]
    async fn remote_azure_storage_rejects_unscoped_access_token_before_network() {
        let config = ClientConfig::Azure(Box::new(AzureClientConfig {
            subscription_id: "subscription".to_string(),
            tenant_id: "tenant".to_string(),
            region: Some("eastus".to_string()),
            credentials: AzureCredentials::AccessToken {
                token: "generic-management-token".to_string(),
            },
            service_overrides: None,
        }));

        let error = materialize_remote_storage_lease(
            RemoteStorageCredentialSource::Direct(config),
            RemoteStorageCredentialScope::AzureBlob {
                account_name: "account".to_string(),
                container_name: "container".to_string(),
            },
        )
        .await
        .expect_err("generic Azure access token must fail closed");
        assert_eq!(error.code, "INTERNAL_ERROR");
    }
}
