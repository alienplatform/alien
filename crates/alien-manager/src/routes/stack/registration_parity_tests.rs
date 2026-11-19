//! The manager expects a setup registration to hold exactly what the setup module registers.
//!
//! These tests generate the real Terraform module and CloudFormation template for a stack, read
//! the resource ids the artifact itself would send, and hand them to the manager's registration
//! check. The expectation is never built from the rule the generators and the manager share
//! (`alien_core::emits_package_scaffolding`): reading it from there would pass whatever that rule
//! says. A generator that renders something the manager does not expect (or the reverse) fails
//! here, whichever side drifted.

use super::*;
use alien_core::{
    CapacityGroup, ComputeCluster, Container, ContainerCode, KubernetesClusterOwnership,
    KubernetesClusterSettings, KubernetesSettings, PermissionProfile, ResourceSpec, Storage, Vault,
};
use alien_terraform::{TerraformOptions, TerraformTarget, TfRegistry};
use std::collections::BTreeSet;

fn test_request() -> StackImportRequest {
    StackImportRequest {
        setup_import_format_version: alien_core::import::CURRENT_SETUP_IMPORT_FORMAT_VERSION,
        deployment_group_token: String::new(),
        deployment_name: "deployment".to_string(),
        resource_prefix: "deployment".to_string(),
        source_kind: None,
        setup_metadata: None,
        release_id: Some("release".to_string()),
        platform: Platform::Aws,
        base_platform: None,
        region: "region".to_string(),
        setup_target: "target".to_string(),
        setup_fingerprint: "fingerprint".to_string(),
        setup_fingerprint_version: 1,
        stack_settings: Default::default(),
        management_config: None,
        input_values: HashMap::new(),
        accepts_short_lived_deployment_token: false,
        resources: vec![],
    }
}

/// A managed-Kubernetes setup format and the cluster it installs onto.
#[derive(Debug, Clone, Copy)]
enum Format {
    Terraform(TerraformTarget),
    CloudFormation,
}

