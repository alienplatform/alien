//! Local HTTP contracts; these tests never contact AWS.
use std::collections::HashMap;

use alien_aws_clients::{
    credential_provider::AwsCredentialProvider,
    lambda::{AddPermissionRequest, LambdaApi, LambdaClient, RemovePermissionRequest},
    ServiceOverrides,
};
use alien_core::{AwsClientConfig, AwsCredentials};
use httpmock::prelude::*;
use serde_json::json;

fn client(server: &MockServer) -> LambdaClient {
    LambdaClient::new(
        reqwest::Client::new(),
        AwsCredentialProvider::from_config_sync(AwsClientConfig {
            account_id: "111122223333".into(),
            region: "us-east-1".into(),
            credentials: AwsCredentials::AccessKeys {
                access_key_id: "test-access-key".into(),
                secret_access_key: "test-secret-key".into(),
                session_token: None,
            },
            service_overrides: Some(ServiceOverrides {
                endpoints: HashMap::from([("lambda".into(), server.base_url())]),
            }),
        }),
    )
}

#[tokio::test]
async fn permission_mutations_send_the_observed_policy_revision() {
    let server = MockServer::start_async().await;
    let client = client(&server);
    let read = server
        .mock_async(|when, then| {
            when.method(GET).path("/2015-03-31/functions/worker/policy");
            then.status(200)
                .json_body(json!({"Policy":"{\"Statement\":[]}","RevisionId":"revision+1"}));
        })
        .await;
    let add = server.mock_async(|when, then| {
        when.method(POST).path("/2015-03-31/functions/worker/policy").json_body(json!({
            "StatementId":"private-owned", "Action":"lambda:InvokeFunction",
            "Principal":"apigateway.amazonaws.com", "SourceArn":"arn:aws:execute-api:us-east-1:111122223333:owned/private/*/*",
            "SourceAccount":"111122223333", "RevisionId":"revision+1"
        }));
        then.status(201).json_body(json!({"Statement":"{}"}));
    }).await;
    let remove = server
        .mock_async(|when, then| {
            when.method(DELETE)
                .path("/2015-03-31/functions/worker/policy/private-owned")
                .query_param("RevisionId", "revision+1")
                .query_param("Qualifier", "live");
            then.status(204);
        })
        .await;
    let policy = client.get_policy("worker", None).await.unwrap();
    assert_eq!(policy.policy.as_deref(), Some("{\"Statement\":[]}"));
    let revision = policy.revision_id.unwrap();
    client
        .add_permission(
            "worker",
            AddPermissionRequest::builder()
                .statement_id("private-owned".into())
                .action("lambda:InvokeFunction".into())
                .principal("apigateway.amazonaws.com".into())
                .source_arn("arn:aws:execute-api:us-east-1:111122223333:owned/private/*/*".into())
                .source_account("111122223333".into())
                .revision_id(revision.clone())
                .build(),
        )
        .await
        .unwrap();
    client
        .remove_permission(
            "worker",
            RemovePermissionRequest::builder()
                .statement_id("private-owned".into())
                .revision_id(revision)
                .qualifier("live".into())
                .build(),
        )
        .await
        .unwrap();
    read.assert_async().await;
    add.assert_async().await;
    remove.assert_async().await;
}

#[tokio::test]
async fn stale_revision_and_denied_removal_remain_failures() {
    for (status, code) in [
        (412, "PreconditionFailedException"),
        (403, "AccessDeniedException"),
        (404, "ResourceNotFoundException"),
    ] {
        let server = MockServer::start_async().await;
        let client = client(&server);
        let remove = server
            .mock_async(|when, then| {
                when.method(DELETE)
                    .path("/2015-03-31/functions/worker/policy/private-owned")
                    .query_param("RevisionId", "stale");
                then.status(status)
                    .json_body(json!({"Type":code,"message":"permission mutation rejected"}));
            })
            .await;
        assert!(client
            .remove_permission(
                "worker",
                RemovePermissionRequest::builder()
                    .statement_id("private-owned".into())
                    .revision_id("stale".into())
                    .build()
            )
            .await
            .is_err());
        remove.assert_async().await;
    }
}
