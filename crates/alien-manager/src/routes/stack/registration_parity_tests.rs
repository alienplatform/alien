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
    CapacityGroup, ComputeCluster, Container, ContainerCode, KubernetesClusterOwnership, KubernetesClusterSettings,
    KubernetesSettings, PermissionProfile, ResourceSpec, Storage, Vault,
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
async fn prepared_for_render(
    source: &Stack,
    format: Format,
    settings: &StackSettings,
) -> Stack {
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
        serde_json::Value::Array(items) => items.iter().for_each(|v| collect_json_entries(v, found)),
        _ => {}
    }
}

/// The registration a Terraform module sends: the entries of its `deployment_resources` local.
fn terraform_registration(prepared: &Stack, target: TerraformTarget, settings: &StackSettings) -> Vec<(String, String)> {
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
    let local = body
        .blocks()
        .flat_map(|block| block.body().attributes())
        .find(|attribute| attribute.key() == "deployment_resources")
        .expect("the module registers its resources in `local.deployment_resources`");
    let mut found = Vec::new();
    collect_hcl_entries(local.expr(), &mut found);
    found
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

/// Every `{ id, type, importData }` object in an HCL expression, wherever a list builder, a
/// conditional or a function call nests it. The entries of a gated resource are collected too:
/// this stack gates none, so every collected entry is sent.
fn collect_hcl_entries(expression: &hcl::Expression, found: &mut Vec<(String, String)>) {
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
                found.push((id, kind));
            }
            map.values().for_each(|v| collect_hcl_entries(v, found));
        }
        hcl::Expression::Array(items) => items.iter().for_each(|v| collect_hcl_entries(v, found)),
        hcl::Expression::Parenthesis(inner) => collect_hcl_entries(inner, found),
        hcl::Expression::Conditional(conditional) => {
            collect_hcl_entries(&conditional.true_expr, found);
            collect_hcl_entries(&conditional.false_expr, found);
        }
        hcl::Expression::FuncCall(call) => {
            call.args.iter().for_each(|v| collect_hcl_entries(v, found))
        }
        _ => {}
    }
}

/// The registration a CloudFormation template sends: the `Resources` its registration custom
/// resource carries.
fn cloudformation_registration(prepared: &Stack, source: &Stack, settings: &StackSettings) -> Vec<(String, String)> {
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
async fn register(
    format: Format,
    ownership: KubernetesClusterOwnership,
) -> (BTreeSet<String>, crate::error::Result<()>) {
    let source = source_stack();
    let settings = settings(ownership);
    let prepared = prepared_for_render(&source, format, &settings).await;
    let registered = match format {
        Format::Terraform(target) => terraform_registration(&prepared, target, &settings),
        Format::CloudFormation => cloudformation_registration(&prepared, &source, &settings),
    };
    let ids: BTreeSet<String> = registered.iter().map(|(id, _)| id.clone()).collect();
    assert_eq!(
        ids.len(),
        registered.len(),
        "{format:?} {ownership:?}: duplicate registration ids {registered:?}"
    );
    let req = registration_request(&registered, format, &settings);
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
