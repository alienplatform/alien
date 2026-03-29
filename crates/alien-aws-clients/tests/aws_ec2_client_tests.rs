/*!
# EC2 Client Integration Tests

These tests perform real AWS EC2 VPC networking operations including creating VPCs, subnets,
internet gateways, NAT gateways, route tables, and security groups.

## Prerequisites

### 1. AWS Credentials
Set up `.env.test` in the workspace root with:
```
AWS_MANAGEMENT_REGION=eu-central-1
AWS_MANAGEMENT_ACCESS_KEY_ID=your_access_key
AWS_MANAGEMENT_SECRET_ACCESS_KEY=your_secret_key
AWS_MANAGEMENT_ACCOUNT_ID=your_account_id
```

### 2. Required Permissions
Your AWS credentials need these permissions:
- `ec2:CreateVpc`, `ec2:DeleteVpc`, `ec2:DescribeVpcs`, `ec2:DescribeVpcAttribute`, `ec2:ModifyVpcAttribute`
- `ec2:CreateSubnet`, `ec2:DeleteSubnet`, `ec2:DescribeSubnets`
- `ec2:CreateInternetGateway`, `ec2:DeleteInternetGateway`, `ec2:AttachInternetGateway`, `ec2:DetachInternetGateway`, `ec2:DescribeInternetGateways`
- `ec2:CreateNatGateway`, `ec2:DeleteNatGateway`, `ec2:DescribeNatGateways`
- `ec2:AllocateAddress`, `ec2:ReleaseAddress`
- `ec2:CreateRouteTable`, `ec2:DeleteRouteTable`, `ec2:CreateRoute`, `ec2:DeleteRoute`, `ec2:AssociateRouteTable`, `ec2:DisassociateRouteTable`, `ec2:DescribeRouteTables`
- `ec2:CreateSecurityGroup`, `ec2:DeleteSecurityGroup`, `ec2:AuthorizeSecurityGroupIngress`, `ec2:AuthorizeSecurityGroupEgress`, `ec2:RevokeSecurityGroupIngress`, `ec2:RevokeSecurityGroupEgress`, `ec2:DescribeSecurityGroups`
- `ec2:DescribeAvailabilityZones`
- `ec2:CreateTags`

## Running Tests
```bash
# Run all EC2 tests
cargo test --package alien-aws-clients --test aws_ec2_client_tests

# Run specific test
cargo test --package alien-aws-clients --test aws_ec2_client_tests test_vpc_networking_e2e -- --nocapture
```

## Notes

- This test creates a complete VPC networking setup and tears it down afterward
- The test takes several minutes to complete due to NAT gateway creation/deletion
- NAT gateways have an hourly cost, but are deleted immediately after testing
*/

use alien_aws_clients::ec2::*;
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

/// Test context that manages AWS resources created during tests.
struct Ec2TestContext {
    client: Ec2Client,
    /// Track VPCs for cleanup
    created_vpcs: Mutex<HashSet<String>>,
    /// Track subnets for cleanup
    created_subnets: Mutex<HashSet<String>>,
    /// Track internet gateways for cleanup
    created_internet_gateways: Mutex<HashSet<String>>,
    /// Track NAT gateways for cleanup
    created_nat_gateways: Mutex<HashSet<String>>,
    /// Track elastic IPs for cleanup
    created_elastic_ips: Mutex<HashSet<String>>,
    /// Track route tables for cleanup
    created_route_tables: Mutex<HashSet<String>>,
    /// Track security groups for cleanup
    created_security_groups: Mutex<HashSet<String>>,
    /// Track route table associations for cleanup
    created_route_table_associations: Mutex<HashSet<String>>,
    /// Track internet gateway attachments for cleanup (igw_id, vpc_id)
    igw_attachments: Mutex<Vec<(String, String)>>,
}

impl AsyncTestContext for Ec2TestContext {
    async fn setup() -> Ec2TestContext {
        let root: StdPathBuf = workspace_root::get_workspace_root();
        dotenvy::from_path(root.join(".env.test")).expect("Failed to load .env.test");
        tracing_subscriber::fmt::try_init().ok();

        // Use us-west-2 specifically for EC2 tests to avoid VPC quota limits in other regions
        let region = "us-west-2".to_string();
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
        let client = Ec2Client::new(
            Client::new(),
            AwsCredentialProvider::from_config_sync(aws_config),
        );

        Ec2TestContext {
            client,
            created_vpcs: Mutex::new(HashSet::new()),
            created_subnets: Mutex::new(HashSet::new()),
            created_internet_gateways: Mutex::new(HashSet::new()),
            created_nat_gateways: Mutex::new(HashSet::new()),
            created_elastic_ips: Mutex::new(HashSet::new()),
            created_route_tables: Mutex::new(HashSet::new()),
            created_security_groups: Mutex::new(HashSet::new()),
            created_route_table_associations: Mutex::new(HashSet::new()),
            igw_attachments: Mutex::new(Vec::new()),
        }
    }

    async fn teardown(self) {
        info!("🧹 Starting EC2 test cleanup...");

        // Cleanup order is important due to dependencies:
        // 1. Route table associations
        // 2. Routes (implicit in route table deletion)
        // 3. Route tables
        // 4. NAT gateways (wait for deletion)
        // 5. Elastic IPs
        // 6. Security groups
        // 7. Internet gateway detachment
        // 8. Internet gateways
        // 9. Subnets
        // 10. VPCs

        // 1. Disassociate route tables
        let associations_to_cleanup = {
            let associations = self.created_route_table_associations.lock().unwrap();
            associations.clone()
        };
        for association_id in associations_to_cleanup {
            self.cleanup_route_table_association(&association_id).await;
        }

        // 2. Delete route tables (non-main ones)
        let route_tables_to_cleanup = {
            let route_tables = self.created_route_tables.lock().unwrap();
            route_tables.clone()
        };
        for rt_id in route_tables_to_cleanup {
            self.cleanup_route_table(&rt_id).await;
        }

        // 3. Delete NAT gateways (and wait for deletion)
        let nat_gateways_to_cleanup = {
            let nat_gateways = self.created_nat_gateways.lock().unwrap();
            nat_gateways.clone()
        };
        for nat_id in &nat_gateways_to_cleanup {
            self.cleanup_nat_gateway(nat_id).await;
        }

        // Wait for NAT gateways to be deleted before releasing EIPs
        if !nat_gateways_to_cleanup.is_empty() {
            info!("⏳ Waiting for NAT gateways to be fully deleted...");
            tokio::time::sleep(Duration::from_secs(60)).await;
        }

        // 4. Release elastic IPs
        let eips_to_cleanup = {
            let eips = self.created_elastic_ips.lock().unwrap();
            eips.clone()
        };
        for allocation_id in eips_to_cleanup {
            self.cleanup_elastic_ip(&allocation_id).await;
        }

        // 5. Delete security groups
        let security_groups_to_cleanup = {
            let security_groups = self.created_security_groups.lock().unwrap();
            security_groups.clone()
        };
        for sg_id in security_groups_to_cleanup {
            self.cleanup_security_group(&sg_id).await;
        }

        // 6. Detach internet gateways
        let igw_attachments_to_cleanup = {
            let attachments = self.igw_attachments.lock().unwrap();
            attachments.clone()
        };
        for (igw_id, vpc_id) in igw_attachments_to_cleanup {
            self.cleanup_igw_attachment(&igw_id, &vpc_id).await;
        }

        // 7. Delete internet gateways
        let igws_to_cleanup = {
            let igws = self.created_internet_gateways.lock().unwrap();
            igws.clone()
        };
        for igw_id in igws_to_cleanup {
            self.cleanup_internet_gateway(&igw_id).await;
        }

        // 8. Delete subnets
        let subnets_to_cleanup = {
            let subnets = self.created_subnets.lock().unwrap();
            subnets.clone()
        };
        for subnet_id in subnets_to_cleanup {
            self.cleanup_subnet(&subnet_id).await;
        }

        // 9. Delete VPCs
        let vpcs_to_cleanup = {
            let vpcs = self.created_vpcs.lock().unwrap();
            vpcs.clone()
        };
        for vpc_id in vpcs_to_cleanup {
            self.cleanup_vpc(&vpc_id).await;
        }

        info!("✅ EC2 test cleanup completed");
    }
}

