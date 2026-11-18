//! The store an Azure sandbox's persistent storage is replicated to.
//!
//! Rendered only when declared: an account with a hierarchical namespace (a directory SAS needs
//! one), no account key, no public access, one private container, and two grants for each identity
//! that creates the group's sandboxes: Storage Blob Delegator on the account and Storage Blob Data
//! Contributor on the container. A sandbox without the declaration renders byte for byte as before.

use super::helpers::{assert_terraform_valid, render};
use alien_core::{
    AzureContainerAppsEnvironment, AzureResourceGroup, Key, PermissionProfile, ResourceLifecycle,
    ResourceRef, Sandbox, SandboxCode, SandboxEgress, SandboxLifecyclePolicy,
    SandboxPersistentStorage, ServiceAccount, Stack, StackSettings, Worker, WorkerCode,
};
use alien_terraform::{ModuleFiles, TerraformTarget};

const DELEGATOR: &str = "db58b8e5-c6ad-4a2a-8342-4190687cbf4a";
const DATA_CONTRIBUTOR: &str = "ba92f5b4-2d11-453d-a403-e96b0029c9fe";

fn declared() -> SandboxPersistentStorage {
    SandboxPersistentStorage {
        path: "/workspace".to_string(),
        exclude: vec!["tmp/".to_string()],
        max_size: Some("8Gi".to_string()),
        encryption_key: None,
    }
}

fn sandbox(id: &str, storage: Option<SandboxPersistentStorage>) -> Sandbox {
    let mut sandbox = Sandbox::new(id.to_string())
        .code(SandboxCode::Image {
            image: "ubuntu".to_string(),
            bundled_from: None,
        })
        .egress(SandboxEgress::Allow)
        .lifecycle(SandboxLifecyclePolicy {
            max_lifetime_seconds: None,
            idle_pause_seconds: None,
        })
        .build();
    sandbox.persistent_storage = storage;
    sandbox
}

/// Two sandboxes, `agents` and `other`, a Live Worker linked to both, and the `execution` identity
/// holding the profile's sets.
fn stack(profile: PermissionProfile, agents_storage: Option<SandboxPersistentStorage>) -> Stack {
    stack_with_key(profile, agents_storage, false)
}

fn stack_with_key(
    profile: PermissionProfile,
    agents_storage: Option<SandboxPersistentStorage>,
    with_key: bool,
) -> Stack {
    let agents = sandbox("agents", agents_storage);
    let other = sandbox("other", None);
    let worker = Worker::new("api".to_string())
        .code(WorkerCode::Image {
            image: "acmeprod.azurecr.io/api:1".to_string(),
        })
        .permissions("execution".to_string())
        .link(&agents)
        .link(&other)
        .build();
    let execution_sa =
        ServiceAccount::from_permission_profile("execution-sa".to_string(), &profile, |name| {
            alien_permissions::get_permission_set(name).cloned()
        })
        .expect("built-in permission sets resolve");
    let stack = Stack::new("sandbox-store".to_string()).permission("execution", profile);
    let stack = if with_key {
        stack.add(
            Key::new("store-key".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
    } else {
        stack
    };
    stack
        .add(
            AzureResourceGroup::new("default-resource-group".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            AzureContainerAppsEnvironment::new("default-container-apps-environment".to_string())
                .build(),
            ResourceLifecycle::Frozen,
        )
        .add(execution_sa, ResourceLifecycle::Frozen)
        .add(agents, ResourceLifecycle::Frozen)
        .add(other, ResourceLifecycle::Frozen)
        .add(worker, ResourceLifecycle::Live)
        .build()
}

fn body(module: &ModuleFiles) -> hcl::Body {
    let rendered = module
        .files
        .iter()
        .filter(|(name, _)| name.ends_with(".tf"))
        .map(|(_, contents)| contents.clone())
        .collect::<Vec<_>>()
        .join("\n");
    hcl::from_str(&rendered).expect("the module parses")
}

fn resources<'a>(body: &'a hcl::Body, kind: &'a str) -> impl Iterator<Item = &'a hcl::Block> {
    body.blocks().filter(move |block| {
        block.identifier() == "resource"
            && block.labels().first().map(|label| label.as_str()) == Some(kind)
    })
}

fn attribute(block: &hcl::Block, name: &str) -> Option<String> {
    block
        .body()
        .attributes()
        .find(|attribute| attribute.key() == name)
        .map(|attribute| attribute.expr().to_string())
}

/// `(label, scope, role guid, principal)` of every store grant.
fn store_grants(body: &hcl::Body) -> Vec<(String, String, String, String)> {
    resources(body, "azurerm_role_assignment")
        .filter_map(|block| {
            let role = attribute(block, "role_definition_id")?;
            let guid = [DELEGATOR, DATA_CONTRIBUTOR]
                .into_iter()
                .find(|guid| role.contains(guid))?;
            Some((
                block.labels()[1].as_str().to_string(),
                attribute(block, "scope").expect("scope"),
                guid.to_string(),
                attribute(block, "principal_id").expect("principal"),
            ))
        })
        .collect()
}

/// The `azapi_resource` blocks of the store: the account, then its container.
fn store_blocks<'a>(body: &'a hcl::Body) -> Vec<&'a hcl::Block> {
    resources(body, "azapi_resource")
        .filter(|block| block.labels()[1].as_str().contains("_store"))
        .collect()
}

fn store_account<'a>(body: &'a hcl::Body) -> &'a hcl::Block {
    store_blocks(body)
        .into_iter()
        .find(|block| block.labels()[1].as_str() == "agents_store")
        .expect("the store account")
}

