use std::time::Duration;

use alien_aws_clients::iam::{CreateRoleRequest, CreateRoleTag};
use alien_core::{
    standard_resource_tags, RemoteBindings, RemoteBindingsOutputs, ResourceOutputs, ResourceStatus,
};
use alien_error::{AlienError, Context, ContextError};
use alien_macros::controller;

use crate::{
    core::ResourceControllerContext,
    error::{ErrorData, Result},
};

#[controller]
pub struct AwsRemoteBindingsController {
    pub(crate) role_arn: Option<String>,
    pub(crate) role_name: Option<String>,
    pub(crate) external_id: Option<String>,
}

#[controller]
impl AwsRemoteBindingsController {
    #[flow_entry(Create)]
    #[handler(state = CreatingRole, on_failure = CreateFailed, status = ResourceStatus::Provisioning)]
    async fn creating_role(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<RemoteBindings>()?;
        let management = ctx.get_aws_management_config()?.ok_or_else(|| {
            AlienError::new(ErrorData::InfrastructureError {
                message: "AWS management configuration is required for Remote Bindings".to_string(),
                operation: Some("create_remote_bindings_identity".to_string()),
                resource_id: Some(config.id.clone()),
            })
        })?;
        let role_name = format!("{}-remote-bindings", ctx.resource_prefix);
        let external_id = ctx.resource_prefix.to_string();
        let client = ctx
            .service_provider
            .get_aws_iam_client(ctx.get_aws_config()?)
            .await?;
        let response = client
            .create_role(
                CreateRoleRequest::builder()
                    .role_name(role_name.clone())
                    .assume_role_policy_document(trust_policy(
                        &management.managing_role_arn,
                        &external_id,
                    ))
                    .description(format!(
                        "Remote Bindings identity. Resource prefix: {}.",
                        ctx.resource_prefix
                    ))
                    .tags(
                        standard_resource_tags(ctx.resource_prefix, &config.id)
                            .into_iter()
                            .map(|(key, value)| CreateRoleTag { key, value })
                            .collect(),
                    )
                    .build(),
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to create Remote Bindings IAM role '{role_name}'"),
                resource_id: Some(config.id.clone()),
            })?;
        self.role_arn = Some(response.create_role_result.role.arn);
        self.role_name = Some(role_name);
        self.external_id = Some(external_id);
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    #[handler(state = Ready, on_failure = RefreshFailed, status = ResourceStatus::Running)]
    async fn ready(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let resource_id = ctx.desired_resource_config::<RemoteBindings>()?.id.clone();
        let role_name = self.role_name.as_deref().ok_or_else(|| {
            AlienError::new(ErrorData::InfrastructureError {
                resource_id: Some(resource_id.clone()),
                operation: Some("read_remote_bindings_identity".to_string()),
                message: "Remote Bindings role name is missing".to_string(),
            })
        })?;
        ctx.service_provider
            .get_aws_iam_client(ctx.get_aws_config()?)
            .await?
            .get_role(role_name)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to read Remote Bindings IAM role".to_string(),
                resource_id: Some(resource_id),
            })?;
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: Some(Duration::from_secs(30)),
        })
    }

    #[flow_entry(Update, from = [Ready, RefreshFailed])]
    #[handler(state = UpdateStart, on_failure = UpdateFailed, status = ResourceStatus::Updating)]
    async fn update_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let resource_id = ctx.desired_resource_config::<RemoteBindings>()?.id.clone();
        let role_name = self.role_name.as_deref().ok_or_else(|| {
            AlienError::new(ErrorData::InfrastructureError {
                resource_id: Some(resource_id.clone()),
                operation: Some("update_remote_bindings_identity".to_string()),
                message: "Remote Bindings role name is missing".to_string(),
            })
        })?;
        let management = ctx.get_aws_management_config()?.ok_or_else(|| {
            AlienError::new(ErrorData::InfrastructureError {
                message: "AWS management configuration is required for Remote Bindings".to_string(),
                operation: Some("update_remote_bindings_identity".to_string()),
                resource_id: Some(resource_id.clone()),
            })
        })?;
        let external_id = ctx.resource_prefix.to_string();
        ctx.service_provider
            .get_aws_iam_client(ctx.get_aws_config()?)
            .await?
            .update_assume_role_policy(
                role_name,
                &trust_policy(&management.managing_role_arn, &external_id),
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to update Remote Bindings IAM role trust".to_string(),
                resource_id: Some(resource_id),
            })?;
        self.external_id = Some(external_id);
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    #[flow_entry(Delete)]
    #[handler(state = DeleteStart, on_failure = DeleteFailed, status = ResourceStatus::Deleting)]
    async fn delete_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let resource_id = ctx.desired_resource_config::<RemoteBindings>()?.id.clone();
        if let Some(role_name) = self.role_name.as_deref() {
            let client = ctx
                .service_provider
                .get_aws_iam_client(ctx.get_aws_config()?)
                .await?;
            match client.list_role_policies(role_name).await {
                Ok(response) => {
                    if let Some(policy_names) = response.list_role_policies_result.policy_names {
                        for policy_name in &policy_names.member {
                            client
                                .delete_role_policy(role_name, policy_name)
                                .await
                                .context(ErrorData::CloudPlatformError {
                                    message: format!(
                                        "Failed to remove Remote Bindings policy '{policy_name}'"
                                    ),
                                    resource_id: Some(resource_id.clone()),
                                })?;
                        }
                    }
                }
                Err(error)
                    if matches!(
                        error.error,
                        Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    self.role_name = None;
                    self.role_arn = None;
                    self.external_id = None;
                    return Ok(HandlerAction::Continue {
                        state: Deleted,
                        suggested_delay: None,
                    });
                }
                Err(error) => {
                    return Err(error
                        .context(ErrorData::CloudPlatformError {
                            message: "Failed to list Remote Bindings policies".to_string(),
                            resource_id: Some(resource_id.clone()),
                        })
                        .into());
                }
            }
            match client.delete_role(role_name).await {
                Ok(_) => {}
                Err(error)
                    if matches!(
                        error.error,
                        Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. })
                    ) => {}
                Err(error) => {
                    return Err(error
                        .context(ErrorData::CloudPlatformError {
                            message: "Failed to delete Remote Bindings IAM role".to_string(),
                            resource_id: Some(resource_id),
                        })
                        .into())
                }
            }
        }
        self.role_name = None;
        self.role_arn = None;
        self.external_id = None;
        Ok(HandlerAction::Continue {
            state: Deleted,
            suggested_delay: None,
        })
    }

    terminal_state!(
        state = CreateFailed,
        status = ResourceStatus::ProvisionFailed
    );
    terminal_state!(state = UpdateFailed, status = ResourceStatus::UpdateFailed);
    terminal_state!(state = DeleteFailed, status = ResourceStatus::DeleteFailed);
    terminal_state!(
        state = RefreshFailed,
        status = ResourceStatus::RefreshFailed
    );
    terminal_state!(state = Deleted, status = ResourceStatus::Deleted);

    fn build_outputs(&self) -> Option<ResourceOutputs> {
        Some(ResourceOutputs::new(RemoteBindingsOutputs {
            resource_id: self.role_arn.clone()?,
            access_configuration: self.role_arn.clone()?,
            external_id: self.external_id.clone(),
        }))
    }
}

fn trust_policy(managing_role_arn: &str, external_id: &str) -> String {
    serde_json::json!({
        "Version": "2012-10-17",
        "Statement": [{
            "Sid": "AllowOnlySpecificSourceRole",
            "Effect": "Allow",
            "Principal": { "AWS": managing_role_arn },
            "Action": "sts:AssumeRole",
            "Condition": { "StringEquals": {
                "aws:PrincipalArn": managing_role_arn,
                "sts:ExternalId": external_id,
            }},
        }],
    })
    .to_string()
}
