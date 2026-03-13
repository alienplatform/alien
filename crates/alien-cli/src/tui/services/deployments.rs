//! Deployments service - SDK calls for deployment operations

use crate::tui::state::deployments::{
    DeploymentItem, DeploymentMetadata, DeploymentPlatform, DeploymentStatus, ReleaseInfo,
    ResourceInfo,
};
use alien_platform_api::{types, Client, SdkResultExt};
use alien_core::{
    ContainerOutputs, EnvironmentInfo, FunctionOutputs, KvOutputs, QueueOutputs, ResourceLifecycle,
    ResourceOutputs, ResourceStatus, ResourceType, StorageOutputs,
};
use alien_error::{AlienError, GenericError};
use serde_json;

/// Service for deployment-related SDK calls
#[derive(Clone)]
pub struct DeploymentsService {
    sdk: Client,
    /// Project ID to filter deployments by (platform mode)
    project_id: Option<String>,
}

impl DeploymentsService {
    pub fn new(sdk: Client, project_id: Option<String>) -> Self {
        Self { sdk, project_id }
    }

    /// List all deployments (filtered by project if set)
    pub async fn list(&self) -> Result<Vec<DeploymentItem>, String> {
        let mut builder = self.sdk.list_deployments().include(vec![
            types::ListDeploymentsIncludeItem::DeploymentGroup,
            types::ListDeploymentsIncludeItem::Release,
        ]);

        // Apply project filter if set
        if let Some(ref project_id) = self.project_id {
            builder = builder.project(project_id);
        }

        let result = builder.send().await.into_sdk_error();

        match result {
            Ok(response) => {
                let items = response
                    .into_inner()
                    .items
                    .into_iter()
                    .map(deployment_from_api)
                    .collect();
                Ok(items)
            }
            Err(e) => Err(format!("Failed to load deployments: {}", e)),
        }
    }

    /// Get a single deployment by ID
    pub async fn get(&self, id: &str) -> Result<DeploymentItem, String> {
        let result = self
            .sdk
            .get_deployment()
            .id(id)
            .include(vec![
                types::GetDeploymentIncludeItem::DeploymentGroup,
                types::GetDeploymentIncludeItem::Release,
            ])
            .send()
            .await
            .into_sdk_error();
        match result {
            Ok(response) => Ok(deployment_from_api_detail(response.into_inner())),
            Err(e) => Err(format!("Failed to get deployment: {}", e)),
        }
    }

    /// Get deployment with its resources and metadata
    pub async fn get_with_resources(
        &self,
        id: &str,
    ) -> Result<(DeploymentItem, Vec<ResourceInfo>, DeploymentMetadata), String> {
        let result = self
            .sdk
            .get_deployment()
            .id(id)
            .include(vec![
                types::GetDeploymentIncludeItem::DeploymentGroup,
                types::GetDeploymentIncludeItem::Release,
            ])
            .send()
            .await
            .into_sdk_error();
        match result {
            Ok(response) => {
                let deployment = response.into_inner();
                let resources = extract_resources_from_stack_state(&deployment);
                let metadata = extract_metadata_from_deployment(&deployment);
                Ok((
                    deployment_from_api_detail(deployment.clone()),
                    resources,
                    metadata,
                ))
            }
            Err(e) => Err(format!("Failed to get deployment: {}", e)),
        }
    }

