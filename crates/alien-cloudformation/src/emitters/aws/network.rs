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
        availability_zone_names, az_condition, existing_security_group_id, get_azs,
        list_parameter_value, required_logical_id, resource_config, select, subnet_refs, tags,
        vpc_id_expr, CONDITION_HAS_VPC_CIDR, PARAM_PRIVATE_SUBNET_IDS, PARAM_PUBLIC_SUBNET_IDS,
        PARAM_VPC_CIDR,
    },
    template::{CfExpression, CfResource},
};
use alien_core::{import::EmitContext, ErrorData, Network, NetworkSettings, Result, Sandbox};
use alien_error::AlienError;

const CONDITION_NETWORK_MODE_CREATE: &str = "NetworkModeCreate";

#[derive(Debug, Clone, Copy, Default)]
pub struct AwsNetworkEmitter;

impl CfEmitter for AwsNetworkEmitter {
    fn emit_resources(&self, ctx: &EmitContext<'_>) -> Result<Vec<CfResource>> {
        let network = resource_config::<Network>(ctx, Network::RESOURCE_TYPE)?;
        let mut resources = match &network.settings {
            NetworkSettings::UseDefault | NetworkSettings::ByoVpcAws { .. } => Ok(vec![]),
            NetworkSettings::Create { .. } => created_network_resources(ctx),
            NetworkSettings::ByoVpcGcp { .. } | NetworkSettings::ByoVnetAzure { .. } => {
                Err(AlienError::new(ErrorData::OperationNotSupported {
                    operation: "generate_cloudformation_template".to_string(),
                    reason: "AWS CloudFormation network emitter received non-AWS network settings"
                        .to_string(),
                }))
            }
        }?;
        if has_sandboxes(ctx) {
            resources.extend(super::sandbox_network_validation::resources(ctx)?);
            if needs_gateway(ctx) {
                resources.extend(gateway_resources(ctx)?);
            }
        }
        Ok(resources)
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
                    list_parameter_value(PARAM_PUBLIC_SUBNET_IDS),
                ),
                (
                    "privateSubnetIds",
                    list_parameter_value(PARAM_PRIVATE_SUBNET_IDS),
                ),
                ("securityGroupId", existing_security_group_id()),
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
                        (
                            "egressPublicIps",
                            CfExpression::list([CfExpression::ref_(network_id(prefix, "NatEip"))]),
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
                            CfExpression::get_att(network_id(prefix, "SecurityGroup"), "GroupId"),
                        ),
                        ("availabilityZones", availability_zone_names(ctx)),
                        ("isByoVpc", CfExpression::from(false)),
                    ]),
                    {
                        let byo = CfExpression::object([
                            ("vpcId", CfExpression::ref_("VpcId")),
                            (
                                "publicSubnetIds",
                                list_parameter_value(PARAM_PUBLIC_SUBNET_IDS),
                            ),
                            (
                                "privateSubnetIds",
                                list_parameter_value(PARAM_PRIVATE_SUBNET_IDS),
                            ),
                            ("securityGroupId", existing_security_group_id()),
                            ("availabilityZones", CfExpression::list([])),
                            ("isByoVpc", CfExpression::from(true)),
                        ]);
                        if ctx.targets_kubernetes
                            || alien_core::restricts_network_mode(ctx.stack, false)
                        {
                            byo
                        } else {
                            CfExpression::if_(
                                "NetworkModeUseExisting",
                                byo,
                                CfExpression::object([
                                    ("publicSubnetIds", CfExpression::list([])),
                                    ("privateSubnetIds", CfExpression::list([])),
                                    ("availabilityZones", CfExpression::list([])),
                                    ("isByoVpc", CfExpression::from(true)),
                                ]),
                            )
                        }
                    },
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
    let eks = super::eks_availability_zones::required(ctx);
    let zones = if eks {
        super::eks_availability_zones::zones("ZoneIds")
    } else {
        get_azs()
    };
    let zone_property = if eks {
        "AvailabilityZoneId"
    } else {
        "AvailabilityZone"
    };
    let mut resources = if eks {
        super::eks_availability_zones::resources(ctx, CfExpression::ref_("AvailabilityZones"))
    } else {
        Vec::new()
    };

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
            .insert(zone_property.to_string(), select(index, zones.clone()));
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
            .insert(zone_property.to_string(), select(index, zones.clone()));
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

    if has_sandboxes(ctx) {
        // Connector subnets have only the VPC's local route. Gateway tasks and the
        // internal NLB occupy dedicated subnets, with outbound traffic using the NAT.
        for (kind, offset) in [("Sandbox", 6), ("SandboxGateway", 9)] {
            if kind == "SandboxGateway" && !needs_gateway(ctx) {
                continue;
            }
            let route_table_id = format!("{prefix}{kind}RouteTable");
            let mut route_table =
                CfResource::new(route_table_id.clone(), "AWS::EC2::RouteTable".to_string());
            route_table
                .properties
                .insert("VpcId".to_string(), CfExpression::ref_(&vpc_id));
            route_table.properties.insert("Tags".to_string(), tags(ctx));
            resources.push(route_table);
            if kind == "SandboxGateway" {
                let mut route = CfResource::new(
                    format!("{prefix}{kind}DefaultRoute"),
                    "AWS::EC2::Route".to_string(),
                );
                route.properties.insert(
                    "RouteTableId".to_string(),
                    CfExpression::ref_(&route_table_id),
                );
                route.properties.insert(
                    "DestinationCidrBlock".to_string(),
                    CfExpression::from("0.0.0.0/0"),
                );
                route
                    .properties
                    .insert("NatGatewayId".to_string(), CfExpression::ref_(&nat_id));
                resources.push(route);
            }
            for index in 0..3usize {
                let subnet_id = format!("{prefix}{kind}Subnet{}", index + 1);
                let mut subnet = CfResource::new(subnet_id.clone(), "AWS::EC2::Subnet".to_string());
                subnet.condition = az_condition(index).map(str::to_string);
                subnet
                    .properties
                    .insert("VpcId".to_string(), CfExpression::ref_(&vpc_id));
                subnet
                    .properties
                    .insert(zone_property.to_string(), select(index, zones.clone()));
                subnet.properties.insert(
                    "CidrBlock".to_string(),
                    select(index + offset, cidr_blocks(&vpc_id)),
                );
                subnet
                    .properties
                    .insert("MapPublicIpOnLaunch".to_string(), CfExpression::from(false));
                subnet.properties.insert("Tags".to_string(), tags(ctx));
                resources.push(subnet);
                let mut association = CfResource::new(
                    format!("{subnet_id}RouteTableAssociation"),
                    "AWS::EC2::SubnetRouteTableAssociation".to_string(),
                );
                association.condition = az_condition(index).map(str::to_string);
                association
                    .properties
                    .insert("SubnetId".to_string(), CfExpression::ref_(&subnet_id));
                association.properties.insert(
                    "RouteTableId".to_string(),
                    CfExpression::ref_(&route_table_id),
                );
                resources.push(association);
            }
        }
    }

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
            CfExpression::Integer(12),
            CfExpression::Integer(8),
        ]),
    )])
}

