//! Sandbox links: services of a sandbox's own deployment that every sandbox of that resource
//! reaches through the egress gateway.
//!
//! The stack declares links with `Sandbox::link`. The controller that publishes the gateway
//! configuration turns each one into a [`GatewayLink`]: the exact internal hostname the
//! gateway accepts, the declared ports, and the private address ranges that hostname may
//! resolve into. The gateway refuses every other private destination as before.

use std::net::{IpAddr, Ipv4Addr};

use ipnet::Ipv4Net;
use serde::{Deserialize, Serialize};

/// Most links one sandbox resource may declare.
pub const MAX_SANDBOX_LINKS: usize = 64;

/// The only ranges a link's addresses may come from: the private and shared address spaces a
/// deployment's internal network uses. Loopback, link-local (every cloud metadata address),
/// multicast, reserved and documentation ranges are outside all of them, so no link can
/// reach them whatever its hostname resolves to.
const LINK_ADDRESS_SPACES: &[(Ipv4Addr, u8)] = &[
    (Ipv4Addr::new(10, 0, 0, 0), 8),
    (Ipv4Addr::new(100, 64, 0, 0), 10),
    (Ipv4Addr::new(172, 16, 0, 0), 12),
    (Ipv4Addr::new(192, 168, 0, 0), 16),
];

/// Most fixed addresses one link may carry.
const MAX_LINK_ADDRESSES: usize = 16;

/// Narrowest prefix length a link range may have: no range wider than a /8.
const WIDEST_LINK_PREFIX: u8 = 8;

/// One linked service as the gateway enforces it, keyed in the gateway configuration by the
/// sandbox resource's audience.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GatewayLink {
    /// Resource id of the linked service; a runtime policy names it as `link:<id>`.
    pub id: String,
    /// The exact internal hostname, lowercase, without a trailing dot. A connection reaches
    /// the link only by this name, never by an address.
    pub host: String,
    /// The service's declared ports. Every other port stays refused.
    pub ports: Vec<u16>,
    /// Address ranges the hostname may resolve into, such as the cluster's Service range.
    pub ranges: Vec<Ipv4Net>,
    /// Fixed addresses the deployment owns for this hostname, such as an internal load
    /// balancer's static frontend IP. When present the gateway connects to these instead of
    /// resolving the name; each must lie in `ranges`. Empty means resolve the name.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub addresses: Vec<Ipv4Addr>,
}

impl GatewayLink {
    /// Checks the link before the gateway accepts a configuration containing it.
    pub fn validate(&self) -> Result<(), String> {
        crate::stack_authoring::validate_resource_id(&self.id)
            .map_err(|reason| format!("link id '{}' {reason}", self.id))?;
        if self.host.is_empty()
            || self.host.len() > 253
            || self.host.parse::<Ipv4Addr>().is_ok()
            || !self.host.split('.').all(|label| {
                !label.is_empty()
                    && label.len() <= 63
                    && !label.starts_with('-')
                    && !label.ends_with('-')
                    && label
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            })
        {
            return Err(format!(
                "link '{}': host must be a lowercase DNS name",
                self.id
            ));
        }
        if self.ports.is_empty() || self.ports.iter().any(|&port| port == 0 || port == 53) {
            return Err(format!(
                "link '{}': ports must be nonzero, not 53, and at least one",
                self.id
            ));
        }
        if self.ranges.is_empty() {
            return Err(format!(
                "link '{}': at least one range is required",
                self.id
            ));
        }
        for range in &self.ranges {
            if !is_link_range(*range) {
                return Err(format!(
                    "link '{}': range {range} must be a private range no wider than /8",
                    self.id
                ));
            }
        }
        if self.addresses.len() > MAX_LINK_ADDRESSES {
            return Err(format!(
                "link '{}': at most {MAX_LINK_ADDRESSES} fixed addresses",
                self.id
            ));
        }
        if let Some(address) = self
            .addresses
            .iter()
            .find(|address| !self.admits(IpAddr::V4(**address)))
        {
            return Err(format!(
                "link '{}': address {address} must lie in the link's ranges",
                self.id
            ));
        }
        Ok(())
    }

    /// Whether a connection to `host:port` names this link.
    pub fn matches(&self, host: &str, port: u16) -> bool {
        self.host == host && self.ports.contains(&port)
    }

    /// Whether `address` is one this link's hostname may resolve to. IPv6 never is.
    pub fn admits(&self, address: IpAddr) -> bool {
        let IpAddr::V4(address) = address else {
            return false;
        };
        is_link_address(address) && self.ranges.iter().any(|range| range.contains(&address))
    }
}

/// Whether `range` lies wholly inside a deployment-internal address space and is no wider
/// than a /8.
pub fn is_link_range(range: Ipv4Net) -> bool {
    range.prefix_len() >= WIDEST_LINK_PREFIX
        && link_address_spaces().any(|space| space.contains(&range))
}

