//! The deployment `secrets` vault on GCP and Azure, rendered through the real preparation.
//!
//! The vendor's management identity writes that vault (environment sync) but must not read it
//! back. Workloads that declare a read keep it.

use super::helpers::{assert_terraform_valid, normalized, render};
use alien_core::{
    AzureManagementConfig, DeploymentConfig, EnvironmentVariablesSnapshot, ExternalBindings,
    GcpManagementConfig, ManagementConfig, PermissionProfile, Platform, ResourceLifecycle, Stack,
    StackSettings, StackState, Worker, WorkerCode, SECRETS_VAULT_ID,
};
use alien_preflights::runner::PreflightRunner;
use alien_terraform::{ModuleFiles, TerraformTarget};

/// A Worker whose profile reads the deployment's secrets vault, which preparation adds.
fn stack() -> Stack {
    Stack::new("acme".to_string())
        .permission(
            "execution",
            PermissionProfile::new().resource(SECRETS_VAULT_ID, ["vault/data-read"]),
        )
        .add(
            Worker::new("api".to_string())
                .code(WorkerCode::Image {
                    image: "registry.example.com/api:1".to_string(),
                })
                .permissions("execution".to_string())
                .build(),
            ResourceLifecycle::Live,
        )
        .build()
}

/// Prepare the stack the way a setup module is prepared: template preflights, then mutations.
async fn prepare(stack: Stack, platform: Platform) -> Stack {
    let runner = PreflightRunner::new();
    runner
        .run_template_preflights(&stack, platform)
        .await
        .expect("template preflights pass");
    let management = match platform {
        Platform::Gcp => ManagementConfig::Gcp(GcpManagementConfig {
            service_account_email: "manager@vendor-project.iam.gserviceaccount.com".to_string(),
        }),
        Platform::Azure => ManagementConfig::Azure(AzureManagementConfig {
            managing_tenant_id: "00000000-0000-0000-0000-000000000000".to_string(),
            oidc_issuer: "https://issuer.example.com".to_string(),
            oidc_subject: "manager".to_string(),
        }),
        other => panic!("unexpected platform {other:?}"),
    };
    let config = DeploymentConfig::builder()
        .deployment_name(stack.id().to_string())
        .stack_settings(StackSettings::default())
        .management_config(management)
        .environment_variables(EnvironmentVariablesSnapshot {
            variables: Vec::new(),
            hash: "empty".to_string(),
            created_at: "1970-01-01T00:00:00Z".to_string(),
        })
        .allow_frozen_changes(false)
        .external_bindings(ExternalBindings::default())
        .build();
    runner
        .apply_mutations(stack, &StackState::new(platform), &config)
        .await
        .expect("template mutations apply")
}

/// A `resource "<type>" "<label>" { ... }` block of a normalized module.
struct Block {
    kind: String,
    label: String,
    body: String,
}

fn resource_blocks(module: &ModuleFiles) -> Vec<Block> {
    let main = normalized(module);
    let starts: Vec<usize> = main.match_indices("resource \"").map(|(i, _)| i).collect();
    starts
        .iter()
        .enumerate()
        .map(|(n, &start)| {
            let block = &main[start..starts.get(n + 1).copied().unwrap_or(main.len())];
            let mut parts = block.split('"');
            Block {
                kind: parts.nth(1).unwrap_or_default().to_string(),
                label: parts.nth(1).unwrap_or_default().to_string(),
                body: block.to_string(),
            }
        })
        .collect()
}

/// The custom role or definition a binding names, as `<type>.<label>`.
fn custom_role<'a>(blocks: &'a [Block], binding: &Block, kind: &str) -> Option<&'a Block> {
    blocks
        .iter()
        .filter(|block| block.kind == kind)
        .find(|block| {
            let reference = format!("{kind}.{}", block.label);
            binding.body.contains(&format!("{reference}["))
                || binding.body.contains(&format!("{reference}."))
        })
}

/// GCP: the predefined roles that read secret values, and the permission that does.
const GCP_SECRET_READ_ROLES: &[&str] = &[
    "\"roles/secretmanager.secretAccessor\"",
    "\"roles/secretmanager.admin\"",
];
const GCP_SECRET_READ_PERMISSION: &str = "\"secretmanager.versions.access\"";

/// Labels of the project bindings that let `member` read secret values.
fn gcp_secret_reads(module: &ModuleFiles, member: &str) -> Vec<String> {
    let blocks = resource_blocks(module);
    blocks
        .iter()
        .filter(|block| block.kind == "google_project_iam_member" && block.body.contains(member))
        .filter(|binding| {
            GCP_SECRET_READ_ROLES
                .iter()
                .any(|role| binding.body.contains(role))
                || custom_role(&blocks, binding, "google_project_iam_custom_role")
                    .is_some_and(|role| role.body.contains(GCP_SECRET_READ_PERMISSION))
        })
        .map(|binding| binding.label.clone())
        .collect()
}

/// Azure built-in roles that read Key Vault secret values: Secrets User, Secrets Officer and
/// Key Vault Administrator.
const AZURE_SECRET_READ_ROLES: &[&str] = &[
    "4633458b-17de-408a-b874-0445c86b69e6",
    "b86a8fe4-44ce-4948-aee5-eccb2c155cd7",
    "00482a5a-887f-4fb3-b363-3b7fe8e74483",
];

/// Whether a custom role definition's data actions include `action` (or a wildcard over it).
fn azure_definition_allows(definition: &Block, action: &str) -> bool {
    let data_actions = definition
        .body
        .split("data_actions = [")
        .nth(1)
        .and_then(|rest| rest.split(']').next())
        .unwrap_or_default();
    data_actions.split(',').any(|granted| {
        let granted = granted.trim().trim_matches('"');
        granted == action
            || granted
                .strip_suffix('*')
                .is_some_and(|prefix| action.starts_with(prefix))
    })
}

