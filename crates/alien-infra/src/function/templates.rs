use async_trait::async_trait;
use tracing::{debug, warn};

use crate::error::{ErrorData, Result};
use crate::{AwsFunctionController, AwsFunctionState, ResourceController};
use alien_aws_clients::lambda::{LambdaApi, LambdaClient};
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::{Function, Ingress, Resource, ResourceDefinition};
use alien_error::{AlienError, Context, ContextError};

/// CloudFormation importer for AWS Function resources
#[derive(Debug, Clone, Default)]
pub struct AwsFunctionCloudFormationImporter;

#[async_trait]
impl crate::cloudformation::traits::CloudFormationResourceImporter
    for AwsFunctionCloudFormationImporter
{
    async fn import_cloudformation_state(
        &self,
        resource: &Resource,
        context: &crate::cloudformation::traits::CloudFormationImportContext,
    ) -> Result<Box<dyn ResourceController>> {
        use crate::cloudformation::utils::sanitize_to_pascal_case;
        use tracing::info;

        let function = resource.downcast_ref::<Function>().ok_or_else(|| {
            AlienError::new(ErrorData::ControllerResourceTypeMismatch {
                expected: Function::RESOURCE_TYPE,
                actual: resource.resource_type(),
                resource_id: resource.id().to_string(),
            })
        })?;

        // Generate logical ID the same way as generator.rs
        let logical_id = sanitize_to_pascal_case(function.id());

        let physical_id = context.cfn_resources.get(&logical_id).ok_or_else(|| {
            AlienError::new(ErrorData::CloudFormationResourceMissing {
                logical_id: logical_id.clone(),
                stack_name: "unknown".to_string(),
                resource_id: Some(function.id.clone()),
            })
        })?;

        let function_name = physical_id.as_str();

        // Create our custom Lambda client using the AWS config from context
        let client = LambdaClient::new(reqwest::Client::new(), context.aws_config.clone());

        info!(name=%function_name, "Importing Lambda function state from CloudFormation");
        let function_result = client
            .get_function_configuration(function_name, None)
            .await
            .context(ErrorData::InfrastructureImportFailed {
                message: format!(
                    "Failed to get function configuration for '{}'",
                    function_name
                ),
                import_source: Some("CloudFormation".to_string()),
                resource_id: Some(function.id.clone()),
            })?;

        let arn = function_result.function_arn.clone();

        // Try to get the function URL configuration if needed
        let mut url = None;
        if function.ingress == Ingress::Public {
            match client.get_function_url_config(function_name, None).await {
                Ok(url_config) => {
                    url = Some(url_config.function_url);
                }
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    // URL config doesn't exist, which is expected for some functions
                    debug!(name=%function_name, "Function URL configuration not found");
                }
                Err(e) => {
                    return Err(e.context(ErrorData::InfrastructureImportFailed {
                        message: format!(
                            "Failed to get function URL configuration for '{}'",
                            function_name
                        ),
                        import_source: Some("CloudFormation".to_string()),
                        resource_id: Some(function.id.clone()),
                    }));
                }
            }
        }

        // For functions with HTTP ingress, also check for the permission statement
        if function.ingress == Ingress::Public && url.is_some() {
            match client.get_policy(function_name, None).await {
                Ok(_policy) => {
                    // Policy exists, which is good
                }
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    warn!(name=%function_name, "Function URL exists but no permission policy found");
                }
                Err(e) => {
                    return Err(e.context(ErrorData::InfrastructureImportFailed {
                        message: format!(
                            "Failed to get Lambda policy for function '{}'",
                            function_name
                        ),
                        import_source: Some("CloudFormation".to_string()),
                        resource_id: Some(function.id.clone()),
                    }));
                }
            }
        }

        Ok(Box::new(AwsFunctionController {
            state: AwsFunctionState::Ready,
            function_name: Some(function_name.to_string()),
            arn,
            url,
            event_source_mappings: Vec::new(), // Event source mappings are not imported from CloudFormation
            fqdn: None,
            certificate_id: None,
            certificate_arn: None,
            api_id: None,
            integration_id: None,
            route_id: None,
            stage_name: None,
            api_mapping_id: None,
            domain_name: None,
            load_balancer: None,
            certificate_issued_at: None,
            uses_custom_domain: false,
            _internal_stay_count: None,
        }))
    }
}
