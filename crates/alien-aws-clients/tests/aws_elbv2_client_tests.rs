/*!
# AWS ELBv2 (Application Load Balancer) Client Integration Tests

These tests perform real AWS ELBv2 operations including creating and managing
Application Load Balancers, target groups, listeners, and targets.

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
- `elasticloadbalancing:CreateLoadBalancer`
- `elasticloadbalancing:DeleteLoadBalancer`
- `elasticloadbalancing:DescribeLoadBalancers`
- `elasticloadbalancing:CreateTargetGroup`
- `elasticloadbalancing:DeleteTargetGroup`
- `elasticloadbalancing:DescribeTargetGroups`
- `elasticloadbalancing:RegisterTargets`
- `elasticloadbalancing:DeregisterTargets`
- `elasticloadbalancing:DescribeTargetHealth`
- `elasticloadbalancing:CreateListener`
- `elasticloadbalancing:DeleteListener`
- `elasticloadbalancing:DescribeListeners`
- `elasticloadbalancing:AddTags`
- `ec2:DescribeVpcs`
- `ec2:DescribeSubnets`
- `ec2:DescribeSecurityGroups`
- `ec2:CreateSecurityGroup`
- `ec2:DeleteSecurityGroup`

### 3. Notes
- ALBs require at least 2 subnets in different AZs
- Tests use us-west-2 to avoid quota issues
- ALB provisioning takes 2-3 minutes

## Running Tests
```bash
# Run all ELBv2 tests
cargo test --package alien-aws-clients --test aws_elbv2_client_tests

# Run specific test
cargo test --package alien-aws-clients --test aws_elbv2_client_tests test_elbv2_e2e -- --nocapture
```
*/

use alien_aws_clients::ec2::*;
use alien_aws_clients::elbv2::*;
use alien_aws_clients::AwsClientConfig;
use alien_aws_clients::AwsCredentialProvider;
use alien_client_core::{Error, ErrorData};
use reqwest::Client;
use std::collections::HashSet;
use std::path::PathBuf as StdPathBuf;
use std::sync::Mutex;
use std::time::Duration;
use test_context::{test_context, AsyncTestContext};
use tracing::{info, warn};
use uuid::Uuid;

struct Elbv2TestContext {
    elbv2_client: Elbv2Client,
    ec2_client: Ec2Client,
    created_load_balancers: Mutex<HashSet<String>>,
    created_target_groups: Mutex<HashSet<String>>,
    created_listeners: Mutex<HashSet<String>>,
    created_security_groups: Mutex<HashSet<String>>,
    default_vpc_id: Option<String>,
    default_subnet_ids: Vec<String>,
}

impl AsyncTestContext for Elbv2TestContext {
    async fn setup() -> Elbv2TestContext {
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

        let elbv2_client = Elbv2Client::new(
            Client::new(),
            AwsCredentialProvider::from_config_sync(aws_config.clone()),
        );
        let ec2_client = Ec2Client::new(
            Client::new(),
            AwsCredentialProvider::from_config_sync(aws_config),
        );

        // Find default VPC and subnets
        let (default_vpc_id, default_subnet_ids) =
            Self::find_default_vpc_and_subnets(&ec2_client).await;

        Elbv2TestContext {
            elbv2_client,
            ec2_client,
            created_load_balancers: Mutex::new(HashSet::new()),
            created_target_groups: Mutex::new(HashSet::new()),
            created_listeners: Mutex::new(HashSet::new()),
            created_security_groups: Mutex::new(HashSet::new()),
            default_vpc_id,
            default_subnet_ids,
        }
    }

