use std::collections::HashMap;

use alien_core::{AwsClientConfig, AwsCredentials, Platform};
use alien_error::{AlienError, Context, ContextError, IntoAlienError, IntoAlienErrorDirect};
use async_trait::async_trait;
use aws_config::{BehaviorVersion, SdkConfig};
use aws_credential_types::Credentials;
use aws_sdk_acm::Client as AcmClient;
use aws_sdk_apigatewayv2::Client as ApiGatewayV2Client;
use aws_sdk_codebuild::Client as CodeBuildClient;
use aws_sdk_dynamodb::Client as DynamoDbClient;
use aws_sdk_ec2::Client as Ec2Client;
use aws_sdk_ecr::Client as EcrClient;
use aws_sdk_eventbridge::Client as EventBridgeClient;
use aws_sdk_iam::Client as IamClient;
use aws_sdk_lambda::Client as LambdaClient;
use aws_sdk_s3::{
    error::ProvideErrorMetadata,
    operation::{
        create_bucket::CreateBucketError, delete_bucket::DeleteBucketError,
        delete_bucket_lifecycle::DeleteBucketLifecycleError,
        delete_bucket_policy::DeleteBucketPolicyError, get_bucket_acl::GetBucketAclError,
        get_bucket_encryption::GetBucketEncryptionError,
        get_bucket_lifecycle_configuration::GetBucketLifecycleConfigurationError,
        get_bucket_notification_configuration::GetBucketNotificationConfigurationError,
        get_bucket_policy::GetBucketPolicyError,
        get_public_access_block::GetPublicAccessBlockError,
        list_object_versions::ListObjectVersionsError, list_objects_v2::ListObjectsV2Error,
    },
    types::{
        BucketLocationConstraint, CreateBucketConfiguration, Delete, ObjectIdentifier,
        Tag as S3Tag, Tagging, VersioningConfiguration,
    },
    Client as S3Client,
};
use aws_sdk_sqs::Client as SqsClient;
use aws_types::region::Region;

use crate::error::{ErrorData, Result};

pub use aws_sdk_ec2::{
    operation::{
        allocate_address::{
            AllocateAddressInput as AllocateAddressRequest,
            AllocateAddressOutput as AllocateAddressResponse,
        },
        associate_route_table::{
            AssociateRouteTableInput as AssociateRouteTableRequest,
            AssociateRouteTableOutput as AssociateRouteTableResponse,
        },
        attach_internet_gateway::AttachInternetGatewayInput as AttachInternetGatewayRequest,
        authorize_security_group_egress::AuthorizeSecurityGroupEgressInput as AuthorizeSecurityGroupEgressRequest,
        authorize_security_group_ingress::AuthorizeSecurityGroupIngressInput as AuthorizeSecurityGroupIngressRequest,
        create_internet_gateway::{
            CreateInternetGatewayInput as CreateInternetGatewayRequest,
            CreateInternetGatewayOutput as CreateInternetGatewayResponse,
        },
        create_nat_gateway::{
            CreateNatGatewayInput as CreateNatGatewayRequest,
            CreateNatGatewayOutput as CreateNatGatewayResponse,
        },
        create_route::CreateRouteInput as CreateRouteRequest,
        create_route_table::{
            CreateRouteTableInput as CreateRouteTableRequest,
            CreateRouteTableOutput as CreateRouteTableResponse,
        },
        create_security_group::{
            CreateSecurityGroupInput as CreateSecurityGroupRequest,
            CreateSecurityGroupOutput as CreateSecurityGroupResponse,
        },
        create_subnet::{
            CreateSubnetInput as CreateSubnetRequest, CreateSubnetOutput as CreateSubnetResponse,
        },
        create_vpc::{CreateVpcInput as CreateVpcRequest, CreateVpcOutput as CreateVpcResponse},
        delete_nat_gateway::{
            DeleteNatGatewayInput as DeleteNatGatewayRequest,
            DeleteNatGatewayOutput as DeleteNatGatewayResponse,
        },
        describe_availability_zones::{
            DescribeAvailabilityZonesInput as DescribeAvailabilityZonesRequest,
            DescribeAvailabilityZonesOutput as DescribeAvailabilityZonesResponse,
        },
        describe_nat_gateways::{
            DescribeNatGatewaysInput as DescribeNatGatewaysRequest,
            DescribeNatGatewaysOutput as DescribeNatGatewaysResponse,
        },
        describe_network_interfaces::{
            DescribeNetworkInterfacesInput as DescribeNetworkInterfacesRequest,
            DescribeNetworkInterfacesOutput as DescribeNetworkInterfacesResponse,
        },
        describe_security_groups::{
            DescribeSecurityGroupsInput as DescribeSecurityGroupsRequest,
            DescribeSecurityGroupsOutput as DescribeSecurityGroupsResponse,
        },
        describe_subnets::{
            DescribeSubnetsInput as DescribeSubnetsRequest,
            DescribeSubnetsOutput as DescribeSubnetsResponse,
        },
        describe_vpcs::{
            DescribeVpcsInput as DescribeVpcsRequest, DescribeVpcsOutput as DescribeVpcsResponse,
        },
        detach_internet_gateway::DetachInternetGatewayInput as DetachInternetGatewayRequest,
        modify_vpc_attribute::ModifyVpcAttributeInput as ModifyVpcAttributeRequest,
    },
    types::{
        AttributeBooleanValue, AvailabilityZone, ConnectivityType, DomainType, Filter,
        InternetGateway, IpPermission, IpRange, NatGateway, NetworkInterface,
        ResourceType as Ec2ResourceType, RouteTable, SecurityGroup, Subnet, Tag as Ec2Tag,
        TagSpecification, Vpc,
    },
};

pub use aws_sdk_iam::{
    operation::{
        create_policy::CreatePolicyOutput as CreatePolicyResponse,
        create_policy_version::CreatePolicyVersionOutput as CreatePolicyVersionResponse,
        create_role::{CreateRoleInput, CreateRoleOutput as CreateRoleResponse},
        get_role::GetRoleOutput as GetRoleResponse,
        get_role_policy::GetRolePolicyOutput as GetRolePolicyResponse,
        list_attached_role_policies::ListAttachedRolePoliciesOutput as ListAttachedRolePoliciesResponse,
        list_policy_versions::ListPolicyVersionsOutput as ListPolicyVersionsResponse,
        list_role_policies::ListRolePoliciesOutput as ListRolePoliciesResponse,
    },
    types::{AttachedPolicy, Policy, PolicyVersion, Role, Tag as IamTag},
};
pub use aws_sdk_s3::types::{
    BucketLifecycleConfiguration as S3BucketLifecycleConfiguration, BucketVersioningStatus,
    Event as S3Event, ExpirationStatus as S3ExpirationStatus, LambdaFunctionConfiguration,
    LifecycleExpiration as S3LifecycleExpiration, LifecycleRule as S3LifecycleRule,
    LifecycleRuleFilter as S3LifecycleRuleFilter, NotificationConfiguration,
    PublicAccessBlockConfiguration as S3PublicAccessBlock,
};

/// S3 bucket metadata used for storage heartbeats.
#[derive(Debug, Clone)]
pub struct S3BucketMetadata {
    /// Bucket region.
    pub region: String,
    /// Bucket versioning status.
    pub versioning_status: Option<BucketVersioningStatus>,
    /// Number of lifecycle rules configured.
    pub lifecycle_rule_count: Option<u64>,
    /// Number of server-side encryption rules configured.
    pub encryption_rule_count: Option<u64>,
    /// Public access block configuration, when present.
    pub public_access_block: Option<S3PublicAccessBlock>,
    /// Whether a non-empty bucket policy is present.
    pub bucket_policy_present: Option<bool>,
    /// Whether bucket ACL metadata is present.
    pub bucket_acl_present: Option<bool>,
}

/// Minimal IAM operations required by infra controllers.
#[cfg_attr(any(test, feature = "test-utils"), mockall::automock)]
#[async_trait]
pub trait IamApi: Send + Sync {
    /// Create an IAM role.
    async fn create_role(&self, request: CreateRoleInput) -> Result<CreateRoleResponse>;
    /// Get an IAM role.
    async fn get_role(&self, role_name: &str) -> Result<GetRoleResponse>;
    /// Delete an IAM role.
    async fn delete_role(&self, role_name: &str) -> Result<()>;
    /// Put an inline role policy.
    async fn put_role_policy(
        &self,
        role_name: &str,
        policy_name: &str,
        policy_document: &str,
    ) -> Result<()>;
    /// Get an inline role policy.
    async fn get_role_policy(
        &self,
        role_name: &str,
        policy_name: &str,
    ) -> Result<GetRolePolicyResponse>;
    /// Delete an inline role policy.
    async fn delete_role_policy(&self, role_name: &str, policy_name: &str) -> Result<()>;
    /// Update a role trust policy.
    async fn update_assume_role_policy(&self, role_name: &str, policy_document: &str)
        -> Result<()>;
    /// List managed policies attached to a role.
    async fn list_attached_role_policies(
        &self,
        role_name: &str,
    ) -> Result<ListAttachedRolePoliciesResponse>;
    /// Create a managed policy.
    async fn create_policy(
        &self,
        policy_name: &str,
        policy_document: &str,
        path: Option<String>,
    ) -> Result<CreatePolicyResponse>;
    /// Delete a managed policy.
    async fn delete_policy(&self, policy_arn: &str) -> Result<()>;
    /// Create a managed policy version.
    async fn create_policy_version(
        &self,
        policy_arn: &str,
        policy_document: &str,
        set_as_default: bool,
    ) -> Result<CreatePolicyVersionResponse>;
    /// Delete a managed policy version.
    async fn delete_policy_version(&self, policy_arn: &str, version_id: &str) -> Result<()>;
    /// List managed policy versions.
    async fn list_policy_versions(&self, policy_arn: &str) -> Result<ListPolicyVersionsResponse>;
    /// Attach a managed policy to a role.
    async fn attach_role_policy(&self, role_name: &str, policy_arn: &str) -> Result<()>;
    /// Detach a managed policy from a role.
    async fn detach_role_policy(&self, role_name: &str, policy_arn: &str) -> Result<()>;
    /// List inline role policies.
    async fn list_role_policies(&self, role_name: &str) -> Result<ListRolePoliciesResponse>;
}

