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
            NetworkSettings::Create { cidr, .. } => Ok(create_topology(label, cidr.clone())),
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
                    ("routerSelfLink", Expression::Null),
                    ("natName", Expression::Null),
                    ("isByoVpc", Expression::Bool(true)),
                ])
            }
            NetworkSettings::Create { .. } => {
                let subnet_label = format!("{label}_workload");
                let router_label = format!("{label}_router");
                let nat_label = format!("{label}_nat");
                expr::object([
                    ("projectId", expr::raw("var.gcp_project")),
                    (
                        "vpcSelfLink",
                        expr::traversal(["google_compute_network", label, "self_link"]),
                    ),
                    (
                        "vpcName",
                        expr::traversal(["google_compute_network", label, "name"]),
                    ),
                    (
                        "subnetSelfLinks",
                        Expression::Array(vec![expr::traversal([
                            "google_compute_subnetwork",
                            &subnet_label,
                            "self_link",
                        ])]),
                    ),
                    (
                        "routerSelfLink",
                        expr::traversal(["google_compute_router", &router_label, "self_link"]),
                    ),
                    (
                        "natName",
                        expr::traversal(["google_compute_router_nat", &nat_label, "name"]),
                    ),
                    ("isByoVpc", Expression::Bool(false)),
                ])
            }
            _ => Expression::Null,
        })
    }
}

fn create_topology(label: &str, cidr: Option<String>) -> TfFragment {
    let cidr_str = cidr.unwrap_or_else(|| "10.44.0.0/16".to_string());
    let subnet_label = format!("{label}_workload");
    let router_label = format!("{label}_router");
    let nat_label = format!("{label}_nat");

    let mut fragment = TfFragment::default();

    fragment.resource_blocks.push(resource_block(
        "google_compute_network",
        label,
        [
            attr(
                "name",
                crate::emitters::gcp::helpers::stack_name_template(label),
            ),
            attr("project", expr::raw("var.gcp_project")),
            attr("auto_create_subnetworks", Expression::Bool(false)),
            attr("routing_mode", Expression::String("REGIONAL".to_string())),
        ],
    ));

    fragment.resource_blocks.push(resource_block(
        "google_compute_subnetwork",
        &subnet_label,
        [
            attr(
                "name",
                expr::template(format!("${{var.stack_name}}-{label}-workload")),
            ),
            attr("project", expr::raw("var.gcp_project")),
            attr("region", expr::raw("var.gcp_region")),
            attr("ip_cidr_range", Expression::String(cidr_str.clone())),
            attr(
                "network",
                expr::traversal(["google_compute_network", label, "id"]),
            ),
            attr("private_ip_google_access", Expression::Bool(true)),
        ],
    ));

    fragment.resource_blocks.push(resource_block(
        "google_compute_router",
        &router_label,
        [
            attr(
                "name",
                expr::template(format!("${{var.stack_name}}-{label}-router")),
            ),
            attr("project", expr::raw("var.gcp_project")),
            attr("region", expr::raw("var.gcp_region")),
            attr(
                "network",
                expr::traversal(["google_compute_network", label, "id"]),
            ),
        ],
    ));

    fragment.resource_blocks.push(resource_block(
        "google_compute_router_nat",
        &nat_label,
        [
            attr(
                "name",
                expr::template(format!("${{var.stack_name}}-{label}-nat")),
            ),
            attr("project", expr::raw("var.gcp_project")),
            attr("region", expr::raw("var.gcp_region")),
            attr(
                "router",
                expr::traversal(["google_compute_router", &router_label, "name"]),
            ),
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
            attr(
                "name",
                expr::template(format!("${{var.stack_name}}-{label}-internal")),
            ),
            attr("project", expr::raw("var.gcp_project")),
            attr(
                "network",
                expr::traversal(["google_compute_network", label, "id"]),
            ),
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
