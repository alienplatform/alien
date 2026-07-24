//! AWS Email — SES configuration set, optional inbound/event wiring, and
//! seed email identities.
//!
//! CloudFormation owns the email *infrastructure*: one shared
//! `AWS::SES::ConfigurationSet`, plus an `AWS::SES::EmailIdentity` with Easy
//! DKIM enabled for each seed domain. Identities created at runtime (through
//! the `email/manage-identities` grant) are application data — they are not
//! tracked by the stack and survive stack deletion. When `events` is
//! configured: an `AWS::SES::ConfigurationSetEventDestination` publishing
//! send / delivery / bounce / complaint / delivery-delay / reject events to an
//! `AWS::SNS::Topic`, which is subscribed to the linked SQS queue (plus the
//! queue policy that allows the topic to send). When `inbound` is configured:
//! an `AWS::SES::ReceiptRuleSet` and a catch-all `AWS::SES::ReceiptRule` whose
//! `S3Action` writes raw incoming mail into the linked storage bucket (the
//! bucket policy statement that allows `ses.amazonaws.com` writes is emitted
//! by the storage emitter — S3 supports only one bucket policy resource per
//! bucket). Because CloudFormation has no native resource for the account-wide
//! active receipt rule set, a stack-local custom resource activates the
//! provisioned rule set after its receipt rule is ready and deactivates it
//! before deletion. SES email receiving is only available in a subset of regions;
//! deploying `inbound` elsewhere fails at the CloudFormation layer.

use crate::{
    emitter::CfEmitter,
    emitters::aws::{
        helpers::{
            cf_from_json, logical_id_for_ref, required_logical_id, resource_config,
            service_account_role_id, stack_name, tags, uniquify_iam_statement_sids,
        },
        service_account::permission_context,
    },
    registry::CfRegistry,
    template::{CfExpression, CfResource},
};
use alien_core::{
    import::EmitContext, ownership_policy_for_resource_type, Email, ErrorData, PermissionProfile,
    PermissionSetReference, Queue, ResourceRef, ResourceType, Result, ServiceAccount, Storage,
};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_permissions::{generators::AwsCloudFormationPermissionsGenerator, BindingTarget};
use std::collections::BTreeSet;

/// Permission-set id prefix for this resource type.
const PERMISSION_SET_PREFIX: &str = "email/";

const ACTIVE_RULE_SET_HANDLER: &str = r#"import boto3
import cfnresponse

ses = boto3.client("ses")

def handler(event, context):
    desired = event["ResourceProperties"]["RuleSetName"]
    physical_id = event.get("PhysicalResourceId", desired)
    try:
        if event["RequestType"] == "Delete":
            active = ses.describe_active_receipt_rule_set().get("Metadata", {}).get("Name")
            if active == physical_id:
                ses.set_active_receipt_rule_set()
        else:
            ses.set_active_receipt_rule_set(RuleSetName=desired)
            physical_id = desired
        cfnresponse.send(
            event, context, cfnresponse.SUCCESS, {}, physicalResourceId=physical_id
        )
    except Exception as error:
        cfnresponse.send(
            event,
            context,
            cfnresponse.FAILED,
            {},
            physicalResourceId=physical_id,
            reason=str(error),
        )
"#;

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
        validate_inbound_topology(ctx)?;
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
            resources.extend(inbound_resources(ctx, email, logical_id, bucket_id));
        }

        resources.extend(email_iam_policies(ctx, email, logical_id)?);

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

    /// The binding intentionally carries no domain list: identities are
    /// created and removed at runtime, so a list frozen at deploy time would
    /// be stale by design. Applications discover the current identities via
    /// `ses:ListEmailIdentities` (granted by `email/manage-identities`).
    fn emit_binding_ref(&self, ctx: &EmitContext<'_>) -> Result<Option<CfExpression>> {
        let email = resource_config::<Email>(ctx, Email::RESOURCE_TYPE)?;
        let logical_id = required_logical_id(ctx)?;
        let mut fields = vec![
            ("service", CfExpression::from("ses")),
            ("region", CfExpression::ref_("AWS::Region")),
            (
                "configurationSet",
                CfExpression::ref_(format!("{logical_id}ConfigSet")),
            ),
        ];
        if email.events.is_some() {
            fields.push((
                "eventTopicArn",
                CfExpression::ref_(format!("{logical_id}EventsTopic")),
            ));
        }
        Ok(Some(CfExpression::object(fields)))
    }
}

fn identity_logical_id(logical_id: &str, index: usize) -> String {
    format!("{logical_id}Identity{index}")
}

