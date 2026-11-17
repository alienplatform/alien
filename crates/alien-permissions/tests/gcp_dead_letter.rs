mod common;

use alien_permissions::{
    generators::GcpRuntimePermissionsGenerator, get_permission_set, BindingTarget,
};
use common::create_test_context;

/// The worker's subscriber grant is limited to subscriptions named for the stack (or the
/// worker), never to every subscription of the project.
#[test]
fn worker_dead_letter_grant_is_limited_to_the_workers_own_push_subscriptions() {
    let generator = GcpRuntimePermissionsGenerator::new();
    let set = get_permission_set("worker/dead-letter").expect("permission set exists");
    let context = create_test_context().with_resource_name("my-stack-consumer");
    for (target, prefix) in [
        (BindingTarget::Stack, "my-stack-"),
        (BindingTarget::Resource, "my-stack-consumer-"),
    ] {
        let result = generator
            .generate_bindings(set, target, &context)
            .expect("bindings generate");
        assert_eq!(result.bindings.len(), 1);
        let condition = result.bindings[0]
            .condition
            .as_ref()
            .expect("the grant is conditioned on the subscription name");
        assert!(
            condition.expression.contains(&format!(
                "resource.name.startsWith(\"projects/my-project/subscriptions/{prefix}\")"
            )),
            "{}",
            condition.expression
        );
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
