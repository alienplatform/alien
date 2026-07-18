use crate::error::Result;
use crate::{CheckResult, CompileTimeCheck};
use alien_core::{Platform, Stack, StackInputKind, StackInputProvider};
use std::collections::HashMap;

/// Validates every deploy-time permission gate against its stack input.
///
/// A gate keeps a permission-set grant only while its input resolves to the
/// gate's value; the setup emitters render that as a Terraform `count` / a
/// CloudFormation `Fn::Equals` condition. Running this before emission closes
/// the ways a malformed gate silently degrades to an unconditional grant
/// (fail-open) or a broken condition:
///
/// - an `input_id` that matches no declared input, or an input the deployer
///   cannot set on the target platform (not deployer-provided, or platform-
///   scoped away), makes `deployer_permission_gate` return `None`, so the
///   emitter drops the gate and emits the grant every time;
/// - an input with no environment mapping can't be resolved on the managed
///   deploy path, so that path would keep the grant unconditionally;
/// - a `Secret` or `StringList` input can't drive an equality condition — a
///   secret would be compared to a plaintext literal, a list is a type mismatch —
///   so the Terraform emitter already rejects them; this check rejects them for
///   CloudFormation and the SDK too;
/// - a value that can never equal a `Boolean`/`Number`/`Enum` input's domain
///   means the grant is silently never included.
pub struct PermissionGatesValidCheck;

