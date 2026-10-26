use std::collections::HashMap;

use alien_aws_clients::{
    dynamodb::{DynamoDbApi, DynamoDbClient, ListTagsOfResourceRequest},
    AwsClientConfig, AwsCredentialProvider, AwsCredentials, ServiceOverrides,
};
use httpmock::prelude::*;
use serde_json::json;

fn client(server: &MockServer) -> DynamoDbClient {
    DynamoDbClient::new(
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
                endpoints: HashMap::from([("dynamodb".into(), server.base_url())]),
            }),
        }),
    )
}

#[tokio::test]
async fn signed_tag_pages_preserve_tokens_and_exact_arn() {
    let server = MockServer::start_async().await;
    let arn = "arn:aws:dynamodb:us-east-1:123456789012:table/owned-data";
    let first = server
        .mock_async(|when, then| {
            when.method(POST)
                .header("x-amz-target", "DynamoDB_20120810.ListTagsOfResource")
                .header_exists("authorization")
                .json_body(json!({"ResourceArn":arn}));
            then.status(200).json_body(
                json!({"Tags":[{"Key":"first","Value":"one"}],"NextToken":"next/page+token"}),
            );
        })
        .await;
    let second = server
        .mock_async(|when, then| {
            when.method(POST)
                .header("x-amz-target", "DynamoDB_20120810.ListTagsOfResource")
                .header_exists("authorization")
                .json_body(json!({"ResourceArn":arn,"NextToken":"next/page+token"}));
            then.status(200)
                .json_body(json!({"Tags":[{"Key":"second","Value":"two"}]}));
        })
        .await;
    let client = client(&server);
    let page = client
        .list_tags_of_resource(ListTagsOfResourceRequest {
            resource_arn: arn.into(),
            next_token: None,
        })
        .await
        .unwrap();
    assert_eq!(page.tags.len(), 1);
    assert_eq!(page.tags[0].key, "first");
    assert_eq!(page.tags[0].value, "one");
    assert_eq!(page.next_token.as_deref(), Some("next/page+token"));
    let page = client
        .list_tags_of_resource(ListTagsOfResourceRequest {
            resource_arn: arn.into(),
            next_token: page.next_token,
        })
        .await
        .unwrap();
    assert_eq!(page.tags.len(), 1);
    assert_eq!(page.tags[0].key, "second");
    assert_eq!(page.tags[0].value, "two");
    assert!(page.next_token.is_none());
    first.assert_async().await;
    second.assert_async().await;
}
