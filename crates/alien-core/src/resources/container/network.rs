//! Validation of container network boundaries.

use super::{Container, ContainerSecurityProfile};
use crate::error::{ErrorData, Result};
use alien_error::AlienError;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::net::Ipv4Addr;

/// A dedicated internal TCP load balancer for one container. Its placement and
/// source rules are immutable; replace the resource to change the boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(untagged, rename_all_fields = "camelCase", deny_unknown_fields)]
pub enum ContainerInternalLoadBalancer {
    /// Explicit, existing subnet identifiers and allowed IPv4 sources.
    Subnets {
        ports: Vec<u16>,
        subnets: Vec<String>,
        /// Exactly one entry per listener. An empty list denies every client.
        #[serde(rename = "allowedSources", deserialize_with = "deserialize_ports")]
        allowed_sources: BTreeMap<u16, Vec<String>>,
    },
    /// Resolve setup-created subnet identifiers and CIDRs from a Network.
    Network {
        ports: Vec<u16>,
        network: String,
        #[serde(rename = "subnetGroup")]
        subnet_group: ContainerLoadBalancerSubnetGroup,
        #[serde(rename = "allowedSourceGroups", deserialize_with = "deserialize_ports")]
        allowed_source_groups: BTreeMap<u16, ContainerLoadBalancerSourceGroup>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "kebab-case")]
pub enum ContainerLoadBalancerSubnetGroup {
    SandboxGateway,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub enum ContainerLoadBalancerSourceGroup {
    Sandbox,
    Runtime,
}

// Untagged deserialization buffers JSON keys as strings; decode them explicitly.
fn deserialize_ports<'de, D, V>(deserializer: D) -> std::result::Result<BTreeMap<u16, V>, D::Error>
where
    D: serde::Deserializer<'de>,
    V: Deserialize<'de>,
{
    BTreeMap::<String, V>::deserialize(deserializer)?
        .into_iter()
        .map(|(port, value)| {
            let parsed = port.parse::<u16>().map_err(serde::de::Error::custom)?;
            if parsed.to_string() != port {
                return Err(serde::de::Error::custom(
                    "listener port keys must be canonical decimal numbers",
                ));
            }
            Ok((parsed, value))
        })
        .collect()
}

impl ContainerInternalLoadBalancer {
    pub fn ports(&self) -> &[u16] {
        match self {
            Self::Subnets { ports, .. } | Self::Network { ports, .. } => ports,
        }
    }

    pub fn network(&self) -> Option<&str> {
        match self {
            Self::Network { network, .. } => Some(network),
            Self::Subnets { .. } => None,
        }
    }
}

impl Container {
    /// Validate network settings before provisioning or updating a container.
    pub fn validate_network_settings(&self) -> Result<()> {
        let invalid = |reason: &str| {
            AlienError::new(ErrorData::InvalidResourceUpdate {
                resource_id: self.id.clone(),
                reason: reason.to_string(),
            })
        };
        if self.internet_only_egress
            && !self.security.as_ref().is_some_and(|security| {
                security.profile == ContainerSecurityProfile::Restricted
                    && security.run_as_user > 0
                    && security.run_as_group > 0
                    && security.read_only_root_filesystem
            })
        {
            return Err(invalid("internetOnlyEgress requires restricted security, non-root user/group, and a read-only root filesystem"));
        }
        let Some(lb) = &self.internal_load_balancer else {
            return Ok(());
        };
        if !self.public_endpoints.is_empty() || self.tunnel.is_some() {
            return Err(invalid(
                "internalLoadBalancer cannot be combined with public endpoints or a tunnel",
            ));
        }
        let ports: BTreeSet<_> = lb.ports().iter().copied().collect();
        if ports.is_empty()
            || ports.len() > 32
            || ports.len() != lb.ports().len()
            || ports.contains(&0)
        {
            return Err(invalid(
                "internalLoadBalancer.ports must contain 1-32 unique nonzero ports",
            ));
        }
        if ports
            .iter()
            .any(|port| !self.ports.iter().any(|declared| declared.port == *port))
        {
            return Err(invalid(
                "internalLoadBalancer.ports must reference declared container ports",
            ));
        }
        match lb {
            ContainerInternalLoadBalancer::Subnets {
                subnets,
                allowed_sources,
                ..
            } => {
                if ports != allowed_sources.keys().copied().collect() {
                    return Err(invalid("internalLoadBalancer.allowedSources must have exactly one entry for each listener port"));
                }
                let unique: BTreeSet<_> = subnets.iter().collect();
                if unique.is_empty()
                    || unique.len() != subnets.len()
                    || subnets
                        .iter()
                        .any(|subnet| subnet.is_empty() || subnet.trim() != subnet)
                {
                    return Err(invalid("internalLoadBalancer.subnets must contain unique nonempty subnet identifiers"));
                }
                for cidr in allowed_sources.values().flatten() {
                    if !is_ipv4_network(cidr) {
                        return Err(invalid(&format!("internalLoadBalancer.allowedSources contains an invalid IPv4 network: {cidr}")));
                    }
                }
            }
            ContainerInternalLoadBalancer::Network {
                network,
                allowed_source_groups,
                ..
            } => {
                if network.is_empty() || network.trim() != network {
                    return Err(invalid(
                        "internalLoadBalancer.network must name a Network resource",
                    ));
                }
                if ports != allowed_source_groups.keys().copied().collect() {
                    return Err(invalid("internalLoadBalancer.allowedSourceGroups must have exactly one entry for each listener port"));
                }
            }
        }
        Ok(())
    }
}

fn is_ipv4_network(cidr: &str) -> bool {
    let Some((address, prefix)) = cidr.split_once('/') else {
        return false;
    };
    let (Ok(address), Ok(prefix)) = (address.parse::<Ipv4Addr>(), prefix.parse::<u32>()) else {
        return false;
    };
    prefix <= 32 && (u32::from(address) & (u32::MAX.checked_shr(prefix).unwrap_or(0))) == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ContainerCode, ContainerSecurity, ResourceSpec};

