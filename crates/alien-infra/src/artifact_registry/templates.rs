use alien_aws_clients::CloudFormationApi as _;
use async_trait::async_trait;

use crate::cloudformation::traits::{CloudFormationImportContext, CloudFormationResourceImporter};
use crate::error::{ErrorData, Result};
use crate::ResourceController;
use alien_aws_clients::{
    cloudformation::{CloudFormationClient, DescribeStacksRequest},
    AwsClientConfig,
};
use alien_core::{ArtifactRegistry, Resource};
use alien_error::AlienError;

/// CloudFormation importer for ArtifactRegistry resources.
///
/// Since AWS ECR registries are implicit, this importer sets up the outputs
/// based on the AWS account and region information.
#[derive(Debug, Default)]
pub struct AwsArtifactRegistryCloudFormationImporter;

#[async_trait]
impl CloudFormationResourceImporter for AwsArtifactRegistryCloudFormationImporter {
    async fn import_cloudformation_state(
        &self,
        resource: &Resource,
        context: &CloudFormationImportContext,
    ) -> Result<Box<dyn ResourceController>> {
        let registry = resource.downcast_ref::<ArtifactRegistry>().ok_or_else(|| {
            AlienError::new(ErrorData::ControllerResourceTypeMismatch {
                expected: ArtifactRegistry::RESOURCE_TYPE,
                actual: resource.resource_type(),
                resource_id: resource.id().to_string(),
            })
        })?;

        // Construct the role ARNs directly from stack information instead of relying on outputs
        // This matches the role names created in the CloudFormation template:
        // Pull role: ${AWS::StackName}-{registry_id}-pull
        // Push role: ${AWS::StackName}-{registry_id}-push

        // In cross-account scenarios, the AWS config contains the managing account ID,
        // but the CloudFormation resources (including IAM roles) are created in the target account.
        // We need to extract the target account ID from the CloudFormation stack ARN.
        let target_account_id =
            extract_account_id_from_stack_name(&context.aws_config, &context.stack_name)
                .await
                .unwrap_or_else(|| context.aws_config.account_id.clone());
        let stack_name = &context.stack_name;

        let pull_role_arn = Some(format!(
            "arn:aws:iam::{}:role/{}-{}-pull",
            target_account_id,
            stack_name,
            registry.id()
        ));

        let push_role_arn = Some(format!(
            "arn:aws:iam::{}:role/{}-{}-push",
            target_account_id,
            stack_name,
            registry.id()
        ));

        let region = &context.aws_config.region;

        let controller = crate::artifact_registry::AwsArtifactRegistryController {
            state: crate::artifact_registry::aws::AwsArtifactRegistryState::Ready,
            account_id: Some(target_account_id.clone()),
            region: Some(region.clone()),
            pull_role_arn,
            push_role_arn,
            repository_prefix: Some(format!("{}-{}", stack_name, registry.id())),
            _internal_stay_count: None,
        };

        Ok(Box::new(controller))
    }
}

/// Helper function to extract the target account ID from a CloudFormation stack
/// In cross-account scenarios, the CloudFormation stack is deployed in the target account,
/// so we can extract the account ID from the stack ARN.
async fn extract_account_id_from_stack_name(
    aws_config: &AwsClientConfig,
    stack_name: &str,
) -> Option<String> {
    let cfn_client = CloudFormationClient::new(reqwest::Client::new(), aws_config.clone());

    let describe_request = DescribeStacksRequest::builder()
        .stack_name(stack_name.to_string())
        .build();

    match cfn_client.describe_stacks(describe_request).await {
        Ok(response) => {
            // Extract account ID from stack ARN: arn:aws:cloudformation:region:account-id:stack/...
            response
                .describe_stacks_result
                .stacks
                .member
                .first()
                .and_then(|stack| stack.stack_id.split(':').nth(4).map(|s| s.to_string()))
        }
        Err(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloudformation::traits::CloudFormationImportContext;
    use alien_aws_clients::AwsClientConfig;
    use alien_core::Resource;
    use std::collections::HashMap;

    fn basic_artifact_registry() -> Resource {
        let registry = ArtifactRegistry::new("my-registry".to_string()).build();
        Resource::new(registry)
    }

    fn mock_import_context() -> CloudFormationImportContext {
        CloudFormationImportContext {
            cfn_resources: HashMap::new(),
            aws_config: AwsClientConfig {
                credentials: alien_aws_clients::AwsCredentials::AccessKeys {
                    access_key_id: "test-key".to_string(),
                    secret_access_key: "test-secret".to_string(),
                    session_token: None,
                },
                region: "us-west-2".to_string(),
                account_id: "123456789012".to_string(),
                service_overrides: None,
            },
            resource_prefix: "test-stack".to_string(),
            stack_name: "test-stack".to_string(),
        }
    }

    #[tokio::test]
    async fn test_import_cloudformation_state_creates_controller() {
        let registry = basic_artifact_registry();
        let context = mock_import_context();
        let importer = AwsArtifactRegistryCloudFormationImporter;

        let result = importer
            .import_cloudformation_state(&registry, &context)
            .await;

        assert!(result.is_ok());
        let _controller = result.unwrap();
    }
}
