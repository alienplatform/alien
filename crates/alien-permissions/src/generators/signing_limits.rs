//! The AWS policies only a protected install has: the limits on its management role and the
//! verifier's own grants.
//!
//! The limits are deny statements in a managed policy `<prefix>-management-limits`, so they hold
//! even if another policy (an extended profile, a customer policy) grants more: only setup
//! changes what the install trusts, only `<prefix>-verifier` changes what runs, and only the
//! verifier launches machines, which boot only the publisher's images.
//!
//! Callers pass the prefix and account as they appear in the setup artifact
//! (`${AWS::StackName}` in CloudFormation, a Terraform interpolation in Terraform).

use serde_json::{json, Value};

use super::verifier_owned::AWS_VERIFIER_OWNED;

/// Lambda writes: what code a function runs, how, and who may call it.
fn lambda_writes() -> Vec<&'static str> {
    AWS_VERIFIER_OWNED
        .iter()
        .copied()
        .filter(|action| action.starts_with("lambda:"))
        .collect()
}

/// Launching machines and deciding what they boot.
fn machine_launches() -> Vec<&'static str> {
    AWS_VERIFIER_OWNED
        .iter()
        .copied()
        .filter(|action| !action.starts_with("lambda:"))
        .collect()
}

/// Running commands on a machine. Machines run the SSM agent with the managed instance core
/// policy, so any of these is root on them.
pub const AWS_MACHINE_COMMANDS: &[&str] = &[
    "ssm:SendCommand",
    "ssm:StartSession",
    "ssm:ResumeSession",
    "ssm:StartAutomationExecution",
    "ssm:StartChangeRequestExecution",
    "ssm:CreateAssociation",
    "ssm:CreateAssociationBatch",
    "ssm:UpdateAssociation",
    "ssm:StartAssociationsOnce",
    "ssm:RegisterTaskWithMaintenanceWindow",
    "ssm:UpdateMaintenanceWindowTask",
    "ssm:CreateDocument",
    "ssm:UpdateDocument",
    "ssm:UpdateDocumentDefaultVersion",
    "ec2-instance-connect:SendSSHPublicKey",
    "ec2-instance-connect:SendSerialConsoleSSHPublicKey",
];

/// Role writes that would let management become the verifier.
const ROLE_WRITES: &[&str] = &[
    "iam:DeleteRole",
    "iam:PutRolePolicy",
    "iam:DeleteRolePolicy",
    "iam:AttachRolePolicy",
    "iam:DetachRolePolicy",
    "iam:UpdateAssumeRolePolicy",
    "iam:PutRolePermissionsBoundary",
    "iam:DeleteRolePermissionsBoundary",
    "iam:UpdateRole",
    "iam:TagRole",
    "iam:UntagRole",
    "iam:PassRole",
];

/// Policy writes that would loosen the limits.
const POLICY_WRITES: &[&str] = &[
    "iam:CreatePolicyVersion",
    "iam:SetDefaultPolicyVersion",
    "iam:DeletePolicyVersion",
    "iam:DeletePolicy",
    "iam:TagPolicy",
    "iam:UntagPolicy",
];

/// Name of the managed policy holding the limits.
pub fn aws_management_limits_name(prefix: &str) -> String {
    format!("{prefix}-management-limits")
}

/// Name of the verifier's function and role.
pub fn aws_verifier_name(prefix: &str) -> String {
    format!("{prefix}-verifier")
}

