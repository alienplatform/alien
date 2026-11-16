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
        .expect("template mutations apply")
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
) -> TerraformRegistration {
    let registry = TfRegistry::built_in();
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
    let env = Env { mode };
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
struct Env {
    mode: KubernetesClusterOwnership,
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
    let registry = alien_cloudformation::CfRegistry::built_in();
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
    let source = source_stack();
    let settings = settings(ownership);
    let prepared = prepared_for_render(&source, format, &settings).await;
    let (registered, registered_network) = match format {
        Format::Terraform(target) => {
            let registration = terraform_registration(&prepared, target, &settings, ownership);
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
    let req = registration_request(&registered, format, &sent_settings);
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

/// Known gap, not a skipped case: an existing AKS cluster with a cloud database still gets a new
/// VNet the cluster cannot reach (the Private Endpoint needs its own subnet in the cluster's
/// VNet). GKE and EKS are covered by the network tests in `alien-terraform`. A follow-up changes
/// the AKS module; this test is the place to turn on when it lands, and it fails until then.
#[tokio::test]
#[ignore = "known gap: AKS existing cluster with a database creates a VNet the cluster cannot reach"]
async fn an_existing_aks_cluster_with_a_database_uses_the_cluster_network() {
    panic!("not implemented: the AKS module creates its own VNet for the database endpoint");
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
