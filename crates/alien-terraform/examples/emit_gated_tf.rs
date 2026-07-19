//! Emit the Terraform module for a minimally gated stack, for the
//! permission-gate apply-and-inspect e2e (`tests/e2e/permission-gate-tf.sh`).
//!
//! Same shape as the CloudFormation example (`emit_gated_cfn` in
//! `alien-cloudformation`): one `Kv` store whose `kv/data-write` grant on the
//! service-account identity is gated on the boolean `kvEnabled` deployer input.
//! Rendered for GCP or Azure so the gate can be proven on real cloud IAM the
//! same way the AWS e2e proves it on CloudFormation-baked IAM: deploying with
//! the input off leaves the grant out of the identity, on adds it.
//!
//! The module is emitted with no self-registration (`registration: None`), so
//! `terraform apply` provisions everything directly, with no external
//! registration step: it just materializes the Frozen resources and the gated
//! IAM binding.
//!
//! Run: `cargo run --example emit_gated_tf -p alien-terraform -- <aws|gcp|azure> <out-dir>`

use alien_core::{
    AzureResourceGroup, AzureStorageAccount, Kv, PermissionGate, PermissionProfile,
    PermissionsConfig, ResourceLifecycle, ServiceAccount, Stack, StackInputDefaultValue,
    StackInputDefinition, StackInputEnvironmentMapping, StackInputKind, StackInputProvider,
    StackSettings,
};
use alien_terraform::{generate_terraform_module, TerraformOptions, TerraformTarget, TfRegistry};
use std::{env, fs, path::Path};

fn main() {
    let mut args = env::args().skip(1);
    let usage = "usage: emit_gated_tf <aws|gcp|azure> <out-dir>";
    let target_arg = args.next().expect(usage);
    let out_dir = args.next().expect(usage);
    let target = match target_arg.as_str() {
        "aws" => TerraformTarget::Aws,
        "gcp" => TerraformTarget::Gcp,
        "azure" => TerraformTarget::Azure,
        other => panic!("unsupported target '{other}' ({usage})"),
    };

    let profile = PermissionProfile::new().resource("store", ["kv/data-write"]);
    let mut permissions = PermissionsConfig::new().with_profile("execution", profile.clone());
    permissions.gates = vec![PermissionGate {
        profile: "execution".to_string(),
        resource: "store".to_string(),
        permission_set_id: "kv/data-write".to_string(),
        input_id: "kvEnabled".to_string(),
        enabled_value: "true".to_string(),
    }];
    let service_account =
        ServiceAccount::from_permission_profile("execution-sa".to_string(), &profile, |name| {
            alien_permissions::get_permission_set(name).cloned()
        })
        .expect("permission set should resolve");

    let kv_enabled = StackInputDefinition {
        id: "kvEnabled".to_string(),
        kind: StackInputKind::Boolean,
        provided_by: vec![StackInputProvider::Deployer],
        required: false,
        label: "KV enabled".to_string(),
        description: "Whether the store's data-write grant is included.".to_string(),
        placeholder: None,
        default: Some(StackInputDefaultValue::Boolean(false)),
        platforms: None,
        validation: None,
        env: vec![StackInputEnvironmentMapping {
            name: "APP_KV_ENABLED".to_string(),
            target_resources: None,
            var_type: None,
        }],
    };

    let mut stack = Stack::new("alien-gate-e2e".to_string())
        .permissions(permissions)
        .inputs(vec![kv_enabled])
        .add(service_account, ResourceLifecycle::Frozen)
        .add(Kv::new("store".to_string()).build(), ResourceLifecycle::Frozen);
    // Azure's Kv is a Storage Table, which needs a sibling storage account (and
    // its resource group). The other clouds' Kv is self-contained (DynamoDB /
    // Firestore). On Azure these are normally preflight-injected; add them here
    // so the module renders and applies standalone.
    if matches!(target, TerraformTarget::Azure) {
        stack = stack
            .add(
                AzureResourceGroup::new("default-resource-group".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                AzureStorageAccount::new("default-storage-account".to_string()).build(),
                ResourceLifecycle::Frozen,
            );
    }
    let stack = stack.build();

    let registry = TfRegistry::built_in();
    let module = generate_terraform_module(
        &stack,
        target,
        TerraformOptions {
            registry: &registry,
            display_name: Some("Permission-gate apply-and-inspect e2e".to_string()),
            stack_settings: StackSettings::default(),
            registration: None,
            helm_install: None,
            supported_aws_regions: vec![],
        },
    )
    .expect("module should render");

    let dir = Path::new(&out_dir);
    fs::create_dir_all(dir).expect("create out dir");
    for (path, contents) in module.iter() {
        let full = dir.join(path);
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent).expect("create parent dir");
        }
        fs::write(&full, contents).expect("write module file");
    }
    eprintln!("wrote {} files to {}", module.files.len(), out_dir);
}
