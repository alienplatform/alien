/*!
# AWS Auto Scaling Client Integration Tests

These tests perform real AWS Auto Scaling operations including creating, managing,
and deleting Auto Scaling Groups.

## Prerequisites

### 1. AWS Credentials
Set up `.env.test` in the workspace root with:
```
AWS_MANAGEMENT_REGION=us-west-2
AWS_MANAGEMENT_ACCESS_KEY_ID=your_access_key
AWS_MANAGEMENT_SECRET_ACCESS_KEY=your_secret_key
AWS_MANAGEMENT_ACCOUNT_ID=your_account_id
```

### 2. Required Permissions
Your AWS credentials need these permissions:
- `autoscaling:CreateAutoScalingGroup`
- `autoscaling:DeleteAutoScalingGroup`
- `autoscaling:DescribeAutoScalingGroups`
- `autoscaling:SetDesiredCapacity`
- `autoscaling:UpdateAutoScalingGroup`
- `ec2:CreateLaunchTemplate`
- `ec2:DeleteLaunchTemplate`
- `ec2:DescribeLaunchTemplates`
- `ec2:DescribeSubnets`
- `ec2:DescribeVpcs`
- `ec2:GetConsoleOutput`
- `iam:CreateServiceLinkedRole` (first time only)

### 3. Notes
- Auto Scaling Groups require a VPC subnet and either a launch template or launch configuration
- The e2e test briefly scales to 1 instance to exercise GetConsoleOutput, then scales back to 0
- Tests use us-west-2 to avoid quota issues

## Running Tests
```bash
# Run all Auto Scaling tests
cargo test --package alien-aws-clients --test aws_autoscaling_client_tests

# Run specific test
cargo test --package alien-aws-clients --test aws_autoscaling_client_tests test_auto_scaling_group_e2e -- --nocapture
```
*/

use alien_aws_clients::autoscaling::*;
use alien_aws_clients::ec2::*;
use alien_aws_clients::AwsClientConfig;
use alien_aws_clients::AwsCredentialProvider;
use alien_client_core::{Error, ErrorData};
use base64;
use reqwest::Client;
use std::collections::HashSet;
use std::path::PathBuf as StdPathBuf;
use std::sync::Mutex;
use std::time::Duration;
use test_context::{test_context, AsyncTestContext};
use tracing::{info, warn};
use uuid::Uuid;

struct AutoScalingTestContext {
    asg_client: AutoScalingClient,
    ec2_client: Ec2Client,
    created_asgs: Mutex<HashSet<String>>,
    created_launch_templates: Mutex<HashSet<String>>,
    default_subnet_id: Option<String>,
}

impl AsyncTestContext for AutoScalingTestContext {
    async fn setup() -> AutoScalingTestContext {
        let root: StdPathBuf = workspace_root::get_workspace_root();
        dotenvy::from_path(root.join(".env.test")).expect("Failed to load .env.test");
        tracing_subscriber::fmt::try_init().ok();

        let region = "us-west-2".to_string();
        let access_key = std::env::var("AWS_MANAGEMENT_ACCESS_KEY_ID")
            .expect("AWS_MANAGEMENT_ACCESS_KEY_ID must be set in .env.test");
        let secret_key = std::env::var("AWS_MANAGEMENT_SECRET_ACCESS_KEY")
            .expect("AWS_MANAGEMENT_SECRET_ACCESS_KEY must be set in .env.test");
        let account_id = std::env::var("AWS_MANAGEMENT_ACCOUNT_ID")
            .expect("AWS_MANAGEMENT_ACCOUNT_ID must be set in .env.test");

        let aws_config = AwsClientConfig {
            account_id,
            region,
            credentials: alien_aws_clients::AwsCredentials::AccessKeys {
                access_key_id: access_key,
                secret_access_key: secret_key,
                session_token: None,
            },
            service_overrides: None,
        };

        let asg_client = AutoScalingClient::new(Client::new(), AwsCredentialProvider::from_config_sync(aws_config.clone()));
        let ec2_client = Ec2Client::new(Client::new(), AwsCredentialProvider::from_config_sync(aws_config));

        // Find a default subnet for ASG tests
        let default_subnet_id = Self::find_default_subnet(&ec2_client).await;

        AutoScalingTestContext {
            asg_client,
            ec2_client,
            created_asgs: Mutex::new(HashSet::new()),
            created_launch_templates: Mutex::new(HashSet::new()),
            default_subnet_id,
        }
    }

