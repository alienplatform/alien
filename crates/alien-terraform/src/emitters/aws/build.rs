//! AWS Build — CodeBuild project plus log group and (fallback) IAM role.

use crate::{
    block::{attr, resource_block},
    emitter::{TfEmitter, TfFragment},
    emitters::aws::helpers::{
        downcast, iam_role_name_template, jsonencode, nested_block, required_label,
        resource_prefix_template, service_account_role_arn, service_assume_role_policy, tags,
    },
    expr,
};
use alien_core::{import::EmitContext, Build, ComputeType, Result};
use hcl::expr::Expression;

#[derive(Debug, Clone, Copy, Default)]
pub struct AwsBuildEmitter;

impl TfEmitter for AwsBuildEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let build = downcast::<Build>(ctx, Build::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;
        let mut fragment = TfFragment::default();

        let role_arn = match service_account_role_arn(ctx, &build.permissions) {
            Some(arn) => arn,
            None => {
                let role_label = format!("{label}_role");
                fragment.resource_blocks.push(resource_block(
                    "aws_iam_role",
                    &role_label,
                    [
                        attr(
                            "name",
                            iam_role_name_template(&format!("{}-build", build.id)),
                        ),
                        attr(
                            "assume_role_policy",
                            service_assume_role_policy(&["codebuild.amazonaws.com"]),
                        ),
                        attr("tags", tags(ctx, "build")),
                    ],
                ));
                fragment.resource_blocks.push(resource_block(
                    "aws_iam_role_policy",
                    &format!("{role_label}_logs"),
                    [
                        attr(
                            "name",
                            Expression::String("alien-managed-policy".to_string()),
                        ),
                        attr("role", expr::traversal(["aws_iam_role", &role_label, "id"])),
                        attr("policy", logs_policy()),
                    ],
                ));
                expr::traversal(["aws_iam_role", &role_label, "arn"])
            }
        };

        let log_group_label = format!("{label}_logs");
        fragment.resource_blocks.push(resource_block(
            "aws_cloudwatch_log_group",
            &log_group_label,
            [
                attr(
                    "name",
                    expr::template(format!(
                        "/aws/codebuild/${{local.resource_prefix}}-{}",
                        build.id
                    )),
                ),
                attr(
                    "retention_in_days",
                    Expression::Number(hcl::Number::from(30i64)),
                ),
                attr("tags", tags(ctx, "build")),
            ],
        ));

        let mut env_vars: Vec<Expression> = Vec::new();
        for (name, value) in &build.environment {
            env_vars.push(expr::object([
                ("name", Expression::String(name.clone())),
                ("value", Expression::String(value.clone())),
                ("type", Expression::String("PLAINTEXT".to_string())),
            ]));
        }

        fragment.resource_blocks.push(resource_block(
            "aws_codebuild_project",
            label,
            [
                attr("name", resource_prefix_template(&build.id)),
                attr("service_role", role_arn),
                nested_block(
                    "artifacts",
                    vec![attr("type", Expression::String("NO_ARTIFACTS".to_string()))],
                ),
                {
                    let mut body = vec![
                        attr("type", Expression::String("LINUX_CONTAINER".to_string())),
                        attr(
                            "image",
                            Expression::String("aws/codebuild/standard:7.0".to_string()),
                        ),
                        attr(
                            "compute_type",
                            Expression::String(
                                codebuild_compute_type(&build.compute_type).to_string(),
                            ),
                        ),
                        attr("privileged_mode", Expression::Bool(true)),
                    ];
                    if !env_vars.is_empty() {
                        for var in &env_vars {
                            body.push(crate::block::nested(crate::block::block(
                                "environment_variable",
                                {
                                    let Expression::Object(o) = var else { continue };
                                    let mut inner = Vec::new();
                                    for (key, value) in o {
                                        let key_str = match key {
                                            hcl::expr::ObjectKey::Identifier(id) => {
                                                id.as_str().to_string()
                                            }
                                            other => format!("{other}"),
                                        };
                                        inner.push(attr(&key_str, value.clone()));
                                    }
                                    inner
                                },
                            )));
                        }
                    }
                    nested_block("environment", body)
                },
                nested_block(
                    "source",
                    vec![
                        attr("type", Expression::String("NO_SOURCE".to_string())),
                        attr(
                            "buildspec",
                            Expression::String(default_buildspec().to_string()),
                        ),
                    ],
                ),
                nested_block(
                    "logs_config",
                    vec![nested_block(
                        "cloudwatch_logs",
                        vec![
                            attr("status", Expression::String("ENABLED".to_string())),
                            attr(
                                "group_name",
                                expr::traversal([
                                    "aws_cloudwatch_log_group",
                                    &log_group_label,
                                    "name",
                                ]),
                            ),
                        ],
                    )],
                ),
                attr("tags", tags(ctx, "build")),
            ],
        ));

        Ok(fragment)
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        let build = downcast::<Build>(ctx, Build::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;
        let env: Vec<(String, Expression)> = build
            .environment
            .iter()
            .map(|(key, value)| (key.clone(), Expression::String(value.clone())))
            .collect();
        Ok(expr::object([
            (
                "projectName",
                expr::traversal(["aws_codebuild_project", label, "name"]),
            ),
            (
                "projectArn",
                expr::traversal(["aws_codebuild_project", label, "arn"]),
            ),
            (
                "buildEnvVars",
                expr::object(env.iter().map(|(k, v)| (k.as_str(), v.clone()))),
            ),
        ]))
    }

    fn emit_binding_ref(&self, ctx: &EmitContext<'_>) -> Result<Option<Expression>> {
        let build = downcast::<Build>(ctx, Build::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;
        let env: Vec<(String, Expression)> = build
            .environment
            .iter()
            .map(|(key, value)| (key.clone(), Expression::String(value.clone())))
            .collect();
        Ok(Some(expr::object([
            ("service", Expression::String("codebuild".to_string())),
            (
                "projectName",
                expr::traversal(["aws_codebuild_project", label, "name"]),
            ),
            (
                "buildEnvVars",
                expr::object(env.iter().map(|(k, v)| (k.as_str(), v.clone()))),
            ),
            ("monitoring", Expression::Null),
        ])))
    }
}

fn logs_policy() -> Expression {
    jsonencode(expr::object([
        ("Version", Expression::String("2012-10-17".to_string())),
        (
            "Statement",
            Expression::Array(vec![expr::object([
                ("Sid", Expression::String("WriteLogs".to_string())),
                ("Effect", Expression::String("Allow".to_string())),
                (
                    "Action",
                    Expression::Array(vec![
                        Expression::String("logs:CreateLogGroup".to_string()),
                        Expression::String("logs:CreateLogStream".to_string()),
                        Expression::String("logs:PutLogEvents".to_string()),
                    ]),
                ),
                ("Resource", Expression::String("*".to_string())),
            ])]),
        ),
    ]))
}

fn codebuild_compute_type(compute_type: &ComputeType) -> &'static str {
    match compute_type {
        ComputeType::Small => "BUILD_GENERAL1_SMALL",
        ComputeType::Medium => "BUILD_GENERAL1_MEDIUM",
        ComputeType::Large | ComputeType::XLarge => "BUILD_GENERAL1_LARGE",
    }
}

fn default_buildspec() -> &'static str {
    r#"version: 0.2
phases:
  build:
    commands:
      - echo "Build project is ready for runtime-provided commands"
"#
}
