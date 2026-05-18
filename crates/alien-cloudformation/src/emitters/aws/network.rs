//! AWS Network — VPC + subnets + NAT + IGW + security group.
//!
//! Three modes:
//!
//! * `UseDefault` — emit nothing, controller falls back to default VPC.
//! * `ByoVpcAws` — emit nothing; existing network IDs are passed via parameters.
//! * `Create` — full topology: VPC, public + private subnets across 1–3
//!   AZs, internet gateway, NAT gateway, route tables, default routes,
//!   subnet associations, security group.

use crate::{
    emitter::CfEmitter,
    emitters::aws::helpers::{
        availability_zone_names, az_condition, first_or_null, get_azs, required_logical_id,
        resource_config, select, subnet_refs, tags, CONDITION_HAS_VPC_CIDR,
        PARAM_PRIVATE_SUBNET_IDS, PARAM_PUBLIC_SUBNET_IDS, PARAM_SECURITY_GROUP_IDS,
        PARAM_VPC_CIDR,
    },
    template::{CfExpression, CfResource},
};
use alien_core::{import::EmitContext, ErrorData, Network, NetworkSettings, Result};
use alien_error::AlienError;

const CONDITION_NETWORK_MODE_CREATE: &str = "NetworkModeCreate";

#[derive(Debug, Clone, Copy, Default)]
pub struct AwsNetworkEmitter;