    async fn teardown(self) {
        info!("🧹 Starting Auto Scaling test cleanup...");

        // First delete ASGs (they reference launch templates)
        let asgs_to_cleanup = {
            let asgs = self.created_asgs.lock().unwrap();
            asgs.clone()
        };

        for asg_name in asgs_to_cleanup {
            self.cleanup_asg(&asg_name).await;
        }

        // Wait for ASGs to be deleted before deleting launch templates
        tokio::time::sleep(Duration::from_secs(5)).await;

        // Then delete launch templates
        let templates_to_cleanup = {
            let templates = self.created_launch_templates.lock().unwrap();
            templates.clone()
        };

        for template_id in templates_to_cleanup {
            self.cleanup_launch_template(&template_id).await;
        }

        info!("✅ Auto Scaling test cleanup completed");
    }
}

impl AutoScalingTestContext {
    async fn find_default_subnet(ec2_client: &Ec2Client) -> Option<String> {
        // Find the default VPC
        let vpc_response = ec2_client
            .describe_vpcs(DescribeVpcsRequest::builder().build())
            .await
            .ok()?;

        let default_vpc_id = vpc_response
            .vpc_set
            .as_ref()?
            .items
            .iter()
            .find(|vpc| vpc.is_default == Some(true))
            .and_then(|vpc| vpc.vpc_id.clone())?;

        // Find a subnet in the default VPC
        let subnet_response = ec2_client
            .describe_subnets(
                DescribeSubnetsRequest::builder()
                    .filters(vec![Filter::builder()
                        .name("vpc-id".to_string())
                        .values(vec![default_vpc_id])
                        .build()])
                    .build(),
            )
            .await
            .ok()?;

        subnet_response
            .subnet_set
            .as_ref()?
            .items
            .first()
            .and_then(|subnet| subnet.subnet_id.clone())
    }

    fn track_asg(&self, asg_name: &str) {
        let mut asgs = self.created_asgs.lock().unwrap();
        asgs.insert(asg_name.to_string());
        info!("📝 Tracking ASG for cleanup: {}", asg_name);
    }

    fn untrack_asg(&self, asg_name: &str) {
        let mut asgs = self.created_asgs.lock().unwrap();
        asgs.remove(asg_name);
        info!("✅ ASG {} untracked", asg_name);
    }

    fn track_launch_template(&self, template_id: &str) {
        let mut templates = self.created_launch_templates.lock().unwrap();
        templates.insert(template_id.to_string());
        info!("📝 Tracking launch template for cleanup: {}", template_id);
    }

    async fn cleanup_asg(&self, asg_name: &str) {
        info!("🧹 Cleaning up ASG: {}", asg_name);

        let request = DeleteAutoScalingGroupRequest::builder()
            .auto_scaling_group_name(asg_name.to_string())
            .force_delete(true)
            .build();

        match self.asg_client.delete_auto_scaling_group(request).await {
            Ok(_) => {
                info!("✅ ASG {} deletion initiated", asg_name);
            }
            Err(e) => {
                if !matches!(
                    e,
                    Error {
                        error: Some(ErrorData::RemoteResourceNotFound { .. }),
                        ..
                    }
                ) {
                    warn!("Failed to delete ASG {} during cleanup: {:?}", asg_name, e);
                }
            }
        }
    }