    fn container() -> Container {
        Container::new("gateway".into())
            .code(ContainerCode::Image {
                image: "example:latest".into(),
            })
            .cpu(ResourceSpec {
                min: "1".into(),
                desired: "1".into(),
            })
            .memory(ResourceSpec {
                min: "128Mi".into(),
                desired: "128Mi".into(),
            })
            .permissions("gateway".into())
            .port(3128)
            .build()
    }

    #[test]
    fn rejects_incomplete_or_ambiguous_listener_rules() {
        let mut config = container();
        let valid = serde_json::json!({"ports":[3128],"subnets":["subnet-1"],"allowedSources":{"3128":["10.2.0.0/24"]}});
        config.internal_load_balancer = Some(serde_json::from_value(valid.clone()).unwrap());
        config.validate_network_settings().unwrap();
        for patch in [
            serde_json::json!({"ports":[3128,3128]}),
            serde_json::json!({"ports":[3129]}),
            serde_json::json!({"subnets":[]}),
            serde_json::json!({"allowedSources":{}}),
            serde_json::json!({"allowedSources":{"3128":["10.2.0.1/24"]}}),
        ] {
            let mut invalid = valid.clone();
            invalid
                .as_object_mut()
                .unwrap()
                .extend(patch.as_object().unwrap().clone());
            config.internal_load_balancer = Some(serde_json::from_value(invalid).unwrap());
            assert!(config.validate_network_settings().is_err());
        }
    }

    #[test]
    fn network_placement_is_unambiguous_and_adds_a_dependency() {
        use crate::ResourceDefinition;
        let mut config = container();
        let mut network = serde_json::json!({"ports":[3128],"network":"default-network","subnetGroup":"sandbox-gateway","allowedSourceGroups":{"3128":"sandbox"}});
        config.internal_load_balancer = Some(serde_json::from_value(network.clone()).unwrap());
        config.validate_network_settings().unwrap();
        assert!(config
            .get_dependencies()
            .iter()
            .any(|dependency| dependency.id == "default-network"));
        network["subnets"] = serde_json::json!(["subnet-1"]);
        assert!(serde_json::from_value::<ContainerInternalLoadBalancer>(network).is_err());
    }

    #[test]
    fn internet_only_requires_hardened_execution() {
        let mut config = container();
        config.internet_only_egress = true;
        assert!(config.validate_network_settings().is_err());
        config.security = Some(ContainerSecurity {
            profile: ContainerSecurityProfile::Restricted,
            run_as_user: 1000,
            run_as_group: 1000,
            read_only_root_filesystem: true,
        });
        config.validate_network_settings().unwrap();
        config.security.as_mut().unwrap().run_as_user = 0;
        assert!(config.validate_network_settings().is_err());
    }

    #[test]
    fn validates_cidr_boundaries_without_truncating_host_bits() {
        for cidr in ["0.0.0.0/0", "10.2.0.0/24", "10.2.0.1/32"] {
            assert!(is_ipv4_network(cidr), "{cidr}");
        }
        for cidr in ["10.2.0.1/24", "10.0.0.0/33", "::/0", "10.0.0.0", "garbage"] {
            assert!(!is_ipv4_network(cidr), "{cidr}");
        }
    }
}