/// Minimal EC2 operations required by infra network and worker controllers.
#[cfg_attr(any(test, feature = "test-utils"), mockall::automock)]
#[async_trait]
pub trait Ec2Api: Send + Sync {
    /// Describe VPCs.
    async fn describe_vpcs(&self, request: DescribeVpcsRequest) -> Result<DescribeVpcsResponse>;
    /// Create a VPC.
    async fn create_vpc(&self, request: CreateVpcRequest) -> Result<CreateVpcResponse>;
    /// Delete a VPC.
    async fn delete_vpc(&self, vpc_id: &str) -> Result<()>;
    /// Modify a VPC attribute.
    async fn modify_vpc_attribute(&self, request: ModifyVpcAttributeRequest) -> Result<()>;
    /// Describe subnets.
    async fn describe_subnets(
        &self,
        request: DescribeSubnetsRequest,
    ) -> Result<DescribeSubnetsResponse>;
    /// Create a subnet.
    async fn create_subnet(&self, request: CreateSubnetRequest) -> Result<CreateSubnetResponse>;
    /// Delete a subnet.
    async fn delete_subnet(&self, subnet_id: &str) -> Result<()>;
    /// Create an internet gateway.
    async fn create_internet_gateway(
        &self,
        request: CreateInternetGatewayRequest,
    ) -> Result<CreateInternetGatewayResponse>;
    /// Delete an internet gateway.
    async fn delete_internet_gateway(&self, internet_gateway_id: &str) -> Result<()>;
    /// Attach an internet gateway.
    async fn attach_internet_gateway(&self, request: AttachInternetGatewayRequest) -> Result<()>;
    /// Detach an internet gateway.
    async fn detach_internet_gateway(&self, request: DetachInternetGatewayRequest) -> Result<()>;
    /// Create a NAT gateway.
    async fn create_nat_gateway(
        &self,
        request: CreateNatGatewayRequest,
    ) -> Result<CreateNatGatewayResponse>;
    /// Delete a NAT gateway.
    async fn delete_nat_gateway(
        &self,
        request: DeleteNatGatewayRequest,
    ) -> Result<DeleteNatGatewayResponse>;
    /// Describe NAT gateways.
    async fn describe_nat_gateways(
        &self,
        request: DescribeNatGatewaysRequest,
    ) -> Result<DescribeNatGatewaysResponse>;
    /// Allocate an Elastic IP.
    async fn allocate_address(
        &self,
        request: AllocateAddressRequest,
    ) -> Result<AllocateAddressResponse>;
    /// Release an Elastic IP.
    async fn release_address(&self, allocation_id: &str) -> Result<()>;
    /// Create a route table.
    async fn create_route_table(
        &self,
        request: CreateRouteTableRequest,
    ) -> Result<CreateRouteTableResponse>;
    /// Delete a route table.
    async fn delete_route_table(&self, route_table_id: &str) -> Result<()>;
    /// Create a route.
    async fn create_route(&self, request: CreateRouteRequest) -> Result<()>;
    /// Associate a route table.
    async fn associate_route_table(
        &self,
        request: AssociateRouteTableRequest,
    ) -> Result<AssociateRouteTableResponse>;
    /// Disassociate a route table.
    async fn disassociate_route_table(&self, association_id: &str) -> Result<()>;
    /// Describe security groups.
    async fn describe_security_groups(
        &self,
        request: DescribeSecurityGroupsRequest,
    ) -> Result<DescribeSecurityGroupsResponse>;
    /// Describe network interfaces.
    async fn describe_network_interfaces(
        &self,
        request: DescribeNetworkInterfacesRequest,
    ) -> Result<DescribeNetworkInterfacesResponse>;
    /// Create a security group.
    async fn create_security_group(
        &self,
        request: CreateSecurityGroupRequest,
    ) -> Result<CreateSecurityGroupResponse>;
    /// Delete a security group.
    async fn delete_security_group(&self, group_id: &str) -> Result<()>;
    /// Authorize ingress rules.
    async fn authorize_security_group_ingress(
        &self,
        request: AuthorizeSecurityGroupIngressRequest,
    ) -> Result<()>;
    /// Authorize egress rules.
    async fn authorize_security_group_egress(
        &self,
        request: AuthorizeSecurityGroupEgressRequest,
    ) -> Result<()>;
    /// Describe availability zones.
    async fn describe_availability_zones(
        &self,
        request: DescribeAvailabilityZonesRequest,
    ) -> Result<DescribeAvailabilityZonesResponse>;
}

/// Minimal S3 operations required by infra controllers.
#[cfg_attr(any(test, feature = "test-utils"), mockall::automock)]
#[async_trait]
pub trait S3Api: Send + Sync {
    /// Create a bucket. Already-owned buckets are treated as success.
    async fn create_bucket(&self, bucket_name: &str) -> Result<()>;

    /// Put ABAC tags on a bucket.
    async fn put_bucket_abac_tags(
        &self,
        bucket_name: &str,
        tags: &HashMap<String, String>,
    ) -> Result<()>;

    /// Configure bucket versioning.
    async fn put_bucket_versioning(
        &self,
        bucket_name: &str,
        status: BucketVersioningStatus,
    ) -> Result<()>;

    /// Configure public access blocking.
    async fn put_public_access_block(
        &self,
        bucket_name: &str,
        config: S3PublicAccessBlock,
    ) -> Result<()>;

    /// Put a bucket policy document.
    async fn put_bucket_policy(&self, bucket_name: &str, policy: &str) -> Result<()>;

    /// Delete the bucket policy. Missing policies are treated as success.
    async fn delete_bucket_policy(&self, bucket_name: &str) -> Result<()>;

    /// Put lifecycle rules.
    async fn put_bucket_lifecycle_configuration(
        &self,
        bucket_name: &str,
        rules: Vec<S3LifecycleRule>,
    ) -> Result<()>;

    /// Delete lifecycle configuration. Missing lifecycle configuration is treated as success.
    async fn delete_bucket_lifecycle(&self, bucket_name: &str) -> Result<()>;

    /// Collect bucket metadata for heartbeat emission.
    async fn get_bucket_metadata(&self, bucket_name: &str) -> Result<S3BucketMetadata>;

    /// Empty a bucket, including versions and delete markers.
    async fn empty_bucket(&self, bucket_name: &str) -> Result<()>;

    /// Delete a bucket. Returns false when the bucket is already absent.
    async fn delete_bucket(&self, bucket_name: &str) -> Result<bool>;

    /// Get bucket notification configuration.
    async fn get_bucket_notification_configuration(
        &self,
        bucket_name: &str,
    ) -> Result<NotificationConfiguration>;

    /// Put bucket notification configuration.
    async fn put_bucket_notification_configuration(
        &self,
        bucket_name: &str,
        config: &NotificationConfiguration,
    ) -> Result<()>;
}

