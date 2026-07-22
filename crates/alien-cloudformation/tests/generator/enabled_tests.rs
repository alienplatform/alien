//! `.enabled(input)` — a resource the deployer can decline.
//!
//! The gate reaches CloudFormation as a parameter, so the template has to stay
//! deployable under both answers. The load-bearing case is the "no" answer:
//! registration runs the typed importer over every entry in the payload, so a
//! declined resource has to be absent from it. An entry that is present but
//! null, or present with null fields, fails deserialization rather than being
//! skipped.

use super::helpers::{
    custom_resource_registration, entry_ids, gate_input, registration_payload,
    render_built_ins_template, resolve, try_render_built_ins, Declined,
};
use alien_cloudformation::{CfExpression, CfTemplate, CloudFormationTarget, RegistrationMode};
use alien_core::{
    Kv, Queue, ResourceLifecycle, ServiceAccount, Stack, StackInputKind, StackSettings,
};
use std::collections::HashMap;

const GATE_PARAMETER: &str = "InputStoreEnabled";
const GATE_CONDITION: &str = "InputStoreEnabledIsTrue";
const TABLE_ID: &str = "Store";

fn gate() -> alien_core::StackInputDefinition {
    gate_input(
        "storeEnabled",
        "Enable the store",
        "Whether to create the key-value store.",
    )
}

fn generate(stack: &Stack, description: &str) -> alien_core::Result<CfTemplate> {
    try_render_built_ins(
        stack,
        StackSettings::default(),
        custom_resource_registration(),
        CloudFormationTarget::Aws,
        "aws",
        description,
    )
}

/// Renders and lints, so every assertion below is made about a template
/// `cfn-lint` already accepted.
fn render(stack: &Stack, description: &str) -> (CfTemplate, String) {
    render_built_ins_template(
        stack,
        StackSettings::default(),
        custom_resource_registration(),
        CloudFormationTarget::Aws,
        "aws",
        description,
    )
}

fn gated_kv_stack() -> Stack {
    Stack::new("gated-stack".to_string())
        .inputs(vec![gate()])
        .add_enabled_when(
            Kv::new("store".to_string()).build(),
            ResourceLifecycle::Frozen,
            "storeEnabled",
        )
        .build()
}

