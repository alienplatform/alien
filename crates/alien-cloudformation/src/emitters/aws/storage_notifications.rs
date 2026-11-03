//! Setup-owned S3 queue notifications and the queue ingress they need.

use crate::{
    emitters::aws::{helpers::logical_id_for_ref, storage::bucket_name},
    template::{CfExpression, CfResource},
};
use alien_core::{
    Result, Storage,
    import::EmitContext,
    storage_notifications::{AWS_QUEUE_INGRESS_SID, storages_notifying_queue},
};

/// The bucket's `NotificationConfiguration`, one queue configuration per rule, and the
/// queue policies S3 validates when the configuration is written.
pub(super) fn notification_configuration(
    ctx: &EmitContext<'_>,
    storage: &Storage,
) -> Result<(CfExpression, Vec<String>)> {
    let mut configurations = Vec::new();
    let mut dependencies = Vec::new();
    for notification in &storage.notifications {
        let queue_id = logical_id_for_ref(ctx, &notification.queue)?;
        let mut rules = Vec::new();
        for (name, value) in [
            ("prefix", &notification.prefix),
            ("suffix", &notification.suffix),
        ] {
            if let Some(value) = value {
                rules.push(CfExpression::object([
                    ("Name", CfExpression::from(name)),
                    ("Value", CfExpression::from(value.as_str())),
                ]));
            }
        }
        let mut configuration = vec![
            ("Event", CfExpression::from("s3:ObjectCreated:*")),
            ("Queue", CfExpression::get_att(queue_id, "Arn")),
        ];
        if !rules.is_empty() {
            configuration.push((
                "Filter",
                CfExpression::object([(
                    "S3Key",
                    CfExpression::object([("Rules", CfExpression::list(rules))]),
                )]),
            ));
        }
        configurations.push(CfExpression::object(configuration));
        let policy = format!("{queue_id}StorageIngress");
        if !dependencies.contains(&policy) {
            dependencies.push(policy);
        }
    }
    Ok((
        CfExpression::object([("QueueConfigurations", CfExpression::list(configurations))]),
        dependencies,
    ))
}

/// Lets S3 deliver this stack's notifications. The bucket ARNs are built from their
/// names rather than `Fn::GetAtt`, because each bucket depends on this policy: S3
/// validates the destination when the notification configuration is written.
pub(super) fn queue_policy(ctx: &EmitContext<'_>, queue_id: &str) -> CfResource {
    let mut policy = CfResource::new(
        format!("{queue_id}StorageIngress"),
        "AWS::SQS::QueuePolicy".into(),
    );
    policy.properties.insert(
        "Queues".into(),
        CfExpression::list([CfExpression::ref_(queue_id)]),
    );
    let buckets = storages_notifying_queue(ctx.stack, ctx.resource_id)
        .iter()
        .map(|storage_id| {
            CfExpression::object([(
                "Fn::Join",
                CfExpression::list([
                    CfExpression::from(""),
                    CfExpression::list([
                        CfExpression::from("arn:"),
                        CfExpression::ref_("AWS::Partition"),
                        CfExpression::from(":s3:::"),
                        bucket_name(storage_id),
                    ]),
                ]),
            )])
        })
        .collect::<Vec<_>>();
    policy.properties.insert(
        "PolicyDocument".into(),
        CfExpression::object([
            ("Version", CfExpression::from("2012-10-17")),
            (
                "Statement",
                CfExpression::list([CfExpression::object([
                    ("Sid", CfExpression::from(AWS_QUEUE_INGRESS_SID)),
                    ("Effect", CfExpression::from("Allow")),
                    (
                        "Principal",
                        CfExpression::object([("Service", CfExpression::from("s3.amazonaws.com"))]),
                    ),
                    ("Action", CfExpression::from("sqs:SendMessage")),
                    ("Resource", CfExpression::get_att(queue_id, "Arn")),
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
                                    CfExpression::list(buckets),
                                )]),
                            ),
                        ]),
                    ),
                ])]),
            ),
        ]),
    );
    policy
}
