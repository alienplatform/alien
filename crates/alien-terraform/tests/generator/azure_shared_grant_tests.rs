//! Azure accepts one role assignment per scope, role and principal, whatever the
//! assignment is called; a second one fails the apply with `RoleAssignmentExists`.
//! Some storage grants sit on the shared storage account (`storage/heartbeat`'s
//! Reader), so every Storage requests the same one. The module must render it once,
//! and it must exist while any Storage that needs it is enabled.

use super::helpers::{assert_terraform_valid, gate_input, render};
use alien_core::{
    AzureResourceGroup, AzureStorageAccount, ManagementPermissions, PermissionProfile,
    RemoteStackManagement, ResourceLifecycle, ServiceAccount, Stack, StackSettings, Storage,
};
use alien_terraform::{ModuleFiles, TerraformTarget};

const STORAGES: [&str; 3] = ["first", "second", "third"];
const STORAGE_BLOB_DATA_CONTRIBUTOR_ROLE_ID: &str = "ba92f5b4-2d11-453d-a403-e96b0029c9fe";
const ACCOUNT_SCOPE: &str = "\"/subscriptions/${var.azure_subscription_id}/resourceGroups/${var.azure_resource_group_name}/providers/Microsoft.Storage/storageAccounts/${azurerm_storage_account.default_storage_account.name}\"";
const PRINCIPALS: [(&str, &str); 2] = [
    ("app", "azurerm_user_assigned_identity.app_sa.principal_id"),
    (
        "management",
        "azurerm_user_assigned_identity.management.principal_id",
    ),
];