    async fn teardown(self) {
        info!("🧹 Starting ELBv2 test cleanup...");

        // Cleanup order: listeners -> load balancers -> target groups -> security groups

        // 1. Delete listeners
        let listeners_to_cleanup = {
            let listeners = self.created_listeners.lock().unwrap();
            listeners.clone()
        };
        for listener_arn in listeners_to_cleanup {
            self.cleanup_listener(&listener_arn).await;
        }

        // 2. Delete load balancers
        let lbs_to_cleanup = {
            let lbs = self.created_load_balancers.lock().unwrap();
            lbs.clone()
        };
        for lb_arn in lbs_to_cleanup {
            self.cleanup_load_balancer(&lb_arn).await;
        }

        // Wait for LBs to be deleted
        if !self.created_load_balancers.lock().unwrap().is_empty() {
            info!("⏳ Waiting for load balancers to be deleted...");
            tokio::time::sleep(Duration::from_secs(30)).await;
        }

        // 3. Delete target groups
        let tgs_to_cleanup = {
            let tgs = self.created_target_groups.lock().unwrap();
            tgs.clone()
        };
        for tg_arn in tgs_to_cleanup {
            self.cleanup_target_group(&tg_arn).await;
        }

        // 4. Delete security groups
        let sgs_to_cleanup = {
            let sgs = self.created_security_groups.lock().unwrap();
            sgs.clone()
        };
        for sg_id in sgs_to_cleanup {
            self.cleanup_security_group(&sg_id).await;
        }

        info!("✅ ELBv2 test cleanup completed");
    }
}

impl Elbv2TestContext {
    async fn find_default_vpc_and_subnets(ec2_client: &Ec2Client) -> (Option<String>, Vec<String>) {
        // Find the default VPC
        let vpc_response = match ec2_client
            .describe_vpcs(DescribeVpcsRequest::builder().build())
            .await
        {
            Ok(r) => r,
            Err(_) => return (None, vec![]),
        };

        let default_vpc_id = vpc_response.vpc_set.as_ref().and_then(|set| {
            set.items
                .iter()
                .find(|vpc| vpc.is_default == Some(true))
                .and_then(|vpc| vpc.vpc_id.clone())
        });

        let vpc_id = match default_vpc_id {
            Some(id) => id,
            None => return (None, vec![]),
        };

        // Find subnets in the default VPC (need at least 2 for ALB)
        let subnet_response = match ec2_client
            .describe_subnets(
                DescribeSubnetsRequest::builder()
                    .filters(vec![Filter::builder()
                        .name("vpc-id".to_string())
                        .values(vec![vpc_id.clone()])
                        .build()])
                    .build(),
            )
            .await
        {
            Ok(r) => r,
            Err(_) => return (Some(vpc_id), vec![]),
        };

        // Get subnets from different AZs
        let mut subnet_ids: Vec<String> = vec![];
        let mut seen_azs: HashSet<String> = HashSet::new();

        if let Some(subnet_set) = subnet_response.subnet_set {
            for subnet in subnet_set.items {
                if let (Some(subnet_id), Some(az)) =
                    (subnet.subnet_id.clone(), subnet.availability_zone.clone())
                {
                    if !seen_azs.contains(&az) {
                        seen_azs.insert(az);
                        subnet_ids.push(subnet_id);
                        if subnet_ids.len() >= 2 {
                            break;
                        }
                    }
                }
            }
        }

        (Some(vpc_id), subnet_ids)
    }

    fn track_load_balancer(&self, lb_arn: &str) {
        let mut lbs = self.created_load_balancers.lock().unwrap();
        lbs.insert(lb_arn.to_string());
        info!("📝 Tracking load balancer for cleanup: {}", lb_arn);
    }

    fn untrack_load_balancer(&self, lb_arn: &str) {
        let mut lbs = self.created_load_balancers.lock().unwrap();
        lbs.remove(lb_arn);
        info!("✅ Load balancer {} untracked", lb_arn);
    }

    fn track_target_group(&self, tg_arn: &str) {
        let mut tgs = self.created_target_groups.lock().unwrap();
        tgs.insert(tg_arn.to_string());
        info!("📝 Tracking target group for cleanup: {}", tg_arn);
    }

