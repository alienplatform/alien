//! Remote resource-binding resolution.
//!
//! The request names only a deployment and a logical resource. The manager
//! validates the authoritative stack state before it releases the resource's
//! binding topology together with materialized, short-lived credentials.

use alien_core::{
    BlobStorageBinding, GcsStorageBinding, Platform, ResourceLifecycle, ResourceStatus,
    S3StorageBinding, Storage, StorageBinding,
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

use super::{auth, credentials::materialize_remote_storage_client_config, AppState};
use crate::error::ErrorData;
use crate::traits::DeploymentRecord;

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

/// Response containing one approved remote binding and short-lived credentials.
#[derive(Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ResolveBindingResponse {
    /// Server-selected storage binding configuration.
    pub binding: RemoteStorageBinding,
    /// Materialized credentials safe to hand to the caller.
    pub client_config: alien_core::ClientConfig,
    /// Server refresh hint for the returned credentials.
    pub expires_at: String,
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
            .field("binding", &"<redacted>")
            .field("client_config", &"<redacted>")
            .field("expires_at", &self.expires_at)
            .finish()
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
    let (client_config, provider_expires_at) =
        match materialize_remote_storage_client_config(resolved).await {
            Ok(materialized) => materialized,
            Err(error) => return error.into_response(),
        };

    let now = Utc::now();
    let expires_at = match remote_binding_expiry(provider_expires_at, now) {
        Ok(expires_at) => expires_at.to_rfc3339_opts(SecondsFormat::Secs, true),
        Err(error) => return error.into_response(),
    };

    tracing::info!(
        event = "remote_binding_credentials_issued",
        deployment_id = %request.deployment_id,
        resource_id = %request.resource_id,
        platform = %client_config.platform(),
        expires_at = %expires_at,
        "Issued remote Storage credentials"
    );

    (
        [(CACHE_CONTROL, "no-store"), (PRAGMA, "no-cache")],
        Json(ResolveBindingResponse {
            binding,
            client_config,
            expires_at,
        }),
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
    use alien_core::{ClientConfig, Platform, Resource, StackResourceState, StackState};

    use super::*;

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
    fn resolve_response_debug_redacts_binding_and_credentials() {
        let response = ResolveBindingResponse {
            binding: RemoteStorageBinding::S3(S3StorageBinding {
                bucket_name: "sensitive-bucket".into(),
            }),
            client_config: ClientConfig::Aws(Box::new(alien_core::AwsClientConfig {
                account_id: "123456789012".to_string(),
                region: "us-east-1".to_string(),
                credentials: alien_core::AwsCredentials::AccessKeys {
                    access_key_id: "AKIASECRET".to_string(),
                    secret_access_key: "TOP_SECRET".to_string(),
                    session_token: None,
                },
                service_overrides: None,
            })),
            expires_at: "2099-01-01T00:00:00Z".to_string(),
        };

        let debug = format!("{response:?}");
        assert!(debug.contains("<redacted>"));
        assert!(!debug.contains("sensitive-bucket"));
        assert!(!debug.contains("AKIASECRET"));
        assert!(!debug.contains("TOP_SECRET"));
    }
}
