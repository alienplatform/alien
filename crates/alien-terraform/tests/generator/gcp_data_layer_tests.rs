//! GCP data-layer scenarios — storage / kv / queue / vault.
//!
//! Each scenario is one multi-file snapshot so reviewers see the
//! complete module a developer would `terraform apply`. Every scenario
//! goes through `terraform fmt -check` + `terraform validate` against
//! the real Google provider.

use super::helpers::{
    assert_terraform_valid, normalize_module_whitespace, render, snapshot_module,
};
use alien_core::{
    Kv, LifecycleRule, ManagementPermissions, PermissionGate, PermissionProfile, PermissionsConfig,
    Queue, RemoteStackManagement, ResourceLifecycle, ServiceAccount, Stack, StackInputDefinition,
    StackInputKind, StackInputProvider, StackSettings, Storage, Vault,
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
fn gcp_vault_resource_permissions_attach_to_service_account() {
    let stack = Stack::new("acme-vault".to_string())
        .permission(
            "execution",
            PermissionProfile::new().resource("secrets", ["vault/data-read"]),
        )
        .add(
            ServiceAccount::new("execution-sa".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Vault::new("secrets".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Gcp, StackSettings::default());
    let rendered = module
        .iter()
        .map(|(_, contents)| contents)
        .collect::<String>();

    assert!(rendered.contains("roles/secretmanager.secretAccessor"));
    assert!(rendered.contains("google_service_account.execution_sa.email"));
    assert!(rendered.contains("local.resource_prefix}-secrets-"));
    assert_terraform_valid(&module, "gcp_vault_service_account_permissions");
}

#[test]
fn gcp_vault_management_permissions_disambiguate_iam_member_labels() {
    let stack = Stack::new("acme-vault".to_string())
        .management(ManagementPermissions::extend(
            PermissionProfile::new().resource("secrets", ["vault/heartbeat", "vault/management"]),
        ))
        .add(
            RemoteStackManagement::new("management".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Vault::new("secrets".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Gcp, StackSettings::default());
    let rendered = module
        .iter()
        .map(|(_, contents)| contents)
        .collect::<String>();

    assert!(rendered.contains("secretmanager_viewer_management_secrets_vault_heartbeat_binding_0"));
    assert!(rendered.contains("secretmanager_viewer_management_secrets_vault_management_binding_0"));
    assert_terraform_valid(&module, "gcp_vault_management_permission_labels");
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

/// A minimal deployer input used only to drive a permission gate.
fn gate_input(id: &str, kind: StackInputKind) -> StackInputDefinition {
    StackInputDefinition {
        id: id.to_string(),
        kind,
        provided_by: vec![StackInputProvider::Deployer],
        required: false,
        label: id.to_string(),
        description: "Gate input.".to_string(),
        placeholder: None,
        default: None,
        platforms: None,
        validation: None,
        env: vec![],
    }
}

#[test]
fn gcp_gated_permission_grants_follow_stack_inputs() {
    // Gate two resource grants on deployer inputs (one enum, one boolean) and
    // leave one ungated. A gated IAM member block carries an input-conditioned
    // `count`, so the binding is absent from the baked role unless the input
    // matches; the ungated grant renders unconditionally.
    let profile = PermissionProfile::new()
        .resource("metadata", ["kv/data-write"])
        .resource("jobs", ["queue/data-write"])
        .resource("assets", ["storage/data-read"]);
    let mut permissions = PermissionsConfig::new().with_profile("execution", profile.clone());
    permissions.gates = vec![
        PermissionGate {
            profile: "execution".to_string(),
            resource: "jobs".to_string(),
            permission_set_id: "queue/data-write".to_string(),
            input_id: "queueMode".to_string(),
            enabled_value: "on".to_string(),
        },
        PermissionGate {
            profile: "execution".to_string(),
            resource: "metadata".to_string(),
            permission_set_id: "kv/data-write".to_string(),
            input_id: "kvEnabled".to_string(),
            enabled_value: "true".to_string(),
        },
    ];
    let service_account =
        ServiceAccount::from_permission_profile("execution-sa".to_string(), &profile, |name| {
            alien_permissions::get_permission_set(name).cloned()
        })
        .expect("service account should resolve from profile");

    let stack = Stack::new("acme-gated".to_string())
        .permissions(permissions)
        .inputs(vec![
            gate_input("queueMode", StackInputKind::Enum),
            gate_input("kvEnabled", StackInputKind::Boolean),
        ])
        .add(service_account, ResourceLifecycle::Frozen)
        .add(Kv::new("metadata".to_string()).build(), ResourceLifecycle::Frozen)
        .add(Queue::new("jobs".to_string()).build(), ResourceLifecycle::Frozen)
        .add(Storage::new("assets".to_string()).build(), ResourceLifecycle::Frozen)
        .build();
    let module = render(&stack, TerraformTarget::Gcp, StackSettings::default());
    snapshot_module("gcp_gated_permission_grants", &module);

    let normalized = normalize_module_whitespace(&module);
    // Fail-closed: an unresolved gate input renders count = 0 so the binding is
    // absent from the baked role. The kv grant is one member block; the queue
    // grant fans out to three (topic publisher + subscription subscriber +
    // viewer), and every gated block carries the input count.
    assert!(
        normalized.contains(
            "count = var.input_kv_enabled == null ? 0 : (tostring(var.input_kv_enabled) == \"true\" ? 1 : 0)"
        ),
        "gated kv/data-write binding must carry the boolean-input count"
    );
    assert!(
        normalized.contains(
            "count = var.input_queue_mode == null ? 0 : (tostring(var.input_queue_mode) == \"on\" ? 1 : 0)"
        ),
        "gated queue/data-write bindings must carry the enum-input count"
    );
    assert_eq!(
        normalized.matches("count = var.input_").count(),
        4,
        "the kv grant (1 block) and the queue grant (topic + 2 subscription blocks) are all gated"
    );
    assert!(
        normalized.contains("roles/datastore.user"),
        "gated kv grant must keep its role"
    );
    assert!(
        normalized.contains("roles/pubsub.publisher"),
        "gated queue grant must keep its topic role"
    );
    assert!(
        normalized.contains("gcp_role_read_cloud_storage_objects_assets_execution_sa_storage_0"),
        "ungated storage/data-read grant must still be emitted"
    );
    assert_terraform_valid(&module, "gcp_gated_permission_grants");
}
