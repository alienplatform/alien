//! Validate notification wiring with the real provider schema and Terraform graph.

use super::helpers::{assert_terraform_valid, render};
use alien_core::{
    Queue, ResourceLifecycle, ResourceRef, Stack, StackSettings, Storage, StorageNotification,
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
