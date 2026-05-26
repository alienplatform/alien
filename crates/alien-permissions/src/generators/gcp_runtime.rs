use crate::{
    error::{ErrorData, Result},
    generators::labels::{
        entry_description, entry_snake_label, entry_title_label, has_explicit_label,
    },
    variables::VariableInterpolator,
    BindingTarget, PermissionContext,
};
use alien_core::{GcpBindingSpec, PermissionGrant, PermissionSet};
use serde::{Deserialize, Serialize};

/// GCP IAM binding condition.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GcpIamCondition {
    /// Human-readable condition title.
    pub title: String,
    /// Description of the condition.
    pub description: String,
    /// CEL expression for the condition.
    pub expression: String,
}

/// GCP custom role definition.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GcpCustomRole {
    /// GCP role ID.
    pub role_id: String,
    /// Fully-qualified role name.
    pub name: String,
    /// Human-readable title.
    pub title: String,
    /// Role description.
    pub description: String,
    /// Permissions included in the custom role.
    pub included_permissions: Vec<String>,
    /// Role launch stage.
    pub stage: String,
}

/// Scope where a GCP IAM role binding should be applied.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum GcpBindingTargetScope {
    /// Bind the role on the target project.
    Project,
    /// Bind the role on the current resource IAM policy.
    CurrentResource,
}

/// Resource family for current-resource IAM bindings that need provider-specific routing.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum GcpBindingResourceKind {
    /// Pub/Sub topic IAM policy.
    PubsubTopic,
    /// Pub/Sub subscription IAM policy.
    PubsubSubscription,
    /// Artifact Registry repository IAM policy.
    ArtifactRegistryRepository,
}

/// GCP IAM policy binding.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GcpIamBinding {
    /// Role to bind to members.
    pub role: String,
    /// List of members (users, service accounts, groups).
    pub members: Vec<String>,
    /// IAM policy scope where this role should be bound.
    pub target: GcpBindingTargetScope,
    /// Resource family for current-resource IAM policy routing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource_kind: Option<GcpBindingResourceKind>,
    /// Optional condition for conditional IAM.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub condition: Option<GcpIamCondition>,
}

/// GCP IAM bindings wrapper.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GcpIamBindings {
    /// List of IAM bindings.
    pub bindings: Vec<GcpIamBinding>,
}

/// GCP grant plan generated from a permission set.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GcpGrantPlan {
    /// Predefined and generated custom-role bindings.
    pub bindings: Vec<GcpIamBinding>,
    /// Residual custom roles that must exist before their bindings are applied.
    pub custom_roles: Vec<GcpCustomRole>,
}

impl GcpGrantPlan {
    /// Return IAM bindings selected for one binding target scope.
    pub fn bindings_for_target(&self, target: GcpBindingTargetScope) -> Vec<GcpIamBinding> {
        self.bindings
            .iter()
            .filter(|binding| binding.target == target)
            .cloned()
            .collect()
    }

    /// Return only the custom roles referenced by `bindings`.
    pub fn custom_roles_for_bindings(&self, bindings: &[GcpIamBinding]) -> Vec<GcpCustomRole> {
        self.custom_roles
            .iter()
            .filter(|custom_role| {
                bindings
                    .iter()
                    .any(|binding| binding.role == custom_role.name)
            })
            .cloned()
            .collect()
    }
}

/// GCP custom-role planner.
pub struct GcpRuntimePermissionsGenerator;

impl GcpRuntimePermissionsGenerator {
    /// Create a new GCP runtime permissions generator.
    pub fn new() -> Self {
        Self
    }

