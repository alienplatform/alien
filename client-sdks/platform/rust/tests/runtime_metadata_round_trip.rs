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
        "protected": false,
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

// A manager writes `DeploymentState` through the generated `DeploymentState` type on every
// checkpoint and refuses the write when the conversion would drop anything. This builds a
// state with every optional field set and every list non-empty, so a field added to
// `alien_core` and not to the vendored `openapi.json` fails here instead of in a running
// manager. Struct literals on purpose: a new field stops this compiling until it is set.
#[cfg(feature = "full-api")]
fn fully_populated_state() -> alien_core::DeploymentState {
    use alien_core::{
        AiAccessCheck, AiAccessCheckResult, AirgappedReportMetadata, AwsSandboxEgressScaffolding,
        CredentialRetry, DeletionPolicy, DeploymentState, DeploymentStatus, InitialSetupAuthority,
        Platform, RegistryAccess, ReleaseInfo, SetupScaffolding,
    };
    let at: chrono::DateTime<chrono::Utc> = "2026-11-10T00:00:00Z".parse().unwrap();
    let stack: alien_core::Stack = serde_json::from_value(json!({
        "id": "demo",
        "resources": {},
        "supportedPlatforms": ["aws"],
        "dynamicContainerRepositories": ["repo"],
    }))
    .expect("stack decodes");
    let metadata = RuntimeMetadata {
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
        deletion_policy: DeletionPolicy::Erase,
        airgapped_report: Some(AirgappedReportMetadata {
            written_at: Some(at),
            received_at: at,
        }),
        operator_version: Some("3.3.61".to_string()),
        credential_retry: Some(CredentialRetry {
            started_at: at,
            status: DeploymentStatus::Provisioning,
        }),
        initial_setup_authority: InitialSetupAuthority::default(),
        automatic_setup: true,
        last_synced_env_vars_hash: Some("hash".to_string()),
        last_synced_secret_names: vec!["DB_PASSWORD".to_string()],
        vault_credentials_revoked: true,
        retained_storage_protection_hash: Some("hash".to_string()),
        retained_storage_protection_confirmed_at: Some(at),
        retained_data_protection_started_at: Some(at),
        prepared_stack: Some(stack.clone()),
        persisted_gate_answers: [("gate".to_string(), true)].into_iter().collect(),
        pending_prepared_stack: Some(stack.clone()),
        pending_prepared_release_id: Some("rel_dddddddddddddddddddddddddddd".to_string()),
        direct_setup_revision: Some("rev".to_string()),
        registry_access_granted: true,
        registry_access: Some(RegistryAccess {
            repositories: vec!["repo".to_string()],
            service_types: vec!["lambda".to_string()],
            verifier: Some("verifier".to_string()),
        }),
        setup_scaffolding: [(
            "worker".to_string(),
            SetupScaffolding::AwsSandbox {
                build_role_name: "build".to_string(),
                egress: Some(AwsSandboxEgressScaffolding {
                    operator_role_name: "operator".to_string(),
                    security_group_id: Some("sg-1".to_string()),
                    connector_arn: Some("arn:aws:lambda:us-east-1:1:connector".to_string()),
                    connector_request: Some("request".to_string()),
                }),
                image_arn: Some("arn:aws:lambda:us-east-1:1:image".to_string()),
            },
        )]
        .into_iter()
        .collect(),
        deployer_secrets: vec![fully_populated_deployer_secret()],
        ai_access_checks: vec![AiAccessCheck {
            resource_id: "ai".to_string(),
            model_id: "claude-sonnet-5".to_string(),
            result: AiAccessCheckResult::Refused,
            detail: Some("not enabled".to_string()),
            tested_at: at,
        }],
        deployer_secrets_checked_at: Some(at),
        resources_waiting_for_secrets: vec!["reader".to_string()],
    };

    let release = ReleaseInfo {
        release_id: Some("rel_eeeeeeeeeeeeeeeeeeeeeeeeeeee".to_string()),
        version: Some("1.0.0".to_string()),
        description: Some("release".to_string()),
        stack,
        signed: None,
        signer_lists: Vec::new(),
    };
    DeploymentState {
        status: DeploymentStatus::Updating,
        platform: Platform::Aws,
        current_release: Some(release.clone()),
        target_release: Some(release),
        stack_state: Some(
            serde_json::from_value(json!({
                "platform": "aws", "resourcePrefix": "demo", "resources": {}
            }))
            .expect("stack state decodes"),
        ),
        error: Some(
            serde_json::from_value(json!({
                "code": "SOME_ERROR", "message": "failed", "retryable": true,
                "internal": false, "httpStatusCode": 500, "context": {"k": "v"}
            }))
            .expect("error decodes"),
        ),
        environment_info: Some(alien_core::EnvironmentInfo::Aws(
            serde_json::from_value(json!({"accountId": "123", "region": "us-east-1"}))
                .expect("environment decodes"),
        )),
        runtime_metadata: Some(metadata),
        retry_requested: true,
        protocol_version: 1,
    }
}

#[cfg(feature = "full-api")]
#[test]
fn fully_populated_deployment_state_survives_the_reconcile_request() {
    let source = serde_json::to_value(fully_populated_state()).expect("state serializes");
    let request: alien_platform_api::types::SyncReconcileRequest = serde_json::from_value(json!({
        "deploymentId": "dep_aaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "state": source.clone(),
    }))
    .expect("reconcile request decodes");
    let encoded = serde_json::to_value(&request).expect("reconcile request encodes");
    let round_tripped: alien_core::DeploymentState =
        serde_json::from_value(encoded["state"].clone()).expect("core state decodes");
    assert_eq!(
        serde_json::to_value(round_tripped).expect("core state serializes"),
        source
    );
}
