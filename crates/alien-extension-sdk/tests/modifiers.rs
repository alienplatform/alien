//! The modifier pass each generator call site runs: a stack's extensions change Alien's
//! CloudFormation template, Terraform module and Helm chart in the order the stack lists them,
//! and a modifier that loses or retypes one of the template's resources fails, naming its
//! extension.

use alien_cloudformation::{
    generate_cloudformation_template, CfExpression, CfResource, CfTemplate, CloudFormationOptions,
    CloudFormationTarget, RegistrationMode,
};
use alien_core::{ResourceLifecycle, Stack, StackSettings, Storage};
use alien_extension_sdk::testing::{build_module, load_module, HookModule};
use alien_helm::{generate_helm_chart, HelmChart, HelmOptions};
use alien_infra::{DeploymentRegistries, ExtensionChanges, HostRegistrations, LoadedExtensions};
use alien_terraform::{generate_terraform_module, ModuleFiles, TerraformOptions, TerraformTarget};
use serde_json::json;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

fn module(package: &str) -> HookModule {
    static MODULES: OnceLock<Mutex<HashMap<String, HookModule>>> = OnceLock::new();
    let mut modules = MODULES.get_or_init(Mutex::default).lock().unwrap();
    modules
        .entry(package.to_string())
        .or_insert_with(|| {
            let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml");
            let target_dir =
                PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("extension-sdk-guests");
            let wasm = build_module(&workspace, package, &target_dir)
                .unwrap_or_else(|error| panic!("{error}"));
            load_module(&wasm).unwrap_or_else(|error| panic!("{error}"))
        })
        .clone()
}

/// The acme and backups guests' modules, under their extension names.
fn loaded() -> LoadedExtensions {
    LoadedExtensions::default()
        .with_hooks("acme", module("acme-hooks"))
        .with_hooks("backups", module("backups-hooks"))
}

