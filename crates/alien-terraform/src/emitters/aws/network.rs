//! AWS Network — VPC + subnets + NAT + IGW + security group.
//!
//! Three modes:
//!
//! * `UseDefault` — declare the account's default VPC and subnets as data
//!   sources. The shared network helpers (`vpc_id_expr`,
//!   `private_subnet_ids_expr`) resolve to them, so any emitter placing
//!   resources in this network can reference them.
//! * `ByoVpcAws` — emit nothing (existing network IDs are passed via variables; the
//!   variables themselves are added to `variables.tf` via the
//!   generator's per-target variables list).
//! * `Create` — full topology: VPC, public + private subnets across N
//!   AZs (`for_each = data.aws_availability_zones`), IGW, NAT gateway,
//!   route tables, default routes, subnet associations, security group.

use crate::{
    block::{attr, resource_block},
    emitter::{TfEmitter, TfFragment},
    emitters::aws::helpers::{
        downcast, nested_block, private_subnet_ids_expr, required_label, tags, vpc_id_expr,
    },
    expr,
};
use alien_core::{import::EmitContext, ErrorData, Network, NetworkSettings, Result, Sandbox};
use alien_error::AlienError;
use hcl::expr::Expression;

#[derive(Debug, Clone, Copy, Default)]
pub struct AwsNetworkEmitter;

