//! Remote resource-binding resolution.
//!
//! The request names only a deployment and a logical resource. The manager
//! validates the authoritative stack state before it releases the resource's
//! binding topology together with materialized, short-lived credentials.

use alien_core::{
    Ai, AiBinding, AwsClientConfig, AwsCredentials, AzureClientConfig, AzureCredentials,
    BindingValue, ClientConfig, DeploymentStatus, GcpClientConfig, GcpCredentials, Key, KeyBinding,
    Platform, ResourceLifecycle, ResourceStatus, Sandbox, SandboxBinding, Storage, StorageBinding,
};
use alien_error::{Context, ContextError, IntoAlienError};
use axum::{
    extract::{Json, State},
    http::{header::CACHE_CONTROL, header::PRAGMA, HeaderMap},
    response::{IntoResponse, Response},
    routing::post,
    Router,
};
use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};

use super::{auth, current_release_resource, load_current_release, AppState};
use crate::credential_materialization::{
    materialize_remote_binding_lease, MaterializedCredentialLease, RemoteBindingCredentialScope,
};
use crate::error::ErrorData;
use crate::traits::{deployment_status_from_record, DeploymentRecord, ReleaseStore};

/// The remote client refreshes five minutes before this server-provided hint.
/// One hour matches the maximum supported lifetime for manager-minted cloud credentials.
const REMOTE_BINDING_REFRESH_HINT_SECONDS: i64 = 3600;

/// Request body for `POST /v1/bindings/resolve`.
#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResolveBindingRequest {
    /// Deployment containing the remote-enabled resource.
    pub deployment_id: String,
    /// Logical remote-enabled resource id in the deployment's stack state.
    pub resource_id: Option<String>,
    /// Deployment-level binding selector. V1 supports the unique managed AI resource.
    pub kind: Option<ResolveBindingKind>,
}

#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "lowercase")]
pub enum ResolveBindingKind {
    Ai,
}

/// One approved remote Storage binding paired with credentials for the same
/// provider. The discriminant makes cross-provider combinations impossible.
#[derive(Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(tag = "service", rename_all = "lowercase")]
pub enum ResolveBindingResponse {
    /// AWS S3 and an AWS session.
    S3 {
        binding: RemoteS3StorageBinding,
        #[serde(rename = "clientConfig")]
        client_config: RemoteAwsClientConfig,
        #[serde(rename = "expiresAt")]
        expires_at: String,
    },
    /// Azure Blob Storage and a storage-audience access token.
    Blob {
        binding: RemoteBlobStorageBinding,
        #[serde(rename = "clientConfig")]
        client_config: RemoteAzureClientConfig,
        #[serde(rename = "expiresAt")]
        expires_at: String,
    },
    /// Google Cloud Storage and a Remote Bindings identity access token.
    Gcs {
        binding: RemoteGcsStorageBinding,
        #[serde(rename = "clientConfig")]
        client_config: RemoteGcpClientConfig,
        #[serde(rename = "expiresAt")]
        expires_at: String,
    },
    /// AWS KMS key and an AWS session.
    #[serde(rename = "kms")]
    Kms {
        binding: RemoteAwsKmsKeyBinding,
        #[serde(rename = "clientConfig")]
        client_config: RemoteAwsClientConfig,
        #[serde(rename = "expiresAt")]
        expires_at: String,
    },
    /// GCP Cloud KMS key and an access token.
    #[serde(rename = "cloud-kms")]
    CloudKms {
        binding: RemoteGcpCloudKmsKeyBinding,
        #[serde(rename = "clientConfig")]
        client_config: RemoteGcpClientConfig,
        #[serde(rename = "expiresAt")]
        expires_at: String,
    },
    /// Azure Key Vault key and a vault-audience access token.
    #[serde(rename = "key-vault-key")]
    KeyVaultKey {
        binding: RemoteAzureKeyVaultKeyBinding,
        #[serde(rename = "clientConfig")]
        client_config: RemoteAzureClientConfig,
        #[serde(rename = "expiresAt")]
        expires_at: String,
    },
    /// AWS Bedrock and an AWS session.
    Bedrock {
        #[serde(rename = "resourceId")]
        resource_id: String,
        binding: RemoteAwsBedrockAiBinding,
        #[serde(rename = "clientConfig")]
        client_config: RemoteAwsClientConfig,
        #[serde(rename = "expiresAt")]
        expires_at: String,
    },
    /// GCP Vertex AI and an access token.
    Vertex {
        #[serde(rename = "resourceId")]
        resource_id: String,
        binding: RemoteGcpVertexAiBinding,
        #[serde(rename = "clientConfig")]
        client_config: RemoteGcpClientConfig,
        #[serde(rename = "expiresAt")]
        expires_at: String,
    },
    /// Azure AI Foundry and a Cognitive Services access token.
    Foundry {
        #[serde(rename = "resourceId")]
        resource_id: String,
        binding: RemoteAzureFoundryAiBinding,
        #[serde(rename = "clientConfig")]
        client_config: RemoteAzureClientConfig,
        #[serde(rename = "expiresAt")]
        expires_at: String,
    },
    /// AWS Lambda MicroVM sandbox and an AWS session.
    #[serde(rename = "sandbox-aws")]
    SandboxAws {
        binding: RemoteAwsSandboxBinding,
        #[serde(rename = "clientConfig")]
        client_config: RemoteAwsClientConfig,
        #[serde(rename = "expiresAt")]
        expires_at: String,
    },
}

