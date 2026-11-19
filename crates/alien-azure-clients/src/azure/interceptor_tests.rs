//! The interceptor hook through Alien's Azure Resource Manager clients, against a local server
//! that records the bytes on the wire.

use crate::azure::authorization::{AuthorizationApi, AzureAuthorizationClient};
use crate::azure::flexible_server::{AzureFlexibleServerClient, FlexibleServer, FlexibleServerApi};
use crate::azure::long_running_operation::{
    LongRunningOperation, LongRunningOperationApi, LongRunningOperationClient, OperationResult,
};
use crate::azure::managed_identity::{
    AzureManagedIdentityClient, FederatedCredentialProperties, FederatedIdentityCredential,
    ManagedIdentityApi,
};
use crate::azure::models::authorization_role_assignments::RoleAssignment;
use crate::azure::token_cache::AzureTokenCache;
use crate::azure::{AzureClientConfig, AzureCredentials, ServiceOverrides};
use alien_client_core::interceptor::{CallInterceptor, Cloud, HookRequest, InterceptorChain};
use alien_client_core::testing::capture::{self, Reply};
use alien_client_core::testing::TestInterceptor;
use alien_client_core::ErrorData;
use reqwest::header::HeaderValue;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

fn chain(interceptors: Vec<TestInterceptor>) -> InterceptorChain {
    InterceptorChain::new(
        interceptors
            .into_iter()
            .map(|interceptor| Arc::new(interceptor) as Arc<dyn CallInterceptor>)
            .collect(),
    )
}

fn azure_config(server: &capture::Server) -> AzureClientConfig {
    AzureClientConfig {
        subscription_id: "sub".into(),
        tenant_id: "tenant".into(),
        region: Some("eastus".into()),
        credentials: AzureCredentials::AccessToken {
            token: "azure-test-token".into(),
        },
        service_overrides: Some(ServiceOverrides {
            endpoints: HashMap::from([("management".to_string(), server.base_url.clone())]),
        }),
    }
}

fn flexible_server() -> FlexibleServer {
    serde_json::from_str(
        r#"{"location":"eastus","sku":{"name":"Standard_B1ms","tier":"Burstable"},
            "properties":{"version":"17","administratorLogin":"alien","administratorLoginPassword":"secret",
            "storage":{"storageSizeGB":32},"backup":{"backupRetentionDays":7},
            "network":{"publicNetworkAccess":"Disabled"}}}"#,
    )
    .unwrap()
}

/// The flexibleServers PUT carries the property the interceptor added, with a Content-Length
/// that matches the new body and Alien's token, and the interceptor never saw the token.
#[tokio::test]
async fn flexible_server_put_carries_an_added_property() {
    let server =
        capture::start(|request| Reply::new(200, "application/json", request.body.clone())).await;
    let seen: Arc<Mutex<Vec<HookRequest>>> = Arc::default();
    let spy = seen.clone();
    let auth_config =
        TestInterceptor::new("acme", Cloud::Azure, "management").on_request(move |request| {
            spy.lock().unwrap().push(request.clone());
            let mut body: serde_json::Value =
                serde_json::from_str(request.body.as_deref().unwrap()).unwrap();
            body["properties"]["authConfig"] =
                serde_json::json!({ "activeDirectoryAuth": "Enabled", "tenantId": "tenant" });
            request.body = Some(body.to_string());
            Ok(())
        });
    let mut client = AzureFlexibleServerClient::new(
        reqwest::Client::new(),
        AzureTokenCache::new(azure_config(&server)),
    );
    client.base.interceptors = chain(vec![auth_config]);
    client
        .create_or_update_server("rg", "pg1", &flexible_server())
        .await
        .unwrap();

    let sent = server.requests();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].method, "PUT");
    assert_eq!(
        sent[0].target,
        "/subscriptions/sub/resourceGroups/rg/providers/Microsoft.DBforPostgreSQL/flexibleServers/pg1?api-version=2024-08-01"
    );
    assert_eq!(
        sent[0].header("authorization"),
        Some("Bearer azure-test-token")
    );
    assert_eq!(
        sent[0].header("content-length").unwrap(),
        sent[0].body.len().to_string()
    );
    let body: serde_json::Value = serde_json::from_slice(&sent[0].body).unwrap();
    assert_eq!(
        body["properties"]["authConfig"]["activeDirectoryAuth"],
        "Enabled"
    );
    assert_eq!(body["properties"]["version"], "17");
    let seen = seen.lock().unwrap();
    assert_eq!(
        seen.len(),
        1,
        "the request hook runs once, outside the retries"
    );
    assert!(seen[0].headers.get("authorization").is_none());
}

