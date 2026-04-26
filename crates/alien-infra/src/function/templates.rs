use async_trait::async_trait;
use tracing::info;

use crate::error::{ErrorData, Result};
use crate::{AwsFunctionController, AwsFunctionState, ResourceController};
use alien_core::{Function, FunctionTrigger, Ingress, Resource, ResourceDefinition};
use alien_error::AlienError;

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

        // Extract the ARN from the function name using the account ID and region from context
        let region = &context.aws_config.region;
        let account_id = &context.aws_config.account_id;
        let arn = Some(format!(
            "arn:aws:lambda:{}:{}:function:{}",
            region, account_id, function_name
        ));

        info!(name=%function_name, "Importing Lambda function state from CloudFormation");

        // Import API Gateway V2 state for public ingress functions
        let mut api_id = None;
        let mut url = None;
        let mut stage_name = None;

        if function.ingress == Ingress::Public {
            let api_logical_id = format!("{}Api", logical_id);
            if let Some(api_physical_id) = context.cfn_resources.get(&api_logical_id) {
                api_id = Some(api_physical_id.clone());
                url = Some(format!(
                    "https://{}.execute-api.{}.amazonaws.com",
                    api_physical_id, region
                ));
                stage_name = Some("$default".to_string());
            }
        }

        // Import event source mappings from CloudFormation
        let mut event_source_mappings = Vec::new();
        for (trigger_index, trigger) in function.triggers.iter().enumerate() {
            if let FunctionTrigger::Queue { .. } = trigger {
                let esm_logical_id = format!("{}QueueTrigger{}", logical_id, trigger_index);
                if let Some(esm_physical_id) = context.cfn_resources.get(&esm_logical_id) {
                    event_source_mappings.push(esm_physical_id.clone());
                }
            }
        }

        // Import EventBridge rule names from CloudFormation
        let mut eventbridge_rule_names = Vec::new();
        let mut schedule_index = 0usize;
        for trigger in &function.triggers {
            if let FunctionTrigger::Schedule { .. } = trigger {
                let rule_logical_id =
                    format!("{}ScheduleTrigger{}", logical_id, schedule_index);
                if let Some(rule_physical_id) = context.cfn_resources.get(&rule_logical_id) {
                    eventbridge_rule_names.push(rule_physical_id.clone());
                }
                schedule_index += 1;
            }
        }

        Ok(Box::new(AwsFunctionController {
            state: AwsFunctionState::Ready,
            function_name: Some(function_name.to_string()),
            arn,
            url,
            event_source_mappings,
            fqdn: None,
            certificate_id: None,
            certificate_arn: None,
            api_id,
            integration_id: None,
            route_id: None,
            stage_name,
            api_mapping_id: None,
            domain_name: None,
            load_balancer: None,
            certificate_issued_at: None,
            uses_custom_domain: false,
            s3_permission_statement_ids: Vec::new(),
            eventbridge_rule_names,
            eventbridge_permission_statement_ids: Vec::new(),
            _internal_stay_count: None,
        }))
    }
}
