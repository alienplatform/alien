//! The GCP wiring that sends Internet sandbox traffic through the egress gateway, owned by setup.
//!
//! Everything here renders from the shared description in `alien_core::gcp_sandbox_gateway`, which
//! direct setup renders too. Frozen, created once by initial setup:
//!
//! - the attachment subnet and the network attachment Agent Platform connects sandboxes through;
//! - the custom role that lets Agent Platform's service agent use the attachment, its binding, and
//!   a wait for the binding to take effect (a template created before it does leaves the
//!   attachment unusable for good);
//! - the gateway subnet, a Cloud NAT that serves that subnet only, and two gateway VMs;
//! - firewall rules: the attachment subnet reaches the gateways on the proxy port and nothing
//!   else, and everything else from it is denied and logged.

use alien_core::{
    gcp_sandbox_gateway as plan,
    import::EmitContext,
    sandbox_egress::{SANDBOX_EGRESS_GATEWAY_ID, SANDBOX_EGRESS_REPLICA_SECRET},
    Container, ContainerCode, ErrorData, NetworkSettings, Result, Sandbox, SandboxEgress,
    SECRETS_VAULT_ID,
};
use alien_error::AlienError;
use hcl::expr::Expression;

use crate::{
    block::{attr, block, data_block, nested, resource_block},
    emitter::TfFragment,
    emitters::gcp::network::{network_mode_expr, workload_cidr_expr},
    expr,
};

/// The secret holding the gateway configuration (see `alien-infra` `GATEWAY_CONFIG`).
pub const CONFIG_SECRET: &str = "sandbox-egress-gateway-config";

/// Whether this stack's setup creates the wiring: it declares an Internet sandbox and the
/// preparation step installed the gateway.
pub fn wiring_enabled(ctx: &EmitContext<'_>) -> bool {
    !ctx.targets_kubernetes
        && alien_core::sandbox_egress::has_sandbox_gateway(ctx.stack)
        && ctx.stack.resources().any(|(_, entry)| {
            entry
                .config
                .downcast_ref::<Sandbox>()
                .is_some_and(|sandbox| sandbox.egress == SandboxEgress::Internet)
        })
}

/// Label of the time delay a template-creating resource must wait for.
pub fn propagation_label(network: &str) -> String {
    format!("{network}_sbx_role_propagation")
}

fn name(suffix: &str) -> Expression {
    expr::raw(format!(
        "trim(substr(\"${{local.resource_prefix}}-{suffix}\", 0, 63), \"-\")"
    ))
}

fn s(value: &str) -> Expression {
    Expression::String(value.to_string())
}

/// The VPC the wiring joins, by `id`, in every network mode.
fn vpc_id(network: &str, settings: &NetworkSettings, k8s: bool) -> Expression {
    match settings {
        NetworkSettings::Create { .. } => {
            let mode = network_mode_expr(k8s);
            expr::raw(format!(
                "{mode} == \"create-new\" ? google_compute_network.{network}[0].id : data.google_compute_network.{network}[0].id"
            ))
        }
        _ => expr::traversal(["data", "google_compute_network", network, "id"]),
    }
}

fn subnet_range(label: &str) -> String {
    format!("google_compute_subnetwork.{label}.ip_cidr_range")
}

fn source_ranges(
    sources: &[plan::RuleSource],
    network: &str,
    settings: &NetworkSettings,
    k8s: bool,
) -> Expression {
    Expression::Array(
        sources
            .iter()
            .map(|source| match source {
                plan::RuleSource::Attachment => {
                    expr::raw(subnet_range(&format!("{network}_sbx_attachment")))
                }
                plan::RuleSource::Gateway => {
                    expr::raw(subnet_range(&format!("{network}_sbx_gateway")))
                }
                plan::RuleSource::Workload => workload_cidr_expr(network, settings, k8s),
            })
            .collect(),
    )
}

