//! Azure Network — VNet + subnets + NAT gateway + NSG.
//!
//! Three modes mirror the AWS / GCP shape:
//!
//! * `UseDefault` — Azure has no provider-default VNet, so this still
//!   emits the full `Create` topology. The runtime controller's docs
//!   describe the same fallback ("Azure has no default VNet, so one is
//!   created along with a NAT Gateway. VMs stay private and use NAT for
//!   egress.").
//! * `ByoVnetAzure` — emit `data.azurerm_virtual_network` + workload
//!   subnet + public subnet data lookups. Customer supplies the VNet
//!   resource id and subnet names; we surface the resolved ids in
//!   ImportData so the controller can plug them into AKS / Container
//!   Apps without an extra cloud round-trip.
//! * `Create` — full topology: VNet, public + private subnet, NAT
//!   gateway with its public IP, NSG.
//!
//! Subnets land at deterministic addresses derived from the VNet CIDR.
//! Default CIDR `10.46.0.0/16` mirrors the AWS (`10.42`) / GCP
//! (`10.44`) defaults so multi-cloud customers don't get surprise CIDR
//! collisions across providers.

use crate::{
    block::{attr, data_block, nested, resource_block},
    emitter::{TfEmitter, TfFragment},
    emitters::azure::helpers::{downcast, required_label, tags},
    expr,
};
use alien_core::{import::EmitContext, ErrorData, Network, NetworkSettings, Result};
use alien_error::AlienError;
use hcl::expr::Expression;

#[derive(Debug, Clone, Copy, Default)]
pub struct AzureNetworkEmitter;

impl TfEmitter for AzureNetworkEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let network = downcast::<Network>(ctx, Network::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;

        match &network.settings {
            // Azure has no default VNet, so even `UseDefault` lands a
            // create-mode topology. Match the runtime controller's docs.
            NetworkSettings::UseDefault => Ok(create_topology(ctx, label, None)),
            NetworkSettings::Create { cidr, .. } => Ok(create_topology(ctx, label, cidr.clone())),
            NetworkSettings::ByoVnetAzure {
                vnet_resource_id,
                public_subnet_name,
                private_subnet_name,
                application_gateway_subnet_name,
            } => Ok(byo_topology(
                label,
                vnet_resource_id,
                public_subnet_name,
                private_subnet_name,
                application_gateway_subnet_name.as_deref(),
            )),
            NetworkSettings::ByoVpcAws { .. } | NetworkSettings::ByoVpcGcp { .. } => {
                Err(AlienError::new(ErrorData::OperationNotSupported {
                    operation: "generate_terraform_module".to_string(),
                    reason: "Azure Terraform network emitter received non-Azure network settings"
                        .to_string(),
                }))
            }
        }
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        let network = downcast::<Network>(ctx, Network::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;
        let public_subnet_label = format!("{label}_public");
        let private_subnet_label = format!("{label}_private");
        let nat_label = format!("{label}_nat");
        let nsg_label = format!("{label}_workload");

        Ok(match &network.settings {
            NetworkSettings::UseDefault | NetworkSettings::Create { .. } => expr::object([
                ("subscriptionId", expr::raw("var.azure_subscription_id")),
                ("resourceGroup", expr::raw("var.azure_resource_group_name")),
                (
                    "vnetId",
                    expr::traversal(["azurerm_virtual_network", label, "id"]),
                ),
                (
                    "vnetName",
                    expr::traversal(["azurerm_virtual_network", label, "name"]),
                ),
                (
                    "subnetIds",
                    Expression::Array(vec![
                        expr::traversal(["azurerm_subnet", &public_subnet_label, "id"]),
                        expr::traversal(["azurerm_subnet", &private_subnet_label, "id"]),
                    ]),
                ),
                (
                    "applicationGatewaySubnetId",
                    expr::traversal(["azurerm_subnet", &format!("{label}_appgw"), "id"]),
                ),
                (
                    "applicationGatewaySubnetName",
                    expr::traversal(["azurerm_subnet", &format!("{label}_appgw"), "name"]),
                ),
                (
                    "natGatewayId",
                    expr::traversal(["azurerm_nat_gateway", &nat_label, "id"]),
                ),
                (
                    "networkSecurityGroupId",
                    expr::traversal(["azurerm_network_security_group", &nsg_label, "id"]),
                ),
                ("isByoVnet", Expression::Bool(false)),
            ]),
            NetworkSettings::ByoVnetAzure { .. } => expr::object([
                ("subscriptionId", expr::raw("var.azure_subscription_id")),
                ("resourceGroup", expr::raw("var.azure_resource_group_name")),
                (
                    "vnetId",
                    expr::traversal(["data", "azurerm_virtual_network", label, "id"]),
                ),
                (
                    "vnetName",
                    expr::traversal(["data", "azurerm_virtual_network", label, "name"]),
                ),
                (
                    "subnetIds",
                    Expression::Array(vec![
                        expr::traversal(["data", "azurerm_subnet", &public_subnet_label, "id"]),
                        expr::traversal(["data", "azurerm_subnet", &private_subnet_label, "id"]),
                    ]),
                ),
                (
                    "applicationGatewaySubnetId",
                    expr::traversal(["data", "azurerm_subnet", &format!("{label}_appgw"), "id"]),
                ),
                (
                    "applicationGatewaySubnetName",
                    expr::traversal(["data", "azurerm_subnet", &format!("{label}_appgw"), "name"]),
                ),
                ("natGatewayId", Expression::Null),
                ("networkSecurityGroupId", Expression::Null),
                ("isByoVnet", Expression::Bool(true)),
            ]),
            _ => unreachable!("validated in emit"),
        })
    }
}

