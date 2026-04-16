use crate::{
    error::{ErrorData, Result},
    variables::VariableInterpolator,
    BindingTarget, PermissionContext,
};
use alien_core::PermissionSet;
use serde::{Deserialize, Serialize};

/// GCP custom role definition
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GcpCustomRole {
    /// Human-readable role title
    pub title: String,
    /// Description of what the role allows
    pub description: String,
    /// Role stage (GA, BETA, ALPHA)
    pub stage: String,
    /// List of GCP permissions included in this role
    pub included_permissions: Vec<String>,
    /// Full GCP role name (projects/{project}/roles/{roleId})
    pub name: String,
}

/// GCP IAM binding condition
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GcpIamCondition {
    /// Human-readable condition title
    pub title: String,
    /// Description of the condition
    pub description: String,
    /// CEL expression for the condition
    pub expression: String,
}

/// GCP IAM policy binding
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GcpIamBinding {
    /// Role to bind to members
    pub role: String,
    /// List of members (users, service accounts, groups)
    pub members: Vec<String>,
    /// Optional condition for conditional IAM
    #[serde(skip_serializing_if = "Option::is_none")]
    pub condition: Option<GcpIamCondition>,
}

/// GCP IAM bindings wrapper
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GcpIamBindings {
    /// List of IAM bindings
    pub bindings: Vec<GcpIamBinding>,
}

/// GCP runtime permissions generator for custom roles and IAM bindings
pub struct GcpRuntimePermissionsGenerator;

impl GcpRuntimePermissionsGenerator {
    /// Create a new GCP runtime permissions generator
    pub fn new() -> Self {
        Self
    }

    /// Generate a GCP custom role from a permission set
    ///
    /// Takes a PermissionSet and produces GCP custom role definitions
    /// that can be created at runtime.
    pub fn generate_custom_role(
        &self,
        permission_set: &PermissionSet,
        context: &PermissionContext,
    ) -> Result<GcpCustomRole> {
        let gcp_platform_permissions = permission_set.platforms.gcp.as_ref().ok_or_else(|| {
            alien_error::AlienError::new(ErrorData::PlatformNotSupported {
                platform: "gcp".to_string(),
                permission_set_id: permission_set.id.clone(),
            })
        })?;

        // For custom role generation, we aggregate all permissions from all platform permissions
        let mut all_permissions = Vec::new();

        for platform_permission in gcp_platform_permissions {
            if let Some(permissions) = &platform_permission.grant.permissions {
                all_permissions.extend(permissions.clone());
            }
        }

        if all_permissions.is_empty() {
            return Err(alien_error::AlienError::new(ErrorData::GeneratorError {
                platform: "gcp".to_string(),
                message: "GCP permission grant must have 'permissions' field".to_string(),
            }));
        }

        let role_name = self.generate_role_name(&permission_set.id);
        let role_id = self.generate_role_id(&permission_set.id);

        // Get project from context for full role name
        let project = context.project_name.as_deref().unwrap_or("PROJECT_NAME");
        let full_role_name = format!("projects/{}/roles/{}", project, role_id);

        Ok(GcpCustomRole {
            title: role_name,
            description: permission_set.description.clone(),
            stage: "GA".to_string(),
            included_permissions: all_permissions,
            name: full_role_name,
        })
    }

