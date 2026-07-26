//! AWS management access — cross-account IAM role.
//!
//! Trust policy allows the manager identity (passed as
//! `var.managing_role_arn`) to assume the role.
//!
//! Inline policies come from `AwsRuntimePermissionsGenerator` over the
//! materialised management profile (the `ManagementPermissionProfileMutation`
//! preflight turns `Auto` into an explicit `Extend` before generation
//! runs). One `aws_iam_role_policy` per permission set, sharing the
//! same role — symmetric with GCP's per-permission-set custom-role
//! emits and with what the runtime controller would attach via
//! `put_role_policy`.

use crate::{
    block::{attr, resource_block},
    emitter::{TfEmitter, TfFragment},
    emitters::aws::helpers::{
        aws_terraform_permission_context, downcast, emit_iam_managed_policy_chunks,
        iam_role_name_template, jsonencode, required_label, tags,
    },
    expr,
};
use alien_core::{
    import::EmitContext, ErrorData, KubernetesCluster, PermissionProfile, PermissionSetReference,
    RemoteStackManagement, ResourceLifecycle, Result, Worker,
};
use alien_error::Context;
use alien_permissions::{
    generators::{AwsIamStatement, AwsRuntimePermissionsGenerator},
    BindingTarget, PermissionContext,
};
use hcl::expr::Expression;

#[derive(Debug, Clone, Copy, Default)]
pub struct AwsRemoteStackManagementEmitter;

impl TfEmitter for AwsRemoteStackManagementEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let _ = downcast::<RemoteStackManagement>(ctx, RemoteStackManagement::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;

        let mut fragment = TfFragment::default();
        fragment.resource_blocks.push(resource_block(
            "aws_iam_role",
            label,
            [
                attr("name", iam_role_name_template("management")),
                attr("assume_role_policy", trust_policy()),
                attr("tags", tags(ctx, "management")),
            ],
        ));

        let generator = AwsRuntimePermissionsGenerator::new();
        let context = aws_terraform_permission_context();
        let stack_context = context.clone().with_resource_name("management".to_string());
        let mut statements = Vec::<AwsIamStatement>::new();
        if let Some(profile) = ctx.stack.management().profile() {
            for permission_set_ref in global_permission_refs(profile) {
                if let Some(permission_set) = permission_set_ref
                    .resolve(|name| alien_permissions::get_permission_set(name).cloned())
                {
                    if permission_set.platforms.aws.is_none() {
                        continue;
                    }

                    let policy = generator
                        .generate_policy(&permission_set, BindingTarget::Stack, &stack_context)
                        .context(ErrorData::GenericError {
                            message: format!(
                                "failed to generate AWS Terraform management policy for permission set '{}'",
                                permission_set.id
                            ),
                        })?;
                    statements.extend(policy.statement);
                }
            }

            for (resource_id, permission_set_ref) in resource_scoped_permission_refs(profile) {
                if permission_set_ref.id().ends_with("/provision") {
                    continue;
                }
                let Some(resource_entry) = ctx.stack.resources.get(resource_id) else {
                    continue;
                };
                if resource_entry.lifecycle != ResourceLifecycle::Live {
                    continue;
                }
                let Some(permission_set) = permission_set_ref
                    .resolve(|name| alien_permissions::get_permission_set(name).cloned())
                else {
                    continue;
                };
                if permission_set.platforms.aws.is_none() {
                    continue;
                }

                let resource_context =
                    resource_scoped_aws_permission_context(resource_id, resource_entry, &context);
                let policy = generator
                    .generate_policy(&permission_set, BindingTarget::Resource, &resource_context)
                    .context(ErrorData::GenericError {
                        message: format!(
                            "failed to generate AWS Terraform resource-scoped management policy for permission set '{}'",
                            permission_set.id
                        ),
                    })?;
                statements.extend(policy.statement);
            }
        }
        emit_iam_managed_policy_chunks(
            &mut fragment,
            label,
            &format!("{label}_managed_policy"),
            "deployment-management",
            statements,
        )?;

        // The management role always needs to read its own role —
        // `iam:GetRole` is what the manager calls to verify the role
        // still exists at heartbeat time. Lives outside alien-permissions
        // because it's a self-reference the controller can't model
        // through a permission set without weird circular naming.
        fragment.resource_blocks.push(resource_block(
            "aws_iam_role_policy",
            &format!("{label}_self_read"),
            [
                attr(
                    "name",
                    Expression::String("deployment-management-self-read".to_string()),
                ),
                attr("role", expr::traversal(["aws_iam_role", label, "id"])),
                attr("policy", self_read_policy(label)),
            ],
        ));

        Ok(fragment)
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        let label = required_label(ctx)?;
        Ok(expr::object([
            ("roleName", expr::traversal(["aws_iam_role", label, "name"])),
            ("roleArn", expr::traversal(["aws_iam_role", label, "arn"])),
            ("managementPermissionsApplied", Expression::Bool(true)),
        ]))
    }
}