/// Seed domains may be empty — a config-set-only resource is valid, and
/// runtime-created identities are managed outside the deployment — but the
/// same domain must not be listed twice.
fn validate_domains(email: &Email) -> Result<()> {
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

fn validate_inbound_topology(ctx: &EmitContext<'_>) -> Result<()> {
    let inbound_email_ids = ctx
        .stack
        .resources()
        .filter_map(|(id, entry)| {
            entry
                .config
                .downcast_ref::<Email>()
                .filter(|email| email.inbound.is_some())
                .map(|_| id.as_str())
        })
        .collect::<Vec<_>>();

    if inbound_email_ids.len() > 1 {
        return Err(AlienError::new(ErrorData::GenericError {
            message: format!(
                "AWS stacks may contain only one email resource with inbound delivery because \
                 SES has one active receipt rule set per account and region; found: {}",
                inbound_email_ids.join(", ")
            ),
        }));
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
fn inbound_resources(
    ctx: &EmitContext<'_>,
    email: &Email,
    logical_id: &str,
    bucket_id: &str,
) -> Vec<CfResource> {
    let rule_set_id = format!("{logical_id}RuleSet");
    let activator_role_id = format!("{logical_id}RuleSetActivatorRole");
    let activator_function_id = format!("{logical_id}RuleSetActivatorFunction");
    let activation_id = format!("{logical_id}RuleSetActivation");

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
            // No `Recipients` filter: SES treats a rule without recipients as
            // a catch-all matching every recipient the account receives mail
            // for. That way mail addressed to identities verified at runtime
            // (outside this deployment) lands in the bucket without any
            // infrastructure change.
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

    let mut activator_role =
        CfResource::new(activator_role_id.clone(), "AWS::IAM::Role".to_string());
    activator_role.properties.insert(
        "AssumeRolePolicyDocument".to_string(),
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
                            CfExpression::from("lambda.amazonaws.com"),
                        )]),
                    ),
                    ("Action", CfExpression::from("sts:AssumeRole")),
                ])]),
            ),
        ]),
    );
    activator_role.properties.insert(
        "Policies".to_string(),
        CfExpression::list([CfExpression::object([
            (
                "PolicyName",
                CfExpression::from("activate-ses-receipt-rule-set"),
            ),
            (
                "PolicyDocument",
                CfExpression::object([
                    ("Version", CfExpression::from("2012-10-17")),
                    (
                        "Statement",
                        CfExpression::list([
                            CfExpression::object([
                                ("Effect", CfExpression::from("Allow")),
                                (
                                    "Action",
                                    CfExpression::list([
                                        CfExpression::from(
                                            "ses:DescribeActiveReceiptRuleSet",
                                        ),
                                        CfExpression::from("ses:SetActiveReceiptRuleSet"),
                                    ]),
                                ),
                                ("Resource", CfExpression::from("*")),
                            ]),
                            CfExpression::object([
                                ("Effect", CfExpression::from("Allow")),
                                (
                                    "Action",
                                    CfExpression::list([
                                        CfExpression::from("logs:CreateLogGroup"),
                                        CfExpression::from("logs:CreateLogStream"),
                                        CfExpression::from("logs:PutLogEvents"),
                                    ]),
                                ),
                                (
                                    "Resource",
                                    CfExpression::sub(
                                        "arn:${AWS::Partition}:logs:${AWS::Region}:${AWS::AccountId}:*",
                                    ),
                                ),
                            ]),
                        ]),
                    ),
                ]),
            ),
        ])]),
    );

    let mut activator_function = CfResource::new(
        activator_function_id.clone(),
        "AWS::Lambda::Function".to_string(),
    );
    activator_function
        .properties
        .insert("Runtime".to_string(), CfExpression::from("python3.13"));
    activator_function
        .properties
        .insert("Handler".to_string(), CfExpression::from("index.handler"));
    activator_function.properties.insert(
        "Role".to_string(),
        CfExpression::get_att(&activator_role_id, "Arn"),
    );
    activator_function.properties.insert(
        "Code".to_string(),
        CfExpression::object([("ZipFile", CfExpression::from(ACTIVE_RULE_SET_HANDLER))]),
    );
    activator_function
        .properties
        .insert("Timeout".to_string(), CfExpression::from(60_u32));
    activator_function
        .properties
        .insert("Tags".to_string(), tags(ctx));
    activator_function
        .depends_on
        .push(activator_role_id.clone());

    let mut activation = CfResource::new(
        activation_id,
        "AWS::CloudFormation::CustomResource".to_string(),
    );
    activation.properties.insert(
        "ServiceToken".to_string(),
        CfExpression::get_att(&activator_function_id, "Arn"),
    );
    activation
        .properties
        .insert("RuleSetName".to_string(), CfExpression::ref_(&rule_set_id));
    activation
        .depends_on
        .push(format!("{logical_id}InboundRule"));

    vec![
        rule_set,
        rule,
        activator_role,
        activator_function,
        activation,
    ]
}

