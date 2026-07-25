use crate::{
    DeploymentConfig, DeploymentState, DeploymentStatus, DeploymentStepResult, ErrorData, Result,
};
use alien_core::{ClientConfig, EnvironmentInfo, Platform, Stack, StackState};
use alien_error::AlienError;
use alien_error::Context;
use tracing::info;

/// Handle Pending → InitialSetup transition
///
/// This step:
/// 1. Initializes stack state with platform-specific settings
/// 2. Collects environment information from the cloud platform
/// 3. Runs preflight checks (mutations are applied in subsequent phases)
pub async fn handle_pending(
    current: DeploymentState,
    target_stack: Stack,
    config: DeploymentConfig,
    client_config: alien_core::ClientConfig,
    _service_provider: std::sync::Arc<dyn alien_infra::PlatformServiceProvider>,
) -> Result<DeploymentStepResult> {
    info!("Handling Pending status");

    // Step 1: Initialize stack state. Direct platform deployments may carry a
    // user-selected resource prefix in their initial stack state.
    let stack_state = current
        .stack_state
        .clone()
        .unwrap_or_else(|| StackState::new(current.platform));
    info!(
        "Initialized stack state for platform {:?}",
        current.platform
    );

    // Step 2: Collect environment information. Kubernetes deployments may run
    // on base cloud infrastructure; collect the base cloud environment while
    // keeping the deployment stack platform as Kubernetes.
    let environment_info =
        collect_deployment_environment_info(current.platform, config.base_platform, &client_config)
            .await?;

    // Step 2.5: Record the frozen-gate answers, then drop gated setup
    // resources the deployer declined, BEFORE the mutations: a declined
    // frozen resource never existed, and leaving it in would derive grants
    // and profile entries for it — or make InitialSetup read it as
    // missing-and-pending and create the very resource the deployer declined.
    // The recorded answers are what the update path holds every later input
    // value against: a frozen gate is answered once.
    let persisted_gate_answers =
        resolve_frozen_gate_answers(&target_stack, &stack_state, &config.input_values)?;
    let frozen_gating = frozen_gating_inputs(&target_stack);
    let target_stack =
        strip_declined_frozen_resources(target_stack, &stack_state, &config.input_values)?;
    let target_stack = strip_frozen_dominated_live_resources(
        target_stack,
        &persisted_gate_answers,
        &frozen_gating,
    );

    // Step 3: Run deployment-time preflights (compile-time + mutations + runtime checks)
    // Store the mutated stack for use in subsequent phases (InitialSetup, Provisioning)
    let runner = alien_preflights::runner::PreflightRunner::new();
    let (mutated_stack, _deployment_summary, _) = runner
        .run_deployment_time_preflights(
            target_stack.clone(),
            &stack_state,
            &config,
            &client_config,
            None, // No old stack for initial deployment
            None,
        )
        .await
        .context(ErrorData::PreflightChecksFailed)?;

    info!("Deployment-time preflight checks completed successfully");

    // Step 3.5: Drop gated live resources whose input says no. Frozen
    // declines were stripped before the mutations above; live declines apply
    // here, after them, so a declined workload's provisioning baseline stays
    // derived and acceptance can return later.
    let mutated_stack = strip_declined_live_resources(
        mutated_stack,
        &config.input_values,
        &persisted_gate_answers,
    )?;

    // Step 4: Store prepared stack and inject environment variables
    let mut runtime_metadata = alien_core::RuntimeMetadata::default();
    runtime_metadata.prepared_stack = Some(mutated_stack.clone());
    runtime_metadata.persisted_gate_answers = persisted_gate_answers;

    // Inject environment variables into the prepared stack for validation
    let mut mutated_stack_with_env = mutated_stack;
    crate::helpers::inject_environment_variables(
        &mut mutated_stack_with_env,
        &config,
        current.platform,
    )?;
    if let Some(monitoring) = &config.monitoring {
        crate::helpers::inject_monitoring_environment_variables(
            &mut mutated_stack_with_env,
            monitoring,
            current.platform,
        )?;
    }

    // Note: We don't store the stack with env vars injected, just validate it works
    // Each phase will inject env vars fresh from the prepared stack

    // Step 5: Return update to transition to InitialSetup
    let mut next = current.clone();
    next.status = DeploymentStatus::InitialSetup;
    next.stack_state = Some(stack_state);
    next.error = None;
    next.environment_info = environment_info;
    next.runtime_metadata = Some(runtime_metadata);
    // Error handled in DeploymentStepResult

    Ok(DeploymentStepResult {
        state: next,
        suggested_delay_ms: None,
        update_heartbeat: false,
        heartbeats: vec![],
        observed_inventory_batches: vec![],
    })
}

/// Remove gated setup-created resources the deployer declined. Runs BEFORE
/// the mutations on both deployment paths: a declined frozen resource never
/// existed, so nothing may be derived from it — no service-account grants, no
/// profile entries, no capacity contribution.
///
/// The answer's source is the import when one seeded the state — the template
/// rendered the resource behind its input, so absence IS the answer — and the
/// initial input values on a direct deployment, where no template ever asked.
/// A non-empty state at Pending can only come from a setup import: Pending
/// runs once, before this runner has created anything, and a direct deploy
/// enters it with an empty state.
///
/// An ungated resource missing from an import stays, so real drift still
/// surfaces as a failure.
pub fn strip_declined_frozen_resources(
    stack: Stack,
    stack_state: &StackState,
    input_values: &std::collections::HashMap<String, serde_json::Value>,
) -> Result<Stack> {
    let present = stack_state.resources.keys().cloned().collect();
    strip_declined_frozen_resources_from_presence(stack, &present, input_values)
}

