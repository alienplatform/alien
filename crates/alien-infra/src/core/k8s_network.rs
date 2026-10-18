//! Cluster destinations excluded from public Container and sandbox gateway egress.

use std::collections::BTreeSet;

use alien_core::ComputeCluster;
use alien_error::Context;

use super::ResourceControllerContext;
use crate::{ErrorData, Result};

pub(crate) async fn cluster_denied_ranges(
    ctx: &ResourceControllerContext<'_>,
) -> Result<BTreeSet<String>> {
    // A cluster may use publicly routed node or pod addresses. These are
    // still cluster destinations, not internet egress.
    let nodes = ctx
        .service_provider
        .get_kubernetes_node_client(ctx.get_kubernetes_config()?)
        .await?
        .list_nodes(None, None)
        .await
        .context(ErrorData::CloudPlatformError {
            message: "Failed to discover cluster addresses before enforcing Container egress"
                .into(),
            resource_id: Some(ctx.desired_config.id().into()),
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
    // Compute overlays are internal even when their configured addresses
    // are public or no Kubernetes node has been allocated in them yet.
    for (_, entry) in ctx.desired_stack.resources() {
        if let Some(cluster) = entry.config.downcast_ref::<ComputeCluster>() {
            let cidr = cluster.container_cidr();
            if !cidr.contains(':') {
                excluded.insert(cidr.to_string());
            }
        }
    }
    Ok(excluded)
}
