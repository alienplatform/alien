//! Direct setup's requests for the GCP sandbox gateway wiring.
//!
//! Every name, range, port, permission and firewall rule comes from the shared description in
//! `alien_core::gcp_sandbox_gateway`, the one the Terraform emitter renders too, so the two setup
//! surfaces create the same resources. The tests below check each request against that
//! description; the Terraform emitter's tests check its output against the same one.

use alien_core::gcp_sandbox_gateway as plan;
use alien_gcp_clients::gcp::compute::{
    Firewall, FirewallAllowed, FirewallDenied, FirewallDirection, FirewallLogConfig,
    FirewallLogConfigMetadata, NatIpAllocateOption, NetworkAttachment, Router, RouterNat,
    RouterNatSubnetworkToNat, SourceIpRangesToNat, SourceSubnetworkIpRangesToNat, Subnetwork,
};
use alien_gcp_clients::gcp::iam::{CreateRoleRequest, Role, RoleLaunchStage};

/// Where the wiring lives: the network, project and region it joins and the resource prefix its
/// names carry.
pub(crate) struct Placement<'a> {
    pub prefix: &'a str,
    pub project: &'a str,
    pub region: &'a str,
    /// Self link of the VPC.
    pub network: &'a str,
    /// Range of the workload subnet, the manager's path to the gateway admin port.
    pub workload_cidr: &'a str,
}

impl Placement<'_> {
    pub(crate) fn name(&self, suffix: &str) -> String {
        plan::resource_name(self.prefix, suffix)
    }

    pub(crate) fn subnet_link(&self, suffix: &str) -> String {
        format!(
            "projects/{}/regions/{}/subnetworks/{}",
            self.project,
            self.region,
            self.name(suffix)
        )
    }

    pub(crate) fn attachment_path(&self) -> String {
        plan::attachment_path(
            self.project,
            self.region,
            &self.name(plan::suffix::ATTACHMENT),
        )
    }
}

pub(crate) fn attachment_subnet(p: &Placement<'_>) -> Subnetwork {
    Subnetwork::builder()
        .name(p.name(plan::suffix::ATTACHMENT_SUBNET))
        .network(p.network.to_string())
        .ip_cidr_range(plan::ATTACHMENT_CIDR.to_string())
        .build()
}

pub(crate) fn network_attachment(p: &Placement<'_>) -> NetworkAttachment {
    NetworkAttachment::builder()
        .name(p.name(plan::suffix::ATTACHMENT))
        .connection_preference("ACCEPT_AUTOMATIC".to_string())
        .subnetworks(vec![p.subnet_link(plan::suffix::ATTACHMENT_SUBNET)])
        .build()
}

pub(crate) fn gateway_subnet(p: &Placement<'_>) -> Subnetwork {
    Subnetwork::builder()
        .name(p.name(plan::suffix::GATEWAY_SUBNET))
        .network(p.network.to_string())
        .ip_cidr_range(plan::GATEWAY_CIDR.to_string())
        .private_ip_google_access(true)
        .build()
}

/// The router, with a NAT that serves the gateway subnet alone: the attachment subnet has no
/// route out.
pub(crate) fn gateway_router(p: &Placement<'_>) -> Router {
    Router::builder()
        .name(p.name(plan::suffix::ROUTER))
        .network(p.network.to_string())
        .nats(vec![RouterNat::builder()
            .name(p.name(plan::suffix::NAT))
            .nat_ip_allocate_option(NatIpAllocateOption::AutoOnly)
            .source_subnetwork_ip_ranges_to_nat(SourceSubnetworkIpRangesToNat::ListOfSubnetworks)
            .subnetworks(vec![RouterNatSubnetworkToNat::builder()
                .name(p.subnet_link(plan::suffix::GATEWAY_SUBNET))
                .source_ip_ranges_to_nat(vec![SourceIpRangesToNat::AllIpRanges])
                .build()])
            .build()])
        .build()
}

fn source_range(p: &Placement<'_>, source: plan::RuleSource) -> String {
    match source {
        plan::RuleSource::Attachment => plan::ATTACHMENT_CIDR.to_string(),
        plan::RuleSource::Gateway => plan::GATEWAY_CIDR.to_string(),
        plan::RuleSource::Workload => p.workload_cidr.to_string(),
    }
}

