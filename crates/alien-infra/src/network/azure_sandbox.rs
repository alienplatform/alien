//! Read-only verification of the setup-owned Azure gateway network.

use std::collections::{BTreeMap, BTreeSet};

use alien_azure_clients::azure::{
    models::{
        network_security_group::{
            SecurityRuleAccess, SecurityRuleDirection, SecurityRulePropertiesFormat,
            SecurityRulePropertiesFormatProtocol,
        },
        virtual_network::{Subnet, SubnetPropertiesFormat},
    },
    network::NetworkApi,
};
use alien_core::import::data::AzureSandboxNetwork;
use alien_error::{AlienError, Context};

use super::AzureNetworkController;
use crate::{ErrorData, Result};

fn invalid(resource: &str, message: impl Into<String>) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::ResourceConfigInvalid {
        message: message.into(),
        resource_id: Some(resource.into()),
    })
}

fn child_name(id: &str, parent: &str, kind: &str) -> Option<String> {
    let id = id.to_ascii_lowercase();
    let prefix = format!("{parent}/{kind}/").to_ascii_lowercase();
    let name = id.strip_prefix(&prefix)?;
    (!name.is_empty() && !name.contains('/')).then(|| name.into())
}

fn prefixes(subnet: &SubnetPropertiesFormat) -> BTreeSet<String> {
    subnet
        .address_prefix
        .iter()
        .chain(&subnet.address_prefixes)
        .cloned()
        .collect()
}

fn unrestricted_source(rule: &SecurityRulePropertiesFormat) -> bool {
    rule.source_address_prefix.as_deref() == Some("*")
        && rule.source_address_prefixes.is_empty()
        && rule.source_application_security_groups.is_empty()
        && rule.source_port_range.as_deref() == Some("*")
        && rule.source_port_ranges.is_empty()
        && rule.destination_application_security_groups.is_empty()
}

fn full_deny(rule: &SecurityRulePropertiesFormat, destination: &str, priority: i32) -> bool {
    rule.access == SecurityRuleAccess::Deny
        && rule.direction == SecurityRuleDirection::Outbound
        && rule.protocol == SecurityRulePropertiesFormatProtocol::X
        && rule.priority == priority
        && unrestricted_source(rule)
        && rule.destination_address_prefix.as_deref() == Some(destination)
        && rule.destination_address_prefixes.is_empty()
        && rule.destination_port_range.as_deref() == Some("*")
        && rule.destination_port_ranges.is_empty()
}

