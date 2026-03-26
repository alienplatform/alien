/*!
# CloudFormation Client Integration Tests

These tests perform real AWS CloudFormation operations including creating, describing,
and deleting CloudFormation stacks programmatically.

## Prerequisites

### 1. AWS Credentials
Set up `.env.test` in the workspace root with:
```
AWS_MANAGEMENT_REGION=us-east-1
AWS_MANAGEMENT_ACCESS_KEY_ID=your_access_key
AWS_MANAGEMENT_SECRET_ACCESS_KEY=your_secret_key
AWS_MANAGEMENT_ACCOUNT_ID=your_account_id
```

### 2. Required Permissions
Your AWS credentials need these permissions:
- `cloudformation:CreateStack`
- `cloudformation:DescribeStacks`
- `cloudformation:DescribeStackResources`
- `cloudformation:DeleteStack`
- `iam:CreateRole`
- `iam:DeleteRole`
- `iam:PassRole`
- `s3:CreateBucket`
- `s3:DeleteBucket`

## Running Tests
```bash
# Run all CloudFormation tests
cargo test --package alien-infra --test cloudformation_client_tests

# Run specific test
cargo test --package alien-infra --test cloudformation_client_tests test_create_describe_delete_stack -- --nocapture
```
*/

use alien_aws_clients::cloudformation::*;
use alien_aws_clients::AwsCredentialProvider;
use alien_client_core::{Error, ErrorData};
use aws_credential_types::Credentials;
use reqwest::Client;
use std::collections::HashSet;
use std::path::PathBuf as StdPathBuf;
use std::sync::Mutex;
use test_context::{test_context, AsyncTestContext};
use tracing::{info, warn};
use uuid::Uuid;
use workspace_root;

struct CloudFormationTestContext {
    client: CloudFormationClient,
    created_stacks: Mutex<HashSet<String>>,
    account_id: String,
}

impl AsyncTestContext for CloudFormationTestContext {
    async fn setup() -> CloudFormationTestContext {
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
            account_id: account_id.clone(),
            region,
            credentials: alien_aws_clients::AwsCredentials::AccessKeys {
                access_key_id: access_key,
                secret_access_key: secret_key,
                session_token: None,
            },
            service_overrides: None,
        };
        let client = CloudFormationClient::new(Client::new(), AwsCredentialProvider::from_config_sync(aws_config));

        CloudFormationTestContext {
            client,
            created_stacks: Mutex::new(HashSet::new()),
            account_id,
        }
    }

    async fn teardown(self) {
        info!("🧹 Starting CloudFormation test cleanup...");

        let stacks_to_cleanup = {
            let stacks = self.created_stacks.lock().unwrap();
            stacks.clone()
        };

        // Cleanup all created stacks
        for stack_name in stacks_to_cleanup {
            self.cleanup_stack(&stack_name).await;
        }

        info!("✅ CloudFormation test cleanup completed");
    }
}

impl CloudFormationTestContext {
    fn track_stack(&self, stack_name: &str) {
        let mut stacks = self.created_stacks.lock().unwrap();
        stacks.insert(stack_name.to_string());
        info!("📝 Tracking stack for cleanup: {}", stack_name);
    }

    fn untrack_stack(&self, stack_name: &str) {
        let mut stacks = self.created_stacks.lock().unwrap();
        stacks.remove(stack_name);
        info!(
            "✅ Stack {} successfully cleaned up and untracked",
            stack_name
        );
    }

    async fn cleanup_stack(&self, stack_name: &str) {
        info!("🧹 Cleaning up stack: {}", stack_name);

        match self
            .client
            .delete_stack(
                DeleteStackRequest::builder()
                    .stack_name(stack_name.to_string())
                    .build(),
            )
            .await
        {
            Ok(_) => {
                info!("✅ Stack {} deletion initiated", stack_name);
                // Wait for stack deletion to complete
                self.wait_for_stack_deletion(stack_name).await;
            }
            Err(e) => {
                if !matches!(e.error, Some(ErrorData::RemoteResourceNotFound { .. })) {
                    warn!(
                        "Failed to delete stack {} during cleanup: {:?}",
                        stack_name, e
                    );
                }
            }
        }
    }

