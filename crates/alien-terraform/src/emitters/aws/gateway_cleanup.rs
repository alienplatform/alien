//! Setup-owned retirement capability; runtime receives only a published version.

use super::helpers::{jsonencode, service_assume_role_policy, tags};
use crate::{
    block::{attr, block, data_block, nested, resource_block},
    emitter::TfFragment,
    expr,
};
use alien_core::{
    aws_worker_gateway::{CLEANUP_FUNCTION_PREFIX, CLEANUP_HANDLER_SOURCE, CLEANUP_RUNTIME},
    import::EmitContext,
};
use hcl::expr::Expression;

pub(super) fn emit(ctx: &EmitContext<'_>, management_label: &str, fragment: &mut TfFragment) {
    let label = format!("{management_label}_gateway_cleanup");
    let name = expr::template(format!(
        "{CLEANUP_FUNCTION_PREFIX}${{local.resource_prefix}}"
    ));
    fragment
        .data_blocks
        .push(data_block("aws_partition", &label, []));
    fragment.data_blocks.push(data_block(
        "archive_file",
        &label,
        [
            attr("type", Expression::from("zip")),
            attr("source_content", Expression::from(CLEANUP_HANDLER_SOURCE)),
            attr("source_content_filename", Expression::from("index.py")),
            attr(
                "output_path",
                expr::template(format!("${{path.module}}/.terraform/{label}.zip")),
            ),
        ],
    ));
    fragment.resource_blocks.push(resource_block(
        "aws_iam_role",
        &label,
        [
            attr("name", name.clone()),
            // A slash separates this role from all flat runtime role-prefix grants.
            attr("path", Expression::from("/setup/")),
            attr(
                "assume_role_policy",
                service_assume_role_policy(&["lambda.amazonaws.com"]),
            ),
            attr("tags", tags(ctx, "management")),
        ],
    ));
    let task_arns = expr::template(format!(
        "arn:${{data.aws_partition.{label}.partition}}:ecs:${{data.aws_region.current.region}}:${{data.aws_caller_identity.current.account_id}}:task-definition/${{local.resource_prefix}}-*-gateway-*:*"
    ));
    let statements = vec![
        statement(
            "ReadGatewayOwnership",
            &["ecs:ListTagsForResource"],
            task_arns.clone(),
        ),
        // ECS does not support a resource constraint for deregistration. The
        // immutable handler checks exact native ownership before this call.
        statement(
            "RetireValidatedGateway",
            &["ecs:DeregisterTaskDefinition"],
            Expression::from("*"),
        ),
        statement(
            "DeleteGatewayRevision",
            &["ecs:DeleteTaskDefinitions"],
            task_arns,
        ),
    ];
    let execution_policy = format!("{label}_execution");
    fragment.resource_blocks.push(resource_block(
        "aws_iam_role_policy",
        &execution_policy,
        [
            attr("name", Expression::from("gateway-retirement")),
            attr("role", expr::traversal(["aws_iam_role", &label, "id"])),
            attr("policy", policy(statements)),
        ],
    ));
    fragment.resource_blocks.push(resource_block(
        "aws_lambda_function",
        &label,
        [
            attr("function_name", name),
            attr(
                "description",
                Expression::from("Retire owned private gateway task revisions"),
            ),
            attr("role", expr::traversal(["aws_iam_role", &label, "arn"])),
            attr("runtime", Expression::from(CLEANUP_RUNTIME)),
            attr("handler", Expression::from("index.handler")),
            attr("timeout", Expression::from(60)),
            attr("publish", Expression::Bool(true)),
            attr(
                "filename",
                expr::traversal(["data", "archive_file", &label, "output_path"]),
            ),
            attr(
                "source_code_hash",
                expr::traversal(["data", "archive_file", &label, "output_base64sha256"]),
            ),
            nested(block(
                "environment",
                [attr(
                    "variables",
                    expr::object([("ALIEN_STACK_PREFIX", expr::raw("local.resource_prefix"))]),
                )],
            )),
            attr("tags", tags(ctx, "management")),
            attr(
                "depends_on",
                Expression::Array(vec![expr::traversal([
                    "aws_iam_role_policy",
                    &execution_policy,
                ])]),
            ),
        ],
    ));
    fragment.resource_blocks.push(resource_block(
        "aws_iam_role_policy",
        &format!("{label}_invoke"),
        [
            attr("name", Expression::from("gateway-retirement-invoke")),
            attr(
                "role",
                expr::traversal(["aws_iam_role", management_label, "id"]),
            ),
            attr(
                "policy",
                policy(vec![statement(
                    "InvokeGatewayRetirement",
                    &["lambda:InvokeFunction"],
                    version_arn(management_label),
                )]),
            ),
        ],
    ));
}

pub(super) fn version_arn(management_label: &str) -> Expression {
    expr::traversal([
        "aws_lambda_function",
        &format!("{management_label}_gateway_cleanup"),
        "qualified_arn",
    ])
}

fn statement(sid: &str, actions: &[&str], resource: Expression) -> Expression {
    expr::object([
        ("Sid", Expression::from(sid)),
        ("Effect", Expression::from("Allow")),
        (
            "Action",
            Expression::Array(
                actions
                    .iter()
                    .map(|action| Expression::from(*action))
                    .collect(),
            ),
        ),
        ("Resource", resource),
    ])
}

fn policy(statements: Vec<Expression>) -> Expression {
    jsonencode(expr::object([
        ("Version", Expression::from("2012-10-17")),
        ("Statement", Expression::Array(statements)),
    ]))
}