/// Concrete MicroVM sandbox topology returned to remote clients.
///
/// Deliberately without the execution role and the egress connectors of the in-cloud binding: the
/// provider passes no role, and a binding carrying connectors is refused before it reaches here.
#[derive(Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemoteAwsSandboxBinding {
    /// MicroVM image the credential lease authorizes sessions against.
    pub image_arn: String,
    /// Image version sessions are enumerated by together with the image.
    pub image_version: String,
    /// Region the MicroVMs run in.
    pub region: String,
    /// Ports a session capability may be minted for.
    pub preview_ports: Vec<u16>,
    /// Idle seconds after which a session suspends, where the declaration asked for one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idle_suspend_seconds: Option<u32>,
    /// Wall-clock ceiling on a session, where the declaration asked for one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_lifetime_seconds: Option<u32>,
    /// Whether the declaration asked for open egress. Always true here, and sent rather than
    /// implied so the client's own fail-open check on an empty connector list still has an answer.
    pub allow_egress: bool,
}

#[derive(Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemoteAwsBedrockAiBinding {
    pub region: String,
}

#[derive(Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemoteGcpVertexAiBinding {
    pub project: String,
    pub location: String,
}

#[derive(Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemoteAzureFoundryAiBinding {
    pub endpoint: String,
    pub account: String,
}

#[derive(Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemoteAwsKmsKeyBinding {
    pub key_arn: String,
    pub region: Option<String>,
}

#[derive(Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemoteGcpCloudKmsKeyBinding {
    pub crypto_key_name: String,
}

#[derive(Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemoteAzureKeyVaultKeyBinding {
    pub key_id: String,
}

/// Concrete S3 topology returned to remote clients.
#[derive(Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemoteS3StorageBinding {
    /// S3 bucket name authorized by the credential lease.
    pub bucket_name: String,
}

/// Concrete Google Cloud Storage topology returned to remote clients.
#[derive(Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemoteGcsStorageBinding {
    /// GCS bucket name authorized by the credential lease.
    pub bucket_name: String,
}

/// Concrete Azure Blob Storage topology returned to remote clients.
#[derive(Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemoteBlobStorageBinding {
    /// Storage account containing the authorized container.
    pub account_name: String,
    /// Blob container authorized by the credential lease.
    pub container_name: String,
}

/// Response-safe AWS client configuration. The public contract deliberately
/// has no static, profile, metadata, or web-identity credential variants.
#[derive(Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemoteAwsClientConfig {
    /// AWS account containing the bucket.
    pub account_id: String,
    /// AWS region containing the bucket.
    pub region: String,
    /// Expiring AWS session credentials.
    pub credentials: RemoteAwsCredentials,
}

/// The only AWS credential form remote binding resolution can return.
#[derive(Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "type"
)]
pub enum RemoteAwsCredentials {
    /// Temporary AWS session credentials with an authoritative expiry.
    SessionCredentials {
        /// AWS access key id.
        #[serde(rename = "accessKeyId")]
        #[cfg_attr(feature = "openapi", schema(rename = "accessKeyId"))]
        access_key_id: String,
        /// AWS secret access key.
        #[serde(rename = "secretAccessKey")]
        #[cfg_attr(feature = "openapi", schema(rename = "secretAccessKey"))]
        secret_access_key: String,
        /// AWS session token.
        #[serde(rename = "sessionToken")]
        #[cfg_attr(feature = "openapi", schema(rename = "sessionToken"))]
        session_token: String,
        /// Provider-reported credential expiry.
        #[serde(rename = "expiresAt")]
        #[cfg_attr(feature = "openapi", schema(rename = "expiresAt"))]
        expires_at: String,
    },
}

/// Response-safe GCP client configuration. Refreshable source credentials and
/// service endpoint overrides cannot be represented by this type.
#[derive(Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemoteGcpClientConfig {
    /// GCP project containing the bucket.
    pub project_id: String,
    /// GCP region configured for the deployment.
    pub region: String,
    /// Already-minted OAuth access token.
    pub credentials: RemoteGcpCredentials,
    /// Numeric GCP project id, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_number: Option<String>,
}

/// The only GCP credential form remote binding resolution can return.
#[derive(Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum RemoteGcpCredentials {
    /// Short-lived OAuth access token. Its expiry is the response `expiresAt`.
    AccessToken {
        /// OAuth bearer token.
        token: String,
    },
}

/// Response-safe Azure client configuration containing one storage-audience
/// access token for the stack's Remote Bindings identity.
#[derive(Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemoteAzureClientConfig {
    /// Azure subscription containing the storage account.
    pub subscription_id: String,
    /// Azure tenant owning the identity.
    pub tenant_id: String,
    /// Azure region configured for the deployment.
    pub region: Option<String>,
    /// A short-lived Azure Storage access token.
    pub credentials: RemoteAzureCredentials,
}

/// The only Azure credential form remote binding resolution can return.
#[derive(Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum RemoteAzureCredentials {
    /// OAuth bearer token for `https://storage.azure.com/.default`.
    AccessToken { token: String },
}

/// Storage binding variants supported by the first hosted remote-bindings release.
#[derive(Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(tag = "service", rename_all = "lowercase")]
pub enum RemoteStorageBinding {
    /// AWS S3.
    S3(RemoteS3StorageBinding),
    /// Azure Blob Storage.
    Blob(RemoteBlobStorageBinding),
    /// Google Cloud Storage.
    Gcs(RemoteGcsStorageBinding),
}

enum RemoteKeyBinding {
    Aws(RemoteAwsKmsKeyBinding),
    Gcp(RemoteGcpCloudKmsKeyBinding),
    Azure(RemoteAzureKeyVaultKeyBinding),
}