/// Three Storages whose app and management grants include an account-level Reader;
/// `gates[i]` gates Storage `i` on a deployer input.
fn stack(gates: [Option<&str>; 3]) -> Stack {
    let mut permissions = PermissionProfile::new();
    for storage in STORAGES {
        permissions = permissions.resource(storage, ["storage/heartbeat", "storage/data-write"]);
    }
    let mut builder = Stack::new("azure-shared-grants".to_string())
        .inputs(
            gates
                .iter()
                .flatten()
                .map(|input| gate_input(input, "Enable", "Whether to create the Storage."))
                .collect(),
        )
        .permission("app", permissions.clone())
        .management(ManagementPermissions::override_(permissions))
        .add(
            AzureResourceGroup::new("default-resource-group".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            AzureStorageAccount::new("default-storage-account".to_string()).build(),
            ResourceLifecycle::Frozen,
        );
    for (storage, gate) in STORAGES.into_iter().zip(gates) {
        let storage = Storage::new(storage.to_string()).build();
        builder = match gate {
            Some(input) => builder.add_enabled_when(storage, ResourceLifecycle::Frozen, input),
            None => builder.add(storage, ResourceLifecycle::Frozen),
        };
    }
    builder
        .add(
            ServiceAccount::new("app-sa".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            RemoteStackManagement::new("management".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build()
}

struct Assignment {
    label: String,
    count: Option<String>,
    scope: String,
    role: String,
    principal: String,
}

fn assignments(module: &ModuleFiles) -> Vec<Assignment> {
    let mut assignments = Vec::new();
    for (file, content) in module.iter() {
        if !file.ends_with(".tf") {
            continue;
        }
        let body = hcl::parse(content).expect("generated HCL should parse");
        for block in body.blocks() {
            let labels = block.labels();
            if block.identifier() != "resource"
                || labels.first().map(|label| label.as_str()) != Some("azurerm_role_assignment")
            {
                continue;
            }
            let label = labels[1].as_str().to_string();
            let attribute = |key: &str| {
                block
                    .body()
                    .attributes()
                    .find(|attribute| attribute.key() == key)
                    .map(|attribute| attribute.expr().to_string())
            };
            assignments.push(Assignment {
                count: attribute("count"),
                scope: attribute("scope").unwrap_or_else(|| panic!("{label} has no scope")),
                role: attribute("role_definition_id")
                    .or_else(|| attribute("role_definition_name"))
                    .unwrap_or_else(|| panic!("{label} names no role")),
                principal: attribute("principal_id")
                    .unwrap_or_else(|| panic!("{label} has no principal")),
                label,
            });
        }
    }
    assignments
}

fn assert_each_grant_is_unique(assignments: &[Assignment]) {
    for (index, assignment) in assignments.iter().enumerate() {
        if let Some(duplicate) = assignments[index + 1..].iter().find(|other| {
            other.scope == assignment.scope
                && other.role == assignment.role
                && other.principal == assignment.principal
        }) {
            panic!(
                "{} and {} grant the same scope, role and principal",
                assignment.label, duplicate.label
            );
        }
    }
}

/// The account-level grant for each principal, as `(label, count)`.
fn account_grants(assignments: &[Assignment]) -> Vec<(String, String, Option<String>)> {
    PRINCIPALS
        .iter()
        .flat_map(|(name, principal)| {
            assignments
                .iter()
                .filter(|assignment| {
                    assignment.scope == ACCOUNT_SCOPE && assignment.principal == *principal
                })
                .map(|assignment| {
                    (
                        name.to_string(),
                        assignment.label.clone(),
                        assignment.count.clone(),
                    )
                })
        })
        .collect()
}

#[test]
fn shared_account_grant_renders_once_and_container_grants_stay_per_storage() {
    let module = render(
        &stack([None, None, None]),
        TerraformTarget::Azure,
        StackSettings::default(),
    );
    let assignments = assignments(&module);
    assert_each_grant_is_unique(&assignments);
    assert_eq!(
        account_grants(&assignments),
        vec![
            ("app".into(), "first_reader_app_assignment_0".into(), None),
            (
                "management".into(),
                "first_reader_management_assignment_0".into(),
                None
            ),
        ],
        "the first Storage keeps the shared grant at its existing address"
    );
    for (name, principal) in PRINCIPALS {
        for storage in STORAGES {
            assert!(
                assignments.iter().any(|assignment| {
                    assignment.principal == principal
                        && assignment
                            .role
                            .contains(STORAGE_BLOB_DATA_CONTRIBUTOR_ROLE_ID)
                        && assignment
                            .scope
                            .contains("/blobServices/default/containers/")
                        && assignment
                            .scope
                            .contains(&format!("resource_prefix}}-{storage}"))
                }),
                "{storage} keeps its own container-scoped grant for {name}"
            );
        }
    }
    assert_terraform_valid(&module, "three ungated Storages");
}

#[test]
fn shared_grant_is_unconditional_when_any_requesting_storage_is() {
    let module = render(
        &stack([Some("firstEnabled"), None, Some("thirdEnabled")]),
        TerraformTarget::Azure,
        StackSettings::default(),
    );
    let assignments = assignments(&module);
    assert_each_grant_is_unique(&assignments);
    for (_, label, count) in account_grants(&assignments) {
        assert_eq!(
            count, None,
            "{label} must survive turning the first Storage off while the second is always on"
        );
    }
    assert_terraform_valid(&module, "first and third Storage gated");
}

#[test]
fn shared_grant_exists_while_any_gated_storage_is_enabled() {
    let module = render(
        &stack([
            Some("firstEnabled"),
            Some("secondEnabled"),
            Some("thirdEnabled"),
        ]),
        TerraformTarget::Azure,
        StackSettings::default(),
    );
    let assignments = assignments(&module);
    assert_each_grant_is_unique(&assignments);
    let grants = account_grants(&assignments);
    assert_eq!(grants.len(), PRINCIPALS.len());
    for (_, label, count) in grants {
        assert_eq!(
            count.as_deref(),
            Some(
                "max(var.input_first_enabled ? 1 : 0, var.input_second_enabled ? 1 : 0, var.input_third_enabled ? 1 : 0)"
            ),
            "{label}"
        );
    }
    assert_terraform_valid(&module, "every Storage gated");
}
