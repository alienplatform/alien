//! AWS ComputeCluster — IAM role + instance profile + security group +
//! machine runtime IAM.
//!
//! The full ASG / Launch Template / machine image wiring is provisioned at runtime by the
//! compute cluster controller. This emitter ships the emit-side subset: an IAM role for worker
//! instances, the matching instance profile, and an empty security group attached to the
//! workload VPC.
//!
//! The IAM shape matches what the runtime controller creates:
//!
//!   `compute-cluster/execute` permission set as an inline policy
//!      via `AwsCloudFormationPermissionsGenerator` — cross-account ECR
//!      pull, per-container service-account assumption, tag discovery,
//!      EBS attachment, and load balancer target lifecycle.

use crate::emitters::aws::helpers::{
    cf_from_json, required_logical_id, resource_config, service_trust_policy, stack_name, tags,
    vpc_id_expr,
};
use crate::{CfEmitter, CfExpression, CfResource};
use alien_core::{
    import::EmitContext, ComputeCluster, Container, ErrorData, ExternalBinding, NetworkSettings,
    Result, Storage, StorageBinding,
};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_permissions::{
    generators::AwsCloudFormationPermissionsGenerator, BindingTarget, PermissionContext,
};

const CONDITION_NETWORK_MODE_CREATE: &str = "NetworkModeCreate";
const CONDITION_NETWORK_MODE_USE_EXISTING: &str = "NetworkModeUseExisting";

#[derive(Debug, Clone, Copy, Default)]
pub struct AwsComputeClusterEmitter;

