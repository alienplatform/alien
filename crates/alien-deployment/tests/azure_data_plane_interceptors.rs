//! Azure data-plane clients never carry the deployment's interceptors. The extension here has
//! one Azure interceptor whose hook traps on every call, so a client that asks it whether it
//! matches fails with INTERCEPTOR_FAILED and sends nothing: an Azure Resource Manager client
//! does, and the Key Vault, Service Bus and sandbox data-plane clients reach the server.

use alien_azure_clients::azure::flexible_server::FlexibleServer;
use alien_infra::{DefaultPlatformServiceProvider, LoadedExtensions, PlatformServiceProvider};
use std::collections::HashMap;
use std::sync::Arc;

fn provider() -> DefaultPlatformServiceProvider {
    let stack: alien_core::Stack = serde_json::from_value(serde_json::json!({
        "id": "app",
        "resources": {},
        "extensions": [{
            "name": "trap", "version": "1.0.0", "digest": "sha256:trap", "resourceTypes": {}
        }]
    }))
    .unwrap();
    DefaultPlatformServiceProvider::default()
        .with_extensions(
            &stack,
            Arc::new(LoadedExtensions::default().with_hooks(
                "trap",
                alien_deployment::testing::hooks_module(
                    "trap",
                    r#"[{"name":"all","kind":"azureInterceptor"}]"#,
                    "unreachable",
                ),
            )),
        )
        .unwrap()
}

fn config(base: &str) -> alien_azure_clients::AzureClientConfig {
    alien_azure_clients::AzureClientConfig {
        subscription_id: "sub".into(),
        tenant_id: "tenant".into(),
        region: Some("eastus".into()),
        credentials: alien_azure_clients::AzureCredentials::AccessToken {
            token: "azure-test-token".into(),
        },
        service_overrides: Some(alien_azure_clients::ServiceOverrides {
            endpoints: HashMap::from([
                ("management".to_string(), base.to_string()),
                ("keyvault".to_string(), base.to_string()),
                ("servicebus".to_string(), base.to_string()),
                ("sandboxDataPlane".to_string(), base.to_string()),
            ]),
        }),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn azure_data_plane_clients_carry_no_interceptors() {
    let server = httpmock::MockServer::start_async().await;
    let any = server
        .mock_async(|_when, then| {
            then.status(200)
                .header("content-type", "application/json")
                .body("{}");
        })
        .await;
    let config = config(&server.base_url());
    let provider = provider();

    // An Azure Resource Manager client asks the hook whether it matches, which fails the call
    // before it is sent.
    let server_config: FlexibleServer = serde_json::from_value(serde_json::json!({
        "location": "eastus",
        "sku": { "name": "Standard_B1ms", "tier": "Burstable" },
        "properties": {
            "version": "17",
            "administratorLogin": "alien",
            "storage": { "storageSizeGB": 32 },
            "backup": { "backupRetentionDays": 7 },
            "network": { "publicNetworkAccess": "Disabled" }
        }
    }))
    .unwrap();
    let error = provider
        .get_azure_flexible_server_client(&config)
        .unwrap()
        .create_or_update_server("rg", "pg", &server_config)
        .await
        .unwrap_err();
    assert_eq!(error.code, "INTERCEPTOR_FAILED", "{error}");
    assert_eq!(any.hits_async().await, 0, "the ARM call was never sent");

    let key_vault = provider
        .get_azure_key_vault_secrets_client(&config)
        .unwrap()
        .get_secret(server.base_url(), "s".into(), None)
        .await;
    if let Err(error) = &key_vault {
        assert_ne!(error.code, "INTERCEPTOR_FAILED", "{error}");
    }
    let after_key_vault = any.hits_async().await;
    assert!(after_key_vault >= 1, "Key Vault reached the server");

    let service_bus = provider
        .get_azure_service_bus_data_plane_client(&config)
        .unwrap()
        .send_message_batch("ns".into(), "q".into(), vec!["{}".into()])
        .await;
    if let Err(error) = &service_bus {
        assert_ne!(error.code, "INTERCEPTOR_FAILED", "{error}");
    }
    let after_service_bus = any.hits_async().await;
    assert!(
        after_service_bus > after_key_vault,
        "Service Bus reached the server"
    );

    let sandbox = provider
        .get_azure_sandbox_data_plane_client(&config, "eastus", "rg")
        .unwrap()
        .get_sandbox("group", "sb-1")
        .await;
    if let Err(error) = &sandbox {
        assert_ne!(error.code, "INTERCEPTOR_FAILED", "{error}");
    }
    assert!(
        any.hits_async().await > after_service_bus,
        "the sandbox data plane reached the server"
    );
}
