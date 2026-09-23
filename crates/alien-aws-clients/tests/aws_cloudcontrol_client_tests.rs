use std::collections::HashMap;

use alien_aws_clients::cloudcontrol::{CreateResourceRequest, OperationStatus};
use alien_aws_clients::{
    AwsClientConfig, AwsCredentialProvider, AwsCredentials, CloudControlApi, CloudControlClient,
    ErrorData, ServiceOverrides,
};
use httpmock::prelude::*;
use serde_json::{json, Value};

const TYPE_NAME: &str = "AWS::Lambda::NetworkConnector";
const ARN: &str = "arn:aws:lambda:us-east-1:123456789012:network-connector:abc123";
const DESIRED: &str = r#"{"Name":"acme-agents","Configuration":{"VpcEgressConfiguration":{}}}"#;

fn body(req: &httpmock::prelude::HttpMockRequest) -> Value {
    serde_json::from_slice(req.body.as_deref().unwrap_or_default()).unwrap()
}

fn client(server: &MockServer) -> CloudControlClient {
    let config = AwsClientConfig {
        account_id: "123456789012".to_string(),
        region: "us-east-1".to_string(),
        credentials: AwsCredentials::AccessKeys {
            access_key_id: "test-access-key".to_string(),
            secret_access_key: "test-secret-key".to_string(),
            session_token: None,
        },
        service_overrides: Some(ServiceOverrides {
            endpoints: HashMap::from([("cloudcontrolapi".to_string(), server.base_url())]),
        }),
    };
    CloudControlClient::new(
        reqwest::Client::new(),
        AwsCredentialProvider::from_config_sync(config),
    )
}

/// The wire shape of a JSON 1.0 call: one POST to `/`, the operation in `X-Amz-Target`.
fn operation(when: httpmock::When, name: &str) -> httpmock::When {
    when.method(POST)
        .path("/")
        .header("x-amz-target", format!("CloudApiService.{name}"))
        .header("content-type", "application/x-amz-json-1.0")
        .header_exists("authorization")
}

#[tokio::test]
async fn create_sends_the_desired_state_as_a_json_string_with_a_fresh_token() {
    let server = MockServer::start_async().await;
    let request = server
        .mock_async(|when, then| {
            operation(when, "CreateResource").matches(|req| {
                let body = body(req);
                body["TypeName"] == TYPE_NAME
                    && body["DesiredState"] == Value::String(DESIRED.to_string())
                    && body["ClientToken"].as_str().is_some_and(|t| t.len() == 36)
            });
            then.status(200).json_body(json!({
                "ProgressEvent": {
                    "TypeName": TYPE_NAME,
                    "RequestToken": "token-1",
                    "Operation": "CREATE",
                    "OperationStatus": "IN_PROGRESS",
                    "EventTime": 1.758612345123e9
                }
            }));
        })
        .await;

    let event = client(&server)
        .create_resource(
            CreateResourceRequest::builder()
                .type_name(TYPE_NAME.to_string())
                .desired_state(DESIRED.to_string())
                .build(),
        )
        .await
        .expect("create accepted");

    request.assert_async().await;
    assert_eq!(event.request_token, "token-1");
    assert_eq!(event.operation_status, OperationStatus::InProgress);
    assert!(!event.is_terminal());
    assert!(event.failure().is_none());
}

#[tokio::test]
async fn a_failed_request_carries_the_handler_code_and_status_message() {
    let server = MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            operation(when, "GetResourceRequestStatus").json_body(json!({
                "RequestToken": "token-1"
            }));
            then.status(200).json_body(json!({
                "ProgressEvent": {
                    "TypeName": TYPE_NAME,
                    "RequestToken": "token-1",
                    "Operation": "CREATE",
                    "OperationStatus": "FAILED",
                    "ErrorCode": "InvalidRequest",
                    "StatusMessage": "unable to assume the provided NetworkConnectorOperatorRole"
                }
            }));
        })
        .await;

    let event = client(&server)
        .get_resource_request_status("token-1")
        .await
        .expect("status read");

    assert!(event.is_terminal());
    let failure = event.failure().expect("a FAILED request is an error");
    assert_eq!(failure.code, "GENERIC_ERROR");
    assert!(
        failure.message.contains("InvalidRequest")
            && failure
                .message
                .contains("unable to assume the provided NetworkConnectorOperatorRole"),
        "{}",
        failure.message
    );
}

