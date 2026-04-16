use crate::{
    error::{ErrorData, Result},
    BindingTarget, PermissionContext,
};
use alien_core::PermissionSet;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};

/// AWS IAM statement for CloudFormation templates
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
pub struct AwsCloudFormationIamStatement {
    /// Statement ID
    pub sid: String,
    /// Effect (Allow/Deny)
    pub effect: String,
    /// List of IAM actions (can be CloudFormation intrinsic functions)
    pub action: Vec<JsonValue>,
    /// List of resource ARNs (can be CloudFormation intrinsic functions)
    pub resource: Vec<JsonValue>,
    /// Optional conditions (can contain CloudFormation intrinsic functions)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub condition: Option<IndexMap<String, IndexMap<String, JsonValue>>>,
}

/// AWS IAM policy document for CloudFormation templates
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
pub struct AwsCloudFormationIamPolicy {
    /// Policy version
    pub version: String,
    /// List of policy statements
    pub statement: Vec<AwsCloudFormationIamStatement>,
}

/// AWS CloudFormation permissions generator for IAM policy documents
pub struct AwsCloudFormationPermissionsGenerator;

impl AwsCloudFormationPermissionsGenerator {
    /// Create a new AWS CloudFormation permissions generator
    pub fn new() -> Self {
        Self
    }

    /// Generate a CloudFormation-compatible IAM policy document from a permission set and binding target
    ///
    /// Takes a PermissionSet and where to bind it, produces AWS IAM policy documents
    /// that can be embedded in CloudFormation templates with intrinsic functions.
    pub fn generate_policy(
        &self,
        permission_set: &PermissionSet,
        binding_target: BindingTarget,
        context: &PermissionContext,
    ) -> Result<AwsCloudFormationIamPolicy> {
        let aws_platform_permissions = permission_set.platforms.aws.as_ref().ok_or_else(|| {
            alien_error::AlienError::new(ErrorData::PlatformNotSupported {
                platform: "aws".to_string(),
                permission_set_id: permission_set.id.clone(),
            })
        })?;

        let mut statements = Vec::new();

        // Process each AWS platform permission in the permission set
        for (index, platform_permission) in aws_platform_permissions.iter().enumerate() {
            let actions = platform_permission.grant.actions.as_ref().ok_or_else(|| {
                alien_error::AlienError::new(ErrorData::GeneratorError {
                    platform: "aws".to_string(),
                    message: "AWS permission grant must have 'actions' field".to_string(),
                })
            })?;

            let binding_spec = match binding_target {
                BindingTarget::Stack => {
                    platform_permission.binding.stack.as_ref().ok_or_else(|| {
                        alien_error::AlienError::new(ErrorData::BindingTargetNotSupported {
                            platform: "aws".to_string(),
                            binding_target: "stack".to_string(),
                            permission_set_id: permission_set.id.clone(),
                        })
                    })?
                }
                BindingTarget::Resource => platform_permission
                    .binding
                    .resource
                    .as_ref()
                    .ok_or_else(|| {
                        alien_error::AlienError::new(ErrorData::BindingTargetNotSupported {
                            platform: "aws".to_string(),
                            binding_target: "resource".to_string(),
                            permission_set_id: permission_set.id.clone(),
                        })
                    })?,
            };

            let resources =
                self.interpolate_cloudformation_resources(&binding_spec.resources, context)?;
            let conditions = self.extract_cloudformation_conditions(binding_spec, context)?;

            let statement_id = if aws_platform_permissions.len() > 1 {
                format!(
                    "{}{}",
                    self.generate_statement_id(&permission_set.id),
                    index + 1
                )
            } else {
                self.generate_statement_id(&permission_set.id)
            };

            // Convert actions to JsonValue (plain strings for now, could be intrinsic functions later)
            // Sort actions to ensure deterministic output
            let mut sorted_actions: Vec<_> = actions.iter().collect();
            sorted_actions.sort();
            let action_values: Vec<JsonValue> = sorted_actions.iter().map(|a| json!(a)).collect();

            let statement = AwsCloudFormationIamStatement {
                sid: statement_id,
                effect: "Allow".to_string(),
                action: action_values,
                resource: resources,
                condition: if conditions.is_empty() {
                    None
                } else {
                    Some(conditions)
                },
            };

            statements.push(statement);
        }

        Ok(AwsCloudFormationIamPolicy {
            version: "2012-10-17".to_string(),
            statement: statements,
        })
    }

    /// Interpolate CloudFormation resource ARNs with intrinsic functions
    fn interpolate_cloudformation_resources(
        &self,
        templates: &[String],
        context: &PermissionContext,
    ) -> Result<Vec<JsonValue>> {
        let mut resources: Result<Vec<JsonValue>> = templates
            .iter()
            .map(|template| self.interpolate_cloudformation_string(template, context))
            .collect();

        // Sort resources for deterministic output
        if let Ok(ref mut resources_vec) = resources {
            resources_vec.sort_by(|a, b| {
                // Convert to string for comparison to ensure deterministic ordering
                let a_str = serde_json::to_string(a).unwrap_or_default();
                let b_str = serde_json::to_string(b).unwrap_or_default();
                a_str.cmp(&b_str)
            });
        }

        resources
    }

