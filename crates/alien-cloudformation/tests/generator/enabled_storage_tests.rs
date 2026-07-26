//! `.enabled(input)` on `storage` — the multi-resource case.
//!
//! `kv` emits one resource, so the generator stamping the gate's condition onto
//! it is the whole story. `storage` emits a bucket, a bucket policy, and one IAM
//! policy per grant, and every one of them names the bucket. The failure mode is
//! an ungated resource left holding a `Ref` to a bucket the deployer declined:
//! that template still lints and still renders, and fails at deploy time.
//!
//! So the assertions below are about the whole resource set, not the bucket.

use super::helpers::{
    custom_resource_registration, entry_ids, gate_input, registration_payload,
    render_built_ins_template, resolve, Declined,
};
use alien_cloudformation::{CfExpression, CfTemplate, CloudFormationTarget};
use alien_core::{
    LifecycleRule, PermissionProfile, ResourceLifecycle, ServiceAccount, Stack, StackBuilder,
    StackSettings, Storage,
};
use std::collections::HashMap;

const GATE_CONDITION: &str = "InputFilesEnabledIsTrue";
const BUCKET_ID: &str = "Files";

fn gate() -> alien_core::StackInputDefinition {
    gate_input(
        "filesEnabled",
        "Enable the bucket",
        "Whether to create the object-storage bucket.",
    )
}

/// Renders and lints, so every assertion below is made about a template
/// `cfn-lint` already accepted.
fn render(stack: &Stack, description: &str) -> CfTemplate {
    render_built_ins_template(
        stack,
        StackSettings::default(),
        custom_resource_registration(),
        CloudFormationTarget::Aws,
        "aws",
        description,
    )
    .0
}

/// Versioning and lifecycle rules fill in the bucket's optional properties, and
/// the permission profile is what makes the emitter produce the IAM policies
/// that name the bucket — the resources most likely to be left ungated.
fn storage() -> Storage {
    Storage::new("files".to_string())
        .versioning(true)
        .lifecycle_rules(vec![LifecycleRule {
            days: 30,
            prefix: Some("tmp/".to_string()),
        }])
        .build()
}