    fn untrack_target_group(&self, tg_arn: &str) {
        let mut tgs = self.created_target_groups.lock().unwrap();
        tgs.remove(tg_arn);
        info!("✅ Target group {} untracked", tg_arn);
    }

    fn track_listener(&self, listener_arn: &str) {
        let mut listeners = self.created_listeners.lock().unwrap();
        listeners.insert(listener_arn.to_string());
        info!("📝 Tracking listener for cleanup: {}", listener_arn);
    }

    fn track_security_group(&self, sg_id: &str) {
        let mut sgs = self.created_security_groups.lock().unwrap();
        sgs.insert(sg_id.to_string());
        info!("📝 Tracking security group for cleanup: {}", sg_id);
    }

    async fn cleanup_listener(&self, listener_arn: &str) {
        info!("🧹 Cleaning up listener: {}", listener_arn);

        match self.elbv2_client.delete_listener(listener_arn).await {
            Ok(_) => info!("✅ Listener {} deleted", listener_arn),
            Err(e) => {
                if !matches!(
                    e,
                    Error {
                        error: Some(ErrorData::RemoteResourceNotFound { .. }),
                        ..
                    }
                ) {
                    warn!(
                        "Failed to delete listener {} during cleanup: {:?}",
                        listener_arn, e
                    );
                }
            }
        }
    }

    async fn cleanup_load_balancer(&self, lb_arn: &str) {
        info!("🧹 Cleaning up load balancer: {}", lb_arn);

        match self.elbv2_client.delete_load_balancer(lb_arn).await {
            Ok(_) => info!("✅ Load balancer {} deletion initiated", lb_arn),
            Err(e) => {
                if !matches!(
                    e,
                    Error {
                        error: Some(ErrorData::RemoteResourceNotFound { .. }),
                        ..
                    }
                ) {
                    warn!(
                        "Failed to delete load balancer {} during cleanup: {:?}",
                        lb_arn, e
                    );
                }
            }
        }
    }

    async fn cleanup_target_group(&self, tg_arn: &str) {
        info!("🧹 Cleaning up target group: {}", tg_arn);

        match self.elbv2_client.delete_target_group(tg_arn).await {
            Ok(_) => info!("✅ Target group {} deleted", tg_arn),
            Err(e) => {
                if !matches!(
                    e,
                    Error {
                        error: Some(ErrorData::RemoteResourceNotFound { .. }),
                        ..
                    }
                ) {
                    warn!(
                        "Failed to delete target group {} during cleanup: {:?}",
                        tg_arn, e
                    );
                }
            }
        }
    }

    async fn cleanup_security_group(&self, sg_id: &str) {
        info!("🧹 Cleaning up security group: {}", sg_id);

        match self.ec2_client.delete_security_group(sg_id).await {
            Ok(_) => info!("✅ Security group {} deleted", sg_id),
            Err(e) => {
                if !matches!(
                    e,
                    Error {
                        error: Some(ErrorData::RemoteResourceNotFound { .. }),
                        ..
                    }
                ) {
                    warn!(
                        "Failed to delete security group {} during cleanup: {:?}",
                        sg_id, e
                    );
                }
            }
        }
    }

    fn get_test_suffix(&self) -> String {
        Uuid::new_v4().simple().to_string()[..8].to_string()
    }

