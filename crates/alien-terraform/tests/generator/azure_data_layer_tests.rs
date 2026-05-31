//! Azure data-layer scenarios — storage / kv / queue / vault.
//!
//! Mirror of `gcp_data_layer_tests.rs` for Azure. Each scenario is a
//! single multi-file snapshot — the security team reads the full
//! rendered module a developer would `terraform apply`. `terraform fmt
//! -check` + `terraform validate` run against the real `hashicorp/azurerm`
//! provider.
//!
//! Auxiliary resources (`AzureResourceGroup`, `AzureStorageAccount`,
//! `AzureServiceBusNamespace`) are added explicitly because the rebuild
//! preflight pipeline is what wires them up at runtime. The tests stay
//! self-contained.

use super::helpers::{assert_terraform_valid, render, snapshot_module};
use alien_core::{
    AzureResourceGroup, AzureServiceBusNamespace, AzureStorageAccount, Kv, LifecycleRule,
    PermissionProfile, Queue, ResourceLifecycle, ResourceRef, ServiceAccount, Stack, StackSettings,
    Storage, Vault,
};
use alien_terraform::{generate_terraform_module, TerraformOptions, TerraformTarget, TfRegistry};

fn resource_group() -> AzureResourceGroup {
    AzureResourceGroup::new("default-resource-group".to_string()).build()
}

fn storage_account() -> AzureStorageAccount {
    AzureStorageAccount::new("default-storage-account".to_string()).build()
}

fn service_bus_namespace() -> AzureServiceBusNamespace {
    AzureServiceBusNamespace::new("default-service-bus-namespace".to_string()).build()
}

#[test]
fn azure_resource_dependencies_emit_depends_on() {
    let stack = Stack::new("acme-deps".to_string())
        .add(resource_group(), ResourceLifecycle::Frozen)
        .add_with_dependencies(
            storage_account(),
            ResourceLifecycle::Frozen,
            vec![ResourceRef::new(
                AzureResourceGroup::RESOURCE_TYPE,
                "default-resource-group",
            )],
        )
        .build();
    let module = render(&stack, TerraformTarget::Azure, StackSettings::default());
    let storage_account_tf = module
        .get("default_storage_account.tf")
        .expect("storage account file");

    assert!(storage_account_tf.contains("depends_on = ["));
    assert!(storage_account_tf.contains("azurerm_resource_group.default_resource_group"));
    assert_terraform_valid(&module, "azure_resource_dependencies");
}