pub fn emit(
    ctx: &EmitContext<'_>,
    network: &str,
    settings: &NetworkSettings,
    fragment: &mut TfFragment,
) -> Result<()> {
    let k8s = ctx.targets_kubernetes;
    let image = gateway_image(ctx)?;
    let vault = ctx.name_for(SECRETS_VAULT_ID).ok_or_else(|| {
        unsupported("the egress gateway reads its configuration from the secrets vault, which the stack does not declare")
    })?;
    let _ = vault;
    let vpc = vpc_id(network, settings, k8s);
    let attachment_subnet = format!("{network}_sbx_attachment");
    let gateway_subnet = format!("{network}_sbx_gateway");
    let project = || attr("project", expr::raw("var.gcp_project"));
    let region = || attr("region", expr::raw("var.gcp_region"));

    // Machine type of the gateway VMs.
    fragment.variable_blocks.push(
        hcl::Block::builder("variable")
            .add_label(format!("{network}_sbx_gateway_machine_type"))
            .add_attribute(("type", expr::raw("string")))
            .add_attribute(("default", s(plan::DEFAULT_MACHINE_TYPE)))
            .add_attribute((
                "description",
                s("Compute Engine machine type of the two egress gateway VMs. Use one with at least 2 vCPUs and 4 GiB of memory that your project has quota for."),
            ))
            .build(),
    );

    // The attachment subnet and the attachment.
    fragment.resource_blocks.push(resource_block(
        "google_compute_subnetwork",
        &attachment_subnet,
        [
            attr("name", name(plan::suffix::ATTACHMENT_SUBNET)),
            project(),
            region(),
            attr("ip_cidr_range", s(plan::ATTACHMENT_CIDR)),
            attr("network", vpc.clone()),
        ],
    ));
    fragment.resource_blocks.push(resource_block(
        "google_compute_network_attachment",
        &attachment_subnet,
        [
            attr("name", name(plan::suffix::ATTACHMENT)),
            project(),
            region(),
            attr("connection_preference", s("ACCEPT_AUTOMATIC")),
            attr(
                "subnetworks",
                Expression::Array(vec![expr::traversal([
                    "google_compute_subnetwork",
                    &attachment_subnet,
                    "self_link",
                ])]),
            ),
        ],
    ));

    // The service agent, the role, the binding and the wait.
    let agent = format!("{network}_sbx_aiplatform_agent");
    fragment.resource_blocks.push(resource_block(
        "google_project_service_identity",
        &agent,
        [
            // Service agents exist only in the google-beta provider.
            attr("provider", expr::raw("google-beta")),
            project(),
            attr("service", s("aiplatform.googleapis.com")),
        ],
    ));
    let project_data = format!("{network}_sbx_project");
    fragment.data_blocks.push(data_block(
        "google_project",
        &project_data,
        [attr("project_id", expr::raw("var.gcp_project"))],
    ));
    let role = format!("{network}_sbx_attachment");
    fragment.resource_blocks.push(resource_block(
        "google_project_iam_custom_role",
        &role,
        [
            attr(
                "role_id",
                expr::raw(
                    "replace(\"${local.resource_prefix}_sandbox_attachment\", \"-\", \"_\")"
                        .to_string(),
                ),
            ),
            attr("title", s("Alien sandbox network attachment")),
            attr(
                "description",
                s("Lets Agent Platform connect sandboxes to the network attachment"),
            ),
            project(),
            attr(
                "permissions",
                Expression::Array(
                    plan::ATTACHMENT_ROLE_PERMISSIONS
                        .iter()
                        .map(|permission| s(permission))
                        .collect(),
                ),
            ),
        ],
    ));
    let binding = format!("{network}_sbx_attachment_agent");
    fragment.resource_blocks.push(resource_block(
        "google_project_iam_member",
        &binding,
        [
            project(),
            attr(
                "role",
                expr::traversal(["google_project_iam_custom_role", &role, "name"]),
            ),
            attr(
                "member",
                expr::template(format!(
                    "serviceAccount:service-${{data.google_project.{project_data}.number}}@gcp-sa-aiplatform.iam.gserviceaccount.com"
                )),
            ),
            attr(
                "depends_on",
                expr::raw(format!("[google_project_service_identity.{agent}]")),
            ),
        ],
    ));
    fragment.resource_blocks.push(resource_block(
        "time_sleep",
        &propagation_label(network),
        [
            attr(
                "create_duration",
                s(&format!("{}s", plan::ROLE_PROPAGATION_SECONDS)),
            ),
            attr(
                "depends_on",
                expr::raw(format!("[google_project_iam_member.{binding}]")),
            ),
        ],
    ));

    // Gateway subnet, and a NAT that serves it alone: the attachment subnet has no route out.
    fragment.resource_blocks.push(resource_block(
        "google_compute_subnetwork",
        &gateway_subnet,
        [
            attr("name", name(plan::suffix::GATEWAY_SUBNET)),
            project(),
            region(),
            attr("ip_cidr_range", s(plan::GATEWAY_CIDR)),
            attr("network", vpc.clone()),
            attr("private_ip_google_access", Expression::Bool(true)),
        ],
    ));
    let router = format!("{network}_sbx_gateway_router");
    fragment.resource_blocks.push(resource_block(
        "google_compute_router",
        &router,
        [
            attr("name", name(plan::suffix::ROUTER)),
            project(),
            region(),
            attr("network", vpc.clone()),
        ],
    ));
    fragment.resource_blocks.push(resource_block(
        "google_compute_router_nat",
        &format!("{network}_sbx_gateway_nat"),
        [
            attr("name", name(plan::suffix::NAT)),
            project(),
            region(),
            attr(
                "router",
                expr::traversal(["google_compute_router", &router, "name"]),
            ),
            attr("nat_ip_allocate_option", s("AUTO_ONLY")),
            attr(
                "source_subnetwork_ip_ranges_to_nat",
                s("LIST_OF_SUBNETWORKS"),
            ),
            nested(block(
                "subnetwork",
                [
                    attr(
                        "name",
                        expr::traversal([
                            "google_compute_subnetwork",
                            &gateway_subnet,
                            "self_link",
                        ]),
                    ),
                    attr(
                        "source_ip_ranges_to_nat",
                        Expression::Array(vec![s("ALL_IP_RANGES")]),
                    ),
                ],
            )),
        ],
    ));

    // Firewall rules.
    for rule in plan::firewall_rules(plan::DEFAULT_PROXY_PORT) {
        let mut body = vec![
            attr("name", name(rule.suffix)),
            project(),
            attr("network", vpc.clone()),
            attr("direction", s("INGRESS")),
            attr(
                "priority",
                Expression::Number(i64::from(rule.priority).into()),
            ),
            attr(
                "source_ranges",
                source_ranges(&rule.sources, network, settings, k8s),
            ),
        ];
        if rule.gateway_only {
            body.push(attr(
                "target_tags",
                Expression::Array(vec![s(plan::GATEWAY_TAG)]),
            ));
        }
        let protocol = |ports: &Option<Vec<u16>>| {
            let mut inner = vec![attr(
                "protocol",
                s(if ports.is_some() { "tcp" } else { "all" }),
            )];
            if let Some(ports) = ports {
                inner.push(attr(
                    "ports",
                    Expression::Array(ports.iter().map(|p| s(&p.to_string())).collect()),
                ));
            }
            nested(block(if rule.allow { "allow" } else { "deny" }, inner))
        };
        body.push(protocol(&rule.tcp_ports));
        if rule.log {
            body.push(nested(block(
                "log_config",
                [attr("metadata", s("INCLUDE_ALL_METADATA"))],
            )));
        }
        fragment.resource_blocks.push(resource_block(
            "google_compute_firewall",
            &format!("{network}_{}", rule.suffix.replace('-', "_")),
            body,
        ));
    }

    // The gateways' identity: reads the gateway configuration and replica secret, nothing else.
    let account = format!("{network}_sbx_gateway");
    fragment.resource_blocks.push(resource_block(
        "google_service_account",
        &account,
        [
            attr(
                "account_id",
                expr::raw(
                    "trim(substr(\"sbx-gw-${local.resource_prefix}\", 0, 30), \"-\")".to_string(),
                ),
            ),
            project(),
            attr("display_name", s("Alien sandbox egress gateway")),
        ],
    ));
    let vault_prefix = format!("${{local.resource_prefix}}-{SECRETS_VAULT_ID}");
    let secret_path = |secret: &str| {
        format!(
            "projects/${{data.google_project.{project_data}.number}}/secrets/{vault_prefix}-{secret}/"
        )
    };
    let condition = [CONFIG_SECRET, SANDBOX_EGRESS_REPLICA_SECRET]
        .iter()
        .map(|secret| format!("resource.name.startsWith(\\\"{}\\\")", secret_path(secret)))
        .collect::<Vec<_>>()
        .join(" || ");
    fragment.resource_blocks.push(resource_block(
        "google_project_iam_member",
        &format!("{account}_secrets"),
        [
            project(),
            attr("role", s("roles/secretmanager.secretAccessor")),
            attr(
                "member",
                expr::template(format!(
                    "serviceAccount:${{google_service_account.{account}.email}}"
                )),
            ),
            nested(block(
                "condition",
                [
                    attr("title", s("gateway secrets only")),
                    attr("expression", expr::template(condition)),
                ],
            )),
        ],
    ));

    // The VMs.
    fragment.data_blocks.push(data_block(
        "google_compute_zones",
        &format!("{network}_sbx"),
        [project(), region(), attr("status", s("UP"))],
    ));
    for (index, host) in plan::GATEWAY_HOSTS.iter().enumerate() {
        let vm = format!("{network}_sbx_gateway_{index}");
        let zones = format!("data.google_compute_zones.{network}_sbx.names");
        fragment.resource_blocks.push(resource_block(
            "google_compute_instance",
            &vm,
            [
                attr("name", name(&plan::suffix::vm(index))),
                project(),
                attr("zone", expr::raw(format!("{zones}[{index}]"))),
                attr(
                    "machine_type",
                    expr::raw(format!("var.{network}_sbx_gateway_machine_type")),
                ),
                attr("tags", Expression::Array(vec![s(plan::GATEWAY_TAG)])),
                nested(block(
                    "boot_disk",
                    [nested(block(
                        "initialize_params",
                        [
                            attr("image", s(plan::BOOT_IMAGE)),
                            attr("size", Expression::Number(plan::BOOT_DISK_GB.into())),
                        ],
                    ))],
                )),
                nested(block(
                    "network_interface",
                    [
                        attr(
                            "subnetwork",
                            expr::traversal([
                                "google_compute_subnetwork",
                                &gateway_subnet,
                                "self_link",
                            ]),
                        ),
                        attr(
                            "network_ip",
                            expr::raw(format!(
                                "cidrhost(google_compute_subnetwork.{gateway_subnet}.ip_cidr_range, {host})"
                            )),
                        ),
                    ],
                )),
                nested(block(
                    "service_account",
                    [
                        attr(
                            "email",
                            expr::traversal(["google_service_account", &account, "email"]),
                        ),
                        attr("scopes", Expression::Array(vec![s("cloud-platform")])),
                    ],
                )),
                nested(block(
                    "shielded_instance_config",
                    [
                        attr("enable_secure_boot", Expression::Bool(true)),
                        attr("enable_vtpm", Expression::Bool(true)),
                        attr("enable_integrity_monitoring", Expression::Bool(true)),
                    ],
                )),
                attr(
                    "metadata",
                    expr::object([
                        ("block-project-ssh-keys", s("true")),
                        ("enable-oslogin", s("TRUE")),
                    ]),
                ),
                attr("metadata_startup_script", startup_script(&image, &vault_prefix)),
                // The first boot needs the NAT to install Docker and pull the image.
                attr(
                    "depends_on",
                    expr::raw(format!(
                        "[google_compute_router_nat.{network}_sbx_gateway_nat, google_project_iam_member.{account}_secrets]"
                    )),
                ),
            ],
        ));
    }
    Ok(())
}