    async fn wait_for_stack_deletion(&self, stack_name: &str) {
        let max_wait_time = 300; // 5 minutes
        let mut elapsed = 0;

        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
            elapsed += 10;

            match self
                .client
                .describe_stacks(
                    DescribeStacksRequest::builder()
                        .stack_name(stack_name.to_string())
                        .build(),
                )
                .await
            {
                Ok(response) => {
                    if let Some(stack) = response.describe_stacks_result.stacks.member.first() {
                        info!("Stack {} status: {}", stack_name, stack.stack_status);

                        if stack.stack_status == "DELETE_COMPLETE" {
                            info!("✅ Stack {} deletion completed", stack_name);
                            return;
                        } else if stack.stack_status.contains("FAILED") {
                            warn!(
                                "⚠️ Stack {} deletion failed with status: {}",
                                stack_name, stack.stack_status
                            );
                            return;
                        }
                    }
                }
                Err(e) if matches!(e.error, Some(ErrorData::RemoteResourceNotFound { .. })) => {
                    info!("✅ Stack {} has been deleted (not found)", stack_name);
                    return;
                }
                Err(e) => {
                    warn!(
                        "Error checking stack {} deletion status: {:?}",
                        stack_name, e
                    );
                }
            }

            if elapsed >= max_wait_time {
                warn!("⚠️ Timeout waiting for stack {} deletion", stack_name);
                return;
            }
        }
    }

    async fn wait_for_stack_creation(&self, stack_name: &str) -> Result<(), Error> {
        let max_wait_time = 600; // 10 minutes
        let mut elapsed = 0;

        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(15)).await;
            elapsed += 15;

            match self
                .client
                .describe_stacks(
                    DescribeStacksRequest::builder()
                        .stack_name(stack_name.to_string())
                        .build(),
                )
                .await
            {
                Ok(response) => {
                    if let Some(stack) = response.describe_stacks_result.stacks.member.first() {
                        info!("Stack {} status: {}", stack_name, stack.stack_status);

                        if stack.stack_status == "CREATE_COMPLETE" {
                            info!("✅ Stack {} creation completed", stack_name);
                            return Ok(());
                        } else if stack.stack_status.contains("FAILED")
                            || stack.stack_status.contains("ROLLBACK")
                        {
                            return Err(Error::new(ErrorData::GenericError {
                                message: format!(
                                    "Stack creation failed with status: {}",
                                    stack.stack_status
                                ),
                            }));
                        }
                    }
                }
                Err(e) => {
                    return Err(e);
                }
            }

            if elapsed >= max_wait_time {
                return Err(Error::new(ErrorData::Timeout {
                    message: "Timeout waiting for stack creation".to_string(),
                }));
            }
        }
    }

    fn get_test_stack_name(&self) -> String {
        // Use shorter UUID to avoid IAM role name length issues
        let uuid_short = &Uuid::new_v4().simple().to_string()[..8];
        format!("alien-test-{}", uuid_short)
    }

    fn get_minimal_template(&self) -> String {
        format!(
            r#"{{
            "AWSTemplateFormatVersion": "2010-09-09",
            "Description": "Minimal test stack for alien-infra CloudFormation client tests",
            "Resources": {{
                "TestS3Bucket": {{
                    "Type": "AWS::S3::Bucket",
                    "Properties": {{
                        "BucketName": {{
                            "Fn::Sub": "alien-test-bucket-{}-${{AWS::StackName}}"
                        }}
                    }}
                }}
            }},
            "Outputs": {{
                "BucketName": {{
                    "Description": "Name of the test S3 bucket",
                    "Value": {{
                        "Ref": "TestS3Bucket"
                    }}
                }}
            }}
        }}"#,
            self.account_id
        )
    }

    fn get_template_with_iam_role(&self) -> String {
        format!(
            r#"{{
            "AWSTemplateFormatVersion": "2010-09-09",
            "Description": "Test stack with IAM role for alien-infra CloudFormation client tests",
            "Resources": {{
                "TestS3Bucket": {{
                    "Type": "AWS::S3::Bucket",
                    "Properties": {{
                        "BucketName": {{
                            "Fn::Sub": "alien-test-bucket-{}-${{AWS::StackName}}"
                        }}
                    }}
                }},
                "TestIAMRole": {{
                    "Type": "AWS::IAM::Role",
                    "Properties": {{
                        "RoleName": {{
                            "Fn::Sub": "AlienTestRole-${{AWS::StackName}}"
                        }},
                        "AssumeRolePolicyDocument": {{
                            "Version": "2012-10-17",
                            "Statement": [
                                {{
                                    "Effect": "Allow",
                                    "Principal": {{
                                        "Service": "lambda.amazonaws.com"
                                    }},
                                    "Action": "sts:AssumeRole"
                                }}
                            ]
                        }}
                    }}
                }}
            }},
            "Outputs": {{
                "BucketName": {{
                    "Description": "Name of the test S3 bucket",
                    "Value": {{
                        "Ref": "TestS3Bucket"
                    }}
                }},
                "RoleArn": {{
                    "Description": "ARN of the test IAM role",
                    "Value": {{
                        "Fn::GetAtt": ["TestIAMRole", "Arn"]
                    }}
                }}
            }}
        }}"#,
            self.account_id
        )
    }

    async fn create_test_stack(
        &self,
        stack_name: &str,
        template: &str,
    ) -> Result<CreateStackResponse, Error> {
        let request = CreateStackRequest::builder()
            .stack_name(stack_name.to_string())
            .template_body(template.to_string())
            .description(
                "Test stack created by alien-infra CloudFormation client tests".to_string(),
            )
            .capabilities(vec!["CAPABILITY_NAMED_IAM".to_string()])
            .timeout_in_minutes(10)
            .build();

        let result = self.client.create_stack(request).await;
        if result.is_ok() {
            self.track_stack(stack_name);
        }
        result
    }

    async fn manual_cleanup_stack(&self, stack_name: &str) {
        self.cleanup_stack(stack_name).await;
        self.untrack_stack(stack_name);
    }
}

