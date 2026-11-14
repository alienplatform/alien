//! Every S3 bucket Alien creates carries the same incomplete-upload rule under CloudFormation
//! and Terraform, built from the definition the storage controller also uses
//! ([`AWS_ABORT_INCOMPLETE_UPLOAD_RULE_ID`], [`AWS_ABORT_INCOMPLETE_UPLOAD_DAYS`]), next to the
//! app's own expiry rules.

use super::helpers::render;
use alien_cloudformation::{
    generate_cloudformation_template, CfRegistry, CloudFormationOptions, CloudFormationTarget,
    RegistrationMode,
};
use alien_core::{
    LifecycleRule, ResourceLifecycle, Stack, StackSettings, Storage,
    AWS_ABORT_INCOMPLETE_UPLOAD_DAYS, AWS_ABORT_INCOMPLETE_UPLOAD_RULE_ID,
    AWS_LIFECYCLE_RULE_LIMIT,
};
use alien_terraform::TerraformTarget;
use serde_json::Value;

/// One rendered lifecycle rule: its id, status, prefix, expiry days and incomplete-upload days.
type Rule = (String, String, String, Option<u64>, Option<u64>);

fn stack(lifecycle_rules: Vec<LifecycleRule>) -> Stack {
    Stack::new("buckets".to_string())
        .add(
            Storage::new("data".to_string())
                .lifecycle_rules(lifecycle_rules)
                .build(),
            ResourceLifecycle::Frozen,
        )
        .build()
}

fn cloudformation_rules(stack: &Stack) -> Vec<Rule> {
    let template = generate_cloudformation_template(
        stack,
        CloudFormationOptions {
            pins: alien_core::SetupPins::compiled_in(),
            registry: &CfRegistry::built_in(),
            source_stack: stack,
            target: CloudFormationTarget::Aws,
            stack_settings: StackSettings::default(),
            setup_target: "aws".to_string(),
            setup_fingerprint: "test".to_string(),
            setup_fingerprint_version: 1,
            registration: RegistrationMode::OutputsFallback,
            description: None,
            management_trust: Default::default(),
        },
    )
    .expect("template renders");
    let template = serde_json::to_value(&template).expect("template serializes");
    let bucket = template["Resources"]
        .as_object()
        .expect("resources")
        .values()
        .find(|resource| resource["Type"] == "AWS::S3::Bucket")
        .expect("one bucket");
    bucket["Properties"]["LifecycleConfiguration"]["Rules"]
        .as_array()
        .expect("the bucket has lifecycle rules")
        .iter()
        .map(|rule| {
            (
                rule["Id"].as_str().unwrap().to_string(),
                rule["Status"].as_str().unwrap().to_string(),
                rule["Prefix"].as_str().unwrap_or("").to_string(),
                rule["ExpirationInDays"].as_u64(),
                rule["AbortIncompleteMultipartUpload"]["DaysAfterInitiation"].as_u64(),
            )
        })
        .collect()
}

fn terraform_rules(stack: &Stack) -> Vec<Rule> {
    let module = render(stack, TerraformTarget::Aws, StackSettings::default());
    let blocks: Vec<hcl::Block> = module
        .iter()
        .filter(|(path, _)| path.ends_with(".tf"))
        .flat_map(|(_, contents)| hcl::parse(contents).unwrap().into_blocks())
        .filter(|block| {
            block.identifier() == "resource"
                && block.labels()[0].as_str() == "aws_s3_bucket_lifecycle_configuration"
        })
        .collect();
    assert_eq!(blocks.len(), 1, "one lifecycle configuration");
    let number = |block: &hcl::Block, nested: &str, key: &str| -> Option<u64> {
        block
            .body()
            .blocks()
            .find(|inner| inner.identifier() == nested)
            .and_then(|inner| inner.body().attributes().find(|a| a.key() == key))
            .map(|attribute| attribute.expr().to_string().parse().expect("a number"))
    };
    let text = |block: &hcl::Block, key: &str| -> String {
        match block
            .body()
            .attributes()
            .find(|a| a.key() == key)
            .map(|a| a.expr())
        {
            Some(hcl::Expression::String(value)) => value.clone(),
            other => panic!("{key} is a string: {other:?}"),
        }
    };
    blocks[0]
        .body()
        .blocks()
        .filter(|rule| rule.identifier() == "rule")
        .map(|rule| {
            let filter = rule
                .body()
                .blocks()
                .find(|inner| inner.identifier() == "filter")
                .expect("every rule has a filter");
            (
                text(rule, "id"),
                text(rule, "status"),
                text(filter, "prefix"),
                number(rule, "expiration", "days"),
                number(
                    rule,
                    "abort_incomplete_multipart_upload",
                    "days_after_initiation",
                ),
            )
        })
        .collect()
}

fn incomplete_upload_rule() -> Rule {
    (
        AWS_ABORT_INCOMPLETE_UPLOAD_RULE_ID.to_string(),
        "Enabled".to_string(),
        String::new(),
        None,
        Some(AWS_ABORT_INCOMPLETE_UPLOAD_DAYS as u64),
    )
}

#[test]
fn a_bucket_without_app_rules_gets_only_the_incomplete_upload_rule() {
    let stack = stack(vec![]);
    assert_eq!(cloudformation_rules(&stack), vec![incomplete_upload_rule()]);
    assert_eq!(terraform_rules(&stack), vec![incomplete_upload_rule()]);
}

#[test]
fn a_bucket_with_app_rules_keeps_them_next_to_the_incomplete_upload_rule() {
    let stack = stack(vec![LifecycleRule {
        days: 30,
        prefix: Some("tmp/".to_string()),
    }]);
    let expiry = |id: &str| -> Rule {
        (
            id.to_string(),
            "Enabled".to_string(),
            "tmp/".to_string(),
            Some(30),
            None,
        )
    };
    assert_eq!(
        cloudformation_rules(&stack),
        vec![expiry("Rule1"), incomplete_upload_rule()]
    );
    assert_eq!(
        terraform_rules(&stack),
        vec![expiry("rule-1"), incomplete_upload_rule()]
    );
}

/// S3 accepts at most 1,000 rules on a bucket, so one whose app rules use them all gets no
/// incomplete-upload rule: it already has a lifecycle configuration.
#[test]
fn a_bucket_whose_app_rules_fill_s3s_limit_gets_no_extra_rule() {
    let rules = |count: usize| {
        (0..count)
            .map(|index| LifecycleRule {
                days: 30,
                prefix: Some(format!("p{index}/")),
            })
            .collect::<Vec<_>>()
    };
    let full = stack(rules(AWS_LIFECYCLE_RULE_LIMIT));
    let cloudformation = cloudformation_rules(&full);
    let terraform = terraform_rules(&full);
    assert_eq!(cloudformation.len(), AWS_LIFECYCLE_RULE_LIMIT);
    assert_eq!(terraform.len(), AWS_LIFECYCLE_RULE_LIMIT);
    assert!(!cloudformation.contains(&incomplete_upload_rule()));
    assert!(!terraform.contains(&incomplete_upload_rule()));

    let room = stack(rules(AWS_LIFECYCLE_RULE_LIMIT - 1));
    assert_eq!(
        cloudformation_rules(&room).last(),
        Some(&incomplete_upload_rule())
    );
    assert_eq!(
        terraform_rules(&room).last(),
        Some(&incomplete_upload_rule())
    );
}
