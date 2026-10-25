use alien_azure_clients::blob_containers::{AzureBlobContainerClient, BlobContainerApi};
use alien_azure_clients::models::{blob::BlobServiceProperties, storage::ManagementPolicy};
use alien_azure_clients::{AzureClientConfig, AzureCredentials, AzureTokenCache};
use alien_client_core::ErrorData;
use httpmock::prelude::*;
use serde_json::json;

fn client(server: &MockServer) -> AzureBlobContainerClient {
    AzureBlobContainerClient::new(
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
    )
}

fn policy() -> serde_json::Value {
    json!({"properties":{"policy":{"rules":[
        {"enabled":true,"name":"first","type":"Lifecycle","definition":{
            "filters":{"blobTypes":["blockBlob"],"prefixMatch":["first/tmp/"]},
            "actions":{"baseBlob":{"delete":{"daysAfterModificationGreaterThan":30.0}}}}},
        {"enabled":false,"name":"second","type":"Lifecycle","definition":{
            "filters":{"blobTypes":["blockBlob"],"prefixMatch":["second/"]},
            "actions":{"version":{"delete":{"daysAfterCreationGreaterThan":60.0}}}}}
    ]}}})
}

#[tokio::test]
async fn shared_settings_round_trip_preserves_both_containers_and_disabled_rules() {
    let server = MockServer::start_async().await;
    let client = client(&server);
    let properties = json!({"properties": {
        "isVersioningEnabled":true,
        "deleteRetentionPolicy":{"enabled":true,"days":7},
        "cors":{"corsRules":[{"allowedOrigins":["https://example.com"],
            "allowedMethods":["GET","HEAD","PUT","POST","DELETE","OPTIONS"],
            "allowedHeaders":["*"],"exposedHeaders":["ETag","Content-Range","Accept-Ranges"],
            "maxAgeInSeconds":3600}]}
    }});
    let service = server.mock(|when, then| {
        when.method(PUT)
            .path("/subscriptions/test-sub/resourceGroups/test-rg/providers/Microsoft.Storage/storageAccounts/testaccount/blobServices/default")
            .query_param("api-version", "2024-01-01")
            .header("authorization", "Bearer test-token")
            .json_body(properties.clone());
        then.status(200).json_body(properties.clone());
    });
    let input: BlobServiceProperties = serde_json::from_value(properties.clone()).unwrap();
    let output = client
        .set_blob_service_properties("test-rg", "testaccount", &input)
        .await
        .unwrap();
    assert_eq!(serde_json::to_value(output).unwrap(), properties);
    service.assert_hits(1);

    let expected = policy();
    let write = server.mock(|when, then| {
        when.method(PUT)
            .path("/subscriptions/test-sub/resourceGroups/test-rg/providers/Microsoft.Storage/storageAccounts/testaccount/managementPolicies/default")
            .query_param("api-version", "2024-01-01")
            .header("authorization", "Bearer test-token")
            .json_body(expected.clone());
        then.status(200).json_body(expected.clone());
    });
    let read = server.mock(|when, then| {
        when.method(GET)
            .path("/subscriptions/test-sub/resourceGroups/test-rg/providers/Microsoft.Storage/storageAccounts/testaccount/managementPolicies/default")
            .query_param("api-version", "2024-01-01")
            .header("authorization", "Bearer test-token");
        then.status(200).json_body(expected.clone());
    });
    let input: ManagementPolicy = serde_json::from_value(expected.clone()).unwrap();
    let output = client
        .set_management_policy("test-rg", "testaccount", &input)
        .await
        .unwrap();
    assert_eq!(serde_json::to_value(output).unwrap(), expected);
    let output = client
        .get_management_policy("test-rg", "testaccount")
        .await
        .unwrap();
    assert_eq!(serde_json::to_value(output).unwrap(), expected);
    write.assert_hits(1);
    read.assert_hits(1);
}

#[tokio::test]
async fn shared_settings_denials_and_absence_are_errors_without_hidden_retry() {
    for status in [403, 404, 409, 500] {
        let server = MockServer::start_async().await;
        let request = server.mock(|when, then| {
            when.path("/subscriptions/test-sub/resourceGroups/test-rg/providers/Microsoft.Storage/storageAccounts/testaccount/managementPolicies/default");
            then.status(status).json_body(json!({"error":{"code":"TestFailure","message":"denied"}}));
        });
        let client = client(&server);
        let error = client
            .get_management_policy("test-rg", "testaccount")
            .await
            .unwrap_err();
        if status == 404 {
            assert!(matches!(
                error.error,
                Some(ErrorData::RemoteResourceNotFound { .. })
            ));
        }
        let input: ManagementPolicy = serde_json::from_value(policy()).unwrap();
        client
            .set_management_policy("test-rg", "testaccount", &input)
            .await
            .unwrap_err();
        request.assert_hits(2);
    }
}

#[tokio::test]
async fn malformed_success_cannot_confirm_settings() {
    let server = MockServer::start_async().await;
    let request = server.mock(|when, then| {
        when.method(PUT);
        then.status(200).body("not a response");
    });
    let input: BlobServiceProperties = serde_json::from_value(
        json!({"properties":{"cors":{"corsRules":[]},"isVersioningEnabled":false}}),
    )
    .unwrap();
    client(&server)
        .set_blob_service_properties("test-rg", "testaccount", &input)
        .await
        .unwrap_err();
    request.assert_hits(1);
}
