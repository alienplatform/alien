//! AWS data-layer scenarios — storage / kv / queue / vault.
//!
//! Each scenario is one multi-file snapshot so reviewers see the
//! complete module a developer would `terraform apply`. Every scenario
//! goes through `terraform fmt -check` + `terraform validate` against
//! the real AWS provider.

use super::helpers::{
    assert_terraform_valid, normalize_module_whitespace, render, snapshot_module,
};
use alien_core::{
    Kv, LifecycleRule, PermissionGate, PermissionProfile, PermissionsConfig, Queue,
    ResourceLifecycle, ServiceAccount, Stack, StackInputDefinition, StackInputKind,
    StackInputProvider, StackSettings, Storage, Vault,
};
use alien_terraform::TerraformTarget;

#[test]
fn aws_storage_minimal_renders_idiomatic_module() {
    let stack = Stack::new("acme-prod".to_string())
        .add(
            Storage::new("data".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Aws, StackSettings::default());
    snapshot_module("aws_storage_minimal", &module);
    assert_terraform_valid(&module, "aws_storage_minimal");
}

#[test]
fn aws_storage_with_versioning_and_lifecycle() {
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
    let module = render(&stack, TerraformTarget::Aws, StackSettings::default());
    snapshot_module("aws_storage_versioning_and_lifecycle", &module);
    assert_terraform_valid(&module, "aws_storage_versioning_and_lifecycle");
}

#[test]
fn aws_storage_public_read_allows_get_object() {
    let stack = Stack::new("acme-public".to_string())
        .add(
            Storage::new("assets".to_string()).public_read(true).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Aws, StackSettings::default());
    snapshot_module("aws_storage_public_read", &module);
    assert_terraform_valid(&module, "aws_storage_public_read");
}

#[test]
fn aws_kv_renders_dynamodb_table_with_pitr() {
    let stack = Stack::new("acme-kv".to_string())
        .add(
            Kv::new("metadata".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Aws, StackSettings::default());
    snapshot_module("aws_kv_minimal", &module);
    assert_terraform_valid(&module, "aws_kv_minimal");
}

#[test]
fn aws_queue_renders_sqs_with_managed_sse() {
    let stack = Stack::new("acme-queue".to_string())
        .add(
            Queue::new("jobs".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Aws, StackSettings::default());
    snapshot_module("aws_queue_minimal", &module);
    assert_terraform_valid(&module, "aws_queue_minimal");
}

#[test]
fn aws_vault_emits_only_import_data() {
    let stack = Stack::new("acme-vault".to_string())
        .add(
            Vault::new("secrets".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Aws, StackSettings::default());
    snapshot_module("aws_vault_minimal", &module);
    assert_terraform_valid(&module, "aws_vault_minimal");
}

#[test]
fn aws_vault_resource_permissions_attach_to_service_account_role() {
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
    let module = render(&stack, TerraformTarget::Aws, StackSettings::default());
    let rendered = module
        .iter()
        .map(|(_, contents)| contents)
        .collect::<String>();

    assert!(rendered.contains("aws_iam_role_policy\" \"execution_sa_vault_secrets_set_0\""));
    assert!(rendered.contains("ssm:GetParameter"));
    assert!(rendered.contains("parameter/${local.resource_prefix}-secrets-*"));
    assert_terraform_valid(&module, "aws_vault_service_account_permissions");
}

#[test]
fn aws_data_layer_renders_complete_stack() {
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
    let module = render(&stack, TerraformTarget::Aws, StackSettings::default());
    snapshot_module("aws_data_layer_full", &module);
    assert_terraform_valid(&module, "aws_data_layer_full");
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
fn aws_gated_permission_grants_follow_stack_inputs() {
    // Two resource grants are gated on deployer inputs (one enum, one boolean)
    // and one is left ungated. The gated grants must carry an input-conditioned
    // `count` as their first attribute so the baked role genuinely lacks the
    // grant when the deployer's value does not match; the ungated grant renders
    // unconditionally, exactly as before gates existed.
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
    let module = render(&stack, TerraformTarget::Aws, StackSettings::default());
    snapshot_module("aws_gated_permission_grants", &module);

    let normalized = normalize_module_whitespace(&module);
    // Fail-closed / least-privilege: an unresolved gate input renders count = 0,
    // so the baked role lacks the grant unless the deployer's value matches.
    assert!(
        normalized.contains(
            "resource \"aws_iam_role_policy\" \"execution_sa_queue_jobs_set_0\" {\ncount = var.input_queue_mode == null ? 0 : (tostring(var.input_queue_mode) == \"on\" ? 1 : 0)"
        ),
        "gated queue/data-write grant must carry the enum-input count as its first attribute"
    );
    assert!(
        normalized.contains(
            "resource \"aws_iam_role_policy\" \"execution_sa_kv_metadata_set_0\" {\ncount = var.input_kv_enabled == null ? 0 : (tostring(var.input_kv_enabled) == \"true\" ? 1 : 0)"
        ),
        "gated kv/data-write grant must carry the boolean-input count as its first attribute"
    );
    assert_eq!(
        normalized.matches("count = var.input_").count(),
        2,
        "only the two gated grants may carry input counts"
    );
    assert!(
        normalized
            .contains("resource \"aws_iam_role_policy\" \"assets_execution_sa_storage-data-read_0\""),
        "ungated storage/data-read grant must still be emitted"
    );
    assert!(
        normalized.contains("sqs:SendMessage"),
        "gated queue grant must keep its statements"
    );
    assert!(
        normalized.contains("table/${aws_dynamodb_table.metadata.name}"),
        "kv grant must scope to the emitted table"
    );
    assert!(
        normalized.contains(":${aws_sqs_queue.jobs.name}"),
        "queue grant must scope to the emitted queue"
    );
    assert_terraform_valid(&module, "aws_gated_permission_grants");
}