impl TfEmitter for AwsNetworkEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let network = downcast::<Network>(ctx, Network::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;

        let mut fragment = match &network.settings {
            // Every consumer of a static default network (setup-owned databases, managed EKS,
            // compute clusters) reads the VPC through `helpers::vpc_id_expr` and friends, which
            // resolve to these data sources unconditionally. Declaring them only for some
            // consumers leaves the others referencing undeclared addresses.
            NetworkSettings::UseDefault => Ok(default_network_data(label, None)),
            NetworkSettings::ByoVpcAws { .. } => {
                // Declare the availability-zones data source so the
                // `availabilityZones` field in import data resolves —
                // the existing topology itself is supplied via variables.
                let mut fragment = TfFragment::default();
                fragment.data_blocks.push(crate::block::data_block(
                    "aws_availability_zones",
                    "available",
                    [attr("state", Expression::String("available".to_string()))],
                ));
                Ok(fragment)
            }
            NetworkSettings::Create {
                cidr,
                availability_zones,
            } => {
                let mut fragment = create_topology(ctx, label, cidr.clone(), *availability_zones);
                if stack_needs_default_network_data(ctx) {
                    fragment.extend(default_network_data(
                        label,
                        Some("var.network_mode == \"use-default\" ? 1 : 0"),
                    ));
                }
                Ok(fragment)
            }
            NetworkSettings::ByoVpcGcp { .. } | NetworkSettings::ByoVnetAzure { .. } => {
                Err(AlienError::new(ErrorData::OperationNotSupported {
                    operation: "generate_terraform_module".to_string(),
                    reason: "AWS Terraform network emitter received non-AWS network settings"
                        .to_string(),
                }))
            }
        }?;
        if has_sandboxes(ctx) {
            fragment.extend(network_validation(ctx, label)?);
            if needs_gateway(ctx) {
                fragment.extend(gateway_topology(ctx, label)?);
            }
        }
        Ok(fragment)
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        let network = downcast::<Network>(ctx, Network::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;
        Ok(match &network.settings {
            NetworkSettings::UseDefault => expr::object([
                ("vpcId", Expression::Null),
                ("cidrBlock", Expression::Null),
                ("internetGatewayId", Expression::Null),
                ("natGatewayId", Expression::Null),
                ("eipAllocationId", Expression::Null),
                ("publicSubnetIds", Expression::Array(vec![])),
                ("privateSubnetIds", Expression::Array(vec![])),
                ("publicRouteTableId", Expression::Null),
                ("privateRouteTableId", Expression::Null),
                ("securityGroupId", Expression::Null),
                ("availabilityZones", Expression::Array(vec![])),
                ("isByoVpc", Expression::Bool(true)),
            ]),
            NetworkSettings::ByoVpcAws { .. } => expr::object([
                ("vpcId", expr::raw(format!("var.{label}_vpc_id"))),
                ("cidrBlock", Expression::Null),
                ("internetGatewayId", Expression::Null),
                ("natGatewayId", Expression::Null),
                ("eipAllocationId", Expression::Null),
                (
                    "publicSubnetIds",
                    expr::raw(format!("compact([for id in var.{label}_public_subnet_ids : trimspace(id)])")),
                ),
                (
                    "privateSubnetIds",
                    expr::raw(format!("compact([for id in var.{label}_private_subnet_ids : trimspace(id)])")),
                ),
                ("publicRouteTableId", Expression::Null),
                ("privateRouteTableId", Expression::Null),
                (
                    "securityGroupId",
                    expr::raw(format!("try(compact([for id in var.{label}_security_group_ids : trimspace(id)])[0], null)")),
                ),
                (
                    "availabilityZones",
                    expr::raw("data.aws_availability_zones.available.names"),
                ),
                ("isByoVpc", Expression::Bool(true)),
            ]),
            NetworkSettings::Create { .. } => expr::object([
                ("vpcId", expr::raw(format!(
                    "var.network_mode == \"create-new\" ? aws_vpc.{label}[0].id : var.network_mode == \"use-existing\" ? var.vpc_id : null"
                ))),
                (
                    "cidrBlock",
                    expr::raw(format!(
                        "var.network_mode == \"create-new\" ? aws_vpc.{label}[0].cidr_block : null"
                    )),
                ),
                (
                    "internetGatewayId",
                    expr::raw(format!(
                        "var.network_mode == \"create-new\" ? aws_internet_gateway.{label}[0].id : null"
                    )),
                ),
                (
                    "natGatewayId",
                    expr::raw(format!(
                        "var.network_mode == \"create-new\" ? aws_nat_gateway.{label}[0].id : null"
                    )),
                ),
                (
                    "eipAllocationId",
                    expr::raw(format!(
                        "var.network_mode == \"create-new\" ? aws_eip.{label}_nat[0].id : null"
                    )),
                ),
                (
                    "egressPublicIps",
                    expr::raw(format!(
                        "var.network_mode == \"create-new\" ? [aws_eip.{label}_nat[0].public_ip] : []"
                    )),
                ),
                (
                    "publicSubnetIds",
                    expr::raw(format!(
                        "var.network_mode == \"create-new\" ? aws_subnet.{label}_public[*].id : var.network_mode == \"use-existing\" ? compact([for id in var.public_subnet_ids : trimspace(id)]) : []"
                    )),
                ),
                (
                    "privateSubnetIds",
                    expr::raw(format!(
                        "var.network_mode == \"create-new\" ? aws_subnet.{label}_private[*].id : var.network_mode == \"use-existing\" ? compact([for id in var.private_subnet_ids : trimspace(id)]) : []"
                    )),
                ),
                (
                    "publicRouteTableId",
                    expr::raw(format!(
                        "var.network_mode == \"create-new\" ? aws_route_table.{label}_public[0].id : null"
                    )),
                ),
                (
                    "privateRouteTableId",
                    expr::raw(format!(
                        "var.network_mode == \"create-new\" ? aws_route_table.{label}_private[0].id : null"
                    )),
                ),
                (
                    "securityGroupId",
                    expr::raw(format!(
                        "var.network_mode == \"create-new\" ? aws_security_group.{label}_workload[0].id : var.network_mode == \"use-existing\" ? try(compact([for id in var.security_group_ids : trimspace(id)])[0], null) : null"
                    )),
                ),
                (
                    "availabilityZones",
                    expr::raw(format!(
                        "var.network_mode == \"use-default\" ? [] : slice(data.aws_availability_zones.available.names, 0, {})",
                        cmp_az_count_expr(&network.settings)
                    )),
                ),
                (
                    "isByoVpc",
                    expr::raw("var.network_mode != \"create-new\""),
                ),
            ]),
            NetworkSettings::ByoVpcGcp { .. } | NetworkSettings::ByoVnetAzure { .. } => {
                unreachable!("validated in emit")
            }
        })
    }
}

fn stack_has_managed_eks(ctx: &EmitContext<'_>) -> bool {
    ctx.stack.resources().any(|(_, entry)| {
        entry.lifecycle == alien_core::ResourceLifecycle::Frozen
            && entry
                .config
                .downcast_ref::<alien_core::KubernetesCluster>()
                .is_some_and(|cluster| {
                    cluster.provider == alien_core::KubernetesClusterProvider::Eks
                        && cluster.ownership == alien_core::KubernetesClusterOwnership::Managed
                })
    })
}

