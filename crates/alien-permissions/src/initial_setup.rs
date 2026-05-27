//! Auto-generates minimal IAM/RBAC permissions for initial setup.
//!
//! Initial setup creates setup-owned Frozen resources. Alien-owned Live
//! resources are created later by the deployment loop with management
//! credentials. This module generates the setup permission set for that first
//! Frozen-resource phase.

use std::collections::HashSet;

use alien_core::{ownership_policy_for_resource_type, Stack};

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
/// Walks resources emitted during setup and includes their `<type>/provision`
/// permission set if one exists in the registry. Live resources are excluded:
/// their provision permissions belong to the ongoing management profile.
/// Also adds cross-cutting permission sets (e.g. `service-account/provision`)
/// that any initial setup requires regardless of the resources declared.
pub fn initial_setup_permission_set_ids(stack: &Stack) -> Vec<String> {
    let mut set_ids = Vec::new();

    for (_, resource_entry) in stack.resources() {
        let raw_resource_type = resource_entry.config.resource_type();
        let raw_resource_type = raw_resource_type.as_ref();
        let policy = ownership_policy_for_resource_type(raw_resource_type);
        if !policy.should_emit_in_setup(resource_entry.lifecycle) {
            continue;
        }

        let resource_type = normalize_resource_type(raw_resource_type);
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

/// Generate a merged AWS IAM policy document containing setup provision
/// permissions for the given platform.
///
/// This generates a complete setup policy covering every Frozen/setup resource
/// type that setup can create. It intentionally excludes Live-only resources,
/// which are created by Alien after setup.
///
/// Customer-facing output: "here's the IAM policy you need to attach to
/// your admin role before running `alien deploy up`."
pub fn generate_aws_initial_setup_policy(
    context: &PermissionContext,
) -> crate::error::Result<AwsIamPolicy> {
    let generator = AwsRuntimePermissionsGenerator::new();
    let all_provision_ids = crate::registry::list_permission_set_ids()
        .into_iter()
        .filter(|id| {
            let Some((resource_type, operation)) = id.split_once('/') else {
                return false;
            };
            if operation != "provision" {
                return false;
            }
            ownership_policy_for_resource_type(resource_type)
                .should_emit_in_setup(alien_core::ResourceLifecycle::Frozen)
        })
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

    ensure_unique_statement_sids(&mut all_statements);

    Ok(AwsIamPolicy {
        version: "2012-10-17".to_string(),
        statement: all_statements,
    })
}

fn ensure_unique_statement_sids(statements: &mut [crate::generators::AwsIamStatement]) {
    let mut used = HashSet::new();

    for statement in statements {
        if used.insert(statement.sid.clone()) {
            continue;
        }

        let base = statement.sid.clone();
        let mut suffix = 2usize;
        loop {
            let candidate = suffixed_statement_sid(&base, suffix);
            if used.insert(candidate.clone()) {
                statement.sid = candidate;
                break;
            }
            suffix += 1;
        }
    }
}

fn suffixed_statement_sid(base: &str, suffix: usize) -> String {
    let suffix = suffix.to_string();
    let max_base_len = 128usize.saturating_sub(suffix.len());
    let trimmed = base.chars().take(max_base_len).collect::<String>();
    format!("{trimmed}{suffix}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{ResourceLifecycle, Storage, Worker, WorkerCode};

    fn test_function(name: &str) -> Worker {
        Worker::new(name.to_string())
            .code(WorkerCode::Image {
                image: "rust:latest".to_string(),
            })
            .permissions("execution".to_string())
            .build()
    }

    #[test]
    fn live_function_stack_excludes_function_provision() {
        let worker = test_function("my-fn");

        let stack = Stack::new("test-stack".to_string())
            .add(worker, ResourceLifecycle::Live)
            .build();

        let ids = initial_setup_permission_set_ids(&stack);
        assert!(
            !ids.contains(&"worker/provision".to_string()),
            "worker/provision belongs to management permissions, got {ids:?}"
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
        let worker = test_function("my-fn");
        let storage = Storage::new("my-bucket".to_string()).build();

        let stack = Stack::new("test-stack".to_string())
            .add(worker, ResourceLifecycle::Live)
            .add(storage, ResourceLifecycle::Frozen)
            .build();

        let ids = initial_setup_permission_set_ids(&stack);
        assert!(!ids.contains(&"worker/provision".to_string()));
        assert!(ids.contains(&"storage/provision".to_string()));
        assert!(ids.contains(&"service-account/provision".to_string()));
    }

    #[test]
    fn complete_aws_initial_setup_policy_excludes_live_only_provision_sets() {
        let context = PermissionContext::new()
            .with_aws_region("us-east-1")
            .with_aws_account_id("123456789012")
            .with_stack_prefix("test-stack")
            .with_resource_name("test");

        let policy = generate_aws_initial_setup_policy(&context).unwrap();
        let actions = policy
            .statement
            .iter()
            .flat_map(|statement| statement.action.iter())
            .collect::<Vec<_>>();

        assert!(
            !actions.contains(&&"lambda:CreateFunction".to_string()),
            "setup policy must not include live worker provision actions"
        );
        assert!(
            actions.iter().any(|action| action.starts_with("s3:")),
            "setup policy should still include frozen-capable resource actions"
        );
    }

    #[test]
    fn complete_aws_initial_setup_policy_has_unique_statement_sids() {
        let context = PermissionContext::new()
            .with_aws_region("us-east-1")
            .with_aws_account_id("123456789012")
            .with_stack_prefix("test-stack")
            .with_resource_name("test");

        let policy = generate_aws_initial_setup_policy(&context).unwrap();
        let mut seen = HashSet::new();

        for statement in policy.statement {
            assert!(
                seen.insert(statement.sid.clone()),
                "duplicate AWS IAM statement Sid: {}",
                statement.sid
            );
        }
    }

    #[test]
    fn complete_aws_initial_setup_policy_can_create_remote_management_policies() {
        let context = PermissionContext::new()
            .with_aws_region("us-east-1")
            .with_aws_account_id("123456789012")
            .with_stack_prefix("test-stack")
            .with_resource_name("test");

        let policy = generate_aws_initial_setup_policy(&context).unwrap();
        let statements = policy
            .statement
            .iter()
            .filter(|statement| statement.action.contains(&"iam:CreatePolicy".to_string()))
            .collect::<Vec<_>>();

        assert!(
            statements.iter().any(|statement| statement.resource.contains(
                &"arn:aws:iam::123456789012:policy/test-stack-deployment-management-*"
                    .to_string()
            )),
            "initial setup policy must be able to create remote-stack-management managed policies, got {statements:?}"
        );
    }
}
