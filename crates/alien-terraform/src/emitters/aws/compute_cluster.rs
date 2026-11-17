//! AWS ComputeCluster — IAM role + instance profile + security group +
//! machine runtime IAM.
//!
//! The full ASG / Launch Template / worker image wiring is provisioned at runtime by the
//! compute cluster controller. This emitter ships the emit-side subset: an IAM role for worker
//! instances, the matching instance profile, and a private security group attached to the
//! workload VPC.
//!
//! The IAM shape matches what the runtime controller creates:
//!
//!   `compute-cluster/execute` permission set as an inline policy
//!      via `AwsRuntimePermissionsGenerator` — cross-account ECR pull
//!      from the managing account, per-container service-account
//!      assumption, tag discovery, EBS attachment, and load balancer
//!      target lifecycle.
//!
//! Without this policy the VMs boot, the machine agent starts, and immediately fails
//! to pull images / attach volumes / register with load balancers.

use alien_core::{
    import::EmitContext, ComputeCluster, Container, ErrorData, ExternalBinding, Result, Storage,
    StorageBinding,
};
use alien_error::{AlienError, Context};
use alien_permissions::{generators::AwsRuntimePermissionsGenerator, BindingTarget};
use crate::block::{attr, resource_block};
use crate::emitters::aws::helpers::{
    aws_terraform_permission_context, downcast, emit_iam_role_policy,
    emit_iam_role_policy_for_target, emit_iam_role_policy_for_target_with_label,
    jsonencode_policy_value, nested_block, required_label, resource_prefix_template,
    service_assume_role_policy, tags, vpc_id_expr,
};
use crate::expr;
use crate::{TfEmitter, TfFragment};
use hcl::expr::Expression;

#[derive(Debug, Clone, Copy, Default)]
pub struct AwsComputeClusterEmitter;

