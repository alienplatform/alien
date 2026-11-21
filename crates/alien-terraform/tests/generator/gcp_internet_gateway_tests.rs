//! The GCP `internet` sandbox's gateway wiring, checked on the rendered module (parsed HCL).

use super::helpers::{prepare_for_target, render, snapshot_module};
use alien_core::{
    ResourceLifecycle, Sandbox, SandboxCode, SandboxEgress, SandboxLifecyclePolicy, Stack,
    StackSettings,
};
use alien_terraform::TerraformTarget;
use hcl::eval::Evaluate;
use hcl::Body;

async fn module_body(egress: SandboxEgress) -> Body {
    module(egress).await.0
}

/// The rendered module, parsed, and the egress gateway's image.
async fn module(egress: SandboxEgress) -> (Body, String) {
    // The render runs the template preflights, which refuse Google Cloud `internet` until the
    // switch flips; these tests check the module the flipped release renders.
    let _on = alien_core::gcp_sandbox_gateway::enable_internet_egress_for_test();
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
    let image = prepared
        .resources()
        .find(|(id, _)| id.as_str() == alien_core::sandbox_egress::SANDBOX_EGRESS_GATEWAY_ID)
        .and_then(|(_, entry)| entry.config.downcast_ref::<alien_core::Container>())
        .and_then(|container| match &container.code {
            alien_core::ContainerCode::Image { image } => Some(image.clone()),
            _ => None,
        })
        .unwrap_or_default();
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
    (merged.build(), image)
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

async fn snapshot_stack(egress: SandboxEgress, lifecycle: ResourceLifecycle, name: &str) {
    let sandbox = Sandbox::new("agents".to_string())
        .code(SandboxCode::Image {
            image: "us-central1-docker.pkg.dev/p/r/agents:1".to_string(),
            bundled_from: None,
        })
        .egress(egress)
        .lifecycle(SandboxLifecyclePolicy::default())
        .build();
    let stack = Stack::new("gcpint".to_string())
        .add(sandbox, lifecycle)
        .build();
    let prepared = prepare_for_target(stack, TerraformTarget::Gcp).await;
    let module = render(&prepared, TerraformTarget::Gcp, StackSettings::default());
    snapshot_module(name, &module);
}

/// A Live `none` sandbox: its management role holds `sandbox/provision` and nothing for the
/// private endpoint, and no gateway wiring is rendered.
#[tokio::test]
async fn a_live_none_sandbox_module_snapshot() {
    snapshot_stack(
        SandboxEgress::None,
        ResourceLifecycle::Live,
        "gcp_live_none_sandbox",
    )
    .await;
}

/// The module the flipped release renders for a Live `internet` sandbox: the gateway wiring and
/// the private endpoint grant beside `sandbox/provision`.
#[tokio::test]
async fn a_live_internet_sandbox_module_snapshot() {
    let _on = alien_core::gcp_sandbox_gateway::enable_internet_egress_for_test();
    snapshot_stack(
        SandboxEgress::Internet,
        ResourceLifecycle::Live,
        "gcp_live_internet_sandbox",
    )
    .await;
}

mod evaluate {
    //! Just enough of Terraform's evaluation to read the gateway wiring as the cloud receives it:
    //! the functions the emitter uses and the attributes other resources read.

    use hcl::eval::{Context, Evaluate, FuncArgs, FuncDef, ParamType};
    use hcl::{Body, Value};

    fn text(value: &Value) -> Result<String, String> {
        value
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| format!("expected a string, got {value}"))
    }

    fn number(value: &Value) -> Result<usize, String> {
        value
            .as_u64()
            .map(|n| n as usize)
            .ok_or_else(|| format!("expected a number, got {value}"))
    }

    fn trim(args: FuncArgs) -> Result<Value, String> {
        let chars = text(&args[1])?;
        Ok(Value::from(
            text(&args[0])?.trim_matches(|c| chars.contains(c)),
        ))
    }

    fn substr(args: FuncArgs) -> Result<Value, String> {
        let value = text(&args[0])?;
        Ok(Value::from(
            value
                .chars()
                .skip(number(&args[1])?)
                .take(number(&args[2])?)
                .collect::<String>(),
        ))
    }

    fn replace(args: FuncArgs) -> Result<Value, String> {
        let pattern = text(&args[1])?;
        if pattern.starts_with('/') {
            return Err("regular expressions are not evaluated here".into());
        }
        Ok(Value::from(
            text(&args[0])?.replace(&pattern, &text(&args[2])?),
        ))
    }

    fn cidrhost(args: FuncArgs) -> Result<Value, String> {
        let range = text(&args[0])?;
        let (base, bits) = range.split_once('/').ok_or("not a range")?;
        let bits: u32 = bits.parse().map_err(|_| "bad prefix")?;
        let base: std::net::Ipv4Addr = base.parse().map_err(|_| "not IPv4")?;
        let host = number(&args[1])? as u32;
        if bits < 32 && host >= 1 << (32 - bits) {
            return Err("host outside the range".into());
        }
        Ok(Value::from(
            std::net::Ipv4Addr::from(u32::from(base) + host).to_string(),
        ))
    }

    fn format(args: FuncArgs) -> Result<Value, String> {
        let spec = text(&args[0])?;
        let mut values = args.iter().skip(1);
        let mut out = String::new();
        let mut chars = spec.chars();
        while let Some(c) = chars.next() {
            if c != '%' {
                out.push(c);
                continue;
            }
            match chars.next() {
                Some('%') => out.push('%'),
                Some('s') => out.push_str(&text(values.next().ok_or("too few values")?)?),
                other => return Err(format!("unsupported verb %{other:?}")),
            }
        }
        if values.next().is_some() {
            return Err("too many values".into());
        }
        Ok(Value::from(out))
    }

    fn jsonencode(args: FuncArgs) -> Result<Value, String> {
        serde_json::to_string(&args[0])
            .map(Value::from)
            .map_err(|e| e.to_string())
    }

    pub fn context() -> Context<'static> {
        let mut ctx = Context::new();
        let s = ParamType::String;
        let n = ParamType::Number;
        ctx.declare_func("trim", FuncDef::new(trim, [s.clone(), s.clone()]));
        ctx.declare_func(
            "substr",
            FuncDef::new(substr, [s.clone(), n.clone(), n.clone()]),
        );
        ctx.declare_func(
            "replace",
            FuncDef::new(replace, [s.clone(), s.clone(), s.clone()]),
        );
        ctx.declare_func("cidrhost", FuncDef::new(cidrhost, [s.clone(), n]));
        ctx.declare_func(
            "format",
            FuncDef::builder()
                .param(s)
                .variadic_param(ParamType::Any)
                .build(format),
        );
        ctx.declare_func("jsonencode", FuncDef::new(jsonencode, [ParamType::Any]));
        ctx
    }

    /// A block's attributes and nested blocks, evaluated. Ordering and provider meta-arguments
    /// are not part of what the cloud receives.
    pub fn block(body: &Body, ctx: &Context<'_>) -> serde_json::Value {
        let mut out = serde_json::Map::new();
        for attribute in body.attributes() {
            if ["depends_on", "provider", "project", "region"].contains(&attribute.key()) {
                continue;
            }
            let value = attribute
                .expr()
                .evaluate(ctx)
                .unwrap_or_else(|e| panic!("{} does not evaluate: {e}", attribute.key()));
            out.insert(
                attribute.key().to_string(),
                serde_json::to_value(value).unwrap(),
            );
        }
        for nested in body.blocks() {
            let value = block(nested.body(), ctx);
            match out.get_mut(nested.identifier()) {
                Some(serde_json::Value::Array(items)) => items.push(value),
                Some(first) => *first = serde_json::Value::Array(vec![first.take(), value]),
                None => {
                    out.insert(nested.identifier().to_string(), value);
                }
            }
        }
        serde_json::Value::Object(out)
    }

    /// Sets `root.path...` to `value` in the context's variables, merging with what is there.
    pub fn declare(vars: &mut serde_json::Value, path: &[&str], value: serde_json::Value) {
        let mut at = vars;
        for key in &path[..path.len() - 1] {
            at = at
                .as_object_mut()
                .unwrap()
                .entry(key.to_string())
                .or_insert_with(|| serde_json::json!({}));
        }
        at.as_object_mut()
            .unwrap()
            .insert(path[path.len() - 1].to_string(), value);
    }

    pub fn with_vars(vars: &serde_json::Value) -> Context<'static> {
        let mut ctx = context();
        for (key, value) in vars.as_object().unwrap() {
            let value: Value = serde_json::from_value(value.clone()).unwrap();
            ctx.declare_var(key.as_str(), value);
        }
        ctx
    }
}

