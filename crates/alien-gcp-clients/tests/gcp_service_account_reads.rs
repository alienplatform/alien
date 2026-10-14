use alien_gcp_clients::{GcpClientConfig, GcpCredentials, IamApi, IamClient, ServiceOverrides};
use httpmock::prelude::*;
use std::collections::HashMap;

fn client(server: &MockServer) -> IamClient {
    IamClient::new(
        reqwest::Client::new(),
        GcpClientConfig {
            project_id: "test-project".into(),
            region: "us-central1".into(),
            credentials: GcpCredentials::AccessToken {
                token: "test-token".into(),
            },
            service_overrides: Some(ServiceOverrides {
                endpoints: HashMap::from([("iam".into(), server.base_url())]),
            }),
            project_number: None,
        },
    )
}

#[tokio::test]
async fn reads_target_project_pages_and_preserves_opaque_cursor_and_account_metadata() {
    let server = MockServer::start_async().await;
    let cursor = "next/+==&part=2";
    let first = server.mock_async(|when, then| {
        when.method(GET)
            .path("/projects/test-project/serviceAccounts")
            .header("authorization", "Bearer test-token")
            .query_param("pageSize", "1")
            .matches(|request| request.query_params.as_ref().is_none_or(|params| params.iter().all(|(key, _)| key != "pageToken")));
        then.status(200).json_body_obj(&serde_json::json!({
            "accounts":[{"name":"projects/test-project/serviceAccounts/100", "projectId":"test-project", "uniqueId":"100", "email":"first@test-project.iam.gserviceaccount.com", "disabled":true}],
            "nextPageToken":cursor
        }));
    }).await;
    let last = server.mock_async(|when, then| {
        when.method(GET)
            .path("/projects/test-project/serviceAccounts")
            .header("authorization", "Bearer test-token")
            .query_param("pageSize", "1")
            .query_param("pageToken", cursor);
        then.status(200).json_body_obj(&serde_json::json!({
            "accounts":[{"name":"projects/test-project/serviceAccounts/200", "projectId":"test-project", "uniqueId":"200", "email":"second@test-project.iam.gserviceaccount.com"}]
        }));
    }).await;
    let iam = client(&server);
    let page = iam.list_service_accounts(Some(1), None).await.unwrap();
    assert_eq!(page.accounts.len(), 1);
    assert_eq!(page.accounts[0].unique_id.as_deref(), Some("100"));
    assert_eq!(page.accounts[0].project_id.as_deref(), Some("test-project"));
    assert_eq!(page.accounts[0].disabled, Some(true));
    assert_eq!(page.next_page_token.as_deref(), Some(cursor));
    let page = iam
        .list_service_accounts(Some(1), page.next_page_token)
        .await
        .unwrap();
    assert_eq!(page.accounts.len(), 1);
    assert_eq!(page.accounts[0].unique_id.as_deref(), Some("200"));
    assert_eq!(page.next_page_token, None);
    first.assert_async().await;
    last.assert_async().await;
}

#[tokio::test]
async fn accepts_omitted_empty_list_but_preserves_denied_read_as_an_error() {
    let server = MockServer::start_async().await;
    let empty = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/projects/test-project/serviceAccounts")
                .matches(|request| request.query_params.as_ref().is_none_or(Vec::is_empty));
            then.status(200).json_body_obj(&serde_json::json!({}));
        })
        .await;
    let iam = client(&server);
    let page = iam.list_service_accounts(None, None).await.unwrap();
    assert!(page.accounts.is_empty());
    assert_eq!(page.next_page_token, None);
    empty.assert_async().await;
    let denied = server.mock_async(|when, then| {
        when.method(GET)
            .path("/projects/test-project/serviceAccounts")
            .query_param("pageSize", "100");
        then.status(403).json_body_obj(&serde_json::json!({
            "error":{"code":403,"status":"PERMISSION_DENIED","message":"Missing iam.serviceAccounts.list"}
        }));
    }).await;
    let error = iam
        .list_service_accounts(Some(100), None)
        .await
        .unwrap_err();
    assert!(matches!(
        error.error,
        Some(alien_gcp_clients::ErrorData::RemoteAccessDenied { .. })
    ));
    denied.assert_hits_async(4).await;
}
