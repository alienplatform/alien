use std::sync::Arc;

use alien_error::{Context, IntoAlienError};
use alien_platform_api::SdkResultExt;
use axum::{extract::Extension, http::HeaderMap, Json};
use serde::{Deserialize, Serialize};
use tracing::info;
use uuid::Uuid;

use crate::providers::platform_api::{
    error::{ErrorData, Result},
    utils::{create_client_with_token, extract_bearer_token},
    PlatformState,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeRequest {
    pub platform: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeResponse {
    pub deployment_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
}

pub async fn initialize_agent(
    Extension(ext): Extension<Arc<PlatformState>>,
    headers: HeaderMap,
    Json(request): Json<InitializeRequest>,
) -> Result<Json<InitializeResponse>> {
    info!(platform = %request.platform, "Received initialize request from Agent");

    let token = extract_bearer_token(&headers)?;
    let subject = validate_token_with_platform(&ext.api_url, &token).await?;

    match subject {
        alien_platform_api::types::Subject::ServiceAccountSubject(sa) => match sa.scope {
            alien_platform_api::types::SubjectScope::DeploymentGroup {
                deployment_group_id,
                project_id,
                ..
            } => {
                info!(deployment_group_id = %deployment_group_id, "Creating new deployment from Deployment Group Token");

                let (deployment_id, token) = create_deployment_from_deployment_group(
                    &ext.api_url,
                    &token,
                    &deployment_group_id,
                    &project_id,
                    &ext.manager_id,
                    &request.platform,
                )
                .await?;

                info!(deployment_id = %deployment_id, "Successfully created deployment");

                Ok(Json(InitializeResponse {
                    deployment_id,
                    token: Some(token),
                }))
            }
            alien_platform_api::types::SubjectScope::Deployment { deployment_id, .. } => {
                info!(deployment_id = %deployment_id, "Using existing deployment token");

                Ok(Json(InitializeResponse {
                    deployment_id,
                    token: None,
                }))
            }
            _ => Err(alien_error::AlienError::new(ErrorData::Unauthorized {
                message: "Token must be agent-scoped or deployment-group-scoped".to_string(),
            })),
        },
        alien_platform_api::types::Subject::UserSubject(_) => {
            Err(alien_error::AlienError::new(ErrorData::Unauthorized {
                message: "User tokens are not supported for agent initialization".to_string(),
            }))
        }
    }
}

async fn validate_token_with_platform(
    api_url: &str,
    token: &str,
) -> Result<alien_platform_api::types::Subject> {
    let temp_client = create_client_with_token(api_url, token)?;

    let whoami_response =
        temp_client
            .whoami()
            .send()
            .await
            .into_sdk_error()
            .context(ErrorData::Unauthorized {
                message: "Token validation failed".to_string(),
            })?;

    Ok(whoami_response.into_inner())
}

async fn create_deployment_from_deployment_group(
    api_url: &str,
    dg_token: &str,
    deployment_group_id: &str,
    project_id: &str,
    manager_id: &str,
    platform: &str,
) -> Result<(String, String)> {
    let temp_client = create_client_with_token(api_url, dg_token)?;

    let deployment_name = format!("deployment-{}", Uuid::new_v4().simple());

    let typed_dg_id =
        deployment_group_id
            .try_into()
            .into_alien_error()
            .context(ErrorData::SyncFailed {
                message: "Invalid deployment group ID format".to_string(),
            })?;

    let typed_project_id =
        project_id
            .try_into()
            .into_alien_error()
            .context(ErrorData::SyncFailed {
                message: "Invalid project ID format".to_string(),
            })?;

    let typed_name = deployment_name
        .as_str()
        .try_into()
        .into_alien_error()
        .context(ErrorData::SyncFailed {
            message: "Invalid deployment name format".to_string(),
        })?;

    let typed_platform = platform
        .try_into()
        .into_alien_error()
        .context(ErrorData::SyncFailed {
            message: "Invalid platform format".to_string(),
        })?;

    let typed_manager_id =
        manager_id
            .try_into()
            .into_alien_error()
            .context(ErrorData::SyncFailed {
                message: "Invalid manager ID format".to_string(),
            })?;

    let stack_settings = alien_platform_api::types::NewDeploymentRequestStackSettings {
        deployment_model: Some(
            alien_platform_api::types::NewDeploymentRequestStackSettingsDeploymentModel::Pull,
        ),
        heartbeats: Some(
            alien_platform_api::types::NewDeploymentRequestStackSettingsHeartbeats::On,
        ),
        telemetry: Some(alien_platform_api::types::NewDeploymentRequestStackSettingsTelemetry::Off),
        updates: Some(alien_platform_api::types::NewDeploymentRequestStackSettingsUpdates::Auto),
        network: None,
        domains: None,
    };

    let create_response = temp_client
        .create_deployment()
        .body(alien_platform_api::types::NewDeploymentRequest {
            name: typed_name,
            platform: typed_platform,
            project: typed_project_id,
            stack_settings: Some(stack_settings),
            manager_id: Some(typed_manager_id),
            pinned_release_id: None,
            environment_variables: None,
            deployment_group_id: Some(typed_dg_id),
            environment_info: None,
        })
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::SyncFailed {
            message: "Failed to create deployment".to_string(),
        })?;

    let response = create_response.into_inner();
    let deployment_id = response.deployment.id.to_string();

    let token = response.token.ok_or_else(|| {
        alien_error::AlienError::new(ErrorData::SyncFailed {
            message: "Deployment created but no token returned".to_string(),
        })
    })?;

    Ok((deployment_id, token))
}