const AZURE_GET_SECRET: &str = "Microsoft.KeyVault/vaults/secrets/getSecret/action";
const AZURE_SET_SECRET: &str = "Microsoft.KeyVault/vaults/secrets/setSecret/action";

/// Labels of the role assignments that let `principal` do `action` on Key Vault secrets.
fn azure_secret_grants(module: &ModuleFiles, principal: &str, action: &str) -> Vec<String> {
    let blocks = resource_blocks(module);
    blocks
        .iter()
        .filter(|block| {
            block.kind == "azurerm_role_assignment"
                && block.body.contains(&format!("principal_id = {principal}"))
        })
        .filter(|assignment| {
            (action == AZURE_GET_SECRET
                && AZURE_SECRET_READ_ROLES
                    .iter()
                    .any(|role| assignment.body.contains(role)))
                || custom_role(&blocks, assignment, "azurerm_role_definition")
                    .is_some_and(|definition| azure_definition_allows(definition, action))
        })
        .map(|assignment| assignment.label.clone())
        .collect()
}

#[tokio::test]
async fn gcp_management_writes_but_cannot_read_the_secrets_vault() {
    let prepared = prepare(stack(), Platform::Gcp).await;
    let module = render(&prepared, TerraformTarget::Gcp, StackSettings::default());
    assert_terraform_valid(&module, "gcp_secrets_vault_management");

    let management = "serviceAccount:${google_service_account.management.email}";
    let reads = gcp_secret_reads(&module, management);
    assert!(
        reads.is_empty(),
        "management may not read secret values: {reads:?}"
    );
    let blocks = resource_blocks(&module);
    assert!(
        blocks
            .iter()
            .any(|block| block.kind == "google_project_iam_member"
                && block.body.contains(management)
                && block
                    .body
                    .contains("\"roles/secretmanager.secretVersionAdder\"")
                && block.body.contains("${local.resource_prefix}-secrets-")),
        "management keeps adding versions to the secrets vault"
    );
    let workload = gcp_secret_reads(
        &module,
        "serviceAccount:${google_service_account.execution_sa.email}",
    );
    assert!(!workload.is_empty(), "the workload keeps its read");
}

#[tokio::test]
async fn azure_management_writes_but_cannot_read_the_secrets_vault() {
    let prepared = prepare(stack(), Platform::Azure).await;
    let module = render(&prepared, TerraformTarget::Azure, StackSettings::default());
    assert_terraform_valid(&module, "azure_secrets_vault_management");

    let management = "azurerm_user_assigned_identity.management.principal_id";
    let reads = azure_secret_grants(&module, management, AZURE_GET_SECRET);
    assert!(
        reads.is_empty(),
        "management may not read secret values: {reads:?}"
    );
    let writes = azure_secret_grants(&module, management, AZURE_SET_SECRET);
    assert!(
        !writes.is_empty(),
        "management keeps writing the secrets vault"
    );
    let workload = azure_secret_grants(
        &module,
        "azurerm_user_assigned_identity.execution_sa.principal_id",
        AZURE_GET_SECRET,
    );
    assert!(!workload.is_empty(), "the workload keeps its read");
}

/// Azure: management assigns roles to the workload identities it runs, so it holds
/// `roleAssignments/write`. It must not be able to use that to give itself a read on the
/// secrets vault, nor add `getSecret` to a role it holds. So:
/// - every unconditioned role management holds leaves out role-assignment writes;
/// - the role that has them is assigned with a condition that excludes management itself;
/// - every custom role management holds is also assignable at the subscription, where it
///   cannot change role definitions.
#[tokio::test]
async fn azure_management_cannot_assign_itself_roles_or_change_its_own() {
    let prepared = prepare(stack(), Platform::Azure).await;
    let module = render(&prepared, TerraformTarget::Azure, StackSettings::default());
    assert_terraform_valid(&module, "azure_management_role_assignments");
    let blocks = resource_blocks(&module);
    let management = "principal_id = azurerm_user_assigned_identity.management.principal_id";
    let writes = |block: &Block| {
        block
            .body
            .contains("\"Microsoft.Authorization/roleAssignments/write\"")
            || block
                .body
                .contains("\"Microsoft.Authorization/roleAssignments/*\"")
            || block.body.contains("\"Microsoft.Authorization/*\"")
            || block.body.contains("actions = [ \"*\"")
    };

    let mut conditioned = 0;
    for assignment in blocks
        .iter()
        .filter(|block| block.kind == "azurerm_role_assignment" && block.body.contains(management))
    {
        let Some(definition) = custom_role(&blocks, assignment, "azurerm_role_definition") else {
            continue;
        };
        assert!(
            definition
                .body
                .contains("\"/subscriptions/${var.azure_subscription_id}\" ]"),
            "{} is assignable only where management can change it: {}",
            definition.label,
            definition.body
        );
        if writes(definition) {
            assert!(
                assignment.body.contains(
                    "@Request[Microsoft.Authorization/roleAssignments:PrincipalId] ForAnyOfAllValues:GuidNotEquals {${azurerm_user_assigned_identity.management.principal_id}}"
                ) && assignment.body.contains("condition_version = \"2.0\""),
                "{} lets management assign roles without excluding itself: {}",
                assignment.label,
                assignment.body
            );
            conditioned += 1;
        }
    }
    assert!(
        conditioned > 0,
        "management still assigns roles to workloads"
    );
}
