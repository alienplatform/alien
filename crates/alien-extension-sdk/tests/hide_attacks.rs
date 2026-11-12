//! Modifier output that would hide one of Alien's resources, or change what Alien's templates
//! render, from a module that returns whatever its input asks for (the `steered` guest). The
//! modifier pass refuses each one, naming the extension.

use alien_cloudformation::{
    generate_cloudformation_template, CloudFormationOptions, CloudFormationTarget, RegistrationMode,
};
use alien_core::{Network, NetworkSettings, ResourceLifecycle, Stack, StackSettings, Storage};
use alien_extension_sdk::testing::{build_module, load_module, HookModule};
use alien_helm::{generate_helm_chart, HelmOptions};
use alien_infra::{DeploymentRegistries, HostRegistrations, LoadedExtensions};
use alien_terraform::{generate_terraform_module, ModuleFiles, TerraformOptions, TerraformTarget};
use indexmap::IndexMap;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

fn steered() -> HookModule {
    static MODULE: OnceLock<HookModule> = OnceLock::new();
    MODULE
        .get_or_init(|| {
            let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml");
            let target_dir =
                PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("extension-sdk-guests");
            let wasm = build_module(&workspace, "steered-hooks", &target_dir)
                .unwrap_or_else(|error| panic!("{error}"));
            load_module(&wasm).unwrap_or_else(|error| panic!("{error}"))
        })
        .clone()
}

fn network_settings() -> StackSettings {
    StackSettings {
        network: Some(NetworkSettings::Create {
            cidr: Some("10.42.0.0/16".to_string()),
            availability_zones: 2,
        }),
        ..StackSettings::default()
    }
}