/// Azure makes Alien poll the URL a response names, with its token. A response interceptor that
/// points that URL at another host is refused when Alien signs the poll, and that host hears
/// nothing. A client without interceptors still follows the URL, which is why the check exists.
#[tokio::test]
async fn a_rewritten_operation_url_does_not_get_the_azure_token() {
    let azure = capture::start(|request| {
        if request.method == "PUT" {
            Reply::new(201, "application/json", "{}").header("azure-asyncoperation", "PLACEHOLDER")
        } else {
            Reply::new(200, "application/json", r#"{"status":"Succeeded"}"#)
        }
    })
    .await;
    let elsewhere =
        capture::start(|_| Reply::new(200, "application/json", r#"{"status":"Succeeded"}"#)).await;
    let target = format!("{}/steal", elsewhere.base_url);
    let rewrite = move || {
        let target = target.clone();
        TestInterceptor::new("acme", Cloud::Azure, "management").on_response(move |_, response| {
            response.headers.insert(
                "azure-asyncoperation",
                HeaderValue::from_str(&target).unwrap(),
            );
            Ok(())
        })
    };
    let config = azure_config(&azure);
    let mut client = AzureFlexibleServerClient::new(
        reqwest::Client::new(),
        AzureTokenCache::new(config.clone()),
    );
    client.base.interceptors = chain(vec![rewrite()]);
    let OperationResult::LongRunning(operation) = client
        .create_or_update_server("rg", "pg1", &flexible_server())
        .await
        .unwrap()
    else {
        panic!("expected a long-running operation");
    };
    assert!(
        operation.url.starts_with(&elsewhere.base_url),
        "the response was rewritten"
    );

    let mut poller = LongRunningOperationClient::new(
        reqwest::Client::new(),
        AzureTokenCache::new(config.clone()),
    );
    poller.base.interceptors = chain(vec![rewrite()]);
    let error = poller
        .check_status(&operation, "CreateOrUpdateFlexibleServer", "pg1")
        .await
        .expect_err("polling another host is refused");
    assert!(
        matches!(error.error, Some(ErrorData::DestinationRefused { .. })),
        "{error}"
    );
    assert!(!error.retryable);
    assert!(elsewhere.requests().is_empty());

    let unchecked =
        LongRunningOperationClient::new(reqwest::Client::new(), AzureTokenCache::new(config));
    let _ = unchecked
        .check_status(&operation, "CreateOrUpdateFlexibleServer", "pg1")
        .await;
    let leaked = elsewhere.requests();
    assert_eq!(leaked.len(), 1);
    assert_eq!(
        leaked[0].header("authorization"),
        Some("Bearer azure-test-token")
    );
}

/// The result an operation's Location URL returns goes through the response hook too.
#[tokio::test]
async fn an_operation_result_goes_through_the_response_hook() {
    let azure = capture::start(|_| {
        Reply::new(200, "application/json", r#"{"name":"pg1","state":"Ready"}"#)
    })
    .await;
    let mut poller = LongRunningOperationClient::new(
        reqwest::Client::new(),
        AzureTokenCache::new(azure_config(&azure)),
    );
    poller.base.interceptors = chain(vec![TestInterceptor::new(
        "acme",
        Cloud::Azure,
        "management",
    )
    .on_response(|request, response| {
        assert_eq!(request.url.path(), "/operations/1/result");
        response.body = response.body.replace("Ready", "SeenByExtension");
        Ok(())
    })]);
    let operation = LongRunningOperation {
        url: format!("{}/operations/1", azure.base_url),
        retry_after: None,
        location_url: Some(format!("{}/operations/1/result", azure.base_url)),
    };
    let result: serde_json::Value = poller
        .fetch_location_result(&operation, "Restart", "pg1")
        .await
        .unwrap();
    assert_eq!(result["state"], "SeenByExtension");
}

/// Azure's identity clients send their calls as Alien built them: a role assignment and a
/// federated credential are never offered, so an interceptor never asked about them can't change
/// who they grant access to.
#[tokio::test]
async fn role_assignments_and_federated_credentials_are_never_offered() {
    let server =
        capture::start(|request| Reply::new(200, "application/json", request.body.clone())).await;
    let never_asked = || {
        chain(vec![TestInterceptor::new(
            "acme",
            Cloud::Azure,
            "management",
        )
        .when(|request| {
            panic!("{} {} was offered", request.method, request.url)
        })])
    };

    let mut authorization = AzureAuthorizationClient::new(
        reqwest::Client::new(),
        AzureTokenCache::new(azure_config(&server)),
    );
    authorization.base.interceptors = never_asked();
    let assignment: RoleAssignment = serde_json::from_value(serde_json::json!({
        "properties": {
            "roleDefinitionId": "/subscriptions/sub/providers/Microsoft.Authorization/roleDefinitions/reader",
            "principalId": "app-principal"
        }
    }))
    .unwrap();
    authorization
        .create_or_update_role_assignment_by_id(
            "/subscriptions/sub/providers/Microsoft.Authorization/roleAssignments/a".into(),
            &assignment,
        )
        .await
        .unwrap();

    let mut identities = AzureManagedIdentityClient::new(
        reqwest::Client::new(),
        AzureTokenCache::new(azure_config(&server)),
    );
    identities.base.interceptors = never_asked();
    identities
        .create_or_update_federated_credential(
            "rg",
            "app",
            "operator",
            &FederatedIdentityCredential {
                id: None,
                name: None,
                type_: None,
                properties: Some(FederatedCredentialProperties {
                    issuer: "https://issuer.example".into(),
                    subject: "system:serviceaccount:ns:app".into(),
                    audiences: vec!["api://AzureADTokenExchange".into()],
                }),
            },
        )
        .await
        .unwrap();

    let sent = server.requests();
    assert_eq!(sent.len(), 2);
    let body: serde_json::Value = serde_json::from_slice(&sent[0].body).unwrap();
    assert_eq!(body["properties"]["principalId"], "app-principal");
    let body: serde_json::Value = serde_json::from_slice(&sent[1].body).unwrap();
    assert_eq!(
        body["properties"]["subject"],
        "system:serviceaccount:ns:app"
    );
}

/// An interceptor that moves a database's PUT to a role assignment fails the call, naming the
/// extension, and nothing is sent.
#[tokio::test]
async fn a_resource_call_turned_into_a_role_assignment_is_refused() {
    let server =
        capture::start(|request| Reply::new(200, "application/json", request.body.clone())).await;
    let assigns_a_role =
        TestInterceptor::new("acme", Cloud::Azure, "management").on_request(|request| {
            request.url.set_path(
                "/subscriptions/sub/resourceGroups/rg/providers/Microsoft.Authorization/roleAssignments/a",
            );
            Ok(())
        });
    let mut client = AzureFlexibleServerClient::new(
        reqwest::Client::new(),
        AzureTokenCache::new(azure_config(&server)),
    );
    client.base.interceptors = chain(vec![assigns_a_role]);
    let error = client
        .create_or_update_server("rg", "pg1", &flexible_server())
        .await
        .unwrap_err();
    assert!(!error.retryable, "{error}");
    assert!(
        matches!(&error.error, Some(ErrorData::InterceptorFailed { extension, .. }) if extension == "acme"),
        "{error}"
    );
    assert!(server.requests().is_empty());
}
