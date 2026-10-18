//! Setup-owned subnet and outbound rules shared by Azure sandbox groups.

use std::collections::BTreeSet;

use alien_core::{import::EmitContext, Result, Sandbox, SandboxEgress};
use hcl::expr::Expression;

use crate::{
    block::{attr, block, nested, resource_block},
    emitter::TfFragment,
    emitters::azure::helpers::tags,
    expr,
};

pub(super) fn emit(ctx: &EmitContext<'_>, network: &str, fragment: &mut TfFragment) -> Result<()> {
    let sandboxes: Vec<_> = ctx
        .stack
        .resources()
        .filter_map(|(_, entry)| entry.config.downcast_ref::<Sandbox>())
        .collect();
    if sandboxes.is_empty() {
        return Ok(());
    }
    let label = format!("{network}_sandbox");
    let nat = format!("{network}_nat");
    let mut ports = BTreeSet::new();
    for sandbox in &sandboxes {
        match sandbox.egress.normalized()? {
            SandboxEgress::None => {}
            SandboxEgress::Internet => {
                ports.insert("80".to_string());
                ports.insert("443".to_string());
            }
            SandboxEgress::Allow { allow } => {
                for target in allow {
                    ports.insert(
                        target
                            .rsplit_once(':')
                            .map_or("443", |(_, port)| port)
                            .to_string(),
                    );
                }
            }
        }
    }
    fragment.resource_blocks.push(resource_block(
        "azurerm_subnet",
        &label,
        [
            attr(
                "name",
                expr::template(format!("${{local.resource_prefix}}-{network}-sandbox")),
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
                    "cidrsubnet(tolist(azurerm_virtual_network.{network}.address_space)[0], 7, 8)"
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
                expr::template(format!("${{local.resource_prefix}}-{network}-sandbox")),
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
