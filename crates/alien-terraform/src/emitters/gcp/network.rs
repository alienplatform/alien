//! GCP Network — VPC + subnetwork + Cloud Router + Cloud NAT.
//!
//! Three modes mirror the AWS network emitter:
//!
//! * `UseDefault` — look up the project's `default` VPC and regional subnet.
//! * `ByoVpcGcp` — emit a `google_compute_network` data source so the
//!   controller can resolve the BYO VPC's self-link without a cloud
//!   API call.
//! * `Create` — full custom-mode VPC, regional subnetwork, Cloud Router
//!   + Cloud NAT for egress, ingress firewall rules.

use crate::{
    block::{attr, block, data_block, nested, resource_block},
    emitter::{TfEmitter, TfFragment},
    emitters::gcp::helpers::{downcast, required_label},
    expr,
};
use alien_core::{import::EmitContext, ErrorData, Network, NetworkSettings, Result};
use alien_error::AlienError;
use hcl::expr::Expression;

/// The Terraform expression that says whether a created-network module creates its network, uses
/// one it looks up, or uses the default one.
///
/// An existing Kubernetes cluster is already attached to a network, so a module that keeps a
/// network for it (the stack has a cloud database) looks that network up from the cluster and
/// creates none, whatever `network_mode` says. Every other module follows `network_mode`.
pub fn network_mode_expr(targets_kubernetes: bool) -> &'static str {
    if targets_kubernetes {
        "local.effective_network_mode"
    } else {
        "var.network_mode"
    }
}

/// Locals of a Kubernetes module that give the network the existing cluster is attached to, and
/// the mode that follows the cluster mode.
fn existing_cluster_network_locals(mut fragment: TfFragment) -> TfFragment {
    let cluster = "data.google_container_cluster.kubernetes_existing[0]";
    let location = "(var.gke_cluster_location == \"\" ? var.gcp_region : var.gke_cluster_location)";
    for (name, value) in [
        (
            "effective_network_mode",
            "var.kubernetes_cluster_mode == \"existing\" ? \"use-existing\" : var.network_mode"
                .to_string(),
        ),
        (
            EXISTING_CLUSTER_NETWORK_NAME,
            format!(
                "var.kubernetes_cluster_mode == \"existing\" ? basename({cluster}.network) : var.network_name"
            ),
        ),
        (
            EXISTING_CLUSTER_SUBNET_NAME,
            format!(
                "var.kubernetes_cluster_mode == \"existing\" ? basename({cluster}.subnetwork) : var.subnet_name"
            ),
        ),
        (
            EXISTING_CLUSTER_NETWORK_REGION,
            format!(
                "var.kubernetes_cluster_mode == \"existing\" ? join(\"-\", slice(split(\"-\", {location}), 0, 2)) : (var.network_region == \"\" ? var.gcp_region : var.network_region)"
            ),
        ),
    ] {
        fragment.locals.insert(name.to_string(), expr::raw(value));
    }
    fragment
}

/// Locals holding the name and region of the network and subnet a lookup of the existing
/// network reads: the cluster's own in an existing cluster, the deployer's inputs otherwise.
pub const EXISTING_CLUSTER_NETWORK_NAME: &str = "existing_network_name";
pub const EXISTING_CLUSTER_SUBNET_NAME: &str = "existing_subnet_name";
pub const EXISTING_CLUSTER_NETWORK_REGION: &str = "existing_network_region";

#[derive(Debug, Clone, Copy, Default)]
pub struct GcpNetworkEmitter;