impl TfEmitter for AwsComputeClusterEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let cluster = downcast::<ComputeCluster>(ctx, ComputeCluster::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;
        let mut fragment = TfFragment::default();
        let isolated_profile = format!("{label}_isolation_v1");
        // Retain the legacy identity even after migration. Only replacement
        // nodes may select the separately imported isolation-v1 profile.
        for (profile_label, suffix, isolated) in [
            (label, format!("{}-instances", cluster.id), false),
            (
                isolated_profile.as_str(),
                format!("{}-isolation-v1", cluster.id),
                true,
            ),
        ] {
            let role_label = format!("{profile_label}_instance");
            fragment.resource_blocks.push(resource_block(
                "aws_iam_role",
                &role_label,
                [
                    attr("name", resource_prefix_template(&suffix)),
                    attr(
                        "assume_role_policy",
                        service_assume_role_policy(&["ec2.amazonaws.com"]),
                    ),
                    attr("tags", tags(ctx, "compute-cluster")),
                ],
            ));
            // compute-cluster/execute — cross-account ECR pull +
            // sts:AssumeRole on per-container SAs. Generated through the
            // same alien-permissions runtime generator the controller uses,
            // so push and pull deployments converge on the same effective
            // IAM. Failure to find the permission set is fatal: the VMs
            // would boot without the permissions they need.
            let execute_set = alien_permissions::get_permission_set("compute-cluster/execute")
            .ok_or_else(|| {
                AlienError::new(ErrorData::GenericError {
                    message: "missing permission set 'compute-cluster/execute' (required by compute-cluster IAM role)".to_string(),
                })
            })?;
            let context = aws_terraform_permission_context().with_resource_name(suffix.clone());
            emit_iam_role_policy(&mut fragment, &role_label, execute_set, 0, &context)?;
            if isolated {
                emit_node_permissions(ctx, cluster, &role_label, &mut fragment)?;
                // A protected install's machines run as this role (see
                // `emit_verifier_machine_profiles`).
                if ctx.stack_settings.signing.is_some() {
                    fragment
                        .resource_blocks
                        .push(machine_signing_policy(&role_label));
                }
            }
            // The host initiates AttachVolume, so EC2 needs a grant on the selected key.
            let keys: std::collections::BTreeSet<_> = ctx
                .stack
                .resources
                .values()
                .filter_map(|entry| entry.config.downcast_ref::<Container>())
                .filter(|container| container.cluster.as_deref() == Some(cluster.id.as_str()))
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
                let key_label = ctx.name_for(key).ok_or_else(|| {
                    AlienError::new(ErrorData::GenericError {
                        message: format!("Volume encryption key '{key}' has no setup resource"),
                    })
                })?;
                let set = alien_permissions::get_permission_set("key/native-volume").ok_or_else(
                    || {
                        AlienError::new(ErrorData::GenericError {
                            message: "Missing key/native-volume permission set".to_string(),
                        })
                    },
                )?;
                let context = aws_terraform_permission_context()
                    .with_resource_name(format!("${{local.{key_label}_selected_key.arn}}"));
                emit_iam_role_policy_for_target_with_label(
                    &mut fragment,
                    &role_label,
                    set,
                    &format!("{role_label}_{key_label}_native_volume"),
                    &format!("volume-key-{key}"),
                    &context,
                    BindingTarget::Resource,
                )?;
            }

            fragment.resource_blocks.push(resource_block(
                "aws_iam_instance_profile",
                profile_label,
                [
                    attr("name", resource_prefix_template(&suffix)),
                    attr(
                        "role",
                        expr::traversal(["aws_iam_role", &role_label, "name"]),
                    ),
                    attr("tags", tags(ctx, "compute-cluster")),
                ],
            ));
        }

        if ctx.stack_settings.signing.is_some() {
            emit_verifier_machine_profiles(
                ctx,
                cluster,
                label,
                &format!("{isolated_profile}_instance"),
                &mut fragment,
            )?;
        }

        emit_volume_backups_role(ctx, cluster, label, &mut fragment)?;

        fragment.resource_blocks.push(resource_block(
            "aws_security_group",
            label,
            [
                attr(
                    "name_prefix",
                    resource_prefix_template(&format!("{}-cluster-", cluster.id)),
                ),
                attr(
                    "description",
                    Expression::String("Container cluster machine security group".to_string()),
                ),
                attr("vpc_id", vpc_id_expr(ctx)),
                nested_block(
                    "egress",
                    vec![
                        attr(
                            "description",
                            Expression::String("Outbound runtime traffic".to_string()),
                        ),
                        attr("from_port", Expression::Number(hcl::Number::from(0i64))),
                        attr("to_port", Expression::Number(hcl::Number::from(0i64))),
                        attr("protocol", Expression::String("-1".to_string())),
                        attr(
                            "cidr_blocks",
                            Expression::Array(vec![Expression::String("0.0.0.0/0".to_string())]),
                        ),
                    ],
                ),
                attr("tags", tags(ctx, "compute-cluster")),
            ],
        ));

        // Own only these two ingress rules. Runtime endpoint controllers may add their
        // own load-balancer ingress; an inline ingress set would revoke those on apply.
        for (suffix, description, source_attr, source) in [
            (
                "containers",
                "Container network",
                "cidr_ipv4",
                Expression::String(cluster.container_cidr().to_string()),
            ),
            (
                "peers",
                "Compute node peer traffic",
                "referenced_security_group_id",
                expr::traversal(["aws_security_group", label, "id"]),
            ),
        ] {
            fragment.resource_blocks.push(resource_block(
                "aws_vpc_security_group_ingress_rule",
                &format!("{label}_{suffix}"),
                [
                    attr(
                        "security_group_id",
                        expr::traversal(["aws_security_group", label, "id"]),
                    ),
                    attr("description", Expression::String(description.to_string())),
                    attr("ip_protocol", Expression::String("-1".to_string())),
                    attr(source_attr, source),
                    attr("tags", tags(ctx, "compute-cluster")),
                ],
            ));
        }

        Ok(fragment)
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        let cluster = downcast::<ComputeCluster>(ctx, ComputeCluster::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;
        Ok(expr::object([
            (
                "clusterId",
                expr::template(format!("${{local.resource_prefix}}-{}", cluster.id)),
            ),
            (
                "instanceProfileArn",
                expr::traversal(["aws_iam_instance_profile", label, "arn"]),
            ),
            (
                "isolatedInstanceProfileArn",
                expr::traversal([
                    "aws_iam_instance_profile",
                    &format!("{label}_isolation_v1"),
                    "arn",
                ]),
            ),
            (
                "securityGroupId",
                expr::traversal(["aws_security_group", label, "id"]),
            ),
        ]))
    }
}

