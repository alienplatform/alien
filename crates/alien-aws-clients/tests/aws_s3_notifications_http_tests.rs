use std::collections::HashMap;

use alien_aws_clients::{
    s3::{EventBridgeConfiguration, S3Api, S3Client},
    AwsClientConfig, AwsCredentialProvider, AwsCredentials, ServiceOverrides,
};
use httpmock::prelude::*;

fn client(server: &MockServer) -> S3Client {
    S3Client::new(
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
                endpoints: HashMap::from([("s3".into(), server.base_url())]),
            }),
        }),
    )
}

// Provider XML, deliberately independent of the client's serde model. S3's XML
// uses S3Key/CloudFunction/Queue/Topic, unlike the AWS CLI's JSON field names.
const DESTINATIONS: &str = concat!(
    "<CloudFunctionConfiguration><Id>worker-one</Id><CloudFunction>arn:aws:lambda:us-east-1:123456789012:function:one</CloudFunction><Event>s3:ObjectCreated:*</Event><Filter><S3Key><FilterRule><Name>prefix</Name><Value>images/</Value></FilterRule></S3Key></Filter></CloudFunctionConfiguration>",
    "<CloudFunctionConfiguration><CloudFunction>arn:aws:lambda:us-east-1:123456789012:function:two</CloudFunction><Event>s3:ObjectRemoved:*</Event><Event>s3:ReducedRedundancyLostObject</Event></CloudFunctionConfiguration>",
    "<QueueConfiguration><Id>external-queue</Id><Queue>arn:aws:sqs:us-east-1:123456789012:one</Queue><Event>s3:ObjectCreated:Put</Event><Event>s3:ObjectCreated:CompleteMultipartUpload</Event><Filter><S3Key><FilterRule><Name>prefix</Name><Value>quote&amp;é/</Value></FilterRule><FilterRule><Name>suffix</Name><Value>.json</Value></FilterRule></S3Key></Filter></QueueConfiguration>",
    "<QueueConfiguration><Queue>arn:aws:sqs:us-east-1:123456789012:two</Queue><Event>s3:ObjectRemoved:*</Event></QueueConfiguration>",
    "<TopicConfiguration><Id>external-topic</Id><Topic>arn:aws:sns:us-east-1:123456789012:one</Topic><Event>s3:ObjectCreated:*</Event><Filter><S3Key><FilterRule><Name>suffix</Name><Value>.txt</Value></FilterRule></S3Key></Filter></TopicConfiguration>",
    "<TopicConfiguration><Topic>arn:aws:sns:us-east-1:123456789012:two</Topic><Event>s3:ObjectRestore:Completed</Event></TopicConfiguration>",
);

#[tokio::test]
async fn notification_read_modify_write_preserves_every_destination_and_filter() {
    for eventbridge_enabled in [false, true] {
        let server = MockServer::start_async().await;
        let eventbridge = if eventbridge_enabled {
            "<EventBridgeConfiguration/>"
        } else {
            ""
        };
        let get = server.mock_async(|when, then| {
            when.method(GET).path("/test-bucket").query_param("notification", "").header_exists("authorization");
            then.status(200).body(format!(
                "<NotificationConfiguration xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">{DESTINATIONS}{eventbridge}</NotificationConfiguration>"
            ));
        }).await;
        let expected = format!(
            "<NotificationConfiguration>{DESTINATIONS}<EventBridgeConfiguration/></NotificationConfiguration>"
        );
        let put = server
            .mock_async(|when, then| {
                when.method(PUT)
                    .path("/test-bucket")
                    .query_param("notification", "")
                    .header_exists("authorization")
                    .body(expected);
                then.status(200);
            })
            .await;
        let client = client(&server);
        let mut config = client
            .get_bucket_notification_configuration("test-bucket")
            .await
            .unwrap();
        assert_eq!(config.lambda_function_configurations.len(), 2);
        assert_eq!(config.queue_configurations.len(), 2);
        assert_eq!(config.topic_configurations.len(), 2);
        assert_eq!(
            config.event_bridge_configuration.is_some(),
            eventbridge_enabled
        );
        assert_eq!(
            config.queue_configurations[0]
                .filter
                .as_ref()
                .unwrap()
                .key
                .filter_rules[0]
                .value,
            "quote&é/"
        );
        config.event_bridge_configuration = Some(EventBridgeConfiguration::default());
        client
            .put_bucket_notification_configuration("test-bucket", &config)
            .await
            .unwrap();
        get.assert_async().await;
        put.assert_async().await;
    }
}

#[tokio::test]
async fn empty_configuration_round_trip_does_not_enable_eventbridge() {
    let server = MockServer::start_async().await;
    let get = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/test-bucket")
                .query_param("notification", "");
            then.status(200).body(
                "<NotificationConfiguration xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\"/>",
            );
        })
        .await;
    let put = server
        .mock_async(|when, then| {
            when.method(PUT)
                .path("/test-bucket")
                .query_param("notification", "")
                .body("<NotificationConfiguration/>");
            then.status(200);
        })
        .await;
    let client = client(&server);
    let config = client
        .get_bucket_notification_configuration("test-bucket")
        .await
        .unwrap();
    assert_eq!(config, Default::default());
    client
        .put_bucket_notification_configuration("test-bucket", &config)
        .await
        .unwrap();
    get.assert_async().await;
    put.assert_async().await;
}