/// Whether `address` lies in a deployment-internal address space.
pub fn is_link_address(address: Ipv4Addr) -> bool {
    link_address_spaces().any(|space| space.contains(&address))
}

fn link_address_spaces() -> impl Iterator<Item = Ipv4Net> {
    LINK_ADDRESS_SPACES
        .iter()
        .map(|&(address, prefix)| Ipv4Net::new_assert(address, prefix))
}

/// A resource a sandbox may link: its internal endpoint as the egress gateway reaches it.
///
/// One answer per resource type. A type without an endpoint inside the deployment's network
/// is not linkable and does not implement this.
pub trait InternalEndpoints {
    /// The ports the resource serves inside the deployment, or why it serves none.
    fn internal_ports(&self) -> Result<Vec<u16>, String>;
}

impl InternalEndpoints for crate::Container {
    fn internal_ports(&self) -> Result<Vec<u16>, String> {
        let mut ports: Vec<u16> = self.ports.iter().map(|port| port.port).collect();
        ports.sort_unstable();
        ports.dedup();
        if ports.is_empty() {
            return Err("it declares no port, so it has no internal endpoint".into());
        }
        Ok(ports)
    }
}

/// The linkable resource `id` in `stack` and its internal ports, or why it cannot be linked.
pub fn internal_ports(stack: &crate::Stack, id: &str) -> Result<Vec<u16>, String> {
    let entry = stack
        .resources
        .get(id)
        .ok_or_else(|| "no such resource in the stack".to_string())?;
    if crate::sandbox_egress::is_managed_gateway(stack, id) {
        return Err("it is the deployment's egress gateway".into());
    }
    if let Some(container) = entry.config.downcast_ref::<crate::Container>() {
        return container.internal_ports();
    }
    Err(format!(
        "a {} is not a linkable service; link a Container with a declared port",
        entry.config.resource_type()
    ))
}

/// Why `platform` cannot route a sandbox's gateway to a linked service, if it cannot.
pub fn platform_refusal(platform: crate::Platform) -> Option<&'static str> {
    match platform {
        crate::Platform::Aws | crate::Platform::Kubernetes | crate::Platform::Local => None,
        // Follows Google Cloud internet sandboxes, which are not available yet.
        crate::Platform::Gcp => Some(
            "links are not available on Google Cloud yet: they go through the deployment's \
             egress gateway, which Google Cloud internet sandboxes do not use yet",
        ),
        crate::Platform::Azure => Some(
            "links are not available on Azure in this release: the gateway runs on VMs outside \
             the container network",
        ),
        _ => Some("links are not available on this platform"),
    }
}

/// Checks a sandbox's declared links against the stack and the platform at plan time.
pub fn validate_sandbox_links(
    stack: &crate::Stack,
    sandbox: &crate::Sandbox,
    platform: crate::Platform,
) -> Result<(), String> {
    if sandbox.gateway_links.is_empty() {
        return Ok(());
    }
    if let Some(reason) = platform_refusal(platform) {
        return Err(reason.into());
    }
    if sandbox.egress != crate::SandboxEgress::Internet {
        return Err(
            "links go through the deployment's egress gateway and need egress `internet`".into(),
        );
    }
    if sandbox.gateway_links.len() > MAX_SANDBOX_LINKS {
        return Err(format!("at most {MAX_SANDBOX_LINKS} links are allowed"));
    }
    if !crate::sandbox_egress::gateway_image_serves_links(
        crate::sandbox_egress::planned_gateway_image(stack),
    ) {
        return Err(
            "the egress gateway image this release installs cannot enforce links; \
             a later release enables them"
                .into(),
        );
    }
    for link in &sandbox.gateway_links {
        internal_ports(stack, &link.id)
            .map_err(|reason| format!("link '{}': {reason}", link.id))?;
    }
    Ok(())
}

/// Every service some sandbox of `stack` links, with its internal ports. The gateway's own
/// network rules open exactly these.
pub fn gateway_link_targets(stack: &crate::Stack) -> std::collections::BTreeMap<String, Vec<u16>> {
    stack
        .resources()
        .filter_map(|(_, entry)| entry.config.downcast_ref::<crate::Sandbox>())
        .filter(|sandbox| sandbox.egress == crate::SandboxEgress::Internet)
        .flat_map(|sandbox| &sandbox.gateway_links)
        .filter_map(|link| Some((link.id.clone(), internal_ports(stack, &link.id).ok()?)))
        .collect()
}