enum RemoteAiBinding {
    Aws(RemoteAwsBedrockAiBinding),
    Gcp(RemoteGcpVertexAiBinding),
    Azure(RemoteAzureFoundryAiBinding),
}

enum RemoteSandboxBinding {
    Aws(RemoteAwsSandboxBinding),
}

enum ResolvedRemoteBinding {
    Storage(RemoteStorageBinding),
    Key(RemoteKeyBinding),
    Ai(RemoteAiBinding),
    Sandbox(RemoteSandboxBinding),
}

impl ResolvedRemoteBinding {
    fn credential_scope(&self) -> RemoteBindingCredentialScope {
        match self {
            Self::Storage(binding) => binding.credential_scope(),
            Self::Key(binding) => binding.credential_scope(),
            Self::Ai(binding) => binding.credential_scope(),
            Self::Sandbox(binding) => binding.credential_scope(),
        }
    }
}

impl RemoteSandboxBinding {
    fn credential_scope(&self) -> RemoteBindingCredentialScope {
        match self {
            Self::Aws(_) => RemoteBindingCredentialScope::AwsSandbox,
        }
    }
}

impl RemoteAiBinding {
    fn credential_scope(&self) -> RemoteBindingCredentialScope {
        match self {
            Self::Aws(_) => RemoteBindingCredentialScope::AwsAi,
            Self::Gcp(_) => RemoteBindingCredentialScope::GcpAi,
            Self::Azure(_) => RemoteBindingCredentialScope::AzureAi,
        }
    }
}

impl RemoteKeyBinding {
    fn credential_scope(&self) -> RemoteBindingCredentialScope {
        match self {
            Self::Aws(_) => RemoteBindingCredentialScope::AwsKms,
            Self::Gcp(_) => RemoteBindingCredentialScope::GcpCloudKms,
            Self::Azure(_) => RemoteBindingCredentialScope::AzureKeyVault,
        }
    }
}

impl RemoteStorageBinding {
    fn credential_scope(&self) -> RemoteBindingCredentialScope {
        match self {
            Self::S3(_) => RemoteBindingCredentialScope::AwsS3,
            Self::Gcs(_) => RemoteBindingCredentialScope::GcpGcs,
            Self::Blob(_) => RemoteBindingCredentialScope::AzureBlob,
        }
    }
}

/// Manual `Debug`: both the binding payload and client configuration can carry
/// sensitive service details or credential material and must never reach logs.
impl std::fmt::Debug for ResolveBindingResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResolveBindingResponse")
            .field("lease", &"<redacted>")
            .finish()
    }
}

impl TryFrom<AwsClientConfig> for RemoteAwsClientConfig {
    type Error = alien_error::AlienError<ErrorData>;

    fn try_from(config: AwsClientConfig) -> Result<Self, Self::Error> {
        if config.service_overrides.is_some() {
            return Err(ErrorData::internal(
                "Remote Bindings AWS response contains service endpoint overrides",
            ));
        }
        let AwsCredentials::SessionCredentials {
            access_key_id,
            secret_access_key,
            session_token,
            expires_at,
        } = config.credentials
        else {
            return Err(ErrorData::internal(
                "Remote Bindings AWS response credentials are not a short-lived session",
            ));
        };

        Ok(Self {
            account_id: config.account_id,
            region: config.region,
            credentials: RemoteAwsCredentials::SessionCredentials {
                access_key_id,
                secret_access_key,
                session_token,
                expires_at,
            },
        })
    }
}

impl TryFrom<GcpClientConfig> for RemoteGcpClientConfig {
    type Error = alien_error::AlienError<ErrorData>;

    fn try_from(config: GcpClientConfig) -> Result<Self, Self::Error> {
        if config.service_overrides.is_some() {
            return Err(ErrorData::internal(
                "Remote Bindings GCP response contains service endpoint overrides",
            ));
        }
        let GcpCredentials::AccessToken { token } = config.credentials else {
            return Err(ErrorData::internal(
                "Remote Bindings GCP response credentials are not a short-lived access token",
            ));
        };

        Ok(Self {
            project_id: config.project_id,
            region: config.region,
            credentials: RemoteGcpCredentials::AccessToken { token },
            project_number: config.project_number,
        })
    }
}

impl TryFrom<AzureClientConfig> for RemoteAzureClientConfig {
    type Error = alien_error::AlienError<ErrorData>;

    fn try_from(config: AzureClientConfig) -> Result<Self, Self::Error> {
        if config.service_overrides.is_some() {
            return Err(ErrorData::internal(
                "Remote Bindings Azure response contains service endpoint overrides",
            ));
        }
        let AzureCredentials::AccessToken { token } = config.credentials else {
            return Err(ErrorData::internal(
                "Remote Bindings Azure response credentials are not a short-lived access token",
            ));
        };

        Ok(Self {
            subscription_id: config.subscription_id,
            tenant_id: config.tenant_id,
            region: config.region,
            credentials: RemoteAzureCredentials::AccessToken { token },
        })
    }
}

fn concrete_binding_value(
    value: &BindingValue<String>,
    field: &str,
) -> Result<String, alien_error::AlienError<ErrorData>> {
    match value {
        BindingValue::Value(value) if !value.is_empty() => Ok(value.clone()),
        _ => Err(ErrorData::internal(format!(
            "Remote binding field '{field}' is not a concrete value"
        ))),
    }
}