impl CfEmitter for AwsNetworkEmitter {
    fn emit_resources(&self, ctx: &EmitContext<'_>) -> Result<Vec<CfResource>> {
        let network = resource_config::<Network>(ctx, Network::RESOURCE_TYPE)?;
        match &network.settings {
            NetworkSettings::UseDefault | NetworkSettings::ByoVpcAws { .. } => Ok(vec![]),
            NetworkSettings::Create { .. } => created_network_resources(ctx),
            NetworkSettings::ByoVpcGcp { .. } | NetworkSettings::ByoVnetAzure { .. } => {
                Err(AlienError::new(ErrorData::OperationNotSupported {
                    operation: "generate_cloudformation_template".to_string(),
                    reason: "AWS CloudFormation network emitter received non-AWS network settings"
                        .to_string(),
                }))
            }
        }
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<CfExpression> {
        let network = resource_config::<Network>(ctx, Network::RESOURCE_TYPE)?;
        let import_data = match &network.settings {
            NetworkSettings::UseDefault => CfExpression::object([
                ("publicSubnetIds", CfExpression::list([])),
                ("privateSubnetIds", CfExpression::list([])),
                ("availabilityZones", CfExpression::list([])),
                ("isByoVpc", CfExpression::from(true)),
            ]),
            NetworkSettings::ByoVpcAws { .. } => CfExpression::object([
                ("vpcId", CfExpression::ref_("VpcId")),
                (
                    "publicSubnetIds",
                    CfExpression::ref_(PARAM_PUBLIC_SUBNET_IDS),
                ),
                (
                    "privateSubnetIds",
                    CfExpression::ref_(PARAM_PRIVATE_SUBNET_IDS),
                ),
                (
                    "securityGroupId",
                    first_or_null(CfExpression::ref_(PARAM_SECURITY_GROUP_IDS)),
                ),
                ("availabilityZones", CfExpression::list([])),
                ("isByoVpc", CfExpression::from(true)),
            ]),
            NetworkSettings::Create { .. } => {
                let prefix = required_logical_id(ctx)?;
                CfExpression::if_(
                    CONDITION_NETWORK_MODE_CREATE,
                    CfExpression::object([
                        ("vpcId", CfExpression::ref_(network_id(prefix, "Vpc"))),
                        (
                            "cidrBlock",
                            CfExpression::get_att(network_id(prefix, "Vpc"), "CidrBlock"),
                        ),
                        (
                            "internetGatewayId",
                            CfExpression::ref_(network_id(prefix, "InternetGateway")),
                        ),
                        (
                            "natGatewayId",
                            CfExpression::ref_(network_id(prefix, "NatGateway")),
                        ),
                        (
                            "eipAllocationId",
                            CfExpression::get_att(network_id(prefix, "NatEip"), "AllocationId"),
                        ),
                        ("publicSubnetIds", subnet_refs(prefix, "PublicSubnet")),
                        ("privateSubnetIds", subnet_refs(prefix, "PrivateSubnet")),
                        (
                            "publicRouteTableId",
                            CfExpression::ref_(network_id(prefix, "PublicRouteTable")),
                        ),
                        (
                            "privateRouteTableId",
                            CfExpression::ref_(network_id(prefix, "PrivateRouteTable")),
                        ),
                        (
                            "securityGroupId",
                            CfExpression::ref_(network_id(prefix, "SecurityGroup")),
                        ),
                        ("availabilityZones", availability_zone_names()),
                        ("isByoVpc", CfExpression::from(false)),
                    ]),
                    CfExpression::if_(
                        "NetworkModeUseExisting",
                        CfExpression::object([
                            ("vpcId", CfExpression::ref_("VpcId")),
                            (
                                "publicSubnetIds",
                                CfExpression::ref_(PARAM_PUBLIC_SUBNET_IDS),
                            ),
                            (
                                "privateSubnetIds",
                                CfExpression::ref_(PARAM_PRIVATE_SUBNET_IDS),
                            ),
                            (
                                "securityGroupId",
                                first_or_null(CfExpression::ref_(PARAM_SECURITY_GROUP_IDS)),
                            ),
                            ("availabilityZones", CfExpression::list([])),
                            ("isByoVpc", CfExpression::from(true)),
                        ]),
                        CfExpression::object([
                            ("publicSubnetIds", CfExpression::list([])),
                            ("privateSubnetIds", CfExpression::list([])),
                            ("availabilityZones", CfExpression::list([])),
                            ("isByoVpc", CfExpression::from(true)),
                        ]),
                    ),
                )
            }
            NetworkSettings::ByoVpcGcp { .. } | NetworkSettings::ByoVnetAzure { .. } => {
                unreachable!("validated in emit_resources")
            }
        };

        Ok(import_data)
    }
}

fn created_network_resources(ctx: &EmitContext<'_>) -> Result<Vec<CfResource>> {
    let prefix = required_logical_id(ctx)?;
    let mut resources = Vec::new();

    let vpc_id = network_id(prefix, "Vpc");
    let mut vpc = CfResource::new(vpc_id.clone(), "AWS::EC2::VPC".to_string());
    vpc.properties.insert(
        "CidrBlock".to_string(),
        CfExpression::if_(
            CONDITION_HAS_VPC_CIDR,
            CfExpression::ref_(PARAM_VPC_CIDR),
            CfExpression::from("10.42.0.0/16"),
        ),
    );
    vpc.properties
        .insert("EnableDnsSupport".to_string(), CfExpression::from(true));
    vpc.properties
        .insert("EnableDnsHostnames".to_string(), CfExpression::from(true));
    vpc.properties.insert("Tags".to_string(), tags(ctx));
    resources.push(vpc);

    let igw_id = network_id(prefix, "InternetGateway");
    let mut igw = CfResource::new(igw_id.clone(), "AWS::EC2::InternetGateway".to_string());
    igw.properties.insert("Tags".to_string(), tags(ctx));
    resources.push(igw);

    let mut attachment = CfResource::new(
        network_id(prefix, "VpcGatewayAttachment"),
        "AWS::EC2::VPCGatewayAttachment".to_string(),
    );
    attachment
        .properties
        .insert("VpcId".to_string(), CfExpression::ref_(&vpc_id));
    attachment
        .properties
        .insert("InternetGatewayId".to_string(), CfExpression::ref_(&igw_id));
    resources.push(attachment);

    for index in 0..3usize {
        let public_id = format!("{prefix}PublicSubnet{}", index + 1);
        let private_id = format!("{prefix}PrivateSubnet{}", index + 1);
        let condition = az_condition(index);

        let mut public_subnet = CfResource::new(public_id.clone(), "AWS::EC2::Subnet".to_string());
        if let Some(condition) = condition {
            public_subnet.condition = Some(condition.to_string());
        }
        public_subnet
            .properties
            .insert("VpcId".to_string(), CfExpression::ref_(&vpc_id));
        public_subnet
            .properties
            .insert("AvailabilityZone".to_string(), select(index, get_azs()));
        public_subnet
            .properties
            .insert("CidrBlock".to_string(), select(index, cidr_blocks(&vpc_id)));
        public_subnet
            .properties
            .insert("MapPublicIpOnLaunch".to_string(), CfExpression::from(true));
        public_subnet
            .properties
            .insert("Tags".to_string(), tags(ctx));
        resources.push(public_subnet);

        let mut private_subnet =
            CfResource::new(private_id.clone(), "AWS::EC2::Subnet".to_string());
        if let Some(condition) = condition {
            private_subnet.condition = Some(condition.to_string());
        }
        private_subnet
            .properties
            .insert("VpcId".to_string(), CfExpression::ref_(&vpc_id));
        private_subnet
            .properties
            .insert("AvailabilityZone".to_string(), select(index, get_azs()));
        private_subnet.properties.insert(
            "CidrBlock".to_string(),
            select(index + 3, cidr_blocks(&vpc_id)),
        );
        private_subnet
            .properties
            .insert("Tags".to_string(), tags(ctx));
        resources.push(private_subnet);
    }

    let public_route_table_id = network_id(prefix, "PublicRouteTable");
    let private_route_table_id = network_id(prefix, "PrivateRouteTable");
    for (route_table_id, is_public) in [
        (public_route_table_id.clone(), true),
        (private_route_table_id.clone(), false),
    ] {
        let mut route_table = CfResource::new(route_table_id, "AWS::EC2::RouteTable".to_string());
        route_table
            .properties
            .insert("VpcId".to_string(), CfExpression::ref_(&vpc_id));
        route_table.properties.insert("Tags".to_string(), tags(ctx));
        resources.push(route_table);

        if is_public {
            let mut route = CfResource::new(
                network_id(prefix, "PublicDefaultRoute"),
                "AWS::EC2::Route".to_string(),
            );
            route
                .depends_on
                .push(network_id(prefix, "VpcGatewayAttachment"));
            route.properties.insert(
                "RouteTableId".to_string(),
                CfExpression::ref_(&public_route_table_id),
            );
            route.properties.insert(
                "DestinationCidrBlock".to_string(),
                CfExpression::from("0.0.0.0/0"),
            );
            route
                .properties
                .insert("GatewayId".to_string(), CfExpression::ref_(&igw_id));
            resources.push(route);
        }
    }

    let eip_id = network_id(prefix, "NatEip");
    let mut eip = CfResource::new(eip_id.clone(), "AWS::EC2::EIP".to_string());
    eip.properties
        .insert("Domain".to_string(), CfExpression::from("vpc"));
    resources.push(eip);

    let nat_id = network_id(prefix, "NatGateway");
    let mut nat = CfResource::new(nat_id.clone(), "AWS::EC2::NatGateway".to_string());
    nat.properties.insert(
        "AllocationId".to_string(),
        CfExpression::get_att(&eip_id, "AllocationId"),
    );
    nat.properties.insert(
        "SubnetId".to_string(),
        CfExpression::ref_(format!("{prefix}PublicSubnet1")),
    );
    nat.properties.insert("Tags".to_string(), tags(ctx));
    resources.push(nat);

    let mut private_route = CfResource::new(
        network_id(prefix, "PrivateDefaultRoute"),
        "AWS::EC2::Route".to_string(),
    );
    private_route.depends_on.push(nat_id.clone());
    private_route.properties.insert(
        "RouteTableId".to_string(),
        CfExpression::ref_(&private_route_table_id),
    );
    private_route.properties.insert(
        "DestinationCidrBlock".to_string(),
        CfExpression::from("0.0.0.0/0"),
    );
    private_route
        .properties
        .insert("NatGatewayId".to_string(), CfExpression::ref_(&nat_id));
    resources.push(private_route);

    for index in 0..3usize {
        for (kind, route_table_id) in [
            ("Public", public_route_table_id.clone()),
            ("Private", private_route_table_id.clone()),
        ] {
            let mut association = CfResource::new(
                format!("{prefix}{kind}Subnet{}RouteTableAssociation", index + 1),
                "AWS::EC2::SubnetRouteTableAssociation".to_string(),
            );
            if let Some(condition) = az_condition(index) {
                association.condition = Some(condition.to_string());
            }
            association.properties.insert(
                "SubnetId".to_string(),
                CfExpression::ref_(format!("{prefix}{kind}Subnet{}", index + 1)),
            );
            association.properties.insert(
                "RouteTableId".to_string(),
                CfExpression::ref_(route_table_id),
            );
            resources.push(association);
        }
    }

    let security_group_id = network_id(prefix, "SecurityGroup");
    let mut security_group = CfResource::new(
        security_group_id.clone(),
        "AWS::EC2::SecurityGroup".to_string(),
    );
    security_group.properties.insert(
        "GroupDescription".to_string(),
        CfExpression::from("Private workload security group"),
    );
    security_group
        .properties
        .insert("VpcId".to_string(), CfExpression::ref_(&vpc_id));
    security_group.properties.insert(
        "SecurityGroupEgress".to_string(),
        CfExpression::list([CfExpression::object([
            ("IpProtocol", CfExpression::from("-1")),
            ("CidrIp", CfExpression::from("0.0.0.0/0")),
        ])]),
    );
    security_group
        .properties
        .insert("Tags".to_string(), tags(ctx));
    resources.push(security_group);

    for resource in &mut resources {
        if resource.condition.is_none() {
            resource.condition = Some(CONDITION_NETWORK_MODE_CREATE.to_string());
        }
    }

    Ok(resources)
}

fn network_id(prefix: &str, suffix: &str) -> String {
    format!("{prefix}{suffix}")
}

fn cidr_blocks(vpc_id: &str) -> CfExpression {
    CfExpression::object([(
        "Fn::Cidr",
        CfExpression::list([
            CfExpression::get_att(vpc_id, "CidrBlock"),
            CfExpression::Integer(6),
            CfExpression::Integer(8),
        ]),
    )])
}
