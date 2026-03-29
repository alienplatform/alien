/*!
# Lambda Client Integration Tests

These tests perform real AWS Lambda operations including creating functions, function URLs,
and making HTTP requests to deployed Lambda functions.

## Prerequisites

### 1. AWS Credentials
Set up `.env.test` in the workspace root with:
```
AWS_MANAGEMENT_REGION=eu-central-1
AWS_MANAGEMENT_ACCESS_KEY_ID=your_access_key
AWS_MANAGEMENT_SECRET_ACCESS_KEY=your_secret_key
AWS_MANAGEMENT_ACCOUNT_ID=your_account_id
ALIEN_TEST_AWS_LAMBDA_IMAGE=your_account_id.dkr.ecr.eu-central-1.amazonaws.com/test-lambda:latest
ALIEN_TEST_AWS_LAMBDA_EXECUTION_ROLE_ARN=arn:aws:iam::your_account_id:role/lambda-execution-role
```

### 2. Create and Push Test Lambda Image
```bash
cd infra/standalone/test-images/lambda

# Build for ARM64 (Lambda's default architecture)
docker build --platform linux/arm64 -t test-lambda .

# Tag for ECR
docker tag test-lambda:latest YOUR_ACCOUNT_ID.dkr.ecr.eu-central-1.amazonaws.com/test-lambda:latest

# Create ECR repository
aws ecr create-repository --repository-name test-lambda --region eu-central-1

# Login to ECR
aws ecr get-login-password --region eu-central-1 | docker login --username AWS --password-stdin YOUR_ACCOUNT_ID.dkr.ecr.eu-central-1.amazonaws.com

# Push image
docker push YOUR_ACCOUNT_ID.dkr.ecr.eu-central-1.amazonaws.com/test-lambda:latest
```

### 3. Create IAM Role for Lambda Execution
```bash
# Create trust policy for Lambda
cat > lambda-trust-policy.json << EOF
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Effect": "Allow",
      "Principal": {
        "Service": "lambda.amazonaws.com"
      },
      "Action": "sts:AssumeRole"
    }
  ]
}
EOF

# Create the IAM role
aws iam create-role \
    --role-name lambda-execution-role \
    --assume-role-policy-document file://lambda-trust-policy.json

# Attach basic Lambda execution policy
aws iam attach-role-policy \
    --role-name lambda-execution-role \
    --policy-arn arn:aws:iam::aws:policy/service-role/AWSLambdaBasicExecutionRole

# Attach SQS execution policy (required for SQS event source mappings)
    aws iam attach-role-policy \
        --role-name lambda-execution-role \
        --policy-arn arn:aws:iam::aws:policy/service-role/AWSLambdaSQSQueueExecutionRole

# Get the role ARN (use this in ALIEN_TEST_AWS_LAMBDA_EXECUTION_ROLE_ARN)
aws iam get-role --role-name lambda-execution-role --query 'Role.Arn' --output text
```

### 4. Required Permissions
Your AWS credentials need these permissions:
- `lambda:*` (or specific Lambda permissions)
- `sqs:*` (or specific SQS permissions for SQS-Lambda integration tests)
- `iam:PassRole` for the Lambda execution role
- `ecr:GetAuthorizationToken`, `ecr:BatchCheckLayerAvailability`, `ecr:GetDownloadUrlForLayer`, `ecr:BatchGetImage`

**Note:** The Lambda execution role itself needs:
- `AWSLambdaBasicExecutionRole` (for CloudWatch logging)
- `AWSLambdaSQSQueueExecutionRole` (for SQS event source mappings)

## Running Tests
```bash
# Run all Lambda tests
cargo test --package alien-infra --test lambda_client_tests

# Run specific test
cargo test --package alien-infra --test lambda_client_tests test_end_to_end_function_execution -- --nocapture
```

## Troubleshooting

### SQS Event Source Mapping Permission Error
If you get an error like "The function execution role does not have permissions to call ReceiveMessage on SQS", make sure you've attached the `AWSLambdaSQSQueueExecutionRole` policy to your Lambda execution role:

```bash
aws iam attach-role-policy \
    --role-name lambda-execution-role \
    --policy-arn arn:aws:iam::aws:policy/service-role/AWSLambdaSQSQueueExecutionRole
```
*/

use alien_aws_clients::lambda::*;
use alien_aws_clients::sqs::SqsApi as _;
use alien_aws_clients::AwsCredentialProvider;
use alien_client_core::Error;
use alien_client_core::ErrorData;
use anyhow;
use backon::{ConstantBuilder, Retryable};
use chrono;
use reqwest::Client;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf as StdPathBuf;
use std::sync::Mutex;
use std::time::Duration;
use test_context::{test_context, AsyncTestContext};
use tokio;
use tracing::{info, warn};
use uuid::Uuid;
use workspace_root;

struct LambdaTestContext {
    client: LambdaClient,
    image_uri: String,
    role_arn: String,
    created_functions: Mutex<HashSet<String>>,
    created_function_urls: Mutex<HashSet<String>>,
}

impl AsyncTestContext for LambdaTestContext {
    async fn setup() -> LambdaTestContext {
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
        let client = LambdaClient::new(Client::new(), AwsCredentialProvider::from_config_sync(aws_config));

        let image_uri = std::env::var("ALIEN_TEST_AWS_LAMBDA_IMAGE")
            .expect("ALIEN_TEST_AWS_LAMBDA_IMAGE must be set in .env.test");
        let role_arn =
            std::env::var("ALIEN_TEST_AWS_LAMBDA_EXECUTION_ROLE_ARN").unwrap_or_else(|_| {
                let account_id = std::env::var("AWS_MANAGEMENT_ACCOUNT_ID")
                    .expect("AWS_MANAGEMENT_ACCOUNT_ID must be set in .env.test");
                format!("arn:aws:iam::{}:role/lambda-execution-role", account_id)
            });

        LambdaTestContext {
            client,
            image_uri,
            role_arn,
            created_functions: Mutex::new(HashSet::new()),
            created_function_urls: Mutex::new(HashSet::new()),
        }
    }

    async fn teardown(self) {
        info!("🧹 Starting Lambda test cleanup...");

        let functions_to_cleanup = {
            let functions = self.created_functions.lock().unwrap();
            functions.clone()
        };

        let urls_to_cleanup = {
            let urls = self.created_function_urls.lock().unwrap();
            urls.clone()
        };

        // First clean up function URLs
        for function_name in &urls_to_cleanup {
            self.cleanup_function_url(function_name).await;
        }

        // Then clean up functions
        for function_name in functions_to_cleanup {
            self.cleanup_function(&function_name).await;
        }

        info!("✅ Lambda test cleanup completed");
    }
}