impl ResolveBindingResponse {
    fn from_parts(
        binding: RemoteStorageBinding,
        lease: MaterializedCredentialLease,
        expires_at: String,
    ) -> Result<Self, alien_error::AlienError<ErrorData>> {
        match (binding, lease.client_config) {
            (RemoteStorageBinding::S3(binding), ClientConfig::Aws(client_config)) => Ok(Self::S3 {
                binding,
                client_config: (*client_config).try_into()?,
                expires_at,
            }),
            (RemoteStorageBinding::Blob(binding), ClientConfig::Azure(client_config)) => {
                Ok(Self::Blob {
                    client_config: (*client_config).try_into()?,
                    binding,
                    expires_at,
                })
            }
            (RemoteStorageBinding::Gcs(binding), ClientConfig::Gcp(client_config)) => {
                Ok(Self::Gcs {
                    binding,
                    client_config: (*client_config).try_into()?,
                    expires_at,
                })
            }
            _ => Err(ErrorData::internal(
                "Remote Storage binding and materialized credential platforms do not match",
            )),
        }
    }

    fn from_key_parts(
        binding: RemoteKeyBinding,
        lease: MaterializedCredentialLease,
        expires_at: String,
    ) -> Result<Self, alien_error::AlienError<ErrorData>> {
        match (binding, lease.client_config) {
            (RemoteKeyBinding::Aws(binding), ClientConfig::Aws(client_config)) => Ok(Self::Kms {
                binding,
                client_config: (*client_config).try_into()?,
                expires_at,
            }),
            (RemoteKeyBinding::Gcp(binding), ClientConfig::Gcp(client_config)) => {
                Ok(Self::CloudKms {
                    binding,
                    client_config: (*client_config).try_into()?,
                    expires_at,
                })
            }
            (RemoteKeyBinding::Azure(binding), ClientConfig::Azure(client_config)) => {
                Ok(Self::KeyVaultKey {
                    binding,
                    client_config: (*client_config).try_into()?,
                    expires_at,
                })
            }
            _ => Err(ErrorData::internal(
                "Remote Key binding and materialized credential platforms do not match",
            )),
        }
    }

    fn from_ai_parts(
        resource_id: String,
        binding: RemoteAiBinding,
        lease: MaterializedCredentialLease,
        expires_at: String,
    ) -> Result<Self, alien_error::AlienError<ErrorData>> {
        match (binding, lease.client_config) {
            (RemoteAiBinding::Aws(binding), ClientConfig::Aws(client_config)) => {
                Ok(Self::Bedrock {
                    resource_id,
                    binding,
                    client_config: (*client_config).try_into()?,
                    expires_at,
                })
            }
            (RemoteAiBinding::Gcp(binding), ClientConfig::Gcp(client_config)) => Ok(Self::Vertex {
                resource_id,
                binding,
                client_config: (*client_config).try_into()?,
                expires_at,
            }),
            (RemoteAiBinding::Azure(binding), ClientConfig::Azure(client_config)) => {
                Ok(Self::Foundry {
                    resource_id,
                    binding,
                    client_config: (*client_config).try_into()?,
                    expires_at,
                })
            }
            _ => Err(ErrorData::internal(
                "Remote AI binding and materialized credential platforms do not match",
            )),
        }
    }

    fn from_sandbox_parts(
        binding: RemoteSandboxBinding,
        lease: MaterializedCredentialLease,
        expires_at: String,
    ) -> Result<Self, alien_error::AlienError<ErrorData>> {
        match (binding, lease.client_config) {
            (RemoteSandboxBinding::Aws(binding), ClientConfig::Aws(client_config)) => {
                Ok(Self::SandboxAws {
                    binding,
                    client_config: (*client_config).try_into()?,
                    expires_at,
                })
            }
            _ => Err(ErrorData::internal(
                "Remote Sandbox binding and materialized credential platforms do not match",
            )),
        }
    }
}

pub fn router() -> Router<AppState> {
    Router::new().route("/v1/bindings/resolve", post(resolve_binding))
}

