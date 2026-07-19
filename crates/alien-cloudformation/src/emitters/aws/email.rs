//! AWS Email — SES email identities, configuration set, and optional
//! inbound/event wiring.
//!
//! Per domain: an `AWS::SES::EmailIdentity` with Easy DKIM enabled, associated
//! with one shared `AWS::SES::ConfigurationSet`. When `events` is configured:
//! an `AWS::SES::ConfigurationSetEventDestination` publishing send / delivery /
//! bounce / complaint / delivery-delay / reject events to an `AWS::SNS::Topic`,
//! which is subscribed to the linked SQS queue (plus the queue policy that
//! allows the topic to send). When `inbound` is configured: an
//! `AWS::SES::ReceiptRuleSet` and an `AWS::SES::ReceiptRule` whose `S3Action`
//! writes raw incoming mail into the linked storage bucket (the bucket policy
//! statement that allows `ses.amazonaws.com` writes is emitted by the storage
//! emitter — S3 supports only one bucket policy resource per bucket).
//!
//! Post-deploy caveat: SES allows only one **active** receipt rule set per
//! account and CloudFormation has no resource that activates one, so the
//! provisioned rule set must be activated manually:
//! `aws ses set-active-receipt-rule-set --rule-set-name <name>`.
//! SES email receiving is also only available in a subset of regions;
//! deploying `inbound` elsewhere fails at the CloudFormation layer.

use crate::{
    emitter::CfEmitter,
    emitters::aws::helpers::{
        logical_id_for_ref, required_logical_id, resource_config, stack_name, tags,
    },
    registry::CfRegistry,
    template::{CfExpression, CfResource},
};
use alien_core::{
    import::EmitContext, ownership_policy_for_resource_type, Email, ErrorData, Queue, ResourceRef,
    ResourceType, Result, Storage,
};
use alien_error::AlienError;
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, Default)]
pub struct AwsEmailEmitter;

impl CfEmitter for AwsEmailEmitter {
    fn emit_resources(&self, ctx: &EmitContext<'_>) -> Result<Vec<CfResource>> {
        let registry = CfRegistry::built_in();
        self.emit_resources_with_registry(ctx, &registry)
    }

