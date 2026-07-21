//! Remote resource-binding resolution.
//!
//! The request names only a deployment and a logical resource. The manager
//! validates the authoritative stack state before it releases the resource's
//! binding topology together with materialized, short-lived credentials.

use alien_core::{
    AwsClientConfig, AwsCredentials, AzureClientConfig, AzureCredentials, BindingValue,
    ClientConfig, GcpClientConfig, GcpCredentials, Platform, ResourceLifecycle, ResourceStatus,
    Storage, StorageBinding,
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

use super::{auth, AppState};
use crate::auth::Subject;
use crate::credential_materialization::{
    materialize_remote_storage_lease, MaterializedCredentialLease, RemoteStorageCredentialScope,
    AZURE_REMOTE_STORAGE_PERMISSIONS,
};
use crate::error::ErrorData;
use crate::traits::{DeploymentRecord, ReleaseStore};

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
    /// Logical Storage resource id in the deployment's stack state.
    pub resource_id: String,
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
    /// Azure Blob Storage and an exact container-scoped SAS.
    Blob {
        binding: RemoteBlobStorageBinding,
        #[serde(rename = "clientConfig")]
        client_config: RemoteAzureClientConfig,
        #[serde(rename = "expiresAt")]
        expires_at: String,
    },
    /// Google Cloud Storage and a bucket-downscoped access token.
    Gcs {
        binding: RemoteGcsStorageBinding,
        #[serde(rename = "clientConfig")]
        client_config: RemoteGcpClientConfig,
        #[serde(rename = "expiresAt")]
        expires_at: String,
    },
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

/// Response-safe Azure client configuration. It contains one container-bound
/// user-delegation SAS and no OAuth or refreshable identity source.
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
    /// A short-lived SAS bound to the requested Blob container.
    pub credentials: RemoteAzureCredentials,
}

/// The only Azure credential form remote binding resolution can return.
#[derive(Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum RemoteAzureCredentials {
    /// User-delegation SAS signed for exactly one container.
    ContainerSas {
        /// Explicit signed fields required to reconstruct the SAS query.
        sas: RemoteAzureContainerSas,
    },
}

/// Explicit fields of an Azure user-delegation SAS. Keeping the fields typed
/// lets clients independently validate container scope, permissions, protocol,
/// and expiry before constructing query parameters.
#[derive(Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemoteAzureContainerSas {
    /// Storage account named by the signed canonical resource.
    pub account_name: String,
    /// Blob container named by the signed canonical resource.
    pub container_name: String,
    /// Canonically ordered SAS permissions (`sp`).
    pub permissions: String,
    /// SAS validity start (`st`).
    pub starts_at: String,
    /// SAS validity end (`se`).
    pub expires_at: String,
    /// Object ID that requested the delegation key (`skoid`).
    pub signed_object_id: String,
    /// Tenant ID that issued the delegation key (`sktid`).
    pub signed_tenant_id: String,
    /// Delegation-key validity start (`skt`).
    pub signed_key_start: String,
    /// Delegation-key validity end (`ske`).
    pub signed_key_expiry: String,
    /// Delegation-key service (`sks`).
    pub signed_key_service: String,
    /// Delegation-key version (`skv`).
    pub signed_key_version: String,
    /// Required transport protocol (`spr`).
    pub protocol: String,
    /// Storage authorization version (`sv`).
    pub service_version: String,
    /// Signed resource kind (`sr`).
    pub signed_resource: String,
    /// HMAC-SHA256 signature (`sig`).
    pub signature: String,
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