#[test_context(CloudFormationTestContext)]
#[tokio::test]
async fn test_create_describe_delete_stack(ctx: &mut CloudFormationTestContext) {
    let stack_name = ctx.get_test_stack_name();
    let template = ctx.get_minimal_template();

    info!(
        "🚀 Testing full CloudFormation stack lifecycle: {}",
        stack_name
    );

    // Create the stack
    let create_result = ctx.create_test_stack(&stack_name, &template).await;
    match create_result {
        Ok(response) => {
            info!(
                "✅ Stack creation initiated: {}",
                response.create_stack_result.stack_id
            );
            assert!(response.create_stack_result.stack_id.contains(&stack_name));

            // Wait for stack creation to complete
            ctx.wait_for_stack_creation(&stack_name)
                .await
                .expect("Stack creation should complete successfully");

            // Describe the stack
            let describe_result = ctx
                .client
                .describe_stacks(
                    DescribeStacksRequest::builder()
                        .stack_name(stack_name.clone())
                        .build(),
                )
                .await;

            match describe_result {
                Ok(response) => {
                    info!("✅ Stack described successfully");
                    let stacks = &response.describe_stacks_result.stacks.member;
                    assert_eq!(stacks.len(), 1);

                    let stack = &stacks[0];
                    assert_eq!(stack.stack_name, stack_name);
                    assert_eq!(stack.stack_status, "CREATE_COMPLETE");
                    assert!(stack.stack_id.contains(&stack_name));
                    assert!(stack.description.is_some());

                    // Check outputs
                    if let Some(ref outputs) = stack.outputs {
                        assert!(!outputs.member.is_empty());
                        for output in &outputs.member {
                            info!(
                                "📤 Stack output: {} = {}",
                                output.output_key, output.output_value
                            );
                            assert!(!output.output_key.is_empty());
                            assert!(!output.output_value.is_empty());
                        }
                    }
                }
                Err(e) => {
                    panic!("Describe stack failed: {:?}", e);
                }
            }

            // Describe stack resources
            let resources_result = ctx
                .client
                .describe_stack_resources(
                    DescribeStackResourcesRequest::builder()
                        .stack_name(stack_name.clone())
                        .build(),
                )
                .await;

            match resources_result {
                Ok(response) => {
                    info!("✅ Stack resources described successfully");
                    let resources = &response
                        .describe_stack_resources_result
                        .stack_resources
                        .member;
                    assert!(!resources.is_empty());

                    for resource in resources {
                        info!(
                            "📦 Resource: {} ({})",
                            resource.logical_resource_id, resource.resource_type
                        );
                        assert!(!resource.logical_resource_id.is_empty());
                        assert!(!resource.resource_type.is_empty());
                        assert!(!resource.resource_status.is_empty());
                    }

                    // Test describe individual stack resource (singular)
                    if let Some(first_resource) = resources.first() {
                        let resource_result = ctx.client.describe_stack_resource(
                            alien_aws_clients::cloudformation::DescribeStackResourceRequest::builder()
                                .stack_name(stack_name.clone())
                                .logical_resource_id(first_resource.logical_resource_id.clone())
                                .build()
                        ).await;

                        match resource_result {
                            Ok(response) => {
                                info!("✅ Individual stack resource described successfully");
                                let detail = &response
                                    .describe_stack_resource_result
                                    .stack_resource_detail;

                                // Verify required fields
                                assert!(!detail.last_updated_timestamp.is_empty());
                                assert!(!detail.logical_resource_id.is_empty());
                                assert!(!detail.resource_status.is_empty());
                                assert!(!detail.resource_type.is_empty());

                                // Verify it matches the resource from list
                                assert_eq!(
                                    detail.logical_resource_id,
                                    first_resource.logical_resource_id
                                );
                                assert_eq!(detail.resource_type, first_resource.resource_type);
                                assert_eq!(detail.resource_status, first_resource.resource_status);

                                info!(
                                    "📦 Resource detail: {} ({}) - Status: {}",
                                    detail.logical_resource_id,
                                    detail.resource_type,
                                    detail.resource_status
                                );

                                // Verify optional fields if present
                                if let Some(ref physical_id) = detail.physical_resource_id {
                                    assert!(!physical_id.is_empty());
                                    info!("🔗 Physical resource ID: {}", physical_id);
                                }
                                if let Some(ref stack_id) = detail.stack_id {
                                    assert!(!stack_id.is_empty());
                                }
                                if let Some(ref stack_name_in_detail) = detail.stack_name {
                                    assert_eq!(stack_name_in_detail, &stack_name);
                                }
                            }
                            Err(e) => {
                                panic!("Describe individual stack resource failed: {:?}", e);
                            }
                        }
                    }

                    // Test describe individual resource with non-existent logical ID
                    let non_existent_resource_result = ctx.client.describe_stack_resource(
                        alien_aws_clients::cloudformation::DescribeStackResourceRequest::builder()
                            .stack_name(stack_name.clone())
                            .logical_resource_id("NonExistentResource".to_string())
                            .build()
                    ).await;

                    assert!(non_existent_resource_result.is_err());
                    match non_existent_resource_result.unwrap_err().error {
                        Some(ErrorData::RemoteResourceNotFound { .. }) => {
                            info!("✅ Correctly detected non-existent resource");
                        }
                        other => {
                            info!(
                                "ℹ️ Got different error for non-existent resource: {:?}",
                                other
                            );
                            // Still acceptable as long as it's an error
                        }
                    }
                }
                Err(e) => {
                    panic!("Describe stack resources failed: {:?}", e);
                }
            }
        }
        Err(e) => {
            panic!("Stack creation failed: {:?}. Please ensure you have proper AWS credentials and CloudFormation permissions", e);
        }
    }

    // Manual cleanup - the stack will also be cleaned up automatically via teardown
    ctx.manual_cleanup_stack(&stack_name).await;
}