#[async_trait]
impl IamApi for IamClient {
    async fn create_role(&self, request: CreateRoleInput) -> Result<CreateRoleResponse> {
        let role_name = request
            .role_name()
            .map(ToString::to_string)
            .ok_or_else(|| {
                AlienError::new(ErrorData::CloudPlatformError {
                    message: "CreateRole request did not include roleName".to_string(),
                    resource_id: None,
                })
            })?;
        if request.assume_role_policy_document().is_none() {
            return Err(AlienError::new(ErrorData::CloudPlatformError {
                message: format!(
                    "CreateRole request for '{role_name}' did not include assumeRolePolicyDocument"
                ),
                resource_id: None,
            }));
        }

        let response = iam_result(
            self.create_role()
                .set_role_name(request.role_name)
                .set_assume_role_policy_document(request.assume_role_policy_document)
                .set_path(request.path)
                .set_description(request.description)
                .set_max_session_duration(request.max_session_duration)
                .set_permissions_boundary(request.permissions_boundary)
                .set_tags(request.tags)
                .send()
                .await,
            "CreateRole",
            "IAM Role",
            &role_name,
        )?;

        response.role().ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: format!(
                    "IAM CreateRole response for '{role_name}' did not include a role"
                ),
                resource_id: None,
            })
        })?;

        Ok(response)
    }

    async fn get_role(&self, role_name: &str) -> Result<GetRoleResponse> {
        let response = iam_result(
            self.get_role().role_name(role_name).send().await,
            "GetRole",
            "IAM Role",
            role_name,
        )?;

        response.role().ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: format!("IAM GetRole response for '{role_name}' did not include a role"),
                resource_id: None,
            })
        })?;

        Ok(response)
    }

    async fn delete_role(&self, role_name: &str) -> Result<()> {
        iam_result(
            self.delete_role().role_name(role_name).send().await,
            "DeleteRole",
            "IAM Role",
            role_name,
        )?;
        Ok(())
    }

    async fn put_role_policy(
        &self,
        role_name: &str,
        policy_name: &str,
        policy_document: &str,
    ) -> Result<()> {
        let resource_name = format!("{role_name}/{policy_name}");
        iam_result(
            self.put_role_policy()
                .role_name(role_name)
                .policy_name(policy_name)
                .policy_document(policy_document)
                .send()
                .await,
            "PutRolePolicy",
            "IAM RolePolicy",
            &resource_name,
        )?;
        Ok(())
    }

    async fn get_role_policy(
        &self,
        role_name: &str,
        policy_name: &str,
    ) -> Result<GetRolePolicyResponse> {
        let resource_name = format!("{role_name}/{policy_name}");
        let response = iam_result(
            self.get_role_policy()
                .role_name(role_name)
                .policy_name(policy_name)
                .send()
                .await,
            "GetRolePolicy",
            "IAM RolePolicy",
            &resource_name,
        )?;

        Ok(response)
    }

    async fn delete_role_policy(&self, role_name: &str, policy_name: &str) -> Result<()> {
        let resource_name = format!("{role_name}/{policy_name}");
        iam_result(
            self.delete_role_policy()
                .role_name(role_name)
                .policy_name(policy_name)
                .send()
                .await,
            "DeleteRolePolicy",
            "IAM RolePolicy",
            &resource_name,
        )?;
        Ok(())
    }

    async fn update_assume_role_policy(
        &self,
        role_name: &str,
        policy_document: &str,
    ) -> Result<()> {
        iam_result(
            self.update_assume_role_policy()
                .role_name(role_name)
                .policy_document(policy_document)
                .send()
                .await,
            "UpdateAssumeRolePolicy",
            "IAM Role",
            role_name,
        )?;
        Ok(())
    }

    async fn list_attached_role_policies(
        &self,
        role_name: &str,
    ) -> Result<ListAttachedRolePoliciesResponse> {
        let response = iam_result(
            self.list_attached_role_policies()
                .role_name(role_name)
                .send()
                .await,
            "ListAttachedRolePolicies",
            "IAM Role",
            role_name,
        )?;

        for policy in response.attached_policies() {
            if policy.policy_name().is_none() || policy.policy_arn().is_none() {
                return Err(AlienError::new(ErrorData::CloudPlatformError {
                    message: format!(
                        "IAM ListAttachedRolePolicies response for '{role_name}' included a policy without both name and ARN"
                    ),
                    resource_id: None,
                }));
            }
        }

        Ok(response)
    }

    async fn create_policy(
        &self,
        policy_name: &str,
        policy_document: &str,
        path: Option<String>,
    ) -> Result<CreatePolicyResponse> {
        let response = iam_result(
            self.create_policy()
                .policy_name(policy_name)
                .policy_document(policy_document)
                .set_path(path)
                .send()
                .await,
            "CreatePolicy",
            "IAM Policy",
            policy_name,
        )?;

        response.policy().ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: format!(
                    "IAM CreatePolicy response for '{policy_name}' did not include a policy"
                ),
                resource_id: None,
            })
        })?;

        Ok(response)
    }

    async fn delete_policy(&self, policy_arn: &str) -> Result<()> {
        iam_result(
            self.delete_policy().policy_arn(policy_arn).send().await,
            "DeletePolicy",
            "IAM Policy",
            policy_arn,
        )?;
        Ok(())
    }

    async fn create_policy_version(
        &self,
        policy_arn: &str,
        policy_document: &str,
        set_as_default: bool,
    ) -> Result<CreatePolicyVersionResponse> {
        let response = iam_result(
            self.create_policy_version()
                .policy_arn(policy_arn)
                .policy_document(policy_document)
                .set_as_default(set_as_default)
                .send()
                .await,
            "CreatePolicyVersion",
            "IAM Policy",
            policy_arn,
        )?;

        response.policy_version().ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: format!(
                    "IAM CreatePolicyVersion response for '{policy_arn}' did not include a version"
                ),
                resource_id: None,
            })
        })?;

        Ok(response)
    }

    async fn delete_policy_version(&self, policy_arn: &str, version_id: &str) -> Result<()> {
        let resource_name = format!("{policy_arn}/{version_id}");
        iam_result(
            self.delete_policy_version()
                .policy_arn(policy_arn)
                .version_id(version_id)
                .send()
                .await,
            "DeletePolicyVersion",
            "IAM PolicyVersion",
            &resource_name,
        )?;
        Ok(())
    }

    async fn list_policy_versions(&self, policy_arn: &str) -> Result<ListPolicyVersionsResponse> {
        let response = iam_result(
            self.list_policy_versions()
                .policy_arn(policy_arn)
                .send()
                .await,
            "ListPolicyVersions",
            "IAM Policy",
            policy_arn,
        )?;

        Ok(response)
    }

    async fn attach_role_policy(&self, role_name: &str, policy_arn: &str) -> Result<()> {
        let resource_name = format!("{role_name}/{policy_arn}");
        iam_result(
            self.attach_role_policy()
                .role_name(role_name)
                .policy_arn(policy_arn)
                .send()
                .await,
            "AttachRolePolicy",
            "IAM RolePolicyAttachment",
            &resource_name,
        )?;
        Ok(())
    }

    async fn detach_role_policy(&self, role_name: &str, policy_arn: &str) -> Result<()> {
        let resource_name = format!("{role_name}/{policy_arn}");
        iam_result(
            self.detach_role_policy()
                .role_name(role_name)
                .policy_arn(policy_arn)
                .send()
                .await,
            "DetachRolePolicy",
            "IAM RolePolicyAttachment",
            &resource_name,
        )?;
        Ok(())
    }

    async fn list_role_policies(&self, role_name: &str) -> Result<ListRolePoliciesResponse> {
        let response = iam_result(
            self.list_role_policies().role_name(role_name).send().await,
            "ListRolePolicies",
            "IAM Role",
            role_name,
        )?;

        Ok(response)
    }
}

#[async_trait]
impl Ec2Api for Ec2Client {
    async fn describe_vpcs(&self, request: DescribeVpcsRequest) -> Result<DescribeVpcsResponse> {
        ec2_result(
            self.describe_vpcs()
                .set_vpc_ids(request.vpc_ids)
                .set_filters(request.filters)
                .set_max_results(request.max_results)
                .set_next_token(request.next_token)
                .set_dry_run(request.dry_run)
                .send()
                .await,
            "DescribeVpcs",
            "VPC",
            "*",
        )
    }

    async fn create_vpc(&self, request: CreateVpcRequest) -> Result<CreateVpcResponse> {
        let cidr_block = request.cidr_block().unwrap_or("<unknown>").to_string();
        ec2_result(
            self.create_vpc()
                .set_cidr_block(request.cidr_block)
                .set_ipv6_pool(request.ipv6_pool)
                .set_ipv6_cidr_block(request.ipv6_cidr_block)
                .set_ipv4_ipam_pool_id(request.ipv4_ipam_pool_id)
                .set_ipv4_netmask_length(request.ipv4_netmask_length)
                .set_ipv6_ipam_pool_id(request.ipv6_ipam_pool_id)
                .set_ipv6_netmask_length(request.ipv6_netmask_length)
                .set_ipv6_cidr_block_network_border_group(
                    request.ipv6_cidr_block_network_border_group,
                )
                .set_vpc_encryption_control(request.vpc_encryption_control)
                .set_tag_specifications(request.tag_specifications)
                .set_dry_run(request.dry_run)
                .set_instance_tenancy(request.instance_tenancy)
                .set_amazon_provided_ipv6_cidr_block(request.amazon_provided_ipv6_cidr_block)
                .send()
                .await,
            "CreateVpc",
            "VPC",
            &cidr_block,
        )
    }

    async fn delete_vpc(&self, vpc_id: &str) -> Result<()> {
        ec2_result(
            self.delete_vpc().vpc_id(vpc_id).send().await,
            "DeleteVpc",
            "VPC",
            vpc_id,
        )?;
        Ok(())
    }

    async fn modify_vpc_attribute(&self, request: ModifyVpcAttributeRequest) -> Result<()> {
        let vpc_id = request.vpc_id().unwrap_or("<unknown>").to_string();

        ec2_result(
            self.modify_vpc_attribute()
                .set_enable_dns_hostnames(request.enable_dns_hostnames)
                .set_enable_dns_support(request.enable_dns_support)
                .set_vpc_id(request.vpc_id)
                .set_enable_network_address_usage_metrics(
                    request.enable_network_address_usage_metrics,
                )
                .send()
                .await,
            "ModifyVpcAttribute",
            "VPC",
            &vpc_id,
        )?;
        Ok(())
    }

    async fn describe_subnets(
        &self,
        request: DescribeSubnetsRequest,
    ) -> Result<DescribeSubnetsResponse> {
        ec2_result(
            self.describe_subnets()
                .set_subnet_ids(request.subnet_ids)
                .set_filters(request.filters)
                .set_max_results(request.max_results)
                .set_next_token(request.next_token)
                .set_dry_run(request.dry_run)
                .send()
                .await,
            "DescribeSubnets",
            "Subnet",
            "*",
        )
    }