// Keep transient errors out of OpenAPI. Progenitor only supports one error
// response type per operation, while its typed payload parse failure drops the
// HTTP status. Leaving 408/425/429/5xx on the unexpected-response path preserves
// retryability and lets callers use still-valid cached credentials.
#[cfg_attr(feature = "openapi", utoipa::path(
    post,
    path = "/v1/bindings/resolve",
    tag = "bindings",
    request_body = ResolveBindingRequest,
    responses(
        (status = 200, description = "Remote binding resolved successfully", body = ResolveBindingResponse),
        (status = 400, description = "The deployment, release, or binding is not eligible for remote access", body = alien_error::AlienError),
        (status = 401, description = "Authentication is required", body = alien_error::AlienError),
        (status = 403, description = "The caller cannot resolve bindings for this deployment", body = alien_error::AlienError),
        (status = 404, description = "The deployment, release, or binding was not found", body = alien_error::AlienError)
    ),
    security(
        ("bearer" = [])
    )
))]
async fn resolve_binding(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<ResolveBindingRequest>,
) -> Response {
    let subject = match auth::require_auth(&state, &headers).await {
        Ok(subject) => subject,
        Err(error) => return error.into_response(),
    };
    let deployment = match state
        .deployment_store
        .get_deployment(&subject, &request.deployment_id)
        .await
    {
        Ok(Some(deployment)) => deployment,
        Ok(None) => return ErrorData::not_found_deployment(&request.deployment_id).into_response(),
        Err(error) => return error.into_response(),
    };
    let resource_id = match (&request.resource_id, &request.kind) {
        (Some(resource_id), None) => resource_id.clone(),
        (None, Some(ResolveBindingKind::Ai)) => {
            match unique_current_release_remote_ai(state.release_store.as_ref(), &deployment).await
            {
                Ok(resource_id) => resource_id,
                Err(error) => return error.into_response(),
            }
        }
        _ => {
            return ErrorData::bad_request(
                "Specify exactly one of resourceId or kind when resolving a remote binding",
            )
            .into_response()
        }
    };
    if !state
        .authz
        .can_resolve_remote_bindings(&subject, &deployment)
    {
        return ErrorData::forbidden("Cannot resolve remote bindings for this deployment")
            .into_response();
    }

    if !deployment_status_allows_remote_bindings(deployment_status_from_record(&deployment.status))
    {
        return ErrorData::bad_request(format!(
            "Deployment is not operational for remote bindings (status '{}')",
            deployment.status
        ))
        .into_response();
    }

    let binding_kind = match require_current_release_remote_access(
        state.release_store.as_ref(),
        &deployment,
        &resource_id,
    )
    .await
    {
        Ok(kind) => kind,
        Err(error) => return error.into_response(),
    };

    if let Err(error) = require_setup_owned_remote_binding(&deployment, &resource_id) {
        return error.into_response();
    }

    let binding = match binding_kind {
        alien_core::remote_bindings::RemoteBindingKind::Storage => {
            remote_storage_binding(&deployment, &resource_id).map(ResolvedRemoteBinding::Storage)
        }
        alien_core::remote_bindings::RemoteBindingKind::Key => {
            remote_key_binding(&deployment, &resource_id).map(ResolvedRemoteBinding::Key)
        }
        alien_core::remote_bindings::RemoteBindingKind::Ai => {
            remote_ai_binding(&deployment, &resource_id).map(ResolvedRemoteBinding::Ai)
        }
        alien_core::remote_bindings::RemoteBindingKind::Sandbox => {
            remote_sandbox_binding(&deployment, &resource_id).map(ResolvedRemoteBinding::Sandbox)
        }
    };
    let binding = match binding {
        Ok(binding) => binding,
        Err(error) => return error.into_response(),
    };

    let scope = binding.credential_scope();
    let resolved = match state
        .credential_resolver
        .resolve_remote_storage_source(&deployment, &resource_id)
        .await
    {
        Ok(source) => source,
        Err(error) => {
            return error
                .context(ErrorData::RemoteCredentialHandoffFailed {
                    deployment_id: deployment.id.clone(),
                    platform: deployment.platform,
                })
                .into_response()
        }
    };
    let lease = match materialize_remote_binding_lease(resolved, scope).await {
        Ok(materialized) => materialized,
        Err(error) => return error.into_response(),
    };

    let now = Utc::now();
    let expires_at = match remote_binding_expiry(lease.expires_at, now) {
        Ok(expires_at) => expires_at.to_rfc3339_opts(SecondsFormat::Secs, true),
        Err(error) => return error.into_response(),
    };

    let response = match binding {
        ResolvedRemoteBinding::Storage(binding) => {
            ResolveBindingResponse::from_parts(binding, lease, expires_at.clone())
        }
        ResolvedRemoteBinding::Key(binding) => {
            ResolveBindingResponse::from_key_parts(binding, lease, expires_at.clone())
        }
        ResolvedRemoteBinding::Ai(binding) => ResolveBindingResponse::from_ai_parts(
            resource_id.clone(),
            binding,
            lease,
            expires_at.clone(),
        ),
        ResolvedRemoteBinding::Sandbox(binding) => {
            ResolveBindingResponse::from_sandbox_parts(binding, lease, expires_at.clone())
        }
    };
    let response = match response {
        Ok(response) => response,
        Err(error) => return error.into_response(),
    };

    tracing::info!(
        event = "remote_binding_credentials_issued",
        deployment_id = %request.deployment_id,
        resource_id = %resource_id,
        platform = %deployment.platform,
        expires_at = %expires_at,
        "Issued remote binding credentials"
    );

    (
        [(CACHE_CONTROL, "no-store"), (PRAGMA, "no-cache")],
        Json(response),
    )
        .into_response()
}

async fn unique_current_release_remote_ai(
    release_store: &dyn ReleaseStore,
    deployment: &DeploymentRecord,
) -> Result<String, alien_error::AlienError<ErrorData>> {
    let release_id = deployment.current_release_id.as_deref().ok_or_else(|| {
        ErrorData::bad_request("Deployment has no current release; remote AI cannot be resolved")
    })?;
    let release = load_current_release(
        release_store,
        deployment,
        release_id,
        "remote AI resolution",
    )
    .await?;
    let stack = release.stacks.get(&deployment.platform).ok_or_else(|| {
        ErrorData::internal(format!(
            "Current release '{release_id}' has no {} stack",
            deployment.platform
        ))
    })?;
    let candidates = stack
        .resources
        .iter()
        .filter(|(_, entry)| {
            entry.config.resource_type() == Ai::RESOURCE_TYPE
                && entry.lifecycle == ResourceLifecycle::Frozen
                && entry.remote_access
        })
        .map(|(resource_id, _)| resource_id)
        .collect::<Vec<_>>();
    match candidates.as_slice() {
        [resource_id] => Ok((*resource_id).clone()),
        [] => Err(ErrorData::bad_request(
            "Deployment has no Frozen AI resource enabled for remote access",
        )),
        _ => Err(ErrorData::bad_request(
            "Deployment has more than one Frozen AI resource enabled for remote access",
        )),
    }
}

