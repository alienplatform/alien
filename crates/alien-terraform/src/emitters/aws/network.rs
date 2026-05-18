//! AWS Network — VPC + subnets + NAT + IGW + security group.
//!
//! Three modes:
//!
//! * `UseDefault` — emit nothing (controller falls back to default VPC).
//! * `ByoVpcAws` — emit nothing (existing network IDs are passed via variables; the
//!   variables themselves are added to `variables.tf` via the
//!   generator's per-target variables list).
//! * `Create` — full topology: VPC, public + private subnets across N
//!   AZs (`for_each = data.aws_availability_zones`), IGW, NAT gateway,
//!   route tables, default routes, subnet associations, security group.

use crate::{
    block::{attr, resource_block},
    emitter::{TfEmitter, TfFragment},
    emitters::aws::helpers::{downcast, nested_block, required_label, tags},
    expr,
};
use alien_core::{import::EmitContext, ErrorData, Network, NetworkSettings, Result};
use alien_error::AlienError;
use hcl::expr::Expression;

#[derive(Debug, Clone, Copy, Default)]
pub struct AwsNetworkEmitter;

impl TfEmitter for AwsNetworkEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let network = downcast::<Network>(ctx, Network::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;

        match &network.settings {
            NetworkSettings::UseDefault => Ok(TfFragment::empty()),
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
            } => Ok(create_topology(
                ctx,
                label,
                cidr.clone(),
                *availability_zones,
            )),
            NetworkSettings::ByoVpcGcp { .. } | NetworkSettings::ByoVnetAzure { .. } => {
                Err(AlienError::new(ErrorData::OperationNotSupported {
                    operation: "generate_terraform_module".to_string(),
                    reason: "AWS Terraform network emitter received non-AWS network settings"
                        .to_string(),
                }))
            }
        }
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
                    expr::raw(format!("var.{label}_public_subnet_ids")),
                ),
                (
                    "privateSubnetIds",
                    expr::raw(format!("var.{label}_private_subnet_ids")),
                ),
                ("publicRouteTableId", Expression::Null),
                ("privateRouteTableId", Expression::Null),
                (
                    "securityGroupId",
                    expr::raw(format!("try(var.{label}_security_group_ids[0], null)")),
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
                    "publicSubnetIds",
                    expr::raw(format!(
                        "var.network_mode == \"create-new\" ? aws_subnet.{label}_public[*].id : var.network_mode == \"use-existing\" ? var.public_subnet_ids : []"
                    )),
                ),
                (
                    "privateSubnetIds",
                    expr::raw(format!(
                        "var.network_mode == \"create-new\" ? aws_subnet.{label}_private[*].id : var.network_mode == \"use-existing\" ? var.private_subnet_ids : []"
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
                        "var.network_mode == \"create-new\" ? aws_security_group.{label}_workload[0].id : var.network_mode == \"use-existing\" ? try(var.security_group_ids[0], null) : null"
                    )),
                ),
                (
                    "availabilityZones",
                    expr::raw(format!(
                        "var.network_mode == \"use-default\" ? [] : slice(data.aws_availability_zones.available.names, 0, {})",
                        cmp_az_count_expr(&network.settings)
                    )),
                ),
                ("isByoVpc", Expression::Bool(false)),
            ]),
            NetworkSettings::ByoVpcGcp { .. } | NetworkSettings::ByoVnetAzure { .. } => {
                unreachable!("validated in emit")
            }
        })
    }
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

    fragment.data_blocks.push(crate::block::data_block(
        "aws_availability_zones",
        "available",
        [attr("state", Expression::String("available".to_string()))],
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
                crate::emitters::aws::helpers::stack_name_template("workload-"),
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

    fragment
}
