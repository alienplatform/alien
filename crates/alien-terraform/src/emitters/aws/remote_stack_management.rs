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
        downcast, emit_iam_managed_policy_chunks, ensure_unique_statement_sids,
        iam_role_name_template, jsonencode, jsonencode_policy_value, managing_role_trust_statement,
        policy_document_expr, required_label, stack_permission_context, tags,
    },
    expr,
};
use alien_core::{
    import::EmitContext, ErrorData, KubernetesCluster, PermissionProfile, PermissionSetReference,
    RemoteStackManagement, ResourceLifecycle, Result, Worker,
};
use alien_error::{AlienError, Context};
use alien_permissions::{
    generators::{aws_stack_permissions_boundary, AwsIamStatement, AwsRuntimePermissionsGenerator},
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

        fragment.resource_blocks.push(resource_block(
            "aws_iam_policy",
            &format!("{label}_permissions_boundary"),
            [
                attr(
                    "name",
                    expr::template("${local.resource_prefix}-permissions-boundary"),
                ),
                attr("policy", boundary_document(ctx)?),
                attr("tags", tags(ctx, "management")),
            ],
        ));
        let generator = AwsRuntimePermissionsGenerator::new();
        let context = stack_permission_context(ctx);
        let stack_context = context.clone().with_resource_name("management".to_string());
        let mut statements = Vec::<AwsIamStatement>::new();
        // Keyed by gate input so resources sharing one input share a policy.
        let mut gated_statements =
            std::collections::BTreeMap::<String, Vec<AwsIamStatement>>::new();
        if let Some(profile) = ctx.stack.management().profile() {
            for permission_set_ref in global_permission_refs(ctx, profile) {
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
                // A grant naming a resource the deployer can decline has to
                // follow that resource's gate. The shared managed policy is
                // unconditional, so these statements go to their own gated
                // policy instead of surviving the resource they name.
                match resource_entry.enabled_when.as_deref() {
                    Some(input_id) => gated_statements
                        .entry(input_id.to_string())
                        .or_default()
                        .extend(policy.statement),
                    None => statements.extend(policy.statement),
                }
            }
        }
        emit_iam_managed_policy_chunks(
            &mut fragment,
            label,
            &format!("{label}_managed_policy"),
            "deployment-management",
            statements,
        )?;
        for (input_id, mut statements) in gated_statements {
            // Two resources gated on one input each contribute the same
            // permission set, so their statements arrive with identical Sids.
            // IAM rejects a policy document that repeats one.
            ensure_unique_statement_sids(&mut statements);
            let suffix = crate::generator::stack_input_variable_name(&input_id);
            let block = resource_block(
                "aws_iam_role_policy",
                &format!("{label}_{suffix}"),
                [
                    attr(
                        "name",
                        Expression::String(format!("deployment-management-{suffix}")),
                    ),
                    attr("role", expr::traversal(["aws_iam_role", label, "id"])),
                    attr("policy", policy_document_expr(statements)?),
                ],
            );
            fragment.push_gated_resource(block, std::slice::from_ref(&input_id));
        }

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
        let fields = vec![
            ("roleName", expr::traversal(["aws_iam_role", label, "name"])),
            ("roleArn", expr::traversal(["aws_iam_role", label, "arn"])),
            ("managementPermissionsApplied", Expression::Bool(true)),
            (
                "permissionsBoundaryArn",
                expr::traversal([
                    "aws_iam_policy",
                    &format!("{label}_permissions_boundary"),
                    "arn",
                ]),
            ),
            (
                "permissionsBoundaryDocument",
                expr::parse(&format!(
                    "jsondecode(aws_iam_policy.{label}_permissions_boundary.policy)"
                ))?,
            ),
        ];
        Ok(expr::object(fields))
    }
}