/// The import-data object recorded on the Network: what a template and the runtime need.
pub fn import_object(network: &str, settings: &NetworkSettings, k8s: bool) -> Expression {
    let attachment = format!("google_compute_network_attachment.{network}_sbx_attachment");
    let gateway_subnet = format!("{network}_sbx_gateway");
    expr::object([
        (
            "networkAttachment",
            expr::raw(format!(
                "\"projects/${{var.gcp_project}}/regions/${{var.gcp_region}}/networkAttachments/${{{attachment}.name}}\""
            )),
        ),
        (
            "attachmentCidr",
            expr::raw(subnet_range(&format!("{network}_sbx_attachment"))),
        ),
        ("gatewayCidr", expr::raw(subnet_range(&gateway_subnet))),
        (
            "gatewayIps",
            Expression::Array(
                (0..plan::GATEWAY_HOSTS.len())
                    .map(|index| {
                        expr::raw(format!(
                            "google_compute_instance.{network}_sbx_gateway_{index}.network_interface[0].network_ip"
                        ))
                    })
                    .collect(),
            ),
        ),
        (
            "proxyPort",
            Expression::Number(i64::from(plan::DEFAULT_PROXY_PORT).into()),
        ),
        (
            "vpcCidrs",
            Expression::Array(vec![
                workload_cidr_expr(network, settings, k8s),
                expr::raw(subnet_range(&format!("{network}_sbx_attachment"))),
                expr::raw(subnet_range(&gateway_subnet)),
            ]),
        ),
    ])
}

