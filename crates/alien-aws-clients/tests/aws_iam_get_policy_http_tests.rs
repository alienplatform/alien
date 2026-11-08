use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use alien_aws_clients::{
    iam::{IamApi, IamClient},
    AwsClientConfig, AwsCredentialProvider, AwsCredentials, ServiceOverrides,
};
use alien_client_core::ErrorData;
use httpmock::prelude::*;

const POLICY_ARN: &str = "arn:aws:iam::123456789012:policy/stack-deployment-management-12";

fn client(server: &MockServer) -> IamClient {
    IamClient::new(
        reqwest::Client::new(),
        AwsCredentialProvider::from_config_sync(AwsClientConfig {
            account_id: "123456789012".into(),
            region: "us-east-1".into(),
            credentials: AwsCredentials::AccessKeys {
                access_key_id: "test-access-key".into(),
                secret_access_key: "test-secret-key".into(),
                session_token: None,
            },
            service_overrides: Some(ServiceOverrides {
                endpoints: HashMap::from([("iam".into(), server.base_url())]),
            }),
        }),
    )
}

fn iam_error(code: &str) -> String {
    format!(
        "<ErrorResponse xmlns=\"https://iam.amazonaws.com/doc/2010-05-08/\">\
         <Error><Type>Sender</Type><Code>{code}</Code><Message>{code}</Message></Error>\
         <RequestId>00000000-0000-0000-0000-000000000000</RequestId></ErrorResponse>"
    )
}

// Callers probe numbered policies that may not exist, so a missing policy is an answer, not a
// failure. Before this was fixed every 404 was sent four times with exponential back-off, about
// 7-14s per missing policy, and a scan over the unused indexes stalled setup for minutes.
#[tokio::test]
async fn get_policy_returns_not_found_after_one_request() {
    let server = MockServer::start_async().await;
    let missing = server
        .mock_async(|when, then| {
            when.method(POST)
                .header_exists("authorization")
                .x_www_form_urlencoded_tuple("Action", "GetPolicy")
                .x_www_form_urlencoded_tuple("PolicyArn", POLICY_ARN);
            then.status(404).body(iam_error("NoSuchEntity"));
        })
        .await;

    let started = Instant::now();
    let error = client(&server)
        .get_policy(POLICY_ARN)
        .await
        .expect_err("a missing policy must be reported");

    assert!(
        matches!(error.error, Some(ErrorData::RemoteResourceNotFound { .. })),
        "expected RemoteResourceNotFound, got {error:?}"
    );
    assert_eq!(missing.hits_async().await, 1, "a 404 must not be resent");
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "a 404 must be answered without back-off, took {:?}",
        started.elapsed()
    );
}

// A failure that may pass is still sent again: 1 request plus the default 3 retries.
#[tokio::test]
async fn get_policy_retries_a_service_failure() {
    let server = MockServer::start_async().await;
    let unavailable = server
        .mock_async(|when, then| {
            when.method(POST)
                .x_www_form_urlencoded_tuple("Action", "GetPolicy")
                .x_www_form_urlencoded_tuple("PolicyArn", POLICY_ARN);
            then.status(503).body(iam_error("ServiceUnavailable"));
        })
        .await;

    let error = client(&server)
        .get_policy(POLICY_ARN)
        .await
        .expect_err("a service that stays unavailable must be reported");

    assert!(
        matches!(
            error.error,
            Some(ErrorData::RemoteServiceUnavailable { .. })
        ),
        "expected RemoteServiceUnavailable, got {error:?}"
    );
    assert_eq!(unavailable.hits_async().await, 4);
}
