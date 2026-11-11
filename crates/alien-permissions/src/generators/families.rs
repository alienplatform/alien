//! The families of cloud actions a protected install's management identity never holds.
//!
//! A protected install runs only what the vendor signed. Three families of action decide that, and
//! the management identity holds none of them, whatever the stack asks for:
//!
//! - RUN: starting or changing code that runs in the customer's environment;
//! - IDENTITY: creating, assuming or changing an identity that could do so;
//! - REACH: letting a principal or network outside the install read, write or connect to it.
//!
//! Each cloud defines the families as predicates over action patterns. A pattern is in a family
//! when it can match a family action, so a wildcard is refused unless every action it matches is
//! outside every family.


/// The family an action belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    /// Runs code the vendor didn't sign.
    Run,
    /// Creates or changes an identity that could run code.
    Identity,
    /// Opens the install's data or services to a principal or network outside it.
    Reach,
}

impl Family {
    /// The family's name as an error shows it.
    pub fn name(self) -> &'static str {
        match self {
            Self::Run => "RUN",
            Self::Identity => "IDENTITY",
            Self::Reach => "REACH",
        }
    }
}

/// An AWS service whose every write is in `family`, with the verbs the service keeps for
/// management.
struct AwsService {
    service: &'static str,
    family: Family,
    /// Verb patterns that are not in the family although the service is: parameters of SSM,
    /// invoking a function, and the like. Each has a reason in the source.
    kept: &'static [&'static str],
}

const AWS_SERVICES: &[AwsService] = &[
    AwsService {
        service: "lambda",
        family: Family::Run,
        // Calling a released function and deleting one change no code.
        kept: &["InvokeFunction", "DeleteFunction"],
    },
    AwsService {
        service: "autoscaling",
        family: Family::Run,
        // Capacity and termination change how many machines run, not what they run.
        kept: &["SetDesiredCapacity", "TerminateInstanceInAutoScalingGroup"],
    },
    AwsService {
        service: "codebuild",
        family: Family::Run,
        kept: &[],
    },
    AwsService {
        service: "ssm",
        family: Family::Run,
        // Parameters are data; documents, commands and sessions run code.
        kept: &[
            "PutParameter",
            "DeleteParameter",
            "DeleteParameters",
            "LabelParameterVersion",
            "UnlabelParameterVersion",
            "AddTagsToResource",
            "RemoveTagsFromResource",
        ],
    },
    AwsService {
        service: "ec2-instance-connect",
        family: Family::Run,
        kept: &[],
    },
    AwsService {
        service: "cloudformation",
        family: Family::Run,
        kept: &[],
    },
    AwsService {
        service: "iam",
        family: Family::Identity,
        // A service-linked role is created and owned by the AWS service that needs it.
        kept: &["CreateServiceLinkedRole"],
    },
    AwsService {
        service: "sts",
        family: Family::Identity,
        kept: &[],
    },
];

/// EC2 actions in the RUN family: what boots, what a machine's disks hold and what a machine
/// runs as.
const AWS_EC2_RUN: &[&str] = &[
    "ec2:RunInstances",
    "ec2:CreateLaunchTemplate",
    "ec2:CreateLaunchTemplateVersion",
    "ec2:ModifyLaunchTemplate",
    "ec2:DeleteLaunchTemplate*",
    "ec2:CreateFleet",
    "ec2:ModifyFleet",
    "ec2:DeleteFleets",
    "ec2:RequestSpot*",
    "ec2:ModifySpot*",
    "ec2:CancelSpot*",
    "ec2:ModifyInstanceAttribute",
    "ec2:AttachVolume",
    "ec2:DetachVolume",
    "ec2:CreateReplaceRootVolumeTask",
    "ec2:*IamInstanceProfile*",
];

/// Destinations and ingress in the REACH family, besides the resource-policy and sharing writes in
/// [`AWS_SHARING`]: where the install's events, messages and network traffic can come from or go.
const AWS_REACH: &[&str] = &[
    "s3:PutBucketNotification*",
    "sns:Subscribe",
    "sns:ConfirmSubscription",
    "lambda:CreateEventSourceMapping",
    "lambda:UpdateEventSourceMapping",
    "events:PutTargets",
    "events:RemoveTargets",
    "ses:CreateReceiptRule*",
    "ses:UpdateReceiptRule*",
    "ses:CreateConfigurationSetEventDestination",
    "ses:UpdateConfigurationSetEventDestination",
    "ec2:AuthorizeSecurityGroupIngress",
    "ec2:ModifySecurityGroupRules",
    "rds:CreateDBInstance",
    "rds:CreateDBCluster",
    "rds:ModifyDBInstance",
    "rds:ModifyDBCluster",
    "rds:RestoreDB*",
];

