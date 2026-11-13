//! The GCP roles the runtime binds at the project after setup, which setup creates, and the
//! conditions that limit the management identity to granting exactly those.
//!
//! With automatic setup, management holds every `*/provision` grant, so it can create the
//! application's service accounts and bind their roles on the project. GCP has no permission for
//! "only the roles this stack declares", so setup builds one:
//!
//! - Setup creates every custom role the runtime binds. Their ids are deterministic per
//!   deployment, so setup and the runtime name them the same way. Management holds no role
//!   writes (`iam.roles.create`, `update`, `undelete`, `delete`), so it can neither create a role
//!   nor change one setup made.
//! - Management's `resourcemanager.projects.setIamPolicy` sits in a role of its own, bound with
//!   conditions on `iam.googleapis.com/modifiedGrantsByRole` that admit only those roles. A
//!   condition lists at most [`GCP_ROLES_PER_GRANT_CONDITION`] roles, so a longer list is split
//!   across several bindings of the same role; GCP grants a call when any binding admits it.
//!
//! A `modifiedGrantsByRole` condition limits which roles are granted, not to whom.

use crate::{
    error::{ErrorData, Result},
    generators::gcp_runtime::{
        custom_role_prefix, GcpBindingTargetScope, GcpCustomRole, GcpGrantPlan, GcpIamCondition,
        GcpRuntimePermissionsGenerator,
    },
    BindingTarget, PermissionContext,
};
use alien_core::{PermissionSet, ServiceAccount, Stack};
use alien_error::{AlienError, Context};
use std::collections::BTreeSet;

/// Permissions that create or change a custom role. Setup owns the role definitions, so management
/// does not hold them.
pub const GCP_ROLE_DEFINITION_WRITES: [&str; 4] = [
    "iam.roles.create",
    "iam.roles.delete",
    "iam.roles.undelete",
    "iam.roles.update",
];

/// The project IAM write management may only use under the role-grant conditions.
pub const GCP_PROJECT_IAM_WRITE: &str = "resourcemanager.projects.setIamPolicy";

/// Most roles one `modifiedGrantsByRole` condition may list.
pub const GCP_ROLES_PER_GRANT_CONDITION: usize = 10;

/// Suffix of the role that holds management's project IAM write.
const PROJECT_IAM_WRITE_ROLE_SUFFIX: &str = "project_iam_write";

/// Every role the runtime binds after setup for a stack.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GcpRuntimeRoles {
    /// Custom roles the runtime binds anywhere, which setup creates. Each id appears once.
    pub custom_roles: Vec<GcpCustomRole>,
    /// Full names of the roles, custom and predefined, the runtime binds on the project.
    pub project_roles: BTreeSet<String>,
}

