use alien_aws_clients::S3Api as _;
use async_trait::async_trait;
use tracing::info;

use crate::error::{ErrorData, Result};
use crate::{AwsStorageController, AwsStorageState, ResourceController};
use alien_aws_clients::s3::S3Client;
use alien_aws_clients::AwsCredentialProvider;
use alien_core::{Resource, Storage};
use alien_error::{AlienError, Context};

/// CloudFormation importer for AWS Storage resources
#[derive(Debug, Clone, Default)]
pub struct AwsStorageCloudFormationImporter;

#[async_trait]
impl crate::cloudformation::traits::CloudFormationResourceImporter
    for AwsStorageCloudFormationImporter
{
    async fn import_cloudformation_state(
        &self,
        resource: &Resource,
        context: &crate::cloudformation::traits::CloudFormationImportContext,
    ) -> Result<Box<dyn ResourceController>> {
        use crate::cloudformation::utils::sanitize_to_pascal_case;
        use tracing::info;

        let storage = resource.downcast_ref::<Storage>().ok_or_else(|| {
            AlienError::new(ErrorData::ControllerResourceTypeMismatch {
                expected: Storage::RESOURCE_TYPE,
                actual: resource.resource_type(),
                resource_id: resource.id().to_string(),
            })
        })?;

        // Generate logical ID the same way as generator.rs
        let logical_id = sanitize_to_pascal_case(storage.id());

        let physical_id = context.cfn_resources.get(&logical_id).ok_or_else(|| {
            AlienError::new(ErrorData::CloudFormationResourceMissing {
                logical_id: logical_id.clone(),
                stack_name: context.aws_config.region.clone(), // We don't have stack name in context, using region as placeholder
                resource_id: Some(storage.id().to_string()),
            })
        })?;

        info!(bucket=%physical_id, "Importing S3 bucket state from CloudFormation");

        // Create our custom S3 client using the AWS config from context
        let credentials = AwsCredentialProvider::from_config(context.aws_config.clone())
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to create AWS credential provider".to_string(),
                resource_id: None,
            })?;
        let client = S3Client::new(reqwest::Client::new(), credentials);

        // Verify the bucket exists using get_bucket_location.
        // This AWS API call is used because it requires s3:GetBucketLocation permission,
        // which is typically included in 'Management' level roles, unlike s3:ListBucket
        // required by head_bucket.
        client
            .get_bucket_location(physical_id.as_str())
            .await
            .context(ErrorData::InfrastructureImportFailed {
                message: format!(
                    "Failed to confirm existence of bucket '{}' via GetBucketLocation",
                    physical_id.clone()
                ),
                import_source: Some("CloudFormation".to_string()),
                resource_id: Some(storage.id().to_string()),
            })?;

        // If public_read, check if the corresponding bucket policy resource exists in CFN outputs.
        if storage.public_read {
            let policy_logical_id = format!("{}Policy", logical_id);
            if !context.cfn_resources.contains_key(&policy_logical_id) {
                return Err(AlienError::new(ErrorData::CloudFormationResourceMissing {
                    logical_id: policy_logical_id,
                    stack_name: context.aws_config.region.clone(), // We don't have stack name in context, using region as placeholder
                    resource_id: Some(storage.id().to_string()),
                }));
            }
            // Again, could verify with client.get_bucket_policy but skipping for now.
        }

        // Similarly, could check for lifecycle rules if needed.

        Ok(Box::new(AwsStorageController {
            state: AwsStorageState::Ready,
            bucket_name: Some(physical_id.clone()),
            _internal_stay_count: None,
        }))
    }
}
