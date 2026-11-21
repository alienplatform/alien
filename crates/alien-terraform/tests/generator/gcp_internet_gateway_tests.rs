//! The GCP `internet` sandbox's gateway wiring, checked on the rendered module (parsed HCL).

use super::helpers::{prepare_for_target, render};
use alien_core::{
    ResourceLifecycle, Sandbox, SandboxCode, SandboxEgress, SandboxLifecyclePolicy, Stack,
    StackSettings,
};
use alien_terraform::TerraformTarget;
use hcl::Body;

async fn module_body(egress: SandboxEgress) -> Body {
    let sandbox = Sandbox::new("agents".to_string())
        .code(SandboxCode::Image {
            image: "us-central1-docker.pkg.dev/p/r/agents:1".to_string(),
            bundled_from: None,
        })
        .egress(egress)
        .lifecycle(SandboxLifecyclePolicy {
            max_lifetime_seconds: None,
            idle_pause_seconds: None,
            ..Default::default()
        })
        .build();
    let stack = Stack::new("gcpint".to_string())
        .add(sandbox, ResourceLifecycle::Frozen)
        .build();
    let prepared = prepare_for_target(stack, TerraformTarget::Gcp).await;
    let module = render(&prepared, TerraformTarget::Gcp, StackSettings::default());
    let mut merged = Body::builder();
    for (path, contents) in module.iter() {
        if path.ends_with(".tf") {
            let body: Body = hcl::from_str(contents).expect("the rendered file parses as HCL");
            for structure in body.into_iter() {
                merged = merged.add_structure(structure);
            }
        }
    }
    merged.build()
}

fn resources<'a>(body: &'a Body, kind: &str) -> Vec<&'a hcl::Block> {
    body.blocks()
        .filter(|b| {
            b.identifier() == "resource" && b.labels().first().map(|l| l.as_str()) == Some(kind)
        })
        .collect()
}

fn attribute(block: &hcl::Block, key: &str) -> String {
    block
        .body()
        .attributes()
        .find(|a| a.key() == key)
        .map(|a| a.expr().to_string())
        .unwrap_or_default()
}

#[tokio::test]
async fn an_internet_sandbox_gets_the_attachment_role_wait_gateways_and_firewalls() {
    let body = module_body(SandboxEgress::Internet).await;
    let attachments = resources(&body, "google_compute_network_attachment");
    assert_eq!(attachments.len(), 1);
    assert!(attribute(attachments[0], "connection_preference").contains("ACCEPT_AUTOMATIC"));
    assert_eq!(
        resources(&body, "google_compute_instance").len(),
        2,
        "two gateway VMs"
    );
    // Both NATs name their subnet: Google refuses an all-subnetworks NAT beside another NAT on
    // the same network (found by the live apply), so the workload NAT must not use that mode.
    let nats = resources(&body, "google_compute_router_nat");
    assert!(
        nats.len() >= 2
            && nats.iter().all(|b| {
                attribute(b, "source_subnetwork_ip_ranges_to_nat").contains("LIST_OF_SUBNETWORKS")
                    && b.body().blocks().any(|x| x.identifier() == "subnetwork")
            }),
        "every NAT lists its subnets: {:?}",
        nats.iter()
            .map(|b| attribute(b, "source_subnetwork_ip_ranges_to_nat"))
            .collect::<Vec<_>>()
    );

    // The role holds exactly the two attachment permissions, and a wait follows its binding.
    let role = resources(&body, "google_project_iam_custom_role");
    let permissions = role
        .iter()
        .map(|b| attribute(b, "permissions"))
        .find(|p| p.contains("networkAttachments"))
        .expect("the attachment role");
    assert!(
        permissions.contains("compute.networkAttachments.get"),
        "{permissions}"
    );
    assert!(
        permissions.contains("compute.networkAttachments.update"),
        "{permissions}"
    );
    let wait = resources(&body, "time_sleep")
        .into_iter()
        .find(|b| attribute(b, "create_duration").contains("180s"))
        .expect("the propagation wait");
    assert!(attribute(wait, "depends_on").contains("google_project_iam_member"));

    // The attachment range reaches the gateways on the proxy port only; everything else is
    // denied and logged, and the admin port never answers it.
    let firewalls = resources(&body, "google_compute_firewall");
    let attachment_rules: Vec<_> = firewalls
        .iter()
        .filter(|b| attribute(b, "source_ranges").contains("sbx_attachment"))
        .collect();
    assert_eq!(attachment_rules.len(), 2, "one allow and one deny");
    let allow = attachment_rules
        .iter()
        .find(|b| b.body().blocks().any(|x| x.identifier() == "allow"))
        .expect("allow");
    assert!(attribute(allow, "target_tags").contains("alien-sandbox-gateway"));
    let allow_block = allow
        .body()
        .blocks()
        .find(|x| x.identifier() == "allow")
        .unwrap();
    assert!(attribute_of(allow_block, "ports").contains("22"));
    assert!(!attribute_of(allow_block, "ports").contains("3129"));
    let deny = attachment_rules
        .iter()
        .find(|b| b.body().blocks().any(|x| x.identifier() == "deny"))
        .expect("deny");
    assert!(deny.body().blocks().any(|x| x.identifier() == "log_config"));
}