/// Every role the runtime may bind after setup: the grants of each service account, both its
/// stack-wide sets and those named for a resource, and of each permission profile.
///
/// Each set is planned for the target it is bound with: a service account's stack-wide sets and a
/// profile's `*` entry at the stack, the rest at a resource.
pub fn gcp_runtime_roles(stack: &Stack, context: &PermissionContext) -> Result<GcpRuntimeRoles> {
    let mut grants: Vec<(PermissionSet, BindingTarget)> = Vec::new();
    let mut add = |set: PermissionSet, target: BindingTarget| {
        if set.platforms.gcp.is_some()
            && !grants.iter().any(|(existing, existing_target)| {
                existing.id == set.id && *existing_target == target
            })
        {
            grants.push((set, target));
        }
    };
    for (_, entry) in stack.resources() {
        if let Some(account) = entry.config.downcast_ref::<ServiceAccount>() {
            for set in &account.stack_permission_sets {
                add(set.clone(), BindingTarget::Stack);
            }
            for set in account.resource_permission_sets.values().flatten() {
                add(set.clone(), BindingTarget::Resource);
            }
        }
    }
    for profile in stack.permissions.profiles.values() {
        for (scope, references) in &profile.0 {
            let target = if scope == "*" {
                BindingTarget::Stack
            } else {
                BindingTarget::Resource
            };
            for reference in references {
                if let Some(set) =
                    reference.resolve(|name| crate::get_permission_set(name).cloned())
                {
                    add(set, target);
                }
            }
        }
    }

    let generator = GcpRuntimePermissionsGenerator::new();
    let resource_context = context.clone().with_resource_name(format!(
        "{}-resource",
        context.stack_prefix.as_deref().unwrap_or("stack")
    ));
    let mut roles = GcpRuntimeRoles::default();
    for (set, target) in &grants {
        let context = match target {
            BindingTarget::Stack => context,
            BindingTarget::Resource => &resource_context,
        };
        let plan = generator
            .generate_grant_plan(set, *target, context)
            .context(ErrorData::GeneratorError {
                platform: "gcp".to_string(),
                message: format!(
                    "Failed to plan the runtime roles of permission set '{}'",
                    set.id
                ),
            })?;
        // Management may grant every role listed here, so no custom role among them may carry the
        // role or project policy writes taken from management.
        if let Some(permission) = plan
            .custom_roles
            .iter()
            .flat_map(|role| &role.included_permissions)
            .find(|permission| {
                *permission == GCP_PROJECT_IAM_WRITE
                    || GCP_ROLE_DEFINITION_WRITES.contains(&permission.as_str())
            })
        {
            return Err(AlienError::new(ErrorData::RuntimeGrantNotDelegable {
                permission_set_id: set.id.clone(),
                permission: permission.clone(),
            }));
        }
        roles.project_roles.extend(
            plan.bindings_for_target(GcpBindingTargetScope::Project)
                .into_iter()
                .map(|binding| binding.role),
        );
        for custom_role in plan.custom_roles {
            if !roles
                .custom_roles
                .iter()
                .any(|existing| existing.role_id == custom_role.role_id)
            {
                roles.custom_roles.push(custom_role);
            }
        }
    }
    let write_role_id = format!(
        "{}{PROJECT_IAM_WRITE_ROLE_SUFFIX}",
        custom_role_prefix(context)
    );
    if roles
        .custom_roles
        .iter()
        .any(|role| role.role_id == write_role_id)
    {
        return Err(AlienError::new(ErrorData::GeneratorError {
            platform: "gcp".to_string(),
            message: format!("A runtime role takes the id '{write_role_id}', which setup reserves"),
        }));
    }
    // Role names are quoted into the grant conditions.
    if let Some(role) = roles.project_roles.iter().find(|role| {
        !role.chars().all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, '/' | '_' | '.' | '-' | '$' | '{' | '}')
        })
    }) {
        return Err(AlienError::new(ErrorData::GeneratorError {
            platform: "gcp".to_string(),
            message: format!("Role name '{role}' cannot be written into a grant condition"),
        }));
    }
    Ok(roles)
}

/// Remove role writes and the project IAM write from a management grant plan, and report
/// whether the plan asked for the project IAM write.
///
/// A custom role left with no permission is dropped together with its bindings. The caller binds
/// the project IAM write through [`gcp_project_iam_write_role`] instead.
pub fn strip_gcp_management_iam_writes(plan: &mut GcpGrantPlan) -> bool {
    let mut project_iam_write = false;
    let mut emptied = Vec::new();
    for role in &mut plan.custom_roles {
        role.included_permissions.retain(|permission| {
            if permission == GCP_PROJECT_IAM_WRITE {
                project_iam_write = true;
                return false;
            }
            !GCP_ROLE_DEFINITION_WRITES.contains(&permission.as_str())
        });
        if role.included_permissions.is_empty() {
            emptied.push(role.name.clone());
        }
    }
    plan.custom_roles
        .retain(|role| !emptied.contains(&role.name));
    plan.bindings
        .retain(|binding| !emptied.contains(&binding.role));
    project_iam_write
}

/// The custom role that holds management's project IAM write, bound only with the conditions of
/// [`gcp_project_role_grant_conditions`].
pub fn gcp_project_iam_write_role(context: &PermissionContext) -> GcpCustomRole {
    let project = context.project_name.as_deref().unwrap_or("PROJECT_NAME");
    let role_id = format!(
        "{}{PROJECT_IAM_WRITE_ROLE_SUFFIX}",
        custom_role_prefix(context)
    );
    let prefix = context.stack_prefix.as_deref().unwrap_or("stack");
    let title = match context.deployment_name.as_deref() {
        Some(deployment) => format!("{deployment}: Project role grants"),
        None => "Project role grants".to_string(),
    };
    GcpCustomRole {
        name: format!("projects/{project}/roles/{role_id}"),
        role_id,
        title,
        description: format!(
            "Read the project IAM policy and grant or revoke, for any principal, only the roles named in the conditions on this role's bindings. Resource prefix: {prefix}."
        ),
        included_permissions: vec![
            "resourcemanager.projects.getIamPolicy".to_string(),
            GCP_PROJECT_IAM_WRITE.to_string(),
        ],
        stage: "GA".to_string(),
    }
}