/// [`strip_declined_frozen_resources`] against a raw set of present resource
/// ids — the setup import path resolves declines from the registration
/// payload's ids before any stack state exists, so its strips can run ahead
/// of the mutations like the deployment paths' do. An empty set means no
/// import seeded the answer and the input values decide.
pub fn strip_declined_frozen_resources_from_presence(
    mut stack: Stack,
    present_resource_ids: &std::collections::HashSet<String>,
    input_values: &std::collections::HashMap<String, serde_json::Value>,
) -> Result<Stack> {
    let mut declined: Vec<String> = Vec::new();
    for (resource_id, entry) in stack.resources() {
        let Some(input_id) = entry.enabled_when.as_deref() else {
            continue;
        };
        let setup_created = alien_core::ownership_policy_for_resource_type(
            entry.config.resource_type().as_ref(),
        )
        .should_emit_in_setup(entry.lifecycle);
        if !setup_created {
            continue;
        }

        let is_declined = if present_resource_ids.is_empty() {
            !gate_resolves_true(&stack.inputs, input_id, input_values, resource_id)?
        } else {
            !present_resource_ids.contains(resource_id.as_str())
        };
        if is_declined {
            declined.push(resource_id.clone());
        }
    }
    remove_declined(&mut stack, &declined);
    Ok(stack)
}

/// Remove live resources whose gate input carries a frozen answer of false.
/// A live resource sharing a frozen-gating input follows the fixed answer
/// (frozen dominance): the answer can never flip, so unlike a live-only
/// decline there is no later acceptance to preserve a baseline for — and
/// leaving the resource in until the late strip would dangle its links to
/// the frozen sibling the early strip just removed, failing the reference
/// preflight on a graph the gate rules explicitly permit.
///
/// Dominance applies only to `still_frozen_gating` inputs — computed from
/// the target stack BEFORE the frozen strip removed declined entries. The
/// recorded answer map may carry answers for inputs a later release freed
/// from frozen gating; those follow live resolution (provided value →
/// recorded answer → default) in the late strip instead, so a freed gate is
/// toggleable again.
pub fn strip_frozen_dominated_live_resources(
    mut stack: Stack,
    frozen_answers: &alien_core::GateAnswers,
    still_frozen_gating: &std::collections::HashSet<String>,
) -> Stack {
    let mut declined: Vec<String> = Vec::new();
    for (resource_id, entry) in stack.resources() {
        let Some(input_id) = entry.enabled_when.as_deref() else {
            continue;
        };
        let setup_created = alien_core::ownership_policy_for_resource_type(
            entry.config.resource_type().as_ref(),
        )
        .should_emit_in_setup(entry.lifecycle);
        if setup_created {
            continue;
        }
        if still_frozen_gating.contains(input_id)
            && frozen_answers.get(input_id) == Some(&false)
        {
            declined.push(resource_id.clone());
        }
    }
    remove_declined(&mut stack, &declined);
    stack
}

/// Remove gated live resources whose input resolves false. Runs AFTER the
/// mutations on both deployment paths, at the boundary where the executor's
/// desired set is built: the mutations must keep seeing a declined live
/// resource so its provisioning baseline — service account, profile grants,
/// capacity contribution — stays stable and acceptance can return without a
/// frozen-compatibility violation.
///
/// The answer is the provided value when present, else the input's declared
/// boolean default; anything else is an error, never a silent keep-or-drop.
/// Dropping the resource from the desired stack is what deprovisions it — the
/// executor deletes state resources absent from the desired stack, so a
/// decline removes the resource AND its data by design.
pub fn strip_declined_live_resources(
    mut stack: Stack,
    input_values: &std::collections::HashMap<String, serde_json::Value>,
    persisted_gate_answers: &alien_core::GateAnswers,
) -> Result<Stack> {
    let mut declined: Vec<String> = Vec::new();
    for (resource_id, entry) in stack.resources() {
        let Some(input_id) = entry.enabled_when.as_deref() else {
            continue;
        };
        let setup_created = alien_core::ownership_policy_for_resource_type(
            entry.config.resource_type().as_ref(),
        )
        .should_emit_in_setup(entry.lifecycle);
        if setup_created {
            continue;
        }

        if !live_gate_resolves_true(
            &stack.inputs,
            input_id,
            input_values,
            persisted_gate_answers,
            resource_id,
        )? {
            declined.push(resource_id.clone());
        }
    }
    remove_declined(&mut stack, &declined);
    Ok(stack)
}

