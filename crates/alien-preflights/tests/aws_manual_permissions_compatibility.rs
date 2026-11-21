//! Compare manual management grants with an independently compiled pre-boundary release.
#[path = "../../alien-permissions/tests/iam_policy/mod.rs"]
mod iam_policy;
#[path = "support/manual_management.rs"]
mod manual_management;

use alien_permissions::generators::{aws_permissions_boundary_covers, AwsIamStatement};
use iam_policy::{evaluate, wildcard_match, Decision, Request};
use serde_json::json;
use std::collections::BTreeMap;

/// The one grant `vault/data-write` added after the legacy release: a value the vault binding
/// creates is tagged with its vault markers, so writing a value includes tagging it. It applies
/// to the same parameters as `ssm:PutParameter`, which the test checks before comparing the rest.
const VAULT_MARKER_TAGGING: &str = "ssm:AddTagsToResource";

#[tokio::test]
async fn legacy_manual_profiles_preserve_iam_and_resource_scoped_grants() {
    let baseline: BTreeMap<String, BTreeMap<String, Vec<AwsIamStatement>>> = serde_json::from_str(
        include_str!("fixtures/legacy_manual_management_policies.json"),
    )
    .unwrap();
    for (name, override_profile) in [("extend", false), ("override", true)] {
        let current = manual_management::compile_manual_management(override_profile).await;
        let previous = &baseline[name];
        assert_eq!(
            current.keys().collect::<Vec<_>>(),
            previous.keys().collect::<Vec<_>>(),
            "{name}: scopes changed"
        );
        assert!(current.contains_key("uploads") && current.contains_key("secrets"));
        for (scope, statements) in &current {
            if scope == "secrets" {
                let tagging: Vec<_> = statements
                    .iter()
                    .filter(|statement| statement.action.iter().any(|a| a == VAULT_MARKER_TAGGING))
                    .collect();
                assert!(
                    !tagging.is_empty(),
                    "{name}/secrets: vault/data-write no longer allows marking new values"
                );
                for statement in tagging {
                    assert!(
                        statement.effect == "Allow"
                            && statement.action.iter().any(|a| a == "ssm:PutParameter"),
                        "{name}/secrets: marker tagging must share the PutParameter statement and its resources"
                    );
                }
            }
            let policy = |statements: &[AwsIamStatement]| {
                let statements: Vec<_> = statements
                    .iter()
                    .filter_map(|statement| {
                        let mut statement = statement.clone();
                        // Global tagging restrictions deliberately changed; compare IAM there.
                        // Resource-scoped policies must preserve every service, including data grants.
                        if scope == "*" {
                            statement.action.retain(|action| {
                                action == "*"
                                    || action.split_once(':').is_some_and(|(service, _)| {
                                        wildcard_match(&service.to_ascii_lowercase(), "iam")
                                    })
                            });
                        }
                        // Checked above: the marker tagging grant is intended on the vault scope.
                        if scope == "secrets" {
                            statement
                                .action
                                .retain(|action| action != VAULT_MARKER_TAGGING);
                        }
                        (!statement.action.is_empty()).then_some(statement)
                    })
                    .collect();
                json!({"Version": "2012-10-17", "Statement": statements})
            };
            let before = policy(&previous[scope]);
            let after = policy(statements);
            assert!(
                aws_permissions_boundary_covers(&before, &after).unwrap(),
                "{name}/{scope}: added grants or removed denies"
            );
            assert!(
                aws_permissions_boundary_covers(&after, &before).unwrap(),
                "{name}/{scope}: removed grants or added denies"
            );
        }
        let combined: Vec<_> = current.into_values().flatten().collect();
        for (action, resource, expected) in [
            (
                "s3:GetObject",
                "arn:aws:s3:::sample-uploads/object",
                Decision::Allowed,
            ),
            (
                "s3:GetObject",
                "arn:aws:s3:::sample-other/object",
                Decision::NotAllowed,
            ),
            (
                "iam:PutRolePolicy",
                "arn:aws:iam::123456789012:role/sample-jobs",
                Decision::Allowed,
            ),
            (
                "iam:PutRolePermissionsBoundary",
                "arn:aws:iam::123456789012:role/sample-jobs",
                Decision::NotAllowed,
            ),
            (
                "iam:PutRolePolicy",
                "arn:aws:iam::123456789012:role/sample-volume-backups-compute",
                Decision::ExplicitlyDenied,
            ),
        ] {
            assert_eq!(
                evaluate(
                    &combined,
                    &Request::new(action, resource)
                        .resource_tags(&[("deployment", "sample"), ("managed-by", "runtime"),]),
                ),
                expected,
                "{name}: {action} on {resource}"
            );
        }
    }
}
