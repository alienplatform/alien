//! Azure identity & network — service-account / remote-stack-management /
//! network (Create + ByoVnetAzure + UseDefault).
//!
//! Mirrors the GCP identity test layout. Service-account covers the
//! `AzureRuntimePermissionsGenerator` integration path; RSM covers the
//! cross-tenant federated-identity-credential shape.

use super::helpers::{assert_terraform_valid, render, snapshot_module};
use alien_core::{
    AzureResourceGroup, ManagementPermissions, Network, NetworkSettings, PermissionProfile,
    RemoteStackManagement, ResourceLifecycle, ServiceAccount, Stack, StackSettings,
};
use alien_terraform::TerraformTarget;

fn resource_group() -> AzureResourceGroup {
    AzureResourceGroup::new("default-resource-group".to_string()).build()
}

#[test]
fn azure_service_account_with_permission_set_emits_role_definitions() {
    let sa = ServiceAccount::new("execution-sa".to_string())
        .stack_permission_set(
            alien_permissions::get_permission_set("storage/data-read")
                .expect("storage/data-read permission set")
                .clone(),
        )
        .stack_permission_set(
            alien_permissions::get_permission_set("queue/data-write")
                .expect("queue/data-write permission set")
                .clone(),
        )
        .build();
    let stack = Stack::new("acme-iam".to_string())
        .add(resource_group(), ResourceLifecycle::Frozen)
        .add(sa, ResourceLifecycle::Frozen)
        .build();
    let module = render(&stack, TerraformTarget::Azure, StackSettings::default());
    snapshot_module("azure_service_account", &module);
    assert_terraform_valid(&module, "azure_service_account");
}

#[test]
fn azure_service_account_storage_data_write_uses_predefined_blob_contributor() {
    let sa = ServiceAccount::new("execution-sa".to_string())
        .stack_permission_set(
            alien_permissions::get_permission_set("storage/data-write")
                .expect("storage/data-write permission set")
                .clone(),
        )
        .build();
    let stack = Stack::new("acme-storage-write".to_string())
        .add(resource_group(), ResourceLifecycle::Frozen)
        .add(sa, ResourceLifecycle::Frozen)
        .build();
    let module = render(&stack, TerraformTarget::Azure, StackSettings::default());
    let rendered = module
        .iter()
        .map(|(_, contents)| contents)
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        rendered.contains(
            "/providers/Microsoft.Authorization/roleDefinitions/ba92f5b4-2d11-453d-a403-e96b0029c9fe"
        ),
        "storage/data-write should assign Storage Blob Data Contributor"
    );
    assert!(
        !rendered.contains("Microsoft.Storage/storageAccounts/listKeys/action"),
        "storage/data-write should not emit storage account key access"
    );

    assert_terraform_valid(&module, "azure_service_account_storage_data_write");
}

#[test]
fn azure_remote_stack_management_emits_uami_with_federated_credential() {
    let stack = Stack::new("acme-mgmt".to_string())
        .management(ManagementPermissions::extend(
            PermissionProfile::new().global([
                "worker/provision",
                "storage/provision",
                "artifact-registry/provision",
                "azure-resource-group/heartbeat",
                "network/heartbeat",
                "storage/heartbeat",
                "service-account/heartbeat",
            ]),
        ))
        .add(resource_group(), ResourceLifecycle::Frozen)
        .add(
            RemoteStackManagement::new("management".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Azure, StackSettings::default());
    let rendered = module
        .iter()
        .map(|(_, contents)| contents)
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        !rendered.contains("time_sleep\" \"azure_rbac_propagation\""),
        "Azure setup/live handoff waits in the imported frozen controller, not Terraform"
    );
    assert!(
        !rendered.contains("hashicorp/time"),
        "Azure setup artifacts should not need the time provider"
    );
    assert_eq!(
        rendered
            .matches(
                "resource \"azurerm_role_assignment\" \"management_management_uami_assignment_"
            )
            .count(),
        2,
        "Azure management should emit one combined custom role assignment plus one deduped Reader assignment"
    );
    assert!(
        rendered.contains("Microsoft.Network/virtualNetworks/read"),
        "network heartbeat read must be included in the management custom role"
    );
    assert!(
        rendered.contains(
            "/providers/Microsoft.Authorization/roleDefinitions/acdd72a7-3385-48ef-bd42-f606fba81ae7"
        ),
        "resource-group/storage/service-account heartbeat should emit a deduped Azure Reader assignment"
    );
    snapshot_module("azure_remote_stack_management", &module);
    assert_terraform_valid(&module, "azure_remote_stack_management");
}

#[test]
fn azure_global_network_heartbeat_does_not_emit_resource_scoped_setup_role() {
    let settings = StackSettings {
        network: Some(NetworkSettings::Create {
            cidr: Some("10.46.0.0/16".to_string()),
            availability_zones: 1,
        }),
        ..StackSettings::default()
    };
    let stack = Stack::new("acme-network-heartbeat".to_string())
        .management(ManagementPermissions::extend(
            PermissionProfile::new().global(["network/heartbeat"]),
        ))
        .add(resource_group(), ResourceLifecycle::Frozen)
        .add(
            Network::new("default-network".to_string())
                .settings(settings.network.clone().expect("network settings"))
                .build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            RemoteStackManagement::new("management".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Azure, settings);
    let rendered = module
        .iter()
        .map(|(_, contents)| contents)
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        !rendered.contains("setup_management_network_heartbeat"),
        "network/heartbeat is stack-scoped for Azure and must not emit setup-owned resource roles"
    );
    assert!(
        rendered.contains("Microsoft.Network/virtualNetworks/read"),
        "network/heartbeat should still be compiled into the Azure management identity"
    );
}

#[test]
fn azure_network_create_emits_vnet_subnets_nat() {
    let settings = StackSettings {
        network: Some(NetworkSettings::Create {
            cidr: Some("10.46.0.0/16".to_string()),
            availability_zones: 2,
        }),
        ..StackSettings::default()
    };
    let stack = Stack::new("acme-net".to_string())
        .add(resource_group(), ResourceLifecycle::Frozen)
        .add(
            Network::new("default-network".to_string())
                .settings(settings.network.clone().expect("network"))
                .build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Azure, settings);
    snapshot_module("azure_network_create", &module);
    assert_terraform_valid(&module, "azure_network_create");
}

#[test]
fn azure_network_byo_vnet_emits_data_lookups() {
    let settings = StackSettings {
        network: Some(NetworkSettings::ByoVnetAzure {
            vnet_resource_id:
                "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/shared/providers/Microsoft.Network/virtualNetworks/shared-vnet"
                    .to_string(),
            public_subnet_name: "public".to_string(),
            private_subnet_name: "private".to_string(),
            application_gateway_subnet_name: Some("appgw".to_string()),
        }),
        ..StackSettings::default()
    };
    let stack = Stack::new("acme-byo-azure".to_string())
        .add(resource_group(), ResourceLifecycle::Frozen)
        .add(
            Network::new("default-network".to_string())
                .settings(settings.network.clone().expect("network"))
                .build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Azure, settings);
    snapshot_module("azure_network_byo_vnet", &module);
    assert_terraform_valid(&module, "azure_network_byo_vnet");
}