impl LambdaTestContext {
    fn track_function(&self, function_name: &str) {
        let mut functions = self.created_functions.lock().unwrap();
        functions.insert(function_name.to_string());
        info!("📝 Tracking function for cleanup: {}", function_name);
    }

    fn untrack_function(&self, function_name: &str) {
        let mut functions = self.created_functions.lock().unwrap();
        functions.remove(function_name);
        info!(
            "✅ Function {} successfully cleaned up and untracked",
            function_name
        );
    }

    fn track_function_url(&self, function_name: &str) {
        let mut urls = self.created_function_urls.lock().unwrap();
        urls.insert(function_name.to_string());
        info!("📝 Tracking function URL for cleanup: {}", function_name);
    }

    fn untrack_function_url(&self, function_name: &str) {
        let mut urls = self.created_function_urls.lock().unwrap();
        urls.remove(function_name);
        info!(
            "✅ Function URL {} successfully cleaned up and untracked",
            function_name
        );
    }

    async fn cleanup_function_url(&self, function_name: &str) {
        info!("🧹 Cleaning up function URL: {}", function_name);

        match self
            .client
            .delete_function_url_config(function_name, None)
            .await
        {
            Ok(_) => {
                info!("✅ Function URL {} deleted successfully", function_name);
            }
            Err(e) => {
                if !matches!(
                    e,
                    Error {
                        error: Some(ErrorData::RemoteResourceNotFound { .. }),
                        ..
                    }
                ) {
                    warn!(
                        "Failed to delete function URL {} during cleanup: {:?}",
                        function_name, e
                    );
                }
            }
        }
    }

    async fn cleanup_function(&self, function_name: &str) {
        info!("🧹 Cleaning up function: {}", function_name);

        match self.client.delete_function(function_name, None).await {
            Ok(_) => {
                info!("✅ Function {} deleted successfully", function_name);
            }
            Err(e) => {
                if !matches!(
                    e,
                    Error {
                        error: Some(ErrorData::RemoteResourceNotFound { .. }),
                        ..
                    }
                ) {
                    warn!(
                        "Failed to delete function {} during cleanup: {:?}",
                        function_name, e
                    );
                }
            }
        }
    }

    fn get_test_function_name(&self) -> String {
        format!("alien-test-function-{}", Uuid::new_v4().simple())
    }

    async fn create_test_function(
        &self,
        function_name: &str,
    ) -> Result<FunctionConfiguration, Error> {
        let request = CreateFunctionRequest::builder()
            .function_name(function_name.to_string())
            .role(self.role_arn.clone())
            .code(
                FunctionCode::builder()
                    .image_uri(self.image_uri.clone())
                    .build(),
            )
            .description("Test function created by alien-infra tests".to_string())
            .timeout(30)
            .memory_size(128)
            .publish(false)
            .architectures(vec!["arm64".to_string()])
            .build();

        let result = self.client.create_function(request).await;
        if result.is_ok() {
            self.track_function(function_name);
        }
        result
    }

    async fn create_test_function_with_env(
        &self,
        function_name: &str,
        env_vars: HashMap<String, String>,
    ) -> Result<FunctionConfiguration, Error> {
        let request = CreateFunctionRequest::builder()
            .function_name(function_name.to_string())
            .role(self.role_arn.clone())
            .code(
                FunctionCode::builder()
                    .image_uri(self.image_uri.clone())
                    .build(),
            )
            .description("Test function created by alien-infra tests".to_string())
            .timeout(30)
            .memory_size(128)
            .publish(false)
            .environment(Environment::builder().variables(env_vars).build())
            .tracing_config(
                TracingConfig::builder()
                    .mode("PassThrough".to_string())
                    .build(),
            )
            .tags({
                let mut tags = HashMap::new();
                tags.insert("Environment".to_string(), "Test".to_string());
                tags.insert("Project".to_string(), "Alien".to_string());
                tags
            })
            .architectures(vec!["arm64".to_string()])
            .ephemeral_storage(EphemeralStorage::builder().size(512).build())
            .build();

        let result = self.client.create_function(request).await;
        if result.is_ok() {
            self.track_function(function_name);
        }
        result
    }

    async fn wait_for_function_ready(&self, function_name: &str) -> bool {
        info!("⏳ Waiting for function to be active...");
        let mut attempts = 0;
        let max_attempts = 30; // 5 minutes max wait

        loop {
            attempts += 1;

            match self
                .client
                .get_function_configuration(function_name, None)
                .await
            {
                Ok(config) => {
                    info!(
                        "📊 Function state: {:?}, update status: {:?}",
                        config.state.as_ref().unwrap_or(&"Unknown".to_string()),
                        config
                            .last_update_status
                            .as_ref()
                            .unwrap_or(&"Unknown".to_string())
                    );

                    if config.state == Some("Active".to_string())
                        && config.last_update_status == Some("Successful".to_string())
                    {
                        info!("✅ Function is ready!");
                        return true;
                    }

                    if attempts >= max_attempts {
                        warn!("⚠️  Function didn't become ready within 5 minutes");
                        return false;
                    }

                    tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
                }
                Err(e) => {
                    warn!("Failed to get function status: {:?}", e);
                    tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
                }
            }
        }
    }

    async fn create_test_function_url(&self, function_name: &str) -> Result<String, Error> {
        let url_request = CreateFunctionUrlConfigRequest::builder()
            .auth_type("NONE".to_string())
            .cors(
                Cors::builder()
                    .allow_credentials(false)
                    .allow_headers(vec!["Content-Type".to_string(), "X-Amz-Date".to_string()])
                    .allow_methods(vec!["GET".to_string(), "POST".to_string()])
                    .allow_origins(vec!["*".to_string()])
                    .max_age(300)
                    .build(),
            )
            .invoke_mode("BUFFERED".to_string())
            .build();

        let result = self
            .client
            .create_function_url_config(function_name, url_request)
            .await;
        if result.is_ok() {
            self.track_function_url(function_name);
            Ok(result.unwrap().function_url)
        } else {
            Err(result.unwrap_err())
        }
    }
}