impl TfEmitter for GcpNetworkEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let network = downcast::<Network>(ctx, Network::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;

        match &network.settings {
            NetworkSettings::UseDefault => {
                let mut fragment = TfFragment::default();
                fragment.data_blocks.push(data_block(
                    "google_compute_network",
                    label,
                    [
                        attr("name", Expression::String("default".to_string())),
                        attr("project", expr::raw("var.gcp_project")),
                    ],
                ));
                fragment.data_blocks.push(data_block(
                    "google_compute_subnetwork",
                    &format!("{label}_subnet"),
                    [
                        attr("name", Expression::String("default".to_string())),
                        attr("project", expr::raw("var.gcp_project")),
                        attr("region", expr::raw("var.gcp_region")),
                    ],
                ));
                Ok(fragment)
            }
            NetworkSettings::ByoVpcGcp {
                network_name,
                subnet_name,
                region,
            } => {
                let mut fragment = TfFragment::default();
                fragment.data_blocks.push(data_block(
                    "google_compute_network",
                    label,
                    [
                        attr("name", Expression::String(network_name.clone())),
                        attr("project", expr::raw("var.gcp_project")),
                    ],
                ));
                let subnet_label = format!("{label}_subnet");
                fragment.data_blocks.push(data_block(
                    "google_compute_subnetwork",
                    &subnet_label,
                    [
                        attr("name", Expression::String(subnet_name.clone())),
                        attr("project", expr::raw("var.gcp_project")),
                        attr("region", Expression::String(region.clone())),
                    ],
                ));
                Ok(fragment)
            }
            NetworkSettings::Create { cidr, .. } => {
                let fragment = dynamic_topology(label, cidr.clone(), ctx.targets_kubernetes);
                Ok(if ctx.targets_kubernetes {
                    existing_cluster_network_locals(fragment)
                } else {
                    fragment
                })
            }
            NetworkSettings::ByoVpcAws { .. } | NetworkSettings::ByoVnetAzure { .. } => {
                Err(AlienError::new(ErrorData::OperationNotSupported {
                    operation: "generate_terraform_module".to_string(),
                    reason: "GCP Terraform network emitter received non-GCP network settings"
                        .to_string(),
                }))
            }
        }
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        let network = downcast::<Network>(ctx, Network::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;
        let mode = network_mode_expr(ctx.targets_kubernetes);
        Ok(match &network.settings {
            NetworkSettings::UseDefault | NetworkSettings::ByoVpcGcp { .. } => {
                let subnet_label = format!("{label}_subnet");
                expr::object([
                    ("projectId", expr::raw("var.gcp_project")),
                    (
                        "vpcSelfLink",
                        expr::traversal(["data", "google_compute_network", label, "self_link"]),
                    ),
                    (
                        "vpcName",
                        expr::traversal(["data", "google_compute_network", label, "name"]),
                    ),
                    (
                        "subnetSelfLinks",
                        subnet_self_links(label, &network.settings, ctx.targets_kubernetes),
                    ),
                    (
                        "cidrBlock",
                        expr::traversal([
                            "data",
                            "google_compute_subnetwork",
                            &subnet_label,
                            "ip_cidr_range",
                        ]),
                    ),
                    ("routerSelfLink", Expression::Null),
                    ("natName", Expression::Null),
                    ("isByoVpc", Expression::Bool(true)),
                ])
            }
            NetworkSettings::Create { .. } => {
                let subnet_label = format!("{label}_workload");
                let router_label = format!("{label}_router");
                let nat_label = format!("{label}_nat");
                let existing_subnet_label = format!("{label}_existing_subnet");
                expr::object([
                    ("projectId", expr::raw("var.gcp_project")),
                    (
                        "vpcSelfLink",
                        expr::raw(format!(
                            "{mode} == \"create-new\" ? google_compute_network.{label}[0].self_link : data.google_compute_network.{label}[0].self_link"
                        )),
                    ),
                    (
                        "vpcName",
                        expr::raw(format!(
                            "{mode} == \"create-new\" ? google_compute_network.{label}[0].name : data.google_compute_network.{label}[0].name"
                        )),
                    ),
                    ("subnetSelfLinks", subnet_self_links(label, &network.settings, ctx.targets_kubernetes)),
                    (
                        "cidrBlock",
                        expr::raw(format!(
                            "{mode} == \"create-new\" ? google_compute_subnetwork.{subnet_label}[0].ip_cidr_range : data.google_compute_subnetwork.{existing_subnet_label}[0].ip_cidr_range"
                        )),
                    ),
                    (
                        "routerSelfLink",
                        expr::raw(format!(
                            "{mode} == \"create-new\" ? google_compute_router.{router_label}[0].self_link : null"
                        )),
                    ),
                    (
                        "natName",
                        expr::raw(format!(
                            "{mode} == \"create-new\" ? google_compute_router_nat.{nat_label}[0].name : null"
                        )),
                    ),
                    ("isByoVpc", expr::raw(format!("{mode} != \"create-new\""))),
                ])
            }
            _ => Expression::Null,
        })
    }
}