#[async_trait::async_trait]
impl CompileTimeCheck for PermissionGatesValidCheck {
    fn description(&self) -> &'static str {
        "Every permission gate must target a declared input whose kind and value can drive it"
    }

    fn should_run(&self, stack: &Stack, _platform: Platform) -> bool {
        !stack.permissions.gates.is_empty()
    }

    async fn check(&self, stack: &Stack, platform: Platform) -> Result<CheckResult> {
        let mut errors = Vec::new();

        for gate in &stack.permissions.gates {
            let Some(input) = stack.inputs.iter().find(|input| input.id == gate.input_id) else {
                errors.push(format!(
                    "Permission gate on '{}' references input '{}' which is not declared on the \
                     stack; an unresolved gate input would emit the grant unconditionally.",
                    gate.permission_set_id, gate.input_id
                ));
                continue;
            };

            // Mirror of the emitter's `deployer_permission_gate` steerability test:
            // an input the deployer cannot set on this platform never applies, so the
            // emitter would render the grant unconditionally (fail-open).
            let deployer_steerable = input.provided_by.contains(&StackInputProvider::Deployer)
                && input
                    .platforms
                    .as_ref()
                    .is_none_or(|platforms| platforms.contains(&platform));
            if !deployer_steerable {
                errors.push(format!(
                    "Permission gate on '{}' targets input '{}', which the deployer cannot set on \
                     {:?} (its providedBy excludes the deployer, or its platforms exclude {:?}); \
                     the gate would never apply and the grant would be emitted unconditionally.",
                    gate.permission_set_id, gate.input_id, platform, platform
                ));
                continue;
            }

            // Required for parity: the managed deploy path resolves a gate through
            // the input's first env mapping, even though the self-hosted setup
            // templates gate on the input variable directly.
            if input.env.first().is_none() {
                errors.push(format!(
                    "Permission gate on '{}' targets input '{}', which declares no environment \
                     mapping; the managed deploy path resolves a gate through the input's env \
                     mapping, so without one the grant would be kept unconditionally.",
                    gate.permission_set_id, gate.input_id
                ));
                continue;
            }

            match input.kind {
                StackInputKind::Secret | StackInputKind::StringList => {
                    errors.push(format!(
                        "Permission gate on '{}' targets input '{}' of kind {:?}, which cannot \
                         drive an equality condition; gate on a string, boolean, number, or enum \
                         input instead.",
                        gate.permission_set_id, gate.input_id, input.kind
                    ));
                }
                StackInputKind::Boolean => {
                    if gate.enabled_value != "true" && gate.enabled_value != "false" {
                        errors.push(format!(
                            "Permission gate on '{}' targets boolean input '{}' but its enabled \
                             value is '{}'; use \"true\" or \"false\".",
                            gate.permission_set_id, gate.input_id, gate.enabled_value
                        ));
                    }
                }
                StackInputKind::Number => {
                    if gate.enabled_value.parse::<f64>().is_err() {
                        errors.push(format!(
                            "Permission gate on '{}' targets number input '{}' but its enabled \
                             value '{}' is not a number.",
                            gate.permission_set_id, gate.input_id, gate.enabled_value
                        ));
                    }
                }
                StackInputKind::Integer => {
                    if gate.enabled_value.parse::<i64>().is_err() {
                        errors.push(format!(
                            "Permission gate on '{}' targets integer input '{}' but its enabled \
                             value '{}' is not an integer.",
                            gate.permission_set_id, gate.input_id, gate.enabled_value
                        ));
                    }
                }
                StackInputKind::Enum => {
                    if let Some(values) = input.validation.as_ref().and_then(|v| v.values.as_ref()) {
                        if !values.contains(&gate.enabled_value) {
                            errors.push(format!(
                                "Permission gate on '{}' targets enum input '{}' with enabled \
                                 value '{}', which is not one of its allowed values [{}]; the \
                                 grant could never be included.",
                                gate.permission_set_id,
                                gate.input_id,
                                gate.enabled_value,
                                values
                                    .iter()
                                    .map(|v| format!("'{}'", v))
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            ));
                        }
                    }
                }
                StackInputKind::String => {}
            }
        }

        // A grant can be gated on only one input: the emitter's `gate_for`
        // returns the first gate on a (profile, resource, set) triple while the
        // mutation applies every gate, so two divergent gates on the same triple
        // split the setup template from the managed deploy.
        let mut seen_gate: HashMap<(&str, &str, &str), (&str, &str)> = HashMap::new();
        for gate in &stack.permissions.gates {
            let key = (
                gate.profile.as_str(),
                gate.resource.as_str(),
                gate.permission_set_id.as_str(),
            );
            let this = (gate.input_id.as_str(), gate.enabled_value.as_str());
            if let Some(&prev) = seen_gate.get(&key) {
                if prev != this {
                    errors.push(format!(
                        "Permission set '{}' in profile '{}' has two conflicting gates on '{}': \
                         input '{}' = '{}' and input '{}' = '{}'. A grant can be gated on only one \
                         input.",
                        gate.permission_set_id,
                        gate.profile,
                        gate.resource,
                        prev.0,
                        prev.1,
                        this.0,
                        this.1
                    ));
                }
            } else {
                seen_gate.insert(key, this);
            }
        }

        // A set granted under both a resource's own key and the wildcard "*" is
        // emitted with origin_keys = [resource_id, "*"], and
        // `deployer_permission_gate` falls back to an unconditional grant unless
        // BOTH keys carry the same gate. So a set gated on one key but ungated or
        // divergently gated on the other is a silent fail-open. The resource-level
        // `.enabled()` lowering never produces this (it uses the same input on
        // every key it gates), but a hand-written gate can.
        for (profile_name, profile) in &stack.permissions.profiles {
            let Some(wildcard_refs) = profile.0.get("*") else {
                continue;
            };
            for (resource_key, refs) in &profile.0 {
                if resource_key == "*" {
                    continue;
                }
                for set in refs {
                    let set_id = set.id();
                    if !wildcard_refs.iter().any(|w| w.id() == set_id) {
                        continue;
                    }
                    let resource_gate =
                        stack.permissions.gate_for(profile_name, resource_key, set_id);
                    let wildcard_gate = stack.permissions.gate_for(profile_name, "*", set_id);
                    let divergent = match (resource_gate, wildcard_gate) {
                        (Some(a), Some(b)) => {
                            a.input_id != b.input_id || a.enabled_value != b.enabled_value
                        }
                        (Some(_), None) | (None, Some(_)) => true,
                        (None, None) => false,
                    };
                    if divergent {
                        errors.push(format!(
                            "Permission set '{}' in profile '{}' is granted under both resource \
                             '{}' and the wildcard '*', but the two are gated differently; the \
                             setup emitter merges them and would emit the grant unconditionally. \
                             Gate both grants on the same input and value, or grant the set under \
                             only one key.",
                            set_id, profile_name, resource_key
                        ));
                    }
                }
            }
        }

        if errors.is_empty() {
            Ok(CheckResult::success())
        } else {
            Ok(CheckResult::failed(errors))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{
        PermissionGate, PermissionProfile, PermissionSetReference, PermissionsConfig,
        StackInputDefinition, StackInputEnvironmentMapping, StackInputKind, StackInputProvider,
        StackInputValidation,
    };
    use indexmap::IndexMap;

    fn input(id: &str, kind: StackInputKind, values: Option<Vec<String>>) -> StackInputDefinition {
        StackInputDefinition {
            id: id.to_string(),
            kind,
            provided_by: vec![StackInputProvider::Deployer],
            required: false,
            label: id.to_string(),
            description: "Gate input.".to_string(),
            placeholder: None,
            default: None,
            platforms: None,
            validation: values.map(|values| StackInputValidation {
                min_length: None,
                max_length: None,
                pattern: None,
                format: None,
                min: None,
                max: None,
                values: Some(values),
                min_items: None,
                max_items: None,
            }),
            env: vec![StackInputEnvironmentMapping {
                name: format!("APP_{}", id.to_uppercase()),
                target_resources: None,
                var_type: None,
            }],
        }
    }

    fn stack(gate: PermissionGate, inputs: Vec<StackInputDefinition>) -> Stack {
        let mut permissions = PermissionsConfig::new();
        permissions.gates = vec![gate];
        Stack {
            id: "test".to_string(),
            resources: IndexMap::new(),
            permissions,
            supported_platforms: None,
            inputs,
        }
    }

    fn gate(input_id: &str, enabled_value: &str) -> PermissionGate {
        PermissionGate {
            profile: "execution".to_string(),
            resource: "jobs".to_string(),
            permission_set_id: "queue/data-write".to_string(),
            input_id: input_id.to_string(),
            enabled_value: enabled_value.to_string(),
        }
    }

    #[tokio::test]
    async fn boolean_gate_with_true_value_passes() {
        let stack = stack(
            gate("kvEnabled", "true"),
            vec![input("kvEnabled", StackInputKind::Boolean, None)],
        );
        let result = PermissionGatesValidCheck
            .check(&stack, Platform::Aws)
            .await
            .unwrap();
        assert!(result.success, "{:?}", result.errors);
    }

    #[tokio::test]
    async fn undeclared_input_fails() {
        let stack = stack(gate("missing", "true"), vec![]);
        let result = PermissionGatesValidCheck
            .check(&stack, Platform::Aws)
            .await
            .unwrap();
        assert!(!result.success);
        assert!(result.errors[0].contains("not declared"));
    }

    #[tokio::test]
    async fn secret_input_gate_fails() {
        let stack = stack(
            gate("apiKey", "anything"),
            vec![input("apiKey", StackInputKind::Secret, None)],
        );
        let result = PermissionGatesValidCheck
            .check(&stack, Platform::Aws)
            .await
            .unwrap();
        assert!(!result.success);
        assert!(result.errors[0].contains("cannot"));
    }

    #[tokio::test]
    async fn boolean_gate_with_non_boolean_value_fails() {
        let stack = stack(
            gate("kvEnabled", "yes"),
            vec![input("kvEnabled", StackInputKind::Boolean, None)],
        );
        let result = PermissionGatesValidCheck
            .check(&stack, Platform::Aws)
            .await
            .unwrap();
        assert!(!result.success);
        assert!(result.errors[0].contains("true"));
    }

    #[tokio::test]
    async fn enum_gate_with_out_of_domain_value_fails() {
        let stack = stack(
            gate("mode", "nope"),
            vec![input(
                "mode",
                StackInputKind::Enum,
                Some(vec!["on".to_string(), "off".to_string()]),
            )],
        );
        let result = PermissionGatesValidCheck
            .check(&stack, Platform::Aws)
            .await
            .unwrap();
        assert!(!result.success);
        assert!(result.errors[0].contains("allowed values"));
    }

    #[tokio::test]
    async fn string_list_gate_fails() {
        let stack = stack(
            gate("tags", "x"),
            vec![input("tags", StackInputKind::StringList, None)],
        );
        let result = PermissionGatesValidCheck
            .check(&stack, Platform::Aws)
            .await
            .unwrap();
        assert!(!result.success);
        assert!(result.errors[0].contains("cannot"));
    }

    #[tokio::test]
    async fn integer_gate_rejects_fractional_value() {
        let stack = stack(
            gate("replicas", "3.5"),
            vec![input("replicas", StackInputKind::Integer, None)],
        );
        let result = PermissionGatesValidCheck
            .check(&stack, Platform::Aws)
            .await
            .unwrap();
        assert!(!result.success);
        assert!(result.errors[0].contains("integer"));
    }

    #[tokio::test]
    async fn string_gate_accepts_any_value() {
        let stack = stack(
            gate("region", "anything"),
            vec![input("region", StackInputKind::String, None)],
        );
        let result = PermissionGatesValidCheck
            .check(&stack, Platform::Aws)
            .await
            .unwrap();
        assert!(result.success, "{:?}", result.errors);
    }

    #[tokio::test]
    async fn non_deployer_input_gate_fails() {
        let mut developer_input = input("mode", StackInputKind::String, None);
        developer_input.provided_by = vec![StackInputProvider::Developer];
        let stack = stack(gate("mode", "x"), vec![developer_input]);
        let result = PermissionGatesValidCheck
            .check(&stack, Platform::Aws)
            .await
            .unwrap();
        assert!(!result.success);
        assert!(result.errors[0].contains("deployer cannot set"));
    }

    #[tokio::test]
    async fn number_gate_accepts_numeric_value() {
        let stack = stack(
            gate("ratio", "0.5"),
            vec![input("ratio", StackInputKind::Number, None)],
        );
        let result = PermissionGatesValidCheck
            .check(&stack, Platform::Aws)
            .await
            .unwrap();
        assert!(result.success, "{:?}", result.errors);
    }

    #[tokio::test]
    async fn enum_gate_with_in_domain_value_passes() {
        let stack = stack(
            gate("mode", "on"),
            vec![input(
                "mode",
                StackInputKind::Enum,
                Some(vec!["on".to_string(), "off".to_string()]),
            )],
        );
        let result = PermissionGatesValidCheck
            .check(&stack, Platform::Aws)
            .await
            .unwrap();
        assert!(result.success, "{:?}", result.errors);
    }

    #[tokio::test]
    async fn env_less_input_gate_fails() {
        let mut no_env = input("mode", StackInputKind::String, None);
        no_env.env = vec![];
        let stack = stack(gate("mode", "x"), vec![no_env]);
        let result = PermissionGatesValidCheck
            .check(&stack, Platform::Aws)
            .await
            .unwrap();
        assert!(!result.success);
        assert!(result.errors[0].contains("environment mapping"));
    }

    #[tokio::test]
    async fn platform_restricted_input_gate_fails() {
        // The input only applies on GCP, but the stack is built for AWS, so the
        // gate could never take effect there — deployer_permission_gate would
        // fall back to an unconditional grant.
        let mut gcp_only = input("mode", StackInputKind::String, None);
        gcp_only.platforms = Some(vec![Platform::Gcp]);
        let stack = stack(gate("mode", "x"), vec![gcp_only]);
        let result = PermissionGatesValidCheck
            .check(&stack, Platform::Aws)
            .await
            .unwrap();
        assert!(!result.success);
        assert!(result.errors[0].contains("cannot set on"));
    }

    /// Build a stack whose `execution` profile grants `kv/data-write` under both
    /// the `store` resource key and the wildcard `"*"`, with the given gates.
    fn multi_key_stack(
        store_gate: Option<PermissionGate>,
        wildcard_gate: Option<PermissionGate>,
        inputs: Vec<StackInputDefinition>,
    ) -> Stack {
        let mut profile = PermissionProfile::new();
        profile
            .0
            .insert("store".to_string(), vec![PermissionSetReference::from_name("kv/data-write")]);
        profile
            .0
            .insert("*".to_string(), vec![PermissionSetReference::from_name("kv/data-write")]);
        let mut permissions = PermissionsConfig::new();
        permissions.profiles.insert("execution".to_string(), profile);
        permissions.gates = [store_gate, wildcard_gate].into_iter().flatten().collect();
        Stack {
            id: "test".to_string(),
            resources: IndexMap::new(),
            permissions,
            supported_platforms: None,
            inputs,
        }
    }

    fn keyed_gate(resource: &str, input_id: &str) -> PermissionGate {
        PermissionGate {
            profile: "execution".to_string(),
            resource: resource.to_string(),
            permission_set_id: "kv/data-write".to_string(),
            input_id: input_id.to_string(),
            enabled_value: "true".to_string(),
        }
    }

    #[tokio::test]
    async fn divergent_multi_key_gate_fails() {
        // Same set gated on different inputs under the resource key vs. "*": the
        // merged emitter falls back to an unconditional grant.
        let stack = multi_key_stack(
            Some(keyed_gate("store", "storeEnabled")),
            Some(keyed_gate("*", "globalEnabled")),
            vec![
                input("storeEnabled", StackInputKind::Boolean, None),
                input("globalEnabled", StackInputKind::Boolean, None),
            ],
        );
        let result = PermissionGatesValidCheck.check(&stack, Platform::Aws).await.unwrap();
        assert!(!result.success);
        assert!(result.errors.iter().any(|e| e.contains("granted under both")));
    }

    #[tokio::test]
    async fn partial_multi_key_gate_fails() {
        // Gated under the resource key, ungated under "*": the merged emitter
        // would still emit the grant unconditionally.
        let stack = multi_key_stack(
            Some(keyed_gate("store", "storeEnabled")),
            None,
            vec![input("storeEnabled", StackInputKind::Boolean, None)],
        );
        let result = PermissionGatesValidCheck.check(&stack, Platform::Aws).await.unwrap();
        assert!(!result.success);
        assert!(result.errors.iter().any(|e| e.contains("gated differently")));
    }

    #[tokio::test]
    async fn matching_multi_key_gate_passes() {
        // Same set gated identically under both keys: the emitter keeps the gate.
        let stack = multi_key_stack(
            Some(keyed_gate("store", "storeEnabled")),
            Some(keyed_gate("*", "storeEnabled")),
            vec![input("storeEnabled", StackInputKind::Boolean, None)],
        );
        let result = PermissionGatesValidCheck.check(&stack, Platform::Aws).await.unwrap();
        assert!(result.success, "{:?}", result.errors);
    }

    #[tokio::test]
    async fn conflicting_gates_on_same_grant_fails() {
        // Two gates on the same (profile, resource, set) with different inputs:
        // the emitter would use the first, the mutation both.
        let mut permissions = PermissionsConfig::new();
        permissions.gates = vec![keyed_gate("store", "inputX"), keyed_gate("store", "inputY")];
        let stack = Stack {
            id: "test".to_string(),
            resources: IndexMap::new(),
            permissions,
            supported_platforms: None,
            inputs: vec![
                input("inputX", StackInputKind::Boolean, None),
                input("inputY", StackInputKind::Boolean, None),
            ],
        };
        let result = PermissionGatesValidCheck.check(&stack, Platform::Aws).await.unwrap();
        assert!(!result.success);
        assert!(result.errors.iter().any(|e| e.contains("two conflicting gates")));
    }
}