#[tokio::test]
async fn a_failed_delete_of_a_missing_resource_reads_as_not_found() {
    let server = MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            operation(when, "DeleteResource").matches(|req| {
                let body = body(req);
                body["TypeName"] == TYPE_NAME
                    && body["Identifier"] == ARN
                    && body["ClientToken"].is_string()
            });
            then.status(200).json_body(json!({
                "ProgressEvent": {
                    "TypeName": TYPE_NAME,
                    "Identifier": ARN,
                    "RequestToken": "token-2",
                    "Operation": "DELETE",
                    "OperationStatus": "FAILED",
                    "ErrorCode": "NotFound",
                    "StatusMessage": "Network connector not found"
                }
            }));
        })
        .await;

    let event = client(&server)
        .delete_resource(TYPE_NAME, ARN)
        .await
        .expect("delete accepted");

    let failure = event.failure().expect("a FAILED request is an error");
    assert!(matches!(
        failure.error,
        Some(ErrorData::RemoteResourceNotFound { .. })
    ));
    assert!(
        failure.to_string().contains("Network connector not found")
            || format!("{failure:?}").contains("Network connector not found"),
        "AWS's message survives the typed wrap: {failure:?}"
    );
}

#[tokio::test]
async fn a_missing_resource_is_not_found() {
    let server = MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            operation(when, "GetResource").json_body(json!({
                "TypeName": TYPE_NAME,
                "Identifier": ARN
            }));
            then.status(400).json_body(json!({
                "__type": "com.amazon.cloudapiservice#ResourceNotFoundException",
                "Message": "AWS::Lambda::NetworkConnector Handler returned status FAILED"
            }));
        })
        .await;

    let error = client(&server)
        .get_resource(TYPE_NAME, ARN)
        .await
        .expect_err("a 400 ResourceNotFoundException is an error");

    assert!(
        matches!(error.error, Some(ErrorData::RemoteResourceNotFound { .. })),
        "{error:?}"
    );
}

#[tokio::test]
async fn an_existing_name_is_a_conflict() {
    let server = MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            operation(when, "CreateResource");
            then.status(400).json_body(json!({
                "__type": "AlreadyExistsException",
                "message": "acme-agents already exists"
            }));
        })
        .await;

    let error = client(&server)
        .create_resource(
            CreateResourceRequest::builder()
                .type_name(TYPE_NAME.to_string())
                .desired_state("{}".to_string())
                .build(),
        )
        .await
        .expect_err("an AlreadyExistsException is an error");

    assert!(
        matches!(
            error.error,
            Some(ErrorData::RemoteResourceConflict { ref message, .. })
                if message == "acme-agents already exists"
        ),
        "{error:?}"
    );
}

#[tokio::test]
async fn list_pages_forward_the_token_and_keep_properties_as_text() {
    let server = MockServer::start_async().await;
    let properties = json!({ "Arn": ARN, "Name": "acme-agents", "State": "ACTIVE" }).to_string();
    let page = server
        .mock_async(|when, then| {
            operation(when, "ListResources").json_body(json!({
                "TypeName": TYPE_NAME,
                "NextToken": "page-2"
            }));
            then.status(200).json_body(json!({
                "TypeName": TYPE_NAME,
                "ResourceDescriptions": [{ "Identifier": ARN, "Properties": properties }]
            }));
        })
        .await;

    let listed = client(&server)
        .list_resources(TYPE_NAME, Some("page-2".to_string()))
        .await
        .expect("list succeeds");

    page.assert_async().await;
    assert_eq!(listed.next_token, None);
    assert_eq!(listed.resource_descriptions.len(), 1);
    assert_eq!(listed.resource_descriptions[0].identifier, ARN);
    let parsed: Value = serde_json::from_str(
        listed.resource_descriptions[0]
            .properties
            .as_deref()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(parsed["Name"], "acme-agents");
}
