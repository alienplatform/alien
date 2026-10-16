//! Azure identity & network — service-account / management /
//! network (Create + ByoVnetAzure + UseDefault).
//!
//! Mirrors the GCP identity test layout. Service-account covers the
//! `AzureRuntimePermissionsGenerator` integration path; RSM covers the
//! cross-tenant federated-identity-credential shape.

use super::helpers::{assert_terraform_valid, gate_input, render, snapshot_module};
use alien_core::{
    AzureContainerAppsEnvironment, AzureResourceGroup, Key, ManagementPermissions, Network,
    NetworkSettings, PermissionProfile, RemoteStackManagement, ResourceLifecycle, ServiceAccount,
    Stack, StackSettings, Worker, WorkerCode,
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

/// Every custom management role assignment in the module, resolved through the role definition it
/// references: scope -> the actions assigned there.
fn management_custom_grants_by_scope(
    module: &alien_terraform::ModuleFiles,
) -> std::collections::BTreeMap<String, std::collections::BTreeSet<String>> {
    use hcl::{Body, Expression};
    let unquote = |expr: &Expression| expr.to_string().trim_matches('"').to_string();
    let bodies: Vec<Body> = module
        .iter()
        .filter(|(path, _)| path.ends_with(".tf"))
        .map(|(path, contents)| hcl::parse(contents).unwrap_or_else(|e| panic!("{path}: {e}")))
        .collect();
    let resources = || {
        bodies.iter().flat_map(|body| {
            body.blocks()
                .filter(|block| block.identifier() == "resource")
                .map(|block| {
                    let labels: Vec<String> = block
                        .labels()
                        .iter()
                        .map(|l| l.as_str().to_string())
                        .collect();
                    (labels[0].clone(), labels[1].clone(), block.clone())
                })
        })
    };
    let role_actions: std::collections::BTreeMap<String, std::collections::BTreeSet<String>> =
        resources()
            .filter(|(kind, _, _)| kind == "azurerm_role_definition")
            .map(|(_, name, block)| {
                let permissions = block
                    .body()
                    .blocks()
                    .find(|b| b.identifier() == "permissions")
                    .expect("a role definition has permissions");
                let actions = permissions
                    .body()
                    .attributes()
                    .find(|a| a.key() == "actions")
                    .expect("permissions have actions");
                let Expression::Array(actions) = actions.expr() else {
                    panic!("actions is a list")
                };
                (name, actions.iter().map(unquote).collect())
            })
            .collect();
    let mut grants = std::collections::BTreeMap::new();
    for (kind, _, block) in resources() {
        if kind != "azurerm_role_assignment" {
            continue;
        }
        let attribute = |key: &str| {
            block
                .body()
                .attributes()
                .find(|a| a.key() == key)
                .map(|a| a.expr().clone())
        };
        let Some(role) = attribute("role_definition_id") else {
            continue;
        };
        let role = hcl::format::to_string(&role).expect("role reference formats");
        let Some(name) = role
            .strip_prefix("azurerm_role_definition.")
            .and_then(|rest| rest.strip_suffix(".role_definition_resource_id"))
        else {
            continue;
        };
        let scope = unquote(&attribute("scope").expect("an assignment has a scope"));
        grants
            .entry(scope)
            .or_insert_with(std::collections::BTreeSet::new)
            .extend(role_actions[name].iter().cloned());
    }
    grants
}

/// A custom role grants its actions on everything under the scope it is assigned at. The
/// subscription assignment may therefore carry only what the permission sets bind at the
/// subscription (SKU and usage discovery); scale set and gateway writes stay at the deployment
/// resource group.
#[test]
fn azure_management_grants_stay_at_the_scope_their_permission_set_names() {
    let stack = Stack::new("acme-compute-zones".to_string())
        .management(ManagementPermissions::extend(
            PermissionProfile::new().global(["compute-cluster/management", "container/provision"]),
        ))
        .add(resource_group(), ResourceLifecycle::Frozen)
        .add(
            RemoteStackManagement::new("management".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Azure, StackSettings::default());
    assert_terraform_valid(&module, "azure_compute_management_scopes");

    let grants = management_custom_grants_by_scope(&module);
    let subscription = grants
        .get("/subscriptions/${var.azure_subscription_id}")
        .expect("SKU discovery is assigned at the subscription");
    assert_eq!(
        subscription.iter().map(String::as_str).collect::<Vec<_>>(),
        [
            "Microsoft.Compute/locations/usages/read",
            "Microsoft.Compute/skus/read"
        ],
        "the subscription assignment must carry only the subscription grants"
    );
    let resource_group = grants
        .get("/subscriptions/${var.azure_subscription_id}/resourceGroups/${var.azure_resource_group_name}")
        .expect("the management grants are assigned at the deployment resource group");
    for action in [
        "Microsoft.Compute/virtualMachineScaleSets/write",
        "Microsoft.Network/applicationGateways/write",
    ] {
        assert!(
            resource_group.contains(action),
            "{action} at the resource group"
        );
    }
    assert_eq!(grants.len(), 2, "{grants:?}");
}

/// An inline permission set can bind its grants at a resource group other than the deployment's.
/// That scope gets its own role, with its own Terraform address and Azure name, so the module
/// still validates and each grant stays at the scope it names.
#[test]
fn azure_management_grant_at_another_resource_group_gets_its_own_role() {
    let mut other = alien_permissions::get_permission_set("network/heartbeat")
        .cloned()
        .expect("the built-in set is registered");
    other.id = "custom/other-group-network-read".to_string();
    let value = serde_json::to_value(&other).expect("permission set serializes");
    // Two groups whose names differ only in punctuation must still get separate roles.
    let bound_at = |group: &str, id: &str| {
        let mut value = value.clone();
        value["id"] = serde_json::json!(id);
        for entry in value["platforms"]["azure"]
            .as_array_mut()
            .expect("network/heartbeat has Azure grants")
        {
            entry["binding"]["stack"]["scope"] =
                serde_json::json!(format!("/subscriptions/${{subscriptionId}}/resourceGroups/{group}"));
        }
        serde_json::from_value::<alien_core::permissions::PermissionSet>(value)
            .expect("permission set deserializes")
    };
    let other = bound_at("shared-network", "custom/other-group-network-read");
    let similar = bound_at("shared.network", "custom/similar-group-network-read");
    let stack = Stack::new("acme-other-group".to_string())
        .management(ManagementPermissions::extend(
            PermissionProfile::new().global([
                alien_core::PermissionSetReference::from_name("container/provision"),
                alien_core::PermissionSetReference::from_inline(other),
                alien_core::PermissionSetReference::from_inline(similar),
            ]),
        ))
        .add(resource_group(), ResourceLifecycle::Frozen)
        .add(
            RemoteStackManagement::new("management".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Azure, StackSettings::default());
    assert_terraform_valid(&module, "azure_management_other_group");

    let grants = management_custom_grants_by_scope(&module);
    let other_group = grants
        .get("/subscriptions/${var.azure_subscription_id}/resourceGroups/shared-network")
        .expect("the other group has its own assignment");
    assert!(
        !other_group.contains("Microsoft.Network/applicationGateways/write"),
        "the other group's role holds only its own grants: {other_group:?}"
    );
    assert!(grants
        .contains_key("/subscriptions/${var.azure_subscription_id}/resourceGroups/shared.network"));
    assert!(grants
        .get("/subscriptions/${var.azure_subscription_id}/resourceGroups/${var.azure_resource_group_name}")
        .expect("the deployment group keeps its role")
        .contains("Microsoft.Network/applicationGateways/write"));
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

/// The sandbox management grants have to reach the module: both are compiled at stack scope, so
/// success alone doesn't prove the actions rendered — a grant compiled to a scope no emitter
/// renders leaves the module valid while the manager can't read the sandbox it owns.
#[test]
fn azure_sandbox_management_grants_reach_the_module() {
    let stack = Stack::new("acme-sbx".to_string())
        .management(ManagementPermissions::extend(
            PermissionProfile::new().global(["sandbox/heartbeat", "sandbox/management"]),
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
        rendered.contains("Microsoft.App/sandboxGroups/read"),
        "the sandbox heartbeat read never reached the module",
    );
    assert!(
        rendered.contains("Microsoft.App/sandboxGroups/sandboxes/write"),
        "the sandbox session lifecycle actions never reached the module",
    );
    assert!(
        !rendered.contains("sandboxes/exec"),
        "session-content access belongs to sandbox/execute, not the management role",
    );
    assert_terraform_valid(&module, "azure_sandbox_management_grants");
}

#[test]
fn azure_provider_tracks_partial_creates_and_protects_detached_keys() {
    let stack = Stack::new("acme-rg".to_string())
        .add(resource_group(), ResourceLifecycle::Frozen)
        .add(
            AzureContainerAppsEnvironment::new("environment".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Key::new("retained-key".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Azure, StackSettings::default());
    let providers = module
        .get("providers.tf")
        .expect("providers.tf")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        providers.contains("persist_id_on_create_before_polling_for_completion = true"),
        "accepted creates must remain tracked if polling fails:\n{providers}"
    );
    assert!(
        providers.contains("resource_group { prevent_deletion_if_contains_resources = true }"),
        "group deletion must preserve detached key vaults:\n{providers}"
    );
    snapshot_module("azure_partial_create_with_retained_key", &module);
    assert_terraform_valid(&module, "azure_provider_partial_create_with_retained_key");
}