    /// Generate a custom role from a permission set.
    ///
    /// GCP uses project custom roles for exact permission-set semantics. The
    /// role ID is derived from the deployment namespace and permission-set ID,
    /// so different service accounts in the same deployment share one role per
    /// permission-set entry without sharing roles across deployments.
    pub fn generate_custom_role(
        &self,
        permission_set: &PermissionSet,
        context: &PermissionContext,
    ) -> Result<GcpCustomRole> {
        let roles = self.generate_custom_roles(permission_set, context)?;
        if roles.len() == 1 {
            return Ok(roles.into_iter().next().expect("single role"));
        }
        if roles.is_empty() {
            return Err(alien_error::AlienError::new(ErrorData::GeneratorError {
                platform: "gcp".to_string(),
                message: format!(
                    "GCP permission set '{}' has no residual custom role",
                    permission_set.id
                ),
            }));
        }

        Err(alien_error::AlienError::new(ErrorData::GeneratorError {
            platform: "gcp".to_string(),
            message: format!(
                "GCP permission set '{}' generates multiple custom roles; use generate_custom_roles() to preserve binding scopes",
                permission_set.id
            ),
        }))
    }

    /// Generate one custom role per unique GCP permission entry.
    ///
    /// Permission-set JSONC can split GCP permissions into multiple entries
    /// when some permissions must be bound at project scope and others at a
    /// resource scope. Keeping those entries as separate custom roles prevents
    /// project-scoped helper permissions from broadening resource permissions,
    /// and vice versa.
    pub fn generate_custom_roles(
        &self,
        permission_set: &PermissionSet,
        context: &PermissionContext,
    ) -> Result<Vec<GcpCustomRole>> {
        let gcp_platform_permissions = permission_set.platforms.gcp.as_ref().ok_or_else(|| {
            alien_error::AlienError::new(ErrorData::PlatformNotSupported {
                platform: "gcp".to_string(),
                permission_set_id: permission_set.id.clone(),
            })
        })?;

        if gcp_platform_permissions.is_empty() {
            return Err(alien_error::AlienError::new(ErrorData::GeneratorError {
                platform: "gcp".to_string(),
                message: format!(
                    "GCP permission set '{}' has no platform entries",
                    permission_set.id
                ),
            }));
        }

        let mut roles: Vec<GcpCustomRole> = Vec::new();
        let residual_entry_count = gcp_platform_permissions
            .iter()
            .filter(|entry| gcp_residual_permissions(&entry.grant).is_some_and(|p| !p.is_empty()))
            .count();
        let has_multiple_entries = residual_entry_count > 1;
        for (index, platform_permission) in gcp_platform_permissions.iter().enumerate() {
            validate_gcp_grant(permission_set, index, &platform_permission.grant)?;

            let Some(permissions) = gcp_residual_permissions(&platform_permission.grant) else {
                continue;
            };
            if permissions.is_empty() {
                continue;
            }
            let role = self.custom_role_for_permissions(
                permission_set,
                permissions.clone(),
                context,
                role_suffix(
                    &platform_permission.grant,
                    platform_permission.label.as_deref(),
                    has_multiple_entries,
                ),
                platform_permission.label.as_deref(),
                platform_permission.description.as_deref(),
            )?;
            if !roles
                .iter()
                .any(|existing| existing.role_id == role.role_id)
            {
                roles.push(role);
            }
        }

        Ok(roles)
    }

    fn custom_role_for_permissions(
        &self,
        permission_set: &PermissionSet,
        mut included_permissions: Vec<String>,
        context: &PermissionContext,
        suffix: Option<String>,
        explicit_label: Option<&str>,
        entry_description_override: Option<&str>,
    ) -> Result<GcpCustomRole> {
        included_permissions.sort();
        included_permissions.dedup();

        let project = context.project_name.as_deref().unwrap_or("PROJECT_NAME");
        let role_id = generate_role_id(permission_set, context, suffix.as_deref(), explicit_label);
        let role_name = format!("projects/{project}/roles/{role_id}");

        Ok(GcpCustomRole {
            role_id: role_id.clone(),
            name: role_name,
            title: custom_role_title(permission_set, context, suffix.as_deref(), explicit_label),
            description: custom_role_description(
                permission_set,
                context,
                entry_description_override,
            ),
            included_permissions,
            stage: "GA".to_string(),
        })
    }