/// The `<prefix>-management-limits` policy document.
pub fn aws_management_limits(prefix: &str, account: &str, region: &str) -> Value {
    let verifier = aws_verifier_name(prefix);
    json!({
        "Version": "2012-10-17",
        "Statement": [
            {
                "Sid": "OnlyVerifierChangesFunctions",
                "Effect": "Deny",
                "Action": lambda_writes(),
                "Resource": "*"
            },
            {
                "Sid": "OnlyVerifierLaunchesMachines",
                "Effect": "Deny",
                "Action": machine_launches(),
                "Resource": "*"
            },
            {
                "Sid": "OnlyVerifierPassesRolesToEc2",
                "Effect": "Deny",
                "Action": "iam:PassRole",
                "Resource": "*",
                "Condition": { "StringEquals": { "iam:PassedToService": "ec2.amazonaws.com" } }
            },
            {
                "Sid": "NoCommandsOnMachines",
                "Effect": "Deny",
                "Action": AWS_MACHINE_COMMANDS,
                "Resource": "*"
            },
            {
                "Sid": "OnlySetupChangesTheVerifier",
                "Effect": "Deny",
                "Action": ROLE_WRITES,
                "Resource": format!("arn:aws:iam::{account}:role/{verifier}")
            },
            {
                "Sid": "OnlySetupChangesTheseLimits",
                "Effect": "Deny",
                "Action": POLICY_WRITES,
                "Resource": format!(
                    "arn:aws:iam::{account}:policy/{}",
                    aws_management_limits_name(prefix)
                )
            },
            {
                "Sid": "CallTheVerifier",
                "Effect": "Allow",
                "Action": "lambda:InvokeFunction",
                "Resource": format!("arn:aws:lambda:{region}:{account}:function:{verifier}")
            }
        ]
    })
}

