//! Validates Storage lifecycle rules against the limits of the provider API they are sent to.
//!
//! Without this check an invalid rule passes build and release and is first rejected by the
//! cloud while the deployment configures the bucket, after the step has used its retry budget.
//!
//! - **AWS S3** (PutBucketLifecycleConfiguration and CloudFormation `AWS::S3::Bucket`):
//!   - `Days` must be a non-zero positive integer of the API's `Integer` type:
//!     <https://docs.aws.amazon.com/AmazonS3/latest/API/API_LifecycleExpiration.html>.
//!     S3 rejects 0 with `InvalidArgument` and accepts 2147483647.
//!   - At most 1,000 rules per bucket:
//!     <https://docs.aws.amazon.com/AmazonS3/latest/userguide/intro-lifecycle-rules.html>.
//!     S3 answers 1,001 rules with `MalformedXML`.
//!   - A prefix is at most 1,024 bytes (S3 answers `InvalidRequest: The maximum size of a prefix
//!     is 1024`; the limit counts UTF-8 bytes, not characters).
//!   - A prefix travels in an XML body, so it can only hold characters XML 1.0 allows:
//!     <https://docs.aws.amazon.com/AmazonS3/latest/API/API_LifecycleRuleFilter.html>,
//!     <https://www.w3.org/TR/xml/#charsets>. S3 answers U+0001 and U+FFFE with `MalformedXML`.
//!   - Overlapping prefixes are allowed; S3 applies the shorter expiration:
//!     <https://docs.aws.amazon.com/AmazonS3/latest/userguide/lifecycle-conflicts.html>.
//! - **GCP Cloud Storage**: `age` is an `int32`, and 0 is valid (it expires objects at the
//!   first midnight UTC after creation): <https://cloud.google.com/storage/docs/lifecycle>.
//!   A bucket's rules hold at most 1,000 prefixes and suffixes in total:
//!   <https://cloud.google.com/storage/quotas>.
//! - **Azure Blob Storage** (the Terraform management policy, so Frozen storage only: the Azure
//!   controller ignores lifecycle rules on Live storage): 0 is valid, the provider accepts
//!   0-99999 days for `delete_after_days_since_modification_greater_than`, and a policy holds
//!   at most 100 rules:
//!   <https://learn.microsoft.com/en-us/azure/storage/blobs/lifecycle-management-policy-structure>.
//!
//! The S3 values were checked against PutBucketLifecycleConfiguration directly. Each test case
//! below records the response S3 gave.

use crate::error::Result;
use crate::{CheckResult, CompileTimeCheck};
use alien_core::{Platform, ResourceLifecycle, Stack, Storage};

/// S3 lifecycle `Days` and Cloud Storage `age` are 32-bit signed integers.
const INT32_MAX_DAYS: u32 = i32::MAX as u32;
const S3_MAX_RULES: usize = 1000;
const S3_MAX_PREFIX_BYTES: usize = 1024;
const GCS_MAX_PREFIXES: usize = 1000;
const AZURE_MAX_DAYS: u32 = 99_999;
const AZURE_MAX_RULES: usize = 100;

pub struct StorageLifecycleRulesCheck;

#[async_trait::async_trait]
impl CompileTimeCheck for StorageLifecycleRulesCheck {
    fn code(&self) -> Option<&'static str> {
        Some("STORAGE_LIFECYCLE_RULES_INVALID")
    }

    fn description(&self) -> &'static str {
        "Storage lifecycle rules are valid for the target platform"
    }

    fn should_run(&self, stack: &Stack, platform: Platform) -> bool {
        matches!(platform, Platform::Aws | Platform::Gcp | Platform::Azure)
            && stack.resources.values().any(|entry| {
                entry
                    .config
                    .downcast_ref::<Storage>()
                    .is_some_and(|storage| !storage.lifecycle_rules.is_empty())
            })
    }

    async fn check(&self, stack: &Stack, platform: Platform) -> Result<CheckResult> {
        let mut result = CheckResult::success();

        for (id, entry) in &stack.resources {
            let Some(storage) = entry.config.downcast_ref::<Storage>() else {
                continue;
            };
            let errors = match platform {
                Platform::Aws => s3_errors(storage),
                Platform::Gcp => gcs_errors(storage),
                // Only setup's Terraform sends Azure lifecycle rules, and setup creates Frozen
                // resources only.
                Platform::Azure if entry.lifecycle == ResourceLifecycle::Frozen => {
                    azure_errors(storage)
                }
                _ => Vec::new(),
            };
            for error in errors {
                result.add_error(format!("Storage '{id}': {error}"));
            }
        }

        Ok(result)
    }
}

