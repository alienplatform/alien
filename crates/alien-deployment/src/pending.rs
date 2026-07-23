use crate::{
    DeploymentConfig, DeploymentState, DeploymentStatus, DeploymentStepResult, ErrorData, Result,
};
use alien_core::{
    ClientConfig, EnvironmentInfo, ManagementPermissions, Platform, Stack, StackState,
};
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

    // Step 3.5: Drop gated setup resources the import did not deliver.
    //
    // A gated resource renders behind its input in the setup template, so for
    // a deployment whose frozen resources arrived through a setup import, its
    // absence from the imported state IS the deployer's answer. Leaving the
    // entry in the prepared stack would make InitialSetup read it as
    // missing-and-pending and create the very resource the deployer declined.
    // A non-empty state at Pending can only come from a setup import: Pending
    // runs once, before this runner has created anything, and a direct deploy
    // enters it with an empty state.
    let mutated_stack =
        strip_declined_resources(mutated_stack, &stack_state, &config.input_values)?;

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

/// Remove gated resources the deployer declined.
///
/// Two rules, one per lifecycle family:
/// - a gated setup-created resource is declined when a setup import seeded
///   the state and the resource is absent from it — the template rendered it
///   behind the input, so absence IS the answer;
/// - a gated live resource is declined when its input resolves false: the
///   provided value when present, else the input's declared boolean default.
///   Dropping it from the desired stack is what deprovisions it — the
///   executor deletes state resources absent from the desired stack, so a
///   toggle-off removes the resource AND its data by design.
///
/// An ungated resource missing from an import stays, so real drift still
/// surfaces as a failure; an unresolvable live gate is an error, never a
/// silent keep-or-drop.
pub fn strip_declined_resources(
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

        let is_declined = if setup_created {
            !stack_state.resources.is_empty()
                && !stack_state.resources.contains_key(resource_id.as_str())
        } else {
            !gate_resolves_true(&stack.inputs, input_id, input_values, resource_id)?
        };
        if is_declined {
            declined.push(resource_id.clone());
        }
    }

    for resource_id in &declined {
        info!(
            resource_id = %resource_id,
            "The deployer declined this gated resource; it leaves the desired stack"
        );
        stack.resources.shift_remove(resource_id);
        // A declined resource keeps no grant. Its resource-scoped entry must also
        // leave every permission profile, or a runtime consumer that derives
        // grants straight from the profile (the GCP service-account controller
        // applies resource grants as project-level bindings, since Vertex can't
        // scope IAM to a sub-resource) would re-grant it — the runtime twin of
        // the setup emitter's enabled_when gate. Removing the exact key suffices
        // because `ResourceEnabledValidCheck` (alien-preflights) already rejects a
        // `*`-scoped or sibling-namespace grant that could still cover a gated
        // resource; the "*" wildcard here is not resource-scoped, so it is left
        // alone.
        for profile in stack.permissions.profiles.values_mut() {
            profile.0.shift_remove(resource_id.as_str());
        }
        // The management profile is a parallel resource-scoped grant store: an
        // Extend/Override entry for a declined resource must go too, or the
        // management role keeps a namespace grant the resource no longer backs.
        // Auto needs no strip — the management mutation re-derives it from the
        // stripped resource set.
        match &mut stack.permissions.management {
            ManagementPermissions::Extend(profile) | ManagementPermissions::Override(profile) => {
                profile.0.shift_remove(resource_id.as_str());
            }
            ManagementPermissions::Auto => {}
        }
    }

    Ok(stack)
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
        Kv, KubernetesClientConfig, ManagementPermissions, PermissionProfile, Resource,
        ResourceLifecycle, ResourceStatus, ServiceAccount, StackInputDefinition, StackResourceState,
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
        Stack::new("gated-stack".to_string())
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

        let stripped = strip_declined_resources(gated_stack(), &state, &Default::default())
            .expect("frozen rules never error");

        assert!(!stripped.resources.contains_key("analytics"));
        assert!(stripped.resources.contains_key("execution-sa"));
    }

    /// A declined gated resource loses its resource-scoped grant from every
    /// permission profile (see `strip_declined_resources` for why). The removal
    /// is exact-key, so a kept resource's own grant and the `"*"` wildcard both
    /// survive — the strip must not sweep a sibling.
    #[test]
    fn a_declined_resource_loses_its_permission_profile_grant() {
        let stack = Stack::new("gated-stack".to_string())
            .add(
                ServiceAccount::new("execution-sa".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                Kv::new("events".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .add_enabled_when(
                Kv::new("analytics".to_string()).build(),
                ResourceLifecycle::Frozen,
                "analyticsEnabled",
            )
            .permission(
                "execution",
                PermissionProfile::new()
                    .resource("analytics", ["kv/write"])
                    .resource("events", ["kv/read"])
                    .resource("*", ["worker/invoke"]),
            )
            .build();

        // The import delivered the service account and the ungated `events`
        // store but not the gated `analytics` store, so `analytics` is the
        // deployer's declined resource.
        let mut state = imported_state_with(
            "execution-sa",
            Resource::new(ServiceAccount::new("execution-sa".to_string()).build()),
        );
        let mut events = StackResourceState::new_pending(
            "kv".to_string(),
            Resource::new(Kv::new("events".to_string()).build()),
            Some(ResourceLifecycle::Frozen),
            Vec::new(),
        );
        events.status = ResourceStatus::Running;
        state.resources.insert("events".to_string(), events);

        let stripped = strip_declined_resources(stack, &state, &Default::default())
            .expect("frozen rules never error");

        assert!(
            !stripped.resources.contains_key("analytics"),
            "the declined resource leaves the desired stack"
        );
        let profile = stripped
            .permissions
            .profiles
            .get("execution")
            .expect("the profile itself survives");
        assert!(
            !profile.0.contains_key("analytics"),
            "the declined resource's grant leaves the profile so no runtime consumer re-applies it"
        );
        assert!(
            profile.0.contains_key("events"),
            "a kept resource's own grant survives: the strip is exact-key, not a prefix sweep"
        );
        assert!(
            profile.0.contains_key("*"),
            "the wildcard grant is not resource-scoped and is untouched"
        );
    }

    /// A declined resource must also lose its resource-scoped grant from an
    /// Extend/Override management profile, not only the named profiles — that
    /// profile is a second store the management role reads from.
    #[test]
    fn a_declined_resource_loses_its_management_grant() {
        let mut stack = Stack::new("gated-stack".to_string())
            .add(
                ServiceAccount::new("execution-sa".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                Kv::new("events".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .add_enabled_when(
                Kv::new("analytics".to_string()).build(),
                ResourceLifecycle::Frozen,
                "analyticsEnabled",
            )
            .build();
        stack.permissions.management = ManagementPermissions::Extend(
            PermissionProfile::new()
                .resource("analytics", ["kv/management"])
                .resource("events", ["kv/management"]),
        );

        // The import delivered the service account but not the gated `analytics`
        // store, so `analytics` is the deployer's declined resource.
        let state = imported_state_with(
            "execution-sa",
            Resource::new(ServiceAccount::new("execution-sa".to_string()).build()),
        );

        let stripped = strip_declined_resources(stack, &state, &Default::default())
            .expect("frozen rules never error");

        let management = match &stripped.permissions.management {
            ManagementPermissions::Extend(profile) | ManagementPermissions::Override(profile) => {
                profile
            }
            ManagementPermissions::Auto => panic!("expected an Extend management profile"),
        };
        assert!(
            !management.0.contains_key("analytics"),
            "the declined resource's management grant is withdrawn"
        );
        assert!(
            management.0.contains_key("events"),
            "a kept resource's management grant survives"
        );
    }

    /// An empty state means this runner creates the frozen resources itself
    /// (a direct deploy), so absence carries no answer and nothing is dropped.
    #[test]
    fn nothing_is_stripped_before_anything_was_imported() {
        let stripped = strip_declined_resources(
            gated_stack(),
            &StackState::new(Platform::Aws),
            &Default::default(),
        )
        .expect("frozen rules never error");

        assert!(stripped.resources.contains_key("analytics"));
    }

    /// A gated resource the import delivered was accepted; it stays.
    #[test]
    fn an_imported_gated_resource_stays() {
        let state = imported_state_with(
            "analytics",
            Resource::new(Kv::new("analytics".to_string()).build()),
        );

        let stripped = strip_declined_resources(gated_stack(), &state, &Default::default())
            .expect("frozen rules never error");

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

        let stripped = strip_declined_resources(gated_stack(), &state, &Default::default())
            .expect("frozen rules never error");

        assert!(stripped.resources.contains_key("execution-sa"));
    }

    /// The deployer said no: the live resource leaves the desired stack, and
    /// because deprovisioning is state-vs-desired reconciliation, it leaves
    /// whether or not the resource already exists.
    #[test]
    fn a_live_gate_answered_false_drops_the_resource() {
        let stripped = strip_declined_resources(
            live_gated_stack(Some(true)),
            &StackState::new(Platform::Aws),
            &std::collections::HashMap::from([("cacheEnabled".to_string(), serde_json::json!(false))]),
        )
        .expect("resolvable gate");
        assert!(!stripped.resources.contains_key("cache"));
    }

    #[test]
    fn a_live_gate_answered_true_keeps_the_resource() {
        let stripped = strip_declined_resources(
            live_gated_stack(Some(false)),
            &StackState::new(Platform::Aws),
            &std::collections::HashMap::from([("cacheEnabled".to_string(), serde_json::json!(true))]),
        )
        .expect("resolvable gate");
        assert!(stripped.resources.contains_key("cache"));
    }

    /// No answer given (a direct deploy): the declared default decides.
    #[test]
    fn an_unanswered_live_gate_follows_its_default() {
        let kept = strip_declined_resources(
            live_gated_stack(Some(true)),
            &StackState::new(Platform::Aws),
            &Default::default(),
        )
        .expect("default resolves");
        assert!(kept.resources.contains_key("cache"));

        let dropped = strip_declined_resources(
            live_gated_stack(Some(false)),
            &StackState::new(Platform::Aws),
            &Default::default(),
        )
        .expect("default resolves");
        assert!(!dropped.resources.contains_key("cache"));
    }

    /// An unresolvable gate is a fault, never a silent keep-or-drop.
    #[test]
    fn an_unresolvable_live_gate_fails_fast() {
        let error = strip_declined_resources(
            live_gated_stack(None),
            &StackState::new(Platform::Aws),
            &Default::default(),
        )
        .expect_err("no value and no default cannot resolve");
        assert!(error.message.contains("cacheEnabled"), "{}", error.message);
    }

    /// Input values are coerced to their declared kinds before they reach
    /// this layer; a non-boolean here is corrupt input and must fail loudly.
    #[test]
    fn a_non_boolean_gate_value_fails_fast() {
        let error = strip_declined_resources(
            live_gated_stack(Some(true)),
            &StackState::new(Platform::Aws),
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