    async fn create_subnet(&self, request: CreateSubnetRequest) -> Result<CreateSubnetResponse> {
        let cidr_block = request.cidr_block().unwrap_or("<unknown>").to_string();
        ec2_result(
            self.create_subnet()
                .set_tag_specifications(request.tag_specifications)
                .set_availability_zone(request.availability_zone)
                .set_availability_zone_id(request.availability_zone_id)
                .set_cidr_block(request.cidr_block)
                .set_ipv6_cidr_block(request.ipv6_cidr_block)
                .set_outpost_arn(request.outpost_arn)
                .set_vpc_id(request.vpc_id)
                .set_ipv6_native(request.ipv6_native)
                .set_ipv4_ipam_pool_id(request.ipv4_ipam_pool_id)
                .set_ipv4_netmask_length(request.ipv4_netmask_length)
                .set_ipv6_ipam_pool_id(request.ipv6_ipam_pool_id)
                .set_ipv6_netmask_length(request.ipv6_netmask_length)
                .set_dry_run(request.dry_run)
                .send()
                .await,
            "CreateSubnet",
            "Subnet",
            &cidr_block,
        )
    }

    async fn delete_subnet(&self, subnet_id: &str) -> Result<()> {
        ec2_result(
            self.delete_subnet().subnet_id(subnet_id).send().await,
            "DeleteSubnet",
            "Subnet",
            subnet_id,
        )?;
        Ok(())
    }

    async fn create_internet_gateway(
        &self,
        request: CreateInternetGatewayRequest,
    ) -> Result<CreateInternetGatewayResponse> {
        ec2_result(
            self.create_internet_gateway()
                .set_tag_specifications(request.tag_specifications)
                .set_dry_run(request.dry_run)
                .send()
                .await,
            "CreateInternetGateway",
            "InternetGateway",
            "*",
        )
    }

    async fn delete_internet_gateway(&self, internet_gateway_id: &str) -> Result<()> {
        ec2_result(
            self.delete_internet_gateway()
                .internet_gateway_id(internet_gateway_id)
                .send()
                .await,
            "DeleteInternetGateway",
            "InternetGateway",
            internet_gateway_id,
        )?;
        Ok(())
    }

    async fn attach_internet_gateway(&self, request: AttachInternetGatewayRequest) -> Result<()> {
        let internet_gateway_id = request
            .internet_gateway_id()
            .unwrap_or("<unknown>")
            .to_string();
        ec2_result(
            self.attach_internet_gateway()
                .set_dry_run(request.dry_run)
                .set_internet_gateway_id(request.internet_gateway_id)
                .set_vpc_id(request.vpc_id)
                .send()
                .await,
            "AttachInternetGateway",
            "InternetGateway",
            &internet_gateway_id,
        )?;
        Ok(())
    }

    async fn detach_internet_gateway(&self, request: DetachInternetGatewayRequest) -> Result<()> {
        let internet_gateway_id = request
            .internet_gateway_id()
            .unwrap_or("<unknown>")
            .to_string();
        ec2_result(
            self.detach_internet_gateway()
                .set_dry_run(request.dry_run)
                .set_internet_gateway_id(request.internet_gateway_id)
                .set_vpc_id(request.vpc_id)
                .send()
                .await,
            "DetachInternetGateway",
            "InternetGateway",
            &internet_gateway_id,
        )?;
        Ok(())
    }

    async fn create_nat_gateway(
        &self,
        request: CreateNatGatewayRequest,
    ) -> Result<CreateNatGatewayResponse> {
        let subnet_id = request.subnet_id().unwrap_or("<unknown>").to_string();

        ec2_result(
            self.create_nat_gateway()
                .set_availability_mode(request.availability_mode)
                .set_allocation_id(request.allocation_id)
                .set_client_token(request.client_token)
                .set_dry_run(request.dry_run)
                .set_subnet_id(request.subnet_id)
                .set_vpc_id(request.vpc_id)
                .set_availability_zone_addresses(request.availability_zone_addresses)
                .set_tag_specifications(request.tag_specifications)
                .set_connectivity_type(request.connectivity_type)
                .set_private_ip_address(request.private_ip_address)
                .set_secondary_allocation_ids(request.secondary_allocation_ids)
                .set_secondary_private_ip_addresses(request.secondary_private_ip_addresses)
                .set_secondary_private_ip_address_count(request.secondary_private_ip_address_count)
                .send()
                .await,
            "CreateNatGateway",
            "NatGateway",
            &subnet_id,
        )
    }

    async fn delete_nat_gateway(
        &self,
        request: DeleteNatGatewayRequest,
    ) -> Result<DeleteNatGatewayResponse> {
        let nat_gateway_id = request.nat_gateway_id().unwrap_or("<unknown>").to_string();

        ec2_result(
            self.delete_nat_gateway()
                .set_dry_run(request.dry_run)
                .set_nat_gateway_id(request.nat_gateway_id)
                .send()
                .await,
            "DeleteNatGateway",
            "NatGateway",
            &nat_gateway_id,
        )
    }

    async fn describe_nat_gateways(
        &self,
        request: DescribeNatGatewaysRequest,
    ) -> Result<DescribeNatGatewaysResponse> {
        ec2_result(
            self.describe_nat_gateways()
                .set_dry_run(request.dry_run)
                .set_nat_gateway_ids(request.nat_gateway_ids)
                .set_filter(request.filter)
                .set_max_results(request.max_results)
                .set_next_token(request.next_token)
                .send()
                .await,
            "DescribeNatGateways",
            "NatGateway",
            "*",
        )
    }

    async fn allocate_address(
        &self,
        request: AllocateAddressRequest,
    ) -> Result<AllocateAddressResponse> {
        ec2_result(
            self.allocate_address()
                .set_domain(request.domain)
                .set_address(request.address)
                .set_public_ipv4_pool(request.public_ipv4_pool)
                .set_network_border_group(request.network_border_group)
                .set_customer_owned_ipv4_pool(request.customer_owned_ipv4_pool)
                .set_tag_specifications(request.tag_specifications)
                .set_ipam_pool_id(request.ipam_pool_id)
                .set_dry_run(request.dry_run)
                .send()
                .await,
            "AllocateAddress",
            "ElasticIP",
            "*",
        )
    }

    async fn release_address(&self, allocation_id: &str) -> Result<()> {
        ec2_result(
            self.release_address()
                .allocation_id(allocation_id)
                .send()
                .await,
            "ReleaseAddress",
            "ElasticIP",
            allocation_id,
        )?;
        Ok(())
    }

    async fn create_route_table(
        &self,
        request: CreateRouteTableRequest,
    ) -> Result<CreateRouteTableResponse> {
        let vpc_id = request.vpc_id().unwrap_or("<unknown>").to_string();

        ec2_result(
            self.create_route_table()
                .set_client_token(request.client_token)
                .set_dry_run(request.dry_run)
                .set_vpc_id(request.vpc_id)
                .set_tag_specifications(request.tag_specifications)
                .send()
                .await,
            "CreateRouteTable",
            "RouteTable",
            &vpc_id,
        )
    }

    async fn delete_route_table(&self, route_table_id: &str) -> Result<()> {
        ec2_result(
            self.delete_route_table()
                .route_table_id(route_table_id)
                .send()
                .await,
            "DeleteRouteTable",
            "RouteTable",
            route_table_id,
        )?;
        Ok(())
    }

    async fn create_route(&self, request: CreateRouteRequest) -> Result<()> {
        let route_table_id = request.route_table_id().unwrap_or("<unknown>").to_string();

        ec2_result(
            self.create_route()
                .set_destination_prefix_list_id(request.destination_prefix_list_id)
                .set_vpc_endpoint_id(request.vpc_endpoint_id)
                .set_transit_gateway_id(request.transit_gateway_id)
                .set_local_gateway_id(request.local_gateway_id)
                .set_carrier_gateway_id(request.carrier_gateway_id)
                .set_core_network_arn(request.core_network_arn)
                .set_odb_network_arn(request.odb_network_arn)
                .set_dry_run(request.dry_run)
                .set_route_table_id(request.route_table_id)
                .set_destination_cidr_block(request.destination_cidr_block)
                .set_gateway_id(request.gateway_id)
                .set_destination_ipv6_cidr_block(request.destination_ipv6_cidr_block)
                .set_egress_only_internet_gateway_id(request.egress_only_internet_gateway_id)
                .set_instance_id(request.instance_id)
                .set_network_interface_id(request.network_interface_id)
                .set_vpc_peering_connection_id(request.vpc_peering_connection_id)
                .set_nat_gateway_id(request.nat_gateway_id)
                .send()
                .await,
            "CreateRoute",
            "Route",
            &route_table_id,
        )?;
        Ok(())
    }

    async fn associate_route_table(
        &self,
        request: AssociateRouteTableRequest,
    ) -> Result<AssociateRouteTableResponse> {
        let route_table_id = request.route_table_id().unwrap_or("<unknown>").to_string();

        ec2_result(
            self.associate_route_table()
                .set_gateway_id(request.gateway_id)
                .set_public_ipv4_pool(request.public_ipv4_pool)
                .set_dry_run(request.dry_run)
                .set_subnet_id(request.subnet_id)
                .set_route_table_id(request.route_table_id)
                .send()
                .await,
            "AssociateRouteTable",
            "RouteTableAssociation",
            &route_table_id,
        )
    }

    async fn disassociate_route_table(&self, association_id: &str) -> Result<()> {
        ec2_result(
            self.disassociate_route_table()
                .association_id(association_id)
                .send()
                .await,
            "DisassociateRouteTable",
            "RouteTableAssociation",
            association_id,
        )?;
        Ok(())
    }