fn stack_needs_default_network_data(ctx: &EmitContext<'_>) -> bool {
    stack_has_managed_eks(ctx)
        || ctx.stack.resources().any(|(_, entry)| {
            entry.lifecycle == alien_core::ResourceLifecycle::Frozen
                && entry
                    .config
                    .downcast_ref::<alien_core::Postgres>()
                    .is_some()
        })
}

/// Data sources used by setup-owned resources when the customer chooses the account's default
/// VPC. Dynamic setup modules gate them by network mode; a statically default network reads them
/// unconditionally.
fn default_network_data(label: &str, count: Option<&str>) -> TfFragment {
    let mut vpc_attributes = Vec::new();
    if let Some(count) = count {
        vpc_attributes.push(attr("count", expr::raw(count)));
    }
    vpc_attributes.push(attr("default", Expression::Bool(true)));

    let mut subnet_attributes = Vec::new();
    if let Some(count) = count {
        subnet_attributes.push(attr("count", expr::raw(count)));
    }
    let vpc_ref = if count.is_some() {
        format!("data.aws_vpc.{label}_default[0].id")
    } else {
        format!("data.aws_vpc.{label}_default.id")
    };
    subnet_attributes.push(nested_block(
        "filter",
        vec![
            attr("name", Expression::String("vpc-id".to_string())),
            attr("values", expr::raw(format!("[{vpc_ref}]"))),
        ],
    ));

    TfFragment::default()
        .with_data(crate::block::data_block(
            "aws_vpc",
            &format!("{label}_default"),
            vpc_attributes,
        ))
        .with_data(crate::block::data_block(
            "aws_subnets",
            &format!("{label}_default"),
            subnet_attributes,
        ))
}

fn cmp_az_count_expr(settings: &NetworkSettings) -> u8 {
    if let NetworkSettings::Create {
        availability_zones, ..
    } = settings
    {
        *availability_zones
    } else {
        2
    }
}