    fn emit_resources_with_registry(
        &self,
        ctx: &EmitContext<'_>,
        _registry: &CfRegistry,
    ) -> Result<Vec<CfResource>> {
        let email = resource_config::<Email>(ctx, Email::RESOURCE_TYPE)?;
        validate_domains(email)?;
        let logical_id = required_logical_id(ctx)?;

        let config_set_id = format!("{logical_id}ConfigSet");
        let mut config_set = CfResource::new(
            config_set_id.clone(),
            "AWS::SES::ConfigurationSet".to_string(),
        );
        config_set
            .properties
            .insert("Name".to_string(), stack_name(email.id()));

        let mut resources = vec![config_set];

        for (index, domain) in email.domains.iter().enumerate() {
            let mut identity = CfResource::new(
                identity_logical_id(logical_id, index),
                "AWS::SES::EmailIdentity".to_string(),
            );
            identity.properties.insert(
                "EmailIdentity".to_string(),
                CfExpression::from(domain.clone()),
            );
            identity.properties.insert(
                "DkimAttributes".to_string(),
                CfExpression::object([("SigningEnabled", CfExpression::from(true))]),
            );
            identity.properties.insert(
                "ConfigurationSetAttributes".to_string(),
                CfExpression::object([(
                    "ConfigurationSetName",
                    CfExpression::ref_(&config_set_id),
                )]),
            );
            identity.properties.insert("Tags".to_string(), tags(ctx));
            resources.push(identity);
        }

        if let Some(events) = &email.events {
            let queue_id =
                linked_logical_id(ctx, &events.queue, &Queue::RESOURCE_TYPE, "events.queue")?;
            resources.extend(event_resources(ctx, logical_id, &config_set_id, queue_id));
        }

        if let Some(inbound) = &email.inbound {
            let bucket_id = linked_logical_id(
                ctx,
                &inbound.storage,
                &Storage::RESOURCE_TYPE,
                "inbound.storage",
            )?;
            resources.extend(inbound_resources(email, logical_id, bucket_id));
        }

        Ok(resources)
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<CfExpression> {
        let email = resource_config::<Email>(ctx, Email::RESOURCE_TYPE)?;
        validate_domains(email)?;
        let logical_id = required_logical_id(ctx)?;
        let config_set_id = format!("{logical_id}ConfigSet");

        let domains =
            CfExpression::object(email.domains.iter().enumerate().map(|(index, domain)| {
                let identity_id = identity_logical_id(logical_id, index);
                (
                    domain.as_str(),
                    CfExpression::object([(
                        "dkimTokens",
                        CfExpression::list((1..=3).map(|token| {
                            CfExpression::object([
                                (
                                    "name",
                                    CfExpression::get_att(
                                        &identity_id,
                                        format!("DkimDNSTokenName{token}"),
                                    ),
                                ),
                                (
                                    "value",
                                    CfExpression::get_att(
                                        &identity_id,
                                        format!("DkimDNSTokenValue{token}"),
                                    ),
                                ),
                            ])
                        })),
                    )]),
                )
            }));

        let mut fields = vec![
            ("configurationSet", CfExpression::ref_(&config_set_id)),
            ("domains", domains),
        ];
        if email.inbound.is_some() {
            fields.push((
                "ruleSetName",
                CfExpression::ref_(format!("{logical_id}RuleSet")),
            ));
        }

        Ok(CfExpression::object(fields))
    }

    fn emit_binding_ref(&self, ctx: &EmitContext<'_>) -> Result<Option<CfExpression>> {
        let email = resource_config::<Email>(ctx, Email::RESOURCE_TYPE)?;
        let logical_id = required_logical_id(ctx)?;
        Ok(Some(CfExpression::object([
            ("service", CfExpression::from("ses")),
            (
                "configurationSet",
                CfExpression::ref_(format!("{logical_id}ConfigSet")),
            ),
            (
                "domains",
                CfExpression::list(
                    email
                        .domains
                        .iter()
                        .map(|domain| CfExpression::from(domain.clone())),
                ),
            ),
            ("region", CfExpression::ref_("AWS::Region")),
        ])))
    }
}

fn identity_logical_id(logical_id: &str, index: usize) -> String {
    format!("{logical_id}Identity{index}")
}

fn validate_domains(email: &Email) -> Result<()> {
    if email.domains.is_empty() {
        return Err(AlienError::new(ErrorData::GenericError {
            message: format!(
                "email resource '{}' must configure at least one domain",
                email.id()
            ),
        }));
    }

    let mut seen = BTreeSet::new();
    for domain in &email.domains {
        if !seen.insert(domain.as_str()) {
            return Err(AlienError::new(ErrorData::GenericError {
                message: format!(
                    "email resource '{}' lists domain '{domain}' more than once",
                    email.id()
                ),
            }));
        }
    }

    Ok(())
}

/// Resolve a linked resource's logical id, failing when the reference points
/// at a missing resource, the wrong resource type, or a resource that is not
/// emitted in setup (a live resource has no CloudFormation logical id, so a
/// `GetAtt` against it could never resolve).
fn linked_logical_id<'a>(
    ctx: &'a EmitContext<'_>,
    reference: &ResourceRef,
    expected_type: &ResourceType,
    field: &str,
) -> Result<&'a str> {
    let entry = ctx.stack.resources.get(reference.id()).ok_or_else(|| {
        AlienError::new(ErrorData::GenericError {
            message: format!(
                "email resource '{}' {field} links to missing resource '{}'",
                ctx.resource_id,
                reference.id()
            ),
        })
    })?;

    let actual_type = entry.config.resource_type();
    if &actual_type != expected_type {
        return Err(AlienError::new(ErrorData::UnexpectedResourceType {
            resource_id: reference.id().to_string(),
            expected: expected_type.clone(),
            actual: actual_type,
        }));
    }

    let ownership = ownership_policy_for_resource_type(actual_type.as_ref());
    if !ownership.should_emit_in_setup(entry.lifecycle) {
        return Err(AlienError::new(ErrorData::GenericError {
            message: format!(
                "email resource '{}' {field} links to '{}', which is not emitted in setup; \
                 link a Frozen resource instead",
                ctx.resource_id,
                reference.id()
            ),
        }));
    }

    logical_id_for_ref(ctx, reference)
}

