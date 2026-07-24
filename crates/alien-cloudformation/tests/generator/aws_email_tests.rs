//! AWS Email scenarios — SES identities, configuration set, inbound and
//! event wiring.

use super::helpers::render_built_ins;
use alien_cloudformation::{CfRegistry, RegistrationMode};
use alien_core::{
    import::EmitContext, AwsEmailImportData, Email, EmailEvents, EmailInbound, Platform, Queue,
    ResourceLifecycle, ResourceRef, Stack, StackSettings, Storage,
};
use indexmap::IndexMap;

fn email_stack() -> Stack {
    let mailbox = Storage::new("mailbox".to_string()).build();
    let mail_events = Queue::new("mail-events".to_string()).build();
    let email = Email::new("mailer".to_string())
        .domains(vec![
            "mail.example.com".to_string(),
            "mail.example.org".to_string(),
        ])
        .inbound(EmailInbound {
            storage: ResourceRef::from(&mailbox),
        })
        .events(EmailEvents {
            queue: ResourceRef::from(&mail_events),
        })
        .build();

    Stack::new("email".to_string())
        .add(mailbox, ResourceLifecycle::Frozen)
        .add(mail_events, ResourceLifecycle::Frozen)
        .add(email, ResourceLifecycle::Frozen)
        .build()
}

#[test]
fn aws_email_renders_ses_infrastructure() {
    let yaml = render_built_ins(
        &email_stack(),
        StackSettings::default(),
        RegistrationMode::OutputsFallback,
        "aws email",
    );

    let template: serde_json::Value =
        serde_yaml::from_str(&yaml).expect("template YAML should parse");
    let resources = &template["Resources"];

    // One identity per domain, each with Easy DKIM and the shared config set.
    for (index, domain) in ["mail.example.com", "mail.example.org"].iter().enumerate() {
        let identity = &resources[format!("MailerIdentity{index}")];
        assert_eq!(identity["Type"], "AWS::SES::EmailIdentity");
        assert_eq!(identity["Properties"]["EmailIdentity"], *domain);
        assert_eq!(
            identity["Properties"]["DkimAttributes"]["SigningEnabled"],
            true
        );
        assert_eq!(
            identity["Properties"]["ConfigurationSetAttributes"]["ConfigurationSetName"]["Ref"],
            "MailerConfigSet"
        );
    }
    assert_eq!(
        resources["MailerConfigSet"]["Type"],
        "AWS::SES::ConfigurationSet"
    );

    // Event wiring: config set → SNS topic → linked SQS queue.
    let destination = &resources["MailerEventDestination"]["Properties"]["EventDestination"];
    assert_eq!(
        destination["MatchingEventTypes"],
        serde_json::json!([
            "SEND",
            "DELIVERY",
            "BOUNCE",
            "COMPLAINT",
            "DELIVERY_DELAY",
            "REJECT"
        ])
    );
    assert_eq!(
        destination["SnsDestination"]["TopicARN"]["Ref"],
        "MailerEventsTopic"
    );
    let subscription = &resources["MailerEventsSubscription"]["Properties"];
    assert_eq!(subscription["Protocol"], "sqs");
    assert_eq!(
        subscription["Endpoint"]["Fn::GetAtt"],
        serde_json::json!(["MailEvents", "Arn"])
    );
    let queue_policy_statement =
        &resources["MailerEventsQueuePolicy"]["Properties"]["PolicyDocument"]["Statement"][0];
    assert_eq!(
        queue_policy_statement["Principal"]["Service"],
        "sns.amazonaws.com"
    );
    assert_eq!(
        queue_policy_statement["Condition"]["ArnEquals"]["aws:SourceArn"]["Ref"],
        "MailerEventsTopic"
    );

    // Inbound wiring: rule set + rule writing into the linked bucket, and the
    // bucket policy grants ses.amazonaws.com the write (scoped to the account).
    assert_eq!(
        resources["MailerRuleSet"]["Type"],
        "AWS::SES::ReceiptRuleSet"
    );
    let rule = &resources["MailerInboundRule"];
    // No Recipients filter: the rule is a catch-all so that mail for
    // identities verified at runtime lands in the bucket without any
    // infrastructure change.
    assert!(rule["Properties"]["Rule"]
        .as_object()
        .expect("rule properties")
        .get("Recipients")
        .is_none());
    assert_eq!(
        rule["Properties"]["Rule"]["Actions"][0]["S3Action"]["BucketName"]["Ref"],
        "Mailbox"
    );
    assert_eq!(
        rule["DependsOn"],
        serde_json::json!(["MailboxBucketPolicy"])
    );
    let activation = &resources["MailerRuleSetActivation"];
    assert_eq!(activation["Type"], "AWS::CloudFormation::CustomResource");
    assert_eq!(
        activation["Properties"]["RuleSetName"]["Ref"],
        "MailerRuleSet"
    );
    assert_eq!(
        activation["DependsOn"],
        serde_json::json!(["MailerInboundRule"]),
        "activation must happen only after the receipt rule is usable; CloudFormation reverses \
         this dependency on delete so the set is deactivated before the rule is removed"
    );
    let activator_policy = &resources["MailerRuleSetActivatorRole"]["Properties"]["Policies"][0]
        ["PolicyDocument"]["Statement"][0];
    assert_eq!(
        activator_policy["Action"],
        serde_json::json!([
            "ses:DescribeActiveReceiptRuleSet",
            "ses:SetActiveReceiptRuleSet"
        ])
    );
    let activator_code = resources["MailerRuleSetActivatorFunction"]["Properties"]["Code"]
        ["ZipFile"]
        .as_str()
        .expect("inline custom-resource handler");
    assert!(activator_code.contains("ses.set_active_receipt_rule_set(RuleSetName=desired)"));
    assert!(activator_code.contains("if active == physical_id:"));
    assert!(activator_code.contains("ses.set_active_receipt_rule_set()"));

    let bucket_statements = resources["MailboxBucketPolicy"]["Properties"]["PolicyDocument"]
        ["Statement"]
        .as_array()
        .expect("bucket policy statements");
    let ses_statement = bucket_statements
        .iter()
        .find(|statement| statement["Sid"] == "AllowSesInboundDelivery")
        .expect("bucket policy should allow SES inbound delivery");
    assert_eq!(ses_statement["Principal"]["Service"], "ses.amazonaws.com");
    assert_eq!(ses_statement["Action"], "s3:PutObject");
    assert_eq!(
        ses_statement["Condition"]["StringEquals"]["aws:SourceAccount"]["Ref"],
        "AWS::AccountId"
    );

    insta::assert_snapshot!("aws_email", yaml);
}

