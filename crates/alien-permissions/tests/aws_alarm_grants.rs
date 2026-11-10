mod iam_policy;

use alien_core::{AwsConditionValue, ALIEN_RESOURCE_TAG_KEY, ALIEN_STACK_TAG_KEY};
use alien_permissions::{
    generators::{AwsIamStatement, AwsRuntimePermissionsGenerator},
    get_permission_set, BindingTarget, PermissionContext,
};
use iam_policy::{evaluate, Decision, Request};

const OWN_ALARM: &str = "arn:aws:cloudwatch:us-east-1:123456789012:alarm:acme-api-cpu";
// "acme-prod" is another deployment whose name starts with "acme-".
const OTHER_ALARM: &str = "arn:aws:cloudwatch:us-east-1:123456789012:alarm:acme-prod-api-cpu";

/// A resource's permission context. Its resource bindings name the resource by the variable
/// their set already uses: `${resourceId}` (container, daemon) or `${resourceName}`
/// (compute cluster, Postgres), both the bare resource id.
fn context_for(stack: &str, resource: &str) -> PermissionContext {
    PermissionContext::new()
        .with_stack_prefix(stack)
        .with_resource_name(resource)
        .with_resource_id(resource)
        .with_managing_account_id("123456789012")
        .with_aws_account_id("123456789012")
        .with_aws_region("us-east-1")
}

fn alarm_statements(id: &str, target: BindingTarget) -> Vec<AwsIamStatement> {
    alarm_statements_for(id, target, &context_for("acme", "api"))
}

fn alarm_statements_for(
    id: &str,
    target: BindingTarget,
    context: &PermissionContext,
) -> Vec<AwsIamStatement> {
    let set = get_permission_set(id).expect("registered permission set");
    AwsRuntimePermissionsGenerator::new()
        .generate_policy(set, target, context)
        .unwrap()
        .statement
        .into_iter()
        .filter(|statement| {
            statement
                .action
                .iter()
                .any(|action| action.to_ascii_lowercase().starts_with("cloudwatch:"))
        })
        .collect()
}

/// Every set that writes alarms. The five sets that keep alarms next to their resources must
/// be among them, so no test here passes by finding none.
fn alarm_writing_sets() -> Vec<&'static str> {
    let sets = all_alarm_writing_sets();
    for id in [
        "container/provision",
        "daemon/provision",
        "compute-cluster/management",
        "compute-cluster/provision",
        "postgres/provision",
    ] {
        assert!(sets.contains(&id), "{id} writes alarms: {sets:?}");
    }
    sets
}

fn all_alarm_writing_sets() -> Vec<&'static str> {
    alien_permissions::list_permission_set_ids()
        .into_iter()
        .filter(|id| {
            get_permission_set(id)
                .unwrap()
                .platforms
                .aws
                .iter()
                .flatten()
                .flat_map(|entry| entry.grant.actions.iter().flatten())
                .any(|action| {
                    let action = action.to_ascii_lowercase();
                    ["putmetricalarm", "deletealarms", "tagresource"]
                        .iter()
                        .any(|write| {
                            iam_policy::wildcard_match(&action, &format!("cloudwatch:{write}"))
                        })
                })
        })
        .collect()
}

fn condition<'a>(
    statement: &'a AwsIamStatement,
    operator: &str,
    key: &str,
) -> Option<&'a AwsConditionValue> {
    statement.condition.as_ref()?.get(operator)?.get(key)
}

