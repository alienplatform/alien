//! Runtime-owned Container endpoint and outbound policies.
use super::kubernetes::KubernetesContainerController;
use crate::core::{
    kubernetes_branded_resource_labels, kubernetes_cleanup_resource_labels,
    ResourceControllerContext,
};
use crate::error::{ErrorData, Result};
use alien_client_core::ErrorData as ClientError;
use alien_core::{links_of, Container, Daemon, Stack};
use alien_error::{AlienError, Context, ContextError};
use k8s_openapi::api::networking::v1::{
    IPBlock, NetworkPolicy, NetworkPolicyEgressRule, NetworkPolicyIngressRule, NetworkPolicyPeer,
    NetworkPolicyPort, NetworkPolicySpec,
};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{LabelSelector, ObjectMeta};
use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;
use std::collections::{BTreeMap, BTreeSet};

/// Cluster members retain private reach; each explicit linker gains the same ports.
pub(super) fn allowed_sources(config: &Container, stack: &Stack) -> BTreeSet<String> {
    stack
        .resources()
        .filter_map(|(id, entry)| {
            let cluster_member = entry
                .config
                .downcast_ref::<Container>()
                .is_some_and(|other| other.cluster == config.cluster)
                || entry
                    .config
                    .downcast_ref::<Daemon>()
                    .is_some_and(|other| other.cluster == config.cluster);
            let linked = links_of(&entry.config).iter().any(|link| {
                link.resource_type == Container::RESOURCE_TYPE && link.id() == config.id
            });
            (cluster_member || linked).then(|| id.clone())
        })
        .collect()
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
        vec![
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
                to: Some(vec![NetworkPolicyPeer {
                    namespace_selector: Some(selector(BTreeMap::from([(
                        "kubernetes.io/metadata.name".into(),
                        "kube-system".into(),
                    )]))),
                    pod_selector: Some(selector(BTreeMap::from([(
                        "k8s-app".into(),
                        "kube-dns".into(),
                    )]))),
                    ..Default::default()
                }]),
                ports: Some(vec![port(53, "UDP"), port(53, "TCP")]),
            },
        ]
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

impl KubernetesContainerController {
    pub(super) async fn reconcile_network_policy(
        &mut self,
        config: &Container,
        workload: &str,
        namespace: &str,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<()> {
        if config.ports.is_empty() && config.egress.is_none() {
            return self.delete_network_policy(namespace, &config.id, ctx).await;
        }
        let name = workload.to_string();
        let client = ctx
            .service_provider
            .get_kubernetes_network_policy_client(ctx.get_kubernetes_config()?)
            .await?;
        let mut desired = build_policy(config, &name, namespace, ctx);
        if config.egress.is_some() {
            // A cluster may use publicly routed node or pod addresses. These are
            // still cluster destinations, not internet egress.
            let nodes = ctx
                .service_provider
                .get_kubernetes_node_client(ctx.get_kubernetes_config()?)
                .await?
                .list_nodes(None, None)
                .await
                .context(ErrorData::CloudPlatformError {
                    message:
                        "Failed to discover cluster addresses before enforcing Container egress"
                            .into(),
                    resource_id: Some(config.id.clone()),
                })?;
            let mut excluded = BTreeSet::new();
            for node in nodes.items {
                if let Some(spec) = node.spec {
                    for cidr in spec
                        .pod_cidrs
                        .unwrap_or_default()
                        .into_iter()
                        .chain(spec.pod_cidr)
                    {
                        if !cidr.contains(':') {
                            excluded.insert(cidr);
                        }
                    }
                }
                if let Some(status) = node.status {
                    for address in status.addresses.unwrap_or_default() {
                        if let Ok(address) = address.address.parse::<std::net::Ipv4Addr>() {
                            excluded.insert(format!("{address}/32"));
                        }
                    }
                }
            }
            if let Some(alien_core::NetworkSettings::Create {
                cidr: Some(cidr), ..
            }) = &ctx.deployment_config.stack_settings.network
            {
                if !cidr.contains(':') {
                    excluded.insert(cidr.clone());
                }
            }
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