#[test_context(CloudFormationTestContext)]
#[tokio::test]
async fn test_create_stack_with_parameters_and_capabilities(ctx: &mut CloudFormationTestContext) {
    let stack_name = ctx.get_test_stack_name();
    let template = ctx.get_template_with_iam_role();

    info!(
        "🎛️ Testing stack creation with parameters and capabilities: {}",
        stack_name
    );

    // Create stack with IAM capabilities
    let create_result = ctx.create_test_stack(&stack_name, &template).await;
    match create_result {
        Ok(response) => {
            info!(
                "✅ Stack with IAM resources creation initiated: {}",
                response.create_stack_result.stack_id
            );

            // Wait for creation to complete
            ctx.wait_for_stack_creation(&stack_name)
                .await
                .expect("Stack creation should complete successfully");

            // Describe the created stack to verify capabilities
            let describe_result = ctx
                .client
                .describe_stacks(
                    DescribeStacksRequest::builder()
                        .stack_name(stack_name.clone())
                        .build(),
                )
                .await
                .expect("Should be able to describe stack");

            let stack = &describe_result.describe_stacks_result.stacks.member[0];
            info!("✅ Stack created with status: {}", stack.stack_status);

            // Verify capabilities were applied
            if let Some(ref capabilities) = stack.capabilities {
                assert!(!capabilities.member.is_empty());
                assert!(capabilities
                    .member
                    .contains(&"CAPABILITY_NAMED_IAM".to_string()));
                info!("✅ Stack capabilities verified: {:?}", capabilities.member);
            }

            // Verify outputs exist
            if let Some(ref outputs) = stack.outputs {
                assert!(!outputs.member.is_empty());
                for output in &outputs.member {
                    info!("📤 Output: {} = {}", output.output_key, output.output_value);
                }
            }
        }
        Err(e) => {
            panic!("Stack creation with capabilities failed: {:?}", e);
        }
    }
}