/// The firewall rules, one request each.
pub(crate) fn firewalls(p: &Placement<'_>, proxy_port: u16) -> Vec<Firewall> {
    plan::firewall_rules(proxy_port)
        .into_iter()
        .map(|rule| {
            let protocol = if rule.tcp_ports.is_some() {
                "tcp"
            } else {
                "all"
            };
            let ports: Vec<String> = rule
                .tcp_ports
                .iter()
                .flatten()
                .map(|port| port.to_string())
                .collect();
            let mut firewall = Firewall::builder()
                .name(p.name(rule.suffix))
                .network(p.network.to_string())
                .direction(FirewallDirection::Ingress)
                .priority(rule.priority as i32)
                .source_ranges(
                    rule.sources
                        .iter()
                        .map(|source| source_range(p, *source))
                        .collect(),
                )
                .target_tags(if rule.gateway_only {
                    vec![plan::GATEWAY_TAG.to_string()]
                } else {
                    Vec::new()
                })
                .build();
            if rule.allow {
                firewall.allowed = vec![FirewallAllowed::builder()
                    .ip_protocol(protocol.to_string())
                    .ports(ports)
                    .build()];
            } else {
                firewall.denied = vec![FirewallDenied::builder()
                    .ip_protocol(protocol.to_string())
                    .ports(ports)
                    .build()];
            }
            if rule.log {
                firewall.log_config = Some(FirewallLogConfig {
                    enable: Some(true),
                    metadata: Some(FirewallLogConfigMetadata::IncludeAllMetadata),
                });
            }
            firewall
        })
        .collect()
}

/// The custom role the Agent Platform service agent holds on the wiring.
pub(crate) fn attachment_role(p: &Placement<'_>) -> (String, CreateRoleRequest) {
    (
        plan::role_id(p.prefix),
        CreateRoleRequest {
            role: Role::builder()
                .title("Alien sandbox network attachment".to_string())
                .description(
                    "Lets Agent Platform connect sandboxes to the network attachment".to_string(),
                )
                .included_permissions(
                    plan::ATTACHMENT_ROLE_PERMISSIONS
                        .iter()
                        .map(|permission| permission.to_string())
                        .collect(),
                )
                .stage(RoleLaunchStage::Ga)
                .build(),
        },
    )
}