/// A stack with one bucket and the extensions `names`, in that order. None of them declares a
/// resource type, so only their modifiers reach a template.
fn stack(names: &[&str]) -> Stack {
    let mut stack = Stack::new("my-app".to_string())
        .add(
            Storage::new("data".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    stack.extensions = names
        .iter()
        .map(|name| {
            serde_json::from_value(json!({
                "name": name,
                "version": "1.0.0",
                "digest": "sha256:0",
                "resourceTypes": {}
            }))
            .unwrap()
        })
        .collect();
    stack
}

fn registries(stack: &Stack, extensions: &LoadedExtensions) -> DeploymentRegistries {
    DeploymentRegistries::for_stack(stack, &HostRegistrations::NONE, extensions)
}

/// Alien's template for `stack`, with an Aurora cluster such as a host's Postgres emitter writes.
fn cloudformation(stack: &Stack, registries: &DeploymentRegistries) -> CfTemplate {
    let mut template = generate_cloudformation_template(
        stack,
        CloudFormationOptions {
            registry: &registries.cloudformation,
            source_stack: stack,
            target: CloudFormationTarget::Aws,
            stack_settings: StackSettings::default(),
            setup_target: "aws".to_string(),
            setup_fingerprint: "test".to_string(),
            setup_fingerprint_version: 1,
            registration: RegistrationMode::OutputsFallback,
            description: None,
            management_trust: Default::default(),
        },
    )
    .unwrap();
    let mut cluster = CfResource::new("DbCluster".into(), "AWS::RDS::DBCluster".into());
    cluster
        .properties
        .insert("BackupRetentionPeriod".into(), CfExpression::from(7));
    template.resources.insert("DbCluster".into(), cluster);
    template
}

/// The AWS part of a host's Postgres module.
const DB_TF: &str = r#"resource "aws_rds_cluster" "db" {
  engine                  = "aurora-postgresql"
  backup_retention_period = 7
  storage_encrypted       = true
}
"#;

/// Alien's AWS module for `stack`, with a host's Postgres in `db.tf`.
fn terraform(stack: &Stack, registries: &DeploymentRegistries) -> ModuleFiles {
    let mut module = generate_terraform_module(
        stack,
        TerraformTarget::Aws,
        TerraformOptions {
            registry: &registries.terraform,
            display_name: None,
            stack_settings: StackSettings::default(),
            registration: None,
            helm_install: None,
            supported_aws_regions: Vec::new(),
        },
    )
    .unwrap();
    module.files.insert("db.tf".into(), DB_TF.into());
    module
}

fn helm(stack: &Stack, registries: &DeploymentRegistries) -> HelmChart {
    generate_helm_chart(
        stack,
        HelmOptions {
            registry: &registries.helm,
            stack_settings: StackSettings::default(),
            chart_name: stack.id().to_string(),
        },
    )
    .unwrap()
}

fn changes(extension: &str, added: &[&str], changed: &[&str]) -> ExtensionChanges {
    ExtensionChanges {
        extension: extension.to_string(),
        added: added.iter().map(|name| name.to_string()).collect(),
        changed: changed.iter().map(|name| name.to_string()).collect(),
    }
}

fn retention(template: &CfTemplate) -> &CfExpression {
    &template.resources["DbCluster"].properties["BackupRetentionPeriod"]
}

#[test]
fn a_modifier_that_changes_a_property_passes_and_its_change_is_listed() {
    let stack = stack(&["backups"]);
    let registries = registries(&stack, &loaded());

    let mut template = cloudformation(&stack, &registries);
    let alien = template.clone();
    let listed = registries
        .modifiers
        .modify_cloudformation(&mut template)
        .unwrap();
    assert_eq!(listed, [changes("backups", &[], &["DbCluster"])]);
    assert_eq!(retention(&template), &CfExpression::from(30));
    let mut expected = alien;
    expected.resources["DbCluster"]
        .properties
        .insert("BackupRetentionPeriod".into(), CfExpression::from(30));
    assert_eq!(template, expected, "only the retention changes");

    let mut module = terraform(&stack, &registries);
    let alien = module.clone();
    let listed = registries.modifiers.modify_terraform(&mut module).unwrap();
    assert_eq!(
        listed,
        [changes("backups", &[], &["db.tf", "aws_rds_cluster.db"])]
    );
    assert_eq!(
        module.files["db.tf"],
        DB_TF.replace(
            "backup_retention_period = 7",
            "backup_retention_period = 30"
        )
    );
    for (path, contents) in &alien.files {
        if path != "db.tf" {
            assert_eq!(&module.files[path], contents, "{path} is as Alien wrote it");
        }
    }
    assert_eq!(module.files.len(), alien.files.len());
}

#[test]
fn two_extensions_apply_in_the_order_the_stack_lists_them() {
    // The acme guest sets the retention a template's description asks for; backups sets 30.
    for (order, last_value) in [(["backups", "acme"], 35), (["acme", "backups"], 30)] {
        let stack = stack(&order);
        let registries = registries(&stack, &loaded());
        let mut template = cloudformation(&stack, &registries);
        template.description = Some("backups 35".into());

        let listed = registries
            .modifiers
            .modify_cloudformation(&mut template)
            .unwrap();
        assert_eq!(retention(&template), &CfExpression::from(last_value));
        assert_eq!(
            listed,
            [
                changes(order[0], &[], &["DbCluster"]),
                changes(order[1], &[], &["DbCluster"])
            ]
        );
    }
}

#[test]
fn a_modifier_that_removes_or_retypes_a_resource_fails_naming_the_extension() {
    let stack = stack(&["backups", "acme"]);
    let registries = registries(&stack, &loaded());
    for (request, refusal) in [
        (
            "remove DbCluster",
            "removed DbCluster (AWS::RDS::DBCluster) from the template",
        ),
        (
            "retype DbCluster AWS::RDS::DBInstance",
            "changed the type of DbCluster from AWS::RDS::DBCluster to AWS::RDS::DBInstance",
        ),
    ] {
        let mut template = cloudformation(&stack, &registries);
        template.description = Some(request.into());
        let error = registries
            .modifiers
            .modify_cloudformation(&mut template)
            .unwrap_err();
        assert!(
            error
                .message
                .starts_with("the CloudFormation modifier `cloudformationModifier/")
                && error.message.contains("of extension 'acme' ")
                && error.message.ends_with(refusal),
            "{}",
            error.message
        );
    }
}

#[test]
fn a_helm_modifier_adds_a_subchart_and_leaves_alien_s_templates_as_they_were() {
    let stack = stack(&["acme", "backups"]);
    let registries = registries(&stack, &loaded());
    let mut chart = helm(&stack, &registries);
    let alien = chart.clone();

    let listed = registries.modifiers.modify_helm(&mut chart).unwrap();
    assert_eq!(
        listed,
        [
            changes(
                "acme",
                &[
                    "charts/acme/Chart.yaml",
                    "charts/acme/values.yaml",
                    "charts/acme/templates/acme-deny-egress.yaml"
                ],
                &["Chart.yaml", "values.schema.json"]
            ),
            changes("backups", &[], &[])
        ]
    );
    assert!(
        chart.files["charts/acme/templates/acme-deny-egress.yaml"].contains("kind: NetworkPolicy")
    );
    assert_eq!(chart.files.len(), alien.files.len() + 3);
    for (path, contents) in &alien.files {
        if path != "Chart.yaml" && path != "values.schema.json" {
            assert_eq!(&chart.files[path], contents, "{path} is as Alien wrote it");
        }
    }
}

#[test]
fn an_extension_whose_own_file_or_subchart_alien_already_has_is_refused() {
    let stack = stack(&["acme"]);
    let registries = registries(&stack, &loaded());

    let mut module = terraform(&stack, &registries);
    module.files.insert("acme.tf".into(), "locals {}\n".into());
    let error = registries
        .modifiers
        .modify_terraform(&mut module)
        .unwrap_err();
    assert_eq!(
        error.message,
        "the Terraform modifiers of extension 'acme' can't run: the module already has acme.tf, the file extension 'acme' adds its resources to"
    );

    let mut chart = helm(&stack, &registries);
    chart
        .files
        .insert("charts/ACME/Chart.yaml".into(), String::new());
    let error = registries.modifiers.modify_helm(&mut chart).unwrap_err();
    assert_eq!(
        error.message,
        "the Helm modifiers of extension 'acme' can't run: the chart already has charts/ACME/Chart.yaml, in the subchart extension 'acme' adds its templates to"
    );
}

#[test]
fn a_listed_extension_whose_module_is_not_loaded_fails_the_pass() {
    let stack = stack(&["acme"]);
    let registries = registries(&stack, &LoadedExtensions::default());
    let mut template = cloudformation(&stack, &registries);
    let error = registries
        .modifiers
        .modify_cloudformation(&mut template)
        .unwrap_err();
    assert_eq!(
        error.message,
        "extension 'acme' can't run: this process has not loaded the extension's hooks module"
    );
}

#[test]
fn a_stack_without_extensions_keeps_every_template_as_the_generator_wrote_it() {
    let stack = stack(&[]);
    // A module loaded for an extension the stack doesn't list never runs.
    let registries = registries(&stack, &loaded());

    let mut template = cloudformation(&stack, &registries);
    let alien = serde_json::to_string(&template).unwrap();
    assert!(registries
        .modifiers
        .modify_cloudformation(&mut template)
        .unwrap()
        .is_empty());
    assert_eq!(serde_json::to_string(&template).unwrap(), alien);

    let mut module = terraform(&stack, &registries);
    let alien = module.clone();
    assert!(registries
        .modifiers
        .modify_terraform(&mut module)
        .unwrap()
        .is_empty());
    assert_eq!(module.files, alien.files);

    let mut chart = helm(&stack, &registries);
    let alien = chart.clone();
    assert!(registries
        .modifiers
        .modify_helm(&mut chart)
        .unwrap()
        .is_empty());
    assert_eq!(chart.files, alien.files);
}