/// External bindings import caller-supplied resource references; they do not
/// prove that generated setup created the resource. Remote Bindings v0 must
/// therefore reject them even if stale synchronized state contains binding
/// parameters from an older manager.
fn require_setup_owned_remote_binding(
    deployment: &DeploymentRecord,
    resource_id: &str,
) -> Result<(), alien_error::AlienError<ErrorData>> {
    let in_deployment_config = deployment
        .deployment_config
        .as_ref()
        .is_some_and(|config| config.external_bindings.has(resource_id));
    let in_stack_settings = deployment
        .stack_settings
        .as_ref()
        .and_then(|settings| settings.external_bindings.as_ref())
        .is_some_and(|bindings| bindings.has(resource_id));
    if in_deployment_config || in_stack_settings {
        return Err(ErrorData::bad_request(format!(
            "Remote resource '{resource_id}' cannot use an external binding; remote access is limited to resources created by setup"
        )));
    }
    Ok(())
}

fn deployment_status_allows_remote_bindings(status: Option<DeploymentStatus>) -> bool {
    match status {
        Some(
            DeploymentStatus::Running
            | DeploymentStatus::RefreshFailed
            | DeploymentStatus::UpdatePending
            | DeploymentStatus::Updating
            | DeploymentStatus::UpdateFailed,
        ) => true,
        Some(
            DeploymentStatus::Pending
            | DeploymentStatus::PreflightsFailed
            | DeploymentStatus::InitialSetup
            | DeploymentStatus::InitialSetupFailed
            | DeploymentStatus::Provisioning
            | DeploymentStatus::WaitingForMachines
            | DeploymentStatus::ProvisioningFailed
            | DeploymentStatus::DeletePending
            | DeploymentStatus::Deleting
            | DeploymentStatus::DeleteFailed
            | DeploymentStatus::TeardownRequired
            | DeploymentStatus::TeardownFailed
            | DeploymentStatus::Deleted
            | DeploymentStatus::Error,
        )
        | None => false,
    }
}

fn remote_binding_expiry(
    provider_expires_at: DateTime<Utc>,
    now: DateTime<Utc>,
) -> Result<DateTime<Utc>, alien_error::AlienError<ErrorData>> {
    let maximum = now + chrono::Duration::seconds(REMOTE_BINDING_REFRESH_HINT_SECONDS);
    let expires_at = provider_expires_at.min(maximum);

    if expires_at <= now {
        return Err(ErrorData::internal(
            "Remote binding credential lease is already expired",
        ));
    }

    Ok(expires_at)
}

/// Require remote access in the user-authored current release before trusting
/// controller-published binding parameters in stack state.
///
/// Stack state can outlive a release update or come from an older manager that
/// did not clear `remote_binding_params`. The current release is therefore the
/// authoritative opt-in source. In particular, desired/prepared release data
/// must not grant access while an update is still in progress.
async fn require_current_release_remote_access(
    release_store: &dyn ReleaseStore,
    deployment: &DeploymentRecord,
    resource_id: &str,
) -> Result<alien_core::remote_bindings::RemoteBindingKind, alien_error::AlienError<ErrorData>> {
    let release_id = deployment.current_release_id.as_deref().ok_or_else(|| {
        ErrorData::bad_request(
            "Deployment has no current release; remote bindings cannot be resolved",
        )
    })?;

    let release = load_current_release(
        release_store,
        deployment,
        release_id,
        "remote binding resolution",
    )
    .await?;
    let (stack, resource) =
        current_release_resource(&release, deployment, release_id, resource_id)?;

    let definition =
        alien_core::remote_bindings::remote_binding_definition(&resource.config.resource_type())
            .ok_or_else(|| {
                ErrorData::bad_request(format!(
                    "Resource '{resource_id}' does not support Remote Bindings"
                ))
            })?;
    if resource.lifecycle != ResourceLifecycle::Frozen {
        return Err(ErrorData::bad_request(format!(
            "Remote resource '{resource_id}' is not Frozen in the deployment's current release"
        )));
    }
    if !resource.remote_access {
        return Err(ErrorData::bad_request(format!(
            "Resource '{resource_id}' is not enabled for remote access in the deployment's current release"
        )));
    }
    if definition.kind == alien_core::remote_bindings::RemoteBindingKind::Key
        && stack
            .resources
            .values()
            .filter(|entry| entry.remote_access)
            .count()
            != 1
    {
        return Err(ErrorData::bad_request(
            "A remotely published Key must be the deployment's only remoteAccess resource",
        ));
    }

    Ok(definition.kind)
}

