use std::sync::Arc;

use alien_core::{ClientConfig, Platform, StackState};
use alien_error::Context;
use alien_infra::RemoteAccessResolver;
use axum::{extract::Extension, http::HeaderMap, Json};
use serde::{Deserialize, Serialize};
use tracing::info;

use super::auth::resolve_subject;
use crate::providers::platform_api::{
    credential_resolver::impersonate_management_service_account,
    error::{ErrorData, Result},
    PlatformState,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveCredentialsRequest {
    pub platform: Platform,
    pub stack_state: StackState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveCredentialsResponse {
    pub client_config: ClientConfig,
}

pub async fn resolve_credentials(
    Extension(ext): Extension<Arc<PlatformState>>,
    headers: HeaderMap,
    Json(request): Json<ResolveCredentialsRequest>,
) -> Result<Json<ResolveCredentialsResponse>> {
    resolve_subject(&ext.api_url, &headers).await?;
    info!("Resolving credentials for platform: {:?}", request.platform);

    let client_config = resolve_client_config_from_stack_state(&ext, &request.stack_state)
        .await
        .context(ErrorData::CredentialResolutionFailed {
            platform: format!("{:?}", request.platform),
            message: "Failed to resolve remote access credentials".to_string(),
        })?;

    info!(
        "Successfully resolved credentials for platform: {:?}",
        request.platform
    );

    Ok(Json(ResolveCredentialsResponse { client_config }))
}

async fn resolve_client_config_from_stack_state(
    ext: &PlatformState,
    stack_state: &StackState,
) -> Result<ClientConfig> {
    let platform = stack_state.platform;

    let provider = ext.provider_for_target(platform);
    let base_config = impersonate_management_service_account(&**provider, platform).await?;

    let resolver = RemoteAccessResolver::new(std::env::vars().collect());
    resolver
        .resolve(base_config, stack_state)
        .await
        .context(ErrorData::ClientConfigError {
            message: "Failed to resolve remote access from stack state".to_string(),
        })
}
