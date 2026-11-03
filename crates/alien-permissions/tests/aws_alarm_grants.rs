mod iam_policy;

use alien_core::{AwsConditionValue, ALIEN_STACK_TAG_KEY};
use alien_permissions::{
    generators::{AwsIamStatement, AwsRuntimePermissionsGenerator},
    get_permission_set, BindingTarget, PermissionContext,
};
use iam_policy::{evaluate, Decision, Request};

const OWN_ALARM: &str = "arn:aws:cloudwatch:us-east-1:123456789012:alarm:acme-api-cpu";
// "acme-prod" is another deployment whose name starts with "acme-".
const OTHER_ALARM: &str = "arn:aws:cloudwatch:us-east-1:123456789012:alarm:acme-prod-api-cpu";

fn context() -> PermissionContext {
    PermissionContext::new()
        .with_stack_prefix("acme")
        .with_resource_name("api")
        .with_resource_id("api")
        .with_managing_account_id("123456789012")
        .with_aws_account_id("123456789012")
        .with_aws_region("us-east-1")
}

fn alarm_statements(id: &str, target: BindingTarget) -> Vec<AwsIamStatement> {
    let set = get_permission_set(id).expect("registered permission set");
    AwsRuntimePermissionsGenerator::new()
        .generate_policy(set, target, &context())
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

fn alarm_writing_sets() -> Vec<&'static str> {
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
                let puts = actions.iter().any(|action| {
                    matches!(
                        action.as_str(),
                        "cloudwatch:putmetricalarm" | "cloudwatch:tagresource"
                    )
                });
                let deletes = actions
                    .iter()
                    .any(|action| action == "cloudwatch:deletealarms");
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
    let own_tag = [(ALIEN_STACK_TAG_KEY, "acme")];
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