/// Checks provider facts before a gateway config can grant access. It never repairs or
/// mutates setup-owned resources. Custom routes, peering and an unknown NAT path are refused.
/// The returned VNet ranges come from Azure, including ranges missing from registration data.
pub async fn validate_azure_sandbox_network(
    client: &dyn NetworkApi,
    resource: &str,
    network: &AzureNetworkController,
) -> Result<AzureSandboxNetwork> {
    let missing = || {
        invalid(
            resource,
            "Azure gateway networking requires completed managed network setup",
        )
    };
    if network.is_byo_vnet {
        return Err(missing());
    }
    let declared = network.sandbox_network.as_ref().ok_or_else(missing)?;
    let group = network.resource_group.as_deref().ok_or_else(missing)?;
    let name = network.vnet_name.as_deref().ok_or_else(missing)?;
    let expected_vnet = network.vnet_resource_id.as_deref().ok_or_else(missing)?;
    let nat_id = network.nat_gateway_id.as_deref().ok_or_else(missing)?;
    let private = network.private_subnet_name.as_deref().ok_or_else(missing)?;
    let cloud_error = |operation: &str| ErrorData::CloudPlatformError {
        message: format!("{operation} failed while verifying Azure sandbox networking"),
        resource_id: Some(resource.into()),
    };
    let vnet = client
        .get_virtual_network(group, name)
        .await
        .context(cloud_error("GetVirtualNetwork"))?;
    let id = vnet.id.as_deref().ok_or_else(missing)?;
    if !id.eq_ignore_ascii_case(expected_vnet) {
        return Err(invalid(
            resource,
            "Azure returned a different deployment VNet",
        ));
    }
    let parent = id.to_ascii_lowercase();
    let network_parent = parent
        .rsplit_once("/virtualnetworks/")
        .ok_or_else(missing)?
        .0;
    let properties = vnet.properties.as_ref().ok_or_else(missing)?;
    if !properties.virtual_network_peerings.is_empty() {
        return Err(invalid(
            resource,
            "Azure gateway networking does not support VNet peering",
        ));
    }
    let vpc_cidrs: BTreeSet<_> = properties
        .address_space
        .as_ref()
        .ok_or_else(missing)?
        .address_prefixes
        .iter()
        .cloned()
        .collect();
    if vpc_cidrs.is_empty() {
        return Err(invalid(
            resource,
            "Azure VNet has no reported address space",
        ));
    }
    let subnets: BTreeMap<_, _> = properties
        .subnets
        .iter()
        .filter_map(|subnet| {
            subnet
                .id
                .as_ref()
                .map(|id| (id.to_ascii_lowercase(), subnet))
        })
        .collect();
    let subnet = |id: &str| -> Result<&SubnetPropertiesFormat> {
        if child_name(id, &parent, "subnets").is_none() {
            return Err(invalid(
                resource,
                "A gateway subnet belongs to another VNet",
            ));
        }
        let found: &Subnet = subnets
            .get(&id.to_ascii_lowercase())
            .copied()
            .ok_or_else(|| invalid(resource, "Azure did not return a declared subnet"))?;
        let properties = found.properties.as_ref().ok_or_else(missing)?;
        if properties.route_table.is_some() {
            return Err(invalid(
                resource,
                "Azure gateway networking does not support custom subnet routes",
            ));
        }
        if prefixes(properties).is_empty() {
            return Err(invalid(
                resource,
                "Azure subnet has no reported address prefix",
            ));
        }
        Ok(properties)
    };
    if declared.sandbox_subnet_ids.is_empty() || declared.gateway_subnet_ids.len() != 1 {
        return Err(invalid(
            resource,
            "Provide dedicated sandbox subnets and one gateway frontend subnet",
        ));
    }
    let mut unique = BTreeSet::new();
    for id in declared
        .sandbox_subnet_ids
        .iter()
        .chain(&declared.gateway_subnet_ids)
    {
        if !unique.insert(id.to_ascii_lowercase()) {
            return Err(invalid(
                resource,
                "Sandbox and gateway subnets must be distinct",
            ));
        }
    }
    let gateway = subnet(&declared.gateway_subnet_ids[0])?;
    if !gateway.delegations.is_empty() {
        return Err(invalid(
            resource,
            "The gateway frontend subnet must be undelegated",
        ));
    }
    let gateway_cidrs = prefixes(gateway);
    if gateway_cidrs.len() != 1 || gateway_cidrs != declared.gateway_cidrs.iter().cloned().collect()
    {
        return Err(invalid(
            resource,
            "Gateway subnet prefixes differ from setup registration",
        ));
    }
    let gateway_cidr = gateway_cidrs.first().ok_or_else(missing)?;
    let private_id = format!("{id}/subnets/{private}");
    if unique.contains(&private_id.to_ascii_lowercase()) {
        return Err(invalid(
            resource,
            "Gateway hosts require a separate workload subnet",
        ));
    }
    let hosting = subnet(&private_id)?;
    if !hosting
        .nat_gateway
        .as_ref()
        .and_then(|nat| nat.id.as_deref())
        .is_some_and(|id| id.eq_ignore_ascii_case(nat_id))
    {
        return Err(invalid(
            resource,
            "The gateway hosting subnet does not use the declared NAT gateway",
        ));
    }
    let mut workload_cidrs = BTreeSet::new();
    for cidr in &declared.workload_cidrs {
        let found = properties
            .subnets
            .iter()
            .find(|subnet| {
                subnet
                    .properties
                    .as_ref()
                    .is_some_and(|p| prefixes(p).contains(cidr))
            })
            .ok_or_else(|| {
                invalid(
                    resource,
                    "A workload source prefix is absent from the deployment VNet",
                )
            })?;
        if unique.contains(&found.id.as_deref().unwrap_or_default().to_ascii_lowercase()) {
            return Err(invalid(
                resource,
                "Workload and sandbox source ranges must be distinct",
            ));
        }
        workload_cidrs.insert(cidr.clone());
    }
    if workload_cidrs.is_empty() || !prefixes(hosting).is_subset(&workload_cidrs) {
        return Err(invalid(
            resource,
            "Gateway hosting sources are missing from workload ranges",
        ));
    }
    let mut sandbox_cidrs = BTreeSet::new();
    for id in &declared.sandbox_subnet_ids {
        let properties = subnet(id)?;
        if properties.delegations.len() != 1
            || properties.delegations[0]
                .properties
                .as_ref()
                .and_then(|p| p.service_name.as_deref())
                != Some("Microsoft.App/environments")
        {
            return Err(invalid(
                resource,
                "Sandbox subnets must be delegated to Microsoft.App/environments",
            ));
        }
        if !properties
            .nat_gateway
            .as_ref()
            .and_then(|nat| nat.id.as_deref())
            .is_some_and(|id| id.eq_ignore_ascii_case(nat_id))
        {
            return Err(invalid(
                resource,
                "A sandbox subnet has a different NAT association",
            ));
        }
        sandbox_cidrs.extend(prefixes(properties));
        validate_sandbox_nsg(
            client,
            resource,
            group,
            network_parent,
            properties,
            gateway_cidr,
        )
        .await?;
    }
    let mapped: Vec<_> = declared
        .sandbox_cidrs_by_resource
        .values()
        .flat_map(|cidrs| cidrs.iter().cloned())
        .collect();
    let mapped_set: BTreeSet<_> = mapped.iter().cloned().collect();
    if sandbox_cidrs != declared.sandbox_cidrs.iter().cloned().collect()
        || mapped_set != sandbox_cidrs
        || mapped.len() != mapped_set.len()
        || declared
            .sandbox_cidrs_by_resource
            .values()
            .any(Vec::is_empty)
    {
        return Err(invalid(
            resource,
            "Per-resource sandbox source ranges do not match dedicated Azure subnets",
        ));
    }
    validate_nat(
        client,
        resource,
        group,
        network_parent,
        nat_id,
        &network.egress_public_ips,
    )
    .await?;
    Ok(AzureSandboxNetwork {
        sandbox_cidrs: sandbox_cidrs.into_iter().collect(),
        gateway_cidrs: gateway_cidrs.into_iter().collect(),
        workload_cidrs: workload_cidrs.into_iter().collect(),
        vpc_cidrs: vpc_cidrs.into_iter().collect(),
        ..declared.clone()
    })
}

