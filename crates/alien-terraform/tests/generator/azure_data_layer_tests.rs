//! Azure data-layer scenarios — storage / kv / queue / vault / ai.
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
    Ai, AzureResourceGroup, AzureServiceBusNamespace, AzureStorageAccount, Key, Kv, LifecycleRule,
    PermissionProfile, Queue, RemoteBindings, RemoteStackManagement, ResourceLifecycle,
    ResourceRef, Sandbox, SandboxCode, SandboxEgress, SandboxSessionPolicy, ServiceAccount, Stack,
    StackSettings, Storage, Vault,
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
fn azure_key_package_is_valid_and_retained() {
    let mut stack = Stack::new("enterprise-key".to_string())
        .add(resource_group(), ResourceLifecycle::Frozen)
        .add_with_remote_access(
            Key::new("customer-key".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            RemoteBindings::new("access".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    stack
        .resources
        .get_mut("customer-key")
        .unwrap()
        .dependencies = vec![
        ResourceRef::new(AzureResourceGroup::RESOURCE_TYPE, "default-resource-group"),
        ResourceRef::new(RemoteBindings::RESOURCE_TYPE, "access"),
    ];

    let module = render(&stack, TerraformTarget::Azure, StackSettings::default());
    let rendered = module
        .files
        .values()
        .cloned()
        .collect::<Vec<_>>()
        .join("\n");

    assert!(rendered.contains("azurerm_key_vault_key"));
    assert!(rendered.matches("prevent_destroy = true").count() >= 2);
    assert!(rendered.contains("azurerm_role_assignment\" \"customer_key_installer_key_admin"));
    assert!(rendered.contains("14b46e9e-c2b7-41b4-b07b-48a6ebf60603"));
    assert!(rendered.contains("time_sleep\" \"customer_key_installer_rbac"));
    assert!(rendered.contains("create_duration = \"60s\""));
    assert!(rendered.contains("Microsoft.KeyVault/vaults/keys/encrypt/action"));
    assert!(rendered.contains("Microsoft.KeyVault/vaults/keys/decrypt/action"));
    let detach = module
        .get("detach-retained-keys.sh")
        .expect("retained Key detach operation");
    assert!(detach.contains("azurerm_key_vault_key.customer_key"));
    assert!(detach.contains("azurerm_key_vault.customer_key"));
    assert_terraform_valid(&module, "azure_key_package");
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
fn azure_storage_account_uses_customer_managed_key() {
    let stack = Stack::new("encrypted-storage".to_string())
        .add(resource_group(), ResourceLifecycle::Frozen)
        .add(
            Key::new("customer-key".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(storage_account(), ResourceLifecycle::Frozen)
        .add(
            Storage::new("data".to_string())
                .encryption_key(ResourceRef::new(Key::RESOURCE_TYPE, "customer-key"))
                .build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Azure, StackSettings::default());
    let rendered = module
        .files
        .values()
        .cloned()
        .collect::<Vec<_>>()
        .join("\n");

    assert!(rendered.contains("customer_managed_key"));
    assert!(rendered.contains("key_vault_key_id"));
    assert!(rendered.contains(".versionless_id"));
    assert!(rendered.contains("scope"));
    assert!(rendered.contains(".resource_versionless_id"));
    assert!(rendered.contains("Key Vault Crypto Service Encryption User"));
    assert!(rendered.contains("\"unwrapKey\""));
    assert!(rendered.contains("\"wrapKey\""));
    assert_terraform_valid(&module, "azure_encrypted_storage");
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
fn azure_byo_bucket_is_acyclic_and_valid() {
    let mut stack = Stack::new("acme-remote-storage".to_string())
        .add(resource_group(), ResourceLifecycle::Frozen)
        .add(storage_account(), ResourceLifecycle::Frozen)
        .add_with_remote_access(
            Storage::new("files".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            RemoteBindings::new("access".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    stack.resources.get_mut("files").unwrap().dependencies =
        vec![ResourceRef::new(RemoteBindings::RESOURCE_TYPE, "access")];

    let module = render(&stack, TerraformTarget::Azure, StackSettings::default());
    snapshot_module("azure_byo_bucket", &module);
    assert_terraform_valid(&module, "azure_remote_storage_management_dependencies");
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

#[test]
fn azure_ai_renders_cognitive_account() {
    // Azure AI provisions an azurerm_cognitive_account (kind=AIServices, sku=S0).
    let stack = Stack::new("acme-ai".to_string())
        .add(resource_group(), ResourceLifecycle::Frozen)
        .add(
            Ai::new("llm".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Azure, StackSettings::default());
    snapshot_module("azure_ai_minimal", &module);
    assert_terraform_valid(&module, "azure_ai_minimal");

    let rendered = module
        .iter()
        .map(|(_, contents)| contents)
        .collect::<String>();
    assert!(
        rendered.contains("azurerm_cognitive_account"),
        "must emit azurerm_cognitive_account"
    );
    assert!(rendered.contains("AIServices"), "kind must be AIServices");
    assert!(rendered.contains("S0"), "sku_name must be S0");

    assert!(
        rendered.contains("azurerm_cognitive_deployment"),
        "must emit a model deployment"
    );
    assert!(
        rendered.contains("gpt-4.1"),
        "must deploy the curated gpt-4.1 model"
    );
    assert!(
        rendered.contains("cognitive_account_id"),
        "deployment must reference the account via cognitive_account_id"
    );
    assert!(
        rendered.contains("GlobalStandard"),
        "deployment sku must be GlobalStandard"
    );

    // Import metadata must carry accountName, endpoint, resourceGroup, location.
    // The import ref appears in locals.tf.
    let locals = module.get("locals.tf").expect("locals.tf should render");
    assert!(
        locals.contains("accountName"),
        "import ref must carry accountName"
    );
    assert!(
        locals.contains("endpoint"),
        "import ref must carry endpoint"
    );
    assert!(
        locals.contains("resourceGroup"),
        "import ref must carry resourceGroup"
    );
    assert!(
        locals.contains("location"),
        "import ref must carry location"
    );
}

#[test]
fn azure_ai_invoke_permissions_emit_cognitive_services_user_role() {
    // When a permission profile references ai/invoke, the AI emitter emits a
    // Cognitive Services User role assignment scoped to the cognitive
    // account, bound to the workload service account.
    let stack = Stack::new("acme-ai".to_string())
        .permissions(alien_core::PermissionsConfig::new().with_profile(
            "execution",
            PermissionProfile::new().resource("llm", ["ai/invoke"]),
        ))
        .add(resource_group(), ResourceLifecycle::Frozen)
        .add(
            ServiceAccount::new("execution-sa".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Ai::new("llm".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Azure, StackSettings::default());
    let rendered = module
        .iter()
        .map(|(_, contents)| contents)
        .collect::<String>();

    // The predefined role ID for "Cognitive Services User" must appear.
    assert!(
        rendered.contains("a97b65f3-24c7-4388-baec-2e87135dc908"),
        "Cognitive Services User role ID must appear"
    );
    assert_terraform_valid(&module, "azure_ai_invoke_permissions");
}

#[test]
fn azure_remote_ai_invoke_permissions_attach_to_access_identity() {
    let stack = Stack::new("remote-ai".to_string())
        .add(resource_group(), ResourceLifecycle::Frozen)
        .add_with_remote_access(
            Ai::new("models".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            RemoteBindings::new("access".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Azure, StackSettings::default());
    let rendered = module
        .iter()
        .map(|(_, contents)| contents)
        .collect::<String>();

    assert!(rendered.contains("a97b65f3-24c7-4388-baec-2e87135dc908"));
    assert!(rendered.contains("azurerm_user_assigned_identity.access.principal_id"));
    assert_terraform_valid(&module, "azure_remote_ai_invoke_permissions");
}

#[test]
fn azure_remote_ai_setup_does_not_request_application_vnet_access() {
    let stack = Stack::new("remote-ai-setup".to_string())
        .add(resource_group(), ResourceLifecycle::Frozen)
        .add_with_remote_access(
            Ai::new("models".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            RemoteBindings::new("access".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            RemoteStackManagement::new("management".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let settings = StackSettings {
        network: Some(alien_core::NetworkSettings::ByoVnetAzure {
            vnet_resource_id:
                "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/shared/providers/Microsoft.Network/virtualNetworks/shared-vnet"
                    .to_string(),
            public_subnet_name: "public".to_string(),
            private_subnet_name: "private".to_string(),
            application_gateway_subnet_name: None,
            private_endpoint_subnet_name: None,
        }),
        ..StackSettings::default()
    };

    let module = render(&stack, TerraformTarget::Azure, settings);
    let management = module
        .get("management.tf")
        .expect("remote AI setup management artifact");
    assert!(
        !management.contains("existing_vnet_reader"),
        "a bindings-only setup must not grant its management identity access to the application VNet"
    );
    assert_terraform_valid(&module, "azure_remote_ai_setup_without_vnet_reader");
}


/// The remote sandbox grant reaches one group and nothing wider, and the group exists to be
/// granted on.
///
/// Both halves matter and neither is provable alone. Azure refuses a role assignment scoped to a
/// resource that does not exist, so a grant is only placeable because setup creates the group in
/// the same module — which is why the scope is asserted as a *reference* to that resource rather
/// than as a rendered path: a literal would render identically today and silently lose the
/// ordering the reference creates.
///
/// The width is the security half. `Container Apps SandboxGroup Data Owner` covers
/// `sandboxGroups/*` on whatever it is scoped to, so a resource-group or subscription scope would
/// hand a remote caller every sibling sandbox in the deployment.
#[test]
fn azure_remote_sandbox_grants_the_access_identity_its_own_group_and_nothing_wider() {
    let sandbox = Sandbox::new("agents".to_string())
        .code(SandboxCode::Image {
            image: "ubuntu".to_string(),
        })
        .egress(SandboxEgress::Allow)
        .session(SandboxSessionPolicy {
            max_lifetime_seconds: None,
            idle_suspend_seconds: None,
        })
        .build();
    let stack = Stack::new("byo-sandbox".to_string())
        .add(resource_group(), ResourceLifecycle::Frozen)
        .add_with_remote_access(sandbox, ResourceLifecycle::Frozen)
        .add(
            RemoteBindings::new("access".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();

    let module = render(&stack, TerraformTarget::Azure, StackSettings::default());
    let rendered = module
        .files
        .values()
        .cloned()
        .collect::<Vec<_>>()
        .join("\n");

    // The scope PERMISSIONS.md documents has to name the group the module creates. A security
    // team approves the grant from that document and cannot see the traversal the assignment
    // actually uses, so a scope that renders to a different name -- or to an unsubstituted
    // template token -- shows them a boundary the deployment does not have.
    let group_name = module
        .files
        .get("agents.tf")
        .expect("the sandbox group renders into its own file")
        .lines()
        // HCL pads `=` to align an attribute with its siblings, so match on the token.
        .find_map(|line| line.split_whitespace().collect::<Vec<_>>().split_first().and_then(
            |(first, rest)| (*first == "name" && rest.first() == Some(&"=")).then(|| rest[1..].join(" ")),
        ))
        .expect("the group carries a name");
    let documented_scope = module
        .files
        .get("PERMISSIONS.md")
        .expect("a remote binding publishes a permissions document")
        .lines()
        .find(|line| line.trim_start().starts_with("Scope:"))
        .expect("the document states the grant's scope")
        .to_string();
    assert!(
        documented_scope.ends_with(&format!("/Microsoft.App/sandboxGroups/${{{group_name}}}`")),
        "the documented scope must end at the created group.\n  documented: {documented_scope}\n  \
         group name: {group_name}"
    );
    // Terraform's own interpolations survive into the document by design; a permission-set token
    // does not. One left behind renders a scope that resolves to nothing, and the document is the
    // only place a reader would ever see it.
    let permissions_md = module
        .files
        .get("PERMISSIONS.md")
        .expect("a remote binding publishes a permissions document");
    for token in [
        "${stackPrefix}",
        "${resourceName}",
        "${subscriptionId}",
        "${resourceGroup}",
        "${projectName}",
        "${awsRegion}",
        "${awsAccountId}",
        "${storageAccountName}",
    ] {
        assert!(
            !permissions_md.contains(token),
            "{token} reached the approver's document unsubstituted:\n{permissions_md}"
        );
    }

    let assignment = rendered
        .split(r#"resource "azurerm_role_assignment" "agents_access_0""#)
        .nth(1)
        .expect("the remote grant attaches to the Remote Bindings identity")
        .split("\nresource ")
        .next()
        .expect("block ends");
    // HCL pads `=` to align an attribute with its siblings, so the column an assertion would
    // match on moves whenever a neighbouring attribute is added or renamed.
    let assignment = assignment
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let assignment = assignment.as_str();

    // The scope attribute itself, not merely a mention of the group somewhere in the block: the
    // resource group legitimately appears in `depends_on`, so a substring search over the whole
    // assignment would pass on a scope that reached the entire group.
    let scope = assignment
        .split("scope = ")
        .nth(1)
        .expect("the assignment carries a scope")
        .split(' ')
        .next()
        .expect("the scope is one token");

    // A reference, not a path. This is what orders the assignment after the group Azure refuses
    // to grant on before it exists.
    assert_eq!(
        scope, "azapi_resource.agents.id",
        "the grant must be scoped to the created group by reference, and nothing wider"
    );
    // Extracted like the scope rather than searched for: a `contains` would also pass on an
    // identity whose name merely starts with this one.
    let principal_id = assignment
        .split("principal_id = ")
        .nth(1)
        .expect("the assignment names a principal")
        .split(' ')
        .next()
        .expect("the principal is one token");
    assert_eq!(
        principal_id, "azurerm_user_assigned_identity.access.principal_id",
        "the grant belongs to the Remote Bindings identity, not the deployment's own"
    );

    // `terraform init` is what proves the azapi provider block was emitted: an `azapi_resource`
    // without one fails at init, not at plan.
    assert_terraform_valid(&module, "azure remote sandbox");
    // The module as a whole, so the artifact an approver reads is the thing CI compares — the
    // group's body and tags, the provider block, and the rendered PERMISSIONS.md.
    snapshot_module("azure_remote_sandbox", &module);
}


/// A remote sandbox renders on its own, with no other resource declared.
///
/// The shape worth checking is a bindings-only stack: the sandbox group's `parent_id` names a
/// resource group, and this pins that it resolves to the deployer-supplied
/// `var.azure_resource_group_name` rather than to a resource the module would have had to
/// declare. `terraform validate` is as far as this reaches — it does not prove apply.
/// An AKS target is `Platform::Azure` but skips sandbox emission, so a note keyed off the platform
/// would tell that installer to register a provider for a resource their package does not contain.
#[test]
fn an_aks_package_is_not_told_about_a_sandbox_group_it_does_not_get() {
    let sandbox = Sandbox::new("agents".to_string())
        .code(SandboxCode::Image {
            image: "ubuntu".to_string(),
        })
        .egress(SandboxEgress::Allow)
        .session(SandboxSessionPolicy {
            max_lifetime_seconds: None,
            idle_suspend_seconds: None,
        })
        .build();
    let stack = Stack::new("byo-sandbox".to_string())
        .add_with_remote_access(sandbox, ResourceLifecycle::Frozen)
        .add(
            RemoteBindings::new("access".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();

    for (target, expects_group) in [
        (TerraformTarget::Aks, false),
        (TerraformTarget::Azure, true),
    ] {
        let module = render(&stack, target, StackSettings::default());
        let emitted_group = module
            .iter()
            .any(|(_, contents)| contents.contains("azapi_resource\" \"agents\""));
        let readme = module.get("README.md").expect("every package has a README");
        assert_eq!(
            emitted_group, expects_group,
            "{target:?} emits the sandbox group"
        );
        assert_eq!(
            readme.contains("## The sandbox group"),
            expects_group,
            "the README documents the group exactly when the package contains one:\n{readme}"
        );
    }
}

#[test]
fn an_azure_remote_sandbox_renders_without_any_other_resource_declared() {
    let sandbox = Sandbox::new("agents".to_string())
        .code(SandboxCode::Image {
            image: "ubuntu".to_string(),
        })
        .egress(SandboxEgress::Allow)
        .session(SandboxSessionPolicy {
            max_lifetime_seconds: None,
            idle_suspend_seconds: None,
        })
        .build();
    let stack = Stack::new("byo-sandbox".to_string())
        .add_with_remote_access(sandbox, ResourceLifecycle::Frozen)
        .add(
            RemoteBindings::new("access".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();

    let module = render(&stack, TerraformTarget::Azure, StackSettings::default());

    // The claim this shape is here to pin: with no other resource declared there is no resource
    // group of Alien's own to parent to, so the group must hang off the one the deployer names.
    // A reference to a resource this stack does not create would fail `terraform validate` below,
    // but a *wrong variable* would not — so the attribute is read rather than searched for.
    let parent_id = module
        .files
        .get("agents.tf")
        .expect("the sandbox group renders into its own file")
        .lines()
        .find_map(|line| {
            let normalized = line.split_whitespace().collect::<Vec<_>>();
            match normalized.split_first() {
                Some((first, rest)) if *first == "parent_id" && rest.first() == Some(&"=") => {
                    Some(rest[1..].join(" "))
                }
                _ => None,
            }
        })
        .expect("the group carries a parent_id");
    assert_eq!(
        parent_id,
        "\"/subscriptions/${var.azure_subscription_id}/resourceGroups/${var.azure_resource_group_name}\""
    );

    assert_terraform_valid(&module, "azure remote sandbox alone");
}
