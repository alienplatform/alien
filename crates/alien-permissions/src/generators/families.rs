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
    "ec2:CreateFleet",
    "ec2:ModifyFleet",
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
    // S3 requires these to create a bucket whose object ownership isn't `BucketOwnerEnforced` or
    // that carries an ACL, so management can't create a bucket with object ACLs on. That is also
    // why `s3:PutObjectAcl` is not here: it is inert under `BucketOwnerEnforced`, and the app's
    // own storage grants hold it.
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

/// Deletes are out of scope (a hacked control plane can delete workloads and data), except the ones
/// that change what runs or how it is limited rather than stopping it:
/// - `lambda:DeleteAlias` makes an alias fall back to another version;
/// - deleting a function's code-signing configuration removes the check on its code;
/// - every IAM delete, because deleting a Deny, a boundary or a policy widens an identity.
const AWS_DELETES_IN_FAMILY: &[&str] = &[
    "lambda:DeleteAlias",
    "lambda:DeleteFunctionCodeSigningConfig",
    "lambda:DeleteCodeSigningConfig",
    "iam:Delete*",
];

/// Verb prefixes of actions that only read.
const AWS_READ_VERBS: &[&str] = &["get", "list", "describe", "batchget", "simulate"];

/// The services a role may be passed to without it running code. Every other service, or a
/// `PassRole` without a service condition, is IDENTITY.
const AWS_PASS_ROLE_NON_CODE_SERVICES: &[&str] =
    &["dlm.amazonaws.com", "vpc-flow-logs.amazonaws.com"];

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
    let is_delete = verb.to_ascii_lowercase().starts_with("delete");
    let delete_in_family = AWS_DELETES_IN_FAMILY
        .iter()
        .any(|delete| glob_patterns_overlap(delete, action));
    if is_delete && !delete_in_family {
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

/// Whether the GCP permission verb only reads.
fn gcp_read_verb(verb: &str) -> bool {
    verb.starts_with("get") || verb.starts_with("list")
}

/// GCP services whose every write but a delete is RUN: a service's write decides what code runs.
const GCP_RUN_PREFIXES: &[&str] = &[
    "run.services.",
    "run.jobs.",
    "cloudbuild.builds.",
    "aiplatform.reasoningEngines.",
    "aiplatform.sandboxEnvironments.",
    "aiplatform.sandboxEnvironmentTemplates.",
    "aiplatform.sandboxEnvironmentSnapshots.",
    "osconfig.",
    "oslogin.",
    "compute.instances.",
    "compute.instanceTemplates.",
    "compute.regionInstanceTemplates.",
    "compute.instanceGroupManagers.",
    "compute.regionInstanceGroupManagers.",
    "compute.projects.set",
];

/// Writes under a RUN prefix the management identity keeps: starting a job as it is defined takes
/// no overrides.
const GCP_RUN_KEPT: &[&str] = &["run.jobs.run"];

/// Writes under a REACH prefix: who may read, write or connect to the install.
const GCP_REACH_PREFIXES: &[&str] = &[
    "iam.roles.",
    "storage.hmacKeys.",
    "pubsub.subscriptions.create",
    "pubsub.subscriptions.update",
    "pubsub.topics.attachSubscription",
    // A schedule job decides what is called, and with an OIDC token as whom.
    "cloudscheduler.jobs.create",
    "cloudscheduler.jobs.update",
    "cloudsql.instances.create",
    "cloudsql.instances.update",
    "cloudsql.instances.restore",
    "cloudsql.instances.clone",
    // Not listed on purpose: `compute.addresses.*` and `compute.forwardingRules.*`. A Cloud SQL
    // private service connect endpoint is an internal address in the install's subnet plus a
    // forwarding rule to the instance's service attachment. That opens nothing from outside,
    // because the attachment admits only the install's own project (pinned by
    // `allowedConsumerProjects` on the instance). Forwarding rules in general carry
    // load-balancer traffic, which is out of scope for these families.
];

/// The family of the GCP permission, if it belongs to one.
///
/// A permission containing `*` is judged as the widest thing it could match, so it is in a
/// family unless it starts with a read verb.
pub fn gcp_family(permission: &str) -> Option<Family> {
    let (resource, verb) = permission.rsplit_once('.').unwrap_or(("", permission));
    // Minting a token for a service account is named like a read.
    let mints_token = matches!(
        permission,
        "iam.serviceAccounts.getAccessToken" | "iam.serviceAccounts.getOpenIdToken"
    );
    if gcp_read_verb(verb) && !permission.contains('*') && !mints_token {
        return None;
    }
    if permission.contains('*') {
        return Some(Family::Run);
    }
    // Deleting is out of scope on every family.
    if verb == "delete" {
        return None;
    }
    if verb == "setIamPolicy" || verb == "useReadOnly" {
        return Some(Family::Reach);
    }
    if GCP_REACH_PREFIXES
        .iter()
        .any(|prefix| permission.starts_with(prefix))
    {
        return Some(Family::Reach);
    }
    if resource.starts_with("iam.") {
        // Service accounts are created, updated and deleted by management: an identity with no
        // grant has no power. Everything else on `iam.` changes who can act as whom.
        let service_account_lifecycle = matches!(
            permission,
            "iam.serviceAccounts.create"
                | "iam.serviceAccounts.update"
                | "iam.serviceAccounts.delete"
                | "iam.serviceAccounts.enable"
                | "iam.serviceAccounts.disable"
                | "iam.serviceAccounts.undelete"
        );
        return (!service_account_lifecycle).then_some(Family::Identity);
    }
    if GCP_RUN_KEPT.contains(&permission) {
        return None;
    }
    if GCP_RUN_PREFIXES
        .iter()
        .any(|prefix| permission.starts_with(prefix))
    {
        return Some(Family::Run);
    }
    None
}

/// Azure control-plane paths whose non-read actions are RUN.
const AZURE_RUN_PATHS: &[&str] = &[
    "microsoft.app/",
    "microsoft.compute/",
    "microsoft.containerinstance/",
    "microsoft.containerservice/",
    "microsoft.web/",
    "microsoft.containerregistry/registries/agentpools/",
    "microsoft.containerregistry/registries/runs/",
    "microsoft.containerregistry/registries/taskruns/",
    "microsoft.containerregistry/registries/tasks/",
    "microsoft.resources/deployments/",
    "microsoft.resources/tags/",
];

/// Azure control-plane paths whose non-read actions are IDENTITY.
const AZURE_IDENTITY_PATHS: &[&str] = &["microsoft.managedidentity/", "microsoft.authorization/"];

/// Azure actions and paths that are REACH: keys and tokens that read or write the install's data
/// from outside, network rules, access policies and the writes that carry a public-access field.
/// A whole write of a storage account, container or registry is the verifier's, because the
/// public-access field is part of it.
const AZURE_REACH_FRAGMENTS: &[&str] = &[
    "/listkeys/",
    "/listkeys",
    "/listaccountsas/",
    "/listservicesas/",
    "/listcredentials/",
    "/listcredentials",
    "/regeneratekey/",
    "/regeneratekeys/",
    "/regenerate",
    "/begingetaccess/",
    "/authorizationrules/",
    "/accesspolicies/",
    "/virtualnetworkpeerings/",
    "/privateendpointconnections/",
    "/networksecuritygroups/",
    "/securityrules/",
    "/eventsubscriptions/",
];

/// Whole-resource writes that carry a public-access field.
const AZURE_REACH_WRITES: &[&str] = &[
    "microsoft.storage/storageaccounts/write",
    "microsoft.storage/storageaccounts/blobservices/containers/write",
    "microsoft.containerregistry/registries/write",
    "microsoft.keyvault/vaults/write",
];

/// The concrete Azure actions the REACH fragments above name for network resources. A predefined
/// role that stands for a wildcard such as `Microsoft.Network/*` is rendered with these in
/// `NotActions`, so the role keeps everything else.
const AZURE_REACH_NETWORK_ACTIONS: &[&str] = &[
    "Microsoft.Network/networkSecurityGroups/write",
    "Microsoft.Network/networkSecurityGroups/join/action",
    "Microsoft.Network/networkSecurityGroups/securityRules/write",
    "Microsoft.Network/virtualNetworks/virtualNetworkPeerings/write",
    "Microsoft.Network/privateLinkServices/privateEndpointConnections/write",
];

/// The REACH actions a wildcard Azure action would grant that a role can exclude with
/// `NotActions`, or none when the pattern reaches a family it can't exclude.
pub fn azure_reach_exclusions(pattern: &str) -> Vec<String> {
    AZURE_REACH_NETWORK_ACTIONS
        .iter()
        .filter(|action| glob_patterns_overlap(pattern, action))
        .map(|action| action.to_string())
        .collect()
}

/// The family of the Azure control-plane action or action pattern, if it is in one.
///
/// A wildcard is in a family when it can match an action in one, unless it ends in `/read`.
pub fn azure_family(action: &str) -> Option<Family> {
    let action = action.to_ascii_lowercase();
    if action.ends_with("/read") && !action.starts_with('*') || action == "*/read" {
        return None;
    }
    if action.starts_with('*') {
        return Some(Family::Run);
    }
    let is_delete = action.ends_with("/delete");
    let in_paths = |paths: &[&str]| {
        paths
            .iter()
            .any(|path| glob_patterns_overlap(&format!("{path}*"), &action))
    };
    // Deleting is out of scope, except an Authorization delete, which removes a lock, policy or
    // assignment that limits an identity.
    if in_paths(AZURE_IDENTITY_PATHS) {
        return Some(Family::Identity);
    }
    if is_delete {
        return None;
    }
    if in_paths(AZURE_RUN_PATHS) {
        return Some(Family::Run);
    }
    if AZURE_REACH_FRAGMENTS
        .iter()
        .any(|fragment| glob_patterns_overlap(&format!("*{fragment}*"), &action))
        || AZURE_REACH_WRITES
            .iter()
            .any(|write| glob_patterns_overlap(write, &action))
    {
        return Some(Family::Reach);
    }
    None
}

/// The family of an Azure data-plane action or pattern, if it is in one: a write or action under
/// `Microsoft.App/` (a sandbox's disk image or sandbox decides what a sandbox runs). Reads and
/// deletes are not, and a wildcard is when it can match one.
pub fn azure_data_family(action: &str) -> Option<Family> {
    let action = action.to_ascii_lowercase();
    if action.ends_with("/read") && !action.starts_with('*') || action.ends_with("/delete") {
        return None;
    }
    if action.starts_with('*') || glob_patterns_overlap("microsoft.app/*", &action) {
        return Some(Family::Run);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Which machine a marked profile starts is what tells horizond a machine is the
    /// verifier's. Creating or changing an instance profile, and attaching one to an
    /// instance, are all kept off management by the families.
    #[test]
    fn instance_profile_writes_are_family_actions() {
        for action in [
            "iam:CreateInstanceProfile",
            "iam:AddRoleToInstanceProfile",
            "iam:RemoveRoleFromInstanceProfile",
            "iam:DeleteInstanceProfile",
            "iam:TagInstanceProfile",
            "ec2:AssociateIamInstanceProfile",
            "ec2:ReplaceIamInstanceProfileAssociation",
            "ec2:DisassociateIamInstanceProfile",
        ] {
            assert!(
                aws_family(action, None).is_some(),
                "{action} must be in a family"
            );
        }
    }

    /// A Live sandbox's image decides what its sessions run: building it, rolling it and
    /// handing it its build role are the verifier's, so management holds none of them.
    #[test]
    fn building_a_sandbox_image_is_a_family_action() {
        for action in [
            "lambda:CreateMicrovmImage",
            "lambda:UpdateMicrovmImage",
            "lambda:TagResource",
        ] {
            assert_eq!(
                aws_family(action, None),
                Some(Family::Run),
                "{action} must be in the RUN family"
            );
        }
        assert_eq!(
            aws_family("iam:PassRole", Some(&["lambda.amazonaws.com".to_string()])),
            Some(Family::Identity)
        );
        // Reading a build and deleting an image stay management's.
        for action in ["lambda:GetMicrovmImage", "lambda:DeleteMicrovmImage"] {
            assert_eq!(
                aws_family(action, None),
                None,
                "{action} stays with management"
            );
        }
    }

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
            ("s3:PutBucketAcl", Family::Reach),
            ("s3:PutBucketOwnershipControls", Family::Reach),
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
            // A delete that changes what runs or widens an identity stays in its family.
            ("lambda:DeleteAlias", Family::Run),
            ("lambda:DeleteFunctionCodeSigningConfig", Family::Run),
            ("lambda:Delete*", Family::Run),
            ("iam:DeleteRolePermissionsBoundary", Family::Identity),
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
            "s3:PutObjectAcl",
            "dynamodb:PutItem",
            "sqs:SendMessage",
            "*:Get*",
            "*:Describe*",
            "ec2:Describe*",
            "autoscaling:SetDesiredCapacity",
            "autoscaling:Describe*",
            // Deletes are out of scope.
            "ec2:DeleteLaunchTemplate",
            "ec2:DeleteFleets",
            "cloudformation:DeleteStack",
            "lambda:DeleteFunctionUrlConfig",
            "s3:DeleteBucketPolicy",
        ] {
            assert_eq!(aws_family(action, None), None, "{action}");
        }
    }

    #[test]
    fn gcp_family_names_the_family_of_each_permission() {
        for (permission, family) in [
            ("run.services.update", Family::Run),
            ("run.jobs.create", Family::Run),
            ("cloudbuild.builds.create", Family::Run),
            ("compute.instances.setMetadata", Family::Run),
            ("compute.instances.attachDisk", Family::Run),
            ("compute.instances.stop", Family::Run),
            ("compute.instanceTemplates.create", Family::Run),
            ("compute.projects.setCommonInstanceMetadata", Family::Run),
            ("osconfig.patchDeployments.create", Family::Run),
            ("aiplatform.sandboxEnvironmentTemplates.create", Family::Run),
            ("iam.serviceAccounts.actAs", Family::Identity),
            ("iam.serviceAccountKeys.create", Family::Identity),
            ("iam.serviceAccounts.getAccessToken", Family::Identity),
            ("storage.buckets.setIamPolicy", Family::Reach),
            ("resourcemanager.projects.setIamPolicy", Family::Reach),
            ("iam.roles.create", Family::Reach),
            ("storage.hmacKeys.create", Family::Reach),
            ("compute.snapshots.useReadOnly", Family::Reach),
            ("pubsub.subscriptions.create", Family::Reach),
            ("pubsub.topics.attachSubscription", Family::Reach),
            ("cloudsql.instances.update", Family::Reach),
            ("cloudscheduler.jobs.create", Family::Reach),
            ("cloudscheduler.jobs.update", Family::Reach),
            ("run.*", Family::Run),
        ] {
            assert_eq!(gcp_family(permission), Some(family), "{permission}");
        }
    }

    #[test]
    fn gcp_family_leaves_reads_data_deletes_and_kept_permissions_alone() {
        for permission in [
            "run.services.get",
            "run.jobs.list",
            "run.jobs.run",
            "compute.instances.get",
            "compute.instances.delete",
            "compute.instanceGroups.create",
            "run.services.delete",
            "storage.hmacKeys.delete",
            "iam.roles.delete",
            "cloudbuild.workerpools.update",
            "iam.serviceAccounts.create",
            "iam.serviceAccounts.update",
            "iam.serviceAccounts.delete",
            "iam.serviceAccounts.get",
            "storage.objects.create",
            "storage.objects.get",
            "pubsub.topics.publish",
            "pubsub.subscriptions.consume",
            "pubsub.subscriptions.delete",
            "secretmanager.versions.add",
            "datastore.entities.update",
            "logging.logEntries.create",
            "serviceusage.services.enable",
        ] {
            assert_eq!(gcp_family(permission), None, "{permission}");
        }
    }

    #[test]
    fn azure_family_names_the_family_of_each_action() {
        for (action, family) in [
            ("Microsoft.App/containerApps/write", Family::Run),
            ("Microsoft.App/jobs/start/action", Family::Run),
            (
                "Microsoft.Compute/virtualMachines/runCommand/action",
                Family::Run,
            ),
            ("Microsoft.Compute/*", Family::Run),
            ("Microsoft.Resources/deployments/write", Family::Run),
            ("Microsoft.Resources/tags/write", Family::Run),
            (
                "Microsoft.ManagedIdentity/userAssignedIdentities/assign/action",
                Family::Identity,
            ),
            (
                "Microsoft.Authorization/roleAssignments/write",
                Family::Identity,
            ),
            ("Microsoft.Authorization/locks/delete", Family::Identity),
            (
                "Microsoft.Storage/storageAccounts/listKeys/action",
                Family::Reach,
            ),
            (
                "Microsoft.Storage/storageAccounts/regenerateKey/action",
                Family::Reach,
            ),
            ("Microsoft.Storage/storageAccounts/write", Family::Reach),
            (
                "Microsoft.ServiceBus/namespaces/authorizationRules/write",
                Family::Reach,
            ),
            (
                "Microsoft.Network/virtualNetworks/virtualNetworkPeerings/write",
                Family::Reach,
            ),
            (
                "Microsoft.Network/networkSecurityGroups/securityRules/write",
                Family::Reach,
            ),
            (
                "Microsoft.KeyVault/vaults/accessPolicies/write",
                Family::Reach,
            ),
            ("*", Family::Run),
            ("*/write", Family::Run),
        ] {
            assert_eq!(azure_family(action), Some(family), "{action}");
        }
    }

    #[test]
    fn azure_family_leaves_reads_deletes_and_infrastructure_alone() {
        for action in [
            "Microsoft.Compute/virtualMachineScaleSets/read",
            "Microsoft.Compute/virtualMachineScaleSets/delete",
            "Microsoft.App/containerApps/delete",
            "Microsoft.Authorization/roleAssignments/read",
            "*/read",
            "Microsoft.Network/virtualNetworks/write",
            "Microsoft.Network/publicIPAddresses/write",
            "Microsoft.Storage/storageAccounts/read",
            "Microsoft.Storage/storageAccounts/delete",
            "Microsoft.ServiceBus/namespaces/queues/write",
            "Microsoft.KeyVault/vaults/read",
            "Microsoft.Insights/components/write",
        ] {
            assert_eq!(azure_family(action), None, "{action}");
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
