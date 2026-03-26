/*!
# API Gateway V2 Client Integration Tests

These tests perform real AWS API Gateway V2 operations (HTTP APIs).

## Prerequisites

Set up `.env.test` in the workspace root with:
```
AWS_MANAGEMENT_REGION=us-east-1
AWS_MANAGEMENT_ACCESS_KEY_ID=your_access_key
AWS_MANAGEMENT_SECRET_ACCESS_KEY=your_secret_key
AWS_MANAGEMENT_ACCOUNT_ID=your_account_id
```

Optional (use a pre-existing Lambda):
```
ALIEN_TEST_APIGW_LAMBDA_ARN=arn:aws:lambda:us-east-1:123456789012:function:your-function
```

If `ALIEN_TEST_APIGW_LAMBDA_ARN` is not set, the tests will create a temporary Lambda
function using these Lambda test settings:
```
ALIEN_TEST_AWS_LAMBDA_IMAGE=your_account_id.dkr.ecr.us-east-1.amazonaws.com/test-lambda:latest
ALIEN_TEST_AWS_LAMBDA_EXECUTION_ROLE_ARN=arn:aws:iam::your_account_id:role/lambda-execution-role
```

Optional (for custom domain test):
```
ALIEN_TEST_APIGW_DOMAIN=api.example.com
ALIEN_TEST_APIGW_CERT_ARN=arn:aws:acm:us-east-1:123456789012:certificate/...
```
*/

use alien_aws_clients::apigatewayv2::*;
use alien_aws_clients::lambda::*;
use alien_aws_clients::AwsCredentialProvider;
use reqwest::Client;
use std::collections::HashSet;
use std::path::PathBuf as StdPathBuf;
use std::sync::Mutex;
use test_context::{test_context, AsyncTestContext};
use tokio;
use tracing::{info, warn};

struct ApiGatewayV2TestContext {
    client: ApiGatewayV2Client,
    lambda_client: LambdaClient,
    created_apis: Mutex<HashSet<String>>,
    created_domains: Mutex<HashSet<String>>,
    created_lambdas: Mutex<HashSet<String>>,
    lambda_arn: String,
}

impl AsyncTestContext for ApiGatewayV2TestContext {
    async fn setup() -> ApiGatewayV2TestContext {
        let root: StdPathBuf = workspace_root::get_workspace_root();
        dotenvy::from_path(root.join(".env.test")).expect("Failed to load .env.test");
        tracing_subscriber::fmt::try_init().ok();

        let region = std::env::var("AWS_MANAGEMENT_REGION")
            .expect("AWS_MANAGEMENT_REGION must be set in .env.test");
        let access_key = std::env::var("AWS_MANAGEMENT_ACCESS_KEY_ID")
            .expect("AWS_MANAGEMENT_ACCESS_KEY_ID must be set in .env.test");
        let secret_key = std::env::var("AWS_MANAGEMENT_SECRET_ACCESS_KEY")
            .expect("AWS_MANAGEMENT_SECRET_ACCESS_KEY must be set in .env.test");
        let account_id = std::env::var("AWS_MANAGEMENT_ACCOUNT_ID")
            .expect("AWS_MANAGEMENT_ACCOUNT_ID must be set in .env.test");

        let aws_config = alien_aws_clients::AwsClientConfig {
            account_id,
            region,
            credentials: alien_aws_clients::AwsCredentials::AccessKeys {
                access_key_id: access_key,
                secret_access_key: secret_key,
                session_token: None,
            },
            service_overrides: None,
        };

        let client = ApiGatewayV2Client::new(Client::new(), AwsCredentialProvider::from_config_sync(aws_config.clone()));
        let lambda_client = LambdaClient::new(Client::new(), AwsCredentialProvider::from_config_sync(aws_config));

        let (lambda_arn, created_lambda) = match std::env::var("ALIEN_TEST_APIGW_LAMBDA_ARN") {
            Ok(value) => (value, None),
            Err(_) => {
                let image_uri = std::env::var("ALIEN_TEST_AWS_LAMBDA_IMAGE")
                    .expect("ALIEN_TEST_AWS_LAMBDA_IMAGE must be set in .env.test when ALIEN_TEST_APIGW_LAMBDA_ARN is not set");
                let role_arn = std::env::var("ALIEN_TEST_AWS_LAMBDA_EXECUTION_ROLE_ARN")
                    .unwrap_or_else(|_| {
                        let account_id = std::env::var("AWS_MANAGEMENT_ACCOUNT_ID")
                            .expect("AWS_MANAGEMENT_ACCOUNT_ID must be set in .env.test");
                        format!("arn:aws:iam::{}:role/lambda-execution-role", account_id)
                    });
                let function_name = format!("alien-test-apigw-{}", uuid::Uuid::new_v4().simple());

                let request = CreateFunctionRequest::builder()
                    .function_name(function_name.clone())
                    .role(role_arn)
                    .code(FunctionCode::builder().image_uri(image_uri).build())
                    .description("Temporary function for API Gateway v2 tests".to_string())
                    .timeout(30)
                    .memory_size(128)
                    .publish(false)
                    .architectures(vec!["arm64".to_string()])
                    .build();

                let config = lambda_client
                    .create_function(request)
                    .await
                    .expect("Failed to create temporary Lambda for API Gateway v2 tests");

                let function_arn = config.function_arn.clone().unwrap_or_else(|| {
                    panic!("Lambda function ARN missing for {}", function_name);
                });

                let ready = wait_for_function_ready(&lambda_client, &function_name).await;
                if !ready {
                    panic!(
                        "Lambda function did not become ready in time: {}",
                        function_name
                    );
                }

                (function_arn, Some(function_name))
            }
        };

        let created_lambdas = Mutex::new(HashSet::new());
        if let Some(function_name) = created_lambda {
            created_lambdas.lock().unwrap().insert(function_name);
        }

        ApiGatewayV2TestContext {
            client,
            lambda_client,
            created_apis: Mutex::new(HashSet::new()),
            created_domains: Mutex::new(HashSet::new()),
            created_lambdas,
            lambda_arn,
        }
    }

