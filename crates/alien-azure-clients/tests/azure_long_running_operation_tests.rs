use alien_azure_clients::{
    long_running_operation::LongRunningOperation, AzureClientConfig, AzureCredentials,
    AzureTokenCache, LongRunningOperationApi, LongRunningOperationClient,
};
use alien_client_core::{ErrorData, Result};
use httpmock::prelude::*;
use serde_json::json;

fn client() -> LongRunningOperationClient {
    LongRunningOperationClient::new(
        reqwest::Client::new(),
        AzureTokenCache::new(AzureClientConfig {
            subscription_id: "test-subscription".to_string(),
            tenant_id: "test-tenant".to_string(),
            region: Some("eastus".to_string()),
            credentials: AzureCredentials::AccessToken {
                token: "test-token".to_string(),
            },
            service_overrides: None,
        }),
    )
}

async fn poll(status: u16, body: &str) -> Result<Option<String>> {
    let server = MockServer::start_async().await;
    let request = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/operation")
                .header("authorization", "Bearer test-token");
            then.status(status).body(body);
        })
        .await;
    let result = client()
        .check_status(
            &LongRunningOperation {
                url: server.url("/operation"),
                retry_after: None,
                location_url: None,
            },
            "CreateVmss",
            "test-scale-set",
        )
        .await;
    if status >= 400 {
        assert!(
            request.hits_async().await > 0,
            "poll request must reach Azure"
        );
    } else {
        request.assert_async().await;
    }
    result
}

#[tokio::test]
async fn terminal_operation_status_is_not_retryable() {
    for status in ["Failed", "Canceled", "fAiLeD", "cAnCeLeD"] {
        let body = json!({"status": status, "error": {
            "code": "VMExtensionProvisioningError", "message": "Extension exited with code 1"
        }})
        .to_string();
        let error = poll(200, &body)
            .await
            .expect_err("terminal operation must fail");
        assert!(
            !error.retryable,
            "polling a terminal operation cannot recover it"
        );
        assert_eq!(error.code, "REMOTE_OPERATION_FAILED");
        assert!(matches!(
            &error.error,
            Some(ErrorData::RemoteOperationFailed { status: actual_status, provider_code: Some(code), .. })
                if actual_status == status && code == "VMExtensionProvisioningError"
        ));
        assert!(error.internal, "provider diagnostics must stay internal");
        for detail in [
            status,
            "VMExtensionProvisioningError",
            "Extension exited with code 1",
            "CreateVmss",
            "test-scale-set",
        ] {
            assert!(
                error.to_string().contains(detail),
                "missing {detail}: {error}"
            );
        }
    }
}

#[tokio::test]
async fn terminal_resource_provisioning_state_is_not_retryable() {
    for status in ["Failed", "Canceled"] {
        for error_at_root in [false, true] {
            let detail =
                json!({"code": "ProvisioningFailed", "message": "Resource provisioning failed"});
            let mut body = json!({"properties": {"provisioningState": status}});
            if error_at_root {
                body["error"] = detail;
            } else {
                body["properties"]["error"] = detail;
            }
            let error = poll(200, &body.to_string())
                .await
                .expect_err("terminal resource must fail");
            assert!(!error.retryable);
            assert_eq!(error.code, "REMOTE_OPERATION_FAILED");
            assert!(error.to_string().contains(status));
            assert!(error.to_string().contains("ProvisioningFailed"));
            assert!(error.to_string().contains("Resource provisioning failed"));
        }
    }
}

#[tokio::test]
async fn terminal_failure_without_provider_details_is_not_retryable() {
    for body in [
        r#"{"status":"Failed"}"#,
        r#"{"properties":{"provisioningState":"Failed"}}"#,
    ] {
        let error = poll(200, body)
            .await
            .expect_err("terminal status must fail without details");
        assert!(!error.retryable);
        assert_eq!(error.code, "REMOTE_OPERATION_FAILED");
        assert!(error.to_string().contains("Failed"));
    }
}

#[tokio::test]
async fn in_progress_responses_keep_polling() {
    for (status, body) in [
        (202, ""),
        (200, r#"{"status":"InProgress"}"#),
        (200, r#"{"properties":{"provisioningState":"Creating"}}"#),
    ] {
        assert_eq!(poll(status, body).await.expect("poll should succeed"), None);
    }
}

#[tokio::test]
async fn successful_responses_return_the_body() {
    for (status, body) in [
        (204, ""),
        (200, ""),
        (200, r#"{"status":"Succeeded"}"#),
        (200, r#"{"properties":{"provisioningState":"Succeeded"}}"#),
    ] {
        assert_eq!(
            poll(status, body).await.expect("poll should succeed"),
            Some(body.to_string())
        );
    }
}

#[tokio::test]
async fn polling_failures_remain_retryable() {
    for (status, body) in [
        (
            429,
            r#"{"error":{"code":"TooManyRequests","message":"Try later"}}"#,
        ),
        (500, "server error"),
        (503, "unavailable"),
        (200, "invalid json"),
    ] {
        let error = poll(status, body).await.expect_err("poll should fail");
        assert!(
            error.retryable,
            "poll failure should remain retryable: {error}"
        );
        assert_ne!(error.code, "REMOTE_OPERATION_FAILED");
    }
}

#[tokio::test]
async fn transport_failure_remains_retryable() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("reserve port");
    let address = listener.local_addr().expect("listener address");
    drop(listener);
    let error = client()
        .check_status(
            &LongRunningOperation {
                url: format!("http://{address}/operation"),
                retry_after: None,
                location_url: None,
            },
            "CreateVmss",
            "test-scale-set",
        )
        .await
        .expect_err("connection should fail");
    assert!(error.retryable);
    assert_eq!(error.code, "HTTP_REQUEST_FAILED");
}