fn unsupported(reason: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::OperationNotSupported {
        operation: "GCP sandbox gateway setup".into(),
        reason: reason.into(),
    })
}

fn gateway_image(ctx: &EmitContext<'_>) -> Result<String> {
    ctx.stack
        .resources()
        .find(|(id, _)| id.as_str() == SANDBOX_EGRESS_GATEWAY_ID)
        .and_then(|(_, entry)| entry.config.downcast_ref::<Container>())
        .and_then(|container| match &container.code {
            ContainerCode::Image { image } => Some(image.clone()),
            _ => None,
        })
        .ok_or_else(|| unsupported("the egress gateway container must name an image"))
}

/// Escapes text for an HCL quoted string.
fn hcl_string(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace("${", "$${")
        .replace("%{", "%%{")
}

/// The boot script, from the shared description: Terraform fills its three values with `format`.
fn startup_script(image: &str, vault_prefix: &str) -> Expression {
    let body = hcl_string(&plan::startup_script_template(image));
    expr::raw(format!(
        "format(\"{body}\", var.gcp_project, var.gcp_region, jsonencode({{service = \"secret-manager\", vaultPrefix = \"{vault_prefix}\"}}))"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn script() -> String {
        let body = hcl::Body::builder()
            .add_attribute((
                "s",
                startup_script("registry.example/gw@sha256:abc", "p-secrets"),
            ))
            .build();
        hcl::to_string(&body).unwrap()
    }

    #[test]
    fn the_container_is_unprivileged_and_unchanged() {
        let text = script();
        for needed in [
            "--cap-drop ALL",
            "--memory 2g",
            "no-new-privileges",
            "--read-only",
            "--user 1000:1000",
            "registry.example/gw@sha256:abc",
            "-p 3128:3128",
            "-p 3129:3129",
        ] {
            assert!(text.contains(needed), "{needed} missing:\n{text}");
        }
        assert!(!text.contains("--privileged") && !text.contains("--network host"));
    }

    #[test]
    fn the_redirect_is_limited_to_the_attachment_range_and_names_the_port_once() {
        let text = script();
        assert!(text.contains("192.168.250.0/28"), "{text}");
        assert!(text.contains("--dport 22"), "{text}");
        let source = include_str!("sandbox_gateway.rs");
        let code = &source[..source.find("#[cfg(test)]").unwrap()];
        // The proxy port is read from the shared description, never written here.
        assert!(!code.contains("dport 22") && !code.contains("\"22\""));
    }

    #[test]
    fn the_script_survives_terraform_format() {
        let text = script();
        assert_eq!(text.matches("%s").count(), 3, "{text}");
        assert!(!text.contains("%{"), "{text}");
    }
}
