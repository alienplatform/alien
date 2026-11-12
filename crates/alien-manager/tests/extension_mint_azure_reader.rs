//! Through the resolver, the ARM read of an Azure extension's identity asks for its token as the
//! deployment's management identity (the outputs' uamiClientId in the outputs' tenant), not the
//! manager's own identity. Its own test binary, since the management binding's impersonation
//! reads the process env.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use alien_bindings::{BindingsProvider, BindingsProviderApi};
use alien_core::{
    bindings::ServiceAccountBinding, AzureClientConfig, AzureCredentials, AzureEnvironmentInfo,
    AzureResourceGroup, AzureResourceGroupOutputs, ClientConfig, EnvironmentInfo, Platform,
    RemoteStackManagement, RemoteStackManagementOutputs, Resource, ResourceLifecycle,
    ResourceOutputs, ResourceStatus, StackResourceState, StackState,
};
use alien_manager::providers::impersonation_credentials::ImpersonationCredentialResolver;
use alien_manager::{CredentialResolver, DeploymentRecord, ExtensionIdentity};

const TENANT: &str = "6c1b1a8e-1f2b-4a5e-9d3c-0a1b2c3d4e5f";
const MANAGER_TENANT: &str = "11111111-1f2b-4a5e-9d3c-0a1b2c3d4e5f";
const ACME_CLIENT_ID: &str = "0a7d6c8e-3b1f-4c2a-8e9d-111111111111";
const DEPLOYMENT_MANAGEMENT_CLIENT_ID: &str = "0a7d6c8e-3b1f-4c2a-8e9d-444444444444";
const MANAGER_CLIENT_ID: &str = "0a7d6c8e-3b1f-4c2a-8e9d-555555555555";

fn deployment() -> DeploymentRecord {
    let mut state = StackState::with_resource_prefix(Platform::Azure, "test-prefix".to_string());
    state.resources.insert(
        "management".to_string(),
        StackResourceState::builder()
            .resource_type(RemoteStackManagement::RESOURCE_TYPE.to_string())
            .status(ResourceStatus::Running)
            .config(Resource::new(
                RemoteStackManagement::new("management".to_string()).build(),
            ))
            .outputs(ResourceOutputs::new(RemoteStackManagementOutputs {
                management_resource_id: "management-identity".to_string(),
                access_configuration: serde_json::json!({
                    "uamiClientId": DEPLOYMENT_MANAGEMENT_CLIENT_ID,
                    "tenantId": TENANT
                })
                .to_string(),
                legacy_remote_bindings_access: None,
            }))
            .lifecycle(ResourceLifecycle::Frozen)
            .dependencies(vec![])
            .build(),
    );
    state.resources.insert(
        "default-resource-group".to_string(),
        StackResourceState::builder()
            .resource_type(AzureResourceGroup::RESOURCE_TYPE.to_string())
            .status(ResourceStatus::Running)
            .config(Resource::new(
                AzureResourceGroup::new("default-resource-group".to_string()).build(),
            ))
            .outputs(ResourceOutputs::new(AzureResourceGroupOutputs {
                name: "rg-a".to_string(),
                resource_id: "/subscriptions/customer-subscription/resourceGroups/rg-a".to_string(),
                location: "eastus".to_string(),
            }))
            .lifecycle(ResourceLifecycle::Frozen)
            .dependencies(vec![])
            .build(),
    );
    let mut record: DeploymentRecord = serde_json::from_value(serde_json::json!({
        "id": "deployment",
        "name": "deployment", "deploymentGroupId": "group", "platform": "azure",
        "deploymentProtocolVersion": alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
        "status": "running", "stackState": null, "environmentInfo": null,
        "runtimeMetadata": null, "currentReleaseId": null, "desiredReleaseId": null,
        "userEnvironmentVariables": null, "retryRequested": false, "lockedBy": null,
        "lockedAt": null, "createdAt": "2026-11-09T00:00:00Z", "updatedAt": null, "error": null
    }))
    .unwrap();
    record.stack_state = Some(state);
    record.environment_info = Some(EnvironmentInfo::Azure(AzureEnvironmentInfo {
        tenant_id: TENANT.to_string(),
        subscription_id: "customer-subscription".to_string(),
        location: "eastus".to_string(),
    }));
    record
}

