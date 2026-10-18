//! Validation of container network boundaries.

use super::{Container, ContainerSecurityProfile};
use crate::error::{ErrorData, Result};
use alien_error::AlienError;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::net::Ipv4Addr;

/// A dedicated internal TCP load balancer for a single container.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContainerInternalLoadBalancer {
    /// TCP listener ports, each matching a declared container port.
    pub ports: Vec<u16>,
    /// Existing subnet identifiers in the container's network.
    pub subnets: Vec<String>,
    /// Allowed IPv4 source CIDRs for each listener port. Every port must have
    /// an entry. An empty list denies all clients on that port.
    pub allowed_sources: BTreeMap<u16, Vec<String>>,
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
        let ports: BTreeSet<_> = lb.ports.iter().copied().collect();
        if ports.is_empty() || ports.len() != lb.ports.len() || ports.contains(&0) {
            return Err(invalid(
                "internalLoadBalancer.ports must contain unique nonzero ports",
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
        if ports != lb.allowed_sources.keys().copied().collect() {
            return Err(invalid("internalLoadBalancer.allowedSources must have exactly one entry for each listener port"));
        }
        let subnets: BTreeSet<_> = lb.subnets.iter().collect();
        if subnets.is_empty()
            || subnets.len() != lb.subnets.len()
            || subnets
                .iter()
                .any(|subnet| subnet.is_empty() || subnet.trim() != subnet.as_str())
        {
            return Err(invalid(
                "internalLoadBalancer.subnets must contain unique nonempty subnet identifiers",
            ));
        }
        for cidr in lb.allowed_sources.values().flatten() {
            if !is_ipv4_network(cidr) {
                return Err(invalid(&format!(
                    "internalLoadBalancer.allowedSources contains an invalid IPv4 network: {cidr}"
                )));
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
        config.internal_load_balancer = Some(ContainerInternalLoadBalancer {
            ports: vec![3128],
            subnets: vec!["subnet-1".into()],
            allowed_sources: BTreeMap::from([(3128, vec!["10.2.0.0/24".into()])]),
        });
        config.validate_network_settings().unwrap();
        let valid = config.internal_load_balancer.clone().unwrap();
        for invalid in [
            ContainerInternalLoadBalancer {
                ports: vec![3128, 3128],
                ..valid.clone()
            },
            ContainerInternalLoadBalancer {
                ports: vec![3129],
                ..valid.clone()
            },
            ContainerInternalLoadBalancer {
                subnets: vec![],
                ..valid.clone()
            },
            ContainerInternalLoadBalancer {
                allowed_sources: BTreeMap::new(),
                ..valid.clone()
            },
            ContainerInternalLoadBalancer {
                allowed_sources: BTreeMap::from([(3128, vec!["10.2.0.1/24".into()])]),
                ..valid.clone()
            },
        ] {
            config.internal_load_balancer = Some(invalid);
            assert!(config.validate_network_settings().is_err());
        }
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