/// An attribute with its whitespace and quotes removed, so `{ "a" = true }` reads `{a=true}`.
fn compact(block: &hcl::Block, name: &str) -> String {
    attribute(block, name)
        .unwrap_or_default()
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '"')
        .collect()
}

fn execute_on_agents() -> PermissionProfile {
    PermissionProfile::new().resource("agents", ["sandbox/execute"])
}

/// The declared sandbox gets an HNS account with no account key, no public access and TLS 1.2,
/// and one private container; its sibling gets nothing.
#[test]
fn a_declared_sandbox_gets_its_own_hns_store() {
    let module = render(
        &stack(execute_on_agents(), Some(declared())),
        TerraformTarget::Azure,
        StackSettings::default(),
    );
    let body = body(&module);

    let blocks = store_blocks(&body);
    assert_eq!(blocks.len(), 2, "one store account and container, for `agents` only");
    let account = store_account(&body);
    assert_eq!(
        compact(account, "type"),
        "Microsoft.Storage/storageAccounts@2023-05-01"
    );
    // Created through ARM alone: no azurerm storage resource reads it with an account key.
    assert_eq!(
        resources(&body, "azurerm_storage_account")
            .filter(|block| block.labels()[1].as_str() != "default_storage_account")
            .count(),
        0
    );
    assert_eq!(resources(&body, "azurerm_storage_container")
        .filter(|block| block.labels()[1].as_str().contains("_store"))
        .count(), 0);
    let properties = compact(account, "body");
    for setting in [
        "kind=StorageV2",
        "isHnsEnabled=true",
        "allowSharedKeyAccess=false",
        "defaultToOAuthAuthentication=true",
        "allowBlobPublicAccess=false",
        "supportsHttpsTrafficOnly=true",
        "minimumTlsVersion=TLS1_2",
    ] {
        assert!(properties.contains(setting), "store account lacks `{setting}`: {properties}");
    }
    let account_name = attribute(account, "name").expect("name");
    assert!(
        account_name.contains("sha256(\"${local.resource_prefix}:agents\")"),
        "{account_name}"
    );

    let container = blocks
        .iter()
        .find(|block| block.labels()[1].as_str() == "agents_store_keys")
        .expect("the store container");
    assert_eq!(
        compact(container, "parent_id"),
        "${azapi_resource.agents_store.id}/blobServices/default"
    );
    assert_eq!(compact(container, "name"), "keys");
    assert!(compact(container, "body").contains("publicAccess=None"));
    assert_terraform_valid(&module, "azure sandbox store");
}

/// The identity that creates the group's sandboxes is the one that mints a key's SAS: Delegator
/// on the account (a user delegation key is an account action) and Data Contributor on the
/// container alone. Nothing at the resource group, nothing to anyone else.
#[test]
fn the_creating_identity_gets_delegator_on_the_account_and_data_on_the_container() {
    let module = render(
        &stack(execute_on_agents(), Some(declared())),
        TerraformTarget::Azure,
        StackSettings::default(),
    );
    let mut grants = store_grants(&body(&module));
    grants.sort();
    let principal = "azurerm_user_assigned_identity.execution_sa.principal_id";
    assert_eq!(
        grants,
        [
            (
                "agents_store-execution_sa-data".to_string(),
                "azapi_resource.agents_store_keys.id".to_string(),
                DATA_CONTRIBUTOR.to_string(),
                principal.to_string(),
            ),
            (
                "agents_store-execution_sa-delegator".to_string(),
                "azapi_resource.agents_store.id".to_string(),
                DELEGATOR.to_string(),
                principal.to_string(),
            ),
        ]
    );
}