#[test_context(CloudFormationTestContext)]
#[tokio::test]
async fn test_create_stack_already_exists(ctx: &mut CloudFormationTestContext) {
    let stack_name = ctx.get_test_stack_name();
    let template = ctx.get_minimal_template();

    info!("🔄 Testing create duplicate stack: {}", stack_name);

    // Create stack first time
    let create_result = ctx.create_test_stack(&stack_name, &template).await;
    match create_result {
        Ok(_) => {
            info!("✅ First stack creation initiated");

            // Try to create the same stack again immediately (before first one completes)
            let duplicate_request = CreateStackRequest::builder()
                .stack_name(stack_name.clone())
                .template_body(template)
                .description("Duplicate test stack".to_string())
                .build();

            let result = ctx.client.create_stack(duplicate_request).await;

            assert!(result.is_err());
            match result.unwrap_err().error {
                Some(ErrorData::RemoteResourceConflict {
                    resource_type,
                    resource_name,
                    ..
                }) => {
                    assert_eq!(resource_type, "CloudFormation Stack");
                    assert_eq!(resource_name, stack_name);
                    info!("✅ Correctly detected duplicate stack creation");
                }
                other => {
                    panic!("Expected RemoteResourceConflict, got: {:?}", other);
                }
            }
        }
        Err(e) => {
            panic!("Initial stack creation failed: {:?}", e);
        }
    }
}

#[test_context(CloudFormationTestContext)]
#[tokio::test]
async fn test_describe_non_existent_stack(ctx: &mut CloudFormationTestContext) {
    let non_existent_stack = format!("alien-non-existent-stack-{}", Uuid::new_v4().simple());

    info!(
        "❌ Testing describe non-existent stack: {}",
        non_existent_stack
    );

    let mut last_http_error = None;
    for attempt in 0..3 {
        let result = ctx
            .client
            .describe_stacks(
                DescribeStacksRequest::builder()
                    .stack_name(non_existent_stack.clone())
                    .build(),
            )
            .await;

        assert!(result.is_err());
        match result.unwrap_err().error {
            Some(ErrorData::RemoteResourceNotFound {
                resource_type,
                resource_name,
                ..
            }) => {
                assert_eq!(resource_type, "CloudFormation Stack");
                assert_eq!(resource_name, non_existent_stack);
                info!("✅ Correctly detected non-existent stack");
                return;
            }
            Some(ErrorData::HttpRequestFailed { .. }) if attempt < 2 => {
                last_http_error = Some("HttpRequestFailed");
                continue;
            }
            other => {
                panic!("Expected RemoteResourceNotFound, got: {:?}", other);
            }
        }
    }

    panic!(
        "Expected RemoteResourceNotFound, got transient error: {:?}",
        last_http_error
    );
}

#[test_context(CloudFormationTestContext)]
#[tokio::test]
async fn test_delete_non_existent_stack(ctx: &mut CloudFormationTestContext) {
    let non_existent_stack = format!("alien-non-existent-stack-{}", Uuid::new_v4().simple());

    info!(
        "❌ Testing delete non-existent stack: {}",
        non_existent_stack
    );

    let result = ctx
        .client
        .delete_stack(
            DeleteStackRequest::builder()
                .stack_name(non_existent_stack.clone())
                .build(),
        )
        .await;

    // AWS CloudFormation DeleteStack is idempotent - it succeeds even if stack doesn't exist
    // This is actually the correct behavior, but we can check if it returns an error or succeeds
    match result {
        Ok(_) => {
            info!("✅ DeleteStack succeeded for non-existent stack (idempotent behavior)");
        }
        Err(e) if matches!(e.error, Some(ErrorData::RemoteResourceNotFound { .. })) => {
            if let Some(ErrorData::RemoteResourceNotFound {
                resource_type,
                resource_name,
                ..
            }) = e.error
            {
                assert_eq!(resource_type, "CloudFormation Stack");
                assert_eq!(resource_name, non_existent_stack);
            }
            info!("✅ Correctly detected non-existent stack for deletion");
        }
        Err(other) => {
            panic!(
                "Expected success or RemoteResourceNotFound, got: {:?}",
                other
            );
        }
    }
}

