//! GCP data-layer scenarios — storage / kv / queue / vault.
//!
//! Each scenario is one multi-file snapshot so reviewers see the
//! complete module a developer would `terraform apply`. Every scenario
//! goes through `terraform fmt -check` + `terraform validate` against
//! the real Google provider.

use super::helpers::{assert_terraform_valid, render, snapshot_module};
use alien_core::{
    Kv, LifecycleRule, PermissionProfile, PermissionsConfig, Queue, ResourceLifecycle,
    ServiceAccount, Stack, StackSettings, Storage, Vault,
};
use alien_terraform::TerraformTarget;

#[test]
fn gcp_storage_minimal_renders_idiomatic_module() {
    let stack = Stack::new("acme-prod".to_string())
        .add(
            Storage::new("data".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Gcp, StackSettings::default());
    snapshot_module("gcp_storage_minimal", &module);
    assert_terraform_valid(&module, "gcp_storage_minimal");
}

#[test]
fn gcp_storage_with_versioning_and_lifecycle() {
    let stack = Stack::new("acme-audit".to_string())
        .add(
            Storage::new("audit".to_string())
                .versioning(true)
                .lifecycle_rules(vec![LifecycleRule {
                    days: 90,
                    prefix: Some("logs/".to_string()),
                }])
                .build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Gcp, StackSettings::default());
    snapshot_module("gcp_storage_versioning_and_lifecycle", &module);
    assert_terraform_valid(&module, "gcp_storage_versioning_and_lifecycle");
}

#[test]
fn gcp_storage_public_read_allows_object_viewer() {
    let stack = Stack::new("acme-public".to_string())
        .add(
            Storage::new("assets".to_string()).public_read(true).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Gcp, StackSettings::default());
    snapshot_module("gcp_storage_public_read", &module);
    assert_terraform_valid(&module, "gcp_storage_public_read");
}

#[test]
fn gcp_kv_renders_firestore_database() {
    let stack = Stack::new("acme-kv".to_string())
        .add(
            Kv::new("metadata".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Gcp, StackSettings::default());
    snapshot_module("gcp_kv_minimal", &module);
    assert_terraform_valid(&module, "gcp_kv_minimal");
}

#[test]
fn gcp_queue_renders_pubsub_topic_and_subscription() {
    let stack = Stack::new("acme-queue".to_string())
        .add(
            Queue::new("jobs".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Gcp, StackSettings::default());
    snapshot_module("gcp_queue_minimal", &module);
    assert_terraform_valid(&module, "gcp_queue_minimal");
}

#[test]
fn gcp_queue_permission_profile_splits_topic_and_subscription_iam() {
    let stack = Stack::new("acme-queue".to_string())
        .permissions(PermissionsConfig::new().with_profile(
            "execution",
            PermissionProfile::new().resource("jobs", ["queue/data-write"]),
        ))
        .add(
            Queue::new("jobs".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            ServiceAccount::new("execution-sa".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Gcp, StackSettings::default());
    let rendered = module
        .iter()
        .map(|(_, contents)| contents)
        .collect::<String>();

    assert!(rendered.contains("google_pubsub_topic_iam_member"));
    assert!(rendered.contains("roles/pubsub.publisher"));
    assert!(rendered.contains("google_pubsub_subscription_iam_member"));
    assert!(rendered.contains("roles/pubsub.subscriber"));
    assert!(rendered.contains("roles/pubsub.viewer"));
    assert_terraform_valid(&module, "gcp_queue_permission_profile");
}

#[test]
fn gcp_vault_emits_only_import_data() {
    let stack = Stack::new("acme-vault".to_string())
        .add(
            Vault::new("secrets".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Gcp, StackSettings::default());
    snapshot_module("gcp_vault_minimal", &module);
    assert_terraform_valid(&module, "gcp_vault_minimal");
}

#[test]
fn gcp_data_layer_renders_complete_stack() {
    let stack = Stack::new("acme-data".to_string())
        .add(
            Storage::new("assets".to_string()).versioning(true).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Queue::new("jobs".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Kv::new("metadata".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Vault::new("secrets".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Gcp, StackSettings::default());
    snapshot_module("gcp_data_layer_full", &module);
    assert_terraform_valid(&module, "gcp_data_layer_full");
}
