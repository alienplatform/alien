//! Integration tests for the OSS importer registry.
//!
//! Each test feeds a wire-format JSON payload (the same shape the manager's
//! `/v1/stack/import` route receives) through `ImporterRegistry::built_in()`
//! → `ImporterRegistry::run` → typed `ImportData` → importer → typed
//! `StackResourceState`. The assertions cover the contract everything
//! downstream of the importer cares about:
//!
//! * `status == Running` for resources that are fully imported at their
//!   controller terminal state.
//! * `status == Provisioning` for imported setup resources that still need a
//!   controller-owned propagation wait before live resources can start.
//! * `internal_state.type` — the type tag injected by `serialize_controller`
//!   must round-trip through `deserialize_controller` (the manager calls
//!   this on every reconcile tick).
//!
//! There is also a registry-walk test that asserts every `(resource_type,
//! cloud) ∈ {storage, kv, vault, queue, network, service-account,
//! remote-stack-management, build, artifact-registry, function} × {Aws, Gcp,
//! Azure}` (plus GCP `service_activation`, plus the four Azure aux
//! resources) is registered. `container` and `compute-cluster` are
//! deliberately *not* asserted — embedders register those controllers
//! separately.

use alien_core::import::{
    data::{
        AwsAiImportData, AwsKeyImportData, AwsKvImportData, AwsRemoteBindingsImportData,
        AwsRemoteStackManagementImportData, AwsSandboxImportData, AwsServiceAccountImportData,
        AwsStorageImportData, AzureAiImportData, AzureContainerAppsEnvironmentImportData,
        AzureKeyImportData, AzureRemoteBindingsImportData, AzureRemoteStackManagementImportData,
        AzureResourceGroupImportData, AzureServiceAccountImportData, AzureStorageAccountImportData,
        AzureStorageImportData, GcpAiImportData, GcpBuildImportData, GcpKeyImportData,
        GcpKvImportData, GcpNetworkImportData, GcpRemoteBindingsImportData,
        GcpRemoteStackManagementImportData, GcpServiceActivationImportData, GcpStorageImportData,
        KubernetesClusterImportData,
    },
    ImportContext,
};
use alien_core::{
    Ai, ArtifactRegistry, AwsManagementConfig, AwsOpenSearch, AwsOpenSearchOutputs,
    AzureContainerAppsEnvironment, AzureContainerAppsEnvironmentOutputs, AzureManagementConfig,
    AzureResourceGroup, AzureResourceGroupOutputs, AzureServiceBusNamespace, AzureStorageAccount,
    AzureStorageAccountOutputs, Build, Email, EmailInbound, EmailOutputs, GcpManagementConfig, Key,
    KeyFingerprint, KeyOutputs, KubernetesCluster, KubernetesClusterOutputs,
    KubernetesClusterOwnership, KubernetesClusterProvider, KubernetesHeartbeatMode, Kv,
    ManagementConfig, Network, NetworkSettings, Platform, Queue, RemoteBindings,
    RemoteBindingsOutputs, RemoteStackManagement, RemoteStackManagementOutputs, Resource,
    ResourceDefinition, ResourceEntry, ResourceLifecycle, ResourceRef, ResourceStatus,
    ResourceType, Sandbox, SandboxCode, SandboxEgress, SandboxSessionPolicy, ServiceAccount,
    ServiceActivation, StackSettings, Storage, Vault, Worker,
};
use alien_infra::{ImporterRegistry, StackResourceStateExt};
use serde_json::json;
use std::collections::HashMap;

#[path = "importers/remote_stack_management.rs"]
mod remote_stack_management;

/// Build a `ResourceEntry` whose `config` is `T`. The importer reads
/// `ctx.resource.config` to derive the resource_type written into the
/// returned `StackResourceState`.
fn entry<T: ResourceDefinition>(resource: T) -> ResourceEntry {
    ResourceEntry {
        config: Resource::new(resource),
        lifecycle: ResourceLifecycle::Live,
        dependencies: vec![],
        remote_access: false,
        enabled_when: None,
    }
}

fn remote_entry<T: ResourceDefinition>(resource: T) -> ResourceEntry {
    ResourceEntry {
        config: Resource::new(resource),
        lifecycle: ResourceLifecycle::Live,
        dependencies: vec![],
        remote_access: true,
        enabled_when: None,
    }
}

fn frozen_entry<T: ResourceDefinition>(resource: T) -> ResourceEntry {
    ResourceEntry {
        config: Resource::new(resource),
        lifecycle: ResourceLifecycle::Frozen,
        dependencies: vec![],
        remote_access: false,
        enabled_when: None,
    }
}

fn aws_management_config() -> ManagementConfig {
    ManagementConfig::Aws(AwsManagementConfig {
        managing_role_arn: "arn:aws:iam::123456789012:role/alien-manager".to_string(),
    })
}

fn gcp_management_config() -> ManagementConfig {
    ManagementConfig::Gcp(GcpManagementConfig {
        service_account_email: "alien-manager@my-project.iam.gserviceaccount.com".to_string(),
    })
}

fn azure_management_config() -> ManagementConfig {
    ManagementConfig::Azure(AzureManagementConfig {
        managing_tenant_id: "00000000-0000-0000-0000-000000000000".to_string(),
        oidc_issuer: "https://issuer.example".to_string(),
        oidc_subject: "system:serviceaccount:alien:manager".to_string(),
    })
}

fn settings() -> StackSettings {
    StackSettings::default()
}

/// Run the full registry path: wire JSON → typed payload → importer →
/// `StackResourceState`. This is the same code the `/v1/stack/import` route
/// will exercise; tests at this layer give us a real round-trip including
/// the `serde_json::from_value` step.
fn run_through_registry(
    resource_type: &ResourceType,
    platform: Platform,
    payload: serde_json::Value,
    entry: &ResourceEntry,
    region: &str,
    management: &ManagementConfig,
) -> alien_core::StackResourceState {
    let registry = ImporterRegistry::built_in();
    let settings = settings();
    let ctx = ImportContext {
        resource_id: "test-resource",
        platform,
        region,
        stack_settings: &settings,
        management_config: Some(management),
        resource: entry,
    };
    registry
        .run(resource_type, platform, payload, &ctx)
        .expect("import should succeed")
}

fn internal_state(state: &alien_core::StackResourceState) -> &serde_json::Value {
    state
        .internal_state
        .as_ref()
        .expect("imported resource must have internal_state set")
}

fn controller_binding_params(state: &alien_core::StackResourceState) -> serde_json::Value {
    state
        .get_internal_controller()
        .expect("imported controller must deserialize")
        .expect("imported resource must have an internal controller")
        .get_binding_params()
        .expect("imported controller binding params must serialize")
        .expect("imported controller must produce binding params")
}

fn assert_running_with_internal_state(state: &alien_core::StackResourceState) {
    assert_eq!(
        state.status,
        ResourceStatus::Running,
        "imported resource must start at Running so the loop's heartbeat path runs immediately"
    );
    let internal = internal_state(state)
        .as_object()
        .expect("internal_state must serialize as object");
    assert!(
        internal.contains_key("type"),
        "serialize_controller must inject a `type` discriminator (controller deserialization depends on it). \
         got keys: {:?}",
        internal.keys().collect::<Vec<_>>()
    );
}

