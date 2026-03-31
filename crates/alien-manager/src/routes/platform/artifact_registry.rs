use std::sync::Arc;

use alien_bindings::traits::ArtifactRegistryPermissions;
use alien_core::Platform;
use alien_error::{AlienError, Context};
use axum::{
    extract::{Extension, Path, Query},
    http::HeaderMap,
    Json,
};
use serde::{Deserialize, Serialize};
use tracing::info;

use super::auth::resolve_subject;
use crate::providers::platform_api::{
    error::{ErrorData, Result},
    PlatformState,
};

#[derive(Debug, Clone, Deserialize)]
pub struct PlatformQuery {
    pub platform: Option<Platform>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateRepositoryRequest {
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryResponse {
    pub id: String,
    pub name: String,
    pub uri: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OperationType {
    Push,
    Pull,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialsRequest {
    pub operation: OperationType,
    pub duration_seconds: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialsResponse {
    pub username: String,
    pub password: String,
    pub registry: String,
    pub expires_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrossAccountAccessRequest {
    pub account_id: String,
    pub platform: String,
}

pub async fn create_repository(
    Extension(ext): Extension<Arc<PlatformState>>,
    headers: HeaderMap,
    Query(query): Query<PlatformQuery>,
    Json(request): Json<CreateRepositoryRequest>,
) -> Result<Json<RepositoryResponse>> {
    resolve_subject(&ext.api_url, &headers).await?;
    info!(name = %request.name, platform = ?query.platform, "Creating artifact registry repository");

    let provider = match query.platform {
        Some(p) => ext.provider_for_target(p),
        None => &ext.bindings,
    };

    let artifact_registry =
        provider
            .load_artifact_registry("artifacts")
            .await
            .context(ErrorData::BindingNotFound {
                binding_name: "artifacts".to_string(),
            })?;

    let repo_info = artifact_registry
        .create_repository(&request.name)
        .await
        .context(ErrorData::ArtifactRegistryOperationFailed {
            operation: format!("create repository '{}'", request.name),
        })?;

    let uri = repo_info
        .uri
        .clone()
        .unwrap_or_else(|| format!("pending-{}", repo_info.name));
    info!(name = %request.name, uri = %uri, "Repository created successfully");

    Ok(Json(RepositoryResponse {
        id: repo_info.name.clone(),
        name: repo_info.name,
        uri,
    }))
}

pub async fn get_repository(
    Extension(ext): Extension<Arc<PlatformState>>,
    headers: HeaderMap,
    Path(repo_id): Path<String>,
    Query(query): Query<PlatformQuery>,
) -> Result<Json<RepositoryResponse>> {
    resolve_subject(&ext.api_url, &headers).await?;
    info!(repo_id = %repo_id, platform = ?query.platform, "Getting repository details");

    let provider = match query.platform {
        Some(p) => ext.provider_for_target(p),
        None => &ext.bindings,
    };

    let artifact_registry =
        provider
            .load_artifact_registry("artifacts")
            .await
            .context(ErrorData::BindingNotFound {
                binding_name: "artifacts".to_string(),
            })?;

    let repo_info = artifact_registry.get_repository(&repo_id).await.context(
        ErrorData::ArtifactRegistryOperationFailed {
            operation: format!("get repository '{}'", repo_id),
        },
    )?;

    Ok(Json(RepositoryResponse {
        id: repo_info.name.clone(),
        name: repo_info.name.clone(),
        uri: repo_info
            .uri
            .unwrap_or_else(|| format!("pending-{}", repo_info.name)),
    }))
}

pub async fn get_credentials(
    Extension(ext): Extension<Arc<PlatformState>>,
    headers: HeaderMap,
    Path(repo_id): Path<String>,
    Query(query): Query<PlatformQuery>,
    Json(request): Json<CredentialsRequest>,
) -> Result<Json<CredentialsResponse>> {
    resolve_subject(&ext.api_url, &headers).await?;
    let operation_str = match request.operation {
        OperationType::Push => "push",
        OperationType::Pull => "pull",
    };
    info!(repo_id = %repo_id, operation = %operation_str, platform = ?query.platform, "Generating credentials");

    let permissions = match request.operation {
        OperationType::Push => ArtifactRegistryPermissions::PushPull,
        OperationType::Pull => ArtifactRegistryPermissions::Pull,
    };

    let provider = match query.platform {
        Some(p) => ext.provider_for_target(p),
        None => &ext.bindings,
    };

    let artifact_registry =
        provider
            .load_artifact_registry("artifacts")
            .await
            .context(ErrorData::BindingNotFound {
                binding_name: "artifacts".to_string(),
            })?;

    let ttl = request.duration_seconds.map(|s| s as u32);
    let credentials = artifact_registry
        .generate_credentials(&repo_id, permissions, ttl)
        .await
        .context(ErrorData::ArtifactRegistryOperationFailed {
            operation: format!(
                "generate {} credentials for repository '{}'",
                operation_str, repo_id
            ),
        })?;

    info!(repo_id = %repo_id, operation = %operation_str, "Credentials generated successfully");

    Ok(Json(CredentialsResponse {
        username: credentials.username,
        password: credentials.password,
        registry: repo_id.clone(),
        expires_at: credentials.expires_at,
    }))
}

pub async fn add_cross_account_access(
    Extension(ext): Extension<Arc<PlatformState>>,
    headers: HeaderMap,
    Path(repo_id): Path<String>,
    Query(query): Query<PlatformQuery>,
    Json(request): Json<CrossAccountAccessRequest>,
) -> Result<axum::http::StatusCode> {
    resolve_subject(&ext.api_url, &headers).await?;
    use alien_bindings::traits::{
        AwsCrossAccountAccess, ComputeServiceType, CrossAccountAccess, GcpCrossAccountAccess,
    };

    info!(
        repo_id = %repo_id,
        account_id = %request.account_id,
        platform = %request.platform,
        "Adding cross-account access"
    );

    let provider = match query.platform {
        Some(p) => ext.provider_for_target(p),
        None => &ext.bindings,
    };

    let artifact_registry =
        provider
            .load_artifact_registry("artifacts")
            .await
            .context(ErrorData::BindingNotFound {
                binding_name: "artifacts".to_string(),
            })?;

    let access = match request.platform.to_lowercase().as_str() {
        "aws" => CrossAccountAccess::Aws(AwsCrossAccountAccess {
            account_ids: vec![request.account_id.clone()],
            allowed_service_types: vec![ComputeServiceType::Function],
            role_arns: vec![],
            regions: vec![],
        }),
        "gcp" => CrossAccountAccess::Gcp(GcpCrossAccountAccess {
            project_numbers: vec![request.account_id.clone()],
            allowed_service_types: vec![ComputeServiceType::Function],
            service_account_emails: vec![],
        }),
        _ => {
            return Err(AlienError::new(ErrorData::FeatureNotSupported {
                feature: format!("cross-account access for platform '{}'", request.platform),
                message: "Supported platforms are 'aws' and 'gcp'".to_string(),
            }))
        }
    };

    artifact_registry
        .add_cross_account_access(&repo_id, access)
        .await
        .context(ErrorData::ArtifactRegistryOperationFailed {
            operation: format!("add cross-account access for repository '{}'", repo_id),
        })?;

    info!(repo_id = %repo_id, "Cross-account access added successfully");
    Ok(axum::http::StatusCode::NO_CONTENT)
}

pub async fn remove_cross_account_access(
    Extension(ext): Extension<Arc<PlatformState>>,
    headers: HeaderMap,
    Path(repo_id): Path<String>,
    Query(query): Query<PlatformQuery>,
    Json(request): Json<CrossAccountAccessRequest>,
) -> Result<axum::http::StatusCode> {
    resolve_subject(&ext.api_url, &headers).await?;
    use alien_bindings::traits::{
        AwsCrossAccountAccess, ComputeServiceType, CrossAccountAccess, GcpCrossAccountAccess,
    };

    info!(
        repo_id = %repo_id,
        account_id = %request.account_id,
        platform = %request.platform,
        "Removing cross-account access"
    );

    let provider = match query.platform {
        Some(p) => ext.provider_for_target(p),
        None => &ext.bindings,
    };

    let artifact_registry =
        provider
            .load_artifact_registry("artifacts")
            .await
            .context(ErrorData::BindingNotFound {
                binding_name: "artifacts".to_string(),
            })?;

    let access = match request.platform.to_lowercase().as_str() {
        "aws" => CrossAccountAccess::Aws(AwsCrossAccountAccess {
            account_ids: vec![request.account_id.clone()],
            allowed_service_types: vec![ComputeServiceType::Function],
            role_arns: vec![],
            regions: vec![],
        }),
        "gcp" => CrossAccountAccess::Gcp(GcpCrossAccountAccess {
            project_numbers: vec![request.account_id.clone()],
            allowed_service_types: vec![ComputeServiceType::Function],
            service_account_emails: vec![],
        }),
        _ => {
            return Err(AlienError::new(ErrorData::FeatureNotSupported {
                feature: format!("cross-account access for platform '{}'", request.platform),
                message: "Supported platforms are 'aws' and 'gcp'".to_string(),
            }))
        }
    };

    artifact_registry
        .remove_cross_account_access(&repo_id, access)
        .await
        .context(ErrorData::ArtifactRegistryOperationFailed {
            operation: format!("remove cross-account access for repository '{}'", repo_id),
        })?;

    info!(repo_id = %repo_id, "Cross-account access removed successfully");
    Ok(axum::http::StatusCode::NO_CONTENT)
}