/// `sandbox/execute` and `sandbox/remote-execute` both create sandboxes; one identity holding
/// both gets each store grant once, since Azure refuses a duplicate assignment. A `"*"` holder of
/// `sandbox/execute` is a creator too.
#[test]
fn each_creating_identity_gets_each_store_grant_once() {
    for profile in [
        PermissionProfile::new().resource("agents", ["sandbox/execute", "sandbox/remote-execute"]),
        PermissionProfile::new().global(["sandbox/execute"]),
    ] {
        let module = render(
            &stack(profile, Some(declared())),
            TerraformTarget::Azure,
            StackSettings::default(),
        );
        let grants = store_grants(&body(&module));
        assert_eq!(grants.len(), 2, "{grants:?}");
    }
}

/// An identity that can manage but not create sandboxes gets nothing on the store.
#[test]
fn an_identity_that_creates_no_sandbox_gets_no_store_grant() {
    let module = render(
        &stack(
            PermissionProfile::new().global(["sandbox/management"]),
            Some(declared()),
        ),
        TerraformTarget::Azure,
        StackSettings::default(),
    );
    let body = body(&module);
    assert_eq!(store_blocks(&body).len(), 2);
    assert!(store_grants(&body).is_empty());
}

/// A stack that declares no persistent storage contains nothing of the store. That its bytes are
/// unchanged is the Azure sandbox snapshots' job: they pass with no snapshot touched.
#[test]
fn an_undeclared_sandbox_renders_no_store() {
    let module = render(
        &stack(execute_on_agents(), None),
        TerraformTarget::Azure,
        StackSettings::default(),
    );
    let body = body(&module);
    assert!(store_blocks(&body).is_empty());
    assert!(store_grants(&body).is_empty());
    for (name, contents) in &module.files {
        assert!(
            !contents.contains("persistentStorage") && !contents.contains("_store"),
            "{name} mentions the store"
        );
    }
}

/// A declared `encryptionKey` encrypts the store with that Key Vault key: the account gets an
/// identity of its own, that identity alone holds the crypto role on the key, and the account is
/// created only after the grant stands.
#[test]
fn a_declared_encryption_key_encrypts_the_store() {
    let mut storage = declared();
    storage.encryption_key = Some(ResourceRef::new(Key::RESOURCE_TYPE, "store-key"));
    let module = render(
        &stack_with_key(execute_on_agents(), Some(storage), true),
        TerraformTarget::Azure,
        StackSettings::default(),
    );
    let body = body(&module);
    let account = store_account(&body);
    let properties = compact(account, "body");
    for setting in [
        "keySource=Microsoft.Keyvault",
        "userAssignedIdentity=azurerm_user_assigned_identity.agents_store_encryption.id",
        "keyname=azurerm_key_vault_key.store_key.name",
        "azurerm_key_vault_key.store_key.versionless_id",
    ] {
        assert!(properties.contains(setting), "store encryption lacks `{setting}`: {properties}");
    }
    let identity = account
        .body()
        .blocks()
        .find(|block| block.identifier() == "identity")
        .expect("the store account has the encryption identity");
    assert_eq!(
        compact(identity, "identity_ids"),
        "[azurerm_user_assigned_identity.agents_store_encryption.id]"
    );
    let depends_on = attribute(account, "depends_on").expect("depends_on");
    assert!(
        depends_on.contains("azurerm_role_assignment.agents_store_encryption_key"),
        "{depends_on}"
    );
    let grant = resources(&body, "azurerm_role_assignment")
        .find(|block| block.labels()[1].as_str() == "agents_store_encryption_key")
        .expect("the key grant");
    assert_eq!(
        attribute(grant, "scope").as_deref(),
        Some("azurerm_key_vault_key.store_key.resource_versionless_id")
    );
    assert_eq!(
        attribute(grant, "principal_id").as_deref(),
        Some("azurerm_user_assigned_identity.agents_store_encryption.principal_id")
    );
    assert_terraform_valid(&module, "azure sandbox store with an encryption key");
}

/// Without an `encryptionKey` the store has no identity and Azure manages its key.
#[test]
fn an_undeclared_encryption_key_leaves_the_store_on_microsoft_managed_keys() {
    let module = render(
        &stack(execute_on_agents(), Some(declared())),
        TerraformTarget::Azure,
        StackSettings::default(),
    );
    let body = body(&module);
    let account = store_account(&body);
    assert!(account.body().blocks().next().is_none());
    assert!(!compact(account, "body").contains("encryption"));
    assert_eq!(
        resources(&body, "azurerm_user_assigned_identity")
            .filter(|b| b.labels()[1].as_str().starts_with("agents_store"))
            .count(),
        0
    );
}