fn create_topology(ctx: &EmitContext<'_>, label: &str, cidr: Option<String>) -> TfFragment {
    let cidr = cidr.unwrap_or_else(|| "10.46.0.0/16".to_string());
    let public_label = format!("{label}_public");
    let private_label = format!("{label}_private");
    let appgw_label = format!("{label}_appgw");
    let nat_label = format!("{label}_nat");
    let nat_pip_label = format!("{label}_nat_pip");
    let nsg_label = format!("{label}_workload");

    let mut fragment = TfFragment::default();

    fragment.resource_blocks.push(resource_block(
        "azurerm_virtual_network",
        label,
        [
            attr(
                "name",
                expr::template(format!("${{local.resource_prefix}}-{label}")),
            ),
            attr(
                "resource_group_name",
                expr::raw("var.azure_resource_group_name"),
            ),
            attr("location", expr::raw("var.azure_location")),
            attr(
                "address_space",
                Expression::Array(vec![Expression::String(cidr.clone())]),
            ),
            attr("tags", tags(ctx, "network")),
        ],
    ));

    fragment.resource_blocks.push(resource_block(
        "azurerm_subnet",
        &public_label,
        [
            attr(
                "name",
                expr::template(format!("${{local.resource_prefix}}-{label}-public")),
            ),
            attr(
                "resource_group_name",
                expr::raw("var.azure_resource_group_name"),
            ),
            attr(
                "virtual_network_name",
                expr::traversal(["azurerm_virtual_network", label, "name"]),
            ),
            attr(
                "address_prefixes",
                Expression::Array(vec![expr::raw(format!(
                    "cidrsubnet(tolist(azurerm_virtual_network.{label}.address_space)[0], 8, 0)"
                ))]),
            ),
        ],
    ));

    fragment.resource_blocks.push(resource_block(
        "azurerm_subnet",
        &private_label,
        [
            attr(
                "name",
                expr::template(format!("${{local.resource_prefix}}-{label}-private")),
            ),
            attr(
                "resource_group_name",
                expr::raw("var.azure_resource_group_name"),
            ),
            attr(
                "virtual_network_name",
                expr::traversal(["azurerm_virtual_network", label, "name"]),
            ),
            attr(
                "address_prefixes",
                Expression::Array(vec![expr::raw(format!(
                    "cidrsubnet(tolist(azurerm_virtual_network.{label}.address_space)[0], 8, 1)"
                ))]),
            ),
        ],
    ));

    fragment.resource_blocks.push(resource_block(
        "azurerm_subnet",
        &appgw_label,
        [
            attr(
                "name",
                expr::template(format!("${{local.resource_prefix}}-{label}-appgw")),
            ),
            attr(
                "resource_group_name",
                expr::raw("var.azure_resource_group_name"),
            ),
            attr(
                "virtual_network_name",
                expr::traversal(["azurerm_virtual_network", label, "name"]),
            ),
            attr(
                "address_prefixes",
                Expression::Array(vec![expr::raw(format!(
                    "cidrsubnet(tolist(azurerm_virtual_network.{label}.address_space)[0], 8, 2)"
                ))]),
            ),
        ],
    ));

    fragment.resource_blocks.push(resource_block(
        "azurerm_public_ip",
        &nat_pip_label,
        [
            attr(
                "name",
                expr::template(format!("${{local.resource_prefix}}-{label}-nat-pip")),
            ),
            attr(
                "resource_group_name",
                expr::raw("var.azure_resource_group_name"),
            ),
            attr("location", expr::raw("var.azure_location")),
            attr(
                "allocation_method",
                Expression::String("Static".to_string()),
            ),
            attr("sku", Expression::String("Standard".to_string())),
            attr("tags", tags(ctx, "network")),
        ],
    ));

    fragment.resource_blocks.push(resource_block(
        "azurerm_nat_gateway",
        &nat_label,
        [
            attr(
                "name",
                expr::template(format!("${{local.resource_prefix}}-{label}-nat")),
            ),
            attr(
                "resource_group_name",
                expr::raw("var.azure_resource_group_name"),
            ),
            attr("location", expr::raw("var.azure_location")),
            attr("sku_name", Expression::String("Standard".to_string())),
            attr(
                "idle_timeout_in_minutes",
                Expression::Number(hcl::Number::from(10i64)),
            ),
            attr("tags", tags(ctx, "network")),
        ],
    ));

    fragment.resource_blocks.push(resource_block(
        "azurerm_nat_gateway_public_ip_association",
        &nat_label,
        [
            attr(
                "nat_gateway_id",
                expr::traversal(["azurerm_nat_gateway", &nat_label, "id"]),
            ),
            attr(
                "public_ip_address_id",
                expr::traversal(["azurerm_public_ip", &nat_pip_label, "id"]),
            ),
        ],
    ));

    fragment.resource_blocks.push(resource_block(
        "azurerm_subnet_nat_gateway_association",
        &private_label,
        [
            attr(
                "subnet_id",
                expr::traversal(["azurerm_subnet", &private_label, "id"]),
            ),
            attr(
                "nat_gateway_id",
                expr::traversal(["azurerm_nat_gateway", &nat_label, "id"]),
            ),
        ],
    ));

    fragment.resource_blocks.push(resource_block(
        "azurerm_network_security_group",
        &nsg_label,
        [
            attr(
                "name",
                expr::template(format!("${{local.resource_prefix}}-{label}-workload")),
            ),
            attr(
                "resource_group_name",
                expr::raw("var.azure_resource_group_name"),
            ),
            attr("location", expr::raw("var.azure_location")),
            nested(crate::block::block(
                "security_rule",
                [
                    attr("name", Expression::String("AllowVnetInbound".to_string())),
                    attr("priority", Expression::Number(hcl::Number::from(100i64))),
                    attr("direction", Expression::String("Inbound".to_string())),
                    attr("access", Expression::String("Allow".to_string())),
                    attr("protocol", Expression::String("*".to_string())),
                    attr("source_port_range", Expression::String("*".to_string())),
                    attr(
                        "destination_port_range",
                        Expression::String("*".to_string()),
                    ),
                    attr(
                        "source_address_prefix",
                        Expression::String("VirtualNetwork".to_string()),
                    ),
                    attr(
                        "destination_address_prefix",
                        Expression::String("VirtualNetwork".to_string()),
                    ),
                ],
            )),
            attr("tags", tags(ctx, "network")),
        ],
    ));

    fragment.resource_blocks.push(resource_block(
        "azurerm_subnet_network_security_group_association",
        &private_label,
        [
            attr(
                "subnet_id",
                expr::traversal(["azurerm_subnet", &private_label, "id"]),
            ),
            attr(
                "network_security_group_id",
                expr::traversal(["azurerm_network_security_group", &nsg_label, "id"]),
            ),
        ],
    ));

    fragment
}

