//! AWS AI (Bedrock) CloudFormation emitter tests.

use super::helpers::render_built_ins;
use alien_cloudformation::RegistrationMode;
use alien_core::{Ai, PermissionProfile, ResourceLifecycle, ServiceAccount, Stack, StackSettings};

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
}
