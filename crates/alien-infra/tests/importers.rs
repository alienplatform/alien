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
        AwsAiImportData, AwsKvImportData, AwsRemoteStackManagementImportData, AwsServiceAccountImportData,
        AwsStorageImportData, AzureAiImportData,
        AzureContainerAppsEnvironmentImportData,
        AzureRemoteStackManagementImportData, AzureResourceGroupImportData,
        AzureServiceAccountImportData, AzureStorageAccountImportData, AzureStorageImportData,
        GcpAiImportData, GcpBuildImportData, GcpKvImportData, GcpNetworkImportData, GcpServiceActivationImportData,
        GcpStorageImportData, KubernetesClusterImportData,
    },
    ImportContext,
};
use alien_core::{
    Ai, ArtifactRegistry, AwsManagementConfig, AwsOpenSearch, AwsOpenSearchOutputs,
    AzureContainerAppsEnvironment, AzureContainerAppsEnvironmentOutputs, AzureManagementConfig,
    AzureResourceGroup, AzureResourceGroupOutputs, AzureServiceBusNamespace, AzureStorageAccount,
    AzureStorageAccountOutputs, Build, Email, EmailInbound, EmailOutputs, GcpManagementConfig,
    KubernetesCluster, KubernetesClusterOutputs, KubernetesClusterOwnership,
    KubernetesClusterProvider, KubernetesHeartbeatMode, Kv, ManagementConfig, Network,
    NetworkSettings, Platform, Queue, RemoteStackManagement, RemoteStackManagementOutputs,
    Resource, ResourceDefinition, ResourceEntry, ResourceLifecycle, ResourceRef, ResourceStatus,
    ResourceType, ServiceAccount, ServiceActivation, StackSettings, Storage, Vault, Worker,
};
use alien_infra::ImporterRegistry;
use serde_json::json;
use std::collections::HashMap;

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

#[test]
fn aws_remote_stack_management_round_trip() {
    let entry = entry(RemoteStackManagement::new("rsm".to_string()).build());
    let data = AwsRemoteStackManagementImportData {
        role_arn: "arn:aws:iam::123456789012:role/alien-stack-mgmt".to_string(),
        role_name: "alien-stack-mgmt".to_string(),
        management_permissions_applied: true,
    };
    let state = run_through_registry(
        &RemoteStackManagement::RESOURCE_TYPE,
        Platform::Aws,
        serde_json::to_value(&data).unwrap(),
        &entry,
        "us-east-1",
        &aws_management_config(),
    );
    assert_running_with_internal_state(&state);
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

    // Manager-provisioned workers receive the mail binding from the imported
    // controller state, mirroring the CloudFormation emitter's binding ref.
    assert_eq!(
        state.remote_binding_params,
        Some(json!({
            "service": "ses",
            "configurationSet": "alien-stack-mailer",
            "region": "us-east-1",
        }))
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

    // Manager-provisioned workers receive the collection binding from the
    // imported controller state, mirroring the CloudFormation emitter's
    // binding ref (SigV4 HTTP with service name `aoss`).
    assert_eq!(
        state.remote_binding_params,
        Some(json!({
            "service": "aoss",
            "endpoint": "https://abc123def456.aoss.us-east-1.on.aws",
            "collectionName": "search-a2591da2",
        }))
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
}

#[test]
fn gcp_kv_round_trip() {
    let entry = entry(Kv::new("settings".to_string()).build());
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
fn gcp_build_round_trip() {
    let entry = entry(
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
fn azure_remote_stack_management_round_trip_includes_access_outputs() {
    let entry = entry(RemoteStackManagement::new("rsm".to_string()).build());
    let data = AzureRemoteStackManagementImportData {
        subscription_id: "00000000-0000-0000-0000-000000000000".to_string(),
        resource_group: "rg-alien".to_string(),
        identity_id: "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/rg-alien/providers/Microsoft.ManagedIdentity/userAssignedIdentities/alien-management".to_string(),
        client_id: "11111111-1111-1111-1111-111111111111".to_string(),
        principal_id: "22222222-2222-2222-2222-222222222222".to_string(),
        tenant_id: "33333333-3333-3333-3333-333333333333".to_string(),
        management_permissions_applied: true,
    };
    let state = run_through_registry(
        &RemoteStackManagement::RESOURCE_TYPE,
        Platform::Azure,
        serde_json::to_value(&data).unwrap(),
        &entry,
        "eastus",
        &azure_management_config(),
    );
    assert_provisioning_with_internal_state(&state);
    assert_eq!(internal_state(&state)["state"], "waitingForRbacPropagation");

    let outputs = state
        .outputs
        .as_ref()
        .and_then(|outputs| outputs.downcast_ref::<RemoteStackManagementOutputs>())
        .expect("Azure remote-stack-management import must produce outputs");
    assert_eq!(outputs.management_resource_id, data.identity_id);

    let access_config: serde_json::Value =
        serde_json::from_str(&outputs.access_configuration).unwrap();
    assert_eq!(
        access_config,
        json!({
            "uamiClientId": data.client_id,
            "tenantId": data.tenant_id,
        })
    );
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
    let binding = state
        .remote_binding_params
        .as_ref()
        .expect("AWS AI import must produce binding params");
    assert_eq!(binding["service"], "bedrock");
    assert_eq!(binding["region"], "us-east-1");

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
    let binding = state
        .remote_binding_params
        .as_ref()
        .expect("GCP AI import must produce binding params");
    assert_eq!(binding["service"], "vertex");
    assert_eq!(binding["project"], "my-project");
    assert_eq!(binding["location"], "us-central1");

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
    let binding = state
        .remote_binding_params
        .as_ref()
        .expect("Azure AI import must produce binding params");
    assert_eq!(binding["service"], "foundry");
    assert_eq!(
        binding["endpoint"],
        "https://myprefix-llm.cognitiveservices.azure.com/"
    );
    assert_eq!(binding["account"], "myprefix-llm");

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
fn registry_built_in_covers_all_oss_pairs() {
    let registry = ImporterRegistry::built_in();

    let aws_pairs: &[ResourceType] = &[
        Ai::RESOURCE_TYPE,
        Storage::RESOURCE_TYPE,
        Kv::RESOURCE_TYPE,
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