/// A stack as a real app declares it: a public container, a bucket, a vault and an explicit
/// compute cluster. (A database is not here: its Terraform emitters are registered by embedders,
/// so the built-in registry binds one to the installer and registers nothing for it.)
/// An explicit compute cluster
fn source_stack() -> Stack {
    let container = Container::new("web".to_string())
        .code(ContainerCode::Image {
            image: "example.com/web:latest".to_string(),
        })
        .cpu(ResourceSpec {
            min: "0.5".to_string(),
            desired: "0.5".to_string(),
        })
        .memory(ResourceSpec {
            min: "512Mi".to_string(),
            desired: "512Mi".to_string(),
        })
        .port(8080)
        .public_endpoint(alien_core::PublicEndpoint {
            name: "web".to_string(),
            port: 8080,
            protocol: alien_core::ExposeProtocol::Http,
            host_label: None,
            wildcard_subdomains: false,
        })
        .pool("apps".to_string())
        .permissions("app".to_string())
        .build();
    let builder = Stack::new("parity".to_string()).permission("app", PermissionProfile::new());
    builder
        .add(
            ComputeCluster::new("compute".to_string())
                .capacity_group(CapacityGroup {
                    group_id: "apps".to_string(),
                    instance_type: None,
                    profile: None,
                    min_size: 1,
                    max_size: 3,
                    scale_policy: None,
                    nested_virtualization: None,
                    data_disk_gib_range: None,
                })
                .build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Storage::new("assets".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Vault::new("app-secrets".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(container, ResourceLifecycle::Live)
        .build()
}

fn settings(ownership: KubernetesClusterOwnership) -> StackSettings {
    StackSettings {
        deployment_model: alien_core::DeploymentModel::Pull,
        setup_management: alien_core::SetupManagement::Manual,
        kubernetes: Some(KubernetesSettings {
            cluster: Some(KubernetesClusterSettings {
                ownership,
                namespace: None,
                cloud: None,
            }),
            exposure: None,
        }),
        ..Default::default()
    }
}

fn cloud(format: Format) -> Platform {
    match format {
        Format::Terraform(target) => target.cloud_platform(),
        Format::CloudFormation => Platform::Aws,
    }
}

/// The stack as the CLI prepares it before rendering a setup artifact: template preflights, then
/// mutations for the deployment's settings.
async fn prepared_for_render(source: &Stack, format: Format, settings: &StackSettings) -> Stack {
    try_prepared_for_render(source, format, settings)
        .await
        .expect("template mutations apply")
}

/// [`prepared_for_render`], returning the refusal the preflights give instead of panicking.
async fn try_prepared_for_render(
    source: &Stack,
    format: Format,
    settings: &StackSettings,
) -> Result<Stack, String> {
    let mut stack = source.clone();
    alien_core::remove_off_platform_gated_resources(&mut stack, Platform::Kubernetes, None);
    let runner = alien_preflights::runner::PreflightRunner::new();
    runner
        .run_template_preflights(&stack, Platform::Kubernetes)
        .await
        .expect("template preflights pass");
    let config = DeploymentConfig::builder()
        .deployment_name(stack.id().to_string())
        .stack_settings(settings.clone())
        .management_config(alien_core::ManagementConfig::Aws(
            alien_core::AwsManagementConfig {
                managing_role_arn: "rendered-from-template-parameter".to_string(),
            },
        ))
        .environment_variables(EnvironmentVariablesSnapshot {
            variables: Vec::new(),
            hash: "empty".to_string(),
            created_at: "1970-01-01T00:00:00Z".to_string(),
        })
        .allow_frozen_changes(false)
        .external_bindings(ExternalBindings::default())
        .base_platform(cloud(format))
        .build();
    runner
        .apply_mutations(stack, &StackState::new(Platform::Kubernetes), &config)
        .await
        .map_err(|error| format!("{error:?}"))
}

/// Every `{ id, type }` pair in a rendered registration, found by shape wherever the artifact
/// nests it.
fn collect_json_entries(value: &serde_json::Value, found: &mut Vec<(String, String)>) {
    match value {
        serde_json::Value::Object(map) => {
            if let (Some(id), Some(kind), true) = (
                map.get("id").and_then(|v| v.as_str()),
                map.get("type").and_then(|v| v.as_str()),
                map.contains_key("importData"),
            ) {
                found.push((id.to_string(), kind.to_string()));
            }
            map.values().for_each(|v| collect_json_entries(v, found));
        }
        serde_json::Value::Array(items) => {
            items.iter().for_each(|v| collect_json_entries(v, found))
        }
        _ => {}
    }
}

/// What the module sends once a deployer has applied it: the registered resources, and whether
/// the registered settings carry a `network` setting.
struct TerraformRegistration {
    resources: Vec<(String, String)>,
    settings_network: bool,
}

/// The registration a Terraform module sends, with every condition in it evaluated for the
/// cluster mode the deployer applies the module in. Evaluating (not collecting both branches of
/// each conditional) is what lets this catch a gate that registers the wrong thing in one mode.
fn terraform_registration(
    prepared: &Stack,
    target: TerraformTarget,
    settings: &StackSettings,
    mode: KubernetesClusterOwnership,
    inputs: &[(&str, bool)],
) -> TerraformRegistration {
    let mut registry = TfRegistry::built_in();
    registry.register(
        NETWORK_EXTENSION_TYPE,
        target.cloud_platform(),
        EmbedderDatabase,
    );
    let module = alien_terraform::generate_terraform_module(
        prepared,
        target,
        TerraformOptions {
            registry: &registry,
            display_name: None,
            stack_settings: settings.clone(),
            registration: None,
            setup_identity: None,
            helm_install: None,
            supported_aws_regions: Vec::new(),
            pins: Default::default(),
        },
    )
    .expect("the Terraform module renders");
    let locals = module
        .iter()
        .find(|(name, _)| *name == "locals.tf")
        .map(|(_, body)| body.to_string())
        .expect("locals.tf");
    let body: hcl::Body = hcl::parse(&locals).expect("locals.tf parses");
    let local = |name: &str| {
        body.blocks()
            .flat_map(|block| block.body().attributes())
            .find(|attribute| attribute.key() == name)
            .unwrap_or_else(|| panic!("the module has no `local.{name}`"))
    };
    let env = Env { mode, inputs };
    let mut resources = Vec::new();
    collect_hcl_entries(local("deployment_resources").expr(), &env, &mut resources);
    let mut settings_network = false;
    collect_settings_network(
        local("deployment_settings").expr(),
        &env,
        &mut settings_network,
    );
    TerraformRegistration {
        resources,
        settings_network,
    }
}

/// The values a deployer's inputs take for one cell. Anything a condition reads beyond these is
/// a new gate this test does not understand yet, which must fail loudly rather than be guessed.
struct Env<'a> {
    mode: KubernetesClusterOwnership,
    /// The deployer's answers to the stack's boolean inputs, by input id.
    inputs: &'a [(&'a str, bool)],
}

/// The Terraform variable a stack input becomes: `zoneEnabled` is `input_zone_enabled`.
fn input_variable(input_id: &str) -> String {
    let mut name = "input_".to_string();
    for character in input_id.chars() {
        if character.is_ascii_uppercase() {
            name.push('_');
        }
        name.push(character.to_ascii_lowercase());
    }
    name
}

#[derive(Debug, Clone, PartialEq)]
enum Value {
    Bool(bool),
    Text(String),
}

