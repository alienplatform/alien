//! Setup-owned private network layer for GCP Agent Platform sandboxes.
//!
//! A sandbox whose egress is `none` or `internet`, or that declares preview ports, reaches the deployment VPC through
//! Private Service Connect interface (PSC-I). Google pins a project's sandbox network placement when
//! its first sandbox is created, so the subnet, the network attachment and the Agent Platform
//! service agent's network grant must exist before any template does. They change the deploying account's
//! VPC and IAM, so setup creates them and the template controller only reads the attachment.
//! Stacks with no such sandbox (`allow` egress, no preview) get nothing from this module.

use alien_core::{import::EmitContext, ErrorData, Result, Sandbox, SandboxEgress};
use alien_error::AlienError;
use hcl::expr::Expression;

use crate::{
    block::{attr, block, data_block, nested, resource_block},
    emitter::TfFragment,
    expr,
};

/// Terraform variable that overrides the dedicated subnet range.
pub(super) const CIDR_VARIABLE: &str = "sandbox_psc_subnet_cidr";

/// Whether this stack has a GCP sandbox that needs PSC-I at setup.
pub fn stack_needs_psc(stack: &alien_core::Stack) -> bool {
    stack.resources().any(|(_, entry)| {
        entry
            .config
            .downcast_ref::<Sandbox>()
            .is_some_and(Sandbox::needs_gcp_private_network)
    })
}

/// A `/28` in the half of the declared range's parent block that the declared range does not
/// occupy. The workload subnet takes the whole declared range, so a sibling block is the nearest
/// range that cannot overlap it. `None` when the range is not a plain IPv4 CIDR.
pub(super) fn default_subnet_cidr(declared: &str) -> Option<String> {
    let (address, prefix) = declared.split_once('/')?;
    let prefix: u32 = prefix.parse().ok()?;
    if !(1..=28).contains(&prefix) {
        return None;
    }
    let address: u32 = address.parse::<std::net::Ipv4Addr>().ok()?.into();
    let size = 1u32 << (32 - prefix);
    let network = address & !(size - 1);
    Some(format!("{}/28", std::net::Ipv4Addr::from(network ^ size)))
}

/// Terraform label of the network attachment emitted for network `label`.
pub(super) fn attachment_label(label: &str) -> String {
    format!("{label}_sandbox_psc")
}

/// Self-link of the attachment, for the network import ref.
pub(super) fn attachment_self_link(label: &str) -> Expression {
    expr::raw(format!(
        "google_compute_network_attachment.{}.self_link",
        attachment_label(label)
    ))
}

