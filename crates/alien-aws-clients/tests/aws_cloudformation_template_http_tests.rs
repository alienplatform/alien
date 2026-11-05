use alien_aws_clients::{
    cloudformation::{CloudFormationApi, CloudFormationClient, GetTemplateRequest},
    AwsClientConfig, AwsCredentialProvider, AwsCredentials, ServiceOverrides,
};
use httpmock::prelude::*;
use std::collections::HashMap;

fn client(server: &MockServer) -> CloudFormationClient {
    CloudFormationClient::new(
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
                endpoints: HashMap::from([("cloudformation".into(), server.base_url())]),
            }),
        }),
    )
}

#[tokio::test]
async fn fetches_original_template_without_expanding_or_reserializing_it() {
    let server = MockServer::start_async().await;
    let request = server
        .mock_async(|when, then| {
            when.method(POST)
                .header_exists("authorization")
                .body_contains("Action=GetTemplate")
                .body_contains("TemplateStage=Original")
                .body_contains("StackName=application%2Fidentity");
            then.status(200).body(
                r#"<GetTemplateResponse><GetTemplateResult><TemplateBody>{
  "Transform": "AWS::LanguageExtensions", "Description": "preserve &amp; retain",
  "Resources": {"Bucket": {"Type": "AWS::S3::Bucket"}}
}</TemplateBody></GetTemplateResult></GetTemplateResponse>"#,
            );
        })
        .await;
    let result = client(&server)
        .get_template(GetTemplateRequest {
            stack_name: "application/identity".into(),
        })
        .await
        .unwrap();
    assert_eq!(result.get_template_result.template_body, "{\n  \"Transform\": \"AWS::LanguageExtensions\", \"Description\": \"preserve & retain\",\n  \"Resources\": {\"Bucket\": {\"Type\": \"AWS::S3::Bucket\"}}\n}");
    request.assert_async().await;
}

#[tokio::test]
async fn rejected_template_read_never_becomes_an_empty_template() {
    let server = MockServer::start_async().await;
    let request = server.mock_async(|when, then| {
        when.method(POST).body_contains("Action=GetTemplate");
        then.status(403).body("<ErrorResponse><Error><Code>AccessDenied</Code><Message>denied</Message></Error></ErrorResponse>");
    }).await;
    assert!(client(&server)
        .get_template(GetTemplateRequest {
            stack_name: "application".into()
        })
        .await
        .is_err());
    request.assert_async().await;
}
