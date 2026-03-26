use async_trait::async_trait;

use crate::error::{ErrorData, Result};
use alien_aws_clients::AwsCredentialProvider;
use alien_core::Resource;
use alien_core::ServiceAccount;
use alien_error::{AlienError, Context};

/// CloudFormation importer for AWS ServiceAccount resources
#[derive(Debug, Clone, Default)]
pub struct AwsServiceAccountCloudFormationImporter;

#[async_trait]
impl crate::cloudformation::traits::CloudFormationResourceImporter
    for AwsServiceAccountCloudFormationImporter
{
    async fn import_cloudformation_state(
        &self,
        resource: &Resource,
        context: &crate::cloudformation::traits::CloudFormationImportContext,
    ) -> Result<Box<dyn crate::core::ResourceController>> {
        use alien_aws_clients::iam::{IamApi, IamClient};
        use tracing::info;

        let service_account = resource.downcast_ref::<ServiceAccount>().ok_or_else(|| {
            AlienError::new(ErrorData::ControllerResourceTypeMismatch {
                expected: ServiceAccount::RESOURCE_TYPE,
                actual: resource.resource_type(),
                resource_id: resource.id().to_string(),
            })
        })?;

        // The logical ID for ServiceAccount is typically the resource ID or a derived name
        // We need to find the CloudFormation logical ID for this ServiceAccount
        let mut role_logical_id = None;
        let mut physical_id = None;

        // Look for a role that matches the expected naming pattern
        // ServiceAccount roles are typically named with the pattern: {stack-name}-{service-account-id}
        let expected_role_name = format!("{}-{}", context.stack_name, service_account.id);
        for (logical_id, resource_physical_id) in &context.cfn_resources {
            // Check if this is exactly the ServiceAccount role we're looking for
            // Use exact match to avoid partial matches that could cause wrong role assignment
            if *resource_physical_id == expected_role_name {
                role_logical_id = Some(logical_id.clone());
                physical_id = Some(resource_physical_id.clone());
                break;
            }
        }

        let role_name = match (role_logical_id, physical_id) {
            (Some(_logical_id), Some(physical_id)) => physical_id,
            _ => {
                return Err(AlienError::new(ErrorData::CloudFormationResourceMissing {
                    logical_id: format!("ServiceAccount role for '{}'", service_account.id),
                    stack_name: context.stack_name.clone(),
                    resource_id: Some(service_account.id().to_string()),
                }));
            }
        };

        // Create IAM client to verify the role exists and get its ARN
        let credentials = AwsCredentialProvider::from_config(context.aws_config.clone())
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to create AWS credential provider".to_string(),
                resource_id: None,
            })?;
        let client = IamClient::new(reqwest::Client::new(), credentials);

        info!(role_name=%role_name, service_account_id=%service_account.id, "Importing ServiceAccount IAM role state from CloudFormation");
        let role_result =
            client
                .get_role(&role_name)
                .await
                .context(ErrorData::InfrastructureImportFailed {
                    message: format!(
                        "Failed to get IAM role '{}' for ServiceAccount '{}'",
                        role_name, service_account.id
                    ),
                    import_source: Some("CloudFormation".to_string()),
                    resource_id: Some(service_account.id().to_string()),
                })?;

        let role_arn = role_result.get_role_result.role.arn;

        // Create the controller in ready state
        Ok(Box::new(
            crate::service_account::AwsServiceAccountController {
                state: crate::service_account::aws::AwsServiceAccountState::Ready,
                role_arn: Some(role_arn),
                role_name: Some(role_name),
                stack_permissions_applied: true,
                _internal_stay_count: None,
            },
        ))
    }
}
