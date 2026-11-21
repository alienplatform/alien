//! Runtime-owned Container endpoint and outbound policies.
use super::kubernetes::KubernetesContainerController;
use crate::core::{
    kubernetes_branded_resource_labels, kubernetes_cleanup_resource_labels,
    ResourceControllerContext,
};
use crate::error::{ErrorData, Result};
use alien_client_core::ErrorData as ClientError;
use alien_core::{Container, Daemon, Stack};
use alien_error::{AlienError, Context, ContextError};
use k8s_openapi::api::core::v1::{PodDNSConfig, PodDNSConfigOption};
use k8s_openapi::api::networking::v1::{
    IPBlock, NetworkPolicy, NetworkPolicyEgressRule, NetworkPolicyIngressRule, NetworkPolicyPeer,
    NetworkPolicyPort, NetworkPolicySpec,
};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{LabelSelector, ObjectMeta};
use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;
use std::collections::{BTreeMap, BTreeSet};

/// Cluster members retain private reach; each explicit linker gains the same ports.
pub(super) fn allowed_sources(config: &Container, stack: &Stack) -> BTreeSet<String> {
    let linked = stack
        .container_linkers(&config.id)
        .into_iter()
        .map(|source| source.id().to_string());
    let cluster_members = stack.resources().filter_map(|(id, entry)| {
        let member = entry
            .config
            .downcast_ref::<Container>()
            .is_some_and(|other| other.cluster == config.cluster)
            || entry
                .config
                .downcast_ref::<Daemon>()
                .is_some_and(|other| other.cluster == config.cluster);
        member.then(|| id.clone())
    });
    linked.chain(cluster_members).collect()
}

/// The namespace a linking Sandbox's pods run in, when the chart sets one.
pub(super) fn sandbox_source_namespace(
    stack: &Stack,
    id: &str,
    configured: Option<String>,
) -> Option<String> {
    let is_sandbox = stack
        .resources()
        .find(|(resource_id, _)| resource_id.as_str() == id)
        .is_some_and(|(_, entry)| entry.config.downcast_ref::<alien_core::Sandbox>().is_some());
    configured.filter(|_| is_sandbox)
}

fn port(number: u16, protocol: &str) -> NetworkPolicyPort {
    NetworkPolicyPort {
        port: Some(IntOrString::Int(number.into())),
        protocol: Some(protocol.into()),
        ..Default::default()
    }
}

fn selector(labels: BTreeMap<String, String>) -> LabelSelector {
    LabelSelector {
        match_labels: Some(labels),
        ..Default::default()
    }
}

fn policy_labels(ctx: &ResourceControllerContext<'_>, id: &str) -> BTreeMap<String, String> {
    let mut labels = kubernetes_cleanup_resource_labels(ctx, id);
    labels.insert("component".into(), "container-network".into());
    labels
}

