mod common;

use alien_permissions::{
    generators::GcpRuntimePermissionsGenerator, get_permission_set, BindingTarget,
};
use common::create_test_context;

/// The worker's subscriber grant is project scoped and unconditioned: a real project refused a
/// subscription-name condition on getIamPolicy, and queue/provision already holds the same
/// permissions at project scope. It carries only the two IAM permissions.
#[test]
fn worker_dead_letter_grant_is_project_scoped_with_only_the_two_iam_permissions() {
    let generator = GcpRuntimePermissionsGenerator::new();
    let set = get_permission_set("worker/dead-letter").expect("permission set exists");
    let context = create_test_context().with_resource_name("my-stack-consumer");
    for target in [BindingTarget::Stack, BindingTarget::Resource] {
        let result = generator
            .generate_bindings(set, target, &context)
            .expect("bindings generate");
        assert_eq!(result.bindings.len(), 1);
        assert!(result.bindings[0].condition.is_none());
    }
    let plan = generator
        .generate_grant_plan(set, BindingTarget::Stack, &context)
        .expect("grant plan generates");
    let permissions: Vec<_> = plan
        .custom_roles_for_bindings(&plan.bindings)
        .into_iter()
        .flat_map(|role| role.included_permissions)
        .collect();
    assert_eq!(
        permissions,
        [
            "pubsub.subscriptions.getIamPolicy",
            "pubsub.subscriptions.setIamPolicy"
        ]
    );
}

/// The queue grant names the queue's own topic and subscription, plus the project number read.
#[test]
fn queue_dead_letter_grant_is_resource_scoped_and_only_reads_the_project() {
    let generator = GcpRuntimePermissionsGenerator::new();
    let set = get_permission_set("queue/dead-letter").expect("permission set exists");
    let context = create_test_context().with_resource_name("my-stack-tasks");
    let result = generator
        .generate_bindings(set, BindingTarget::Resource, &context)
        .expect("bindings generate");
    let permissions: std::collections::BTreeSet<_> = set
        .platforms
        .gcp
        .as_ref()
        .unwrap()
        .iter()
        .flat_map(|grant| grant.grant.permissions.clone().unwrap_or_default())
        .collect();
    assert_eq!(
        permissions.into_iter().collect::<Vec<_>>(),
        [
            "pubsub.subscriptions.getIamPolicy",
            "pubsub.subscriptions.setIamPolicy",
            "pubsub.subscriptions.update",
            "pubsub.topics.getIamPolicy",
            "pubsub.topics.setIamPolicy",
            "resourcemanager.projects.get",
        ]
    );
    assert_eq!(result.bindings.len(), 3);
}