/// The self links of the subnets the network `label` with `settings` gives workloads: what
/// setup reports to the worker controller, which puts unprotected functions there, and what
/// it gives a protected install's verifier for the same.
pub fn subnet_self_links(
    label: &str,
    settings: &NetworkSettings,
    targets_kubernetes: bool,
) -> Expression {
    let mode = network_mode_expr(targets_kubernetes);
    match settings {
        NetworkSettings::Create { .. } => expr::raw(format!(
            "{mode} == \"create-new\" ? [google_compute_subnetwork.{label}_workload[0].self_link] : [data.google_compute_subnetwork.{label}_existing_subnet[0].self_link]"
        )),
        _ => Expression::Array(vec![expr::traversal([
            "data",
            "google_compute_subnetwork",
            &format!("{label}_subnet"),
            "self_link",
        ])]),
    }
}

/// The Direct VPC egress Cloud Run functions in the network `label` with `settings` use, as
/// the worker controller gives unprotected ones: all traffic, or private ranges only on the
/// project's default network.
pub fn function_egress(settings: &NetworkSettings, targets_kubernetes: bool) -> Expression {
    match settings {
        NetworkSettings::Create { .. } => expr::raw(format!(
            "{} == \"use-default\" ? \"PRIVATE_RANGES_ONLY\" : \"ALL_TRAFFIC\"",
            network_mode_expr(targets_kubernetes)
        )),
        NetworkSettings::UseDefault => Expression::String("PRIVATE_RANGES_ONLY".to_string()),
        _ => Expression::String("ALL_TRAFFIC".to_string()),
    }
}

fn dynamic_topology(label: &str, cidr: Option<String>, targets_kubernetes: bool) -> TfFragment {
    let mode = network_mode_expr(targets_kubernetes);
    // The names and region of the network a lookup reads: the cluster's in an existing cluster.
    let (name, subnet, region_expr) = if targets_kubernetes {
        (
            format!("local.{EXISTING_CLUSTER_NETWORK_NAME}"),
            format!("local.{EXISTING_CLUSTER_SUBNET_NAME}"),
            format!(
                "{mode} == \"use-default\" ? var.gcp_region : local.{EXISTING_CLUSTER_NETWORK_REGION}"
            ),
        )
    } else {
        (
            "var.network_name".to_string(),
            "var.subnet_name".to_string(),
            "var.network_mode == \"use-default\" || var.network_region == \"\" ? var.gcp_region : var.network_region".to_string(),
        )
    };
    let mut fragment = create_topology(label, cidr, true);
    for resource in &mut fragment.resource_blocks {
        let existing = std::mem::take(&mut resource.body);
        resource.body = hcl::structure::Body::from(
            std::iter::once(attr(
                "count",
                expr::raw(format!("{mode} == \"create-new\" ? 1 : 0")),
            ))
            .chain(existing.into_iter())
            .collect::<Vec<_>>(),
        );
    }

    let existing_subnet_label = format!("{label}_existing_subnet");
    fragment.data_blocks.push(data_block(
        "google_compute_network",
        label,
        [
            attr(
                "count",
                expr::raw(format!("{mode} != \"create-new\" ? 1 : 0")),
            ),
            attr(
                "name",
                expr::raw(format!("{mode} == \"use-default\" ? \"default\" : {name}")),
            ),
            attr("project", expr::raw("var.gcp_project")),
        ],
    ));
    fragment.data_blocks.push(data_block(
        "google_compute_subnetwork",
        &existing_subnet_label,
        [
            attr(
                "count",
                expr::raw(format!("{mode} != \"create-new\" ? 1 : 0")),
            ),
            attr("name", expr::raw(format!("{mode} == \"use-default\" ? \"default\" : {subnet}"))),
            attr("project", expr::raw("var.gcp_project")),
            attr(
                "region",
                expr::raw(region_expr),
            ),
        ],
    ));

    fragment
}