    async fn describe_security_groups(
        &self,
        request: DescribeSecurityGroupsRequest,
    ) -> Result<DescribeSecurityGroupsResponse> {
        ec2_result(
            self.describe_security_groups()
                .set_group_ids(request.group_ids)
                .set_group_names(request.group_names)
                .set_next_token(request.next_token)
                .set_max_results(request.max_results)
                .set_dry_run(request.dry_run)
                .set_filters(request.filters)
                .send()
                .await,
            "DescribeSecurityGroups",
            "SecurityGroup",
            "*",
        )
    }

    async fn describe_network_interfaces(
        &self,
        request: DescribeNetworkInterfacesRequest,
    ) -> Result<DescribeNetworkInterfacesResponse> {
        ec2_result(
            self.describe_network_interfaces()
                .set_next_token(request.next_token)
                .set_max_results(request.max_results)
                .set_dry_run(request.dry_run)
                .set_network_interface_ids(request.network_interface_ids)
                .set_filters(request.filters)
                .send()
                .await,
            "DescribeNetworkInterfaces",
            "NetworkInterface",
            "*",
        )
    }

    async fn create_security_group(
        &self,
        request: CreateSecurityGroupRequest,
    ) -> Result<CreateSecurityGroupResponse> {
        let group_name = request.group_name().unwrap_or("<unknown>").to_string();

        ec2_result(
            self.create_security_group()
                .set_description(request.description)
                .set_group_name(request.group_name)
                .set_vpc_id(request.vpc_id)
                .set_tag_specifications(request.tag_specifications)
                .set_dry_run(request.dry_run)
                .send()
                .await,
            "CreateSecurityGroup",
            "SecurityGroup",
            &group_name,
        )
    }

    async fn delete_security_group(&self, group_id: &str) -> Result<()> {
        ec2_result(
            self.delete_security_group().group_id(group_id).send().await,
            "DeleteSecurityGroup",
            "SecurityGroup",
            group_id,
        )?;
        Ok(())
    }

    async fn authorize_security_group_ingress(
        &self,
        request: AuthorizeSecurityGroupIngressRequest,
    ) -> Result<()> {
        let group_id = request.group_id().unwrap_or("<unknown>").to_string();

        ec2_result(
            self.authorize_security_group_ingress()
                .set_cidr_ip(request.cidr_ip)
                .set_from_port(request.from_port)
                .set_group_id(request.group_id)
                .set_group_name(request.group_name)
                .set_ip_permissions(request.ip_permissions)
                .set_ip_protocol(request.ip_protocol)
                .set_source_security_group_name(request.source_security_group_name)
                .set_source_security_group_owner_id(request.source_security_group_owner_id)
                .set_to_port(request.to_port)
                .set_tag_specifications(request.tag_specifications)
                .set_dry_run(request.dry_run)
                .send()
                .await,
            "AuthorizeSecurityGroupIngress",
            "SecurityGroupRule",
            &group_id,
        )?;
        Ok(())
    }

    async fn authorize_security_group_egress(
        &self,
        request: AuthorizeSecurityGroupEgressRequest,
    ) -> Result<()> {
        let group_id = request.group_id().unwrap_or("<unknown>").to_string();

        ec2_result(
            self.authorize_security_group_egress()
                .set_tag_specifications(request.tag_specifications)
                .set_dry_run(request.dry_run)
                .set_group_id(request.group_id)
                .set_source_security_group_name(request.source_security_group_name)
                .set_source_security_group_owner_id(request.source_security_group_owner_id)
                .set_ip_protocol(request.ip_protocol)
                .set_from_port(request.from_port)
                .set_to_port(request.to_port)
                .set_cidr_ip(request.cidr_ip)
                .set_ip_permissions(request.ip_permissions)
                .send()
                .await,
            "AuthorizeSecurityGroupEgress",
            "SecurityGroupRule",
            &group_id,
        )?;
        Ok(())
    }

    async fn describe_availability_zones(
        &self,
        request: DescribeAvailabilityZonesRequest,
    ) -> Result<DescribeAvailabilityZonesResponse> {
        ec2_result(
            self.describe_availability_zones()
                .set_zone_names(request.zone_names)
                .set_zone_ids(request.zone_ids)
                .set_all_availability_zones(request.all_availability_zones)
                .set_dry_run(request.dry_run)
                .set_filters(request.filters)
                .send()
                .await,
            "DescribeAvailabilityZones",
            "AvailabilityZone",
            "*",
        )
    }
}

#[async_trait]
impl S3Api for S3Client {
    async fn create_bucket(&self, bucket_name: &str) -> Result<()> {
        let mut request = self.create_bucket().bucket(bucket_name);
        let region = self
            .config()
            .region()
            .map(|region| region.as_ref().to_string())
            .unwrap_or_else(|| "us-east-1".to_string());

        if region != "us-east-1" {
            let configuration = CreateBucketConfiguration::builder()
                .location_constraint(BucketLocationConstraint::from(region.as_str()))
                .build();
            request = request.create_bucket_configuration(configuration);
        }

        match request.send().await {
            Ok(_) => Ok(()),
            Err(err) if is_s3_create_bucket_already_owned(&err) => Ok(()),
            Err(err) => Err(err
                .into_alien_error()
                .context(ErrorData::CloudPlatformError {
                    message: format!("S3 CreateBucket API failed for bucket '{bucket_name}'"),
                    resource_id: None,
                })),
        }
    }

