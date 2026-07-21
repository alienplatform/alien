use super::types::*;
use alien_client_core::Result;
use async_trait::async_trait;

#[cfg(feature = "test-utils")]
use mockall::automock;

// ---------------------------------------------------------------------------
// EC2 API Trait
// ---------------------------------------------------------------------------

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
pub trait Ec2Api: Send + Sync + std::fmt::Debug {
    // VPC Operations
    async fn describe_vpcs(&self, request: DescribeVpcsRequest) -> Result<DescribeVpcsResponse>;
    async fn describe_vpc_attribute(
        &self,
        request: DescribeVpcAttributeRequest,
    ) -> Result<DescribeVpcAttributeResponse>;
    async fn create_vpc(&self, request: CreateVpcRequest) -> Result<CreateVpcResponse>;
    async fn delete_vpc(&self, vpc_id: &str) -> Result<()>;
    async fn modify_vpc_attribute(&self, request: ModifyVpcAttributeRequest) -> Result<()>;

    // Subnet Operations
    async fn describe_subnets(
        &self,
        request: DescribeSubnetsRequest,
    ) -> Result<DescribeSubnetsResponse>;
    async fn create_subnet(&self, request: CreateSubnetRequest) -> Result<CreateSubnetResponse>;
    async fn delete_subnet(&self, subnet_id: &str) -> Result<()>;

    // Internet Gateway Operations
    async fn create_internet_gateway(
        &self,
        request: CreateInternetGatewayRequest,
    ) -> Result<CreateInternetGatewayResponse>;
    async fn delete_internet_gateway(&self, internet_gateway_id: &str) -> Result<()>;
    async fn attach_internet_gateway(&self, request: AttachInternetGatewayRequest) -> Result<()>;
    async fn detach_internet_gateway(&self, request: DetachInternetGatewayRequest) -> Result<()>;
    async fn describe_internet_gateways(
        &self,
        request: DescribeInternetGatewaysRequest,
    ) -> Result<DescribeInternetGatewaysResponse>;

    // NAT Gateway Operations
    async fn create_nat_gateway(
        &self,
        request: CreateNatGatewayRequest,
    ) -> Result<CreateNatGatewayResponse>;
    async fn delete_nat_gateway(&self, nat_gateway_id: &str) -> Result<DeleteNatGatewayResponse>;
    async fn describe_nat_gateways(
        &self,
        request: DescribeNatGatewaysRequest,
    ) -> Result<DescribeNatGatewaysResponse>;

    // Elastic IP Operations
    async fn allocate_address(
        &self,
        request: AllocateAddressRequest,
    ) -> Result<AllocateAddressResponse>;
    async fn release_address(&self, allocation_id: &str) -> Result<()>;

    // Route Table Operations
    async fn describe_route_tables(
        &self,
        request: DescribeRouteTablesRequest,
    ) -> Result<DescribeRouteTablesResponse>;
    async fn create_route_table(
        &self,
        request: CreateRouteTableRequest,
    ) -> Result<CreateRouteTableResponse>;
    async fn delete_route_table(&self, route_table_id: &str) -> Result<()>;
    async fn create_route(&self, request: CreateRouteRequest) -> Result<()>;
    async fn delete_route(&self, request: DeleteRouteRequest) -> Result<()>;
    async fn associate_route_table(
        &self,
        request: AssociateRouteTableRequest,
    ) -> Result<AssociateRouteTableResponse>;
    async fn disassociate_route_table(&self, association_id: &str) -> Result<()>;

    // Security Group Operations
    async fn describe_security_groups(
        &self,
        request: DescribeSecurityGroupsRequest,
    ) -> Result<DescribeSecurityGroupsResponse>;
    async fn describe_network_interfaces(
        &self,
        request: DescribeNetworkInterfacesRequest,
    ) -> Result<DescribeNetworkInterfacesResponse>;
    async fn create_security_group(
        &self,
        request: CreateSecurityGroupRequest,
    ) -> Result<CreateSecurityGroupResponse>;
    async fn delete_security_group(&self, group_id: &str) -> Result<()>;
    async fn authorize_security_group_ingress(
        &self,
        request: AuthorizeSecurityGroupIngressRequest,
    ) -> Result<()>;
    async fn authorize_security_group_egress(
        &self,
        request: AuthorizeSecurityGroupEgressRequest,
    ) -> Result<()>;
    async fn revoke_security_group_ingress(
        &self,
        request: RevokeSecurityGroupIngressRequest,
    ) -> Result<()>;
    async fn revoke_security_group_egress(
        &self,
        request: RevokeSecurityGroupEgressRequest,
    ) -> Result<()>;

    // Availability Zone Operations
    async fn describe_availability_zones(
        &self,
        request: DescribeAvailabilityZonesRequest,
    ) -> Result<DescribeAvailabilityZonesResponse>;

    // AMI Operations
    async fn describe_images(
        &self,
        request: DescribeImagesRequest,
    ) -> Result<DescribeImagesResponse>;

    // Instance Operations
    async fn terminate_instances(
        &self,
        instance_ids: Vec<String>,
    ) -> Result<TerminateInstancesResponse>;
    async fn describe_instances(
        &self,
        request: DescribeInstancesRequest,
    ) -> Result<DescribeInstancesResponse>;

    // Volume Operations
    async fn create_volume(&self, request: CreateVolumeRequest) -> Result<CreateVolumeResponse>;
    async fn modify_volume(&self, request: ModifyVolumeRequest) -> Result<ModifyVolumeResponse>;
    async fn describe_volumes_modifications(
        &self,
        request: DescribeVolumesModificationsRequest,
    ) -> Result<DescribeVolumesModificationsResponse>;
    async fn delete_volume(&self, volume_id: &str) -> Result<()>;
    async fn describe_volumes(
        &self,
        request: DescribeVolumesRequest,
    ) -> Result<DescribeVolumesResponse>;
    async fn attach_volume(&self, request: AttachVolumeRequest) -> Result<AttachVolumeResponse>;
    async fn detach_volume(&self, request: DetachVolumeRequest) -> Result<DetachVolumeResponse>;

    // Launch Template Operations
    async fn create_launch_template(
        &self,
        request: CreateLaunchTemplateRequest,
    ) -> Result<CreateLaunchTemplateResponse>;
    /// Creates a new version of an existing launch template with updated launch template data.
    /// ASGs using $Latest will automatically pick up the new version.
    /// See: https://docs.aws.amazon.com/AWSEC2/latest/APIReference/API_CreateLaunchTemplateVersion.html
    async fn create_launch_template_version(
        &self,
        request: CreateLaunchTemplateVersionRequest,
    ) -> Result<CreateLaunchTemplateVersionResponse>;
    async fn delete_launch_template(
        &self,
        request: DeleteLaunchTemplateRequest,
    ) -> Result<DeleteLaunchTemplateResponse>;
    async fn describe_launch_templates(
        &self,
        request: DescribeLaunchTemplatesRequest,
    ) -> Result<DescribeLaunchTemplatesResponse>;

    // Console Output
    /// Gets the console output for an EC2 instance (base64-encoded).
    /// See: https://docs.aws.amazon.com/AWSEC2/latest/APIReference/API_GetConsoleOutput.html
    async fn get_console_output(&self, instance_id: String) -> Result<GetConsoleOutputResponse>;
}