async fn validate_sandbox_nsg(
    client: &dyn NetworkApi,
    resource: &str,
    group: &str,
    network_parent: &str,
    properties: &SubnetPropertiesFormat,
    gateway_cidr: &str,
) -> Result<()> {
    let missing = || {
        invalid(
            resource,
            "Azure did not return complete sandbox network metadata",
        )
    };
    let nsg_id = properties
        .network_security_group
        .as_ref()
        .and_then(|nsg| nsg.id.as_deref())
        .ok_or_else(|| invalid(resource, "A sandbox subnet has no NSG"))?;
    let nsg_name =
        child_name(nsg_id, network_parent, "networkSecurityGroups").ok_or_else(|| {
            invalid(
                resource,
                "A sandbox NSG is outside the deployment resource group",
            )
        })?;
    let nsg = client
        .get_network_security_group(group, &nsg_name)
        .await
        .context(cloud_error(resource, "GetNetworkSecurityGroup"))?;
    if !nsg
        .id
        .as_deref()
        .is_some_and(|id| id.eq_ignore_ascii_case(nsg_id))
    {
        return Err(invalid(resource, "Azure returned a different sandbox NSG"));
    }
    let rules: Vec<_> = nsg
        .properties
        .as_ref()
        .ok_or_else(missing)?
        .security_rules
        .iter()
        .map(|rule| rule.properties.as_ref().ok_or_else(missing))
        .collect::<Result<_>>()?;
    let mut allowed = 0;
    for rule in rules.iter().filter(|rule| {
        rule.direction == SecurityRuleDirection::Outbound
            && rule.access == SecurityRuleAccess::Allow
    }) {
        let ports: BTreeSet<_> = rule
            .destination_port_ranges
            .iter()
            .map(String::as_str)
            .collect();
        if rule.priority != 100
            || rule.protocol != SecurityRulePropertiesFormatProtocol::Tcp
            || !unrestricted_source(rule)
            || rule.destination_address_prefix.as_deref() != Some(gateway_cidr)
            || !rule.destination_address_prefixes.is_empty()
            || rule.destination_port_range.is_some()
            || ports != BTreeSet::from(["3128", "3129"])
        {
            return Err(invalid(
                resource,
                "Sandbox NSG permits traffic outside the gateway proxy and admin ports",
            ));
        }
        allowed += 1;
    }
    if allowed != 1
        || ![
            ("AzurePlatformIMDS", 101),
            ("AzurePlatformDNS", 140),
            ("*", 300),
        ]
        .iter()
        .all(|(destination, priority)| {
            rules
                .iter()
                .any(|rule| full_deny(rule, destination, *priority))
        })
    {
        return Err(invalid(
            resource,
            "Sandbox NSG is missing its gateway allow or platform/default denies",
        ));
    }
    Ok(())
}