fn evaluate(expression: &hcl::Expression, env: &Env) -> Value {
    use hcl::expr::{BinaryOperator, Operation, UnaryOperator};
    match expression {
        hcl::Expression::Bool(value) => Value::Bool(*value),
        hcl::Expression::Parenthesis(inner) => evaluate(inner, env),
        hcl::Expression::String(_) | hcl::Expression::TemplateExpr(_) => {
            Value::Text(hcl_string(expression).expect("a quoted string"))
        }
        hcl::Expression::Traversal(traversal) => {
            let root = match &traversal.expr {
                hcl::Expression::Variable(variable) => variable.to_string(),
                other => panic!("unsupported condition operand {other:?}"),
            };
            let path: Vec<String> = traversal
                .operators
                .iter()
                .map(|operator| match operator {
                    hcl::expr::TraversalOperator::GetAttr(name) => name.to_string(),
                    other => panic!("unsupported traversal {other:?}"),
                })
                .collect();
            match (root.as_str(), path.as_slice()) {
                ("var", [name]) if name == "kubernetes_cluster_mode" => {
                    Value::Text(match env.mode {
                        KubernetesClusterOwnership::Existing => "existing".to_string(),
                        _ => "create".to_string(),
                    })
                }
                // The module's own default; a deployer who picks another network mode is a
                // different cell (the cluster mode decides whether the module honours it).
                ("var", [name]) if name == "network_mode" => Value::Text("create-new".to_string()),
                // The deployer asked for no network of their own in advanced settings.
                ("local", [name]) if name == "deployment_requests_network" => Value::Bool(false),
                ("var", [name]) if env.inputs.iter().any(|(id, _)| input_variable(id) == *name) => {
                    Value::Bool(
                        env.inputs
                            .iter()
                            .find(|(id, _)| input_variable(id) == *name)
                            .map(|(_, answer)| *answer)
                            .unwrap(),
                    )
                }
                _ => panic!(
                    "the module gates on `{root}.{}`, which this parity test does not evaluate; teach it how that input is applied",
                    path.join(".")
                ),
            }
        }
        hcl::Expression::Operation(operation) => match operation.as_ref() {
            Operation::Unary(unary) => match (unary.operator, evaluate(&unary.expr, env)) {
                (UnaryOperator::Not, Value::Bool(value)) => Value::Bool(!value),
                other => panic!("unsupported unary condition {other:?}"),
            },
            Operation::Binary(binary) => {
                let (lhs, rhs) = (
                    evaluate(&binary.lhs_expr, env),
                    evaluate(&binary.rhs_expr, env),
                );
                match (binary.operator, &lhs, &rhs) {
                    (BinaryOperator::Eq, _, _) => Value::Bool(lhs == rhs),
                    (BinaryOperator::NotEq, _, _) => Value::Bool(lhs != rhs),
                    (BinaryOperator::And, Value::Bool(a), Value::Bool(b)) => Value::Bool(*a && *b),
                    (BinaryOperator::Or, Value::Bool(a), Value::Bool(b)) => Value::Bool(*a || *b),
                    other => panic!("unsupported binary condition {other:?}"),
                }
            }
        },
        other => panic!("unsupported condition {other:?}"),
    }
}

fn hcl_string(expression: &hcl::Expression) -> Option<String> {
    match expression {
        hcl::Expression::String(text) => Some(text.clone()),
        hcl::Expression::TemplateExpr(template) => match template.as_ref() {
            hcl::TemplateExpr::QuotedString(text) => Some(text.clone()),
            hcl::TemplateExpr::Heredoc(_) => None,
        },
        _ => None,
    }
}

fn taken_branch<'a>(conditional: &'a hcl::expr::Conditional, env: &Env) -> &'a hcl::Expression {
    match evaluate(&conditional.cond_expr, env) {
        Value::Bool(true) => &conditional.true_expr,
        Value::Bool(false) => &conditional.false_expr,
        other => panic!("a condition evaluated to {other:?}, not a boolean"),
    }
}

/// Every `{ id, type, importData }` object in an HCL expression, descending only into the branch
/// of each conditional that the cell's inputs select.
fn collect_hcl_entries(expression: &hcl::Expression, env: &Env, found: &mut Vec<(String, String)>) {
    match expression {
        hcl::Expression::Object(map) => {
            let field = |name: &str| {
                map.iter()
                    .find(|(key, _)| key.to_string() == name)
                    .map(|(_, value)| value)
            };
            if let (Some(id), Some(kind), Some(_)) = (
                field("id").and_then(hcl_string),
                field("type").and_then(hcl_string),
                field("importData"),
            ) {
                // An entry's `importData` is its own business; only the list builder's
                // conditionals decide which entries are sent.
                found.push((id, kind));
                return;
            }
            map.values()
                .for_each(|v| collect_hcl_entries(v, env, found));
        }
        hcl::Expression::Array(items) => items
            .iter()
            .for_each(|v| collect_hcl_entries(v, env, found)),
        hcl::Expression::Parenthesis(inner) => collect_hcl_entries(inner, env, found),
        hcl::Expression::Conditional(conditional) => {
            collect_hcl_entries(taken_branch(conditional, env), env, found)
        }
        hcl::Expression::FuncCall(call) => call
            .args
            .iter()
            .for_each(|v| collect_hcl_entries(v, env, found)),
        _ => {}
    }
}

/// Whether the settings a module registers carry a `network` key, for the cell's inputs. The
/// conditionals that choose between settings are evaluated; what goes inside a `jsonencode` (the
/// network's own fields) is not read, only whether it names `network`.
fn collect_settings_network(expression: &hcl::Expression, env: &Env, found: &mut bool) {
    match expression {
        hcl::Expression::Object(map) => {
            if map.keys().any(|key| key.to_string() == "network") {
                *found = true;
            }
        }
        hcl::Expression::Parenthesis(inner) => collect_settings_network(inner, env, found),
        hcl::Expression::Conditional(conditional) => {
            collect_settings_network(taken_branch(conditional, env), env, found)
        }
        hcl::Expression::FuncCall(call) => call
            .args
            .iter()
            .for_each(|v| collect_settings_network(v, env, found)),
        _ => {}
    }
}

