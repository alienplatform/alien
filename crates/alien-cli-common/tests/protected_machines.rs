//! Machines on protected GCP and Azure installs: the verifier launches them into the network
//! the compute cluster's emitter names (`TfEmitter::machine_network`), read from its own
//! `VERIFIER_MACHINE_NETWORK`, and the management identity can't change them.

use alien_cli_common::setup_templates::{
    render_setup_template, RenderedSetupTemplate, SetupHostingOptions, SetupRenderOptions,
};
use alien_cloudformation::CfRegistry;
use alien_core::{
    import::EmitContext, ComputeCluster, Platform, SetupPins, Stack, StackSettings, StackSigning,
};
use alien_helm::HelmRegistry;
use alien_terraform::{expr, TfEmitter, TfFragment, TfRegistry};
use hcl::Expression;

const FINGERPRINT: &str = "SHA256:9f2c3b1a77d0e5c20b19a4f36c8e2d5100112233445566778899aabbccddeeff";

/// Stands in for a composed CLI's compute-cluster emitter: it creates nothing and names the
/// machine network it was built with.
struct Cluster(Option<&'static str>);

impl TfEmitter for Cluster {
    fn emit(&self, _ctx: &EmitContext<'_>) -> alien_core::Result<TfFragment> {
        Ok(TfFragment::default())
    }

    fn emit_import_ref(&self, _ctx: &EmitContext<'_>) -> alien_core::Result<Expression> {
        Ok(expr::raw("true"))
    }

    fn machine_network(&self, _ctx: &EmitContext<'_>) -> alien_core::Result<Option<Expression>> {
        Ok(self.0.map(|subnet| {
            expr::jsonencode(expr::object([
                ("subnetIds", Expression::Array(vec![expr::template(subnet)])),
                (
                    "securityGroupIds",
                    Expression::Array(vec![expr::template("${local.resource_prefix}-machines")]),
                ),
                (
                    "identity",
                    expr::template("${local.resource_prefix}-machines"),
                ),
            ]))
        }))
    }
}

fn pins() -> SetupPins {
    SetupPins {
        aws_publisher: Some("111122223333".to_string()),
        aws_verifier_image: Some(format!("verifier@sha256:{}", "ab".repeat(32))),
        gcp_verifier_image: Some(format!("gcr.example/verifier@sha256:{}", "6e".repeat(32))),
        azure_verifier_image: Some(format!(
            "registry.example/verifier@sha256:{}",
            "7f".repeat(32)
        )),
        operator_digest: Some(format!("sha256:{}", "ef".repeat(32))),
    }
}

/// One container, so preparation adds a compute cluster.
fn stack() -> Stack {
    serde_json::from_value(serde_json::json!({
        "id": "machines",
        "resources": {
            "web": {
                "config": {
                    "type": "container", "id": "web",
                    "code": { "type": "image", "image": "registry.example/web@sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef" },
                    "cpu": { "min": "0.25", "desired": "0.25" },
                    "memory": { "min": "256Mi", "desired": "256Mi" },
                    "replicas": 1, "permissions": "execution", "ports": [{ "port": 8080 }],
                    "links": [], "environment": {}
                },
                "lifecycle": "live", "dependencies": [], "remoteAccess": false
            }
        },
        "permissions": { "profiles": { "execution": {} }, "management": "auto" }
    }))
    .expect("fixture stack parses")
}

fn signing() -> StackSigning {
    StackSigning {
        private_machine_api_url: None,
        fingerprint: FINGERPRINT.to_string(),
        channel: "production".to_string(),
    }
}

async fn render(
    template: &str,
    subnet: Option<&'static str>,
    protected: bool,
) -> Result<RenderedSetupTemplate, String> {
    let signing = signing();
    let platform = match template {
        "terraform-gcp" => Platform::Gcp,
        _ => Platform::Azure,
    };
    let mut terraform = TfRegistry::built_in();
    terraform.register(ComputeCluster::RESOURCE_TYPE, platform, Cluster(subnet));
    let cloudformation = CfRegistry::built_in();
    let helm = HelmRegistry::built_in();
    let options = SetupRenderOptions::builder()
        .signing(&signing)
        .cloudformation_registry(&cloudformation)
        .terraform_registry(&terraform)
        .helm_registry(&helm)
        .hosting(
            serde_json::from_value::<SetupHostingOptions>(
                serde_json::json!({ "registration": { "mode": "outputsFallback" } }),
            )
            .expect("hosting options parse"),
        )
        .pins(pins())
        .build();
    let settings = StackSettings {
        signing: protected.then(|| signing.clone()),
        ..Default::default()
    };
    let rendered = render_setup_template(template, &stack(), &settings, &options)
        .await
        .map_err(|error| format!("{error:?}"))?;
    if let Some(dir) = std::env::var_os("SETUP_TEMPLATES_OUT") {
        let name = format!(
            "{template}-machines{}",
            if protected { "" } else { "-unprotected" }
        );
        std::fs::write(std::path::Path::new(&dir).join(name), &rendered.bytes)
            .expect("write rendered template");
    }
    Ok(rendered)
}

/// The verifier's environment in a rendered module: `(name, value)` of every `env` block of the
/// resource `kind.signing_verifier`, values as HCL text.
fn verifier_env(rendered: &RenderedSetupTemplate, kind: &str) -> Vec<(String, String)> {
    let files: std::collections::BTreeMap<String, String> =
        serde_json::from_slice(&rendered.bytes).expect("module is a file map");
    let mut found = None;
    for contents in files.values() {
        let Ok(body) = hcl::parse(contents) else {
            continue;
        };
        for block in body.blocks().filter(|block| {
            block.identifier() == "resource"
                && block.labels().first().map(|label| label.as_str()) == Some(kind)
                && block.labels().get(1).map(|label| label.as_str()) == Some("signing_verifier")
        }) {
            assert!(found.is_none(), "one verifier");
            found = Some(collect_env(block.body()));
        }
    }
    found.unwrap_or_else(|| panic!("module has {kind}.signing_verifier"))
}

fn collect_env(body: &hcl::Body) -> Vec<(String, String)> {
    let mut env = Vec::new();
    for block in body.blocks() {
        if block.identifier() == "env" {
            let value = |key: &str| {
                block
                    .body()
                    .attributes()
                    .find(|attribute| attribute.key() == key)
                    .map(|attribute| hcl::format::to_string(attribute.expr()).expect("format"))
                    .unwrap_or_else(|| panic!("env block has {key}"))
            };
            env.push((value("name").trim_matches('"').to_string(), value("value")));
        } else {
            env.extend(collect_env(block.body()));
        }
    }
    env
}

fn subnet(template: &str) -> &'static str {
    match template {
        "terraform-gcp" => "projects/${var.gcp_project}/regions/${var.gcp_region}/subnetworks/machines",
        _ => "/subscriptions/${var.azure_subscription_id}/resourceGroups/${var.azure_resource_group_name}/providers/Microsoft.Network/virtualNetworks/machines/subnets/machines",
    }
}