    async fn put_bucket_abac_tags(
        &self,
        bucket_name: &str,
        tags: &HashMap<String, String>,
    ) -> Result<()> {
        let tag_set = tags
            .iter()
            .map(|(key, value)| {
                S3Tag::builder()
                    .key(key)
                    .value(value)
                    .build()
                    .into_alien_error()
                    .context(ErrorData::CloudPlatformError {
                        message: format!("Failed to build S3 tag for bucket '{bucket_name}'"),
                        resource_id: None,
                    })
            })
            .collect::<Result<Vec<_>>>()?;
        let tagging = Tagging::builder()
            .set_tag_set(Some(tag_set))
            .build()
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to build S3 tagging for bucket '{bucket_name}'"),
                resource_id: None,
            })?;

        self.put_bucket_tagging()
            .bucket(bucket_name)
            .tagging(tagging)
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: format!("S3 PutBucketTagging API failed for bucket '{bucket_name}'"),
                resource_id: None,
            })?;

        Ok(())
    }

    async fn put_bucket_versioning(
        &self,
        bucket_name: &str,
        status: BucketVersioningStatus,
    ) -> Result<()> {
        let versioning_configuration = VersioningConfiguration::builder().status(status).build();

        self.put_bucket_versioning()
            .bucket(bucket_name)
            .versioning_configuration(versioning_configuration)
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: format!("S3 PutBucketVersioning API failed for bucket '{bucket_name}'"),
                resource_id: None,
            })?;

        Ok(())
    }

    async fn put_public_access_block(
        &self,
        bucket_name: &str,
        config: S3PublicAccessBlock,
    ) -> Result<()> {
        self.put_public_access_block()
            .bucket(bucket_name)
            .public_access_block_configuration(config)
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: format!("S3 PutPublicAccessBlock API failed for bucket '{bucket_name}'"),
                resource_id: None,
            })?;

        Ok(())
    }

    async fn put_bucket_policy(&self, bucket_name: &str, policy: &str) -> Result<()> {
        self.put_bucket_policy()
            .bucket(bucket_name)
            .policy(policy)
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: format!("S3 PutBucketPolicy API failed for bucket '{bucket_name}'"),
                resource_id: None,
            })?;

        Ok(())
    }

    async fn delete_bucket_policy(&self, bucket_name: &str) -> Result<()> {
        match self.delete_bucket_policy().bucket(bucket_name).send().await {
            Ok(_) => Ok(()),
            Err(err) if is_s3_delete_bucket_policy_not_found(&err) => Ok(()),
            Err(err) => Err(err
                .into_alien_error()
                .context(ErrorData::CloudPlatformError {
                    message: format!("S3 DeleteBucketPolicy API failed for bucket '{bucket_name}'"),
                    resource_id: None,
                })),
        }
    }

    async fn put_bucket_lifecycle_configuration(
        &self,
        bucket_name: &str,
        rules: Vec<S3LifecycleRule>,
    ) -> Result<()> {
        let configuration = S3BucketLifecycleConfiguration::builder()
            .set_rules(Some(rules))
            .build()
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to build S3 lifecycle configuration for bucket '{bucket_name}'"
                ),
                resource_id: None,
            })?;

        self.put_bucket_lifecycle_configuration()
            .bucket(bucket_name)
            .lifecycle_configuration(configuration)
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "S3 PutBucketLifecycleConfiguration API failed for bucket '{bucket_name}'"
                ),
                resource_id: None,
            })?;

        Ok(())
    }

    async fn delete_bucket_lifecycle(&self, bucket_name: &str) -> Result<()> {
        match self
            .delete_bucket_lifecycle()
            .bucket(bucket_name)
            .send()
            .await
        {
            Ok(_) => Ok(()),
            Err(err) if is_s3_delete_bucket_lifecycle_not_found(&err) => Ok(()),
            Err(err) => Err(err
                .into_alien_error()
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "S3 DeleteBucketLifecycle API failed for bucket '{bucket_name}'"
                    ),
                    resource_id: None,
                })),
        }
    }

    async fn get_bucket_metadata(&self, bucket_name: &str) -> Result<S3BucketMetadata> {
        let location = match self.get_bucket_location().bucket(bucket_name).send().await {
            Ok(output) => output,
            Err(err) => {
                return Err(err
                    .into_alien_error()
                    .context(ErrorData::CloudPlatformError {
                        message: format!(
                            "S3 GetBucketLocation API failed for bucket '{bucket_name}'"
                        ),
                        resource_id: None,
                    }));
            }
        };

        let versioning = match self
            .get_bucket_versioning()
            .bucket(bucket_name)
            .send()
            .await
        {
            Ok(output) => output,
            Err(err) => {
                return Err(err
                    .into_alien_error()
                    .context(ErrorData::CloudPlatformError {
                        message: format!(
                            "S3 GetBucketVersioning API failed for bucket '{bucket_name}'"
                        ),
                        resource_id: None,
                    }));
            }
        };

        let lifecycle_rule_count = match self
            .get_bucket_lifecycle_configuration()
            .bucket(bucket_name)
            .send()
            .await
        {
            Ok(output) => Some(output.rules().len() as u64),
            Err(err) if is_s3_get_lifecycle_not_found(&err) => None,
            Err(err) => {
                return Err(err
                    .into_alien_error()
                    .context(ErrorData::CloudPlatformError {
                        message: format!(
                        "S3 GetBucketLifecycleConfiguration API failed for bucket '{bucket_name}'"
                    ),
                        resource_id: None,
                    }));
            }
        };

        let encryption_rule_count = match self
            .get_bucket_encryption()
            .bucket(bucket_name)
            .send()
            .await
        {
            Ok(output) => output
                .server_side_encryption_configuration()
                .map(|configuration| configuration.rules().len() as u64),
            Err(err) if is_s3_get_encryption_not_found(&err) => None,
            Err(err) => {
                return Err(err
                    .into_alien_error()
                    .context(ErrorData::CloudPlatformError {
                        message: format!(
                            "S3 GetBucketEncryption API failed for bucket '{bucket_name}'"
                        ),
                        resource_id: None,
                    }));
            }
        };

        let public_access_block = match self
            .get_public_access_block()
            .bucket(bucket_name)
            .send()
            .await
        {
            Ok(output) => output.public_access_block_configuration,
            Err(err) if is_s3_get_public_access_block_not_found(&err) => None,
            Err(err) => {
                return Err(err
                    .into_alien_error()
                    .context(ErrorData::CloudPlatformError {
                        message: format!(
                            "S3 GetPublicAccessBlock API failed for bucket '{bucket_name}'"
                        ),
                        resource_id: None,
                    }));
            }
        };

        let bucket_policy_present = match self.get_bucket_policy().bucket(bucket_name).send().await
        {
            Ok(output) => Some(
                output
                    .policy()
                    .is_some_and(|policy| !policy.trim().is_empty()),
            ),
            Err(err) if is_s3_get_bucket_policy_not_found(&err) => Some(false),
            Err(err) => {
                return Err(err
                    .into_alien_error()
                    .context(ErrorData::CloudPlatformError {
                        message: format!(
                            "S3 GetBucketPolicy API failed for bucket '{bucket_name}'"
                        ),
                        resource_id: None,
                    }));
            }
        };

        let bucket_acl_present = match self.get_bucket_acl().bucket(bucket_name).send().await {
            Ok(output) => Some(output.owner().is_some() || !output.grants().is_empty()),
            Err(err) if is_s3_get_bucket_acl_not_found(&err) => Some(false),
            Err(err) => {
                return Err(err
                    .into_alien_error()
                    .context(ErrorData::CloudPlatformError {
                        message: format!("S3 GetBucketAcl API failed for bucket '{bucket_name}'"),
                        resource_id: None,
                    }));
            }
        };

        Ok(S3BucketMetadata {
            region: s3_bucket_location_region(location.location_constraint().map(|c| c.as_str())),
            versioning_status: versioning.status().cloned(),
            lifecycle_rule_count,
            encryption_rule_count,
            public_access_block,
            bucket_policy_present,
            bucket_acl_present,
        })
    }

    async fn empty_bucket(&self, bucket_name: &str) -> Result<()> {
        let mut key_marker = None;
        let mut version_id_marker = None;

        loop {
            match self
                .list_object_versions()
                .bucket(bucket_name)
                .set_key_marker(key_marker.clone())
                .set_version_id_marker(version_id_marker.clone())
                .max_keys(1000)
                .send()
                .await
            {
                Ok(output) => {
                    let mut objects =
                        Vec::with_capacity(output.versions().len() + output.delete_markers().len());
                    for version in output.versions() {
                        if let (Some(key), Some(version_id)) = (version.key(), version.version_id())
                        {
                            objects.push(s3_object_identifier(key, Some(version_id))?);
                        }
                    }
                    for marker in output.delete_markers() {
                        if let (Some(key), Some(version_id)) = (marker.key(), marker.version_id()) {
                            objects.push(s3_object_identifier(key, Some(version_id))?);
                        }
                    }

                    if !objects.is_empty() {
                        delete_s3_objects(self, bucket_name, objects).await?;
                    }

                    if output.is_truncated().unwrap_or(false) {
                        key_marker = output.next_key_marker().map(ToString::to_string);
                        version_id_marker =
                            output.next_version_id_marker().map(ToString::to_string);
                        continue;
                    }

                    break;
                }
                Err(err) if is_s3_list_versions_bucket_not_found(&err) => return Ok(()),
                Err(err) if is_s3_list_versions_invalid_argument(&err) => break,
                Err(err) => {
                    return Err(err
                        .into_alien_error()
                        .context(ErrorData::CloudPlatformError {
                            message: format!(
                                "S3 ListObjectVersions API failed for bucket '{bucket_name}'"
                            ),
                            resource_id: None,
                        }));
                }
            }
        }

        let mut continuation_token = None;
        loop {
            let output = match self
                .list_objects_v2()
                .bucket(bucket_name)
                .set_continuation_token(continuation_token.clone())
                .max_keys(1000)
                .send()
                .await
            {
                Ok(output) => output,
                Err(err) if is_s3_list_objects_bucket_not_found(&err) => return Ok(()),
                Err(err) => {
                    return Err(err
                        .into_alien_error()
                        .context(ErrorData::CloudPlatformError {
                            message: format!(
                                "S3 ListObjectsV2 API failed for bucket '{bucket_name}'"
                            ),
                            resource_id: None,
                        }));
                }
            };

            let objects = output
                .contents()
                .iter()
                .filter_map(|object| object.key())
                .map(|key| s3_object_identifier(key, None))
                .collect::<Result<Vec<_>>>()?;

            if !objects.is_empty() {
                delete_s3_objects(self, bucket_name, objects).await?;
            }

            if output.is_truncated().unwrap_or(false) {
                continuation_token = output.next_continuation_token().map(ToString::to_string);
            } else {
                break;
            }
        }

        Ok(())
    }

    async fn delete_bucket(&self, bucket_name: &str) -> Result<bool> {
        match self.delete_bucket().bucket(bucket_name).send().await {
            Ok(_) => Ok(true),
            Err(err) if is_s3_delete_bucket_not_found(&err) => Ok(false),
            Err(err) => Err(err
                .into_alien_error()
                .context(ErrorData::CloudPlatformError {
                    message: format!("S3 DeleteBucket API failed for bucket '{bucket_name}'"),
                    resource_id: None,
                })),
        }
    }

    async fn get_bucket_notification_configuration(
        &self,
        bucket_name: &str,
    ) -> Result<NotificationConfiguration> {
        match self
            .get_bucket_notification_configuration()
            .bucket(bucket_name)
            .send()
            .await
        {
            Ok(output) => Ok(notification_configuration_from_get_output(output)),
            Err(err) if is_s3_get_notification_not_found(&err) => {
                Ok(NotificationConfiguration::builder().build())
            }
            Err(err) => Err(err
                .into_alien_error()
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                    "S3 GetBucketNotificationConfiguration API failed for bucket '{bucket_name}'"
                ),
                    resource_id: None,
                })),
        }
    }

    async fn put_bucket_notification_configuration(
        &self,
        bucket_name: &str,
        config: &NotificationConfiguration,
    ) -> Result<()> {
        self.put_bucket_notification_configuration()
            .bucket(bucket_name)
            .notification_configuration(config.clone())
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "S3 PutBucketNotificationConfiguration API failed for bucket '{bucket_name}'"
                ),
                resource_id: None,
            })?;

        Ok(())
    }
}

fn ec2_result<T, E>(
    result: std::result::Result<T, aws_sdk_ec2::error::SdkError<E>>,
    operation: &str,
    resource_type: &str,
    resource_name: &str,
) -> Result<T>
where
    E: ProvideErrorMetadata + std::error::Error + Send + Sync + 'static,
{
    match result {
        Ok(value) => Ok(value),
        Err(error) => {
            if let Some(service_error) = error.as_service_error() {
                if let Some(error_data) = ec2_error_data(
                    service_error.code(),
                    service_error.message(),
                    operation,
                    resource_type,
                    resource_name,
                ) {
                    return Err(AlienError::new(error_data));
                }
            }

            Err(error
                .into_alien_error()
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "EC2 {operation} API failed for {resource_type} '{resource_name}'"
                    ),
                    resource_id: None,
                }))
        }
    }
}