// Alarm actions run as CloudWatch, not as the role that put the alarm, so an alarm action
// could stop instances or publish to topics outside the deployment. Every grant that can put
// an alarm must therefore refuse actions, and every alarm write must stay on alarms that
// carry the deployment's tag, since a name prefix alone also matches longer deployment names.
#[test]
fn every_alarm_write_refuses_actions_and_is_bound_to_the_deployment_tag() {
    let sets = alarm_writing_sets();
    assert!(
        sets.contains(&"container/provision") && sets.contains(&"compute-cluster/management"),
        "{sets:?}"
    );
    let stack_tag = format!("aws:RequestTag/{ALIEN_STACK_TAG_KEY}");
    let existing_tag = format!("aws:ResourceTag/{ALIEN_STACK_TAG_KEY}");
    let own = AwsConditionValue::from("acme");
    for id in sets {
        for target in [BindingTarget::Stack, BindingTarget::Resource] {
            for statement in alarm_statements(id, target) {
                let actions: Vec<_> = statement
                    .action
                    .iter()
                    .map(|action| action.to_ascii_lowercase())
                    .collect();
                assert!(
                    actions.iter().all(|action| !action.contains('*')),
                    "{id} {target:?}: {actions:?}"
                );
                let puts = actions
                    .iter()
                    .any(|action| action == "cloudwatch:putmetricalarm");
                let tags = actions
                    .iter()
                    .any(|action| action == "cloudwatch:tagresource");
                let deletes = actions
                    .iter()
                    .any(|action| action == "cloudwatch:deletealarms");
                assert!(
                    !(puts && tags),
                    "{id} {target:?}: TagResource needs its own, stricter statement"
                );
                if tags {
                    // Retagging needs the alarm to carry the tag already: with IfExists an
                    // untagged alarm under the prefix could be tagged in and then deleted.
                    assert_eq!(
                        condition(&statement, "StringEquals", &existing_tag),
                        Some(&own),
                        "{id} {target:?}"
                    );
                    assert_eq!(
                        condition(&statement, "StringEquals", &stack_tag),
                        Some(&own),
                        "{id} {target:?}"
                    );
                }
                if puts {
                    assert_eq!(
                        condition(&statement, "Null", "cloudwatch:AlarmActions"),
                        Some(&AwsConditionValue::from("true")),
                        "{id} {target:?} allows alarm actions: {statement:?}"
                    );
                    assert_eq!(
                        condition(&statement, "StringEquals", &stack_tag),
                        Some(&own),
                        "{id} {target:?}"
                    );
                    assert_eq!(
                        condition(&statement, "StringEqualsIfExists", &existing_tag),
                        Some(&own),
                        "{id} {target:?}"
                    );
                }
                if deletes {
                    assert_eq!(
                        condition(&statement, "StringEquals", &existing_tag),
                        Some(&own),
                        "{id} {target:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn alarm_grants_reach_only_this_deployments_alarms_without_actions() {
    let own_tag = [
        (ALIEN_STACK_TAG_KEY, "acme"),
        (ALIEN_RESOURCE_TAG_KEY, "api"),
    ];
    let other_tag = [(ALIEN_STACK_TAG_KEY, "acme-prod")];
    for id in alarm_writing_sets() {
        for target in [BindingTarget::Stack, BindingTarget::Resource] {
            let statements = alarm_statements(id, target);
            let decide = |request: Request| evaluate(&statements, &request);
            let put = |resource: &str| Request::new("cloudwatch:PutMetricAlarm", resource);

            // A new alarm with the deployment tag and no actions.
            assert_eq!(
                decide(put(OWN_ALARM).request_tags(&own_tag)),
                Decision::Allowed,
                "{id} {target:?}"
            );
            // Replacing an alarm this deployment owns.
            assert_eq!(
                decide(
                    put(OWN_ALARM)
                        .request_tags(&own_tag)
                        .resource_tags(&own_tag)
                ),
                Decision::Allowed,
                "{id} {target:?}"
            );
            assert_eq!(
                decide(put(OWN_ALARM)),
                Decision::NotAllowed,
                "{id} {target:?} untagged"
            );
            assert_eq!(
                decide(put(OWN_ALARM).request_tags(&own_tag).with_values(
                    "cloudwatch:AlarmActions",
                    &["arn:aws:automate:us-east-1:ec2:stop"]
                )),
                Decision::NotAllowed,
                "{id} {target:?} with actions"
            );
            // An alarm of another deployment under the same name prefix: neither replaced,
            // retagged nor deleted.
            let foreign = "arn:aws:cloudwatch:us-east-1:123456789012:alarm:acme-api-imported";
            assert_eq!(
                decide(
                    put(foreign)
                        .request_tags(&own_tag)
                        .resource_tags(&other_tag)
                ),
                Decision::NotAllowed,
                "{id} {target:?}"
            );
            assert_eq!(
                decide(
                    Request::new("cloudwatch:TagResource", foreign)
                        .request_tags(&own_tag)
                        .resource_tags(&other_tag)
                ),
                Decision::NotAllowed,
                "{id} {target:?}"
            );
            assert_eq!(
                decide(Request::new("cloudwatch:DeleteAlarms", foreign).resource_tags(&other_tag)),
                Decision::NotAllowed,
                "{id} {target:?}"
            );
            assert_eq!(
                decide(Request::new("cloudwatch:DeleteAlarms", OWN_ALARM).resource_tags(&own_tag)),
                Decision::Allowed,
                "{id} {target:?}"
            );
            // Another deployment's own alarm names stay out of reach for the resource binding.
            if target == BindingTarget::Resource {
                assert_eq!(
                    decide(
                        Request::new("cloudwatch:DeleteAlarms", OTHER_ALARM)
                            .resource_tags(&own_tag)
                    ),
                    Decision::NotAllowed,
                    "{id}"
                );
            }
            for action in [
                "cloudwatch:SetAlarmState",
                "cloudwatch:PutCompositeAlarm",
                "cloudwatch:DeleteDashboards",
            ] {
                assert_eq!(
                    decide(Request::new(action, OWN_ALARM).resource_tags(&own_tag)),
                    Decision::NotAllowed,
                    "{id} {target:?} {action}"
                );
            }
        }
    }
}

/// "a" and "a-prod" are two deployments; every name of a-prod's starts with "a-". Its alarms
/// carry its own tag, and none of a's alarm writes is allowed on them. On an untagged alarm
/// named under a-prod the only write allowed is an overwrite, through the stack binding, which
/// leaves it untagged (CloudWatch ignores tags on an update): it can't be deleted, retagged or
/// untagged.
#[test]
fn a_deployment_cannot_take_over_the_alarms_of_a_deployment_whose_name_extends_its_own() {
    let own_tag = [(ALIEN_STACK_TAG_KEY, "a"), (ALIEN_RESOURCE_TAG_KEY, "api")];
    let prod_tag = [
        (ALIEN_STACK_TAG_KEY, "a-prod"),
        (ALIEN_RESOURCE_TAG_KEY, "api"),
    ];
    let prod_alarm = "arn:aws:cloudwatch:us-east-1:123456789012:alarm:a-prod-api-cpu";
    for id in alarm_writing_sets() {
        for target in [BindingTarget::Stack, BindingTarget::Resource] {
            let statements = alarm_statements_for(id, target, &context_for("a", "api"));
            let decide = |request: Request| evaluate(&statements, &request);
            // Every write except an overwrite is denied, on a-prod's tagged alarm and on an
            // untagged one alike.
            for resource_tags in [&prod_tag[..], &[]] {
                for action in [
                    "cloudwatch:TagResource",
                    "cloudwatch:UntagResource",
                    "cloudwatch:DeleteAlarms",
                    "cloudwatch:SetAlarmState",
                    "cloudwatch:EnableAlarmActions",
                    "cloudwatch:DisableAlarmActions",
                ] {
                    assert_eq!(
                        decide(
                            Request::new(action, prod_alarm)
                                .request_tags(&own_tag)
                                .resource_tags(resource_tags)
                        ),
                        Decision::NotAllowed,
                        "{id} {target:?} {action} {resource_tags:?}"
                    );
                }
            }
            assert_eq!(
                decide(
                    Request::new("cloudwatch:PutMetricAlarm", prod_alarm)
                        .request_tags(&own_tag)
                        .resource_tags(&prod_tag)
                ),
                Decision::NotAllowed,
                "{id} {target:?} overwrite"
            );
            let overwrite_untagged = decide(
                Request::new("cloudwatch:PutMetricAlarm", prod_alarm).request_tags(&own_tag),
            );
            let expected = match target {
                BindingTarget::Stack => Decision::Allowed,
                _ => Decision::NotAllowed,
            };
            assert_eq!(overwrite_untagged, expected, "{id} {target:?}");
        }
    }
}

/// A resource's grants reach only its own alarms: "db" never reaches the alarms of "db2",
/// even though both carry the deployment's tag.
#[test]
fn a_resource_reaches_only_its_own_alarms() {
    let own_tag = [
        (ALIEN_STACK_TAG_KEY, "acme"),
        (ALIEN_RESOURCE_TAG_KEY, "db"),
    ];
    let db_alarm = "arn:aws:cloudwatch:us-east-1:123456789012:alarm:acme-db-writer-cpu";
    let db2_alarm = "arn:aws:cloudwatch:us-east-1:123456789012:alarm:acme-db2-writer-cpu";
    for id in alarm_writing_sets() {
        let statements =
            alarm_statements_for(id, BindingTarget::Resource, &context_for("acme", "db"));
        let decide = |request: Request| evaluate(&statements, &request);
        for (alarm, expected) in [
            (db_alarm, Decision::Allowed),
            (db2_alarm, Decision::NotAllowed),
        ] {
            assert_eq!(
                decide(
                    Request::new("cloudwatch:PutMetricAlarm", alarm)
                        .request_tags(&own_tag)
                        .resource_tags(&own_tag)
                ),
                expected,
                "{id} put {alarm}"
            );
            assert_eq!(
                decide(
                    Request::new("cloudwatch:TagResource", alarm)
                        .request_tags(&own_tag)
                        .resource_tags(&own_tag)
                ),
                expected,
                "{id} tag {alarm}"
            );
            assert_eq!(
                decide(Request::new("cloudwatch:DeleteAlarms", alarm).resource_tags(&own_tag)),
                expected,
                "{id} delete {alarm}"
            );
        }
    }
}

/// An Auto Scaling group's CPU alarm is named after the group (`<prefix>-<cluster id>-<group>`,
/// plus `-<domain>` for a group per domain), so the cluster's resource binding covers it, and
/// stack "a" never reaches stack "a-prod"'s group alarms.
#[test]
fn a_compute_cluster_reaches_only_its_own_group_alarms() {
    let own_tag = [
        (ALIEN_STACK_TAG_KEY, "a"),
        (ALIEN_RESOURCE_TAG_KEY, "compute"),
    ];
    let alarm = |name: &str| format!("arn:aws:cloudwatch:us-east-1:123456789012:alarm:{name}");
    for id in ["compute-cluster/management", "compute-cluster/provision"] {
        let statements =
            alarm_statements_for(id, BindingTarget::Resource, &context_for("a", "compute"));
        let decide = |request: Request| evaluate(&statements, &request);
        for (name, expected) in [
            ("a-compute-general-cpu", Decision::Allowed),
            ("a-compute-general-us-east-1a-cpu", Decision::Allowed),
            ("a-prod-compute-general-cpu", Decision::NotAllowed),
            ("a-compute2-general-cpu", Decision::NotAllowed),
        ] {
            assert_eq!(
                decide(
                    Request::new("cloudwatch:PutMetricAlarm", &alarm(name))
                        .request_tags(&own_tag)
                        .resource_tags(&own_tag)
                ),
                expected,
                "{id} put {name}"
            );
            assert_eq!(
                decide(
                    Request::new("cloudwatch:DeleteAlarms", &alarm(name)).resource_tags(&own_tag)
                ),
                expected,
                "{id} delete {name}"
            );
        }
    }
}

/// Stack "a" with a resource named "prod": its per-resource binding `a-prod-*` also matches
/// every alarm name of stack "a-prod". The deployment tag is what keeps a-prod's alarms out of
/// reach: none of a's alarm writes is allowed on an alarm tagged for a-prod.
#[test]
fn the_deployment_tag_denies_writes_when_a_resource_name_matches_another_deployment() {
    let own_tag = [(ALIEN_STACK_TAG_KEY, "a"), (ALIEN_RESOURCE_TAG_KEY, "prod")];
    let prod_tag = [
        (ALIEN_STACK_TAG_KEY, "a-prod"),
        (ALIEN_RESOURCE_TAG_KEY, "prod"),
    ];
    let prod_alarm = "arn:aws:cloudwatch:us-east-1:123456789012:alarm:a-prod-api-cpu";
    let own_alarm = "arn:aws:cloudwatch:us-east-1:123456789012:alarm:a-prod-writer-cpu";
    for id in alarm_writing_sets() {
        let statements =
            alarm_statements_for(id, BindingTarget::Resource, &context_for("a", "prod"));
        let decide = |request: Request| evaluate(&statements, &request);
        // The name matches this resource's own binding.
        assert_eq!(
            decide(
                Request::new("cloudwatch:PutMetricAlarm", own_alarm)
                    .request_tags(&own_tag)
                    .resource_tags(&own_tag)
            ),
            Decision::Allowed,
            "{id} own alarm"
        );
        for request in [
            Request::new("cloudwatch:PutMetricAlarm", prod_alarm)
                .request_tags(&own_tag)
                .resource_tags(&prod_tag),
            Request::new("cloudwatch:TagResource", prod_alarm)
                .request_tags(&own_tag)
                .resource_tags(&prod_tag),
            Request::new("cloudwatch:DeleteAlarms", prod_alarm).resource_tags(&prod_tag),
        ] {
            assert_eq!(decide(request), Decision::NotAllowed, "{id}");
        }
    }
}

/// Resource ids may contain dashes, so cluster "compute"'s binding `acme-compute-*` also matches
/// the names of cluster "compute-prod" in the same deployment, whose alarms carry the same
/// deployment tag. The resource tag every alarm carries is what keeps them apart: the
/// per-resource grants write only alarms tagged with their own resource.
#[test]
fn a_resource_cannot_write_the_alarms_of_a_sibling_whose_id_extends_its_own() {
    let deployment = (ALIEN_STACK_TAG_KEY, "acme");
    let own = [deployment, (ALIEN_RESOURCE_TAG_KEY, "compute")];
    let sibling = [deployment, (ALIEN_RESOURCE_TAG_KEY, "compute-prod")];
    let own_alarm = "arn:aws:cloudwatch:us-east-1:123456789012:alarm:acme-compute-general-cpu";
    let sibling_alarm =
        "arn:aws:cloudwatch:us-east-1:123456789012:alarm:acme-compute-prod-general-cpu";
    for id in alarm_writing_sets() {
        let statements =
            alarm_statements_for(id, BindingTarget::Resource, &context_for("acme", "compute"));
        let decide = |request: Request| evaluate(&statements, &request);
        assert_eq!(
            decide(
                Request::new("cloudwatch:PutMetricAlarm", own_alarm)
                    .request_tags(&own)
                    .resource_tags(&own)
            ),
            Decision::Allowed,
            "{id} own alarm"
        );
        assert_eq!(
            decide(Request::new("cloudwatch:DeleteAlarms", own_alarm).resource_tags(&own)),
            Decision::Allowed,
            "{id} delete own alarm"
        );
        for request in [
            // A new alarm must carry its own resource's tag.
            Request::new("cloudwatch:PutMetricAlarm", sibling_alarm).request_tags(&sibling),
            Request::new("cloudwatch:PutMetricAlarm", sibling_alarm)
                .request_tags(&own)
                .resource_tags(&sibling),
            Request::new("cloudwatch:TagResource", sibling_alarm)
                .request_tags(&own)
                .resource_tags(&sibling),
            Request::new("cloudwatch:DeleteAlarms", sibling_alarm).resource_tags(&sibling),
        ] {
            assert_eq!(
                decide(request),
                Decision::NotAllowed,
                "{id} sibling's alarm"
            );
        }
    }
}
