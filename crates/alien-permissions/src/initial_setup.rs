//! Auto-generates minimal IAM/RBAC permissions for initial setup.
//!
//! Initial setup (admin runs `alien deploy up`) creates ALL cloud resources
//! (frozen AND live). This module generates the minimal permission set needed,
//! given a stack definition and target platform.

use alien_core::Stack;

use crate::generators::{AwsIamPolicy, AwsRuntimePermissionsGenerator};
use crate::registry::get_permission_set;
use crate::{BindingTarget, PermissionContext};

/// Normalizes a resource type string to match permission set ID conventions.
/// Resource types use mixed conventions (`service_activation`, `azure_storage_account`)
/// while permission set IDs consistently use kebab-case (`service-activation`, `azure-storage-account`).
fn normalize_resource_type(resource_type: &str) -> String {
    resource_type.replace('_', "-")
}

/// Collects all provision permission set IDs needed for a stack's initial setup.
///
/// Walks every resource in the stack and includes its `<type>/provision` permission
/// set if one exists in the registry. Also adds cross-cutting permission sets
/// (e.g. `service-account/provision`) that any initial setup requires regardless
/// of the resources declared.
pub fn initial_setup_permission_set_ids(stack: &Stack) -> Vec<String> {
    let mut set_ids = Vec::new();

    for (_, resource_entry) in stack.resources() {
        let resource_type = normalize_resource_type(resource_entry.config.resource_type().as_ref());
        let provision_id = format!("{resource_type}/provision");

        if get_permission_set(&provision_id).is_some() && !set_ids.contains(&provision_id) {
            set_ids.push(provision_id);
        }
    }

    let cross_cutting = ["service-account/provision"];

    for id in cross_cutting {
        if get_permission_set(id).is_some() && !set_ids.contains(&id.to_string()) {
            set_ids.push(id.to_string());
        }
    }

    set_ids
}

/// Generate a merged AWS IAM policy document containing ALL provision
/// permissions for the given platform.
///
/// This generates the COMPLETE initial setup policy covering every resource
/// type that Alien can provision. This is intentionally broad — it includes
/// permissions for resources that preflights may add (RSM, ServiceAccount,
/// SecretsVault, etc.) which aren't in the raw stack definition.
///
/// Customer-facing output: "here's the IAM policy you need to attach to
/// your admin role before running `alien deploy up`."
pub fn generate_aws_initial_setup_policy(
    context: &PermissionContext,
) -> crate::error::Result<AwsIamPolicy> {
    let generator = AwsRuntimePermissionsGenerator::new();
    let all_provision_ids = crate::registry::list_permission_set_ids()
        .into_iter()
        .filter(|id| id.ends_with("/provision"))
        .collect::<Vec<_>>();

    let mut all_statements = Vec::new();

    for perm_id in &all_provision_ids {
        if let Some(perm_set) = get_permission_set(perm_id) {
            match generator.generate_policy(perm_set, BindingTarget::Stack, context) {
                Ok(policy) => {
                    all_statements.extend(policy.statement);
                }
                Err(_) => {
                    // Permission set has no AWS platform definition — skip
                }
            }
        }
    }

    Ok(AwsIamPolicy {
        version: "2012-10-17".to_string(),
        statement: all_statements,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{Function, FunctionCode, ResourceLifecycle, Storage};

    fn test_function(name: &str) -> Function {
        Function::new(name.to_string())
            .code(FunctionCode::Image {
                image: "rust:latest".to_string(),
            })
            .permissions("execution".to_string())
            .build()
    }

    #[test]
    fn function_stack_includes_function_provision() {
        let function = test_function("my-fn");

        let stack = Stack::new("test-stack".to_string())
            .add(function, ResourceLifecycle::Live)
            .build();

        let ids = initial_setup_permission_set_ids(&stack);
        assert!(
            ids.contains(&"function/provision".to_string()),
            "Expected function/provision in {ids:?}"
        );
    }

    #[test]
    fn storage_stack_includes_storage_provision() {
        let storage = Storage::new("my-bucket".to_string()).build();

        let stack = Stack::new("test-stack".to_string())
            .add(storage, ResourceLifecycle::Frozen)
            .build();

        let ids = initial_setup_permission_set_ids(&stack);
        assert!(
            ids.contains(&"storage/provision".to_string()),
            "Expected storage/provision in {ids:?}"
        );
    }

    #[test]
    fn cross_cutting_service_account_always_included() {
        let stack = Stack::new("empty-stack".to_string()).build();

        let ids = initial_setup_permission_set_ids(&stack);
        assert!(
            ids.contains(&"service-account/provision".to_string()),
            "Expected service-account/provision in {ids:?}"
        );
    }

    #[test]
    fn no_duplicates() {
        let s1 = Storage::new("bucket-a".to_string()).build();
        let s2 = Storage::new("bucket-b".to_string()).build();

        let stack = Stack::new("test-stack".to_string())
            .add(s1, ResourceLifecycle::Frozen)
            .add(s2, ResourceLifecycle::Frozen)
            .build();

        let ids = initial_setup_permission_set_ids(&stack);
        let storage_count = ids.iter().filter(|id| *id == "storage/provision").count();
        assert_eq!(
            storage_count, 1,
            "storage/provision should appear exactly once"
        );
    }

    #[test]
    fn combined_stack_includes_all_resource_types() {
        let function = test_function("my-fn");
        let storage = Storage::new("my-bucket".to_string()).build();

        let stack = Stack::new("test-stack".to_string())
            .add(function, ResourceLifecycle::Live)
            .add(storage, ResourceLifecycle::Frozen)
            .build();

        let ids = initial_setup_permission_set_ids(&stack);
        assert!(ids.contains(&"function/provision".to_string()));
        assert!(ids.contains(&"storage/provision".to_string()));
        assert!(ids.contains(&"service-account/provision".to_string()));
    }
}