    /// Generate IAM bindings from a permission set and binding target.
    pub fn generate_bindings(
        &self,
        permission_set: &PermissionSet,
        binding_target: BindingTarget,
        context: &PermissionContext,
    ) -> Result<GcpIamBindings> {
        Ok(GcpIamBindings {
            bindings: self
                .generate_grant_plan(permission_set, binding_target, context)?
                .bindings,
        })
    }

    /// Generate the full GCP grant plan from a permission set and binding target.
    pub fn generate_grant_plan(
        &self,
        permission_set: &PermissionSet,
        binding_target: BindingTarget,
        context: &PermissionContext,
    ) -> Result<GcpGrantPlan> {
        let gcp_platform_permissions = permission_set.platforms.gcp.as_ref().ok_or_else(|| {
            alien_error::AlienError::new(ErrorData::PlatformNotSupported {
                platform: "gcp".to_string(),
                permission_set_id: permission_set.id.clone(),
            })
        })?;

        if gcp_platform_permissions.is_empty() {
            return Err(alien_error::AlienError::new(ErrorData::GeneratorError {
                platform: "gcp".to_string(),
                message: format!(
                    "GCP permission set '{}' has no platform entries",
                    permission_set.id
                ),
            }));
        }

        let project = context.project_name.as_deref().unwrap_or("PROJECT_NAME");
        let service_account = format!(
            "serviceAccount:{}@{}.iam.gserviceaccount.com",
            context
                .service_account_name
                .as_deref()
                .unwrap_or("SERVICE_ACCOUNT"),
            project
        );

        let mut bindings = Vec::new();
        let mut custom_roles = Vec::new();
        let residual_entry_count = gcp_platform_permissions
            .iter()
            .filter(|entry| gcp_residual_permissions(&entry.grant).is_some_and(|p| !p.is_empty()))
            .count();
        let has_multiple_entries = residual_entry_count > 1;
        for (index, platform_permission) in gcp_platform_permissions.iter().enumerate() {
            validate_gcp_grant(permission_set, index, &platform_permission.grant)?;

            let binding_spec = match binding_target {
                BindingTarget::Stack => platform_permission.binding.stack.as_ref(),
                BindingTarget::Resource => platform_permission.binding.resource.as_ref(),
            };

            let Some(binding_spec) = binding_spec else {
                continue;
            };

            let target = binding_target_scope(binding_spec);
            let resource_kind = binding_resource_kind(binding_spec);
            let condition = self.binding_condition(binding_spec, context)?;

            if let Some(predefined_roles) = &platform_permission.grant.predefined_roles {
                for predefined_role in predefined_roles {
                    bindings.push(GcpIamBinding {
                        role: predefined_role.clone(),
                        members: vec![service_account.clone()],
                        target,
                        resource_kind,
                        condition: condition.clone(),
                    });
                }
            }

            let Some(permissions) = gcp_residual_permissions(&platform_permission.grant) else {
                continue;
            };
            if permissions.is_empty() {
                continue;
            }

            let custom_role = self.custom_role_for_permissions(
                permission_set,
                permissions.clone(),
                context,
                role_suffix(
                    &platform_permission.grant,
                    platform_permission.label.as_deref(),
                    has_multiple_entries,
                ),
                platform_permission.label.as_deref(),
                platform_permission.description.as_deref(),
            )?;
            bindings.push(GcpIamBinding {
                role: custom_role.name.clone(),
                members: vec![service_account.clone()],
                target,
                resource_kind,
                condition,
            });
            if !custom_roles
                .iter()
                .any(|existing: &GcpCustomRole| existing.role_id == custom_role.role_id)
            {
                custom_roles.push(custom_role);
            }
        }

        Ok(GcpGrantPlan {
            bindings: dedupe_bindings(bindings),
            custom_roles,
        })
    }

    fn binding_condition(
        &self,
        binding_spec: &GcpBindingSpec,
        context: &PermissionContext,
    ) -> Result<Option<GcpIamCondition>> {
        let Some(gcp_condition) = binding_spec.condition.as_ref() else {
            return Ok(None);
        };

        let interpolated = self.interpolate_condition(gcp_condition, context)?;
        Ok(Some(GcpIamCondition {
            title: interpolated.title.clone(),
            description: format!("Limit to {}", interpolated.title),
            expression: interpolated.expression,
        }))
    }