/// What the verifier may do: exactly the powers management lost, on the install's own
/// resources, plus the reads it needs. Machines it launches boot only `publisher`'s images.
pub fn aws_verifier_policy(prefix: &str, account: &str, region: &str, publisher: &str) -> Value {
    let verifier = aws_verifier_name(prefix);
    let own_tag = json!({ "aws:RequestTag/deployment": prefix });
    json!({
        "Version": "2012-10-17",
        "Statement": [
            {
                "Sid": "ReadSigningSettings",
                "Effect": "Allow",
                "Action": ["ssm:GetParameter", "ssm:GetParameters"],
                "Resource": format!("arn:aws:ssm:{region}:{account}:parameter/signed/*")
            },
            {
                "Sid": "RecordWhatThisInstallAccepted",
                "Effect": "Allow",
                "Action": "ssm:PutParameter",
                "Resource": format!("arn:aws:ssm:{region}:{account}:parameter/signed/{prefix}/state")
            },
            {
                "Sid": "ChangeThisInstallsFunctions",
                "Effect": "Allow",
                "Action": lambda_writes(),
                "Resource": [
                    format!("arn:aws:lambda:{region}:{account}:function:{prefix}-*"),
                    format!("arn:aws:lambda:{region}:{account}:function:{prefix}-*:*")
                ]
            },
            {
                "Sid": "VerifierNeverChangesItself",
                "Effect": "Deny",
                "Action": lambda_writes(),
                "Resource": [
                    format!("arn:aws:lambda:{region}:{account}:function:{verifier}"),
                    format!("arn:aws:lambda:{region}:{account}:function:{verifier}:*")
                ]
            },
            {
                "Sid": "ReadFunctionsAndMachines",
                "Effect": "Allow",
                "Action": [
                    "lambda:GetFunction",
                    "lambda:GetFunctionConfiguration",
                    "lambda:ListVersionsByFunction",
                    "lambda:ListAliases",
                    "ec2:DescribeLaunchTemplates",
                    "ec2:DescribeLaunchTemplateVersions",
                    "ec2:DescribeImages",
                    "ec2:DescribeInstances",
                    "ec2:DescribeSubnets",
                    "ec2:DescribeSecurityGroups",
                    "autoscaling:DescribeAutoScalingGroups",
                    "autoscaling:DescribeInstanceRefreshes",
                    "ecr:GetAuthorizationToken"
                ],
                "Resource": "*"
            },
            {
                "Sid": "PullWorkloadImages",
                "Effect": "Allow",
                "Action": ["ecr:BatchGetImage", "ecr:GetDownloadUrlForLayer"],
                "Resource": "*"
            },
            {
                "Sid": "PassOnlyThisInstallsRolesToFunctions",
                "Effect": "Allow",
                "Action": "iam:PassRole",
                "Resource": format!("arn:aws:iam::{account}:role/{prefix}-*"),
                "Condition": { "StringEquals": { "iam:PassedToService": "lambda.amazonaws.com" } }
            },
            {
                "Sid": "PassOnlyComputeRolesToEc2",
                "Effect": "Allow",
                "Action": "iam:PassRole",
                "Resource": format!("arn:aws:iam::{account}:role/{prefix}-*"),
                "Condition": { "StringEquals": { "iam:PassedToService": "ec2.amazonaws.com" } }
            },
            {
                "Sid": "NeverPassTheSetupRoles",
                "Effect": "Deny",
                "Action": "iam:PassRole",
                "Resource": [
                    format!("arn:aws:iam::{account}:role/{verifier}"),
                    format!("arn:aws:iam::{account}:role/{prefix}-management")
                ]
            },
            {
                "Sid": "CreateThisInstallsLaunchTemplates",
                "Effect": "Allow",
                "Action": ["ec2:CreateLaunchTemplate", "ec2:CreateTags"],
                "Resource": "*",
                "Condition": { "StringEquals": own_tag }
            },
            {
                "Sid": "ChangeThisInstallsLaunchTemplates",
                "Effect": "Allow",
                "Action": [
                    "ec2:CreateLaunchTemplateVersion",
                    "ec2:ModifyLaunchTemplate",
                    "ec2:DeleteLaunchTemplate"
                ],
                "Resource": format!("arn:aws:ec2:{region}:{account}:launch-template/*"),
                "Condition": { "StringEquals": { "aws:ResourceTag/deployment": prefix } }
            },
            {
                "Sid": "BootOnlyPublisherImages",
                "Effect": "Allow",
                "Action": "ec2:RunInstances",
                "Resource": format!("arn:aws:ec2:{region}::image/*"),
                "Condition": { "StringEquals": { "ec2:Owner": publisher } }
            },
            {
                "Sid": "OnlyVerifierLaunchesMachines",
                "Effect": "Allow",
                "Action": "ec2:RunInstances",
                "Resource": [
                    format!("arn:aws:ec2:{region}:{account}:subnet/*"),
                    format!("arn:aws:ec2:{region}:{account}:security-group/*"),
                    format!("arn:aws:ec2:{region}:{account}:network-interface/*"),
                    format!("arn:aws:ec2:{region}:{account}:volume/*"),
                    format!("arn:aws:ec2:{region}:{account}:instance/*"),
                    format!("arn:aws:ec2:{region}:{account}:launch-template/*")
                ]
            },
            {
                "Sid": "ChangeThisInstallsMachineGroups",
                "Effect": "Allow",
                "Action": [
                    "autoscaling:CreateAutoScalingGroup",
                    "autoscaling:UpdateAutoScalingGroup",
                    "autoscaling:StartInstanceRefresh",
                    "autoscaling:CreateOrUpdateTags"
                ],
                "Resource": format!(
                    "arn:aws:autoscaling:{region}:{account}:autoScalingGroup:*:autoScalingGroupName/{prefix}-*"
                )
            },
            {
                "Sid": "WriteOwnLogs",
                "Effect": "Allow",
                "Action": ["logs:CreateLogGroup", "logs:CreateLogStream", "logs:PutLogEvents"],
                "Resource": format!(
                    "arn:aws:logs:{region}:{account}:log-group:/aws/lambda/{verifier}:*"
                )
            }
        ]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limits_deny_everything_the_verifier_owns() {
        let limits = aws_management_limits("orders", "111122223333", "us-east-1");
        let denied: Vec<String> = limits["Statement"]
            .as_array()
            .expect("statements")
            .iter()
            .filter(|statement| statement["Effect"] == "Deny" && statement["Resource"] == "*")
            .flat_map(|statement| match &statement["Action"] {
                Value::String(action) => vec![action.clone()],
                Value::Array(actions) => actions
                    .iter()
                    .map(|action| action.as_str().expect("action").to_string())
                    .collect(),
                other => panic!("unexpected action {other}"),
            })
            .collect();
        for owned in AWS_VERIFIER_OWNED {
            assert!(denied.iter().any(|action| action == owned), "{owned}");
        }
        for command in AWS_MACHINE_COMMANDS {
            assert!(denied.iter().any(|action| action == command), "{command}");
        }
    }
}
