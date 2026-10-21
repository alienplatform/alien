use alien_client_core::ErrorData;
use alien_gcp_clients::gcp::compute::{
    BackendService, ComputeApi, ComputeClient, ForwardingRule, HealthCheck, NetworkEndpointGroup,
    OperationStatus,
};
use alien_gcp_clients::{GcpClientConfig, GcpCredentials, ServiceOverrides};
use httpmock::prelude::*;
use serde_json::json;
use std::collections::HashMap;

fn client(server: &MockServer) -> ComputeClient {
    ComputeClient::new(
        reqwest::Client::new(),
        GcpClientConfig {
            project_id: "test-project".into(),
            region: "us-central1".into(),
            credentials: GcpCredentials::AccessToken {
                token: "test-token".into(),
            },
            service_overrides: Some(ServiceOverrides {
                endpoints: HashMap::from([("compute".into(), server.base_url())]),
            }),
            project_number: None,
        },
    )
}

#[tokio::test]
async fn regional_backend_lifecycle_uses_regional_routes_and_returns_pending_operations() {
    let server = MockServer::start_async().await;
    let body = json!({"name":"gateway", "network":"projects/test-project/global/networks/app", "protocol":"TCP", "loadBalancingScheme":"INTERNAL", "healthChecks":["projects/test-project/regions/us-central1/healthChecks/gateway"], "backends":[{"group":"projects/test-project/zones/us-central1-a/networkEndpointGroups/gateway", "balancingMode":"CONNECTION"}]});
    let create = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/projects/test-project/regions/us-central1/backendServices")
                .header("authorization", "Bearer test-token")
                .json_body(body.clone());
            then.status(200)
                .json_body(json!({"name":"create", "status":"PENDING"}));
        })
        .await;
    let read = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/projects/test-project/regions/us-central1/backendServices/gateway");
            then.status(200).json_body(body.clone());
        })
        .await;
    let patch = server
        .mock_async(|when, then| {
            when.method(httpmock::Method::PATCH)
                .path("/projects/test-project/regions/us-central1/backendServices/gateway")
                .json_body(body.clone());
            then.status(200)
                .json_body(json!({"name":"update", "status":"PENDING"}));
        })
        .await;
    let health = server.mock_async(|when, then| {
        when.method(POST).path("/projects/test-project/regions/us-central1/backendServices/gateway/getHealth")
            .json_body(json!({"group":"projects/test-project/zones/us-central1-a/networkEndpointGroups/gateway"}));
        then.status(200).json_body(json!({"healthStatus":[{"ipAddress":"10.0.0.5", "port":8001, "healthState":"HEALTHY"}]}));
    }).await;
    let delete = server
        .mock_async(|when, then| {
            when.method(DELETE)
                .path("/projects/test-project/regions/us-central1/backendServices/gateway");
            then.status(200)
                .json_body(json!({"name":"delete", "status":"PENDING"}));
        })
        .await;
    let api = client(&server);
    let backend: BackendService = serde_json::from_value(body).unwrap();
    assert_eq!(
        api.insert_regional_backend_service("us-central1".into(), backend.clone())
            .await
            .unwrap()
            .status,
        Some(OperationStatus::Pending)
    );
    assert_eq!(
        api.get_regional_backend_service("us-central1".into(), "gateway".into())
            .await
            .unwrap()
            .network,
        backend.network
    );
    assert_eq!(
        api.patch_regional_backend_service("us-central1".into(), "gateway".into(), backend)
            .await
            .unwrap()
            .status,
        Some(OperationStatus::Pending)
    );
    let health_result = api
        .get_regional_backend_service_health(
            "us-central1".into(),
            "gateway".into(),
            "projects/test-project/zones/us-central1-a/networkEndpointGroups/gateway".into(),
        )
        .await
        .unwrap();
    assert_eq!(health_result.health_status.len(), 1);
    assert_eq!(
        health_result.health_status[0].ip_address.as_deref(),
        Some("10.0.0.5")
    );
    assert_eq!(
        api.delete_regional_backend_service("us-central1".into(), "gateway".into())
            .await
            .unwrap()
            .status,
        Some(OperationStatus::Pending)
    );
    for request in [create, read, patch, health, delete] {
        request.assert_async().await;
    }
}