/// The member the role is bound to: Agent Platform's service agent in this project.
pub(crate) fn attachment_member(project_number: &str) -> String {
    format!("serviceAccount:service-{project_number}@gcp-sa-aiplatform.iam.gserviceaccount.com")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn placement() -> Placement<'static> {
        Placement {
            prefix: "acme",
            project: "proj",
            region: "us-central1",
            network: "projects/proj/global/networks/vpc",
            workload_cidr: "100.70.0.0/16",
        }
    }

    #[test]
    fn the_attachment_connects_automatically_on_its_own_subnet() {
        let p = placement();
        let attachment = network_attachment(&p);
        assert_eq!(
            attachment.connection_preference.as_deref(),
            Some("ACCEPT_AUTOMATIC")
        );
        assert_eq!(
            attachment.subnetworks,
            vec!["projects/proj/regions/us-central1/subnetworks/acme-sbx-attachment"]
        );
        assert_eq!(
            attachment_subnet(&p).ip_cidr_range.as_deref(),
            Some(plan::ATTACHMENT_CIDR)
        );
        assert_eq!(
            p.attachment_path(),
            "projects/proj/regions/us-central1/networkAttachments/acme-sbx-attachment"
        );
    }

    #[test]
    fn the_nat_serves_the_gateway_subnet_only() {
        let router = gateway_router(&placement());
        assert_eq!(router.nats.len(), 1);
        let nat = &router.nats[0];
        assert_eq!(
            nat.source_subnetwork_ip_ranges_to_nat,
            Some(SourceSubnetworkIpRangesToNat::ListOfSubnetworks)
        );
        assert_eq!(nat.subnetworks.len(), 1);
        assert!(nat.subnetworks[0]
            .name
            .as_deref()
            .unwrap()
            .ends_with("/acme-sbx-gateway"));
    }

    #[test]
    fn the_attachment_range_reaches_the_proxy_port_and_nothing_else() {
        for port in [plan::DEFAULT_PROXY_PORT, 2222] {
            let rules = firewalls(&placement(), port);
            assert_eq!(rules.len(), plan::firewall_rules(port).len());
            let from_attachment: Vec<_> = rules
                .iter()
                .filter(|rule| {
                    rule.source_ranges
                        .contains(&plan::ATTACHMENT_CIDR.to_string())
                })
                .collect();
            assert_eq!(from_attachment.len(), 2, "one allow and one deny");
            let allow = from_attachment
                .iter()
                .find(|r| !r.allowed.is_empty())
                .unwrap();
            assert_eq!(allow.allowed[0].ports, vec![port.to_string()]);
            assert_eq!(allow.target_tags, vec![plan::GATEWAY_TAG.to_string()]);
            let deny = from_attachment
                .iter()
                .find(|r| !r.denied.is_empty())
                .unwrap();
            assert!(deny
                .log_config
                .as_ref()
                .is_some_and(|log| log.enable == Some(true)));
            assert!(deny.priority > allow.priority);
            // The admin port never answers the attachment range.
            for rule in &rules {
                if rule
                    .source_ranges
                    .contains(&plan::ATTACHMENT_CIDR.to_string())
                {
                    assert!(rule
                        .allowed
                        .iter()
                        .all(|a| !a.ports.contains(&plan::ADMIN_PORT.to_string())));
                }
            }
        }
    }

    #[test]
    fn the_role_holds_exactly_the_two_attachment_permissions() {
        let (id, request) = attachment_role(&placement());
        assert_eq!(id, "acme_sandbox_attachment");
        assert_eq!(
            request.role.included_permissions,
            vec![
                "compute.networkAttachments.get".to_string(),
                "compute.networkAttachments.update".to_string()
            ]
        );
        assert_eq!(
            attachment_member("123"),
            "serviceAccount:service-123@gcp-sa-aiplatform.iam.gserviceaccount.com"
        );
    }

    /// The same facts the Terraform emitter's test derives from its rendered module, checked
    /// against one shared fixture: the two setup surfaces cannot drift apart without one failing.
    #[test]
    fn the_firewall_requests_match_the_fixture_the_terraform_module_is_checked_against() {
        let p = placement();
        let facts: Vec<serde_json::Value> = firewalls(&p, plan::DEFAULT_PROXY_PORT)
            .iter()
            .map(|f| {
                let (action, rules) = if f.allowed.is_empty() {
                    (
                        "deny",
                        f.denied
                            .iter()
                            .map(|d| (&d.ip_protocol, &d.ports))
                            .collect::<Vec<_>>(),
                    )
                } else {
                    (
                        "allow",
                        f.allowed
                            .iter()
                            .map(|a| (&a.ip_protocol, &a.ports))
                            .collect::<Vec<_>>(),
                    )
                };
                let sources: Vec<&str> = f
                    .source_ranges
                    .iter()
                    .map(|range| match range.as_str() {
                        plan::ATTACHMENT_CIDR => "attachment",
                        plan::GATEWAY_CIDR => "gateway",
                        other if other == p.workload_cidr => "workload",
                        other => panic!("unexpected source {other}"),
                    })
                    .collect();
                serde_json::json!({
                    "suffix": f.name.as_deref().unwrap().strip_prefix("acme-").unwrap(),
                    "priority": f.priority,
                    "action": action,
                    "protocol": rules[0].0,
                    "ports": rules[0].1,
                    "sources": sources,
                    "gateway_only": !f.target_tags.is_empty(),
                    "log": f.log_config.is_some(),
                })
            })
            .collect();
        let fixture: Vec<serde_json::Value> = serde_json::from_str(include_str!(
            "../../../alien-core/tests/fixtures/gcp_gateway_firewalls.json"
        ))
        .unwrap();
        assert_eq!(facts, fixture);
    }
}