fn assert_provisioning_with_internal_state(state: &alien_core::StackResourceState) {
    assert_eq!(
        state.status,
        ResourceStatus::Provisioning,
        "imported setup resource must finish controller-owned propagation before live provisioning"
    );
    let internal = internal_state(state)
        .as_object()
        .expect("internal_state must serialize as object");
    assert!(
        internal.contains_key("type"),
        "serialize_controller must inject a `type` discriminator (controller deserialization depends on it). \
         got keys: {:?}",
        internal.keys().collect::<Vec<_>>()
    );
}

#[test]
fn kubernetes_cluster_handoff_imports_as_running() {
    let entry = frozen_entry(
        KubernetesCluster::new("kubernetes".to_string())
            .provider(KubernetesClusterProvider::Eks)
            .ownership(KubernetesClusterOwnership::Managed)
            .namespace("alien-test".to_string())
            .heartbeat_mode(KubernetesHeartbeatMode::KubernetesApiAndCloudMetadata)
            .build(),
    );
    let data = KubernetesClusterImportData {
        provider: KubernetesClusterProvider::Eks,
        ownership: KubernetesClusterOwnership::Managed,
        namespace: "alien-test".to_string(),
        cluster_name: Some("alien-e2e-a2591da2".to_string()),
        cluster_id: Some("alien-e2e-a2591da2".to_string()),
        cloud_metadata_ready: Some(true),
        azure_application_gateway_for_containers: None,
    };
    let state = run_through_registry(
        &KubernetesCluster::RESOURCE_TYPE,
        Platform::Kubernetes,
        serde_json::to_value(&data).unwrap(),
        &entry,
        "us-east-2",
        &aws_management_config(),
    );

    assert_running_with_internal_state(&state);
    let outputs = state
        .outputs
        .as_ref()
        .and_then(|outputs| outputs.downcast_ref::<KubernetesClusterOutputs>())
        .expect("KubernetesCluster import must expose typed outputs");
    assert!(outputs.kubernetes_api_reachable);
    assert!(outputs.namespace_ready);
    assert!(outputs.rbac_ready);
    assert!(!outputs.operator_ready);
    assert_eq!(outputs.cloud_metadata_ready, Some(true));
}

#[test]
fn aws_storage_round_trip() {
    let entry = entry(Storage::new("my-bucket".to_string()).build());
    let data = AwsStorageImportData {
        bucket_name: "alien-stack-my-bucket".to_string(),
        bucket_arn: "arn:aws:s3:::alien-stack-my-bucket".to_string(),
    };
    let state = run_through_registry(
        &Storage::RESOURCE_TYPE,
        Platform::Aws,
        serde_json::to_value(&data).unwrap(),
        &entry,
        "us-east-1",
        &aws_management_config(),
    );

    assert_running_with_internal_state(&state);
    assert_eq!(
        internal_state(&state)["bucketName"],
        "alien-stack-my-bucket"
    );
}

#[test]
fn aws_kv_round_trip() {
    let entry = entry(Kv::new("settings".to_string()).build());
    let data = AwsKvImportData {
        table_name: "alien-stack-settings".to_string(),
        table_arn: "arn:aws:dynamodb:us-east-1:123456789012:table/alien-stack-settings".to_string(),
    };
    let state = run_through_registry(
        &Kv::RESOURCE_TYPE,
        Platform::Aws,
        serde_json::to_value(&data).unwrap(),
        &entry,
        "us-east-1",
        &aws_management_config(),
    );
    assert_running_with_internal_state(&state);
    assert_eq!(internal_state(&state)["tableName"], "alien-stack-settings");
}

#[test]
fn aws_service_account_round_trip() {
    let entry = entry(ServiceAccount::new("execution".to_string()).build());
    let data = AwsServiceAccountImportData {
        role_arn: "arn:aws:iam::123456789012:role/alien-stack-execution".to_string(),
        role_name: "alien-stack-execution".to_string(),
        stack_permissions_applied: true,
    };
    let state = run_through_registry(
        &ServiceAccount::RESOURCE_TYPE,
        Platform::Aws,
        serde_json::to_value(&data).unwrap(),
        &entry,
        "us-east-1",
        &aws_management_config(),
    );
    assert_running_with_internal_state(&state);
    let internal = internal_state(&state);
    assert_eq!(
        internal["roleArn"],
        "arn:aws:iam::123456789012:role/alien-stack-execution"
    );
    assert_eq!(internal["stackPermissionsApplied"], true);
}

/// A fully wired email resource (seed domain + inbound) imports as Running
/// with typed [`EmailOutputs`] carrying exactly what the setup stack handed
/// over: DKIM CNAME records per domain, the configuration set, and the
/// receipt rule set name.
#[test]
fn aws_email_round_trip() {
    let storage = Storage::new("mailbox".to_string()).build();
    let email = Email::new("mailer".to_string())
        .domains(vec!["mail.example.com".to_string()])
        .inbound(EmailInbound {
            storage: ResourceRef::from(&storage),
        })
        .build();
    let entry = frozen_entry(email);
    // Wire-shaped payload: the same key structure the AWS email emitter's
    // `emit_import_ref` produces after CloudFormation resolves it.
    let payload = json!({
        "configurationSet": "alien-stack-mailer",
        "domains": {
            "mail.example.com": {
                "dkimTokens": [
                    {"name": "t1._domainkey.mail.example.com", "value": "t1.dkim.amazonses.com"},
                    {"name": "t2._domainkey.mail.example.com", "value": "t2.dkim.amazonses.com"},
                    {"name": "t3._domainkey.mail.example.com", "value": "t3.dkim.amazonses.com"}
                ]
            }
        },
        "ruleSetName": "alien-stack-mailer"
    });
    let state = run_through_registry(
        &Email::RESOURCE_TYPE,
        Platform::Aws,
        payload,
        &entry,
        "us-east-1",
        &aws_management_config(),
    );

    assert_running_with_internal_state(&state);
    let internal = internal_state(&state);
    assert_eq!(internal["type"], "AwsEmailController");
    assert_eq!(internal["state"], "ready");
    assert_eq!(internal["configurationSet"], "alien-stack-mailer");
    assert_eq!(internal["ruleSetName"], "alien-stack-mailer");

    let outputs = state
        .outputs
        .as_ref()
        .and_then(|outputs| outputs.downcast_ref::<EmailOutputs>())
        .expect("email import must expose typed EmailOutputs");
    assert_eq!(outputs.configuration_set, "alien-stack-mailer");
    assert_eq!(outputs.rule_set_name.as_deref(), Some("alien-stack-mailer"));
    let domain = outputs
        .domains
        .get("mail.example.com")
        .expect("seed domain must be present in outputs");
    assert_eq!(domain.dkim_tokens.len(), 3);
    assert_eq!(domain.dkim_tokens[0].name, "t1._domainkey.mail.example.com");
    assert_eq!(domain.dkim_tokens[0].value, "t1.dkim.amazonses.com");

    // Imported resources must not publish binding material unless the entry
    // explicitly opts into remote access.
    assert_eq!(
        state.remote_binding_params, None,
        "an imported resource without remote access must not publish its binding params"
    );
}