#[test]
fn azure_storage_minimal_renders_idiomatic_module() {
    let stack = Stack::new("acme-prod".to_string())
        .add(resource_group(), ResourceLifecycle::Frozen)
        .add(storage_account(), ResourceLifecycle::Frozen)
        .add(
            Storage::new("data".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Azure, StackSettings::default());
    snapshot_module("azure_storage_minimal", &module);
    assert_terraform_valid(&module, "azure_storage_minimal");
}

#[test]
fn azure_storage_profile_permissions_emit_container_role_assignment() {
    let stack = Stack::new("acme-storage-permissions".to_string())
        .permissions(alien_core::PermissionsConfig::new().with_profile(
            "app",
            PermissionProfile::new().resource("files", ["storage/data-write"]),
        ))
        .add(resource_group(), ResourceLifecycle::Frozen)
        .add(storage_account(), ResourceLifecycle::Frozen)
        .add(
            Storage::new("files".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            ServiceAccount::new("app-sa".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();

    let module = render(&stack, TerraformTarget::Azure, StackSettings::default());
    let rendered = module
        .iter()
        .map(|(_, contents)| contents.as_ref())
        .collect::<Vec<_>>()
        .join("\n");

    assert!(rendered.contains("ba92f5b4-2d11-453d-a403-e96b0029c9fe"));
    assert!(rendered.contains("azurerm_storage_container.files.name"));
    assert!(rendered.contains("azurerm_user_assigned_identity.app_sa.principal_id"));
    assert!(rendered.contains("blobServices/default/containers"));
    assert_terraform_valid(&module, "azure_storage_profile_permissions");
}

#[test]
fn azure_storage_profile_permissions_fail_for_unknown_permission_set() {
    let stack = Stack::new("acme-storage-permissions".to_string())
        .permissions(alien_core::PermissionsConfig::new().with_profile(
            "app",
            PermissionProfile::new().resource("files", ["storage/not-a-real-permission"]),
        ))
        .add(resource_group(), ResourceLifecycle::Frozen)
        .add(storage_account(), ResourceLifecycle::Frozen)
        .add(
            Storage::new("files".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            ServiceAccount::new("app-sa".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let registry = TfRegistry::built_in();
    let err = generate_terraform_module(
        &stack,
        TerraformTarget::Azure,
        TerraformOptions {
            display_name: None,
            registry: &registry,
            stack_settings: StackSettings::default(),
            registration: None,
            helm_install: None,
            supported_aws_regions: Vec::new(),
        },
    )
    .expect_err("unknown Azure storage permission set should fail module generation");

    assert!(err.to_string().contains("storage/not-a-real-permission"));
}

#[test]
fn azure_storage_with_versioning_lifts_versioning_to_account() {
    let stack = Stack::new("acme-audit".to_string())
        .add(resource_group(), ResourceLifecycle::Frozen)
        .add(storage_account(), ResourceLifecycle::Frozen)
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
    let module = render(&stack, TerraformTarget::Azure, StackSettings::default());
    snapshot_module("azure_storage_versioning_and_lifecycle", &module);
    assert_terraform_valid(&module, "azure_storage_versioning_and_lifecycle");
}

#[test]
fn azure_storage_public_read_uses_blob_access_type() {
    let stack = Stack::new("acme-public".to_string())
        .add(resource_group(), ResourceLifecycle::Frozen)
        .add(storage_account(), ResourceLifecycle::Frozen)
        .add(
            Storage::new("assets".to_string()).public_read(true).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Azure, StackSettings::default());
    snapshot_module("azure_storage_public_read", &module);
    assert_terraform_valid(&module, "azure_storage_public_read");
}

#[test]
fn azure_kv_renders_storage_table() {
    let stack = Stack::new("acme-kv".to_string())
        .add(resource_group(), ResourceLifecycle::Frozen)
        .add(storage_account(), ResourceLifecycle::Frozen)
        .add(
            Kv::new("metadata".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Azure, StackSettings::default());
    snapshot_module("azure_kv_minimal", &module);
    assert_terraform_valid(&module, "azure_kv_minimal");
}

#[test]
fn azure_queue_renders_service_bus_queue() {
    let stack = Stack::new("acme-queue".to_string())
        .add(resource_group(), ResourceLifecycle::Frozen)
        .add(service_bus_namespace(), ResourceLifecycle::Frozen)
        .add(
            Queue::new("jobs".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Azure, StackSettings::default());
    snapshot_module("azure_queue_minimal", &module);
    assert_terraform_valid(&module, "azure_queue_minimal");
}

#[test]
fn azure_vault_renders_key_vault() {
    let stack = Stack::new("acme-vault".to_string())
        .add(resource_group(), ResourceLifecycle::Frozen)
        .add(
            Vault::new("secrets".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Azure, StackSettings::default());
    snapshot_module("azure_vault_minimal", &module);
    assert_terraform_valid(&module, "azure_vault_minimal");
}

#[test]
fn azure_vault_resource_permissions_attach_to_service_account() {
    let stack = Stack::new("acme-vault".to_string())
        .permission(
            "execution",
            PermissionProfile::new().resource("secrets", ["vault/data-read"]),
        )
        .add(resource_group(), ResourceLifecycle::Frozen)
        .add(
            ServiceAccount::new("execution-sa".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Vault::new("secrets".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Azure, StackSettings::default());
    let rendered = module
        .iter()
        .map(|(_, contents)| contents)
        .collect::<String>();

    assert!(rendered.contains("4633458b-17de-408a-b874-0445c86b69e6"));
    assert!(rendered.contains("azurerm_user_assigned_identity.execution_sa.principal_id"));
    assert!(rendered.contains("secrets_user_execution"));
    assert_terraform_valid(&module, "azure_vault_service_account_permissions");
}

#[test]
fn azure_data_layer_renders_complete_stack() {
    let stack = Stack::new("acme-data".to_string())
        .add(resource_group(), ResourceLifecycle::Frozen)
        .add(storage_account(), ResourceLifecycle::Frozen)
        .add(service_bus_namespace(), ResourceLifecycle::Frozen)
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
    let module = render(&stack, TerraformTarget::Azure, StackSettings::default());
    snapshot_module("azure_data_layer_full", &module);
    assert_terraform_valid(&module, "azure_data_layer_full");
}
