use async_trait::async_trait;

use crate::cloudformation::traits::{CloudFormationImportContext, CloudFormationResourceImporter};
use crate::error::{ErrorData, Result};
use crate::{AwsVaultController, ResourceController};
use alien_core::{Resource, Vault};
use alien_error::AlienError;

/// CloudFormation importer for Vault resources.
///
/// Since AWS Secrets Manager exists implicitly, this importer sets up the vault reference
/// based on the AWS account and region information from the CloudFormation context.
#[derive(Debug, Default)]
pub struct AwsVaultCloudFormationImporter;

#[async_trait]
impl CloudFormationResourceImporter for AwsVaultCloudFormationImporter {
    async fn import_cloudformation_state(
        &self,
        resource: &Resource,
        context: &CloudFormationImportContext,
    ) -> Result<Box<dyn ResourceController>> {
        let vault = resource.downcast_ref::<Vault>().ok_or_else(|| {
            AlienError::new(ErrorData::ControllerResourceTypeMismatch {
                expected: Vault::RESOURCE_TYPE,
                actual: resource.resource_type(),
                resource_id: resource.id().to_string(),
            })
        })?;

        // For AWS Secrets Manager, we construct the vault reference from the context
        let account_id = context.aws_config.account_id.clone();
        let region = context.aws_config.region.clone();
        let vault_prefix = format!("{}-{}", context.stack_name, vault.id());

        let controller = crate::vault::AwsVaultController {
            state: crate::vault::AwsVaultState::Ready,
            account_id: Some(account_id),
            region: Some(region),
            vault_prefix: Some(vault_prefix),
            _internal_stay_count: None,
        };

        Ok(Box::new(controller))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloudformation::traits::CloudFormationImportContext;
    use alien_aws_clients::AwsClientConfig;
    use alien_core::Resource;
    use std::collections::HashMap;

    fn basic_vault() -> Resource {
        let vault = Vault::new("my-vault".to_string()).build();
        Resource::new(vault)
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
        let vault = basic_vault();
        let context = mock_import_context();
        let importer = AwsVaultCloudFormationImporter;

        let result = importer.import_cloudformation_state(&vault, &context).await;

        assert!(result.is_ok());
        let _controller = result.unwrap();
    }
}
