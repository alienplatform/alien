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
        CONDITION_HAS_VPC_CIDR, PARAM_PRIVATE_SUBNET_IDS, PARAM_PUBLIC_SUBNET_IDS, PARAM_VPC_CIDR,
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
        let prefix = required_logical_id(ctx)?;
        let network_import = |fields: Vec<(&str, CfExpression)>, created: bool| {
            let sandbox_network = has_sandboxes(ctx).then(|| {
                CfExpression::object([
                    (
                        "sandboxSubnetIds",
                        if created {
                            created_sandbox_subnet_values(prefix, "SandboxSubnet", None)
                        } else {
                            CfExpression::ref_("SandboxSubnetIds")
                        },
                    ),
                    (
                        "gatewaySubnetIds",
                        if created {
                            created_sandbox_subnet_values(prefix, "SandboxGatewaySubnet", None)
                        } else {
                            CfExpression::ref_("SandboxGatewaySubnetIds")
                        },
                    ),
                    ("sandboxCidrs", CfExpression::list([])),
                    (
                        "gatewayCidrs",
                        if created {
                            created_sandbox_subnet_values(
                                prefix,
                                "SandboxGatewaySubnet",
                                Some("CidrBlock"),
                            )
                        } else {
                            CfExpression::ref_("SandboxGatewaySubnetCidrs")
                        },
                    ),
                    ("workloadCidrs", CfExpression::list([])),
                    ("vpcCidrs", CfExpression::list([])),
                ])
            });
            CfExpression::object(
                fields
                    .into_iter()
                    .chain(sandbox_network.map(|network| ("sandboxNetwork", network))),
            )
        };
        let import_data = match &network.settings {
            NetworkSettings::UseDefault => network_import(
                vec![
                    ("publicSubnetIds", CfExpression::list([])),
                    ("privateSubnetIds", CfExpression::list([])),
                    ("availabilityZones", CfExpression::list([])),
                    ("isByoVpc", CfExpression::from(true)),
                ],
                false,
            ),
            NetworkSettings::ByoVpcAws { .. } => network_import(
                vec![
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
                ],
                false,
            ),
            NetworkSettings::Create { .. } => {
                let prefix = required_logical_id(ctx)?;
                CfExpression::if_(
                    CONDITION_NETWORK_MODE_CREATE,
                    network_import(
                        vec![
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
                                CfExpression::list([CfExpression::ref_(network_id(
                                    prefix, "NatEip",
                                ))]),
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
                                CfExpression::get_att(
                                    network_id(prefix, "SecurityGroup"),
                                    "GroupId",
                                ),
                            ),
                            ("availabilityZones", availability_zone_names(ctx)),
                            ("isByoVpc", CfExpression::from(false)),
                        ],
                        true,
                    ),
                    {
                        let byo = network_import(
                            vec![
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
                            ],
                            false,
                        );
                        if ctx.targets_kubernetes
                            || alien_core::restricts_network_mode(ctx.stack, false)
                        {
                            byo
                        } else {
                            CfExpression::if_(
                                "NetworkModeUseExisting",
                                byo,
                                network_import(
                                    vec![
                                        ("publicSubnetIds", CfExpression::list([])),
                                        ("privateSubnetIds", CfExpression::list([])),
                                        ("availabilityZones", CfExpression::list([])),
                                        ("isByoVpc", CfExpression::from(true)),
                                    ],
                                    false,
                                ),
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
        public_subnet.properties.insert(
            "CidrBlock".to_string(),
            select(index, cidr_blocks(&vpc_id, 6)),
        );
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
            select(index + 3, cidr_blocks(&vpc_id, 6)),
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
            (
                "Description",
                CfExpression::from("Outbound workload traffic"),
            ),
        ])]),
    );
    security_group
        .properties
        .insert("Tags".to_string(), tags(ctx));
    resources.push(security_group);

    if has_sandboxes(ctx) {
        // Connector subnets have only local VPC routes. The internal NLB has dedicated
        // gateway subnets; gateway tasks share the private compute subnets and NAT.
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
                    select(index + offset, cidr_blocks(&vpc_id, 12)),
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

fn cidr_blocks(vpc_id: &str, count: i64) -> CfExpression {
    CfExpression::object([(
        "Fn::Cidr",
        CfExpression::list([
            CfExpression::get_att(vpc_id, "CidrBlock"),
            CfExpression::Integer(count),
            CfExpression::Integer(8),
        ]),
    )])
}

// AWS::NoValue becomes JSON null inside Fn::ToJsonString. Select the complete
// list so setup registration receives only the two or three real subnet values.
fn created_sandbox_subnet_values(
    prefix: &str,
    suffix: &str,
    attribute: Option<&str>,
) -> CfExpression {
    let value = |zone| {
        let id = format!("{prefix}{suffix}{zone}");
        match attribute {
            Some(attribute) => CfExpression::get_att(id, attribute),
            None => CfExpression::ref_(id),
        }
    };
    CfExpression::if_(
        "NetworkCreateUseAz3",
        CfExpression::list([value(1), value(2), value(3)]),
        CfExpression::list([value(1), value(2)]),
    )
}

/// BYO setup supplies exact subnet CIDRs for the Frozen connector group; runtime
/// cross-checks them against EC2 before publishing internet sandbox bindings.
pub(crate) fn gateway_egress_rules(ctx: &EmitContext<'_>) -> Result<CfExpression> {
    let network =
        alien_core::sandbox_egress::sandbox_egress_network(ctx.stack).map_err(|error| {
            AlienError::new(ErrorData::OperationNotSupported {
                operation: "sandbox gateway security group".into(),
                reason: error.to_string(),
            })
        })?;
    let rule = |cidr| {
        CfExpression::object([
            ("IpProtocol", CfExpression::from("tcp")),
            ("FromPort", CfExpression::Integer(3128)),
            ("ToPort", CfExpression::Integer(3129)),
            ("CidrIp", cidr),
            (
                "Description",
                CfExpression::from("Sandbox gateway proxy traffic"),
            ),
        ])
    };
    let supplied = CfExpression::list([
        rule(select(0, CfExpression::ref_("SandboxGatewaySubnetCidrs"))),
        rule(select(1, CfExpression::ref_("SandboxGatewaySubnetCidrs"))),
        CfExpression::if_(
            "SandboxGatewayHasThirdSubnet",
            rule(select(2, CfExpression::ref_("SandboxGatewaySubnetCidrs"))),
            CfExpression::no_value(),
        ),
    ]);
    if matches!(
        network.vpc,
        alien_core::sandbox_egress::SandboxEgressVpc::BroughtByCustomer
    ) {
        return Ok(supplied);
    }
    let prefix = ctx.name_for(network.id).ok_or_else(|| {
        AlienError::new(ErrorData::GenericError {
            message: format!("network '{}' has no generated name", network.id),
        })
    })?;
    let created = CfExpression::list([
        rule(CfExpression::get_att(
            format!("{prefix}SandboxGatewaySubnet1"),
            "CidrBlock",
        )),
        rule(CfExpression::get_att(
            format!("{prefix}SandboxGatewaySubnet2"),
            "CidrBlock",
        )),
        CfExpression::if_(
            "NetworkCreateUseAz3",
            rule(CfExpression::get_att(
                format!("{prefix}SandboxGatewaySubnet3"),
                "CidrBlock",
            )),
            CfExpression::no_value(),
        ),
    ]);
    Ok(CfExpression::if_(
        CONDITION_NETWORK_MODE_CREATE,
        created,
        supplied,
    ))
}

pub(crate) fn needs_gateway(ctx: &EmitContext<'_>) -> bool {
    alien_core::sandbox_egress::has_sandbox_gateway(ctx.stack)
}

fn has_sandboxes(ctx: &EmitContext<'_>) -> bool {
    !ctx.targets_kubernetes
        && ctx
            .stack
            .resources()
            .find(|(_, entry)| entry.config.downcast_ref::<Network>().is_some())
            .is_some_and(|(id, _)| id == ctx.resource_id)
        && ctx.stack.resources().any(|(_, entry)| {
            entry
                .config
                .downcast_ref::<Sandbox>()
                .is_some_and(|sandbox| {
                    matches!(sandbox.egress, alien_core::SandboxEgress::Internet)
                })
        })
}