fn s3_errors(storage: &Storage) -> Vec<String> {
    let rules = &storage.lifecycle_rules;
    let mut errors = Vec::new();

    if rules.len() > S3_MAX_RULES {
        errors.push(format!(
            "lifecycleRules has {} rules; AWS S3 allows at most {S3_MAX_RULES} per bucket",
            rules.len()
        ));
    }

    for (index, rule) in rules.iter().enumerate() {
        if rule.days == 0 {
            errors.push(format!(
                "lifecycleRules[{index}].days is 0; AWS S3 requires an expiration of at least 1 day"
            ));
        } else if rule.days > INT32_MAX_DAYS {
            errors.push(format!(
                "lifecycleRules[{index}].days is {}; AWS S3 accepts at most {INT32_MAX_DAYS}",
                rule.days
            ));
        }

        let Some(prefix) = &rule.prefix else {
            continue;
        };
        if prefix.len() > S3_MAX_PREFIX_BYTES {
            errors.push(format!(
                "lifecycleRules[{index}].prefix is {} bytes; AWS S3 allows at most {S3_MAX_PREFIX_BYTES} bytes",
                prefix.len()
            ));
        }
        if let Some(character) = prefix.chars().find(|c| !is_xml_char(*c)) {
            errors.push(format!(
                "lifecycleRules[{index}].prefix contains U+{:04X}, which AWS S3 lifecycle configuration (XML) cannot carry",
                u32::from(character)
            ));
        }
    }

    errors
}

fn gcs_errors(storage: &Storage) -> Vec<String> {
    let rules = &storage.lifecycle_rules;
    let mut errors = Vec::new();

    let prefixes = rules.iter().filter(|rule| rule.prefix.is_some()).count();
    if prefixes > GCS_MAX_PREFIXES {
        errors.push(format!(
            "lifecycleRules has {prefixes} prefixes; GCP Cloud Storage allows at most {GCS_MAX_PREFIXES} per bucket"
        ));
    }

    for (index, rule) in rules.iter().enumerate() {
        if rule.days > INT32_MAX_DAYS {
            errors.push(format!(
                "lifecycleRules[{index}].days is {}; GCP Cloud Storage accepts at most {INT32_MAX_DAYS}",
                rule.days
            ));
        }
    }

    errors
}

fn azure_errors(storage: &Storage) -> Vec<String> {
    let rules = &storage.lifecycle_rules;
    let mut errors = Vec::new();

    if rules.len() > AZURE_MAX_RULES {
        errors.push(format!(
            "lifecycleRules has {} rules; an Azure Storage lifecycle policy allows at most {AZURE_MAX_RULES}",
            rules.len()
        ));
    }

    for (index, rule) in rules.iter().enumerate() {
        if rule.days > AZURE_MAX_DAYS {
            errors.push(format!(
                "lifecycleRules[{index}].days is {}; Azure Storage accepts at most {AZURE_MAX_DAYS}",
                rule.days
            ));
        }
    }

    errors
}

