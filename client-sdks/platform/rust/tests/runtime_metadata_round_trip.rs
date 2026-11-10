//! The Platform API stores a deployment's runtime metadata and hands it back through this
//! client. A field the vendored spec does not know is silently dropped on decode, so a field
//! added to `alien_core::RuntimeMetadata` without refreshing `openapi.json` loses durable
//! state. These tests send fully populated values through the generated types and require
//! them back unchanged.

use alien_core::{
    CloudSetupRequired, DeployerSecretLocation, DeployerSecretReport, DeployerSecretStatus,
    DeployerSecretStore, InstalledHelmRelease, RuntimeMetadata, SetupReportRefusal,
    SetupUpdateAuthorization,
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
            commands: None,
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

// What a Helm Operator's installed-setup report leaves on a deployment.
fn helm_setup_update_metadata() -> RuntimeMetadata {
    RuntimeMetadata {
        setup_update_authorization: Some(SetupUpdateAuthorization {
            nonce: "nonce-1".to_string(),
            baseline_frozen_digest: "before".to_string(),
            target_frozen_digest: "after".to_string(),
            release_id: "rel_aaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
            setup_target: "kubernetes".to_string(),
            setup_fingerprint: "fingerprint".to_string(),
            setup_fingerprint_version: 1,
        }),
        setup_report_refusal: Some(SetupReportRefusal {
            reason: "Release rel_b is unknown.".to_string(),
            reported_release_id: "rel_bbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string(),
            at: "2026-11-10T00:00:00Z".parse().expect("timestamp parses"),
        }),
        cloud_setup_required: Some(CloudSetupRequired {
            release_id: "rel_cccccccccccccccccccccccccccc".to_string(),
            reason: "This release adds or changes 'assets', which lives in your cloud account"
                .to_string(),
        }),
        installed_helm_release: Some(InstalledHelmRelease {
            name: "my-app".to_string(),
            namespace: "my-app".to_string(),
        }),
        ..RuntimeMetadata::default()
    }
}

// A manager reports deployment state in `SyncReconcileRequest.state` on every sync. Before
// sending, it refuses a conversion that would lose any field, so a field missing from the
// spec makes every state write with that field fail. Only the full SDK has the reconcile
// request.
#[cfg(feature = "full-api")]
#[test]
fn reconcile_request_keeps_the_helm_setup_update_fields() {
    use alien_core::{DeploymentState, DeploymentStatus, Platform};

    let source = DeploymentState {
        status: DeploymentStatus::Running,
        platform: Platform::Kubernetes,
        current_release: None,
        target_release: None,
        stack_state: None,
        error: None,
        environment_info: None,
        runtime_metadata: Some(helm_setup_update_metadata()),
        retry_requested: false,
        protocol_version: 1,
    };
    let source = serde_json::to_value(&source).expect("deployment state serializes");

    let request: alien_platform_api::types::SyncReconcileRequest = serde_json::from_value(json!({
        "deploymentId": "dep_aaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "state": source.clone(),
    }))
    .expect("reconcile request decodes");
    let encoded = serde_json::to_value(&request).expect("reconcile request encodes");
    let round_tripped: DeploymentState = serde_json::from_value(
        encoded
            .get("state")
            .cloned()
            .expect("reconcile request keeps its state"),
    )
    .expect("deployment state decodes");

    assert_eq!(
        serde_json::to_value(round_tripped).expect("deployment state serializes"),
        source
    );
}

#[test]
fn deployment_keeps_the_helm_setup_update_fields() {
    assert_eq!(
        deployment_round_trip(helm_setup_update_metadata()),
        helm_setup_update_metadata()
    );
}

#[test]
fn deployment_keeps_every_deployer_secret_report_field() {
    let source = RuntimeMetadata {
        deployer_secrets: vec![fully_populated_deployer_secret()],
        ..RuntimeMetadata::default()
    };
    assert_eq!(deployment_round_trip(source.clone()), source);
}

fn deployment_round_trip(source: RuntimeMetadata) -> RuntimeMetadata {
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
    serde_json::from_value(
        encoded
            .get("runtimeMetadata")
            .cloned()
            .expect("deployment keeps its runtime metadata"),
    )
    .expect("runtime metadata decodes")
}
