//! Secrets travel in the bodies of KMS, SSM and Secrets Manager calls. A failed call must not
//! keep them in the error, which is logged and stored in deployment state.

use std::collections::HashMap;

use alien_aws_clients::{
    kms::{DecryptRequest, EncryptRequest, KmsApi, KmsClient},
    secrets_manager::{
        CreateSecretRequest, GetSecretValueRequest, SecretsManagerApi, SecretsManagerClient,
    },
    ssm::{GetParameterRequest, PutParameterRequest, SsmApi, SsmClient},
    AwsClientConfig, AwsCredentialProvider, AwsCredentials, ServiceOverrides,
};
use httpmock::prelude::*;
use serde_json::json;

const SENTINEL: &str = "c2VudGluZWwtY2FuYXJ5LTA3Mjg";

fn credentials(service: &str, server: &MockServer) -> AwsCredentialProvider {
    AwsCredentialProvider::from_config_sync(AwsClientConfig {
        account_id: "123456789012".into(),
        region: "us-east-1".into(),
        credentials: AwsCredentials::AccessKeys {
            access_key_id: "test-access-key".into(),
            secret_access_key: "test-secret-key".into(),
            session_token: None,
        },
        service_overrides: Some(ServiceOverrides {
            endpoints: HashMap::from([(service.to_string(), server.base_url())]),
        }),
    })
}

fn recorded(error: &alien_aws_clients::Error) -> String {
    let recorded = serde_json::to_string(error).unwrap();
    assert!(!recorded.contains(SENTINEL), "{recorded}");
    recorded
}

async fn mock(server: &MockServer, target: &str, status: u16, body: serde_json::Value) {
    let target = target.to_string();
    server
        .mock_async(move |when, then| {
            when.method(POST).header("x-amz-target", target);
            then.status(status)
                .header("content-type", "application/x-amz-json-1.1")
                .json_body(body);
        })
        .await;
}

#[tokio::test]
async fn kms_encrypt_error_does_not_record_the_plaintext() {
    let server = MockServer::start_async().await;
    mock(
        &server,
        "TrentService.Encrypt",
        400,
        json!({"__type": "DisabledException", "message": "key is disabled"}),
    )
    .await;
    let client = KmsClient::new(reqwest::Client::new(), credentials("kms", &server));
    let error = client
        .encrypt(EncryptRequest::builder("key-id".into(), SENTINEL.into()).build())
        .await
        .expect_err("HTTP 400 must reach the caller");
    assert!(recorded(&error).contains("DisabledException"));
}

#[tokio::test]
async fn kms_decrypt_error_does_not_record_the_plaintext_answer() {
    let server = MockServer::start_async().await;
    // KeyId has the wrong type, so the success answer cannot be parsed.
    mock(
        &server,
        "TrentService.Decrypt",
        200,
        json!({"Plaintext": SENTINEL, "KeyId": 7}),
    )
    .await;
    let client = KmsClient::new(reqwest::Client::new(), credentials("kms", &server));
    let error = client
        .decrypt(DecryptRequest::builder("key-id".into(), "Y2lwaGVy".into()).build())
        .await
        .expect_err("an unparsable answer must be an error");
    recorded(&error);
}

#[tokio::test]
async fn ssm_errors_do_not_record_parameter_values() {
    let server = MockServer::start_async().await;
    mock(
        &server,
        "AmazonSSM.PutParameter",
        400,
        json!({"__type": "ValidationException", "message": "bad parameter"}),
    )
    .await;
    mock(
        &server,
        "AmazonSSM.GetParameter",
        200,
        json!({"Parameter": {"Name": "p", "Value": SENTINEL, "Version": "one"}}),
    )
    .await;
    let client = SsmClient::new(reqwest::Client::new(), credentials("ssm", &server));
    let put = client
        .put_parameter(
            PutParameterRequest::builder()
                .name("p".into())
                .value(SENTINEL.into())
                .parameter_type("SecureString".into())
                .build(),
        )
        .await
        .expect_err("HTTP 400 must reach the caller");
    assert!(recorded(&put).contains("ValidationException"));
    let get = client
        .get_parameter(
            GetParameterRequest::builder()
                .name("p".into())
                .with_decryption(true)
                .build(),
        )
        .await
        .expect_err("an unparsable answer must be an error");
    recorded(&get);
}

#[tokio::test]
async fn secrets_manager_errors_do_not_record_secret_values() {
    let server = MockServer::start_async().await;
    mock(
        &server,
        "secretsmanager.CreateSecret",
        400,
        json!({"__type": "InvalidRequestException", "message": "bad secret"}),
    )
    .await;
    mock(
        &server,
        "secretsmanager.GetSecretValue",
        200,
        json!({"Name": "s", "SecretString": SENTINEL, "VersionId": 7}),
    )
    .await;
    let client = SecretsManagerClient::new(
        reqwest::Client::new(),
        credentials("secretsmanager", &server),
    );
    let create = client
        .create_secret(
            CreateSecretRequest::builder()
                .name("s".into())
                .secret_string(SENTINEL.into())
                .build(),
        )
        .await
        .expect_err("HTTP 400 must reach the caller");
    assert!(recorded(&create).contains("InvalidRequestException"));
    let get = client
        .get_secret_value(GetSecretValueRequest {
            secret_id: "s".into(),
            version_id: None,
            version_stage: None,
        })
        .await
        .expect_err("an unparsable answer must be an error");
    recorded(&get);
}
