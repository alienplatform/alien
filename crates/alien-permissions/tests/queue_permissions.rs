mod common;

use alien_permissions::{
    generators::{
        AwsCloudFormationPermissionsGenerator, AzureRuntimePermissionsGenerator,
        GcpBindingResourceKind, GcpBindingTargetScope, GcpRuntimePermissionsGenerator,
    },
    get_permission_set, BindingTarget,
};
use common::create_test_context;
use serde_json::json;

#[test]
fn aws_queue_roles_separate_publish_from_consume_and_acknowledge() {
    let generator = AwsCloudFormationPermissionsGenerator::new();
    let context = create_test_context().with_resource_name("my-stack-jobs");

    let actions_for = |permission_set_id| {
        let permission_set = get_permission_set(permission_set_id).expect("queue permission set");
        let policy = generator
            .generate_policy(permission_set, BindingTarget::Resource, &context)
            .expect("queue IAM policy");
        assert_eq!(policy.statement.len(), 1);
        assert_eq!(
            policy.statement[0].resource,
            vec![
                json!({"Fn::Sub": "arn:${AWS::Partition}:sqs:${AWS::Region}:${AWS::AccountId}:my-stack-jobs"})
            ]
        );
        policy.statement[0].action.clone()
    };

    let publisher = actions_for("queue/publish");
    assert!(publisher.contains(&json!("sqs:SendMessage")));
    for action in [
        "sqs:ReceiveMessage",
        "sqs:DeleteMessage",
        "sqs:ChangeMessageVisibility",
    ] {
        assert!(
            !publisher.contains(&json!(action)),
            "publisher grants {action}"
        );
    }

    let consumer = actions_for("queue/data-read");
    for action in [
        "sqs:ReceiveMessage",
        "sqs:DeleteMessage",
        "sqs:ChangeMessageVisibility",
    ] {
        assert!(consumer.contains(&json!(action)), "consumer lacks {action}");
    }
    assert!(!consumer.contains(&json!("sqs:SendMessage")));

    // Existing profiles continue to work until callers can adopt queue/publish.
    let legacy = actions_for("queue/data-write");
    assert!(legacy.contains(&json!("sqs:SendMessage")));
    assert!(legacy.contains(&json!("sqs:DeleteMessage")));
}

#[test]
fn gcp_queue_publisher_has_only_a_topic_binding() {
    let generator = GcpRuntimePermissionsGenerator::new();
    let context = create_test_context().with_resource_name("jobs");
    let permission_set =
        get_permission_set("queue/publish").expect("queue publisher permission set");
    let bindings = generator
        .generate_bindings(permission_set, BindingTarget::Resource, &context)
        .expect("GCP queue publisher bindings");

    assert_eq!(bindings.bindings.len(), 1);
    let binding = &bindings.bindings[0];
    assert_eq!(binding.role, "roles/pubsub.publisher");
    assert_eq!(
        binding.resource_kind,
        Some(GcpBindingResourceKind::PubsubTopic)
    );
    assert_eq!(binding.target, GcpBindingTargetScope::CurrentResource);
}

#[test]
fn azure_queue_publisher_and_consumer_have_distinct_queue_roles() {
    let generator = AzureRuntimePermissionsGenerator::new();
    let context = create_test_context().with_resource_name("jobs");
    let role_for = |permission_set_id| {
        let permission_set = get_permission_set(permission_set_id).expect("queue permission set");
        let plan = generator
            .generate_grant_plan(permission_set, BindingTarget::Resource, &context)
            .expect("Azure queue grant plan");
        assert!(plan.custom_roles.is_empty());
        assert_eq!(plan.bindings.len(), 1);
        let binding = &plan.bindings[0];
        assert_eq!(
            binding.scope,
            "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/rg-observability-prod/providers/Microsoft.ServiceBus/namespaces/jobs-sb/queues/jobs"
        );
        binding.role_name.clone()
    };

    assert_eq!(role_for("queue/publish"), "Azure Service Bus Data Sender");
    assert_eq!(
        role_for("queue/data-read"),
        "Azure Service Bus Data Receiver"
    );
}
