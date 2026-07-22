//! AWS data-layer scenarios — storage / kv / queue / vault / ai.
//!
//! Each scenario is one multi-file snapshot so reviewers see the
//! complete module a developer would `terraform apply`. Every scenario
//! goes through `terraform fmt -check` + `terraform validate` against
//! the real AWS provider.

use super::helpers::{assert_terraform_valid, render, snapshot_module};
use alien_core::{
    Ai, Kv, LifecycleRule, PermissionProfile, Queue, ResourceLifecycle, ServiceAccount, Stack,
    StackSettings, Storage, Vault,
};
use alien_terraform::TerraformTarget;

#[test]
fn aws_storage_minimal_renders_idiomatic_module() {
    let stack = Stack::new("acme-prod".to_string())
        .add(
            Storage::new("data".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Aws, StackSettings::default());
    snapshot_module("aws_storage_minimal", &module);
    assert_terraform_valid(&module, "aws_storage_minimal");
}

#[test]
fn aws_storage_with_versioning_and_lifecycle() {
    let stack = Stack::new("acme-audit".to_string())
        .add(
            Storage::new("audit".to_string())
                .versioning(true)
                .lifecycle_rules(vec![LifecycleRule {
                    days: 90,
                    prefix: Some("logs/".to_string()),
                }])
                .build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Aws, StackSettings::default());
    snapshot_module("aws_storage_versioning_and_lifecycle", &module);
    assert_terraform_valid(&module, "aws_storage_versioning_and_lifecycle");
}

#[test]
fn aws_storage_public_read_allows_get_object() {
    let stack = Stack::new("acme-public".to_string())
        .add(
            Storage::new("assets".to_string()).public_read(true).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Aws, StackSettings::default());
    snapshot_module("aws_storage_public_read", &module);
    assert_terraform_valid(&module, "aws_storage_public_read");
}

#[test]
fn aws_kv_renders_dynamodb_table_with_pitr() {
    let stack = Stack::new("acme-kv".to_string())
        .add(
            Kv::new("metadata".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Aws, StackSettings::default());
    snapshot_module("aws_kv_minimal", &module);
    assert_terraform_valid(&module, "aws_kv_minimal");
}

#[test]
fn aws_queue_renders_sqs_with_managed_sse() {
    let stack = Stack::new("acme-queue".to_string())
        .add(
            Queue::new("jobs".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Aws, StackSettings::default());
    snapshot_module("aws_queue_minimal", &module);
    assert_terraform_valid(&module, "aws_queue_minimal");
}

#[test]
fn aws_vault_emits_only_import_data() {
    let stack = Stack::new("acme-vault".to_string())
        .add(
            Vault::new("secrets".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Aws, StackSettings::default());
    snapshot_module("aws_vault_minimal", &module);
    assert_terraform_valid(&module, "aws_vault_minimal");
}

#[test]
fn aws_vault_resource_permissions_attach_to_service_account_role() {
    let stack = Stack::new("acme-vault".to_string())
        .permission(
            "execution",
            PermissionProfile::new().resource("secrets", ["vault/data-read"]),
        )
        .add(
            ServiceAccount::new("execution-sa".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Vault::new("secrets".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Aws, StackSettings::default());
    let rendered = module
        .iter()
        .map(|(_, contents)| contents)
        .collect::<String>();

    assert!(rendered.contains("aws_iam_role_policy\" \"execution_sa_vault_secrets_set_0\""));
    assert!(rendered.contains("ssm:GetParameter"));
    assert!(rendered.contains("parameter/${local.resource_prefix}-secrets-*"));
    assert_terraform_valid(&module, "aws_vault_service_account_permissions");
}

#[test]
fn aws_data_layer_renders_complete_stack() {
    let stack = Stack::new("acme-data".to_string())
        .add(
            Storage::new("assets".to_string()).versioning(true).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Queue::new("jobs".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Kv::new("metadata".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Vault::new("secrets".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Aws, StackSettings::default());
    snapshot_module("aws_data_layer_full", &module);
    assert_terraform_valid(&module, "aws_data_layer_full");
}

#[test]
fn aws_ai_emits_only_import_data() {
    // AWS Bedrock has no per-stack cloud resource to provision. The emitter
    // returns an empty fragment so only the import metadata JSON is produced.
    let stack = Stack::new("acme-ai".to_string())
        .add(Ai::new("llm".to_string()).build(), ResourceLifecycle::Frozen)
        .build();
    let module = render(&stack, TerraformTarget::Aws, StackSettings::default());
    snapshot_module("aws_ai_minimal", &module);
    assert_terraform_valid(&module, "aws_ai_minimal");

    // Import metadata must carry the region so the controller can reconstruct
    // the Bedrock endpoint. The import ref appears in locals.tf.
    let locals = module
        .get("locals.tf")
        .expect("locals.tf should render");
    assert!(locals.contains("region"), "import ref must carry region");
}

#[test]
fn aws_ai_invoke_permissions_attach_to_service_account_role() {
    // When a permission profile references ai/invoke, the AI emitter attaches the
    // bedrock IAM policy to the workload (service-account) role.
    let stack = Stack::new("acme-ai".to_string())
        .permission(
            "execution",
            PermissionProfile::new().resource("llm", ["ai/invoke"]),
        )
        .add(
            ServiceAccount::new("execution-sa".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(Ai::new("llm".to_string()).build(), ResourceLifecycle::Frozen)
        .build();
    let module = render(&stack, TerraformTarget::Aws, StackSettings::default());
    let rendered = module
        .iter()
        .map(|(_, contents)| contents)
        .collect::<String>();

    assert!(
        rendered.contains("bedrock:InvokeModel"),
        "bedrock InvokeModel action must appear"
    );
    assert!(
        rendered.contains("bedrock:InvokeModelWithResponseStream"),
        "bedrock InvokeModelWithResponseStream action must appear"
    );
    assert!(
        rendered.contains("arn:aws:bedrock:*::foundation-model/*"),
        "bedrock foundation-model ARN must appear"
    );
    assert_terraform_valid(&module, "aws_ai_invoke_permissions");
}