impl Ec2TestContext {
    // ----- Tracking helpers -----

    fn track_vpc(&self, vpc_id: &str) {
        let mut vpcs = self.created_vpcs.lock().unwrap();
        vpcs.insert(vpc_id.to_string());
        info!("📝 Tracking VPC for cleanup: {}", vpc_id);
    }

    fn track_subnet(&self, subnet_id: &str) {
        let mut subnets = self.created_subnets.lock().unwrap();
        subnets.insert(subnet_id.to_string());
        info!("📝 Tracking subnet for cleanup: {}", subnet_id);
    }

    fn track_internet_gateway(&self, igw_id: &str) {
        let mut igws = self.created_internet_gateways.lock().unwrap();
        igws.insert(igw_id.to_string());
        info!("📝 Tracking internet gateway for cleanup: {}", igw_id);
    }

    fn track_igw_attachment(&self, igw_id: &str, vpc_id: &str) {
        let mut attachments = self.igw_attachments.lock().unwrap();
        attachments.push((igw_id.to_string(), vpc_id.to_string()));
        info!(
            "📝 Tracking IGW attachment for cleanup: {} -> {}",
            igw_id, vpc_id
        );
    }

    fn track_nat_gateway(&self, nat_id: &str) {
        let mut nat_gateways = self.created_nat_gateways.lock().unwrap();
        nat_gateways.insert(nat_id.to_string());
        info!("📝 Tracking NAT gateway for cleanup: {}", nat_id);
    }

    fn track_elastic_ip(&self, allocation_id: &str) {
        let mut eips = self.created_elastic_ips.lock().unwrap();
        eips.insert(allocation_id.to_string());
        info!("📝 Tracking Elastic IP for cleanup: {}", allocation_id);
    }

    fn track_route_table(&self, rt_id: &str) {
        let mut route_tables = self.created_route_tables.lock().unwrap();
        route_tables.insert(rt_id.to_string());
        info!("📝 Tracking route table for cleanup: {}", rt_id);
    }

    fn track_security_group(&self, sg_id: &str) {
        let mut security_groups = self.created_security_groups.lock().unwrap();
        security_groups.insert(sg_id.to_string());
        info!("📝 Tracking security group for cleanup: {}", sg_id);
    }

    fn track_route_table_association(&self, association_id: &str) {
        let mut associations = self.created_route_table_associations.lock().unwrap();
        associations.insert(association_id.to_string());
        info!(
            "📝 Tracking route table association for cleanup: {}",
            association_id
        );
    }

    // ----- Cleanup helpers -----

    async fn cleanup_route_table_association(&self, association_id: &str) {
        info!("🧹 Disassociating route table: {}", association_id);
        match self.client.disassociate_route_table(association_id).await {
            Ok(_) => info!(
                "✅ Route table association {} disassociated",
                association_id
            ),
            Err(e) => {
                if !matches!(
                    e,
                    Error {
                        error: Some(ErrorData::RemoteResourceNotFound { .. }),
                        ..
                    }
                ) {
                    warn!(
                        "Failed to disassociate route table {}: {:?}",
                        association_id, e
                    );
                }
            }
        }
    }

    async fn cleanup_route_table(&self, rt_id: &str) {
        info!("🧹 Deleting route table: {}", rt_id);
        match self.client.delete_route_table(rt_id).await {
            Ok(_) => info!("✅ Route table {} deleted", rt_id),
            Err(e) => {
                if !matches!(
                    e,
                    Error {
                        error: Some(ErrorData::RemoteResourceNotFound { .. }),
                        ..
                    }
                ) {
                    warn!("Failed to delete route table {}: {:?}", rt_id, e);
                }
            }
        }
    }

    async fn cleanup_nat_gateway(&self, nat_id: &str) {
        info!("🧹 Deleting NAT gateway: {}", nat_id);
        match self.client.delete_nat_gateway(nat_id).await {
            Ok(_) => info!("✅ NAT gateway {} deletion initiated", nat_id),
            Err(e) => {
                if !matches!(
                    e,
                    Error {
                        error: Some(ErrorData::RemoteResourceNotFound { .. }),
                        ..
                    }
                ) {
                    warn!("Failed to delete NAT gateway {}: {:?}", nat_id, e);
                }
            }
        }
    }

    async fn cleanup_elastic_ip(&self, allocation_id: &str) {
        info!("🧹 Releasing Elastic IP: {}", allocation_id);
        match self.client.release_address(allocation_id).await {
            Ok(_) => info!("✅ Elastic IP {} released", allocation_id),
            Err(e) => {
                if !matches!(
                    e,
                    Error {
                        error: Some(ErrorData::RemoteResourceNotFound { .. }),
                        ..
                    }
                ) {
                    warn!("Failed to release Elastic IP {}: {:?}", allocation_id, e);
                }
            }
        }
    }

    async fn cleanup_security_group(&self, sg_id: &str) {
        info!("🧹 Deleting security group: {}", sg_id);
        match self.client.delete_security_group(sg_id).await {
            Ok(_) => info!("✅ Security group {} deleted", sg_id),
            Err(e) => {
                if !matches!(
                    e,
                    Error {
                        error: Some(ErrorData::RemoteResourceNotFound { .. }),
                        ..
                    }
                ) {
                    warn!("Failed to delete security group {}: {:?}", sg_id, e);
                }
            }
        }
    }

