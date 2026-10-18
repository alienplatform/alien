//! Validation of container network boundaries.

use super::{Container, ContainerSecurityProfile};
use crate::error::{ErrorData, Result};
use alien_error::AlienError;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Alien-internal gateway placement. Source rules and placement are immutable.
#[doc(hidden)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContainerInternalLoadBalancer {
    pub ports: Vec<u16>,
    pub network: String,
    pub subnet_group: ContainerLoadBalancerSubnetGroup,
    #[serde(deserialize_with = "deserialize_ports")]
    pub allowed_source_groups: BTreeMap<u16, ContainerLoadBalancerSourceGroup>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ContainerLoadBalancerSubnetGroup {
    SandboxGateway,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ContainerLoadBalancerSourceGroup {
    Sandbox,
    Runtime,
}

// Require canonical decimal port keys in serialized stack settings.
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

impl Container {
    /// Whether deployment preflight assigned the managed gateway egress boundary.
    pub fn internet_only_egress(&self) -> bool {
        self.security
            .as_ref()
            .is_some_and(|security| security.egress == Some(super::ContainerEgress::InternetOnly))
    }

    /// Validate network settings before provisioning or updating a container.
    pub fn validate_network_settings(&self) -> Result<()> {
        let invalid = |reason: &str| {
            AlienError::new(ErrorData::InvalidResourceUpdate {
                resource_id: self.id.clone(),
                reason: reason.to_string(),
            })
        };
        if self.internet_only_egress()
            && !self.security.as_ref().is_some_and(|security| {
                security.profile == ContainerSecurityProfile::Restricted
                    && security.run_as_user > 0
                    && security.run_as_group > 0
                    && security.read_only_root_filesystem
            })
        {
            return Err(invalid(
                "security.egress=internet-only requires restricted security, non-root user/group, and a read-only root filesystem",
            ));
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
        if ports.is_empty()
            || ports.len() > 32
            || ports.len() != lb.ports.len()
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
        if lb.network.is_empty() || lb.network.trim() != lb.network {
            return Err(invalid(
                "internalLoadBalancer.network must name a Network resource",
            ));
        }
        if ports != lb.allowed_source_groups.keys().copied().collect() {
            return Err(invalid(
                "internalLoadBalancer.allowedSourceGroups must have exactly one entry for each listener port",
            ));
        }
        Ok(())
    }
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
        let valid = serde_json::json!({"ports":[3128],"network":"default-network","subnetGroup":"sandbox-gateway","allowedSourceGroups":{"3128":"sandbox"}});
        config.internal_load_balancer = Some(serde_json::from_value(valid.clone()).unwrap());
        config.validate_network_settings().unwrap();
        for patch in [
            serde_json::json!({"ports":[3128,3128]}),
            serde_json::json!({"ports":[3129]}),
            serde_json::json!({"network":""}),
            serde_json::json!({"allowedSourceGroups":{}}),
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
        assert!(
            config
                .get_dependencies()
                .iter()
                .any(|dependency| dependency.id == "default-network")
        );
        network["subnets"] = serde_json::json!(["subnet-1"]);
        assert!(serde_json::from_value::<ContainerInternalLoadBalancer>(network).is_err());
    }

    #[test]
    fn internet_only_requires_hardened_execution() {
        let mut config = container();
        config.security = Some(ContainerSecurity {
            profile: ContainerSecurityProfile::Restricted,
            run_as_user: 1000,
            run_as_group: 1000,
            read_only_root_filesystem: true,
            egress: Some(super::super::ContainerEgress::InternetOnly),
        });
        config.validate_network_settings().unwrap();
        config.security.as_mut().unwrap().run_as_user = 0;
        assert!(config.validate_network_settings().is_err());
    }
}