#[tokio::test]
async fn the_verifier_launches_machines_into_the_network_the_cluster_names() {
    for (template, kind) in [
        ("terraform-gcp", "google_cloud_run_v2_service"),
        ("terraform-azure", "azurerm_container_app"),
    ] {
        let rendered = render(template, Some(subnet(template)), true)
            .await
            .unwrap_or_else(|error| panic!("{template}: protected render: {error:?}"));
        let env = verifier_env(&rendered, kind);
        let network: Vec<_> = env
            .iter()
            .filter(|(name, _)| name == "VERIFIER_MACHINE_NETWORK")
            .map(|(_, value)| value.as_str())
            .collect();
        assert_eq!(network.len(), 1, "{template}: {env:?}");
        let expected = hcl::format::to_string(&expr::jsonencode(expr::object([
            (
                "subnetIds",
                Expression::Array(vec![expr::template(subnet(template))]),
            ),
            (
                "securityGroupIds",
                Expression::Array(vec![expr::template("${local.resource_prefix}-machines")]),
            ),
            (
                "identity",
                expr::template("${local.resource_prefix}-machines"),
            ),
        ])))
        .expect("format");
        assert_eq!(
            network[0].split_whitespace().collect::<String>(),
            expected.split_whitespace().collect::<String>(),
            "{template}"
        );
    }
}