    async fn cleanup_igw_attachment(&self, igw_id: &str, vpc_id: &str) {
        info!(
            "🧹 Detaching internet gateway {} from VPC {}",
            igw_id, vpc_id
        );
        let request = DetachInternetGatewayRequest::builder()
            .internet_gateway_id(igw_id.to_string())
            .vpc_id(vpc_id.to_string())
            .build();
        match self.client.detach_internet_gateway(request).await {
            Ok(_) => info!(
                "✅ Internet gateway {} detached from VPC {}",
                igw_id, vpc_id
            ),
            Err(e) => {
                if !matches!(
                    e,
                    Error {
                        error: Some(ErrorData::RemoteResourceNotFound { .. }),
                        ..
                    }
                ) {
                    warn!(
                        "Failed to detach IGW {} from VPC {}: {:?}",
                        igw_id, vpc_id, e
                    );
                }
            }
        }
    }

    async fn cleanup_internet_gateway(&self, igw_id: &str) {
        info!("🧹 Deleting internet gateway: {}", igw_id);
        match self.client.delete_internet_gateway(igw_id).await {
            Ok(_) => info!("✅ Internet gateway {} deleted", igw_id),
            Err(e) => {
                if !matches!(
                    e,
                    Error {
                        error: Some(ErrorData::RemoteResourceNotFound { .. }),
                        ..
                    }
                ) {
                    warn!("Failed to delete internet gateway {}: {:?}", igw_id, e);
                }
            }
        }
    }

    async fn cleanup_subnet(&self, subnet_id: &str) {
        info!("🧹 Deleting subnet: {}", subnet_id);
        match self.client.delete_subnet(subnet_id).await {
            Ok(_) => info!("✅ Subnet {} deleted", subnet_id),
            Err(e) => {
                if !matches!(
                    e,
                    Error {
                        error: Some(ErrorData::RemoteResourceNotFound { .. }),
                        ..
                    }
                ) {
                    warn!("Failed to delete subnet {}: {:?}", subnet_id, e);
                }
            }
        }
    }

    async fn cleanup_vpc(&self, vpc_id: &str) {
        info!("🧹 Deleting VPC: {}", vpc_id);
        match self.client.delete_vpc(vpc_id).await {
            Ok(_) => info!("✅ VPC {} deleted", vpc_id),
            Err(e) => {
                if !matches!(
                    e,
                    Error {
                        error: Some(ErrorData::RemoteResourceNotFound { .. }),
                        ..
                    }
                ) {
                    warn!("Failed to delete VPC {}: {:?}", vpc_id, e);
                }
            }
        }
    }

    // ----- Helper methods -----

    fn get_test_name_suffix(&self) -> String {
        Uuid::new_v4().simple().to_string()[..8].to_string()
    }