    async fn wait_for_load_balancer_active(&self, lb_arn: &str) -> bool {
        info!(
            "⏳ Waiting for load balancer {} to become active...",
            lb_arn
        );
        let mut attempts = 0;
        let max_attempts = 60; // 10 minutes max wait

        loop {
            attempts += 1;

            let request = DescribeLoadBalancersRequest::builder()
                .load_balancer_arns(vec![lb_arn.to_string()])
                .build();

            match self.elbv2_client.describe_load_balancers(request).await {
                Ok(response) => {
                    if let Some(lbs) = response.describe_load_balancers_result.load_balancers {
                        if let Some(lb) = lbs.members.first() {
                            if let Some(state) = &lb.state {
                                let state_code = state.code.as_deref().unwrap_or("unknown");
                                info!("📊 Load balancer state: {}", state_code);

                                if state_code == "active" {
                                    info!("✅ Load balancer is active!");
                                    return true;
                                }

                                if state_code == "failed" {
                                    warn!("⚠️ Load balancer entered failed state");
                                    return false;
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    warn!("Failed to get load balancer status: {:?}", e);
                }
            }

            if attempts >= max_attempts {
                warn!("⚠️ Load balancer didn't become active within 10 minutes");
                return false;
            }

            tokio::time::sleep(Duration::from_secs(10)).await;
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

/// Test describing load balancers (read-only, no resources created).
#[test_context(Elbv2TestContext)]
#[tokio::test]
async fn test_describe_load_balancers(ctx: &mut Elbv2TestContext) {
    info!("🔍 Testing describe load balancers...");

    let request = DescribeLoadBalancersRequest::builder().build();

    let response = ctx
        .elbv2_client
        .describe_load_balancers(request)
        .await
        .expect("Failed to describe load balancers");

    let lb_count = response
        .describe_load_balancers_result
        .load_balancers
        .as_ref()
        .map(|l| l.members.len())
        .unwrap_or(0);
    info!("✅ Found {} load balancers in the account", lb_count);
}

/// Test describing target groups (read-only, no resources created).
#[test_context(Elbv2TestContext)]
#[tokio::test]
async fn test_describe_target_groups(ctx: &mut Elbv2TestContext) {
    info!("🔍 Testing describe target groups...");

    let request = DescribeTargetGroupsRequest::builder().build();

    let response = ctx
        .elbv2_client
        .describe_target_groups(request)
        .await
        .expect("Failed to describe target groups");

    let tg_count = response
        .describe_target_groups_result
        .target_groups
        .as_ref()
        .map(|t| t.members.len())
        .unwrap_or(0);
    info!("✅ Found {} target groups in the account", tg_count);
}

/// Test creating and deleting a target group (simpler than full ALB test).
#[test_context(Elbv2TestContext)]
#[tokio::test]
async fn test_target_group_lifecycle(ctx: &mut Elbv2TestContext) {
    let vpc_id = match &ctx.default_vpc_id {
        Some(id) => id.clone(),
        None => {
            warn!("⚠️ No default VPC found, skipping target group test");
            return;
        }
    };

    let suffix = ctx.get_test_suffix();
    let tg_name = format!("alien-test-tg-{}", suffix);

    info!("🚀 Starting target group lifecycle test: {}", tg_name);

    // =========================================================================
    // Step 1: Create target group
    // =========================================================================
    info!("📝 Step 1: Creating target group");
    let create_request = CreateTargetGroupRequest::builder()
        .name(tg_name.clone())
        .protocol("HTTP".to_string())
        .port(80)
        .vpc_id(vpc_id)
        .target_type("instance".to_string())
        .health_check_path("/health".to_string())
        .health_check_interval_seconds(30)
        .health_check_timeout_seconds(5)
        .healthy_threshold_count(2)
        .unhealthy_threshold_count(2)
        .build();

    let create_response = ctx
        .elbv2_client
        .create_target_group(create_request)
        .await
        .expect("Failed to create target group");

    let tg_arn = create_response
        .create_target_group_result
        .target_groups
        .as_ref()
        .and_then(|tgs| tgs.members.first())
        .and_then(|tg| tg.target_group_arn.clone())
        .expect("Target group should have ARN");

    ctx.track_target_group(&tg_arn);
    info!("✅ Target group created: {}", tg_arn);

    // =========================================================================
    // Step 2: Describe target group
    // =========================================================================
    info!("🔍 Step 2: Describing target group");
    let describe_request = DescribeTargetGroupsRequest::builder()
        .target_group_arns(vec![tg_arn.clone()])
        .build();

    let describe_response = ctx
        .elbv2_client
        .describe_target_groups(describe_request)
        .await
        .expect("Failed to describe target group");

    let tgs = describe_response
        .describe_target_groups_result
        .target_groups
        .expect("Should have target groups");
    assert_eq!(tgs.members.len(), 1);
    assert_eq!(
        tgs.members[0].target_group_name.as_deref(),
        Some(tg_name.as_str())
    );
    info!("✅ Target group verified");

    // =========================================================================
    // Step 3: Check target health (empty, no targets registered)
    // =========================================================================
    info!("🩺 Step 3: Checking target health");
    let health_request = DescribeTargetHealthRequest::builder()
        .target_group_arn(tg_arn.clone())
        .build();

    let health_response = ctx
        .elbv2_client
        .describe_target_health(health_request)
        .await
        .expect("Failed to describe target health");

    let targets = health_response
        .describe_target_health_result
        .target_health_descriptions
        .map(|t| t.members)
        .unwrap_or_default();
    assert!(targets.is_empty(), "No targets should be registered");
    info!("✅ Target health check passed (no targets)");

    // =========================================================================
    // Step 4: Delete target group
    // =========================================================================
    info!("🗑️ Step 4: Deleting target group");
    ctx.elbv2_client
        .delete_target_group(&tg_arn)
        .await
        .expect("Failed to delete target group");
    ctx.untrack_target_group(&tg_arn);
    info!("✅ Target group deleted");

    info!("🎉 Target group lifecycle test completed successfully!");
}

/// Comprehensive E2E test for ELBv2 (creates ALB, target group, listener).
/// This test is more expensive as ALBs have hourly charges.
#[test_context(Elbv2TestContext)]
#[tokio::test]
async fn test_elbv2_e2e(ctx: &mut Elbv2TestContext) {
    // Check prerequisites
    let vpc_id = match &ctx.default_vpc_id {
        Some(id) => id.clone(),
        None => {
            warn!("⚠️ No default VPC found, skipping ELBv2 E2E test");
            return;
        }
    };

    if ctx.default_subnet_ids.len() < 2 {
        warn!("⚠️ Need at least 2 subnets in different AZs, skipping ELBv2 E2E test");
        return;
    }

    let suffix = ctx.get_test_suffix();
    let lb_name = format!("alien-test-{}", suffix);
    let tg_name = format!("alien-tg-{}", suffix);

    info!("🚀 Starting ELBv2 E2E test: {}", lb_name);

    // =========================================================================
    // Step 1: Create security group for ALB
    // =========================================================================
    info!("🔒 Step 1: Creating security group for ALB");
    let sg_request = CreateSecurityGroupRequest::builder()
        .group_name(format!("alien-test-sg-{}", suffix))
        .description("Security group for ALB test".to_string())
        .vpc_id(vpc_id.clone())
        .build();

    let sg_response = ctx
        .ec2_client
        .create_security_group(sg_request)
        .await
        .expect("Failed to create security group");

    let sg_id = sg_response.group_id.expect("SG should have ID");
    ctx.track_security_group(&sg_id);
    info!("✅ Security group created: {}", sg_id);

    // Add ingress rule for HTTP
    let ingress_request = AuthorizeSecurityGroupIngressRequest::builder()
        .group_id(sg_id.clone())
        .ip_permissions(vec![IpPermission::builder()
            .ip_protocol("tcp".to_string())
            .from_port(80)
            .to_port(80)
            .ip_ranges(vec![IpRange::builder()
                .cidr_ip("0.0.0.0/0".to_string())
                .build()])
            .build()])
        .build();

    ctx.ec2_client
        .authorize_security_group_ingress(ingress_request)
        .await
        .expect("Failed to add ingress rule");
    info!("✅ Ingress rule added");

    // =========================================================================
    // Step 2: Create target group
    // =========================================================================
    info!("🎯 Step 2: Creating target group");
    let tg_request = CreateTargetGroupRequest::builder()
        .name(tg_name.clone())
        .protocol("HTTP".to_string())
        .port(80)
        .vpc_id(vpc_id.clone())
        .target_type("instance".to_string())
        .health_check_path("/".to_string())
        .build();

    let tg_response = ctx
        .elbv2_client
        .create_target_group(tg_request)
        .await
        .expect("Failed to create target group");

    let tg_arn = tg_response
        .create_target_group_result
        .target_groups
        .as_ref()
        .and_then(|tgs| tgs.members.first())
        .and_then(|tg| tg.target_group_arn.clone())
        .expect("Target group should have ARN");

    ctx.track_target_group(&tg_arn);
    info!("✅ Target group created: {}", tg_arn);

    // =========================================================================
    // Step 3: Create Application Load Balancer
    // =========================================================================
    info!("⚖️ Step 3: Creating Application Load Balancer");
    let lb_request = CreateLoadBalancerRequest::builder()
        .name(lb_name.clone())
        .subnets(ctx.default_subnet_ids.clone())
        .security_groups(vec![sg_id.clone()])
        .scheme("internet-facing".to_string())
        .load_balancer_type("application".to_string())
        .ip_address_type("ipv4".to_string())
        .build();

    let lb_response = ctx
        .elbv2_client
        .create_load_balancer(lb_request)
        .await
        .expect("Failed to create load balancer");

    let lb_arn = lb_response
        .create_load_balancer_result
        .load_balancers
        .as_ref()
        .and_then(|lbs| lbs.members.first())
        .and_then(|lb| lb.load_balancer_arn.clone())
        .expect("Load balancer should have ARN");

    ctx.track_load_balancer(&lb_arn);
    info!("✅ Load balancer created: {}", lb_arn);

    // Wait for ALB to become active
    let is_active = ctx.wait_for_load_balancer_active(&lb_arn).await;
    if !is_active {
        warn!("⚠️ Load balancer didn't become active, continuing with test anyway");
    }

    // =========================================================================
    // Step 4: Create listener
    // =========================================================================
    info!("👂 Step 4: Creating listener");
    let listener_request = CreateListenerRequest::builder()
        .load_balancer_arn(lb_arn.clone())
        .port(80)
        .protocol("HTTP".to_string())
        .default_actions(vec![Action::builder()
            .action_type("forward".to_string())
            .target_group_arn(tg_arn.clone())
            .build()])
        .build();

    let listener_response = ctx
        .elbv2_client
        .create_listener(listener_request)
        .await
        .expect("Failed to create listener");

    let listener_arn = listener_response
        .create_listener_result
        .listeners
        .as_ref()
        .and_then(|l| l.members.first())
        .and_then(|l| l.listener_arn.clone())
        .expect("Listener should have ARN");

    ctx.track_listener(&listener_arn);
    info!("✅ Listener created: {}", listener_arn);

    // =========================================================================
    // Step 5: Describe listeners
    // =========================================================================
    info!("🔍 Step 5: Describing listeners");
    let describe_listeners_request = DescribeListenersRequest::builder()
        .load_balancer_arn(lb_arn.clone())
        .build();

    let describe_listeners_response = ctx
        .elbv2_client
        .describe_listeners(describe_listeners_request)
        .await
        .expect("Failed to describe listeners");

    let listeners = describe_listeners_response
        .describe_listeners_result
        .listeners
        .expect("Should have listeners");
    assert_eq!(listeners.members.len(), 1);
    assert_eq!(listeners.members[0].port, Some(80));
    info!("✅ Listener verified");

    // =========================================================================
    // Step 6: Verify load balancer
    // =========================================================================
    info!("🔍 Step 6: Verifying load balancer");
    let describe_lb_request = DescribeLoadBalancersRequest::builder()
        .load_balancer_arns(vec![lb_arn.clone()])
        .build();

    let describe_lb_response = ctx
        .elbv2_client
        .describe_load_balancers(describe_lb_request)
        .await
        .expect("Failed to describe load balancer");

    let lbs = describe_lb_response
        .describe_load_balancers_result
        .load_balancers
        .expect("Should have LBs");
    assert_eq!(lbs.members.len(), 1);

    let lb = &lbs.members[0];
    assert_eq!(lb.load_balancer_name.as_deref(), Some(lb_name.as_str()));
    assert_eq!(lb.scheme.as_deref(), Some("internet-facing"));
    assert_eq!(lb.load_balancer_type.as_deref(), Some("application"));

    if let Some(dns_name) = &lb.dns_name {
        info!("✅ Load balancer DNS: {}", dns_name);
    }

    // =========================================================================
    // Step 7: Delete listener
    // =========================================================================
    info!("🗑️ Step 7: Deleting listener");
    ctx.elbv2_client
        .delete_listener(&listener_arn)
        .await
        .expect("Failed to delete listener");

    let mut listeners = ctx.created_listeners.lock().unwrap();
    listeners.remove(&listener_arn);
    drop(listeners);
    info!("✅ Listener deleted");

    // =========================================================================
    // Step 8: Delete load balancer
    // =========================================================================
    info!("🗑️ Step 8: Deleting load balancer");
    ctx.elbv2_client
        .delete_load_balancer(&lb_arn)
        .await
        .expect("Failed to delete load balancer");
    ctx.untrack_load_balancer(&lb_arn);
    info!("✅ Load balancer deletion initiated");

    // Wait for LB to be deleted before deleting target group
    info!("⏳ Waiting for load balancer to be deleted...");
    tokio::time::sleep(Duration::from_secs(30)).await;

    // =========================================================================
    // Step 9: Delete target group
    // =========================================================================
    info!("🗑️ Step 9: Deleting target group");
    ctx.elbv2_client
        .delete_target_group(&tg_arn)
        .await
        .expect("Failed to delete target group");
    ctx.untrack_target_group(&tg_arn);
    info!("✅ Target group deleted");

    info!("🎉 ELBv2 E2E test completed successfully!");
    info!("📊 Summary:");
    info!("   - Load Balancer: {}", lb_name);
    info!("   - Target Group: {}", tg_name);
    info!("   - Security Group: {}", sg_id);
}

/// Test ELBv2 client with invalid credentials.
#[test_context(Elbv2TestContext)]
#[tokio::test]
async fn test_elbv2_client_with_invalid_credentials(_ctx: &mut Elbv2TestContext) {
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
    let invalid_client = Elbv2Client::new(
        Client::new(),
        AwsCredentialProvider::from_config_sync(invalid_config),
    );

    info!("🔐 Testing ELBv2 client with invalid credentials");

    let request = DescribeLoadBalancersRequest::builder().build();

    let result = invalid_client.describe_load_balancers(request).await;

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

/// Test error handling for non-existent load balancer.
#[test_context(Elbv2TestContext)]
#[tokio::test]
async fn test_describe_non_existent_load_balancer(ctx: &mut Elbv2TestContext) {
    let non_existent_arn =
        "arn:aws:elasticloadbalancing:us-west-2:123456789012:loadbalancer/app/non-existent/1234567890123456";

    info!(
        "❌ Testing describe non-existent load balancer: {}",
        non_existent_arn
    );

    let request = DescribeLoadBalancersRequest::builder()
        .load_balancer_arns(vec![non_existent_arn.to_string()])
        .build();

    let result = ctx.elbv2_client.describe_load_balancers(request).await;

    assert!(result.is_err());
    match result.unwrap_err() {
        Error {
            error: Some(ErrorData::RemoteResourceNotFound { .. }),
            ..
        } => {
            info!("✅ Correctly detected non-existent load balancer");
        }
        other => {
            warn!(
                "Got unexpected error for non-existent load balancer: {:?}",
                other
            );
        }
    }
}