#[test]
fn aws_email_without_links_omits_event_and_inbound_wiring() {
    let stack = Stack::new("email-minimal".to_string())
        .add(
            Email::new("mailer".to_string())
                .domains(vec!["mail.example.com".to_string()])
                .build(),
            ResourceLifecycle::Frozen,
        )
        .build();

    let yaml = render_built_ins(
        &stack,
        StackSettings::default(),
        RegistrationMode::OutputsFallback,
        "aws email minimal",
    );

    let template: serde_json::Value =
        serde_yaml::from_str(&yaml).expect("template YAML should parse");
    let resources = template["Resources"].as_object().expect("resources map");
    assert!(resources.contains_key("MailerIdentity0"));
    assert!(resources.contains_key("MailerConfigSet"));
    assert!(!resources.contains_key("MailerEventsTopic"));
    assert!(!resources.contains_key("MailerRuleSet"));

    insta::assert_snapshot!("aws_email_minimal", yaml);
}

/// Seed domains are optional: a resource with no domains and no links still
/// provisions the configuration set. Identities are then created entirely at
/// runtime through the email/manage-identities grant.
#[test]
fn aws_email_without_seed_domains_renders_config_set_only() {
    let stack = Stack::new("email-config-only".to_string())
        .add(
            Email::new("mailer".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();

    let yaml = render_built_ins(
        &stack,
        StackSettings::default(),
        RegistrationMode::OutputsFallback,
        "aws email config set only",
    );

    let template: serde_json::Value =
        serde_yaml::from_str(&yaml).expect("template YAML should parse");
    let resources = template["Resources"].as_object().expect("resources map");
    assert_eq!(
        resources["MailerConfigSet"]["Type"],
        "AWS::SES::ConfigurationSet"
    );
    assert!(!resources.contains_key("MailerIdentity0"));
    assert!(!resources.contains_key("MailerEventsTopic"));
    assert!(!resources.contains_key("MailerRuleSet"));

    insta::assert_snapshot!("aws_email_config_set_only", yaml);
}

#[test]
fn aws_email_import_ref_carries_dkim_tokens_and_rule_set() {
    let stack = email_stack();
    let (_, entry) = stack
        .resources()
        .find(|(id, _)| id.as_str() == "mailer")
        .expect("mailer resource");
    let names: IndexMap<String, String> = IndexMap::from([
        ("mailer".to_string(), "Mailer".to_string()),
        ("mailbox".to_string(), "Mailbox".to_string()),
        ("mail-events".to_string(), "MailEvents".to_string()),
    ]);
    let ctx = EmitContext {
        stack: &stack,
        resource: entry,
        resource_id: "mailer",
        platform: Platform::Aws,
        stack_settings: &StackSettings::default(),
        names: &names,
    };

    let registry = CfRegistry::built_in();
    let emitter = registry
        .require(&Email::RESOURCE_TYPE, Platform::Aws)
        .expect("email emitter should be registered");

    let import_ref = emitter
        .emit_import_ref(&ctx)
        .expect("import ref should render");
    let import_json = serde_json::to_value(&import_ref).expect("import ref should serialize");
    assert_eq!(import_json["configurationSet"]["Ref"], "MailerConfigSet");
    assert_eq!(import_json["ruleSetName"]["Ref"], "MailerRuleSet");
    let tokens = import_json["domains"]["mail.example.com"]["dkimTokens"]
        .as_array()
        .expect("dkim tokens");
    assert_eq!(tokens.len(), 3);
    assert_eq!(
        tokens[0]["name"]["Fn::GetAtt"],
        serde_json::json!(["MailerIdentity0", "DkimDNSTokenName1"])
    );
    assert_eq!(
        tokens[2]["value"]["Fn::GetAtt"],
        serde_json::json!(["MailerIdentity0", "DkimDNSTokenValue3"])
    );

    let binding_ref = emitter
        .emit_binding_ref(&ctx)
        .expect("binding ref should render")
        .expect("email emitter should provide a binding");
    let binding_json = serde_json::to_value(&binding_ref).expect("binding ref should serialize");
    assert_eq!(binding_json["service"], "ses");
    assert_eq!(binding_json["region"]["Ref"], "AWS::Region");
    assert_eq!(binding_json["configurationSet"]["Ref"], "MailerConfigSet");
    assert_eq!(
        binding_json["eventTopicArn"]["Ref"], "MailerEventsTopic",
        "binding should expose the events topic ARN when events are configured"
    );
    // Deliberately no domain list: identities are created and removed at
    // runtime, so applications discover them via ses:ListEmailIdentities
    // instead of a deploy-frozen list.
    assert!(binding_json
        .as_object()
        .expect("binding object")
        .get("domains")
        .is_none());
}

#[test]
fn aws_email_rejects_live_linked_queue() {
    let mail_events = Queue::new("mail-events".to_string()).build();
    let email = Email::new("mailer".to_string())
        .domains(vec!["mail.example.com".to_string()])
        .events(EmailEvents {
            queue: ResourceRef::from(&mail_events),
        })
        .build();
    let stack = Stack::new("email-live-queue".to_string())
        .add(mail_events, ResourceLifecycle::Live)
        .add(email, ResourceLifecycle::Frozen)
        .build();
    let (_, entry) = stack
        .resources()
        .find(|(id, _)| id.as_str() == "mailer")
        .expect("mailer resource");
    let names: IndexMap<String, String> =
        IndexMap::from([("mailer".to_string(), "Mailer".to_string())]);
    let ctx = EmitContext {
        stack: &stack,
        resource: entry,
        resource_id: "mailer",
        platform: Platform::Aws,
        stack_settings: &StackSettings::default(),
        names: &names,
    };

    let registry = CfRegistry::built_in();
    let emitter = registry
        .require(&Email::RESOURCE_TYPE, Platform::Aws)
        .expect("email emitter should be registered");

    let error = emitter
        .emit_resources_with_registry(&ctx, &registry)
        .expect_err("linking a live queue must be rejected");
    assert!(error.to_string().contains("not emitted in setup"));
}

#[test]
fn aws_email_rejects_multiple_inbound_rule_sets() {
    let first_mailbox = Storage::new("first-mailbox".to_string()).build();
    let second_mailbox = Storage::new("second-mailbox".to_string()).build();
    let first_email = Email::new("first-mailer".to_string())
        .inbound(EmailInbound {
            storage: ResourceRef::from(&first_mailbox),
        })
        .build();
    let second_email = Email::new("second-mailer".to_string())
        .inbound(EmailInbound {
            storage: ResourceRef::from(&second_mailbox),
        })
        .build();
    let stack = Stack::new("multiple-inbound-email".to_string())
        .add(first_mailbox, ResourceLifecycle::Frozen)
        .add(second_mailbox, ResourceLifecycle::Frozen)
        .add(first_email, ResourceLifecycle::Frozen)
        .add(second_email, ResourceLifecycle::Frozen)
        .build();
    let (_, entry) = stack
        .resources()
        .find(|(id, _)| id.as_str() == "first-mailer")
        .expect("first email resource");
    let names: IndexMap<String, String> =
        IndexMap::from([("first-mailer".to_string(), "FirstMailer".to_string())]);
    let ctx = EmitContext {
        stack: &stack,
        resource: entry,
        resource_id: "first-mailer",
        platform: Platform::Aws,
        stack_settings: &StackSettings::default(),
        names: &names,
    };
    let registry = CfRegistry::built_in();
    let emitter = registry
        .require(&Email::RESOURCE_TYPE, Platform::Aws)
        .expect("email emitter should be registered");

    let error = emitter
        .emit_resources_with_registry(&ctx, &registry)
        .expect_err("multiple account-wide inbound rule sets must be rejected");
    assert!(
        error
            .to_string()
            .contains("only one email resource with inbound delivery"),
        "{error}"
    );
    assert!(error.to_string().contains("first-mailer"));
    assert!(error.to_string().contains("second-mailer"));
}

/// `emit_import_ref` and `AwsEmailImportData` are two halves of one contract:
/// the manager-side importer deserializes exactly what the deployed setup
/// stack resolves this expression to. Resolving every intrinsic to a
/// placeholder string and parsing the result catches key or shape drift
/// between the emitter and the import struct at test time.
#[test]
fn aws_email_import_ref_matches_import_data_contract() {
    let stack = email_stack();
    let (_, entry) = stack
        .resources()
        .find(|(id, _)| id.as_str() == "mailer")
        .expect("mailer resource");
    let names: IndexMap<String, String> = IndexMap::from([
        ("mailer".to_string(), "Mailer".to_string()),
        ("mailbox".to_string(), "Mailbox".to_string()),
        ("mail-events".to_string(), "MailEvents".to_string()),
    ]);
    let ctx = EmitContext {
        stack: &stack,
        resource: entry,
        resource_id: "mailer",
        platform: Platform::Aws,
        stack_settings: &StackSettings::default(),
        names: &names,
    };

    let registry = CfRegistry::built_in();
    let emitter = registry
        .require(&Email::RESOURCE_TYPE, Platform::Aws)
        .expect("email emitter should be registered");
    let import_ref = emitter
        .emit_import_ref(&ctx)
        .expect("import ref should render");
    let import_json = serde_json::to_value(&import_ref).expect("import ref should serialize");

    let resolved = resolve_cfn_intrinsics(import_json);
    let data: AwsEmailImportData = serde_json::from_value(resolved)
        .expect("resolved import ref must deserialize into AwsEmailImportData");
    assert_eq!(data.domains.len(), 2, "one entry per seed domain");
    for domain in ["mail.example.com", "mail.example.org"] {
        let entry = data
            .domains
            .get(domain)
            .unwrap_or_else(|| panic!("domain '{domain}' must be present"));
        assert_eq!(entry.dkim_tokens.len(), 3, "Easy DKIM emits three tokens");
    }
    assert!(
        data.rule_set_name.is_some(),
        "inbound is configured, so the rule set name must be exported"
    );
}

/// Replace every CloudFormation intrinsic (`Ref` / `Fn::*`) with a
/// placeholder string — the shape the payload has after the deployed stack
/// resolves it.
fn resolve_cfn_intrinsics(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let is_intrinsic = map.len() == 1
                && map
                    .keys()
                    .next()
                    .is_some_and(|key| key == "Ref" || key.starts_with("Fn::"));
            if is_intrinsic {
                serde_json::Value::String("resolved".to_string())
            } else {
                serde_json::Value::Object(
                    map.into_iter()
                        .map(|(key, value)| (key, resolve_cfn_intrinsics(value)))
                        .collect(),
                )
            }
        }
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.into_iter().map(resolve_cfn_intrinsics).collect())
        }
        other => other,
    }
}

