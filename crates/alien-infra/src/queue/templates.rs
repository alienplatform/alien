use async_trait::async_trait;

use crate::error::{ErrorData, Result};
use alien_core::{Queue, Resource};
use alien_error::{AlienError, Context};

/// CloudFormation importer for AWS Queue resources (SQS)
#[derive(Debug, Clone, Default)]
pub struct AwsQueueCloudFormationImporter;

#[async_trait]
impl crate::cloudformation::traits::CloudFormationResourceImporter
    for AwsQueueCloudFormationImporter
{
    async fn import_cloudformation_state(
        &self,
        resource: &Resource,
        context: &crate::cloudformation::traits::CloudFormationImportContext,
    ) -> Result<Box<dyn crate::core::ResourceController>> {
        use crate::cloudformation::utils::sanitize_to_pascal_case;
        use alien_aws_clients::sqs::{GetQueueUrlRequest, SqsApi, SqsClient};
        use tracing::info;

        let queue = resource.downcast_ref::<Queue>().ok_or_else(|| {
            AlienError::new(ErrorData::ControllerResourceTypeMismatch {
                expected: Queue::RESOURCE_TYPE,
                actual: resource.resource_type(),
                resource_id: resource.id().to_string(),
            })
        })?;

        let logical_id = sanitize_to_pascal_case(queue.id());
        let physical_name = context.cfn_resources.get(&logical_id).ok_or_else(|| {
            AlienError::new(ErrorData::CloudFormationResourceMissing {
                logical_id: logical_id.clone(),
                stack_name: context.aws_config.region.clone(),
                resource_id: Some(queue.id().to_string()),
            })
        })?;

        info!(queue_name=%physical_name, "Importing SQS queue state from CloudFormation");

        let client = SqsClient::new(reqwest::Client::new(), context.aws_config.clone());
        let url = client
            .get_queue_url(
                GetQueueUrlRequest::builder()
                    .queue_name(physical_name.clone())
                    .build(),
            )
            .await
            .context(ErrorData::InfrastructureImportFailed {
                message: format!(
                    "Failed to resolve SQS queue URL for '{}' via GetQueueUrl",
                    physical_name
                ),
                import_source: Some("CloudFormation".to_string()),
                resource_id: Some(queue.id().to_string()),
            })?
            .get_queue_url_result
            .queue_url;

        Ok(Box::new(crate::queue::aws::AwsQueueController {
            state: crate::queue::aws::AwsQueueState::Ready,
            queue_url: Some(url),
            queue_name: Some(physical_name.clone()),
            _internal_stay_count: None,
        }))
    }
}