/// A config-set-only email resource (no seed domains, no inbound) is valid —
/// runtime-created identities are managed outside the deployment — and must
/// import with empty domains and no rule set name.
#[test]
fn aws_email_config_set_only_round_trip() {
    let entry = frozen_entry(Email::new("mailer".to_string()).build());
    let payload = json!({
        "configurationSet": "alien-stack-mailer",
        "domains": {}
    });
    let state = run_through_registry(
        &Email::RESOURCE_TYPE,
        Platform::Aws,
        payload,
        &entry,
        "us-east-1",
        &aws_management_config(),
    );

    assert_running_with_internal_state(&state);
    let outputs = state
        .outputs
        .as_ref()
        .and_then(|outputs| outputs.downcast_ref::<EmailOutputs>())
        .expect("email import must expose typed EmailOutputs");
    assert_eq!(outputs.configuration_set, "alien-stack-mailer");
    assert!(outputs.domains.is_empty());
    assert!(outputs.rule_set_name.is_none());
}

/// A payload missing the required `configurationSet` field must surface as a
/// typed deserialization error naming the resource — not a silent default.
#[test]
fn aws_email_missing_configuration_set_is_a_typed_error() {
    let entry = frozen_entry(Email::new("mailer".to_string()).build());
    let registry = ImporterRegistry::built_in();
    let settings = settings();
    let mgmt = aws_management_config();
    let ctx = ImportContext {
        resource_id: "mailer",
        platform: Platform::Aws,
        region: "us-east-1",
        stack_settings: &settings,
        management_config: Some(&mgmt),
        resource: &entry,
    };
    let err = registry
        .run(
            &Email::RESOURCE_TYPE,
            Platform::Aws,
            json!({ "domains": {} }),
            &ctx,
        )
        .expect_err("payload without configurationSet must fail");
    let msg = err.to_string();
    assert!(
        msg.contains("configurationSet") && msg.contains("mailer"),
        "error must name the missing field and the resource, got: {msg}"
    );
}

/// An OpenSearch Serverless collection imports as Running with typed
/// [`AwsOpenSearchOutputs`] carrying the data-plane endpoint and ARN the
/// setup stack handed over.
#[test]
fn aws_open_search_round_trip() {
    let entry = frozen_entry(AwsOpenSearch::new("search".to_string()).build());
    // Wire-shaped payload: the same key structure the AWS OpenSearch
    // emitter's `emit_import_ref` produces after CloudFormation resolves it.
    let payload = json!({
        "collectionName": "search-a2591da2",
        "collectionId": "abc123def456",
        "collectionArn": "arn:aws:aoss:us-east-1:123456789012:collection/abc123def456",
        "endpoint": "https://abc123def456.aoss.us-east-1.on.aws"
    });
    let state = run_through_registry(
        &AwsOpenSearch::RESOURCE_TYPE,
        Platform::Aws,
        payload,
        &entry,
        "us-east-1",
        &aws_management_config(),
    );

    assert_running_with_internal_state(&state);
    let internal = internal_state(&state);
    assert_eq!(internal["type"], "AwsOpenSearchController");
    assert_eq!(internal["state"], "ready");
    assert_eq!(internal["collectionName"], "search-a2591da2");
    assert_eq!(internal["collectionId"], "abc123def456");

    let outputs = state
        .outputs
        .as_ref()
        .and_then(|outputs| outputs.downcast_ref::<AwsOpenSearchOutputs>())
        .expect("aws-opensearch import must expose typed AwsOpenSearchOutputs");
    assert_eq!(
        outputs.endpoint,
        "https://abc123def456.aoss.us-east-1.on.aws"
    );
    assert_eq!(
        outputs.collection_arn,
        "arn:aws:aoss:us-east-1:123456789012:collection/abc123def456"
    );

    // Imported resources must not publish binding material unless the entry
    // explicitly opts into remote access.
    assert_eq!(
        state.remote_binding_params, None,
        "an imported resource without remote access must not publish its binding params"
    );
}

/// A payload missing the required `endpoint` field must surface as a typed
/// deserialization error naming the resource — not a silent default.
#[test]
fn aws_open_search_missing_endpoint_is_a_typed_error() {
    let entry = frozen_entry(AwsOpenSearch::new("search".to_string()).build());
    let registry = ImporterRegistry::built_in();
    let settings = settings();
    let mgmt = aws_management_config();
    let ctx = ImportContext {
        resource_id: "search",
        platform: Platform::Aws,
        region: "us-east-1",
        stack_settings: &settings,
        management_config: Some(&mgmt),
        resource: &entry,
    };
    let err = registry
        .run(
            &AwsOpenSearch::RESOURCE_TYPE,
            Platform::Aws,
            json!({
                "collectionName": "search-a2591da2",
                "collectionId": "abc123def456",
                "collectionArn": "arn:aws:aoss:us-east-1:123456789012:collection/abc123def456"
            }),
            &ctx,
        )
        .expect_err("payload without endpoint must fail");
    let msg = err.to_string();
    assert!(
        msg.contains("endpoint") && msg.contains("search"),
        "error must name the missing field and the resource, got: {msg}"
    );
}

#[test]
fn gcp_storage_round_trip() {
    let entry = entry(Storage::new("my-bucket".to_string()).build());
    let data = GcpStorageImportData {
        project_id: "my-project".to_string(),
        bucket_name: "alien-stack-my-bucket".to_string(),
        bucket_self_link: "https://www.googleapis.com/storage/v1/b/alien-stack-my-bucket"
            .to_string(),
        location: "us-central1".to_string(),
    };
    let state = run_through_registry(
        &Storage::RESOURCE_TYPE,
        Platform::Gcp,
        serde_json::to_value(&data).unwrap(),
        &entry,
        "us-central1",
        &gcp_management_config(),
    );
    assert_running_with_internal_state(&state);
    assert_eq!(
        internal_state(&state)["bucketName"],
        "alien-stack-my-bucket"
    );
    assert_eq!(
        state.remote_binding_params, None,
        "an imported resource without remote access must not publish its binding params"
    );
}

#[test]
fn gcp_storage_remote_access_round_trip() {
    let entry = remote_entry(Storage::new("my-bucket".to_string()).build());
    let data = GcpStorageImportData {
        project_id: "my-project".to_string(),
        bucket_name: "alien-stack-my-bucket".to_string(),
        bucket_self_link: "https://www.googleapis.com/storage/v1/b/alien-stack-my-bucket"
            .to_string(),
        location: "us-central1".to_string(),
    };
    let state = run_through_registry(
        &Storage::RESOURCE_TYPE,
        Platform::Gcp,
        serde_json::to_value(&data).unwrap(),
        &entry,
        "us-central1",
        &gcp_management_config(),
    );

    assert_running_with_internal_state(&state);
    assert_eq!(
        state.remote_binding_params,
        Some(json!({
            "service": "gcs",
            "bucketName": "alien-stack-my-bucket",
        })),
        "an imported resource with remote access must publish its binding params"
    );
}

