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
pub const MAX_SANDBOX_LINKS: usize = 16;

/// Most services the sandboxes of one deployment may link in total: the gateway's own network
/// rules open each one, and the compute cluster's runtime takes at most this many.
pub const MAX_GATEWAY_LINKED_SERVICES: usize = 16;

/// Most ports one linked service may declare.
pub const MAX_LINK_PORTS: usize = 16;

/// Ports a link never opens: the gateway answers DNS itself, so a link cannot name port 53.
const REFUSED_LINK_PORTS: &[u16] = &[0, 53];

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
        if self.ports.is_empty()
            || self.ports.len() > MAX_LINK_PORTS
            || self
                .ports
                .iter()
                .any(|port| REFUSED_LINK_PORTS.contains(port))
        {
            return Err(format!(
                "link '{}': ports must be 1 to {MAX_LINK_PORTS}, nonzero and not 53",
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
        // Azure fronts each linked container with an internal load balancer; links ship with
        // the gateway image that serves them, as Azure internet sandboxes do.
        crate::Platform::Aws
        | crate::Platform::Kubernetes
        | crate::Platform::Local
        | crate::Platform::Azure => None,
        // Follows Google Cloud internet sandboxes, which are not available yet.
        crate::Platform::Gcp => Some(
            "links are not available on Google Cloud yet: they go through the deployment's \
             egress gateway, which Google Cloud internet sandboxes do not use yet",
        ),
        _ => Some("links are not available on this platform"),
    }
}

/// Why Azure cannot front the linked container `id` with an internal load balancer, if it
/// cannot. A container on Azure joins exactly one load balancer pool through one host
/// port, so the container must have no public endpoint (which already takes that pool) and
/// exactly one port.
pub fn azure_link_refusal(stack: &crate::Stack, id: &str) -> Option<String> {
    let container = stack
        .resources
        .get(id)?
        .config
        .downcast_ref::<crate::Container>()?;
    if !container.public_endpoints.is_empty() {
        return Some(
            "on Azure a linked container cannot also have a public endpoint: it joins one \
             load balancer pool, which the public endpoint already uses"
                .into(),
        );
    }
    if container
        .internal_ports()
        .is_ok_and(|ports| ports.len() != 1)
    {
        return Some(
            "on Azure a linked container must declare exactly one port: its internal load \
             balancer forwards one host port"
                .into(),
        );
    }
    None
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
    let linked_services = gateway_link_targets(stack).len();
    if linked_services > MAX_GATEWAY_LINKED_SERVICES {
        return Err(format!(
            "the deployment's sandboxes link {linked_services} services; at most \
             {MAX_GATEWAY_LINKED_SERVICES} are allowed"
        ));
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
        let ports = internal_ports(stack, &link.id)
            .map_err(|reason| format!("link '{}': {reason}", link.id))?;
        if ports.len() > MAX_LINK_PORTS {
            return Err(format!(
                "link '{}': a linked service may declare at most {MAX_LINK_PORTS} ports",
                link.id
            ));
        }
        if let Some(port) = ports.iter().find(|port| REFUSED_LINK_PORTS.contains(port)) {
            return Err(format!(
                "link '{}': port {port} cannot be linked; the egress gateway answers DNS itself",
                link.id
            ));
        }
        if platform == crate::Platform::Azure {
            if let Some(reason) = azure_link_refusal(stack, &link.id) {
                return Err(format!("link '{}': {reason}", link.id));
            }
        }
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
    let link = GatewayLink {
        id: id.to_string(),
        host: format!("{}.svc", id.to_ascii_lowercase()),
        ports,
        ranges: vec![range],
        addresses: Vec::new(),
    };
    // The gateway refuses a whole configuration with one bad link, so one is never published.
    link.validate().is_ok().then_some(link)
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

/// Every internet sandbox's links where the deployment owns a fixed address for each linked
/// Container (on Azure, its internal load balancer's static frontend IP), keyed by audience
/// `<prefix>/<id>`. Each link keeps the overlay name `<id>.svc` and the declared ports, and
/// carries that one address, which must lie in `ranges` (on Azure, the private subnet the load
/// balancers' frontends sit in). A link whose Container has no address yet, or one outside
/// `ranges`, is left out. Empty for a stack without links.
pub fn addressed_gateway_links(
    stack: &crate::Stack,
    resource_prefix: &str,
    ranges: &[Ipv4Net],
    address_of: impl Fn(&str) -> Option<Ipv4Addr>,
) -> std::collections::BTreeMap<String, Vec<GatewayLink>> {
    stack
        .resources()
        .filter_map(|(_, entry)| entry.config.downcast_ref::<crate::Sandbox>())
        .filter(|sandbox| sandbox.egress == crate::SandboxEgress::Internet)
        .filter_map(|sandbox| {
            let links: Vec<_> = sandbox
                .gateway_links
                .iter()
                .filter_map(|link| {
                    let address = address_of(&link.id)?;
                    let link = GatewayLink {
                        id: link.id.clone(),
                        host: format!("{}.svc", link.id.to_ascii_lowercase()),
                        ports: internal_ports(stack, &link.id).ok()?,
                        ranges: ranges.to_vec(),
                        addresses: vec![address],
                    };
                    link.validate().is_ok().then_some(link)
                })
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
        // A link the gateway would refuse (here port 53) is never published, so it cannot stop
        // the gateway from loading every other sandbox's configuration.
        let mut dns = stack("10.244.0.0/16", true);
        dns.resources
            .get_mut("api")
            .unwrap()
            .config
            .downcast_mut::<Container>()
            .unwrap()
            .ports[0]
            .port = 53;
        assert!(cluster_gateway_links(&dns, "p").is_empty());
    }
}

#[cfg(test)]
mod azure_tests {
    use super::azure_link_refusal;
    use crate::{
        Container, ContainerCode, ExposeProtocol, PublicEndpoint, ResourceLifecycle, ResourceSpec,
        Stack,
    };

    fn container(id: &str, ports: &[u16], public: bool) -> Container {
        let mut builder = Container::new(id.into())
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
            });
        for &port in ports {
            builder = builder.port(port);
        }
        if public {
            builder = builder.public_endpoint(PublicEndpoint {
                name: "web".into(),
                port: ports[0],
                protocol: ExposeProtocol::Http,
                host_label: None,
                wildcard_subdomains: false,
            });
        }
        builder.build()
    }

    #[test]
    fn azure_links_a_single_port_container_without_a_public_endpoint() {
        let stack = Stack::new("s".into())
            .add(container("api", &[8080], false), ResourceLifecycle::Live)
            .add(
                container("two", &[8080, 9090], false),
                ResourceLifecycle::Live,
            )
            .add(container("web", &[8080], true), ResourceLifecycle::Live)
            .build();
        assert_eq!(azure_link_refusal(&stack, "api"), None);
        assert!(azure_link_refusal(&stack, "two")
            .unwrap()
            .contains("exactly one port"));
        assert!(azure_link_refusal(&stack, "web")
            .unwrap()
            .contains("public endpoint"));
    }
}

#[cfg(test)]
mod addressed_tests {
    use super::addressed_gateway_links;
    use crate::{
        Container, ContainerCode, ResourceLifecycle, ResourceSpec, Sandbox, SandboxCode,
        SandboxEgress, Stack,
    };

    #[test]
    fn addressed_links_carry_the_owned_address_and_admit_only_it() {
        let container = |id: &str| {
            Container::new(id.into())
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
                .port(8080)
                .build()
        };
        let (api, billing) = (container("api"), container("billing"));
        let sandbox = Sandbox::new("agents".into())
            .link(&api)
            .link(&billing)
            .code(SandboxCode::Image {
                image: "ubuntu".into(),
                bundled_from: None,
            })
            .egress(SandboxEgress::Internet)
            .lifecycle(Default::default())
            .build();
        let stack = Stack::new("s".into())
            .add(api, ResourceLifecycle::Live)
            .add(billing, ResourceLifecycle::Live)
            .add(sandbox, ResourceLifecycle::Live)
            .build();
        // billing has no address yet; a metadata address is never accepted.
        let private: Vec<ipnet::Ipv4Net> = vec!["10.0.2.0/24".parse().unwrap()];
        let links = addressed_gateway_links(&stack, "prefix", &private, |id| match id {
            "api" => Some("10.0.2.9".parse().unwrap()),
            _ => None,
        });
        let agents = &links["prefix/agents"];
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].host, "api.svc");
        assert_eq!(agents[0].ports, vec![8080]);
        assert_eq!(
            agents[0].addresses,
            vec!["10.0.2.9".parse::<std::net::Ipv4Addr>().unwrap()]
        );
        assert_eq!(agents[0].ranges, private);
        assert!(agents[0].validate().is_ok());
        for outside in ["169.254.169.254", "10.0.3.9", "172.16.0.4"] {
            assert!(
                addressed_gateway_links(&stack, "prefix", &private, |_| Some(
                    outside.parse().unwrap()
                ))
                .is_empty(),
                "{outside}"
            );
        }
        assert!(addressed_gateway_links(&stack, "prefix", &private, |_| None).is_empty());
    }
}
