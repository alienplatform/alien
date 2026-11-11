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

use super::{families::aws_family, verifier_owned::AWS_VERIFIER_OWNED};

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

/// What the limits deny of `actions`: those in a family. Deletes that change no code stay with
/// management.
fn in_a_family(actions: Vec<&'static str>) -> Vec<&'static str> {
    actions
        .into_iter()
        .filter(|action| aws_family(action, None).is_some())
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

/// Starting or changing a microVM image runs whatever code the caller names.
pub const AWS_MICROVM_RUNS: &[&str] = &[
    "lambda:RunMicrovm",
    "lambda:ResumeMicrovm",
    "lambda:CreateMicrovmImage",
    "lambda:UpdateMicrovmImage",
];

/// A build project runs the steps and image it is given, as its service role.
pub const AWS_BUILD_RUNS: &[&str] = &[
    "codebuild:CreateProject",
    "codebuild:UpdateProject",
    "codebuild:StartBuild",
    "codebuild:StartBuildBatch",
    "codebuild:RetryBuild",
    "codebuild:RetryBuildBatch",
];

/// Changing what an install's role may do, or who may assume it, or removing it. Setup creates
/// the install's roles; on a protected install management doesn't change them, and can't strip a
/// grant setup gave one (such as a machine role's read of the install's signing settings).
pub const AWS_ROLE_POLICY_WRITES: &[&str] = &[
    "iam:PutRolePolicy",
    "iam:AttachRolePolicy",
    "iam:DeleteRolePolicy",
    "iam:DetachRolePolicy",
    "iam:DeleteRole",
    "iam:UpdateAssumeRolePolicy",
    "iam:PutRolePermissionsBoundary",
    "iam:DeleteRolePermissionsBoundary",
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

/// Writes that decide who can reach a bucket, topic or queue or where its events go. The
/// controllers send the ones they make for the install's Live resources to the verifier, which
/// checks them against the signed stack, so management holds none of them.
const AWS_SHARING_WRITES: &[&str] = &[
    "s3:PutBucketPolicy",
    "s3:PutBucketPublicAccessBlock",
    "s3:PutBucketNotification",
    "sqs:CreateQueue",
    "sqs:SetQueueAttributes",
    "lambda:CreateEventSourceMapping",
    "lambda:UpdateEventSourceMapping",
    "s3:PutBucketAcl",
    "s3:PutBucketOwnershipControls",
    "sns:AddPermission",
    "sns:CreateTopic",
    "sns:SetTopicAttributes",
    "sqs:AddPermission",
    "ssm:PutResourcePolicy",
];

/// Changing the value of an SSM parameter, which is where vault values live.
const AWS_PARAMETER_WRITES: &[&str] = &[
    "ssm:PutParameter",
    "ssm:DeleteParameter",
    "ssm:DeleteParameters",
    "ssm:LabelParameterVersion",
    "ssm:UnlabelParameterVersion",
];

/// The keys of the `secrets` vault the deployment sync writes or removes: the telemetry auth
/// headers and the command token. Each `?????` stands for the five-letter product prefix of
/// the key, which setup artifacts don't spell out.
const SECRETS_VAULT_KEYS: &[&str] = &[
    "__?????_runtime_otlp_logs_auth_header",
    "__?????_runtime_otlp_metrics_auth_header",
    "?????_COMMANDS_TOKEN",
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
                "Action": in_a_family(lambda_writes()),
                "Resource": "*"
            },
            {
                "Sid": "OnlyVerifierLaunchesMachines",
                "Effect": "Deny",
                "Action": in_a_family(machine_launches()),
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
                "Sid": "NoUnsignedMicrovms",
                "Effect": "Deny",
                "Action": AWS_MICROVM_RUNS,
                "Resource": "*"
            },
            {
                "Sid": "NoUnsignedBuilds",
                "Effect": "Deny",
                "Action": AWS_BUILD_RUNS,
                "Resource": "*"
            },
            {
                "Sid": "NoSharingInstallDataWithOutsiders",
                "Effect": "Deny",
                "Action": AWS_SHARING_WRITES,
                "Resource": "*"
            },
            {
                // Apps read their vault values, so management writes only the keys above.
                "Sid": "OnlyTheDeploymentSyncWritesVaultValues",
                "Effect": "Deny",
                "Action": AWS_PARAMETER_WRITES,
                "NotResource": SECRETS_VAULT_KEYS
                    .iter()
                    .map(|key| format!("arn:aws:ssm:{region}:{account}:parameter/{prefix}-secrets-{key}"))
                    .collect::<Vec<_>>()
            },
            {
                "Sid": "OnlySetupChangesWhatRolesMayDo",
                "Effect": "Deny",
                "Action": AWS_ROLE_POLICY_WRITES,
                "Resource": format!("arn:aws:iam::{account}:role/{prefix}-*")
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
            },
            {
                // Management refuses a deployment config without signing settings when it can
                // read these. Only protected installs grant the read.
                "Sid": "ReadThisInstallsSigningSettings",
                "Effect": "Allow",
                "Action": "ssm:GetParameter",
                "Resource": format!("arn:aws:ssm:{region}:{account}:parameter/signed/{prefix}/fingerprint")
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
                // Who may reach a storage's data from outside is the verifier's to write: it
                // checks every statement of a bucket policy against the signed stack.
                "Sid": "SetThisInstallsBucketPolicies",
                "Effect": "Allow",
                "Action": [
                    "s3:PutBucketPolicy",
                    "s3:PutBucketPublicAccessBlock",
                    "s3:PutBucketNotification",
                    "s3:GetBucketNotification"
                ],
                "Resource": format!("arn:aws:s3:::{prefix}-*")
            },
            {
                // Which queue feeds a worker is the verifier's to write: it checks the mapping
                // against the signed stack.
                "Sid": "MapThisInstallsEventSources",
                "Effect": "Allow",
                "Action": ["lambda:CreateEventSourceMapping", "lambda:UpdateEventSourceMapping"],
                "Resource": "*",
                "Condition": {
                    "StringLike": {
                        "lambda:FunctionArn":
                            format!("arn:aws:lambda:{region}:{account}:function:{prefix}-*")
                    }
                }
            },
            {
                // It reads a mapping before it changes it, to be sure it is the worker's own.
                "Sid": "ReadEventSourceMappings",
                "Effect": "Allow",
                "Action": "lambda:GetEventSourceMapping",
                "Resource": format!("arn:aws:lambda:{region}:{account}:event-source-mapping:*")
            },
            {
                // Who may send to a queue is the verifier's to write: it checks the policy
                // against the signed stack.
                "Sid": "SetThisInstallsQueues",
                "Effect": "Allow",
                "Action": ["sqs:CreateQueue", "sqs:SetQueueAttributes", "sqs:TagQueue"],
                "Resource": format!("arn:aws:sqs:{region}:{account}:{prefix}-*")
            },
            {
                // Uninstall deletes a Live workload through the verifier, which takes its
                // concurrency, URL, aliases and grants along.
                "Sid": "DeleteThisInstallsFunctions",
                "Effect": "Allow",
                "Action": "lambda:DeleteFunction",
                "Resource": [
                    format!("arn:aws:lambda:{region}:{account}:function:{prefix}-*")
                ]
            },
            {
                // A Live sandbox's image is built from a bundle the verifier renders from the
                // signed image, so the bundle prefix of the install's bundle bucket has no other
                // writer (its bucket policy denies the rest).
                "Sid": "WriteThisInstallsSandboxBundles",
                "Effect": "Allow",
                "Action": "s3:PutObject",
                "Resource": format!("arn:aws:s3:::{prefix}-sandbox-bundles/sandbox-bundle/*")
            },
            {
                // The build itself: the verifier checks the request against the signed sandbox.
                // Creation is authorized against no resource type, so the install's tags bound it.
                "Sid": "CreateThisInstallsSandboxImages",
                "Effect": "Allow",
                "Action": "lambda:CreateMicrovmImage",
                "Resource": "*",
                "Condition": {
                    "StringEquals": {
                        "aws:RequestTag/deployment": prefix,
                        "aws:RequestTag/managed-by": "runtime"
                    }
                }
            },
            {
                "Sid": "TagThisInstallsSandboxImages",
                "Effect": "Allow",
                "Action": "lambda:TagResource",
                "NotResource": "arn:*:lambda:*:*:*",
                "Condition": {
                    "StringEquals": {
                        "aws:RequestTag/deployment": prefix,
                        "aws:RequestTag/managed-by": "runtime"
                    }
                }
            },
            {
                "Sid": "RollThisInstallsSandboxImages",
                "Effect": "Allow",
                "Action": "lambda:UpdateMicrovmImage",
                "Resource": format!("arn:aws:lambda:{region}:{account}:microvm-image:{prefix}-*")
            },
            {
                "Sid": "PassThisInstallsBuildRoles",
                "Effect": "Allow",
                "Action": "iam:PassRole",
                "Resource": format!("arn:aws:iam::{account}:role/{prefix}-*-build"),
                "Condition": {
                    "StringEquals": { "iam:PassedToService": "lambda.amazonaws.com" }
                }
            },
            {
                // It creates the install's functions with the tags management's grants on
                // runtime resources are scoped to.
                "Sid": "TagThisInstallsFunctions",
                "Effect": "Allow",
                "Action": "lambda:TagResource",
                "Resource": [
                    format!("arn:aws:lambda:{region}:{account}:function:{prefix}-*")
                ]
            },
            {
                "Sid": "VerifierNeverChangesItself",
                "Effect": "Deny",
                "Action": lambda_writes().into_iter().chain(["lambda:DeleteFunction"]).collect::<Vec<_>>(),
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
                    "lambda:GetFunctionUrlConfig",
                    "lambda:ListVersionsByFunction",
                    "lambda:ListAliases",
                    // It keeps a function's resource policy to what the signed stack grants.
                    "lambda:GetPolicy",
                    "ec2:DescribeLaunchTemplates",
                    "ec2:DescribeLaunchTemplateVersions",
                    "ec2:DescribeImages",
                    "ec2:DescribeInstances",
                    "ec2:DescribeSubnets",
                    "ec2:DescribeSecurityGroups",
                    // Lambda checks a function's VPC as its caller.
                    "ec2:DescribeVpcs",
                    "autoscaling:DescribeAutoScalingGroups",
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
                "Sid": "CreateThisInstallsMachineGroups",
                "Effect": "Allow",
                "Action": "autoscaling:CreateAutoScalingGroup",
                "Resource": format!(
                    "arn:aws:autoscaling:{region}:{account}:autoScalingGroup:*:autoScalingGroupName/{prefix}-*"
                ),
                "Condition": { "StringEquals": own_tag }
            },
            {
                // A group already tagged for another install is not retagged into this one.
                "Sid": "TagThisInstallsMachineGroups",
                "Effect": "Allow",
                "Action": "autoscaling:CreateOrUpdateTags",
                "Resource": format!(
                    "arn:aws:autoscaling:{region}:{account}:autoScalingGroup:*:autoScalingGroupName/{prefix}-*"
                ),
                "Condition": {
                    "StringEquals": own_tag,
                    "StringEqualsIfExists": { "aws:ResourceTag/deployment": prefix }
                }
            },
            {
                // Only groups tagged for this install, so the name prefix alone cannot reach
                // another install's group (`acme` against `acme-prod-compute-general`).
                "Sid": "ChangeThisInstallsMachineGroups",
                "Effect": "Allow",
                "Action": [
                    "autoscaling:UpdateAutoScalingGroup",
                    // Uninstall deletes a group with its machines through the verifier.
                    "autoscaling:DeleteAutoScalingGroup"
                ],
                "Resource": format!(
                    "arn:aws:autoscaling:{region}:{account}:autoScalingGroup:*:autoScalingGroupName/{prefix}-*"
                ),
                "Condition": { "StringEquals": { "aws:ResourceTag/deployment": prefix } }
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

    /// Whether IAM pattern `pattern` (with `*` wildcards) matches `value`.
    fn matches(pattern: &str, value: &str) -> bool {
        let (pattern, value): (Vec<char>, Vec<char>) = (
            pattern.to_ascii_lowercase().chars().collect(),
            value.to_ascii_lowercase().chars().collect(),
        );
        glob(&pattern, &value)
    }

    /// IAM wildcards: `*` is any run of characters, `?` exactly one.
    fn glob(pattern: &[char], value: &[char]) -> bool {
        match pattern.split_first() {
            None => value.is_empty(),
            Some(('*', rest)) => (0..=value.len()).any(|skip| glob(rest, &value[skip..])),
            Some(('?', rest)) => !value.is_empty() && glob(rest, &value[1..]),
            Some((ch, rest)) => value.first() == Some(ch) && glob(rest, &value[1..]),
        }
    }

    fn strings(value: &Value) -> Vec<String> {
        match value {
            Value::String(text) => vec![text.clone()],
            Value::Array(items) => items.iter().flat_map(strings).collect(),
            _ => Vec::new(),
        }
    }

    /// Whether an unconditional Deny in `policy` covers `action` on `resource`.
    fn denies(policy: &Value, action: &str, resource: &str) -> bool {
        policy["Statement"]
            .as_array()
            .expect("statements")
            .iter()
            .any(|statement| {
                statement["Effect"] == "Deny"
                    && statement.get("Condition").is_none()
                    && strings(&statement["Action"])
                        .iter()
                        .any(|pattern| matches(pattern, action))
                    && covers(statement, resource)
            })
    }

    /// Whether the statement's `Resource` matches `resource`, or its `NotResource` does not.
    fn covers(statement: &Value, resource: &str) -> bool {
        if statement.get("NotResource").is_some() {
            return !strings(&statement["NotResource"])
                .iter()
                .any(|pattern| matches(pattern, resource));
        }
        strings(&statement["Resource"])
            .iter()
            .any(|pattern| matches(pattern, resource))
    }

    /// Whether an unconditional Allow in `policy` covers `action` on `resource` and no Deny does.
    fn allows(policy: &Value, action: &str, resource: &str) -> bool {
        !denies(policy, action, resource)
            && policy["Statement"]
                .as_array()
                .expect("statements")
                .iter()
                .any(|statement| {
                    statement["Effect"] == "Allow"
                        && statement.get("Condition").is_none()
                        && strings(&statement["Action"])
                            .iter()
                            .any(|pattern| matches(pattern, action))
                        && strings(&statement["Resource"])
                            .iter()
                            .any(|pattern| matches(pattern, resource))
                })
    }

    /// Lambda checks the subnets, security groups and VPC of a function's `VpcConfig` as its
    /// caller (the Lambda developer guide, "Required IAM permissions" for VPC access), so the
    /// verifier keeps the install's functions in its network only with these reads.
    #[test]
    fn the_verifier_may_put_functions_in_the_install_network() {
        let policy = aws_verifier_policy("orders", "111122223333", "us-east-1", "444455556666");
        for action in [
            "ec2:DescribeSecurityGroups",
            "ec2:DescribeSubnets",
            "ec2:DescribeVpcs",
        ] {
            assert!(allows(&policy, action, "*"), "{action}");
        }
        assert!(allows(
            &policy,
            "lambda:UpdateFunctionConfiguration",
            "arn:aws:lambda:us-east-1:111122223333:function:orders-api"
        ));
    }

    /// Uninstall deletes the install's Live functions through the verifier; the verifier never
    /// deletes itself or another install's function.
    #[test]
    fn the_verifier_deletes_the_installs_functions_but_never_itself() {
        let policy = aws_verifier_policy("orders", "111122223333", "us-east-1", "444455556666");
        let function =
            |name: &str| format!("arn:aws:lambda:us-east-1:111122223333:function:{name}");
        assert!(allows(
            &policy,
            "lambda:DeleteFunction",
            &function("orders-api")
        ));
        assert!(denies(
            &policy,
            "lambda:DeleteFunction",
            &function("orders-verifier")
        ));
        assert!(!allows(
            &policy,
            "lambda:DeleteFunction",
            &function("billing-api")
        ));
    }

    #[test]
    fn management_can_neither_change_nor_strip_nor_delete_an_install_role() {
        let limits = aws_management_limits("orders", "111122223333", "us-east-1");
        for role in ["orders-machine", "orders-execution-sa"] {
            let arn = format!("arn:aws:iam::111122223333:role/{role}");
            for action in [
                "iam:PutRolePolicy",
                "iam:AttachRolePolicy",
                "iam:DeleteRolePolicy",
                "iam:DetachRolePolicy",
                "iam:DeleteRole",
                "iam:UpdateAssumeRolePolicy",
            ] {
                assert!(denies(&limits, action, &arn), "{action} on {arn}");
            }
        }
        // Another install's roles are not this policy's business.
        assert!(!denies(
            &limits,
            "iam:DeleteRole",
            "arn:aws:iam::111122223333:role/billing-machine"
        ));
    }

    /// Management never makes these calls (there is no client for them), so the limits take
    /// them away: each opens a bucket, a topic or a queue to another account.
    #[test]
    fn management_cannot_open_the_installs_data_to_outsiders() {
        let limits = aws_management_limits("acme", "111122223333", "us-east-1");
        for (action, resource) in [
            ("s3:PutBucketAcl", "arn:aws:s3:::acme-uploads"),
            ("s3:PutBucketOwnershipControls", "arn:aws:s3:::acme-uploads"),
            (
                "sns:SetTopicAttributes",
                "arn:aws:sns:us-east-1:111122223333:acme-events",
            ),
            (
                "sns:AddPermission",
                "arn:aws:sns:us-east-1:111122223333:acme-events",
            ),
            (
                "sqs:AddPermission",
                "arn:aws:sqs:us-east-1:111122223333:acme-jobs",
            ),
            (
                "ssm:PutResourcePolicy",
                "arn:aws:ssm:us-east-1:111122223333:parameter/acme-secrets-DB_URL",
            ),
        ] {
            assert!(denies(&limits, action, resource), "{action} on {resource}");
        }
    }

    /// The controllers send these for the install's Live buckets, queues and workers to the
    /// verifier, so management holds none of them.
    #[test]
    fn management_cannot_write_what_the_verifier_writes_for_live_resources() {
        let limits = aws_management_limits("acme", "111122223333", "us-east-1");
        for (action, resource) in [
            ("s3:PutBucketPolicy", "arn:aws:s3:::acme-uploads"),
            ("s3:PutBucketPublicAccessBlock", "arn:aws:s3:::acme-uploads"),
            ("s3:PutBucketNotification", "arn:aws:s3:::acme-uploads"),
            (
                "sqs:CreateQueue",
                "arn:aws:sqs:us-east-1:111122223333:acme-jobs",
            ),
            (
                "sqs:SetQueueAttributes",
                "arn:aws:sqs:us-east-1:111122223333:acme-jobs",
            ),
            ("lambda:CreateEventSourceMapping", "*"),
            ("lambda:UpdateEventSourceMapping", "*"),
        ] {
            assert!(denies(&limits, action, resource), "{action} on {resource}");
        }
        // Removing a mapping changes no reach.
        assert!(!denies(&limits, "lambda:DeleteEventSourceMapping", "*"));
    }

    #[test]
    fn management_cannot_overwrite_what_an_app_reads_from_its_vaults() {
        let limits = aws_management_limits("acme", "111122223333", "us-east-1");
        let parameter = |name: &str| format!("arn:aws:ssm:us-east-1:111122223333:parameter/{name}");
        for action in [
            "ssm:PutParameter",
            "ssm:DeleteParameter",
            "ssm:DeleteParameters",
            "ssm:LabelParameterVersion",
            "ssm:UnlabelParameterVersion",
        ] {
            for name in [
                "acme-config-API_URL",
                "acme-deployer-secrets-WEBHOOK_URL",
                "acme-secrets-DATABASE_URL",
            ] {
                assert!(denies(&limits, action, &parameter(name)), "{action} {name}");
            }
        }
    }

    /// What the deployment sync writes to the `secrets` vault: the telemetry headers, and the
    /// command token it removes.
    #[test]
    fn management_still_syncs_the_runtime_credentials_to_the_secrets_vault() {
        let limits = aws_management_limits("acme", "111122223333", "us-east-1");
        for name in [
            "acme-secrets-__alien_runtime_otlp_logs_auth_header",
            "acme-secrets-__alien_runtime_otlp_metrics_auth_header",
            "acme-secrets-ALIEN_COMMANDS_TOKEN",
        ] {
            let arn = format!("arn:aws:ssm:us-east-1:111122223333:parameter/{name}");
            for action in ["ssm:PutParameter", "ssm:DeleteParameter"] {
                assert!(!denies(&limits, action, &arn), "{action} {name}");
            }
        }
    }

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
        for owned in AWS_VERIFIER_OWNED
            .iter()
            .filter(|owned| aws_family(owned, None).is_some())
        {
            assert!(denied.iter().any(|action| action == owned), "{owned}");
        }
        for action in AWS_MACHINE_COMMANDS
            .iter()
            .chain(AWS_MICROVM_RUNS)
            .chain(AWS_BUILD_RUNS)
        {
            assert!(denied.iter().any(|denied| denied == action), "{action}");
        }
    }

    #[test]
    fn every_run_or_reach_deny_is_in_a_family() {
        use super::super::families::aws_family;
        let limits = aws_management_limits("orders", "111122223333", "us-east-1");
        for statement in limits["Statement"].as_array().expect("statements") {
            let sid = statement["Sid"].as_str().expect("sid");
            if !matches!(
                sid,
                "OnlyVerifierChangesFunctions"
                    | "OnlyVerifierLaunchesMachines"
                    | "NoCommandsOnMachines"
                    | "NoUnsignedMicrovms"
                    | "NoUnsignedBuilds"
                    | "NoSharingInstallDataWithOutsiders"
            ) {
                continue;
            }
            let actions: Vec<&str> = match &statement["Action"] {
                Value::Array(actions) => actions.iter().filter_map(Value::as_str).collect(),
                other => panic!("{sid}: unexpected action {other}"),
            };
            for action in actions {
                assert!(
                    aws_family(action, None).is_some(),
                    "{sid} denies {action}, which no family covers: the list and the families disagree"
                );
            }
        }
    }
}
