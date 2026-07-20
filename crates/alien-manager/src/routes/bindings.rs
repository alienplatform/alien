//! Remote resource-binding resolution.
//!
//! The request names only a deployment and a logical resource. The manager
//! validates the authoritative stack state before it releases the resource's
//! binding topology together with materialized, short-lived credentials.

use alien_core::{
    AwsClientConfig, AwsCredentials, AzureClientConfig, AzureCredentials, BlobStorageBinding,
    ClientConfig, GcpClientConfig, GcpCredentials, GcsStorageBinding, Platform, ResourceLifecycle,
    ResourceStatus, S3StorageBinding, Storage, StorageBinding,
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
    materialize_remote_storage_lease, MaterializedCredentialLease, AZURE_STORAGE_SCOPE,
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
        binding: S3StorageBinding,
        #[serde(rename = "clientConfig")]
        client_config: RemoteAwsClientConfig,
        #[serde(rename = "expiresAt")]
        expires_at: String,
    },
    /// Azure Blob Storage and an exact storage-audience token.
    Blob {
        binding: BlobStorageBinding,
        #[serde(rename = "clientConfig")]
        client_config: RemoteAzureClientConfig,
        #[serde(rename = "expiresAt")]
        expires_at: String,
    },
    /// Google Cloud Storage and a minted access token.
    Gcs {
        binding: GcsStorageBinding,
        #[serde(rename = "clientConfig")]
        client_config: RemoteGcpClientConfig,
        #[serde(rename = "expiresAt")]
        expires_at: String,
    },
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
#[serde(rename_all = "camelCase", tag = "type")]
pub enum RemoteAwsCredentials {
    /// Temporary AWS session credentials with an authoritative expiry.
    SessionCredentials {
        /// AWS access key id.
        access_key_id: String,
        /// AWS secret access key.
        secret_access_key: String,
        /// AWS session token.
        session_token: String,
        /// Provider-reported credential expiry.
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

/// Response-safe Azure client configuration. It contains one exact
/// storage-audience token and no refreshable identity source.
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
    /// One token keyed by the exact Azure Storage OAuth scope.
    pub credentials: RemoteAzureCredentials,
}

/// The only Azure credential form remote binding resolution can return.
#[derive(Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum RemoteAzureCredentials {
    /// Exact scope-to-token map containing only the Azure Storage scope.
    ScopedAccessTokens {
        /// The one Azure Storage OAuth token.
        tokens: RemoteAzureStorageToken,
    },
}

/// Exact Azure Storage OAuth scope-to-token object.
#[derive(Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct RemoteAzureStorageToken {
    /// Bearer token for `https://storage.azure.com/.default`.
    #[serde(rename = "https://storage.azure.com/.default")]
    pub token: String,
}

/// Storage binding variants supported by the first hosted remote-bindings release.
#[derive(Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(tag = "service", rename_all = "lowercase")]
pub enum RemoteStorageBinding {
    /// AWS S3.
    S3(S3StorageBinding),
    /// Azure Blob Storage.
    Blob(BlobStorageBinding),
    /// Google Cloud Storage.
    Gcs(GcsStorageBinding),
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

impl TryFrom<AzureClientConfig> for RemoteAzureClientConfig {
    type Error = alien_error::AlienError<ErrorData>;