#[test]
fn gcp_kv_remote_access_round_trip() {
    let entry = remote_entry(Kv::new("settings".to_string()).build());
    let data = GcpKvImportData {
        project_id: "my-project".to_string(),
        database_id: "alien-stack-settings".to_string(),
        location: "us-central1".to_string(),
    };
    let state = run_through_registry(
        &Kv::RESOURCE_TYPE,
        Platform::Gcp,
        serde_json::to_value(&data).unwrap(),
        &entry,
        "us-central1",
        &gcp_management_config(),
    );
    assert_running_with_internal_state(&state);
    assert_eq!(internal_state(&state)["collectionName"], "settings");
    assert_eq!(
        state.remote_binding_params,
        Some(json!({
            "service": "firestore",
            "projectId": "my-project",
            "databaseId": "alien-stack-settings",
            "collectionName": "settings",
        }))
    );
}

#[test]
fn gcp_build_remote_access_round_trip() {
    let entry = remote_entry(
        Build::new("builder".to_string())
            .permissions("build-execution".to_string())
            .environment(HashMap::from([(
                "TEST_VAR".to_string(),
                "test-value".to_string(),
            )]))
            .build(),
    );
    let data = GcpBuildImportData {
        project_id: "my-project".to_string(),
        region: "us-central1".to_string(),
        trigger_id: "12345678-1234-1234-1234-123456789abc".to_string(),
        trigger_name: "alien-stack-builder".to_string(),
        build_env_vars: HashMap::from([("TEST_VAR".to_string(), "test-value".to_string())]),
        service_account_email: "builder@my-project.iam.gserviceaccount.com".to_string(),
    };
    let state = run_through_registry(
        &Build::RESOURCE_TYPE,
        Platform::Gcp,
        serde_json::to_value(&data).unwrap(),
        &entry,
        "us-central1",
        &gcp_management_config(),
    );
    assert_running_with_internal_state(&state);
    assert_eq!(
        internal_state(&state)["buildConfigId"],
        "alien-stack-builder"
    );
    assert_eq!(
        state.remote_binding_params,
        Some(json!({
            "service": "cloudbuild",
            "buildEnvVars": {
                "TEST_VAR": "test-value",
            },
            "serviceAccount": "builder@my-project.iam.gserviceaccount.com",
            "monitoring": null,
        }))
    );
}

#[test]
fn gcp_service_activation_round_trip() {
    let entry = entry(
        ServiceActivation::new("activate-run".to_string())
            .service_name("run.googleapis.com".to_string())
            .build(),
    );
    let data = GcpServiceActivationImportData {
        project_id: "my-project".to_string(),
        service_name: "run.googleapis.com".to_string(),
        activated: true,
    };
    let state = run_through_registry(
        &ServiceActivation::RESOURCE_TYPE,
        Platform::Gcp,
        serde_json::to_value(&data).unwrap(),
        &entry,
        "us-central1",
        &gcp_management_config(),
    );
    assert_running_with_internal_state(&state);
}

#[test]
fn gcp_network_import_derives_subnetwork_name() {
    // Regression: the importer must reconstruct `subnetwork_name` from the subnet self-link.
    // Without it the live worker's `get_vpc_access` short-circuits and the Cloud Run service gets
    // no Direct VPC egress — unable to reach a private Cloud SQL PSC endpoint in that subnet.
    let entry = entry(
        Network::new("default-network".to_string())
            .settings(NetworkSettings::UseDefault)
            .build(),
    );
    let data = GcpNetworkImportData {
        project_id: "my-project".to_string(),
        vpc_self_link: Some(
            "https://www.googleapis.com/compute/v1/projects/my-project/global/networks/alien-stack-vpc"
                .to_string(),
        ),
        vpc_name: Some("alien-stack-vpc".to_string()),
        subnet_self_links: vec![
            "https://www.googleapis.com/compute/v1/projects/my-project/regions/us-central1/subnetworks/alien-stack-workload"
                .to_string(),
        ],
        cidr_block: Some("10.0.0.0/20".to_string()),
        router_self_link: None,
        nat_name: None,
        is_byo_vpc: false,
    };
    let state = run_through_registry(
        &Network::RESOURCE_TYPE,
        Platform::Gcp,
        serde_json::to_value(&data).unwrap(),
        &entry,
        "us-central1",
        &gcp_management_config(),
    );
    assert_running_with_internal_state(&state);
    assert_eq!(
        internal_state(&state)["subnetworkName"], "alien-stack-workload",
        "importer must derive subnetwork_name from the subnet self-link, else the worker gets no VPC egress"
    );
}

#[test]
fn azure_storage_round_trip() {
    let entry = entry(Storage::new("my-bucket".to_string()).build());
    let data = AzureStorageImportData {
        subscription_id: "00000000-0000-0000-0000-000000000000".to_string(),
        resource_group: "rg-alien".to_string(),
        storage_account_name: "alienstg".to_string(),
        container_name: "alien-stack-my-bucket".to_string(),
    };
    let state = run_through_registry(
        &Storage::RESOURCE_TYPE,
        Platform::Azure,
        serde_json::to_value(&data).unwrap(),
        &entry,
        "eastus",
        &azure_management_config(),
    );
    assert_running_with_internal_state(&state);
}

#[test]
fn azure_storage_account_round_trip_includes_dependency_outputs() {
    let entry = entry(AzureStorageAccount::new("default-storage-account".to_string()).build());
    let data = AzureStorageAccountImportData {
        subscription_id: "00000000-0000-0000-0000-000000000000".to_string(),
        resource_group: "rg-alien".to_string(),
        storage_account_name: "alienstg".to_string(),
        resource_id: "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/rg-alien/providers/Microsoft.Storage/storageAccounts/alienstg".to_string(),
        blob_endpoint: "https://alienstg.blob.core.windows.net/".to_string(),
        file_endpoint: "https://alienstg.file.core.windows.net/".to_string(),
        queue_endpoint: "https://alienstg.queue.core.windows.net/".to_string(),
        table_endpoint: "https://alienstg.table.core.windows.net/".to_string(),
    };
    let state = run_through_registry(
        &AzureStorageAccount::RESOURCE_TYPE,
        Platform::Azure,
        serde_json::to_value(&data).unwrap(),
        &entry,
        "eastus",
        &azure_management_config(),
    );
    assert_running_with_internal_state(&state);

    let outputs = state
        .outputs
        .as_ref()
        .and_then(|outputs| outputs.downcast_ref::<AzureStorageAccountOutputs>())
        .expect("imported Azure storage account must expose dependency outputs");
    assert_eq!(outputs.account_name, data.storage_account_name);
    assert_eq!(outputs.resource_id, data.resource_id);
    assert_eq!(outputs.primary_blob_endpoint, data.blob_endpoint);
    assert_eq!(outputs.primary_file_endpoint, data.file_endpoint);
    assert_eq!(outputs.primary_queue_endpoint, data.queue_endpoint);
    assert_eq!(outputs.primary_table_endpoint, data.table_endpoint);
}

#[test]
fn azure_resource_group_round_trip() {
    let entry = entry(AzureResourceGroup::new("default-resource-group".to_string()).build());
    let data = AzureResourceGroupImportData {
        subscription_id: "00000000-0000-0000-0000-000000000000".to_string(),
        resource_group: "rg-alien".to_string(),
        location: "eastus".to_string(),
    };
    let state = run_through_registry(
        &AzureResourceGroup::RESOURCE_TYPE,
        Platform::Azure,
        serde_json::to_value(&data).unwrap(),
        &entry,
        "eastus",
        &azure_management_config(),
    );
    assert_running_with_internal_state(&state);
    assert_eq!(internal_state(&state)["resourceGroupName"], "rg-alien");
    let outputs = state
        .outputs
        .as_ref()
        .and_then(|outputs| outputs.downcast_ref::<AzureResourceGroupOutputs>())
        .expect("imported Azure resource group must expose dependency outputs");
    assert_eq!(outputs.name, "rg-alien");
    assert_eq!(
        outputs.resource_id,
        "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/rg-alien"
    );
    assert_eq!(outputs.location, "eastus");
}

