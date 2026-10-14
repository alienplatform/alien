//! How IAM policy writes classify GCP's 400 rejections.
//!
//! A custom role that was just undeleted stays "deleted" to IAM policy writes
//! for a few seconds. Setup undeletes a soft-deleted role and binds it right
//! away, so that rejection must come back retryable for the executor to retry
//! the step. Other 400s stay non-retryable.

use alien_client_core::ErrorData;
use alien_gcp_clients::{
    iam::{Binding, IamPolicy},
    GcpClientConfig, GcpCredentials, GcsApi, GcsClient, ServiceOverrides,
};
use httpmock::prelude::*;
use std::collections::HashMap;

const BUCKET: &str = "test-bucket";
const ROLE: &str = "projects/test-project/roles/role_test_storage_data_write";

fn client(server: &MockServer) -> GcsClient {
    GcsClient::new(
        reqwest::Client::new(),
        GcpClientConfig {
            project_id: "test-project".into(),
            region: "us-central1".into(),
            credentials: GcpCredentials::AccessToken {
                token: "test-token".into(),
            },
            service_overrides: Some(ServiceOverrides {
                endpoints: HashMap::from([("storage".into(), server.base_url())]),
            }),
            project_number: None,
        },
    )
}

fn policy_granting_custom_role() -> IamPolicy {
    IamPolicy {
        version: Some(3),
        bindings: vec![Binding {
            role: ROLE.to_string(),
            members: vec!["serviceAccount:sa@test-project.iam.gserviceaccount.com".to_string()],
            condition: None,
        }],
        etag: None,
        kind: None,
        resource_id: None,
    }
}

/// Cloud Storage's response body, as returned by `PUT /b/{bucket}/iam` right
/// after `roles.undelete` succeeded for the granted role.
fn grant_to_deleted_role_body() -> serde_json::Value {
    let message = format!("Cannot add new grants to deleted role ({ROLE}).");
    serde_json::json!({
        "error": {
            "code": 400,
            "message": message,
            "errors": [{ "message": message, "domain": "global", "reason": "invalid" }]
        }
    })
}

#[tokio::test]
async fn grant_to_a_role_still_seen_as_deleted_is_retryable() {
    let server = MockServer::start_async().await;
    let write = server
        .mock_async(|when, then| {
            when.method(PUT).path(format!("/b/{BUCKET}/iam"));
            then.status(400)
                .json_body_obj(&grant_to_deleted_role_body());
        })
        .await;

    let error = client(&server)
        .set_bucket_iam_policy(BUCKET.to_string(), policy_granting_custom_role())
        .await
        .expect_err("the write is rejected");

    assert!(write.hits_async().await >= 1);
    assert!(
        error.retryable,
        "a grant rejected while an undelete propagates must be retried: {error:?}"
    );
    assert_eq!(error.code, "REMOTE_SERVICE_UNAVAILABLE");
    match error.error {
        Some(ErrorData::RemoteServiceUnavailable { message }) => assert_eq!(
            message,
            format!("Cannot add new grants to deleted role ({ROLE}).")
        ),
        other => panic!("expected RemoteServiceUnavailable, got {other:?}"),
    }
}

#[tokio::test]
async fn other_invalid_policy_writes_stay_non_retryable() {
    let server = MockServer::start_async().await;
    let message = "Role roles/compute.admin is not supported for this resource.";
    let write = server
        .mock_async(|when, then| {
            when.method(PUT).path(format!("/b/{BUCKET}/iam"));
            then.status(400).json_body_obj(&serde_json::json!({
                "error": { "code": 400, "message": message }
            }));
        })
        .await;

    let error = client(&server)
        .set_bucket_iam_policy(BUCKET.to_string(), policy_granting_custom_role())
        .await
        .expect_err("the write is rejected");

    assert!(write.hits_async().await >= 1);
    assert!(
        !error.retryable,
        "an invalid policy is not transient: {error:?}"
    );
    assert_eq!(error.code, "INVALID_INPUT");
}