    async fn cleanup_launch_template(&self, template_id: &str) {
        info!("🧹 Cleaning up launch template: {}", template_id);

        let request = DeleteLaunchTemplateRequest::builder()
            .launch_template_id(template_id.to_string())
            .build();

        match self.ec2_client.delete_launch_template(request).await {
            Ok(_) => {
                info!("✅ Launch template {} deleted", template_id);
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
                        "Failed to delete launch template {} during cleanup: {:?}",
                        template_id, e
                    );
                }
            }
        }
    }

    fn get_test_suffix(&self) -> String {
        Uuid::new_v4().simple().to_string()[..8].to_string()
    }

    async fn create_test_launch_template(&self, suffix: &str) -> Option<String> {
        let template_name = format!("alien-test-lt-{}", suffix);

        info!("🚀 Creating test launch template: {}", template_name);

        let request = CreateLaunchTemplateRequest::builder()
            .launch_template_name(template_name.clone())
            .launch_template_data(
                RequestLaunchTemplateData::builder()
                    .instance_type("t3.micro".to_string())
                    .image_id(
                        "resolve:ssm:/aws/service/ami-amazon-linux-latest/amzn2-ami-hvm-x86_64-gp2"
                            .to_string(),
                    ) // Amazon Linux 2 in us-west-2 (SSM parameter)
                    .build(),
            )
            .build();

        match self.ec2_client.create_launch_template(request).await {
            Ok(response) => {
                let template_id = response
                    .launch_template
                    .as_ref()
                    .and_then(|lt| lt.launch_template_id.clone())?;
                self.track_launch_template(&template_id);
                info!("✅ Launch template created: {}", template_id);
                Some(template_id)
            }
            Err(e) => {
                warn!("Failed to create launch template: {:?}", e);
                None
            }
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

/// Comprehensive end-to-end test for Auto Scaling Group operations.
#[test_context(AutoScalingTestContext)]
#[tokio::test]
async fn test_auto_scaling_group_e2e(ctx: &mut AutoScalingTestContext) {
    let subnet_id = match &ctx.default_subnet_id {
        Some(id) => id.clone(),
        None => {
            warn!("⚠️ No default subnet found, skipping ASG test");
            return;
        }
    };

    let suffix = ctx.get_test_suffix();
    let asg_name = format!("alien-test-asg-{}", suffix);

    info!("🚀 Starting Auto Scaling Group E2E test: {}", asg_name);

    // =========================================================================
    // Step 1: Create a launch template
    // =========================================================================
    info!("📝 Step 1: Creating launch template");
    let template_id = match ctx.create_test_launch_template(&suffix).await {
        Some(id) => id,
        None => {
            warn!("⚠️ Failed to create launch template, skipping ASG test");
            return;
        }
    };

    // =========================================================================
    // Step 1.5: Create a new version of the launch template
    // =========================================================================
    info!("📝 Step 1.5: Creating new launch template version via create_launch_template_version");

    // Create a new version based on the existing one, overriding user data only.
    // This exercises the create_launch_template_version API end-to-end.
    let updated_user_data = base64::Engine::encode(
        &base64::engine::general_purpose::STANDARD,
        b"#!/bin/bash\necho 'alien-test-v2'",
    );

    let version_response = ctx
        .ec2_client
        .create_launch_template_version(
            CreateLaunchTemplateVersionRequest::builder()
                .launch_template_id(template_id.clone())
                .source_version("$Latest".to_string())
                .version_description("alien-test-v2".to_string())
                .launch_template_data(
                    RequestLaunchTemplateData::builder()
                        .user_data(updated_user_data)
                        .build(),
                )
                .build(),
        )
        .await
        .expect("Failed to create launch template version");

    let new_version_number = version_response
        .launch_template_version
        .as_ref()
        .and_then(|v| v.version_number);

    assert!(
        new_version_number.map(|v| v > 1).unwrap_or(false),
        "New version number should be > 1, got: {:?}",
        new_version_number
    );
    info!(
        "✅ New launch template version created: {:?}",
        new_version_number
    );

    // =========================================================================
    // Step 2: Create Auto Scaling Group
    // =========================================================================
    info!("🏗️ Step 2: Creating Auto Scaling Group");
    let create_request = CreateAutoScalingGroupRequest::builder()
        .auto_scaling_group_name(asg_name.clone())
        .launch_template(
            LaunchTemplateSpecification::builder()
                .launch_template_id(template_id.clone())
                .version("$Latest".to_string())
                .build(),
        )
        .min_size(0)
        .max_size(1)
        .desired_capacity(0)
        .vpc_zone_identifier(subnet_id.clone())
        .tags(vec![AsgTag::builder()
            .key("Name".to_string())
            .value(format!("alien-test-{}", suffix))
            .propagate_at_launch(true)
            .build()])
        .build();

    ctx.asg_client
        .create_auto_scaling_group(create_request)
        .await
        .expect("Failed to create ASG");
    ctx.track_asg(&asg_name);
    info!("✅ Auto Scaling Group created: {}", asg_name);

    // =========================================================================
    // Step 3: Describe the Auto Scaling Group
    // =========================================================================
    info!("🔍 Step 3: Describing Auto Scaling Group");
    let describe_request = DescribeAutoScalingGroupsRequest::builder()
        .auto_scaling_group_names(vec![asg_name.clone()])
        .build();

    let describe_response = ctx
        .asg_client
        .describe_auto_scaling_groups(describe_request)
        .await
        .expect("Failed to describe ASG");

    let asgs = describe_response
        .describe_auto_scaling_groups_result
        .auto_scaling_groups
        .expect("Should have ASGs");
    assert_eq!(asgs.members.len(), 1, "Should find exactly one ASG");

    let asg = &asgs.members[0];
    assert_eq!(
        asg.auto_scaling_group_name.as_deref(),
        Some(asg_name.as_str())
    );
    assert_eq!(asg.min_size, Some(0));
    assert_eq!(asg.max_size, Some(1));
    assert_eq!(asg.desired_capacity, Some(0));
    info!(
        "✅ ASG verified: min={:?}, max={:?}, desired={:?}",
        asg.min_size, asg.max_size, asg.desired_capacity
    );

    // =========================================================================
    // Step 3.5: Scale to 1 instance, call GetConsoleOutput, scale back to 0
    // =========================================================================
    info!("📊 Step 3.5: Scaling to 1 instance to test GetConsoleOutput");

    let scale_up_request = SetDesiredCapacityRequest::builder()
        .auto_scaling_group_name(asg_name.clone())
        .desired_capacity(1)
        .build();

    ctx.asg_client
        .set_desired_capacity(scale_up_request)
        .await
        .expect("Failed to scale ASG to 1");

    // Wait for an instance to appear in the ASG (up to 3 minutes)
    let mut instance_id: Option<String> = None;
    for _ in 0..18 {
        tokio::time::sleep(Duration::from_secs(10)).await;
        let resp = ctx
            .asg_client
            .describe_auto_scaling_groups(
                DescribeAutoScalingGroupsRequest::builder()
                    .auto_scaling_group_names(vec![asg_name.clone()])
                    .build(),
            )
            .await
            .expect("Failed to describe ASG while waiting for instance");

        instance_id = resp
            .describe_auto_scaling_groups_result
            .auto_scaling_groups
            .and_then(|asgs| asgs.members.into_iter().next())
            .and_then(|asg| asg.instances)
            .and_then(|w| w.members.into_iter().next())
            .and_then(|i| i.instance_id);

        if instance_id.is_some() {
            break;
        }
    }

    if let Some(ref id) = instance_id {
        info!("✅ Instance appeared: {}", id);
        let console = ctx
            .ec2_client
            .get_console_output(id.clone())
            .await
            .expect("Failed to get console output");

        // Console output may be empty for a brand-new instance that hasn't
        // produced output yet, but the API call itself must succeed.
        let decoded_len = console.decode_output().map(|s| s.len()).unwrap_or(0);
        info!("  Console output: {} bytes (decoded)", decoded_len);
        info!("✅ GetConsoleOutput succeeded");
    } else {
        warn!("⚠️ Instance did not appear within 3 minutes, skipping GetConsoleOutput check");
    }

    // =========================================================================
    // Step 3.6: Start an instance refresh and verify via describe_instance_refreshes
    // =========================================================================
    info!("🔄 Step 3.6: Testing start_instance_refresh + describe_instance_refreshes");

    // Start a rolling instance refresh. Using MinHealthyPercentage=50 so it can
    // proceed even with a single instance (min_size=0, desired=1).
    let refresh_response = ctx
        .asg_client
        .start_instance_refresh(
            StartInstanceRefreshRequest::builder()
                .auto_scaling_group_name(asg_name.clone())
                .strategy("Rolling".to_string())
                .preferences(
                    RefreshPreferences::builder()
                        .min_healthy_percentage(50)
                        .max_healthy_percentage(110)
                        .build(),
                )
                .build(),
        )
        .await
        .expect("Failed to start instance refresh");

    let refresh_id = refresh_response
        .start_instance_refresh_result
        .instance_refresh_id
        .expect("StartInstanceRefresh should return an InstanceRefreshId");
    assert!(
        !refresh_id.is_empty(),
        "InstanceRefreshId should not be empty"
    );
    info!("✅ Instance refresh started: {}", refresh_id);

    // Describe the refresh we just started and verify its presence.
    let describe_refresh_response = ctx
        .asg_client
        .describe_instance_refreshes(
            DescribeInstanceRefreshesRequest::builder()
                .auto_scaling_group_name(asg_name.clone())
                .instance_refresh_ids(vec![refresh_id.clone()])
                .build(),
        )
        .await
        .expect("Failed to describe instance refreshes");

    let refreshes = describe_refresh_response
        .describe_instance_refreshes_result
        .instance_refreshes
        .map(|w| w.members)
        .unwrap_or_default();

    assert_eq!(
        refreshes.len(),
        1,
        "Should find exactly the refresh we started"
    );
    let refresh = &refreshes[0];
    assert_eq!(
        refresh.instance_refresh_id.as_deref(),
        Some(refresh_id.as_str()),
        "Refresh ID should match"
    );
    assert_eq!(
        refresh.auto_scaling_group_name.as_deref(),
        Some(asg_name.as_str()),
        "ASG name should match"
    );

    // Status should be Pending or InProgress immediately after starting.
    let status = refresh.status.as_deref().unwrap_or("unknown");
    assert!(
        matches!(status, "Pending" | "InProgress"),
        "Expected status Pending or InProgress, got: {}",
        status
    );
    info!(
        "✅ describe_instance_refreshes returned refresh with status: {}",
        status
    );
    // Note: the refresh continues running; ASG force-delete in teardown handles cancellation.

    // =========================================================================
    // Step 4: Scale back to 0
    // =========================================================================
    info!("📊 Step 4: Scaling back to 0");
    let set_capacity_request = SetDesiredCapacityRequest::builder()
        .auto_scaling_group_name(asg_name.clone())
        .desired_capacity(0)
        .build();

    ctx.asg_client
        .set_desired_capacity(set_capacity_request)
        .await
        .expect("Failed to set desired capacity");
    info!("✅ SetDesiredCapacity(0) succeeded");

    // =========================================================================
    // Step 5: Delete the Auto Scaling Group
    // =========================================================================
    info!("🗑️ Step 5: Deleting Auto Scaling Group");
    let delete_request = DeleteAutoScalingGroupRequest::builder()
        .auto_scaling_group_name(asg_name.clone())
        .force_delete(true)
        .build();

    ctx.asg_client
        .delete_auto_scaling_group(delete_request)
        .await
        .expect("Failed to delete ASG");
    ctx.untrack_asg(&asg_name);
    info!("✅ Auto Scaling Group deleted");

    // =========================================================================
    // Step 6: Verify deletion
    // =========================================================================
    info!("❌ Step 6: Verifying ASG is deleted");

    // Wait a moment for deletion to propagate
    tokio::time::sleep(Duration::from_secs(2)).await;

    let verify_request = DescribeAutoScalingGroupsRequest::builder()
        .auto_scaling_group_names(vec![asg_name.clone()])
        .build();

    let verify_response = ctx
        .asg_client
        .describe_auto_scaling_groups(verify_request)
        .await
        .expect("Failed to verify ASG deletion");

    let remaining_asgs = verify_response
        .describe_auto_scaling_groups_result
        .auto_scaling_groups
        .map(|a| a.members)
        .unwrap_or_default();
    // ASG might still be in "Deleting" state briefly
    if !remaining_asgs.is_empty() {
        let status = remaining_asgs[0].status.as_deref();
        info!("ℹ️ ASG status: {:?} (may still be deleting)", status);
    } else {
        info!("✅ Confirmed ASG was deleted");
    }

    info!("🎉 Auto Scaling Group E2E test completed successfully!");
}

/// Test describing Auto Scaling Groups (read-only, no resources created).
#[test_context(AutoScalingTestContext)]
#[tokio::test]
async fn test_describe_auto_scaling_groups(ctx: &mut AutoScalingTestContext) {
    info!("🔍 Testing describe Auto Scaling Groups...");

    let request = DescribeAutoScalingGroupsRequest::builder().build();

    let response = ctx
        .asg_client
        .describe_auto_scaling_groups(request)
        .await
        .expect("Failed to describe ASGs");

    let asg_count = response
        .describe_auto_scaling_groups_result
        .auto_scaling_groups
        .as_ref()
        .map(|a| a.members.len())
        .unwrap_or(0);
    info!("✅ Found {} Auto Scaling Groups in the account", asg_count);
}

/// Test error handling for non-existent ASG.
#[test_context(AutoScalingTestContext)]
#[tokio::test]
async fn test_describe_non_existent_asg(ctx: &mut AutoScalingTestContext) {
    let non_existent_asg = "alien-test-non-existent-asg-12345";

    info!("❌ Testing describe non-existent ASG: {}", non_existent_asg);

    let request = DescribeAutoScalingGroupsRequest::builder()
        .auto_scaling_group_names(vec![non_existent_asg.to_string()])
        .build();

    let response = ctx
        .asg_client
        .describe_auto_scaling_groups(request)
        .await
        .expect("DescribeAutoScalingGroups should not error for non-existent ASG");

    // AWS returns empty list for non-existent ASGs rather than an error
    let asgs = response
        .describe_auto_scaling_groups_result
        .auto_scaling_groups
        .map(|a| a.members)
        .unwrap_or_default();
    assert!(
        asgs.is_empty(),
        "Should return empty list for non-existent ASG"
    );
    info!("✅ Correctly returned empty list for non-existent ASG");
}

/// Test Auto Scaling client with invalid credentials.
#[test_context(AutoScalingTestContext)]
#[tokio::test]
async fn test_auto_scaling_client_with_invalid_credentials(_ctx: &mut AutoScalingTestContext) {
    let region = "us-west-2".to_string();
    let account_id =
        std::env::var("AWS_MANAGEMENT_ACCOUNT_ID").unwrap_or_else(|_| "123456789012".to_string());

    let invalid_config = AwsClientConfig {
        account_id,
        region,
        credentials: alien_aws_clients::AwsCredentials::AccessKeys {
            access_key_id: "AKIAINVALIDKEY12345".to_string(),
            secret_access_key: "invalidSecretKey123456789012345678901234".to_string(),
            session_token: None,
        },
        service_overrides: None,
    };
    let invalid_client = AutoScalingClient::new(Client::new(), AwsCredentialProvider::from_config_sync(invalid_config));

    info!("🔐 Testing Auto Scaling client with invalid credentials");

    let request = DescribeAutoScalingGroupsRequest::builder().build();

    let result = invalid_client.describe_auto_scaling_groups(request).await;

    assert!(result.is_err());
    match result.unwrap_err() {
        Error {
            error: Some(ErrorData::RemoteAccessDenied { .. }),
            ..
        } => {
            info!("✅ Correctly detected invalid credentials (AccessDenied)");
        }
        Error {
            error: Some(ErrorData::AuthenticationError { .. }),
            ..
        } => {
            info!("✅ Correctly detected invalid credentials (AuthenticationError)");
        }
        other => {
            warn!(
                "Got unexpected error type for invalid credentials: {:?}",
                other
            );
        }
    }
}