fn boundary_document(ctx: &EmitContext<'_>) -> Result<Expression> {
    let generate = |context: &PermissionContext| {
        aws_stack_permissions_boundary(ctx.stack, context, |_, scope| {
            boundary_grant_context(ctx, context, scope)
        })
        .context(ErrorData::GenericError {
            message: "failed to generate AWS permissions boundary".to_string(),
        })
    };
    // Template tokens must not hide an oversized resolved document. Key UUID
    // budgets are distinct so separate physical grants cannot collapse here.
    generate(
        &PermissionContext::new()
            .with_stack_prefix("s".repeat(40))
            .with_aws_region("r".repeat(16))
            .with_aws_account_id("123456789012")
            .with_managing_role_arn(format!(
                "arn:aws:iam::210987654321:role/{}",
                "r".repeat(2019)
            ))
            .with_managing_account_id("210987654321"),
    )?;
    let context = PermissionContext::new()
        .with_stack_prefix("${Prefix}")
        .with_aws_region("${Region}")
        .with_aws_account_id("${Account}")
        .with_managing_account_id("${ManagingAccount}")
        .with_managing_role_arn("${ManagingRole}");
    let mut document = generate(&context)?;
    let mut replacements = vec![
        (
            "${Prefix}".to_string(),
            "${local.resource_prefix}".to_string(),
        ),
        (
            "${Region}".to_string(),
            "${data.aws_region.current.region}".to_string(),
        ),
        (
            "${Account}".to_string(),
            "${data.aws_caller_identity.current.account_id}".to_string(),
        ),
        (
            "${ManagingAccount}".to_string(),
            "${var.managing_account_id}".to_string(),
        ),
        (
            "${ManagingRole}".to_string(),
            "${var.managing_role_arn}".to_string(),
        ),
    ];
    for (index, (id, entry)) in ctx.stack.resources.iter().enumerate() {
        if entry.config.resource_type() == alien_core::Key::RESOURCE_TYPE {
            if let Some(label) = ctx.name_for(id) {
                let key = format!("arn:aws:kms:${{Region}}:${{Account}}:key/${{Key{index}}}");
                let value = if let Some(input) = &entry.enabled_when {
                    if document["Statement"].as_array().is_some_and(|statements| {
                        statements.iter().any(|statement| {
                            statement["NotAction"] != "iam:*"
                                && statement.to_string().contains(&key)
                        })
                    }) {
                        return Err(AlienError::new(ErrorData::GenericError {
                            message: format!("AWS permissions boundary key '{id}' used by inline grants must be unconditional"),
                        }));
                    }
                    format!(
                        "${{var.{} ? local.{label}_selected_key.arn : \"\"}}",
                        crate::generator::stack_input_variable_name(input)
                    )
                } else {
                    format!("${{local.{label}_selected_key.arn}}")
                };
                replacements.insert(0, (key, value));
            }
        }
    }
    replace_boundary_tokens(&mut document, &replacements);
    let mut expression = jsonencode_policy_value(document);
    // Disabled keys contribute no ARN, while the shared ceiling retains its
    // unconditional stack-prefixed resources. Never dereference a disabled key.
    if ctx.stack.resources.values().any(|entry| {
        entry.config.resource_type() == alien_core::Key::RESOURCE_TYPE
            && entry.enabled_when.is_some()
    }) {
        if let Expression::FuncCall(call) = &mut expression {
            if let Some(Expression::Object(fields)) = call.args.first_mut() {
                if let Some(Expression::Array(statements)) =
                    fields.get_mut(&hcl::Identifier::sanitized("Statement").into())
                {
                    for statement in statements {
                        let Expression::Object(fields) = statement else {
                            continue;
                        };
                        if fields.get(&hcl::Identifier::sanitized("NotAction").into())
                            != Some(&Expression::String("iam:*".into()))
                        {
                            continue;
                        }
                        if let Some(resource @ Expression::Array(_)) =
                            fields.get_mut(&hcl::Identifier::sanitized("Resource").into())
                        {
                            *resource = Expression::FuncCall(Box::new(
                                hcl::expr::FuncCall::builder("compact")
                                    .arg(std::mem::replace(resource, Expression::Null))
                                    .build(),
                            ));
                        }
                    }
                }
            }
        }
    }
    Ok(expression)
}

fn replace_boundary_tokens(value: &mut serde_json::Value, replacements: &[(String, String)]) {
    match value {
        serde_json::Value::String(value) => {
            for (from, to) in replacements {
                *value = value.replace(from, to);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                replace_boundary_tokens(item, replacements);
            }
        }
        serde_json::Value::Object(fields) => {
            for item in fields.values_mut() {
                replace_boundary_tokens(item, replacements);
            }
        }
        _ => {}
    }
}

fn boundary_grant_context(
    ctx: &EmitContext<'_>,
    base: &PermissionContext,
    scope: &str,
) -> alien_permissions::Result<PermissionContext> {
    alien_permissions::generators::aws_boundary_scope_context(ctx.stack, base, scope, |scope| {
        if ctx.stack.resources[scope].config.resource_type() != alien_core::Key::RESOURCE_TYPE {
            return Ok(None);
        }
        let _label = ctx.name_for(scope).ok_or_else(|| {
            AlienError::new(alien_permissions::ErrorData::InvalidPermissionSet {
                message: format!("AWS permissions boundary key '{scope}' has no setup resource"),
            })
        })?;
        let index = ctx.stack.resources.get_index_of(scope).ok_or_else(|| {
            AlienError::new(alien_permissions::ErrorData::InvalidPermissionSet {
                message: format!("AWS permissions boundary key '{scope}' is missing"),
            })
        })?;
        Ok(Some(if base.stack_prefix.as_deref() == Some("${Prefix}") {
            format!("arn:aws:kms:${{Region}}:${{Account}}:key/${{Key{index}}}")
        } else {
            format!(
                "arn:aws:kms:{}:{}:key/00000000-0000-0000-0000-{index:012x}",
                base.aws_region.as_deref().unwrap_or_default(),
                base.aws_account_id.as_deref().unwrap_or_default()
            )
        }))
    })
}

