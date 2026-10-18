//! Setup-owned subnet and outbound rules for each Azure sandbox group.

use std::collections::BTreeSet;

use alien_core::{import::EmitContext, ErrorData, Result, Sandbox, SandboxEgress};
use alien_error::AlienError;
use hcl::expr::Expression;

use crate::{
    block::{attr, block, nested, resource_block},
    emitter::TfFragment,
    emitters::azure::helpers::tags,
    expr,
};

pub(super) fn emit(
    ctx: &EmitContext<'_>,
    network: &str,
    cidr: &str,
    fragment: &mut TfFragment,
) -> Result<()> {
    if ctx.targets_kubernetes {
        return Ok(());
    }
    let sandboxes: Vec<_> = ctx
        .stack
        .resources()
        .filter_map(|(id, entry)| {
            entry
                .config
                .downcast_ref::<Sandbox>()
                .map(|sandbox| (id, sandbox))
        })
        .collect();
    if sandboxes.is_empty() {
        return Ok(());
    }
    // Keep the existing infrastructure allocation and reserve /23 or larger subnets for
    // sandboxes. Each group requires its own delegated subnet, which cannot move later.
    let supported_cidr = cidr.split_once('/').is_some_and(|(address, prefix)| {
        address.parse::<std::net::Ipv4Addr>().is_ok()
            && prefix.parse::<u8>().is_ok_and(|prefix| prefix <= 16)
    });
    if !supported_cidr || sandboxes.len() > 120 {
        return Err(AlienError::new(ErrorData::OperationNotSupported {
            operation: "Azure sandbox network setup".to_string(),
            reason: "the dedicated subnet layout requires an IPv4 VNet /16 or larger and at most 120 sandbox resources".to_string(),
        }));
    }
    for (index, (id, sandbox)) in sandboxes.into_iter().enumerate() {
        let name = ctx.name_for(id).ok_or_else(|| {
            AlienError::new(ErrorData::OperationNotSupported {
                operation: "Azure sandbox network setup".to_string(),
                reason: format!("sandbox {id} has no Terraform resource name"),
            })
        })?;
        emit_group(ctx, network, name, index + 8, sandbox, fragment)?;
    }
    Ok(())
}

pub(super) fn subnet_label(network: &str, sandbox: &str) -> String {
    format!("{network}_sandbox_{sandbox}")
}

