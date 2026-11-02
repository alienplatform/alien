//! VPC flow logs for a VPC the module creates.
//!
//! The flow log sends every accepted and rejected flow to a log group that keeps records
//! for [`AWS_VPC_FLOW_LOG_RETENTION_DAYS`]. The flow logs service writes to the group with a
//! role that can only write to that group. All three are destroyed with the VPC.

use crate::{
    block::{attr, resource_block},
    emitters::aws::helpers::{iam_role_name_template, jsonencode, policy_attr},
    expr,
};
use alien_core::AWS_VPC_FLOW_LOG_RETENTION_DAYS;
use hcl::{expr::Expression, structure::Block};

/// The log group, role, role policy and flow log for `vpc_id`, created when `count` is 1.
/// `name` is the resource id the names are derived from.
pub(crate) fn blocks(
    label: &str,
    name: &str,
    count: &str,
    vpc_id: Expression,
    tags: Expression,
) -> Vec<Block> {
    let flow_logs_label = format!("{label}_flow_logs");
    let log_group_name = format!("${{local.resource_prefix}}-{name}-flow-logs");
    let log_group_arn = format!("aws_cloudwatch_log_group.{flow_logs_label}[0].arn");
    // Spelled out rather than read from the log group, so the IAM size check can bound it.
    let log_group_policy_arn = format!(
        "arn:${{data.aws_partition.current.partition}}:logs:${{data.aws_region.current.region}}:${{data.aws_caller_identity.current.account_id}}:log-group:{log_group_name}"
    );
    let role = format!("aws_iam_role.{flow_logs_label}[0]");
    vec![
        resource_block(
            "aws_cloudwatch_log_group",
            &flow_logs_label,
            [
                attr("count", expr::raw(count)),
                attr("name", expr::template(&log_group_name)),
                attr(
                    "retention_in_days",
                    Expression::Number(hcl::Number::from(i64::from(
                        AWS_VPC_FLOW_LOG_RETENTION_DAYS,
                    ))),
                ),
                attr("tags", tags.clone()),
            ],
        ),
        resource_block(
            "aws_iam_role",
            &flow_logs_label,
            [
                attr("count", expr::raw(count)),
                attr("name", iam_role_name_template(&format!("{name}-flow-logs"))),
                attr("assume_role_policy", trust_policy()),
                attr("tags", tags.clone()),
            ],
        ),
        resource_block(
            "aws_iam_role_policy",
            &flow_logs_label,
            [
                attr("count", expr::raw(count)),
                attr("name", Expression::String("flow-logs-delivery".to_string())),
                attr("role", expr::raw(format!("{role}.id"))),
                policy_attr(vec![expr::object([
                    ("Effect", Expression::String("Allow".to_string())),
                    (
                        "Action",
                        Expression::Array(
                            [
                                "logs:CreateLogStream",
                                "logs:PutLogEvents",
                                "logs:DescribeLogGroups",
                                "logs:DescribeLogStreams",
                            ]
                            .map(|action| Expression::String(action.to_string()))
                            .to_vec(),
                        ),
                    ),
                    // The group and its streams.
                    (
                        "Resource",
                        Expression::Array(vec![
                            expr::template(&log_group_policy_arn),
                            expr::template(format!("{log_group_policy_arn}:*")),
                        ]),
                    ),
                ])]),
            ],
        ),
        resource_block(
            "aws_flow_log",
            label,
            [
                attr("count", expr::raw(count)),
                attr("vpc_id", vpc_id),
                attr("traffic_type", Expression::String("ALL".to_string())),
                attr(
                    "log_destination_type",
                    Expression::String("cloud-watch-logs".to_string()),
                ),
                attr("log_destination", expr::raw(&log_group_arn)),
                attr("iam_role_arn", expr::raw(format!("{role}.arn"))),
                attr(
                    "max_aggregation_interval",
                    Expression::Number(hcl::Number::from(600i64)),
                ),
                attr("tags", tags),
            ],
        ),
    ]
}

/// Lets only the flow logs service, acting for a flow log in this account and region,
/// assume the role.
fn trust_policy() -> Expression {
    jsonencode(expr::object([
        ("Version", Expression::String("2012-10-17".to_string())),
        (
            "Statement",
            Expression::Array(vec![expr::object([
                ("Effect", Expression::String("Allow".to_string())),
                (
                    "Principal",
                    expr::object([(
                        "Service",
                        Expression::String("vpc-flow-logs.amazonaws.com".to_string()),
                    )]),
                ),
                ("Action", Expression::String("sts:AssumeRole".to_string())),
                (
                    "Condition",
                    expr::object([
                        (
                            "StringEquals",
                            expr::object([(
                                "aws:SourceAccount",
                                expr::raw("data.aws_caller_identity.current.account_id"),
                            )]),
                        ),
                        (
                            "ArnLike",
                            expr::object([(
                                "aws:SourceArn",
                                expr::template(
                                    "arn:${data.aws_partition.current.partition}:ec2:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:vpc-flow-log/*",
                                ),
                            )]),
                        ),
                    ]),
                ),
            ])]),
        ),
    ]))
}