#[tokio::test]
async fn regional_health_checks_preserve_errors_and_do_not_poll_mutations() {
    let server = MockServer::start_async().await;
    let body = json!({"name":"gateway", "type":"TCP", "tcpHealthCheck":{"port":8001}});
    let create = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/projects/test-project/regions/us-central1/healthChecks")
                .json_body(body.clone());
            then.status(200)
                .json_body(json!({"name":"create", "status":"PENDING"}));
        })
        .await;
    let read = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/projects/test-project/regions/us-central1/healthChecks/gateway");
            then.status(200).json_body(body.clone());
        })
        .await;
    let delete = server.mock_async(|when, then| {
        when.method(DELETE).path("/projects/test-project/regions/us-central1/healthChecks/gateway");
        then.status(403).json_body(json!({"error":{"code":403,"status":"PERMISSION_DENIED","message":"Missing compute.healthChecks.delete"}}));
    }).await;
    let api = client(&server);
    let check: HealthCheck = serde_json::from_value(body).unwrap();
    assert_eq!(
        api.insert_regional_health_check("us-central1".into(), check)
            .await
            .unwrap()
            .status,
        Some(OperationStatus::Pending)
    );
    let read_check = api
        .get_regional_health_check("us-central1".into(), "gateway".into())
        .await
        .unwrap();
    assert_eq!(read_check.tcp_health_check.unwrap().port, Some(8001));
    let error = api
        .delete_regional_health_check("us-central1".into(), "gateway".into())
        .await
        .unwrap_err();
    assert!(matches!(
        error.error,
        Some(ErrorData::RemoteAccessDenied { .. })
    ));
    create.assert_async().await;
    read.assert_async().await;
    assert!(delete.hits_async().await > 0); // The shared client owns permission-propagation retries.
}

#[tokio::test]
async fn internal_forwarding_rule_and_ip_only_neg_reach_the_provider_without_losing_fields() {
    let server = MockServer::start_async().await;
    let forwarding = json!({"name":"gateway", "IPAddress":"10.0.0.9", "IPProtocol":"TCP", "ports":["8001","8002"], "loadBalancingScheme":"INTERNAL", "network":"projects/test-project/global/networks/app", "subnetwork":"projects/test-project/regions/us-central1/subnetworks/app", "backendService":"projects/test-project/regions/us-central1/backendServices/gateway", "allowGlobalAccess":true});
    let neg = json!({"name":"gateway", "networkEndpointType":"GCE_VM_IP", "network":"projects/test-project/global/networks/app", "subnetwork":"projects/test-project/regions/us-central1/subnetworks/app"});
    let forwarding_call = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/projects/test-project/regions/us-central1/forwardingRules")
                .json_body(forwarding.clone());
            then.status(200)
                .json_body(json!({"name":"forwarding", "status":"PENDING"}));
        })
        .await;
    let neg_call = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/projects/test-project/zones/us-central1-a/networkEndpointGroups")
                .json_body(neg.clone());
            then.status(200)
                .json_body(json!({"name":"neg", "status":"PENDING"}));
        })
        .await;
    let api = client(&server);
    let rule: ForwardingRule = serde_json::from_value(forwarding).unwrap();
    assert_eq!(
        api.insert_forwarding_rule("us-central1".into(), rule)
            .await
            .unwrap()
            .status,
        Some(OperationStatus::Pending)
    );
    let group: NetworkEndpointGroup = serde_json::from_value(neg).unwrap();
    assert_eq!(
        api.insert_network_endpoint_group("us-central1-a".into(), group)
            .await
            .unwrap()
            .status,
        Some(OperationStatus::Pending)
    );
    forwarding_call.assert_async().await;
    neg_call.assert_async().await;
}