    /// Wait for a NAT gateway to become available.
    async fn wait_for_nat_gateway_available(&self, nat_gateway_id: &str) -> bool {
        info!(
            "⏳ Waiting for NAT gateway {} to become available...",
            nat_gateway_id
        );
        let mut attempts = 0;
        let max_attempts = 60; // 10 minutes max wait

        loop {
            attempts += 1;

            let request = DescribeNatGatewaysRequest::builder()
                .nat_gateway_ids(vec![nat_gateway_id.to_string()])
                .build();

            match self.client.describe_nat_gateways(request).await {
                Ok(response) => {
                    if let Some(nat_gateway_set) = response.nat_gateway_set {
                        if let Some(nat_gateway) = nat_gateway_set.items.first() {
                            let state = nat_gateway.state.as_deref().unwrap_or("unknown");
                            info!("📊 NAT gateway state: {}", state);

                            if state == "available" {
                                info!("✅ NAT gateway is available!");
                                return true;
                            }

                            if state == "failed" || state == "deleted" {
                                warn!("⚠️ NAT gateway entered {} state", state);
                                return false;
                            }
                        }
                    }
                }
                Err(e) => {
                    warn!("Failed to get NAT gateway status: {:?}", e);
                }
            }

            if attempts >= max_attempts {
                warn!("⚠️ NAT gateway didn't become available within 10 minutes");
                return false;
            }

            tokio::time::sleep(Duration::from_secs(10)).await;
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

/// Comprehensive end-to-end test that creates a full VPC networking setup.
///
/// This test creates:
/// - A VPC with DNS support enabled
/// - Public and private subnets in different AZs
/// - An internet gateway attached to the VPC
/// - An Elastic IP for the NAT gateway
/// - A NAT gateway in the public subnet
/// - Public and private route tables with appropriate routes
/// - A security group with ingress/egress rules
#[test_context(Ec2TestContext)]
#[tokio::test]
async fn test_vpc_networking_e2e(ctx: &mut Ec2TestContext) {
    let test_suffix = ctx.get_test_name_suffix();
    info!(
        "🚀 Starting VPC networking E2E test (suffix: {})",
        test_suffix
    );

    // =========================================================================
    // Step 1: Describe availability zones
    // =========================================================================
    info!("📍 Step 1: Getting availability zones...");
    let az_response = ctx
        .client
        .describe_availability_zones(DescribeAvailabilityZonesRequest::builder().build())
        .await
        .expect("Failed to describe availability zones");

    let azs: Vec<String> = az_response
        .availability_zone_info
        .as_ref()
        .map(|set| {
            set.items
                .iter()
                .filter_map(|az| az.zone_name.clone())
                .filter(|name| !name.contains("local")) // Filter out local zones
                .take(2)
                .collect()
        })
        .unwrap_or_default();

    assert!(azs.len() >= 2, "Need at least 2 availability zones");
    info!("✅ Found {} AZs: {:?}", azs.len(), azs);

    // =========================================================================
    // Step 2: Create VPC
    // =========================================================================
    info!("🏗️ Step 2: Creating VPC...");
    let vpc_cidr = "10.0.0.0/16";
    let create_vpc_request = CreateVpcRequest::builder()
        .cidr_block(vpc_cidr.to_string())
        .tag_specifications(vec![TagSpecification::builder()
            .resource_type("vpc".to_string())
            .tags(vec![
                Tag::builder()
                    .key("Name".to_string())
                    .value(format!("alien-test-vpc-{}", test_suffix))
                    .build(),
                Tag::builder()
                    .key("Environment".to_string())
                    .value("Test".to_string())
                    .build(),
            ])
            .build()])
        .build();

    let vpc_response = ctx
        .client
        .create_vpc(create_vpc_request)
        .await
        .expect("Failed to create VPC");

    let vpc_id = vpc_response
        .vpc
        .as_ref()
        .and_then(|vpc| vpc.vpc_id.clone())
        .expect("VPC should have an ID");

    ctx.track_vpc(&vpc_id);
    info!("✅ Created VPC: {}", vpc_id);

    // =========================================================================
    // Step 3: Enable DNS support and hostnames
    // =========================================================================
    info!("🔧 Step 3: Enabling DNS support and hostnames...");

    // Enable DNS support
    ctx.client
        .modify_vpc_attribute(
            ModifyVpcAttributeRequest::builder()
                .vpc_id(vpc_id.clone())
                .enable_dns_support(true)
                .build(),
        )
        .await
        .expect("Failed to enable DNS support");

    // Enable DNS hostnames
    ctx.client
        .modify_vpc_attribute(
            ModifyVpcAttributeRequest::builder()
                .vpc_id(vpc_id.clone())
                .enable_dns_hostnames(true)
                .build(),
        )
        .await
        .expect("Failed to enable DNS hostnames");

    info!("✅ DNS support and hostnames enabled");

    // Verify DNS attributes
    let dns_support_response = ctx
        .client
        .describe_vpc_attribute(
            DescribeVpcAttributeRequest::builder()
                .vpc_id(vpc_id.clone())
                .attribute("enableDnsSupport".to_string())
                .build(),
        )
        .await
        .expect("Failed to describe VPC DNS support attribute");

    assert!(
        dns_support_response
            .enable_dns_support
            .and_then(|v| v.value)
            .unwrap_or(false),
        "DNS support should be enabled"
    );

    info!("✅ Verified DNS support is enabled");

    // =========================================================================
    // Step 4: Create subnets
    // =========================================================================
    info!("🏗️ Step 4: Creating subnets...");

    // Create public subnet 1
    let public_subnet1_request = CreateSubnetRequest::builder()
        .vpc_id(vpc_id.clone())
        .cidr_block("10.0.1.0/24".to_string())
        .availability_zone(azs[0].clone())
        .tag_specifications(vec![TagSpecification::builder()
            .resource_type("subnet".to_string())
            .tags(vec![Tag::builder()
                .key("Name".to_string())
                .value(format!("alien-test-public-1-{}", test_suffix))
                .build()])
            .build()])
        .build();

    let public_subnet1_response = ctx
        .client
        .create_subnet(public_subnet1_request)
        .await
        .expect("Failed to create public subnet 1");
    let public_subnet1_id = public_subnet1_response
        .subnet
        .as_ref()
        .and_then(|s| s.subnet_id.clone())
        .expect("Subnet should have an ID");
    ctx.track_subnet(&public_subnet1_id);
    info!("✅ Created public subnet 1: {}", public_subnet1_id);

    // Create public subnet 2
    let public_subnet2_request = CreateSubnetRequest::builder()
        .vpc_id(vpc_id.clone())
        .cidr_block("10.0.2.0/24".to_string())
        .availability_zone(azs[1].clone())
        .tag_specifications(vec![TagSpecification::builder()
            .resource_type("subnet".to_string())
            .tags(vec![Tag::builder()
                .key("Name".to_string())
                .value(format!("alien-test-public-2-{}", test_suffix))
                .build()])
            .build()])
        .build();

    let public_subnet2_response = ctx
        .client
        .create_subnet(public_subnet2_request)
        .await
        .expect("Failed to create public subnet 2");
    let public_subnet2_id = public_subnet2_response
        .subnet
        .as_ref()
        .and_then(|s| s.subnet_id.clone())
        .expect("Subnet should have an ID");
    ctx.track_subnet(&public_subnet2_id);
    info!("✅ Created public subnet 2: {}", public_subnet2_id);

    // Create private subnet 1
    let private_subnet1_request = CreateSubnetRequest::builder()
        .vpc_id(vpc_id.clone())
        .cidr_block("10.0.10.0/24".to_string())
        .availability_zone(azs[0].clone())
        .tag_specifications(vec![TagSpecification::builder()
            .resource_type("subnet".to_string())
            .tags(vec![Tag::builder()
                .key("Name".to_string())
                .value(format!("alien-test-private-1-{}", test_suffix))
                .build()])
            .build()])
        .build();

    let private_subnet1_response = ctx
        .client
        .create_subnet(private_subnet1_request)
        .await
        .expect("Failed to create private subnet 1");
    let private_subnet1_id = private_subnet1_response
        .subnet
        .as_ref()
        .and_then(|s| s.subnet_id.clone())
        .expect("Subnet should have an ID");
    ctx.track_subnet(&private_subnet1_id);
    info!("✅ Created private subnet 1: {}", private_subnet1_id);

    // =========================================================================
    // Step 5: Verify subnets
    // =========================================================================
    info!("🔍 Step 5: Verifying subnets...");
    let describe_subnets_response = ctx
        .client
        .describe_subnets(
            DescribeSubnetsRequest::builder()
                .subnet_ids(vec![
                    public_subnet1_id.clone(),
                    public_subnet2_id.clone(),
                    private_subnet1_id.clone(),
                ])
                .build(),
        )
        .await
        .expect("Failed to describe subnets");

    let subnets_count = describe_subnets_response
        .subnet_set
        .as_ref()
        .map(|set| set.items.len())
        .unwrap_or(0);
    assert_eq!(subnets_count, 3, "Should have 3 subnets");
    info!("✅ Verified {} subnets exist", subnets_count);

    // =========================================================================
    // Step 6: Create Internet Gateway
    // =========================================================================
    info!("🌐 Step 6: Creating Internet Gateway...");
    let igw_request = CreateInternetGatewayRequest::builder()
        .tag_specifications(vec![TagSpecification::builder()
            .resource_type("internet-gateway".to_string())
            .tags(vec![Tag::builder()
                .key("Name".to_string())
                .value(format!("alien-test-igw-{}", test_suffix))
                .build()])
            .build()])
        .build();

    let igw_response = ctx
        .client
        .create_internet_gateway(igw_request)
        .await
        .expect("Failed to create internet gateway");
    let igw_id = igw_response
        .internet_gateway
        .as_ref()
        .and_then(|igw| igw.internet_gateway_id.clone())
        .expect("IGW should have an ID");
    ctx.track_internet_gateway(&igw_id);
    info!("✅ Created Internet Gateway: {}", igw_id);

    // Attach IGW to VPC
    info!("🔗 Attaching Internet Gateway to VPC...");
    ctx.client
        .attach_internet_gateway(
            AttachInternetGatewayRequest::builder()
                .internet_gateway_id(igw_id.clone())
                .vpc_id(vpc_id.clone())
                .build(),
        )
        .await
        .expect("Failed to attach internet gateway");
    ctx.track_igw_attachment(&igw_id, &vpc_id);
    info!("✅ Internet Gateway attached to VPC");

    // =========================================================================
    // Step 7: Verify Internet Gateway
    // =========================================================================
    info!("🔍 Step 7: Verifying Internet Gateway...");
    let describe_igw_response = ctx
        .client
        .describe_internet_gateways(
            DescribeInternetGatewaysRequest::builder()
                .internet_gateway_ids(vec![igw_id.clone()])
                .build(),
        )
        .await
        .expect("Failed to describe internet gateways");

    let igw_attached = describe_igw_response
        .internet_gateway_set
        .as_ref()
        .and_then(|set| set.items.first())
        .and_then(|igw| igw.attachment_set.as_ref())
        .map(|attachments| {
            attachments
                .items
                .iter()
                .any(|a| a.vpc_id.as_deref() == Some(&vpc_id))
        })
        .unwrap_or(false);
    assert!(igw_attached, "IGW should be attached to VPC");
    info!("✅ Verified Internet Gateway is attached");

    // =========================================================================
    // Step 8: Create Elastic IP for NAT Gateway
    // =========================================================================
    info!("💰 Step 8: Allocating Elastic IP...");
    let eip_request = AllocateAddressRequest::builder()
        .domain("vpc".to_string())
        .tag_specifications(vec![TagSpecification::builder()
            .resource_type("elastic-ip".to_string())
            .tags(vec![Tag::builder()
                .key("Name".to_string())
                .value(format!("alien-test-eip-{}", test_suffix))
                .build()])
            .build()])
        .build();

    let eip_response = ctx
        .client
        .allocate_address(eip_request)
        .await
        .expect("Failed to allocate Elastic IP");
    let allocation_id = eip_response
        .allocation_id
        .as_ref()
        .expect("EIP should have allocation ID")
        .clone();
    let public_ip = eip_response
        .public_ip
        .as_ref()
        .expect("EIP should have public IP");
    ctx.track_elastic_ip(&allocation_id);
    info!("✅ Allocated Elastic IP: {} ({})", allocation_id, public_ip);

    // =========================================================================
    // Step 9: Create NAT Gateway
    // =========================================================================
    info!("🚪 Step 9: Creating NAT Gateway...");
    let nat_request = CreateNatGatewayRequest::builder()
        .subnet_id(public_subnet1_id.clone())
        .allocation_id(allocation_id.clone())
        .connectivity_type("public".to_string())
        .tag_specifications(vec![TagSpecification::builder()
            .resource_type("natgateway".to_string())
            .tags(vec![Tag::builder()
                .key("Name".to_string())
                .value(format!("alien-test-nat-{}", test_suffix))
                .build()])
            .build()])
        .build();

    let nat_response = ctx
        .client
        .create_nat_gateway(nat_request)
        .await
        .expect("Failed to create NAT gateway");
    let nat_gateway_id = nat_response
        .nat_gateway
        .as_ref()
        .and_then(|nat| nat.nat_gateway_id.clone())
        .expect("NAT gateway should have an ID");
    ctx.track_nat_gateway(&nat_gateway_id);
    info!("✅ Created NAT Gateway: {}", nat_gateway_id);

    // Wait for NAT gateway to become available
    let nat_available = ctx.wait_for_nat_gateway_available(&nat_gateway_id).await;
    assert!(nat_available, "NAT gateway should become available");

    // =========================================================================
    // Step 10: Create Route Tables
    // =========================================================================
    info!("🛣️ Step 10: Creating Route Tables...");

    // Create public route table
    let public_rt_request = CreateRouteTableRequest::builder()
        .vpc_id(vpc_id.clone())
        .tag_specifications(vec![TagSpecification::builder()
            .resource_type("route-table".to_string())
            .tags(vec![Tag::builder()
                .key("Name".to_string())
                .value(format!("alien-test-public-rt-{}", test_suffix))
                .build()])
            .build()])
        .build();

    let public_rt_response = ctx
        .client
        .create_route_table(public_rt_request)
        .await
        .expect("Failed to create public route table");
    let public_rt_id = public_rt_response
        .route_table
        .as_ref()
        .and_then(|rt| rt.route_table_id.clone())
        .expect("Route table should have an ID");
    ctx.track_route_table(&public_rt_id);
    info!("✅ Created public route table: {}", public_rt_id);

    // Create private route table
    let private_rt_request = CreateRouteTableRequest::builder()
        .vpc_id(vpc_id.clone())
        .tag_specifications(vec![TagSpecification::builder()
            .resource_type("route-table".to_string())
            .tags(vec![Tag::builder()
                .key("Name".to_string())
                .value(format!("alien-test-private-rt-{}", test_suffix))
                .build()])
            .build()])
        .build();

    let private_rt_response = ctx
        .client
        .create_route_table(private_rt_request)
        .await
        .expect("Failed to create private route table");
    let private_rt_id = private_rt_response
        .route_table
        .as_ref()
        .and_then(|rt| rt.route_table_id.clone())
        .expect("Route table should have an ID");
    ctx.track_route_table(&private_rt_id);
    info!("✅ Created private route table: {}", private_rt_id);

    // =========================================================================
    // Step 11: Create Routes
    // =========================================================================
    info!("🛤️ Step 11: Creating Routes...");

    // Add route to internet gateway in public route table
    ctx.client
        .create_route(
            CreateRouteRequest::builder()
                .route_table_id(public_rt_id.clone())
                .destination_cidr_block("0.0.0.0/0".to_string())
                .gateway_id(igw_id.clone())
                .build(),
        )
        .await
        .expect("Failed to create public route to IGW");
    info!("✅ Created route 0.0.0.0/0 -> IGW in public route table");

    // Add route to NAT gateway in private route table
    ctx.client
        .create_route(
            CreateRouteRequest::builder()
                .route_table_id(private_rt_id.clone())
                .destination_cidr_block("0.0.0.0/0".to_string())
                .nat_gateway_id(nat_gateway_id.clone())
                .build(),
        )
        .await
        .expect("Failed to create private route to NAT");
    info!("✅ Created route 0.0.0.0/0 -> NAT in private route table");

    // =========================================================================
    // Step 12: Associate Route Tables with Subnets
    // =========================================================================
    info!("🔗 Step 12: Associating Route Tables with Subnets...");

    // Associate public route table with public subnets
    let assoc1 = ctx
        .client
        .associate_route_table(
            AssociateRouteTableRequest::builder()
                .route_table_id(public_rt_id.clone())
                .subnet_id(public_subnet1_id.clone())
                .build(),
        )
        .await
        .expect("Failed to associate public RT with subnet 1");
    let assoc1_id = assoc1
        .association_id
        .expect("Association should have an ID");
    ctx.track_route_table_association(&assoc1_id);
    info!("✅ Associated public RT with public subnet 1");

    let assoc2 = ctx
        .client
        .associate_route_table(
            AssociateRouteTableRequest::builder()
                .route_table_id(public_rt_id.clone())
                .subnet_id(public_subnet2_id.clone())
                .build(),
        )
        .await
        .expect("Failed to associate public RT with subnet 2");
    let assoc2_id = assoc2
        .association_id
        .expect("Association should have an ID");
    ctx.track_route_table_association(&assoc2_id);
    info!("✅ Associated public RT with public subnet 2");

    // Associate private route table with private subnet
    let assoc3 = ctx
        .client
        .associate_route_table(
            AssociateRouteTableRequest::builder()
                .route_table_id(private_rt_id.clone())
                .subnet_id(private_subnet1_id.clone())
                .build(),
        )
        .await
        .expect("Failed to associate private RT with subnet");
    let assoc3_id = assoc3
        .association_id
        .expect("Association should have an ID");
    ctx.track_route_table_association(&assoc3_id);
    info!("✅ Associated private RT with private subnet");

    // =========================================================================
    // Step 13: Verify Route Tables
    // =========================================================================
    info!("🔍 Step 13: Verifying Route Tables...");
    let describe_rt_response = ctx
        .client
        .describe_route_tables(
            DescribeRouteTablesRequest::builder()
                .route_table_ids(vec![public_rt_id.clone(), private_rt_id.clone()])
                .build(),
        )
        .await
        .expect("Failed to describe route tables");

    let route_tables = describe_rt_response
        .route_table_set
        .as_ref()
        .map(|set| &set.items)
        .expect("Should have route tables");
    assert_eq!(route_tables.len(), 2, "Should have 2 route tables");

    // Verify public route table has IGW route
    let public_rt = route_tables
        .iter()
        .find(|rt| rt.route_table_id.as_deref() == Some(&public_rt_id))
        .expect("Should find public route table");
    let has_igw_route = public_rt
        .route_set
        .as_ref()
        .map(|rs| {
            rs.items.iter().any(|r| {
                r.destination_cidr_block.as_deref() == Some("0.0.0.0/0") && r.gateway_id.is_some()
            })
        })
        .unwrap_or(false);
    assert!(has_igw_route, "Public RT should have route to IGW");
    info!("✅ Verified public route table has IGW route");

    // Verify private route table has NAT route
    let private_rt = route_tables
        .iter()
        .find(|rt| rt.route_table_id.as_deref() == Some(&private_rt_id))
        .expect("Should find private route table");
    let has_nat_route = private_rt
        .route_set
        .as_ref()
        .map(|rs| {
            rs.items.iter().any(|r| {
                r.destination_cidr_block.as_deref() == Some("0.0.0.0/0")
                    && r.nat_gateway_id.is_some()
            })
        })
        .unwrap_or(false);
    assert!(has_nat_route, "Private RT should have route to NAT");
    info!("✅ Verified private route table has NAT route");

    // =========================================================================
    // Step 14: Create Security Group
    // =========================================================================
    info!("🔒 Step 14: Creating Security Group...");
    let sg_request = CreateSecurityGroupRequest::builder()
        .group_name(format!("alien-test-sg-{}", test_suffix))
        .description("Test security group created by alien-aws-clients tests".to_string())
        .vpc_id(vpc_id.clone())
        .tag_specifications(vec![TagSpecification::builder()
            .resource_type("security-group".to_string())
            .tags(vec![Tag::builder()
                .key("Name".to_string())
                .value(format!("alien-test-sg-{}", test_suffix))
                .build()])
            .build()])
        .build();

    let sg_response = ctx
        .client
        .create_security_group(sg_request)
        .await
        .expect("Failed to create security group");
    let sg_id = sg_response.group_id.expect("SG should have an ID");
    ctx.track_security_group(&sg_id);
    info!("✅ Created Security Group: {}", sg_id);

    // =========================================================================
    // Step 15: Add Security Group Rules
    // =========================================================================
    info!("📋 Step 15: Adding Security Group Rules...");

    // Add ingress rules (allow HTTP, HTTPS, SSH)
    let ingress_permissions = vec![
        IpPermission::builder()
            .ip_protocol("tcp".to_string())
            .from_port(80)
            .to_port(80)
            .ip_ranges(vec![IpRange::builder()
                .cidr_ip("0.0.0.0/0".to_string())
                .description("Allow HTTP from anywhere".to_string())
                .build()])
            .build(),
        IpPermission::builder()
            .ip_protocol("tcp".to_string())
            .from_port(443)
            .to_port(443)
            .ip_ranges(vec![IpRange::builder()
                .cidr_ip("0.0.0.0/0".to_string())
                .description("Allow HTTPS from anywhere".to_string())
                .build()])
            .build(),
        IpPermission::builder()
            .ip_protocol("tcp".to_string())
            .from_port(22)
            .to_port(22)
            .ip_ranges(vec![IpRange::builder()
                .cidr_ip("10.0.0.0/8".to_string())
                .description("Allow SSH from internal network".to_string())
                .build()])
            .build(),
    ];

    ctx.client
        .authorize_security_group_ingress(
            AuthorizeSecurityGroupIngressRequest::builder()
                .group_id(sg_id.clone())
                .ip_permissions(ingress_permissions)
                .build(),
        )
        .await
        .expect("Failed to add ingress rules");
    info!("✅ Added ingress rules (HTTP, HTTPS, SSH)");

    // =========================================================================
    // Step 16: Verify Security Group
    // =========================================================================
    info!("🔍 Step 16: Verifying Security Group...");
    let describe_sg_response = ctx
        .client
        .describe_security_groups(
            DescribeSecurityGroupsRequest::builder()
                .group_ids(vec![sg_id.clone()])
                .build(),
        )
        .await
        .expect("Failed to describe security groups");

    let sg = describe_sg_response
        .security_group_info
        .as_ref()
        .and_then(|set| set.items.first())
        .expect("Should find security group");

    assert_eq!(
        sg.vpc_id.as_deref(),
        Some(vpc_id.as_str()),
        "SG should be in correct VPC"
    );

    let ingress_rules_count = sg
        .ip_permissions
        .as_ref()
        .map(|perms| perms.items.len())
        .unwrap_or(0);
    assert!(
        ingress_rules_count >= 3,
        "Should have at least 3 ingress rules"
    );
    info!(
        "✅ Verified security group has {} ingress rules",
        ingress_rules_count
    );

    // =========================================================================
    // Step 17: Test VPC Describe with Filters
    // =========================================================================
    info!("🔍 Step 17: Testing VPC describe with filters...");
    let filtered_vpc_response = ctx
        .client
        .describe_vpcs(
            DescribeVpcsRequest::builder()
                .filters(vec![Filter::builder()
                    .name("vpc-id".to_string())
                    .values(vec![vpc_id.clone()])
                    .build()])
                .build(),
        )
        .await
        .expect("Failed to describe VPCs with filter");

    let filtered_vpcs_count = filtered_vpc_response
        .vpc_set
        .as_ref()
        .map(|set| set.items.len())
        .unwrap_or(0);
    assert_eq!(
        filtered_vpcs_count, 1,
        "Should find exactly 1 VPC with filter"
    );
    info!(
        "✅ VPC filter query returned {} VPC(s)",
        filtered_vpcs_count
    );

    // =========================================================================
    // Step 18: Test error handling - non-existent resource
    // =========================================================================
    info!("🚫 Step 18: Testing error handling...");
    let non_existent_result = ctx
        .client
        .describe_vpcs(
            DescribeVpcsRequest::builder()
                .vpc_ids(vec!["vpc-nonexistent12345".to_string()])
                .build(),
        )
        .await;

    match non_existent_result {
        Err(Error {
            error: Some(ErrorData::RemoteResourceNotFound { .. }),
            ..
        }) => {
            info!("✅ Correctly returned ResourceNotFound for non-existent VPC");
        }
        Err(Error {
            error: Some(ErrorData::InvalidInput { .. }),
            ..
        }) => {
            info!("✅ Correctly returned InvalidInput for non-existent VPC");
        }
        Ok(_) => {
            // Some AWS regions return empty results instead of error
            info!("ℹ️ AWS returned empty results for non-existent VPC (region-dependent behavior)");
        }
        Err(e) => {
            warn!("Unexpected error type: {:?}", e);
        }
    }

    info!("🎉 VPC networking E2E test completed successfully!");
    info!("📊 Summary:");
    info!("   - VPC: {}", vpc_id);
    info!(
        "   - Subnets: {}, {}, {}",
        public_subnet1_id, public_subnet2_id, private_subnet1_id
    );
    info!("   - Internet Gateway: {}", igw_id);
    info!("   - NAT Gateway: {}", nat_gateway_id);
    info!("   - Elastic IP: {} ({})", allocation_id, public_ip);
    info!("   - Route Tables: {}, {}", public_rt_id, private_rt_id);
    info!("   - Security Group: {}", sg_id);
}

/// Test describing VPCs without creating any resources.
#[test_context(Ec2TestContext)]
#[tokio::test]
async fn test_describe_vpcs(ctx: &mut Ec2TestContext) {
    info!("🔍 Testing describe VPCs...");

    let response = ctx
        .client
        .describe_vpcs(DescribeVpcsRequest::builder().build())
        .await
        .expect("Failed to describe VPCs");

    let vpc_count = response
        .vpc_set
        .as_ref()
        .map(|set| set.items.len())
        .unwrap_or(0);

    info!("✅ Found {} VPCs in the account", vpc_count);

    // Check if there's a default VPC
    let has_default = response
        .vpc_set
        .as_ref()
        .map(|set| set.items.iter().any(|vpc| vpc.is_default == Some(true)))
        .unwrap_or(false);

    info!("ℹ️ Account has default VPC: {}", has_default);
}

/// Test describing availability zones.
#[test_context(Ec2TestContext)]
#[tokio::test]
async fn test_describe_availability_zones(ctx: &mut Ec2TestContext) {
    info!("📍 Testing describe availability zones...");

    let response = ctx
        .client
        .describe_availability_zones(DescribeAvailabilityZonesRequest::builder().build())
        .await
        .expect("Failed to describe availability zones");

    let azs = response
        .availability_zone_info
        .as_ref()
        .map(|set| &set.items)
        .expect("Should have availability zones");

    assert!(!azs.is_empty(), "Should have at least one AZ");

    info!("✅ Found {} availability zones:", azs.len());
    for az in azs {
        info!(
            "   - {} ({}) - {}",
            az.zone_name.as_deref().unwrap_or("unknown"),
            az.zone_id.as_deref().unwrap_or("unknown"),
            az.zone_state.as_deref().unwrap_or("unknown")
        );
    }
}

/// Test error handling with invalid credentials.
#[test_context(Ec2TestContext)]
#[tokio::test]
async fn test_ec2_client_with_invalid_credentials(_ctx: &mut Ec2TestContext) {
    info!("🔐 Testing EC2 client with invalid credentials...");

    let region = "us-west-2".to_string();
    let account_id =
        std::env::var("AWS_MANAGEMENT_ACCOUNT_ID").unwrap_or_else(|_| "123456789012".to_string());

    let aws_config = alien_aws_clients::AwsClientConfig {
        account_id,
        region,
        credentials: alien_aws_clients::AwsCredentials::AccessKeys {
            access_key_id: "AKIAINVALIDKEY12345".to_string(),
            secret_access_key: "invalidSecretKey123456789012345678901234".to_string(),
            session_token: None,
        },
        service_overrides: None,
    };

    let invalid_client = Ec2Client::new(
        Client::new(),
        AwsCredentialProvider::from_config_sync(aws_config),
    );

    let result = invalid_client
        .describe_vpcs(DescribeVpcsRequest::builder().build())
        .await;

    assert!(result.is_err(), "Should fail with invalid credentials");

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
        Error {
            error: Some(ErrorData::HttpResponseError { http_status, .. }),
            ..
        } if http_status == 403 => {
            info!("✅ Correctly detected invalid credentials (HTTP 403)");
        }
        other => {
            warn!("Got unexpected error type: {:?}", other);
        }
    }
}

// ============================================================================
// AMI/Image Tests
// ============================================================================

/// Test describing AMIs (Amazon Machine Images).
#[test_context(Ec2TestContext)]
#[tokio::test]
async fn test_describe_images(ctx: &mut Ec2TestContext) {
    info!("🖼️ Testing describe images (AMIs)...");

    // Query for Amazon-owned AMIs without filters
    // This ensures we get consistent results across regions
    let request = DescribeImagesRequest::builder()
        .owners(vec!["amazon".to_string()])
        .max_results(5)
        .build();

    let response = ctx
        .client
        .describe_images(request)
        .await
        .expect("Failed to describe images");

    let images = response.images_set.expect("Should have images");

    assert!(
        !images.items.is_empty(),
        "Should find at least one Amazon-owned AMI"
    );

    info!("✅ Found {} Amazon-owned AMIs:", images.items.len());
    for image in images.items.iter().take(3) {
        info!(
            "   - {} ({})",
            image.image_id.as_deref().unwrap_or("unknown"),
            image.name.as_deref().unwrap_or("unknown")
        );
    }
}

/// Test describe instances (read-only).
#[test_context(Ec2TestContext)]
#[tokio::test]
async fn test_describe_instances(ctx: &mut Ec2TestContext) {
    info!("💻 Testing describe instances...");

    let request = DescribeInstancesRequest::builder().max_results(10).build();

    let response = ctx
        .client
        .describe_instances(request)
        .await
        .expect("Failed to describe instances");

    let instance_count = response
        .reservation_set
        .as_ref()
        .map(|rs| {
            rs.items
                .iter()
                .flat_map(|r| r.instances_set.as_ref())
                .flat_map(|is| &is.items)
                .count()
        })
        .unwrap_or(0);

    info!("✅ Found {} instances in the account", instance_count);
}

// ============================================================================
// Launch Template Tests
// ============================================================================

/// Test launch template lifecycle (create, describe, delete).
#[test_context(Ec2TestContext)]
#[tokio::test]
async fn test_launch_template_lifecycle(ctx: &mut Ec2TestContext) {
    let test_suffix = ctx.get_test_name_suffix();
    let template_name = format!("alien-test-lt-{}", test_suffix);

    info!("🚀 Testing launch template lifecycle: {}", template_name);

    // =========================================================================
    // Step 1: Create launch template
    // =========================================================================
    info!("📝 Step 1: Creating launch template");
    let create_request = CreateLaunchTemplateRequest::builder()
        .launch_template_name(template_name.clone())
        .launch_template_data(
            RequestLaunchTemplateData::builder()
                .instance_type("t3.micro".to_string())
                .image_id("ami-0c55b159cbfafe1f0".to_string()) // Amazon Linux 2 in us-west-2
                .build(),
        )
        .tag_specifications(vec![TagSpecification::builder()
            .resource_type("launch-template".to_string())
            .tags(vec![
                Tag::builder()
                    .key("Name".to_string())
                    .value(format!("alien-test-{}", test_suffix))
                    .build(),
                Tag::builder()
                    .key("Environment".to_string())
                    .value("Test".to_string())
                    .build(),
            ])
            .build()])
        .build();

    let create_response = ctx
        .client
        .create_launch_template(create_request)
        .await
        .expect("Failed to create launch template");

    let template_id = create_response
        .launch_template
        .as_ref()
        .and_then(|lt| lt.launch_template_id.clone())
        .expect("Launch template should have ID");

    info!(
        "✅ Launch template created: {} ({})",
        template_name, template_id
    );

    // =========================================================================
    // Step 2: Describe launch template
    // =========================================================================
    info!("🔍 Step 2: Describing launch template");
    let describe_request = DescribeLaunchTemplatesRequest::builder()
        .launch_template_ids(vec![template_id.clone()])
        .build();

    let describe_response = ctx
        .client
        .describe_launch_templates(describe_request)
        .await
        .expect("Failed to describe launch template");

    let templates = describe_response
        .launch_templates
        .expect("Should have launch templates");

    assert_eq!(
        templates.items.len(),
        1,
        "Should find exactly one launch template"
    );
    assert_eq!(
        templates.items[0].launch_template_name.as_deref(),
        Some(template_name.as_str())
    );
    assert_eq!(
        templates.items[0].launch_template_id.as_deref(),
        Some(template_id.as_str())
    );
    info!("✅ Launch template verified");

    // =========================================================================
    // Step 3: Delete launch template
    // =========================================================================
    info!("🗑️ Step 3: Deleting launch template");
    let delete_request = DeleteLaunchTemplateRequest::builder()
        .launch_template_id(template_id.clone())
        .build();

    ctx.client
        .delete_launch_template(delete_request)
        .await
        .expect("Failed to delete launch template");

    info!("✅ Launch template deleted");

    // =========================================================================
    // Step 4: Verify deletion
    // =========================================================================
    info!("❌ Step 4: Verifying launch template is deleted");
    let verify_request = DescribeLaunchTemplatesRequest::builder()
        .launch_template_ids(vec![template_id.clone()])
        .build();

    let result = ctx.client.describe_launch_templates(verify_request).await;

    match result {
        Err(Error {
            error: Some(ErrorData::RemoteResourceNotFound { .. }),
            ..
        }) => {
            info!("✅ Confirmed launch template was deleted");
        }
        Err(Error {
            error: Some(ErrorData::InvalidInput { .. }),
            ..
        }) => {
            info!("✅ Confirmed launch template was deleted (InvalidInput)");
        }
        Ok(response) => {
            let count = response
                .launch_templates
                .map(|t| t.items.len())
                .unwrap_or(0);
            assert_eq!(count, 0, "Should find no launch templates after deletion");
            info!("✅ Confirmed launch template was deleted (empty result)");
        }
        Err(other) => {
            warn!("Got unexpected error: {:?}", other);
        }
    }

    info!("🎉 Launch template lifecycle test completed successfully!");
}

// ============================================================================
// EBS Volume Tests
// ============================================================================

/// Test EBS volume lifecycle (create, describe, delete).
/// Note: We don't test attach/detach as they require a running EC2 instance.
#[test_context(Ec2TestContext)]
#[tokio::test]
async fn test_ebs_volume_lifecycle(ctx: &mut Ec2TestContext) {
    let test_suffix = ctx.get_test_name_suffix();

    info!("💾 Testing EBS volume lifecycle");

    // Get an AZ to create the volume in
    let az_response = ctx
        .client
        .describe_availability_zones(DescribeAvailabilityZonesRequest::builder().build())
        .await
        .expect("Failed to describe availability zones");

    let az = az_response
        .availability_zone_info
        .as_ref()
        .and_then(|set| set.items.first())
        .and_then(|az| az.zone_name.clone())
        .expect("Should have at least one AZ");

    // =========================================================================
    // Step 1: Create volume
    // =========================================================================
    info!("📝 Step 1: Creating EBS volume in {}", az);
    let create_request = CreateVolumeRequest::builder()
        .availability_zone(az.clone())
        .size(1) // 1 GB - minimum size
        .volume_type("gp3".to_string())
        .tag_specifications(vec![TagSpecification::builder()
            .resource_type("volume".to_string())
            .tags(vec![Tag::builder()
                .key("Name".to_string())
                .value(format!("alien-test-volume-{}", test_suffix))
                .build()])
            .build()])
        .build();

    let create_response = ctx
        .client
        .create_volume(create_request)
        .await
        .expect("Failed to create EBS volume");

    let volume_id = create_response.volume_id.expect("Volume should have ID");

    info!("✅ EBS volume created: {}", volume_id);

    // =========================================================================
    // Step 2: Describe volume
    // =========================================================================
    info!("🔍 Step 2: Describing EBS volume");

    // Wait a moment for volume to be available
    tokio::time::sleep(Duration::from_secs(2)).await;

    let describe_request = DescribeVolumesRequest::builder()
        .volume_ids(vec![volume_id.clone()])
        .build();

    let describe_response = ctx
        .client
        .describe_volumes(describe_request)
        .await
        .expect("Failed to describe volumes");

    let volumes = describe_response.volume_set.expect("Should have volumes");

    assert_eq!(volumes.items.len(), 1, "Should find exactly one volume");

    let volume = &volumes.items[0];
    assert_eq!(volume.volume_id.as_deref(), Some(volume_id.as_str()));
    assert_eq!(volume.size, Some(1));
    assert_eq!(volume.volume_type.as_deref(), Some("gp3"));
    info!(
        "✅ Volume verified: size={:?}GB, type={:?}, state={:?}",
        volume.size, volume.volume_type, volume.state
    );

    // =========================================================================
    // Step 3: Delete volume
    // =========================================================================
    info!("🗑️ Step 3: Deleting EBS volume");
    ctx.client
        .delete_volume(&volume_id)
        .await
        .expect("Failed to delete EBS volume");

    info!("✅ EBS volume deleted");

    // =========================================================================
    // Step 4: Verify deletion
    // =========================================================================
    info!("❌ Step 4: Verifying volume is deleted");

    // Wait a moment for deletion to propagate
    tokio::time::sleep(Duration::from_secs(2)).await;

    let verify_request = DescribeVolumesRequest::builder()
        .volume_ids(vec![volume_id.clone()])
        .build();

    let result = ctx.client.describe_volumes(verify_request).await;

    match result {
        Err(Error {
            error: Some(ErrorData::RemoteResourceNotFound { .. }),
            ..
        }) => {
            info!("✅ Confirmed volume was deleted");
        }
        Err(Error {
            error: Some(ErrorData::InvalidInput { .. }),
            ..
        }) => {
            info!("✅ Confirmed volume was deleted (InvalidInput)");
        }
        Ok(response) => {
            // Volume might still exist but in "deleted" state
            if let Some(vol_set) = response.volume_set {
                for vol in vol_set.items {
                    let state = vol.state.as_deref().unwrap_or("unknown");
                    if state == "deleted" || state == "deleting" {
                        info!("✅ Volume is in {} state", state);
                    } else {
                        warn!("Volume still exists with state: {}", state);
                    }
                }
            }
        }
        Err(other) => {
            warn!("Got unexpected error: {:?}", other);
        }
    }

    info!("🎉 EBS volume lifecycle test completed successfully!");
}