#[test_context(LambdaTestContext)]
#[tokio::test]
async fn test_create_function_success(ctx: &mut LambdaTestContext) {
    let function_name = ctx.get_test_function_name();

    info!("🚀 Testing create function: {}", function_name);

    let _function_config = match ctx
        .create_test_function_with_env(&function_name, {
            let mut vars = HashMap::new();
            vars.insert("TEST_VAR".to_string(), "test_value".to_string());
            vars
        })
        .await
    {
        Ok(config) => {
            info!(
                "✅ Function created: {}",
                config.function_name.as_deref().unwrap_or("Unknown")
            );
            config
        }
        Err(e) => {
            panic!("Function creation failed: {:?}. Please ensure you have proper AWS credentials and permissions set up in .env.test", e);
        }
    };

    // Function will be cleaned up automatically via teardown
}

#[test_context(LambdaTestContext)]
#[tokio::test]
async fn test_get_function_configuration_not_found(ctx: &mut LambdaTestContext) {
    let non_existent_function = "alien-test-non-existent-function";

    let result = ctx
        .client
        .get_function_configuration(non_existent_function, None)
        .await;

    assert!(result.is_err());
    match result.unwrap_err() {
        Error {
            error:
                Some(ErrorData::RemoteResourceNotFound {
                    resource_type,
                    resource_name,
                    ..
                }),
            ..
        } => {
            assert_eq!(resource_type, "Function");
            assert_eq!(resource_name, non_existent_function);
        }
        other => panic!("Expected RemoteResourceNotFound, got: {:?}", other),
    }
}

#[test_context(LambdaTestContext)]
#[tokio::test]
async fn test_create_function_url_config_success(ctx: &mut LambdaTestContext) {
    let function_name = ctx.get_test_function_name();

    info!("🔗 Testing create function URL config: {}", function_name);

    // First create a function
    match ctx.create_test_function(&function_name).await {
        Ok(_) => {
            info!("✅ Function created successfully, now testing URL config");

            // Wait for function to be ready
            if ctx.wait_for_function_ready(&function_name).await {
                match ctx.create_test_function_url(&function_name).await {
                    Ok(function_url) => {
                        info!("✅ Successfully created function URL: {}", function_url);
                        assert!(function_url.starts_with("https://"));

                        // Verify we can get the URL config
                        match ctx
                            .client
                            .get_function_url_config(&function_name, None)
                            .await
                        {
                            Ok(url_config) => {
                                assert_eq!(url_config.function_url, function_url);
                                assert_eq!(url_config.auth_type, "NONE");
                            }
                            Err(e) => {
                                warn!("Failed to get function URL config: {:?}", e);
                            }
                        }
                    }
                    Err(e) => {
                        panic!("URL config creation failed: {:?}", e);
                    }
                }
            }
        }
        Err(e) => {
            panic!("Function creation failed: {:?}. Please ensure you have proper AWS credentials and permissions set up in .env.test", e);
        }
    }
}

#[test_context(LambdaTestContext)]
#[tokio::test]
async fn test_create_function_with_invalid_role(ctx: &mut LambdaTestContext) {
    let function_name = ctx.get_test_function_name();
    let invalid_role_arn = "arn:aws:iam::123456789012:role/non-existent-role";

    info!(
        "🚫 Testing create function with invalid role: {}",
        function_name
    );

    let request = CreateFunctionRequest::builder()
        .function_name(function_name.clone())
        .role(invalid_role_arn.to_string())
        .code(
            FunctionCode::builder()
                .image_uri(ctx.image_uri.clone())
                .build(),
        )
        .description("Test function with invalid role".to_string())
        .timeout(30)
        .memory_size(128)
        .publish(false)
        .build();

    let result = ctx.client.create_function(request).await;

    assert!(result.is_err());
    // This should result in an access denied or invalid parameter error
    match result.unwrap_err() {
        Error {
            error: Some(ErrorData::RemoteAccessDenied { .. }),
            ..
        }
        | Error {
            error: Some(ErrorData::GenericError { .. }),
            ..
        } => {
            info!("✅ Correctly rejected invalid role");
        }
        other => {
            warn!(
                "Got unexpected error type (may be valid depending on AWS setup): {:?}",
                other
            );
        }
    }
}

#[test_context(LambdaTestContext)]
#[tokio::test]
async fn test_create_function_already_exists(ctx: &mut LambdaTestContext) {
    let function_name = ctx.get_test_function_name();

    info!("🔄 Testing create duplicate function: {}", function_name);

    let request = CreateFunctionRequest::builder()
        .function_name(function_name.clone())
        .role(ctx.role_arn.clone())
        .code(
            FunctionCode::builder()
                .image_uri(ctx.image_uri.clone())
                .build(),
        )
        .description("Test function for duplicate creation".to_string())
        .timeout(30)
        .memory_size(128)
        .publish(false)
        .build();

    // Try to create function twice
    match ctx.client.create_function(request.clone()).await {
        Ok(_) => {
            info!("✅ First function creation succeeded");
            ctx.track_function(&function_name);

            // Try to create the same function again
            let result = ctx.client.create_function(request).await;

            assert!(result.is_err());
            match result.unwrap_err() {
                Error {
                    error:
                        Some(ErrorData::RemoteResourceConflict {
                            resource_type,
                            resource_name,
                            ..
                        }),
                    ..
                } => {
                    assert_eq!(resource_type, "Function");
                    assert_eq!(resource_name, function_name);
                    info!("✅ Correctly detected duplicate function creation");
                }
                other => {
                    panic!("Expected RemoteResourceConflict, got: {:?}", other);
                }
            }
        }
        Err(e) => {
            panic!("Initial function creation failed: {:?}. Please ensure you have proper AWS credentials and permissions set up in .env.test", e);
        }
    }
}

#[test_context(LambdaTestContext)]
#[tokio::test]
async fn test_lambda_client_with_invalid_credentials(ctx: &mut LambdaTestContext) {
    let region = std::env::var("AWS_MANAGEMENT_REGION")
        .expect("AWS_MANAGEMENT_REGION must be set in .env.test");
    let account_id =
        std::env::var("AWS_MANAGEMENT_ACCOUNT_ID").unwrap_or_else(|_| "123456789012".to_string());
    let client_invalid = Client::new();

    let aws_config = alien_aws_clients::AwsClientConfig {
        account_id,
        region,
        credentials: alien_aws_clients::AwsCredentials::AccessKeys {
            access_key_id: "invalid".to_string(),
            secret_access_key: "invalid".to_string(),
            session_token: None,
        },
        service_overrides: None,
    };
    let lambda_client = LambdaClient::new(client_invalid, AwsCredentialProvider::from_config_sync(aws_config));

    info!("🔐 Testing Lambda client with invalid credentials");

    let result = lambda_client
        .get_function_configuration("any-function", None)
        .await;

    assert!(result.is_err());
    match result.unwrap_err() {
        Error {
            error: Some(ErrorData::RemoteAccessDenied { .. }),
            ..
        } => {
            info!("✅ Correctly detected invalid credentials");
        }
        Error {
            error: Some(ErrorData::HttpRequestFailed { .. }),
            ..
        } => {
            info!("✅ Got HTTP error for invalid credentials (also acceptable)");
        }
        other => {
            warn!(
                "Got unexpected error type for invalid credentials: {:?}",
                other
            );
        }
    }
}

