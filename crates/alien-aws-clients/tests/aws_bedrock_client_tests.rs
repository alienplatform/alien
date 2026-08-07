use std::collections::HashMap;

use alien_aws_clients::{
    AwsClientConfig, AwsCredentialProvider, AwsCredentials, BedrockApi, BedrockClient,
    ServiceOverrides,
};
use httpmock::prelude::*;

fn client(server: &MockServer) -> BedrockClient {
    let config = AwsClientConfig {
        account_id: "123456789012".to_string(),
        region: "us-east-1".to_string(),
        credentials: AwsCredentials::AccessKeys {
            access_key_id: "test-access-key".to_string(),
            secret_access_key: "test-secret-key".to_string(),
            session_token: None,
        },
        service_overrides: Some(ServiceOverrides {
            endpoints: HashMap::from([("bedrock".to_string(), server.base_url())]),
        }),
    };
    BedrockClient::new(
        reqwest::Client::new(),
        AwsCredentialProvider::from_config_sync(config),
    )
}

#[tokio::test]
async fn gets_signed_foundation_model_availability_without_invoking_model() {
    let server = MockServer::start_async().await;
    let request = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/foundation-model-availability/openai.gpt-oss-20b-1%3A0")
                .header_exists("authorization");
            then.status(200).json_body_obj(&serde_json::json!({
                "agreementAvailability": {"status": "AVAILABLE"},
                "authorizationStatus": "AUTHORIZED",
                "entitlementAvailability": "AVAILABLE",
                "regionAvailability": "AVAILABLE"
            }));
        })
        .await;

    let result = client(&server)
        .get_foundation_model_availability("openai.gpt-oss-20b-1:0")
        .await
        .expect("availability response");

    request.assert_async().await;
    assert_eq!(result.authorization_status.as_deref(), Some("AUTHORIZED"));
    assert_eq!(
        result.entitlement_availability.as_deref(),
        Some("AVAILABLE")
    );
}

#[tokio::test]
async fn rejects_provider_access_denied_response() {
    let server = MockServer::start_async().await;
    let request = server
        .mock_async(|when, then| {
            when.method(GET);
            then.status(403).json_body_obj(&serde_json::json!({
                "message": "not allowed"
            }));
        })
        .await;

    let error = client(&server)
        .get_foundation_model_availability("openai.gpt-oss-20b-1:0")
        .await
        .expect_err("403 must fail");

    assert!(request.hits_async().await >= 1, "request was not sent");
    assert!(
        error.error.is_some(),
        "provider error must retain structured data"
    );
}