#[test_context(CloudFormationTestContext)]
#[tokio::test]
async fn test_create_stack_with_invalid_template(ctx: &mut CloudFormationTestContext) {
    let stack_name = ctx.get_test_stack_name();

    info!(
        "📝 Testing create stack with invalid template: {}",
        stack_name
    );

    let invalid_template = r#"{"invalid": "template", "missing": "required_fields"}"#;

    let request = CreateStackRequest::builder()
        .stack_name(stack_name.clone())
        .template_body(invalid_template.to_string())
        .description("Test stack with invalid template".to_string())
        .build();

    let result = ctx.client.create_stack(request).await;

    assert!(result.is_err());
    match result.unwrap_err().error {
        Some(ErrorData::GenericError { .. }) => {
            info!("✅ Correctly rejected invalid template");
        }
        other => {
            warn!(
                "Got unexpected error type for invalid template: {:?}",
                other
            );
            // Still acceptable as long as it's an error
        }
    }
}

#[test_context(CloudFormationTestContext)]
#[tokio::test]
async fn test_describe_stacks_list_all(ctx: &mut CloudFormationTestContext) {
    info!("📋 Testing describe all stacks (no stack name filter)");

    let result = ctx
        .client
        .describe_stacks(DescribeStacksRequest::builder().build())
        .await;

    match result {
        Ok(response) => {
            info!("✅ Successfully listed all stacks");
            let stacks = &response.describe_stacks_result.stacks.member;
            info!("📊 Total stacks found: {}", stacks.len());

            for stack in stacks {
                info!("📦 Stack: {} ({})", stack.stack_name, stack.stack_status);
                assert!(!stack.stack_name.is_empty());
                assert!(!stack.stack_status.is_empty());
                assert!(!stack.stack_id.is_empty());
            }
        }
        Err(e) => {
            // This might fail if there are no stacks or permission issues
            warn!("List all stacks failed (might be expected): {:?}", e);
        }
    }
}