fn ungated_kv_stack() -> Stack {
    Stack::new("gated-stack".to_string())
        .inputs(vec![gate()])
        .add(
            Kv::new("store".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build()
}

/// One gated resource beside an ungated one, so the "off" answer has something
/// to leave behind, and the guard is shown to fire only on what is gated.
fn mixed_stack() -> Stack {
    Stack::new("gated-stack".to_string())
        .inputs(vec![gate()])
        .add_enabled_when(
            Kv::new("store".to_string()).build(),
            ResourceLifecycle::Frozen,
            "storeEnabled",
        )
        .add(
            Queue::new("jobs".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build()
}

/// The `importData` a resolved payload carries for `resource_id`.
fn import_data(payload: &CfExpression, resource_id: &str) -> CfExpression {
    let CfExpression::List(entries) = payload else {
        panic!("registration payload should be a list: {payload:?}");
    };
    entries
        .iter()
        .find_map(|entry| {
            let CfExpression::Object(fields) = entry else {
                return None;
            };
            (fields.get("id") == Some(&CfExpression::from(resource_id))).then(|| {
                fields
                    .get("importData")
                    .expect("registration entry importData")
                    .clone()
            })
        })
        .unwrap_or_else(|| panic!("no registration entry for '{resource_id}'"))
}

/// The point of the feature: the table itself is conditional, not just something
/// derived from it.
#[test]
fn a_gated_table_is_created_only_when_the_deployer_says_yes() {
    let (template, yaml) = render(&gated_kv_stack(), "gated kv stack");

    let table = template
        .resources
        .get(TABLE_ID)
        .expect("gated table should still be declared");
    assert_eq!(table.resource_type, "AWS::DynamoDB::Table");
    assert_eq!(table.condition.as_deref(), Some(GATE_CONDITION));

    insta::assert_snapshot!("enabled_gated_kv", yaml);
}

/// A dangling condition is the silent failure mode here: the resource renders
/// conditionally, but against a parameter the deployer is never asked for.
#[test]
fn the_gate_condition_tests_the_input_parameter() {
    let (template, _) = render(&gated_kv_stack(), "gated kv stack");

    let parameter = template
        .parameters
        .get(GATE_PARAMETER)
        .expect("gate input should be a deployer parameter");
    assert_eq!(parameter.parameter_type, "String");
    assert_eq!(
        parameter.allowed_values,
        Some(vec![
            CfExpression::from("true"),
            CfExpression::from("false"),
        ]),
        "a boolean input reaches CloudFormation as a constrained string"
    );

    assert_eq!(
        template.conditions.get(GATE_CONDITION),
        Some(&CfExpression::equals(
            CfExpression::ref_(GATE_PARAMETER),
            CfExpression::from("true"),
        ))
    );
}

/// The invariant that actually matters: registration must never see an entry for
/// a resource the deployer declined, because it will try to deserialize it.
#[test]
fn a_declined_resource_leaves_no_registration_entry() {
    let (template, _) = render(&mixed_stack(), "gated plus ungated");
    let payload = registration_payload(&template);

    let accepted = resolve(
        &payload,
        &HashMap::from([(GATE_CONDITION, true)]),
        Declined::Removed,
    )
    .expect("payload survives when the gate is on");
    assert_eq!(
        entry_ids(&accepted),
        vec!["store".to_string(), "jobs".to_string()],
        "both resources register when the deployer says yes"
    );
    assert_eq!(
        import_data(&accepted, "store"),
        CfExpression::object([
            ("tableName", CfExpression::ref_(TABLE_ID)),
            ("tableArn", CfExpression::get_att(TABLE_ID, "Arn")),
        ]),
        "the accepted payload still points at the real table"
    );

    let declined = resolve(
        &payload,
        &HashMap::from([(GATE_CONDITION, false)]),
        Declined::Removed,
    )
    .expect("payload survives when the gate is off");
    assert_eq!(
        entry_ids(&declined),
        vec!["jobs".to_string()],
        "the declined resource must be absent from the payload, not null"
    );
}

/// The element is removed via `AWS::NoValue` specifically. Nulling it instead
/// would still render and still lint, and would still break registration.
#[test]
fn the_declined_entry_is_removed_rather_than_blanked() {
    let (template, _) = render(&gated_kv_stack(), "gated kv stack");
    let CfExpression::List(entries) = registration_payload(&template) else {
        panic!("registration payload should be a list");
    };
    let [entry] = &entries[..] else {
        panic!("expected exactly one registration entry: {entries:?}");
    };
    let CfExpression::Object(fields) = entry else {
        panic!("registration entry should be an object: {entry:?}");
    };
    let Some(CfExpression::List(branches)) = fields.get("Fn::If") else {
        panic!("a gated entry wraps in Fn::If: {fields:?}");
    };

    assert_eq!(branches[0], CfExpression::from(GATE_CONDITION));
    assert_eq!(
        branches[2],
        CfExpression::no_value(),
        "the off branch must delete the element, not blank it"
    );
}

/// An ungated stack's output must not change, or every existing deployment
/// would see a template diff on its next re-apply.
#[test]
fn an_ungated_stack_gains_no_conditions() {
    let (template, _) = render(&ungated_kv_stack(), "ungated kv stack");

    assert!(
        template.conditions.is_empty(),
        "nothing is gated, so no condition belongs in the template: {:?}",
        template.conditions
    );
    assert_eq!(
        template
            .resources
            .get(TABLE_ID)
            .expect("table")
            .condition
            .as_deref(),
        None
    );

    let payload = registration_payload(&template);
    assert_eq!(entry_ids(&payload), vec!["store".to_string()]);
    assert_eq!(
        import_data(&payload, "store"),
        CfExpression::object([
            ("tableName", CfExpression::ref_(TABLE_ID)),
            ("tableArn", CfExpression::get_att(TABLE_ID, "Arn")),
        ]),
        "an ungated registration entry stays a plain reference"
    );
}

/// Rendering a gated resource through an emitter that ignores the gate would
/// create exactly what the deployer declined. ServiceAccount stays a safe
/// stand-in for "unconverted": the compile-time check forbids gating
/// framework-derived types, so its emitter never needs to convert.
#[test]
fn a_gate_on_an_unconverted_emitter_fails() {
    let stack = Stack::new("gated-stack".to_string())
        .inputs(vec![gate()])
        .add_enabled_when(
            ServiceAccount::new("execution-sa".to_string()).build(),
            ResourceLifecycle::Frozen,
            "storeEnabled",
        )
        .build();

    let error = generate(&stack, "unconverted emitter").expect_err("should refuse to render");
    assert_eq!(error.code, "OPERATION_NOT_SUPPORTED");
    assert!(
        error.message.contains("service-account"),
        "the error should name the resource type: {}",
        error.message
    );
}

/// A gate on an input the template never asks for would emit a condition
/// referencing an undeclared parameter.
#[test]
fn a_gate_on_an_undeclared_input_fails() {
    let stack = Stack::new("gated-stack".to_string())
        .add_enabled_when(
            Kv::new("store".to_string()).build(),
            ResourceLifecycle::Frozen,
            "storeEnabled",
        )
        .build();

    let error = generate(&stack, "undeclared gate input").expect_err("should refuse to render");
    assert_eq!(error.code, "OPERATION_NOT_SUPPORTED");
    assert!(
        error.message.contains("storeEnabled"),
        "the error should name the missing input: {}",
        error.message
    );
}

/// The gate compares against `"true"`, which only means anything for a boolean.
#[test]
fn a_gate_on_a_non_boolean_input_fails() {
    let mut input = gate();
    input.kind = StackInputKind::String;
    input.default = None;

    let stack = Stack::new("gated-stack".to_string())
        .inputs(vec![input])
        .add_enabled_when(
            Kv::new("store".to_string()).build(),
            ResourceLifecycle::Frozen,
            "storeEnabled",
        )
        .build();

    let error = generate(&stack, "non-boolean gate input").expect_err("should refuse to render");
    assert_eq!(error.code, "OPERATION_NOT_SUPPORTED");
    assert!(
        error.message.contains("boolean"),
        "the error should say a boolean was expected: {}",
        error.message
    );
}

/// The same invariant on the Outputs fallback, the path `alien render` picks
/// whenever no notification lambda is configured. It carries the payload as
/// JSON text, and the intrinsic producing that text renders a declined entry
/// as a literal `null` instead of dropping it. Nothing about that is visible
/// in the rendered template — it only happens once CloudFormation resolves the
/// conditions — so this renders through the linter and then asserts on the
/// resolved text the registering consumer reads.
#[test]
fn the_outputs_fallback_omits_a_declined_resource() {
    let (template, _) = render_built_ins_template(
        &mixed_stack(),
        StackSettings::default(),
        RegistrationMode::OutputsFallback,
        CloudFormationTarget::Aws,
        "aws",
        "gated plus ungated, outputs fallback",
    );

    let payload = template
        .outputs
        .get("DeploymentResources")
        .expect("outputs fallback should carry the resources payload")
        .value
        .clone();

    for (answer, expected) in [
        (true, vec!["store".to_string(), "jobs".to_string()]),
        (false, vec!["jobs".to_string()]),
    ] {
        let resolved = resolve(
            &payload,
            &HashMap::from([(GATE_CONDITION, answer)]),
            Declined::Removed,
        )
        .expect("the resources output survives condition resolution");
        let CfExpression::String(text) = resolved else {
            panic!("the resources output should resolve to JSON text: {resolved:?}");
        };
        assert!(
            !text.contains("null"),
            "a declined resource must leave nothing behind, but the payload \
             carries a null with the gate {answer}: {text}"
        );
        let parsed: serde_json::Value = serde_json::from_str(&text)
            .unwrap_or_else(|error| panic!("resources output should be valid JSON: {error}\n{text}"));
        let serde_json::Value::Array(entries) = parsed else {
            panic!("resources output should be a JSON array: {text}");
        };
        let ids: Vec<String> = entries
            .iter()
            .map(|entry| {
                entry
                    .get("id")
                    .and_then(|id| id.as_str())
                    .unwrap_or_else(|| panic!("registration entry has no string id: {entry}"))
                    .to_string()
            })
            .collect();
        assert_eq!(ids, expected, "gate {answer} registers exactly what exists");
    }
}
