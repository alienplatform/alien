//! What a protected deployment's GCP and Azure setup keeps from the management identity: the
//! gate alone writes what runs code. Checked on the management grants and the deny settings
//! exactly as Terraform renders them.

use std::collections::BTreeSet;

use super::helpers::{assert_terraform_valid, render};
use alien_core::{
    AzureResourceGroup, ManagementPermissions, PermissionProfile, ReleaseTrustSettings,
    RemoteStackManagement, ResourceLifecycle, Stack, StackSettings,
};
use alien_release_trust::{SessionApproval, SoftwareKey};
use alien_terraform::{ModuleFiles, TerraformTarget};

const MANAGEMENT_SETS: [&str; 10] = [
    "compute-cluster/management",
    "compute-cluster/provision",
    "worker/management",
    "worker/provision",
    "service-account/provision",
    "build/management",
    "build/provision",
    "sandbox/provision",
    "storage/provision",
    "queue/provision",
];

fn protected() -> StackSettings {
    StackSettings {
        release_trust: Some(ReleaseTrustSettings {
            project: "prj_123".to_string(),
            deployment: "dep_456".to_string(),
            signing: format!("SHA256:{}", "ab".repeat(32)),
            components: SoftwareKey::from_secret_bytes(&[22; 32])
                .expect("a valid test key")
                .public_key()
                .as_str()
                .to_string(),
            session_approval: SessionApproval::Developer,
        }),
        ..StackSettings::default()
    }
}

