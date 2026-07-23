//! AWS data-layer scenarios — storage / kv / queue / vault / ai.
//!
//! Each scenario is one multi-file snapshot so reviewers see the
//! complete module a developer would `terraform apply`. Every scenario
//! goes through `terraform fmt -check` + `terraform validate` against
//! the real AWS provider.

use super::helpers::{assert_terraform_valid, render, snapshot_module};
use alien_core::{
    Ai, FinetuneMethod, FinetuneSpec, Kv, LifecycleRule, PermissionProfile, Queue,
    ResourceLifecycle, ServiceAccount, Stack, StackSettings, Storage, Vault,
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

#[test]
fn aws_ai_finetune_emits_bedrock_trusted_role_with_s3_policy() {
    // When a permission profile references ai/finetune on the AI resource, the
    // emitter provisions a dedicated IAM role Bedrock can assume (the real fix for
    // AccessDenied: service-account roles only trust compute principals) with an
    // inline S3 policy over the stack's storage buckets.
    let stack = Stack::new("acme-ai".to_string())
        .permission(
            "execution",
            PermissionProfile::new().resource("llm", ["ai/finetune"]),
        )
        .add(
            ServiceAccount::new("execution-sa".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Storage::new("training".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Ai::new("llm".to_string())
                .finetune(FinetuneSpec {
                    base_model: "amazon.nova-lite-v1:0".to_string(),
                    training_data: "training".to_string(),
                    training_key: "training.jsonl".to_string(),
                    served_model_id: None,
                    method: FinetuneMethod::Sft,
                })
                .build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Aws, StackSettings::default());
    let rendered = module
        .iter()
        .map(|(_, contents)| contents)
        .collect::<String>();

    // The dedicated finetune role exists, named deterministically to match the
    // controller's role_arn (`${resource_prefix}-llm-finetune`).
    assert!(
        rendered.contains("llm_finetune"),
        "expected a dedicated finetune aws_iam_role:\n{rendered}"
    );
    assert!(
        rendered.contains("llm-finetune"),
        "expected role name suffix llm-finetune matching the controller role_arn:\n{rendered}"
    );
    // Its trust policy allows Bedrock to assume it — the crux of the fix.
    assert!(
        rendered.contains("bedrock.amazonaws.com"),
        "finetune role must trust bedrock.amazonaws.com:\n{rendered}"
    );
    // The inline policy grants S3 read (training data) and write (output),
    // scoped to the stack's storage bucket ARN (not "*").
    assert!(
        rendered.contains("s3:GetObject") && rendered.contains("s3:ListBucket"),
        "finetune role must read the training dataset from S3:\n{rendered}"
    );
    assert!(
        rendered.contains("s3:PutObject"),
        "finetune role must write tuning output to S3:\n{rendered}"
    );
    assert!(
        rendered.contains("aws_s3_bucket.training.arn"),
        "S3 grants must reference the storage bucket ARN, not \"*\":\n{rendered}"
    );
    assert_terraform_valid(&module, "aws_ai_finetune_role");
}
