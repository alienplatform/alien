//! Validate a compiled boundary's CloudFormation intrinsics in a real template.

use alien_cloudformation::{test_utils::cfn_lint, to_yaml, CfTemplate};
use alien_permissions::generators::AwsCloudFormationPermissionsGenerator;
use serde_json::json;

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
    let rendered = AwsCloudFormationPermissionsGenerator::new()
        .render_document(&document)
        .expect("compiled policy renders");
    let template: CfTemplate = serde_json::from_value(json!({
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
    }))
    .expect("template deserializes");
    cfn_lint(&to_yaml(&template).expect("template serializes"))
        .assert_ok("compiled permissions boundary document");
}
