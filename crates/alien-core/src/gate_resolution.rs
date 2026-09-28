//! How a stack's `.enabled()` gates resolve to a yes or no, shared by the deployment steps that
//! strip declined resources and the preflight runner that hashes the stack they will keep.

use crate::{
    ownership_policy_for_resource_type, GateAnswers, ResourceEntry, Stack, StackInputDefaultValue,
    StackInputDefinition,
};
use std::collections::{HashMap, HashSet};

/// A gate value from the wire: JSON booleans stay booleans, and the
/// CloudFormation parameter strings "true"/"false" coerce — CloudFormation
/// has no boolean parameter type, so its registration payloads deliver gate
/// answers as strings. Anything else is `None`, refused loudly by callers.
pub fn gate_value_as_bool(value: &serde_json::Value) -> Option<bool> {
    match value {
        serde_json::Value::Bool(answer) => Some(*answer),
        serde_json::Value::String(text) => match text.as_str() {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

/// The gate input of a setup-created gated resource, `None` for anything else.
pub fn frozen_gate_of(entry: &ResourceEntry) -> Option<&str> {
    gate_of(entry, true)
}

/// The gate input of a runtime-created gated resource, `None` for anything
/// else. The mirror of [`frozen_gate_of`] — which side of the setup boundary a
/// gated resource falls on is decided in exactly these two places.
pub fn live_gate_of(entry: &ResourceEntry) -> Option<&str> {
    gate_of(entry, false)
}

fn gate_of(entry: &ResourceEntry, setup_created: bool) -> Option<&str> {
    let input_id = entry.enabled_when.as_deref()?;
    let emitted_in_setup =
        ownership_policy_for_resource_type(entry.config.resource_type().as_ref())
            .should_emit_in_setup(entry.lifecycle);
    (emitted_in_setup == setup_created).then_some(input_id)
}

/// Every gated resource setup creates, as `(resource_id, input_id)`.
pub fn frozen_gated(stack: &Stack) -> impl Iterator<Item = (&String, &str)> {
    stack
        .resources()
        .filter_map(|(resource_id, entry)| Some((resource_id, frozen_gate_of(entry)?)))
}

/// Every gated resource the runtime creates, as `(resource_id, input_id)`.
pub fn live_gated(stack: &Stack) -> impl Iterator<Item = (&String, &str)> {
    stack
        .resources()
        .filter_map(|(resource_id, entry)| Some((resource_id, live_gate_of(entry)?)))
}

/// The deployer's answer for a live gate: the provided value, else the
/// input's declared boolean default. The error is the reason, for the caller to wrap.
pub fn gate_resolves_true(
    inputs: &[StackInputDefinition],
    input_id: &str,
    input_values: &HashMap<String, serde_json::Value>,
    resource_id: &str,
) -> Result<bool, String> {
    if let Some(value) = input_values.get(input_id) {
        return gate_value_as_bool(value).ok_or_else(|| {
            format!(
                "Input '{input_id}' enables resource '{resource_id}' but its value is not \
                 a boolean: {value}"
            )
        });
    }

    match inputs
        .iter()
        .find(|input| input.id == input_id)
        .and_then(|input| input.default.as_ref())
    {
        Some(StackInputDefaultValue::Boolean(answer)) => Ok(*answer),
        _ => Err(format!(
            "Input '{input_id}' enables resource '{resource_id}' but no value was provided \
             and the input declares no boolean default"
        )),
    }
}

/// A live gate's answer: the provided value, else the answer recorded when
/// the deployment was created (frozen dominance — a live resource sharing a
/// frozen-gating input follows the fixed answer, not the declared default),
/// else the declared default.
pub fn live_gate_resolves_true(
    inputs: &[StackInputDefinition],
    input_id: &str,
    input_values: &HashMap<String, serde_json::Value>,
    persisted_gate_answers: &GateAnswers,
    still_frozen_gating: &HashSet<String>,
    resource_id: &str,
) -> Result<(bool, &'static str), String> {
    // The recorded answer outranks the default only while the input actually
    // gates a frozen resource — that is what dominance means. Once a release
    // frees the input, its recorded answer is history, and a live gate
    // resolves the way any other live gate does.
    //
    // The source travels with the answer so the audit log cannot describe a
    // precedence this function did not apply.
    if input_values.contains_key(input_id) {
        return Ok((
            gate_resolves_true(inputs, input_id, input_values, resource_id)?,
            "provided",
        ));
    }
    if still_frozen_gating.contains(input_id) {
        if let Some(answer) = persisted_gate_answers.get(input_id) {
            return Ok((*answer, "persisted"));
        }
    }
    Ok((
        gate_resolves_true(inputs, input_id, input_values, resource_id)?,
        "default",
    ))
}

/// The ids of the gated live resources whose input resolves false.
pub fn declined_live_resources(
    stack: &Stack,
    input_values: &HashMap<String, serde_json::Value>,
    persisted_gate_answers: &GateAnswers,
    still_frozen_gating: &HashSet<String>,
) -> Result<Vec<String>, String> {
    let mut declined = Vec::new();
    for (resource_id, input_id) in live_gated(stack) {
        let (accepted, _source) = live_gate_resolves_true(
            &stack.inputs,
            input_id,
            input_values,
            persisted_gate_answers,
            still_frozen_gating,
            resource_id,
        )?;
        if !accepted {
            declined.push(resource_id.clone());
        }
    }
    Ok(declined)
}

/// Frozen-gate answers read off a stack whose Frozen declines were already stripped, for a caller
/// without the recorded answers: a setup-created resource still carrying a gate means its answer
/// was yes, and a declined one left no resource to read.
pub fn surviving_frozen_gate_answers(stack: &Stack) -> (GateAnswers, HashSet<String>) {
    let still_frozen_gating: HashSet<String> = frozen_gated(stack)
        .map(|(_, input_id)| input_id.to_string())
        .collect();
    let answers = still_frozen_gating
        .iter()
        .map(|input_id| (input_id.clone(), true))
        .collect();
    (answers, still_frozen_gating)
}
