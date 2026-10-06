//! The Platform API stores a deployment's runtime metadata and hands it back through this
//! client. A field the vendored spec does not know is silently dropped on decode, so a field
//! added to `alien_core::RuntimeMetadata` without refreshing `openapi.json` loses durable
//! state. These tests send fully populated values through the generated types and require
//! them back unchanged.

use alien_core::{
    DeployerSecretLocation, DeployerSecretReport, DeployerSecretStatus, DeployerSecretStore,
    RuntimeMetadata,
};
use alien_platform_api::types::Deployment;
use serde_json::json;

// Struct literals on purpose: adding a field to either type stops this from compiling until
// the field is set here, and then the round trip checks the spec carries it.
fn fully_populated_deployer_secret() -> DeployerSecretReport {
    DeployerSecretReport {
        input_id: "databasePassword".to_string(),
        label: "Database password".to_string(),
        required: true,
        status: DeployerSecretStatus::Invalid,
        message: Some("must be a SecureString".to_string()),
        version: Some("3".to_string()),
        location: DeployerSecretLocation {
            store: DeployerSecretStore::AzureKeyVault,
            name: "my-app-database-password".to_string(),
            vault_name: Some("my-app-vault".to_string()),
            console_url: Some("https://portal.azure.com/#secret".to_string()),
            cli_command: "az keyvault secret set --vault-name my-app-vault --name my-app-database-password --value <VALUE>".to_string(),
            delete_command: Some(
                "az keyvault secret delete --vault-name my-app-vault --name my-app-database-password"
                    .to_string(),
            ),
        },
    }
}

#[test]
fn deployment_keeps_every_deployer_secret_report_field() {
    let source = RuntimeMetadata {
        deployer_secrets: vec![fully_populated_deployer_secret()],
        ..RuntimeMetadata::default()
    };

    let deployment: Deployment = serde_json::from_value(json!({
        "id": "dep_aaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "name": "my-app",
        "status": "running",
        "projectId": "prj_aaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "platform": "azure",
        "deploymentProtocolVersion": 1,
        "deploymentGroupId": "dg_aaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "purpose": "application",
        "stackSettings": {},
        "releaseChannel": "stable",
        "retryRequested": false,
        "createdAt": "2026-10-06T00:00:00Z",
        "updatedAt": "2026-10-06T00:00:00Z",
        "managerId": "mgr_aaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "workspaceId": "ws_AAAAAAAAAAAAAAAAAAAAAAAA",
        "runtimeMetadata": serde_json::to_value(&source).expect("runtime metadata serializes"),
    }))
    .expect("deployment decodes");

    let encoded = serde_json::to_value(&deployment).expect("deployment encodes");
    let round_tripped: RuntimeMetadata = serde_json::from_value(
        encoded
            .get("runtimeMetadata")
            .cloned()
            .expect("deployment keeps its runtime metadata"),
    )
    .expect("runtime metadata decodes");

    assert_eq!(round_tripped, source);
}