/// Machines the verifier has nowhere to launch can't be protected; the render says which
/// cluster.
#[tokio::test]
async fn a_protected_cluster_without_a_machine_network_is_refused() {
    for template in ["terraform-gcp", "terraform-azure"] {
        let error = render(template, None, true)
            .await
            .expect_err("no machine network, no protected render");
        assert!(
            error.contains("names no network for the verifier to launch machines into"),
            "{template}: {error}"
        );
    }
}

/// Unprotected installs have no verifier: the hook isn't asked, so what it names changes
/// nothing (D25).
#[tokio::test]
async fn unprotected_renders_ignore_the_machine_network() {
    for template in ["terraform-gcp", "terraform-azure"] {
        let named = render(template, Some(subnet(template)), false)
            .await
            .expect("unprotected render");
        let none = render(template, None, false)
            .await
            .expect("unprotected render without a network");
        assert_eq!(named.bytes, none.bytes, "{template}");
    }
}

/// Every module file, parsed.
fn module_bodies(rendered: &RenderedSetupTemplate) -> Vec<hcl::Body> {
    let files: std::collections::BTreeMap<String, String> =
        serde_json::from_slice(&rendered.bytes).expect("module is a file map");
    files
        .iter()
        .filter(|(path, _)| path.ends_with(".tf"))
        .map(|(path, contents)| {
            hcl::parse(contents).unwrap_or_else(|error| panic!("{path} parses: {error}"))
        })
        .collect()
}

/// Whether the Azure action pattern `pattern` (with `*` wildcards) grants `action`.
fn azure_grants(pattern: &str, action: &str) -> bool {
    let (pattern, action) = (pattern.to_ascii_lowercase(), action.to_ascii_lowercase());
    let mut parts = pattern.split('*');
    let first = parts.next().unwrap_or_default();
    let Some(mut rest) = action.strip_prefix(first) else {
        return false;
    };
    let parts: Vec<&str> = parts.collect();
    if parts.is_empty() {
        return rest.is_empty();
    }
    for (index, part) in parts.iter().enumerate() {
        if index == parts.len() - 1 {
            return rest.ends_with(part);
        }
        match rest.find(part) {
            Some(at) => rest = &rest[at + part.len()..],
            None => return false,
        }
    }
    true
}

/// The actions of every custom role in the module except the verifier's and the machines' own,
/// and the built-in role ids assigned to the management identity.
fn management_azure_actions(rendered: &RenderedSetupTemplate) -> (Vec<String>, Vec<String>) {
    let mut actions = Vec::new();
    let mut built_in = Vec::new();
    for body in module_bodies(rendered) {
        for block in body
            .blocks()
            .filter(|block| block.identifier() == "resource")
        {
            let labels: Vec<&str> = block.labels().iter().map(|label| label.as_str()).collect();
            match labels.as_slice() {
                ["azurerm_role_definition", label]
                    if !label.ends_with("_verifier")
                        && !label.ends_with("_verifier_images")
                        && !label.ends_with("_machine_signing") =>
                {
                    for permissions in block
                        .body()
                        .blocks()
                        .filter(|nested| nested.identifier() == "permissions")
                    {
                        for attribute in permissions
                            .body()
                            .attributes()
                            .filter(|attribute| attribute.key() == "actions")
                        {
                            let Expression::Array(items) = attribute.expr() else {
                                panic!("{label}: actions is a list");
                            };
                            actions.extend(items.iter().map(|item| match item {
                                Expression::String(action) => action.clone(),
                                other => panic!("{label}: action {other:?} is a literal"),
                            }));
                        }
                    }
                }
                ["azurerm_role_assignment", label] => {
                    let attribute = |key: &str| {
                        block
                            .body()
                            .attributes()
                            .find(|attribute| attribute.key() == key)
                            .map(|attribute| {
                                hcl::format::to_string(attribute.expr()).expect("format")
                            })
                    };
                    if attribute("principal_id").as_deref()
                        != Some("azurerm_user_assigned_identity.management.principal_id")
                    {
                        continue;
                    }
                    if let Some(name) = attribute("role_definition_name") {
                        built_in.push(name);
                    } else if let Some(id) = attribute("role_definition_id").filter(|id| {
                        id.contains("/providers/Microsoft.Authorization/roleDefinitions/")
                    }) {
                        built_in.push(
                            id.trim_matches('"')
                                .rsplit('/')
                                .next()
                                .unwrap_or_default()
                                .to_string(),
                        );
                    } else if attribute("role_definition_id").is_none() {
                        panic!("{label}: a management assignment names its role");
                    }
                }
                _ => {}
            }
        }
    }
    (actions, built_in)
}