impl RemoteStorageBinding {
    fn credential_scope(&self) -> RemoteStorageCredentialScope {
        match self {
            Self::S3(binding) => RemoteStorageCredentialScope::AwsS3 {
                bucket_name: binding.bucket_name.clone(),
            },
            Self::Gcs(binding) => RemoteStorageCredentialScope::GcpGcs {
                bucket_name: binding.bucket_name.clone(),
            },
            Self::Blob(binding) => RemoteStorageCredentialScope::AzureBlob {
                account_name: binding.account_name.clone(),
                container_name: binding.container_name.clone(),
            },
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
                "Remote AWS Storage response contains service endpoint overrides",
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
                "Remote AWS Storage response credentials are not a short-lived session",
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
                "Remote GCP Storage response contains service endpoint overrides",
            ));
        }
        let GcpCredentials::AccessToken { token } = config.credentials else {
            return Err(ErrorData::internal(
                "Remote GCP Storage response credentials are not a short-lived access token",
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

impl TryFrom<(AzureClientConfig, &RemoteBlobStorageBinding)> for RemoteAzureClientConfig {
    type Error = alien_error::AlienError<ErrorData>;

    fn try_from(
        (config, binding): (AzureClientConfig, &RemoteBlobStorageBinding),
    ) -> Result<Self, Self::Error> {
        if config.service_overrides.is_some() {
            return Err(ErrorData::internal(
                "Remote Azure Storage response contains service endpoint overrides",
            ));
        }
        let AzureCredentials::SasToken {
            mut query_parameters,
        } = config.credentials
        else {
            return Err(ErrorData::internal(
                "Remote Azure Storage response credentials are not a container SAS",
            ));
        };
        let credentials = RemoteAzureContainerSas {
            account_name: binding.account_name.clone(),
            container_name: binding.container_name.clone(),
            permissions: take_sas_parameter(&mut query_parameters, "sp")?,
            starts_at: take_sas_parameter(&mut query_parameters, "st")?,
            expires_at: take_sas_parameter(&mut query_parameters, "se")?,
            signed_object_id: take_sas_parameter(&mut query_parameters, "skoid")?,
            signed_tenant_id: take_sas_parameter(&mut query_parameters, "sktid")?,
            signed_key_start: take_sas_parameter(&mut query_parameters, "skt")?,
            signed_key_expiry: take_sas_parameter(&mut query_parameters, "ske")?,
            signed_key_service: take_sas_parameter(&mut query_parameters, "sks")?,
            signed_key_version: take_sas_parameter(&mut query_parameters, "skv")?,
            protocol: take_sas_parameter(&mut query_parameters, "spr")?,
            service_version: take_sas_parameter(&mut query_parameters, "sv")?,
            signed_resource: take_sas_parameter(&mut query_parameters, "sr")?,
            signature: take_sas_parameter(&mut query_parameters, "sig")?,
        };
        if !query_parameters.is_empty()
            || credentials.permissions != AZURE_REMOTE_STORAGE_PERMISSIONS
            || credentials.protocol != "https"
            || credentials.signed_resource != "c"
            || credentials.signed_key_service != "b"
        {
            return Err(ErrorData::internal(
                "Remote Azure Storage SAS is not exactly container scoped",
            ));
        }

        Ok(Self {
            subscription_id: config.subscription_id,
            tenant_id: config.tenant_id,
            region: config.region,
            credentials: RemoteAzureCredentials::ContainerSas { sas: credentials },
        })
    }
}

fn take_sas_parameter(
    parameters: &mut std::collections::HashMap<String, String>,
    name: &str,
) -> Result<String, alien_error::AlienError<ErrorData>> {
    parameters.remove(name).ok_or_else(|| {
        ErrorData::internal(format!(
            "Remote Azure Storage SAS is missing required parameter '{name}'"
        ))
    })
}

fn concrete_binding_value(
    value: &BindingValue<String>,
    field: &str,
) -> Result<String, alien_error::AlienError<ErrorData>> {
    match value {
        BindingValue::Value(value) if !value.is_empty() => Ok(value.clone()),
        _ => Err(ErrorData::internal(format!(
            "Remote Storage binding field '{field}' is not a concrete value"
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
                    client_config: ((*client_config), &binding).try_into()?,
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
    if !state
        .authz
        .can_resolve_remote_bindings(&subject, &deployment)
    {
        return ErrorData::forbidden("Cannot resolve remote bindings for this deployment")
            .into_response();
    }

    if !deployment_status_allows_remote_bindings(&deployment.status) {
        return ErrorData::bad_request(format!(
            "Deployment is not operational for remote bindings (status '{}')",
            deployment.status
        ))
        .into_response();
    }

    if let Err(error) = require_current_release_remote_access(
        state.release_store.as_ref(),
        &deployment,
        &request.resource_id,
    )
    .await
    {
        return error.into_response();
    }

    if let Err(error) = require_setup_owned_remote_storage(&deployment, &request.resource_id) {
        return error.into_response();
    }

    let binding = match remote_storage_binding(&deployment, &request.resource_id) {
        Ok(binding) => binding,
        Err(error) => return error.into_response(),
    };

    let scope = binding.credential_scope();
    let resolved = match state
        .credential_resolver
        .resolve_remote_storage_source(&deployment)
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
    let lease = match materialize_remote_storage_lease(resolved, scope).await {
        Ok(materialized) => materialized,
        Err(error) => return error.into_response(),
    };

    let now = Utc::now();
    let expires_at = match remote_binding_expiry(lease.expires_at, now) {
        Ok(expires_at) => expires_at.to_rfc3339_opts(SecondsFormat::Secs, true),
        Err(error) => return error.into_response(),
    };

    let response = match ResolveBindingResponse::from_parts(binding, lease, expires_at.clone()) {
        Ok(response) => response,
        Err(error) => return error.into_response(),
    };

    tracing::info!(
        event = "remote_binding_credentials_issued",
        deployment_id = %request.deployment_id,
        resource_id = %request.resource_id,
        platform = %deployment.platform,
        expires_at = %expires_at,
        "Issued remote Storage credentials"
    );

    (
        [(CACHE_CONTROL, "no-store"), (PRAGMA, "no-cache")],
        Json(response),
    )
        .into_response()
}

/// External bindings import caller-supplied resource references; they do not
/// prove that generated setup created the resource. Remote Bindings v0 must
/// therefore reject them even if stale synchronized state contains binding
/// parameters from an older manager.
fn require_setup_owned_remote_storage(
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
            "Remote Storage resource '{resource_id}' cannot use an external binding; remote access is limited to resources created by setup"
        )));
    }
    Ok(())
}

fn deployment_status_allows_remote_bindings(status: &str) -> bool {
    matches!(
        status,
        "running" | "refresh-failed" | "update-pending" | "updating" | "update-failed"
    )
}

fn remote_binding_expiry(
    provider_expires_at: DateTime<Utc>,
    now: DateTime<Utc>,
) -> Result<DateTime<Utc>, alien_error::AlienError<ErrorData>> {
    let maximum = now + chrono::Duration::seconds(REMOTE_BINDING_REFRESH_HINT_SECONDS);
    let expires_at = provider_expires_at.min(maximum);

    if expires_at <= now {
        return Err(ErrorData::internal(
            "Remote Storage credential lease is already expired",
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
) -> Result<(), alien_error::AlienError<ErrorData>> {
    let release_id = deployment.current_release_id.as_deref().ok_or_else(|| {
        ErrorData::bad_request(
            "Deployment has no current release; remote bindings cannot be resolved",
        )
    })?;

    let release = release_store
        .get_release(&Subject::system(), release_id)
        .await
        .context(ErrorData::InternalError {
            message: format!(
                "Failed to load current release '{release_id}' for remote binding resolution"
            ),
        })?
        .ok_or_else(|| {
            ErrorData::internal(format!(
                "Current release '{release_id}' for deployment '{}' does not exist",
                deployment.id
            ))
        })?;

    let stack = release.stacks.get(&deployment.platform).ok_or_else(|| {
        ErrorData::internal(format!(
            "Current release '{release_id}' has no {} stack",
            deployment.platform
        ))
    })?;
    let resource = stack.resources.get(resource_id).ok_or_else(|| {
        ErrorData::bad_request(format!(
            "Resource '{resource_id}' is not part of the deployment's current release"
        ))
    })?;

    if resource.config.resource_type() != Storage::RESOURCE_TYPE {
        return Err(ErrorData::bad_request(format!(
            "Resource '{resource_id}' is not storage in the deployment's current release"
        )));
    }
    if resource.lifecycle != ResourceLifecycle::Frozen {
        return Err(ErrorData::bad_request(format!(
            "Storage resource '{resource_id}' is not Frozen in the deployment's current release"
        )));
    }
    if !resource.remote_access {
        return Err(ErrorData::bad_request(format!(
            "Storage resource '{resource_id}' is not enabled for remote access in the deployment's current release"
        )));
    }

    Ok(())
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

#[cfg(test)]
#[path = "bindings/tests.rs"]
mod tests;
