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
    excluded.extend(service_ranges(ctx).await?);
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

/// Service addresses are cluster destinations too, and a cluster may number them publicly.
/// The ServiceCIDR API lists the ranges. An API server without it must show that Services
/// use private addresses, which the gateway and the public-address rule already exclude;
/// otherwise configuration is refused rather than leaving Services reachable.
async fn service_ranges(ctx: &ResourceControllerContext<'_>) -> Result<BTreeSet<String>> {
    let failed = |message: &str| ErrorData::CloudPlatformError {
        message: message.into(),
        resource_id: Some(ctx.desired_config.id().into()),
    };
    let config = ctx.get_kubernetes_config()?;
    match ctx
        .service_provider
        .get_kubernetes_node_client(config)
        .await?
        .list_service_cidrs()
        .await
    {
        Ok(list) => Ok(list
            .items
            .into_iter()
            .filter_map(|range| range.spec)
            .flat_map(|spec| spec.cidrs.unwrap_or_default())
            .filter(|cidr| !cidr.contains(':'))
            .collect()),
        Err(error)
            if matches!(
                error.error,
                Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. })
            ) =>
        {
            let service = ctx
                .service_provider
                .get_kubernetes_service_client(config)
                .await?
                .get_service("default", "kubernetes")
                .await
                .context(failed(
                    "Failed to read the API server Service to classify Service addresses",
                ))?;
            let address = service
                .spec
                .and_then(|spec| spec.cluster_ip)
                .and_then(|address| address.parse::<std::net::Ipv4Addr>().ok());
            match address {
                Some(address) if is_private(address) => Ok(BTreeSet::new()),
                _ => Err(alien_error::AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: "This cluster numbers Services outside private ranges and does not serve the ServiceCIDR API, so Service addresses cannot be excluded from egress".into(),
                    resource_id: Some(ctx.desired_config.id().into()),
                })),
            }
        }
        Err(error) => Err(error).context(failed("Failed to list Service address ranges")),
    }
}

fn is_private(address: std::net::Ipv4Addr) -> bool {
    let [a, b, ..] = address.octets();
    a == 10
        || (a == 172 && (16..32).contains(&b))
        || (a == 192 && b == 168)
        || (a == 100 && (64..128).contains(&b))
}
