//! Setup-owned S3 queue notifications and the queue ingress they need.

use crate::{
    block::{attr, block, nested, resource_block},
    emitter::TfFragment,
    expr,
};
use alien_core::{
    ErrorData, Result, Storage,
    import::EmitContext,
    storage_notifications::{AWS_QUEUE_INGRESS_SID, aws_notification_id, storages_notifying_queue},
};
use alien_error::AlienError;
use hcl::{expr::Expression, structure::Block};

pub(super) fn emit(
    ctx: &EmitContext<'_>,
    fragment: &mut TfFragment,
    label: &str,
    storage: &Storage,
) -> Result<()> {
    if storage.notifications.is_empty() {
        return Ok(());
    }
    let mut body = vec![attr(
        "bucket",
        expr::traversal(["aws_s3_bucket", label, "bucket"]),
    )];
    let mut policies = Vec::new();
    for (index, notification) in storage.notifications.iter().enumerate() {
        let queue_label = ctx.name_for(&notification.queue.id).ok_or_else(|| {
            AlienError::new(ErrorData::GenericError {
                message: format!(
                    "Storage '{}' notification {index} queue '{}' has no Terraform resource",
                    storage.id(),
                    notification.queue.id
                ),
            })
        })?;
        let mut queue = vec![
            attr(
                "id",
                expr::template(aws_notification_id(
                    "${local.resource_prefix}",
                    storage.id(),
                    index,
                )),
            ),
            attr(
                "queue_arn",
                expr::traversal(["aws_sqs_queue", queue_label, "arn"]),
            ),
            attr(
                "events",
                Expression::Array(vec![Expression::String("s3:ObjectCreated:*".into())]),
            ),
        ];
        if let Some(prefix) = &notification.prefix {
            queue.push(attr("filter_prefix", Expression::String(prefix.clone())));
        }
        if let Some(suffix) = &notification.suffix {
            queue.push(attr("filter_suffix", Expression::String(suffix.clone())));
        }
        body.push(nested(block("queue", queue)));
        let policy = expr::traversal([
            "aws_sqs_queue_policy",
            &format!("{queue_label}_storage_ingress"),
        ]);
        if !policies.contains(&policy) {
            policies.push(policy);
        }
    }
    // S3 validates each destination when the configuration is written.
    body.push(attr("depends_on", Expression::Array(policies)));
    // Setup owns the bucket's whole notification configuration once notifications are
    // declared; a preflight forbids Worker storage triggers on the same bucket.
    fragment.resource_blocks.push(resource_block(
        "aws_s3_bucket_notification",
        &format!("{label}_notifications"),
        body,
    ));
    // Destroying it writes an empty document. When the bucket is retained, retention has
    // already removed exactly the entries setup wrote, so detach it with the bucket and
    // keep every entry someone else added.
    fragment.retain_state(|kind, _| kind == "aws_s3_bucket_notification");
    Ok(())
}

/// Lets S3 deliver this stack's notifications, from exactly the notifying buckets. Also
/// returns the deployer inputs the policy should follow: when every notifying Storage can
/// be turned off, the policy only exists while one of them is on.
pub(super) fn queue_policy(ctx: &EmitContext<'_>, label: &str) -> Result<(Block, Vec<String>)> {
    let mut buckets = Vec::new();
    let mut gates = Vec::new();
    // A gated queue already gates its own policy.
    let mut unconditional = ctx.resource.enabled_when.is_some();
    for storage_id in storages_notifying_queue(ctx.stack, ctx.resource_id) {
        let storage_label = ctx.name_for(&storage_id).ok_or_else(|| {
            AlienError::new(ErrorData::GenericError {
                message: format!("Storage '{storage_id}' has no Terraform resource"),
            })
        })?;
        // A splat yields no ARN for a bucket the deployer turned off.
        buckets.push(expr::raw(format!("aws_s3_bucket.{storage_label}[*].arn")));
        match ctx
            .stack
            .resources()
            .find(|(id, _)| **id == storage_id)
            .and_then(|(_, entry)| entry.enabled_when.clone())
        {
            Some(input) => gates.push(input),
            None => unconditional = true,
        }
    }
    if unconditional {
        gates.clear();
    }
    let sources = Expression::FuncCall(Box::new(
        hcl::expr::FuncCall::builder(hcl::Identifier::sanitized("flatten"))
            .arg(Expression::Array(buckets))
            .build(),
    ));
    let policy = resource_block(
        "aws_sqs_queue_policy",
        &format!("{label}_storage_ingress"),
        [
            attr(
                "queue_url",
                expr::traversal(["aws_sqs_queue", label, "url"]),
            ),
            attr(
                "policy",
                expr::jsonencode(expr::object([
                    ("Version", Expression::String("2012-10-17".into())),
                    (
                        "Statement",
                        Expression::Array(vec![expr::object([
                            ("Sid", Expression::String(AWS_QUEUE_INGRESS_SID.into())),
                            ("Effect", Expression::String("Allow".into())),
                            (
                                "Principal",
                                expr::object([(
                                    "Service",
                                    Expression::String("s3.amazonaws.com".into()),
                                )]),
                            ),
                            ("Action", Expression::String("sqs:SendMessage".into())),
                            ("Resource", expr::traversal(["aws_sqs_queue", label, "arn"])),
                            (
                                "Condition",
                                expr::object([
                                    (
                                        "StringEquals",
                                        expr::object([(
                                            "aws:SourceAccount",
                                            expr::traversal([
                                                "data",
                                                "aws_caller_identity",
                                                "current",
                                                "account_id",
                                            ]),
                                        )]),
                                    ),
                                    ("ArnLike", expr::object([("aws:SourceArn", sources)])),
                                ]),
                            ),
                        ])]),
                    ),
                ])),
            ),
        ],
    );
    Ok((policy, gates))
}
