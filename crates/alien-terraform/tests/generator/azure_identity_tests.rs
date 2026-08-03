//! Azure identity & network — service-account / management /
//! network (Create + ByoVnetAzure + UseDefault).
//!
//! Mirrors the GCP identity test layout. Service-account covers the
//! `AzureRuntimePermissionsGenerator` integration path; RSM covers the
//! cross-tenant federated-identity-credential shape.

use super::helpers::{assert_terraform_valid, gate_input, render, snapshot_module};
use alien_core::{
    AzureResourceGroup, ManagementPermissions, Network, NetworkSettings, PermissionProfile,
    RemoteStackManagement, ResourceLifecycle, ServiceAccount, Stack, StackSettings, Worker,
    WorkerCode,
};
use alien_terraform::TerraformTarget;

/// A minimal Live worker for the management-grant tests.
fn dispatch_worker(id: &str) -> Worker {
    Worker::new(id.to_string())
        .code(WorkerCode::Image {
            image: format!("registry.example.com/app/{id}:1.2.3"),
        })
        .permissions("execution".to_string())
        .build()
}

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
        "Azure setup/live handoff waits in the registered frozen controller, not Terraform"
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
fn azure_compute_management_emits_subscription_scoped_sku_discovery() {
    let stack = Stack::new("acme-compute-zones".to_string())
        .management(ManagementPermissions::extend(
            PermissionProfile::new().global(["compute-cluster/management"]),
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

    assert!(rendered.contains("Microsoft.Compute/skus/read"));
    assert!(rendered.contains("\"/subscriptions/${var.azure_subscription_id}\""));
    assert_terraform_valid(&module, "azure_compute_management_sku_discovery");
}

/// Azure folds every resource-scoped management permission set into one
/// assignment, so the grant belongs to all of its contributors at once. When
/// every contributor is gated the assignment carries their gates, and
/// declining them all takes the grant with it. The role definition stays
/// ungated on purpose — a definition nothing is assigned to grants nothing.
#[test]
fn azure_management_assignment_follows_the_gates_of_its_only_contributors() {
    let stack = Stack::new("acme-mgmt-gated".to_string())
        .inputs(vec![
            gate_input(
                "jobsEnabled",
                "Enable jobs",
                "Whether to run the jobs worker.",
            ),
            gate_input(
                "auditEnabled",
                "Enable audit",
                "Whether to run the audit worker.",
            ),
        ])
        .management(ManagementPermissions::extend(
            PermissionProfile::new()
                .resource("jobs", ["worker/dispatch-command"])
                .resource("audit", ["worker/dispatch-command"]),
        ))
        .add(resource_group(), ResourceLifecycle::Frozen)
        .add(
            RemoteStackManagement::new("management".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add_enabled_when(
            dispatch_worker("jobs"),
            ResourceLifecycle::Live,
            "jobsEnabled",
        )
        .add_enabled_when(
            dispatch_worker("audit"),
            ResourceLifecycle::Live,
            "auditEnabled",
        )
        .build();

    let module = render(&stack, TerraformTarget::Azure, StackSettings::default());
    let rendered = module
        .iter()
        .map(|(_, contents)| contents)
        .collect::<Vec<_>>()
        .join("\n");

    let assignment = rendered
        .split("resource \"")
        .find(|block| {
            block.starts_with("azurerm_role_assignment\" \"management_management_uami_assignment")
        })
        .unwrap_or_else(|| panic!("a management assignment should render:\n{rendered}"));
    // Gates render in sorted order so reordering resources cannot churn the plan.
    assert!(
        assignment.contains("var.input_audit_enabled || var.input_jobs_enabled ? 1 : 0"),
        "the merged grant must follow every contributor's gate:\n{assignment}"
    );

    // A custom role definition, when one is needed at all, stays ungated: a
    // definition nothing is assigned to grants nothing. (This fixture's grant
    // resolves to a predefined Azure role, so none is emitted here.)
    for block in rendered.split("resource \"") {
        if block.starts_with("azurerm_role_definition\"") {
            assert!(
                !block.contains("count"),
                "an unassigned role definition grants nothing, so it stays ungated:\n{block}"
            );
        }
    }

    assert_terraform_valid(&module, "azure_management_assignment_gated");
}

/// One ungated contributor means the grant is needed unconditionally —
/// declining its gated sibling must not revoke it.
#[test]
fn azure_management_assignment_stays_ungated_when_a_contributor_is_ungated() {
    let stack = Stack::new("acme-mgmt-mixed".to_string())
        .inputs(vec![gate_input(
            "jobsEnabled",
            "Enable jobs",
            "Whether to run the jobs worker.",
        )])
        .management(ManagementPermissions::extend(
            PermissionProfile::new()
                .resource("jobs", ["worker/dispatch-command"])
                .resource("always", ["worker/dispatch-command"]),
        ))
        .add(resource_group(), ResourceLifecycle::Frozen)
        .add(
            RemoteStackManagement::new("management".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add_enabled_when(
            dispatch_worker("jobs"),
            ResourceLifecycle::Live,
            "jobsEnabled",
        )
        .add(dispatch_worker("always"), ResourceLifecycle::Live)
        .build();

    let module = render(&stack, TerraformTarget::Azure, StackSettings::default());
    let rendered = module
        .iter()
        .map(|(_, contents)| contents)
        .collect::<Vec<_>>()
        .join("\n");

    let assignment = rendered
        .split("resource \"")
        .find(|block| {
            block.starts_with("azurerm_role_assignment\" \"management_management_uami_assignment")
        })
        .unwrap_or_else(|| panic!("a management assignment should render:\n{rendered}"));
    assert!(
        !assignment.contains("var.input_jobs_enabled"),
        "an ungated contributor keeps the grant unconditional:\n{assignment}"
    );

    assert_terraform_valid(&module, "azure_management_assignment_mixed");
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
            private_endpoint_subnet_name: None,
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
