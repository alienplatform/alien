//! Native S3 EventBridge configuration, routing and queue ingress.

use crate::{
    block::{attr, block, nested, resource_block},
    emitter::TfFragment,
    expr,
};
use alien_core::{
    ErrorData, Result, Storage,
    import::EmitContext,
    storage_notifications::{
        AWS_NOTIFICATION_INPUT_TEMPLATE, aws_eventbridge_pattern, aws_notification_input_paths,
        aws_notification_rule_name, aws_notification_rule_name_pattern,
    },
};
use alien_error::{AlienError, Context, IntoAlienError};
use hcl::{
    expr::{Expression, ObjectKey},
    structure::Block,
};

pub(super) fn emit(
    ctx: &EmitContext<'_>,
    fragment: &mut TfFragment,
    label: &str,
    storage: &Storage,
) -> Result<()> {
    if storage.notifications.is_empty() {
        return Ok(());
    }
    let switch_label = format!("{label}_eventbridge");
    // Setup owns the entire notification configuration once this is declared.
    // The preflight forbids mixing this source with Worker storage triggers.
    fragment.resource_blocks.push(resource_block(
        "aws_s3_bucket_notification",
        &switch_label,
        [
            attr(
                "bucket",
                expr::traversal(["aws_s3_bucket", label, "bucket"]),
            ),
            attr("eventbridge", Expression::Bool(true)),
        ],
    ));
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
        let rule_label = format!("{label}_notification_{index}");
        // Preserve literal interpolation syntax in user filters: only the bucket
        // field is a Terraform traversal, all remaining JSON values are literals.
        let mut pattern = hcl::to_expression(&aws_eventbridge_pattern(notification, ""))
            .into_alien_error()
            .context(ErrorData::TemplateSerializationFailed {
                format: "Terraform storage event pattern".into(),
                reason: "Could not encode event pattern".into(),
            })?;
        *object_field(object_field(&mut pattern, "detail")?, "bucket")? = expr::object([(
            "name",
            Expression::Array(vec![expr::traversal(["aws_s3_bucket", label, "bucket"])]),
        )]);
        fragment.resource_blocks.push(resource_block(
            "aws_cloudwatch_event_rule",
            &rule_label,
            [
                attr(
                    "name",
                    expr::template(aws_notification_rule_name(
                        "${local.resource_prefix}",
                        storage.id(),
                        index,
                    )),
                ),
                attr(
                    "description",
                    expr::template(format!(
                        "Storage queue notifications for ${{aws_s3_bucket.{label}.bucket}}"
                    )),
                ),
                attr("event_pattern", expr::jsonencode(pattern)),
                attr("state", Expression::String("ENABLED".into())),
                attr(
                    "depends_on",
                    Expression::Array(vec![
                        expr::traversal(["aws_s3_bucket_notification", &switch_label]),
                        expr::traversal([
                            "aws_sqs_queue_policy",
                            &format!("{queue_label}_storage_ingress"),
                        ]),
                    ]),
                ),
            ],
        ));
        fragment.resource_blocks.push(resource_block(
            "aws_cloudwatch_event_target",
            &rule_label,
            [
                attr(
                    "rule",
                    expr::traversal(["aws_cloudwatch_event_rule", &rule_label, "name"]),
                ),
                attr("target_id", Expression::String("queue".into())),
                attr(
                    "arn",
                    expr::traversal(["aws_sqs_queue", queue_label, "arn"]),
                ),
                nested(block(
                    "input_transformer",
                    [
                        attr(
                            "input_paths",
                            expr::object(
                                aws_notification_input_paths()
                                    .into_iter()
                                    .map(|(key, value)| (key, Expression::String(value))),
                            ),
                        ),
                        attr(
                            "input_template",
                            Expression::String(AWS_NOTIFICATION_INPUT_TEMPLATE.into()),
                        ),
                    ],
                )),
            ],
        ));
    }
    Ok(())
}

pub(super) fn queue_policy(label: &str) -> Block {
    resource_block(
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
                            (
                                "Sid",
                                Expression::String("StorageNotificationIngress".into()),
                            ),
                            ("Effect", Expression::String("Allow".into())),
                            (
                                "Principal",
                                expr::object([(
                                    "Service",
                                    Expression::String("events.amazonaws.com".into()),
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
                                    (
                                        "ArnLike",
                                        expr::object([(
                                            "aws:SourceArn",
                                            expr::template(format!(
                                                "arn:aws:events:${{var.aws_region}}:${{data.aws_caller_identity.current.account_id}}:rule/{}",
                                                aws_notification_rule_name_pattern(
                                                    "${local.resource_prefix}"
                                                )
                                            )),
                                        )]),
                                    ),
                                ]),
                            ),
                        ])]),
                    ),
                ])),
            ),
        ],
    )
}

/// Replace the known dynamic field without interpreting any user filter as HCL.
fn object_field<'a>(value: &'a mut Expression, name: &str) -> Result<&'a mut Expression> {
    if let Expression::Object(fields) = value {
        if let Some((_, value)) = fields.iter_mut().find(|(key, _)| match key {
            ObjectKey::Identifier(key) => key.as_str() == name,
            ObjectKey::Expression(Expression::String(key)) => key == name,
            _ => false,
        }) {
            return Ok(value);
        }
    }
    Err(AlienError::new(ErrorData::TemplateSerializationFailed {
        format: "Terraform storage event pattern".into(),
        reason: format!("Shared event pattern is missing object field '{name}'"),
    }))
}