fn stack_base() -> StackBuilder {
    Stack::new("gated-storage".to_string())
        .inputs(vec![gate()])
        .permission(
            "app",
            PermissionProfile::new().resource("files", ["storage/data-write"]),
        )
        .add(
            ServiceAccount::new("app-sa".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
}

fn gated_stack() -> Stack {
    stack_base()
        .add_enabled_when(storage(), ResourceLifecycle::Frozen, "filesEnabled")
        .build()
}

fn ungated_stack() -> Stack {
    stack_base()
        .add(storage(), ResourceLifecycle::Frozen)
        .build()
}

/// Logical ids of every resource that names `logical_id` anywhere in its
/// properties or `DependsOn` — the resources that would break if it vanished.
fn resources_referencing(template: &CfTemplate, logical_id: &str) -> Vec<String> {
    template
        .resources
        .iter()
        .filter(|(id, _)| *id != logical_id)
        .filter(|(_, resource)| {
            resource.depends_on.iter().any(|dep| dep == logical_id)
                || resource
                    .properties
                    .values()
                    .any(|value| names_resource(value, logical_id))
        })
        .map(|(id, _)| id.clone())
        .collect()
}

/// Whether an expression reaches `logical_id` through `Ref`, `Fn::GetAtt`, or a
/// `Fn::Sub` template that interpolates it.
fn names_resource(expression: &CfExpression, logical_id: &str) -> bool {
    match expression {
        CfExpression::String(value) => value.contains(&format!("${{{logical_id}}}")),
        CfExpression::Object(fields) => fields.iter().any(|(key, value)| match key.as_str() {
            "Ref" => value == &CfExpression::from(logical_id),
            "Fn::GetAtt" => match value {
                CfExpression::List(parts) => parts
                    .first()
                    .is_some_and(|part| part == &CfExpression::from(logical_id)),
                other => names_resource(other, logical_id),
            },
            _ => names_resource(value, logical_id),
        }),
        CfExpression::List(items) => items.iter().any(|item| names_resource(item, logical_id)),
        _ => false,
    }
}

/// The invariant that matters at deploy time: nothing that survives the "no"
/// answer may still point at the bucket. `cfn-lint` does not catch this — the
/// template is well-formed either way.
#[test]
fn nothing_ungated_is_left_pointing_at_a_declined_bucket() {
    let template = render(&gated_stack(), "gated storage stack");

    let mut referrers = resources_referencing(&template, BUCKET_ID);
    assert!(
        referrers.len() > 1,
        "the fixture must produce resources that name the bucket, or this proves nothing: \
         {referrers:?}"
    );

    // The registration custom resource is the one referrer that handles the
    // "no" answer itself: it reaches the bucket only from inside the gate's own
    // `Fn::If`, which drops the whole entry. `a_declined_bucket_leaves_no_
    // registration_entry` is what proves that.
    let registration = referrers
        .iter()
        .position(|id| id == "DeploymentRegistration")
        .expect("registration should carry the bucket's import data");
    referrers.remove(registration);

    for referrer in &referrers {
        assert_eq!(
            template.resources[referrer].condition.as_deref(),
            Some(GATE_CONDITION),
            "`{referrer}` names the bucket, so it must disappear with it; \
             referrers were {referrers:?}"
        );
    }
}

/// The bucket and every resource the emitter derives from it, named explicitly,
/// so a newly added sibling that forgets the gate shows up as a failure here
/// rather than at a customer's deploy.
#[test]
fn the_bucket_and_its_policies_all_carry_the_gate() {
    let template = render(&gated_stack(), "gated storage stack");

    let bucket = template
        .resources
        .get(BUCKET_ID)
        .expect("gated bucket should still be declared");
    assert_eq!(bucket.resource_type, "AWS::S3::Bucket");
    assert_eq!(bucket.condition.as_deref(), Some(GATE_CONDITION));

    let policy = template
        .resources
        .get(&format!("{BUCKET_ID}BucketPolicy"))
        .expect("bucket policy should still be declared");
    assert_eq!(policy.resource_type, "AWS::S3::BucketPolicy");
    assert_eq!(policy.condition.as_deref(), Some(GATE_CONDITION));

    let iam_policies = template
        .resources
        .iter()
        .filter(|(id, resource)| {
            resource.resource_type == "AWS::IAM::Policy" && id.starts_with(BUCKET_ID)
        })
        .collect::<Vec<_>>();
    assert!(
        !iam_policies.is_empty(),
        "the permission profile should produce at least one storage IAM policy"
    );
    for (id, resource) in iam_policies {
        assert_eq!(
            resource.condition.as_deref(),
            Some(GATE_CONDITION),
            "`{id}` embeds the bucket in its policy document"
        );
    }
}

/// The IAM policy attaches to a role this emitter does not own. Gating that role
/// would take the service account's other grants down with the bucket.
#[test]
fn the_service_account_role_is_not_gated() {
    let template = render(&gated_stack(), "gated storage stack");

    let roles = template
        .resources
        .iter()
        .filter(|(_, resource)| resource.resource_type == "AWS::IAM::Role")
        .collect::<Vec<_>>();
    assert!(!roles.is_empty(), "the stack should declare a role");
    for (id, resource) in roles {
        assert_eq!(
            resource.condition.as_deref(),
            None,
            "`{id}` belongs to the service account, not the bucket"
        );
    }
}

/// Registration runs the typed importer over every entry it receives, so a
/// declined bucket has to be absent from the payload rather than present with a
/// null one.
#[test]
fn a_declined_bucket_leaves_no_registration_entry() {
    let template = render(&gated_stack(), "gated storage stack");
    let payload = registration_payload(&template);

    let accepted = resolve(
        &payload,
        &HashMap::from([(GATE_CONDITION, true)]),
        Declined::Removed,
    )
    .expect("payload survives when the gate is on");
    assert!(
        entry_ids(&accepted).contains(&"files".to_string()),
        "the bucket registers when the deployer says yes: {:?}",
        entry_ids(&accepted)
    );

    let declined = resolve(
        &payload,
        &HashMap::from([(GATE_CONDITION, false)]),
        Declined::Removed,
    )
    .expect("payload survives when the gate is off");
    assert!(
        !entry_ids(&declined).contains(&"files".to_string()),
        "the declined bucket must be absent from the payload, not null: {:?}",
        entry_ids(&declined)
    );
    assert!(
        entry_ids(&declined).contains(&"app-sa".to_string()),
        "the ungated service account still registers: {:?}",
        entry_ids(&declined)
    );
}

/// Ungated stacks gain no conditions: opt-in means no `.enabled(...)`, so no gating.
#[test]
fn an_ungated_storage_stack_gains_no_conditions() {
    let template = render(&ungated_stack(), "ungated storage stack");

    assert!(
        template.conditions.is_empty(),
        "nothing is gated, so no condition belongs in the template: {:?}",
        template.conditions
    );
    for (id, resource) in template.resources.iter() {
        assert_eq!(
            resource.condition.as_deref(),
            None,
            "`{id}` gained a condition on an ungated stack"
        );
    }
    assert!(entry_ids(&registration_payload(&template)).contains(&"files".to_string()));
}
