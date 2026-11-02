//! VPC flow logs for a VPC the template creates.
//!
//! The flow log sends every accepted and rejected flow to a log group that keeps records
//! for [`AWS_VPC_FLOW_LOG_RETENTION_DAYS`]. The flow logs service writes to the group with a
//! role that can only write to that group. All three are deleted with the VPC.

use crate::{
    emitters::aws::helpers::tags,
    template::{CfExpression, CfResource},
};
use alien_core::{import::EmitContext, AWS_VPC_FLOW_LOG_RETENTION_DAYS};

/// The log group, role and flow log for the VPC `vpc_id`, under `condition`.
pub(crate) fn resources(
    ctx: &EmitContext<'_>,
    prefix: &str,
    vpc_id: &str,
    condition: &str,
) -> Vec<CfResource> {
    let log_group_id = format!("{prefix}FlowLogGroup");
    let role_id = format!("{prefix}FlowLogRole");

    let mut log_group = CfResource::new(log_group_id.clone(), "AWS::Logs::LogGroup".to_string());
    log_group.properties.insert(
        "RetentionInDays".to_string(),
        CfExpression::Integer(AWS_VPC_FLOW_LOG_RETENTION_DAYS.into()),
    );
    log_group.properties.insert("Tags".to_string(), tags(ctx));

    let mut role = CfResource::new(role_id.clone(), "AWS::IAM::Role".to_string());
    role.properties.insert(
        "AssumeRolePolicyDocument".to_string(),
        flow_logs_trust_policy(),
    );
    role.properties.insert(
        "Policies".to_string(),
        CfExpression::list([CfExpression::object([
            ("PolicyName", CfExpression::from("flow-logs-delivery")),
            (
                "PolicyDocument",
                CfExpression::object([
                    ("Version", CfExpression::from("2012-10-17")),
                    (
                        "Statement",
                        CfExpression::list([CfExpression::object([
                            ("Effect", CfExpression::from("Allow")),
                            (
                                "Action",
                                CfExpression::list(
                                    [
                                        "logs:CreateLogStream",
                                        "logs:PutLogEvents",
                                        "logs:DescribeLogGroups",
                                        "logs:DescribeLogStreams",
                                    ]
                                    .map(CfExpression::from),
                                ),
                            ),
                            // `Arn` of a log group ends in `:*`, so it covers its streams.
                            ("Resource", CfExpression::get_att(&log_group_id, "Arn")),
                        ])]),
                    ),
                ]),
            ),
        ])]),
    );
    role.properties.insert("Tags".to_string(), tags(ctx));

    let mut flow_log = CfResource::new(format!("{prefix}FlowLog"), "AWS::EC2::FlowLog".to_string());
    flow_log
        .properties
        .insert("ResourceId".to_string(), CfExpression::ref_(vpc_id));
    flow_log
        .properties
        .insert("ResourceType".to_string(), CfExpression::from("VPC"));
    flow_log
        .properties
        .insert("TrafficType".to_string(), CfExpression::from("ALL"));
    flow_log.properties.insert(
        "LogDestinationType".to_string(),
        CfExpression::from("cloud-watch-logs"),
    );
    flow_log.properties.insert(
        "LogGroupName".to_string(),
        CfExpression::ref_(&log_group_id),
    );
    flow_log.properties.insert(
        "DeliverLogsPermissionArn".to_string(),
        CfExpression::get_att(&role_id, "Arn"),
    );
    flow_log.properties.insert(
        "MaxAggregationInterval".to_string(),
        CfExpression::from(600u32),
    );
    flow_log.properties.insert("Tags".to_string(), tags(ctx));

    let mut resources = vec![log_group, role, flow_log];
    for resource in &mut resources {
        resource.condition = Some(condition.to_string());
    }
    resources
}

/// Lets only the flow logs service, acting for a flow log in this account and region,
/// assume the role.
fn flow_logs_trust_policy() -> CfExpression {
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
                        CfExpression::from("vpc-flow-logs.amazonaws.com"),
                    )]),
                ),
                ("Action", CfExpression::from("sts:AssumeRole")),
                (
                    "Condition",
                    CfExpression::object([
                        (
                            "StringEquals",
                            CfExpression::object([(
                                "aws:SourceAccount",
                                CfExpression::ref_("AWS::AccountId"),
                            )]),
                        ),
                        (
                            "ArnLike",
                            CfExpression::object([(
                                "aws:SourceArn",
                                CfExpression::sub(
                                    "arn:${AWS::Partition}:ec2:${AWS::Region}:${AWS::AccountId}:vpc-flow-log/*",
                                ),
                            )]),
                        ),
                    ]),
                ),
            ])]),
        ),
    ])
}