impl CfEmitter for AwsComputeClusterEmitter {
    fn emit_resources(&self, ctx: &EmitContext<'_>) -> Result<Vec<CfResource>> {
        let cluster = resource_config::<ComputeCluster>(ctx, ComputeCluster::RESOURCE_TYPE)?;
        let logical_id = required_logical_id(ctx)?;
        let role_id = format!("{logical_id}InstanceRole");
        let profile_id = format!("{logical_id}InstanceProfile");
        let security_group_id = format!("{logical_id}SecurityGroup");
        let container_ingress_id = format!("{logical_id}SecurityGroupContainerIngress");
        let peer_ingress_id = format!("{logical_id}SecurityGroupPeerIngress");

        let mut role = CfResource::new(role_id.clone(), "AWS::IAM::Role".to_string());
        // The management role's iam:PassRole grant is scoped to
        // `${AWS::StackName}-*`. Leaving RoleName implicit lets CloudFormation
        // truncate long generated physical names before the end of the stack
        // name, so the role can fall outside that boundary and ASG creation is
        // denied. Keep the physical name deterministic and inside the same
        // stack prefix used by the permission generator.
        role.properties
            .insert("RoleName".to_string(), stack_name("compute"));
        role.properties.insert(
            "AssumeRolePolicyDocument".to_string(),
            service_trust_policy(["ec2.amazonaws.com"]),
        );
        let mut policies = Vec::new();
        if let Some(execute_policy) = compute_cluster_execute_policy(cluster)? {
            policies.push(CfExpression::object([
                ("PolicyName", CfExpression::from("ComputeClusterExecute")),
                ("PolicyDocument", execute_policy),
            ]));
        }
        role.properties
            .insert("Policies".to_string(), CfExpression::list(policies.clone()));
        role.properties.insert("Tags".to_string(), tags(ctx));

        let mut profile =
            CfResource::new(profile_id.clone(), "AWS::IAM::InstanceProfile".to_string());
        profile.properties.insert(
            "Roles".to_string(),
            CfExpression::list([CfExpression::ref_(&role_id)]),
        );
        profile.depends_on.push(role_id.clone());

        let mut security_group = CfResource::new(
            security_group_id.clone(),
            "AWS::EC2::SecurityGroup".to_string(),
        );
        security_group.properties.insert(
            "GroupDescription".to_string(),
            CfExpression::from("Compute cluster machine security group"),
        );
        security_group
            .properties
            .insert("VpcId".to_string(), vpc_id_expr(ctx));
        security_group.properties.insert(
            "SecurityGroupEgress".to_string(),
            security_group_egress(ctx),
        );
        security_group
            .properties
            .insert("Tags".to_string(), tags(ctx));

        let container_ingress = security_group_ingress(
            container_ingress_id,
            &security_group_id,
            [
                ("IpProtocol", CfExpression::from("-1")),
                ("CidrIp", CfExpression::from(cluster.container_cidr())),
                ("Description", CfExpression::from("Container network")),
            ],
        );
        let peer_ingress = security_group_ingress(
            peer_ingress_id,
            &security_group_id,
            [
                ("IpProtocol", CfExpression::from("-1")),
                (
                    "SourceSecurityGroupId",
                    CfExpression::get_att(&security_group_id, "GroupId"),
                ),
                (
                    "Description",
                    CfExpression::from("Compute node peer traffic"),
                ),
            ],
        );

        // Keep the legacy physical role/profile declared for old nodes until
        // cluster destruction; node data grants belong only to the new identity.
        let isolated_role_id = format!("{logical_id}IsolationV1Role");
        let isolated_profile_id = format!("{logical_id}IsolationV1Profile");
        let mut isolated_role = role.clone();
        isolated_role.logical_id = isolated_role_id.clone();
        isolated_role.properties.insert(
            "RoleName".to_string(),
            stack_name(&format!("{}-isolation-v1", cluster.id)),
        );
        // A protected install's machines run as this role (see `verifier_machine_profiles`).
        if ctx.stack_settings.signing.is_some() {
            let mut isolated_policies = policies;
            isolated_policies.push(CfExpression::object([
                ("PolicyName", CfExpression::from("SigningSettings")),
                ("PolicyDocument", machine_signing_policy()?),
            ]));
            isolated_role.properties.insert(
                "Policies".to_string(),
                CfExpression::list(isolated_policies),
            );
        }
        let mut isolated_profile =
            CfResource::new(isolated_profile_id, "AWS::IAM::InstanceProfile".to_string());
        isolated_profile.properties.insert(
            "Roles".to_string(),
            CfExpression::list([CfExpression::ref_(&isolated_role_id)]),
        );
        isolated_profile.depends_on.push(isolated_role_id.clone());
        let mut resources = vec![
            role,
            profile,
            isolated_role,
            isolated_profile,
            security_group,
            container_ingress,
            peer_ingress,
        ];
        resources.extend(node_storage_policies(ctx, cluster, &isolated_role_id)?);
        if ctx.stack_settings.signing.is_some() {
            resources.extend(verifier_machine_profiles(
                cluster,
                logical_id,
                &isolated_role_id,
            )?);
        }
        let persistent: Vec<&Container> = ctx
            .stack
            .resources
            .values()
            .filter_map(|entry| entry.config.downcast_ref::<Container>())
            .filter(|container| container.cluster.as_deref() == Some(cluster.id.as_str()))
            .filter(|container| container.persistent_storage.is_some())
            .collect();
        let backups_role_id = format!("{logical_id}VolumeBackupsRole");
        let mut backup_key_policies = Vec::new();
        let keys: std::collections::BTreeSet<_> = persistent
            .iter()
            .filter_map(|container| {
                container
                    .persistent_storage
                    .as_ref()?
                    .encryption_key
                    .as_ref()
            })
            .map(|key| key.id())
            .collect();
        for key in keys {
            let key_id = ctx.name_for(key).ok_or_else(|| {
                AlienError::new(ErrorData::GenericError {
                    message: format!("Volume encryption key '{key}' has no setup resource"),
                })
            })?;
            let set =
                alien_permissions::get_permission_set("key/native-volume").ok_or_else(|| {
                    AlienError::new(ErrorData::GenericError {
                        message: "Missing key/native-volume permission set".to_string(),
                    })
                })?;
            let mut document = AwsCloudFormationPermissionsGenerator::new()
                .generate_policy(
                    set,
                    BindingTarget::Resource,
                    &compute_cluster_permission_context(cluster).with_resource_name("selected-key"),
                )
                .context(ErrorData::GenericError {
                    message: "Failed to generate volume key permissions".to_string(),
                })?;
            for statement in &mut document.statement {
                statement.resource = vec![serde_json::json!({"Fn::GetAtt": [key_id, "Arn"]})];
            }
            let mut policy = CfResource::new(
                format!("{logical_id}{key_id}VolumeKeyPolicy"),
                "AWS::IAM::Policy".to_string(),
            );
            policy.properties.insert(
                "PolicyName".to_string(),
                CfExpression::from(format!("volume-key-{key}")),
            );
            // Install the backup role's inline grant before removing its old
            // shared attachment. This also preserves the previous update ordering.
            policy.depends_on.push(backups_role_id.clone());
            // Keep node grants setup-owned independently of the backup role.
            policy.properties.insert(
                "Roles".to_string(),
                CfExpression::list([
                    CfExpression::ref_(&role_id),
                    CfExpression::ref_(&isolated_role_id),
                ]),
            );
            let document =
                cf_from_json(serde_json::to_value(document).into_alien_error().context(
                    ErrorData::GenericError {
                        message: "Failed to serialize volume key permissions".to_string(),
                    },
                )?)?;
            // Use a distinct policy name so removing the old shared attachment
            // cannot delete the new inline grant during an update or rollback.
            backup_key_policies.push(CfExpression::object([
                (
                    "PolicyName",
                    CfExpression::from(format!("backup-volume-key-{key}")),
                ),
                ("PolicyDocument", document.clone()),
            ]));
            policy
                .properties
                .insert("PolicyDocument".to_string(), document);
            resources.push(policy);
        }
        if !persistent.is_empty() {
            resources.push(volume_backups_role(
                ctx,
                cluster,
                &backups_role_id,
                backup_key_policies,
            )?);
        }

        Ok(resources)
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<CfExpression> {
        let cluster = resource_config::<ComputeCluster>(ctx, ComputeCluster::RESOURCE_TYPE)?;
        let logical_id = required_logical_id(ctx)?;
        Ok(CfExpression::object([
            (
                "clusterId",
                CfExpression::sub(format!("${{AWS::StackName}}-{}", cluster.id)),
            ),
            (
                "instanceProfileArn",
                CfExpression::get_att(format!("{logical_id}InstanceProfile"), "Arn"),
            ),
            (
                "isolatedInstanceProfileArn",
                CfExpression::get_att(format!("{logical_id}IsolationV1Profile"), "Arn"),
            ),
            (
                "securityGroupId",
                CfExpression::get_att(format!("{logical_id}SecurityGroup"), "GroupId"),
            ),
        ]))
    }
}

/// On a protected install the verifier writes each capacity group's launch template with the
/// instance profile `/alien-signed/<prefix>-<cluster>-<group>-signed`, so setup creates one per group, holding the
/// node role that carries the node grants. Management can't pass roles to EC2 there.
fn verifier_machine_profiles(
    cluster: &ComputeCluster,
    logical_id: &str,
    role_id: &str,
) -> Result<Vec<CfResource>> {
    let mut profiles: Vec<CfResource> = Vec::new();
    for group in &cluster.capacity_groups {
        if matches!(group.group_id.as_str(), "isolation-v1" | "instances") {
            return Err(AlienError::new(ErrorData::GenericError {
                message: format!(
                    "Capacity group '{}' of cluster '{}' has a name setup uses for the cluster's own instance profile",
                    group.group_id, cluster.id
                ),
            }));
        }
        let id_part: String = group
            .group_id
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .collect();
        let profile_id = format!("{logical_id}{id_part}MachineProfile");
        if profiles.iter().any(|profile| profile.logical_id == profile_id) {
            return Err(AlienError::new(ErrorData::GenericError {
                message: format!(
                    "Capacity groups of cluster '{}' differ only in punctuation ('{}'), so their instance profiles can't be told apart",
                    cluster.id, group.group_id
                ),
            }));
        }
        let mut profile = CfResource::new(profile_id, "AWS::IAM::InstanceProfile".to_string());
        profile.properties.insert(
            "InstanceProfileName".to_string(),
            CfExpression::sub(format!(
                "${{AWS::StackName}}-{}",
                alien_signing::settings::names::aws_signed_profile_name(&format!(
                    "{}-{}",
                    cluster.id, group.group_id
                ))
            )),
        );
        // The reserved path marks a machine as launched by the verifier; management holds no
        // action on it.
        profile.properties.insert(
            "Path".to_string(),
            CfExpression::from(alien_signing::settings::names::AWS_SIGNED_PROFILE_PATH),
        );
        profile.properties.insert(
            "Roles".to_string(),
            CfExpression::list([CfExpression::ref_(role_id)]),
        );
        profile.depends_on.push(role_id.to_string());
        profiles.push(profile);
    }
    Ok(profiles)
}

fn security_group_egress(ctx: &EmitContext<'_>) -> CfExpression {
    // Restricted setup artifacts have only create/existing modes, so the
    // optional-default condition is not declared by the outer generator. A protected stack
    // whose verifier launches machines is restricted the same way (alien-cloudformation
    // `network_mode_names_subnets`).
    if alien_core::restricts_network_mode(ctx.stack, ctx.targets_kubernetes)
        || alien_core::verifier_launches_machines(ctx.stack)
    {
        return allow_all_egress();
    }
    match ctx.stack_settings.network.as_ref() {
        Some(NetworkSettings::UseDefault) => CfExpression::no_value(),
        Some(NetworkSettings::Create { .. } | NetworkSettings::ByoVpcAws { .. }) => {
            CfExpression::if_(
                CONDITION_NETWORK_MODE_CREATE,
                allow_all_egress(),
                CfExpression::if_(
                    CONDITION_NETWORK_MODE_USE_EXISTING,
                    allow_all_egress(),
                    CfExpression::no_value(),
                ),
            )
        }
        Some(NetworkSettings::ByoVpcGcp { .. } | NetworkSettings::ByoVnetAzure { .. }) | None => {
            allow_all_egress()
        }
    }
}

fn allow_all_egress() -> CfExpression {
    CfExpression::list([CfExpression::object([
        ("IpProtocol", CfExpression::from("-1")),
        ("CidrIp", CfExpression::from("0.0.0.0/0")),
        (
            "Description",
            CfExpression::from("Outbound runtime traffic"),
        ),
    ])])
}

fn security_group_ingress<I, K>(
    logical_id: String,
    security_group_id: &str,
    properties: I,
) -> CfResource
where
    I: IntoIterator<Item = (K, CfExpression)>,
    K: Into<String>,
{
    let mut ingress = CfResource::new(logical_id, "AWS::EC2::SecurityGroupIngress".to_string());
    ingress.properties.insert(
        "GroupId".to_string(),
        CfExpression::get_att(security_group_id, "GroupId"),
    );
    for (key, value) in properties {
        ingress.properties.insert(key.into(), value);
    }
    ingress
}

/// The execution role the cluster's containers run their DLM snapshot policies
/// as. Setup owns it: the management identity may only pass it to DLM. Inline
/// policies grant `container/volume-backups` and access to the volume keys through
/// EC2. They cannot share or copy a snapshot. The name is what the container
/// controller and the management grants expect.
fn volume_backups_role(
    ctx: &EmitContext<'_>,
    cluster: &ComputeCluster,
    logical_id: &str,
    mut key_policies: Vec<CfExpression>,
) -> Result<CfResource> {
    let set =
        alien_permissions::get_permission_set("container/volume-backups").ok_or_else(|| {
            AlienError::new(ErrorData::GenericError {
                message: "Missing container/volume-backups permission set".to_string(),
            })
        })?;
    let policy = AwsCloudFormationPermissionsGenerator::new()
        .generate_policy(
            set,
            BindingTarget::Stack,
            &compute_cluster_permission_context(cluster),
        )
        .context(ErrorData::GenericError {
            message: "Failed to generate volume backup permissions".to_string(),
        })?;
    let policy = cf_from_json(serde_json::to_value(policy).into_alien_error().context(
        ErrorData::TemplateSerializationFailed {
            format: "CloudFormation IAM policy".to_string(),
            reason: "Failed to serialize container/volume-backups policy".to_string(),
        },
    )?)?;
    let mut role = CfResource::new(logical_id.to_string(), "AWS::IAM::Role".to_string());
    role.properties.insert(
        "RoleName".to_string(),
        stack_name(&format!("volume-backups-{}", cluster.id)),
    );
    // DLM names the account it acts for, so only this account's DLM can use the role.
    role.properties.insert(
        "AssumeRolePolicyDocument".to_string(),
        CfExpression::object([
            ("Version", CfExpression::from("2012-10-17")),
            (
                "Statement",
                CfExpression::list([CfExpression::object([
                    ("Effect", CfExpression::from("Allow")),
                    (
                        "Principal",
                        CfExpression::object([(
                            "Service",
                            CfExpression::from("dlm.amazonaws.com"),
                        )]),
                    ),
                    ("Action", CfExpression::from("sts:AssumeRole")),
                    (
                        "Condition",
                        CfExpression::object([(
                            "StringEquals",
                            CfExpression::object([(
                                "aws:SourceAccount",
                                CfExpression::ref_("AWS::AccountId"),
                            )]),
                        )]),
                    ),
                ])]),
            ),
        ]),
    );
    key_policies.insert(
        0,
        CfExpression::object([
            ("PolicyName", CfExpression::from("VolumeBackups")),
            ("PolicyDocument", policy),
        ]),
    );
    role.properties
        .insert("Policies".to_string(), CfExpression::list(key_policies));
    role.properties.insert("Tags".to_string(), tags(ctx));
    Ok(role)
}

/// Inline policy from the `compute-cluster/execute` permission set —
/// cross-account ECR pull + `sts:AssumeRole` on per-container SAs +
/// EBS attach/detach + ELB target registration.
///
/// Generated through `AwsCloudFormationPermissionsGenerator` so the
/// CFN template uses native intrinsics (`!Sub`, `!Ref`) for the
/// `${AWS::Region}` / `${AWS::AccountId}` / `${ManagingRoleArn}`
/// placeholders rather than rendering them inline as strings.
/// What a protected install's machines read and write: the signing settings and deployer inputs
/// under `/signed/<stack>/`, and the ledger `/signed/<stack>/state`, which they update as they
/// accept releases. Only setup writes the rest; the management role is denied all of it.
fn machine_signing_policy() -> Result<CfExpression> {
    let parameter = |path: &str| {
        serde_json::json!({
            "Fn::Sub": format!("arn:aws:ssm:${{AWS::Region}}:${{AWS::AccountId}}:parameter/signed/${{AWS::StackName}}/{path}")
        })
    };
    cf_from_json(serde_json::json!({
        "Version": "2012-10-17",
        "Statement": [
            {
                "Effect": "Allow",
                "Action": ["ssm:GetParameter", "ssm:GetParametersByPath"],
                "Resource": parameter("*"),
            },
            {
                "Effect": "Allow",
                "Action": "ssm:PutParameter",
                "Resource": parameter("state"),
            },
        ],
    }))
}

fn compute_cluster_execute_policy(cluster: &ComputeCluster) -> Result<Option<CfExpression>> {
    let Some(execute_set) = alien_permissions::get_permission_set("compute-cluster/execute") else {
        return Err(AlienError::new(ErrorData::GenericError {
            message:
                "missing permission set 'compute-cluster/execute' (required by compute-cluster IAM role)"
                    .to_string(),
        }));
    };

    let generator = AwsCloudFormationPermissionsGenerator::new();
    let context = compute_cluster_permission_context(cluster);
    let policy = generator
        .generate_policy(execute_set, BindingTarget::Stack, &context)
        .context(ErrorData::GenericError {
            message: format!(
                "failed to generate compute-cluster/execute policy for '{}'",
                cluster.id
            ),
        })?;

    if policy.statement.is_empty() {
        return Ok(None);
    }

    let policy_value = serde_json::to_value(policy).into_alien_error().context(
        ErrorData::TemplateSerializationFailed {
            format: "CloudFormation IAM policy".to_string(),
            reason: "Failed to serialize compute-cluster/execute policy".to_string(),
        },
    )?;

    Ok(Some(cf_from_json(policy_value)?))
}

/// CFN-flavored permission context for the compute-cluster role.
/// Uses the stack-template parameters that the generator already adds
/// (`ManagingRoleArn`, `ManagingAccountId`) and CFN intrinsics for
/// region/account ID, so the rendered template stays portable across
/// regions and resolves cross-account ECR ARNs against the manager.
///
/// `stack_prefix` is intentionally empty: the CFN generator's
/// [`AwsCloudFormationPermissionsGenerator`] expands `${stackPrefix}`
/// to `${AWS::StackName}` when the context value is empty, but
/// concatenates `${AWS::StackName}-{value}` when non-empty. Passing
/// `${AWS::StackName}` here would produce a doubled
/// `${AWS::StackName}-${AWS::StackName}-…` ARN. Mirrors the same
/// convention `service_account::permission_context()` uses.
fn compute_cluster_permission_context(cluster: &ComputeCluster) -> PermissionContext {
    PermissionContext::new()
        .with_stack_prefix("")
        .with_aws_region("${AWS::Region}")
        .with_aws_account_id("${AWS::AccountId}")
        .with_managing_role_arn("${ManagingRoleArn}")
        .with_managing_account_id("${ManagingAccountId}")
        .with_resource_name(format!("{}-instances", cluster.id))
}

fn node_storage_policies(
    ctx: &EmitContext<'_>,
    cluster: &ComputeCluster,
    role_id: &str,
) -> Result<Vec<CfResource>> {
    let mut resources = Vec::new();
    let Some(profile) = &cluster.node_permissions else {
        return Ok(resources);
    };
    let generator = AwsCloudFormationPermissionsGenerator::new();
    for (target, references) in &profile.0 {
        if references.is_empty() {
            continue;
        }
        let invalid = || ErrorData::GenericError {
            message: format!("Node permissions require a supported AWS Storage target '{target}'"),
        };
        let entry = ctx
            .stack
            .resources
            .get(target)
            .filter(|entry| entry.config.downcast_ref::<Storage>().is_some())
            .ok_or_else(|| AlienError::new(invalid()))?;
        let external = ctx
            .stack_settings
            .external_bindings
            .as_ref()
            .and_then(|bindings| bindings.get(target));
        let bucket = match external {
            Some(ExternalBinding::Storage(StorageBinding::S3(binding)))
                if binding.endpoint.is_none() =>
            {
                binding
                    .bucket_name
                    .clone()
                    .into_value(target, "bucketName")?
            }
            Some(_) => return Err(AlienError::new(invalid())),
            None => {
                let logical_id = ctx
                    .name_for(target)
                    .ok_or_else(|| AlienError::new(invalid()))?;
                format!("${{{logical_id}}}")
            }
        };
        let context = compute_cluster_permission_context(cluster)
            .with_resource_id(target)
            .with_resource_name(bucket);
        for reference in references {
            let set = reference
                .resolve(|name| alien_permissions::get_permission_set(name).cloned())
                .ok_or_else(|| AlienError::new(invalid()))?;
            let policy = generator
                .generate_policy(&set, BindingTarget::Resource, &context)
                .context(invalid())?;
            if policy.statement.is_empty() {
                return Err(AlienError::new(invalid()));
            }
            let policy_value = serde_json::to_value(policy)
                .into_alien_error()
                .context(invalid())?;
            let index = resources.len();
            let mut resource = CfResource::new(
                format!("{role_id}NodeGrant{index}"),
                "AWS::IAM::Policy".to_string(),
            );
            resource.properties.insert(
                "PolicyName".to_string(),
                CfExpression::from(format!("NodeGrant{index}")),
            );
            resource.properties.insert(
                "Roles".to_string(),
                CfExpression::list([CfExpression::ref_(role_id)]),
            );
            resource
                .properties
                .insert("PolicyDocument".to_string(), cf_from_json(policy_value)?);
            resource.condition = entry
                .enabled_when
                .as_deref()
                .map(crate::emitters::enabled::condition_name);
            resources.push(resource);
        }
    }
    Ok(resources)
}
