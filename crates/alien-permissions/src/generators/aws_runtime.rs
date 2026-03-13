use crate::{
    error::{ErrorData, Result},
    variables::VariableInterpolator,
    BindingTarget, PermissionContext,
};
use alien_core::PermissionSet;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

/// AWS IAM policy statement
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
pub struct AwsIamStatement {
    /// Statement ID
    pub sid: String,
    /// Effect (Allow/Deny)
    pub effect: String,
    /// List of IAM actions
    pub action: Vec<String>,
    /// List of resource ARNs
    pub resource: Vec<String>,
    /// Optional conditions
    #[serde(skip_serializing_if = "Option::is_none")]
    pub condition: Option<IndexMap<String, IndexMap<String, String>>>,
}

/// AWS IAM policy document
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
pub struct AwsIamPolicy {
    /// Policy version
    pub version: String,
    /// List of policy statements
    pub statement: Vec<AwsIamStatement>,
}

/// AWS runtime permissions generator for IAM policy documents
pub struct AwsRuntimePermissionsGenerator;

impl AwsRuntimePermissionsGenerator {
    /// Create a new AWS runtime permissions generator
    pub fn new() -> Self {
        Self
    }

    /// Generate an IAM policy document from a permission set and binding target
    ///
    /// Takes a PermissionSet and where to bind it, produces AWS IAM policy documents
    /// that can be created at runtime.
    pub fn generate_policy(
        &self,
        permission_set: &PermissionSet,
        binding_target: BindingTarget,
        context: &PermissionContext,
    ) -> Result<AwsIamPolicy> {
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
                VariableInterpolator::interpolate_string_list(&binding_spec.resources, context)?;
            let conditions = self.extract_conditions(binding_spec, context)?;

            let statement_id = if aws_platform_permissions.len() > 1 {
                format!(
                    "{}{}",
                    self.generate_statement_id(&permission_set.id),
                    index + 1
                )
            } else {
                self.generate_statement_id(&permission_set.id)
            };

            let statement = AwsIamStatement {
                sid: statement_id,
                effect: "Allow".to_string(),
                action: actions.clone(),
                resource: resources,
                condition: if conditions.is_empty() {
                    None
                } else {
                    Some(conditions)
                },
            };

            statements.push(statement);
        }

        Ok(AwsIamPolicy {
            version: "2012-10-17".to_string(),
            statement: statements,
        })
    }

    /// Extract AWS conditions from binding spec
    fn extract_conditions(
        &self,
        binding_spec: &alien_core::AwsBindingSpec,
        context: &PermissionContext,
    ) -> Result<IndexMap<String, IndexMap<String, String>>> {
        if let Some(condition_template) = &binding_spec.condition {
            let mut interpolated_conditions = IndexMap::new();

            for (condition_key, condition_values) in condition_template {
                let mut interpolated_values = IndexMap::new();

                for (value_key, value_template) in condition_values {
                    let interpolated_value =
                        VariableInterpolator::interpolate_variables(value_template, context)?;
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

impl Default for AwsRuntimePermissionsGenerator {
    fn default() -> Self {
        Self::new()
    }
}