/// A live gate's answer: the provided value, else the answer recorded when
/// the deployment was created (frozen dominance — a live resource sharing a
/// frozen-gating input follows the fixed answer, not the declared default),
/// else the declared default.
fn live_gate_resolves_true(
    inputs: &[alien_core::StackInputDefinition],
    input_id: &str,
    input_values: &std::collections::HashMap<String, serde_json::Value>,
    persisted_gate_answers: &alien_core::GateAnswers,
    resource_id: &str,
) -> Result<bool> {
    if !input_values.contains_key(input_id) {
        if let Some(answer) = persisted_gate_answers.get(input_id) {
            return Ok(*answer);
        }
    }
    gate_resolves_true(inputs, input_id, input_values, resource_id)
}

/// The canonical resolved answers for every input that gates a Frozen
/// resource in `stack`, from the same sources the frozen strip reads: the
/// import when one seeded the state (per-resource presence, which must agree
/// across resources sharing one input), else the initial input values.
///
/// Recorded on the deployment at creation; the update path refuses input
/// values that conflict with them for the deployment's lifetime.
pub fn resolve_frozen_gate_answers(
    stack: &Stack,
    stack_state: &StackState,
    input_values: &std::collections::HashMap<String, serde_json::Value>,
) -> Result<alien_core::GateAnswers> {
    let present = stack_state.resources.keys().cloned().collect();
    resolve_frozen_gate_answers_from_presence(stack, &present, input_values)
}

/// [`resolve_frozen_gate_answers`] against a raw set of present resource ids
/// — see [`strip_declined_frozen_resources_from_presence`] for why the setup
/// import path resolves from the registration payload directly.
pub fn resolve_frozen_gate_answers_from_presence(
    stack: &Stack,
    present_resource_ids: &std::collections::HashSet<String>,
    input_values: &std::collections::HashMap<String, serde_json::Value>,
) -> Result<alien_core::GateAnswers> {
    let mut answers = alien_core::GateAnswers::new();
    for (resource_id, entry) in stack.resources() {
        let Some(input_id) = entry.enabled_when.as_deref() else {
            continue;
        };
        let setup_created = alien_core::ownership_policy_for_resource_type(
            entry.config.resource_type().as_ref(),
        )
        .should_emit_in_setup(entry.lifecycle);
        if !setup_created {
            continue;
        }

        let answer = if present_resource_ids.is_empty() {
            gate_resolves_true(&stack.inputs, input_id, input_values, resource_id)?
        } else {
            present_resource_ids.contains(resource_id.as_str())
        };
        if let Some(previous) = answers.insert(input_id.to_string(), answer) {
            if previous != answer {
                return Err(AlienError::new(
                    crate::error::ErrorData::FrozenGateAnswerUnderivable {
                        input_id: input_id.to_string(),
                        reason: format!(
                            "resources sharing this gate disagree — '{resource_id}' resolves \
                             {answer} while a sibling resolved {previous}; the template renders \
                             them behind one input, so a consistent import cannot produce this"
                        ),
                    },
                )
                .into());
            }
        }
    }
    Ok(answers)
}

/// Reconstruct what a deployment from before answers were recorded can
/// actually prove about its frozen gates, from its settled state alone.
///
/// A gated resource present in the settled state was accepted — setup
/// created it, which no other answer explains. Absence proves nothing: the
/// deployer may have declined it, or the release being deployed may have
/// introduced the gate after that state settled. Recording a decline from
/// absence would fabricate history and refuse the gate's first legitimate
/// acceptance ever after, so an unprovable gate gets no recorded answer and
/// stays unconstrained until a setup import answers it for real — which is
/// exactly where these deployments already were.
pub fn derive_legacy_frozen_gate_answers(
    target_stack: &Stack,
    stack_state: &StackState,
) -> alien_core::GateAnswers {
    let mut answers = alien_core::GateAnswers::new();
    for (resource_id, entry) in target_stack.resources() {
        let Some(input_id) = entry.enabled_when.as_deref() else {
            continue;
        };
        let setup_created = alien_core::ownership_policy_for_resource_type(
            entry.config.resource_type().as_ref(),
        )
        .should_emit_in_setup(entry.lifecycle);
        if setup_created && stack_state.resources.contains_key(resource_id.as_str()) {
            answers.insert(input_id.to_string(), true);
        }
    }
    answers
}

/// The inputs that gate a setup-created resource in `stack`. Fixity applies
/// to exactly these: an input whose last frozen resource left the release is
/// no longer frozen-gating, and its recorded answer must not keep refusing
/// live toggles.
pub fn frozen_gating_inputs(stack: &Stack) -> std::collections::HashSet<String> {
    stack
        .resources()
        .filter_map(|(_, entry)| {
            let input_id = entry.enabled_when.as_deref()?;
            alien_core::ownership_policy_for_resource_type(entry.config.resource_type().as_ref())
                .should_emit_in_setup(entry.lifecycle)
                .then(|| input_id.to_string())
        })
        .collect()
}