fn trust_policy() -> Expression {
    jsonencode(expr::object([
        ("Version", Expression::String("2012-10-17".to_string())),
        (
            "Statement",
            Expression::Array(vec![managing_role_trust_statement()]),
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

fn global_permission_refs<'a>(
    ctx: &EmitContext<'_>,
    profile: &'a PermissionProfile,
) -> Vec<&'a PermissionSetReference> {
    alien_permissions::management_identity_global_refs(ctx.stack.resources.values(), profile)
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
    use crate::emitters::aws::helpers::aws_terraform_permission_context;
    use alien_core::{
        KubernetesClusterOwnership, KubernetesClusterProvider, KubernetesHeartbeatMode, Network,
        NetworkSettings, PermissionSet, Platform, Resource, ResourceEntry, ResourceLifecycle,
        Stack, StackSettings, Storage, Worker, WorkerCode,
    };
    use hcl::eval::{Context, Evaluate};
    use serde_json::json;

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
            display_name: None,
            description: None,
            remote_access: false,
            enabled_when: None,
        }
    }

    #[test]
    fn installed_boundary_matches_runtime_for_default_cluster_and_literal_grants() {
        let grant = |resource: &str| -> PermissionSet {
            serde_json::from_value(json!({
                "id": "custom/read", "description": "Read the approved resource",
                "platforms": {"aws": [{"grant": {"actions": ["eks:DescribeCluster"]},
                    "binding": {"resource": {"resources": [resource]}}}]}
            }))
            .unwrap()
        };
        let stack = Stack::new("test".to_string())
            .add(
                KubernetesCluster::new("cluster".to_string())
                    .provider(KubernetesClusterProvider::Eks)
                    .ownership(KubernetesClusterOwnership::Managed)
                    .namespace("workloads".to_string())
                    .heartbeat_mode(KubernetesHeartbeatMode::KubernetesApi)
                    .build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                Network::new("network".to_string())
                    .settings(NetworkSettings::UseDefault)
                    .build(),
                ResourceLifecycle::Frozen,
            )
            .permission(
                "reader",
                PermissionProfile::new()
                    .resource(
                        "cluster",
                        [grant(
                            "arn:aws:eks:${awsRegion}:${awsAccountId}:cluster/${resourceName}",
                        )],
                    )
                    .resource(
                        "network",
                        [grant("arn:aws:eks:us-east-1:123456789012:cluster/approved")],
                    ),
            )
            .build();
        let ctx = EmitContext {
            stack: &stack,
            resource: &stack.resources["cluster"],
            resource_id: "cluster",
            platform: Platform::Aws,
            targets_kubernetes: true,
            stack_settings: &StackSettings::default(),
            names: &Default::default(),
        };
        let emitted = boundary_document(&ctx).unwrap();
        let Expression::FuncCall(call) = emitted else {
            panic!("expected jsonencode")
        };
        let mut evaluation = Context::new();
        for (name, value) in [
            ("local", json!({"resource_prefix": "app"})),
            (
                "data",
                json!({"aws_region": {"current": {"region": "us-east-1"}}, "aws_caller_identity": {"current": {"account_id": "123456789012"}}}),
            ),
            (
                "var",
                json!({"managing_account_id": "210987654321", "managing_role_arn": "arn:aws:iam::210987654321:role/manager"}),
            ),
        ] {
            evaluation.declare_var(name, hcl::to_value(value).unwrap());
        }
        let installed = serde_json::to_value(call.args[0].evaluate(&evaluation).unwrap()).unwrap();
        let base = PermissionContext::new()
            .with_stack_prefix("app")
            .with_aws_region("us-east-1")
            .with_aws_account_id("123456789012")
            .with_managing_account_id("210987654321")
            .with_managing_role_arn("arn:aws:iam::210987654321:role/manager");
        let required = aws_stack_permissions_boundary(&stack, &base, |_, scope| {
            alien_permissions::generators::aws_boundary_scope_context(&stack, &base, scope, |_| {
                Ok(None)
            })
        })
        .unwrap();
        assert_eq!(installed, required);
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
            display_name: None,
            description: None,
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
