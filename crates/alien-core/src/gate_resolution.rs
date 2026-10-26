//! How a stack's `.enabled()` gates resolve to a yes or no, shared by the deployment steps that
//! strip declined resources and the preflight runner that hashes the stack they will keep.

use crate::{
    ownership_policy_for_resource_type, GateAnswers, Platform, ResourceEntry, Stack,
    StackInputDefaultValue, StackInputDefinition,
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

/// Whether a gate's input is excluded from the deployment platform.
pub fn gate_is_off_platform(
    inputs: &[StackInputDefinition],
    input_id: &str,
    platform: Platform,
) -> bool {
    inputs
        .iter()
        .find(|input| input.id == input_id)
        .and_then(|input| input.platforms.as_ref())
        .is_some_and(|platforms| !platforms.contains(&platform))
}

/// Remove resources whose gate input is unavailable on this platform, including their links
/// and grants. Unlike a mutable live decline, these resources can never be enabled here.
pub fn remove_off_platform_gated_resources(stack: &mut Stack, platform: Platform) {
    let declined = stack
        .resources()
        .filter_map(|(id, entry)| {
            let input_id = entry.enabled_when.as_deref()?;
            gate_is_off_platform(&stack.inputs, input_id, platform).then(|| id.clone())
        })
        .collect::<Vec<_>>();
    remove_declined_resources(stack, &declined);
}

/// Inputs scoped away from the platform always resolve false, even with a provided value.
/// The deployer's answer for a live gate: the provided value, else the
/// input's declared boolean default. The error is the reason, for the caller to wrap.
pub fn gate_resolves_true(
    inputs: &[StackInputDefinition],
    input_id: &str,
    input_values: &HashMap<String, serde_json::Value>,
    resource_id: &str,
    platform: Platform,
) -> Result<bool, String> {
    if gate_is_off_platform(inputs, input_id, platform) {
        return Ok(false);
    }
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
    platform: Platform,
) -> Result<(bool, &'static str), String> {
    if gate_is_off_platform(inputs, input_id, platform) {
        return Ok((false, "platform"));
    }
    // The recorded answer outranks the default only while the input actually
    // gates a frozen resource — that is what dominance means. Once a release
    // frees the input, its recorded answer is history, and a live gate
    // resolves the way any other live gate does.
    //
    // The source travels with the answer so the audit log cannot describe a
    // precedence this function did not apply.
    if input_values.contains_key(input_id) {
        return Ok((
            gate_resolves_true(inputs, input_id, input_values, resource_id, platform)?,
            "provided",
        ));
    }
    if still_frozen_gating.contains(input_id) {
        if let Some(answer) = persisted_gate_answers.get(input_id) {
            return Ok((*answer, "persisted"));
        }
    }
    Ok((
        gate_resolves_true(inputs, input_id, input_values, resource_id, platform)?,
        "default",
    ))
}

/// The ids of the gated live resources whose input resolves false.
pub fn declined_live_resources(
    stack: &Stack,
    input_values: &HashMap<String, serde_json::Value>,
    persisted_gate_answers: &GateAnswers,
    still_frozen_gating: &HashSet<String>,
    platform: Platform,
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
            platform,
        )?;
        if !accepted {
            declined.push(resource_id.clone());
        }
    }
    Ok(declined)
}

/// Frozen-gate answers read off a stack whose Frozen declines were already stripped: a surviving
/// gated setup-created resource reads as yes, even when its gate was never answered. So a strip
/// built on it never drops what the real strip keeps: a digest can miss a match, never fake one.
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

