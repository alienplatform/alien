use std::collections::HashMap;

use alien_aws_clients::{
    dynamodb::{
        DescribeContinuousBackupsRequest, DynamoDbApi, DynamoDbClient,
        PointInTimeRecoverySpecification, UpdateContinuousBackupsRequest,
    },
    AwsClientConfig, AwsCredentialProvider, AwsCredentials, ServiceOverrides,
};
use alien_client_core::ErrorData;
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
async fn signed_enable_and_readback_distinguish_pitr_from_continuous_backups() {
    let server = MockServer::start_async().await;
    let update = server.mock_async(|when, then| {
        when.method(POST).header("x-amz-target", "DynamoDB_20120810.UpdateContinuousBackups")
            .header_exists("authorization")
            .json_body(json!({"TableName": "test-table", "PointInTimeRecoverySpecification": {"PointInTimeRecoveryEnabled": true}}));
        then.status(200).json_body(json!({"ContinuousBackupsDescription": {"ContinuousBackupsStatus": "ENABLED"}}));
    }).await;
    let read = server
        .mock_async(|when, then| {
            when.method(POST)
                .header(
                    "x-amz-target",
                    "DynamoDB_20120810.DescribeContinuousBackups",
                )
                .header_exists("authorization")
                .json_body(json!({"TableName": "test-table"}));
            then.status(200).json_body(json!({"ContinuousBackupsDescription": {
            "ContinuousBackupsStatus": "ENABLED", "PointInTimeRecoveryDescription": {
                "PointInTimeRecoveryStatus": "ENABLED", "RecoveryPeriodInDays": 35,
                "EarliestRestorableDateTime": 1700000000.0, "LatestRestorableDateTime": 1700000100.0
            }
        }}));
        })
        .await;
    let client = client(&server);
    let response = client
        .update_continuous_backups(UpdateContinuousBackupsRequest {
            table_name: "test-table".into(),
            point_in_time_recovery_specification: PointInTimeRecoverySpecification {
                point_in_time_recovery_enabled: true,
            },
        })
        .await
        .unwrap();
    assert!(!response.point_in_time_recovery_enabled());
    assert!(client
        .describe_continuous_backups(DescribeContinuousBackupsRequest {
            table_name: "test-table".into()
        })
        .await
        .unwrap()
        .point_in_time_recovery_enabled());
    update.assert_async().await;
    read.assert_async().await;
}

#[tokio::test]
async fn backup_errors_preserve_retry_and_access_semantics() {
    for code in [
        "TableNotFoundException",
        "ContinuousBackupsUnavailableException",
        "AccessDeniedException",
    ] {
        let server = MockServer::start_async().await;
        let request = server
            .mock_async(|when, then| {
                when.method(POST);
                then.status(400)
                    .json_body(json!({"__type": code, "message": "provider failure"}));
            })
            .await;
        let error = client(&server)
            .update_continuous_backups(UpdateContinuousBackupsRequest {
                table_name: "test-table".into(),
                point_in_time_recovery_specification: PointInTimeRecoverySpecification {
                    point_in_time_recovery_enabled: true,
                },
            })
            .await
            .unwrap_err();
        match code {
            "TableNotFoundException" => assert!(matches!(
                error.error,
                Some(ErrorData::RemoteResourceNotFound { .. })
            )),
            "ContinuousBackupsUnavailableException" => assert!(matches!(
                error.error,
                Some(ErrorData::RemoteServiceUnavailable { .. })
            )),
            "AccessDeniedException" => assert!(matches!(
                error.error,
                Some(ErrorData::RemoteAccessDenied { .. })
            )),
            _ => unreachable!(),
        }
        request.assert_async().await;
    }
}