/// The conditions to bind management's project IAM write role with, one binding each.
///
/// Each admits a project IAM write only when every grant it adds or removes is of a listed role.
/// Returns no condition for an empty list: management then needs no project IAM write at all.
pub fn gcp_project_role_grant_conditions(roles: &BTreeSet<String>) -> Vec<GcpIamCondition> {
    let roles: Vec<&String> = roles.iter().collect();
    let shards = roles.chunks(GCP_ROLES_PER_GRANT_CONDITION).count();
    roles
        .chunks(GCP_ROLES_PER_GRANT_CONDITION)
        .enumerate()
        .map(|(index, chunk)| {
            let list = chunk
                .iter()
                .map(|role| format!("'{role}'"))
                .collect::<Vec<_>>()
                .join(", ");
            GcpIamCondition {
                title: format!("Deployment role grants {}/{shards}", index + 1),
                description: "Allow project IAM changes only to grants of the listed roles".to_string(),
                expression: format!(
                    "api.getAttribute('iam.googleapis.com/modifiedGrantsByRole', []).hasOnly([{list}])"
                ),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::get_permission_set;
    use alien_core::{
        permissions::{PermissionProfile, PermissionSetReference},
        PermissionsConfig, ResourceLifecycle, Storage,
    };

    fn context() -> PermissionContext {
        PermissionContext::new()
            .with_project_name("proj-a".to_string())
            .with_stack_prefix("app1".to_string())
            .with_deployment_name("demo".to_string())
            .with_region("us-central1".to_string())
            .with_project_number("123".to_string())
            .with_service_account_name("app1-management".to_string())
    }

    fn set(id: &str) -> PermissionSet {
        get_permission_set(id)
            .unwrap_or_else(|| panic!("permission set {id} exists"))
            .clone()
    }

    fn stack_with_profile(refs: &[&str]) -> Stack {
        Stack::new("demo".into())
            .add(
                Storage::new("files".into()).build(),
                ResourceLifecycle::Live,
            )
            .permissions(
                PermissionsConfig::new().with_profile(
                    "app",
                    PermissionProfile::new()
                        .global(refs.iter().map(|id| PermissionSetReference::from_name(*id))),
                ),
            )
            .build()
    }

    /// Management's plan for `service-account/provision` keeps creating service accounts but
    /// loses every way to write a role or the project policy.
    #[test]
    fn management_plan_loses_role_and_project_policy_writes() {
        let mut plan = GcpRuntimePermissionsGenerator::new()
            .generate_grant_plan(
                &set("service-account/provision"),
                BindingTarget::Stack,
                &context(),
            )
            .unwrap();
        let before: BTreeSet<String> = plan
            .custom_roles
            .iter()
            .flat_map(|role| role.included_permissions.clone())
            .collect();
        assert!(before.contains("iam.roles.create"));
        assert!(before.contains(GCP_PROJECT_IAM_WRITE));

        assert!(strip_gcp_management_iam_writes(&mut plan));

        let after: BTreeSet<String> = plan
            .custom_roles
            .iter()
            .flat_map(|role| role.included_permissions.clone())
            .collect();
        for write in GCP_ROLE_DEFINITION_WRITES {
            assert!(!after.contains(write), "{write} must be removed");
        }
        assert!(!after.contains(GCP_PROJECT_IAM_WRITE));
        assert!(after.contains("iam.serviceAccounts.create"));
        assert!(after.contains("iam.roles.get"));
        assert_eq!(
            before.len() - after.len(),
            4,
            "only create, update, undelete and setIamPolicy were held"
        );
        for binding in &plan.bindings {
            assert!(
                binding.role.starts_with("roles/")
                    || plan
                        .custom_roles
                        .iter()
                        .any(|role| role.name == binding.role),
                "binding {} must reference a role the plan still defines",
                binding.role
            );
        }
    }

    /// A set without IAM writes is left exactly as generated.
    #[test]
    fn management_plan_without_iam_writes_is_unchanged() {
        let generated = GcpRuntimePermissionsGenerator::new()
            .generate_grant_plan(&set("storage/provision"), BindingTarget::Stack, &context())
            .unwrap();
        let mut plan = generated.clone();
        assert!(!strip_gcp_management_iam_writes(&mut plan));
        assert_eq!(plan, generated);
    }

    /// A grant named for one resource is planned only for that resource, so its stack-wide form
    /// (here, token creation on every service account in the project) is never listed.
    #[test]
    fn resource_grants_list_only_their_resource_form() {
        let stack = Stack::new("demo".into())
            .add(
                Storage::new("files".into()).build(),
                ResourceLifecycle::Live,
            )
            .permissions(
                PermissionsConfig::new().with_profile(
                    "app",
                    PermissionProfile::new()
                        .resource(
                            "worker-sa",
                            [PermissionSetReference::from_name(
                                "service-account/impersonate",
                            )],
                        )
                        .resource(
                            "files",
                            [PermissionSetReference::from_name("storage/data-read")],
                        ),
                ),
            )
            .build();
        let stack_form = GcpRuntimePermissionsGenerator::new()
            .generate_grant_plan(
                &set("service-account/impersonate"),
                BindingTarget::Stack,
                &context(),
            )
            .unwrap()
            .bindings_for_target(GcpBindingTargetScope::Project);
        assert!(
            !stack_form.is_empty(),
            "the stack-wide form binds at the project"
        );

        // A resource grant next to it is still listed.
        let resource_form = GcpRuntimePermissionsGenerator::new()
            .generate_grant_plan(
                &set("storage/data-read"),
                BindingTarget::Resource,
                &context().with_resource_name("x"),
            )
            .unwrap();
        assert!(
            !resource_form.custom_roles.is_empty(),
            "the resource form binds a custom role"
        );

        let roles = gcp_runtime_roles(&stack, &context()).unwrap();

        for role in &resource_form.custom_roles {
            assert!(
                roles.custom_roles.iter().any(|r| r.role_id == role.role_id),
                "{} missing",
                role.role_id
            );
        }
        for binding in stack_form {
            assert!(
                !roles.project_roles.contains(&binding.role),
                "{} listed",
                binding.role
            );
        }
    }

    /// Management may grant every listed role, so an application grant that carries role or
    /// project policy writes is refused rather than listed.
    #[test]
    fn application_grant_of_role_writes_is_refused() {
        let stack = stack_with_profile(&["storage/data-read", "service-account/provision"]);

        let error = gcp_runtime_roles(&stack, &context()).expect_err("refused");

        assert_eq!(error.code, "RUNTIME_GRANT_NOT_DELEGABLE");
        assert!(matches!(
            error.error,
            Some(ErrorData::RuntimeGrantNotDelegable { ref permission_set_id, ref permission })
                if permission_set_id == "service-account/provision"
                    && (GCP_ROLE_DEFINITION_WRITES.contains(&permission.as_str())
                        || permission == GCP_PROJECT_IAM_WRITE)
        ));
    }

    /// The runtime roles are those the application's grants bind, named in this deployment's
    /// namespace, and include every project-level role the service account controller binds.
    #[test]
    fn runtime_roles_cover_the_application_grants() {
        let stack = stack_with_profile(&["storage/data-read", "storage/data-write"]);
        let roles = gcp_runtime_roles(&stack, &context()).unwrap();

        let generator = GcpRuntimePermissionsGenerator::new();
        for id in ["storage/data-read", "storage/data-write"] {
            let plan = generator
                .generate_grant_plan(&set(id), BindingTarget::Stack, &context())
                .unwrap();
            for role in &plan.custom_roles {
                assert!(
                    roles.custom_roles.iter().any(|r| r.role_id == role.role_id),
                    "{} missing",
                    role.role_id
                );
            }
            for binding in plan.bindings_for_target(GcpBindingTargetScope::Project) {
                assert!(roles.project_roles.contains(&binding.role));
            }
        }
        assert!(!roles.project_roles.is_empty());
        for role in &roles.custom_roles {
            assert!(role.name.starts_with("projects/proj-a/roles/role_app1_"));
        }
        assert!(
            !roles
                .project_roles
                .iter()
                .any(|role| role.contains("service_account_provision")),
            "management's own grants are not runtime roles"
        );
    }

    #[test]
    fn conditions_list_at_most_ten_roles_each_and_cover_all() {
        let roles: BTreeSet<String> = (0..23)
            .map(|index| format!("projects/proj-a/roles/role_app1_r{index:02}"))
            .collect();
        let conditions = gcp_project_role_grant_conditions(&roles);
        assert_eq!(conditions.len(), 3);
        let mut seen = BTreeSet::new();
        for condition in &conditions {
            let listed: Vec<&str> = condition
                .expression
                .split('\'')
                .filter(|part| part.starts_with("projects/"))
                .collect();
            assert!(listed.len() <= GCP_ROLES_PER_GRANT_CONDITION);
            assert!(condition.expression.starts_with(
                "api.getAttribute('iam.googleapis.com/modifiedGrantsByRole', []).hasOnly(["
            ));
            seen.extend(listed.into_iter().map(str::to_string));
        }
        assert_eq!(seen, roles);
        assert!(gcp_project_role_grant_conditions(&BTreeSet::new()).is_empty());
    }

    #[test]
    fn project_iam_write_role_is_owned_by_the_deployment() {
        let role = gcp_project_iam_write_role(&context());
        assert_eq!(
            role.name,
            "projects/proj-a/roles/role_app1_project_iam_write"
        );
        assert!(crate::generators::custom_role_description_names_prefix(
            &role.description,
            "app1"
        ));
        assert_eq!(
            role.included_permissions,
            vec![
                "resourcemanager.projects.getIamPolicy".to_string(),
                GCP_PROJECT_IAM_WRITE.to_string()
            ]
        );
    }
}