    /// Create a new deployment
    pub async fn create(
        &self,
        name: &str,
        project_id: &str,
        deployment_group_id: Option<&str>,
        platform: &str,
    ) -> Result<DeploymentItem, String> {
        // Convert platform string to SDK enum
        let platform_enum = match platform {
            "local" => types::NewDeploymentRequestPlatform::Local,
            "aws" => types::NewDeploymentRequestPlatform::Aws,
            "gcp" => types::NewDeploymentRequestPlatform::Gcp,
            "azure" => types::NewDeploymentRequestPlatform::Azure,
            "kubernetes" => types::NewDeploymentRequestPlatform::Kubernetes,
            _ => types::NewDeploymentRequestPlatform::Local,
        };

        let mut builder = types::NewDeploymentRequest::builder()
            .name(name)
            .platform(platform_enum)
            .project(project_id.to_string());

        if let Some(dg_id) = deployment_group_id {
            builder = builder.deployment_group_id(dg_id.to_string());
        }

        let request: types::NewDeploymentRequest = builder
            .try_into()
            .map_err(|e| format!("Failed to build request: {:?}", e))?;

        let result = self.sdk.create_deployment().body(request).send().await;
        match result {
            Ok(response) => Ok(deployment_from_api_base(response.into_inner().deployment)),
            Err(e) => Err(format!("Failed to create deployment: {}", e)),
        }
    }

    /// Delete a deployment
    pub async fn delete(&self, id: &str) -> Result<(), String> {
        let result = self.sdk.delete_deployment().id(id).send().await.into_sdk_error();
        match result {
            Ok(_) => Ok(()),
            Err(e) => Err(format!("Failed to delete deployment: {}", e)),
        }
    }
}

/// Extract resources from deployment's stack_state with typed outputs
fn extract_resources_from_stack_state(
    deployment: &types::DeploymentDetailResponse,
) -> Vec<ResourceInfo> {
    let stack_state: &types::DeploymentDetailResponseStackState = match &deployment.stack_state {
        Some(state) => state,
        None => return Vec::new(),
    };

    // SDK types need JSON round-trip to access nested fields
    let json = match serde_json::to_value(stack_state) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };

    let resources_value = match json.get("resources") {
        Some(v) => v,
        None => return Vec::new(),
    };

    let resources_map = match resources_value.as_object() {
        Some(m) => m,
        None => return Vec::new(),
    };

    resources_map
        .iter()
        .filter_map(|(id, resource_value)| {
            let resource_type = resource_value.get("type").and_then(|v| v.as_str())?;

            let status_str = resource_value
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("pending");

            let lifecycle_str = resource_value.get("lifecycle").and_then(|v| v.as_str());

            let status = parse_resource_status(status_str);
            let lifecycle = lifecycle_str
                .map(parse_resource_lifecycle)
                .unwrap_or(ResourceLifecycle::Live);

            // Parse typed outputs based on resource type
            let outputs =
                resource_value
                    .get("outputs")
                    .and_then(|outputs_json| match resource_type {
                        "function" => {
                            serde_json::from_value::<FunctionOutputs>(outputs_json.clone())
                                .ok()
                                .map(ResourceOutputs::new)
                        }
                        "container" => {
                            serde_json::from_value::<ContainerOutputs>(outputs_json.clone())
                                .ok()
                                .map(ResourceOutputs::new)
                        }
                        "storage" => serde_json::from_value::<StorageOutputs>(outputs_json.clone())
                            .ok()
                            .map(ResourceOutputs::new),
                        "kv" => serde_json::from_value::<KvOutputs>(outputs_json.clone())
                            .ok()
                            .map(ResourceOutputs::new),
                        "queue" => serde_json::from_value::<QueueOutputs>(outputs_json.clone())
                            .ok()
                            .map(ResourceOutputs::new),
                        _ => None,
                    });

            Some(ResourceInfo {
                id: id.clone(),
                resource_type: ResourceType::from(resource_type),
                lifecycle,
                status,
                outputs,
            })
        })
        .collect()
}

