use super::*;

impl Ec2Client {
    pub(super) async fn describe_vpcs_impl(
        &self,
        request: DescribeVpcsRequest,
    ) -> Result<DescribeVpcsResponse> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "DescribeVpcs".to_string());
        form_data.insert("Version".to_string(), "2016-11-15".to_string());

        if let Some(vpc_ids) = &request.vpc_ids {
            for (i, vpc_id) in vpc_ids.iter().enumerate() {
                form_data.insert(format!("VpcId.{}", i + 1), vpc_id.clone());
            }
        }

        if let Some(filters) = &request.filters {
            Self::add_filters(&mut form_data, filters);
        }

        if let Some(max_results) = request.max_results {
            form_data.insert("MaxResults".to_string(), max_results.to_string());
        }

        if let Some(next_token) = &request.next_token {
            form_data.insert("NextToken".to_string(), next_token.clone());
        }

        self.send_form(form_data, "DescribeVpcs", "VPC").await
    }

    pub(super) async fn describe_vpc_attribute_impl(
        &self,
        request: DescribeVpcAttributeRequest,
    ) -> Result<DescribeVpcAttributeResponse> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "DescribeVpcAttribute".to_string());
        form_data.insert("Version".to_string(), "2016-11-15".to_string());
        form_data.insert("VpcId".to_string(), request.vpc_id.clone());
        form_data.insert("Attribute".to_string(), request.attribute.clone());

        self.send_form(form_data, "DescribeVpcAttribute", &request.vpc_id)
            .await
    }

    pub(super) async fn create_vpc_impl(
        &self,
        request: CreateVpcRequest,
    ) -> Result<CreateVpcResponse> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "CreateVpc".to_string());
        form_data.insert("Version".to_string(), "2016-11-15".to_string());
        form_data.insert("CidrBlock".to_string(), request.cidr_block.clone());

        if let Some(instance_tenancy) = &request.instance_tenancy {
            form_data.insert("InstanceTenancy".to_string(), instance_tenancy.clone());
        }

        if let Some(amazon_provided_ipv6_cidr_block) = request.amazon_provided_ipv6_cidr_block {
            form_data.insert(
                "AmazonProvidedIpv6CidrBlock".to_string(),
                amazon_provided_ipv6_cidr_block.to_string(),
            );
        }

        if let Some(tag_specs) = &request.tag_specifications {
            Self::add_tag_specifications(&mut form_data, tag_specs);
        }

        self.send_form(form_data, "CreateVpc", &request.cidr_block)
            .await
    }

    pub(super) async fn delete_vpc_impl(&self, vpc_id: &str) -> Result<()> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "DeleteVpc".to_string());
        form_data.insert("Version".to_string(), "2016-11-15".to_string());
        form_data.insert("VpcId".to_string(), vpc_id.to_string());

        self.send_form_no_body(form_data, "DeleteVpc", vpc_id).await
    }

    pub(super) async fn modify_vpc_attribute_impl(
        &self,
        request: ModifyVpcAttributeRequest,
    ) -> Result<()> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "ModifyVpcAttribute".to_string());
        form_data.insert("Version".to_string(), "2016-11-15".to_string());
        form_data.insert("VpcId".to_string(), request.vpc_id.clone());

        if let Some(enable_dns_support) = request.enable_dns_support {
            form_data.insert(
                "EnableDnsSupport.Value".to_string(),
                enable_dns_support.to_string(),
            );
        }

        if let Some(enable_dns_hostnames) = request.enable_dns_hostnames {
            form_data.insert(
                "EnableDnsHostnames.Value".to_string(),
                enable_dns_hostnames.to_string(),
            );
        }

        self.send_form_no_body(form_data, "ModifyVpcAttribute", &request.vpc_id)
            .await
    }

    // ---------------------------------------------------------------------------
    // Subnet Operations
    // ---------------------------------------------------------------------------

    pub(super) async fn describe_subnets_impl(
        &self,
        request: DescribeSubnetsRequest,
    ) -> Result<DescribeSubnetsResponse> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "DescribeSubnets".to_string());
        form_data.insert("Version".to_string(), "2016-11-15".to_string());

        if let Some(subnet_ids) = &request.subnet_ids {
            for (i, subnet_id) in subnet_ids.iter().enumerate() {
                form_data.insert(format!("SubnetId.{}", i + 1), subnet_id.clone());
            }
        }

        if let Some(filters) = &request.filters {
            Self::add_filters(&mut form_data, filters);
        }

        if let Some(max_results) = request.max_results {
            form_data.insert("MaxResults".to_string(), max_results.to_string());
        }

        if let Some(next_token) = &request.next_token {
            form_data.insert("NextToken".to_string(), next_token.clone());
        }

        self.send_form(form_data, "DescribeSubnets", "Subnet").await
    }

    pub(super) async fn create_subnet_impl(
        &self,
        request: CreateSubnetRequest,
    ) -> Result<CreateSubnetResponse> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "CreateSubnet".to_string());
        form_data.insert("Version".to_string(), "2016-11-15".to_string());
        form_data.insert("VpcId".to_string(), request.vpc_id.clone());
        form_data.insert("CidrBlock".to_string(), request.cidr_block.clone());

        if let Some(availability_zone) = &request.availability_zone {
            form_data.insert("AvailabilityZone".to_string(), availability_zone.clone());
        }

        if let Some(availability_zone_id) = &request.availability_zone_id {
            form_data.insert(
                "AvailabilityZoneId".to_string(),
                availability_zone_id.clone(),
            );
        }

        if let Some(tag_specs) = &request.tag_specifications {
            Self::add_tag_specifications(&mut form_data, tag_specs);
        }

        self.send_form(form_data, "CreateSubnet", &request.cidr_block)
            .await
    }

    pub(super) async fn delete_subnet_impl(&self, subnet_id: &str) -> Result<()> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "DeleteSubnet".to_string());
        form_data.insert("Version".to_string(), "2016-11-15".to_string());
        form_data.insert("SubnetId".to_string(), subnet_id.to_string());

        self.send_form_no_body(form_data, "DeleteSubnet", subnet_id)
            .await
    }

    // ---------------------------------------------------------------------------
    // Internet Gateway Operations
    // ---------------------------------------------------------------------------

    pub(super) async fn create_internet_gateway_impl(
        &self,
        request: CreateInternetGatewayRequest,
    ) -> Result<CreateInternetGatewayResponse> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "CreateInternetGateway".to_string());
        form_data.insert("Version".to_string(), "2016-11-15".to_string());

        if let Some(tag_specs) = &request.tag_specifications {
            Self::add_tag_specifications(&mut form_data, tag_specs);
        }

        self.send_form(form_data, "CreateInternetGateway", "InternetGateway")
            .await
    }

    pub(super) async fn delete_internet_gateway_impl(
        &self,
        internet_gateway_id: &str,
    ) -> Result<()> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "DeleteInternetGateway".to_string());
        form_data.insert("Version".to_string(), "2016-11-15".to_string());
        form_data.insert(
            "InternetGatewayId".to_string(),
            internet_gateway_id.to_string(),
        );

        self.send_form_no_body(form_data, "DeleteInternetGateway", internet_gateway_id)
            .await
    }

    pub(super) async fn attach_internet_gateway_impl(
        &self,
        request: AttachInternetGatewayRequest,
    ) -> Result<()> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "AttachInternetGateway".to_string());
        form_data.insert("Version".to_string(), "2016-11-15".to_string());
        form_data.insert(
            "InternetGatewayId".to_string(),
            request.internet_gateway_id.clone(),
        );
        form_data.insert("VpcId".to_string(), request.vpc_id.clone());

        self.send_form_no_body(
            form_data,
            "AttachInternetGateway",
            &request.internet_gateway_id,
        )
        .await
    }

    pub(super) async fn detach_internet_gateway_impl(
        &self,
        request: DetachInternetGatewayRequest,
    ) -> Result<()> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "DetachInternetGateway".to_string());
        form_data.insert("Version".to_string(), "2016-11-15".to_string());
        form_data.insert(
            "InternetGatewayId".to_string(),
            request.internet_gateway_id.clone(),
        );
        form_data.insert("VpcId".to_string(), request.vpc_id.clone());

        self.send_form_no_body(
            form_data,
            "DetachInternetGateway",
            &request.internet_gateway_id,
        )
        .await
    }

    pub(super) async fn describe_internet_gateways_impl(
        &self,
        request: DescribeInternetGatewaysRequest,
    ) -> Result<DescribeInternetGatewaysResponse> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "DescribeInternetGateways".to_string());
        form_data.insert("Version".to_string(), "2016-11-15".to_string());

        if let Some(igw_ids) = &request.internet_gateway_ids {
            for (i, igw_id) in igw_ids.iter().enumerate() {
                form_data.insert(format!("InternetGatewayId.{}", i + 1), igw_id.clone());
            }
        }

        if let Some(filters) = &request.filters {
            Self::add_filters(&mut form_data, filters);
        }

        if let Some(max_results) = request.max_results {
            form_data.insert("MaxResults".to_string(), max_results.to_string());
        }

        if let Some(next_token) = &request.next_token {
            form_data.insert("NextToken".to_string(), next_token.clone());
        }

        self.send_form(form_data, "DescribeInternetGateways", "InternetGateway")
            .await
    }

    // ---------------------------------------------------------------------------
    // NAT Gateway Operations
    // ---------------------------------------------------------------------------

    pub(super) async fn create_nat_gateway_impl(
        &self,
        request: CreateNatGatewayRequest,
    ) -> Result<CreateNatGatewayResponse> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "CreateNatGateway".to_string());
        form_data.insert("Version".to_string(), "2016-11-15".to_string());
        form_data.insert("SubnetId".to_string(), request.subnet_id.clone());

        if let Some(allocation_id) = &request.allocation_id {
            form_data.insert("AllocationId".to_string(), allocation_id.clone());
        }

        if let Some(connectivity_type) = &request.connectivity_type {
            form_data.insert("ConnectivityType".to_string(), connectivity_type.clone());
        }

        if let Some(private_ip_address) = &request.private_ip_address {
            form_data.insert("PrivateIpAddress".to_string(), private_ip_address.clone());
        }

        if let Some(tag_specs) = &request.tag_specifications {
            Self::add_tag_specifications(&mut form_data, tag_specs);
        }

        self.send_form(form_data, "CreateNatGateway", &request.subnet_id)
            .await
    }

    pub(super) async fn delete_nat_gateway_impl(
        &self,
        nat_gateway_id: &str,
    ) -> Result<DeleteNatGatewayResponse> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "DeleteNatGateway".to_string());
        form_data.insert("Version".to_string(), "2016-11-15".to_string());
        form_data.insert("NatGatewayId".to_string(), nat_gateway_id.to_string());

        self.send_form(form_data, "DeleteNatGateway", nat_gateway_id)
            .await
    }

    pub(super) async fn describe_nat_gateways_impl(
        &self,
        request: DescribeNatGatewaysRequest,
    ) -> Result<DescribeNatGatewaysResponse> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "DescribeNatGateways".to_string());
        form_data.insert("Version".to_string(), "2016-11-15".to_string());

        if let Some(nat_gateway_ids) = &request.nat_gateway_ids {
            for (i, nat_id) in nat_gateway_ids.iter().enumerate() {
                form_data.insert(format!("NatGatewayId.{}", i + 1), nat_id.clone());
            }
        }

        if let Some(filters) = &request.filters {
            Self::add_filters(&mut form_data, filters);
        }

        if let Some(max_results) = request.max_results {
            form_data.insert("MaxResults".to_string(), max_results.to_string());
        }

        if let Some(next_token) = &request.next_token {
            form_data.insert("NextToken".to_string(), next_token.clone());
        }

        self.send_form(form_data, "DescribeNatGateways", "NatGateway")
            .await
    }

    // ---------------------------------------------------------------------------
    // Elastic IP Operations
    // ---------------------------------------------------------------------------

    pub(super) async fn allocate_address_impl(
        &self,
        request: AllocateAddressRequest,
    ) -> Result<AllocateAddressResponse> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "AllocateAddress".to_string());
        form_data.insert("Version".to_string(), "2016-11-15".to_string());

        // Default to VPC domain
        let domain = request.domain.as_deref().unwrap_or("vpc");
        form_data.insert("Domain".to_string(), domain.to_string());

        if let Some(tag_specs) = &request.tag_specifications {
            Self::add_tag_specifications(&mut form_data, tag_specs);
        }

        self.send_form(form_data, "AllocateAddress", "ElasticIP")
            .await
    }

    pub(super) async fn release_address_impl(&self, allocation_id: &str) -> Result<()> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "ReleaseAddress".to_string());
        form_data.insert("Version".to_string(), "2016-11-15".to_string());
        form_data.insert("AllocationId".to_string(), allocation_id.to_string());

        self.send_form_no_body(form_data, "ReleaseAddress", allocation_id)
            .await
    }

    // ---------------------------------------------------------------------------
    // Route Table Operations
    // ---------------------------------------------------------------------------

    pub(super) async fn describe_route_tables_impl(
        &self,
        request: DescribeRouteTablesRequest,
    ) -> Result<DescribeRouteTablesResponse> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "DescribeRouteTables".to_string());
        form_data.insert("Version".to_string(), "2016-11-15".to_string());

        if let Some(rt_ids) = &request.route_table_ids {
            for (i, rt_id) in rt_ids.iter().enumerate() {
                form_data.insert(format!("RouteTableId.{}", i + 1), rt_id.clone());
            }
        }

        if let Some(filters) = &request.filters {
            Self::add_filters(&mut form_data, filters);
        }

        if let Some(max_results) = request.max_results {
            form_data.insert("MaxResults".to_string(), max_results.to_string());
        }

        if let Some(next_token) = &request.next_token {
            form_data.insert("NextToken".to_string(), next_token.clone());
        }

        self.send_form(form_data, "DescribeRouteTables", "RouteTable")
            .await
    }

    pub(super) async fn create_route_table_impl(
        &self,
        request: CreateRouteTableRequest,
    ) -> Result<CreateRouteTableResponse> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "CreateRouteTable".to_string());
        form_data.insert("Version".to_string(), "2016-11-15".to_string());
        form_data.insert("VpcId".to_string(), request.vpc_id.clone());

        if let Some(tag_specs) = &request.tag_specifications {
            Self::add_tag_specifications(&mut form_data, tag_specs);
        }

        self.send_form(form_data, "CreateRouteTable", &request.vpc_id)
            .await
    }

    pub(super) async fn delete_route_table_impl(&self, route_table_id: &str) -> Result<()> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "DeleteRouteTable".to_string());
        form_data.insert("Version".to_string(), "2016-11-15".to_string());
        form_data.insert("RouteTableId".to_string(), route_table_id.to_string());

        self.send_form_no_body(form_data, "DeleteRouteTable", route_table_id)
            .await
    }

    pub(super) async fn create_route_impl(&self, request: CreateRouteRequest) -> Result<()> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "CreateRoute".to_string());
        form_data.insert("Version".to_string(), "2016-11-15".to_string());
        form_data.insert("RouteTableId".to_string(), request.route_table_id.clone());
        form_data.insert(
            "DestinationCidrBlock".to_string(),
            request.destination_cidr_block.clone(),
        );

        if let Some(gateway_id) = &request.gateway_id {
            form_data.insert("GatewayId".to_string(), gateway_id.clone());
        }

        if let Some(nat_gateway_id) = &request.nat_gateway_id {
            form_data.insert("NatGatewayId".to_string(), nat_gateway_id.clone());
        }

        if let Some(instance_id) = &request.instance_id {
            form_data.insert("InstanceId".to_string(), instance_id.clone());
        }

        if let Some(network_interface_id) = &request.network_interface_id {
            form_data.insert(
                "NetworkInterfaceId".to_string(),
                network_interface_id.clone(),
            );
        }

        if let Some(vpc_peering_connection_id) = &request.vpc_peering_connection_id {
            form_data.insert(
                "VpcPeeringConnectionId".to_string(),
                vpc_peering_connection_id.clone(),
            );
        }

        if let Some(transit_gateway_id) = &request.transit_gateway_id {
            form_data.insert("TransitGatewayId".to_string(), transit_gateway_id.clone());
        }

        self.send_form_no_body(form_data, "CreateRoute", &request.route_table_id)
            .await
    }

    pub(super) async fn delete_route_impl(&self, request: DeleteRouteRequest) -> Result<()> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "DeleteRoute".to_string());
        form_data.insert("Version".to_string(), "2016-11-15".to_string());
        form_data.insert("RouteTableId".to_string(), request.route_table_id.clone());
        form_data.insert(
            "DestinationCidrBlock".to_string(),
            request.destination_cidr_block.clone(),
        );

        self.send_form_no_body(form_data, "DeleteRoute", &request.route_table_id)
            .await
    }

    pub(super) async fn associate_route_table_impl(
        &self,
        request: AssociateRouteTableRequest,
    ) -> Result<AssociateRouteTableResponse> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "AssociateRouteTable".to_string());
        form_data.insert("Version".to_string(), "2016-11-15".to_string());
        form_data.insert("RouteTableId".to_string(), request.route_table_id.clone());
        form_data.insert("SubnetId".to_string(), request.subnet_id.clone());

        self.send_form(form_data, "AssociateRouteTable", &request.route_table_id)
            .await
    }

    pub(super) async fn disassociate_route_table_impl(&self, association_id: &str) -> Result<()> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "DisassociateRouteTable".to_string());
        form_data.insert("Version".to_string(), "2016-11-15".to_string());
        form_data.insert("AssociationId".to_string(), association_id.to_string());

        self.send_form_no_body(form_data, "DisassociateRouteTable", association_id)
            .await
    }

    // ---------------------------------------------------------------------------
    // Security Group Operations
    // ---------------------------------------------------------------------------

    pub(super) async fn describe_security_groups_impl(
        &self,
        request: DescribeSecurityGroupsRequest,
    ) -> Result<DescribeSecurityGroupsResponse> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "DescribeSecurityGroups".to_string());
        form_data.insert("Version".to_string(), "2016-11-15".to_string());

        if let Some(group_ids) = &request.group_ids {
            for (i, group_id) in group_ids.iter().enumerate() {
                form_data.insert(format!("GroupId.{}", i + 1), group_id.clone());
            }
        }

        if let Some(group_names) = &request.group_names {
            for (i, group_name) in group_names.iter().enumerate() {
                form_data.insert(format!("GroupName.{}", i + 1), group_name.clone());
            }
        }

        if let Some(filters) = &request.filters {
            Self::add_filters(&mut form_data, filters);
        }

        if let Some(max_results) = request.max_results {
            form_data.insert("MaxResults".to_string(), max_results.to_string());
        }

        if let Some(next_token) = &request.next_token {
            form_data.insert("NextToken".to_string(), next_token.clone());
        }

        self.send_form(form_data, "DescribeSecurityGroups", "SecurityGroup")
            .await
    }

    pub(super) async fn describe_network_interfaces_impl(
        &self,
        request: DescribeNetworkInterfacesRequest,
    ) -> Result<DescribeNetworkInterfacesResponse> {
        let mut form_data = HashMap::new();
        form_data.insert(
            "Action".to_string(),
            "DescribeNetworkInterfaces".to_string(),
        );
        form_data.insert("Version".to_string(), "2016-11-15".to_string());

        if let Some(network_interface_ids) = &request.network_interface_ids {
            for (i, network_interface_id) in network_interface_ids.iter().enumerate() {
                form_data.insert(
                    format!("NetworkInterfaceId.{}", i + 1),
                    network_interface_id.clone(),
                );
            }
        }

        if let Some(filters) = &request.filters {
            Self::add_filters(&mut form_data, filters);
        }

        if let Some(max_results) = request.max_results {
            form_data.insert("MaxResults".to_string(), max_results.to_string());
        }

        if let Some(next_token) = &request.next_token {
            form_data.insert("NextToken".to_string(), next_token.clone());
        }

        self.send_form(form_data, "DescribeNetworkInterfaces", "NetworkInterface")
            .await
    }

    pub(super) async fn create_security_group_impl(
        &self,
        request: CreateSecurityGroupRequest,
    ) -> Result<CreateSecurityGroupResponse> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "CreateSecurityGroup".to_string());
        form_data.insert("Version".to_string(), "2016-11-15".to_string());
        form_data.insert("GroupName".to_string(), request.group_name.clone());
        form_data.insert("GroupDescription".to_string(), request.description.clone());
        form_data.insert("VpcId".to_string(), request.vpc_id.clone());

        if let Some(tag_specs) = &request.tag_specifications {
            Self::add_tag_specifications(&mut form_data, tag_specs);
        }

        self.send_form(form_data, "CreateSecurityGroup", &request.group_name)
            .await
    }

    pub(super) async fn delete_security_group_impl(&self, group_id: &str) -> Result<()> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "DeleteSecurityGroup".to_string());
        form_data.insert("Version".to_string(), "2016-11-15".to_string());
        form_data.insert("GroupId".to_string(), group_id.to_string());

        self.send_form_no_body(form_data, "DeleteSecurityGroup", group_id)
            .await
    }

    pub(super) async fn authorize_security_group_ingress_impl(
        &self,
        request: AuthorizeSecurityGroupIngressRequest,
    ) -> Result<()> {
        let mut form_data = HashMap::new();
        form_data.insert(
            "Action".to_string(),
            "AuthorizeSecurityGroupIngress".to_string(),
        );
        form_data.insert("Version".to_string(), "2016-11-15".to_string());
        form_data.insert("GroupId".to_string(), request.group_id.clone());

        Self::add_ip_permissions(&mut form_data, &request.ip_permissions);

        self.send_form_no_body(
            form_data,
            "AuthorizeSecurityGroupIngress",
            &request.group_id,
        )
        .await
    }

    pub(super) async fn authorize_security_group_egress_impl(
        &self,
        request: AuthorizeSecurityGroupEgressRequest,
    ) -> Result<()> {
        let mut form_data = HashMap::new();
        form_data.insert(
            "Action".to_string(),
            "AuthorizeSecurityGroupEgress".to_string(),
        );
        form_data.insert("Version".to_string(), "2016-11-15".to_string());
        form_data.insert("GroupId".to_string(), request.group_id.clone());

        Self::add_ip_permissions(&mut form_data, &request.ip_permissions);

        self.send_form_no_body(form_data, "AuthorizeSecurityGroupEgress", &request.group_id)
            .await
    }

    pub(super) async fn revoke_security_group_ingress_impl(
        &self,
        request: RevokeSecurityGroupIngressRequest,
    ) -> Result<()> {
        let mut form_data = HashMap::new();
        form_data.insert(
            "Action".to_string(),
            "RevokeSecurityGroupIngress".to_string(),
        );
        form_data.insert("Version".to_string(), "2016-11-15".to_string());
        form_data.insert("GroupId".to_string(), request.group_id.clone());

        Self::add_ip_permissions(&mut form_data, &request.ip_permissions);

        self.send_form_no_body(form_data, "RevokeSecurityGroupIngress", &request.group_id)
            .await
    }

    pub(super) async fn revoke_security_group_egress_impl(
        &self,
        request: RevokeSecurityGroupEgressRequest,
    ) -> Result<()> {
        let mut form_data = HashMap::new();
        form_data.insert(
            "Action".to_string(),
            "RevokeSecurityGroupEgress".to_string(),
        );
        form_data.insert("Version".to_string(), "2016-11-15".to_string());
        form_data.insert("GroupId".to_string(), request.group_id.clone());

        Self::add_ip_permissions(&mut form_data, &request.ip_permissions);

        self.send_form_no_body(form_data, "RevokeSecurityGroupEgress", &request.group_id)
            .await
    }

    // ---------------------------------------------------------------------------
    // Availability Zone Operations
    // ---------------------------------------------------------------------------

    pub(super) async fn describe_availability_zones_impl(
        &self,
        request: DescribeAvailabilityZonesRequest,
    ) -> Result<DescribeAvailabilityZonesResponse> {
        let mut form_data = HashMap::new();
        form_data.insert(
            "Action".to_string(),
            "DescribeAvailabilityZones".to_string(),
        );
        form_data.insert("Version".to_string(), "2016-11-15".to_string());

        if let Some(zone_names) = &request.zone_names {
            for (i, zone_name) in zone_names.iter().enumerate() {
                form_data.insert(format!("ZoneName.{}", i + 1), zone_name.clone());
            }
        }

        if let Some(zone_ids) = &request.zone_ids {
            for (i, zone_id) in zone_ids.iter().enumerate() {
                form_data.insert(format!("ZoneId.{}", i + 1), zone_id.clone());
            }
        }

        if let Some(filters) = &request.filters {
            Self::add_filters(&mut form_data, filters);
        }

        if let Some(all_availability_zones) = request.all_availability_zones {
            form_data.insert(
                "AllAvailabilityZones".to_string(),
                all_availability_zones.to_string(),
            );
        }

        self.send_form(form_data, "DescribeAvailabilityZones", "AvailabilityZone")
            .await
    }

    // ---------------------------------------------------------------------------
    // AMI Operations
    // ---------------------------------------------------------------------------
}