fn ec2_error_data(
    code: Option<&str>,
    message: Option<&str>,
    operation: &str,
    resource_type: &str,
    resource_name: &str,
) -> Option<ErrorData> {
    let code = code?;
    let message = message.unwrap_or(code);

    let not_found_resource_type = match code {
        "InvalidVpcID.NotFound" | "InvalidVpc.NotFound" => Some("VPC"),
        "InvalidSubnetID.NotFound" | "InvalidSubnet.NotFound" => Some("Subnet"),
        "InvalidInternetGatewayID.NotFound" | "InvalidInternetGateway.NotFound" => {
            Some("InternetGateway")
        }
        "InvalidNatGatewayID.NotFound" | "NatGatewayNotFound" => Some("NatGateway"),
        "InvalidRouteTableID.NotFound" | "InvalidRouteTableId.NotFound" => Some("RouteTable"),
        "InvalidGroup.NotFound" | "InvalidSecurityGroupID.NotFound" => Some("SecurityGroup"),
        "InvalidAllocationID.NotFound" | "InvalidAddress.NotFound" => Some("ElasticIP"),
        "InvalidAssociationID.NotFound" => Some("RouteTableAssociation"),
        _ => None,
    };
    if let Some(resource_type) = not_found_resource_type {
        return Some(ErrorData::CloudResourceNotFound {
            resource_type: resource_type.to_string(),
            resource_name: resource_name.to_string(),
        });
    }

    match code {
        "InvalidGroup.Duplicate"
        | "InvalidPermission.Duplicate"
        | "DependencyViolation"
        | "ResourceInUse"
        | "Gateway.NotAttached"
        | "RouteAlreadyExists" => Some(ErrorData::CloudResourceConflict {
            resource_type: resource_type.to_string(),
            resource_name: resource_name.to_string(),
            message: format!("{operation} reported {code}: {message}"),
        }),
        _ => None,
    }
}

fn iam_result<T, E>(
    result: std::result::Result<T, aws_sdk_iam::error::SdkError<E>>,
    operation: &str,
    resource_type: &str,
    resource_name: &str,
) -> Result<T>
where
    E: ProvideErrorMetadata + std::error::Error + Send + Sync + 'static,
{
    match result {
        Ok(value) => Ok(value),
        Err(error) => {
            if let Some(service_error) = error.as_service_error() {
                match service_error.code() {
                    Some("NoSuchEntity") => {
                        return Err(AlienError::new(ErrorData::CloudResourceNotFound {
                            resource_type: resource_type.to_string(),
                            resource_name: resource_name.to_string(),
                        }));
                    }
                    Some("EntityAlreadyExists") => {
                        return Err(AlienError::new(ErrorData::CloudResourceConflict {
                            resource_type: resource_type.to_string(),
                            resource_name: resource_name.to_string(),
                            message: format!("{operation} reported EntityAlreadyExists"),
                        }));
                    }
                    _ => {}
                }
            }

            Err(error
                .into_alien_error()
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "IAM {operation} API failed for {resource_type} '{resource_name}'"
                    ),
                    resource_id: None,
                }))
        }
    }
}

fn is_s3_create_bucket_already_owned(
    error: &aws_sdk_s3::error::SdkError<CreateBucketError>,
) -> bool {
    error
        .as_service_error()
        .is_some_and(CreateBucketError::is_bucket_already_owned_by_you)
}

fn is_s3_delete_bucket_not_found(error: &aws_sdk_s3::error::SdkError<DeleteBucketError>) -> bool {
    s3_error_code(error.as_service_error(), &["NoSuchBucket"])
}

fn is_s3_delete_bucket_policy_not_found(
    error: &aws_sdk_s3::error::SdkError<DeleteBucketPolicyError>,
) -> bool {
    s3_error_code(
        error.as_service_error(),
        &["NoSuchBucket", "NoSuchBucketPolicy"],
    )
}

fn is_s3_delete_bucket_lifecycle_not_found(
    error: &aws_sdk_s3::error::SdkError<DeleteBucketLifecycleError>,
) -> bool {
    s3_error_code(
        error.as_service_error(),
        &["NoSuchBucket", "NoSuchLifecycleConfiguration"],
    )
}

fn is_s3_get_lifecycle_not_found(
    error: &aws_sdk_s3::error::SdkError<GetBucketLifecycleConfigurationError>,
) -> bool {
    s3_error_code(
        error.as_service_error(),
        &["NoSuchBucket", "NoSuchLifecycleConfiguration"],
    )
}

fn is_s3_get_encryption_not_found(
    error: &aws_sdk_s3::error::SdkError<GetBucketEncryptionError>,
) -> bool {
    s3_error_code(
        error.as_service_error(),
        &[
            "NoSuchBucket",
            "ServerSideEncryptionConfigurationNotFoundError",
        ],
    )
}

fn is_s3_get_public_access_block_not_found(
    error: &aws_sdk_s3::error::SdkError<GetPublicAccessBlockError>,
) -> bool {
    s3_error_code(
        error.as_service_error(),
        &["NoSuchBucket", "NoSuchPublicAccessBlockConfiguration"],
    )
}

fn is_s3_get_bucket_policy_not_found(
    error: &aws_sdk_s3::error::SdkError<GetBucketPolicyError>,
) -> bool {
    s3_error_code(
        error.as_service_error(),
        &["NoSuchBucket", "NoSuchBucketPolicy"],
    )
}

fn is_s3_get_bucket_acl_not_found(error: &aws_sdk_s3::error::SdkError<GetBucketAclError>) -> bool {
    s3_error_code(error.as_service_error(), &["NoSuchBucket"])
}

fn is_s3_get_notification_not_found(
    error: &aws_sdk_s3::error::SdkError<GetBucketNotificationConfigurationError>,
) -> bool {
    s3_error_code(error.as_service_error(), &["NoSuchBucket"])
}

fn is_s3_list_versions_bucket_not_found(
    error: &aws_sdk_s3::error::SdkError<ListObjectVersionsError>,
) -> bool {
    s3_error_code(error.as_service_error(), &["NoSuchBucket"])
}

fn is_s3_list_versions_invalid_argument(
    error: &aws_sdk_s3::error::SdkError<ListObjectVersionsError>,
) -> bool {
    s3_error_code(error.as_service_error(), &["InvalidArgument"])
}

fn is_s3_list_objects_bucket_not_found(
    error: &aws_sdk_s3::error::SdkError<ListObjectsV2Error>,
) -> bool {
    error
        .as_service_error()
        .is_some_and(ListObjectsV2Error::is_no_such_bucket)
}

fn s3_error_code<E>(error: Option<&E>, codes: &[&str]) -> bool
where
    E: ProvideErrorMetadata,
{
    error
        .and_then(ProvideErrorMetadata::code)
        .is_some_and(|code| codes.contains(&code))
}

fn s3_bucket_location_region(location_constraint: Option<&str>) -> String {
    match location_constraint {
        None | Some("") => "us-east-1".to_string(),
        Some("EU") => "eu-west-1".to_string(),
        Some(region) => region.to_string(),
    }
}

fn s3_object_identifier(key: &str, version_id: Option<&str>) -> Result<ObjectIdentifier> {
    ObjectIdentifier::builder()
        .key(key)
        .set_version_id(version_id.map(ToString::to_string))
        .build()
        .into_alien_error()
        .context(ErrorData::CloudPlatformError {
            message: format!("Failed to build S3 object identifier for key '{key}'"),
            resource_id: None,
        })
}

async fn delete_s3_objects(
    client: &S3Client,
    bucket_name: &str,
    objects: Vec<ObjectIdentifier>,
) -> Result<()> {
    let delete = Delete::builder()
        .set_objects(Some(objects))
        .quiet(true)
        .build()
        .into_alien_error()
        .context(ErrorData::CloudPlatformError {
            message: format!("Failed to build S3 DeleteObjects request for '{bucket_name}'"),
            resource_id: None,
        })?;

    client
        .delete_objects()
        .bucket(bucket_name)
        .delete(delete)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::CloudPlatformError {
            message: format!("S3 DeleteObjects API failed for bucket '{bucket_name}'"),
            resource_id: None,
        })?;

    Ok(())
}

fn notification_configuration_from_get_output(
    output: aws_sdk_s3::operation::get_bucket_notification_configuration::GetBucketNotificationConfigurationOutput,
) -> NotificationConfiguration {
    NotificationConfiguration::builder()
        .set_topic_configurations(output.topic_configurations)
        .set_queue_configurations(output.queue_configurations)
        .set_lambda_function_configurations(output.lambda_function_configurations)
        .set_event_bridge_configuration(output.event_bridge_configuration)
        .build()
}