fn build_policy(
    config: &Container,
    name: &str,
    namespace: &str,
    ctx: &ResourceControllerContext<'_>,
) -> NetworkPolicy {
    let ingress = (!config.ports.is_empty()).then(|| {
        let sources = allowed_sources(config, ctx.desired_stack);
        let mut rules = if sources.is_empty() {
            Vec::new()
        } else {
            vec![NetworkPolicyIngressRule {
                ports: Some(config.ports.iter().map(|p| port(p.port, "TCP")).collect()),
                from: Some(
                    sources
                        .iter()
                        .map(|id| NetworkPolicyPeer {
                            // A Sandbox's pods run in the sandbox namespace when the chart sets
                            // one, so a bare pod selector (this namespace only) never matches them.
                            namespace_selector: sandbox_source_namespace(
                                ctx.desired_stack,
                                id,
                                crate::sandbox::configured_sandbox_namespace(),
                            )
                            .map(|namespace| {
                                selector(BTreeMap::from([(
                                    "kubernetes.io/metadata.name".into(),
                                    namespace,
                                )]))
                            }),
                            pod_selector: Some(selector(kubernetes_branded_resource_labels(
                                ctx, id,
                            ))),
                            ..Default::default()
                        })
                        .collect(),
                ),
            }]
        };
        // Only explicit public endpoints are reachable from outside the link set.
        rules.extend(
            config
                .public_endpoints
                .iter()
                .map(|endpoint| NetworkPolicyIngressRule {
                    ports: Some(vec![port(endpoint.port, "TCP")]),
                    from: None,
                }),
        );
        rules
    });
    let egress = config.egress.map(|_| {
        let mut rules = vec![
            NetworkPolicyEgressRule {
                to: Some(vec![NetworkPolicyPeer {
                    ip_block: Some(IPBlock {
                        cidr: "0.0.0.0/0".into(),
                        // No IPv6 rule: it remains denied, including mapped/private ranges.
                        except: Some(
                            [
                                "0.0.0.0/8",
                                "10.0.0.0/8",
                                "100.64.0.0/10",
                                "127.0.0.0/8",
                                "169.254.0.0/16",
                                "172.16.0.0/12",
                                "192.0.0.0/24",
                                "192.0.2.0/24",
                                "192.168.0.0/16",
                                "198.18.0.0/15",
                                "198.51.100.0/24",
                                "203.0.113.0/24",
                                "224.0.0.0/4",
                                "240.0.0.0/4",
                                "168.63.129.16/32",
                            ]
                            .into_iter()
                            .map(str::to_string)
                            .collect(),
                        ),
                    }),
                    ..Default::default()
                }]),
                ports: None,
            },
            NetworkPolicyEgressRule {
                // Only the cluster DNS pods, never arbitrary private UDP/TCP 53.
                to: Some(cluster_dns_peers()),
                ports: Some(vec![port(53, "UDP"), port(53, "TCP")]),
            },
        ];
        // Replicas may synchronize only with the same Container in this deployment,
        // on its declared ports. Other overlay destinations remain denied.
        if !config.ports.is_empty() {
            rules.push(NetworkPolicyEgressRule {
                to: Some(vec![NetworkPolicyPeer {
                    pod_selector: Some(selector(kubernetes_branded_resource_labels(
                        ctx, &config.id,
                    ))),
                    ..Default::default()
                }]),
                ports: Some(config.ports.iter().map(|p| port(p.port, "TCP")).collect()),
            });
        }
        rules
    });
    let mut policy_types = Vec::new();
    if ingress.is_some() {
        policy_types.push("Ingress".into());
    }
    if egress.is_some() {
        policy_types.push("Egress".into());
    }
    NetworkPolicy {
        metadata: ObjectMeta {
            name: Some(name.into()),
            namespace: Some(namespace.into()),
            labels: Some(policy_labels(ctx, &config.id)),
            ..Default::default()
        },
        spec: Some(NetworkPolicySpec {
            pod_selector: selector(kubernetes_branded_resource_labels(ctx, &config.id)),
            policy_types: Some(policy_types),
            ingress,
            egress,
        }),
    }
}

/// The pods that answer a cluster's DNS: CoreDNS or kube-dns, and on clusters running NodeLocal
/// DNSCache with a CNI that redirects the DNS Service address to the node's cache pod, that cache.
/// A CNI judges the destination after the redirect, so the cache pod must be allowed by selector.
fn cluster_dns_peers() -> Vec<NetworkPolicyPeer> {
    ["kube-dns", "node-local-dns"]
        .into_iter()
        .map(|app| NetworkPolicyPeer {
            namespace_selector: Some(selector(BTreeMap::from([(
                "kubernetes.io/metadata.name".into(),
                "kube-system".into(),
            )]))),
            pod_selector: Some(selector(BTreeMap::from([("k8s-app".into(), app.into())]))),
            ..Default::default()
        })
        .collect()
}