fn remote_storage_binding(
    deployment: &DeploymentRecord,
    resource_id: &str,
) -> Result<RemoteStorageBinding, alien_error::AlienError<ErrorData>> {
    if !matches!(
        deployment.platform,
        Platform::Aws | Platform::Gcp | Platform::Azure
    ) {
        return Err(ErrorData::bad_request(format!(
            "Remote Storage is not supported for deployment platform '{}'",
            deployment.platform
        )));
    }
    let stack_state = deployment.stack_state.as_ref().ok_or_else(|| {
        ErrorData::bad_request("Deployment has no stack state (not yet provisioned)")
    })?;
    let resource = stack_state.resource(resource_id).ok_or_else(|| {
        ErrorData::bad_request(format!(
            "Resource '{resource_id}' does not exist in stack state"
        ))
    })?;
    if resource.resource_type != Storage::RESOURCE_TYPE.as_ref() {
        return Err(ErrorData::bad_request(format!(
            "Resource '{resource_id}' is not storage"
        )));
    }
    if resource.lifecycle != Some(ResourceLifecycle::Frozen) {
        return Err(ErrorData::bad_request(format!(
            "Storage resource '{resource_id}' is not Frozen"
        )));
    }
    if resource.status != ResourceStatus::Running {
        return Err(ErrorData::bad_request(format!(
            "Storage resource '{resource_id}' is not running"
        )));
    }
    let binding = resource.remote_binding_params.clone().ok_or_else(|| {
        ErrorData::bad_request(format!(
            "Storage resource '{resource_id}' is not enabled for remote access"
        ))
    })?;
    let binding: StorageBinding =
        serde_json::from_value(binding)
            .into_alien_error()
            .context(ErrorData::BadRequest {
                reason: format!("Storage resource '{resource_id}' has an invalid remote binding"),
            })?;
    match (deployment.platform, binding) {
        (Platform::Aws, StorageBinding::S3(binding)) => {
            Ok(RemoteStorageBinding::S3(RemoteS3StorageBinding {
                bucket_name: concrete_binding_value(&binding.bucket_name, "S3 bucketName")?,
            }))
        }
        (Platform::Gcp, StorageBinding::Gcs(binding)) => {
            Ok(RemoteStorageBinding::Gcs(RemoteGcsStorageBinding {
                bucket_name: concrete_binding_value(&binding.bucket_name, "GCS bucketName")?,
            }))
        }
        (Platform::Azure, StorageBinding::Blob(binding)) => {
            Ok(RemoteStorageBinding::Blob(RemoteBlobStorageBinding {
                account_name: concrete_binding_value(
                    &binding.account_name,
                    "Azure Blob Storage accountName",
                )?,
                container_name: concrete_binding_value(
                    &binding.container_name,
                    "Azure Blob Storage containerName",
                )?,
            }))
        }
        _ => Err(ErrorData::bad_request(format!(
            "Storage resource '{resource_id}' binding does not match deployment platform '{}'",
            deployment.platform
        ))),
    }
}

fn remote_key_binding(
    deployment: &DeploymentRecord,
    resource_id: &str,
) -> Result<RemoteKeyBinding, alien_error::AlienError<ErrorData>> {
    if !matches!(
        deployment.platform,
        Platform::Aws | Platform::Gcp | Platform::Azure
    ) {
        return Err(ErrorData::bad_request(format!(
            "Remote Key is not supported for deployment platform '{}'",
            deployment.platform
        )));
    }
    let stack_state = deployment.stack_state.as_ref().ok_or_else(|| {
        ErrorData::bad_request("Deployment has no stack state (not yet provisioned)")
    })?;
    let resource = stack_state.resource(resource_id).ok_or_else(|| {
        ErrorData::bad_request(format!(
            "Resource '{resource_id}' does not exist in stack state"
        ))
    })?;
    if resource.resource_type != Key::RESOURCE_TYPE.as_ref() {
        return Err(ErrorData::bad_request(format!(
            "Resource '{resource_id}' is not a key"
        )));
    }
    if resource.lifecycle != Some(ResourceLifecycle::Frozen) {
        return Err(ErrorData::bad_request(format!(
            "Key resource '{resource_id}' is not Frozen"
        )));
    }
    if resource.status != ResourceStatus::Running {
        return Err(ErrorData::bad_request(format!(
            "Key resource '{resource_id}' is not running"
        )));
    }
    let binding = resource.remote_binding_params.clone().ok_or_else(|| {
        ErrorData::bad_request(format!(
            "Key resource '{resource_id}' is not enabled for remote access"
        ))
    })?;
    let binding: KeyBinding =
        serde_json::from_value(binding)
            .into_alien_error()
            .context(ErrorData::BadRequest {
                reason: format!("Key resource '{resource_id}' has an invalid remote binding"),
            })?;
    match (deployment.platform, binding) {
        (Platform::Aws, KeyBinding::AwsKms(binding)) => {
            Ok(RemoteKeyBinding::Aws(RemoteAwsKmsKeyBinding {
                key_arn: concrete_binding_value(&binding.key_arn, "AWS KMS keyArn")?,
                region: binding
                    .region
                    .as_ref()
                    .map(|region| concrete_binding_value(region, "AWS KMS region"))
                    .transpose()?,
            }))
        }
        (Platform::Gcp, KeyBinding::GcpCloudKms(binding)) => {
            Ok(RemoteKeyBinding::Gcp(RemoteGcpCloudKmsKeyBinding {
                crypto_key_name: concrete_binding_value(
                    &binding.crypto_key_name,
                    "GCP Cloud KMS cryptoKeyName",
                )?,
            }))
        }
        (Platform::Azure, KeyBinding::AzureKeyVault(binding)) => {
            Ok(RemoteKeyBinding::Azure(RemoteAzureKeyVaultKeyBinding {
                key_id: concrete_binding_value(&binding.key_id, "Azure Key Vault keyId")?,
            }))
        }
        _ => Err(ErrorData::bad_request(format!(
            "Key resource '{resource_id}' binding does not match deployment platform '{}'",
            deployment.platform
        ))),
    }
}