/// Setup-owned load balancer; the controller registers the gateway tasks behind it.
fn gateway_resources(ctx: &EmitContext<'_>) -> Result<Vec<CfResource>> {
    let prefix = required_logical_id(ctx)?;
    let mut resources = Vec::new();
    let nlb_group_id = format!("{prefix}SandboxGatewayLoadBalancerSecurityGroup");
    let mut nlb_group = CfResource::new(nlb_group_id.clone(), "AWS::EC2::SecurityGroup".into());
    nlb_group.properties.insert(
        "GroupDescription".into(),
        CfExpression::from("Sandbox proxy and workload admin ingress"),
    );
    nlb_group
        .properties
        .insert("VpcId".into(), vpc_id_expr(ctx));
    nlb_group.properties.insert(
        "SecurityGroupIngress".into(),
        CfExpression::get_att(format!("{prefix}SandboxNetworkValidation"), "NlbIngress"),
    );
    nlb_group.properties.insert(
        "SecurityGroupEgress".into(),
        CfExpression::get_att(format!("{prefix}SandboxNetworkValidation"), "NlbEgress"),
    );
    nlb_group.properties.insert("Tags".into(), tags(ctx));
    resources.push(nlb_group);
    for (index, rules) in alien_core::sandbox_egress::gateway_task_egress(ctx.stack)?
        .chunks(60)
        .enumerate()
    {
        let mut group = CfResource::new(
            format!("{prefix}SandboxGatewayTaskSecurityGroup{index}"),
            "AWS::EC2::SecurityGroup".into(),
        );
        group.properties.insert(
            "GroupDescription".into(),
            CfExpression::from("Gateway public-only TCP destinations"),
        );
        group.properties.insert("VpcId".into(), vpc_id_expr(ctx));
        if index == 0 {
            group.properties.insert(
                "SecurityGroupIngress".into(),
                CfExpression::list([CfExpression::object([
                    ("IpProtocol", CfExpression::from("tcp")),
                    ("FromPort", CfExpression::Integer(3128)),
                    ("ToPort", CfExpression::Integer(3129)),
                    (
                        "SourceSecurityGroupId",
                        CfExpression::get_att(&nlb_group_id, "GroupId"),
                    ),
                ])]),
            );
        }
        group.properties.insert(
            "SecurityGroupEgress".into(),
            CfExpression::list(rules.iter().map(|(cidr, port)| {
                CfExpression::object([
                    ("IpProtocol", CfExpression::from("tcp")),
                    ("FromPort", CfExpression::Integer(i64::from(*port))),
                    ("ToPort", CfExpression::Integer(i64::from(*port))),
                    ("CidrIp", CfExpression::from(cidr.as_str())),
                ])
            })),
        );
        group.properties.insert("Tags".into(), tags(ctx));
        resources.push(group);
    }
    let nlb_id = format!("{prefix}SandboxGatewayLoadBalancer");
    let mut nlb = CfResource::new(
        nlb_id.clone(),
        "AWS::ElasticLoadBalancingV2::LoadBalancer".to_string(),
    );
    nlb.properties
        .insert("Type".to_string(), CfExpression::from("network"));
    nlb.properties.insert(
        "SecurityGroups".into(),
        CfExpression::list([CfExpression::get_att(&nlb_group_id, "GroupId")]),
    );
    nlb.properties
        .insert("Scheme".to_string(), CfExpression::from("internal"));
    nlb.properties
        .insert("IpAddressType".to_string(), CfExpression::from("ipv4"));
    nlb.properties
        .insert("Subnets".to_string(), gateway_subnet_ids(ctx)?);
    nlb.properties.insert(
        "LoadBalancerAttributes".to_string(),
        CfExpression::list([CfExpression::object([
            (
                "Key",
                CfExpression::from("load_balancing.cross_zone.enabled"),
            ),
            ("Value", CfExpression::from("true")),
        ])]),
    );
    nlb.properties.insert("Tags".to_string(), tags(ctx));
    resources.push(nlb);
    for (kind, port) in [("Proxy", 3128), ("Admin", 3129)] {
        let target_id = format!("{prefix}SandboxGateway{kind}Targets");
        let mut target = CfResource::new(
            target_id.clone(),
            "AWS::ElasticLoadBalancingV2::TargetGroup".to_string(),
        );
        target
            .properties
            .insert("VpcId".to_string(), vpc_id_expr(ctx));
        target
            .properties
            .insert("TargetType".to_string(), CfExpression::from("ip"));
        target
            .properties
            .insert("Protocol".to_string(), CfExpression::from("TCP"));
        target
            .properties
            .insert("Port".to_string(), CfExpression::Integer(port));
        target
            .properties
            .insert("HealthCheckProtocol".to_string(), CfExpression::from("TCP"));
        target.properties.insert("Tags".to_string(), tags(ctx));
        resources.push(target);
        let mut listener = CfResource::new(
            format!("{prefix}SandboxGateway{kind}Listener"),
            "AWS::ElasticLoadBalancingV2::Listener".to_string(),
        );
        listener
            .properties
            .insert("LoadBalancerArn".to_string(), CfExpression::ref_(&nlb_id));
        listener
            .properties
            .insert("Protocol".to_string(), CfExpression::from("TCP"));
        listener
            .properties
            .insert("Port".to_string(), CfExpression::Integer(port));
        listener.properties.insert(
            "DefaultActions".to_string(),
            CfExpression::list([CfExpression::object([
                ("Type", CfExpression::from("forward")),
                ("TargetGroupArn", CfExpression::ref_(&target_id)),
            ])]),
        );
        resources.push(listener);
    }
    Ok(resources)
}

