use std::num::NonZeroU64;

use alien_error::{AlienError, Context};
use alien_platform_api::{types, SdkResultExt as _};

use crate::error::{ErrorData, Result};
use crate::execution_context::{ExecutionMode, ManagerContext};

pub struct ResolvedDeployment {
    pub detail: types::DeploymentDetailResponse,
    pub manager: ManagerContext,
}

pub async fn resolve_with_manager(
    ctx: &ExecutionMode,
    reference: &str,
    project_override: Option<&str>,
    allow_prompt: bool,
) -> Result<ResolvedDeployment> {
    let workspace = ctx.resolve_workspace_with_bootstrap(allow_prompt).await?;
    let client = ctx.sdk_client().await?;
    let detail = resolve(
        ctx,
        &client,
        &workspace,
        reference,
        project_override,
        allow_prompt,
    )
    .await?;
    let manager_id = String::from(detail.manager_id.clone());
    let manager = ctx.connect_manager_by_id(&manager_id, &workspace).await?;
    Ok(ResolvedDeployment { detail, manager })
}

pub async fn resolve(
    ctx: &ExecutionMode,
    client: &alien_platform_api::Client,
    workspace: &str,
    reference: &str,
    project_override: Option<&str>,
    allow_prompt: bool,
) -> Result<types::DeploymentDetailResponse> {
    let id = if reference.starts_with("dep_") {
        reference.to_string()
    } else {
        let Some((group_name, deployment_name)) = reference.split_once('/') else {
            return Err(AlienError::new(ErrorData::ValidationError {
                field: "deployment".to_string(),
                message:
                    "Use a deployment ID like dep_... or <deployment-group-name>/<deployment-name>."
                        .to_string(),
            }));
        };
        let (project_id, _) = ctx.resolve_project(project_override, allow_prompt).await?;
        let groups = client
            .list_deployment_groups()
            .workspace(workspace)
            .project(project_id.as_str())
            .search(group_name)
            .limit(NonZeroU64::new(50).expect("constant is non-zero"))
            .send()
            .await
            .into_sdk_error()
            .context(ErrorData::ApiRequestFailed {
                message: format!("Failed to resolve deployment group {group_name}"),
                url: None,
            })?
            .into_inner()
            .items;
        let group = groups
            .into_iter()
            .find(|group| String::from(group.name.clone()) == group_name)
            .ok_or_else(|| {
                AlienError::new(ErrorData::ValidationError {
                    field: "deployment".to_string(),
                    message: format!("Deployment group '{group_name}' was not found."),
                })
            })?;
        let group_id = String::from(group.id);
        let deployments = client
            .list_deployments()
            .workspace(workspace)
            .project(project_id.as_str())
            .deployment_group(group_id.as_str())
            .search(deployment_name)
            .limit(NonZeroU64::new(50).expect("constant is non-zero"))
            .send()
            .await
            .into_sdk_error()
            .context(ErrorData::ApiRequestFailed {
                message: format!("Failed to resolve deployment {reference}"),
                url: None,
            })?
            .into_inner()
            .items;
        String::from(
            deployments
                .into_iter()
                .find(|item| item.name == deployment_name)
                .ok_or_else(|| {
                    AlienError::new(ErrorData::ValidationError {
                        field: "deployment".to_string(),
                        message: format!(
                            "Deployment '{deployment_name}' was not found in group '{group_name}'."
                        ),
                    })
                })?
                .id,
        )
    };

    client
        .get_deployment()
        .id(id.as_str())
        .workspace(workspace)
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: format!("Failed to get deployment {id}"),
            url: None,
        })
        .map(|response| response.into_inner())
}
