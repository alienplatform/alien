//! Setup-owned EventBridge rules and queue ingress.

use crate::{
    emitters::aws::helpers::{cf_from_json, logical_id_for_ref},
    template::{CfExpression, CfResource},
};
use alien_core::{
    Result, Storage,
    import::EmitContext,
    storage_notifications::{
        AWS_NOTIFICATION_INPUT_TEMPLATE, aws_eventbridge_pattern, aws_notification_input_paths,
        aws_notification_rule_name,
    },
};
use serde_json::json;

pub(super) fn resources(
    ctx: &EmitContext<'_>,
    storage: &Storage,
    bucket_id: &str,
) -> Result<Vec<CfResource>> {
    if storage.notifications.is_empty() {
        return Ok(vec![]);
    }
    let mut resources = Vec::new();
    for (index, notification) in storage.notifications.iter().enumerate() {
        let queue_id = logical_id_for_ref(ctx, &notification.queue)?;
        let mut rule = CfResource::new(
            format!("{bucket_id}Notification{index}"),
            "AWS::Events::Rule".into(),
        );
        rule.properties.insert(
            "Name".into(),
            CfExpression::sub(aws_notification_rule_name(
                "${AWS::StackName}",
                storage.id(),
                index,
            )),
        );
        rule.properties.insert(
            "Description".into(),
            CfExpression::object([(
                "Fn::Join",
                CfExpression::list([
                    CfExpression::from(""),
                    CfExpression::list([
                        CfExpression::from("Storage queue notifications for "),
                        CfExpression::ref_(bucket_id),
                    ]),
                ]),
            )]),
        );
        let mut pattern = aws_eventbridge_pattern(notification, "");
        pattern["detail"]["bucket"]["name"] = json!([{"Ref": bucket_id}]);
        rule.properties
            .insert("EventPattern".into(), cf_from_json(pattern)?);
        rule.properties
            .insert("State".into(), CfExpression::from("ENABLED"));
        rule.properties.insert(
            "Targets".into(),
            CfExpression::list([CfExpression::object([
                ("Id", CfExpression::from("queue")),
                ("Arn", CfExpression::get_att(queue_id, "Arn")),
                (
                    "InputTransformer",
                    CfExpression::object([
                        (
                            "InputPathsMap",
                            CfExpression::object(
                                aws_notification_input_paths()
                                    .into_iter()
                                    .map(|(key, path)| (key, CfExpression::from(path))),
                            ),
                        ),
                        (
                            "InputTemplate",
                            CfExpression::from(AWS_NOTIFICATION_INPUT_TEMPLATE),
                        ),
                    ]),
                ),
            ])]),
        );
        rule.depends_on
            .extend([bucket_id.to_string(), format!("{queue_id}StorageIngress")]);
        resources.push(rule);
    }
    Ok(resources)
}

pub(super) fn queue_policy(queue_id: &str) -> CfResource {
    let mut policy = CfResource::new(
        format!("{queue_id}StorageIngress"),
        "AWS::SQS::QueuePolicy".into(),
    );
    policy.properties.insert(
        "Queues".into(),
        CfExpression::list([CfExpression::ref_(queue_id)]),
    );
    policy.properties.insert("PolicyDocument".into(), CfExpression::object([
        ("Version", CfExpression::from("2012-10-17")),
        ("Statement", CfExpression::list([CfExpression::object([
            ("Sid", CfExpression::from("StorageNotificationIngress")),
            ("Effect", CfExpression::from("Allow")),
            ("Principal", CfExpression::object([("Service", CfExpression::from("events.amazonaws.com"))])),
            ("Action", CfExpression::from("sqs:SendMessage")),
            ("Resource", CfExpression::get_att(queue_id, "Arn")),
            ("Condition", CfExpression::object([
                ("StringEquals", CfExpression::object([("aws:SourceAccount", CfExpression::ref_("AWS::AccountId"))])),
                ("ArnLike", CfExpression::object([("aws:SourceArn", CfExpression::sub("arn:${AWS::Partition}:events:${AWS::Region}:${AWS::AccountId}:rule/${AWS::StackName}-sn-*"))])),
            ])),
        ])])),
    ]));
    policy
}