fn stack(azure: bool) -> Stack {
    let mut stack = Stack::new("orders".to_string());
    if azure {
        stack = stack.add(
            AzureResourceGroup::new("default-resource-group".to_string()).build(),
            ResourceLifecycle::Frozen,
        );
    }
    stack
        .add(
            RemoteStackManagement::new("management".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .management(ManagementPermissions::Override(
            PermissionProfile::new().global(MANAGEMENT_SETS),
        ))
        .build()
}

fn resources(module: &ModuleFiles, kind: &str) -> Vec<hcl::Block> {
    module
        .iter()
        .filter(|(path, _)| path.ends_with(".tf"))
        .flat_map(|(path, contents)| {
            hcl::parse(contents)
                .unwrap_or_else(|error| panic!("{path} parses: {error}"))
                .into_blocks()
                .filter(|block| {
                    block.identifier() == "resource"
                        && block.labels().first().map(|label| label.as_str()) == Some(kind)
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

fn attribute<'a>(block: &'a hcl::Block, name: &str) -> Option<&'a hcl::Expression> {
    block
        .body()
        .attributes()
        .find(|attribute| attribute.key() == name)
        .map(|attribute| attribute.expr())
}

fn strings(expression: &hcl::Expression) -> Vec<String> {
    let hcl::Expression::Array(items) = expression else {
        panic!("expected a list, got {expression:?}");
    };
    items
        .iter()
        .map(|item| match item {
            hcl::Expression::String(value) => value.clone(),
            other => panic!("expected a string, got {other:?}"),
        })
        .collect()
}

/// Every permission the GCP management service account holds through a custom role bound on
/// the project.
fn gcp_management_permissions(module: &ModuleFiles) -> BTreeSet<String> {
    let roles = resources(module, "google_project_iam_custom_role");
    let bound: BTreeSet<String> = resources(module, "google_project_iam_member")
        .iter()
        .filter(|binding| {
            hcl::format::to_string(attribute(binding, "member").expect("a member"))
                .expect("formats")
                .contains("google_service_account.management.email")
        })
        .filter_map(|binding| {
            let role = hcl::format::to_string(attribute(binding, "role").expect("a role"))
                .expect("formats");
            role.split("google_project_iam_custom_role.")
                .nth(1)
                .map(|rest| rest.split('[').next().expect("label").to_string())
        })
        .collect();
    assert!(
        !bound.is_empty(),
        "the management account is bound to custom roles"
    );
    roles
        .iter()
        .filter(|role| bound.contains(role.labels()[1].as_str()))
        .flat_map(|role| strings(attribute(role, "permissions").expect("permissions")))
        .collect()
}

#[test]
fn gcp_management_loses_what_only_the_gate_may_do() {
    let open = gcp_management_permissions(&render(
        &stack(false),
        TerraformTarget::Gcp,
        StackSettings::default(),
    ));
    let locked_module = render(&stack(false), TerraformTarget::Gcp, protected());
    assert_terraform_valid(&locked_module, "protected GCP management");
    let locked = gcp_management_permissions(&locked_module);

    for owned in [
        "run.services.create",
        "run.services.update",
        "run.services.delete",
        "compute.instances.create",
        "compute.instances.setMetadata",
        "compute.instanceTemplates.create",
        "iam.serviceAccounts.actAs",
        "iam.roles.update",
        "resourcemanager.projects.setIamPolicy",
        "iam.serviceAccounts.setIamPolicy",
        "cloudbuild.builds.update",
        "aiplatform.reasoningEngines.create",
    ] {
        assert!(
            open.contains(owned),
            "an unprotected deployment keeps {owned}"
        );
        assert!(
            !locked.contains(owned),
            "a protected deployment loses {owned}"
        );
    }
    let set_iam: Vec<&String> = locked
        .iter()
        .filter(|permission| permission.ends_with(".setIamPolicy"))
        .collect();
    assert!(set_iam.is_empty(), "{set_iam:?}");
    for kept in [
        "compute.instanceGroupManagers.update",
        "compute.instanceTemplates.get",
        "run.services.get",
        "secretmanager.versions.add",
    ] {
        assert!(locked.contains(kept), "a protected deployment keeps {kept}");
    }
    assert!(locked.is_subset(&open));
}

/// Whether a `denyWriteAndDelete` deny assignment refuses `principal` the `action`: it covers
/// every write and delete but the excluded actions, for every principal but the excluded ones.
fn deny_refuses(deny: &Deny, principal: &str, action: &str) -> bool {
    let action = action.to_ascii_lowercase();
    (action.ends_with("/write") || action.ends_with("/delete"))
        && !deny
            .excluded_actions
            .iter()
            .any(|excluded| excluded.eq_ignore_ascii_case(&action))
        && !deny.excluded_principals.iter().any(|p| p == principal)
}

struct Deny {
    excluded_actions: Vec<String>,
    excluded_principals: Vec<String>,
}

fn object_field<'a>(expression: &'a hcl::Expression, key: &str) -> &'a hcl::Expression {
    let hcl::Expression::Object(object) = expression else {
        panic!("expected an object, got {expression:?}");
    };
    object
        .iter()
        .find(|(name, _)| match name {
            hcl::ObjectKey::Identifier(identifier) => identifier.as_str() == key,
            hcl::ObjectKey::Expression(hcl::Expression::String(name)) => name == key,
            _ => false,
        })
        .map(|(_, value)| value)
        .unwrap_or_else(|| panic!("no {key}"))
}

/// The actions the Azure management identity's custom roles grant.
fn azure_management_actions(module: &ModuleFiles) -> BTreeSet<String> {
    resources(module, "azurerm_role_definition")
        .iter()
        .filter(|role| role.labels()[1].as_str().starts_with("management"))
        .flat_map(|role| {
            role.body()
                .blocks()
                .filter(|block| block.identifier() == "permissions")
                .flat_map(|permissions| {
                    attribute(permissions, "actions")
                        .map(strings)
                        .unwrap_or_default()
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

#[test]
fn azure_setup_denies_management_what_runs_code() {
    let open_module = render(
        &stack(true),
        TerraformTarget::Azure,
        StackSettings::default(),
    );
    assert!(resources(&open_module, "azapi_resource").is_empty());
    let open = azure_management_actions(&open_module);

    let module = render(&stack(true), TerraformTarget::Azure, protected());
    assert_terraform_valid(&module, "protected Azure deployment stack");
    let stacks = resources(&module, "azapi_resource");
    assert_eq!(stacks.len(), 1, "one deployment stack");
    let protection = &stacks[0];
    assert_eq!(
        attribute(protection, "type"),
        Some(&hcl::Expression::String(
            "Microsoft.Resources/deploymentStacks@2024-03-01".to_string()
        ))
    );
    let properties = object_field(attribute(protection, "body").expect("a body"), "properties");
    // The stack manages the deployment's resource group, so its deny covers what is in it.
    let template = object_field(properties, "template");
    let hcl::Expression::Array(managed) = object_field(template, "resources") else {
        panic!("template resources");
    };
    assert_eq!(managed.len(), 1);
    assert_eq!(
        object_field(&managed[0], "type"),
        &hcl::Expression::String("Microsoft.Resources/resourceGroups".to_string())
    );
    let name = hcl::format::to_string(object_field(&managed[0], "name")).expect("formats");
    assert_eq!(name, "azurerm_resource_group.default_resource_group.name");

    let settings = object_field(properties, "denySettings");
    assert_eq!(
        object_field(settings, "mode"),
        &hcl::Expression::String("denyWriteAndDelete".to_string())
    );
    assert_eq!(
        object_field(settings, "applyToChildScopes"),
        &hcl::Expression::Bool(true)
    );
    let hcl::Expression::Array(principals) = object_field(settings, "excludedPrincipals") else {
        panic!("excluded principals");
    };
    let deny = Deny {
        excluded_actions: strings(object_field(settings, "excludedActions")),
        excluded_principals: principals
            .iter()
            .map(|principal| hcl::format::to_string(principal).expect("formats"))
            .collect(),
    };
    assert_eq!(
        deny.excluded_principals,
        [
            "azurerm_user_assigned_identity.management_gate.principal_id",
            "data.azurerm_client_config.management_current.object_id",
        ]
    );
    assert!(deny.excluded_actions.len() <= 200);
    let management = "azurerm_user_assigned_identity.management.principal_id";
    let gate = "azurerm_user_assigned_identity.management_gate.principal_id";

    let locked = azure_management_actions(&module);
    for action in [
        "Microsoft.Compute/virtualMachineScaleSets/write",
        "Microsoft.Compute/virtualMachineScaleSets/delete",
        "Microsoft.App/jobs/write",
        "Microsoft.App/containerApps/write",
        "Microsoft.Authorization/roleAssignments/write",
        "Microsoft.ManagedIdentity/userAssignedIdentities/federatedIdentityCredentials/write",
    ] {
        assert!(
            open.contains(action) || action.starts_with("Microsoft.ManagedIdentity/"),
            "the management roles grant {action}, so the deny is what refuses it"
        );
        assert!(deny_refuses(&deny, management, action), "{action}");
        assert!(
            !deny_refuses(&deny, gate, action),
            "the gate writes {action}"
        );
    }
    // What runs no code stays with the management identity.
    for action in [
        "Microsoft.Storage/storageAccounts/blobServices/containers/write",
        "Microsoft.Storage/storageAccounts/blobServices/containers/delete",
        "Microsoft.ServiceBus/namespaces/queues/write",
        "Microsoft.Network/networkSecurityGroups/write",
    ] {
        assert!(locked.contains(action), "{action}");
        assert!(!deny_refuses(&deny, management, action), "{action}");
    }
    // Each excluded action is one the management identity holds and that runs no code.
    for action in &deny.excluded_actions {
        assert!(
            locked.contains(action),
            "{action} is excluded but not granted"
        );
        for path in [
            "Microsoft.App/",
            "Microsoft.Compute/",
            "Microsoft.Authorization/",
        ] {
            assert!(!action.starts_with(path), "{action} is excluded");
        }
    }
    // A deny cannot refuse an `/action`, so the roles leave out the ones that run code, and
    // nothing in them reaches the deployment stack.
    for action in [
        "Microsoft.App/containerApps/revisions/activate/action",
        "Microsoft.ManagedIdentity/userAssignedIdentities/assign/action",
    ] {
        assert!(
            open.contains(action),
            "an unprotected deployment keeps {action}"
        );
        assert!(
            !locked.contains(action),
            "a protected deployment loses {action}"
        );
    }
    assert!(!locked.contains("Microsoft.App/jobs/start/action"));
    assert!(locked.is_subset(&open));
    assert!(locked.iter().all(|action| !action
        .starts_with("Microsoft.Resources/deploymentStacks")
        && !action.contains('*')));
}