fn byo_topology(
    label: &str,
    vnet_resource_id: &str,
    public_subnet_name: &str,
    private_subnet_name: &str,
    application_gateway_subnet_name: Option<&str>,
) -> TfFragment {
    let public_label = format!("{label}_public");
    let private_label = format!("{label}_private");
    let appgw_label = format!("{label}_appgw");
    let mut fragment = TfFragment::default();

    fragment.data_blocks.push(data_block(
        "azurerm_virtual_network",
        label,
        [
            attr(
                "name",
                Expression::String(vnet_name_from_id(vnet_resource_id)),
            ),
            attr(
                "resource_group_name",
                Expression::String(resource_group_from_id(vnet_resource_id)),
            ),
        ],
    ));

    let appgw_subnet_name = application_gateway_subnet_name.unwrap_or(public_subnet_name);
    for (subnet_label, subnet_name) in [
        (public_label, public_subnet_name),
        (private_label, private_subnet_name),
        (appgw_label, appgw_subnet_name),
    ] {
        fragment.data_blocks.push(data_block(
            "azurerm_subnet",
            &subnet_label,
            [
                attr("name", Expression::String(subnet_name.to_string())),
                attr(
                    "virtual_network_name",
                    expr::traversal(["data", "azurerm_virtual_network", label, "name"]),
                ),
                attr(
                    "resource_group_name",
                    expr::traversal([
                        "data",
                        "azurerm_virtual_network",
                        label,
                        "resource_group_name",
                    ]),
                ),
            ],
        ));
    }

    fragment
}

/// Best-effort parse of the VNet name out of an ARM resource id of the
/// shape `/subscriptions/.../resourceGroups/<rg>/providers/Microsoft.Network/virtualNetworks/<name>`.
/// Returns the raw id if the path doesn't match — the AzureRM provider
/// will surface a parse error at apply time, which is the right place
/// for that diagnostic.
fn vnet_name_from_id(resource_id: &str) -> String {
    resource_id
        .rsplit_once("/virtualNetworks/")
        .map(|(_, name)| name.to_string())
        .unwrap_or_else(|| resource_id.to_string())
}

fn resource_group_from_id(resource_id: &str) -> String {
    resource_id
        .split_once("/resourceGroups/")
        .and_then(|(_, rest)| rest.split_once('/').map(|(rg, _)| rg.to_string()))
        .unwrap_or_else(|| resource_id.to_string())
}
