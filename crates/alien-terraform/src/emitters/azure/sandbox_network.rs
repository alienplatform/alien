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
    emit_gateway_subnet(ctx, network, fragment)?;
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

use super::sandbox_gateway_vm::GATEWAY_HOSTS;

/// Terraform expression for the static private IP at host number `host` of the gateway subnet.
/// Azure reserves .0 to .3, so the gateway VMs take .4 and .5 (`GATEWAY_HOSTS`).
pub(super) fn gateway_ip(network: &str, host: usize) -> Expression {
    expr::raw(format!(
        "cidrhost(azurerm_subnet.{network}_sandbox_gateway.address_prefixes[0], {host})"
    ))
}

/// The sandbox NSG's allow rules for the egress gateway: the proxy port on each gateway IP
/// only, never the subnet or the admin port.
fn gateway_rules(fragment: &mut TfFragment, label: &str, network: &str) {
    for index in 0..GATEWAY_HOSTS.len() {
        rule(
            fragment,
            label,
            Rule {
                name: &format!("gateway_{index}"),
                priority: 100 + index as i64,
                destination: gateway_ip(network, GATEWAY_HOSTS[index]),
                allowed_ports: Some(BTreeSet::from(["3128".to_string()])),
                protocol: "Tcp",
            },
        );
    }
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
    nsg_rules(fragment, &label, network);
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

fn nsg_rules(fragment: &mut TfFragment, label: &str, network: &str) {
    gateway_rules(fragment, label, network);
    // Azure's platform metadata/DNS addresses need their service tags: ordinary range denies
    // do not apply to these platform services. The gateway uses a numeric internal address.
    for (name, priority, destination) in [
        ("metadata", 103, "AzurePlatformIMDS"),
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
            label,
            Rule {
                name,
                priority,
                destination: Expression::String(destination.to_string()),
                allowed_ports: None,
                protocol: "*",
            },
        );
    }
    // The platform resolver is the only non-gateway destination: DNS lookups resolve, which
    // the public docs list as a limit. It sits at 102, ahead of the VirtualNetwork deny.
    rule(
        fragment,
        label,
        Rule {
            name: "dns",
            priority: 102,
            destination: Expression::String("168.63.129.16".to_string()),
            allowed_ports: Some(BTreeSet::from(["53".to_string()])),
            protocol: "*",
        },
    );
    // Deny the Internet tag explicitly in addition to the catch-all, so an edit that removes
    // the catch-all does not open the Internet.
    rule(
        fragment,
        label,
        Rule {
            name: "internet",
            priority: 128,
            destination: Expression::String("Internet".to_string()),
            allowed_ports: None,
            protocol: "*",
        },
    );
}

struct Rule<'a> {
    name: &'a str,
    priority: i64,
    destination: Expression,
    /// An allow rule for these ports when present; otherwise deny every port and protocol.
    allowed_ports: Option<BTreeSet<String>>,
    /// The protocol of an allow rule.
    protocol: &'a str,
}

