//! AWS ArtifactRegistry — ECR repository plus pull and push IAM roles.

use crate::{
    block::{attr, nested, resource_block},
    emitter::{TfEmitter, TfFragment},
    emitters::aws::helpers::{
        downcast, jsonencode, nested_block, required_label, stack_name_template, tags,
    },
    expr,
};
use alien_core::{import::EmitContext, ArtifactRegistry, Result, ServiceAccount};
use hcl::expr::Expression;

#[derive(Debug, Clone, Copy, Default)]
pub struct AwsArtifactRegistryEmitter;

impl TfEmitter for AwsArtifactRegistryEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let registry = downcast::<ArtifactRegistry>(ctx, ArtifactRegistry::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;
        let pull_label = format!("{label}_pull");
        let push_label = format!("{label}_push");

        let mut fragment = TfFragment::default();

        fragment.resource_blocks.push(resource_block(
            "aws_ecr_repository",
            label,
            [
                attr("name", stack_name_template(registry.id())),
                attr(
                    "image_tag_mutability",
                    Expression::String("IMMUTABLE".to_string()),
                ),
                nested_block(
                    "image_scanning_configuration",
                    vec![attr("scan_on_push", Expression::Bool(true))],
                ),
                nested_block(
                    "encryption_configuration",
                    vec![attr(
                        "encryption_type",
                        Expression::String("AES256".to_string()),
                    )],
                ),
                attr("force_delete", Expression::Bool(false)),
                attr("tags", tags(ctx, "artifact-registry")),
                nested(crate::block::block(
                    "lifecycle",
                    [attr("prevent_destroy", Expression::Bool(false))],
                )),
            ],
        ));

        fragment.resource_blocks.push(resource_block(
            "aws_ecr_lifecycle_policy",
            label,
            [
                attr(
                    "repository",
                    expr::traversal(["aws_ecr_repository", label, "name"]),
                ),
                attr("policy", lifecycle_policy()),
            ],
        ));

        fragment
            .resource_blocks
            .push(ecr_role(ctx, &pull_label, registry, "pull", false));
        fragment
            .resource_blocks
            .push(ecr_role(ctx, &push_label, registry, "push", true));
        fragment
            .resource_blocks
            .push(ecr_role_policy(label, &pull_label, /* push */ false));
        fragment
            .resource_blocks
            .push(ecr_role_policy(label, &push_label, /* push */ true));

        Ok(fragment)
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        let label = required_label(ctx)?;
        let registry = downcast::<ArtifactRegistry>(ctx, ArtifactRegistry::RESOURCE_TYPE)?;
        Ok(expr::object([
            (
                "accountId",
                expr::raw("data.aws_caller_identity.current.account_id"),
            ),
            ("region", expr::raw("data.aws_region.current.region")),
            (
                "registryId",
                expr::template(
                    "${data.aws_caller_identity.current.account_id}:${data.aws_region.current.region}".to_string(),
                ),
            ),
            (
                "registryEndpoint",
                expr::traversal(["aws_ecr_repository", label, "registry_id"]),
            ),
            (
                "repositoryPrefix",
                expr::template(format!("${{var.stack_name}}-{}", registry.id())),
            ),
            (
                "pullRoleArn",
                expr::traversal(["aws_iam_role", &format!("{label}_pull"), "arn"]),
            ),
            (
                "pushRoleArn",
                expr::traversal(["aws_iam_role", &format!("{label}_push"), "arn"]),
            ),
        ]))
    }
}

fn ecr_role(
    ctx: &EmitContext<'_>,
    label: &str,
    registry: &ArtifactRegistry,
    suffix: &str,
    _push: bool,
) -> hcl::structure::Block {
    let principals = trust_principals(ctx);
    resource_block(
        "aws_iam_role",
        label,
        [
            attr(
                "name",
                stack_name_template(&format!("{}-{suffix}", registry.id())),
            ),
            attr("assume_role_policy", ecr_role_trust_policy(&principals)),
            attr("tags", tags(ctx, "artifact-registry")),
        ],
    )
}