    /// Interpolate variables in a GCP condition.
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

fn gcp_residual_permissions(grant: &PermissionGrant) -> Option<&Vec<String>> {
    grant
        .residual_permissions
        .as_ref()
        .or(grant.permissions.as_ref())
}

fn validate_gcp_grant(
    permission_set: &PermissionSet,
    index: usize,
    grant: &PermissionGrant,
) -> Result<()> {
    if let Some(predefined_roles) = &grant.predefined_roles {
        if predefined_roles.is_empty() {
            return Err(alien_error::AlienError::new(ErrorData::GeneratorError {
                platform: "gcp".to_string(),
                message: format!(
                    "GCP permission set '{}' entry {} has an empty predefinedRoles list",
                    permission_set.id, index
                ),
            }));
        }
        for role in predefined_roles {
            if !role.starts_with("roles/") {
                return Err(alien_error::AlienError::new(ErrorData::GeneratorError {
                    platform: "gcp".to_string(),
                    message: format!(
                        "GCP permission set '{}' entry {} has invalid predefined role '{}'",
                        permission_set.id, index, role
                    ),
                }));
            }
        }
    }

    if let Some(permissions) = gcp_residual_permissions(grant) {
        if permissions.is_empty() {
            return Err(alien_error::AlienError::new(ErrorData::GeneratorError {
                platform: "gcp".to_string(),
                message: format!(
                    "GCP permission set '{}' entry {} has an empty permissions list",
                    permission_set.id, index
                ),
            }));
        }
    }

    if grant.predefined_roles.is_none() && gcp_residual_permissions(grant).is_none() {
        return Err(alien_error::AlienError::new(ErrorData::GeneratorError {
            platform: "gcp".to_string(),
            message: format!(
                "GCP permission set '{}' entry {} has no permissions",
                permission_set.id, index
            ),
        }));
    }

    Ok(())
}

fn generate_role_id(
    permission_set: &PermissionSet,
    context: &PermissionContext,
    suffix: Option<&str>,
    explicit_label: Option<&str>,
) -> String {
    let namespace = custom_role_namespace(context);
    if has_explicit_label(explicit_label) {
        let suffix = suffix.unwrap_or("custom");
        return format!("role_{namespace}_{suffix}");
    }

    let permission_set_slug = sanitize_role_segment(&permission_set.id.replace('/', "_"), 28);
    match suffix {
        Some(suffix) => format!("role_{namespace}_{permission_set_slug}_{suffix}"),
        None => format!("role_{namespace}_{permission_set_slug}"),
    }
}

/// Return the project custom-role prefix for all roles owned by this stack.
pub fn custom_role_prefix(context: &PermissionContext) -> String {
    format!("role_{}_", custom_role_namespace(context))
}

/// Return the project custom-role prefix for one permission set in this stack.
pub fn custom_role_permission_set_prefix(
    permission_set_id: &str,
    context: &PermissionContext,
) -> String {
    let permission_set_slug = sanitize_role_segment(&permission_set_id.replace('/', "_"), 28);
    format!("{}{permission_set_slug}", custom_role_prefix(context))
}

fn custom_role_namespace(context: &PermissionContext) -> String {
    sanitize_role_segment(context.stack_prefix.as_deref().unwrap_or("stack"), 18)
}

fn custom_role_title(
    permission_set: &PermissionSet,
    context: &PermissionContext,
    suffix: Option<&str>,
    explicit_label: Option<&str>,
) -> String {
    let deployment_name = context.deployment_name.as_deref();
    if has_explicit_label(explicit_label) {
        let title = entry_title_label(explicit_label, &PermissionGrant::default());
        return role_title(deployment_name, &title);
    }

    let label = permission_set_display_label(&permission_set.id);
    let title = match suffix {
        Some(suffix) => format!(
            "{label} - {}",
            entry_title_label(Some(suffix), &PermissionGrant::default())
        ),
        None => label,
    };
    role_title(deployment_name, &title)
}

fn custom_role_description(
    permission_set: &PermissionSet,
    context: &PermissionContext,
    entry_description_override: Option<&str>,
) -> String {
    let stack_prefix = context.stack_prefix.as_deref().unwrap_or("unknown");
    let description = entry_description(entry_description_override, &permission_set.description);
    let description = description.trim_end_matches('.');
    match context.deployment_name.as_deref() {
        Some(deployment_name) if !deployment_name.trim().is_empty() => {
            format!("Used by {deployment_name}. {description}. Resource prefix: {stack_prefix}.")
        }
        _ => format!("{description}. Resource prefix: {stack_prefix}."),
    }
}

fn role_title(deployment_name: Option<&str>, title: &str) -> String {
    match deployment_name {
        Some(deployment_name) if !deployment_name.trim().is_empty() => {
            format!("{deployment_name}: {title}")
        }
        _ => title.to_string(),
    }
}

fn permission_set_display_label(permission_set_id: &str) -> String {
    let mut words = Vec::new();
    let mut current = String::new();

    for ch in permission_set_id.chars() {
        if ch.is_ascii_alphanumeric() {
            current.push(ch.to_ascii_lowercase());
        } else if !current.is_empty() {
            words.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        words.push(current);
    }

    let mut label = words.join(" ");
    if let Some(first) = label.get_mut(0..1) {
        first.make_ascii_uppercase();
    }
    label
}

fn role_suffix(
    grant: &PermissionGrant,
    explicit_label: Option<&str>,
    has_multiple_entries: bool,
) -> Option<String> {
    if has_explicit_label(explicit_label) || has_multiple_entries {
        let max_len = if has_explicit_label(explicit_label) {
            40
        } else {
            28
        };
        Some(sanitize_role_segment(
            &entry_snake_label(explicit_label, grant),
            max_len,
        ))
    } else {
        None
    }
}

fn sanitize_role_segment(value: &str, max_len: usize) -> String {
    let mut out = String::with_capacity(value.len());
    let mut previous_underscore = false;
    for ch in value.chars() {
        let next = if ch.is_ascii_alphanumeric() {
            ch.to_ascii_lowercase()
        } else {
            '_'
        };
        if next == '_' {
            if !previous_underscore {
                out.push(next);
            }
            previous_underscore = true;
        } else {
            out.push(next);
            previous_underscore = false;
        }
    }

    let trimmed = out.trim_matches('_');
    let mut segment = if trimmed.is_empty() {
        "x".to_string()
    } else {
        trimmed.to_string()
    };
    if segment.len() > max_len {
        segment.truncate(max_len);
        while segment.ends_with('_') {
            segment.pop();
        }
    }
    if segment.is_empty() {
        "x".to_string()
    } else {
        segment
    }
}

fn binding_target_scope(binding_spec: &GcpBindingSpec) -> GcpBindingTargetScope {
    let scope = binding_spec.scope.trim();
    match scope.strip_prefix("projects/") {
        Some(project_scope) if !project_scope.contains('/') => GcpBindingTargetScope::Project,
        _ => GcpBindingTargetScope::CurrentResource,
    }
}

fn binding_resource_kind(binding_spec: &GcpBindingSpec) -> Option<GcpBindingResourceKind> {
    let scope = binding_spec.scope.trim();
    if scope.contains("/topics/") {
        return Some(GcpBindingResourceKind::PubsubTopic);
    }
    if scope.contains("/subscriptions/") {
        return Some(GcpBindingResourceKind::PubsubSubscription);
    }
    if scope.contains("/repositories/") {
        return Some(GcpBindingResourceKind::ArtifactRegistryRepository);
    }
    None
}

fn dedupe_bindings(bindings: Vec<GcpIamBinding>) -> Vec<GcpIamBinding> {
    let mut deduped = Vec::new();
    for binding in bindings {
        if !deduped.contains(&binding) {
            deduped.push(binding);
        }
    }
    deduped
}