#[test]
fn azure_container_apps_environment_round_trip_includes_dependency_outputs() {
    let entry =
        entry(AzureContainerAppsEnvironment::new("default-container-env".to_string()).build());
    let data = AzureContainerAppsEnvironmentImportData {
        subscription_id: "00000000-0000-0000-0000-000000000000".to_string(),
        resource_group: "rg-alien".to_string(),
        environment_name: "alien-env".to_string(),
        resource_id: "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/rg-alien/providers/Microsoft.App/managedEnvironments/alien-env".to_string(),
        default_domain: "alien-env.example.azurecontainerapps.io".to_string(),
        custom_domain_verification_id: Some("verification-id".to_string()),
    };
    let state = run_through_registry(
        &AzureContainerAppsEnvironment::RESOURCE_TYPE,
        Platform::Azure,
        serde_json::to_value(&data).unwrap(),
        &entry,
        "eastus",
        &azure_management_config(),
    );
    assert_running_with_internal_state(&state);

    let outputs = state
        .outputs
        .as_ref()
        .and_then(|outputs| outputs.downcast_ref::<AzureContainerAppsEnvironmentOutputs>())
        .expect("imported Azure Container Apps Environment must expose dependency outputs");
    assert_eq!(outputs.environment_name, data.environment_name);
    assert_eq!(outputs.resource_id, data.resource_id);
    assert_eq!(outputs.resource_group_name, data.resource_group);
    assert_eq!(outputs.default_domain, data.default_domain);
    assert_eq!(
        outputs.custom_domain_verification_id,
        data.custom_domain_verification_id
    );
}

#[test]
fn azure_service_account_import_waits_for_stack_permission_propagation() {
    let entry = entry(ServiceAccount::new("execution".to_string()).build());
    let data = AzureServiceAccountImportData {
        subscription_id: "00000000-0000-0000-0000-000000000000".to_string(),
        resource_group: "rg-alien".to_string(),
        identity_id: "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/rg-alien/providers/Microsoft.ManagedIdentity/userAssignedIdentities/execution".to_string(),
        client_id: "11111111-1111-1111-1111-111111111111".to_string(),
        principal_id: "22222222-2222-2222-2222-222222222222".to_string(),
        stack_permissions_applied: true,
    };
    let state = run_through_registry(
        &ServiceAccount::RESOURCE_TYPE,
        Platform::Azure,
        serde_json::to_value(&data).unwrap(),
        &entry,
        "eastus",
        &azure_management_config(),
    );
    assert_provisioning_with_internal_state(&state);
    assert_eq!(internal_state(&state)["state"], "waitingForRbacPropagation");
}

#[test]
fn aws_ai_round_trip() {
    let entry = entry(Ai::new("llm".to_string()).build());
    let data = AwsAiImportData {
        region: "us-east-1".to_string(),
    };
    let state = run_through_registry(
        &Ai::RESOURCE_TYPE,
        Platform::Aws,
        serde_json::to_value(&data).unwrap(),
        &entry,
        "us-east-1",
        &aws_management_config(),
    );
    assert_running_with_internal_state(&state);

    // binding params must carry the region and identify Bedrock
    let binding = controller_binding_params(&state);
    assert_eq!(binding["service"], "bedrock");
    assert_eq!(binding["region"], "us-east-1");
    assert_eq!(
        state.remote_binding_params, None,
        "AI binding params must stay on the controller unless remote access is enabled"
    );

    // outputs must expose provider "bedrock"
    let outputs = state
        .outputs
        .as_ref()
        .and_then(|o| o.downcast_ref::<alien_core::AiOutputs>())
        .expect("AWS AI import must produce AiOutputs");
    assert_eq!(outputs.provider, "bedrock");
    assert!(
        outputs
            .endpoint
            .as_ref()
            .is_some_and(|ep| ep.contains("us-east-1")),
        "endpoint must contain the region"
    );
}

#[test]
fn gcp_ai_round_trip() {
    let entry = entry(Ai::new("llm".to_string()).build());
    let data = GcpAiImportData {
        project_id: "my-project".to_string(),
        location: "us-central1".to_string(),
    };
    let state = run_through_registry(
        &Ai::RESOURCE_TYPE,
        Platform::Gcp,
        serde_json::to_value(&data).unwrap(),
        &entry,
        "us-central1",
        &gcp_management_config(),
    );
    assert_running_with_internal_state(&state);

    // binding params must carry project + location and identify Vertex AI
    let binding = controller_binding_params(&state);
    assert_eq!(binding["service"], "vertex");
    assert_eq!(binding["project"], "my-project");
    assert_eq!(binding["location"], "us-central1");
    assert_eq!(
        state.remote_binding_params, None,
        "AI binding params must stay on the controller unless remote access is enabled"
    );

    // outputs must expose provider "vertex"
    let outputs = state
        .outputs
        .as_ref()
        .and_then(|o| o.downcast_ref::<alien_core::AiOutputs>())
        .expect("GCP AI import must produce AiOutputs");
    assert_eq!(outputs.provider, "vertex");
    assert!(
        outputs
            .endpoint
            .as_ref()
            .is_some_and(|ep| ep.contains("us-central1") && ep.contains("my-project")),
        "endpoint must contain the location and project"
    );
}

#[test]
fn azure_ai_round_trip() {
    let entry = entry(Ai::new("llm".to_string()).build());
    let data = AzureAiImportData {
        account_name: "myprefix-llm".to_string(),
        endpoint: "https://myprefix-llm.cognitiveservices.azure.com/".to_string(),
        resource_group: "rg-alien".to_string(),
        location: "eastus".to_string(),
    };
    let state = run_through_registry(
        &Ai::RESOURCE_TYPE,
        Platform::Azure,
        serde_json::to_value(&data).unwrap(),
        &entry,
        "eastus",
        &azure_management_config(),
    );
    assert_running_with_internal_state(&state);

    // binding params must carry the endpoint + account and identify Foundry
    let binding = controller_binding_params(&state);
    assert_eq!(binding["service"], "foundry");
    assert_eq!(
        binding["endpoint"],
        "https://myprefix-llm.cognitiveservices.azure.com/"
    );
    assert_eq!(binding["account"], "myprefix-llm");
    assert_eq!(
        state.remote_binding_params, None,
        "AI binding params must stay on the controller unless remote access is enabled"
    );

    // outputs must expose provider "foundry"
    let outputs = state
        .outputs
        .as_ref()
        .and_then(|o| o.downcast_ref::<alien_core::AiOutputs>())
        .expect("Azure AI import must produce AiOutputs");
    assert_eq!(outputs.provider, "foundry");
    assert_eq!(
        outputs.endpoint.as_deref(),
        Some("https://myprefix-llm.cognitiveservices.azure.com/")
    );
    assert_eq!(outputs.account.as_deref(), Some("myprefix-llm"));
}

