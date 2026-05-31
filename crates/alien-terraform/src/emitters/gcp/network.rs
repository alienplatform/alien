//! GCP Network — VPC + subnetwork + Cloud Router + Cloud NAT.
//!
//! Three modes mirror the AWS network emitter:
//!
//! * `UseDefault` — emit nothing; controller falls back to the project's
//!   `default` network at runtime.
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

#[derive(Debug, Clone, Copy, Default)]
pub struct GcpNetworkEmitter;

impl TfEmitter for GcpNetworkEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let network = downcast::<Network>(ctx, Network::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;

        match &network.settings {
            NetworkSettings::UseDefault => Ok(TfFragment::empty()),
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
            NetworkSettings::Create { cidr, .. } => Ok(dynamic_topology(label, cidr.clone())),
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
        Ok(match &network.settings {
            NetworkSettings::UseDefault => expr::object([
                ("projectId", expr::raw("var.gcp_project")),
                ("vpcSelfLink", Expression::Null),
                ("vpcName", Expression::Null),
                ("subnetSelfLinks", Expression::Array(vec![])),
                ("cidrBlock", Expression::Null),
                ("routerSelfLink", Expression::Null),
                ("natName", Expression::Null),
                ("isByoVpc", Expression::Bool(true)),
            ]),
            NetworkSettings::ByoVpcGcp { .. } => {
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
                        Expression::Array(vec![expr::traversal([
                            "data",
                            "google_compute_subnetwork",
                            &subnet_label,
                            "self_link",
                        ])]),
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
                            "var.network_mode == \"create-new\" ? google_compute_network.{label}[0].self_link : var.network_mode == \"use-existing\" ? data.google_compute_network.{label}[0].self_link : null"
                        )),
                    ),
                    (
                        "vpcName",
                        expr::raw(format!(
                            "var.network_mode == \"create-new\" ? google_compute_network.{label}[0].name : var.network_mode == \"use-existing\" ? data.google_compute_network.{label}[0].name : null"
                        )),
                    ),
                    (
                        "subnetSelfLinks",
                        expr::raw(format!(
                            "var.network_mode == \"create-new\" ? [google_compute_subnetwork.{subnet_label}[0].self_link] : var.network_mode == \"use-existing\" ? [data.google_compute_subnetwork.{existing_subnet_label}[0].self_link] : []"
                        )),
                    ),
                    (
                        "cidrBlock",
                        expr::raw(format!(
                            "var.network_mode == \"create-new\" ? google_compute_subnetwork.{subnet_label}[0].ip_cidr_range : var.network_mode == \"use-existing\" ? data.google_compute_subnetwork.{existing_subnet_label}[0].ip_cidr_range : null"
                        )),
                    ),
                    (
                        "routerSelfLink",
                        expr::raw(format!(
                            "var.network_mode == \"create-new\" ? google_compute_router.{router_label}[0].self_link : null"
                        )),
                    ),
                    (
                        "natName",
                        expr::raw(format!(
                            "var.network_mode == \"create-new\" ? google_compute_router_nat.{nat_label}[0].name : null"
                        )),
                    ),
                    ("isByoVpc", expr::raw("var.network_mode != \"create-new\"")),
                ])
            }
            _ => Expression::Null,
        })
    }
}

fn dynamic_topology(label: &str, cidr: Option<String>) -> TfFragment {
    let mut fragment = create_topology(label, cidr, true);
    for resource in &mut fragment.resource_blocks {
        let existing = std::mem::take(&mut resource.body);
        resource.body = hcl::structure::Body::from(
            std::iter::once(attr(
                "count",
                expr::raw("var.network_mode == \"create-new\" ? 1 : 0"),
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
                expr::raw("var.network_mode == \"use-existing\" ? 1 : 0"),
            ),
            attr("name", expr::raw("var.network_name")),
            attr("project", expr::raw("var.gcp_project")),
        ],
    ));
    fragment.data_blocks.push(data_block(
        "google_compute_subnetwork",
        &existing_subnet_label,
        [
            attr(
                "count",
                expr::raw("var.network_mode == \"use-existing\" ? 1 : 0"),
            ),
            attr("name", expr::raw("var.subnet_name")),
            attr("project", expr::raw("var.gcp_project")),
            attr(
                "region",
                expr::raw("var.network_region == \"\" ? var.gcp_region : var.network_region"),
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