/// IAM policies attaching granted `email/*` permission sets (send /
/// manage-identities) to the owning service-account roles. Statements keep the
/// generator's own resource patterns: identities are named after customer mail
/// domains (never stack-prefixed), and the configuration-set pattern already
/// resolves through `${AWS::StackName}`.
fn email_iam_policies(
    ctx: &EmitContext<'_>,
    email: &Email,
    logical_id: &str,
) -> Result<Vec<CfResource>> {
    let mut resources = Vec::new();
    let generator = AwsCloudFormationPermissionsGenerator::new();
    let context =
        permission_context().with_resource_name(format!("${{AWS::StackName}}-{}", email.id()));

    for (owner_index, (role_id, permission_refs)) in permission_owners(ctx).into_iter().enumerate()
    {
        for (permission_index, permission_ref) in permission_refs.iter().enumerate() {
            let Some(permission_set) =
                permission_ref.resolve(|name| alien_permissions::get_permission_set(name).cloned())
            else {
                continue;
            };
            if !permission_set.id.starts_with(PERMISSION_SET_PREFIX) {
                continue;
            }

            let policy = generator
                .generate_policy(&permission_set, BindingTarget::Resource, &context)
                .context(ErrorData::GenericError {
                    message: format!(
                        "failed to generate AWS CloudFormation email IAM policy for '{}'",
                        email.id()
                    ),
                })?;
            let policy_value = serde_json::to_value(policy).into_alien_error().context(
                ErrorData::TemplateSerializationFailed {
                    format: "CloudFormation IAM policy".to_string(),
                    reason: "Failed to serialize IAM policy".to_string(),
                },
            )?;
            let CfExpression::Object(mut policy_object) = cf_from_json(policy_value)? else {
                return Err(AlienError::new(ErrorData::TemplateSerializationFailed {
                    format: "CloudFormation IAM policy".to_string(),
                    reason: "policy did not serialize to a JSON object".to_string(),
                }));
            };
            let Some(CfExpression::List(policy_statements)) =
                policy_object.shift_remove("Statement")
            else {
                continue;
            };

            let policy_id =
                format!("{logical_id}{role_id}EmailPermission{owner_index}{permission_index}");
            let mut policy_resource = CfResource::new(policy_id, "AWS::IAM::Policy".to_string());
            policy_resource.properties.insert(
                "PolicyName".to_string(),
                CfExpression::sub(format!(
                    "${{AWS::StackName}}-{}-email-{owner_index}-{permission_index}",
                    email.id()
                )),
            );
            policy_resource.properties.insert(
                "PolicyDocument".to_string(),
                CfExpression::object([
                    ("Version", CfExpression::from("2012-10-17")),
                    (
                        "Statement",
                        CfExpression::list(uniquify_iam_statement_sids(policy_statements)),
                    ),
                ]),
            );
            policy_resource.properties.insert(
                "Roles".to_string(),
                CfExpression::list([CfExpression::ref_(&role_id)]),
            );
            policy_resource.depends_on.push(role_id.clone());
            resources.push(policy_resource);
        }
    }

    Ok(resources)
}

/// Service-account roles whose permission profile references an `email/*`
/// permission set for this resource (either directly by resource id or
/// through a `*` wildcard grant).
fn permission_owners(ctx: &EmitContext<'_>) -> Vec<(String, Vec<PermissionSetReference>)> {
    let mut owners = Vec::new();
    for (profile_name, profile) in ctx.stack.permission_profiles() {
        let refs = resource_permission_refs(profile, ctx.resource_id);
        if refs.is_empty() {
            continue;
        }

        let service_account_id = format!("{profile_name}-sa");
        if service_account_for_id(ctx, &service_account_id).is_some() {
            if let Some(role_id) = service_account_role_id(ctx, profile_name) {
                owners.push((role_id, refs));
            }
        }
    }
    owners
}

fn resource_permission_refs(
    profile: &PermissionProfile,
    resource_id: &str,
) -> Vec<PermissionSetReference> {
    let mut refs = Vec::new();
    let mut seen_ids = std::collections::HashSet::new();

    if let Some(resource_refs) = profile.0.get(resource_id) {
        for permission_ref in resource_refs
            .iter()
            .filter(|permission_ref| permission_ref.id().starts_with(PERMISSION_SET_PREFIX))
        {
            if seen_ids.insert(permission_ref.id().to_string()) {
                refs.push(permission_ref.clone());
            }
        }
    }

    if let Some(wildcard_refs) = profile.0.get("*") {
        for permission_ref in wildcard_refs
            .iter()
            .filter(|permission_ref| permission_ref.id().starts_with(PERMISSION_SET_PREFIX))
        {
            if seen_ids.insert(permission_ref.id().to_string()) {
                refs.push(permission_ref.clone());
            }
        }
    }

    refs
}

fn service_account_for_id<'a>(
    ctx: &'a EmitContext<'_>,
    service_account_id: &str,
) -> Option<&'a ServiceAccount> {
    let (_id, entry) = ctx
        .stack
        .resources()
        .find(|(id, _entry)| id.as_str() == service_account_id)?;
    entry.config.downcast_ref::<ServiceAccount>()
}
