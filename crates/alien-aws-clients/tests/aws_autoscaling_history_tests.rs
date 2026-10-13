use alien_aws_clients::{
    autoscaling::{AutoScalingApi, AutoScalingClient, DescribeScalingActivitiesRequest},
    AwsClientConfig, AwsCredentialProvider, AwsCredentials, ServiceOverrides,
};
use httpmock::prelude::*;
use std::collections::HashMap;

#[tokio::test]
async fn deleted_group_history_preserves_pagination_and_explicit_opt_in() {
    let server = MockServer::start_async().await;
    let client = AutoScalingClient::new(
        reqwest::Client::new(),
        AwsCredentialProvider::from_config_sync(AwsClientConfig {
            account_id: String::new(),
            region: "us-east-1".to_string(),
            credentials: AwsCredentials::AccessKeys {
                access_key_id: "test-key".to_string(),
                secret_access_key: "test-secret".to_string(),
                session_token: None,
            },
            service_overrides: Some(ServiceOverrides {
                endpoints: HashMap::from([("autoscaling".to_string(), server.base_url())]),
            }),
        }),
    );
    let first = server.mock_async(|when, then| {
        when.method(POST).body_contains("IncludeDeletedGroups=true")
            .body_contains("AutoScalingGroupName=removed-group")
            .body_contains("MaxRecords=100");
        then.status(200).body("<DescribeScalingActivitiesResponse><DescribeScalingActivitiesResult><Activities><member><ActivityId>launch</ActivityId><StatusCode>Successful</StatusCode></member></Activities><NextToken>page-two</NextToken></DescribeScalingActivitiesResult></DescribeScalingActivitiesResponse>");
    }).await;
    let response = client.describe_scaling_activities(DescribeScalingActivitiesRequest::builder()
        .auto_scaling_group_name("removed-group".to_string()).include_deleted_groups(true)
        .max_records(100).build()).await.unwrap().describe_scaling_activities_result;
    first.assert_async().await;
    assert_eq!(response.activities.unwrap().members[0].status_code.as_deref(), Some("Successful"));
    assert_eq!(response.next_token.as_deref(), Some("page-two"));
    let second = server.mock_async(|when, then| {
        when.method(POST).body_contains("IncludeDeletedGroups=true")
            .body_contains("NextToken=page-two");
        then.status(200).body("<DescribeScalingActivitiesResponse><DescribeScalingActivitiesResult><Activities/></DescribeScalingActivitiesResult></DescribeScalingActivitiesResponse>");
    }).await;
    let response = client.describe_scaling_activities(DescribeScalingActivitiesRequest::builder()
        .auto_scaling_group_name("removed-group".to_string()).include_deleted_groups(true)
        .next_token(response.next_token.unwrap()).build()).await.unwrap();
    second.assert_async().await;
    assert!(response.describe_scaling_activities_result.next_token.is_none());
}