/// The rendered gateway wiring, evaluated as Terraform would for a deployment with resource
/// prefix `acme` in project `proj` (number 123), reduced to the facts in the shared fixture that
/// direct setup's requests are checked against in `alien-infra`
/// (`gcp_sandbox_gateway_setup.rs`). A difference in any name, range, NAT setting, role, binding,
/// account or VM between the two setup surfaces fails one of the two tests, and every attribute
/// the module sets must be one of those facts.
#[tokio::test]
async fn the_rendered_wiring_matches_the_fixture_direct_setup_is_checked_against() {
    use serde_json::{json, Value as Json};
    let (body, image) = module(SandboxEgress::Internet).await;
    assert!(!image.is_empty(), "the gateway container names an image");
    let net = "default_network";
    let l = |suffix: &str| format!("{net}_{suffix}");
    let mut seen: Vec<String> = Vec::new();
    let mut find = |identifier: &str, labels: &[&str]| -> hcl::Body {
        seen.push(labels.join("."));
        body.blocks()
            .find(|b| {
                b.identifier() == identifier
                    && b.labels()
                        .iter()
                        .map(|l| l.as_str())
                        .eq(labels.iter().copied())
            })
            .unwrap_or_else(|| panic!("{identifier} {labels:?} is rendered"))
            .body()
            .clone()
    };
    // Every gateway block is accounted for below.
    let rendered: Vec<String> = body
        .blocks()
        .filter(|b| {
            b.labels()
                .iter()
                .any(|l| l.as_str().starts_with(&l_prefix(net)))
        })
        .map(|b| {
            b.labels()
                .iter()
                .map(|l| l.as_str())
                .collect::<Vec<_>>()
                .join(".")
        })
        .collect();
    fn l_prefix(net: &str) -> String {
        format!("{net}_sbx")
    }

    /// Removes and returns an attribute; what is left at the end was not compared.
    fn take(object: &mut Json, key: &str) -> Json {
        object
            .as_object_mut()
            .unwrap()
            .remove(key)
            .unwrap_or_else(|| panic!("no {key} in {object}"))
    }
    fn done(object: Json) {
        assert_eq!(object, json!({}), "attributes no fact covers");
    }
    fn leaf(link: &Json) -> Json {
        json!(link.as_str().unwrap().rsplit('/').next().unwrap())
    }

    let mut vars = json!({});
    let set = |vars: &mut Json, path: &[&str], value: Json| evaluate::declare(vars, path, value);
    set(&mut vars, &["local", "resource_prefix"], json!("acme"));
    set(&mut vars, &["var", "gcp_project"], json!("proj"));
    set(&mut vars, &["var", "gcp_region"], json!("us-central1"));
    set(&mut vars, &["var", "network_mode"], json!("create-new"));
    set(
        &mut vars,
        &["google_compute_network", net],
        json!([{"id": "vpc"}]),
    );
    set(
        &mut vars,
        &["data", "google_compute_network", net],
        json!([{"id": "vpc"}]),
    );
    let project_data = l("sbx_project");
    set(
        &mut vars,
        &["data", "google_project", &project_data, "number"],
        json!("123"),
    );
    let zones = l("sbx");
    set(
        &mut vars,
        &["data", "google_compute_zones", &zones, "names"],
        json!(["zone-0", "zone-1"]),
    );
    let machine = l("sbx_gateway_machine_type");
    // A variable's `type` is a type, not a value: only its default is read.
    let variable = find("variable", &[&machine]);
    let default = variable
        .attributes()
        .find(|a| a.key() == "default")
        .expect("the machine type has a default")
        .expr()
        .evaluate(&evaluate::context())
        .unwrap();
    set(
        &mut vars,
        &["var", &machine],
        serde_json::to_value(default).unwrap(),
    );
    let mut project = evaluate::block(
        &find("data", &["google_project", &project_data]),
        &evaluate::with_vars(&vars),
    );
    assert_eq!(take(&mut project, "project_id"), json!("proj"));
    done(project);
    let mut zone_data = evaluate::block(
        &find("data", &["google_compute_zones", &zones]),
        &evaluate::with_vars(&vars),
    );
    assert_eq!(take(&mut zone_data, "status"), json!("UP"));
    done(zone_data);

    let vpc = |value: Json| {
        assert_eq!(value, json!("vpc"));
        json!("vpc")
    };
    let mut subnets = Vec::new();
    for label in [l("sbx_attachment"), l("sbx_gateway")] {
        let mut subnet = evaluate::block(
            &find("resource", &["google_compute_subnetwork", &label]),
            &evaluate::with_vars(&vars),
        );
        let name = take(&mut subnet, "name");
        let cidr = take(&mut subnet, "ip_cidr_range");
        let private = subnet
            .as_object_mut()
            .unwrap()
            .remove("private_ip_google_access")
            .unwrap_or(json!(false));
        subnets.push(json!({
            "name": name,
            "network": vpc(take(&mut subnet, "network")),
            "cidr": cidr,
            "private_google_access": private,
        }));
        done(subnet);
        set(
            &mut vars,
            &["google_compute_subnetwork", &label],
            json!({
                "name": name,
                "ip_cidr_range": cidr,
                "self_link": format!("projects/proj/regions/us-central1/subnetworks/{}", name.as_str().unwrap()),
            }),
        );
    }
    let ctx = evaluate::with_vars(&vars);
    let mut attachment = evaluate::block(
        &find(
            "resource",
            &["google_compute_network_attachment", &l("sbx_attachment")],
        ),
        &ctx,
    );
    let attachment_facts = json!({
        "name": take(&mut attachment, "name"),
        "connection_preference": take(&mut attachment, "connection_preference"),
        "subnets": take(&mut attachment, "subnetworks").as_array().unwrap().iter().map(leaf).collect::<Vec<_>>(),
    });
    done(attachment);
    let mut identity = evaluate::block(
        &find(
            "resource",
            &[
                "google_project_service_identity",
                &l("sbx_aiplatform_agent"),
            ],
        ),
        &ctx,
    );
    let service = take(&mut identity, "service");
    done(identity);
    let mut role = evaluate::block(
        &find(
            "resource",
            &["google_project_iam_custom_role", &l("sbx_attachment")],
        ),
        &ctx,
    );
    let role_id = take(&mut role, "role_id");
    let role_facts = json!({
        "id": role_id,
        "title": take(&mut role, "title"),
        "description": take(&mut role, "description"),
        "permissions": take(&mut role, "permissions"),
    });
    done(role);
    set(
        &mut vars,
        &[
            "google_project_iam_custom_role",
            &l("sbx_attachment"),
            "name",
        ],
        json!(format!("projects/proj/roles/{}", role_id.as_str().unwrap())),
    );
    let router_label = l("sbx_gateway_router");
    let mut router = evaluate::block(
        &find("resource", &["google_compute_router", &router_label]),
        &evaluate::with_vars(&vars),
    );
    let router_name = take(&mut router, "name");
    let router_network = vpc(take(&mut router, "network"));
    done(router);
    set(
        &mut vars,
        &["google_compute_router", &router_label, "name"],
        router_name.clone(),
    );
    let account = l("sbx_gateway");
    let mut gateway_account = evaluate::block(
        &find("resource", &["google_service_account", &account]),
        &evaluate::with_vars(&vars),
    );
    let account_id = take(&mut gateway_account, "account_id");
    let account_facts = json!({
        "id": account_id,
        "display_name": take(&mut gateway_account, "display_name"),
    });
    done(gateway_account);
    set(
        &mut vars,
        &["google_service_account", &account, "email"],
        json!(format!(
            "{}@proj.iam.gserviceaccount.com",
            account_id.as_str().unwrap()
        )),
    );
    let ctx = evaluate::with_vars(&vars);

    let mut nat = evaluate::block(
        &find(
            "resource",
            &["google_compute_router_nat", &l("sbx_gateway_nat")],
        ),
        &ctx,
    );
    assert_eq!(take(&mut nat, "router"), router_name);
    let nat_subnets = match take(&mut nat, "subnetwork") {
        Json::Array(items) => items,
        one => vec![one],
    };
    let nat_facts = json!({
        "name": take(&mut nat, "name"),
        "ip_allocate": take(&mut nat, "nat_ip_allocate_option"),
        "source": take(&mut nat, "source_subnetwork_ip_ranges_to_nat"),
        "subnets": nat_subnets.into_iter().map(|mut s| {
            let fact = json!({
                "subnet": leaf(&take(&mut s, "name")),
                "ranges": take(&mut s, "source_ip_ranges_to_nat"),
            });
            done(s);
            fact
        }).collect::<Vec<_>>(),
    });
    done(nat);
    let mut binding = evaluate::block(
        &find(
            "resource",
            &["google_project_iam_member", &l("sbx_attachment_agent")],
        ),
        &ctx,
    );
    let binding_facts = json!({
        "role": take(&mut binding, "role"),
        "member": take(&mut binding, "member"),
    });
    done(binding);
    let mut secrets = evaluate::block(
        &find(
            "resource",
            &["google_project_iam_member", &format!("{account}_secrets")],
        ),
        &ctx,
    );
    let mut condition = take(&mut secrets, "condition");
    let secret_facts = json!({
        "role": take(&mut secrets, "role"),
        "member": take(&mut secrets, "member"),
        "title": take(&mut condition, "title"),
        "expression": take(&mut condition, "expression"),
    });
    done(condition);
    done(secrets);

    let script = alien_core::gcp_sandbox_gateway::startup_script(
        &image,
        "proj",
        "us-central1",
        "acme-secrets",
    );
    let mut vms = Vec::new();
    for index in 0..alien_core::gcp_sandbox_gateway::GATEWAY_HOSTS.len() {
        let mut vm = evaluate::block(
            &find(
                "resource",
                &[
                    "google_compute_instance",
                    &l(&format!("sbx_gateway_{index}")),
                ],
            ),
            &ctx,
        );
        assert_eq!(take(&mut vm, "metadata_startup_script"), json!(script));
        let mut disk = take(&mut vm, "boot_disk");
        let mut params = take(&mut disk, "initialize_params");
        done(disk);
        let mut nic = take(&mut vm, "network_interface");
        let mut identity = take(&mut vm, "service_account");
        let mut shielded = take(&mut vm, "shielded_instance_config");
        let zone = take(&mut vm, "zone");
        let zone = ["zone-0", "zone-1"]
            .iter()
            .position(|z| json!(z) == zone)
            .expect("a zone of the region");
        let fact = json!({
            "name": take(&mut vm, "name"),
            "zone": zone,
            "machine_type": take(&mut vm, "machine_type"),
            "tags": take(&mut vm, "tags"),
            "image": take(&mut params, "image"),
            "disk_gb": take(&mut params, "size"),
            "subnet": leaf(&take(&mut nic, "subnetwork")),
            "ip": take(&mut nic, "network_ip"),
            "service_account": take(&mut identity, "email"),
            // The provider's alias for the full scope URL direct setup sends.
            "scopes": take(&mut identity, "scopes").as_array().unwrap().iter().map(|scope| {
                match scope.as_str().unwrap() {
                    "cloud-platform" => json!("https://www.googleapis.com/auth/cloud-platform"),
                    other => json!(other),
                }
            }).collect::<Vec<_>>(),
            "shielded": {
                "secure_boot": take(&mut shielded, "enable_secure_boot"),
                "vtpm": take(&mut shielded, "enable_vtpm"),
                "integrity_monitoring": take(&mut shielded, "enable_integrity_monitoring"),
            },
            "metadata": take(&mut vm, "metadata"),
            "startup_script": "the shared script for project proj, region us-central1, vault prefix acme-secrets",
        });
        for left in [params, nic, identity, shielded, vm] {
            done(left);
        }
        vms.push(fact);
    }

    let facts = json!({
        "subnets": subnets,
        "attachment": attachment_facts,
        "router": {"name": router_name, "network": router_network, "nat": nat_facts},
        "service_identity": service,
        "role": role_facts,
        "role_binding": binding_facts,
        "gateway_account": account_facts,
        "secret_binding": secret_facts,
        "vms": vms,
    });
    let fixture: Json = serde_json::from_str(include_str!(
        "../../../alien-core/tests/fixtures/gcp_gateway_setup.json"
    ))
    .unwrap();
    assert_eq!(facts, fixture);

    // The firewalls are compared in their own test; the wait has no cloud counterpart.
    let mut covered = seen;
    covered.push(format!("time_sleep.{net}_sbx_role_propagation"));
    for rule in ["sbx_allow_proxy", "sbx_allow_admin", "sbx_deny_attachment"] {
        covered.push(format!("google_compute_firewall.{}", l(rule)));
    }
    for block in rendered {
        assert!(
            covered.contains(&block),
            "{block} is rendered but not compared: {covered:?}"
        );
    }
}