#[tokio::test]
async fn the_arm_read_asks_for_a_token_as_the_deployments_management_identity() {
    let server = httpmock::MockServer::start_async().await;
    // Every token request is refused, so ARM itself (management.azure.com) is never called.
    let as_deployment = server
        .mock_async(|when, then| {
            when.method(httpmock::Method::POST)
                .path(format!("/{TENANT}/oauth2/v2.0/token"))
                .body_contains(format!("client_id={DEPLOYMENT_MANAGEMENT_CLIENT_ID}"))
                .body_contains("management.azure.com");
            then.status(400)
                .json_body(serde_json::json!({ "error": "probe" }));
        })
        .await;
    let any_other = server
        .mock_async(|when, then| {
            when.any_request();
            then.status(400)
                .json_body(serde_json::json!({ "error": "probe-other" }));
        })
        .await;
    let directory = tempfile::tempdir().unwrap();
    let token_file = directory.path().join("token");
    std::fs::write(&token_file, "manager-federated-token").unwrap();
    // The management binding's impersonation reads the process env (this binary has one test).
    std::env::set_var("AZURE_FEDERATED_TOKEN_FILE", &token_file);
    std::env::set_var("AZURE_AUTHORITY_HOST", server.base_url());
    std::env::set_var("AZURE_TENANT_ID", MANAGER_TENANT);

    let base = ClientConfig::Azure(Box::new(AzureClientConfig {
        subscription_id: "manager-subscription".to_string(),
        tenant_id: MANAGER_TENANT.to_string(),
        region: Some("eastus".to_string()),
        credentials: AzureCredentials::WorkloadIdentity {
            client_id: MANAGER_CLIENT_ID.to_string(),
            tenant_id: MANAGER_TENANT.to_string(),
            federated_token_file: token_file.to_string_lossy().to_string(),
            authority_host: server.base_url(),
        },
        service_overrides: None,
    }));
    let provider: Arc<dyn BindingsProviderApi> = Arc::new(
        BindingsProvider::new(
            base,
            HashMap::from([(
                "management".to_string(),
                serde_json::to_value(ServiceAccountBinding::azure_managed_identity(
                    MANAGER_CLIENT_ID,
                    "/subscriptions/manager-subscription/resourceGroups/m/providers/Microsoft.ManagedIdentity/userAssignedIdentities/manager",
                    "9f9f9f9f-0000-4000-8000-000000000009",
                ))
                .unwrap(),
            )]),
        )
        .unwrap(),
    );
    let resolver = ImpersonationCredentialResolver::new(
        provider.clone(),
        HashMap::from([(Platform::Azure, provider)]),
        HashSet::from([Platform::Azure]),
    )
    .with_environment(HashMap::from([
        (
            "AZURE_FEDERATED_TOKEN_FILE".to_string(),
            token_file.to_string_lossy().to_string(),
        ),
        ("AZURE_AUTHORITY_HOST".to_string(), server.base_url()),
    ]));

    let error = resolver
        .resolve_extension_credentials(
            &deployment(),
            &ExtensionIdentity {
                extension: "acme".to_string(),
                id: "acme-ext".to_string(),
                identity: ACME_CLIENT_ID.to_string(),
            },
        )
        .await
        .expect_err("the token endpoint refuses");
    assert!(
        as_deployment.hits_async().await >= 1,
        "ARM's token is asked for as the deployment's management identity: {error}"
    );
    assert_eq!(
        any_other.hits_async().await,
        0,
        "no other token request (not the manager's own identity, not the extension's): {error}"
    );
}