/// Refuse an update whose input values conflict with a persisted frozen-gate
/// answer, for inputs that still gate a frozen resource in the declared
/// stack. Inputs the update does not mention keep their recorded answer.
pub fn enforce_frozen_gate_fixity(
    persisted: &alien_core::GateAnswers,
    still_frozen_gating: &std::collections::HashSet<String>,
    input_values: &std::collections::HashMap<String, serde_json::Value>,
) -> Result<()> {
    for (input_id, persisted_answer) in persisted {
        if !still_frozen_gating.contains(input_id) {
            continue;
        }
        let Some(value) = input_values.get(input_id) else {
            continue;
        };
        let Some(requested) = gate_value_as_bool(value) else {
            return Err(AlienError::new(ErrorData::MissingConfiguration {
                message: format!(
                    "Input '{input_id}' gates a setup-created resource but its value is not a \
                     boolean: {value}"
                ),
            }));
        };
        if requested != *persisted_answer {
            return Err(AlienError::new(
                crate::error::ErrorData::FrozenGateAnswerChanged {
                    input_id: input_id.clone(),
                    persisted: *persisted_answer,
                    requested,
                },
            )
            .into());
        }
    }
    Ok(())
}

/// Audit the gate-driven transitions this update requests: a live decline
/// that will delete an existing resource (data included) and a live
/// acceptance that will recreate a previously declined one.
///
/// The operation id derives from what makes the transition itself — resource,
/// input, answer, release — so a retried step logs the same id (correlate,
/// don't double-count) while a later flip in another release gets a fresh
/// one. Completion and failure are the executor's per-resource status
/// transitions, correlated by resource id; both flow through the ordinary
/// tracing pipeline and the deployment-state sync.
pub fn audit_live_gate_transitions(
    stack: &Stack,
    stack_state: &StackState,
    input_values: &std::collections::HashMap<String, serde_json::Value>,
    persisted_gate_answers: &alien_core::GateAnswers,
    release_id: Option<&str>,
) {
    for (resource_id, entry) in stack.resources() {
        let Some(input_id) = entry.enabled_when.as_deref() else {
            continue;
        };
        let setup_created = alien_core::ownership_policy_for_resource_type(
            entry.config.resource_type().as_ref(),
        )
        .should_emit_in_setup(entry.lifecycle);
        if setup_created {
            continue;
        }
        // The same resolver the strip uses, or the audit would describe a
        // transition that never happens — and miss the one that does.
        let Ok(accepted) = live_gate_resolves_true(
            &stack.inputs,
            input_id,
            input_values,
            persisted_gate_answers,
            resource_id,
        ) else {
            // The strip right after this reports the unresolvable gate as the
            // step's error; nothing to audit for a step that will not run.
            continue;
        };
        let exists = stack_state.resources.contains_key(resource_id.as_str());
        let source = source_of(input_values, persisted_gate_answers, input_id);
        let (transition, value_source) = match (accepted, exists) {
            (false, true) => ("delete", source),
            (true, false) => ("create", source),
            _ => continue,
        };
        let release = release_id.unwrap_or("unversioned");
        let operation_id = format!("gate:{resource_id}:{input_id}:{accepted}:{release}");
        info!(
            audit = "live-gate",
            phase = "requested",
            operation_id = %operation_id,
            resource_id = %resource_id,
            input_id = %input_id,
            resolved_value = accepted,
            value_source = value_source,
            lifecycle = "live",
            transition = transition,
            "A live gate transition was requested; the executor's status \
             transitions for this resource complete or fail it"
        );
    }
}

fn source_of(
    input_values: &std::collections::HashMap<String, serde_json::Value>,
    persisted_gate_answers: &alien_core::GateAnswers,
    input_id: &str,
) -> &'static str {
    if input_values.contains_key(input_id) {
        "provided"
    } else if persisted_gate_answers.contains_key(input_id) {
        "persisted"
    } else {
        "default"
    }
}

fn remove_declined(stack: &mut Stack, declined: &[String]) {
    for resource_id in declined {
        info!(
            resource_id = %resource_id,
            "The deployer declined this gated resource; it leaves the desired stack"
        );
        stack.resources.shift_remove(resource_id);
    }
}