/// A linked Container as a gateway reaches it on a compute cluster: its overlay name
/// `<id>.svc`, its declared ports, and the overlay range of the cluster it runs in. `None` when
/// it is not linkable, has no cluster yet, or its cluster's overlay is not a private range.
pub fn cluster_gateway_link(stack: &crate::Stack, id: &str) -> Option<GatewayLink> {
    let ports = internal_ports(stack, id).ok()?;
    let container = stack
        .resources
        .get(id)?
        .config
        .downcast_ref::<crate::Container>()?;
    let cluster = stack
        .resources
        .get(container.cluster.as_deref()?)?
        .config
        .downcast_ref::<crate::ComputeCluster>()?;
    let range: Ipv4Net = cluster.container_cidr().parse().ok()?;
    is_link_range(range).then(|| GatewayLink {
        id: id.to_string(),
        host: format!("{}.svc", id.to_ascii_lowercase()),
        ports,
        ranges: vec![range],
        addresses: Vec::new(),
    })
}

/// Every internet sandbox's compute-cluster links, keyed by its audience `<prefix>/<id>`. Empty for a
/// stack without links, so its gateway configuration does not change.
pub fn cluster_gateway_links(
    stack: &crate::Stack,
    resource_prefix: &str,
) -> std::collections::BTreeMap<String, Vec<GatewayLink>> {
    stack
        .resources()
        .filter_map(|(_, entry)| entry.config.downcast_ref::<crate::Sandbox>())
        .filter(|sandbox| sandbox.egress == crate::SandboxEgress::Internet)
        .filter_map(|sandbox| {
            let links: Vec<_> = sandbox
                .gateway_links
                .iter()
                .filter_map(|link| cluster_gateway_link(stack, &link.id))
                .collect();
            (!links.is_empty()).then(|| (format!("{resource_prefix}/{}", sandbox.id), links))
        })
        .collect()
}

