use alien_aws_clients::DynamoDbApi as _;
use async_trait::async_trait;
use tracing::info;

use crate::error::{ErrorData, Result};
use crate::{AwsKvController, ResourceController};
use alien_aws_clients::dynamodb::{DescribeTableRequest, DynamoDbClient};
use alien_core::{Kv, Resource};
use alien_error::{AlienError, Context};

/// CloudFormation importer for AWS KV resources (DynamoDB)
#[derive(Debug, Clone, Default)]
pub struct AwsKvCloudFormationImporter;

#[async_trait]
impl crate::cloudformation::traits::CloudFormationResourceImporter for AwsKvCloudFormationImporter {
    async fn import_cloudformation_state(
        &self,
        resource: &Resource,
        context: &crate::cloudformation::traits::CloudFormationImportContext,
    ) -> Result<Box<dyn ResourceController>> {
        use crate::cloudformation::utils::sanitize_to_pascal_case;
        use tracing::info;

        let kv = resource.downcast_ref::<Kv>().ok_or_else(|| {
            AlienError::new(ErrorData::ControllerResourceTypeMismatch {
                expected: Kv::RESOURCE_TYPE,
                actual: resource.resource_type(),
                resource_id: resource.id().to_string(),
            })
        })?;

        // Generate logical ID the same way as generator.rs
        let logical_id = sanitize_to_pascal_case(kv.id());

        let physical_id = context.cfn_resources.get(&logical_id).ok_or_else(|| {
            AlienError::new(ErrorData::CloudFormationResourceMissing {
                logical_id: logical_id.clone(),
                stack_name: context.aws_config.region.clone(), // We don't have stack name in context, using region as placeholder
                resource_id: Some(kv.id().to_string()),
            })
        })?;

        info!(table_name=%physical_id, "Importing DynamoDB table state from CloudFormation");

        // Create our custom DynamoDB client using the AWS config from context
        let client = DynamoDbClient::new(reqwest::Client::new(), context.aws_config.clone());

        // Verify the table exists using describe_table.
        // This AWS API call is used because it provides detailed table information
        // and requires dynamodb:DescribeTable permission, which is typically included
        // in management-level roles.
        let describe_response = client
            .describe_table(
                DescribeTableRequest::builder()
                    .table_name(physical_id.clone())
                    .build(),
            )
            .await
            .context(ErrorData::InfrastructureImportFailed {
                message: format!(
                    "Failed to confirm existence of DynamoDB table '{}' via DescribeTable",
                    physical_id.clone()
                ),
                import_source: Some("CloudFormation".to_string()),
                resource_id: Some(kv.id().to_string()),
            })?;

        // Extract table ARN from the describe response
        let table_arn = describe_response.table.table_arn.ok_or_else(|| {
            AlienError::new(ErrorData::InfrastructureImportFailed {
                message: format!(
                    "DynamoDB table '{}' missing ARN in describe response",
                    physical_id
                ),
                import_source: Some("CloudFormation".to_string()),
                resource_id: Some(kv.id().to_string()),
            })
        })?;

        // Create controller in ready state with imported data
        Ok(Box::new(AwsKvController {
            state: crate::kv::aws::AwsKvState::Ready,
            table_name: Some(physical_id.clone()),
            table_arn: Some(table_arn),
            _internal_stay_count: None,
        }))
    }
}