pub(super) fn emit(
    ctx: &EmitContext<'_>,
    label: &str,
    vpc_self_link: Expression,
    declared_cidr: Option<&str>,
    fragment: &mut TfFragment,
) -> Result<()> {
    if ctx.targets_kubernetes || !stack_needs_psc(ctx.stack) {
        return Ok(());
    }
    let psc = attachment_label(label);
    let segment = label.replace('_', "-");
    let name = |suffix: &str| {
        expr::raw(format!(
            "trim(substr(\"${{local.resource_prefix}}-{segment}{suffix}\", 0, 63), \"-\")"
        ))
    };
    let cidr = match declared_cidr.and_then(default_subnet_cidr) {
        Some(default) => expr::raw(format!(
            "var.{CIDR_VARIABLE} != \"\" ? var.{CIDR_VARIABLE} : \"{default}\""
        )),
        None => expr::raw(format!("var.{CIDR_VARIABLE}")),
    };
    if declared_cidr.and_then(default_subnet_cidr).is_none() && declared_cidr.is_some() {
        return Err(AlienError::new(ErrorData::OperationNotSupported {
            operation: "GCP sandbox network setup".to_string(),
            reason: format!(
                "the declared network range '{}' is not an IPv4 CIDR between /1 and /28, so no dedicated sandbox subnet range can be chosen",
                declared_cidr.unwrap_or_default()
            ),
        }));
    }

    fragment.resource_blocks.push(resource_block(
        "google_compute_subnetwork",
        &psc,
        [
            attr("name", name("-sandbox-psc")),
            attr("project", expr::raw("var.gcp_project")),
            attr("region", expr::raw("var.gcp_region")),
            attr("ip_cidr_range", cidr),
            attr("network", vpc_self_link),
            attr("private_ip_google_access", Expression::Bool(true)),
            nested(block(
                "lifecycle",
                [nested(block(
                    "precondition",
                    [
                        attr(
                            "condition",
                            expr::raw(if declared_cidr.is_some() {
                                format!("var.{CIDR_VARIABLE} != \"\" || var.network_mode == \"create-new\"")
                            } else {
                                format!("var.{CIDR_VARIABLE} != \"\"")
                            }),
                        ),
                        attr(
                            "error_message",
                            Expression::String(format!(
                                "{CIDR_VARIABLE} is required with an existing VPC: setup cannot see which ranges are free, so name a /28 or larger range that overlaps no subnet."
                            )),
                        ),
                    ],
                ))],
            )),
        ],
    ));
    fragment.resource_blocks.push(resource_block(
        "google_compute_network_attachment",
        &psc,
        [
            attr("name", name("-sandbox-psc")),
            attr("project", expr::raw("var.gcp_project")),
            attr("region", expr::raw("var.gcp_region")),
            attr(
                "connection_preference",
                Expression::String("ACCEPT_AUTOMATIC".to_string()),
            ),
            attr(
                "subnetworks",
                Expression::Array(vec![expr::raw(format!(
                    "google_compute_subnetwork.{psc}.self_link"
                ))]),
            ),
        ],
    ));

    // Agent Platform reads the attachment as its own service agent. The grant is project level
    // because the provider has no IAM resource for a network attachment, and Google measured
    // `NetworkAttachments.Get` as 403 without it. The agent only exists once the service has
    // been used, so setup creates it first.
    let agent = format!("{psc}_agent");
    fragment.resource_blocks.push(resource_block(
        "google_project_service_identity",
        &agent,
        [
            attr("provider", expr::raw("google-beta")),
            attr("project", expr::raw("var.gcp_project")),
            attr(
                "service",
                Expression::String("aiplatform.googleapis.com".to_string()),
            ),
        ],
    ));
    let project = format!("{psc}_project");
    fragment.data_blocks.push(data_block(
        "google_project",
        &project,
        [attr("project_id", expr::raw("var.gcp_project"))],
    ));
    fragment.resource_blocks.push(resource_block(
        "google_project_iam_member",
        &format!("{psc}_network_admin"),
        [
            attr("project", expr::raw("var.gcp_project")),
            attr(
                "role",
                Expression::String("roles/compute.networkAdmin".to_string()),
            ),
            attr(
                "member",
                expr::template(format!(
                    "serviceAccount:service-${{data.google_project.{project}.number}}@gcp-sa-aiplatform.iam.gserviceaccount.com"
                )),
            ),
            attr(
                "depends_on",
                expr::raw(format!("[google_project_service_identity.{agent}]")),
            ),
        ],
    ));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_range_is_the_sibling_of_the_declared_range() {
        assert_eq!(
            default_subnet_cidr("10.44.0.0/16").as_deref(),
            Some("10.45.0.0/28")
        );
        assert_eq!(
            default_subnet_cidr("10.45.0.0/16").as_deref(),
            Some("10.44.0.0/28")
        );
        assert_eq!(
            default_subnet_cidr("192.168.1.0/24").as_deref(),
            Some("192.168.0.0/28")
        );
        assert_eq!(
            default_subnet_cidr("10.0.0.0/28").as_deref(),
            Some("10.0.0.16/28")
        );
    }

    #[test]
    fn unusable_ranges_have_no_default() {
        for declared in [
            "10.0.0.0/28x",
            "10.0.0.0/32",
            "::1/64",
            "10.0.0.0/0",
            "10.0.0.0",
        ] {
            assert_eq!(default_subnet_cidr(declared), None, "{declared}");
        }
    }

    mod stacks {
        use super::super::super::network::GcpNetworkEmitter;
        use crate::emitter::TfEmitter;
        use alien_core::{
            import::EmitContext, Network, NetworkSettings, Platform, ResourceLifecycle, Sandbox,
            SandboxCode, SandboxEgress, Stack, StackSettings,
        };
        use indexmap::IndexMap;

        fn stack(settings: NetworkSettings, sandbox: Option<(SandboxEgress, Vec<u16>)>) -> Stack {
            let mut builder = Stack::new("acme".to_string()).add(
                Network::new("net".to_string()).settings(settings).build(),
                ResourceLifecycle::Frozen,
            );
            if let Some((egress, ports)) = sandbox {
                builder = builder.add(
                    Sandbox::new("agents".to_string())
                        .code(SandboxCode::Image {
                            image: "ubuntu".to_string(),
                            bundled_from: None,
                        })
                        .egress(egress)
                        .preview_ports(ports)
                        .lifecycle(alien_core::SandboxLifecyclePolicy {
                            max_lifetime_seconds: None,
                            idle_pause_seconds: None,
                        })
                        .build(),
                    ResourceLifecycle::Live,
                );
            }
            builder.build()
        }

        fn emit(stack: &Stack) -> (crate::emitter::TfFragment, hcl::expr::Expression) {
            let names = IndexMap::from([
                ("net".to_string(), "net".to_string()),
                ("agents".to_string(), "agents".to_string()),
            ]);
            let settings = StackSettings::default();
            let ctx = EmitContext {
                stack,
                resource: stack.resources.get("net").unwrap(),
                resource_id: "net",
                platform: Platform::Gcp,
                targets_kubernetes: false,
                stack_settings: &settings,
                names: &names,
                pins: &alien_core::SetupPins::default(),
            };
            (
                GcpNetworkEmitter.emit(&ctx).unwrap(),
                GcpNetworkEmitter.emit_import_ref(&ctx).unwrap(),
            )
        }

        fn types(fragment: &crate::emitter::TfFragment) -> Vec<String> {
            fragment
                .resource_blocks
                .iter()
                .map(|b| b.labels[0].as_str().to_string())
                .collect()
        }

        const CREATE: fn() -> NetworkSettings = || NetworkSettings::Create {
            cidr: None,
            availability_zones: 2,
        };

        #[test]
        fn a_stack_without_a_psc_sandbox_gets_no_new_resources_or_import_field() {
            for sandbox in [None, Some((SandboxEgress::Allow, vec![]))] {
                let (fragment, import_ref) = emit(&stack(CREATE(), sandbox));
                assert!(!types(&fragment)
                    .iter()
                    .any(|t| t == "google_compute_network_attachment"));
                assert!(!types(&fragment)
                    .iter()
                    .any(|t| t == "google_project_iam_member"));
                assert!(!format!("{import_ref:?}").contains("sandboxPscAttachmentSelfLink"));
            }
        }

        #[test]
        fn deny_internet_or_preview_ports_create_the_subnet_attachment_and_grant() {
            for sandbox in [
                (SandboxEgress::Internet, vec![]),
                (SandboxEgress::Deny, vec![]),
                (SandboxEgress::Deny, vec![8080]),
            ] {
                let (fragment, import_ref) = emit(&stack(CREATE(), Some(sandbox)));
                let kinds = types(&fragment);
                for kind in [
                    "google_compute_network_attachment",
                    "google_project_iam_member",
                    "google_project_service_identity",
                ] {
                    assert_eq!(kinds.iter().filter(|t| *t == kind).count(), 1, "{kind}");
                }
                let rendered = format!("{fragment:?}");
                assert!(
                    rendered.contains("10.45.0.0/28"),
                    "sibling of 10.44.0.0/16: {rendered}"
                );
                assert!(rendered.contains("roles/compute.networkAdmin"));
                assert!(rendered.contains("gcp-sa-aiplatform"));
                assert!(format!("{import_ref:?}").contains("sandboxPscAttachmentSelfLink"));
            }
        }

        #[test]
        fn an_existing_vpc_must_name_the_range() {
            let settings = NetworkSettings::ByoVpcGcp {
                network_name: "vpc".to_string(),
                subnet_name: "sub".to_string(),
                region: "us-central1".to_string(),
            };
            let (fragment, _) = emit(&stack(settings, Some((SandboxEgress::Internet, vec![]))));
            let rendered = format!("{fragment:?}");
            assert!(rendered.contains("sandbox_psc_subnet_cidr"));
            assert!(!rendered.contains("10.45.0.0/28"));
            assert!(rendered.contains("precondition"));
        }
    }
}