fn emit_group(
    ctx: &EmitContext<'_>,
    network: &str,
    sandbox_name: &str,
    subnet_index: usize,
    sandbox: &Sandbox,
    fragment: &mut TfFragment,
) -> Result<()> {
    let label = subnet_label(network, sandbox_name);
    let nat = format!("{network}_nat");
    let ports = match sandbox.egress {
        SandboxEgress::None => BTreeSet::new(),
        SandboxEgress::Internet => BTreeSet::from(["*".to_string()]),
    };
    fragment.resource_blocks.push(resource_block(
        "azurerm_subnet",
        &label,
        [
            attr(
                "name",
                expr::template(format!("${{local.resource_prefix}}-{network}-sandbox-{sandbox_name}")),
            ),
            attr(
                "resource_group_name",
                expr::raw("var.azure_resource_group_name"),
            ),
            attr(
                "virtual_network_name",
                expr::traversal(["azurerm_virtual_network", network, "name"]),
            ),
            // /23 with the default /16; disjoint from the existing infrastructure /24s 0..5.
            attr(
                "address_prefixes",
                Expression::Array(vec![expr::raw(format!(
                    "cidrsubnet(tolist(azurerm_virtual_network.{network}.address_space)[0], 7, {subnet_index})"
                ))]),
            ),
            nested(block(
                "delegation",
                [
                    attr(
                        "name",
                        Expression::String("sandbox-environment".to_string()),
                    ),
                    nested(block(
                        "service_delegation",
                        [
                            attr(
                                "name",
                                Expression::String("Microsoft.App/environments".to_string()),
                            ),
                            attr(
                                "actions",
                                Expression::Array(vec![Expression::String(
                                    "Microsoft.Network/virtualNetworks/subnets/join/action"
                                        .to_string(),
                                )]),
                            ),
                        ],
                    )),
                ],
            )),
        ],
    ));
    fragment.resource_blocks.push(resource_block(
        "azurerm_network_security_group",
        &label,
        [
            attr(
                "name",
                expr::template(format!(
                    "${{local.resource_prefix}}-{network}-sandbox-{sandbox_name}"
                )),
            ),
            attr(
                "resource_group_name",
                expr::raw("var.azure_resource_group_name"),
            ),
            attr("location", expr::raw("var.azure_location")),
            attr("tags", tags(ctx, "sandbox-network")),
        ],
    ));
    // Azure's platform metadata/DNS addresses need their service tags: ordinary range denies
    // do not apply to these platform services. DNS remains necessary for native host policies.
    for (name, priority, destination) in [
        ("metadata", 100, "AzurePlatformIMDS"),
        ("vnet", 110, "VirtualNetwork"),
        ("unspecified", 120, "0.0.0.0/8"),
        ("private_10", 121, "10.0.0.0/8"),
        ("shared", 122, "100.64.0.0/10"),
        ("loopback", 123, "127.0.0.0/8"),
        ("link_local", 124, "169.254.0.0/16"),
        ("private_172", 125, "172.16.0.0/12"),
        ("private_192", 126, "192.168.0.0/16"),
        ("multicast", 127, "224.0.0.0/4"),
        ("other", 300, "*"),
    ] {
        rule(
            fragment,
            &label,
            Rule {
                name,
                priority,
                destination,
                allowed_ports: None,
            },
        );
    }
    if ports.is_empty() {
        rule(
            fragment,
            &label,
            Rule {
                name: "dns",
                priority: 140,
                destination: "AzurePlatformDNS",
                allowed_ports: None,
            },
        );
    } else {
        rule(
            fragment,
            &label,
            Rule {
                name: "internet",
                priority: 200,
                destination: "Internet",
                allowed_ports: Some(ports),
            },
        );
    }
    for (resource, field, target) in [
        (
            "azurerm_subnet_nat_gateway_association",
            "nat_gateway_id",
            expr::traversal(["azurerm_nat_gateway", &nat, "id"]),
        ),
        (
            "azurerm_subnet_network_security_group_association",
            "network_security_group_id",
            expr::traversal(["azurerm_network_security_group", &label, "id"]),
        ),
    ] {
        fragment.resource_blocks.push(resource_block(
            resource,
            &label,
            [
                attr(
                    "subnet_id",
                    expr::traversal(["azurerm_subnet", &label, "id"]),
                ),
                attr(field, target),
            ],
        ));
    }
    Ok(())
}

struct Rule<'a> {
    name: &'a str,
    priority: i64,
    destination: &'a str,
    /// A public TCP allow rule when present; otherwise deny every port and protocol.
    allowed_ports: Option<BTreeSet<String>>,
}

fn rule(fragment: &mut TfFragment, label: &str, rule: Rule<'_>) {
    let Rule {
        name,
        priority,
        destination,
        allowed_ports,
    } = rule;
    let (access, protocol) = if allowed_ports.is_some() {
        ("Allow", "Tcp")
    } else {
        ("Deny", "*")
    };
    let mut fields = vec![
        attr("name", Expression::String(name.to_string())),
        attr(
            "resource_group_name",
            expr::raw("var.azure_resource_group_name"),
        ),
        attr(
            "network_security_group_name",
            expr::traversal(["azurerm_network_security_group", label, "name"]),
        ),
        attr("priority", Expression::Number(priority.into())),
        attr("direction", Expression::String("Outbound".to_string())),
        attr("access", Expression::String(access.to_string())),
        attr("protocol", Expression::String(protocol.to_string())),
        attr("source_port_range", Expression::String("*".to_string())),
        attr("source_address_prefix", Expression::String("*".to_string())),
        attr(
            "destination_address_prefix",
            Expression::String(destination.to_string()),
        ),
    ];
    fields.push(match allowed_ports {
        Some(ports) => attr(
            "destination_port_ranges",
            Expression::Array(ports.into_iter().map(Expression::String).collect()),
        ),
        None => attr(
            "destination_port_range",
            Expression::String("*".to_string()),
        ),
    });
    fragment.resource_blocks.push(resource_block(
        "azurerm_network_security_rule",
        &format!("{label}_{name}"),
        fields,
    ));
}
