use alien_gcp_clients::{GcpClientConfig, GcpClientConfigExt};
use httpmock::{Method::POST, MockServer};

async fn assert_retry_classification(impersonate: bool) {
    let server = MockServer::start_async().await;
    let directory = tempfile::tempdir().unwrap();
    let token_file = directory.path().join("subject-token");
    std::fs::write(&token_file, "synthetic-subject-token").unwrap();

    let _exchange = if impersonate {
        Some(
            server
                .mock_async(|when, then| {
                    when.method(POST).path("/token");
                    then.status(200)
                        .json_body(serde_json::json!({"access_token": "synthetic-sts-token"}));
                })
                .await,
        )
    } else {
        None
    };
    let impersonation_url = server.url("/impersonate");
    for (status, retryable) in [
        (400, false),
        (401, false),
        (403, false),
        (408, true),
        (429, true),
        (503, true),
    ] {
        let response = server
            .mock_async(|when, then| {
                when.method(POST).path(if impersonate {
                    "/impersonate"
                } else {
                    "/token"
                });
                then.status(status)
                    .body("synthetic token exchange rejection");
            })
            .await;
        let error = GcpClientConfig::exchange_external_account_token(
            "synthetic-audience",
            "urn:ietf:params:oauth:token-type:jwt",
            &server.url("/token"),
            token_file.to_str().unwrap(),
            impersonate.then_some(impersonation_url.as_str()),
        )
        .await
        .unwrap_err();
        response.assert_hits_async(1).await;
        assert_eq!(error.retryable, retryable, "HTTP {status}: {error}");
        assert_eq!(error.code, "HTTP_RESPONSE_ERROR");
        assert!(error.message.contains(&status.to_string()));
        assert!(error.message.contains("synthetic token exchange rejection"));
        response.delete_async().await;
    }
}

#[tokio::test]
async fn external_account_token_errors_retry_only_transient_responses() {
    assert_retry_classification(false).await;
}

#[tokio::test]
async fn external_account_impersonation_errors_retry_only_transient_responses() {
    assert_retry_classification(true).await;
}