pub(crate) fn gateway_subnet_ids(ctx: &EmitContext<'_>) -> Result<CfExpression> {
    let network =
        alien_core::sandbox_egress::sandbox_egress_network(ctx.stack).map_err(|error| {
            AlienError::new(ErrorData::OperationNotSupported {
                operation: "sandbox gateway subnets".to_string(),
                reason: error.to_string(),
            })
        })?;
    let prefix = ctx
        .name_for(network.id)
        .expect("network has a generated name");
    Ok(match network.vpc {
        alien_core::sandbox_egress::SandboxEgressVpc::Created => CfExpression::if_(
            CONDITION_NETWORK_MODE_CREATE,
            subnet_refs(prefix, "SandboxGatewaySubnet"),
            CfExpression::ref_("SandboxGatewaySubnetIds"),
        ),
        alien_core::sandbox_egress::SandboxEgressVpc::BroughtByCustomer => {
            CfExpression::ref_("SandboxGatewaySubnetIds")
        }
    })
}

pub(crate) fn gateway_egress_rules(ctx: &EmitContext<'_>) -> Result<CfExpression> {
    let network =
        alien_core::sandbox_egress::sandbox_egress_network(ctx.stack).map_err(|error| {
            AlienError::new(ErrorData::OperationNotSupported {
                operation: "sandbox gateway security group".into(),
                reason: error.to_string(),
            })
        })?;
    let prefix = ctx.name_for(network.id).ok_or_else(|| {
        AlienError::new(ErrorData::GenericError {
            message: format!("network '{}' has no generated name", network.id),
        })
    })?;
    let rule = |index| {
        CfExpression::object([
            ("IpProtocol", CfExpression::from("tcp")),
            ("FromPort", CfExpression::Integer(3128)),
            ("ToPort", CfExpression::Integer(3128)),
            (
                "CidrIp",
                CfExpression::get_att(
                    format!("{prefix}SandboxNetworkValidation"),
                    format!("GatewayCidr{index}"),
                ),
            ),
        ])
    };
    let mut rules = vec![rule(1), rule(2)];
    if matches!(
        network.vpc,
        alien_core::sandbox_egress::SandboxEgressVpc::Created
    ) {
        rules.push(CfExpression::if_(
            az_condition(2).expect("third zone is conditional"),
            rule(3),
            CfExpression::no_value(),
        ));
    }
    Ok(CfExpression::list(rules))
}