/// Clusters with NodeLocal DNSCache point pods at a link-local resolver, which the public
/// rule excludes and the kube-dns selector does not match. Read in the controller's pod,
/// the resolver list is the cluster's own: allow DNS to exactly those addresses. Outside
/// the cluster (no `svc.` search entry) the host's resolvers are not cluster resolvers.
fn resolver_rule(resolv_conf: Option<&str>) -> Option<NetworkPolicyEgressRule> {
    let resolv_conf = resolv_conf?;
    let in_cluster = resolv_conf
        .lines()
        .filter_map(|line| line.trim().strip_prefix("search"))
        .flat_map(str::split_whitespace)
        .any(|entry| entry.starts_with("svc."));
    let resolvers: BTreeSet<std::net::Ipv4Addr> = resolv_conf
        .lines()
        .filter_map(|line| line.trim().strip_prefix("nameserver"))
        .filter_map(|address| address.trim().parse().ok())
        .collect();
    (in_cluster && !resolvers.is_empty()).then(|| NetworkPolicyEgressRule {
        to: Some(
            resolvers
                .into_iter()
                .map(|address| NetworkPolicyPeer {
                    ip_block: Some(IPBlock {
                        cidr: format!("{address}/32"),
                        except: None,
                    }),
                    ..Default::default()
                })
                .collect(),
        ),
        ports: Some(vec![port(53, "UDP"), port(53, "TCP")]),
    })
}

/// Resolver settings that send a pod's lookups to the cluster DNS Service address.
///
/// By default the kubelet writes its own resolver address into the pod. A cluster that runs a
/// node-local DNS cache points it at a link-local address (for example AKS LocalDNS), and the
/// network policy dataplane may not match a link-local address as an address block. The
/// cluster DNS Service address is matched through the `kube-dns` pod selector in the policy.
/// The search path and `ndots` are the kubelet's defaults for a pod in `namespace`.
pub(super) fn cluster_dns_config(
    namespace: &str,
    domain: &str,
    service_address: &str,
) -> PodDNSConfig {
    PodDNSConfig {
        nameservers: Some(vec![service_address.to_string()]),
        searches: Some(vec![
            format!("{namespace}.svc.{domain}"),
            format!("svc.{domain}"),
            domain.to_string(),
        ]),
        options: Some(vec![PodDNSConfigOption {
            name: Some("ndots".into()),
            value: Some("5".into()),
        }]),
    }
}

/// The resolver the sandbox gateway's pods use.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum GatewayDns {
    /// Whatever the cluster writes into every pod.
    ClusterDefault,
    /// The `kube-dns` Service address, with the cluster domain for the search path.
    ClusterService { address: String, domain: String },
}

/// The resolver the gateway should use now. `None` when the `kube-dns` Service could not be read
/// for any reason (the read is refused, the Service is absent, the API server answers 5xx or does
/// not answer): that never decides whether the gateway deploys, and never rolls a running one.
async fn read_gateway_dns(
    domain: Option<String>,
    ctx: &ResourceControllerContext<'_>,
) -> Result<Option<GatewayDns>> {
    let Some(domain) = domain else {
        return Ok(Some(GatewayDns::ClusterDefault));
    };
    let service = ctx
        .service_provider
        .get_kubernetes_service_client(ctx.get_kubernetes_config()?)
        .await?
        .get_service("kube-system", "kube-dns")
        .await;
    let service = match service {
        Ok(service) => service,
        Err(error) => {
            tracing::warn!(
                error = %error,
                "Could not read the cluster DNS Service; the gateway keeps its resolver"
            );
            return Ok(None);
        }
    };
    let address = service
        .spec
        .and_then(|spec| spec.cluster_ip)
        .filter(|address| address.parse::<std::net::Ipv4Addr>().is_ok());
    Ok(Some(match address {
        Some(address) => GatewayDns::ClusterService { address, domain },
        None => GatewayDns::ClusterDefault,
    }))
}

/// Whether the gateway's pods must roll for `read`. A gateway with no recorded resolver was built
/// before the choice was recorded, with the cluster default.
fn gateway_dns_differs(applied: Option<&GatewayDns>, read: &GatewayDns) -> bool {
    applied.unwrap_or(&GatewayDns::ClusterDefault) != read
}