#[test]
fn aws_email_resource_permissions_attach_to_service_account_role() {
    use alien_core::{PermissionProfile, ServiceAccount};

    let email = Email::new("mailer".to_string()).build();
    let stack = Stack::new("email-permissions".to_string())
        .permission(
            "sender",
            PermissionProfile::new().resource("mailer", ["email/send", "email/manage-identities"]),
        )
        .add(
            ServiceAccount::new("sender-sa".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(email, ResourceLifecycle::Frozen)
        .build();

    let yaml = render_built_ins(
        &stack,
        StackSettings::default(),
        RegistrationMode::OutputsFallback,
        "aws email service account permissions",
    );
    let template: serde_json::Value =
        serde_yaml::from_str(&yaml).expect("template YAML should parse");

    let send_policy = &template["Resources"]["MailerSenderSaRoleEmailPermission00"];
    let identities_policy = &template["Resources"]["MailerSenderSaRoleEmailPermission01"];
    assert_eq!(send_policy["Type"], "AWS::IAM::Policy");
    assert_eq!(identities_policy["Type"], "AWS::IAM::Policy");

    let send_actions = send_policy["Properties"]["PolicyDocument"]["Statement"][0]["Action"]
        .as_array()
        .expect("send statement should list actions");
    assert!(send_actions.contains(&serde_json::json!("ses:SendEmail")));
    assert!(send_actions.contains(&serde_json::json!("ses:SendRawEmail")));

    let identity_statements = identities_policy["Properties"]["PolicyDocument"]["Statement"]
        .as_array()
        .expect("manage-identities statements");
    let identity_actions: Vec<String> = identity_statements
        .iter()
        .flat_map(|statement| statement["Action"].as_array().cloned().unwrap_or_default())
        .filter_map(|action| action.as_str().map(str::to_string))
        .collect();
    assert!(identity_actions.contains(&"ses:CreateEmailIdentity".to_string()));
    let identity_resources = identity_statements[0]["Resource"]
        .as_array()
        .expect("manage-identities statement should list resources");
    assert!(identity_resources.contains(&serde_json::json!({
        "Fn::Sub":
            "arn:${AWS::Partition}:ses:${AWS::Region}:${AWS::AccountId}:configuration-set/${AWS::StackName}-mailer"
    })));

    for policy in [send_policy, identities_policy] {
        assert_eq!(
            policy["Properties"]["Roles"][0]["Ref"],
            serde_json::json!("SenderSaRole")
        );
    }
}