    /// Interpolate a CloudFormation string template with variables
    /// Creates CloudFormation intrinsic functions (Fn::Sub, Ref) where appropriate
    fn interpolate_cloudformation_string(
        &self,
        template: &str,
        context: &PermissionContext,
    ) -> Result<JsonValue> {
        // Check if the template contains CloudFormation variables or regular variables
        let contains_cf_vars = template.contains("${AWS::") || template.contains("${!");
        let contains_regular_vars = template.contains("${") && !contains_cf_vars;

        if contains_cf_vars {
            // Template already contains CloudFormation variables, wrap in Fn::Sub
            Ok(json!({
                "Fn::Sub": template
            }))
        } else if contains_regular_vars {
            // Template contains our custom variables that need to be replaced
            let mut result = template.to_string();

            // First, replace our known variables with CloudFormation equivalents or literal values
            if let Some(stack_prefix) = context.stack_prefix.as_ref() {
                if stack_prefix.is_empty() {
                    // Empty stack prefix means just use the stack name
                    result = result.replace("${stackPrefix}", "${AWS::StackName}");
                } else {
                    // Non-empty stack prefix gets appended with a dash
                    result = result.replace(
                        "${stackPrefix}",
                        &format!("${{AWS::StackName}}-{}", stack_prefix),
                    );
                }
            } else {
                result = result.replace("${stackPrefix}", "${AWS::StackName}");
            }

            if let Some(resource_name) = context.resource_name.as_ref() {
                // For resource names in CloudFormation context, we usually want the raw logical ID
                // unless it's in an ARN context where we need to build the full ARN
                if result.contains("arn:aws:") && result.contains("${resourceName}") {
                    // This is an ARN template, replace with the resource name directly
                    result = result.replace("${resourceName}", resource_name);
                } else {
                    // Simple resource reference, just use the name
                    result = result.replace("${resourceName}", resource_name);
                }
            }

            // Handle AWS-specific variables that should map to CloudFormation pseudo parameters
            result = result.replace("${awsRegion}", "${AWS::Region}");
            result = result.replace("${awsAccountId}", "${AWS::AccountId}");

            // Handle external ID
            if let Some(external_id) = context.external_id.as_ref() {
                result = result.replace("${externalId}", external_id);
            }

            // Handle managing account ID - extract from ManagingRoleArn parameter
            // ManagingRoleArn format: arn:aws:iam::123456789012:role/role-name
            // We need to extract the account ID (element 4 when split by ':')
            let needs_managing_account_id = result.contains("${managingAccountId}");
            if needs_managing_account_id {
                result = result.replace("${managingAccountId}", "${ManagingAccountId}");

                // Use Fn::Sub with variable map to extract account ID from role ARN
                return Ok(json!({
                    "Fn::Sub": [
                        result,
                        {
                            "ManagingAccountId": {
                                "Fn::Select": [4, {"Fn::Split": [":", {"Ref": "ManagingRoleArn"}]}]
                            }
                        }
                    ]
                }));
            }

            // If the result still contains CloudFormation variables after our substitutions, wrap in Fn::Sub
            // This includes AWS pseudo parameters (${AWS::...}), CloudFormation parameters (${ParameterName}),
            // and other CloudFormation references (${!...})
            if result.contains("${") {
                Ok(json!({
                    "Fn::Sub": result
                }))
            } else {
                // Just a plain string after substitution
                Ok(json!(result))
            }
        } else {
            // No variables, just return as plain string
            Ok(json!(template))
        }
    }

    /// Extract AWS conditions from binding spec for CloudFormation
    fn extract_cloudformation_conditions(
        &self,
        binding_spec: &alien_core::AwsBindingSpec,
        context: &PermissionContext,
    ) -> Result<IndexMap<String, IndexMap<String, JsonValue>>> {
        if let Some(condition_template) = &binding_spec.condition {
            let mut interpolated_conditions = IndexMap::new();

            // Sort condition keys for deterministic output
            let mut sorted_condition_keys: Vec<_> = condition_template.keys().collect();
            sorted_condition_keys.sort();

            for condition_key in sorted_condition_keys {
                let condition_values = &condition_template[condition_key];
                let mut interpolated_values = IndexMap::new();

                // Sort value keys for deterministic output
                let mut sorted_value_keys: Vec<_> = condition_values.keys().collect();
                sorted_value_keys.sort();

                for value_key in sorted_value_keys {
                    let value_template = &condition_values[value_key];
                    let interpolated_value =
                        self.interpolate_cloudformation_string(value_template, context)?;
                    interpolated_values.insert(value_key.clone(), interpolated_value);
                }

                interpolated_conditions.insert(condition_key.clone(), interpolated_values);
            }

            Ok(interpolated_conditions)
        } else {
            Ok(IndexMap::new())
        }
    }

    /// Generate a valid IAM statement ID from a permission set ID
    fn generate_statement_id(&self, permission_set_id: &str) -> String {
        // Convert to PascalCase and remove special characters for valid AWS Sid
        permission_set_id
            .split('/')
            .map(|part| {
                part.split('-')
                    .map(|word| {
                        let mut chars = word.chars();
                        match chars.next() {
                            None => String::new(),
                            Some(first) => {
                                first.to_uppercase().collect::<String>()
                                    + &chars.as_str().to_lowercase()
                            }
                        }
                    })
                    .collect::<String>()
            })
            .collect::<String>()
    }
}

impl Default for AwsCloudFormationPermissionsGenerator {
    fn default() -> Self {
        Self::new()
    }
}