/// Take declined gated resources out of `stack` with every link, ordering edge and grant naming
/// them. The deployment strips and the preflight runner's digest projection share it, so the
/// installed stack and the target they compare are stripped the same way.
pub fn remove_declined_resources(stack: &mut Stack, declined: &[String]) {
    if declined.is_empty() {
        return;
    }

    for resource_id in declined {
        tracing::info!(
            resource_id = %resource_id,
            "The deployer declined this gated resource; it leaves the desired stack"
        );
        stack.resources.shift_remove(resource_id);
    }

    // Removing the resource without its inbound links would leave a survivor pointing at
    // something that was never created, which the executor and binding resolution both
    // reject. Scrubbing here is what lets an ungated resource link a gated one.
    for (resource_id, entry) in stack.resources.iter_mut() {
        let dropped = match crate::resource_links_mut(&mut entry.config) {
            Some(owner) => {
                let before = owner.links().len();
                owner
                    .links_mut()
                    .retain(|link| !declined.contains(&link.id));
                before - owner.links().len()
            }
            None => 0,
        };
        if dropped > 0 {
            tracing::info!(
                resource_id = %resource_id,
                dropped,
                declined = ?declined,
                "Dropped links to declined resources; this resource keeps its own lifecycle"
            );
        }

        let ordering_before = entry.dependencies.len();
        entry
            .dependencies
            .retain(|dependency| !declined.contains(&dependency.id));
        // The release-time preflight refuses authored ordering edges onto gated resources,
        // so one reaching here predates the rule; dropping it keeps the stack coherent.
        if ordering_before > entry.dependencies.len() {
            tracing::info!(
                resource_id = %resource_id,
                dropped = ordering_before - entry.dependencies.len(),
                "Dropped ordering edges to declined resources"
            );
        }
    }

    scrub_declined_grants(stack, declined);
}

/// Drop grants naming a declined resource from every permission profile.
///
/// Not inert: GCP applies every non-`"*"` entry without consulting the desired resources.
/// Nothing is lost, because the mutations re-derive them whenever the gate is accepted.
fn scrub_declined_grants(stack: &mut Stack, declined: &[String]) {
    let scrub = |profile: &mut crate::permissions::PermissionProfile| {
        for resource_id in declined {
            if profile.0.shift_remove(resource_id).is_some() {
                tracing::info!(
                    resource_id = %resource_id,
                    "Dropped the grant for a declined resource"
                );
            }
        }
    };

    for profile in stack.permissions.profiles.values_mut() {
        scrub(profile);
    }
    match &mut stack.permissions.management {
        crate::permissions::ManagementPermissions::Extend(profile)
        | crate::permissions::ManagementPermissions::Override(profile) => scrub(profile),
        crate::permissions::ManagementPermissions::Auto => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn off_platform_gate_needs_neither_a_valid_value_nor_a_default() {
        let mut input = StackInputDefinition::deployer_boolean(
            "managedBucket",
            "Managed bucket",
            "Create a bucket",
            None,
        );
        input.platforms = Some(vec![Platform::Aws]);
        let values = HashMap::from([(input.id.clone(), serde_json::json!("not a boolean"))]);
        assert!(!gate_resolves_true(
            std::slice::from_ref(&input),
            &input.id,
            &values,
            "data",
            Platform::Machines
        )
        .unwrap());
        assert!(gate_resolves_true(
            std::slice::from_ref(&input),
            &input.id,
            &values,
            "data",
            Platform::Aws
        )
        .is_err());
    }

    #[test]
    fn platform_scope_overrides_defaults_provided_and_recorded_answers() {
        for platforms in [None, Some(vec![Platform::Aws]), Some(vec![])] {
            for default in [false, true] {
                let mut input = StackInputDefinition::deployer_boolean(
                    "managedBucket",
                    "Managed bucket",
                    "Create a bucket",
                    Some(default),
                );
                input.platforms = platforms.clone();
                for platform in [
                    Platform::Aws,
                    Platform::Machines,
                    Platform::Local,
                    Platform::Kubernetes,
                ] {
                    for provided in [None, Some(false), Some(true)] {
                        let values = provided
                            .map(|value| (input.id.clone(), serde_json::json!(value)))
                            .into_iter()
                            .collect();
                        let excluded = platforms
                            .as_ref()
                            .is_some_and(|scope| !scope.contains(&platform));
                        let expected = !excluded && provided.unwrap_or(default);
                        assert_eq!(
                            gate_resolves_true(
                                std::slice::from_ref(&input),
                                &input.id,
                                &values,
                                "data",
                                platform
                            )
                            .unwrap(),
                            expected
                        );
                        for recorded in [false, true] {
                            let answers = GateAnswers::from([(input.id.clone(), recorded)]);
                            for frozen in [HashSet::new(), HashSet::from([input.id.clone()])] {
                                let expected = !excluded
                                    && provided.unwrap_or(if frozen.is_empty() {
                                        default
                                    } else {
                                        recorded
                                    });
                                let (actual, source) = live_gate_resolves_true(
                                    std::slice::from_ref(&input),
                                    &input.id,
                                    &values,
                                    &answers,
                                    &frozen,
                                    "data",
                                    platform,
                                )
                                .unwrap();
                                assert_eq!(actual, expected);
                                if excluded {
                                    assert_eq!(source, "platform");
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