fn remote_ai_binding(
    deployment: &DeploymentRecord,
    resource_id: &str,
) -> Result<RemoteAiBinding, alien_error::AlienError<ErrorData>> {
    if !matches!(
        deployment.platform,
        Platform::Aws | Platform::Gcp | Platform::Azure
    ) {
        return Err(ErrorData::bad_request(format!(
            "Remote AI is not supported for deployment platform '{}'",
            deployment.platform
        )));
    }
    let stack_state = deployment.stack_state.as_ref().ok_or_else(|| {
        ErrorData::bad_request("Deployment has no stack state (not yet provisioned)")
    })?;
    let resource = stack_state.resource(resource_id).ok_or_else(|| {
        ErrorData::bad_request(format!(
            "Resource '{resource_id}' does not exist in stack state"
        ))
    })?;
    if resource.resource_type != Ai::RESOURCE_TYPE.as_ref() {
        return Err(ErrorData::bad_request(format!(
            "Resource '{resource_id}' is not AI"
        )));
    }
    if resource.lifecycle != Some(ResourceLifecycle::Frozen) {
        return Err(ErrorData::bad_request(format!(
            "AI resource '{resource_id}' is not Frozen"
        )));
    }
    if resource.status != ResourceStatus::Running {
        return Err(ErrorData::bad_request(format!(
            "AI resource '{resource_id}' is not running"
        )));
    }
    let binding = resource.remote_binding_params.clone().ok_or_else(|| {
        ErrorData::bad_request(format!(
            "AI resource '{resource_id}' is not enabled for remote access"
        ))
    })?;
    let binding: AiBinding =
        serde_json::from_value(binding)
            .into_alien_error()
            .context(ErrorData::BadRequest {
                reason: format!("AI resource '{resource_id}' has an invalid remote binding"),
            })?;

    match (deployment.platform, binding) {
        (Platform::Aws, AiBinding::Bedrock(binding)) => {
            Ok(RemoteAiBinding::Aws(RemoteAwsBedrockAiBinding {
                region: binding.region,
            }))
        }
        (Platform::Gcp, AiBinding::Vertex(binding)) => {
            Ok(RemoteAiBinding::Gcp(RemoteGcpVertexAiBinding {
                project: binding.project,
                location: binding.location,
            }))
        }
        (Platform::Azure, AiBinding::Foundry(binding)) => {
            Ok(RemoteAiBinding::Azure(RemoteAzureFoundryAiBinding {
                endpoint: binding.endpoint,
                account: binding.account,
            }))
        }
        (_, AiBinding::External(_)) => Err(ErrorData::bad_request(format!(
            "AI resource '{resource_id}' uses an external binding and cannot be resolved remotely"
        ))),
        _ => Err(ErrorData::bad_request(format!(
            "AI resource '{resource_id}' binding does not match deployment platform '{}'",
            deployment.platform
        ))),
    }
}

fn remote_sandbox_binding(
    deployment: &DeploymentRecord,
    resource_id: &str,
) -> Result<RemoteSandboxBinding, alien_error::AlienError<ErrorData>> {
    // AWS alone, unlike the other kinds. A GCP sandbox is a subprocess of the application's own
    // Cloud Run instance, and an Azure sandbox group is created by the runtime controller, so
    // neither has a durable parent a setup-owned identity could be scoped to.
    if deployment.platform != Platform::Aws {
        return Err(ErrorData::bad_request(format!(
            "Remote Sandbox is only supported on AWS, not deployment platform '{}'",
            deployment.platform
        )));
    }
    let stack_state = deployment.stack_state.as_ref().ok_or_else(|| {
        ErrorData::bad_request("Deployment has no stack state (not yet provisioned)")
    })?;
    let resource = stack_state.resource(resource_id).ok_or_else(|| {
        ErrorData::bad_request(format!(
            "Resource '{resource_id}' does not exist in stack state"
        ))
    })?;
    if resource.resource_type != Sandbox::RESOURCE_TYPE.as_ref() {
        return Err(ErrorData::bad_request(format!(
            "Resource '{resource_id}' is not a sandbox"
        )));
    }
    if resource.lifecycle != Some(ResourceLifecycle::Frozen) {
        return Err(ErrorData::bad_request(format!(
            "Sandbox resource '{resource_id}' is not Frozen"
        )));
    }
    if resource.status != ResourceStatus::Running {
        return Err(ErrorData::bad_request(format!(
            "Sandbox resource '{resource_id}' is not running"
        )));
    }
    let binding = resource.remote_binding_params.clone().ok_or_else(|| {
        ErrorData::bad_request(format!(
            "Sandbox resource '{resource_id}' is not enabled for remote access"
        ))
    })?;
    let binding: SandboxBinding =
        serde_json::from_value(binding)
            .into_alien_error()
            .context(ErrorData::BadRequest {
                reason: format!("Sandbox resource '{resource_id}' has an invalid remote binding"),
            })?;

    let SandboxBinding::Aws(binding) = binding else {
        return Err(ErrorData::bad_request(format!(
            "Sandbox resource '{resource_id}' binding does not match deployment platform '{}'",
            deployment.platform
        )));
    };
    // Starting a session on a connector is a third authorization, `lambda:PassNetworkConnector`,
    // which AWS scopes to no resource and no condition key. `sandbox/remote-execute` therefore
    // withholds it, and a sandbox that restricts egress is refused here rather than reaching the
    // caller as an AccessDenied from inside its own `create()`.
    if !binding.egress_connector_arns.is_empty() {
        return Err(ErrorData::bad_request(format!(
            "Sandbox resource '{resource_id}' restricts egress; Remote Bindings can only reach a sandbox declared with open egress"
        )));
    }

    Ok(RemoteSandboxBinding::Aws(RemoteAwsSandboxBinding {
        image_arn: concrete_binding_value(&binding.image_arn, "AWS sandbox imageArn")?,
        image_version: concrete_binding_value(&binding.image_version, "AWS sandbox imageVersion")?,
        region: concrete_binding_value(&binding.region, "AWS sandbox region")?,
        preview_ports: binding.preview_ports,
        idle_suspend_seconds: binding.idle_suspend_seconds,
        max_lifetime_seconds: binding.max_lifetime_seconds,
        allow_egress: binding.allow_egress,
    }))
}

#[cfg(test)]
#[path = "bindings/tests.rs"]
mod tests;