#[test]
fn key_handoff_preserves_provider_identity_and_remote_binding() {
    let cases = [
        (
            Platform::Aws,
            serde_json::to_value(AwsKeyImportData {
                key_arn: "arn:aws:kms:us-east-1:123456789012:key/11111111-2222-3333-4444-555555555555".to_string(),
            })
            .unwrap(),
            KeyFingerprint::Aws {
                key_arn: "arn:aws:kms:us-east-1:123456789012:key/11111111-2222-3333-4444-555555555555".to_string(),
            },
            "arn:aws:kms:us-east-1:123456789012:key/11111111-2222-3333-4444-555555555555",
            "kms",
        ),
        (
            Platform::Gcp,
            serde_json::to_value(GcpKeyImportData {
                crypto_key_name: "projects/example/locations/us/keyRings/data/cryptoKeys/customer".to_string(),
                primary_version: "projects/example/locations/us/keyRings/data/cryptoKeys/customer/cryptoKeyVersions/1".to_string(),
            })
            .unwrap(),
            KeyFingerprint::Gcp {
                crypto_key_name: "projects/example/locations/us/keyRings/data/cryptoKeys/customer".to_string(),
            },
            "projects/example/locations/us/keyRings/data/cryptoKeys/customer/cryptoKeyVersions/1",
            "cloud-kms",
        ),
        (
            Platform::Azure,
            serde_json::to_value(AzureKeyImportData {
                vault_resource_id: "/subscriptions/sub/resourceGroups/rg/providers/Microsoft.KeyVault/vaults/example".to_string(),
                key_name: "customer".to_string(),
                lineage_version_id: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
                key_id: "https://example.vault.azure.net/keys/customer/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
            })
            .unwrap(),
            KeyFingerprint::Azure {
                vault_resource_id: "/subscriptions/sub/resourceGroups/rg/providers/Microsoft.KeyVault/vaults/example".to_string(),
                key_name: "customer".to_string(),
                lineage_version_id: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
            },
            "https://example.vault.azure.net/keys/customer/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "key-vault-key",
        ),
    ];

    for (platform, payload, expected_fingerprint, expected_wrapping_key, expected_service) in cases
    {
        let entry = remote_entry(Key::new("enterprise-key".to_string()).build());
        let management = match platform {
            Platform::Aws => aws_management_config(),
            Platform::Gcp => gcp_management_config(),
            Platform::Azure => azure_management_config(),
            _ => unreachable!(),
        };
        let state = run_through_registry(
            &Key::RESOURCE_TYPE,
            platform,
            payload,
            &entry,
            "test-region",
            &management,
        );

        assert_running_with_internal_state(&state);
        let outputs = state
            .outputs
            .as_ref()
            .and_then(|outputs| outputs.downcast_ref::<KeyOutputs>())
            .expect("key import must expose typed outputs");
        assert_eq!(outputs.fingerprint, expected_fingerprint);
        assert_eq!(outputs.wrapping_key_id, expected_wrapping_key);
        assert_eq!(
            state.remote_binding_params.as_ref().unwrap()["service"],
            expected_service
        );
        if platform == Platform::Aws {
            assert_eq!(
                state.remote_binding_params.as_ref().unwrap()["region"],
                "test-region"
            );
        }
    }
}

#[test]
fn registry_built_in_covers_all_oss_pairs() {
    let registry = ImporterRegistry::built_in();

    let aws_pairs: &[ResourceType] = &[
        Ai::RESOURCE_TYPE,
        Storage::RESOURCE_TYPE,
        Kv::RESOURCE_TYPE,
        Key::RESOURCE_TYPE,
        Vault::RESOURCE_TYPE,
        Queue::RESOURCE_TYPE,
        Network::RESOURCE_TYPE,
        ServiceAccount::RESOURCE_TYPE,
        RemoteStackManagement::RESOURCE_TYPE,
        Build::RESOURCE_TYPE,
        ArtifactRegistry::RESOURCE_TYPE,
        Worker::RESOURCE_TYPE,
        Email::RESOURCE_TYPE,
        AwsOpenSearch::RESOURCE_TYPE,
    ];
    for rt in aws_pairs {
        assert!(
            registry.importer(rt, Platform::Aws).is_some(),
            "missing AWS importer for {}",
            rt
        );
    }

    let gcp_pairs: &[ResourceType] = &[
        Ai::RESOURCE_TYPE,
        Storage::RESOURCE_TYPE,
        Kv::RESOURCE_TYPE,
        Key::RESOURCE_TYPE,
        Vault::RESOURCE_TYPE,
        Queue::RESOURCE_TYPE,
        Network::RESOURCE_TYPE,
        ServiceAccount::RESOURCE_TYPE,
        RemoteStackManagement::RESOURCE_TYPE,
        Build::RESOURCE_TYPE,
        ArtifactRegistry::RESOURCE_TYPE,
        Worker::RESOURCE_TYPE,
        ServiceActivation::RESOURCE_TYPE,
    ];
    for rt in gcp_pairs {
        assert!(
            registry.importer(rt, Platform::Gcp).is_some(),
            "missing GCP importer for {}",
            rt
        );
    }

    let azure_pairs: &[ResourceType] = &[
        Ai::RESOURCE_TYPE,
        Storage::RESOURCE_TYPE,
        Kv::RESOURCE_TYPE,
        Key::RESOURCE_TYPE,
        Vault::RESOURCE_TYPE,
        Queue::RESOURCE_TYPE,
        Network::RESOURCE_TYPE,
        ServiceAccount::RESOURCE_TYPE,
        RemoteStackManagement::RESOURCE_TYPE,
        Build::RESOURCE_TYPE,
        ArtifactRegistry::RESOURCE_TYPE,
        Worker::RESOURCE_TYPE,
        ServiceActivation::RESOURCE_TYPE,
        AzureResourceGroup::RESOURCE_TYPE,
        AzureStorageAccount::RESOURCE_TYPE,
        AzureContainerAppsEnvironment::RESOURCE_TYPE,
        AzureServiceBusNamespace::RESOURCE_TYPE,
    ];
    for rt in azure_pairs {
        assert!(
            registry.importer(rt, Platform::Azure).is_some(),
            "missing Azure importer for {}",
            rt
        );
    }

    // Container / compute-cluster live in the platform crate.
    let compute_cluster: ResourceType = "compute-cluster".into();
    assert!(
        registry.importer(&compute_cluster, Platform::Aws).is_none(),
        "compute-cluster must not be registered in OSS built_in (it lives in alien-platform-controllers)"
    );
}

#[test]
fn missing_importer_returns_typed_error() {
    let registry = ImporterRegistry::built_in();
    let entry = entry(Storage::new("dummy".to_string()).build());
    let settings = settings();
    let mgmt = aws_management_config();
    let ctx = ImportContext {
        resource_id: "missing",
        platform: Platform::Kubernetes,
        region: "n/a",
        stack_settings: &settings,
        management_config: Some(&mgmt),
        resource: &entry,
    };
    // Storage is registered for AWS/GCP/Azure but not for Kubernetes —
    // the registry must surface that as a typed `ImportRegistrationMissing`
    // error rather than silently producing an empty state.
    let err = registry
        .run(
            &Storage::RESOURCE_TYPE,
            Platform::Kubernetes,
            json!({}),
            &ctx,
        )
        .expect_err("Kubernetes storage importer is intentionally unregistered");
    let msg = err.to_string();
    assert!(
        msg.contains("ImportRegistration") || msg.contains("import"),
        "expected ImportRegistrationMissing, got: {}",
        msg
    );
}

