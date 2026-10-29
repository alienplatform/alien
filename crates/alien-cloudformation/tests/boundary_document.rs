//! Validate a compiled boundary's CloudFormation intrinsics in a real template.

use alien_cloudformation::{test_utils::cfn_lint, to_yaml, CfTemplate};
use alien_core::{Key, PermissionProfile, ResourceLifecycle, Stack};
use alien_permissions::{
    generators::{aws_stack_permissions_boundary, AwsCloudFormationPermissionsGenerator},
    PermissionContext,
};
use serde_json::{json, Value};

#[test]
fn compiled_boundary_document_passes_cloudformation_validation() {
    let document = json!({
        "Version": "2012-10-17",
        "Statement": [
            {
                "Effect": "Allow", "NotAction": "iam:*",
                "Resource": ["arn:aws:s3:::${AWS::StackName}-*"]
            },
            {
                "Effect": "Allow", "Action": ["ecr:BatchGetImage"],
                "Resource": ["arn:aws:ecr:${AWS::Region}:${managingAccountId}:repository/*"]
            },
            {
                "Effect": "Allow", "Action": ["s3:GetObject"],
                "Resource": ["arn:aws:s3:::approved/${aws:username}/*"],
                "Condition": {
                    "ArnEquals": {"aws:PrincipalArn": "${ManagingRoleArn}"},
                    "StringLike": {"s3:ExistingObjectTag/team": "${AWS::StackName}/${aws:PrincipalTag/team, 'unassigned'}"},
                    "Bool": {"aws:SecureTransport": true}
                }
            }
        ]
    });
    validate_document(&document);
}

#[test]
fn generated_catalog_boundary_with_selected_key_passes_cloudformation_validation() {
    let stack = Stack::new("boundary".to_string())
        .permission(
            "application",
            PermissionProfile::new().resource("encryption", ["key/native-volume"]),
        )
        .add(
            Key::new("encryption".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let context = PermissionContext::new()
        .with_stack_prefix("s".repeat(40))
        .with_aws_region("ap-southeast-7")
        .with_aws_account_id("123456789012")
        .with_managing_account_id("210987654321");
    let document = aws_stack_permissions_boundary(&stack, &context, |_, scope| {
        assert_eq!(scope, "encryption");
        Ok(context.clone().with_resource_name(
            "arn:aws:kms:ap-southeast-7:123456789012:key/1234abcd-12ab-34cd-56ef-1234567890ab",
        ))
    })
    .expect("catalog and selected key fit the managed policy limit");
    validate_document(&document);
}

fn validate_document(document: &Value) {
    let rendered = AwsCloudFormationPermissionsGenerator::new()
        .render_document(document)
        .expect("compiled policy renders");
    let mut value = json!({
        "AWSTemplateFormatVersion": "2010-09-09",
        "Parameters": {"ManagingRoleArn": {"Type": "String"}},
        "Resources": {
            "Boundary": {
                "Type": "AWS::IAM::ManagedPolicy",
                "Properties": {
                    "ManagedPolicyName": {"Fn::Sub": "${AWS::StackName}-permissions-boundary"},
                    "PolicyDocument": rendered
                }
            }
        }
    });
    if !rendered.to_string().contains("ManagingRoleArn") {
        value.as_object_mut().unwrap().remove("Parameters");
    }
    let template: CfTemplate = serde_json::from_value(value).expect("template deserializes");
    cfn_lint(&to_yaml(&template).expect("template serializes"))
        .assert_ok("compiled permissions boundary document");
}