/// The execution role the cluster's containers run their DLM snapshot policies
/// as, with `container/volume-backups` and the volume keys through EC2. Setup
/// owns it: the management identity may only pass it to DLM. The name is what
/// the container controller and the management grants expect.
/// What a protected install's machines read and write: the signing settings and deployer inputs
/// under `/signed/<prefix>/`, and the ledger `/signed/<prefix>/state`, which they update as they
/// accept releases. Only setup writes the rest; the management role is denied all of it.
fn machine_signing_policy(role_label: &str) -> hcl::Block {
    let parameter = |path: &str| {
        format!(
            "arn:aws:ssm:${{data.aws_region.current.region}}:${{data.aws_caller_identity.current.account_id}}:parameter/signed/${{local.resource_prefix}}/{path}"
        )
    };
    resource_block(
        "aws_iam_role_policy",
        &format!("{role_label}_signing"),
        [
            attr("name", Expression::String("signing-settings".to_string())),
            attr("role", expr::traversal(["aws_iam_role", role_label, "id"])),
            attr(
                "policy",
                jsonencode_policy_value(serde_json::json!({
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
                })),
            ),
        ],
    )
}

/// On a protected install the verifier writes each capacity group's launch template with the
/// instance profile `/alien-signed/<prefix>-<cluster>-<group>-signed`, so setup creates one per group, holding the
/// node role that carries the node grants. Management can't pass roles to EC2 there.
fn emit_verifier_machine_profiles(
    ctx: &EmitContext<'_>,
    cluster: &ComputeCluster,
    label: &str,
    role_label: &str,
    fragment: &mut TfFragment,
) -> Result<()> {
    let mut labels = std::collections::BTreeSet::new();
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
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect();
        let profile_label = format!("{label}_group_{id_part}");
        if !labels.insert(profile_label.clone()) {
            return Err(AlienError::new(ErrorData::GenericError {
                message: format!(
                    "Capacity groups of cluster '{}' differ only in punctuation ('{}'), so their instance profiles can't be told apart",
                    cluster.id, group.group_id
                ),
            }));
        }
        fragment.resource_blocks.push(resource_block(
            "aws_iam_instance_profile",
            &profile_label,
            [
                attr(
                    "name",
                    resource_prefix_template(&alien_signing::settings::names::aws_signed_profile_name(
                        &format!("{}-{}", cluster.id, group.group_id),
                    )),
                ),
                // The reserved path marks a machine as launched by the verifier; management holds
                // no action on it.
                attr(
                    "path",
                    Expression::from(alien_signing::settings::names::AWS_SIGNED_PROFILE_PATH),
                ),
                attr("role", expr::traversal(["aws_iam_role", role_label, "name"])),
                attr("tags", tags(ctx, "compute-cluster")),
            ],
        ));
    }
    Ok(())
}

