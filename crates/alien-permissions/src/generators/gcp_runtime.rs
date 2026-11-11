use crate::{
    error::{ErrorData, Result},
    generators::{
        families::gcp_family,
        labels::{entry_description, entry_snake_label, entry_title_label, has_explicit_label},
        protected_allowlist::is_built_in_unchanged,
    },
    variables::VariableInterpolator,
    BindingTarget, PermissionContext,
};
use alien_core::{GcpBindingSpec, PermissionGrant, PermissionSet};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const GCP_CUSTOM_ROLE_ID_MAX_LEN: usize = 64;
const CUSTOM_ROLE_NAMESPACE_MAX_LEN: usize = 18;
const ROLE_ID_HASH_LEN: usize = 8;

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
    /// Bind on the executing service account's own IAM policy.
    ServiceAccount,
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
    /// Vertex AI reasoning engine IAM policy.
    VertexAiReasoningEngine,
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
    /// Explicit full IAM target for an executing-service-account binding.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_resource_name: Option<String>,
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

        // An empty list is a deliberate "grants nothing on GCP" (for example the Redis
        // management set), as opposed to a missing key, which means the platform is unsupported.
        if gcp_platform_permissions.is_empty() {
            return Ok(Vec::new());
        }

        let mut roles: Vec<GcpCustomRole> = Vec::new();
        let residual_entry_count = gcp_platform_permissions
            .iter()
            .filter(|entry| gcp_residual_permissions(&entry.grant).is_some_and(|p| !p.is_empty()))
            .count();
        let has_multiple_entries = residual_entry_count > 1;
        for (index, platform_permission) in gcp_platform_permissions.iter().enumerate() {
            validate_gcp_grant(permission_set, index, &platform_permission.grant)?;

            let residual = gcp_residual_permissions(&platform_permission.grant);
            let replaced = replaced_role_permissions(&platform_permission.grant, context);
            if residual.is_none() && replaced.is_empty() {
                continue;
            }
            let mut permissions: Vec<String> = residual.cloned().unwrap_or_default();
            permissions.extend(replaced);
            let permissions = granted_permissions(permission_set, &permissions, context)?;
            if permissions.is_empty() {
                continue;
            }
            let role = self.custom_role_for_permissions(
                permission_set,
                permissions,
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

        // See `generate_custom_roles`: an empty list grants nothing.
        if gcp_platform_permissions.is_empty() {
            return Ok(GcpGrantPlan {
                bindings: Vec::new(),
                custom_roles: Vec::new(),
            });
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
            let target_resource_name = if target == GcpBindingTargetScope::ServiceAccount {
                for variable in ["projectName", "serviceAccountName"] {
                    let value = context
                        .get_variable(variable)
                        .filter(|value| !value.trim().is_empty());
                    if value.is_none()
                        || value.is_some_and(|value| {
                            value.contains(&format!("${{{variable}}}"))
                                || matches!(value, "SERVICE_ACCOUNT" | "PROJECT_NAME")
                        })
                    {
                        return Err(alien_error::AlienError::new(ErrorData::GeneratorError {
                            platform: "gcp".to_string(),
                            message: format!(
                                "Executing service-account binding requires a resolved {variable}"
                            ),
                        }));
                    }
                }
                Some(VariableInterpolator::interpolate_variables(
                    &binding_spec.scope,
                    context,
                )?)
            } else {
                None
            };
            let resource_kind = binding_resource_kind(binding_spec);
            let condition = self.binding_condition(binding_spec, context)?;
            if context.signing_protected
                && condition.is_some()
                && !is_built_in_unchanged(permission_set)
            {
                return Err(alien_error::AlienError::new(ErrorData::GeneratorError {
                    platform: "gcp".to_string(),
                    message: format!(
                        "Permission set '{}' puts an IAM condition on a management binding; \
                         a protected install doesn't support conditions the stack supplies",
                        permission_set.id
                    ),
                }));
            }

            // A protected install's management identity holds predefined roles only from the
            // built-in list. A role with a family permission becomes a custom role without it.
            let mut replaced_role_permissions: Vec<String> = Vec::new();
            if let Some(predefined_roles) = &platform_permission.grant.predefined_roles {
                for predefined_role in predefined_roles {
                    if context.signing_protected {
                        if let Some(permissions) =
                            gcp_role_without_family_permissions(predefined_role)
                        {
                            replaced_role_permissions
                                .extend(permissions.iter().map(|p| p.to_string()));
                            continue;
                        }
                        require_built_in_gcp_role(&permission_set.id, predefined_role)?;
                    }
                    bindings.push(GcpIamBinding {
                        role: predefined_role.clone(),
                        members: vec![service_account.clone()],
                        target,
                        target_resource_name: target_resource_name.clone(),
                        resource_kind,
                        condition: protected_secret_condition(
                            condition.clone(),
                            predefined_role.contains("secretmanager"),
                            context,
                        )?,
                    });
                }
            }

            let residual = gcp_residual_permissions(&platform_permission.grant);
            if residual.is_none() && replaced_role_permissions.is_empty() {
                continue;
            }
            let mut permissions: Vec<String> = residual.cloned().unwrap_or_default();
            permissions.extend(replaced_role_permissions);
            let permissions = granted_permissions(permission_set, &permissions, context)?;
            if permissions.is_empty() {
                continue;
            }

            let custom_role = self.custom_role_for_permissions(
                permission_set,
                permissions,
                context,
                role_suffix(
                    &platform_permission.grant,
                    platform_permission.label.as_deref(),
                    has_multiple_entries,
                ),
                platform_permission.label.as_deref(),
                platform_permission.description.as_deref(),
            )?;
            let condition = protected_secret_condition(
                condition,
                custom_role
                    .included_permissions
                    .iter()
                    .any(|permission| permission.starts_with("secretmanager.")),
                context,
            )?;
            bindings.push(GcpIamBinding {
                role: custom_role.name.clone(),
                members: vec![service_account.clone()],
                target,
                target_resource_name,
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

/// The predefined roles the built-in management profile binds on GCP. A predefined role can't be
/// narrowed, so a protected install refuses every other one (a broad role such as `roles/editor`,
/// or one GCP adds later). Roles that hold a family permission are not here: see
/// [`gcp_role_without_family_permissions`].
pub const GCP_BUILT_IN_ROLES: &[&str] = &[
    "roles/artifactregistry.reader",
    "roles/cloudkms.cryptoKeyEncrypterDecrypter",
    "roles/cloudkms.viewer",
    "roles/datastore.user",
    "roles/datastore.viewer",
    "roles/iam.roleViewer",
    "roles/iam.serviceAccountViewer",
    "roles/logging.logWriter",
    "roles/pubsub.publisher",
    "roles/pubsub.viewer",
    "roles/run.invoker",
    "roles/run.viewer",
    "roles/secretmanager.secretAccessor",
    "roles/secretmanager.secretVersionAdder",
    "roles/secretmanager.viewer",
    "roles/serviceusage.serviceUsageViewer",
    "roles/storage.bucketViewer",
    "roles/storage.objectCreator",
    "roles/storage.objectViewer",
];

/// Predefined roles that carry a family permission, with each role's complete permission list.
/// A protected install's management identity gets a custom role setup creates from this list minus
/// the family permissions ([`gcp_role_without_family_permissions`]).
///
/// Taken from the public IAM dataset and checked against the cloud provider's role reference:
/// a predefined role can change, so the snapshot is pinned here and a test fails when a
/// replacement would hold a family permission.
const GCP_ROLES_WITH_FAMILY_PERMISSIONS: &[(&str, &[&str])] = &[
    (
        "roles/storage.objectAdmin",
        &[
            "monitoring.timeSeries.create",
            "orgpolicy.policy.get",
            "resourcemanager.projects.get",
            "resourcemanager.projects.list",
            "storage.folders.create",
            "storage.folders.delete",
            "storage.folders.get",
            "storage.folders.list",
            "storage.folders.rename",
            "storage.managedFolders.create",
            "storage.managedFolders.delete",
            "storage.managedFolders.get",
            "storage.managedFolders.list",
            "storage.managedFolders.update",
            "storage.multipartUploads.abort",
            "storage.multipartUploads.create",
            "storage.multipartUploads.list",
            "storage.multipartUploads.listParts",
            "storage.objects.create",
            "storage.objects.createContext",
            "storage.objects.delete",
            "storage.objects.deleteContext",
            "storage.objects.get",
            "storage.objects.getIamPolicy",
            "storage.objects.list",
            "storage.objects.move",
            "storage.objects.overrideUnlockedRetention",
            "storage.objects.restore",
            "storage.objects.setIamPolicy",
            "storage.objects.setRetention",
            "storage.objects.update",
            "storage.objects.updateContext",
        ],
    ),
    (
        "roles/pubsub.subscriber",
        &[
            "pubsub.snapshots.seek",
            "pubsub.subscriptions.consume",
            "pubsub.topics.attachSubscription",
        ],
    ),
    (
        "roles/serviceusage.serviceUsageAdmin",
        &[
            "cloudquotas.quotas.get",
            "cloudquotas.quotas.update",
            "monitoring.timeSeries.list",
            "serviceusage.catalogs.listMembers",
            "serviceusage.consumerpolicy.analyze",
            "serviceusage.consumerpolicy.get",
            "serviceusage.consumerpolicy.update",
            "serviceusage.contentsecuritypolicy.get",
            "serviceusage.contentsecuritypolicy.update",
            "serviceusage.effectivemcppolicy.get",
            "serviceusage.effectivepolicy.get",
            "serviceusage.groups.list",
            "serviceusage.groups.listExpandedMembers",
            "serviceusage.groups.listMembers",
            "serviceusage.mcppolicy.get",
            "serviceusage.mcppolicy.update",
            "serviceusage.operations.get",
            "serviceusage.quotas.get",
            "serviceusage.quotas.update",
            "serviceusage.services.disable",
            "serviceusage.services.enable",
            "serviceusage.services.get",
            "serviceusage.services.list",
            "serviceusage.services.listApis",
            "serviceusage.services.listDependentServices",
            "serviceusage.services.use",
            "serviceusage.values.test",
        ],
    ),
];

/// The custom-role permissions that replace `role` on a protected install: the role's permissions
/// without the family ones. `None` for a role with no family permission to take out.
pub fn gcp_role_without_family_permissions(role: &str) -> Option<Vec<&'static str>> {
    GCP_ROLES_WITH_FAMILY_PERMISSIONS
        .iter()
        .find(|(name, _)| *name == role)
        .map(|(_, permissions)| {
            permissions
                .iter()
                .copied()
                .filter(|permission| gcp_family(permission).is_none())
                .collect()
        })
}

/// What a protected install's management identity gets instead of the predefined roles in `grant`
/// that carry a family permission.
fn replaced_role_permissions(grant: &PermissionGrant, context: &PermissionContext) -> Vec<String> {
    if !context.signing_protected {
        return Vec::new();
    }
    grant
        .predefined_roles
        .iter()
        .flatten()
        .filter_map(|role| gcp_role_without_family_permissions(role))
        .flat_map(|permissions| permissions.into_iter().map(|p| p.to_string()))
        .collect()
}

fn require_built_in_gcp_role(permission_set_id: &str, role: &str) -> Result<()> {
    if GCP_BUILT_IN_ROLES.contains(&role) {
        return Ok(());
    }
    Err(alien_error::AlienError::new(ErrorData::GeneratorError {
        platform: "gcp".to_string(),
        message: format!(
            "Permission set '{permission_set_id}' binds the predefined role '{role}' to a protected \
             install's management identity; a predefined role can't be narrowed, so only the \
             built-in management roles are accepted"
        ),
    }))
}

/// The permissions of a catalog entry the rendered identity receives: all of them, except that a
/// protected install's management identity never receives what only the verifier may do. The
/// entry keeps its role ID either way, so a role loses permissions instead of being replaced.
///
/// What a set the stack defines grants must also be on the GCP allow-list, or rendering is
/// refused. Predefined roles are checked against the same file for every set, since a role can't
/// be narrowed permission by permission.
fn granted_permissions(
    permission_set: &PermissionSet,
    permissions: &[String],
    context: &PermissionContext,
) -> Result<Vec<String>> {
    if context.signing_protected && !is_built_in_unchanged(permission_set) {
        for permission in permissions {
            if let Some(family) = gcp_family(permission) {
                return Err(alien_error::AlienError::new(
                    ErrorData::ProtectedManagementFamilyAction {
                        permission_set_id: permission_set.id.clone(),
                        action: permission.clone(),
                        family: family.name().to_string(),
                    },
                ));
            }
        }
    }
    let kept: Vec<String> = permissions
        .iter()
        .filter(|permission| !(context.signing_protected && gcp_family(permission).is_some()))
        .cloned()
        .collect();
    Ok(kept)
}

/// Secrets named `signed-…` hold a protected install's inputs; only setup writes them. On a
/// protected install, every binding that grants Secret Manager permissions excludes them, for
/// any prefix, so an install whose prefix extends this one's can't reach them either.
/// IAM conditions don't support `matches`; `extract` returns the empty string for other names.
const SIGNED_SECRETS_EXCLUDED: &str =
    r#"!resource.name.extract("/secrets/{name}").startsWith("signed-")"#;

fn protected_secret_condition(
    condition: Option<GcpIamCondition>,
    grants_secrets: bool,
    context: &PermissionContext,
) -> Result<Option<GcpIamCondition>> {
    if !(context.signing_protected && grants_secrets) {
        return Ok(condition);
    }
    Ok(Some(match condition {
        Some(condition) => {
            if !is_single_cel_operand(&condition.expression) {
                return Err(alien_error::AlienError::new(ErrorData::GeneratorError {
                    platform: "gcp".to_string(),
                    message: format!(
                        "IAM condition '{}' can't be combined with the signing-secret exclusion: \
                         its parentheses don't balance or it holds a comment",
                        condition.title
                    ),
                }));
            }
            GcpIamCondition {
                expression: format!("({}) && {SIGNED_SECRETS_EXCLUDED}", condition.expression),
                ..condition
            }
        }
        None => GcpIamCondition {
            title: "Not signing inputs".to_string(),
            description: "Leaves out the secrets only setup writes".to_string(),
            expression: SIGNED_SECRETS_EXCLUDED.to_string(),
        },
    }))
}

/// Whether wrapping `expression` in one pair of parentheses makes it a single operand: outside
/// string literals the parentheses balance and never close more than they opened, and there is
/// no `//` comment that would swallow what follows.
fn is_single_cel_operand(expression: &str) -> bool {
    let mut depth = 0usize;
    let mut quote: Option<char> = None;
    let mut chars = expression.chars().peekable();
    while let Some(c) = chars.next() {
        if let Some(open) = quote {
            if c == '\\' {
                chars.next();
            } else if c == open {
                quote = None;
            }
            continue;
        }
        match c {
            '"' | '\'' => quote = Some(c),
            '(' => depth += 1,
            ')' => {
                let Some(next) = depth.checked_sub(1) else {
                    return false;
                };
                depth = next;
            }
            '/' if chars.peek() == Some(&'/') => return false,
            _ => {}
        }
    }
    depth == 0 && quote.is_none()
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
        let prefix = format!("role_{namespace}_");
        let suffix = fit_role_suffix(suffix, GCP_CUSTOM_ROLE_ID_MAX_LEN - prefix.len());
        return format!("{prefix}{suffix}");
    }

    let permission_set_slug = sanitize_role_segment(&permission_set.id.replace('/', "_"), 28);
    match suffix {
        Some(suffix) => {
            let prefix = format!("role_{namespace}_{permission_set_slug}_");
            let suffix = fit_role_suffix(suffix, GCP_CUSTOM_ROLE_ID_MAX_LEN - prefix.len());
            format!("{prefix}{suffix}")
        }
        None => format!("role_{namespace}_{permission_set_slug}"),
    }
}

/// Return the project custom-role ID prefix of this stack's roles.
///
/// Another deployment's namespace can extend this one (`acme` and
/// `acme_prod`), so the prefix never proves which deployment owns a role.
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
    match &context.gcp_custom_role_namespace {
        Some(namespace) => namespace.clone(),
        None => {
            custom_role_namespace_for_prefix(context.stack_prefix.as_deref().unwrap_or("stack"))
        }
    }
}

/// Return the custom-role namespace for a resource prefix.
///
/// A prefix of up to 18 characters is used as is. A longer one keeps its first
/// 9 characters and appends the first 8 hex characters of its SHA-256, so
/// prefixes that share a beginning still get distinct role IDs. This is the
/// `gcp_custom_role_prefix` local of the generated Terraform module, so both
/// setup paths name a deployment's roles identically.
pub fn custom_role_namespace_for_prefix(resource_prefix: &str) -> String {
    let segment = sanitize_role_segment(resource_prefix, usize::MAX);
    if segment.len() <= CUSTOM_ROLE_NAMESPACE_MAX_LEN {
        return segment;
    }
    let digest = Sha256::digest(resource_prefix.as_bytes());
    let hash: String = digest[..4].iter().map(|b| format!("{b:02x}")).collect();
    format!("{}_{hash}", &segment[..9])
}

/// Return the namespace runtime setup used before long prefixes were hashed:
/// the prefix cut to 18 characters, which two long prefixes can share.
pub fn legacy_custom_role_namespace_for_prefix(resource_prefix: &str) -> String {
    sanitize_role_segment(resource_prefix, CUSTOM_ROLE_NAMESPACE_MAX_LEN)
}

/// Whether a custom role's description names `resource_prefix` as its owner.
///
/// Every role generated for a deployment ends its description with
/// `Resource prefix: <prefix>.`, both at runtime and from Terraform.
pub fn custom_role_description_names_prefix(description: &str, resource_prefix: &str) -> bool {
    description.ends_with(&format!("Resource prefix: {resource_prefix}."))
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
    let explicit = has_explicit_label(explicit_label);
    if explicit || has_multiple_entries {
        let max_len = if explicit { 40 } else { 28 };
        let mut label = entry_snake_label(explicit_label, grant);
        if !explicit {
            label.push('_');
            label.push_str(&grant_suffix_hash(grant));
        }
        Some(sanitize_role_segment(&label, max_len))
    } else {
        None
    }
}

fn grant_suffix_hash(grant: &PermissionGrant) -> String {
    let mut values = Vec::new();
    if let Some(predefined_roles) = &grant.predefined_roles {
        values.extend(predefined_roles.iter().map(|role| format!("role:{role}")));
    }
    if let Some(permissions) = gcp_residual_permissions(grant) {
        values.extend(
            permissions
                .iter()
                .map(|permission| format!("permission:{permission}")),
        );
    }
    values.sort();
    stable_role_hash(&values.join("|"))
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

fn fit_role_suffix(value: &str, max_len: usize) -> String {
    if value.len() <= max_len {
        return value.to_string();
    }

    let hash = stable_role_hash(value);
    if max_len <= ROLE_ID_HASH_LEN {
        return hash[..max_len].to_string();
    }

    let prefix_len = max_len - ROLE_ID_HASH_LEN - 1;
    let mut prefix = value.to_string();
    prefix.truncate(prefix_len);
    while prefix.ends_with('_') {
        prefix.pop();
    }

    if prefix.is_empty() {
        hash[..max_len].to_string()
    } else {
        format!("{prefix}_{hash}")
    }
}

fn stable_role_hash(value: &str) -> String {
    let mut hash = 0x811c9dc5_u32;
    for byte in value.bytes() {
        hash ^= u32::from(byte);
        hash = hash.wrapping_mul(0x01000193);
    }
    format!("{hash:08x}")
}

fn binding_target_scope(binding_spec: &GcpBindingSpec) -> GcpBindingTargetScope {
    let scope = binding_spec.scope.trim();
    if scope == "projects/${projectName}/serviceAccounts/${serviceAccountName}@${projectName}.iam.gserviceaccount.com" {
        return GcpBindingTargetScope::ServiceAccount;
    }
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
    if scope.contains("/reasoningEngines/") {
        return Some(GcpBindingResourceKind::VertexAiReasoningEngine);
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

#[cfg(test)]
mod tests {
    use super::is_single_cel_operand;

    #[test]
    fn only_a_balanced_comment_free_expression_is_one_operand() {
        for ok in [
            "resource.name.startsWith(\"a\")",
            "a && (b || c)",
            "resource.name == \"a)(\"",
            "x == 'it\\'s ) fine'",
        ] {
            assert!(is_single_cel_operand(ok), "{ok}");
        }
        for bad in ["true) || (true", "a)", "(a", "a // comment", "x == \"open"] {
            assert!(!is_single_cel_operand(bad), "{bad}");
        }
    }
}
