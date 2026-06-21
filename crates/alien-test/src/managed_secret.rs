use std::sync::Arc;

use alien_core::{Platform, StackState, VaultOutputs};
use anyhow::Context;
use tokio::process::Command;
use tracing::info;

use crate::{
    azure_sdk::{AzureArmClient, RoleAssignment, RoleAssignmentProperties, Scope},
    config::AzureConfig,
    deployment::TestDeployment,
    manager::TestManager,
};

/// Provision the `MANAGED_TEST_SECRET` in the deployment's preflight-managed
/// `secrets` vault via the manager's vault API. This is an Alien-managed test
/// secret, not a customer-owned external vault secret.
pub(crate) async fn provision_managed_test_secret(
    manager: &Arc<TestManager>,
    deployment: &TestDeployment,
) -> anyhow::Result<()> {
    ensure_local_azure_manager_vault_access(manager, deployment, "secrets").await?;

    let http = manager.http_client();
    let vault_name = "secrets";
    let secret_key = "MANAGED_TEST_SECRET";
    let secret_value = "e2e-test-managed-secret-value";

    let url = format!(
        "{}/v1/deployments/{}/vault/{}/secrets/{}",
        manager.url, deployment.id, vault_name, secret_key,
    );

    info!(
        deployment_id = %deployment.id,
        vault_name,
        secret_key,
        "Provisioning managed test secret via manager vault API"
    );

    let resp = http
        .put(&url)
        .json(&serde_json::json!({ "value": secret_value }))
        .send()
        .await
        .context("Failed to call vault set secret API")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!(
            "Failed to provision managed test secret ({}): {}",
            status,
            body
        );
    }

    info!(
        deployment_id = %deployment.id,
        "Managed test secret provisioned"
    );

    Ok(())
}

async fn ensure_local_azure_manager_vault_access(
    manager: &Arc<TestManager>,
    deployment: &TestDeployment,
    vault_name: &str,
) -> anyhow::Result<()> {
    if deployment.platform != Platform::Azure.as_str() || !is_local_azure_target_resolver_mode() {
        return Ok(());
    }

    let Some(config) = manager.test_config() else {
        return Ok(());
    };
    let Some(target) = config.azure_target.as_ref() else {
        return Ok(());
    };

    let principal_id = azure_target_principal_id(target).await?;
    let vault_scope = azure_vault_scope_for_deployment(manager, deployment, vault_name).await?;
    let azure_config = alien_core::AzureClientConfig {
        subscription_id: target.subscription_id.clone(),
        tenant_id: target.tenant_id.clone(),
        region: Some(target.region.clone()),
        credentials: alien_core::AzureCredentials::ServicePrincipal {
            client_id: target.client_id.clone(),
            client_secret: target.client_secret.clone(),
        },
        service_overrides: None,
    };

    let arm_client = AzureArmClient::new(azure_config.clone())?;
    let scope_text = vault_scope.to_resource_id_string(&azure_config);

    // The local OSS Azure harness can run the manager with the target service
    // principal when no workload identity token is available. Azure Key Vault
    // uses data-plane RBAC, so ARM owner/contributor permissions are not enough
    // for the manager vault API to set test secrets.
    for (role_name, role_id) in [
        (
            "Key Vault Secrets Officer",
            "b86a8fe4-44ce-4948-aee5-eccb2c155cd7",
        ),
        (
            "Key Vault Secrets User",
            "4633458b-17de-408a-b874-0445c86b69e6",
        ),
    ] {
        let role_definition_id = format!(
            "/subscriptions/{}/providers/Microsoft.Authorization/roleDefinitions/{}",
            target.subscription_id, role_id
        );
        let assignment_id = uuid::Uuid::new_v5(
            &uuid::Uuid::NAMESPACE_OID,
            format!(
                "alien:e2e:local-azure-manager-vault:{}:{}:{}:{}",
                deployment.id, vault_name, principal_id, role_id
            )
            .as_bytes(),
        )
        .to_string();
        let full_assignment_id = arm_client.role_assignment_id(&vault_scope, assignment_id);

        arm_client
            .create_or_update_role_assignment(
                full_assignment_id,
                &RoleAssignment {
                    id: None,
                    name: None,
                    type_: None,
                    properties: RoleAssignmentProperties {
                        principal_id: principal_id.clone(),
                        role_definition_id,
                        scope: scope_text.clone(),
                        principal_type: "ServicePrincipal".to_string(),
                        condition: None,
                        condition_version: None,
                        delegated_managed_identity_resource_id: None,
                        description: Some("E2E local Azure manager vault API access".to_string()),
                    },
                },
            )
            .await
            .with_context(|| {
                format!(
                    "Failed to grant {role_name} to local Azure manager identity on {scope_text}"
                )
            })?;
    }

    info!(
        deployment_id = %deployment.id,
        vault_name,
        principal_id = %principal_id,
        scope = %scope_text,
        "Granted local Azure manager vault API access"
    );

    Ok(())
}