    fn try_from(config: AzureClientConfig) -> Result<Self, Self::Error> {
        if config.service_overrides.is_some() {
            return Err(ErrorData::internal(
                "Remote Azure Storage response contains service endpoint overrides",
            ));
        }
        let AzureCredentials::ScopedAccessTokens { mut tokens } = config.credentials else {
            return Err(ErrorData::internal(
                "Remote Azure Storage response credentials are not exact scoped access tokens",
            ));
        };
        if tokens.len() != 1 {
            return Err(ErrorData::internal(
                "Remote Azure Storage response must contain only the exact storage-scope token",
            ));
        }
        let storage_token = tokens.remove(AZURE_STORAGE_SCOPE).ok_or_else(|| {
            ErrorData::internal(
                "Remote Azure Storage response must contain only the exact storage-scope token",
            )
        })?;

        Ok(Self {
            subscription_id: config.subscription_id,
            tenant_id: config.tenant_id,
            region: config.region,
            credentials: RemoteAzureCredentials::ScopedAccessTokens {
                tokens: RemoteAzureStorageToken {
                    token: storage_token,
                },
            },
        })
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
                    binding,
                    client_config: (*client_config).try_into()?,
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

#[cfg_attr(feature = "openapi", utoipa::path(
    post,
    path = "/v1/bindings/resolve",
    tag = "bindings",
    request_body = ResolveBindingRequest,
    responses(
        (status = 200, description = "Remote binding resolved successfully", body = ResolveBindingResponse)
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

    let binding = match remote_storage_binding(&deployment, &request.resource_id) {
        Ok(binding) => binding,
        Err(error) => return error.into_response(),
    };

    let resolved = match state.credential_resolver.resolve(&deployment).await {
        Ok(client_config) => client_config,
        Err(error) => {
            return error
                .context(ErrorData::RemoteCredentialHandoffFailed {
                    deployment_id: deployment.id.clone(),
                    platform: deployment.platform,
                })
                .into_response()
        }
    };
    if resolved.platform() != deployment.platform {
        return ErrorData::internal(format!(
            "Credential resolver returned platform '{}' for deployment platform '{}'",
            resolved.platform(),
            deployment.platform
        ))
        .into_response();
    }
    let lease = match materialize_remote_storage_lease(resolved).await {
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
        (Platform::Aws, StorageBinding::S3(binding)) => Ok(RemoteStorageBinding::S3(binding)),
        (Platform::Gcp, StorageBinding::Gcs(binding)) => Ok(RemoteStorageBinding::Gcs(binding)),
        (Platform::Azure, StorageBinding::Blob(binding)) => Ok(RemoteStorageBinding::Blob(binding)),
        _ => Err(ErrorData::bad_request(format!(
            "Storage resource '{resource_id}' binding does not match deployment platform '{}'",
            deployment.platform
        ))),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use alien_core::{Platform, Resource, Stack, StackResourceState, StackState};
    use alien_error::AlienError;
    use async_trait::async_trait;

    use super::*;
    use crate::traits::{CreateReleaseParams, ReleaseRecord};

    #[derive(Default)]
    struct StubReleaseStore {
        releases: HashMap<String, ReleaseRecord>,
    }

    #[async_trait]
    impl ReleaseStore for StubReleaseStore {
        async fn create_release(
            &self,
            caller: &Subject,
            params: CreateReleaseParams,
        ) -> Result<ReleaseRecord, AlienError> {
            Ok(ReleaseRecord {
                id: "created-release".to_string(),
                workspace_id: caller.workspace_id.clone(),
                project_id: params.project_id,
                stacks: params.stacks,
                git_commit_sha: params.git_commit_sha,
                git_commit_ref: params.git_commit_ref,
                git_commit_message: params.git_commit_message,
                created_at: Utc::now(),
            })
        }

        async fn get_release(
            &self,
            _caller: &Subject,
            id: &str,
        ) -> Result<Option<ReleaseRecord>, AlienError> {
            Ok(self.releases.get(id).cloned())
        }

        async fn get_latest_release(
            &self,
            _caller: &Subject,
        ) -> Result<Option<ReleaseRecord>, AlienError> {
            Ok(self.releases.values().next().cloned())
        }

        async fn list_releases(&self, _caller: &Subject) -> Result<Vec<ReleaseRecord>, AlienError> {
            Ok(self.releases.values().cloned().collect())
        }
    }

    fn stack_state_with_resource(
        resource_type: &str,
        lifecycle: Option<ResourceLifecycle>,
        status: ResourceStatus,
        remote_binding_params: Option<serde_json::Value>,
    ) -> StackState {
        let mut stack_state = StackState::new(Platform::Aws);
        stack_state.resources.insert(
            "files".to_string(),
            StackResourceState::builder()
                .resource_type(resource_type.to_string())
                .status(status)
                .config(Resource::new(Storage {
                    id: "files".to_string(),
                    public_read: false,
                    versioning: false,
                    lifecycle_rules: Vec::new(),
                }))
                .maybe_lifecycle(lifecycle)
                .maybe_remote_binding_params(remote_binding_params)
                .dependencies(Vec::new())
                .build(),
        );
        stack_state
    }

    fn deployment(stack_state: StackState) -> DeploymentRecord {
        deployment_on_platform(stack_state, Platform::Aws)
    }

    fn deployment_on_platform(stack_state: StackState, platform: Platform) -> DeploymentRecord {
        DeploymentRecord {
            id: "deployment".to_string(),
            workspace_id: "default".to_string(),
            project_id: "default".to_string(),
            name: "deployment".to_string(),
            deployment_group_id: "group".to_string(),
            platform,
            deployment_protocol_version: 1,
            base_platform: None,
            status: "running".to_string(),
            stack_settings: None,
            stack_state: Some(stack_state),
            environment_info: None,
            runtime_metadata: None,
            current_release_id: None,
            desired_release_id: None,
            import_source: None,
            setup_method: None,
            setup_metadata: None,
            setup_target: None,
            setup_fingerprint: None,
            setup_fingerprint_version: None,
            user_environment_variables: None,
            management_config: None,
            deployment_config: None,
            deployment_token: None,
            retry_requested: false,
            locked_by: None,
            locked_at: None,
            created_at: Utc::now(),
            updated_at: None,
            error: None,
        }
    }

    fn storage() -> Storage {
        Storage {
            id: "files".to_string(),
            public_read: false,
            versioning: false,
            lifecycle_rules: Vec::new(),
        }
    }

    fn storage_stack(remote_access: bool) -> Stack {
        let builder = Stack::new("stack".to_string());
        if remote_access {
            builder
                .add_with_remote_access(storage(), ResourceLifecycle::Frozen)
                .build()
        } else {
            builder.add(storage(), ResourceLifecycle::Frozen).build()
        }
    }

    fn release(id: &str, platform: Platform, stack: Stack) -> ReleaseRecord {
        ReleaseRecord {
            id: id.to_string(),
            workspace_id: "default".to_string(),
            project_id: "default".to_string(),
            stacks: HashMap::from([(platform, stack)]),
            git_commit_sha: None,
            git_commit_ref: None,
            git_commit_message: None,
            created_at: Utc::now(),
        }
    }

    fn lease(client_config: ClientConfig) -> MaterializedCredentialLease {
        MaterializedCredentialLease {
            client_config,
            expires_at: Utc::now() + chrono::Duration::minutes(15),
        }
    }

    #[test]
    fn remote_storage_validation_accepts_only_running_frozen_storage_with_binding() {
        let binding = StorageBinding::s3("files");
        let deployment = deployment(stack_state_with_resource(
            Storage::RESOURCE_TYPE.as_ref(),
            Some(ResourceLifecycle::Frozen),
            ResourceStatus::Running,
            Some(serde_json::to_value(&binding).unwrap()),
        ));

        assert!(matches!(
            remote_storage_binding(&deployment, "files"),
            Ok(RemoteStorageBinding::S3(S3StorageBinding { .. }))
        ));
    }

    #[tokio::test]
    async fn remote_access_uses_the_current_release_not_the_desired_release() {
        let mut deployment = deployment(stack_state_with_resource(
            Storage::RESOURCE_TYPE.as_ref(),
            Some(ResourceLifecycle::Frozen),
            ResourceStatus::Running,
            Some(serde_json::to_value(StorageBinding::s3("files")).unwrap()),
        ));
        deployment.current_release_id = Some("current".to_string());
        deployment.desired_release_id = Some("desired".to_string());
        let store = StubReleaseStore {
            releases: HashMap::from([
                (
                    "current".to_string(),
                    release("current", Platform::Aws, storage_stack(true)),
                ),
                (
                    "desired".to_string(),
                    release("desired", Platform::Aws, storage_stack(false)),
                ),
            ]),
        };

        require_current_release_remote_access(&store, &deployment, "files")
            .await
            .expect("the current release explicitly enables remote access");
    }

    #[tokio::test]
    async fn legacy_binding_params_cannot_bypass_a_disabled_current_release() {
        let mut deployment = deployment(stack_state_with_resource(
            Storage::RESOURCE_TYPE.as_ref(),
            Some(ResourceLifecycle::Frozen),
            ResourceStatus::Running,
            Some(serde_json::to_value(StorageBinding::s3("files")).unwrap()),
        ));
        deployment.current_release_id = Some("current".to_string());
        let store = StubReleaseStore {
            releases: HashMap::from([(
                "current".to_string(),
                release("current", Platform::Aws, storage_stack(false)),
            )]),
        };

        assert!(remote_storage_binding(&deployment, "files").is_ok());
        let error = require_current_release_remote_access(&store, &deployment, "files")
            .await
            .expect_err("stack-state binding params cannot grant access by themselves");
        assert_eq!(error.code, "BAD_REQUEST");
        assert!(error.message.contains("current release"));
        assert!(error.message.contains("not enabled for remote access"));
    }

    #[tokio::test]
    async fn remote_access_fails_closed_when_current_release_context_is_missing() {
        let stack_state = stack_state_with_resource(
            Storage::RESOURCE_TYPE.as_ref(),
            Some(ResourceLifecycle::Frozen),
            ResourceStatus::Running,
            Some(serde_json::to_value(StorageBinding::s3("files")).unwrap()),
        );
        let store = StubReleaseStore::default();

        let no_current_release = deployment(stack_state.clone());
        let error = require_current_release_remote_access(&store, &no_current_release, "files")
            .await
            .expect_err("missing current release must deny access");
        assert_eq!(error.code, "BAD_REQUEST");

        let mut missing_release = deployment(stack_state.clone());
        missing_release.current_release_id = Some("missing".to_string());
        let error = require_current_release_remote_access(&store, &missing_release, "files")
            .await
            .expect_err("a dangling current release id must deny access");
        assert_eq!(error.code, "INTERNAL_ERROR");

        let mut missing_platform_stack = deployment(stack_state.clone());
        missing_platform_stack.current_release_id = Some("current".to_string());
        let store = StubReleaseStore {
            releases: HashMap::from([(
                "current".to_string(),
                release("current", Platform::Gcp, storage_stack(true)),
            )]),
        };
        let error = require_current_release_remote_access(&store, &missing_platform_stack, "files")
            .await
            .expect_err("missing platform stack must deny access");
        assert_eq!(error.code, "INTERNAL_ERROR");

        let mut missing_resource = deployment(stack_state);
        missing_resource.current_release_id = Some("current".to_string());
        let empty_stack = Stack::new("stack".to_string()).build();
        let store = StubReleaseStore {
            releases: HashMap::from([(
                "current".to_string(),
                release("current", Platform::Aws, empty_stack),
            )]),
        };
        let error = require_current_release_remote_access(&store, &missing_resource, "files")
            .await
            .expect_err("resource absent from the current release must deny access");
        assert_eq!(error.code, "BAD_REQUEST");
    }

    #[test]
    fn remote_storage_validation_rejects_unsupported_and_mismatched_platforms() {
        let s3 = serde_json::to_value(StorageBinding::s3("files")).unwrap();
        let gcs = serde_json::to_value(StorageBinding::gcs("files")).unwrap();
        let local = deployment_on_platform(
            stack_state_with_resource(
                Storage::RESOURCE_TYPE.as_ref(),
                Some(ResourceLifecycle::Frozen),
                ResourceStatus::Running,
                Some(s3.clone()),
            ),
            Platform::Local,
        );
        assert!(remote_storage_binding(&local, "files").is_err());

        let mismatched = deployment(stack_state_with_resource(
            Storage::RESOURCE_TYPE.as_ref(),
            Some(ResourceLifecycle::Frozen),
            ResourceStatus::Running,
            Some(gcs),
        ));
        assert!(remote_storage_binding(&mismatched, "files").is_err());
    }

    #[test]
    fn remote_binding_deployment_status_gate_is_post_handoff_only() {
        for status in [
            "running",
            "refresh-failed",
            "update-pending",
            "updating",
            "update-failed",
        ] {
            assert!(deployment_status_allows_remote_bindings(status), "{status}");
        }
        for status in [
            "pending",
            "initial-setup",
            "provisioning",
            "delete-pending",
            "deleting",
            "delete-failed",
            "deleted",
            "error",
        ] {
            assert!(
                !deployment_status_allows_remote_bindings(status),
                "{status}"
            );
        }
    }

    #[test]
    fn aws_remote_binding_expiry_uses_provider_expiry_and_rejects_expired_sessions() {
        let now = DateTime::parse_from_rfc3339("2030-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(
            remote_binding_expiry(now + chrono::Duration::minutes(15), now).unwrap(),
            now + chrono::Duration::minutes(15)
        );
        assert!(remote_binding_expiry(now - chrono::Duration::seconds(1), now).is_err());
    }

    #[test]
    fn remote_storage_validation_rejects_missing_non_storage_non_frozen_non_running_and_non_remote()
    {
        let rejected = [
            stack_state_with_resource(
                Storage::RESOURCE_TYPE.as_ref(),
                Some(ResourceLifecycle::Frozen),
                ResourceStatus::Running,
                None,
            ),
            stack_state_with_resource(
                "queue",
                Some(ResourceLifecycle::Frozen),
                ResourceStatus::Running,
                Some(serde_json::json!({"service": "s3"})),
            ),
            stack_state_with_resource(
                Storage::RESOURCE_TYPE.as_ref(),
                Some(ResourceLifecycle::Live),
                ResourceStatus::Running,
                Some(serde_json::json!({"service": "s3"})),
            ),
            stack_state_with_resource(
                Storage::RESOURCE_TYPE.as_ref(),
                Some(ResourceLifecycle::Frozen),
                ResourceStatus::Provisioning,
                Some(serde_json::json!({"service": "s3"})),
            ),
        ];

        for stack_state in rejected {
            assert!(remote_storage_binding(&deployment(stack_state), "files").is_err());
        }

        assert!(
            remote_storage_binding(&deployment(StackState::new(Platform::Aws)), "missing").is_err()
        );
    }

    #[test]
    fn response_contract_constructs_only_materialized_provider_credentials() {
        let aws = ResolveBindingResponse::from_parts(
            RemoteStorageBinding::S3(S3StorageBinding {
                bucket_name: "bucket".into(),
            }),
            lease(ClientConfig::Aws(Box::new(AwsClientConfig {
                account_id: "123456789012".to_string(),
                region: "us-east-1".to_string(),
                credentials: AwsCredentials::SessionCredentials {
                    access_key_id: "AKIA".to_string(),
                    secret_access_key: "secret".to_string(),
                    session_token: "session".to_string(),
                    expires_at: "2030-01-01T00:00:00Z".to_string(),
                },
                service_overrides: None,
            }))),
            "2030-01-01T00:00:00Z".to_string(),
        )
        .expect("short-lived AWS session should be accepted");
        let aws = serde_json::to_value(aws).unwrap();
        assert_eq!(
            aws.pointer("/clientConfig/credentials/type"),
            Some(&serde_json::json!("sessionCredentials"))
        );
        assert!(aws.pointer("/clientConfig/serviceOverrides").is_none());

        let gcp = ResolveBindingResponse::from_parts(
            RemoteStorageBinding::Gcs(GcsStorageBinding {
                bucket_name: "bucket".into(),
            }),
            lease(ClientConfig::Gcp(Box::new(GcpClientConfig {
                project_id: "project".to_string(),
                region: "us-central1".to_string(),
                credentials: GcpCredentials::AccessToken {
                    token: "token".to_string(),
                },
                service_overrides: None,
                project_number: Some("123".to_string()),
            }))),
            "2030-01-01T00:00:00Z".to_string(),
        )
        .expect("short-lived GCP access token should be accepted");
        let gcp = serde_json::to_value(gcp).unwrap();
        assert_eq!(
            gcp.pointer("/clientConfig/credentials/type"),
            Some(&serde_json::json!("accessToken"))
        );
        assert_eq!(
            gcp.pointer("/clientConfig/projectNumber"),
            Some(&serde_json::json!("123"))
        );

        let azure = ResolveBindingResponse::from_parts(
            RemoteStorageBinding::Blob(BlobStorageBinding {
                account_name: "account".into(),
                container_name: "container".into(),
            }),
            lease(ClientConfig::Azure(Box::new(AzureClientConfig {
                subscription_id: "subscription".to_string(),
                tenant_id: "tenant".to_string(),
                region: Some("eastus".to_string()),
                credentials: AzureCredentials::ScopedAccessTokens {
                    tokens: HashMap::from([(AZURE_STORAGE_SCOPE.to_string(), "token".to_string())]),
                },
                service_overrides: None,
            }))),
            "2030-01-01T00:00:00Z".to_string(),
        )
        .expect("exact Azure storage-scope token should be accepted");
        let azure = serde_json::to_value(azure).unwrap();
        assert_eq!(
            azure.pointer("/clientConfig/credentials/type"),
            Some(&serde_json::json!("scopedAccessTokens"))
        );
        assert_eq!(
            azure.pointer(&format!(
                "/clientConfig/credentials/tokens/{}",
                AZURE_STORAGE_SCOPE.replace('~', "~0").replace('/', "~1")
            )),
            Some(&serde_json::json!("token"))
        );
    }

    #[test]
    fn response_contract_rejects_refreshable_static_and_overbroad_credentials() {
        let aws_error = RemoteAwsClientConfig::try_from(AwsClientConfig {
            account_id: "123456789012".to_string(),
            region: "us-east-1".to_string(),
            credentials: AwsCredentials::AccessKeys {
                access_key_id: "AKIA".to_string(),
                secret_access_key: "secret".to_string(),
                session_token: None,
            },
            service_overrides: None,
        })
        .err()
        .expect("static AWS access keys must not enter a remote response");
        assert_eq!(aws_error.code, "INTERNAL_ERROR");

        let gcp_error = RemoteGcpClientConfig::try_from(GcpClientConfig {
            project_id: "project".to_string(),
            region: "us-central1".to_string(),
            credentials: GcpCredentials::ServiceMetadata,
            service_overrides: None,
            project_number: None,
        })
        .err()
        .expect("refreshable GCP metadata credentials must not enter a remote response");
        assert_eq!(gcp_error.code, "INTERNAL_ERROR");

        let azure_error = RemoteAzureClientConfig::try_from(AzureClientConfig {
            subscription_id: "subscription".to_string(),
            tenant_id: "tenant".to_string(),
            region: Some("eastus".to_string()),
            credentials: AzureCredentials::ScopedAccessTokens {
                tokens: HashMap::from([
                    (AZURE_STORAGE_SCOPE.to_string(), "storage".to_string()),
                    (
                        "https://management.azure.com/.default".to_string(),
                        "management".to_string(),
                    ),
                ]),
            },
            service_overrides: None,
        })
        .err()
        .expect("non-storage Azure scopes must not enter a remote response");
        assert_eq!(azure_error.code, "INTERNAL_ERROR");
    }

    #[test]
    fn resolve_response_debug_redacts_binding_and_credentials() {
        let response = ResolveBindingResponse::from_parts(
            RemoteStorageBinding::S3(S3StorageBinding {
                bucket_name: "sensitive-bucket".into(),
            }),
            lease(ClientConfig::Aws(Box::new(AwsClientConfig {
                account_id: "123456789012".to_string(),
                region: "us-east-1".to_string(),
                credentials: AwsCredentials::SessionCredentials {
                    access_key_id: "AKIASECRET".to_string(),
                    secret_access_key: "TOP_SECRET".to_string(),
                    session_token: "SESSION_SECRET".to_string(),
                    expires_at: "2099-01-01T00:00:00Z".to_string(),
                },
                service_overrides: None,
            }))),
            "2099-01-01T00:00:00Z".to_string(),
        )
        .expect("short-lived AWS session should construct a response");

        let debug = format!("{response:?}");
        assert!(debug.contains("<redacted>"));
        assert!(!debug.contains("sensitive-bucket"));
        assert!(!debug.contains("AKIASECRET"));
        assert!(!debug.contains("TOP_SECRET"));
        assert!(!debug.contains("SESSION_SECRET"));
    }
}
