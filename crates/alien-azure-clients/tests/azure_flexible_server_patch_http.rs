use alien_azure_clients::azure::flexible_server::{
    AzureFlexibleServerClient, FlexibleServerApi, FlexibleServerHighAvailability,
    FlexibleServerUpdate, FlexibleServerUpdateProperties,
};
use alien_azure_clients::{AzureClientConfig, AzureCredentials, AzureTokenCache};
use httpmock::{prelude::*, Method::PATCH};
use serde_json::json;

#[tokio::test]
async fn patch_preserves_unrelated_settings_and_sends_once() {
    for status in [200, 202, 403, 409, 500] {
        let server = MockServer::start_async().await;
        let patch = server.mock(|when, then| {
            when.method(PATCH)
                .path("/subscriptions/test-sub/resourceGroups/test-rg/providers/Microsoft.DBforPostgreSQL/flexibleServers/test-db")
                .query_param("api-version", "2024-08-01")
                .header("authorization", "Bearer test-token")
                .json_body(json!({"properties": {"highAvailability": {"mode": "Disabled"}}}));
            then.status(status)
                .header("azure-asyncoperation", server.url("/operation"))
                .header("retry-after", "10")
                .json_body(json!({"error": {"code": "TestFailure", "message": "denied"}}));
        });
        let client = AzureFlexibleServerClient::new(
            reqwest::Client::new(),
            AzureTokenCache::new(AzureClientConfig {
                subscription_id: "test-sub".into(),
                tenant_id: "test-tenant".into(),
                region: Some("eastus".into()),
                credentials: AzureCredentials::AccessToken {
                    token: "test-token".into(),
                },
                service_overrides: Some(alien_azure_clients::azure::ServiceOverrides {
                    endpoints: std::collections::HashMap::from([(
                        "management".into(),
                        server.base_url(),
                    )]),
                }),
            }),
        );
        let result = client
            .update_server(
                "test-rg",
                "test-db",
                &FlexibleServerUpdate {
                    sku: None,
                    properties: FlexibleServerUpdateProperties {
                        storage: None,
                        high_availability: Some(FlexibleServerHighAvailability {
                            mode: "Disabled".into(),
                            state: None,
                        }),
                    },
                },
            )
            .await;
        if status < 300 {
            let operation = result
                .unwrap()
                .expect("accepted update retains operation even on HTTP 200");
            assert_eq!(operation.url, server.url("/operation"));
            assert_eq!(operation.retry_after.unwrap().as_secs(), 10);
        } else {
            assert!(result.is_err());
        }
        patch.assert_hits(1);
    }
}