/// The `Char` production of XML 1.0. Rust strings cannot hold surrogates, so only control
/// characters and U+FFFE/U+FFFF fall outside it.
fn is_xml_char(c: char) -> bool {
    matches!(c, '\u{9}' | '\u{A}' | '\u{D}' | '\u{20}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..='\u{10FFFF}')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{error::ErrorData, runner::PreflightRunner};
    use alien_core::LifecycleRule;

    fn rule(days: u32, prefix: Option<&str>) -> LifecycleRule {
        LifecycleRule {
            days,
            prefix: prefix.map(str::to_string),
        }
    }

    fn stack(rules: Vec<LifecycleRule>) -> Stack {
        stack_with(rules, ResourceLifecycle::Live)
    }

    fn stack_with(rules: Vec<LifecycleRule>, lifecycle: ResourceLifecycle) -> Stack {
        Stack::new("lifecycle".to_string())
            .add(
                Storage::new("st".to_string())
                    .lifecycle_rules(rules)
                    .build(),
                lifecycle,
            )
            .build()
    }

    async fn errors(rules: Vec<LifecycleRule>, platform: Platform) -> Vec<String> {
        errors_with(rules, ResourceLifecycle::Live, platform).await
    }

    async fn errors_with(
        rules: Vec<LifecycleRule>,
        lifecycle: ResourceLifecycle,
        platform: Platform,
    ) -> Vec<String> {
        let stack = stack_with(rules, lifecycle);
        assert!(StorageLifecycleRulesCheck.should_run(&stack, platform));
        StorageLifecycleRulesCheck
            .check(&stack, platform)
            .await
            .expect("the check runs")
            .errors
    }

    fn prefixed_rules(count: usize) -> Vec<LifecycleRule> {
        (0..count)
            .map(|index| rule(3, Some(&format!("p{index}/"))))
            .collect()
    }

    /// The release from the report: one 0-day rule. Every entry point that validates a stack
    /// must refuse it before any cloud call: `alien build` and `alien release` (build-time),
    /// setup template rendering, and deployment preflights (compile-time checks).
    #[tokio::test]
    async fn a_zero_day_rule_fails_build_setup_and_deployment_preflights_on_aws() {
        let stack = stack(vec![rule(0, None)]);
        let runner = PreflightRunner::new();
        let expected = "Storage 'st': lifecycleRules[0].days is 0; AWS S3 requires an expiration of at least 1 day";

        for (entry_point, outcome) in [
            (
                "build",
                runner
                    .run_build_time_preflights(&stack, Platform::Aws)
                    .await
                    .map(|_| ()),
            ),
            (
                "template",
                runner
                    .run_template_preflights(&stack, Platform::Aws)
                    .await
                    .map(|_| ()),
            ),
        ] {
            let error = outcome.expect_err(entry_point);
            let Some(ErrorData::ValidationFailed { results, .. }) = &error.error else {
                panic!("{entry_point}: expected ValidationFailed, got {error:?}");
            };
            let failed: Vec<_> = results.iter().filter(|result| !result.success).collect();
            assert_eq!(failed.len(), 1, "{entry_point}: {failed:?}");
            assert_eq!(
                failed[0].code.as_deref(),
                Some("STORAGE_LIFECYCLE_RULES_INVALID")
            );
            assert_eq!(
                failed[0].errors,
                vec![expected.to_string()],
                "{entry_point}"
            );
        }

        // Deployment-time preflights start with exactly these compile-time checks.
        let summary = runner
            .run_compile_time_checks(&stack, Platform::Aws)
            .await
            .expect("compile-time checks run");
        assert!(!summary.success);
        assert_eq!(
            summary
                .results
                .iter()
                .flat_map(|result| result.errors.iter())
                .collect::<Vec<_>>(),
            vec![expected]
        );
    }

    /// Each case is a value sent to PutBucketLifecycleConfiguration, with S3's answer.
    #[tokio::test]
    async fn s3_limits_match_what_s3_accepts_and_rejects() {
        let accepted: Vec<(&str, Vec<LifecycleRule>)> = vec![
            ("1 day: accepted", vec![rule(1, None)]),
            (
                "2147483647 days: accepted",
                vec![rule(INT32_MAX_DAYS, None)],
            ),
            ("empty prefix: accepted", vec![rule(3, Some(""))]),
            (
                "1024-byte prefix: accepted",
                vec![rule(3, Some(&"a".repeat(1024)))],
            ),
            (
                "341 three-byte characters plus 'a' (1024 bytes): accepted",
                vec![rule(3, Some(&format!("{}a", "€".repeat(341))))],
            ),
            (
                "tab, DEL and emoji in a prefix: accepted",
                vec![rule(3, Some("a\tb\u{7f}c/\u{1F600}/"))],
            ),
            (
                "duplicate and overlapping prefixes, two unfiltered rules: accepted",
                vec![
                    rule(3, Some("logs/")),
                    rule(7, Some("logs/")),
                    rule(7, Some("logs/a")),
                    rule(3, None),
                    rule(7, None),
                ],
            ),
            ("1000 rules: accepted", prefixed_rules(1000)),
        ];
        for (case, rules) in accepted {
            assert_eq!(
                errors(rules, Platform::Aws).await,
                Vec::<String>::new(),
                "{case}"
            );
        }

        let rejected: Vec<(&str, Vec<LifecycleRule>, &str)> = vec![
            (
                "0 days: InvalidArgument 'Days' for Expiration action must be a positive integer",
                vec![rule(0, Some("logs/"))],
                "Storage 'st': lifecycleRules[0].days is 0; AWS S3 requires an expiration of at least 1 day",
            ),
            (
                // The controller casts days to i32, so this would reach S3 as -2147483648, which
                // S3 rejects like -1: InvalidArgument.
                "2147483648 days",
                vec![rule(1, None), rule(INT32_MAX_DAYS + 1, None)],
                "Storage 'st': lifecycleRules[1].days is 2147483648; AWS S3 accepts at most 2147483647",
            ),
            (
                "1025-byte prefix: InvalidRequest The maximum size of a prefix is 1024",
                vec![rule(3, Some(&"a".repeat(1025)))],
                "Storage 'st': lifecycleRules[0].prefix is 1025 bytes; AWS S3 allows at most 1024 bytes",
            ),
            (
                "342 three-byte characters (1026 bytes): InvalidRequest",
                vec![rule(3, Some(&"€".repeat(342)))],
                "Storage 'st': lifecycleRules[0].prefix is 1026 bytes; AWS S3 allows at most 1024 bytes",
            ),
            (
                "U+0001 in a prefix: MalformedXML",
                vec![rule(3, Some("a\u{1}b"))],
                "Storage 'st': lifecycleRules[0].prefix contains U+0001, which AWS S3 lifecycle configuration (XML) cannot carry",
            ),
            (
                "U+FFFE in a prefix: MalformedXML",
                vec![rule(3, Some("a\u{FFFE}b"))],
                "Storage 'st': lifecycleRules[0].prefix contains U+FFFE, which AWS S3 lifecycle configuration (XML) cannot carry",
            ),
            (
                "1001 rules: MalformedXML",
                prefixed_rules(1001),
                "Storage 'st': lifecycleRules has 1001 rules; AWS S3 allows at most 1000 per bucket",
            ),
        ];
        for (case, rules, expected) in rejected {
            assert_eq!(
                errors(rules, Platform::Aws).await,
                vec![expected.to_string()],
                "{case}"
            );
        }
    }

    #[tokio::test]
    async fn every_invalid_field_is_reported_with_its_rule_index() {
        assert_eq!(
            errors(
                vec![rule(1, Some("ok/")), rule(0, Some(&"a".repeat(2000)))],
                Platform::Aws
            )
            .await,
            vec![
                "Storage 'st': lifecycleRules[1].days is 0; AWS S3 requires an expiration of at least 1 day",
                "Storage 'st': lifecycleRules[1].prefix is 2000 bytes; AWS S3 allows at most 1024 bytes",
            ]
        );
    }

    #[tokio::test]
    async fn gcp_accepts_a_zero_day_age_and_checks_its_own_limits() {
        assert_eq!(
            errors(
                vec![rule(0, Some("tmp/")), rule(INT32_MAX_DAYS, None)],
                Platform::Gcp
            )
            .await,
            Vec::<String>::new()
        );
        // Rules without a prefix don't count toward the prefix quota.
        let mut unprefixed = prefixed_rules(GCS_MAX_PREFIXES);
        unprefixed.push(rule(3, None));
        assert_eq!(
            errors(unprefixed, Platform::Gcp).await,
            Vec::<String>::new()
        );

        assert_eq!(
            errors(
                vec![rule(INT32_MAX_DAYS + 1, None)],
                Platform::Gcp
            )
            .await,
            vec!["Storage 'st': lifecycleRules[0].days is 2147483648; GCP Cloud Storage accepts at most 2147483647"]
        );
        assert_eq!(
            errors(prefixed_rules(GCS_MAX_PREFIXES + 1), Platform::Gcp).await,
            vec!["Storage 'st': lifecycleRules has 1001 prefixes; GCP Cloud Storage allows at most 1000 per bucket"]
        );
    }

    /// Azure rules reach the provider only through setup's Terraform management policy, which
    /// covers Frozen storage. The Azure controller ignores rules on Live storage.
    #[tokio::test]
    async fn azure_checks_frozen_storage_rules_and_accepts_a_zero_day_rule() {
        let frozen = ResourceLifecycle::Frozen;
        assert_eq!(
            errors_with(
                vec![rule(0, None), rule(AZURE_MAX_DAYS, Some("a"))],
                frozen,
                Platform::Azure
            )
            .await,
            Vec::<String>::new()
        );
        assert_eq!(
            errors_with(prefixed_rules(AZURE_MAX_RULES), frozen, Platform::Azure).await,
            Vec::<String>::new()
        );

        assert_eq!(
            errors_with(vec![rule(AZURE_MAX_DAYS + 1, None)], frozen, Platform::Azure).await,
            vec!["Storage 'st': lifecycleRules[0].days is 100000; Azure Storage accepts at most 99999"]
        );
        assert_eq!(
            errors_with(prefixed_rules(AZURE_MAX_RULES + 1), frozen, Platform::Azure).await,
            vec!["Storage 'st': lifecycleRules has 101 rules; an Azure Storage lifecycle policy allows at most 100"]
        );

        let mut unused = prefixed_rules(AZURE_MAX_RULES + 1);
        unused.push(rule(AZURE_MAX_DAYS + 1, None));
        assert_eq!(errors(unused, Platform::Azure).await, Vec::<String>::new());
    }

    /// AWS and GCP controllers send the rules of Live storage too.
    #[tokio::test]
    async fn aws_and_gcp_check_frozen_storage_like_live_storage() {
        assert_eq!(
            errors_with(vec![rule(0, None)], ResourceLifecycle::Frozen, Platform::Aws).await,
            vec!["Storage 'st': lifecycleRules[0].days is 0; AWS S3 requires an expiration of at least 1 day"]
        );
        assert_eq!(
            errors_with(
                vec![rule(INT32_MAX_DAYS + 1, None)],
                ResourceLifecycle::Frozen,
                Platform::Gcp
            )
            .await,
            vec!["Storage 'st': lifecycleRules[0].days is 2147483648; GCP Cloud Storage accepts at most 2147483647"]
        );
    }

    /// Kubernetes uses externally bound storage and the local platform has no lifecycle support,
    /// so neither sends rules anywhere.
    #[test]
    fn platforms_without_lifecycle_support_are_not_checked() {
        let stack = stack(vec![rule(0, None)]);
        for platform in [
            Platform::Kubernetes,
            Platform::Local,
            Platform::Machines,
            Platform::Test,
        ] {
            assert!(
                !StorageLifecycleRulesCheck.should_run(&stack, platform),
                "{platform:?}"
            );
        }
    }
}