/// A bucket and the default network, which Alien's templates create behind a network-mode
/// gate, with the `steered` extension.
fn registries() -> (Stack, DeploymentRegistries) {
    let mut stack = Stack::new("my-app".to_string())
        .add(
            Storage::new("data".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Network::new("default-network".to_string())
                .settings(network_settings().network.unwrap())
                .build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    stack.extensions = vec![serde_json::from_value(json!({
        "name": "steered", "version": "1.0.0", "digest": "sha256:0", "resourceTypes": {}
    }))
    .unwrap()];
    let loaded = LoadedExtensions::default().with_hooks("steered", steered());
    let registries = DeploymentRegistries::for_stack(&stack, &HostRegistrations::NONE, &loaded);
    (stack, registries)
}

/// The pass's refusal when the CloudFormation modifier returns Alien's template after `edit`.
fn cloudformation_refusal(edit: impl Fn(&mut serde_json::Value)) -> String {
    let (stack, registries) = registries();
    let mut template = generate_cloudformation_template(
        &stack,
        CloudFormationOptions {
            registry: &registries.cloudformation,
            source_stack: &stack,
            target: CloudFormationTarget::Aws,
            stack_settings: network_settings(),
            setup_target: "aws".to_string(),
            setup_fingerprint: "test".to_string(),
            setup_fingerprint_version: 1,
            registration: RegistrationMode::OutputsFallback,
            description: None,
            management_trust: Default::default(),
        },
    )
    .unwrap();
    let mut modified = serde_json::to_value(&template).unwrap();
    edit(&mut modified);
    template.description = Some(format!("STEER{modified}"));
    registries
        .modifiers
        .modify_cloudformation(&mut template)
        .unwrap_err()
        .message
}

fn terraform(target: TerraformTarget) -> (ModuleFiles, DeploymentRegistries) {
    let (stack, registries) = registries();
    let module = generate_terraform_module(
        &stack,
        target,
        TerraformOptions {
            registry: &registries.terraform,
            display_name: None,
            stack_settings: network_settings(),
            registration: None,
            helm_install: None,
            supported_aws_regions: Vec::new(),
        },
    )
    .unwrap();
    (module, registries)
}

/// The pass's refusal when the Terraform modifier changes the files `edit` returns.
fn terraform_refusal(
    target: TerraformTarget,
    edit: impl Fn(&IndexMap<String, String>) -> IndexMap<String, String>,
) -> String {
    let (mut module, registries) = terraform(target);
    let changed = edit(&module.files);
    module.files.insert(
        "steer.json".into(),
        json!({ "changed": changed }).to_string(),
    );
    registries
        .modifiers
        .modify_terraform(&mut module)
        .unwrap_err()
        .message
}

/// `path` of Alien's module with its first `from` replaced by `to`.
fn replaced(
    files: &IndexMap<String, String>,
    path: &str,
    from: &str,
    to: &str,
) -> IndexMap<String, String> {
    let original = &files[path];
    assert!(original.contains(from), "{path} lacks {from:?}");
    IndexMap::from([(path.to_string(), original.replacen(from, to, 1))])
}

fn assert_refused(message: String, modifier: &str, refusal: &str) {
    assert!(
        message.starts_with(&format!("the {modifier} of extension 'steered' {refusal}")),
        "{message}"
    );
}

const CLOUDFORMATION: &str = "CloudFormation modifier `cloudformationModifier/0`";
const TERRAFORM: &str = "Terraform modifier `terraformModifier/1`";
const HELM: &str = "Helm modifier `helmModifier/2`";

#[test]
fn redefined_conditions_that_would_skip_alien_s_network_are_refused() {
    let message = cloudformation_refusal(|template| {
        for (_, condition) in template["Conditions"].as_object_mut().unwrap() {
            *condition = json!({ "Fn::Equals": ["steered", "never"] });
        }
    });
    assert_refused(message, CLOUDFORMATION, "changed or removed Conditions.");
}

#[test]
fn changed_parameter_defaults_are_refused() {
    let message = cloudformation_refusal(|template| {
        for (_, parameter) in template["Parameters"].as_object_mut().unwrap() {
            if parameter.get("Default").is_some() {
                parameter["Default"] = json!("steered");
                parameter.as_object_mut().unwrap().remove("AllowedValues");
            }
        }
    });
    assert_refused(message, CLOUDFORMATION, "changed or removed Parameters.");
}

#[test]
fn a_stack_set_is_refused() {
    let message = cloudformation_refusal(|template| {
        template["Resources"]["SteeredStackSet"] = json!({
            "Type": "AWS::CloudFormation::StackSet",
            "Properties": { "StackSetName": "steered", "PermissionModel": "SELF_MANAGED", "TemplateURL": "https://vendor.example/x.yaml" }
        });
    });
    assert_refused(
        message,
        CLOUDFORMATION,
        "added or changed SteeredStackSet, a nested stack (AWS::CloudFormation::StackSet)",
    );
}

#[test]
fn a_network_mode_default_that_would_empty_the_network_s_count_is_refused() {
    let message = terraform_refusal(TerraformTarget::Aws, |files| {
        let variables = &files["variables.tf"];
        let at = variables
            .find("variable \"network_mode\"")
            .expect("Alien's network_mode variable");
        let default = at
            + variables[at..]
                .find("\"create-new\"\n")
                .expect("its default");
        let mut edited = variables.clone();
        edited.replace_range(default..default + "\"create-new\"".len(), "\"use-default\"");
        IndexMap::from([("variables.tf".to_string(), edited)])
    });
    assert_refused(
        message,
        TERRAFORM,
        "changed or removed variable.network_mode in variables.tf, which Alien's resources read",
    );
}

#[test]
fn changed_locals_are_refused() {
    let message = terraform_refusal(TerraformTarget::Aws, |files| {
        replaced(files, "locals.tf", "locals {", "locals {\n  steered = 1")
    });
    assert_refused(
        message,
        TERRAFORM,
        "changed or removed locals in locals.tf, which Alien's resources read",
    );
}

#[test]
fn alien_s_provider_and_terraform_settings_are_refused() {
    let message = terraform_refusal(TerraformTarget::Aws, |files| {
        replaced(
            files,
            "providers.tf",
            "region = var.aws_region",
            "region = \"eu-north-1\"\n  assume_role {\n    role_arn = \"arn:aws:iam::111111111111:role/vendor\"\n  }",
        )
    });
    assert_refused(
        message,
        TERRAFORM,
        "changed or removed provider.aws in providers.tf, which Alien's resources read",
    );
    let message = terraform_refusal(TerraformTarget::Aws, |files| {
        replaced(
            files,
            "versions.tf",
            "source  = \"hashicorp/aws\"",
            "source  = \"vendor/aws\"",
        )
    });
    assert_refused(
        message,
        TERRAFORM,
        "changed or removed terraform in versions.tf, which Alien's resources read",
    );
}

#[test]
fn an_extension_file_that_configures_terraform_or_reaches_alien_s_resources_is_refused() {
    for (block, refusal) in [
        ("provider \"aws\" {\n  region = \"eu-north-1\"\n}\n", "declares a provider block in steered.tf"),
        ("terraform {\n  required_providers {\n    vendor = {\n      source = \"vendor/vendor\"\n    }\n  }\n}\n", "declares a terraform block in steered.tf"),
        ("moved {\n  from = aws_s3_bucket.data\n  to   = aws_s3_bucket.vendor\n}\n", "declares a moved block in steered.tf"),
        ("import {\n  to = aws_s3_bucket.data\n  id = \"vendor-bucket\"\n}\n", "declares an import block in steered.tf"),
        ("removed {\n  from = aws_s3_bucket.data\n  lifecycle {\n    destroy = false\n  }\n}\n", "declares a removed block in steered.tf"),
        ("module \"vendor\" {\n  source = \"git::https://vendor.example/x.git\"\n}\n", "declares module.vendor in steered.tf with a source that isn't a local path"),
    ] {
        let message = terraform_refusal(TerraformTarget::Aws, |_| {
            IndexMap::from([("steered.tf".to_string(), block.to_string())])
        });
        assert_refused(message, TERRAFORM, refusal);
    }
}

/// The pass's refusal when the Helm modifier returns `templates`.
fn helm_refusal(templates: &[(&str, &str)]) -> String {
    let (stack, registries) = registries();
    let mut chart = generate_helm_chart(
        &stack,
        HelmOptions {
            registry: &registries.helm,
            stack_settings: StackSettings::default(),
            chart_name: stack.id().to_string(),
        },
    )
    .unwrap();
    let templates: IndexMap<&str, &str> = templates.iter().copied().collect();
    chart.files.insert(
        "steer.json".into(),
        json!({ "templates": templates }).to_string(),
    );
    registries
        .modifiers
        .modify_helm(&mut chart)
        .unwrap_err()
        .message
}

#[test]
fn a_template_that_changes_the_shared_release_is_refused() {
    for template in [
        // An upgrade render would skip Alien's guard against a changed encryption key.
        "{{- $_ := set .Release \"IsUpgrade\" false -}}\n",
        // Through a variable, it would move Alien's objects to another namespace.
        "{{- $r := .Release -}}{{- $_ := set $r \"Namespace\" \"vendor-ns\" -}}\n",
    ] {
        assert_refused(
            helm_refusal(&[("zz-steered.yaml", template)]),
            HELM,
            "calls set in zz-steered.yaml;",
        );
    }
    assert_refused(
        helm_refusal(&[(
            "zz-tpl.yaml",
            "{{- tpl \"{{ define \\\"deployment.fullname\\\" }}vendor{{ end }}\" . -}}\n",
        )]),
        HELM,
        "calls tpl in zz-tpl.yaml;",
    );
}
