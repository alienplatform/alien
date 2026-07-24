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

    // Step 2.5: Drop gated setup resources the deployer declined, BEFORE the
    // mutations: a declined frozen resource never existed, and leaving it in
    // would derive grants and profile entries for it — or make InitialSetup
    // read it as missing-and-pending and create the very resource the
    // deployer declined.
    let target_stack =
        strip_declined_frozen_resources(target_stack, &stack_state, &config.input_values)?;

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
    let mutated_stack = strip_declined_live_resources(mutated_stack, &config.input_values)?;

    // Step 4: Store prepared stack and inject environment variables
    let mut runtime_metadata = alien_core::RuntimeMetadata::default();
    runtime_metadata.prepared_stack = Some(mutated_stack.clone());

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
    mut stack: Stack,
    stack_state: &StackState,
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

        let is_declined = if stack_state.resources.is_empty() {
            !gate_resolves_true(&stack.inputs, input_id, input_values, resource_id)?
        } else {
            !stack_state.resources.contains_key(resource_id.as_str())
        };
        if is_declined {
            declined.push(resource_id.clone());
        }
    }
    remove_declined(&mut stack, &declined);
    Ok(stack)
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

        if !gate_resolves_true(&stack.inputs, input_id, input_values, resource_id)? {
            declined.push(resource_id.clone());
        }
    }
    remove_declined(&mut stack, &declined);
    Ok(stack)
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

/// The deployer's answer for a live gate: the provided value, else the
/// input's declared boolean default.
fn gate_resolves_true(
    inputs: &[alien_core::StackInputDefinition],
    input_id: &str,
    input_values: &std::collections::HashMap<String, serde_json::Value>,
    resource_id: &str,
) -> Result<bool> {
    if let Some(value) = input_values.get(input_id) {
        return value.as_bool().ok_or_else(|| {
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
        )
        .expect("resolvable gate");
        assert!(!stripped.resources.contains_key("cache"));
    }

    #[test]
    fn a_live_gate_answered_true_keeps_the_resource() {
        let stripped = strip_declined_live_resources(
            live_gated_stack(Some(false)),
            &std::collections::HashMap::from([("cacheEnabled".to_string(), serde_json::json!(true))]),
        )
        .expect("resolvable gate");
        assert!(stripped.resources.contains_key("cache"));
    }

    /// No answer given (a direct deploy): the declared default decides.
    #[test]
    fn an_unanswered_live_gate_follows_its_default() {
        let kept = strip_declined_live_resources(live_gated_stack(Some(true)), &Default::default())
            .expect("default resolves");
        assert!(kept.resources.contains_key("cache"));

        let dropped =
            strip_declined_live_resources(live_gated_stack(Some(false)), &Default::default())
                .expect("default resolves");
        assert!(!dropped.resources.contains_key("cache"));
    }

    /// An unresolvable gate is a fault, never a silent keep-or-drop.
    #[test]
    fn an_unresolvable_live_gate_fails_fast() {
        let error = strip_declined_live_resources(live_gated_stack(None), &Default::default())
            .expect_err("no value and no default cannot resolve");
        assert!(error.message.contains("cacheEnabled"), "{}", error.message);
    }

    /// Input values are coerced to their declared kinds before they reach
    /// this layer; a non-boolean here is corrupt input and must fail loudly.
    #[test]
    fn a_non_boolean_gate_value_fails_fast() {
        let error = strip_declined_live_resources(
            live_gated_stack(Some(true)),
            &std::collections::HashMap::from([("cacheEnabled".to_string(), serde_json::json!("false"))]),
        )
        .expect_err("string values are not answers");
        assert!(error.message.contains("boolean"), "{}", error.message);
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