fn create_topology(
    ctx: &EmitContext<'_>,
    label: &str,
    cidr: Option<String>,
    _az_count: u8,
) -> TfFragment {
    let mut fragment = TfFragment::default();
    let cidr = cidr.unwrap_or_else(|| "10.42.0.0/16".to_string());

    let mut zones = vec![attr("state", Expression::String("available".to_string()))];
    if stack_has_managed_eks(ctx) {
        // Filter before taking the requested number of zones, so the shared
        // network cannot leave EKS with only one supported availability zone.
        zones.push(attr(
            "exclude_zone_ids",
            expr::raw("var.unsupported_availability_zone_ids"),
        ));
    }
    fragment.data_blocks.push(crate::block::data_block(
        "aws_availability_zones",
        "available",
        zones,
    ));

    fragment.resource_blocks.push(resource_block(
        "aws_vpc",
        label,
        [
            attr(
                "count",
                expr::raw("var.network_mode == \"create-new\" ? 1 : 0"),
            ),
            attr(
                "cidr_block",
                expr::raw(format!("var.vpc_cidr == \"\" ? \"{cidr}\" : var.vpc_cidr")),
            ),
            attr("enable_dns_support", Expression::Bool(true)),
            attr("enable_dns_hostnames", Expression::Bool(true)),
            attr("tags", tags(ctx, "network")),
        ],
    ));

    fragment.resource_blocks.push(resource_block(
        "aws_internet_gateway",
        label,
        [
            attr(
                "count",
                expr::raw("var.network_mode == \"create-new\" ? 1 : 0"),
            ),
            attr("vpc_id", expr::raw(format!("aws_vpc.{label}[0].id"))),
            attr("tags", tags(ctx, "network")),
        ],
    ));

    fragment.resource_blocks.push(resource_block(
        "aws_subnet",
        &format!("{label}_public"),
        [
            attr(
                "count",
                expr::raw(format!(
                    "var.network_mode == \"create-new\" ? var.availability_zones : 0"
                )),
            ),
            attr("vpc_id", expr::raw(format!("aws_vpc.{label}[0].id"))),
            attr(
                "cidr_block",
                expr::raw(format!(
                    "cidrsubnet(aws_vpc.{label}[0].cidr_block, 8, count.index)"
                )),
            ),
            attr(
                "availability_zone",
                expr::raw("data.aws_availability_zones.available.names[count.index]"),
            ),
            attr("map_public_ip_on_launch", Expression::Bool(true)),
            attr("tags", tags(ctx, "network")),
        ],
    ));

    fragment.resource_blocks.push(resource_block(
        "aws_subnet",
        &format!("{label}_private"),
        [
            attr(
                "count",
                expr::raw("var.network_mode == \"create-new\" ? var.availability_zones : 0"),
            ),
            attr("vpc_id", expr::raw(format!("aws_vpc.{label}[0].id"))),
            attr(
                "cidr_block",
                expr::raw(format!(
                    "cidrsubnet(aws_vpc.{label}[0].cidr_block, 8, count.index + var.availability_zones)"
                )),
            ),
            attr(
                "availability_zone",
                expr::raw("data.aws_availability_zones.available.names[count.index]"),
            ),
            attr("tags", tags(ctx, "network")),
        ],
    ));

    fragment.resource_blocks.push(resource_block(
        "aws_eip",
        &format!("{label}_nat"),
        [
            attr(
                "count",
                expr::raw("var.network_mode == \"create-new\" ? 1 : 0"),
            ),
            attr("domain", Expression::String("vpc".to_string())),
            attr("tags", tags(ctx, "network")),
        ],
    ));

    fragment.resource_blocks.push(resource_block(
        "aws_nat_gateway",
        label,
        [
            attr(
                "count",
                expr::raw("var.network_mode == \"create-new\" ? 1 : 0"),
            ),
            attr(
                "allocation_id",
                expr::raw(format!("aws_eip.{label}_nat[0].id")),
            ),
            attr(
                "subnet_id",
                expr::raw(format!("aws_subnet.{label}_public[0].id")),
            ),
            attr("tags", tags(ctx, "network")),
        ],
    ));

    fragment.resource_blocks.push(resource_block(
        "aws_route_table",
        &format!("{label}_public"),
        [
            attr(
                "count",
                expr::raw("var.network_mode == \"create-new\" ? 1 : 0"),
            ),
            attr("vpc_id", expr::raw(format!("aws_vpc.{label}[0].id"))),
            nested_block(
                "route",
                vec![
                    attr("cidr_block", Expression::String("0.0.0.0/0".to_string())),
                    attr(
                        "gateway_id",
                        expr::raw(format!("aws_internet_gateway.{label}[0].id")),
                    ),
                ],
            ),
            attr("tags", tags(ctx, "network")),
        ],
    ));

    fragment.resource_blocks.push(resource_block(
        "aws_route_table",
        &format!("{label}_private"),
        [
            attr(
                "count",
                expr::raw("var.network_mode == \"create-new\" ? 1 : 0"),
            ),
            attr("vpc_id", expr::raw(format!("aws_vpc.{label}[0].id"))),
            nested_block(
                "route",
                vec![
                    attr("cidr_block", Expression::String("0.0.0.0/0".to_string())),
                    attr(
                        "nat_gateway_id",
                        expr::raw(format!("aws_nat_gateway.{label}[0].id")),
                    ),
                ],
            ),
            attr("tags", tags(ctx, "network")),
        ],
    ));

    for (kind, table_label) in [
        ("public", format!("{label}_public")),
        ("private", format!("{label}_private")),
    ] {
        let assoc_label = format!("{label}_{kind}");
        fragment.resource_blocks.push(resource_block(
            "aws_route_table_association",
            &assoc_label,
            [
                attr(
                    "count",
                    expr::raw("var.network_mode == \"create-new\" ? var.availability_zones : 0"),
                ),
                attr(
                    "subnet_id",
                    expr::raw(format!("aws_subnet.{label}_{kind}[count.index].id")),
                ),
                attr(
                    "route_table_id",
                    expr::raw(format!("aws_route_table.{table_label}[0].id")),
                ),
            ],
        ));
    }

    fragment.resource_blocks.push(resource_block(
        "aws_security_group",
        &format!("{label}_workload"),
        [
            attr(
                "count",
                expr::raw("var.network_mode == \"create-new\" ? 1 : 0"),
            ),
            attr(
                "name_prefix",
                crate::emitters::aws::helpers::resource_prefix_template("workload-"),
            ),
            attr(
                "description",
                Expression::String("Private workload security group".to_string()),
            ),
            attr("vpc_id", expr::raw(format!("aws_vpc.{label}[0].id"))),
            nested_block(
                "egress",
                vec![
                    attr("from_port", Expression::Number(hcl::Number::from(0i64))),
                    attr("to_port", Expression::Number(hcl::Number::from(0i64))),
                    attr("protocol", Expression::String("-1".to_string())),
                    attr(
                        "cidr_blocks",
                        Expression::Array(vec![Expression::String("0.0.0.0/0".to_string())]),
                    ),
                ],
            ),
            attr("tags", tags(ctx, "network")),
        ],
    ));

    if has_sandboxes(ctx) {
        let one = || {
            attr(
                "count",
                expr::raw("var.network_mode == \"create-new\" ? 1 : 0"),
            )
        };
        for (kind, offset) in [("sandbox", 6), ("sandbox_gateway", 9)] {
            if kind == "sandbox_gateway" && !needs_gateway(ctx) {
                continue;
            }
            let subnet_label = format!("{label}_{kind}");
            fragment.resource_blocks.push(resource_block(
                "aws_route_table",
                &subnet_label,
                [
                    one(),
                    attr("vpc_id", expr::raw(format!("aws_vpc.{label}[0].id"))),
                    attr("tags", tags(ctx, "network")),
                ],
            ));
            if kind == "sandbox_gateway" {
                fragment.resource_blocks.push(resource_block(
                    "aws_route",
                    &format!("{subnet_label}_nat"),
                    [
                        one(),
                        attr(
                            "route_table_id",
                            expr::raw(format!("aws_route_table.{subnet_label}[0].id")),
                        ),
                        attr("destination_cidr_block", Expression::from("0.0.0.0/0")),
                        attr(
                            "nat_gateway_id",
                            expr::raw(format!("aws_nat_gateway.{label}[0].id")),
                        ),
                    ],
                ));
            }
            fragment.resource_blocks.push(resource_block(
                "aws_subnet",
                &subnet_label,
                [
                    attr(
                        "count",
                        expr::raw(
                            "var.network_mode == \"create-new\" ? var.availability_zones : 0",
                        ),
                    ),
                    attr("vpc_id", expr::raw(format!("aws_vpc.{label}[0].id"))),
                    attr(
                        "cidr_block",
                        expr::raw(format!(
                            "cidrsubnet(aws_vpc.{label}[0].cidr_block, 8, count.index + {offset})"
                        )),
                    ),
                    attr(
                        "availability_zone",
                        expr::raw("data.aws_availability_zones.available.names[count.index]"),
                    ),
                    attr("map_public_ip_on_launch", Expression::Bool(false)),
                    attr("tags", tags(ctx, "network")),
                ],
            ));
            fragment.resource_blocks.push(resource_block(
                "aws_route_table_association",
                &subnet_label,
                [
                    attr(
                        "count",
                        expr::raw(
                            "var.network_mode == \"create-new\" ? var.availability_zones : 0",
                        ),
                    ),
                    attr(
                        "subnet_id",
                        expr::raw(format!("aws_subnet.{subnet_label}[count.index].id")),
                    ),
                    attr(
                        "route_table_id",
                        expr::raw(format!("aws_route_table.{subnet_label}[0].id")),
                    ),
                ],
            ));
        }
    }

    fragment
}