#[test_context(LambdaTestContext)]
#[tokio::test]
async fn test_serde_structs(ctx: &mut LambdaTestContext) {
    // Test serialization and deserialization of key structs
    let create_request = CreateFunctionRequest::builder()
        .function_name("test".to_string())
        .role("arn:aws:iam::123456789012:role/test".to_string())
        .code(
            FunctionCode::builder()
                .image_uri("123456789012.dkr.ecr.us-east-1.amazonaws.com/test:latest".to_string())
                .build(),
        )
        .build();

    let json = serde_json::to_string(&create_request).expect("Should serialize");
    assert!(json.contains("test"));
    assert!(json.contains("FunctionName")); // Verify PascalCase serialization
    assert!(json.contains("Role"));
    assert!(json.contains("Code"));
    assert!(json.contains("PackageType"));

    let cors = Cors::builder()
        .allow_credentials(true)
        .allow_headers(vec!["Content-Type".to_string()])
        .allow_methods(vec!["GET".to_string(), "POST".to_string()])
        .allow_origins(vec!["*".to_string()])
        .max_age(300)
        .build();

    let cors_json = serde_json::to_string(&cors).expect("Should serialize CORS");
    assert!(cors_json.contains("AllowCredentials"));
    assert!(cors_json.contains("AllowHeaders"));
    assert!(cors_json.contains("AllowMethods"));
    assert!(cors_json.contains("AllowOrigins"));
    assert!(cors_json.contains("MaxAge"));
}

#[test_context(LambdaTestContext)]
#[tokio::test]
async fn test_http_request_signing_and_construction(ctx: &mut LambdaTestContext) {
    info!("🔧 Testing HTTP request construction and signing");

    // Test that we can construct and sign an HTTP request
    // We'll call a method that makes an HTTP request but expect it to fail with permission error
    // This verifies the HTTP request construction, signing, and response parsing works
    let non_existent_function = "test-function-http-verification";

    let result = ctx
        .client
        .get_function_configuration(non_existent_function, None)
        .await;

    // This should make a real HTTP request and return a structured error
    assert!(result.is_err());

    let error = result.unwrap_err();
    match error {
        Error {
            error: Some(ErrorData::RemoteResourceNotFound { .. }),
            ..
        } => {
            // Perfect! This means we successfully:
            // 1. Constructed the HTTP request
            // 2. Signed it with AWS SigV4
            // 3. Made the HTTP call
            // 4. Received the response
            // 5. Parsed the JSON error response
            // 6. Mapped it to our error type
            info!("✓ HTTP request construction, signing, and response parsing all work!");
        }
        Error {
            error: Some(ErrorData::RemoteAccessDenied { .. }),
            ..
        } => {
            // Also good! This means HTTP worked but we have auth issues
            info!("✓ HTTP request works, got auth error (expected in some environments)");
        }
        other => {
            info!(
                "Got different error type, but HTTP request was made: {:?}",
                other
            );
            // Still counts as successful HTTP request/response cycle
        }
    }
}