/// Build an official AWS SDK config from Alien's public AWS client config.
pub async fn sdk_config_from_alien_config(config: &AwsClientConfig) -> Result<SdkConfig> {
    let region = Region::new(config.region.clone());
    let loader = aws_config::defaults(BehaviorVersion::latest()).region(region.clone());

    let loader = match &config.credentials {
        AwsCredentials::AccessKeys {
            access_key_id,
            secret_access_key,
            session_token,
        } => loader.credentials_provider(Credentials::new(
            access_key_id,
            secret_access_key,
            session_token.clone(),
            None,
            "AlienAccessKeys",
        )),
        AwsCredentials::SessionCredentials {
            access_key_id,
            secret_access_key,
            session_token,
            expires_at,
        } => {
            let expires_after = chrono::DateTime::parse_from_rfc3339(expires_at)
                .map(|expires_at| expires_at.to_utc().into())
                .into_alien_error()
                .context(ErrorData::ClientConfigInvalid {
                    platform: Platform::Aws,
                    message: format!("Invalid AWS credential expiration timestamp: {expires_at}"),
                })?;

            loader.credentials_provider(Credentials::new(
                access_key_id,
                secret_access_key,
                Some(session_token.clone()),
                Some(expires_after),
                "AlienSessionCredentials",
            ))
        }
        AwsCredentials::Profile { name } => loader.profile_name(name),
        AwsCredentials::WebIdentity { config } => {
            let provider_config = aws_config::provider_config::ProviderConfig::without_region()
                .with_region(Some(region));
            let provider =
                aws_config::web_identity_token::WebIdentityTokenCredentialsProvider::builder()
                    .configure(&provider_config)
                    .static_configuration(aws_config::web_identity_token::StaticConfiguration {
                        web_identity_token_file: config.web_identity_token_file.clone().into(),
                        role_arn: config.role_arn.clone(),
                        session_name: config
                            .session_name
                            .clone()
                            .unwrap_or_else(|| "alien-web-identity".to_string()),
                    })
                    .build();
            loader.credentials_provider(provider)
        }
        AwsCredentials::Imds { endpoint } => {
            let provider_config = aws_config::provider_config::ProviderConfig::without_region()
                .with_region(Some(region));
            let mut client_builder =
                aws_config::imds::Client::builder().configure(&provider_config);
            if let Some(endpoint) = endpoint {
                client_builder = client_builder.endpoint(endpoint).map_err(|err| {
                    AlienError::new(ErrorData::ClientConfigInvalid {
                        platform: Platform::Aws,
                        message: format!("Invalid AWS IMDS endpoint override '{endpoint}': {err}"),
                    })
                })?;
            }
            let imds_client = client_builder.build();
            let provider = aws_config::imds::credentials::ImdsCredentialsProvider::builder()
                .configure(&provider_config)
                .imds_client(imds_client)
                .build();
            loader.credentials_provider(provider)
        }
    };

    Ok(loader.load().await)
}

/// Create an official AWS SDK CodeBuild client with Alien endpoint override support.
pub async fn codebuild_client_from_alien_config(
    config: &AwsClientConfig,
) -> Result<CodeBuildClient> {
    let sdk_config = sdk_config_from_alien_config(config).await?;
    let mut codebuild_config = aws_sdk_codebuild::config::Builder::from(&sdk_config);

    if let Some(endpoint) = config
        .service_overrides
        .as_ref()
        .and_then(|overrides| overrides.endpoints.get("codebuild"))
    {
        codebuild_config = codebuild_config.endpoint_url(endpoint);
    }

    Ok(CodeBuildClient::from_conf(codebuild_config.build()))
}

/// Create an official AWS SDK ACM client with Alien endpoint override support.
pub async fn acm_client_from_alien_config(config: &AwsClientConfig) -> Result<AcmClient> {
    let sdk_config = sdk_config_from_alien_config(config).await?;
    let mut acm_config = aws_sdk_acm::config::Builder::from(&sdk_config);

    if let Some(endpoint) = config
        .service_overrides
        .as_ref()
        .and_then(|overrides| overrides.endpoints.get("acm"))
    {
        acm_config = acm_config.endpoint_url(endpoint);
    }

    Ok(AcmClient::from_conf(acm_config.build()))
}

/// Create an official AWS SDK Lambda client with Alien endpoint override support.
pub async fn lambda_client_from_alien_config(config: &AwsClientConfig) -> Result<LambdaClient> {
    let sdk_config = sdk_config_from_alien_config(config).await?;
    let mut lambda_config = aws_sdk_lambda::config::Builder::from(&sdk_config);

    if let Some(endpoint) = config
        .service_overrides
        .as_ref()
        .and_then(|overrides| overrides.endpoints.get("lambda"))
    {
        lambda_config = lambda_config.endpoint_url(endpoint);
    }

    Ok(LambdaClient::from_conf(lambda_config.build()))
}

/// Create an official AWS SDK API Gateway V2 client with Alien endpoint override support.
pub async fn apigatewayv2_client_from_alien_config(
    config: &AwsClientConfig,
) -> Result<ApiGatewayV2Client> {
    let sdk_config = sdk_config_from_alien_config(config).await?;
    let mut apigatewayv2_config = aws_sdk_apigatewayv2::config::Builder::from(&sdk_config);

    if let Some(endpoint) = config
        .service_overrides
        .as_ref()
        .and_then(|overrides| overrides.endpoints.get("apigateway"))
    {
        apigatewayv2_config = apigatewayv2_config.endpoint_url(endpoint);
    }

    Ok(ApiGatewayV2Client::from_conf(apigatewayv2_config.build()))
}

/// Create an official AWS SDK EventBridge client with Alien endpoint override support.
pub async fn eventbridge_client_from_alien_config(
    config: &AwsClientConfig,
) -> Result<EventBridgeClient> {
    let sdk_config = sdk_config_from_alien_config(config).await?;
    let mut eventbridge_config = aws_sdk_eventbridge::config::Builder::from(&sdk_config);

    if let Some(endpoint) = config
        .service_overrides
        .as_ref()
        .and_then(|overrides| overrides.endpoints.get("events"))
    {
        eventbridge_config = eventbridge_config.endpoint_url(endpoint);
    }

    Ok(EventBridgeClient::from_conf(eventbridge_config.build()))
}

/// Create an official AWS SDK EC2 client with Alien endpoint override support.
pub async fn ec2_client_from_alien_config(config: &AwsClientConfig) -> Result<Ec2Client> {
    let sdk_config = sdk_config_from_alien_config(config).await?;
    let mut ec2_config = aws_sdk_ec2::config::Builder::from(&sdk_config);

    if let Some(endpoint) = config
        .service_overrides
        .as_ref()
        .and_then(|overrides| overrides.endpoints.get("ec2"))
    {
        ec2_config = ec2_config.endpoint_url(endpoint);
    }

    Ok(Ec2Client::from_conf(ec2_config.build()))
}

/// Create an official AWS SDK ECR client with Alien endpoint override support.
pub async fn ecr_client_from_alien_config(config: &AwsClientConfig) -> Result<EcrClient> {
    let sdk_config = sdk_config_from_alien_config(config).await?;
    let mut ecr_config = aws_sdk_ecr::config::Builder::from(&sdk_config);

    if let Some(endpoint) = config
        .service_overrides
        .as_ref()
        .and_then(|overrides| overrides.endpoints.get("ecr"))
    {
        ecr_config = ecr_config.endpoint_url(endpoint);
    }

    Ok(EcrClient::from_conf(ecr_config.build()))
}

/// Create an official AWS SDK IAM client with Alien endpoint override support.
pub async fn iam_client_from_alien_config(config: &AwsClientConfig) -> Result<IamClient> {
    let sdk_config = sdk_config_from_alien_config(config).await?;
    let mut iam_config = aws_sdk_iam::config::Builder::from(&sdk_config);

    if let Some(endpoint) = config
        .service_overrides
        .as_ref()
        .and_then(|overrides| overrides.endpoints.get("iam"))
    {
        iam_config = iam_config.endpoint_url(endpoint);
    }

    Ok(IamClient::from_conf(iam_config.build()))
}

/// Create an official AWS SDK SSM client with Alien endpoint override support.
pub async fn ssm_client_from_alien_config(config: &AwsClientConfig) -> Result<aws_sdk_ssm::Client> {
    let sdk_config = sdk_config_from_alien_config(config).await?;
    let mut ssm_config = aws_sdk_ssm::config::Builder::from(&sdk_config);

    if let Some(endpoint) = config
        .service_overrides
        .as_ref()
        .and_then(|overrides| overrides.endpoints.get("ssm"))
    {
        ssm_config = ssm_config.endpoint_url(endpoint);
    }

    Ok(aws_sdk_ssm::Client::from_conf(ssm_config.build()))
}

/// Create an official AWS SDK DynamoDB client with Alien endpoint override support.
pub async fn dynamodb_client_from_alien_config(config: &AwsClientConfig) -> Result<DynamoDbClient> {
    let sdk_config = sdk_config_from_alien_config(config).await?;
    let mut dynamodb_config = aws_sdk_dynamodb::config::Builder::from(&sdk_config);

    if let Some(endpoint) = config
        .service_overrides
        .as_ref()
        .and_then(|overrides| overrides.endpoints.get("dynamodb"))
    {
        dynamodb_config = dynamodb_config.endpoint_url(endpoint);
    }

    Ok(DynamoDbClient::from_conf(dynamodb_config.build()))
}

/// Create an official AWS SDK SQS client with Alien endpoint override support.
pub async fn sqs_client_from_alien_config(config: &AwsClientConfig) -> Result<SqsClient> {
    let sdk_config = sdk_config_from_alien_config(config).await?;
    let mut sqs_config = aws_sdk_sqs::config::Builder::from(&sdk_config);

    if let Some(endpoint) = config
        .service_overrides
        .as_ref()
        .and_then(|overrides| overrides.endpoints.get("sqs"))
    {
        sqs_config = sqs_config.endpoint_url(endpoint);
    }

    Ok(SqsClient::from_conf(sqs_config.build()))
}

/// Create an official AWS SDK S3 client with Alien endpoint override support.
pub async fn s3_client_from_alien_config(config: &AwsClientConfig) -> Result<S3Client> {
    let sdk_config = sdk_config_from_alien_config(config).await?;
    let mut s3_config = aws_sdk_s3::config::Builder::from(&sdk_config);

    if let Some(endpoint) = config
        .service_overrides
        .as_ref()
        .and_then(|overrides| overrides.endpoints.get("s3"))
    {
        s3_config = s3_config.endpoint_url(endpoint);
    }

    Ok(S3Client::from_conf(s3_config.build()))
}
