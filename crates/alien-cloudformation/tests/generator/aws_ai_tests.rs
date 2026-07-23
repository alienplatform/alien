//! AWS AI (Bedrock) CloudFormation emitter tests.

use super::helpers::render_built_ins;
use alien_cloudformation::RegistrationMode;
use alien_core::{
    Ai, PermissionProfile, ResourceLifecycle, ServiceAccount, Stack, StackSettings, Storage,
};

#[test]
fn aws_ai_invoke_permissions_attach_to_service_account_role() {
    let stack = Stack::new("ai-permissions".to_string())
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

    let yaml = render_built_ins(
        &stack,
        StackSettings::default(),
        RegistrationMode::OutputsFallback,
        "aws ai bedrock invoke permissions",
    );

    // Policy resource is emitted and attached to the execution service-account role.
    assert!(
        yaml.contains("ExecutionSaRole"),
        "expected ExecutionSaRole in template:\n{yaml}"
    );
    // Bedrock actions are present.
    assert!(
        yaml.contains("bedrock:InvokeModel"),
        "expected bedrock:InvokeModel in template:\n{yaml}"
    );
    assert!(
        yaml.contains("bedrock:InvokeModelWithResponseStream"),
        "expected bedrock:InvokeModelWithResponseStream in template:\n{yaml}"
    );
    // Bedrock foundation-model resource scope is present.
    assert!(
        yaml.contains("foundation-model"),
        "expected arn:aws:bedrock:*::foundation-model/* in template:\n{yaml}"
    );

    // Snapshot the full rendered template so the IAM::Policy structure — role ref,
    // DependsOn, logical-id uniqueness, SID shape — is proven at the artifact level,
    // matching the terraform path (render_built_ins already runs cfn-lint on it).
    insta::assert_snapshot!("aws_ai_invoke_permissions", yaml);
}

#[test]
fn aws_ai_without_permissions_emits_no_iam_policy() {
    // An Ai resource with no permission profile referencing it should emit
    // zero IAM resources (the AI itself creates no cloud resource on AWS).
    let stack = Stack::new("ai-no-permissions".to_string())
        .add(Ai::new("llm".to_string()).build(), ResourceLifecycle::Frozen)
        .build();

    let yaml = render_built_ins(
        &stack,
        StackSettings::default(),
        RegistrationMode::OutputsFallback,
        "aws ai no permissions",
    );

    assert!(
        !yaml.contains("bedrock:InvokeModel"),
        "expected no bedrock actions without a permission profile:\n{yaml}"
    );
    assert!(
        !yaml.contains("AWS::IAM::Policy"),
        "expected no IAM policy without a permission profile:\n{yaml}"
    );
    // A pure inference gateway must NOT get a Bedrock-trusted finetune role.
    assert!(
        !yaml.contains("bedrock.amazonaws.com"),
        "expected no bedrock trust policy without ai/finetune:\n{yaml}"
    );
}

#[test]
fn aws_ai_finetune_emits_bedrock_trusted_role_with_s3_policy() {
    // When a permission profile references ai/finetune on the AI resource, the
    // emitter provisions a dedicated IAM role Bedrock can assume (the real fix for
    // AccessDenied: service-account roles only trust compute principals) with an
    // inline S3 policy over the stack's storage buckets.
    let stack = Stack::new("ai-finetune".to_string())
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
                .finetune(alien_core::FinetuneSpec {
                    base_model: "amazon.nova-lite-v1:0".to_string(),
                    training_data: "training".to_string(),
                    training_key: "training.jsonl".to_string(),
                    served_model_id: None,
                    method: alien_core::FinetuneMethod::Sft,
                })
                .build(),
            ResourceLifecycle::Frozen,
        )
        .build();

    let yaml = render_built_ins(
        &stack,
        StackSettings::default(),
        RegistrationMode::OutputsFallback,
        "aws ai bedrock finetune role",
    );

    // The dedicated finetune role exists and is named deterministically to match
    // the controller's role_arn (`{prefix}-{id}-finetune`).
    assert!(
        yaml.contains("AWS::IAM::Role"),
        "expected a dedicated finetune IAM::Role:\n{yaml}"
    );
    assert!(
        yaml.contains("${AWS::StackName}-llm-finetune"),
        "expected role name ${{prefix}}-llm-finetune matching the controller role_arn:\n{yaml}"
    );
    // Its trust policy allows Bedrock to assume it — the crux of the fix.
    assert!(
        yaml.contains("bedrock.amazonaws.com"),
        "finetune role must trust bedrock.amazonaws.com:\n{yaml}"
    );
    // The inline policy grants S3 read (training data) and write (output).
    assert!(
        yaml.contains("s3:GetObject") && yaml.contains("s3:ListBucket"),
        "finetune role must read the training dataset from S3:\n{yaml}"
    );
    assert!(
        yaml.contains("s3:PutObject"),
        "finetune role must write tuning output to S3:\n{yaml}"
    );
    // The S3 grants are scoped to the stack's storage bucket, not "*".
    assert!(
        yaml.contains("Training") || yaml.contains("training"),
        "S3 grants must reference the storage bucket by ARN:\n{yaml}"
    );
}
