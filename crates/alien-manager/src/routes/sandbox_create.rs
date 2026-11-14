//! Creating a sandbox for a remote caller on a protected Azure install.
//!
//! The remote caller's identity may not create sandboxes there (a sandbox starts signed code in
//! the install's network), so the request comes here. The caller names only the deployment, the
//! sandbox resource and the sandbox's environment. Everything else is rebuilt from the
//! deployment's recorded binding and handed to the install's verifier, which checks it against
//! the signed release and refuses any difference, so this route cannot be asked to widen
//! anything.

use std::collections::BTreeMap;
use std::sync::Arc;

use alien_bindings::providers::sandbox::azure::AzureSandbox;
use alien_core::{Platform, ReleaseStatements};
use alien_error::AlienError;
use axum::{
    extract::State,
    http::HeaderMap,
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use serde::{Deserialize, Serialize};

use super::bindings::{
    current_release_binding_kind, deployment_status_allows_remote_bindings, remote_sandbox_binding,
    require_current_release_remote_access, require_setup_owned_remote_binding,
    RemoteSandboxBinding,
};
use super::{auth, AppState};
use crate::auth::remote_binding_capability;
use crate::error::ErrorData;
use crate::traits::{deployment_status_from_record, DeploymentRecord};

pub fn router() -> Router<AppState> {
    Router::new().route("/v1/bindings/sandboxes/create", post(create_sandbox))
}

/// What a caller chooses. An unknown field is refused, so nothing else can be asked for.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateRemoteSandboxRequest {
    pub deployment_id: String,
    pub resource_id: String,
    /// Variables placed in the sandbox.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// Request-key and expiry labels, the only labels a caller sets. Each is format-checked
    /// here and again by the verifier.
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
}

/// Whether the deployment is a protected Azure install, whose remote creates go through here.
pub fn creates_through_manager(deployment: &DeploymentRecord) -> bool {
    deployment.platform == Platform::Azure
        && deployment
            .stack_settings
            .as_ref()
            .is_some_and(|settings| settings.signing.is_some())
}

