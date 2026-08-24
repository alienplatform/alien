//! `.enabled(input)` for `Queue` on CloudFormation.
//!
//! `enabled_tests.rs` covers the mechanics on `Kv`. A queue is where the
//! registration payload gets exercised against a stack that mixes a gated
//! resource with an ungated neighbour, on both of the paths that publish it.

use super::helpers::{
    custom_resource_registration, gate_input, render_built_ins_template, resolve, Declined,
};
use alien_cloudformation::{CfExpression, CfTemplate, CloudFormationTarget, RegistrationMode};
use alien_core::{
    PermissionProfile, Queue, ResourceLifecycle, ServiceAccount, Stack, StackBuilder,
    StackInputDefinition, StackSettings,
};
use std::collections::HashMap;

fn render_as(stack: &Stack, registration: RegistrationMode, description: &str) -> CfTemplate {
    render_built_ins_template(
        stack,
        StackSettings::default(),
        registration,
        CloudFormationTarget::Aws,
        "aws",
        description,
    )
    .0
}

fn render(stack: &Stack, description: &str) -> CfTemplate {
    render_as(stack, custom_resource_registration(), description)
}

const QUEUE_CONDITION: &str = "InputQueueEnabledIsTrue";

fn gate_inputs() -> Vec<StackInputDefinition> {
    vec![gate_input(
        "queueEnabled",
        "queue",
        "Whether to create the queue.",
    )]
}