fn gateway_topology(ctx: &EmitContext<'_>, label: &str) -> Result<TfFragment> {
    let mut fragment = TfFragment::default();
    let nlb_group = format!("{label}_sandbox_gateway_nlb");
    fragment.resource_blocks.push(resource_block(
        "aws_security_group",
        &nlb_group,
        [
            attr("vpc_id", vpc_id_expr(ctx)),
            attr(
                "description",
                Expression::from("Sandbox proxy and workload admin ingress"),
            ),
            nested_block(
                "ingress",
                vec![
                    attr("protocol", Expression::from("tcp")),
                    attr("from_port", Expression::from(3128)),
                    attr("to_port", Expression::from(3128)),
                    attr(
                        "cidr_blocks",
                        expr::raw(format!("data.aws_subnet.{label}_sandbox[*].cidr_block")),
                    ),
                ],
            ),
            nested_block(
                "ingress",
                vec![
                    attr("protocol", Expression::from("tcp")),
                    attr("from_port", Expression::from(3129)),
                    attr("to_port", Expression::from(3129)),
                    attr(
                        "cidr_blocks",
                        expr::raw(format!("data.aws_subnet.{label}_private[*].cidr_block")),
                    ),
                ],
            ),
            nested_block(
                "egress",
                vec![
                    attr("protocol", Expression::from("tcp")),
                    attr("from_port", Expression::from(3128)),
                    attr("to_port", Expression::from(3129)),
                    attr(
                        "cidr_blocks",
                        expr::raw(format!(
                            "data.aws_subnet.{label}_sandbox_gateway[*].cidr_block"
                        )),
                    ),
                ],
            ),
            attr("tags", tags(ctx, "network")),
        ],
    ));
    for (index, rules) in alien_core::sandbox_egress::gateway_task_egress(ctx.stack)?
        .chunks(60)
        .enumerate()
    {
        let mut attributes = vec![
            attr("vpc_id", vpc_id_expr(ctx)),
            attr(
                "description",
                Expression::from("Gateway public-only TCP destinations"),
            ),
            attr("tags", tags(ctx, "network")),
        ];
        if index == 0 {
            attributes.push(nested_block(
                "ingress",
                vec![
                    attr("protocol", Expression::from("tcp")),
                    attr("from_port", Expression::from(3128)),
                    attr("to_port", Expression::from(3129)),
                    attr(
                        "security_groups",
                        expr::raw(format!("[aws_security_group.{nlb_group}.id]")),
                    ),
                ],
            ));
        }
        attributes.extend(rules.iter().map(|(cidr, port)| {
            nested_block(
                "egress",
                vec![
                    attr("protocol", Expression::from("tcp")),
                    attr("from_port", Expression::from(*port)),
                    attr("to_port", Expression::from(*port)),
                    attr(
                        "cidr_blocks",
                        Expression::from(vec![Expression::from(cidr.as_str())]),
                    ),
                ],
            )
        }));
        fragment.resource_blocks.push(resource_block(
            "aws_security_group",
            &format!("{label}_sandbox_gateway_task_{index}"),
            attributes,
        ));
    }
    let gateway_label = format!("{label}_sandbox_gateway");
    fragment.resource_blocks.push(resource_block(
        "aws_lb",
        &gateway_label,
        [
            attr("internal", Expression::Bool(true)),
            attr(
                "security_groups",
                expr::raw(format!("[aws_security_group.{nlb_group}.id]")),
            ),
            attr("load_balancer_type", Expression::from("network")),
            attr("ip_address_type", Expression::from("ipv4")),
            attr("subnets", gateway_subnet_ids(ctx)?),
            attr("enable_cross_zone_load_balancing", Expression::Bool(true)),
            attr("tags", tags(ctx, "network")),
        ],
    ));
    for (kind, port) in [("proxy", 3128), ("admin", 3129)] {
        let target_label = format!("{gateway_label}_{kind}");
        fragment.resource_blocks.push(resource_block(
            "aws_lb_target_group",
            &target_label,
            [
                attr("vpc_id", vpc_id_expr(ctx)),
                attr("target_type", Expression::from("ip")),
                attr("protocol", Expression::from("TCP")),
                attr("port", Expression::from(port)),
                nested_block(
                    "health_check",
                    vec![attr("protocol", Expression::from("TCP"))],
                ),
                attr("tags", tags(ctx, "network")),
            ],
        ));
        fragment.resource_blocks.push(resource_block(
            "aws_lb_listener",
            &target_label,
            [
                attr(
                    "load_balancer_arn",
                    expr::raw(format!("aws_lb.{gateway_label}.arn")),
                ),
                attr("protocol", Expression::from("TCP")),
                attr("port", Expression::from(port)),
                nested_block(
                    "default_action",
                    vec![
                        attr("type", Expression::from("forward")),
                        attr(
                            "target_group_arn",
                            expr::raw(format!("aws_lb_target_group.{target_label}.arn")),
                        ),
                    ],
                ),
            ],
        ));
    }
    Ok(fragment)
}

