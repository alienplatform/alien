//! Image import against a local ARM stand-in.

use std::collections::HashMap;

use super::{AzureContainerRegistryClient, ContainerRegistryApi};
use crate::{
    azure::ServiceOverrides,
    long_running_operation::{LongRunningOperation, OperationResult},
    models::containerregistry::ImportImageParameters,
    AzureClientConfig, AzureCredentials, AzureTokenCache,
};
use httpmock::prelude::*;
use serde_json::json;

fn client(server: &MockServer) -> AzureContainerRegistryClient {
    AzureContainerRegistryClient::new(
        reqwest::Client::new(),
        AzureTokenCache::new(AzureClientConfig {
            subscription_id: "test-sub".into(),
            tenant_id: "test-tenant".into(),
            region: Some("eastus".into()),
            credentials: AzureCredentials::AccessToken {
                token: "test-token".into(),
            },
            service_overrides: Some(ServiceOverrides {
                endpoints: HashMap::from([("management".into(), server.base_url())]),
            }),
        }),
    )
}

fn parameters() -> ImportImageParameters {
    serde_json::from_value(json!({
        "source": {
            "sourceImage": format!("app@sha256:{}", "a".repeat(64)),
            "registryUri": "source.example",
            "credentials": {"username":"deployment", "password":"fixture-private-source"}
        },
        "untaggedTargetRepositories": ["app"],
        "mode":"NoForce"
    }))
    .unwrap()
}

#[tokio::test]
async fn imports_digest_with_empty_completion_or_scoped_operation_handle() {
    for status in [200, 202] {
        let server = MockServer::start_async().await;
        let params = parameters();
        let request = server.mock(|when, then| {
            when.method(POST)
                .path("/subscriptions/test-sub/resourceGroups/test-rg/providers/Microsoft.ContainerRegistry/registries/testregistry/importImage")
                .query_param("api-version", "2025-04-01")
                .header("authorization", "Bearer test-token")
                .json_body(serde_json::to_value(&params).unwrap());
            let then = then.status(status);
            if status == 202 { then.header("azure-asyncoperation", server.url("/operation")); }
        });
        match client(&server)
            .import_image("test-rg", "testregistry", &params)
            .await
            .unwrap()
        {
            OperationResult::Completed(()) => assert_eq!(status, 200),
            OperationResult::LongRunning(operation) => {
                assert_eq!(status, 202);
                assert_eq!(operation.url, server.url("/operation"));
            }
        }
        request.assert_hits(1);
    }
}

#[tokio::test]
async fn import_errors_do_not_retain_source_password_or_echoed_response() {
    for (status, code) in [
        (401, "AUTHENTICATION_ERROR"),
        (429, "RATE_LIMIT_EXCEEDED"),
        (500, "HTTP_RESPONSE_ERROR"),
    ] {
        let server = MockServer::start_async().await;
        let request = server.mock(|when, then| {
            when.method(POST);
            then.status(status).body("fixture-private-source");
        });
        let error = client(&server)
            .import_image("test-rg", "testregistry", &parameters())
            .await
            .unwrap_err();
        assert_eq!(error.code, code);
        assert!(!format!("{error:?}").contains("fixture-private-source"));
        request.assert_hits(1);
    }
}

#[tokio::test]
async fn import_refuses_redirect_and_cross_origin_polling_without_forwarding_credentials() {
    for status in [307, 202] {
        let source = MockServer::start_async().await;
        let other = MockServer::start_async().await;
        let destination = other.mock(|when, then| {
            when.any_request();
            then.status(200);
        });
        let request = source.mock(|when, then| {
            when.method(POST);
            then.status(status).header("location", other.url("/steal"));
        });
        assert!(client(&source)
            .import_image("test-rg", "testregistry", &parameters())
            .await
            .is_err());
        request.assert_hits(1);
        destination.assert_hits(0);
    }
}

#[tokio::test]
async fn mutable_source_is_rejected_before_network_request() {
    let server = MockServer::start_async().await;
    let request = server.mock(|when, then| {
        when.any_request();
        then.status(200);
    });
    let mut params = parameters();
    params.source.source_image = "app:latest".into();
    let error = client(&server)
        .import_image("test-rg", "testregistry", &params)
        .await
        .unwrap_err();
    assert_eq!(error.code, "INVALID_INPUT");
    request.assert_hits(0);
}

#[tokio::test]
async fn import_polling_is_bounded_to_the_arm_origin_and_drops_error_bodies() {
    for (status, body, complete) in [
        (200, json!({"status":"Running"}), Some(false)),
        (200, json!({"status":"Succeeded"}), Some(true)),
        (
            200,
            json!({"status":"Failed","error":"fixture-private-source"}),
            None,
        ),
        (500, json!({"error":"fixture-private-source"}), None),
    ] {
        let server = MockServer::start_async().await;
        let poll = server.mock(|when, then| {
            when.method(GET).path("/operation");
            then.status(status).json_body(body);
        });
        let operation = LongRunningOperation {
            url: format!("{}/operation?api-version=2025-04-01", server.base_url()),
            retry_after: None,
            location_url: None,
        };
        let result = client(&server).poll_image_import(&operation).await;
        if let Some(complete) = complete {
            assert_eq!(result.unwrap(), complete);
        } else {
            let error = result.unwrap_err();
            assert!(!format!("{error:?} {error}").contains("fixture-private-source"));
        }
        assert_eq!(poll.hits_async().await, 1);
    }
    let server = MockServer::start_async().await;
    let other = MockServer::start_async().await;
    let unexpected = other.mock(|when, then| {
        when.any_request();
        then.status(200);
    });
    let redirect = server.mock(|when, then| {
        when.method(GET).path("/redirect");
        then.status(307)
            .header("location", format!("{}/target", other.base_url()));
    });
    for url in [
        format!("{}/redirect", server.base_url()),
        format!("{}/target", other.base_url()),
        format!(
            "{}/operation?token=fixture-private-source",
            server.base_url()
        ),
    ] {
        let operation = LongRunningOperation {
            url,
            retry_after: None,
            location_url: None,
        };
        let error = client(&server)
            .poll_image_import(&operation)
            .await
            .unwrap_err();
        assert!(!format!("{error:?} {error}").contains("fixture-private-source"));
    }
    assert_eq!(redirect.hits_async().await, 1);
    assert_eq!(unexpected.hits_async().await, 0);
}
