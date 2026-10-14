use std::{collections::HashMap, time::Duration};

use alien_azure_clients::{
    AzureClientConfig, AzureContainerAppsClient, AzureCredentials, AzureTokenCache,
    ContainerAppsApi,
    azure::{
        ServiceOverrides,
        long_running_operation::OperationResult,
        models::container_apps::{IpSecurityRestrictionRule, IpSecurityRestrictionRuleAction},
    },
};
use httpmock::prelude::*;
use serde_json::json;

const APP_PATH: &str = "/subscriptions/test-subscription/resourceGroups/test-rg/providers/Microsoft.App/containerApps/test-app";

fn client(server: &MockServer) -> AzureContainerAppsClient {
    AzureContainerAppsClient::new(
        reqwest::Client::new(),
        AzureTokenCache::new(AzureClientConfig {
            subscription_id: "test-subscription".to_string(),
            tenant_id: "test-tenant".to_string(),
            region: Some("eastus".to_string()),
            credentials: AzureCredentials::AccessToken {
                token: "test-token".to_string(),
            },
            service_overrides: Some(ServiceOverrides {
                endpoints: HashMap::from([("management".to_string(), server.base_url())]),
            }),
        }),
    )
}

#[tokio::test]
async fn replaces_restrictions_without_sending_other_app_settings() {
    let server = MockServer::start_async().await;
    let request = server
        .mock_async(|when, then| {
            when.method(PATCH)
                .path(APP_PATH)
                .query_param("api-version", "2025-07-01")
                .header("authorization", "Bearer test-token")
                .header("content-type", "application/json")
                .json_body(json!({"properties":{"configuration":{"ingress":{
                    "ipSecurityRestrictions":[{
                        "name":"gateway", "action":"Allow", "ipAddressRange":"192.0.2.10/32"
                    }]
                }}}}));
            then.status(200)
                .json_body(json!({"name":"test-app", "location":"eastus"}));
        })
        .await;

    let result = client(&server)
        .update_container_app_ip_security_restrictions(
            "test-rg",
            "test-app",
            &[IpSecurityRestrictionRule {
                name: "gateway".to_string(),
                action: IpSecurityRestrictionRuleAction::Allow,
                ip_address_range: "192.0.2.10/32".to_string(),
                description: None,
            }],
        )
        .await
        .expect("restriction update");
    request.assert_async().await;
    let OperationResult::Completed(app) = result else {
        panic!("expected immediate app response");
    };
    assert_eq!(app.name.as_deref(), Some("test-app"));
}

#[tokio::test]
async fn clears_restrictions_explicitly_and_retains_async_operation() {
    let server = MockServer::start_async().await;
    let poll_url = format!("{}/operations/update", server.base_url());
    let request = server
        .mock_async(|when, then| {
            when.method(PATCH)
                .path(APP_PATH)
                .query_param("api-version", "2025-07-01")
                .header("authorization", "Bearer test-token")
                .json_body(json!({"properties":{"configuration":{"ingress":{
                    "ipSecurityRestrictions":[]
                }}}}));
            then.status(202)
                .header("azure-asyncoperation", &poll_url)
                .header("retry-after", "3");
        })
        .await;

    let result = client(&server)
        .update_container_app_ip_security_restrictions("test-rg", "test-app", &[])
        .await
        .expect("clear restrictions");
    request.assert_async().await;
    let OperationResult::LongRunning(operation) = result else {
        panic!("expected async operation");
    };
    assert_eq!(operation.url, poll_url);
    assert_eq!(operation.retry_after, Some(Duration::from_secs(3)));
}

#[tokio::test]
async fn missing_app_is_an_error_without_creation_fallback() {
    let server = MockServer::start_async().await;
    let request = server
        .mock_async(|when, then| {
            when.method(PATCH).path(APP_PATH);
            then.status(404).json_body(json!({"error":{
                "code":"ResourceNotFound", "message":"Container app does not exist"
            }}));
        })
        .await;
    let error = client(&server)
        .update_container_app_ip_security_restrictions("test-rg", "test-app", &[])
        .await
        .expect_err("missing app must fail");
    request.assert_async().await;
    assert_eq!(error.code, "REMOTE_RESOURCE_NOT_FOUND");
}