/// Resource-policy and sharing writes that could expose stack resources outside the account or to
/// the public, including creates that accept a resource policy. Runtime catalog grants never need
/// them. An inline application grant that names one of these exactly lifts the Deny only on that
/// grant's own resources; entries with wildcards, which keep the boundary within IAM's size limit,
/// are never lifted.
pub(super) const AWS_SHARING: &[&str] = &[
    "aoss:*AccessPolicy",
    "aoss:*SecurityPolicy",
    "backup:PutBackupVaultAccessPolicy",
    "codebuild:PutResourcePolicy",
    "dlm:*LifecyclePolicy",
    "dynamodb:PutResourcePolicy",
    "ec2:CopySnapshot",
    "ec2:CreateVpcPeeringConnection",
    "ec2:Modify*ImageAttribute",
    "ec2:ModifySnapshotAttribute",
    "ec2:ModifyVpcEndpointServicePermissions",
    "ecr:*RepositoryCreationTemplate",
    "ecr:PutRegistryPolicy",
    "ecr:PutReplicationConfiguration",
    "ecr:SetRepositoryPolicy",
    "elasticfilesystem:PutFileSystemPolicy",
    "es:Create*Domain",
    "es:Update*DomainConfig",
    "events:PutPermission",
    "glacier:SetVaultAccessPolicy",
    "glue:PutResourcePolicy",
    "kinesis:PutResourcePolicy",
    "kms:PutKeyPolicy",
    "lambda:Add*Permission",
    "lambda:CreateFunctionUrlConfig",
    "lambda:UpdateFunctionUrlConfig",
    "logs:PutDe*Policy",
    "logs:PutResourcePolicy",
    "logs:PutSubscriptionFilter",
    "ram:*",
    "rds:ModifyDB*SnapshotAttribute",
    "s3:*BucketPolicy",
    "s3:Put*PublicAccessBlock",
    "s3:PutAccessPointPolicy*",
    "s3:PutBucketAcl",
    "s3:PutBucketOwnershipControls",
    "s3:PutMultiRegionAccessPointPolicy",
    "s3:PutReplicationConfiguration",
    "secretsmanager:PutResourcePolicy",
    "ses:*IdentityPolicy",
    "sns:AddPermission",
    "sns:CreateTopic",
    "sns:SetTopicAttributes",
    "sqs:AddPermission",
    "sqs:CreateQueue",
    "sqs:SetQueueAttributes",
];

/// Verb prefixes of actions that only read.
const AWS_READ_VERBS: &[&str] = &["get", "list", "describe", "batchget", "simulate"];

/// The services a role may be passed to without it running code. Every other service, or a
/// `PassRole` without a service condition, is IDENTITY.
const AWS_PASS_ROLE_NON_CODE_SERVICES: &[&str] = &["dlm.amazonaws.com", "vpc-flow-logs.amazonaws.com"];

fn is_read_verb_pattern(verb: &str) -> bool {
    let verb = verb.to_ascii_lowercase();
    AWS_READ_VERBS.iter().any(|read| verb.starts_with(read))
}

/// Whether some string matches both glob patterns, with `*` for any run and `?` for one
/// character, ignoring case.
pub fn glob_patterns_overlap(a: &str, b: &str) -> bool {
    fn overlap(a: &[u8], b: &[u8]) -> bool {
        match (a.first(), b.first()) {
            (None, None) => true,
            (Some(b'*'), _) | (_, Some(b'*')) => {
                (a.first() == Some(&b'*')
                    && (overlap(&a[1..], b) || (!b.is_empty() && overlap(a, &b[1..]))))
                    || (b.first() == Some(&b'*')
                        && (overlap(a, &b[1..]) || (!a.is_empty() && overlap(&a[1..], b))))
            }
            (None, _) | (_, None) => false,
            (Some(x), Some(y)) => (*x == b'?' || *y == b'?' || x == y) && overlap(&a[1..], &b[1..]),
        }
    }
    overlap(
        a.to_ascii_lowercase().as_bytes(),
        b.to_ascii_lowercase().as_bytes(),
    )
}

