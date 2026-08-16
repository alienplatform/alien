use std::collections::HashMap;

use alien_azure_clients::azure::ServiceOverrides;
use alien_azure_clients::{
    AzureClientConfig, AzureCognitiveServicesClient, AzureCredentials, AzureTokenCache,
    CognitiveServicesAccountsApi,
};
use httpmock::prelude::*;

fn client(server: &MockServer) -> AzureCognitiveServicesClient {
    let config = AzureClientConfig {
        subscription_id: "test-subscription".to_string(),
        tenant_id: "test-tenant".to_string(),
        region: Some("eastus".to_string()),
        credentials: AzureCredentials::AccessToken {
            token: "test-token".to_string(),
        },
        service_overrides: Some(ServiceOverrides {
            endpoints: HashMap::from([("management".to_string(), server.base_url())]),
        }),
    };
    AzureCognitiveServicesClient::new(reqwest::Client::new(), AzureTokenCache::new(config))
}

#[tokio::test]
async fn lists_configured_deployments_without_invoking_models() {
    let server = MockServer::start_async().await;
    let request = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/subscriptions/test-subscription/resourceGroups/test-rg/providers/Microsoft.CognitiveServices/accounts/test-ai/deployments")
                .query_param("api-version", "2024-10-01")
                .header("authorization", "Bearer test-token");
            then.status(200).json_body_obj(&serde_json::json!({
                "value": [{
                    "name": "gpt-4.1",
                    "properties": {
                        "model": {"format": "OpenAI", "name": "gpt-4.1", "version": "2025-04-14"},
                        "provisioningState": "Succeeded"
                    }
                }]
            }));
        })
        .await;

    let deployments = client(&server)
        .list_deployments("test-rg", "test-ai")
        .await
        .expect("deployment list");

    request.assert_async().await;
    assert_eq!(deployments.len(), 1);
    assert_eq!(deployments[0].name.as_deref(), Some("gpt-4.1"));
}
