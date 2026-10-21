//! A missing vault is deleted; other provider errors retain its retry checkpoint.
use alien_azure_clients::keyvault::MockKeyVaultManagementApi;
use alien_client_core::ErrorData;
use alien_core::{
    AzureClientConfig, AzureCredentials, ClientConfig, DeploymentConfig,
    EnvironmentVariablesSnapshot, ExternalBindings, Platform, Resource, ResourceLifecycle,
    ResourceStatus, StackResourceState, StackSettings, StackState, Vault,
};
use alien_error::AlienError;
use alien_infra::{
    deserialize_controller, MockPlatformServiceProvider, StackExecutor, StackResourceStateExt,
};
use serde_json::json;
use std::sync::Arc;

async fn delete_vault(error: ErrorData) -> StackResourceState {
    let mut client = MockKeyVaultManagementApi::new();
    client
        .expect_delete_vault()
        .times(1..)
        .returning(move |group, vault| {
            assert_eq!(group, "synthetic-group");
            assert_eq!(vault, "synthetic-vault");
            Err(AlienError::new(error.clone()))
        });
    let client = Arc::new(client);
    let mut provider = MockPlatformServiceProvider::new();
    provider
        .expect_get_azure_key_vault_management_client()
        .returning(move |_| Ok(client.clone()));
    let client_config = ClientConfig::Azure(Box::new(AzureClientConfig {
        subscription_id: "synthetic-subscription".into(),
        tenant_id: "synthetic-tenant".into(),
        region: Some("westus2".into()),
        credentials: AzureCredentials::ServicePrincipal {
            client_id: "synthetic".into(),
            client_secret: "synthetic".into(),
        },
        service_overrides: None,
    }));
    let config = DeploymentConfig::builder()
        .stack_settings(StackSettings::default())
        .environment_variables(EnvironmentVariablesSnapshot {
            variables: vec![],
            hash: String::new(),
            created_at: String::new(),
        })
        .external_bindings(ExternalBindings::default())
        .allow_frozen_changes(false)
        .build();
    let mut vault = StackResourceState::new_pending(
        "vault".into(),
        Resource::new(Vault::new("secrets".into()).build()),
        Some(ResourceLifecycle::Live),
        vec![],
    );
    vault.status = ResourceStatus::Running;
    vault
        .set_internal_controller(Some(
            deserialize_controller(json!({
                "type":"AzureVaultController", "_controllerStateVersion":1, "state":"ready",
                "vaultName":"synthetic-vault", "resourceGroupName":"synthetic-group",
                "vaultUri":"https://synthetic-vault.vault.azure.net"
            }))
            .unwrap(),
        ))
        .unwrap();
    let mut state = StackState::new(Platform::Azure);
    state.resources.insert("secrets".into(), vault);
    let executor = StackExecutor::for_deletion_with_service_provider(
        client_config,
        &config,
        Arc::new(provider),
        Some(vec![ResourceLifecycle::Live]),
    )
    .unwrap();
    for _ in 0..12 {
        state = executor.step(state).await.unwrap().next_state;
        state = serde_json::from_slice(&serde_json::to_vec(&state).unwrap()).unwrap();
        if matches!(
            state.resources["secrets"].status,
            ResourceStatus::Deleted | ResourceStatus::DeleteFailed
        ) {
            break;
        }
    }
    let vault = state.resources.remove("secrets").unwrap();
    println!("vault status after persisted deletion: {:?}", vault.status);
    vault
}

#[tokio::test]
async fn already_missing_azure_vault_finishes_deletion() {
    let vault = delete_vault(ErrorData::RemoteResourceNotFound {
        resource_type: "vault".into(),
        resource_name: "synthetic-vault".into(),
    })
    .await;
    assert_eq!(vault.status, ResourceStatus::Deleted);
    assert!(vault.error.is_none());
    if let Some(saved) = vault.internal_state {
        assert!(saved["vaultName"].is_null());
        assert!(saved["resourceGroupName"].is_null());
        assert!(saved["vaultUri"].is_null());
    }
}

#[tokio::test]
async fn denied_azure_vault_delete_preserves_identifiers() {
    let vault = delete_vault(ErrorData::RemoteAccessDenied {
        resource_type: "vault".into(),
        resource_name: "synthetic-vault".into(),
    })
    .await;
    assert_eq!(vault.status, ResourceStatus::DeleteFailed);
    assert!(vault.error.is_some());
    let saved = vault.internal_state.as_ref().unwrap();
    assert_eq!(saved["vaultName"], "synthetic-vault");
    assert_eq!(saved["resourceGroupName"], "synthetic-group");
    assert_eq!(saved["vaultUri"], "https://synthetic-vault.vault.azure.net");
    let failed = vault.last_failed_state.as_ref().unwrap();
    assert_eq!(failed["vaultName"], "synthetic-vault");
    assert_eq!(failed["resourceGroupName"], "synthetic-group");
}