fn trust_policy() -> Expression {
    jsonencode(expr::object([
        ("Version", Expression::String("2012-10-17".to_string())),
        (
            "Statement",
            Expression::Array(vec![expr::object([
                ("Sid", Expression::String("AllowManagingRole".to_string())),
                ("Effect", Expression::String("Allow".to_string())),
                (
                    "Principal",
                    expr::object([("AWS", expr::raw("var.managing_role_arn"))]),
                ),
                ("Action", Expression::String("sts:AssumeRole".to_string())),
                (
                    "Condition",
                    expr::object([(
                        "StringEquals",
                        expr::object([("aws:PrincipalArn", expr::raw("var.managing_role_arn"))]),
                    )]),
                ),
            ])]),
        ),
    ]))
}

fn self_read_policy(label: &str) -> Expression {
    jsonencode(expr::object([
        ("Version", Expression::String("2012-10-17".to_string())),
        (
            "Statement",
            Expression::Array(vec![expr::object([
                (
                    "Sid",
                    Expression::String("ReadOwnManagementRole".to_string()),
                ),
                ("Effect", Expression::String("Allow".to_string())),
                ("Action", Expression::String("iam:GetRole".to_string())),
                ("Resource", expr::traversal(["aws_iam_role", label, "arn"])),
            ])]),
        ),
    ]))
}

fn global_permission_refs(profile: &PermissionProfile) -> Vec<&PermissionSetReference> {
    profile
        .0
        .get("*")
        .map(|refs| refs.iter().collect())
        .unwrap_or_default()
}

fn resource_scoped_permission_refs(
    profile: &PermissionProfile,
) -> Vec<(&str, &PermissionSetReference)> {
    profile
        .0
        .iter()
        .filter(|(scope, _)| scope.as_str() != "*")
        .flat_map(|(resource_id, refs)| {
            refs.iter()
                .map(move |permission_set_ref| (resource_id.as_str(), permission_set_ref))
        })
        .collect()
}

fn resource_scoped_aws_permission_context(
    resource_id: &str,
    resource_entry: &alien_core::ResourceEntry,
    base_context: &PermissionContext,
) -> PermissionContext {
    let mut context = base_context
        .clone()
        .with_resource_id(resource_id.to_string());
    context.resource_name = None;

    if resource_entry.config.downcast_ref::<Worker>().is_some() {
        return context.with_resource_name(format!("${{local.resource_prefix}}-{resource_id}"));
    }

    if let Some(cluster) = resource_entry.config.downcast_ref::<KubernetesCluster>() {
        return context.with_resource_name(kubernetes_cluster_name(cluster));
    }

    context
}

fn kubernetes_cluster_name(cluster: &KubernetesCluster) -> String {
    cluster
        .cloud
        .as_ref()
        .and_then(|cloud| cloud.cluster_name.clone())
        .unwrap_or_else(|| {
            "${var.kubernetes_cluster_mode == \"create\" ? format(\"%s-k8s\", local.resource_prefix) : var.eks_cluster_name}".to_string()
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{Resource, ResourceEntry, ResourceLifecycle, Storage, Worker, WorkerCode};

    fn live_worker_entry(id: &str) -> ResourceEntry {
        ResourceEntry {
            config: Resource::new(
                Worker::new(id.to_string())
                    .code(WorkerCode::Image {
                        image: "test:latest".to_string(),
                    })
                    .permissions("default".to_string())
                    .build(),
            ),
            lifecycle: ResourceLifecycle::Live,
            dependencies: Vec::new(),
            remote_access: false,
            enabled_when: None,
        }
    }

    #[test]
    fn aws_remote_management_resource_scope_names_future_worker_lambda() {
        let entry = live_worker_entry("jobs");
        let context = resource_scoped_aws_permission_context(
            "jobs",
            &entry,
            &aws_terraform_permission_context(),
        );

        assert_eq!(context.resource_id.as_deref(), Some("jobs"));
        assert_eq!(
            context.resource_name.as_deref(),
            Some("${local.resource_prefix}-jobs")
        );
    }

    #[test]
    fn aws_remote_management_resource_scope_does_not_inherit_management_name() {
        let entry = ResourceEntry {
            enabled_when: None,
            config: Resource::new(Storage::new("uploads".to_string()).build()),
            lifecycle: ResourceLifecycle::Live,
            dependencies: Vec::new(),
            remote_access: false,
        };
        let context = resource_scoped_aws_permission_context(
            "uploads",
            &entry,
            &aws_terraform_permission_context().with_resource_name("management"),
        );

        assert_eq!(context.resource_id.as_deref(), Some("uploads"));
        assert_eq!(context.resource_name, None);
    }
}