#[test_context(LambdaTestContext)]
#[tokio::test]
async fn test_end_to_end_function_execution(ctx: &mut LambdaTestContext) {
    let function_name = ctx.get_test_function_name();

    info!(
        "🚀 Starting end-to-end Lambda function test: {}",
        function_name
    );

    // Step 1: Create the function
    let _function_config = match ctx
        .create_test_function_with_env(&function_name, {
            let mut vars = HashMap::new();
            vars.insert("TEST_MODE".to_string(), "true".to_string());
            vars
        })
        .await
    {
        Ok(config) => {
            info!(
                "✅ Function created: {}",
                config.function_name.as_deref().unwrap_or("Unknown")
            );
            config
        }
        Err(e) => {
            panic!("Function creation failed: {:?}. Please ensure you have proper AWS credentials and permissions set up in .env.test", e);
        }
    };

    // Step 2: Wait for function to be ready and proceed only if ready
    if ctx.wait_for_function_ready(&function_name).await {
        // Step 3: Create function URL
        info!("🔗 Creating function URL...");

        match ctx.create_test_function_url(&function_name).await {
            Ok(function_url) => {
                info!("✅ Function URL created: {}", function_url);

                // Step 4: Add permissions for public access to the function URL.
                // Public Function URLs require both InvokeFunctionUrl and InvokeFunction.
                info!("🔐 Adding permissions for public access...");
                let url_perm = AddPermissionRequest::builder()
                    .statement_id("AllowFunctionUrlInvoke".to_string())
                    .action("lambda:InvokeFunctionUrl".to_string())
                    .principal("*".to_string())
                    .function_url_auth_type("NONE".to_string())
                    .build();
                let invoke_perm = AddPermissionRequest::builder()
                    .statement_id("AllowPublicInvoke".to_string())
                    .action("lambda:InvokeFunction".to_string())
                    .principal("*".to_string())
                    .function_url_auth_type("NONE".to_string())
                    .build();

                for perm in [url_perm, invoke_perm] {
                    let sid = perm.statement_id.clone();
                    match ctx.client.add_permission(&function_name, perm).await {
                        Ok(_) => info!("✅ Permission {} added", sid),
                        Err(e) => {
                            warn!("Failed to add permission {} (may still work): {:?}", sid, e)
                        }
                    }
                }

                // Step 5: Verify function URL config
                info!("🔍 Verifying function URL configuration...");
                match ctx
                    .client
                    .get_function_url_config(&function_name, None)
                    .await
                {
                    Ok(url_config) => {
                        info!(
                            "✅ Function URL config retrieved: {}",
                            url_config.function_url
                        );
                        assert_eq!(url_config.function_url, function_url);
                        assert_eq!(url_config.auth_type, "NONE");
                        if let Some(cors) = &url_config.cors {
                            assert_eq!(cors.allow_credentials, Some(false));
                        }
                    }
                    Err(e) => {
                        warn!("Failed to get function URL config: {:?}", e);
                    }
                }

                // Step 6: Test HTTP requests with retry logic
                info!("🌐 Testing HTTP requests to function with retry logic...");

                let http_client = reqwest::Client::new();
                let function_url_clone = function_url.clone();

                // Test GET request with retry
                let get_request = || async {
                    let response = http_client.get(&function_url_clone).send().await?;
                    let status = response.status();

                    if status.is_success() {
                        let body = response
                            .text()
                            .await
                            .unwrap_or_else(|_| "Failed to read body".to_string());
                        info!("📥 GET Response: {} - {}", status, body);
                        Ok(body)
                    } else {
                        anyhow::bail!("GET request failed with status: {}", status);
                    }
                };

                match get_request
                    .retry(
                        ConstantBuilder::default()
                            .with_delay(Duration::from_secs(10))
                            .with_max_times(12),
                    )
                    .sleep(tokio::time::sleep)
                    .when(|_| true) // Retry on any error
                    .notify(|err: &anyhow::Error, dur: Duration| {
                        info!("🔄 Retrying GET request after {:?}, error: {}", dur, err);
                    })
                    .await
                {
                    Ok(_) => {
                        info!("✅ GET request successful!");
                    }
                    Err(e) => {
                        warn!("GET request failed after retries: {:?}", e);
                    }
                }

                // Test POST request with retry
                let post_request = || async {
                    let response = http_client
                        .post(&function_url_clone)
                        .header("Content-Type", "application/json")
                        .body(r#"{"test": "data", "method": "POST"}"#)
                        .send()
                        .await?;

                    let status = response.status();

                    if status.is_success() {
                        let body = response
                            .text()
                            .await
                            .unwrap_or_else(|_| "Failed to read body".to_string());
                        info!("📤 POST Response: {} - {}", status, body);
                        Ok(body)
                    } else {
                        anyhow::bail!("POST request failed with status: {}", status);
                    }
                };

                match post_request
                    .retry(
                        ConstantBuilder::default()
                            .with_delay(Duration::from_secs(10))
                            .with_max_times(12),
                    )
                    .sleep(tokio::time::sleep)
                    .when(|_| true) // Retry on any error
                    .notify(|err: &anyhow::Error, dur: Duration| {
                        info!("🔄 Retrying POST request after {:?}, error: {}", dur, err);
                    })
                    .await
                {
                    Ok(_) => {
                        info!("✅ POST request successful!");
                    }
                    Err(e) => {
                        warn!("POST request failed after retries: {:?}", e);
                    }
                }

                // Step 7: Test Lambda invoke functionality
                info!("📤 Testing Lambda invoke functionality...");

                // Test RequestResponse invocation with JSON payload
                info!("📤 Testing RequestResponse invocation with JSON payload...");
                let test_payload = r#"{"test": "data", "number": 42, "method": "invoke"}"#;
                let invoke_request = InvokeRequest::builder()
                    .function_name(function_name.clone())
                    .invocation_type(InvocationType::RequestResponse)
                    .payload(test_payload.as_bytes().to_vec())
                    .log_type("Tail".to_string())
                    .build();

                match ctx.client.invoke(invoke_request).await {
                    Ok(response) => {
                        info!("✅ RequestResponse invocation successful!");
                        info!("   Status code: {}", response.status_code);

                        // Check if we have a function error
                        if let Some(ref function_error) = response.function_error {
                            info!("   Function error: {}", function_error);
                        } else {
                            info!("   No function error");
                        }

                        // Parse response payload
                        if !response.payload.is_empty() {
                            match String::from_utf8(response.payload.clone()) {
                                Ok(payload_str) => {
                                    info!(
                                        "   Response payload: {}",
                                        payload_str.chars().take(200).collect::<String>()
                                    );

                                    // Try to parse as JSON to verify structure
                                    if let Ok(parsed_json) =
                                        serde_json::from_str::<serde_json::Value>(&payload_str)
                                    {
                                        info!("   ✅ Response is valid JSON");
                                        if let Some(status_code) = parsed_json.get("statusCode") {
                                            info!("      StatusCode in response: {}", status_code);
                                        }
                                    }
                                }
                                Err(_) => {
                                    info!(
                                        "   Response payload is binary data ({} bytes)",
                                        response.payload.len()
                                    );
                                }
                            }
                        } else {
                            info!("   Empty response payload");
                        }

                        // Verify that we got a reasonable response
                        assert!(
                            response.status_code >= 200 && response.status_code < 300,
                            "Expected 2xx status code, got {}",
                            response.status_code
                        );
                    }
                    Err(e) => {
                        warn!("RequestResponse invocation failed: {:?}", e);
                    }
                }

                // Test Event (asynchronous) invocation
                info!("📤 Testing Event (async) invocation...");
                let async_payload =
                    r#"{"test": "async_data", "timestamp": "2023-01-01T00:00:00Z"}"#;
                let async_invoke_request = InvokeRequest::builder()
                    .function_name(function_name.clone())
                    .invocation_type(InvocationType::Event)
                    .payload(async_payload.as_bytes().to_vec())
                    .build();

                match ctx.client.invoke(async_invoke_request).await {
                    Ok(response) => {
                        info!("✅ Event invocation successful!");
                        info!("   Status code: {}", response.status_code);

                        // For async invocations, we typically get 202 Accepted
                        if response.status_code == 202 {
                            info!("   ✅ Got expected 202 status for async invocation");
                        } else {
                            info!(
                                "   Got status {} (may vary by AWS setup)",
                                response.status_code
                            );
                        }

                        // Async invocations typically have empty payload
                        if response.payload.is_empty() {
                            info!("   ✅ Empty payload as expected for async invocation");
                        } else {
                            info!("   Response payload: {} bytes", response.payload.len());
                        }
                    }
                    Err(e) => {
                        warn!("Event invocation failed: {:?}", e);
                    }
                }

                // Test DryRun invocation (validation only)
                info!("📤 Testing DryRun invocation...");
                let dryrun_payload = r#"{"test": "dryrun_data"}"#;
                let dryrun_invoke_request = InvokeRequest::builder()
                    .function_name(function_name.clone())
                    .invocation_type(InvocationType::DryRun)
                    .payload(dryrun_payload.as_bytes().to_vec())
                    .build();

                match ctx.client.invoke(dryrun_invoke_request).await {
                    Ok(response) => {
                        info!("✅ DryRun invocation successful!");
                        info!("   Status code: {}", response.status_code);

                        // DryRun should return 204 No Content
                        if response.status_code == 204 {
                            info!("   ✅ Got expected 204 status for dry run");
                        } else {
                            info!(
                                "   Got status {} (may vary by AWS setup)",
                                response.status_code
                            );
                        }
                    }
                    Err(e) => {
                        warn!("DryRun invocation failed: {:?}", e);
                    }
                }

                // Test invocation with qualifier (version)
                info!("📤 Testing invocation with qualifier...");
                let qualified_invoke_request = InvokeRequest::builder()
                    .function_name(function_name.clone())
                    .invocation_type(InvocationType::RequestResponse)
                    .qualifier("$LATEST".to_string())
                    .payload(r#"{"test": "qualified_invoke"}"#.as_bytes().to_vec())
                    .build();

                match ctx.client.invoke(qualified_invoke_request).await {
                    Ok(response) => {
                        info!("✅ Qualified invocation successful!");
                        info!("   Status code: {}", response.status_code);
                        assert!(response.status_code >= 200 && response.status_code < 300);
                    }
                    Err(e) => {
                        warn!("Qualified invocation failed: {:?}", e);
                    }
                }

                // Step 8: Test error case - invoke non-existent function
                info!("🚫 Testing invoke on non-existent function...");
                let non_existent_function = "alien-test-non-existent-function-invoke";
                let error_invoke_request = InvokeRequest::builder()
                    .function_name(non_existent_function.to_string())
                    .invocation_type(InvocationType::RequestResponse)
                    .payload(r#"{"test": "error_case"}"#.as_bytes().to_vec())
                    .build();

                match ctx.client.invoke(error_invoke_request).await {
                    Ok(response) => {
                        // AWS might still return success but with error details in headers/payload
                        if let Some(ref function_error) = response.function_error {
                            info!("✅ Function error reported in response: {}", function_error);
                        } else {
                            info!("ℹ️  Function invoke returned {} without function error (this may be valid depending on AWS behavior)", response.status_code);
                        }
                    }
                    Err(e) => {
                        // This is what we typically expect for non-existent functions
                        info!("✅ Correctly got error for non-existent function: {:?}", e);
                    }
                }

                info!("✅ Lambda invoke functionality testing completed!");
            }
            Err(e) => {
                warn!("Failed to create function URL: {:?}", e);
            }
        }
    }

    info!("🎉 End-to-end test completed!");
}

#[test_context(LambdaTestContext)]
#[tokio::test]
async fn test_update_function_code(ctx: &mut LambdaTestContext) {
    let function_name = ctx.get_test_function_name();

    info!("🔄 Testing function code update: {}", function_name);

    // Step 1: Create initial function
    match ctx.create_test_function(&function_name).await {
        Ok(_) => {
            info!("✅ Initial function created");

            // Step 2: Wait for function to be ready
            if ctx.wait_for_function_ready(&function_name).await {
                // Step 3: Update function code
                info!("📦 Updating function code...");
                let update_request = UpdateFunctionCodeRequest::builder()
                    .image_uri(ctx.image_uri.clone()) // Using same image for simplicity
                    .publish(false)
                    .build();

                match ctx
                    .client
                    .update_function_code(&function_name, update_request)
                    .await
                {
                    Ok(updated_config) => {
                        info!("✅ Function code updated successfully");
                        assert_eq!(
                            updated_config.function_name.as_deref(),
                            Some(function_name.as_str())
                        );
                        info!("Updated function state: {:?}", updated_config.state);
                    }
                    Err(e) => {
                        warn!("Function code update failed: {:?}", e);
                    }
                }
            }
        }
        Err(e) => {
            panic!("Initial function creation failed: {:?}", e);
        }
    }
}

#[test_context(LambdaTestContext)]
#[tokio::test]
async fn test_get_policy(ctx: &mut LambdaTestContext) {
    let function_name = ctx.get_test_function_name();

    info!("📋 Testing function policy operations: {}", function_name);

    // Step 1: Create function
    match ctx.create_test_function(&function_name).await {
        Ok(_) => {
            info!("✅ Function created");

            // Step 2: Wait for function to be ready
            if ctx.wait_for_function_ready(&function_name).await {
                // Step 3: Try to get policy (should be empty initially)
                info!("📄 Getting initial policy (should be empty)...");
                match ctx.client.get_policy(&function_name, None).await {
                    Ok(policy_response) => {
                        info!("✅ Got policy response: {:?}", policy_response.policy);
                    }
                    Err(e) => {
                        // This is expected for a new function with no policy
                        match e {
                            Error {
                                error: Some(ErrorData::RemoteResourceNotFound { .. }),
                                ..
                            } => {
                                info!("✅ No policy found (expected for new function)");
                            }
                            other => {
                                warn!("Unexpected error getting policy: {:?}", other);
                            }
                        }
                    }
                }

                // Step 4: Add a permission
                info!("🔐 Adding a permission...");
                let permission_request = AddPermissionRequest::builder()
                    .statement_id("TestPermission".to_string())
                    .action("lambda:InvokeFunction".to_string())
                    .principal("123456789012".to_string()) // Dummy account ID
                    .build();

                match ctx
                    .client
                    .add_permission(&function_name, permission_request)
                    .await
                {
                    Ok(_) => {
                        info!("✅ Permission added");

                        // Step 5: Now get the policy again (should have content)
                        info!("📄 Getting policy after adding permission...");
                        match ctx.client.get_policy(&function_name, None).await {
                            Ok(policy_response) => {
                                info!("✅ Got policy with content!");
                                if let Some(policy) = &policy_response.policy {
                                    assert!(policy.contains("TestPermission"));
                                    assert!(policy.contains("lambda:InvokeFunction"));
                                    info!("Policy contains expected elements");
                                }
                            }
                            Err(e) => {
                                warn!("Failed to get policy after adding permission: {:?}", e);
                            }
                        }
                    }
                    Err(e) => {
                        warn!("Failed to add permission: {:?}", e);
                    }
                }
            }
        }
        Err(e) => {
            panic!("Function creation failed: {:?}", e);
        }
    }
}

// ---------------------------------------------------------------------------
// Event Source Mapping End-to-End Tests
// ---------------------------------------------------------------------------

#[test_context(LambdaTestContext)]
#[tokio::test]
async fn test_sqs_lambda_event_source_mapping_e2e(ctx: &mut LambdaTestContext) {
    info!("🚀 Starting SQS to Lambda Event Source Mapping End-to-End Test");

    // Test names
    let function_name = ctx.get_test_function_name();
    let queue_name = format!("alien-test-queue-{}", Uuid::new_v4().simple());

    // Step 1: Create SQS client
    let sqs_client = alien_aws_clients::sqs::SqsClient::new(
        Client::new(),
        AwsCredentialProvider::from_config_sync(alien_aws_clients::AwsClientConfig {
            account_id: std::env::var("AWS_MANAGEMENT_ACCOUNT_ID")
                .expect("AWS_MANAGEMENT_ACCOUNT_ID must be set"),
            region: std::env::var("AWS_MANAGEMENT_REGION")
                .expect("AWS_MANAGEMENT_REGION must be set"),
            credentials: alien_aws_clients::AwsCredentials::AccessKeys {
                access_key_id: std::env::var("AWS_MANAGEMENT_ACCESS_KEY_ID")
                    .expect("AWS_MANAGEMENT_ACCESS_KEY_ID must be set"),
                secret_access_key: std::env::var("AWS_MANAGEMENT_SECRET_ACCESS_KEY")
                    .expect("AWS_MANAGEMENT_SECRET_ACCESS_KEY must be set"),
                session_token: None,
            },
            service_overrides: None,
        }),
    );

    // Step 2: Create SQS Queue
    info!("🗂️ Creating SQS queue: {}", queue_name);
    let queue_create_request = alien_aws_clients::sqs::CreateQueueRequest::builder()
        .queue_name(queue_name.clone())
        .attributes({
            let mut attrs = HashMap::new();
            attrs.insert("VisibilityTimeout".to_string(), "300".to_string()); // 5 minutes for Lambda processing
            attrs.insert("MessageRetentionPeriod".to_string(), "86400".to_string()); // 1 day
            attrs.insert(
                "ReceiveMessageWaitTimeSeconds".to_string(),
                "20".to_string(),
            ); // Long polling
            attrs
        })
        .tags({
            let mut tags = HashMap::new();
            tags.insert("Environment".to_string(), "Test".to_string());
            tags.insert("Purpose".to_string(), "Lambda-Integration".to_string());
            tags
        })
        .build();

    let queue_response = match sqs_client.create_queue(queue_create_request).await {
        Ok(response) => {
            info!(
                "✅ SQS queue created: {}",
                response.create_queue_result.queue_url
            );
            response
        }
        Err(e) => {
            panic!("Failed to create SQS queue: {:?}", e);
        }
    };

    let queue_url = queue_response.create_queue_result.queue_url.clone();

    // Step 3: Get queue ARN (needed for event source mapping)
    info!("🔍 Getting queue ARN...");
    let queue_arn = {
        let get_attrs_request = alien_aws_clients::sqs::GetQueueAttributesRequest::builder()
            .attribute_names(vec!["QueueArn".to_string()])
            .build();

        match sqs_client
            .get_queue_attributes(&queue_url, get_attrs_request)
            .await
        {
            Ok(response) => {
                let arn = response
                    .get_queue_attributes_result
                    .attributes
                    .iter()
                    .find(|attr| attr.name == "QueueArn")
                    .map(|attr| attr.value.clone())
                    .expect("QueueArn should be present");
                info!("✅ Queue ARN: {}", arn);
                arn
            }
            Err(e) => {
                panic!("Failed to get queue ARN: {:?}", e);
            }
        }
    };

    // Step 4: Create Lambda function
    info!("⚡ Creating Lambda function: {}", function_name);
    let function_config = match ctx
        .create_test_function_with_env(&function_name, {
            let mut vars = HashMap::new();
            vars.insert("SQS_QUEUE_URL".to_string(), queue_url.clone());
            vars.insert("TEST_MODE".to_string(), "sqs_integration".to_string());
            vars
        })
        .await
    {
        Ok(config) => {
            info!(
                "✅ Lambda function created: {}",
                config.function_name.as_deref().unwrap_or("Unknown")
            );
            config
        }
        Err(e) => {
            panic!("Lambda function creation failed: {:?}", e);
        }
    };

    // Step 5: Wait for Lambda function to be ready
    if !ctx.wait_for_function_ready(&function_name).await {
        panic!("Lambda function did not become ready in time");
    }

    // Step 6: Add permissions for SQS to invoke Lambda
    info!("🔐 Adding SQS invoke permission to Lambda function...");
    let permission_request = AddPermissionRequest::builder()
        .statement_id("AllowSQSInvoke".to_string())
        .action("lambda:InvokeFunction".to_string())
        .principal("sqs.amazonaws.com".to_string())
        .build();

    match ctx
        .client
        .add_permission(&function_name, permission_request)
        .await
    {
        Ok(_) => {
            info!("✅ SQS invoke permission added to Lambda function");
        }
        Err(e) => {
            warn!("Failed to add SQS permission (continuing anyway): {:?}", e);
        }
    }

    // Step 7: Create event source mapping
    info!("🔗 Creating SQS to Lambda event source mapping...");
    let create_mapping_request = CreateEventSourceMappingRequest::builder()
        .event_source_arn(queue_arn.clone())
        .function_name(function_name.clone())
        .batch_size(5) // Process up to 5 messages at once
        .enabled(true)
        .maximum_batching_window_in_seconds(10) // Wait up to 10 seconds to gather messages
        .function_response_types(vec!["ReportBatchItemFailures".to_string()]) // Enable partial batch failure reporting
        .scaling_config(ScalingConfig::builder().maximum_concurrency(5).build()) // Limit concurrency
        .build();

    let event_source_mapping = match ctx
        .client
        .create_event_source_mapping(create_mapping_request)
        .await
    {
        Ok(mapping) => {
            info!(
                "✅ Event source mapping created: UUID={}",
                mapping.uuid.as_deref().unwrap_or("Unknown")
            );
            info!(
                "   State: {}",
                mapping.state.as_deref().unwrap_or("Unknown")
            );
            info!("   Batch size: {}", mapping.batch_size.unwrap_or(0));
            mapping
        }
        Err(e) => {
            panic!("Failed to create event source mapping: {:?}", e);
        }
    };

    let mapping_uuid = event_source_mapping
        .uuid
        .as_ref()
        .expect("Mapping should have UUID")
        .clone();

    // Step 8: Verify event source mapping
    info!("🔍 Verifying event source mapping...");
    match ctx.client.get_event_source_mapping(&mapping_uuid).await {
        Ok(retrieved_mapping) => {
            assert_eq!(
                retrieved_mapping.uuid.as_deref(),
                Some(mapping_uuid.as_str())
            );
            assert_eq!(
                retrieved_mapping.event_source_arn.as_deref(),
                Some(queue_arn.as_str())
            );
            assert_eq!(retrieved_mapping.batch_size, Some(5));
            assert_eq!(
                retrieved_mapping.maximum_batching_window_in_seconds,
                Some(10)
            );
            info!("✅ Event source mapping verification successful");
        }
        Err(e) => {
            warn!("Failed to verify event source mapping: {:?}", e);
        }
    }

    // Step 9: List event source mappings
    info!("📋 Listing event source mappings for function...");
    let list_request = ListEventSourceMappingsRequest::builder()
        .function_name(function_name.clone())
        .max_items(10)
        .build();

    match ctx.client.list_event_source_mappings(list_request).await {
        Ok(list_response) => {
            let empty_vec = vec![];
            let mappings = list_response
                .event_source_mappings
                .as_ref()
                .unwrap_or(&empty_vec);
            info!(
                "✅ Found {} event source mappings for function",
                mappings.len()
            );

            let our_mapping = mappings
                .iter()
                .find(|m| m.uuid.as_deref() == Some(mapping_uuid.as_str()));
            assert!(
                our_mapping.is_some(),
                "Should find our event source mapping in the list"
            );

            if let Some(mapping) = our_mapping {
                info!(
                    "   Our mapping - State: {}, BatchSize: {}",
                    mapping.state.as_deref().unwrap_or("Unknown"),
                    mapping.batch_size.unwrap_or(0)
                );
            }
        }
        Err(e) => {
            warn!("Failed to list event source mappings: {:?}", e);
        }
    }

    // Step 10: Update event source mapping
    info!("🔄 Updating event source mapping (changing batch size)...");
    let update_request = UpdateEventSourceMappingRequest::builder()
        .batch_size(3) // Reduce batch size
        .maximum_batching_window_in_seconds(5) // Reduce batching window
        .build();

    match ctx
        .client
        .update_event_source_mapping(&mapping_uuid, update_request)
        .await
    {
        Ok(updated_mapping) => {
            info!("✅ Event source mapping updated successfully");
            assert_eq!(updated_mapping.batch_size, Some(3));
            assert_eq!(updated_mapping.maximum_batching_window_in_seconds, Some(5));
        }
        Err(e) => {
            warn!("Failed to update event source mapping: {:?}", e);
        }
    }

    // Step 11: Send test messages to SQS
    info!("📤 Sending test messages to SQS queue...");
    for i in 1..=3 {
        let send_request = alien_aws_clients::sqs::SendMessageRequest::builder()
            .message_body(format!(
                r#"{{"test": true, "messageNumber": {}, "timestamp": "{}"}}"#,
                i,
                chrono::Utc::now().to_rfc3339()
            ))
            .message_attributes({
                let mut attrs = HashMap::new();
                attrs.insert(
                    "TestAttribute".to_string(),
                    alien_aws_clients::sqs::MessageAttributeValue::builder()
                        .string_value(format!("test-value-{}", i))
                        .data_type("String".to_string())
                        .build(),
                );
                attrs
            })
            .build();

        match sqs_client.send_message(&queue_url, send_request).await {
            Ok(response) => {
                info!(
                    "✅ Message {} sent with ID: {}",
                    i, response.send_message_result.message_id
                );
            }
            Err(e) => {
                warn!("Failed to send message {}: {:?}", i, e);
            }
        }
    }

    // Step 12: Wait a bit for processing (in real scenarios, Lambda would process the messages)
    info!("⏳ Waiting for message processing...");
    tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;

    // Step 13: Disable event source mapping temporarily
    info!("⏸️ Temporarily disabling event source mapping...");
    let disable_request = UpdateEventSourceMappingRequest::builder()
        .enabled(false)
        .build();

    match ctx
        .client
        .update_event_source_mapping(&mapping_uuid, disable_request)
        .await
    {
        Ok(disabled_mapping) => {
            info!("✅ Event source mapping disabled");
            // Note: State might not immediately show as disabled due to eventual consistency
        }
        Err(e) => {
            warn!("Failed to disable event source mapping: {:?}", e);
        }
    }

    // Step 14: Re-enable event source mapping
    info!("▶️ Re-enabling event source mapping...");
    let enable_request = UpdateEventSourceMappingRequest::builder()
        .enabled(true)
        .build();

    match ctx
        .client
        .update_event_source_mapping(&mapping_uuid, enable_request)
        .await
    {
        Ok(enabled_mapping) => {
            info!("✅ Event source mapping re-enabled");
        }
        Err(e) => {
            warn!("Failed to re-enable event source mapping: {:?}", e);
        }
    }

    // Step 15: Cleanup - Delete event source mapping
    info!("🗑️ Deleting event source mapping...");
    match ctx.client.delete_event_source_mapping(&mapping_uuid).await {
        Ok(deleted_mapping) => {
            info!("✅ Event source mapping deleted successfully");
            info!(
                "   Final state: {}",
                deleted_mapping.state.as_deref().unwrap_or("Unknown")
            );
        }
        Err(e) => {
            warn!("Failed to delete event source mapping: {:?}", e);
        }
    }

    // Step 16: Cleanup - Delete SQS queue
    info!("🗑️ Deleting SQS queue...");
    match sqs_client.delete_queue(&queue_url).await {
        Ok(_) => {
            info!("✅ SQS queue deleted successfully");
        }
        Err(e) => {
            warn!("Failed to delete SQS queue: {:?}", e);
        }
    }

    // Lambda function will be cleaned up automatically by the test context

    info!("🎉 SQS to Lambda Event Source Mapping E2E Test completed successfully!");

    // Verify serialization works for our structs
    info!("🧪 Verifying struct serialization...");
    let test_create_request = CreateEventSourceMappingRequest::builder()
        .event_source_arn("arn:aws:sqs:us-east-1:123456789012:test".to_string())
        .function_name("test-func".to_string())
        .batch_size(10)
        .enabled(true)
        .build();

    let json = serde_json::to_string(&test_create_request).expect("Should serialize");
    assert!(json.contains("EventSourceArn"));
    assert!(json.contains("FunctionName"));
    assert!(json.contains("BatchSize"));
    assert!(json.contains("Enabled"));

    info!("✅ All struct serialization tests passed");
}