    /// Generate IAM bindings from permission set and binding target
    ///
    /// Takes a PermissionSet and binding target, produces GCP IAM bindings
    /// that can be applied to resources at runtime.
    pub fn generate_bindings(
        &self,
        permission_set: &PermissionSet,
        binding_target: BindingTarget,
        context: &PermissionContext,
    ) -> Result<GcpIamBindings> {
        let gcp_platform_permissions = permission_set.platforms.gcp.as_ref().ok_or_else(|| {
            alien_error::AlienError::new(ErrorData::PlatformNotSupported {
                platform: "gcp".to_string(),
                permission_set_id: permission_set.id.clone(),
            })
        })?;

        let role_id = self.generate_role_id(&permission_set.id);
        let project = context.project_name.as_deref().unwrap_or("PROJECT_NAME");
        let full_role_name = format!("projects/{}/roles/{}", project, role_id);

        // For this example, we'll use a placeholder service account
        let service_account = format!(
            "serviceAccount:{}@{}.iam.gserviceaccount.com",
            context
                .service_account_name
                .as_deref()
                .unwrap_or("SERVICE_ACCOUNT"),
            project
        );

        let mut bindings: Vec<GcpIamBinding> = Vec::new();

        // All entries in a permission set share the same custom role. We only
        // need one binding per unique condition — entries without conditions
        // collapse into a single unconditional binding.
        let mut has_unconditional = false;

        for platform_permission in gcp_platform_permissions {
            let binding_spec = match binding_target {
                BindingTarget::Stack => match platform_permission.binding.stack.as_ref() {
                    Some(spec) => spec,
                    None => continue,
                },
                BindingTarget::Resource => match platform_permission.binding.resource.as_ref() {
                    Some(spec) => spec,
                    None => continue,
                },
            };

            if let Some(gcp_condition) = &binding_spec.condition {
                let interpolated_condition = self.interpolate_condition(gcp_condition, context)?;
                let condition = GcpIamCondition {
                    title: interpolated_condition.title.clone(),
                    description: format!("Limit to {}", interpolated_condition.title),
                    expression: interpolated_condition.expression,
                };

                // Only add if we don't already have a binding with this condition
                let already_exists = bindings.iter().any(|b| {
                    b.condition
                        .as_ref()
                        .map(|c| c.expression == condition.expression)
                        .unwrap_or(false)
                });
                if !already_exists {
                    bindings.push(GcpIamBinding {
                        role: full_role_name.clone(),
                        members: vec![service_account.clone()],
                        condition: Some(condition),
                    });
                }
            } else if !has_unconditional {
                has_unconditional = true;
                bindings.push(GcpIamBinding {
                    role: full_role_name.clone(),
                    members: vec![service_account.clone()],
                    condition: None,
                });
            }
        }

        Ok(GcpIamBindings { bindings })
    }

    /// Generate a human-readable role name
    fn generate_role_name(&self, permission_set_id: &str) -> String {
        permission_set_id
            .split('/')
            .map(|part| {
                part.split('-')
                    .map(|word| {
                        let mut chars = word.chars();
                        match chars.next() {
                            None => String::new(),
                            Some(first) => {
                                first.to_uppercase().collect::<String>() + chars.as_str()
                            }
                        }
                    })
                    .collect::<Vec<String>>()
                    .join(" ")
            })
            .collect::<Vec<String>>()
            .join(" ")
    }

    /// Generate a valid GCP role ID
    fn generate_role_id(&self, permission_set_id: &str) -> String {
        // Convert to camelCase and remove special characters for valid GCP role ID
        let all_parts: Vec<&str> = permission_set_id
            .split('/')
            .flat_map(|part| part.split('-'))
            .collect();

        all_parts
            .iter()
            .enumerate()
            .map(|(i, word)| {
                if i == 0 {
                    word.to_lowercase()
                } else {
                    let mut chars = word.chars();
                    match chars.next() {
                        None => String::new(),
                        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                    }
                }
            })
            .collect::<String>()
    }

    /// Interpolate variables in a GCP condition
    fn interpolate_condition(
        &self,
        condition: &alien_core::GcpCondition,
        context: &PermissionContext,
    ) -> Result<alien_core::GcpCondition> {
        let interpolated_title =
            VariableInterpolator::interpolate_variables(&condition.title, context)?;
        let interpolated_expression =
            VariableInterpolator::interpolate_variables(&condition.expression, context)?;

        Ok(alien_core::GcpCondition {
            title: interpolated_title,
            expression: interpolated_expression,
        })
    }
}

impl Default for GcpRuntimePermissionsGenerator {
    fn default() -> Self {
        Self::new()
    }
}