/// What decides how a scale set's machines boot or who they run as: its model and tags (the
/// machine identity's client id tag is the verifier's), its instances, and the group's tags.
const MACHINE_WRITES: &[&str] = &[
    "Microsoft.Compute/virtualMachineScaleSets/write",
    "Microsoft.Compute/virtualMachineScaleSets/delete/action",
    "Microsoft.Compute/virtualMachineScaleSets/virtualMachines/write",
    "Microsoft.Compute/virtualMachineScaleSets/extensions/write",
    "Microsoft.Resources/tags/write",
    "Microsoft.Resources/subscriptions/resourceGroups/write",
    "Microsoft.ManagedIdentity/userAssignedIdentities/assign/action",
];

/// Protected Azure management holds no scale set, tag or identity-assignment write in any role
/// setup gives it; unprotected management keeps the scale set writes it has today.
#[tokio::test]
async fn protected_azure_management_cannot_change_machines_or_tags() {
    let template = "terraform-azure";
    let held = |actions: &[String]| -> Vec<String> {
        MACHINE_WRITES
            .iter()
            .filter(|target| actions.iter().any(|pattern| azure_grants(pattern, target)))
            .map(|target| target.to_string())
            .collect()
    };

    let protected = render(template, Some(subnet(template)), true)
        .await
        .expect("protected render");
    let (actions, built_in) = management_azure_actions(&protected);
    assert!(!actions.is_empty(), "management has custom roles");
    assert_eq!(
        held(&actions),
        Vec::<String>::new(),
        "protected management roles: {actions:?}"
    );
    // Reader, by name and by id.
    for role in &built_in {
        assert!(
            matches!(
                role.trim_matches('"'),
                "Reader" | "acdd72a7-3385-48ef-bd42-f606fba81ae7"
            ),
            "protected management is assigned the built-in role {role}"
        );
    }

    let unprotected = render(template, Some(subnet(template)), false)
        .await
        .expect("unprotected render");
    let (actions, _) = management_azure_actions(&unprotected);
    assert!(
        held(&actions).contains(&"Microsoft.Compute/virtualMachineScaleSets/write".to_string()),
        "unprotected management keeps its scale set writes: {actions:?}"
    );
}

#[test]
fn azure_wildcards_grant_what_they_cover() {
    assert!(azure_grants("*", "Microsoft.Resources/tags/write"));
    assert!(azure_grants("*/write", "Microsoft.Resources/tags/write"));
    assert!(azure_grants(
        "Microsoft.Compute/*",
        "Microsoft.Compute/virtualMachineScaleSets/write"
    ));
    assert!(azure_grants(
        "microsoft.compute/virtualmachinescalesets/*/write",
        "Microsoft.Compute/virtualMachineScaleSets/virtualMachines/write"
    ));
    assert!(!azure_grants(
        "Microsoft.Compute/*/read",
        "Microsoft.Compute/virtualMachineScaleSets/write"
    ));
    assert!(!azure_grants(
        "Microsoft.Resources/tags/read",
        "Microsoft.Resources/tags/write"
    ));
}