fn emit_volume_backups_role(
    ctx: &EmitContext<'_>,
    cluster: &ComputeCluster,
    label: &str,
    fragment: &mut TfFragment,
) -> Result<()> {
    let containers: Vec<&Container> = ctx
        .stack
        .resources
        .values()
        .filter_map(|entry| entry.config.downcast_ref::<Container>())
        .filter(|container| container.cluster.as_deref() == Some(cluster.id.as_str()))
        .filter(|container| container.persistent_storage.is_some())
        .collect();
    if containers.is_empty() {
        return Ok(());
    }
    let role_label = format!("{label}_volume_backups");
    let suffix = format!("volume-backups-{}", cluster.id);
    fragment.resource_blocks.push(resource_block(
        "aws_iam_role",
        &role_label,
        [
            attr("name", resource_prefix_template(&suffix)),
            // DLM names the account it acts for, so only this account's DLM can use the role.
            attr(
                "assume_role_policy",
                jsonencode_policy_value(serde_json::json!({
                    "Version": "2012-10-17",
                    "Statement": [{
                        "Effect": "Allow",
                        "Principal": { "Service": "dlm.amazonaws.com" },
                        "Action": "sts:AssumeRole",
                        "Condition": {
                            "StringEquals": {
                                "aws:SourceAccount": "${data.aws_caller_identity.current.account_id}"
                            }
                        }
                    }]
                })),
            ),
            attr("tags", tags(ctx, "compute-cluster")),
        ],
    ));
    let set =
        alien_permissions::get_permission_set("container/volume-backups").ok_or_else(|| {
            AlienError::new(ErrorData::GenericError {
                message: "Missing container/volume-backups permission set".to_string(),
            })
        })?;
    let context = aws_terraform_permission_context().with_resource_name(suffix);
    emit_iam_role_policy(fragment, &role_label, set, 0, &context)?;
    let keys: std::collections::BTreeSet<_> = containers
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
        let key_label = ctx.name_for(key).ok_or_else(|| {
            AlienError::new(ErrorData::GenericError {
                message: format!("Volume encryption key '{key}' has no setup resource"),
            })
        })?;
        let set = alien_permissions::get_permission_set("key/native-volume").ok_or_else(|| {
            AlienError::new(ErrorData::GenericError {
                message: "Missing key/native-volume permission set".to_string(),
            })
        })?;
        let context = aws_terraform_permission_context()
            .with_resource_name(format!("${{local.{key_label}_selected_key.arn}}"));
        emit_iam_role_policy_for_target_with_label(
            fragment,
            &role_label,
            set,
            &format!("{role_label}_{key_label}_native_volume"),
            &format!("volume-key-{key}"),
            &context,
            BindingTarget::Resource,
        )?;
    }
    Ok(())
}

/// Attach grants after both the node identity and the actual bucket exist. Keeping
/// these policies with the cluster avoids a Storage -> cluster dependency cycle.
fn emit_node_permissions(
    ctx: &EmitContext<'_>,
    cluster: &ComputeCluster,
    role_label: &str,
    fragment: &mut TfFragment,
) -> Result<()> {
    let Some(profile) = &cluster.node_permissions else {
        return Ok(());
    };
    let mut index = 1;
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
                let label = ctx
                    .name_for(target)
                    .ok_or_else(|| AlienError::new(invalid()))?;
                format!("${{aws_s3_bucket.{label}.bucket}}")
            }
        };
        let context = aws_terraform_permission_context()
            .with_resource_id(target)
            .with_resource_name(bucket);
        for reference in references {
            let set = reference
                .resolve(|name| alien_permissions::get_permission_set(name).cloned())
                .ok_or_else(|| AlienError::new(invalid()))?;
            // The ordinary emitter skips sets without AWS grants; an explicit node
            // request must instead fail before setup produces an underprivileged host.
            let policy = AwsRuntimePermissionsGenerator::new()
                .generate_policy(&set, BindingTarget::Resource, &context)
                .context(invalid())?;
            if policy.statement.is_empty() {
                return Err(AlienError::new(invalid()));
            }
            let appended_from = fragment.resource_blocks.len();
            emit_iam_role_policy_for_target(
                fragment,
                role_label,
                &set,
                index,
                &context,
                BindingTarget::Resource,
            )?;
            fragment.mark_gated_from(appended_from, entry.enabled_when.as_deref());
            index += 1;
        }
    }
    Ok(())
}
