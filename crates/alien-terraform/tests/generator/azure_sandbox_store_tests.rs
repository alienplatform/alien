//! The store an Azure sandbox's persistent storage is replicated to.
//!
//! Rendered only when declared: an account with a hierarchical namespace (a directory SAS needs
//! one), no account key, no public access, one private container, and two grants for each identity
//! that creates the group's sandboxes: Storage Blob Delegator on the account and Storage Blob Data
//! Contributor on the container. A sandbox without the declaration renders byte for byte as before.

use super::helpers::{assert_terraform_valid, render};
use alien_core::{
    AzureContainerAppsEnvironment, AzureResourceGroup, PermissionProfile, ResourceLifecycle,
    ResourceRef, Sandbox, SandboxCode, SandboxEgress, SandboxLifecyclePolicy,
    SandboxPersistentStorage, ServiceAccount, Stack, StackSettings, Storage, Worker, WorkerCode,
};
use alien_terraform::{ModuleFiles, TerraformTarget};

const DELEGATOR: &str = "db58b8e5-c6ad-4a2a-8342-4190687cbf4a";
const DATA_CONTRIBUTOR: &str = "ba92f5b4-2d11-453d-a403-e96b0029c9fe";

fn declared() -> SandboxPersistentStorage {
    SandboxPersistentStorage {
        path: "/workspace".to_string(),
        storage: ResourceRef::new(Storage::RESOURCE_TYPE, "files"),
        exclude: vec!["tmp/".to_string()],
        max_size: Some("8Gi".to_string()),
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
    Stack::new("sandbox-store".to_string())
        .permission("execution", profile)
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

    let accounts: Vec<_> = resources(&body, "azurerm_storage_account")
        .filter(|block| block.labels()[1].as_str() != "default_storage_account")
        .collect();
    assert_eq!(accounts.len(), 1, "one store, for `agents` only");
    let account = accounts[0];
    assert_eq!(account.labels()[1].as_str(), "agents_store");
    for (name, value) in [
        ("is_hns_enabled", "true"),
        ("shared_access_key_enabled", "false"),
        ("default_to_oauth_authentication", "true"),
        ("allow_nested_items_to_be_public", "false"),
        ("https_traffic_only_enabled", "true"),
        ("min_tls_version", "\"TLS1_2\""),
        ("account_kind", "\"StorageV2\""),
    ] {
        assert_eq!(
            attribute(account, name).as_deref(),
            Some(value),
            "store account `{name}`"
        );
    }
    let account_name = attribute(account, "name").expect("name");
    assert!(
        account_name.contains("sha256(\"${local.resource_prefix}:agents\")"),
        "{account_name}"
    );

    let containers: Vec<_> = resources(&body, "azurerm_storage_container").collect();
    assert_eq!(containers.len(), 1);
    assert_eq!(
        attribute(containers[0], "storage_account_id").as_deref(),
        Some("azurerm_storage_account.agents_store.id")
    );
    assert_eq!(
        attribute(containers[0], "container_access_type").as_deref(),
        Some("\"private\"")
    );
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
                "azurerm_storage_container.agents_store.resource_manager_id".to_string(),
                DATA_CONTRIBUTOR.to_string(),
                principal.to_string(),
            ),
            (
                "agents_store-execution_sa-delegator".to_string(),
                "azurerm_storage_account.agents_store.id".to_string(),
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
    assert_eq!(resources(&body, "azurerm_storage_account").count(), 1);
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
    assert_eq!(resources(&body, "azurerm_storage_container").count(), 0);
    assert!(store_grants(&body).is_empty());
    for (name, contents) in &module.files {
        assert!(
            !contents.contains("persistentStorage") && !contents.contains("_store"),
            "{name} mentions the store"
        );
    }
}