/// SNS topic + SES event destination + SQS subscription and queue policy.
fn event_resources(
    ctx: &EmitContext<'_>,
    logical_id: &str,
    config_set_id: &str,
    queue_id: &str,
) -> Vec<CfResource> {
    let topic_id = format!("{logical_id}EventsTopic");

    let mut topic = CfResource::new(topic_id.clone(), "AWS::SNS::Topic".to_string());
    topic.properties.insert(
        "TopicName".to_string(),
        stack_name(&format!("{}-email-events", ctx.resource_id)),
    );
    topic.properties.insert("Tags".to_string(), tags(ctx));

    let mut destination = CfResource::new(
        format!("{logical_id}EventDestination"),
        "AWS::SES::ConfigurationSetEventDestination".to_string(),
    );
    destination.properties.insert(
        "ConfigurationSetName".to_string(),
        CfExpression::ref_(config_set_id),
    );
    destination.properties.insert(
        "EventDestination".to_string(),
        CfExpression::object([
            ("Name", stack_name(&format!("{}-events", ctx.resource_id))),
            ("Enabled", CfExpression::from(true)),
            (
                "MatchingEventTypes",
                CfExpression::list(
                    [
                        "SEND",
                        "DELIVERY",
                        "BOUNCE",
                        "COMPLAINT",
                        "DELIVERY_DELAY",
                        "REJECT",
                    ]
                    .map(CfExpression::from),
                ),
            ),
            (
                "SnsDestination",
                CfExpression::object([("TopicARN", CfExpression::ref_(&topic_id))]),
            ),
        ]),
    );

    let mut subscription = CfResource::new(
        format!("{logical_id}EventsSubscription"),
        "AWS::SNS::Subscription".to_string(),
    );
    subscription
        .properties
        .insert("TopicArn".to_string(), CfExpression::ref_(&topic_id));
    subscription
        .properties
        .insert("Protocol".to_string(), CfExpression::from("sqs"));
    subscription.properties.insert(
        "Endpoint".to_string(),
        CfExpression::get_att(queue_id, "Arn"),
    );
    subscription
        .properties
        .insert("RawMessageDelivery".to_string(), CfExpression::from(true));

    // Note: SQS supports a single effective policy per queue. If two email
    // resources ever target the same queue, their policy resources would
    // overwrite each other — keep one email resource per events queue.
    let mut queue_policy = CfResource::new(
        format!("{logical_id}EventsQueuePolicy"),
        "AWS::SQS::QueuePolicy".to_string(),
    );
    queue_policy.properties.insert(
        "Queues".to_string(),
        CfExpression::list([CfExpression::ref_(queue_id)]),
    );
    queue_policy.properties.insert(
        "PolicyDocument".to_string(),
        CfExpression::object([
            ("Version", CfExpression::from("2012-10-17")),
            (
                "Statement",
                CfExpression::list([CfExpression::object([
                    ("Sid", CfExpression::from("AllowSesEventsTopic")),
                    ("Effect", CfExpression::from("Allow")),
                    (
                        "Principal",
                        CfExpression::object([(
                            "Service",
                            CfExpression::from("sns.amazonaws.com"),
                        )]),
                    ),
                    ("Action", CfExpression::from("sqs:SendMessage")),
                    ("Resource", CfExpression::get_att(queue_id, "Arn")),
                    (
                        "Condition",
                        CfExpression::object([(
                            "ArnEquals",
                            CfExpression::object([(
                                "aws:SourceArn",
                                CfExpression::ref_(&topic_id),
                            )]),
                        )]),
                    ),
                ])]),
            ),
        ]),
    );

    vec![topic, destination, subscription, queue_policy]
}

/// SES receipt rule set + receipt rule with an S3 action into the linked
/// bucket. The rule depends on the bucket policy resource emitted by the
/// storage emitter, because SES validates its write access to the bucket
/// when the rule is created.
fn inbound_resources(email: &Email, logical_id: &str, bucket_id: &str) -> Vec<CfResource> {
    let rule_set_id = format!("{logical_id}RuleSet");

    let mut rule_set = CfResource::new(rule_set_id.clone(), "AWS::SES::ReceiptRuleSet".to_string());
    rule_set
        .properties
        .insert("RuleSetName".to_string(), stack_name(email.id()));

    let mut rule = CfResource::new(
        format!("{logical_id}InboundRule"),
        "AWS::SES::ReceiptRule".to_string(),
    );
    rule.properties
        .insert("RuleSetName".to_string(), CfExpression::ref_(&rule_set_id));
    rule.properties.insert(
        "Rule".to_string(),
        CfExpression::object([
            ("Name", stack_name(&format!("{}-inbound", email.id()))),
            ("Enabled", CfExpression::from(true)),
            ("ScanEnabled", CfExpression::from(true)),
            (
                "Recipients",
                CfExpression::list(
                    email
                        .domains
                        .iter()
                        .map(|domain| CfExpression::from(domain.clone())),
                ),
            ),
            (
                "Actions",
                CfExpression::list([CfExpression::object([(
                    "S3Action",
                    CfExpression::object([("BucketName", CfExpression::ref_(bucket_id))]),
                )])]),
            ),
        ]),
    );
    // The storage emitter emits `{bucket}BucketPolicy` with the statement that
    // allows ses.amazonaws.com to write; SES rejects the rule without it.
    rule.depends_on.push(format!("{bucket_id}BucketPolicy"));

    vec![rule_set, rule]
}