impl KubernetesContainerController {
    /// Resolves and records the gateway's resolver for the pod spec about to be built. A read that
    /// fails keeps the resolver already applied, or the cluster default.
    pub(super) async fn refresh_gateway_dns(
        &mut self,
        config: &alien_core::Container,
        domain: Option<String>,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<()> {
        if config.id != alien_core::sandbox_egress::SANDBOX_EGRESS_GATEWAY_ID {
            return Ok(());
        }
        let read = read_gateway_dns(domain, ctx).await?;
        self.applied_gateway_dns = Some(
            read.or_else(|| self.applied_gateway_dns.take())
                .unwrap_or(GatewayDns::ClusterDefault),
        );
        Ok(())
    }

    /// Whether the resolver now chosen for the gateway differs from the one its pods run with.
    /// Rolls the gateway once per change; an unreadable Service is no change.
    pub(super) async fn gateway_dns_drifted(
        &mut self,
        domain: Option<String>,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<bool> {
        let Some(read) = read_gateway_dns(domain, ctx).await? else {
            return Ok(false);
        };
        let differs = gateway_dns_differs(self.applied_gateway_dns.as_ref(), &read);
        if !differs && self.applied_gateway_dns.is_none() {
            self.applied_gateway_dns = Some(read);
        }
        Ok(differs)
    }
}

fn require_owner(
    policy: &NetworkPolicy,
    ctx: &ResourceControllerContext<'_>,
    id: &str,
) -> Result<()> {
    let expected = policy_labels(ctx, id);
    if expected.iter().all(|(key, value)| {
        policy
            .metadata
            .labels
            .as_ref()
            .and_then(|labels| labels.get(key))
            == Some(value)
    }) {
        Ok(())
    } else {
        Err(AlienError::new(ErrorData::ResourceConfigInvalid {
            resource_id: Some(id.into()), message: format!("Refusing to mutate a NetworkPolicy not owned by Container '{id}' in this deployment"),
        }))
    }
}

/// The managed gateway's egress to the services sandboxes link: each linked Container's pods
/// and its Service ClusterIP, on its declared ports only. A plugin that filters after the
/// Service address is translated sees the pod; one that filters before sees the ClusterIP.
/// The ClusterIP is added once the Service exists; the Ready refresh picks it up. Nothing is
/// added for a stack without links, and nothing for any Container but the gateway.
pub(super) async fn gateway_link_rules(
    config: &Container,
    namespace: &str,
    ctx: &ResourceControllerContext<'_>,
) -> Result<Vec<NetworkPolicyEgressRule>> {
    if !alien_core::sandbox_egress::is_managed_gateway(ctx.desired_stack, &config.id) {
        return Ok(Vec::new());
    }
    let mut rules = Vec::new();
    for (target, ports) in alien_core::sandbox_links::gateway_link_targets(ctx.desired_stack) {
        let service = alien_core::kubernetes_resource_name(&ctx.resource_prefix, &target);
        let mut peers = vec![NetworkPolicyPeer {
            pod_selector: Some(selector(kubernetes_branded_resource_labels(ctx, &target))),
            ..Default::default()
        }];
        if let Some(address) =
            crate::core::k8s_network::service_cluster_ip(ctx, namespace, &service).await?
        {
            peers.push(NetworkPolicyPeer {
                ip_block: Some(IPBlock {
                    cidr: format!("{address}/32"),
                    except: None,
                }),
                ..Default::default()
            });
        }
        rules.push(NetworkPolicyEgressRule {
            to: Some(peers),
            ports: Some(ports.iter().map(|&number| port(number, "TCP")).collect()),
        });
    }
    Ok(rules)
}

impl KubernetesContainerController {
    pub(super) async fn reconcile_network_policy(
        &mut self,
        config: &Container,
        workload: &str,
        namespace: &str,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<()> {
        // Only Containers that declare egress (on Kubernetes, the managed sandbox gateway)
        // get a NetworkPolicy. Others keep the cluster's default reachability.
        if config.egress.is_none() {
            return self.delete_network_policy(namespace, &config.id, ctx).await;
        }
        let name = workload.to_string();
        let client = ctx
            .service_provider
            .get_kubernetes_network_policy_client(ctx.get_kubernetes_config()?)
            .await?;
        let mut desired = build_policy(config, &name, namespace, ctx);
        if config.egress.is_some() {
            let excluded = crate::core::k8s_network::cluster_denied_ranges(ctx).await?;
            if let Some(block) = desired
                .spec
                .as_mut()
                .and_then(|spec| spec.egress.as_mut())
                .and_then(|rules| rules.first_mut())
                .and_then(|rule| rule.to.as_mut())
                .and_then(|peers| peers.first_mut())
                .and_then(|peer| peer.ip_block.as_mut())
            {
                block.except.get_or_insert_default().extend(excluded);
            }
            if let Some(rule) =
                resolver_rule(std::fs::read_to_string("/etc/resolv.conf").ok().as_deref())
            {
                if let Some(rules) = desired.spec.as_mut().and_then(|spec| spec.egress.as_mut()) {
                    rules.push(rule);
                }
            }
            let links = gateway_link_rules(config, namespace, ctx).await?;
            if let Some(rules) = desired.spec.as_mut().and_then(|spec| spec.egress.as_mut()) {
                rules.extend(links);
            }
        }
        let existing = match client.get_network_policy(namespace, &name).await {
            Ok(existing) => Some(existing),
            Err(error)
                if matches!(
                    error.error,
                    Some(ClientError::RemoteResourceNotFound { .. })
                ) =>
            {
                None
            }
            Err(error) => {
                return Err(error.context(ErrorData::CloudPlatformError {
                    message: "Failed to inspect Container NetworkPolicy".into(),
                    resource_id: Some(config.id.clone()),
                }))
            }
        };
        if let Some(existing) = existing {
            require_owner(&existing, ctx, &config.id)?;
            if existing.spec == desired.spec {
                self.network_policy_name = Some(name);
                return Ok(());
            }
            desired.metadata.resource_version = existing.metadata.resource_version;
            client
                .update_network_policy(namespace, &name, &desired)
                .await
        } else {
            client.create_network_policy(namespace, &desired).await
        }
        .context(ErrorData::CloudPlatformError {
            message: "Failed to enforce Container NetworkPolicy".into(),
            resource_id: Some(config.id.clone()),
        })?;
        self.network_policy_name = Some(name);
        Ok(())
    }

    pub(super) async fn delete_network_policy(
        &mut self,
        namespace: &str,
        id: &str,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<()> {
        let Some(name) = self.network_policy_name.as_deref() else {
            return Ok(());
        };
        let client = ctx
            .service_provider
            .get_kubernetes_network_policy_client(ctx.get_kubernetes_config()?)
            .await?;
        match client.get_network_policy(namespace, name).await {
            Ok(existing) => {
                require_owner(&existing, ctx, id)?;
                match client.delete_network_policy(namespace, name).await {
                    Ok(()) => {}
                    Err(error)
                        if matches!(
                            error.error,
                            Some(ClientError::RemoteResourceNotFound { .. })
                        ) => {}
                    Err(error) => {
                        return Err(error.context(ErrorData::CloudPlatformError {
                            message: "Failed to delete Container NetworkPolicy".into(),
                            resource_id: Some(id.into()),
                        }))
                    }
                }
            }
            Err(error)
                if matches!(
                    error.error,
                    Some(ClientError::RemoteResourceNotFound { .. })
                ) => {}
            Err(error) => {
                return Err(error.context(ErrorData::CloudPlatformError {
                    message: "Failed to inspect Container NetworkPolicy before deletion".into(),
                    resource_id: Some(id.into()),
                }))
            }
        }
        self.network_policy_name = None;
        Ok(())
    }
}

#[cfg(test)]
#[path = "kubernetes_network_tests.rs"]
mod tests;