pub(crate) fn gateway_subnet_ids(ctx: &EmitContext<'_>) -> Result<Expression> {
    let network =
        alien_core::sandbox_egress::sandbox_egress_network(ctx.stack).map_err(|error| {
            AlienError::new(ErrorData::OperationNotSupported {
                operation: "sandbox gateway subnets".to_string(),
                reason: error.to_string(),
            })
        })?;
    let label = ctx
        .name_for(network.id)
        .expect("network has a generated name");
    Ok(match network.vpc {
        alien_core::sandbox_egress::SandboxEgressVpc::Created => expr::raw(format!("var.network_mode == \"create-new\" ? aws_subnet.{label}_sandbox_gateway[*].id : var.sandbox_gateway_subnet_ids")),
        alien_core::sandbox_egress::SandboxEgressVpc::BroughtByCustomer => expr::raw("var.sandbox_gateway_subnet_ids"),
    })
}

fn has_sandboxes(ctx: &EmitContext<'_>) -> bool {
    !ctx.targets_kubernetes
        && ctx
            .stack
            .resources()
            .find(|(_, entry)| entry.config.downcast_ref::<Network>().is_some())
            .is_some_and(|(id, _)| id == ctx.resource_id)
        && ctx
            .stack
            .resources()
            .any(|(_, entry)| entry.config.downcast_ref::<Sandbox>().is_some())
}

