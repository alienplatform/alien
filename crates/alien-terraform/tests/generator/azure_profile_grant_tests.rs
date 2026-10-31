//! Terraform-created KVs and artifact registries are imported already Ready, so their
//! controllers never apply the grants permission profiles hold on them. Setup must, or a
//! Worker identity linked to a Frozen KV gets 403 on its table and one pulling from a Frozen
//! registry can't pull.

use std::collections::HashSet;

use super::helpers::{assert_terraform_valid, render};
use alien_core::{
    ArtifactRegistry, AzureResourceGroup, AzureStorageAccount, Kv, PermissionProfile,
    ResourceLifecycle, ResourceRef, ServiceAccount, Stack, StackSettings,
};
use alien_terraform::{ModuleFiles, TerraformTarget};

const STORAGE_TABLE_DATA_CONTRIBUTOR: &str = "0a9a7e1f-b9d0-4cc4-a60d-0319b160aaa3";
const ACR_PULL: &str = "7f951dda-4ed3-4680-a7ca-43fe172d538d";

/// Every `azurerm_role_assignment` in the module as (file, scope, role, principal).
fn role_assignments(module: &ModuleFiles) -> Vec<(String, String, String, String)> {
    let mut assignments = Vec::new();
    for (file, content) in module.iter().filter(|(file, _)| file.ends_with(".tf")) {
        let body = hcl::parse(content).expect("generated HCL should parse");
        for block in body.blocks() {
            if block.identifier() != "resource"
                || block.labels().first().map(|label| label.as_str())
                    != Some("azurerm_role_assignment")
            {
                continue;
            }
            let attribute = |key: &str| {
                block
                    .body()
                    .attributes()
                    .find(|attribute| attribute.key() == key)
                    .map(|attribute| attribute.expr().to_string())
                    .unwrap_or_default()
            };
            assignments.push((
                file.to_string(),
                attribute("scope"),
                attribute("role_definition_id"),
                attribute("principal_id"),
            ));
        }
    }
    assignments
}

/// Azure rejects a second assignment of the same role to the same principal at the same
/// scope (RoleAssignmentExists), which fails the whole apply.
fn assert_no_duplicate_assignments(assignments: &[(String, String, String, String)]) {
    let mut seen = HashSet::new();
    for (file, scope, role, principal) in assignments {
        assert!(
            seen.insert((scope, role, principal)),
            "duplicate role assignment in {file}: {role} for {principal} at {scope}"
        );
    }
}

fn kv_stack() -> Stack {
    let mut stack = Stack::new("kv-grants".to_string())
        .permission(
            "execution",
            PermissionProfile::new()
                .resource("cache", ["kv/data-write"])
                .resource("sessions", ["kv/data-write"]),
        )
        .add(
            AzureResourceGroup::new("default-resource-group".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            AzureStorageAccount::new("storage".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Kv::new("cache".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Kv::new("sessions".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            ServiceAccount::new("execution-sa".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    for id in ["cache", "sessions"] {
        stack
            .resources
            .get_mut(id)
            .unwrap()
            .dependencies
            .push(ResourceRef::new(
                AzureStorageAccount::RESOURCE_TYPE,
                "storage",
            ));
    }
    stack
}

/// Two KVs share one storage account. The controller grants `kv/data-*` on the account, so
/// mirroring it would render the same assignment twice; setup grants each on its own table.
#[test]
fn frozen_kvs_grant_profile_access_on_each_table() {
    let module = render(
        &kv_stack(),
        TerraformTarget::Azure,
        StackSettings::default(),
    );
    let assignments = role_assignments(&module);

    let grants: Vec<_> = assignments
        .iter()
        .filter(|(_, _, role, _)| role.contains(STORAGE_TABLE_DATA_CONTRIBUTOR))
        .map(|(file, scope, _, principal)| (file.clone(), scope.clone(), principal.clone()))
        .collect();
    let table_scope = |kv: &str| {
        format!(
            "\"/subscriptions/${{var.azure_subscription_id}}/resourceGroups/${{var.azure_resource_group_name}}/providers/Microsoft.Storage/storageAccounts/${{azurerm_storage_account.storage.name}}/tableServices/default/tables/${{azapi_resource.{kv}.name}}\""
        )
    };
    let principal = "azurerm_user_assigned_identity.execution_sa.principal_id".to_string();
    assert_eq!(
        grants,
        vec![
            (
                "cache.tf".to_string(),
                table_scope("cache"),
                principal.clone()
            ),
            (
                "sessions.tf".to_string(),
                table_scope("sessions"),
                principal
            ),
        ],
        "one table-data grant per KV, for the profile's identity, on that KV's table"
    );
    assert_no_duplicate_assignments(&assignments);
    assert_terraform_valid(&module, "Frozen KVs with a linking profile");
}

#[test]
fn frozen_artifact_registry_grants_profile_pull_on_the_registry() {
    let stack = Stack::new("registry-grants".to_string())
        .permission(
            "execution",
            PermissionProfile::new().resource("images", ["artifact-registry/pull"]),
        )
        .add(
            AzureResourceGroup::new("default-resource-group".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            ArtifactRegistry::new("images".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            ServiceAccount::new("execution-sa".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Azure, StackSettings::default());
    let assignments = role_assignments(&module);

    let grants: Vec<_> = assignments
        .iter()
        .filter(|(_, _, role, _)| role.contains(ACR_PULL))
        .map(|(file, scope, _, principal)| (file.clone(), scope.clone(), principal.clone()))
        .collect();
    assert_eq!(
        grants,
        vec![(
            "images.tf".to_string(),
            "\"/subscriptions/${var.azure_subscription_id}/resourceGroups/${var.azure_resource_group_name}/providers/Microsoft.ContainerRegistry/registries/${azurerm_container_registry.images.name}\"".to_string(),
            "azurerm_user_assigned_identity.execution_sa.principal_id".to_string(),
        )],
        "one pull grant, for the profile's identity, on this registry"
    );
    assert_no_duplicate_assignments(&assignments);
    assert_terraform_valid(&module, "Frozen artifact registry with a pulling profile");
}