fn ecr_role_policy(repo_label: &str, role_label: &str, push: bool) -> hcl::structure::Block {
    let mut actions = vec![
        Expression::String("ecr:BatchCheckLayerAvailability".to_string()),
        Expression::String("ecr:BatchGetImage".to_string()),
        Expression::String("ecr:DescribeImages".to_string()),
        Expression::String("ecr:DescribeRepositories".to_string()),
        Expression::String("ecr:GetDownloadUrlForLayer".to_string()),
        Expression::String("ecr:ListImages".to_string()),
    ];
    if push {
        for action in [
            "ecr:CompleteLayerUpload",
            "ecr:InitiateLayerUpload",
            "ecr:PutImage",
            "ecr:UploadLayerPart",
        ] {
            actions.push(Expression::String(action.to_string()));
        }
    }

    let policy = jsonencode(expr::object([
        ("Version", Expression::String("2012-10-17".to_string())),
        (
            "Statement",
            Expression::Array(vec![
                expr::object([
                    (
                        "Sid",
                        Expression::String("GetAuthorizationToken".to_string()),
                    ),
                    ("Effect", Expression::String("Allow".to_string())),
                    (
                        "Action",
                        Expression::String("ecr:GetAuthorizationToken".to_string()),
                    ),
                    ("Resource", Expression::String("*".to_string())),
                ]),
                expr::object([
                    ("Sid", Expression::String("RepositoryAccess".to_string())),
                    ("Effect", Expression::String("Allow".to_string())),
                    ("Action", Expression::Array(actions)),
                    (
                        "Resource",
                        expr::traversal(["aws_ecr_repository", repo_label, "arn"]),
                    ),
                ]),
            ]),
        ),
    ]));

    resource_block(
        "aws_iam_role_policy",
        &format!("{role_label}_policy"),
        [
            attr(
                "name",
                Expression::String(if push { "ecr-push-pull" } else { "ecr-pull" }.to_string()),
            ),
            attr("role", expr::traversal(["aws_iam_role", role_label, "id"])),
            attr("policy", policy),
        ],
    )
}

fn trust_principals(ctx: &EmitContext<'_>) -> Vec<Expression> {
    let mut principals = Vec::new();
    for (id, entry) in ctx.stack.resources() {
        if entry.config.downcast_ref::<ServiceAccount>().is_some() {
            if let Some(label) = ctx.name_for(id) {
                principals.push(expr::traversal(["aws_iam_role", label, "arn"]));
            }
        }
    }
    if principals.is_empty() {
        principals.push(expr::raw("var.managing_role_arn"));
    }
    principals
}

fn ecr_role_trust_policy(principals: &[Expression]) -> Expression {
    let principal_value = if principals.len() == 1 {
        principals[0].clone()
    } else {
        Expression::Array(principals.to_vec())
    };
    jsonencode(expr::object([
        ("Version", Expression::String("2012-10-17".to_string())),
        (
            "Statement",
            Expression::Array(vec![expr::object([
                ("Effect", Expression::String("Allow".to_string())),
                ("Principal", expr::object([("AWS", principal_value)])),
                ("Action", Expression::String("sts:AssumeRole".to_string())),
            ])]),
        ),
    ]))
}

fn lifecycle_policy() -> Expression {
    jsonencode(expr::object([(
        "rules",
        Expression::Array(vec![expr::object([
            ("rulePriority", Expression::Number(hcl::Number::from(1i64))),
            (
                "description",
                Expression::String("Retain the 100 most recent images".to_string()),
            ),
            (
                "selection",
                expr::object([
                    ("tagStatus", Expression::String("any".to_string())),
                    (
                        "countType",
                        Expression::String("imageCountMoreThan".to_string()),
                    ),
                    ("countNumber", Expression::Number(hcl::Number::from(100i64))),
                ]),
            ),
            (
                "action",
                expr::object([("type", Expression::String("expire".to_string()))]),
            ),
        ])]),
    )]))
}