/// The bindings a sandbox's commands receive for its compute-cluster links: each linked Container's
/// overlay URLs, never a load balancer or public URL.
pub fn cluster_link_bindings(
    stack: &crate::Stack,
    sandbox: &crate::Sandbox,
) -> std::collections::BTreeMap<String, crate::bindings::ContainerBinding> {
    use crate::bindings::{BindingValue, ContainerBinding, HorizonContainerBinding};
    sandbox
        .gateway_links
        .iter()
        .filter_map(|link| {
            let link = cluster_gateway_link(stack, &link.id)?;
            let url = |port: u16| format!("http://{}:{port}", link.host);
            Some((
                link.id.clone(),
                ContainerBinding::Horizon(HorizonContainerBinding {
                    container_name: BindingValue::value(link.id.clone()),
                    internal_url: BindingValue::value(url(link.ports[0])),
                    public_url: None,
                    internal_urls: link
                        .ports
                        .iter()
                        .map(|&port| (port, BindingValue::value(url(port))))
                        .collect(),
                }),
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link() -> GatewayLink {
        GatewayLink {
            id: "api".into(),
            host: "api.svc".into(),
            ports: vec![8080],
            addresses: Vec::new(),
            ranges: vec!["10.244.0.0/16".parse().unwrap()],
        }
    }

    #[test]
    fn a_link_admits_only_its_ranges_inside_internal_address_spaces() {
        let link = link();
        link.validate().unwrap();
        assert!(link.admits("10.244.3.7".parse().unwrap()));
        for address in [
            "10.0.0.1",
            "169.254.169.254",
            "127.0.0.1",
            "168.63.129.16",
            "8.8.8.8",
            "fd00:ec2::254",
            "fe80::1",
            "::ffff:10.244.3.7",
        ] {
            assert!(!link.admits(address.parse().unwrap()), "{address}");
        }
        assert!(link.matches("api.svc", 8080));
        assert!(!link.matches("api.svc", 8081));
        assert!(!link.matches("other.svc", 8080));
    }

    #[test]
    fn validation_refuses_ranges_outside_internal_spaces_and_bad_fields() {
        for range in [
            "169.254.0.0/16",
            "127.0.0.0/8",
            "0.0.0.0/0",
            "8.8.8.0/24",
            "168.63.129.16/32",
            "10.0.0.0/7",
            "172.0.0.0/8",
            "192.168.0.0/15",
        ] {
            let mut link = link();
            link.ranges = vec![range.parse().unwrap()];
            assert!(link.validate().is_err(), "{range}");
        }
        for mutate in [
            (|l: &mut GatewayLink| l.host = "API.svc".into()) as fn(&mut GatewayLink),
            |l| l.host = "10.244.0.1".into(),
            |l| l.host = "api.svc.".into(),
            |l| l.ports = vec![],
            |l| l.ports = vec![53],
            |l| l.ports = vec![0],
            |l| l.ranges = vec![],
            |l| l.id = "Api".into(),
        ] {
            let mut link = link();
            mutate(&mut link);
            assert!(link.validate().is_err(), "{link:?}");
        }
    }
}

#[cfg(test)]
mod builder_tests {
    use super::{cluster_gateway_links, cluster_link_bindings, GatewayLink};
    use crate::{Container, ContainerCode, ResourceSpec, Sandbox, SandboxCode, SandboxEgress};

    #[test]
    fn link_records_each_service_once_and_a_sandbox_without_links_serializes_unchanged() {
        let api = Container::new("api".into())
            .code(ContainerCode::Image {
                image: "example.com/api:1".into(),
            })
            .permissions("execution".into())
            .cpu(ResourceSpec {
                min: "1".into(),
                desired: "1".into(),
            })
            .memory(ResourceSpec {
                min: "1Gi".into(),
                desired: "1Gi".into(),
            })
            .port(8080)
            .build();
        let build = |linked: bool| {
            let builder = Sandbox::new("agents".into());
            let builder = if linked {
                builder.link(&api).link(&api)
            } else {
                builder
            };
            builder
                .code(SandboxCode::Image {
                    image: "ubuntu".into(),
                    bundled_from: None,
                })
                .egress(SandboxEgress::Internet)
                .lifecycle(Default::default())
                .build()
        };
        let linked = build(true);
        assert_eq!(linked.gateway_links.len(), 1);
        assert_eq!(linked.gateway_links[0].id, "api");
        // Links are not lifecycle dependencies: the linked service may link the sandbox.
        assert!(crate::ResourceDefinition::get_dependencies(&linked).is_empty());
        let json = serde_json::to_value(&linked).unwrap();
        assert_eq!(json["gatewayLinks"][0]["id"], "api");
        let plain = serde_json::to_value(build(false)).unwrap();
        assert!(plain.get("gatewayLinks").is_none());
        assert_eq!(serde_json::from_value::<Sandbox>(json).unwrap(), linked);
    }

    #[test]
    fn cluster_links_name_the_overlay_service_inside_its_cluster_range() {
        use crate::{
            CapacityGroup, ComputeCluster, Container, ContainerCode, ResourceLifecycle,
            ResourceSpec, SandboxCode, SandboxEgress, Stack,
        };
        let cluster = |cidr: &str| {
            ComputeCluster::new("compute".into())
                .capacity_group(CapacityGroup {
                    group_id: "apps".into(),
                    instance_type: Some("m7g.large".into()),
                    profile: None,
                    min_size: 1,
                    max_size: 1,
                    scale_policy: None,
                    nested_virtualization: None,
                    data_disk_gib_range: None,
                })
                .container_cidr(cidr.into())
                .build()
        };
        let api = Container::new("api".into())
            .code(ContainerCode::Image {
                image: "api".into(),
            })
            .cpu(ResourceSpec {
                min: "1".into(),
                desired: "1".into(),
            })
            .memory(ResourceSpec {
                min: "1Gi".into(),
                desired: "1Gi".into(),
            })
            .permissions("default".into())
            .cluster("compute".into())
            .port(8080)
            .build();
        let sandbox = |id: &str, linked: bool| {
            let builder = crate::Sandbox::new(id.into());
            let builder = if linked { builder.link(&api) } else { builder };
            builder
                .code(SandboxCode::Image {
                    image: "ubuntu".into(),
                    bundled_from: None,
                })
                .egress(SandboxEgress::Internet)
                .lifecycle(Default::default())
                .build()
        };
        let stack = |cidr: &str, linked: bool| {
            Stack::new("s".into())
                .add(cluster(cidr), ResourceLifecycle::Frozen)
                .add(api.clone(), ResourceLifecycle::Live)
                .add(sandbox("agents", linked), ResourceLifecycle::Live)
                .add(sandbox("plain", false), ResourceLifecycle::Live)
                .build()
        };
        let linked = stack("10.244.0.0/16", true);
        let links = cluster_gateway_links(&linked, "p");
        assert_eq!(links.keys().collect::<Vec<_>>(), ["p/agents"]);
        assert_eq!(
            links["p/agents"],
            vec![GatewayLink {
                id: "api".into(),
                host: "api.svc".into(),
                ports: vec![8080],
                ranges: vec!["10.244.0.0/16".parse().unwrap()],
                addresses: Vec::new(),
            }]
        );
        links["p/agents"][0].validate().unwrap();
        let bindings = cluster_link_bindings(&linked, &sandbox("agents", true));
        let binding = &bindings["api"];
        assert_eq!(
            binding.get_internal_url().as_deref(),
            Some("http://api.svc:8080")
        );
        assert!(binding.get_public_url().is_none());
        // Without links nothing is published; a public overlay is never a link range.
        assert!(cluster_gateway_links(&stack("10.244.0.0/16", false), "p").is_empty());
        assert!(cluster_gateway_links(&stack("11.244.0.0/16", true), "p").is_empty());
    }
}