fn attribute_of(block: &hcl::Block, key: &str) -> String {
    attribute(block, key)
}

#[tokio::test]
async fn a_stack_without_an_internet_sandbox_renders_no_gateway_wiring() {
    let body = module_body(SandboxEgress::None).await;
    for kind in [
        "google_compute_network_attachment",
        "google_compute_instance",
    ] {
        assert!(resources(&body, kind).is_empty(), "{kind} is not rendered");
    }
    assert!(
        !resources(&body, "google_project_iam_custom_role")
            .iter()
            .any(|role| attribute(role, "permissions").contains("networkAttachments")),
        "no attachment role"
    );
}

/// The same facts the direct-setup request builders are checked against in `alien-infra`, from one
/// shared fixture: the two setup surfaces cannot drift apart without one of the tests failing.
#[tokio::test]
async fn the_rendered_firewalls_match_the_fixture_direct_setup_is_checked_against() {
    let body = module_body(SandboxEgress::Internet).await;
    let mut facts: Vec<serde_json::Value> = Vec::new();
    for block in resources(&body, "google_compute_firewall") {
        let label = block.labels()[1].as_str().to_string();
        let Some(suffix) = ["sbx-allow-proxy", "sbx-allow-admin", "sbx-deny-attachment"]
            .into_iter()
            .find(|s| label.ends_with(&s.replace('-', "_")))
        else {
            continue;
        };
        let (action, rule) = ["allow", "deny"]
            .into_iter()
            .find_map(|a| {
                block
                    .body()
                    .blocks()
                    .find(|b| b.identifier() == a)
                    .map(|b| (a, b))
            })
            .expect("allow or deny block");
        let ports: Vec<String> = rule
            .body()
            .attributes()
            .find(|a| a.key() == "ports")
            .map(|a| match a.expr() {
                hcl::Expression::Array(items) => items
                    .iter()
                    .map(|i| match i {
                        hcl::Expression::String(s) => s.clone(),
                        other => panic!("unexpected port {other}"),
                    })
                    .collect(),
                other => panic!("unexpected ports {other}"),
            })
            .unwrap_or_default();
        let ranges = attribute(block, "source_ranges");
        let sources: Vec<&str> = match suffix {
            "sbx-allow-proxy" | "sbx-deny-attachment" => {
                assert!(ranges.contains("sbx_attachment"), "{ranges}");
                vec!["attachment"]
            }
            _ => {
                assert!(ranges.contains("sbx_gateway"), "{ranges}");
                vec!["gateway", "workload"]
            }
        };
        facts.push(serde_json::json!({
            "suffix": suffix,
            "priority": attribute(block, "priority").parse::<i64>().unwrap(),
            "action": action,
            "protocol": attribute_of(rule, "protocol").trim_matches('"'),
            "ports": ports,
            "sources": sources,
            "gateway_only": block.body().attributes().any(|a| a.key() == "target_tags"),
            "log": block.body().blocks().any(|b| b.identifier() == "log_config"),
        }));
    }
    facts.sort_by_key(|f| {
        ["sbx-allow-proxy", "sbx-allow-admin", "sbx-deny-attachment"]
            .iter()
            .position(|s| f["suffix"] == *s)
    });
    let fixture: Vec<serde_json::Value> = serde_json::from_str(include_str!(
        "../../../alien-core/tests/fixtures/gcp_gateway_firewalls.json"
    ))
    .unwrap();
    assert_eq!(facts, fixture);
}