fn rule(fragment: &mut TfFragment, label: &str, rule: Rule<'_>) {
    let Rule {
        name,
        priority,
        destination,
        allowed_ports,
        protocol,
    } = rule;
    let (access, protocol) = if allowed_ports.is_some() {
        ("Allow", protocol)
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

fn emit_gateway_subnet(
    ctx: &EmitContext<'_>,
    network: &str,
    fragment: &mut TfFragment,
) -> Result<()> {
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
    emit_gateway_subnet_nsg(ctx, network, fragment)?;
    super::sandbox_gateway_vm::emit(ctx, network, fragment)?;
    Ok(())
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
        (
            "gatewayIps",
            Expression::Array(
                super::sandbox_gateway_vm::GATEWAY_HOSTS
                    .into_iter()
                    .map(|host| gateway_ip(network, host))
                    .collect(),
            ),
        ),
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

/// Inbound rules for the gateway subnet: the proxy port from sandbox subnets only, the
/// admin port from workload subnets only, and nothing else (not even VNet traffic).
/// The subnet reaches the Internet through the deployment's existing NAT gateway.
fn emit_gateway_subnet_nsg(
    ctx: &EmitContext<'_>,
    network: &str,
    fragment: &mut TfFragment,
) -> Result<()> {
    let label = format!("{network}_sandbox_gateway");
    let nat = format!("{network}_nat");
    fragment.resource_blocks.push(resource_block(
        "azurerm_network_security_group",
        &label,
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
            attr("location", expr::raw("var.azure_location")),
            attr("tags", tags(ctx, "sandbox-gateway-network")),
        ],
    ));
    let mut priority = 100;
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
        let sandbox_label = subnet_label(network, name);
        let source = expr::raw(format!(
            "azurerm_subnet.{sandbox_label}.address_prefixes[0]"
        ));
        inbound_rule(
            fragment,
            &label,
            &format!("proxy_from_{sandbox_label}"),
            priority,
            "Allow",
            source,
            "3128",
        );
        priority += 1;
    }
    for (index, suffix) in ["public", "private", "container_apps"]
        .into_iter()
        .enumerate()
    {
        inbound_rule(
            fragment,
            &label,
            &format!("admin_from_{suffix}"),
            200 + index as i64,
            "Allow",
            expr::raw(format!(
                "azurerm_subnet.{network}_{suffix}.address_prefixes[0]"
            )),
            "3129",
        );
    }
    peer_rules(fragment, &label, network);
    inbound_rule(
        fragment,
        &label,
        "deny_all",
        4000,
        "Deny",
        Expression::String("*".into()),
        "*",
    );
    for (resource, field, target) in [
        (
            "azurerm_subnet_network_security_group_association",
            "network_security_group_id",
            expr::traversal(["azurerm_network_security_group", &label, "id"]),
        ),
        (
            "azurerm_subnet_nat_gateway_association",
            "nat_gateway_id",
            expr::traversal(["azurerm_nat_gateway", &nat, "id"]),
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

/// The two gateway VMs synchronize policies over the authenticated admin port, and only from
/// each other's static addresses.
fn peer_rules(fragment: &mut TfFragment, label: &str, network: &str) {
    for (index, host) in GATEWAY_HOSTS.iter().enumerate() {
        inbound_rule(
            fragment,
            label,
            &format!("peer_{index}"),
            210 + index as i64,
            "Allow",
            gateway_ip(network, *host),
            "3129",
        );
    }
}

fn inbound_rule(
    fragment: &mut TfFragment,
    label: &str,
    name: &str,
    priority: i64,
    access: &str,
    source: Expression,
    port: &str,
) {
    let protocol = if access == "Allow" { "Tcp" } else { "*" };
    fragment.resource_blocks.push(resource_block(
        "azurerm_network_security_rule",
        &format!("{label}_{name}"),
        [
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
            attr("direction", Expression::String("Inbound".to_string())),
            attr("access", Expression::String(access.to_string())),
            attr("protocol", Expression::String(protocol.to_string())),
            attr("source_port_range", Expression::String("*".to_string())),
            attr("source_address_prefix", source),
            attr(
                "destination_address_prefix",
                Expression::String("*".to_string()),
            ),
            attr(
                "destination_port_range",
                Expression::String(port.to_string()),
            ),
        ],
    ));
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

#[cfg(test)]
mod proxy_rule_tests {
    use super::*;

    fn rendered_rules() -> String {
        let mut fragment = TfFragment::default();
        nsg_rules(&mut fragment, "agents", "net");
        hcl::to_string(
            &hcl::Body::builder()
                .add_blocks(fragment.resource_blocks.clone())
                .build(),
        )
        .unwrap()
    }

    /// Azure refuses two rules with one priority and direction, and priorities below 100.
    #[test]
    fn every_rule_has_its_own_priority_of_at_least_100() {
        let rendered = rendered_rules();
        let mut seen = std::collections::BTreeSet::new();
        for line in rendered.lines().filter(|l| l.trim_start().starts_with("priority")) {
            let value: i64 = line.rsplit('=').next().unwrap().trim().parse().unwrap();
            assert!(value >= 100, "{line}");
            assert!(seen.insert(value), "duplicate priority {value}:\n{rendered}");
        }
    }

    /// Blocks of one rule, so assertions cannot match across rules.
    fn rule_block<'a>(rendered: &'a str, name: &str) -> &'a str {
        let header = format!("resource \"azurerm_network_security_rule\" \"agents_{name}\"");
        let start = rendered
            .find(&header)
            .unwrap_or_else(|| panic!("no rule {name}:\n{rendered}"));
        let rest = &rendered[start..];
        &rest[..rest.find("\n}\n").map_or(rest.len(), |end| end + 3)]
    }

    #[test]
    fn each_gateway_ip_gets_a_proxy_port_allow_and_nothing_names_the_admin_port() {
        let rendered = rendered_rules();
        for index in 0..2 {
            let block = rule_block(&rendered, &format!("gateway_{index}"));
            assert!(block.contains("\"Allow\""), "{block}");
            assert!(block.contains("\"3128\""), "{block}");
            assert!(
                block.contains(&format!(
                    "cidrhost(azurerm_subnet.net_sandbox_gateway.address_prefixes[0], {})",
                    GATEWAY_HOSTS[index]
                )),
                "{block}"
            );
        }
        assert!(!rendered.contains("3129"));
        // The subnet as a whole is never a destination.
        assert!(!rendered.contains("address_prefixes[0]\""), "{rendered}");
    }

    #[test]
    fn the_only_other_allow_is_dns_to_the_platform_resolver() {
        let rendered = rendered_rules();
        let allows = rendered.matches("access             = \"Allow\"").count()
            + rendered.matches("access = \"Allow\"").count();
        assert_eq!(allows, 3, "two gateways and the resolver:\n{rendered}");
        let dns = rule_block(&rendered, "dns");
        assert!(
            dns.contains("\"168.63.129.16\"") && dns.contains("\"53\""),
            "{dns}"
        );
    }

    #[test]
    fn virtual_network_and_internet_are_denied_by_name_and_by_catch_all() {
        let rendered = rendered_rules();
        for (name, destination) in [
            ("vnet", "VirtualNetwork"),
            ("internet", "Internet"),
            ("other", "*"),
        ] {
            let block = rule_block(&rendered, name);
            assert!(
                block.contains("\"Deny\"") && block.contains(&format!("\"{destination}\"")),
                "{block}"
            );
        }
    }
}

#[cfg(test)]
mod gateway_inbound_tests {
    use super::*;

    #[test]
    fn each_gateway_may_reach_the_admin_port_of_its_peer_only_by_its_own_address() {
        let mut fragment = TfFragment::default();
        peer_rules(&mut fragment, "gw", "net");
        let rendered = hcl::to_string(
            &hcl::Body::builder()
                .add_blocks(fragment.resource_blocks)
                .build(),
        )
        .unwrap();
        for (index, host) in GATEWAY_HOSTS.iter().enumerate() {
            assert!(rendered.contains(&format!("\"peer_{index}\"")), "{rendered}");
            assert!(
                rendered.contains(&format!(
                    "cidrhost(azurerm_subnet.net_sandbox_gateway.address_prefixes[0], {host})"
                )),
                "{rendered}"
            );
        }
        assert_eq!(rendered.matches("destination_port_range = \"3129\"").count(), 2);
        assert_eq!(rendered.matches("\"Allow\"").count(), 2);
    }

    fn render(access: &str, port: &str) -> String {
        let mut fragment = TfFragment::default();
        inbound_rule(
            &mut fragment,
            "gw",
            "r",
            100,
            access,
            Expression::String("10.0.0.0/24".into()),
            port,
        );
        hcl::to_string(
            &hcl::Body::builder()
                .add_blocks(fragment.resource_blocks)
                .build(),
        )
        .unwrap()
    }

    #[test]
    fn allow_is_tcp_on_one_port_and_deny_covers_every_protocol() {
        let allow = render("Allow", "3128");
        assert!(allow.contains("direction = \"Inbound\"") && allow.contains("protocol = \"Tcp\""));
        assert!(allow.contains("destination_port_range = \"3128\""));
        let deny = render("Deny", "*");
        assert!(deny.contains("access = \"Deny\"") && deny.contains("protocol = \"*\""));
    }
}