#[cfg_attr(feature = "openapi", utoipa::path(
    post,
    path = "/v1/bindings/sandboxes/create",
    tag = "bindings",
    request_body = CreateRemoteSandboxRequest,
    responses(
        (status = 200, description = "The sandbox the data plane created, as it reports it", body = serde_json::Value),
        (status = 400, description = "The deployment creates its own sandboxes, or the install's verifier refused the request", body = alien_error::AlienError),
        (status = 401, description = "Authentication is required", body = alien_error::AlienError),
        (status = 403, description = "The caller cannot create sandboxes for this deployment", body = alien_error::AlienError),
        (status = 404, description = "The deployment was not found", body = alien_error::AlienError)
    ),
    security(
        ("bearer" = [])
    )
))]
pub async fn create_sandbox(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let subject = match auth::require_auth(&state, &headers).await {
        Ok(subject) => subject,
        Err(error) => return error.into_response(),
    };
    let request: CreateRemoteSandboxRequest = match serde_json::from_slice(&body) {
        Ok(request) => request,
        Err(error) => {
            return ErrorData::bad_request(format!("Invalid sandbox create request: {error}"))
                .into_response()
        }
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
    // The same gates as resolving the sandbox's binding: a caller that could not resolve it
    // cannot create in it.
    if !remote_binding_capability::names_deployment(&subject, &deployment)
        && !state.authz.can_update_deployment(&subject, &deployment)
    {
        return ErrorData::forbidden("Cannot create sandboxes for this deployment").into_response();
    }
    let (release, binding_kind) = match current_release_binding_kind(
        state.release_store.as_ref(),
        &deployment,
        &request.resource_id,
    )
    .await
    {
        Ok(found) => found,
        Err(error) => return error.into_response(),
    };
    if binding_kind != alien_core::remote_bindings::RemoteBindingKind::Sandbox
        || !state.authz.can_resolve_remote_binding(
            &subject,
            &deployment,
            binding_kind,
            &request.resource_id,
        )
    {
        return ErrorData::forbidden("Cannot create sandboxes for this deployment").into_response();
    }
    if !creates_through_manager(&deployment) {
        return ErrorData::bad_request(
            "This deployment's remote callers create sandboxes themselves",
        )
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
    if let Err(error) =
        require_current_release_remote_access(&release, &deployment, &request.resource_id)
    {
        return error.into_response();
    }
    if let Err(error) = require_setup_owned_remote_binding(&deployment, &request.resource_id) {
        return error.into_response();
    }
    let binding = match remote_sandbox_binding(&deployment, &request.resource_id) {
        Ok(RemoteSandboxBinding::Azure(binding)) => binding,
        Ok(_) => {
            return ErrorData::bad_request(
                "Remote sandbox creation through the manager is Azure only",
            )
            .into_response()
        }
        Err(error) => return error.into_response(),
    };

    let result = async {
        let signing = state.signing.as_ref().ok_or_else(|| {
            ErrorData::internal("This manager stores no signer lists to verify releases with")
        })?;
        let statements = ReleaseStatements {
            release: release
                .signed_stacks
                .get(&deployment.platform)
                .cloned()
                .ok_or_else(|| {
                    ErrorData::bad_request("The current release carries no signed statement")
                })?,
            signer_lists: signing
                .signer_lists(&release.project_id)
                .await
                .map_err(|e| ErrorData::internal(format!("Failed to read the signer lists: {e}")))?
                .into_iter()
                .map(|record| record.statement)
                .collect(),
        };
        let signed_config = signing
            .latest_config_statement(&deployment.id)
            .await
            .map_err(|e| ErrorData::internal(format!("Failed to read the signed config: {e}")))?;

        let management = state
            .credential_resolver
            .resolve(&deployment)
            .await
            .map_err(|e| {
                ErrorData::internal(format!(
                    "Failed to resolve the deployment's management credentials: {e}"
                ))
            })?;
        let config = management
            .azure_config()
            .ok_or_else(|| ErrorData::internal("The deployment has no Azure credentials"))?
            .clone();
        let stack_state = deployment
            .stack_state
            .as_ref()
            .ok_or_else(|| ErrorData::bad_request("Deployment has no stack state"))?;
        let resource_group = alien_infra::azure_utils::get_resource_group_name(stack_state)
            .map_err(|e| {
                ErrorData::internal(format!("The install's resource group is unknown: {e}"))
            })?;

        let provider = alien_infra::DefaultPlatformServiceProvider::default();
        let apps = alien_infra::PlatformServiceProvider::get_azure_container_apps_client(
            &provider, &config,
        )
        .map_err(|e| ErrorData::internal(format!("Container Apps client: {e}")))?;
        let verifier = alien_infra::verifier::azure_verifier_in(
            &config,
            &stack_state.resource_prefix,
            &resource_group,
            apps.as_ref(),
        )
        .await
        .map_err(|e| ErrorData::internal(format!("The install's verifier: {e}")))?;

        let provider_sandbox = Arc::new(binding_sandbox(&binding, &config)?);
        let (base, create) = provider_sandbox
            .verified_create(request.env.clone(), request.labels.clone())
            .await
            .map_err(|e| ErrorData::bad_request(format!("{e}")))?;
        alien_infra::verifier::create_azure_sandbox(
            &verifier,
            &statements,
            signed_config.as_ref(),
            &request.resource_id,
            &binding.sandbox_group,
            &base,
            create,
        )
        .await
        .map_err(|e| ErrorData::bad_request(format!("The verifier refused the sandbox: {e}")))
    }
    .await;
    match result {
        Ok(sandbox) => Json(sandbox).into_response(),
        Err(error) => error.into_response(),
    }
}

/// The provider the binding describes, read-only: used for the disk image lookup and the body
/// the caller's own provider would send.
fn binding_sandbox(
    binding: &super::bindings::RemoteAzureSandboxBinding,
    config: &alien_core::AzureClientConfig,
) -> Result<AzureSandbox, AlienError<ErrorData>> {
    use alien_azure_clients::azure::sandbox_data_plane::AzureSandboxDataPlaneClient;
    use alien_azure_clients::azure::token_cache::AzureTokenCache;
    let client = AzureSandboxDataPlaneClient::new(
        reqwest::Client::new(),
        &binding.region,
        &binding.resource_group,
        AzureTokenCache::new(config.clone()),
    );
    let sandbox = AzureSandbox::new(
        Arc::new(client),
        binding.sandbox_group.clone(),
        binding.disk_image.clone(),
        alien_core::SandboxEgress::Allow,
        binding.idle_pause_seconds,
        binding.cpu.clone().unwrap_or_else(|| {
            alien_azure_clients::azure::sandbox_data_plane::DEFAULT_CPU.to_string()
        }),
        binding.memory.clone().unwrap_or_else(|| {
            alien_azure_clients::azure::sandbox_data_plane::DEFAULT_MEMORY.to_string()
        }),
        binding.disk.clone(),
    );
    Ok(sandbox.with_agent(binding.agent.unwrap_or(false)))
}