/// Extract metadata from deployment response
fn extract_metadata_from_deployment(deployment: &types::DeploymentDetailResponse) -> DeploymentMetadata {
    // Parse environment info from SDK type (which is a JSON value)
    let environment_info = deployment
        .environment_info
        .as_ref()
        .and_then(|v| serde_json::to_value(v).ok())
        .and_then(|v| serde_json::from_value::<EnvironmentInfo>(v).ok());

    // Parse stack_settings from SDK type (which is a JSON value)
    let stack_settings = serde_json::to_value(&deployment.stack_settings)
        .ok()
        .and_then(|v| serde_json::from_value::<alien_core::StackSettings>(v).ok())
        .unwrap_or_default();

    // Convert created_at from DateTime to String
    let created_at = deployment.created_at.to_rfc3339();

    // Convert current_release_id
    let current_release_id = deployment
        .current_release_id
        .as_ref()
        .map(|id| id.to_string());

    // Parse error from SDK type (which is a JSON value)
    let error = deployment
        .error
        .as_ref()
        .and_then(|v| serde_json::to_value(v).ok())
        .and_then(|v| serde_json::from_value::<AlienError<GenericError>>(v).ok());

    DeploymentMetadata {
        created_at,
        platform: platform_from_api_detail(deployment.platform.clone()),
        stack_settings,
        environment_info,
        current_release_id,
        error,
    }
}

fn parse_resource_status(s: &str) -> ResourceStatus {
    match s {
        "pending" => ResourceStatus::Pending,
        "provisioning" => ResourceStatus::Provisioning,
        "running" => ResourceStatus::Running,
        "updating" => ResourceStatus::Updating,
        "deleting" => ResourceStatus::Deleting,
        "deleted" => ResourceStatus::Deleted,
        "provision-failed" | "provisionFailed" => ResourceStatus::ProvisionFailed,
        "update-failed" | "updateFailed" => ResourceStatus::UpdateFailed,
        "delete-failed" | "deleteFailed" => ResourceStatus::DeleteFailed,
        "refresh-failed" | "refreshFailed" => ResourceStatus::RefreshFailed,
        _ => ResourceStatus::Pending,
    }
}

fn parse_resource_lifecycle(s: &str) -> ResourceLifecycle {
    match s {
        "frozen" => ResourceLifecycle::Frozen,
        "live" => ResourceLifecycle::Live,
        _ => ResourceLifecycle::Live,
    }
}

/// Convert API base deployment type (types::Deployment) to state type (no release/deployment group includes)
fn deployment_from_api_base(deployment: types::Deployment) -> DeploymentItem {
    DeploymentItem {
        id: deployment.id.as_str().to_string(),
        name: deployment.name.as_str().to_string(),
        deployment_group_id: deployment.deployment_group_id.as_str().to_string(),
        deployment_group_name: None, // Not included in base Deployment type
        status: status_from_api(deployment.status),
        platform: platform_from_api(deployment.platform),
        release_info: None, // Not included in base Deployment type
    }
}

/// Convert API list item to state type
fn deployment_from_api(deployment: types::DeploymentListItemResponse) -> DeploymentItem {
    // Extract deployment group name if present
    let deployment_group_name = deployment
        .deployment_group
        .as_ref()
        .map(|dg| dg.name.as_str().to_string());

    // Extract release info if present
    // `release` is Option<DeploymentReleaseInfo> where DeploymentReleaseInfo wraps Option<Inner>
    let release_info = deployment.release.as_deref().and_then(Option::as_ref).map(|rel| {
        let gm = rel.git_metadata.as_deref().and_then(Option::as_ref);
        ReleaseInfo {
            id: rel.id.as_str().to_string(),
            git_commit_sha: gm.and_then(|gm| gm.commit_sha.as_ref().map(|cs| cs.as_str().to_string())),
            git_branch: gm.and_then(|gm| gm.commit_ref.as_ref().map(|cr| cr.as_str().to_string())),
            created_at: rel.created_at,
        }
    });

    DeploymentItem {
        id: deployment.id.as_str().to_string(),
        name: deployment.name.as_str().to_string(),
        deployment_group_id: deployment.deployment_group_id.as_str().to_string(),
        deployment_group_name,
        status: status_from_api_list(deployment.status),
        platform: platform_from_api_list(deployment.platform),
        release_info,
    }
}