#[test_context(CloudFormationTestContext)]
#[tokio::test]
async fn test_cloudformation_client_with_invalid_credentials(ctx: &mut CloudFormationTestContext) {
    let aws_config = alien_aws_clients::AwsClientConfig {
        account_id: "123456789012".to_string(),
        region: "us-east-1".to_string(),
        credentials: alien_aws_clients::AwsCredentials::AccessKeys {
            access_key_id: "invalid".to_string(),
            secret_access_key: "invalid".to_string(),
            session_token: None,
        },
        service_overrides: None,
    };
    let cf_client = CloudFormationClient::new(Client::new(), AwsCredentialProvider::from_config_sync(aws_config));

    info!("🔐 Testing CloudFormation client with invalid credentials");

    let result = cf_client
        .describe_stacks(
            DescribeStacksRequest::builder()
                .stack_name("any-stack".to_string())
                .build(),
        )
        .await;

    assert!(result.is_err());
    match result.unwrap_err().error {
        Some(ErrorData::RemoteAccessDenied { .. }) => {
            info!("✅ Correctly detected invalid credentials");
        }
        Some(ErrorData::HttpRequestFailed { .. }) => {
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

#[test_context(CloudFormationTestContext)]
#[tokio::test]
async fn test_stack_request_serialization(ctx: &mut CloudFormationTestContext) {
    info!("🔧 Testing CloudFormation request structure serialization");

    // Test CreateStackRequest builder
    let create_request = CreateStackRequest::builder()
        .stack_name("test-stack".to_string())
        .template_body(ctx.get_minimal_template())
        .description("Test description".to_string())
        .capabilities(vec!["CAPABILITY_IAM".to_string()])
        .timeout_in_minutes(15)
        .build();

    assert_eq!(create_request.stack_name, "test-stack");
    assert!(!create_request.template_body.is_empty());
    assert_eq!(
        create_request.description,
        Some("Test description".to_string())
    );
    assert!(create_request.capabilities.is_some());
    assert_eq!(create_request.timeout_in_minutes, Some(15));

    // Test DescribeStacksRequest builder
    let describe_request = DescribeStacksRequest::builder()
        .stack_name("test-stack".to_string())
        .build();

    assert_eq!(describe_request.stack_name, Some("test-stack".to_string()));

    // Test DeleteStackRequest builder
    let delete_request = DeleteStackRequest::builder()
        .stack_name("test-stack".to_string())
        .build();

    assert_eq!(delete_request.stack_name, "test-stack");

    info!("✅ All request structures are valid");
}

#[test_context(CloudFormationTestContext)]
#[tokio::test]
async fn test_describe_stack_events(ctx: &mut CloudFormationTestContext) {
    let stack_name = ctx.get_test_stack_name();
    let template = ctx.get_minimal_template();

    info!("📅 Testing describe stack events: {}", stack_name);

    // Create a stack first to generate events
    let create_result = ctx.create_test_stack(&stack_name, &template).await;
    match create_result {
        Ok(response) => {
            info!(
                "✅ Stack creation initiated for events test: {}",
                response.create_stack_result.stack_id
            );

            // Wait for stack creation to complete to ensure we have events
            ctx.wait_for_stack_creation(&stack_name)
                .await
                .expect("Stack creation should complete successfully");

            // Test describe stack events
            let events_result = ctx
                .client
                .describe_stack_events(
                    alien_aws_clients::cloudformation::DescribeStackEventsRequest::builder()
                        .stack_name(stack_name.clone())
                        .build(),
                )
                .await;

            match events_result {
                Ok(response) => {
                    info!("✅ Stack events described successfully");
                    let events = &response.describe_stack_events_result.stack_events.member;
                    assert!(!events.is_empty(), "Stack should have at least some events");

                    // Verify event structure
                    for event in events {
                        info!(
                            "📅 Event: {} - {} ({})",
                            event.timestamp,
                            event.resource_type.as_deref().unwrap_or("Unknown"),
                            event.resource_status.as_deref().unwrap_or("Unknown")
                        );

                        // Verify required fields are present
                        assert!(!event.stack_name.is_empty());
                        assert!(!event.event_id.is_empty());
                        assert!(!event.timestamp.is_empty());
                        assert!(!event.stack_id.is_empty());

                        // Verify optional fields if present
                        if let Some(ref resource_type) = event.resource_type {
                            assert!(!resource_type.is_empty());
                        }
                        if let Some(ref logical_resource_id) = event.logical_resource_id {
                            assert!(!logical_resource_id.is_empty());
                        }
                        if let Some(ref resource_status) = event.resource_status {
                            assert!(!resource_status.is_empty());
                        }
                    }

                    // There should be CREATE_IN_PROGRESS and CREATE_COMPLETE events
                    let has_in_progress = events.iter().any(|e| {
                        e.resource_status
                            .as_ref()
                            .is_some_and(|status| status.contains("IN_PROGRESS"))
                    });
                    let has_complete = events.iter().any(|e| {
                        e.resource_status
                            .as_ref()
                            .is_some_and(|status| status.contains("COMPLETE"))
                    });

                    assert!(
                        has_in_progress || has_complete,
                        "Should have progress or complete events"
                    );
                    info!("✅ Stack events validation completed");
                }
                Err(e) => {
                    panic!("Describe stack events failed: {:?}", e);
                }
            }

            // Test describe events for non-existent stack
            let non_existent_stack =
                format!("alien-non-existent-events-{}", Uuid::new_v4().simple());
            let non_existent_result = ctx
                .client
                .describe_stack_events(
                    alien_aws_clients::cloudformation::DescribeStackEventsRequest::builder()
                        .stack_name(non_existent_stack.clone())
                        .build(),
                )
                .await;

            assert!(
                non_existent_result.is_err(),
                "Should fail for non-existent stack"
            );
            match non_existent_result.unwrap_err().error {
                Some(ErrorData::RemoteResourceNotFound {
                    resource_type,
                    resource_name,
                    ..
                }) => {
                    assert_eq!(resource_type, "CloudFormation Stack");
                    assert_eq!(resource_name, non_existent_stack);
                    info!("✅ Correctly detected non-existent stack for events");
                }
                other => {
                    panic!(
                        "Expected RemoteResourceNotFound for events, got: {:?}",
                        other
                    );
                }
            }
        }
        Err(e) => {
            panic!("Stack creation failed for events test: {:?}", e);
        }
    }

    // Manual cleanup
    ctx.manual_cleanup_stack(&stack_name).await;
}