/// The registration a CloudFormation template sends: the `Resources` its registration custom
/// resource carries.
fn cloudformation_registration(
    prepared: &Stack,
    source: &Stack,
    settings: &StackSettings,
) -> Vec<(String, String)> {
    let mut registry = alien_cloudformation::CfRegistry::built_in();
    registry.register(NETWORK_EXTENSION_TYPE, Platform::Aws, EmbedderDatabase);
    let template = alien_cloudformation::generate_cloudformation_template(
        prepared,
        alien_cloudformation::CloudFormationOptions {
            registry: &registry,
            target: alien_cloudformation::CloudFormationTarget::Eks,
            source_stack: source,
            stack_settings: settings.clone(),
            setup_target: "kubernetes".to_string(),
            setup_fingerprint: "test".to_string(),
            setup_fingerprint_version: 1,
            registration: alien_cloudformation::RegistrationMode::CustomResource {
                lambda_arn: "arn:aws:lambda:us-east-1:111122223333:function:register".to_string(),
                callback_url: None,
            },
            description: None,
            management_trust: Default::default(),
            pins: Default::default(),
        },
    )
    .expect("the CloudFormation template renders");
    let json = serde_json::to_value(&template).expect("template serializes");
    let mut found = Vec::new();
    collect_json_entries(&json, &mut found);
    found
}

/// The request a setup sends once it has applied: one resource per registered id.
fn registration_request(
    registered: &[(String, String)],
    format: Format,
    settings: &StackSettings,
) -> StackImportRequest {
    let mut req = test_request();
    req.platform = Platform::Kubernetes;
    req.base_platform = Some(cloud(format));
    req.stack_settings = settings.clone();
    req.resources = registered
        .iter()
        .map(|(id, kind)| ImportedResource {
            id: id.clone(),
            resource_type: alien_core::ResourceType::from(kind.clone()),
            import_data: serde_json::json!({}),
        })
        .collect();
    req
}

/// Runs one cell of the matrix: the artifact's registration, through the manager's check.
///
/// The request carries the settings the module registers for the cell's cluster mode (a `network`
/// setting only while the module sends one), because the manager prepares the expected stack from
/// them.
async fn register(
    format: Format,
    ownership: KubernetesClusterOwnership,
) -> (BTreeSet<String>, crate::error::Result<()>) {
    register_source(source_stack(), format, ownership, &[]).await
}

/// [`register`] for a given source stack, with the deployer's answers to its boolean inputs.
/// Before preparing the expected stack the manager drops what a declined frozen gate removes,
/// exactly as the registration route does.
async fn register_source(
    source: Stack,
    format: Format,
    ownership: KubernetesClusterOwnership,
    inputs: &[(&str, bool)],
) -> (BTreeSet<String>, crate::error::Result<()>) {
    let settings = settings(ownership);
    let prepared = prepared_for_render(&source, format, &settings).await;
    let (registered, registered_network) = match format {
        Format::Terraform(target) => {
            let registration =
                terraform_registration(&prepared, target, &settings, ownership, inputs);
            (registration.resources, Some(registration.settings_network))
        }
        Format::CloudFormation => (
            cloudformation_registration(&prepared, &source, &settings),
            None,
        ),
    };
    let ids: BTreeSet<String> = registered.iter().map(|(id, _)| id.clone()).collect();
    assert_eq!(
        ids.len(),
        registered.len(),
        "{format:?} {ownership:?}: duplicate registration ids {registered:?}"
    );
    let mut sent_settings = settings.clone();
    if let Some(sent) = registered_network {
        sent_settings.network = sent.then(|| alien_core::NetworkSettings::Create {
            cidr: None,
            availability_zones: 2,
        });
    }
    let mut req = registration_request(&registered, format, &sent_settings);
    req.input_values = inputs
        .iter()
        .map(|(id, answer)| (id.to_string(), serde_json::Value::Bool(*answer)))
        .collect();
    let delivered: std::collections::HashSet<String> = ids.iter().cloned().collect();
    let answers = alien_deployment::resolve_frozen_gate_answers_from_presence(
        &source,
        &delivered,
        &req.input_values,
        req.platform,
    )
    .expect("the registration answers every frozen gate");
    let frozen_gating = alien_deployment::frozen_gating_inputs(&source, req.platform);
    let source = alien_deployment::strip_frozen_declines_with_compute_settings(
        source,
        &answers,
        &frozen_gating,
        None,
        req.platform,
    );
    let expected_stack = prepare_import_stack(source, &req, &req.stack_settings)
        .await
        .expect("the manager prepares the stack");
    (ids, validate_import_handoff(&req, &expected_stack))
}

const MATRIX: [Format; 4] = [
    Format::Terraform(TerraformTarget::Eks),
    Format::Terraform(TerraformTarget::Gke),
    Format::Terraform(TerraformTarget::Aks),
    Format::CloudFormation,
];