fn create_topology(label: &str, cidr: Option<String>, counted: bool) -> TfFragment {
    let cidr_str = cidr.unwrap_or_else(|| "10.44.0.0/16".to_string());
    let subnet_label = format!("{label}_workload");
    let router_label = format!("{label}_router");
    let nat_label = format!("{label}_nat");
    let network_name = gcp_network_name(label, "");
    let subnet_name = gcp_network_name(label, "-workload");
    let router_name_attr = gcp_network_name(label, "-router");
    let nat_name = gcp_network_name(label, "-nat");
    let firewall_name = gcp_network_name(label, "-internal");
    let network_id = if counted {
        expr::raw(format!("google_compute_network.{label}[0].id"))
    } else {
        expr::traversal(["google_compute_network", label, "id"])
    };
    let router_name = if counted {
        expr::raw(format!("google_compute_router.{router_label}[0].name"))
    } else {
        expr::traversal(["google_compute_router", &router_label, "name"])
    };

    let mut fragment = TfFragment::default();

    fragment.resource_blocks.push(resource_block(
        "google_compute_network",
        label,
        [
            attr("name", network_name),
            attr("project", expr::raw("var.gcp_project")),
            attr("auto_create_subnetworks", Expression::Bool(false)),
            attr("routing_mode", Expression::String("REGIONAL".to_string())),
        ],
    ));

    fragment.resource_blocks.push(resource_block(
        "google_compute_subnetwork",
        &subnet_label,
        [
            attr("name", subnet_name),
            attr("project", expr::raw("var.gcp_project")),
            attr("region", expr::raw("var.gcp_region")),
            attr("ip_cidr_range", Expression::String(cidr_str.clone())),
            attr("network", network_id.clone()),
            attr("private_ip_google_access", Expression::Bool(true)),
        ],
    ));

    fragment.resource_blocks.push(resource_block(
        "google_compute_router",
        &router_label,
        [
            attr("name", router_name_attr),
            attr("project", expr::raw("var.gcp_project")),
            attr("region", expr::raw("var.gcp_region")),
            attr("network", network_id.clone()),
        ],
    ));

    fragment.resource_blocks.push(resource_block(
        "google_compute_router_nat",
        &nat_label,
        [
            attr("name", nat_name),
            attr("project", expr::raw("var.gcp_project")),
            attr("region", expr::raw("var.gcp_region")),
            attr("router", router_name),
            attr(
                "nat_ip_allocate_option",
                Expression::String("AUTO_ONLY".to_string()),
            ),
            attr(
                "source_subnetwork_ip_ranges_to_nat",
                Expression::String("ALL_SUBNETWORKS_ALL_IP_RANGES".to_string()),
            ),
            nested(block(
                "log_config",
                [
                    attr("enable", Expression::Bool(true)),
                    attr("filter", Expression::String("ERRORS_ONLY".to_string())),
                ],
            )),
        ],
    ));

    fragment.resource_blocks.push(resource_block(
        "google_compute_firewall",
        &format!("{label}_internal"),
        [
            attr("name", firewall_name),
            attr("project", expr::raw("var.gcp_project")),
            attr("network", network_id),
            attr("direction", Expression::String("INGRESS".to_string())),
            attr(
                "source_ranges",
                Expression::Array(vec![Expression::String(cidr_str)]),
            ),
            nested(block(
                "allow",
                [attr("protocol", Expression::String("all".to_string()))],
            )),
        ],
    ));

    fragment
}

fn gcp_network_name(label: &str, suffix: &str) -> Expression {
    let segment = label.replace('_', "-");
    expr::raw(format!(
        "trim(substr(\"${{local.resource_prefix}}-{segment}{suffix}\", 0, 63), \"-\")"
    ))
}