fn sandbox_resource() -> Sandbox {
    Sandbox::new("agents".to_string())
        .code(SandboxCode::Image {
            image: "manager.example.com/alien-artifacts-proj:base".to_string(),
        })
        .egress(SandboxEgress::Deny)
        .session(SandboxSessionPolicy {
            max_lifetime_seconds: Some(1800),
            idle_suspend_seconds: None,
        })
        .build()
}

fn sandbox_entry(lifecycle: ResourceLifecycle, remote_access: bool) -> ResourceEntry {
    ResourceEntry {
        config: Resource::new(sandbox_resource()),
        lifecycle,
        dependencies: vec![],
        remote_access,
        enabled_when: None,
    }
}

/// A Frozen sandbox registers the image its stack built and imports Running at the
/// controller's Ready state, with a binding an application can start sessions from
/// immediately.
#[test]
fn aws_sandbox_frozen_shape_imports_running_with_a_binding() {
    let entry = sandbox_entry(ResourceLifecycle::Frozen, true);
    // Built through the typed struct so a renamed field breaks here, not at a customer's
    // registration.
    let payload = serde_json::to_value(AwsSandboxImportData {
        image_identifier: Some(
            "arn:aws:lambda:us-east-2:123456789012:microvm-image:stack-agents".to_string(),
        ),
        image_arn: Some(
            "arn:aws:lambda:us-east-2:123456789012:microvm-image:stack-agents".to_string(),
        ),
        image_version: Some("1.0".to_string()),
        build_role_arn: None,
        bundle_uri: None,
        egress_connector_arns: vec![
            "arn:aws:lambda:us-east-2:123456789012:network-connector:nc-1".to_string(),
        ],
        allow_egress: false,
        preview_ports: vec![8080],
    })
    .unwrap();

    let state = run_through_registry(
        &Sandbox::RESOURCE_TYPE,
        Platform::Aws,
        payload,
        &entry,
        "us-east-2",
        &aws_management_config(),
    );

    assert_running_with_internal_state(&state);
    let internal = internal_state(&state);
    assert_eq!(internal["type"], "AwsSandboxController");
    assert_eq!(internal["state"], "ready");

    let binding = controller_binding_params(&state);
    assert_eq!(binding["imageVersion"], "1.0");
    // The region is parsed out of the ARN, not defaulted: a binding naming the wrong region
    // addresses no MicroVM at all.
    assert_eq!(binding["region"], "us-east-2");
    assert_eq!(
        binding["egressConnectorArns"][0],
        "arn:aws:lambda:us-east-2:123456789012:network-connector:nc-1"
    );
}

/// A later release re-registers the same Live sandbox. Replacing the state would drop the
/// version the controller built — withdrawing the binding of a sandbox that is serving, and
/// re-running the create flow against an image that already exists — so the image facts are
/// preserved and only the setup-owned ones are taken from the registration.
#[test]
fn aws_sandbox_reimport_preserves_the_built_image_and_takes_the_new_connector() {
    let entry = sandbox_entry(ResourceLifecycle::Live, true);
    let settings = settings();
    let management = aws_management_config();
    let ctx = ImportContext {
        resource_id: "agents",
        platform: Platform::Aws,
        region: "us-east-2",
        stack_settings: &settings,
        management_config: Some(&management),
        resource: &entry,
    };
    let registry = ImporterRegistry::built_in();

    // What the runtime controller reached: an image it built, serving version 1.0.
    let existing = registry
        .run(
            &Sandbox::RESOURCE_TYPE,
            Platform::Aws,
            serde_json::to_value(AwsSandboxImportData {
                image_identifier: None,
                image_arn: None,
                image_version: None,
                build_role_arn: Some(
                    "arn:aws:iam::123456789012:role/stack-agents-build".to_string(),
                ),
                bundle_uri: Some("s3://alien-bundles/sandbox/bundle.zip".to_string()),
                egress_connector_arns: vec![
                    "arn:aws:lambda:us-east-2:123456789012:network-connector:nc-1".to_string(),
                ],
                allow_egress: false,
                preview_ports: vec![8080],
            })
            .unwrap(),
            &ctx,
        )
        .expect("first import should succeed");
    let mut existing_internal = internal_state(&existing).clone();
    existing_internal["state"] = serde_json::json!("ready");
    existing_internal["imageIdentifier"] =
        serde_json::json!("arn:aws:lambda:us-east-2:123456789012:microvm-image:stack-agents");
    existing_internal["imageArn"] =
        serde_json::json!("arn:aws:lambda:us-east-2:123456789012:microvm-image:stack-agents");
    existing_internal["activeVersion"] = serde_json::json!("1.0");
    existing_internal["region"] = serde_json::json!("us-east-2");
    let existing = alien_core::StackResourceState {
        internal_state: Some(existing_internal),
        status: alien_core::ResourceStatus::Running,
        ..existing
    };

    // The new release re-registers with a different egress connector.
    let imported = registry
        .run(
            &Sandbox::RESOURCE_TYPE,
            Platform::Aws,
            serde_json::to_value(AwsSandboxImportData {
                image_identifier: None,
                image_arn: None,
                image_version: None,
                build_role_arn: Some(
                    "arn:aws:iam::123456789012:role/stack-agents-build".to_string(),
                ),
                bundle_uri: Some("s3://alien-bundles/sandbox/bundle-v2.zip".to_string()),
                egress_connector_arns: vec![
                    "arn:aws:lambda:us-east-2:123456789012:network-connector:nc-2".to_string(),
                ],
                allow_egress: false,
                preview_ports: vec![8080],
            })
            .unwrap(),
            &ctx,
        )
        .expect("re-import should succeed");

    let merged = registry
        .merge_reimport(
            &Sandbox::RESOURCE_TYPE,
            Platform::Aws,
            existing,
            imported,
            &ctx,
        )
        .expect("merge should succeed");

    let internal = internal_state(&merged);
    assert_eq!(
        internal["state"], "ready",
        "the controller must keep its position, not restart the create flow"
    );
    assert_eq!(
        internal["activeVersion"], "1.0",
        "the built version must survive a re-import"
    );
    assert_eq!(
        internal["bundleUri"], "s3://alien-bundles/sandbox/bundle.zip",
        "a new bundle reaches the image through the update flow, not through re-import"
    );
    assert_eq!(
        merged.status,
        alien_core::ResourceStatus::Running,
        "a serving sandbox must not drop back to Provisioning"
    );

    let binding = controller_binding_params(&merged);
    assert_eq!(binding["imageVersion"], "1.0");
    assert_eq!(
        binding["imageArn"],
        "arn:aws:lambda:us-east-2:123456789012:microvm-image:stack-agents"
    );
    assert_eq!(
        binding["egressConnectorArns"][0],
        "arn:aws:lambda:us-east-2:123456789012:network-connector:nc-2",
        "a changed connector decides what a session can reach and must reach the application"
    );
}

