//! Setup-owned subnet and outbound rules for each Azure sandbox group.

use std::collections::{BTreeMap, BTreeSet};

use alien_core::{import::EmitContext, Container, ErrorData, Result, Sandbox, SandboxEgress};
use alien_error::AlienError;
use hcl::expr::Expression;
use sha2::{Digest, Sha256};

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
                .filter(|sandbox| sandbox.egress == SandboxEgress::Internet)
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
    if !gateway_enabled(ctx) {
        return Err(AlienError::new(ErrorData::OperationNotSupported {
            operation: "Azure sandbox network setup".into(),
            reason:
                "egress internet requires the deployment gateway; native fallback is unavailable"
                    .into(),
        }));
    }
    emit_gateway_subnet(network, fragment);
    let slots = subnet_slots(sandboxes.iter().map(|(id, _)| id.as_str()))?;
    for (id, _sandbox) in sandboxes {
        let name = ctx.name_for(id).ok_or_else(|| {
            AlienError::new(ErrorData::OperationNotSupported {
                operation: "Azure sandbox network setup".to_string(),
                reason: format!("sandbox {id} has no Terraform resource name"),
            })
        })?;
        emit_group(ctx, network, name, slots[id.as_str()], fragment)?;
    }
    Ok(())
}

// Never probe for another slot: adding or removing another resource must not change
// an existing delegated subnet. A collision is a setup error with both IDs named.
fn subnet_slots<'a>(ids: impl IntoIterator<Item = &'a str>) -> Result<BTreeMap<&'a str, usize>> {
    let mut owners = BTreeMap::new();
    let mut slots = BTreeMap::new();
    for id in ids {
        let digest = Sha256::digest(id.as_bytes());
        let slot =
            8 + u64::from_be_bytes(digest[..8].try_into().expect("SHA-256 has 32 bytes")) % 120;
        if let Some(other) = owners.insert(slot, id) {
            return Err(AlienError::new(ErrorData::OperationNotSupported {
                operation: "Azure sandbox network setup".into(),
                reason: format!(
                    "sandbox IDs '{other}' and '{id}' select the same dedicated subnet slot {slot}; choose a different ID for the new sandbox, keeping existing IDs unchanged"
                ),
            }));
        }
        slots.insert(id, slot as usize);
    }
    Ok(slots)
}

pub(super) fn subnet_label(network: &str, sandbox: &str) -> String {
    format!("{network}_sandbox_{sandbox}")
}

fn emit_group(
    ctx: &EmitContext<'_>,
    network: &str,
    sandbox_name: &str,
    subnet_index: usize,
    fragment: &mut TfFragment,
) -> Result<()> {
    let label = subnet_label(network, sandbox_name);
    let nat = format!("{network}_nat");
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
    rule(
        fragment,
        &label,
        Rule {
            name: "gateway",
            priority: 100,
            destination: expr::raw(format!(
                "azurerm_subnet.{network}_sandbox_gateway.address_prefixes[0]"
            )),
            allowed_ports: Some(BTreeSet::from(["3128".to_string(), "3129".to_string()])),
        },
    );
    // Azure's platform metadata/DNS addresses need their service tags: ordinary range denies
    // do not apply to these platform services. The gateway uses a numeric internal address.
    for (name, priority, destination) in [
        ("metadata", 101, "AzurePlatformIMDS"),
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
                destination: Expression::String(destination.to_string()),
                allowed_ports: None,
            },
        );
    }
    rule(
        fragment,
        &label,
        Rule {
            name: "dns",
            priority: 140,
            destination: Expression::String("AzurePlatformDNS".to_string()),
            allowed_ports: None,
        },
    );
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
    destination: Expression,
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
        attr("destination_address_prefix", destination),
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

fn gateway_enabled(ctx: &EmitContext<'_>) -> bool {
    !ctx.targets_kubernetes
        && ctx.stack.resources().any(|(id, entry)| {
            id == "sandbox-egress-gateway" && entry.config.downcast_ref::<Container>().is_some()
        })
        && ctx.stack.resources().any(|(_, entry)| {
            entry
                .config
                .downcast_ref::<Sandbox>()
                .is_some_and(|sandbox| sandbox.egress == SandboxEgress::Internet)
        })
}

