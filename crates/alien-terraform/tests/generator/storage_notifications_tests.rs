//! Validate notification wiring with the real provider schema and Terraform graph.

use super::helpers::{assert_terraform_valid, render};
use alien_core::{
    AzureResourceGroup, AzureServiceBusNamespace, AzureStorageAccount, Queue, ResourceLifecycle,
    ResourceRef, Stack, StackSettings, Storage, StorageNotification,
};
use alien_terraform::TerraformTarget;

#[test]
fn gcp_storage_notifications_with_a_shared_queue_are_provider_valid() {
    let notification = |prefix: &str, suffix: &str| {
        StorageNotification::builder()
            .queue(ResourceRef::new(Queue::RESOURCE_TYPE, "jobs"))
            .prefix(prefix.to_string())
            .suffix(suffix.to_string())
            .build()
    };
    let stack = Stack::new("notification-validation".into())
        .add(Queue::new("jobs".into()).build(), ResourceLifecycle::Frozen)
        .add(
            Storage::new("first".into())
                .notifications(vec![
                    notification("in/", ".json"),
                    notification("overlap/", ".data"),
                ])
                .build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Storage::new("second".into())
                .notifications(vec![notification("other/", ".json")])
                .build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Gcp, StackSettings::default());
    // Provider validation rejects duplicate grant addresses, unresolved queue
    // references, dependency cycles, and unsupported notification properties.
    assert_terraform_valid(
        &module,
        "GCS notifications sharing one queue-owned publisher grant",
    );
}

#[test]
fn azure_storage_notifications_share_account_and_queue() {
    let notification = |prefix: &str, suffix: &str| {
        StorageNotification::builder()
            .queue(ResourceRef::new(Queue::RESOURCE_TYPE, "jobs"))
            .prefix(prefix.to_string())
            .suffix(suffix.to_string())
            .build()
    };
    let stack = Stack::new("notification-validation".into())
        .add(
            AzureResourceGroup::new("default-resource-group".into()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            AzureStorageAccount::new("default-storage-account".into()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            AzureServiceBusNamespace::new("default-service-bus-namespace".into()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(Queue::new("jobs".into()).build(), ResourceLifecycle::Frozen)
        .add(
            Storage::new("first".into())
                .notifications(vec![
                    notification("in/", ".json"),
                    notification("other/", ".data"),
                ])
                .build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Storage::new("second".into())
                .notifications(vec![notification("literal/${x}%{x}/", ".json")])
                .build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Azure, StackSettings::default());
    assert_terraform_valid(
        &module,
        "Azure account-scoped subscriptions sharing a queue",
    );
}

#[test]
fn aws_storage_notifications_with_shared_queue_are_provider_valid() {
    let notification = |prefix: &str, suffix: &str| {
        StorageNotification::builder()
            .queue(ResourceRef::new(Queue::RESOURCE_TYPE, "jobs"))
            .prefix(prefix.to_string())
            .suffix(suffix.to_string())
            .build()
    };
    let stack = Stack::new("storage-delivery".into())
        .add(Queue::new("jobs".into()).build(), ResourceLifecycle::Frozen)
        .add(
            Storage::new("first".into())
                .notifications(vec![
                    notification("in/", ".json"),
                    notification("same", "same"),
                ])
                .build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Storage::new("second".into())
                .notifications(vec![notification("literal/${x}%{x}*\\é/", ".data")])
                .build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Aws, StackSettings::default());
    assert_terraform_valid(&module, "AWS notification rules and shared queue ingress");
}

#[test]
fn aws_storage_notifications_follow_deployer_enablement() {
    let storage = Storage::new("files".into()).notifications(vec![
        StorageNotification::builder().queue(ResourceRef::new(Queue::RESOURCE_TYPE, "jobs")).build(),
    ]).build();
    let stack = Stack::new("gated-notifications".into())
        .inputs(vec![super::helpers::gate_input("deliveryEnabled", "Enable delivery", "Create the bucket and queue")])
        .add_enabled_when(Queue::new("jobs".into()).build(), ResourceLifecycle::Frozen, "deliveryEnabled")
        .add_enabled_when(storage, ResourceLifecycle::Frozen, "deliveryEnabled")
        .build();
    assert_terraform_valid(&render(&stack, TerraformTarget::Aws, StackSettings::default()), "Gated storage notifications");
}

/// A gated Storage notifying an always-on queue: the queue's ingress policy exists only
/// while the Storage does, and never indexes into a bucket the deployer turned off.
#[test]
fn aws_queue_ingress_follows_the_only_notifying_storage_gate() {
    let storage = Storage::new("files".into())
        .notifications(vec![StorageNotification::builder()
            .queue(ResourceRef::new(Queue::RESOURCE_TYPE, "jobs"))
            .build()])
        .build();
    let stack = Stack::new("gated-source".into())
        .inputs(vec![super::helpers::gate_input(
            "filesEnabled",
            "Enable files",
            "Create the bucket",
        )])
        .add(Queue::new("jobs".into()).build(), ResourceLifecycle::Frozen)
        .add_enabled_when(storage, ResourceLifecycle::Frozen, "filesEnabled")
        .build();
    let module = render(&stack, TerraformTarget::Aws, StackSettings::default());
    let body = hcl::parse(module.get("jobs.tf").expect("queue module")).expect("valid HCL");
    let policy = body
        .blocks()
        .find(|block| {
            block.identifier() == "resource"
                && block.labels().iter().map(|label| label.as_str()).collect::<Vec<_>>()
                    == ["aws_sqs_queue_policy", "jobs_storage_ingress"]
        })
        .expect("queue ingress policy");
    let attribute = |key: &str| {
        policy
            .body()
            .attributes()
            .find(|attribute| attribute.key() == key)
            .map(|attribute| attribute.expr().to_string())
            .unwrap_or_else(|| panic!("policy has no {key}"))
    };
    assert_eq!(attribute("count"), "var.input_files_enabled ? 1 : 0");
    assert!(
        attribute("policy").contains("flatten([aws_s3_bucket.files[*].arn])"),
        "{}",
        attribute("policy")
    );
    assert_terraform_valid(&module, "Gated notifying storage with an always-on queue");
}