/// A gate value from the wire: JSON booleans stay booleans, and the
/// CloudFormation parameter strings "true"/"false" coerce — CloudFormation
/// has no boolean parameter type, so its registration payloads deliver gate
/// answers as strings. Anything else is `None`, refused loudly by callers.
fn gate_value_as_bool(value: &serde_json::Value) -> Option<bool> {
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

/// The deployer's answer for a live gate: the provided value, else the
/// input's declared boolean default.
fn gate_resolves_true(
    inputs: &[alien_core::StackInputDefinition],
    input_id: &str,
    input_values: &std::collections::HashMap<String, serde_json::Value>,
    resource_id: &str,
) -> Result<bool> {
    if let Some(value) = input_values.get(input_id) {
        return gate_value_as_bool(value).ok_or_else(|| {
            AlienError::new(ErrorData::MissingConfiguration {
                message: format!(
                    "Input '{input_id}' enables resource '{resource_id}' but its value is not \
                     a boolean: {value}"
                ),
            })
        });
    }

    match inputs
        .iter()
        .find(|input| input.id == input_id)
        .and_then(|input| input.default.as_ref())
    {
        Some(alien_core::StackInputDefaultValue::Boolean(answer)) => Ok(*answer),
        _ => Err(AlienError::new(ErrorData::MissingConfiguration {
            message: format!(
                "Input '{input_id}' enables resource '{resource_id}' but no value was provided \
                 and the input declares no boolean default"
            ),
        })),
    }
}

fn should_collect_environment_info(platform: Platform) -> bool {
    !matches!(platform, Platform::Machines)
}

async fn collect_deployment_environment_info(
    platform: Platform,
    base_platform: Option<Platform>,
    client_config: &ClientConfig,
) -> Result<Option<EnvironmentInfo>> {
    if !should_collect_environment_info(platform) {
        return Ok(None);
    }

    let (environment_platform, environment_client_config) =
        environment_collection_context(platform, base_platform, client_config)?;
    let environment_info =
        crate::helpers::collect_environment_info(environment_platform, &environment_client_config)
            .await
            .context(ErrorData::EnvironmentInfoCollectionFailed {
                platform: format!("{:?}", environment_platform),
                reason: "Failed to collect cloud environment details".to_string(),
            })?;

    info!(
        "Collected environment info for platform {:?}",
        environment_platform
    );

    Ok(Some(environment_info))
}

fn environment_collection_context(
    platform: Platform,
    base_platform: Option<Platform>,
    client_config: &ClientConfig,
) -> Result<(Platform, ClientConfig)> {
    let environment_platform = base_platform.unwrap_or(platform);
    let environment_client_config = client_config
        .config_for_platform(environment_platform)
        .ok_or_else(|| {
            AlienError::new(ErrorData::MissingConfiguration {
                message: format!(
                    "Client config for environment platform '{}' is missing",
                    environment_platform
                ),
            })
        })?;
    Ok((environment_platform, environment_client_config))
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{
        Kv, KubernetesClientConfig, Resource, ResourceLifecycle, ResourceStatus, ServiceAccount,
        StackInputDefinition, StackResourceState,
    };

    fn imported_state_with(resource_id: &str, resource: Resource) -> StackState {
        let mut entry = StackResourceState::new_pending(
            resource.resource_type().as_ref().to_string(),
            resource,
            Some(ResourceLifecycle::Frozen),
            Vec::new(),
        );
        entry.status = ResourceStatus::Running;
        let mut state = StackState::new(Platform::Aws);
        state.resources.insert(resource_id.to_string(), entry);
        state
    }

    fn gated_stack() -> Stack {
        gated_stack_with_default(Some(true))
    }

    fn gated_stack_with_default(default: Option<bool>) -> Stack {
        let input = StackInputDefinition::deployer_boolean(
            "analyticsEnabled",
            "Enable analytics",
            "Whether to create the analytics store.",
            default,
        );
        Stack::new("gated-stack".to_string())
            .inputs(vec![input])
            .add(
                ServiceAccount::new("execution-sa".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .add_enabled_when(
                Kv::new("analytics".to_string()).build(),
                ResourceLifecycle::Frozen,
                "analyticsEnabled",
            )
            .build()
    }

    fn live_gated_stack(default: Option<bool>) -> Stack {
        let input = StackInputDefinition::deployer_boolean(
            "cacheEnabled",
            "Enable the cache",
            "Whether to run the cache store.",
            default,
        );
        Stack::new("gated-stack".to_string())
            .inputs(vec![input])
            .add_enabled_when(
                Kv::new("cache".to_string()).build(),
                ResourceLifecycle::Live,
                "cacheEnabled",
            )
            .build()
    }

    /// The import delivered the service account but not the gated store: the
    /// deployer declined it, so the runner must not try to create it.
    #[test]
    fn a_gated_resource_absent_from_an_import_is_stripped() {
        let state = imported_state_with(
            "execution-sa",
            Resource::new(ServiceAccount::new("execution-sa".to_string()).build()),
        );

        let stripped =
            strip_declined_frozen_resources(gated_stack(), &state, &Default::default())
                .expect("an imported answer resolves without error");

        assert!(!stripped.resources.contains_key("analytics"));
        assert!(stripped.resources.contains_key("execution-sa"));
    }

    /// An empty state means this runner creates the frozen resources itself
    /// (a direct deploy), so no template ever asked the deployer: the initial
    /// input values answer instead — provided value, else the declared
    /// default, and never a guess.
    #[test]
    fn a_direct_deploy_frozen_gate_follows_the_input() {
        let kept = strip_declined_frozen_resources(
            gated_stack_with_default(Some(true)),
            &StackState::new(Platform::Aws),
            &Default::default(),
        )
        .expect("default resolves");
        assert!(kept.resources.contains_key("analytics"));

        let dropped = strip_declined_frozen_resources(
            gated_stack_with_default(Some(true)),
            &StackState::new(Platform::Aws),
            &std::collections::HashMap::from([(
                "analyticsEnabled".to_string(),
                serde_json::json!(false),
            )]),
        )
        .expect("provided answer resolves");
        assert!(!dropped.resources.contains_key("analytics"));

        let error = strip_declined_frozen_resources(
            gated_stack_with_default(None),
            &StackState::new(Platform::Aws),
            &Default::default(),
        )
        .expect_err("no value and no default cannot resolve");
        assert!(error.message.contains("analyticsEnabled"), "{}", error.message);
    }

    /// A gated resource the import delivered was accepted; it stays.
    #[test]
    fn an_imported_gated_resource_stays() {
        let state = imported_state_with(
            "analytics",
            Resource::new(Kv::new("analytics".to_string()).build()),
        );

        let stripped =
            strip_declined_frozen_resources(gated_stack(), &state, &Default::default())
                .expect("an imported answer resolves without error");

        assert!(stripped.resources.contains_key("analytics"));
    }

    /// An ungated resource missing from an import is drift, not an answer;
    /// leaving it in keeps the failure visible.
    #[test]
    fn an_ungated_missing_resource_is_not_papered_over() {
        let state = imported_state_with(
            "analytics",
            Resource::new(Kv::new("analytics".to_string()).build()),
        );

        let stripped =
            strip_declined_frozen_resources(gated_stack(), &state, &Default::default())
                .expect("an imported answer resolves without error");

        assert!(stripped.resources.contains_key("execution-sa"));
    }

    /// The deployer said no: the live resource leaves the desired stack, and
    /// because deprovisioning is state-vs-desired reconciliation, it leaves
    /// whether or not the resource already exists.
    #[test]
    fn a_live_gate_answered_false_drops_the_resource() {
        let stripped = strip_declined_live_resources(
            live_gated_stack(Some(true)),
            &std::collections::HashMap::from([("cacheEnabled".to_string(), serde_json::json!(false))]),
            &Default::default(),
        )
        .expect("resolvable gate");
        assert!(!stripped.resources.contains_key("cache"));
    }

    #[test]
    fn a_live_gate_answered_true_keeps_the_resource() {
        let stripped = strip_declined_live_resources(
            live_gated_stack(Some(false)),
            &std::collections::HashMap::from([("cacheEnabled".to_string(), serde_json::json!(true))]),
            &Default::default(),
        )
        .expect("resolvable gate");
        assert!(stripped.resources.contains_key("cache"));
    }

    /// No answer given (a direct deploy): the declared default decides.
    #[test]
    fn an_unanswered_live_gate_follows_its_default() {
        let kept = strip_declined_live_resources(
            live_gated_stack(Some(true)),
            &Default::default(),
            &Default::default(),
        )
        .expect("default resolves");
        assert!(kept.resources.contains_key("cache"));

        let dropped = strip_declined_live_resources(
            live_gated_stack(Some(false)),
            &Default::default(),
            &Default::default(),
        )
        .expect("default resolves");
        assert!(!dropped.resources.contains_key("cache"));
    }

    /// An unresolvable gate is a fault, never a silent keep-or-drop.
    #[test]
    fn an_unresolvable_live_gate_fails_fast() {
        let error = strip_declined_live_resources(
            live_gated_stack(None),
            &Default::default(),
            &Default::default(),
        )
        .expect_err("no value and no default cannot resolve");
        assert!(error.message.contains("cacheEnabled"), "{}", error.message);
    }

    /// Answers derive from import presence when a state exists, from the
    /// initial input values on a direct deploy, and refuse to guess when
    /// resources sharing one gate disagree.
    #[test]
    fn frozen_gate_answers_resolve_from_their_provenance() {
        let imported = imported_state_with(
            "analytics",
            Resource::new(Kv::new("analytics".to_string()).build()),
        );
        let answers =
            resolve_frozen_gate_answers(&gated_stack(), &imported, &Default::default())
                .expect("presence resolves");
        assert_eq!(answers.get("analyticsEnabled"), Some(&true));

        let declined = imported_state_with(
            "execution-sa",
            Resource::new(ServiceAccount::new("execution-sa".to_string()).build()),
        );
        let answers =
            resolve_frozen_gate_answers(&gated_stack(), &declined, &Default::default())
                .expect("absence resolves");
        assert_eq!(answers.get("analyticsEnabled"), Some(&false));

        let direct = resolve_frozen_gate_answers(
            &gated_stack_with_default(Some(false)),
            &StackState::new(Platform::Aws),
            &Default::default(),
        )
        .expect("the declared default resolves");
        assert_eq!(direct.get("analyticsEnabled"), Some(&false));
    }

    #[test]
    fn a_shared_gate_with_disagreeing_imports_is_refused() {
        let mut stack = gated_stack();
        stack.resources.insert(
            "metrics".to_string(),
            alien_core::ResourceEntry {
                config: Resource::new(Kv::new("metrics".to_string()).build()),
                lifecycle: ResourceLifecycle::Frozen,
                dependencies: Vec::new(),
                remote_access: false,
                enabled_when: Some("analyticsEnabled".to_string()),
            },
        );
        // The import delivered one of the two resources behind the gate.
        let state = imported_state_with(
            "analytics",
            Resource::new(Kv::new("analytics".to_string()).build()),
        );

        let error = resolve_frozen_gate_answers(&stack, &state, &Default::default())
            .expect_err("a half-delivered shared gate cannot resolve");
        assert_eq!(error.code, "FROZEN_GATE_ANSWER_UNDERIVABLE");
    }

    /// A persisted answer wins over any later input value; inputs the update
    /// does not mention keep their recorded answer silently.
    #[test]
    fn fixity_refuses_conflicting_answers_only() {
        let persisted = alien_core::GateAnswers::from_iter([("analyticsEnabled".to_string(), false)]);
        let still_frozen =
            std::collections::HashSet::from(["analyticsEnabled".to_string()]);

        enforce_frozen_gate_fixity(&persisted, &still_frozen, &Default::default())
            .expect("an unmentioned input keeps its answer");
        enforce_frozen_gate_fixity(
            &persisted,
            &still_frozen,
            &std::collections::HashMap::from([(
                "analyticsEnabled".to_string(),
                serde_json::json!(false),
            )]),
        )
        .expect("a matching answer passes");

        let error = enforce_frozen_gate_fixity(
            &persisted,
            &still_frozen,
            &std::collections::HashMap::from([(
                "analyticsEnabled".to_string(),
                serde_json::json!(true),
            )]),
        )
        .expect_err("a flipped answer is refused");
        assert_eq!(error.code, "FROZEN_GATE_ANSWER_CHANGED");
    }

    /// A release that removes the last frozen resource behind an input frees
    /// the input: the recorded answer stops binding, so a live toggle on it
    /// is a normal update again.
    #[test]
    fn fixity_releases_inputs_no_longer_frozen_gating() {
        let persisted = alien_core::GateAnswers::from_iter([("analyticsEnabled".to_string(), false)]);

        enforce_frozen_gate_fixity(
            &persisted,
            &Default::default(),
            &std::collections::HashMap::from([(
                "analyticsEnabled".to_string(),
                serde_json::json!(true),
            )]),
        )
        .expect("an input that no longer gates a frozen resource is free to change");

        let live_only = Stack::new("s".to_string())
            .inputs(vec![StackInputDefinition::deployer_boolean(
                "analyticsEnabled",
                "Enable analytics",
                "Whether to run analytics.",
                Some(true),
            )])
            .add_enabled_when(
                Kv::new("analytics".to_string()).build(),
                ResourceLifecycle::Live,
                "analyticsEnabled",
            )
            .build();
        assert!(
            frozen_gating_inputs(&live_only).is_empty(),
            "a live-gated input is not frozen-gating"
        );
    }

    /// Frozen dominance on an omitted shared input: the live strip resolves
    /// the recorded answer before the declared default, so an update that
    /// omits the input cannot deprovision the accepted live resource.
    #[test]
    fn an_omitted_live_gate_follows_the_persisted_answer_over_the_default() {
        let persisted = alien_core::GateAnswers::from_iter([("cacheEnabled".to_string(), true)]);

        let kept = strip_declined_live_resources(
            live_gated_stack(Some(false)),
            &Default::default(),
            &persisted,
        )
        .expect("the recorded answer resolves");
        assert!(
            kept.resources.contains_key("cache"),
            "the recorded true answer outranks the declared false default"
        );

        let provided_wins = strip_declined_live_resources(
            live_gated_stack(Some(false)),
            &std::collections::HashMap::from([("cacheEnabled".to_string(), serde_json::json!(false))]),
            &persisted,
        )
        .expect("a provided value resolves");
        assert!(
            !provided_wins.resources.contains_key("cache"),
            "a provided value still outranks the recorded answer for live-only inputs"
        );
    }

    /// CloudFormation has no boolean parameter type, so its registration
    /// payloads deliver gate answers as the strings "true"/"false" — those
    /// coerce. Anything else is corrupt input and must fail loudly.
    #[test]
    fn a_string_boolean_gate_value_coerces_and_anything_else_fails_fast() {
        let dropped = strip_declined_live_resources(
            live_gated_stack(Some(true)),
            &std::collections::HashMap::from([("cacheEnabled".to_string(), serde_json::json!("false"))]),
            &Default::default(),
        )
        .expect("a CloudFormation string boolean resolves");
        assert!(
            !dropped.resources.contains_key("cache"),
            "the string \"false\" declines like the boolean"
        );

        let error = strip_declined_live_resources(
            live_gated_stack(Some(true)),
            &std::collections::HashMap::from([("cacheEnabled".to_string(), serde_json::json!("yes"))]),
            &Default::default(),
        )
        .expect_err("an arbitrary string is not an answer");
        assert!(error.message.contains("boolean"), "{}", error.message);
    }

    /// The pause-consumer shape: a live consumer sharing its frozen
    /// dependency's gate. A decline must remove BOTH before the preflights —
    /// the frozen strip alone would leave the consumer's link dangling and
    /// fail the reference check on a graph the gate rules explicitly permit.
    /// The answer can never flip (frozen dominance), so no baseline needs
    /// preserving for a later acceptance.
    #[test]
    fn a_declined_shared_gate_strips_the_dominated_live_resource() {
        let shared_gate_stack = |value: bool| {
            let input = StackInputDefinition::deployer_boolean(
                "extrasEnabled",
                "Enable extras",
                "Whether to run the extras store and its consumer.",
                Some(value),
            );
            Stack::new("gated-stack".to_string())
                .inputs(vec![input])
                .add_enabled_when(
                    Kv::new("extras".to_string()).build(),
                    ResourceLifecycle::Frozen,
                    "extrasEnabled",
                )
                .add_enabled_when(
                    Kv::new("extras-cache".to_string()).build(),
                    ResourceLifecycle::Live,
                    "extrasEnabled",
                )
                .build()
        };

        let declined = shared_gate_stack(false);
        let answers =
            resolve_frozen_gate_answers(&declined, &StackState::new(Platform::Aws), &Default::default())
                .expect("the declared default resolves");
        assert_eq!(answers.get("extrasEnabled"), Some(&false));
        let frozen_gating = frozen_gating_inputs(&declined);
        let stripped =
            strip_declined_frozen_resources(declined, &StackState::new(Platform::Aws), &Default::default())
                .expect("the frozen strip resolves");
        let stripped = strip_frozen_dominated_live_resources(stripped, &answers, &frozen_gating);
        assert!(!stripped.resources.contains_key("extras"));
        assert!(
            !stripped.resources.contains_key("extras-cache"),
            "the dominated live consumer leaves with its frozen dependency"
        );

        let accepted = shared_gate_stack(true);
        let answers =
            resolve_frozen_gate_answers(&accepted, &StackState::new(Platform::Aws), &Default::default())
                .expect("the declared default resolves");
        let frozen_gating = frozen_gating_inputs(&accepted);
        let kept =
            strip_declined_frozen_resources(accepted, &StackState::new(Platform::Aws), &Default::default())
                .expect("the frozen strip resolves");
        let kept = strip_frozen_dominated_live_resources(kept, &answers, &frozen_gating);
        assert!(kept.resources.contains_key("extras"));
        assert!(kept.resources.contains_key("extras-cache"));
    }

    /// A later release freed the input: its last frozen resource is gone and
    /// only the live resource remains. Dominance no longer applies — the
    /// stale recorded false must not strip the workload here; the live strip
    /// resolves it (provided value → recorded answer → default) so the gate
    /// is toggleable again.
    #[test]
    fn a_freed_gate_is_not_dominated_by_its_stale_frozen_answer() {
        let stale_answers =
            alien_core::GateAnswers::from_iter([("cacheEnabled".to_string(), false)]);
        let freed_target = live_gated_stack(Some(false));
        let frozen_gating = frozen_gating_inputs(&freed_target);
        assert!(frozen_gating.is_empty(), "nothing frozen gates this input anymore");

        let kept = strip_frozen_dominated_live_resources(
            freed_target,
            &stale_answers,
            &frozen_gating,
        );
        assert!(
            kept.resources.contains_key("cache"),
            "a freed gate follows live resolution, not the stale frozen answer"
        );

        let accepted = strip_declined_live_resources(
            kept,
            &std::collections::HashMap::from([("cacheEnabled".to_string(), serde_json::json!(true))]),
            &stale_answers,
        )
        .expect("a provided value resolves");
        assert!(
            accepted.resources.contains_key("cache"),
            "an explicit true re-enables the workload once the gate is freed"
        );
    }

    /// A legacy deployment's baseline reads the settled state against the
    /// release it settled under: a gate the target release introduces has no
    /// history to fabricate, while a gate the settled release declared still
    /// derives from presence and keeps refusing flips.
    #[test]
    fn a_legacy_gate_absent_from_the_settled_state_records_no_answer() {
        // The gated store is absent: either the deployer declined it, or the
        // release being deployed introduced the gate after this state
        // settled. Both look identical here, so neither is recorded.
        let without_the_store = imported_state_with(
            "execution-sa",
            Resource::new(ServiceAccount::new("execution-sa".to_string()).build()),
        );
        let unprovable = derive_legacy_frozen_gate_answers(&gated_stack(), &without_the_store);
        assert!(
            unprovable.is_empty(),
            "absence proves nothing, so no answer may be fabricated from it"
        );

        // The gated store exists: setup created it, which no answer other
        // than yes explains.
        let with_the_store = imported_state_with(
            "analytics",
            Resource::new(Kv::new("analytics".to_string()).build()),
        );
        let proven = derive_legacy_frozen_gate_answers(&gated_stack(), &with_the_store);
        assert_eq!(
            proven.get("analyticsEnabled"),
            Some(&true),
            "a gated resource that exists was accepted"
        );
    }

    /// A live gate has no frozen history to reconstruct — only setup-created
    /// resources leave the presence evidence this derivation reads.
    #[test]
    fn a_legacy_derivation_ignores_live_gates() {
        let state = imported_state_with(
            "cache",
            Resource::new(Kv::new("cache".to_string()).build()),
        );

        let answers = derive_legacy_frozen_gate_answers(&live_gated_stack(Some(true)), &state);
        assert!(
            answers.is_empty(),
            "a live gate re-resolves every cycle; it has no answered-once history"
        );
    }

    #[test]
    fn kubernetes_base_platform_collects_base_environment() {
        let client_config = ClientConfig::KubernetesCloud {
            kubernetes: Box::new(KubernetesClientConfig::InCluster {
                namespace: Some("alien-test".to_string()),
                additional_headers: None,
            }),
            cloud: Box::new(ClientConfig::Test),
        };

        let (platform, config) = environment_collection_context(
            Platform::Kubernetes,
            Some(Platform::Test),
            &client_config,
        )
        .expect("base platform client config should be selected");

        assert_eq!(platform, Platform::Test);
        assert!(matches!(config, ClientConfig::Test));
    }

    #[tokio::test]
    async fn machines_skips_environment_collection() {
        let environment_info =
            collect_deployment_environment_info(Platform::Machines, None, &ClientConfig::Test)
                .await
                .expect("machines should not require a cloud client config");

        assert!(environment_info.is_none());
    }
}