fn needs_gateway(ctx: &EmitContext<'_>) -> bool {
    ctx.stack.resources().any(|(_, entry)| {
        entry
            .config
            .downcast_ref::<Sandbox>()
            .is_some_and(|sandbox| !matches!(sandbox.egress, alien_core::SandboxEgress::None))
    })
}

pub(crate) fn sandbox_subnet_ids(ctx: &EmitContext<'_>) -> Result<Expression> {
    let network =
        alien_core::sandbox_egress::sandbox_egress_network(ctx.stack).map_err(|error| {
            AlienError::new(ErrorData::OperationNotSupported {
                operation: "sandbox connector subnets".into(),
                reason: error.to_string(),
            })
        })?;
    let label = ctx.name_for(network.id).ok_or_else(|| {
        AlienError::new(ErrorData::GenericError {
            message: format!("network '{}' has no generated name", network.id),
        })
    })?;
    Ok(match network.vpc {
        alien_core::sandbox_egress::SandboxEgressVpc::Created => expr::raw(format!("var.network_mode == \"create-new\" ? aws_subnet.{label}_sandbox[*].id : var.sandbox_subnet_ids")),
        alien_core::sandbox_egress::SandboxEgressVpc::BroughtByCustomer => expr::raw("var.sandbox_subnet_ids"),
    })
}

