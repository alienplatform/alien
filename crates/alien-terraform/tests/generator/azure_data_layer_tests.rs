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

use super::helpers::{assert_terraform_valid, linter_files, render, snapshot_module, test_utils};
use alien_core::{
    Ai, AzureContainerAppsEnvironment, AzureResourceGroup, AzureServiceBusNamespace,
    AzureStorageAccount, Key, Kv, LifecycleRule, PermissionProfile, PermissionSetReference, Queue,
    RemoteBindings, RemoteStackManagement, ResourceLifecycle, ResourceRef, Sandbox, SandboxCode,
    SandboxEgress, SandboxLifecyclePolicy, ServiceAccount, Stack, StackSettings, Storage, Vault,
    Worker, WorkerCode,
};
use alien_permissions::{BindingTarget, PermissionContext};
use alien_terraform::{
    emitters::azure::helpers::emit_role_definition_and_assignments_for_target,
    generate_terraform_module, TerraformOptions, TerraformTarget, TfFragment, TfRegistry,
};
use hcl::Expression;
use std::collections::HashSet;

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
/// granted on. The scope is asserted as a *reference*, not a rendered path — a literal would
/// render identically today but lose the ordering that makes setup create the group first.
/// `Container Apps SandboxGroup Data Owner` covers `sandboxGroups/*` on whatever it's scoped to,
/// so a resource-group or subscription scope would hand a remote caller every sibling sandbox.
#[test]
fn azure_remote_sandbox_grants_the_access_identity_its_own_group_and_nothing_wider() {
    let sandbox = Sandbox::new("agents".to_string())
        .code(SandboxCode::Image {
            image: "ubuntu".to_string(),
        })
        .egress(SandboxEgress::Allow)
        .lifecycle(SandboxLifecyclePolicy {
            max_lifetime_seconds: None,
            idle_pause_seconds: None,
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

    // The scope PERMISSIONS.md documents must name the group the module creates: a security team
    // approves the grant from that document and cannot see the traversal the assignment actually
    // uses, so a name that differs — or an unsubstituted template token — misdocuments the boundary.
    let group_name = module
        .files
        .get("agents.tf")
        .expect("the sandbox group renders into its own file")
        .lines()
        // HCL pads `=` to align an attribute with its siblings, so match on the token.
        .find_map(|line| {
            line.split_whitespace()
                .collect::<Vec<_>>()
                .split_first()
                .and_then(|(first, rest)| {
                    (*first == "name" && rest.first() == Some(&"=")).then(|| rest[1..].join(" "))
                })
        })
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
    let assignment = assignment.split_whitespace().collect::<Vec<_>>().join(" ");
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

    // A reference, not a path — see the test doc for why the ordering matters.
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

/// The manager's disk-image grant lands on the one group and on the management identity, through
/// a role that carries the disk-image data actions and nothing that reaches a sandbox. A grant
/// that rendered at resource-group scope would let the manager replace a sibling's image.
#[test]
fn azure_sandbox_image_grant_reaches_the_manager_on_its_own_group_only() {
    let sandbox = Sandbox::new("agents".to_string())
        .code(SandboxCode::Image {
            image: "docker.io/library/python:3.14-slim".to_string(),
        })
        .egress(SandboxEgress::Allow)
        .lifecycle(SandboxLifecyclePolicy {
            max_lifetime_seconds: None,
            idle_pause_seconds: None,
        })
        .build();
    let stack = Stack::new("byo-sandbox".to_string())
        .management(alien_core::ManagementPermissions::extend(
            PermissionProfile::new().resource("agents", ["sandbox/images"]),
        ))
        .add(resource_group(), ResourceLifecycle::Frozen)
        .add(sandbox, ResourceLifecycle::Frozen)
        .add(
            RemoteStackManagement::new("management".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();

    let module = render(&stack, TerraformTarget::Azure, StackSettings::default());
    let rendered = module
        .files
        .iter()
        .filter(|(name, _)| name.ends_with(".tf"))
        .map(|(_, contents)| contents.clone())
        .collect::<Vec<_>>()
        .join("\n");
    let body: hcl::Body = hcl::from_str(&rendered).expect("the module parses");

    let assignments: Vec<&hcl::Block> = body
        .blocks()
        .filter(|block| {
            block.identifier() == "resource"
                && block.labels().first().map(|label| label.as_str())
                    == Some("azurerm_role_assignment")
                && block
                    .labels()
                    .get(1)
                    .is_some_and(|label| label.as_str().starts_with("agents_images_"))
        })
        .collect();
    assert_eq!(assignments.len(), 1, "{rendered}");
    let attribute = |name: &str| {
        assignments[0]
            .body()
            .attributes()
            .find(|attribute| attribute.key() == name)
            .map(|attribute| attribute.expr().to_string())
            .unwrap_or_default()
    };
    assert_eq!(attribute("scope"), "azapi_resource.agents.id");
    assert_eq!(
        attribute("principal_id"),
        "azurerm_user_assigned_identity.management.principal_id"
    );

    let role_label = attribute("role_definition_id")
        .strip_prefix("azurerm_role_definition.")
        .and_then(|rest| rest.strip_suffix(".role_definition_resource_id"))
        .map(str::to_string)
        .expect("the assignment names a rendered custom role");
    let role = body
        .blocks()
        .find(|block| {
            block.identifier() == "resource"
                && block.labels().get(1).map(|label| label.as_str()) == Some(role_label.as_str())
        })
        .unwrap_or_else(|| panic!("role definition '{role_label}' is not rendered: {rendered}"));
    let role_text = hcl::to_string(role).expect("the role renders");
    for action in ["diskimages/read", "diskimages/write", "diskimages/delete"] {
        assert!(role_text.contains(action), "{action}: {role_text}");
    }
    assert!(!role_text.contains("sandboxes/"), "{role_text}");

    snapshot_module("azure_sandbox_image_grant", &module);
    assert_terraform_valid(&module, "azure sandbox image grant");
}

const SANDBOX_DATA_PLANE_ROLE_ID: &str = "c24cf47c-5077-412d-a19c-45202126392c";

fn frozen_sandbox(id: &str) -> Sandbox {
    Sandbox::new(id.to_string())
        .code(SandboxCode::Image {
            image: "ubuntu".to_string(),
        })
        .egress(SandboxEgress::Allow)
        .lifecycle(SandboxLifecyclePolicy {
            max_lifetime_seconds: None,
            idle_pause_seconds: None,
        })
        .build()
}

/// The stack a customer renders: Frozen sandboxes, a Live Worker linked to each, and the
/// `execution` identity carrying the profile's stack-wide sets the way the preflight builds it, so
/// the module shows every grant that identity ends up with.
fn workload_sandbox_stack(profile: PermissionProfile, sandbox_ids: &[&str]) -> Stack {
    let sandboxes: Vec<Sandbox> = sandbox_ids.iter().map(|id| frozen_sandbox(id)).collect();
    let mut worker = Worker::new("api".to_string())
        .code(WorkerCode::Image {
            image: "acmeprod.azurecr.io/api:1".to_string(),
        })
        .permissions("execution".to_string());
    for sandbox in &sandboxes {
        worker = worker.link(sandbox);
    }
    let execution_sa =
        ServiceAccount::from_permission_profile("execution-sa".to_string(), &profile, |name| {
            alien_permissions::get_permission_set(name).cloned()
        })
        .expect("built-in permission sets resolve");

    let mut stack = Stack::new("byo-sandbox".to_string())
        .permission("execution", profile)
        .add(resource_group(), ResourceLifecycle::Frozen)
        .add(
            AzureContainerAppsEnvironment::new("default-container-apps-environment".to_string())
                .build(),
            ResourceLifecycle::Frozen,
        )
        .add(execution_sa, ResourceLifecycle::Frozen);
    for sandbox in sandboxes {
        stack = stack.add(sandbox, ResourceLifecycle::Frozen);
    }
    stack.add(worker.build(), ResourceLifecycle::Live).build()
}

fn rendered_tf(module: &alien_terraform::ModuleFiles) -> String {
    module
        .files
        .iter()
        .filter(|(name, _)| name.ends_with(".tf"))
        .map(|(_, contents)| contents.clone())
        .collect::<Vec<_>>()
        .join("\n")
}

fn attribute(block: &hcl::Block, name: &str) -> String {
    block
        .body()
        .attributes()
        .find(|attribute| attribute.key() == name)
        .map(|attribute| attribute.expr().to_string())
        .unwrap_or_else(|| {
            panic!(
                "{} block has no `{name}` attribute: {}",
                block
                    .labels()
                    .iter()
                    .map(|label| label.as_str())
                    .collect::<Vec<_>>()
                    .join(" "),
                hcl::to_string(block).expect("the block renders")
            )
        })
}

/// `(label, name, description)` of every rendered role assignment: its Terraform address, the
/// Azure name seed it is applied under, and the text an approver reads.
fn assignment_addresses(rendered: &str) -> Vec<(String, String, String)> {
    let body: hcl::Body = hcl::from_str(rendered).expect("the module parses");
    body.blocks()
        .filter(|block| {
            block.identifier() == "resource"
                && block.labels().first().map(|label| label.as_str())
                    == Some("azurerm_role_assignment")
        })
        .map(|block| {
            (
                block.labels()[1].as_str().to_string(),
                attribute(block, "name"),
                attribute(block, "description"),
            )
        })
        .collect()
}

/// The error the module refuses to render with, for a workload profile on these sandboxes. A
/// refusal is never retryable: the stack has to change.
fn refusal(
    profile: PermissionProfile,
    sandbox_ids: &[&str],
) -> alien_error::AlienError<alien_core::ErrorData> {
    let stack = workload_sandbox_stack(profile, sandbox_ids);
    let error =
        super::helpers::try_render(&stack, TerraformTarget::Azure, StackSettings::default())
            .expect_err("the module is refused");
    assert_eq!(error.code, "OPERATION_NOT_SUPPORTED", "{error}");
    assert!(!error.retryable, "{error}");
    error
}

/// `(scope, role_definition_id, principal_id)` of every rendered role assignment.
fn role_assignments(rendered: &str) -> Vec<(String, String, String)> {
    let body: hcl::Body = hcl::from_str(rendered).expect("the module parses");
    body.blocks()
        .filter(|block| {
            block.identifier() == "resource"
                && block.labels().first().map(|label| label.as_str())
                    == Some("azurerm_role_assignment")
        })
        .map(|block| {
            (
                attribute(block, "scope"),
                attribute(block, "role_definition_id"),
                attribute(block, "principal_id"),
            )
        })
        .collect()
}

/// The scopes on which the execution identity holds the sandbox data-plane role.
fn data_plane_scopes(rendered: &str) -> Vec<String> {
    role_assignments(rendered)
        .into_iter()
        .filter(|(_, role, principal)| {
            role.ends_with(&format!("roleDefinitions/{SANDBOX_DATA_PLANE_ROLE_ID}\""))
                && principal == "azurerm_user_assigned_identity.execution_sa.principal_id"
        })
        .map(|(scope, _, _)| scope)
        .collect()
}

/// A grant keyed by one sandbox reaches that group and no sibling.
///
/// `sandbox/execute` binds at resource scope only, so the identity's stack-wide grants cannot
/// carry it; without this assignment the data plane answers every `sandbox.create` with 403.
#[test]
fn a_keyed_execute_grant_lands_on_that_sandbox_group_alone() {
    let stack = workload_sandbox_stack(
        PermissionProfile::new().resource("agents", ["sandbox/execute"]),
        &["agents", "other"],
    );
    let module = render(&stack, TerraformTarget::Azure, StackSettings::default());
    let rendered = rendered_tf(&module);

    assert_eq!(
        data_plane_scopes(&rendered),
        ["azapi_resource.agents.id"],
        "{rendered}"
    );
    assert_terraform_valid(&module, "azure sandbox keyed workload grant");
}

/// A grant of `sandbox/management` keyed by one sandbox lands on that group through the
/// setup-owned custom role rendered for the same profile, so lifecycle control stops at the one
/// group rather than every sandbox in the resource group.
#[test]
fn a_keyed_management_grant_uses_the_setup_owned_role_on_that_sandbox_group() {
    let stack = workload_sandbox_stack(
        PermissionProfile::new().resource("agents", ["sandbox/management"]),
        &["agents", "other"],
    );
    let module = render(&stack, TerraformTarget::Azure, StackSettings::default());
    let rendered = rendered_tf(&module);

    let execution_assignments: Vec<_> = role_assignments(&rendered)
        .into_iter()
        .filter(|(_, _, principal)| {
            principal == "azurerm_user_assigned_identity.execution_sa.principal_id"
        })
        .collect();
    assert_eq!(execution_assignments.len(), 1, "{rendered}");
    let (scope, role, _) = &execution_assignments[0];
    assert_eq!(scope, "azapi_resource.agents.id", "{rendered}");
    assert!(
        role.starts_with("azurerm_role_definition.setup_execution_sandbox_management_"),
        "the group grant uses the setup-owned role for this profile: {role}"
    );
    // Addressed by the set and the role's key, not its display name, so renaming the role keeps
    // the address.
    let addresses = assignment_addresses(&rendered)
        .into_iter()
        .filter(|(label, _, _)| label.starts_with("agents_execution_"))
        .collect::<Vec<_>>();
    assert_eq!(
        addresses
            .iter()
            .map(|(label, _, _)| label.as_str())
            .collect::<Vec<_>>(),
        ["agents_execution_sandbox_management_microsoft_app_sandbox_groups_sandboxes_write_permissions"],
        "{rendered}"
    );
    assert!(
        addresses[0].1.ends_with(
            ":agents:execution:sandbox_management_microsoft_app_sandbox_groups_sandboxes_write_permissions\")"
        ),
        "{}",
        addresses[0].1
    );
    assert_terraform_valid(&module, "azure sandbox keyed management grant");
}

/// A `"*"` grant of `sandbox/execute` reaches every sandbox group in the deployment, one
/// assignment each, and never the resource group: the preflight that authors link grants trusts a
/// `"*"` entry to cover the link. `sandbox/management` under the same `"*"` (the role carrying
/// `sandboxes/write`) already lands at the resource group through the identity's stack-wide
/// grants, so no group-scoped copy of it renders.
#[test]
fn a_stack_wide_execute_grant_lands_on_every_sandbox_group_and_never_the_resource_group() {
    let stack = workload_sandbox_stack(
        PermissionProfile::new().global(["sandbox/management", "sandbox/execute"]),
        &["agents", "other"],
    );
    let module = render(&stack, TerraformTarget::Azure, StackSettings::default());
    let rendered = rendered_tf(&module);

    let mut scopes = data_plane_scopes(&rendered);
    scopes.sort();
    assert_eq!(
        scopes,
        ["azapi_resource.agents.id", "azapi_resource.other.id"],
        "{rendered}"
    );

    let execution_assignments: Vec<_> = role_assignments(&rendered)
        .into_iter()
        .filter(|(_, _, principal)| {
            principal == "azurerm_user_assigned_identity.execution_sa.principal_id"
        })
        .collect();
    let group_scoped_management = execution_assignments
        .iter()
        .filter(|(scope, role, _)| {
            scope.starts_with("azapi_resource.") && role.contains("sandboxes_write_permissions")
        })
        .count();
    assert_eq!(
        group_scoped_management, 0,
        "a stack-wide management grant stays at the resource group:\n{rendered}"
    );
    let resource_group_management = execution_assignments
        .iter()
        .filter(|(scope, role, _)| {
            scope.contains("resourceGroups/${var.azure_resource_group_name}\"")
                && role.contains("sandboxes_write_permissions")
        })
        .count();
    assert_eq!(
        resource_group_management, 1,
        "the identity holds management at the resource group once:\n{rendered}"
    );

    snapshot_module("azure_sandbox_workload_grant", &module);
    assert_terraform_valid(&module, "azure sandbox stack-wide workload grant");
}

/// A profile with no resource-only sandbox set gets nothing on a sandbox group: an empty one, and
/// one whose `"*"` names `sandbox/management`, which the stack-wide path already delivers.
#[test]
fn a_profile_without_a_resource_only_sandbox_set_gets_no_group_grant() {
    for (case, profile, expected_at_the_resource_group) in [
        ("no sets", PermissionProfile::new(), 0),
        (
            "stack-wide management only",
            PermissionProfile::new().global(["sandbox/management"]),
            1,
        ),
    ] {
        let stack = workload_sandbox_stack(profile, &["agents"]);
        let module = render(&stack, TerraformTarget::Azure, StackSettings::default());
        let rendered = rendered_tf(&module);

        let execution_assignments: Vec<_> = role_assignments(&rendered)
            .into_iter()
            .filter(|(_, _, principal)| {
                principal == "azurerm_user_assigned_identity.execution_sa.principal_id"
            })
            .collect();
        let on_the_group = execution_assignments
            .iter()
            .filter(|(scope, _, _)| scope == "azapi_resource.agents.id")
            .count();
        assert_eq!(on_the_group, 0, "{case}:\n{rendered}");
        let at_the_resource_group = execution_assignments
            .iter()
            .filter(|(scope, _, _)| {
                scope.contains("resourceGroups/${var.azure_resource_group_name}\"")
            })
            .count();
        assert_eq!(
            at_the_resource_group, expected_at_the_resource_group,
            "{case}: a stack-wide management grant lands at the resource group once:\n{rendered}"
        );
        assert_terraform_valid(&module, &format!("azure sandbox no group grant, {case}"));
    }
}

/// `sandbox/execute` and `sandbox/remote-execute` both resolve to the data-plane role; Azure
/// refuses a second assignment of one role to one principal at one scope, so one renders.
#[test]
fn execute_and_remote_execute_share_one_data_plane_assignment() {
    let stack = workload_sandbox_stack(
        PermissionProfile::new().resource("agents", ["sandbox/execute", "sandbox/remote-execute"]),
        &["agents"],
    );
    let module = render(&stack, TerraformTarget::Azure, StackSettings::default());
    let rendered = rendered_tf(&module);

    assert_eq!(
        data_plane_scopes(&rendered),
        ["azapi_resource.agents.id"],
        "{rendered}"
    );
    assert_terraform_valid(&module, "azure sandbox deduplicated workload grant");
}

/// A set of another resource type keyed on a sandbox has no sandbox-group scope to render on, so
/// the module is refused rather than rendered with a grant nobody declared.
#[test]
fn a_set_of_another_resource_type_keyed_on_a_sandbox_is_refused() {
    let error = refusal(
        PermissionProfile::new().resource("agents", ["storage/data-read"]),
        &["agents"],
    );
    let message = error.to_string();
    assert!(
        message.contains("grant storage/data-read to profile execution on sandbox agents")
            && message.contains("only sandbox/* permission sets can be granted on a sandbox"),
        "{message}"
    );
}

/// An inline set may reuse a built-in id while granting something else, and the role label the
/// id resolves to here is the built-in set's, so a keyed inline set is refused rather than handed
/// the built-in role.
#[test]
fn an_inline_set_reusing_a_built_in_id_keyed_on_a_sandbox_is_refused() {
    let inline = alien_core::permissions::PermissionSet {
        id: "sandbox/management".to_string(),
        description: "not the built-in set".to_string(),
        platforms: alien_core::permissions::PlatformPermissions {
            aws: None,
            gcp: None,
            azure: None,
        },
    };
    let error = refusal(
        PermissionProfile::new().resource("agents", [PermissionSetReference::from_inline(inline)]),
        &["agents"],
    );
    let message = error.to_string();
    assert!(
        message.contains("grant sandbox/management to profile execution on sandbox agents")
            && message.contains("only built-in permission sets referenced by name"),
        "{message}"
    );
}

/// Provisioning creates and deletes the group itself, so a workload profile keyed on a sandbox
/// cannot hold it; a `"*"` provision grant is the service-account emitter's and renders no group
/// grant here.
#[test]
fn a_keyed_provision_grant_on_a_sandbox_is_refused_and_a_stack_wide_one_is_skipped() {
    let error = refusal(
        PermissionProfile::new().resource("agents", ["sandbox/provision"]),
        &["agents"],
    );
    let message = error.to_string();
    assert!(
        message.contains("grant sandbox/provision to profile execution on sandbox agents")
            && message.contains("compiled at stack scope only on Azure"),
        "{message}"
    );

    let stack = workload_sandbox_stack(
        PermissionProfile::new().global(["sandbox/provision"]),
        &["agents"],
    );
    let module = render(&stack, TerraformTarget::Azure, StackSettings::default());
    let rendered = rendered_tf(&module);
    let execution_assignments: Vec<_> = role_assignments(&rendered)
        .into_iter()
        .filter(|(_, _, principal)| {
            principal == "azurerm_user_assigned_identity.execution_sa.principal_id"
        })
        .collect();
    let on_the_group = execution_assignments
        .iter()
        .filter(|(scope, _, _)| scope == "azapi_resource.agents.id")
        .count();
    assert_eq!(on_the_group, 0, "{rendered}");
    let at_the_resource_group = execution_assignments
        .iter()
        .filter(|(scope, _, _)| scope.contains("resourceGroups/${var.azure_resource_group_name}\""))
        .count();
    assert_eq!(
        at_the_resource_group, 1,
        "a stack-wide provision grant lands at the resource group once:\n{rendered}"
    );
}

/// The setup renders one role definition per profile and set id and takes inline sets that bind on
/// an Azure resource, so such an inline set reusing a built-in id anywhere in the profile could be
/// the one a keyed custom-role grant on another sandbox binds, carrying the inline set's actions.
/// Inline sets need their own ids there.
#[test]
fn an_inline_set_reusing_a_built_in_id_anywhere_in_the_profile_is_refused() {
    let inline = alien_permissions::get_permission_set("sandbox/management")
        .cloned()
        .expect("the built-in set is registered");
    let error = refusal(
        PermissionProfile::new()
            .global([PermissionSetReference::from_inline(inline)])
            .resource("other", ["sandbox/management"]),
        &["agents", "other"],
    );
    let message = error.to_string();
    assert!(
        message.contains("grant sandbox/management to profile execution on sandbox other")
            && message.contains("reuses this built-in id"),
        "{message}"
    );
}

/// An inline `sandbox/*` set under `"*"` that binds at the sandbox group only is delivered by no
/// setup emitter, so it is refused; one that binds at the stack is the service-account emitter's
/// and renders at the resource group with nothing on the group.
#[test]
fn an_inline_stack_wide_set_bound_only_at_the_group_is_refused() {
    let mut group_only = alien_permissions::get_permission_set("sandbox/execute")
        .cloned()
        .expect("the built-in set is registered");
    group_only.id = "sandbox/exec-copy".to_string();
    let error = refusal(
        PermissionProfile::new().global([PermissionSetReference::from_inline(group_only)]),
        &["agents"],
    );
    let message = error.to_string();
    assert!(
        message.contains("grant sandbox/exec-copy to profile execution on sandbox agents")
            && message.contains("delivered by no setup emitter"),
        "{message}"
    );

    let mut stack_bound = alien_permissions::get_permission_set("sandbox/management")
        .cloned()
        .expect("the built-in set is registered");
    stack_bound.id = "sandbox/management-copy".to_string();
    let stack = workload_sandbox_stack(
        PermissionProfile::new().global([PermissionSetReference::from_inline(stack_bound)]),
        &["agents"],
    );
    let module = render(&stack, TerraformTarget::Azure, StackSettings::default());
    let rendered = rendered_tf(&module);
    let execution_scopes: Vec<String> = role_assignments(&rendered)
        .into_iter()
        .filter(|(_, _, principal)| {
            principal == "azurerm_user_assigned_identity.execution_sa.principal_id"
        })
        .map(|(scope, _, _)| scope)
        .collect();
    assert!(
        !execution_scopes
            .iter()
            .any(|scope| scope == "azapi_resource.agents.id"),
        "{rendered}"
    );
    assert!(
        execution_scopes
            .iter()
            .any(|scope| scope.contains("resourceGroups/${var.azure_resource_group_name}\"")),
        "the stack-bound copy lands at the resource group:\n{rendered}"
    );
}

/// Each assignment is addressed by sandbox, profile and role, so the order a profile lists its
/// sets in, and which of two sets resolving to one role comes first, change nothing in the
/// rendered block: not the Terraform address or Azure name (see the emitter doc), nor the
/// description (a changed attribute is a plan diff on a stack nobody changed).
#[test]
fn a_workload_assignment_keeps_its_address_when_the_profile_is_reordered() {
    let addresses = |sets: [&str; 2]| {
        let stack = workload_sandbox_stack(
            PermissionProfile::new().resource("agents", sets),
            &["agents"],
        );
        let module = render(&stack, TerraformTarget::Azure, StackSettings::default());
        let mut addresses = assignment_addresses(&rendered_tf(&module));
        addresses.sort();
        addresses
    };

    let forward = addresses(["sandbox/execute", "sandbox/remote-execute"]);
    let reversed = addresses(["sandbox/remote-execute", "sandbox/execute"]);
    assert_eq!(forward, reversed);
    assert_eq!(forward.len(), 1, "{forward:?}");
    let (label, name, _) = &forward[0];
    assert_eq!(
        label,
        &format!(
            "agents_execution_{}",
            SANDBOX_DATA_PLANE_ROLE_ID.replace('-', "_")
        )
    );
    assert!(
        name.ends_with(&format!(
            ":agents:execution:{}\")",
            SANDBOX_DATA_PLANE_ROLE_ID.replace('-', "_")
        )),
        "{name}"
    );
}

/// Which of two sets resolving to the data-plane role a profile names changes nothing in the
/// rendered assignment: its address, Azure name and description follow the role.
#[test]
fn execute_and_remote_execute_alone_render_the_same_assignment() {
    let addresses = |set: &str| {
        let stack = workload_sandbox_stack(
            PermissionProfile::new().resource("agents", [set]),
            &["agents"],
        );
        let module = render(&stack, TerraformTarget::Azure, StackSettings::default());
        assignment_addresses(&rendered_tf(&module))
            .into_iter()
            .filter(|(label, _, _)| label.starts_with("agents_execution_"))
            .collect::<Vec<_>>()
    };

    let execute = addresses("sandbox/execute");
    assert_eq!(execute.len(), 1, "{execute:?}");
    assert_eq!(execute, addresses("sandbox/remote-execute"));
    assert!(
        execute[0]
            .2
            .contains("Container Apps SandboxGroup Data Owner"),
        "the description names the role an approver looks up: {}",
        execute[0].2
    );
}

/// Two sets granted on one sandbox through their own custom roles each get an assignment; neither
/// is taken for the other's.
#[test]
fn two_custom_role_sets_on_one_sandbox_each_get_an_assignment() {
    let stack = workload_sandbox_stack(
        PermissionProfile::new().resource("agents", ["sandbox/management", "sandbox/heartbeat"]),
        &["agents"],
    );
    let module = render(&stack, TerraformTarget::Azure, StackSettings::default());
    let rendered = rendered_tf(&module);

    let mut on_the_group: Vec<String> = role_assignments(&rendered)
        .into_iter()
        .filter(|(scope, _, principal)| {
            scope == "azapi_resource.agents.id"
                && principal == "azurerm_user_assigned_identity.execution_sa.principal_id"
        })
        .map(|(_, role, _)| role)
        .collect();
    on_the_group.sort();
    on_the_group.dedup();
    assert_eq!(on_the_group.len(), 2, "{rendered}");
    assert!(
        on_the_group
            .iter()
            .all(|role| role.starts_with("azurerm_role_definition.setup_execution_sandbox_")),
        "{on_the_group:?}"
    );
    assert_terraform_valid(&module, "azure sandbox two custom-role workload grants");
}

/// An inline set may reuse a built-in id when it grants nothing on Azure: no setup role definition
/// is rendered from it, so the keyed grant still renders from the built-in set, whether that set
/// resolves to a predefined role or a setup-owned custom one, as the link preflight expects.
#[test]
fn an_inline_set_reusing_a_built_in_id_that_reaches_no_setup_role_keeps_the_grant() {
    let no_azure = |id: &str| alien_core::permissions::PermissionSet {
        id: id.to_string(),
        description: "a narrower grant on other platforms".to_string(),
        platforms: alien_core::permissions::PlatformPermissions {
            aws: None,
            gcp: None,
            azure: None,
        },
    };

    for (case, inline, set, expected_role) in [
        (
            "execute, inline without Azure",
            no_azure("sandbox/execute"),
            "sandbox/execute",
            SANDBOX_DATA_PLANE_ROLE_ID,
        ),
        (
            "management, inline without Azure",
            no_azure("sandbox/management"),
            "sandbox/management",
            "setup_execution_sandbox_management_",
        ),
    ] {
        let stack = workload_sandbox_stack(
            PermissionProfile::new()
                .global([PermissionSetReference::from_inline(inline)])
                .resource("agents", [set]),
            &["agents"],
        );
        let module = render(&stack, TerraformTarget::Azure, StackSettings::default());
        let rendered = rendered_tf(&module);
        let on_the_group = role_assignments(&rendered)
            .into_iter()
            .filter(|(scope, role, principal)| {
                scope == "azapi_resource.agents.id"
                    && role.contains(expected_role)
                    && principal == "azurerm_user_assigned_identity.execution_sa.principal_id"
            })
            .count();
        assert_eq!(on_the_group, 1, "{case}:\n{rendered}");
        assert_terraform_valid(&module, &format!("azure sandbox inline reuse, {case}"));
    }
}

/// An AKS target is `Platform::Azure` but skips sandbox emission, so a note keyed off the platform
/// would tell that installer to register a provider for a resource their package does not contain.
#[test]
fn an_aks_package_is_not_told_about_a_sandbox_group_it_does_not_get() {
    let sandbox = Sandbox::new("agents".to_string())
        .code(SandboxCode::Image {
            image: "ubuntu".to_string(),
        })
        .egress(SandboxEgress::Allow)
        .lifecycle(SandboxLifecyclePolicy {
            max_lifetime_seconds: None,
            idle_pause_seconds: None,
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
        // The document a security team approves the grant from. Naming a data-plane role on a
        // group the module never creates asks them to approve a scope that does not exist.
        let permissions = module.get("PERMISSIONS.md").unwrap_or("");
        assert_eq!(
            permissions.contains("sandboxGroups"),
            expects_group,
            "{target:?} documents the sandbox grant exactly when it installs one:\n{permissions}"
        );
    }
}

/// A remote sandbox renders on its own, with no other resource declared — which is what makes
/// `parent_id` worth pinning: it must resolve to the deployer-supplied
/// `var.azure_resource_group_name`, not a resource group the module would have had to declare.
/// Reaches `terraform validate` only, not apply.
#[test]
fn an_azure_remote_sandbox_renders_without_any_other_resource_declared() {
    let sandbox = Sandbox::new("agents".to_string())
        .code(SandboxCode::Image {
            image: "ubuntu".to_string(),
        })
        .egress(SandboxEgress::Allow)
        .lifecycle(SandboxLifecyclePolicy {
            max_lifetime_seconds: None,
            idle_pause_seconds: None,
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

    // With no other resource declared there's no resource group of this stack's own to parent to,
    // so the group must hang off the one the deployer names. A wrong reference would fail
    // `terraform validate` below, but a wrong *variable* would not — so this reads the attribute.
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

#[test]
fn azure_explicit_resource_grant_fragments_validate() {
    let stack = Stack::new("example".to_string())
        .add(resource_group(), ResourceLifecycle::Frozen)
        .build();
    let mut module = render(&stack, TerraformTarget::Azure, StackSettings::default());
    let builtin = alien_permissions::get_permission_set("storage/data-write").unwrap();
    let mut fragment = TfFragment::default();
    let mut seen = HashSet::new();
    for (label, resource, set) in [
        ("node_archive", "archive", builtin),
        ("node_backup", "backup", builtin),
    ] {
        let context = PermissionContext::new()
            .with_subscription_id("11111111-1111-1111-1111-111111111111")
            .with_resource_group("data")
            .with_storage_account_name("archiveaccount")
            .with_resource_name(resource)
            .with_stack_prefix("example");
        emit_role_definition_and_assignments_for_target(
            &mut fragment,
            label,
            "node",
            BindingTarget::Resource,
            Expression::String("22222222-2222-2222-2222-222222222222".to_string()),
            set,
            &context,
            &mut seen,
        )
        .unwrap();
    }
    assert_eq!(fragment.resource_blocks.len(), 2);
    let mut body = hcl::Body::builder();
    for block in fragment.resource_blocks {
        body = body.add_block(block);
    }
    module.files.insert(
        "node-permissions.tf".to_string(),
        hcl::to_string(&body.build()).unwrap(),
    );
    test_utils::terraform_fmt_and_validate(&linter_files(&module))
        .assert_ok("explicit resource grant fragments: fmt, provider init, validate");
}

/// A stack with `agents` published for remote access beside an unpublished `other`, which the
/// Worker links, and `execution` holding `profile`.
fn published_sandbox_stack(profile: PermissionProfile) -> Stack {
    let execution_sa =
        ServiceAccount::from_permission_profile("execution-sa".to_string(), &profile, |name| {
            alien_permissions::get_permission_set(name).cloned()
        })
        .expect("built-in permission sets resolve");
    let worker = Worker::new("api".to_string())
        .code(WorkerCode::Image {
            image: "acmeprod.azurecr.io/api:1".to_string(),
        })
        .permissions("execution".to_string())
        .link(&frozen_sandbox("other"))
        .build();
    Stack::new("byo-sandbox".to_string())
        .permission("execution", profile)
        .add(resource_group(), ResourceLifecycle::Frozen)
        .add(
            AzureContainerAppsEnvironment::new("default-container-apps-environment".to_string())
                .build(),
            ResourceLifecycle::Frozen,
        )
        .add(execution_sa, ResourceLifecycle::Frozen)
        .add_with_remote_access(frozen_sandbox("agents"), ResourceLifecycle::Frozen)
        .add(frozen_sandbox("other"), ResourceLifecycle::Frozen)
        .add(worker, ResourceLifecycle::Live)
        .build()
}

/// A sandbox published for remote access belongs to its remote caller, so a `"*"` grant fans out
/// to every other sandbox group and skips that one. `sandbox/images` is the case that matters: the
/// single-tenant preflight does not count it as reaching a sandbox, and on a published group it
/// would let the deployment replace the image remote sessions boot from.
#[test]
fn a_stack_wide_grant_skips_a_sandbox_published_for_remote_access() {
    let profile = PermissionProfile::new().global(["sandbox/images"]);
    let stack = published_sandbox_stack(profile);
    let module = render(&stack, TerraformTarget::Azure, StackSettings::default());
    let rendered = rendered_tf(&module);

    let execution_scopes: Vec<String> = role_assignments(&rendered)
        .into_iter()
        .filter(|(_, _, principal)| {
            principal == "azurerm_user_assigned_identity.execution_sa.principal_id"
        })
        .map(|(scope, _, _)| scope)
        .collect();
    assert!(
        !execution_scopes
            .iter()
            .any(|scope| scope == "azapi_resource.agents.id"),
        "{rendered}"
    );
    assert!(
        execution_scopes
            .iter()
            .any(|scope| scope == "azapi_resource.other.id"),
        "the unpublished group still gets the grant:\n{rendered}"
    );
    assert_terraform_valid(
        &module,
        "azure sandbox stack-wide grant beside a published one",
    );
}

/// A grant keyed by a published sandbox is refused rather than rendered: the single-tenant preflight
/// counts only grants that reach a session, and a disk-image write changes what remote sessions
/// boot without reaching one. A set with nothing on Azure, from a profile shared across clouds,
/// still renders nothing and is not refused.
#[test]
fn a_keyed_grant_on_a_sandbox_published_for_remote_access_is_refused() {
    let shared =
        published_sandbox_stack(PermissionProfile::new().resource("agents", ["sandbox/templates"]));
    render(&shared, TerraformTarget::Azure, StackSettings::default());

    let stack =
        published_sandbox_stack(PermissionProfile::new().resource("agents", ["sandbox/images"]));
    let error =
        super::helpers::try_render(&stack, TerraformTarget::Azure, StackSettings::default())
            .expect_err("the module is refused");
    assert_eq!(error.code, "OPERATION_NOT_SUPPORTED", "{error}");
    assert!(!error.retryable, "{error}");
    let message = error.to_string();
    assert!(
        message.contains("grant sandbox/images to profile execution on sandbox agents")
            && message.contains("published for remote access"),
        "{message}"
    );
}