/// Convert API detail response to state type
fn deployment_from_api_detail(deployment: types::DeploymentDetailResponse) -> DeploymentItem {
    // Extract deployment group name if present
    let deployment_group_name = deployment
        .deployment_group
        .as_ref()
        .map(|dg| dg.name.as_str().to_string());

    // Extract release info if present
    // `release` is Option<DeploymentReleaseInfo> where DeploymentReleaseInfo wraps Option<Inner>
    let release_info = deployment.release.as_deref().and_then(Option::as_ref).map(|rel| {
        let gm = rel.git_metadata.as_deref().and_then(Option::as_ref);
        ReleaseInfo {
            id: rel.id.as_str().to_string(),
            git_commit_sha: gm.and_then(|gm| gm.commit_sha.as_ref().map(|cs| cs.as_str().to_string())),
            git_branch: gm.and_then(|gm| gm.commit_ref.as_ref().map(|cr| cr.as_str().to_string())),
            created_at: rel.created_at,
        }
    });

    DeploymentItem {
        id: deployment.id.as_str().to_string(),
        name: deployment.name.as_str().to_string(),
        deployment_group_id: deployment.deployment_group_id.as_str().to_string(),
        deployment_group_name,
        status: status_from_api_detail(deployment.status),
        platform: platform_from_api_detail(deployment.platform),
        release_info,
    }
}

fn status_from_api_list(status: types::DeploymentListItemResponseStatus) -> DeploymentStatus {
    match status {
        types::DeploymentListItemResponseStatus::Pending => DeploymentStatus::Pending,
        types::DeploymentListItemResponseStatus::Provisioning => DeploymentStatus::Provisioning,
        types::DeploymentListItemResponseStatus::InitialSetup => DeploymentStatus::InitialSetup,
        types::DeploymentListItemResponseStatus::Running => DeploymentStatus::Running,
        types::DeploymentListItemResponseStatus::Updating => DeploymentStatus::Updating,
        types::DeploymentListItemResponseStatus::UpdatePending => DeploymentStatus::UpdatePending,
        types::DeploymentListItemResponseStatus::InitialSetupFailed => {
            DeploymentStatus::InitialSetupFailed
        }
        types::DeploymentListItemResponseStatus::ProvisioningFailed => {
            DeploymentStatus::ProvisioningFailed
        }
        types::DeploymentListItemResponseStatus::RefreshFailed => DeploymentStatus::RefreshFailed,
        types::DeploymentListItemResponseStatus::UpdateFailed => DeploymentStatus::UpdateFailed,
        types::DeploymentListItemResponseStatus::DeleteFailed => DeploymentStatus::DeleteFailed,
        types::DeploymentListItemResponseStatus::DeletePending => DeploymentStatus::DeletePending,
        types::DeploymentListItemResponseStatus::Deleting => DeploymentStatus::Deleting,
        types::DeploymentListItemResponseStatus::Deleted => DeploymentStatus::Deleted,
    }
}

fn status_from_api_detail(status: types::DeploymentDetailResponseStatus) -> DeploymentStatus {
    match status {
        types::DeploymentDetailResponseStatus::Pending => DeploymentStatus::Pending,
        types::DeploymentDetailResponseStatus::Provisioning => DeploymentStatus::Provisioning,
        types::DeploymentDetailResponseStatus::InitialSetup => DeploymentStatus::InitialSetup,
        types::DeploymentDetailResponseStatus::Running => DeploymentStatus::Running,
        types::DeploymentDetailResponseStatus::Updating => DeploymentStatus::Updating,
        types::DeploymentDetailResponseStatus::UpdatePending => DeploymentStatus::UpdatePending,
        types::DeploymentDetailResponseStatus::InitialSetupFailed => {
            DeploymentStatus::InitialSetupFailed
        }
        types::DeploymentDetailResponseStatus::ProvisioningFailed => {
            DeploymentStatus::ProvisioningFailed
        }
        types::DeploymentDetailResponseStatus::RefreshFailed => DeploymentStatus::RefreshFailed,
        types::DeploymentDetailResponseStatus::UpdateFailed => DeploymentStatus::UpdateFailed,
        types::DeploymentDetailResponseStatus::DeleteFailed => DeploymentStatus::DeleteFailed,
        types::DeploymentDetailResponseStatus::DeletePending => DeploymentStatus::DeletePending,
        types::DeploymentDetailResponseStatus::Deleting => DeploymentStatus::Deleting,
        types::DeploymentDetailResponseStatus::Deleted => DeploymentStatus::Deleted,
    }
}