async fn internet_module(storage: Option<SandboxPersistentStorage>) -> ModuleFiles {
    let mut sandbox = Sandbox::new("agents".to_string())
        .code(SandboxCode::Image {
            image: "registry.example/agents:1".to_string(),
            bundled_from: None,
        })
        .egress(SandboxEgress::Internet)
        .lifecycle(SandboxLifecyclePolicy {
            max_lifetime_seconds: None,
            idle_pause_seconds: None,
        })
        .build();
    sandbox.persistent_storage = storage;
    let stack = Stack::new("azint".to_string())
        .add(sandbox, ResourceLifecycle::Frozen)
        .build();
    let prepared =
        super::helpers::prepare_for_target_unchecked(stack, TerraformTarget::Azure).await;
    render(&prepared, TerraformTarget::Azure, StackSettings::default())
}

/// `(name, priority, access, protocol, destination, ports)` of every NSG rule on the declared
/// sandbox's subnet.
fn sandbox_nsg_rules(body: &hcl::Body) -> Vec<[String; 6]> {
    resources(body, "azurerm_network_security_rule")
        .filter(|block| block.labels()[1].as_str().contains("_sandbox_agents_"))
        .map(|block| {
            [
                "name",
                "priority",
                "access",
                "protocol",
                "destination_address_prefix",
                "destination_port_ranges",
            ]
            .map(|key| {
                attribute(block, key)
                    .unwrap_or_default()
                    .split_whitespace()
                    .collect()
            })
        })
        .collect()
}

/// Under `internet` the subnet of a sandbox with persistent storage, and only it, reaches Azure
/// Storage in its region on 443, with its own priority ahead of the Internet and catch-all
/// denies that would cover it; the rendered module validates against the real provider.
#[tokio::test]
async fn an_internet_sandbox_with_storage_reaches_regional_storage_on_443_only() {
    let module = internet_module(Some(declared())).await;
    let rules = sandbox_nsg_rules(&body(&module));
    let storage: Vec<_> = rules
        .iter()
        .filter(|rule| rule[0] == "\"storage\"")
        .collect();
    assert_eq!(
        storage,
        [&[
            "\"storage\"".to_string(),
            "104".to_string(),
            "\"Allow\"".to_string(),
            "\"Tcp\"".to_string(),
            "\"Storage.${var.azure_location}\"".to_string(),
            "[\"443\"]".to_string(),
        ]]
    );
    let mut priorities: Vec<&String> = rules.iter().map(|rule| &rule[1]).collect();
    priorities.sort();
    let before = priorities.len();
    priorities.dedup();
    assert_eq!(
        before,
        priorities.len(),
        "two rules share a priority: {rules:?}"
    );
    assert!(
        rules
            .iter()
            .filter(|rule| rule[0] == "\"internet\"" || rule[0] == "\"other\"")
            .all(|rule| rule[1].parse::<i64>().unwrap() > 104),
        "{rules:?}"
    );
    assert_terraform_valid(&module, "azure internet sandbox with a store");

    let plain = internet_module(None).await;
    assert!(sandbox_nsg_rules(&body(&plain))
        .iter()
        .all(|rule| rule[0] != "\"storage\""));
}

/// The runtime controller publishes the binding the deployment's own workloads read, and it learns
/// the store only from what setup registers: the sandbox's import data carries the binding's
/// `persistentStorage`, and an undeclared sandbox's import data is unchanged.
#[test]
fn the_registration_hands_the_controller_the_store() {
    let module = render(
        &stack(execute_on_agents(), Some(declared())),
        TerraformTarget::Azure,
        StackSettings::default(),
    );
    let rendered = module
        .files
        .iter()
        .filter(|(name, _)| name.ends_with(".tf"))
        .map(|(_, contents)| contents.clone())
        .collect::<Vec<_>>()
        .join("\n");
    let import_of = |id: &str| -> String {
        // The formatter aligns `=` across an object, so the gap before it varies.
        let quoted = format!("\"{id}\"");
        let at = rendered
            .match_indices(&quoted)
            .map(|(at, _)| at)
            .find(|&at| {
                let before = rendered[..at].trim_end();
                before.ends_with('=')
                    && before[..before.len() - 1].trim_end().ends_with("id")
                    && !before[..before.len() - 1].trim_end().ends_with("_id")
            })
            .unwrap_or_else(|| panic!("no registration for {id}"));
        let rest = &rendered[at..];
        let start = rest.find("importData = {").expect("importData");
        let mut depth = 0;
        for (offset, ch) in rest[start..].char_indices() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return rest[start..=start + offset].to_string();
                    }
                }
                _ => {}
            }
        }
        panic!("unbalanced importData for {id}")
    };
    let agents = import_of("agents");
    assert!(agents.contains("persistentStorage"), "{agents}");
    assert!(
        agents.contains("azapi_resource.agents_store.name"),
        "{agents}"
    );
    assert!(agents.contains("\"keys\""), "{agents}");
    let other = import_of("other");
    assert!(!other.contains("persistentStorage"), "{other}");
}