async fn validate_nat(
    client: &dyn NetworkApi,
    resource: &str,
    group: &str,
    network_parent: &str,
    nat_id: &str,
    expected_public_ips: &[String],
) -> Result<()> {
    let missing = || {
        invalid(
            resource,
            "Azure did not return complete NAT gateway metadata",
        )
    };
    let nat_name = child_name(nat_id, network_parent, "natGateways").ok_or_else(missing)?;
    let nat = client
        .get_nat_gateway(group, &nat_name)
        .await
        .context(cloud_error(resource, "GetNatGateway"))?;
    if !nat
        .id
        .as_deref()
        .is_some_and(|id| id.eq_ignore_ascii_case(nat_id))
    {
        return Err(invalid(resource, "Azure returned a different NAT gateway"));
    }
    let nat = nat.properties.as_ref().ok_or_else(missing)?;
    if !nat.public_ip_prefixes.is_empty() {
        return Err(invalid(
            resource,
            "Azure sandbox gateway requires explicit NAT public IP addresses",
        ));
    }
    let mut public_ips = BTreeSet::new();
    for public in &nat.public_ip_addresses {
        let public_id = public.id.as_deref().ok_or_else(missing)?;
        let public_name =
            child_name(public_id, network_parent, "publicIPAddresses").ok_or_else(missing)?;
        let public = client
            .get_public_ip_address(group, &public_name)
            .await
            .context(cloud_error(resource, "GetPublicIPAddress"))?;
        if !public
            .id
            .as_deref()
            .is_some_and(|id| id.eq_ignore_ascii_case(public_id))
        {
            return Err(invalid(
                resource,
                "Azure returned a different NAT public IP",
            ));
        }
        let ip = public
            .properties
            .as_ref()
            .and_then(|p| p.ip_address.clone())
            .ok_or_else(missing)?;
        public_ips.insert(ip);
    }
    if public_ips.is_empty() || public_ips != expected_public_ips.iter().cloned().collect() {
        return Err(invalid(
            resource,
            "Azure NAT public addresses differ from setup registration",
        ));
    }
    Ok(())
}

fn cloud_error(resource: &str, operation: &str) -> ErrorData {
    ErrorData::CloudPlatformError {
        message: format!("{operation} failed while verifying Azure sandbox networking"),
        resource_id: Some(resource.into()),
    }
}

#[cfg(test)]
#[path = "azure_sandbox_tests.rs"]
pub(crate) mod tests;