/// A Frozen sandbox's image is built and owned by stack creation, so its registration stays
/// authoritative and a re-import replaces.
#[test]
fn aws_sandbox_frozen_reimport_replaces() {
    let entry = sandbox_entry(ResourceLifecycle::Frozen, true);
    let settings = settings();
    let management = aws_management_config();
    let ctx = ImportContext {
        resource_id: "agents",
        platform: Platform::Aws,
        region: "us-east-2",
        stack_settings: &settings,
        management_config: Some(&management),
        resource: &entry,
    };
    let registry = ImporterRegistry::built_in();
    let frozen = |version: &str| {
        serde_json::to_value(AwsSandboxImportData {
            image_identifier: Some(
                "arn:aws:lambda:us-east-2:123456789012:microvm-image:stack-agents".to_string(),
            ),
            image_arn: Some(
                "arn:aws:lambda:us-east-2:123456789012:microvm-image:stack-agents".to_string(),
            ),
            image_version: Some(version.to_string()),
            build_role_arn: None,
            bundle_uri: None,
            egress_connector_arns: vec![
                "arn:aws:lambda:us-east-2:123456789012:network-connector:nc-1".to_string(),
            ],
            allow_egress: false,
            preview_ports: vec![8080],
        })
        .unwrap()
    };

    let existing = registry
        .run(&Sandbox::RESOURCE_TYPE, Platform::Aws, frozen("1.0"), &ctx)
        .expect("first import");
    let imported = registry
        .run(&Sandbox::RESOURCE_TYPE, Platform::Aws, frozen("2.0"), &ctx)
        .expect("re-import");

    let merged = registry
        .merge_reimport(
            &Sandbox::RESOURCE_TYPE,
            Platform::Aws,
            existing,
            imported,
            &ctx,
        )
        .expect("merge should succeed");

    assert_eq!(
        controller_binding_params(&merged)["imageVersion"],
        "2.0",
        "stack creation is authoritative about a setup-owned image"
    );

    // Replacement is decided by the lifecycle, not by what the payload names: a Frozen
    // registration carrying build inputs still replaces rather than merging onto the image.
    let existing = registry
        .run(&Sandbox::RESOURCE_TYPE, Platform::Aws, frozen("2.0"), &ctx)
        .expect("existing");
    let imported = registry
        .run(
            &Sandbox::RESOURCE_TYPE,
            Platform::Aws,
            serde_json::to_value(AwsSandboxImportData {
                image_identifier: None,
                image_arn: None,
                image_version: None,
                build_role_arn: Some(
                    "arn:aws:iam::123456789012:role/stack-agents-build".to_string(),
                ),
                bundle_uri: Some("s3://alien-bundles/sandbox/bundle.zip".to_string()),
                egress_connector_arns: Vec::new(),
                allow_egress: true,
                preview_ports: Vec::new(),
            })
            .unwrap(),
            &ctx,
        )
        .expect("re-import");
    let replaced = registry
        .merge_reimport(
            &Sandbox::RESOURCE_TYPE,
            Platform::Aws,
            existing,
            imported.clone(),
            &ctx,
        )
        .expect("merge should succeed");
    assert_eq!(replaced.status, imported.status);
    assert_eq!(
        replaced.internal_state, imported.internal_state,
        "a Frozen sandbox takes the registration as it is"
    );
}

/// A Live sandbox registers its build inputs instead of an image, and imports Provisioning at
/// the create entry state so the deployment loop builds the image — with no binding, because
/// there is nothing to start a session from yet.
#[test]
fn aws_sandbox_runtime_shape_imports_provisioning_with_the_build_inputs() {
    let entry = sandbox_entry(ResourceLifecycle::Live, true);
    let payload = serde_json::to_value(AwsSandboxImportData {
        image_identifier: None,
        image_arn: None,
        image_version: None,
        build_role_arn: Some("arn:aws:iam::123456789012:role/stack-agents-build".to_string()),
        bundle_uri: Some("s3://alien-bundles/sandbox/bundle.zip".to_string()),
        egress_connector_arns: vec![
            "arn:aws:lambda:us-east-2:123456789012:network-connector:nc-1".to_string(),
        ],
        allow_egress: false,
        preview_ports: vec![8080],
    })
    .unwrap();

    let state = run_through_registry(
        &Sandbox::RESOURCE_TYPE,
        Platform::Aws,
        payload,
        &entry,
        "us-east-2",
        &aws_management_config(),
    );

    assert_eq!(
        state.status,
        ResourceStatus::Provisioning,
        "a runtime-provisioned sandbox still owes its image build"
    );
    let internal = internal_state(&state);
    assert_eq!(internal["type"], "AwsSandboxController");
    assert_eq!(internal["state"], "creatingImage");
    assert_eq!(
        internal["buildRoleArn"],
        "arn:aws:iam::123456789012:role/stack-agents-build"
    );
    assert_eq!(
        internal["bundleUri"],
        "s3://alien-bundles/sandbox/bundle.zip"
    );
    assert_eq!(
        state.remote_binding_params, None,
        "no binding may be published before the image exists"
    );
}

/// A payload with neither a complete image nor complete build inputs is a contract violation:
/// guessing through it either enumerates the wrong sessions or builds nothing.
#[test]
fn aws_sandbox_import_refuses_an_arn_with_no_region() {
    let entry = sandbox_entry(ResourceLifecycle::Frozen, false);
    let registry = ImporterRegistry::built_in();
    let settings = settings();
    let mgmt = aws_management_config();
    let ctx = ImportContext {
        resource_id: "agents",
        platform: Platform::Aws,
        region: "us-east-2",
        stack_settings: &settings,
        management_config: Some(&mgmt),
        resource: &entry,
    };

    // Sessions are addressed through the binding, and the binding needs the region the ARN
    // names; an ARN this malformed is an unexpected registration, not a wait-for-next-tick.
    let err = registry
        .run(
            &Sandbox::RESOURCE_TYPE,
            Platform::Aws,
            json!({
                "imageIdentifier": "stack-agents",
                "imageArn": "not-an-arn",
                "imageVersion": "1.0"
            }),
            &ctx,
        )
        .expect_err("an image ARN naming no region must be refused");
    assert!(
        err.to_string().contains("agents"),
        "the refusal must name the resource, got: {err}"
    );
}

#[test]
fn aws_sandbox_partial_shape_is_refused() {
    let entry = sandbox_entry(ResourceLifecycle::Live, false);
    let registry = ImporterRegistry::built_in();
    let settings = settings();
    let mgmt = aws_management_config();
    let ctx = ImportContext {
        resource_id: "agents",
        platform: Platform::Aws,
        region: "us-east-2",
        stack_settings: &settings,
        management_config: Some(&mgmt),
        resource: &entry,
    };

    let err = registry
        .run(
            &Sandbox::RESOURCE_TYPE,
            Platform::Aws,
            json!({
                "imageArn": "arn:aws:lambda:us-east-2:123456789012:microvm-image:stack-agents"
            }),
            &ctx,
        )
        .expect_err("an image ARN with no identifier, version, or build inputs must be refused");
    let msg = err.to_string();
    assert!(
        msg.contains("agents"),
        "the refusal must name the resource, got: {msg}"
    );
}