/// The family of the AWS action or action pattern, if it can match an action in one.
///
/// `pass_role_services` is the `iam:PassedToService` condition of the grant the action is in, if
/// it has one. Without it, `iam:PassRole` is IDENTITY.
pub fn aws_family(action: &str, pass_role_services: Option<&[String]>) -> Option<Family> {
    let (service, verb) = action.split_once(':').unwrap_or((action, "*"));
    if action != "*" && is_read_verb_pattern(verb) && !verb.eq_ignore_ascii_case("PassRole") {
        return None;
    }
    if action == "*" || service == "*" {
        return Some(Family::Run);
    }
    if verb.eq_ignore_ascii_case("PassRole") {
        let code_free = pass_role_services.is_some_and(|services| {
            !services.is_empty()
                && services.iter().all(|service| {
                    AWS_PASS_ROLE_NON_CODE_SERVICES
                        .iter()
                        .any(|non_code| non_code.eq_ignore_ascii_case(service))
                })
        });
        return (!code_free).then_some(Family::Identity);
    }
    // A pattern that starts with a read verb matches only reads, in every service.
    if is_read_verb_pattern(verb) {
        return None;
    }
    for entry in AWS_SERVICES {
        if !glob_patterns_overlap(service, entry.service) {
            continue;
        }
        if entry
            .kept
            .iter()
            .any(|kept| kept.eq_ignore_ascii_case(verb))
        {
            continue;
        }
        return Some(entry.family);
    }
    if AWS_EC2_RUN
        .iter()
        .any(|run| glob_patterns_overlap(run, action))
    {
        return Some(Family::Run);
    }
    if AWS_SHARING
        .iter()
        .chain(AWS_REACH)
        .any(|reach| glob_patterns_overlap(reach, action))
    {
        return Some(Family::Reach);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aws_family_names_the_family_of_each_action() {
        for (action, family) in [
            ("lambda:UpdateFunctionCode", Family::Run),
            ("lambda:CreateEventSourceMapping", Family::Run),
            ("ssm:SendCommand", Family::Run),
            ("ec2:RunInstances", Family::Run),
            ("ec2:AssociateIamInstanceProfile", Family::Run),
            ("cloudformation:CreateStack", Family::Run),
            ("iam:CreateRole", Family::Identity),
            ("iam:PutRolePolicy", Family::Identity),
            ("sts:AssumeRole", Family::Identity),
            ("iam:PassRole", Family::Identity),
            ("s3:PutBucketPolicy", Family::Reach),
            ("sns:Subscribe", Family::Reach),
            ("ec2:AuthorizeSecurityGroupIngress", Family::Reach),
            ("rds:ModifyDBInstance", Family::Reach),
            // A pattern is in a family when it can match one of its actions.
            ("lambda:*", Family::Run),
            ("iam:*", Family::Identity),
            ("*", Family::Run),
            ("*:*", Family::Run),
            ("s3:*", Family::Reach),
            ("s3:Put*", Family::Reach),
        ] {
            assert_eq!(aws_family(action, None), Some(family), "{action}");
        }
    }

    #[test]
    fn aws_family_leaves_reads_data_and_kept_actions_alone() {
        for action in [
            "lambda:GetFunction",
            "lambda:InvokeFunction",
            "lambda:DeleteFunction",
            "iam:GetRole",
            "iam:ListRoles",
            "iam:CreateServiceLinkedRole",
            "sts:GetCallerIdentity",
            "ssm:GetParameter",
            "ssm:PutParameter",
            "s3:GetObject",
            "s3:PutObject",
            "dynamodb:PutItem",
            "sqs:SendMessage",
            "*:Get*",
            "*:Describe*",
            "ec2:Describe*",
            "autoscaling:SetDesiredCapacity",
            "autoscaling:Describe*",
        ] {
            assert_eq!(aws_family(action, None), None, "{action}");
        }
    }

    #[test]
    fn passing_a_role_is_identity_unless_it_goes_to_a_service_that_runs_no_code() {
        let to = |service: &str| vec![service.to_string()];
        assert_eq!(
            aws_family("iam:PassRole", Some(&to("lambda.amazonaws.com"))),
            Some(Family::Identity)
        );
        assert_eq!(
            aws_family("iam:PassRole", Some(&to("ec2.amazonaws.com"))),
            Some(Family::Identity)
        );
        assert_eq!(
            aws_family("iam:PassRole", Some(&to("dlm.amazonaws.com"))),
            None
        );
    }
}