fn emit_gateway_subnet(network: &str, fragment: &mut TfFragment) {
    // Existing infrastructure uses /24 indices 0..5; sandbox groups start at /23 index 8.
    // Nothing else is placed here, so sandbox NSGs can allow the frontend range before the
    // provider allocates the ILB address. Gateway replicas run on the NAT-backed workload subnet.
    fragment.resource_blocks.push(resource_block(
        "azurerm_subnet",
        &format!("{network}_sandbox_gateway"),
        [
            attr(
                "name",
                expr::template(format!(
                    "${{local.resource_prefix}}-{network}-sandbox-gateway"
                )),
            ),
            attr(
                "resource_group_name",
                expr::raw("var.azure_resource_group_name"),
            ),
            attr(
                "virtual_network_name",
                expr::traversal(["azurerm_virtual_network", network, "name"]),
            ),
            attr(
                "address_prefixes",
                Expression::Array(vec![expr::raw(format!(
                    "cidrsubnet(tolist(azurerm_virtual_network.{network}.address_space)[0], 8, 6)"
                ))]),
            ),
        ],
    ));
}

pub(super) fn gateway_handoff(ctx: &EmitContext<'_>, network: &str) -> Result<Option<Expression>> {
    if !gateway_enabled(ctx) {
        return Ok(None);
    }
    let mut ids = Vec::new();
    let mut cidrs = Vec::new();
    let mut cidrs_by_resource = Vec::new();
    for (id, entry) in ctx.stack.resources() {
        let Some(sandbox) = entry.config.downcast_ref::<Sandbox>() else {
            continue;
        };
        if sandbox.egress != SandboxEgress::Internet {
            continue;
        }
        let name = ctx.name_for(id).ok_or_else(|| {
            AlienError::new(ErrorData::OperationNotSupported {
                operation: "Azure sandbox network setup".into(),
                reason: format!("sandbox {id} has no Terraform resource name"),
            })
        })?;
        let label = subnet_label(network, name);
        ids.push(expr::traversal(["azurerm_subnet", &label, "id"]));
        let cidr = expr::raw(format!("azurerm_subnet.{label}.address_prefixes[0]"));
        cidrs.push(cidr.clone());
        cidrs_by_resource.push((id.to_string(), Expression::Array(vec![cidr])));
    }
    let gateway = format!("{network}_sandbox_gateway");
    Ok(Some(expr::object([
        ("sandboxSubnetIds", Expression::Array(ids)),
        (
            "gatewaySubnetIds",
            Expression::Array(vec![expr::traversal(["azurerm_subnet", &gateway, "id"])]),
        ),
        ("sandboxCidrs", Expression::Array(cidrs)),
        ("sandboxCidrsByResource", expr::object(cidrs_by_resource)),
        (
            "gatewayCidrs",
            expr::traversal(["azurerm_subnet", &gateway, "address_prefixes"]),
        ),
        (
            "workloadCidrs",
            Expression::Array(
                ["public", "private", "container_apps"]
                    .into_iter()
                    .map(|suffix| {
                        expr::raw(format!(
                            "azurerm_subnet.{network}_{suffix}.address_prefixes[0]"
                        ))
                    })
                    .collect(),
            ),
        ),
        (
            "vpcCidrs",
            expr::traversal(["azurerm_virtual_network", network, "address_space"]),
        ),
    ])))
}

#[cfg(test)]
mod allocation_tests {
    use super::*;

    #[test]
    fn existing_subnets_survive_resource_add_remove_and_reorder() {
        let before = subnet_slots(["alpha", "beta"]).unwrap();
        assert_eq!(before, subnet_slots(["beta", "alpha"]).unwrap());
        let after = subnet_slots(["gamma", "alpha", "beta"]).unwrap();
        for (id, slot) in &before {
            assert_eq!(after[id], *slot);
            assert_eq!(subnet_slots([*id]).unwrap()[id], *slot);
            assert!((8..128).contains(slot));
        }
    }

    #[test]
    fn collisions_are_refused_instead_of_reassigning_existing_subnets() {
        let ids: Vec<_> = (0..121).map(|i| format!("sandbox-{i}")).collect();
        let mut owners = BTreeMap::new();
        for id in &ids {
            let slot = subnet_slots([id.as_str()]).unwrap()[id.as_str()];
            if let Some(other) = owners.insert(slot, id.as_str()) {
                let error = subnet_slots([other, id.as_str()]).unwrap_err().to_string();
                assert!(
                    error.contains(other)
                        && error.contains(id)
                        && error.contains("choose a different ID"),
                    "{error}"
                );
                return;
            }
        }
        panic!("121 IDs cannot occupy 120 slots without a collision");
    }
}