fn status_from_api(status: types::DeploymentStatus) -> DeploymentStatus {
    match status {
        types::DeploymentStatus::Pending => DeploymentStatus::Pending,
        types::DeploymentStatus::Provisioning => DeploymentStatus::Provisioning,
        types::DeploymentStatus::InitialSetup => DeploymentStatus::InitialSetup,
        types::DeploymentStatus::Running => DeploymentStatus::Running,
        types::DeploymentStatus::Updating => DeploymentStatus::Updating,
        types::DeploymentStatus::UpdatePending => DeploymentStatus::UpdatePending,
        types::DeploymentStatus::InitialSetupFailed => DeploymentStatus::InitialSetupFailed,
        types::DeploymentStatus::ProvisioningFailed => DeploymentStatus::ProvisioningFailed,
        types::DeploymentStatus::RefreshFailed => DeploymentStatus::RefreshFailed,
        types::DeploymentStatus::UpdateFailed => DeploymentStatus::UpdateFailed,
        types::DeploymentStatus::DeleteFailed => DeploymentStatus::DeleteFailed,
        types::DeploymentStatus::DeletePending => DeploymentStatus::DeletePending,
        types::DeploymentStatus::Deleting => DeploymentStatus::Deleting,
        types::DeploymentStatus::Deleted => DeploymentStatus::Deleted,
    }
}

fn platform_from_api_list(platform: types::DeploymentListItemResponsePlatform) -> DeploymentPlatform {
    match platform {
        types::DeploymentListItemResponsePlatform::Aws => DeploymentPlatform::Aws,
        types::DeploymentListItemResponsePlatform::Gcp => DeploymentPlatform::Gcp,
        types::DeploymentListItemResponsePlatform::Azure => DeploymentPlatform::Azure,
        types::DeploymentListItemResponsePlatform::Local => DeploymentPlatform::Local,
        types::DeploymentListItemResponsePlatform::Kubernetes => DeploymentPlatform::Kubernetes,
        types::DeploymentListItemResponsePlatform::Test => DeploymentPlatform::Test,
    }
}

fn platform_from_api_detail(platform: types::DeploymentDetailResponsePlatform) -> DeploymentPlatform {
    match platform {
        types::DeploymentDetailResponsePlatform::Aws => DeploymentPlatform::Aws,
        types::DeploymentDetailResponsePlatform::Gcp => DeploymentPlatform::Gcp,
        types::DeploymentDetailResponsePlatform::Azure => DeploymentPlatform::Azure,
        types::DeploymentDetailResponsePlatform::Local => DeploymentPlatform::Local,
        types::DeploymentDetailResponsePlatform::Kubernetes => DeploymentPlatform::Kubernetes,
        types::DeploymentDetailResponsePlatform::Test => DeploymentPlatform::Test,
    }
}

fn platform_from_api(platform: types::DeploymentPlatform) -> DeploymentPlatform {
    match platform {
        types::DeploymentPlatform::Aws => DeploymentPlatform::Aws,
        types::DeploymentPlatform::Gcp => DeploymentPlatform::Gcp,
        types::DeploymentPlatform::Azure => DeploymentPlatform::Azure,
        types::DeploymentPlatform::Local => DeploymentPlatform::Local,
        types::DeploymentPlatform::Kubernetes => DeploymentPlatform::Kubernetes,
        types::DeploymentPlatform::Test => DeploymentPlatform::Test,
    }
}