fn is_local_azure_target_resolver_mode() -> bool {
    std::env::var("AZURE_FEDERATED_TOKEN_FILE")
        .ok()
        .filter(|value| !value.is_empty())
        .is_none()
        && !(std::env::var("IDENTITY_ENDPOINT").is_ok() && std::env::var("IDENTITY_HEADER").is_ok())
}

async fn azure_target_principal_id(target: &AzureConfig) -> anyhow::Result<String> {
    if let Some(principal_id) = target.principal_id.as_ref() {
        return Ok(principal_id.clone());
    }

    let output = Command::new("az")
        .args([
            "ad",
            "sp",
            "show",
            "--id",
            &target.client_id,
            "--query",
            "id",
            "-o",
            "tsv",
        ])
        .output()
        .await
        .context(
            "Failed to resolve AZURE_TARGET_PRINCIPAL_ID with Azure CLI; set AZURE_TARGET_PRINCIPAL_ID or install/login az",
        )?;

    if !output.status.success() {
        anyhow::bail!(
            "Failed to resolve AZURE_TARGET_PRINCIPAL_ID with Azure CLI: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    let principal_id = String::from_utf8(output.stdout)
        .context("Azure CLI returned a non-UTF8 service principal id")?
        .trim()
        .to_string();
    if principal_id.is_empty() {
        anyhow::bail!("Azure CLI returned an empty service principal id");
    }
    Ok(principal_id)
}

async fn azure_vault_scope_for_deployment(
    manager: &Arc<TestManager>,
    deployment: &TestDeployment,
    vault_name: &str,
) -> anyhow::Result<Scope> {
    let deployment_record = manager
        .client()
        .get_deployment()
        .id(&deployment.id)
        .send()
        .await
        .map_err(|error| anyhow::anyhow!("Failed to load deployment after running: {error}"))?
        .into_inner();
    let state_value = deployment_record
        .stack_state
        .context("Deployment has no stack_state after reaching running")?;
    let stack_state: StackState = serde_json::from_value(state_value)
        .context("Failed to deserialize deployment stack_state")?;
    let vault_outputs = stack_state
        .get_resource_outputs::<VaultOutputs>(vault_name)
        .with_context(|| {
            format!("Deployment stack_state has no outputs for vault '{vault_name}'")
        })?;

    azure_key_vault_scope_from_resource_id(&vault_outputs.vault_id)
}

fn azure_key_vault_scope_from_resource_id(resource_id: &str) -> anyhow::Result<Scope> {
    let segments: Vec<&str> = resource_id
        .trim_matches('/')
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect();

    let resource_group = segments
        .windows(2)
        .find(|window| window[0].eq_ignore_ascii_case("resourceGroups"))
        .map(|window| window[1].to_string())
        .with_context(|| format!("Key Vault resource id missing resource group: {resource_id}"))?;
    let vault_name = segments
        .windows(2)
        .find(|window| window[0].eq_ignore_ascii_case("vaults"))
        .map(|window| window[1].to_string())
        .with_context(|| format!("Key Vault resource id missing vault name: {resource_id}"))?;

    Ok(Scope::Resource {
        resource_group_name: resource_group,
        resource_provider: "Microsoft.KeyVault".to_string(),
        parent_resource_path: None,
        resource_type: "vaults".to_string(),
        resource_name: vault_name,
    })
}