pub(crate) fn needs_gateway(ctx: &EmitContext<'_>) -> bool {
    ctx.stack.resources().any(|(_, entry)| {
        entry
            .config
            .downcast_ref::<Sandbox>()
            .is_some_and(|sandbox| !matches!(sandbox.egress, alien_core::SandboxEgress::None))
    })
}

pub(crate) fn sandbox_subnet_ids(ctx: &EmitContext<'_>) -> Result<CfExpression> {
    let network =
        alien_core::sandbox_egress::sandbox_egress_network(ctx.stack).map_err(|error| {
            AlienError::new(ErrorData::OperationNotSupported {
                operation: "sandbox connector subnets".into(),
                reason: error.to_string(),
            })
        })?;
    let prefix = ctx.name_for(network.id).ok_or_else(|| {
        AlienError::new(ErrorData::GenericError {
            message: format!("network '{}' has no generated name", network.id),
        })
    })?;
    Ok(match network.vpc {
        alien_core::sandbox_egress::SandboxEgressVpc::Created => CfExpression::if_(
            CONDITION_NETWORK_MODE_CREATE,
            subnet_refs(prefix, "SandboxSubnet"),
            CfExpression::ref_("SandboxSubnetIds"),
        ),
        alien_core::sandbox_egress::SandboxEgressVpc::BroughtByCustomer => {
            CfExpression::ref_("SandboxSubnetIds")
        }
    })
}

fn has_sandboxes(ctx: &EmitContext<'_>) -> bool {
    ctx.stack
        .resources()
        .find(|(_, entry)| entry.config.downcast_ref::<Network>().is_some())
        .is_some_and(|(id, _)| id == ctx.resource_id)
        && ctx
            .stack
            .resources()
            .any(|(_, entry)| entry.config.downcast_ref::<Sandbox>().is_some())
}