fn network_validation(ctx: &EmitContext<'_>, label: &str) -> Result<TfFragment> {
    let mut fragment = TfFragment::default();
    if needs_gateway(ctx) {
        fragment.data_blocks.push(crate::block::data_block("aws_vpc",&format!("{label}_sandbox_vpc"),[
            attr("id",vpc_id_expr(ctx)),
            nested_block("lifecycle",vec![nested_block("postcondition",vec![
                attr("condition",expr::raw("alltrue([for association in self.cidr_block_associations : (startswith(association.cidr_block, \"10.\") && tonumber(split(\"/\", association.cidr_block)[1]) >= 8) || (split(\".\", association.cidr_block)[0] == \"172\" && tonumber(split(\".\", association.cidr_block)[1]) >= 16 && tonumber(split(\".\", association.cidr_block)[1]) <= 31 && tonumber(split(\"/\", association.cidr_block)[1]) >= 12) || (startswith(association.cidr_block, \"192.168.\") && tonumber(split(\"/\", association.cidr_block)[1]) >= 16)])")),
                attr("error_message",Expression::from("Sandbox gateways require VPC IPv4 CIDRs within RFC1918 private ranges.")),
            ])]),
        ]));
    }
    let main = format!("{label}_sandbox_main");
    fragment.data_blocks.push(crate::block::data_block(
        "aws_route_table",
        &main,
        [
            attr("vpc_id", vpc_id_expr(ctx)),
            nested_block(
                "filter",
                vec![
                    attr("name", Expression::from("association.main")),
                    attr("values", Expression::from(vec![Expression::from("true")])),
                ],
            ),
        ],
    ));
    let mut scopes = vec![("sandbox", sandbox_subnet_ids(ctx)?)];
    if needs_gateway(ctx) {
        scopes.extend([
            ("sandbox_gateway", gateway_subnet_ids(ctx)?),
            ("private", private_subnet_ids_expr(ctx)),
        ]);
    }
    let created = ctx
        .resource
        .config
        .downcast_ref::<Network>()
        .is_some_and(|network| matches!(network.settings, NetworkSettings::Create { .. }));
    for (kind, ids) in scopes {
        let name = format!("{label}_{kind}");
        let expression = hcl::format::to_string(&ids).expect("expression serializes");
        let count = || attr("count", expr::raw(format!("length({expression})")));
        let mut subnet_attributes = vec![
            count(),
            attr("id", expr::raw(format!("({expression})[count.index]"))),
        ];
        subnet_attributes.push(nested_block("lifecycle",vec![nested_block("postcondition",vec![
            attr("condition",expr::raw(format!("self.vpc_id == {}",hcl::format::to_string(&vpc_id_expr(ctx)).expect("expression serializes")))),
            attr("error_message",Expression::from("Sandbox, gateway, and workload subnets must belong to the deployment VPC.")),
        ])]));
        fragment.data_blocks.push(crate::block::data_block(
            "aws_subnet",
            &name,
            subnet_attributes,
        ));
        let mut table_ids = vec![
            count(),
            attr("vpc_id", vpc_id_expr(ctx)),
            nested_block(
                "filter",
                vec![
                    attr("name", Expression::from("association.subnet-id")),
                    attr(
                        "values",
                        expr::raw(format!("[({expression})[count.index]]")),
                    ),
                ],
            ),
        ];
        if created {
            let mut dependencies = vec![expr::traversal(["aws_route_table_association", &name])];
            if kind == "sandbox_gateway" {
                dependencies.push(expr::traversal([
                    "aws_route",
                    &format!("{label}_sandbox_gateway_nat"),
                ]));
            }
            table_ids.push(attr("depends_on", Expression::from(dependencies)));
        }
        fragment.data_blocks.push(crate::block::data_block(
            "aws_route_tables",
            &name,
            table_ids,
        ));
        let (condition, error_message) = match kind {
            "sandbox" => (
                "alltrue([for route in self.routes : route.gateway_id == \"local\"])",
                "Sandbox connector subnets must have only local VPC routes.",
            ),
            "sandbox_gateway" => (
                "length([for route in self.routes : route if route.cidr_block == \"0.0.0.0/0\" && route.nat_gateway_id != \"\"]) == 1 && alltrue([for route in self.routes : route.gateway_id == \"local\" || (route.cidr_block == \"0.0.0.0/0\" && route.nat_gateway_id != \"\")])",
                "Gateway subnets must have only local VPC routes and the NAT default route.",
            ),
            _ => (
                "length([for route in self.routes : route if route.cidr_block == \"0.0.0.0/0\" && route.nat_gateway_id != \"\"]) == 1",
                "Workload subnets must have a NAT default route.",
            ),
        };
        fragment.data_blocks.push(crate::block::data_block(
            "aws_route_table",
            &name,
            [
                count(),
                attr("route_table_id", expr::raw(format!("coalesce(one(data.aws_route_tables.{name}[count.index].ids),data.aws_route_table.{main}.id)"))),
                nested_block("lifecycle", vec![nested_block("postcondition", vec![
                    attr("condition", expr::raw(condition)),
                    attr("error_message", Expression::from(error_message)),
                ])]),
            ],
        ));
    }
    let mut conditions=vec![
        (format!("length(distinct(data.aws_subnet.{label}_sandbox[*].availability_zone_id)) >= 2"),"Sandbox subnets must span at least two availability zones."),
        (format!("alltrue([for subnet in data.aws_subnet.{label}_sandbox : subnet.ipv6_cidr_block == \"\"] )"),"Sandbox connector subnets must not have IPv6 CIDRs."),
    ];
    if needs_gateway(ctx) {
        conditions.extend([
        (format!("length(distinct(data.aws_subnet.{label}_sandbox_gateway[*].availability_zone_id)) >= 2"),"Gateway subnets must span at least two availability zones."),
        (format!("length(setintersection(toset(data.aws_subnet.{label}_sandbox[*].id),toset(concat(data.aws_subnet.{label}_sandbox_gateway[*].id,data.aws_subnet.{label}_private[*].id)))) == 0 && length(setintersection(toset(data.aws_subnet.{label}_sandbox_gateway[*].id),toset(data.aws_subnet.{label}_private[*].id))) == 0"),"Sandbox and gateway subnets must be dedicated."),
        (format!("length(distinct(flatten([for table in concat(data.aws_route_table.{label}_sandbox_gateway,data.aws_route_table.{label}_private) : [for route in table.routes : route.nat_gateway_id if route.cidr_block == \"0.0.0.0/0\"]]))) == 1"),"Gateway and workloads must share the deployment NAT gateway."),
    ]);
    }
    fragment.resource_blocks.push(resource_block(
        "terraform_data",
        &format!("{label}_sandbox_network_validation"),
        [
            attr(
                "input",
                expr::raw(format!("data.aws_route_table.{label}_sandbox[*].id")),
            ),
            nested_block(
                "lifecycle",
                conditions
                    .into_iter()
                    .map(|(condition, message)| {
                        nested_block(
                            "precondition",
                            vec![
                                attr("condition", expr::raw(condition)),
                                attr("error_message", Expression::from(message)),
                            ],
                        )
                    })
                    .collect(),
            ),
        ],
    ));
    Ok(fragment)
}