/// The service account gives the "off" answer an ungated neighbour to leave
/// behind, so the payload assertions say something about which entries survive
/// rather than about the payload being empty.
fn base() -> StackBuilder {
    Stack::new("gated-stack".to_string())
        .inputs(gate_inputs())
        .permission(
            "execution",
            PermissionProfile::new().resource("jobs", ["queue/data-write"]),
        )
        .add(
            ServiceAccount::new("execution-sa".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
}

fn stack(gated: bool) -> Stack {
    let queue = Queue::new("jobs".to_string()).build();
    let builder = base();
    if gated {
        builder
            .add_enabled_when(queue, ResourceLifecycle::Frozen, "queueEnabled")
            .build()
    } else {
        builder.add(queue, ResourceLifecycle::Frozen).build()
    }
}

fn registration_entry_ids(template: &CfTemplate, conditions: &HashMap<&str, bool>) -> Vec<String> {
    let payload = template
        .resources
        .get("DeploymentRegistration")
        .expect("registration custom resource")
        .properties
        .get("Resources")
        .expect("registration resource list")
        .clone();
    let CfExpression::List(entries) = resolve(&payload, conditions, Declined::Removed)
        .expect("payload survives condition resolution")
    else {
        panic!("registration payload should resolve to a list");
    };
    entries
        .iter()
        .map(|entry| {
            let CfExpression::Object(fields) = entry else {
                panic!("registration entry should be an object: {entry:?}");
            };
            match fields.get("id").expect("registration entry id") {
                CfExpression::String(id) => id.clone(),
                other => panic!("registration entry id should be a string: {other:?}"),
            }
        })
        .collect()
}

/// The `DeploymentResources` output, resolved and parsed the way the registering
/// consumer reads it.
///
/// Returns the raw text alongside the parsed value so a caller can assert on the
/// exact bytes CloudFormation would hand over, not just on what survived
/// `serde_json`.
fn resolved_outputs_payload(
    template: &CfTemplate,
    conditions: &HashMap<&str, bool>,
) -> (String, serde_json::Value) {
    let value = &template
        .outputs
        .get("DeploymentResources")
        .expect("outputs fallback should carry the resources payload")
        .value;
    let resolved = resolve(value, conditions, Declined::Removed)
        .expect("the resources output survives condition resolution");
    let CfExpression::String(text) = resolved else {
        panic!("the resources output should resolve to JSON text: {resolved:?}");
    };
    let parsed = serde_json::from_str(&text)
        .unwrap_or_else(|error| panic!("resources output should be valid JSON: {error}\n{text}"));
    (text, parsed)
}

/// Extract ids from the Outputs payload, verifying that every entry is a
/// well-formed object.
fn outputs_entry_ids(template: &CfTemplate, conditions: &HashMap<&str, bool>) -> Vec<String> {
    let (text, parsed) = resolved_outputs_payload(template, conditions);
    let serde_json::Value::Array(entries) = parsed else {
        panic!("resources output should be a JSON array: {text}");
    };
    entries
        .iter()
        .map(|entry| {
            let id = entry
                .get("id")
                .unwrap_or_else(|| panic!("registration entry has no id: {entry}\n{text}"));
            id.as_str()
                .unwrap_or_else(|| panic!("registration entry id should be a string: {id}"))
                .to_string()
        })
        .collect()
}

/// Logical ids of every resource carrying `condition`.
fn resources_conditioned_on(template: &CfTemplate, condition: &str) -> Vec<String> {
    let mut ids = template
        .resources
        .iter()
        .filter(|(_, resource)| resource.condition.as_deref() == Some(condition))
        .map(|(logical_id, _)| logical_id.clone())
        .collect::<Vec<_>>();
    ids.sort();
    ids
}

#[test]
fn a_gated_queue_is_created_only_when_the_deployer_says_yes() {
    let template = render(&stack(true), "gated queue");

    let queue = template
        .resources
        .get("Jobs")
        .expect("gated queue should still be declared");
    assert_eq!(queue.resource_type, "AWS::SQS::Queue");
    assert_eq!(queue.condition.as_deref(), Some(QUEUE_CONDITION));
    assert_eq!(
        resources_conditioned_on(&template, QUEUE_CONDITION),
        vec![
            "Jobs".to_string(),
            "JobsExecutionSaRoleQueuePermission00".to_string(),
        ],
        "the queue owns the SQS queue and its resource-scoped grant, both gated"
    );
}

/// Registration runs the typed importer over every entry it receives, so a
/// declined resource must be absent, not present-and-null.
#[test]
fn declined_resources_leave_no_registration_entry() {
    let template = render(&stack(true), "gated queue");

    let queue_on = HashMap::from([(QUEUE_CONDITION, true)]);
    assert_eq!(
        registration_entry_ids(&template, &queue_on),
        vec!["execution-sa".to_string(), "jobs".to_string()],
        "everything registers when the deployer says yes"
    );

    let queue_off = HashMap::from([(QUEUE_CONDITION, false)]);
    assert_eq!(
        registration_entry_ids(&template, &queue_off),
        vec!["execution-sa".to_string()],
        "declined resources must be absent from the payload, not null"
    );
}

/// The same invariant on the Outputs fallback, which is the path `alien render`
/// picks whenever no notification lambda is configured.
///
/// It carries the payload as JSON text rather than as a list, and the intrinsic
/// that produces that text renders a declined entry as a literal `null` instead
/// of dropping it. Nothing about that is visible in the rendered template — it
/// only happens once CloudFormation resolves the conditions — so this asserts on
/// the resolved text.
#[test]
fn the_outputs_fallback_omits_declined_resources() {
    let template = render_as(
        &stack(true),
        RegistrationMode::OutputsFallback,
        "gated queue, outputs fallback",
    );

    let queue_on = HashMap::from([(QUEUE_CONDITION, true)]);
    assert_eq!(
        outputs_entry_ids(&template, &queue_on),
        vec!["execution-sa".to_string(), "jobs".to_string()],
        "everything registers when the deployer says yes"
    );

    let queue_off = HashMap::from([(QUEUE_CONDITION, false)]);
    let (text, _) = resolved_outputs_payload(&template, &queue_off);
    assert!(
        !text.contains("null"),
        "a declined resource must leave nothing behind, but the payload still \
         carries a null: {text}"
    );
    assert_eq!(
        outputs_entry_ids(&template, &queue_off),
        vec!["execution-sa".to_string()],
        "only the ungated resource registers"
    );
}

/// Declining every gated resource must still leave a payload the consumer can
/// read, rather than a malformed array or a stray delimiter.
#[test]
fn the_outputs_fallback_survives_every_resource_being_declined() {
    let template = render_as(
        &Stack::new("gated-stack".to_string())
            .inputs(gate_inputs())
            .add_enabled_when(
                Queue::new("jobs".to_string()).build(),
                ResourceLifecycle::Frozen,
                "queueEnabled",
            )
            .build(),
        RegistrationMode::OutputsFallback,
        "every resource gated, outputs fallback",
    );

    let all_off = HashMap::from([(QUEUE_CONDITION, false)]);
    let (text, parsed) = resolved_outputs_payload(&template, &all_off);
    assert_eq!(text, "[]", "an all-declined payload is an empty array");
    assert_eq!(parsed, serde_json::json!([]));
}

/// Ungated stacks gain no conditions: opt-in means no `.enabled(...)`, so no gating.
#[test]
fn an_ungated_stack_gains_no_conditions() {
    let template = render(&stack(false), "ungated queue");

    assert!(
        template.conditions.is_empty(),
        "nothing is gated, so no condition belongs in the template: {:?}",
        template.conditions
    );
    assert!(
        template
            .resources
            .values()
            .all(|resource| resource.condition.is_none()),
        "no resource may carry a condition when nothing is gated"
    );
    assert_eq!(
        registration_entry_ids(&template, &HashMap::new()),
        vec!["execution-sa".to_string(), "jobs".to_string()],
    );
}
