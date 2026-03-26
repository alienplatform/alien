use alien_aws_clients::IamApi as _;
use alien_aws_clients::AwsCredentialProvider;
use async_trait::async_trait;

use crate::error::{ErrorData, Result};
use alien_core::RemoteStackManagement;
use alien_core::Resource;
use alien_error::{AlienError, Context};

/// CloudFormation importer for AWS RemoteStackManagement resources
#[derive(Debug, Clone, Default)]
pub struct AwsRemoteStackManagementCloudFormationImporter;

#[async_trait]
impl crate::cloudformation::traits::CloudFormationResourceImporter
    for AwsRemoteStackManagementCloudFormationImporter
{
    async fn import_cloudformation_state(
        &self,
        resource: &Resource,
        context: &crate::cloudformation::traits::CloudFormationImportContext,
    ) -> Result<Box<dyn crate::core::ResourceController>> {
        use alien_aws_clients::iam::IamClient;
        use tracing::info;

        let remote_stack_mgmt = resource
            .downcast_ref::<RemoteStackManagement>()
            .ok_or_else(|| {
                AlienError::new(ErrorData::ControllerResourceTypeMismatch {
                    expected: RemoteStackManagement::RESOURCE_TYPE,
                    actual: resource.resource_type(),
                    resource_id: resource.id().to_string(),
                })
            })?;

        // The management role is always named "ManagementRole" in CloudFormation
        let role_logical_id = "ManagementRole";

        let physical_id = context.cfn_resources.get(role_logical_id).ok_or_else(|| {
            AlienError::new(ErrorData::CloudFormationResourceMissing {
                logical_id: role_logical_id.to_string(),
                stack_name: context.stack_name.clone(),
                resource_id: Some(remote_stack_mgmt.id().to_string()),
            })
        })?;

        let role_name = physical_id.as_str();

        // Create IAM client to verify the role exists and get its ARN
        let credentials = AwsCredentialProvider::from_config(context.aws_config.clone())
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to create AWS credential provider".to_string(),
                resource_id: None,
            })?;
        let client = IamClient::new(reqwest::Client::new(), credentials);

        info!(role_name=%role_name, "Importing RemoteStackManagement IAM role state from CloudFormation");
        let role_result =
            client
                .get_role(role_name)
                .await
                .context(ErrorData::InfrastructureImportFailed {
                    message: format!(
                        "Failed to get IAM role '{}' for RemoteStackManagement",
                        role_name
                    ),
                    import_source: Some("CloudFormation".to_string()),
                    resource_id: Some(remote_stack_mgmt.id().to_string()),
                })?;

        let role_arn = role_result.get_role_result.role.arn;

        // Create the controller in ready state
        Ok(Box::new(
            crate::remote_stack_management::AwsRemoteStackManagementController {
                state: crate::remote_stack_management::AwsRemoteStackManagementState::Ready,
                role_arn: Some(role_arn),
                role_name: Some(role_name.to_string()),
                management_permissions_applied: true,
                _internal_stay_count: None,
            },
        ))
    }
}