#[tokio::test]
async fn every_kubernetes_setup_registers_what_the_manager_expects() {
    let mut failures = Vec::new();
    for format in MATRIX {
        for ownership in [
            KubernetesClusterOwnership::Existing,
            KubernetesClusterOwnership::Managed,
        ] {
            let (ids, result) = register(format, ownership).await;
            // The module must register the setup-owned data layer, so an empty or shrunken
            // registration cannot pass for want of anything to compare.
            for expected in ["assets", "app-secrets"] {
                assert!(
                    ids.contains(expected),
                    "{format:?} {ownership:?} registers {ids:?}, which lacks '{expected}'"
                );
            }
            assert!(
                !ids.contains("compute"),
                "{format:?} {ownership:?}: a Kubernetes cluster's nodes are the compute, so setup registers none"
            );
            if let Err(error) = result {
                failures.push(format!("{format:?} {ownership:?}: {}", error.message));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// An extension type that requires the network. Its setup comes from the extension's template
/// modifier, which these renders stand in for with the embedder's emitter.
const NETWORK_EXTENSION_TYPE: &str = "acme/dns-zone";

/// How a stack holds a zone whose extension type requires the network.
#[derive(Debug, Clone, Copy)]
enum NetworkExtension {
    /// Frozen, with no gate.
    Frozen,
    /// Frozen, behind an input of its own.
    FrozenOwnInput,
    /// Live, behind an input that gates nothing frozen.
    LiveOwnInput,
    /// Live, behind an input that also gates a frozen bucket.
    LiveSharedInput,
}

/// [`source_stack`] plus a zone whose extension type requires the network.
fn with_network_extension(case: NetworkExtension) -> Stack {
    let mut source = source_stack();
    let zone = alien_core::CustomResource::from_untagged(
        NETWORK_EXTENSION_TYPE,
        serde_json::json!({ "id": "zone", "domain": "internal.acme.dev" }),
    )
    .unwrap();
    let gated =
        |config: alien_core::Resource, lifecycle, gate: Option<&str>| alien_core::ResourceEntry {
            config,
            lifecycle,
            dependencies: Vec::new(),
            display_name: None,
            description: None,
            remote_access: false,
            enabled_when: gate.map(str::to_string),
        };
    let entry = match case {
        NetworkExtension::Frozen => gated(
            alien_core::Resource::new(zone),
            ResourceLifecycle::Frozen,
            None,
        ),
        NetworkExtension::FrozenOwnInput => gated(
            alien_core::Resource::new(zone),
            ResourceLifecycle::Frozen,
            Some("zoneEnabled"),
        ),
        NetworkExtension::LiveOwnInput | NetworkExtension::LiveSharedInput => gated(
            alien_core::Resource::new(zone),
            ResourceLifecycle::Live,
            Some("zoneEnabled"),
        ),
    };
    if !matches!(case, NetworkExtension::Frozen) {
        source
            .inputs
            .push(alien_core::StackInputDefinition::deployer_boolean(
                "zoneEnabled",
                "Enable the zone",
                "Whether to create the zone.",
                Some(false),
            ));
    }
    if matches!(case, NetworkExtension::LiveSharedInput) {
        source.resources.insert(
            "zone-logs".to_string(),
            gated(
                alien_core::Resource::new(Storage::new("zone-logs".to_string()).build()),
                ResourceLifecycle::Frozen,
                Some("zoneEnabled"),
            ),
        );
    }
    source.resources.insert("zone".to_string(), entry);
    source.extensions = vec![serde_json::from_value(serde_json::json!({
        "name": "acme",
        "version": "0.1.0",
        "digest": "sha256:4ff931a2",
        "resourceTypes": { "acme/dns-zone": {
            "lifecycles": ["frozen", "live"],
            "holdsData": false,
            "requiresNetwork": true,
            "setup": {
                "cloudformation": ["aws"],
                "terraform": ["aws", "gcp", "azure"],
                "helm": ["kubernetes"],
                "importers": ["aws", "gcp", "azure"]
            },
            "controllerPlatforms": ["aws", "gcp", "azure"],
            "permissionSets": [],
            "link": { "permissionSets": [] },
            "schemas": {
                "config": { "type": "object", "required": ["id", "domain"] },
                "outputs": { "type": "object" }
            }
        } }
    }))
    .unwrap()];
    source
}

/// The manager keeps a network for an extension type that requires it in every cluster mode, as
/// it does for a cloud database, so every setup registers that network too.
#[tokio::test]
async fn a_type_that_requires_the_network_registers_it_in_every_cluster_mode() {
    let mut failures = Vec::new();
    for format in MATRIX {
        for ownership in [
            KubernetesClusterOwnership::Existing,
            KubernetesClusterOwnership::Managed,
        ] {
            let (ids, result) = register_source(
                with_network_extension(NetworkExtension::Frozen),
                format,
                ownership,
                &[],
            )
            .await;
            assert!(
                ids.contains("default-network") && ids.contains("zone"),
                "{format:?} {ownership:?} registers {ids:?}"
            );
            if let Err(error) = result {
                failures.push(format!("{format:?} {ownership:?}: {}", error.message));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A declined live resource that requires the network keeps it while its input gates nothing
/// frozen, because the manager drops such a resource only after it prepares the stack. When the
/// input also gates a frozen resource, the manager drops both before, so an existing cluster then
/// gets no network. (The Terraform module evaluates these gates per cluster mode; the
/// CloudFormation template's own conditions are outside what this test evaluates.)
#[tokio::test]
async fn a_declined_live_type_keeps_the_network_unless_its_input_gates_a_frozen_resource() {
    let mut failures = Vec::new();
    for target in [
        TerraformTarget::Eks,
        TerraformTarget::Gke,
        TerraformTarget::Aks,
    ] {
        for ownership in [
            KubernetesClusterOwnership::Existing,
            KubernetesClusterOwnership::Managed,
        ] {
            for (case, answer, network) in [
                (NetworkExtension::LiveOwnInput, false, true),
                (NetworkExtension::LiveOwnInput, true, true),
                (
                    NetworkExtension::LiveSharedInput,
                    false,
                    ownership == KubernetesClusterOwnership::Managed,
                ),
                (NetworkExtension::LiveSharedInput, true, true),
            ] {
                let (ids, result) = register_source(
                    with_network_extension(case),
                    Format::Terraform(target),
                    ownership,
                    &[("zoneEnabled", answer)],
                )
                .await;
                let cell = format!("{target:?} {ownership:?} {case:?} zoneEnabled={answer}");
                assert_eq!(
                    ids.contains("default-network"),
                    network,
                    "{cell} registers {ids:?}"
                );
                assert!(
                    !ids.contains("zone"),
                    "{cell}: a live zone is the runtime's"
                );
                if let Err(error) = result {
                    failures.push(format!("{cell}: {}", error.message));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// CloudFormation fixes the network when the template is rendered, so on an existing cluster it
/// cannot follow an input that drops a network-requiring type before the manager prepares the
/// stack. It refuses that shape, as it refuses a database the cluster could not reach, and keeps
/// rendering the shapes whose network stays whatever the deployer answers.
#[tokio::test]
async fn cloudformation_refuses_a_declinable_network_type_on_an_existing_cluster() {
    use KubernetesClusterOwnership::{Existing, Managed};
    use NetworkExtension::{FrozenOwnInput, LiveOwnInput, LiveSharedInput};
    // (case, cluster, a second zone with no gate, refused)
    let cells = [
        (LiveSharedInput, Existing, false, true),
        (FrozenOwnInput, Existing, false, true),
        (LiveSharedInput, Managed, false, false),
        (LiveOwnInput, Existing, false, false),
        (LiveSharedInput, Existing, true, false),
    ];
    for (case, ownership, second_zone, refused) in cells {
        let cell = format!("{case:?} {ownership:?} second_zone={second_zone}");
        let mut source = with_network_extension(case);
        if second_zone {
            let mut entry =
                with_network_extension(NetworkExtension::Frozen).resources["zone"].clone();
            entry.config = alien_core::Resource::new(
                alien_core::CustomResource::from_untagged(
                    NETWORK_EXTENSION_TYPE,
                    serde_json::json!({ "id": "zone-2", "domain": "other.acme.dev" }),
                )
                .unwrap(),
            );
            source.resources.insert("zone-2".to_string(), entry);
        }
        let settings = settings(ownership);
        let prepared = prepared_for_render(&source, Format::CloudFormation, &settings).await;
        let result =
            render_with_database_emitters(&prepared, &source, Format::CloudFormation, &settings);
        if refused {
            let error = result.expect_err(&format!("{cell} is refused"));
            assert_eq!(error.code, "SETUP_PLACEMENT_UNSUPPORTED", "{cell}");
            assert!(
                error.message.contains("'zone'")
                    && error
                        .message
                        .contains("'zoneEnabled' also turns off a resource setup creates")
                    && error.message.contains("Use a cluster that setup creates"),
                "{cell}: {}",
                error.message
            );
        } else {
            result.unwrap_or_else(|error| panic!("{cell}: {error:?}"));
        }
    }
}

/// What an embedder registers for a database or cache: a resource setup creates.
struct EmbedderDatabase;

impl alien_terraform::TfEmitter for EmbedderDatabase {
    fn emit(
        &self,
        _ctx: &alien_core::import::EmitContext<'_>,
    ) -> alien_core::Result<alien_terraform::TfFragment> {
        Ok(alien_terraform::TfFragment::default())
    }

    fn emit_import_ref(
        &self,
        _ctx: &alien_core::import::EmitContext<'_>,
    ) -> alien_core::Result<hcl::expr::Expression> {
        Ok(hcl::expr::Expression::Object(Default::default()))
    }
}

impl alien_cloudformation::CfEmitter for EmbedderDatabase {
    fn emit_resources(
        &self,
        _ctx: &alien_core::import::EmitContext<'_>,
    ) -> alien_core::Result<Vec<alien_cloudformation::CfResource>> {
        Ok(Vec::new())
    }

    fn emit_import_ref(
        &self,
        _ctx: &alien_core::import::EmitContext<'_>,
    ) -> alien_core::Result<alien_cloudformation::CfExpression> {
        Ok(alien_cloudformation::CfExpression::object([(
            "id",
            alien_cloudformation::CfExpression::from("x"),
        )]))
    }
}

/// Render the setup of `prepared` with the embedder's database emitters registered, returning the
/// generator's refusal if it gives one.
fn render_with_database_emitters(
    prepared: &Stack,
    source: &Stack,
    format: Format,
    settings: &StackSettings,
) -> alien_core::Result<()> {
    match format {
        Format::Terraform(target) => {
            let mut registry = TfRegistry::built_in();
            for resource_type in [
                alien_core::Postgres::RESOURCE_TYPE,
                alien_core::Redis::RESOURCE_TYPE,
            ] {
                registry.register(resource_type, target.cloud_platform(), EmbedderDatabase);
            }
            alien_terraform::generate_terraform_module(
                prepared,
                target,
                TerraformOptions {
                    registry: &registry,
                    display_name: None,
                    stack_settings: settings.clone(),
                    registration: None,
                    setup_identity: None,
                    helm_install: None,
                    supported_aws_regions: Vec::new(),
                    pins: Default::default(),
                },
            )
            .map(|_| ())
        }
        Format::CloudFormation => {
            let mut registry = alien_cloudformation::CfRegistry::built_in();
            for resource_type in [
                alien_core::Postgres::RESOURCE_TYPE,
                alien_core::Redis::RESOURCE_TYPE,
            ] {
                registry.register(resource_type, Platform::Aws, EmbedderDatabase);
            }
            registry.register(NETWORK_EXTENSION_TYPE, Platform::Aws, EmbedderDatabase);
            alien_cloudformation::generate_cloudformation_template(
                prepared,
                alien_cloudformation::CloudFormationOptions {
                    registry: &registry,
                    target: alien_cloudformation::CloudFormationTarget::Eks,
                    source_stack: source,
                    stack_settings: settings.clone(),
                    setup_target: "kubernetes".to_string(),
                    setup_fingerprint: "test".to_string(),
                    setup_fingerprint_version: 1,
                    registration: alien_cloudformation::RegistrationMode::CustomResource {
                        lambda_arn: "arn:aws:lambda:us-east-1:111122223333:function:register"
                            .to_string(),
                        callback_url: None,
                    },
                    description: None,
                    management_trust: Default::default(),
                    pins: Default::default(),
                },
            )
            .map(|_| ())
        }
    }
}

/// Setup puts a database or cache on an existing cluster's network only for a GCP Postgres.
/// Every other combination would land the endpoint on a network the cluster is not on, so the
/// generator refuses it with a message that says what to do instead. A created cluster keeps all
/// of them, and the supported cell (Postgres on an existing GKE cluster) is not refused.
#[tokio::test]
async fn a_database_an_existing_cluster_cannot_reach_is_refused_in_every_format() {
    for format in MATRIX {
        for resource in ["postgres", "redis"] {
            let mut source = source_stack();
            source.resources.insert(
                resource.to_string(),
                alien_core::ResourceEntry {
                    config: if resource == "postgres" {
                        alien_core::Resource::new(
                            alien_core::Postgres::new(resource.to_string()).build(),
                        )
                    } else {
                        alien_core::Resource::new(
                            alien_core::Redis::new(resource.to_string()).build(),
                        )
                    },
                    lifecycle: ResourceLifecycle::Frozen,
                    dependencies: Vec::new(),
                    display_name: None,
                    description: None,
                    remote_access: false,
                    enabled_when: None,
                },
            );
            let supported =
                matches!(format, Format::Terraform(TerraformTarget::Gke)) && resource == "postgres";
            let existing = settings(KubernetesClusterOwnership::Existing);
            let prepared = prepared_for_render(&source, format, &existing).await;
            let result = render_with_database_emitters(&prepared, &source, format, &existing);
            if supported {
                result.unwrap_or_else(|error| panic!("{format:?} {resource}: {error:?}"));
            } else {
                let error = result.expect_err(&format!("{format:?} {resource} is refused"));
                assert_eq!(error.code, "SETUP_PLACEMENT_UNSUPPORTED");
                assert_eq!(error.http_status_code, Some(422));
                assert!(!error.retryable);
                assert!(
                    error.message.contains(&format!(
                        "Setup cannot yet place the {resource} '{resource}' on an existing cluster's network"
                    )) && error.message.contains("Use a cluster that setup creates")
                        && error.message.contains(&format!("or remove the {resource}")),
                    "{format:?} {resource}: {}",
                    error.message
                );
            }
            let created = settings(KubernetesClusterOwnership::Managed);
            let prepared = prepared_for_render(&source, format, &created).await;
            render_with_database_emitters(&prepared, &source, format, &created).unwrap_or_else(
                |error| panic!("{format:?} {resource} on a created cluster: {error:?}"),
            );
        }
    }
}

/// Customers already hold modules generated before this rule moved into one place. They never registered a `compute` for a Kubernetes target,
/// and the manager must accept exactly what they send, without regenerating them. The ids below
/// are the ones the v3.3.68 Terraform generator registers for this stack on a GKE cluster that
/// already exists (generated from that tag and copied here), written out rather than generated so a
/// later generator change cannot move them.
#[tokio::test]
async fn a_module_generated_before_the_shared_rule_still_registers() {
    let source = source_stack();
    let settings = settings(KubernetesClusterOwnership::Existing);
    let registered: Vec<(String, String)> = [
        ("assets", "storage"),
        ("app-secrets", "vault"),
        ("kubernetes", "kubernetes-cluster"),
        ("app-sa", "service-account"),
        ("enable-cloud-quotas", "service_activation"),
        ("enable-cloud-storage", "service_activation"),
        ("enable-compute-engine", "service_activation"),
        ("enable-container", "service_activation"),
        ("enable-iam-credentials", "service_activation"),
        ("enable-secret-manager", "service_activation"),
    ]
    .into_iter()
    .map(|(id, kind)| (id.to_string(), kind.to_string()))
    .collect();
    let req = registration_request(
        &registered,
        Format::Terraform(TerraformTarget::Gke),
        &settings,
    );
    let prepared = prepare_import_stack(source, &req, &req.stack_settings)
        .await
        .expect("the manager prepares the stack");
    validate_import_handoff(&req, &prepared)
        .expect("a module that registers no compute must be accepted");
    // The compute cluster stays declared, Frozen and unregistered: the executor seeds it for the
    // Operator's Kubernetes controller (see `imported_kubernetes_compute_retries_namespace_verification`
    // in alien-infra, which continues an imported Kubernetes state that has none), so provisioning
    // never asks a cloud controller to create it.
    let compute = prepared
        .resources
        .get("compute")
        .expect("the stack still declares its compute cluster");
    assert_eq!(compute.lifecycle, ResourceLifecycle::Frozen);
    assert!(!req.resources.iter().any(|r| r.id == "compute"));
}

/// Only a module generated before Kubernetes compute stopped being registered would send a
/// `compute` for a Kubernetes target. The manager rejects such a registration, with a message that
/// names the resource and says to regenerate; it does not accept and ignore it. (No known install
/// sends one: this test fixes which of the two the manager does.)
#[tokio::test]
async fn an_older_registration_that_includes_compute_is_rejected_with_a_clear_message() {
    let source = source_stack();
    let settings = settings(KubernetesClusterOwnership::Existing);
    let registered: Vec<(String, String)> = [
        ("assets", "storage"),
        ("app-secrets", "vault"),
        ("kubernetes", "kubernetes-cluster"),
        ("app-sa", "service-account"),
        ("compute", "compute-cluster"),
    ]
    .into_iter()
    .map(|(id, kind)| (id.to_string(), kind.to_string()))
    .collect();
    let req = registration_request(
        &registered,
        Format::Terraform(TerraformTarget::Gke),
        &settings,
    );
    let prepared = prepare_import_stack(source, &req, &req.stack_settings)
        .await
        .expect("the manager prepares the stack");
    let message = validate_import_handoff(&req, &prepared)
        .expect_err("a Kubernetes registration with `compute` is refused")
        .message;
    assert!(
        message.contains("compute") && message.contains("Regenerate"),
        "{message}"
    );
}

/// An existing GKE cluster with a database: a module generated before the cluster's own network
/// was used registered a created network; the module generated now registers the cluster's as
/// one the deployer brought. Running the new module over the installed deployment is a setup
/// update, and the manager must take it: the settings change passes the management-permission
/// check and the registration passes the handoff check.
#[tokio::test]
async fn a_setup_update_that_moves_a_gke_database_to_the_clusters_own_network_is_accepted() {
    let source = Stack::new("parity-db".to_string())
        .permission("app", PermissionProfile::new())
        .add(
            alien_core::Postgres::new("db".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Storage::new("assets".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let mut installed = settings(KubernetesClusterOwnership::Existing);
    installed.network = Some(alien_core::NetworkSettings::Create {
        cidr: None,
        availability_zones: 2,
    });
    let mut requested = installed.clone();
    requested.network = Some(alien_core::NetworkSettings::ByoVpcGcp {
        network_name: "cluster-net".to_string(),
        subnet_name: "cluster-subnet".to_string(),
        region: "region".to_string(),
    });
    let ids = [
        ("assets", "storage"),
        ("default-network", "network"),
        ("db", "postgres"),
        ("app-sa", "service-account"),
        ("enable-cloud-quotas", "service_activation"),
        ("enable-cloud-sql", "service_activation"),
        ("enable-cloud-storage", "service_activation"),
        ("enable-compute-engine", "service_activation"),
        ("enable-secret-manager", "service_activation"),
    ];
    let registered: Vec<(String, String)> = ids
        .into_iter()
        .map(|(id, kind)| (id.to_string(), kind.to_string()))
        .collect();
    let req = registration_request(
        &registered,
        Format::Terraform(TerraformTarget::Gke),
        &requested,
    );
    refuse_management_permission_changes(&source, &installed, &req)
        .await
        .expect("moving the database's network to the cluster's is not a management change");
    let prepared = prepare_import_stack(source, &req, &req.stack_settings)
        .await
        .expect("the manager prepares the stack");
    let network = prepared
        .resources
        .get("default-network")
        .expect("a stack with a database keeps a network in an existing cluster");
    assert!(matches!(
        network
            .config
            .downcast_ref::<alien_core::Network>()
            .map(|n| &n.settings),
        Some(alien_core::NetworkSettings::ByoVpcGcp { .. })
    ));
    validate_import_handoff(&req, &prepared).expect("the updated module's registration is taken");
}