    async fn teardown(self) {
        info!("🧹 Starting API Gateway test cleanup...");
        let api_ids: Vec<String> = self.created_apis.lock().unwrap().iter().cloned().collect();
        for api_id in api_ids {
            let _ = self.client.delete_api(&api_id).await;
        }
        let domains: Vec<String> = self
            .created_domains
            .lock()
            .unwrap()
            .iter()
            .cloned()
            .collect();
        for domain in domains {
            let _ = self.client.delete_domain_name(&domain).await;
        }
        let lambdas: Vec<String> = self
            .created_lambdas
            .lock()
            .unwrap()
            .iter()
            .cloned()
            .collect();
        for function_name in lambdas {
            let _ = self
                .lambda_client
                .delete_function(&function_name, None)
                .await;
        }
    }
}

async fn wait_for_function_ready(client: &LambdaClient, function_name: &str) -> bool {
    let mut attempts = 0;
    let max_attempts = 30;

    loop {
        attempts += 1;
        match client.get_function_configuration(function_name, None).await {
            Ok(config) => {
                if config.state == Some("Active".to_string())
                    && config.last_update_status == Some("Successful".to_string())
                {
                    return true;
                }
            }
            Err(_) => {}
        }

        if attempts >= max_attempts {
            return false;
        }
        tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
    }
}

#[test_context(ApiGatewayV2TestContext)]
#[tokio::test]
async fn test_create_api_integration_route_stage(ctx: &mut ApiGatewayV2TestContext) {
    let api = ctx
        .client
        .create_api(
            CreateApiRequest::builder()
                .name(format!("alien-test-api-{}", uuid::Uuid::new_v4().simple()))
                .protocol_type("HTTP".to_string())
                .build(),
        )
        .await
        .expect("Failed to create API");

    let api_id = api.api_id.clone().expect("API ID missing");
    ctx.created_apis.lock().unwrap().insert(api_id.clone());

    let integration = ctx
        .client
        .create_integration(
            &api_id,
            CreateIntegrationRequest::builder()
                .integration_type("AWS_PROXY".to_string())
                .integration_uri(ctx.lambda_arn.clone())
                .payload_format_version("2.0".to_string())
                .build(),
        )
        .await
        .expect("Failed to create integration");

    let integration_id = integration
        .integration_id
        .clone()
        .expect("Integration ID missing");

    ctx.client
        .create_route(
            &api_id,
            CreateRouteRequest::builder()
                .route_key("$default".to_string())
                .target(format!("integrations/{}", integration_id))
                .build(),
        )
        .await
        .expect("Failed to create route");

    ctx.client
        .create_stage(
            &api_id,
            CreateStageRequest::builder()
                .stage_name("$default".to_string())
                .auto_deploy(true)
                .build(),
        )
        .await
        .expect("Failed to create stage");
}

#[test_context(ApiGatewayV2TestContext)]
#[tokio::test]
async fn test_create_domain_and_mapping(ctx: &mut ApiGatewayV2TestContext) {
    let domain_name = match std::env::var("ALIEN_TEST_APIGW_DOMAIN") {
        Ok(value) => value,
        Err(_) => {
            warn!("ALIEN_TEST_APIGW_DOMAIN not set; skipping API Gateway domain test");
            return;
        }
    };
    let cert_arn = match std::env::var("ALIEN_TEST_APIGW_CERT_ARN") {
        Ok(value) => value,
        Err(_) => {
            warn!("ALIEN_TEST_APIGW_CERT_ARN not set; skipping API Gateway domain test");
            return;
        }
    };

    let api = ctx
        .client
        .create_api(
            CreateApiRequest::builder()
                .name(format!("alien-test-api-{}", uuid::Uuid::new_v4().simple()))
                .protocol_type("HTTP".to_string())
                .build(),
        )
        .await
        .expect("Failed to create API");

    let api_id = api.api_id.clone().expect("API ID missing");
    ctx.created_apis.lock().unwrap().insert(api_id.clone());

    ctx.client
        .create_domain_name(
            CreateDomainNameRequest::builder()
                .domain_name(domain_name.clone())
                .domain_name_configurations(vec![DomainNameConfiguration::builder()
                    .certificate_arn(cert_arn)
                    .endpoint_type("REGIONAL".to_string())
                    .security_policy("TLS_1_2".to_string())
                    .build()])
                .build(),
        )
        .await
        .expect("Failed to create domain name");
    ctx.created_domains
        .lock()
        .unwrap()
        .insert(domain_name.clone());

    let mapping = ctx
        .client
        .create_api_mapping(
            &domain_name,
            CreateApiMappingRequest::builder()
                .api_id(api_id.clone())
                .stage("$default".to_string())
                .build(),
        )
        .await
        .expect("Failed to create API mapping");

    let mapping_id = mapping
        .api_mapping_id
        .clone()
        .expect("API mapping ID missing");

    ctx.client
        .delete_api_mapping(&domain_name, &mapping_id)
        .await
        .expect("Failed to delete API mapping");
}
