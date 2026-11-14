use crate::error::{ErrorData, Result};
use crate::{CheckResult, CompileTimeCheck, PreflightRegistry, PreflightSummary};
use alien_core::{DeploymentConfig, Platform, Stack, StackState};
use alien_error::{AlienError, Context};
use tracing::{debug, error, info, warn};

#[cfg(feature = "runtime-checks")]
use alien_core::ClientConfig;

/// Preflight runner that executes all checks and mutations
pub struct PreflightRunner {
    registry: PreflightRegistry,
}

impl PreflightRunner {
    /// Create a new preflight runner with the default registry
    pub fn new() -> Self {
        Self {
            registry: PreflightRegistry::with_built_ins(),
        }
    }

    /// Create a preflight runner with a custom registry
    pub fn with_registry(registry: PreflightRegistry) -> Self {
        Self { registry }
    }

    /// Run compile-time checks on a stack
    pub async fn run_compile_time_checks(
        &self,
        stack: &Stack,
        platform: Platform,
    ) -> Result<PreflightSummary> {
        info!("Running compile-time checks for platform {:?}", platform);
        let checks = self.registry.get_compile_time_checks(stack, platform);
        let results = run_stack_checks(checks, stack, platform).await?;
        Ok(PreflightSummary::from_results(results))
    }

    /// Run stack compatibility checks between two stacks.
    ///
    /// The Frozen check runs on every call rather than from the registry, because it needs the
    /// installed stack's platform, which a registered check cannot be given.
    pub async fn run_compatibility_checks(
        &self,
        old_stack: &Stack,
        new_stack: &Stack,
        config: &DeploymentConfig,
        platform: Platform,
    ) -> Result<PreflightSummary> {
        info!("Running stack compatibility checks");

        let frozen_check = crate::compatibility::FrozenResourcesUnchangedCheck { platform };
        let mut checks = self.registry.get_compatibility_checks();
        checks.push(&frozen_check);
        let mut results = Vec::new();

        for check in checks {
            debug!("Running compatibility check: {}", check.description());

            let mut result = check
                .check_with_config(old_stack, new_stack, config, platform)
                .await
                .context(ErrorData::StackCompatibilityCheckFailed {
                    check_name: check.description().to_string(),
                    message: "Compatibility check execution failed".to_string(),
                    old_resource_id: None,
                    new_resource_id: None,
                })?;

            result = result.with_check_metadata(check.code(), check.description());

            if !result.success {
                error!(check = %check.description(), "Compatibility check failed");
                for msg in &result.errors {
                    error!(check = %check.description(), "  {}", msg);
                }
            }

            for warning in &result.warnings {
                warn!(check = %check.description(), "  Warning: {}", warning);
            }

            results.push(result);
        }

        Ok(PreflightSummary::from_results(results))
    }

    /// Run runtime checks on a stack
    #[cfg(feature = "runtime-checks")]
    pub async fn run_runtime_checks(
        &self,
        stack: &Stack,
        stack_state: &StackState,
        config: &DeploymentConfig,
        client_config: &ClientConfig,
        platform: Platform,
    ) -> Result<PreflightSummary> {
        crate::quotas::validate_warning_overrides(
            platform,
            &config.stack_settings.quota_warning_overrides,
        )?;
        info!("Running runtime checks for platform {:?}", platform);

        let checks = self.registry.get_runtime_checks(stack, platform);
        let mut results = Vec::new();

        for check in checks {
            debug!("Running runtime check: {}", check.description());

            let mut result = check
                .check(stack, stack_state, config, client_config)
                .await
                .context(ErrorData::RuntimeCheckFailed {
                    check_name: check.description().to_string(),
                    message: "Runtime check execution failed".to_string(),
                    platform: Some(platform.to_string()),
                })?;

            crate::quotas::apply_warning_overrides(
                &mut result,
                &config.stack_settings.quota_warning_overrides,
            );
            result = result.with_check_metadata(check.code(), check.description());

            if !result.success {
                error!(check = %check.description(), "Runtime check failed");
                for msg in &result.errors {
                    error!(check = %check.description(), "  {}", msg);
                }
            }

            for warning in &result.warnings {
                warn!(check = %check.description(), "  Warning: {}", warning);
            }

            results.push(result);
        }

        Ok(PreflightSummary::from_results(results))
    }

    /// Run deployment prerequisite checks on the final stack/config.
    pub async fn run_deployment_prerequisite_checks(
        &self,
        stack: &Stack,
        stack_state: &StackState,
        config: &DeploymentConfig,
    ) -> Result<PreflightSummary> {
        info!(
            "Running deployment prerequisite checks for platform {:?}",
            stack_state.platform
        );

        let checks = self
            .registry
            .get_deployment_prerequisite_checks(stack, stack_state, config);
        let mut results = Vec::new();

        for check in checks {
            debug!(
                "Running deployment prerequisite check: {}",
                check.description()
            );

            let mut result = check.check(stack, stack_state, config).await.context(
                ErrorData::DeploymentPrerequisiteCheckFailed {
                    check_name: check.description().to_string(),
                    message: "Check execution failed".to_string(),
                    platform: Some(stack_state.platform.to_string()),
                },
            )?;

            result = result.with_check_metadata(check.code(), check.description());

            if !result.success {
                error!(check = %check.description(), "Deployment prerequisite check failed");
                for msg in &result.errors {
                    error!(check = %check.description(), "  {}", msg);
                }
            }

            for warning in &result.warnings {
                warn!(check = %check.description(), "  Warning: {}", warning);
            }

            results.push(result);
        }

        Ok(PreflightSummary::from_results(results))
    }

    /// At first setup on Azure, automatic selection picks the first machine series, in catalog
    /// order, whose family and regional quota have room for the pool's machines. Only pools
    /// that are not installed yet get a quota-aware choice; an installed pool keeps its machine.
    /// Returns the settings to plan with, or `None` when nothing changes. When no series has
    /// room the settings stay as they were, so the quota check reports the refusal, and a
    /// warning names the series checked.
    /// Automatic selection could not read quota: say why, in the log and in the preflight
    /// result, and keep the catalog pick. Never a silent fallback.
    #[cfg(feature = "runtime-checks")]
    fn quota_unreadable(results: &mut Vec<CheckResult>, reason: &str) -> Option<DeploymentConfig> {
        warn!(%reason, "Automatic machine selection cannot read Azure quota; using the catalog pick");
        results.push(
            CheckResult::with_warnings(vec![format!(
                "Automatic machine selection could not use quota, so the catalog pick stays: {reason}."
            )])
            .with_check_description("Automatic machine selection".into()),
        );
        None
    }

    #[cfg(feature = "runtime-checks")]
    async fn choose_machines_with_room(
        &self,
        stack: &Stack,
        stack_state: &StackState,
        config: &DeploymentConfig,
        client_config: &ClientConfig,
        results: &mut Vec<CheckResult>,
    ) -> Result<Option<DeploymentConfig>> {
        use crate::runtime::azure_compute::ComputeRoom;
        use alien_core::{ComputeCluster, Container};
        if stack_state.platform != Platform::Azure
            || !stack.resources.values().any(|entry| {
                entry.config.downcast_ref::<Container>().is_some()
                    || entry.config.downcast_ref::<ComputeCluster>().is_some()
            })
        {
            return Ok(None);
        }
        let Some(azure) = client_config.azure_config() else {
            return Ok(Self::quota_unreadable(
                results,
                "the deployment has no Azure credentials for reading quota",
            ));
        };
        let http = match reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(5))
            .timeout(std::time::Duration::from_secs(15))
            .build()
        {
            Ok(http) => http,
            Err(err) => {
                return Ok(Self::quota_unreadable(
                    results,
                    &format!("the HTTP client could not be built: {err}"),
                ))
            }
        };
        let skus = alien_azure_clients::AzureResourceSkusClient::new(
            http.clone(),
            alien_azure_clients::AzureTokenCache::new(azure.clone()),
        );
        let usages = alien_azure_clients::AzureComputeUsagesClient::new(
            http,
            alien_azure_clients::AzureTokenCache::new(azure.clone()),
        );
        let region = azure.region.as_deref().unwrap_or("eastus");
        let room = match ComputeRoom::read(&skus, &usages, region).await {
            Ok(room) => room,
            Err(reason) => {
                return Ok(Self::quota_unreadable(
                    results,
                    &format!("in {region}, {reason}"),
                ))
            }
        };
        let ledger = crate::runtime::azure_compute::PoolQuotaLedger::new(&room, stack_state);
        let chosen = crate::mutations::ComputeClusterMutation.choose_pool_machines(
            stack.clone(),
            stack_state,
            config,
            &ledger,
        )?;
        for (pool, asked) in ledger.without_room() {
            let checked: Vec<String> = asked
                .iter()
                .map(|(series, machines)| format!("{series} ({machines} machines)"))
                .collect();
            results.push(
                CheckResult::with_warnings(vec![format!(
                    "No VM series has enough free quota in {region} for pool '{pool}'. Checked: {}.",
                    checked.join(", ")
                )])
                .with_check_description("Automatic machine selection".into()),
            );
        }
        Ok(Some(chosen))
    }

    /// Apply all stack mutations to a stack.
    ///
    /// Mutations are evaluated incrementally: each mutation's `should_run()` is checked
    /// against the current (already-mutated) stack, not the original. This ensures that
    /// mutations can react to resources created by earlier mutations (e.g., service
    /// activations seeing vault resources added by SecretsVaultMutation).
    pub async fn apply_mutations(
        &self,
        stack: Stack,
        stack_state: &StackState,
        config: &DeploymentConfig,
    ) -> Result<Stack> {
        info!(
            "Applying stack mutations for platform {:?}",
            stack_state.platform
        );

        let mut current_stack = stack;
        let mut effective_config = config.clone();
        alien_core::remove_off_platform_gated_resources(
            &mut current_stack,
            stack_state.platform,
            effective_config.stack_settings.compute.as_mut(),
        );
        let config = &effective_config;

        for mutation in self.registry.get_all_mutations() {
            if !mutation.should_run(&current_stack, stack_state, config) {
                continue;
            }

            debug!("Applying mutation: {}", mutation.description());

            current_stack = mutation
                .mutate(current_stack, stack_state, config)
                .await
                .context(ErrorData::StackMutationFailed {
                    mutation_name: mutation.description().to_string(),
                    message: "Mutation execution failed".to_string(),
                    resource_id: None,
                })?;
        }

        // Generated containers need the same capability checks as developer input.
        let network_check = crate::compile_time::ContainerNetworkSettingsCheck;
        if network_check.should_run(&current_stack, stack_state.platform) {
            let result = network_check
                .check(&current_stack, stack_state.platform)
                .await?
                .with_check_metadata(network_check.code(), network_check.description());
            if !result.success {
                return Err(AlienError::new(ErrorData::ValidationFailed {
                    error_count: 1,
                    warning_count: result.warnings.len(),
                    results: vec![result],
                }));
            }
        }

        let mut dependency_result =
            crate::compile_time::validate_stack_dependencies(&current_stack);
        dependency_result = dependency_result.with_check_metadata(
            Some("POST_MUTATION_DEPENDENCY_INVALID"),
            "Mutated resource dependencies should be valid and shouldn't create circular references",
        );
        if !dependency_result.success {
            error!(
                error_count = dependency_result.errors.len(),
                "Post-mutation dependency validation failed"
            );
            for msg in &dependency_result.errors {
                error!("  {}", msg);
            }
            return Err(AlienError::new(ErrorData::ValidationFailed {
                error_count: 1,
                warning_count: dependency_result.warnings.len(),
                results: vec![dependency_result],
            }));
        }

        Ok(current_stack)
    }

    /// Run template-generation preflights.
    ///
    /// Template generation uses only structural compile-time checks. Deployment
    /// prerequisite checks run later with `DeploymentConfig`.
    pub async fn run_template_preflights(
        &self,
        stack: &Stack,
        platform: Platform,
    ) -> Result<PreflightSummary> {
        info!(
            "Running template-generation preflights for platform {:?}",
            platform
        );

        let mut platform_stack = stack.clone();
        alien_core::remove_off_platform_gated_resources(&mut platform_stack, platform, None);
        let stack = &platform_stack;
        let checks = self.registry.get_template_checks(stack, platform);
        let mut results = Vec::new();

        for check in checks {
            debug!("Running check: {}", check.description());

            let mut result =
                check
                    .check(stack, platform)
                    .await
                    .context(ErrorData::CompileTimeCheckFailed {
                        check_name: check.description().to_string(),
                        message: "Check execution failed".to_string(),
                        resource_id: None,
                    })?;

            result = result.with_check_metadata(check.code(), check.description());

            if !result.success {
                error!(check = %check.description(), "Template preflight check failed");
                for msg in &result.errors {
                    error!(check = %check.description(), "  {}", msg);
                }
            }

            for warning in &result.warnings {
                warn!(check = %check.description(), "  Warning: {}", warning);
            }

            results.push(result);
        }

        let summary = PreflightSummary::from_results(results);

        if !summary.success {
            error!(
                error_count = summary.failed_checks,
                warning_count = summary.warning_count,
                "Template preflight checks failed"
            );
            return Err(AlienError::new(ErrorData::ValidationFailed {
                error_count: summary.failed_checks,
                warning_count: summary.warning_count,
                results: summary.results,
            }));
        }

        Ok(summary)
    }

    /// Run only the build-time-only checks, for a stack that was built earlier and is now being
    /// released without a rebuild (`alien release --prebuilt`). The other compile-time checks
    /// already ran on its source stack, and some of them do not accept a built stack's local
    /// image paths.
    pub async fn run_prebuilt_release_checks(
        &self,
        stack: &Stack,
        platform: Platform,
    ) -> Result<PreflightSummary> {
        let checks = self.registry.get_build_time_checks(stack, platform);
        let summary =
            PreflightSummary::from_results(run_stack_checks(checks, stack, platform).await?);
        if !summary.success {
            return Err(AlienError::new(ErrorData::ValidationFailed {
                error_count: summary.failed_checks,
                warning_count: summary.warning_count,
                results: summary.results,
            }));
        }
        new_release_checks(stack, platform).await?;
        Ok(summary)
    }

    /// Run the complete preflight pipeline for build-time (compile-time checks only)
    pub async fn run_build_time_preflights(
        &self,
        stack: &Stack,
        platform: Platform,
    ) -> Result<PreflightSummary> {
        info!("Running build-time preflights for platform {:?}", platform);

        // Run compile-time checks only - mutations are now deployment-time only. Build-time-only
        // checks apply to new stacks, never to ones that are already released.
        let mut checks = self.registry.get_compile_time_checks(stack, platform);
        checks.extend(self.registry.get_build_time_checks(stack, platform));
        let check_summary =
            PreflightSummary::from_results(run_stack_checks(checks, stack, platform).await?);

        // If checks failed, return early with the error summary
        if !check_summary.success {
            error!(
                error_count = check_summary.failed_checks,
                warning_count = check_summary.warning_count,
                "Build-time preflight checks failed"
            );
            return Err(AlienError::new(ErrorData::ValidationFailed {
                error_count: check_summary.failed_checks,
                warning_count: check_summary.warning_count,
                results: check_summary.results,
            }));
        }
        new_release_checks(stack, platform).await?;

        Ok(check_summary)
    }

    /// Run deployment-time preflights (compile-time checks + mutations + prerequisite checks + compatibility checks + runtime checks)
    ///
    /// The order is critical:
    /// 1. Compile-time checks on user-provided stack (fast validation)
    /// 2. Apply mutations to add infrastructure resources
    /// 3. Deployment prerequisite checks on mutated stack/config
    /// 4. Compatibility checks on mutated stacks (detects frozen resource changes)
    /// 5. Runtime checks on mutated stack (cloud API validation)
    #[cfg(feature = "runtime-checks")]
    pub async fn run_deployment_time_preflights(
        &self,
        stack: Stack,
        stack_state: &StackState,
        config: &DeploymentConfig,
        client_config: &ClientConfig,
        old_stack: Option<&Stack>,
        setup_update_authorization: Option<&alien_core::SetupUpdateAuthorization>,
        setup_authority: Option<alien_core::InitialSetupAuthority>,
    ) -> Result<(Stack, PreflightSummary, bool)> {
        // Invalid deployment settings must not be hidden by setup or
        // compatibility failures encountered while preparing the target.
        crate::quotas::validate_warning_overrides(
            stack_state.platform,
            &config.stack_settings.quota_warning_overrides,
        )?;

        // Updates reuse saved allocations across releases. Ignore entries for
        // containers no longer declared, including ones removed several releases
        // ago. Fresh installs and planning still reject unknown container IDs.
        let mut effective_config = config.clone();
        if old_stack.is_some() {
            if let Some(compute) = effective_config.stack_settings.compute.as_mut() {
                compute.containers.retain(|id, _| {
                    stack.resources.get(id).is_some_and(|entry| {
                        entry
                            .config
                            .downcast_ref::<alien_core::Container>()
                            .is_some()
                    })
                });
            }
        }
        let platform = stack_state.platform;
        validate_installed_gate_platform_changes(
            stack_state.resources.iter().filter_map(|(id, resource)| {
                (resource.status != alien_core::ResourceStatus::Deleted).then_some(id)
            }),
            &stack,
            platform,
        )?;
        let warnings = stack
            .inputs
            .iter()
            .filter(|input| {
                !input.applies_to(platform)
                    && config.input_values.contains_key(&input.id)
                    && stack.resources.values().any(|entry| {
                        entry.enabled_when.as_deref() == Some(input.id.as_str())
                    })
            })
            .map(|input| format!(
                "Ignoring supplied gate input '{}' because it is unavailable on deployment platform '{platform}'; its gated resources are declined",
                input.id
            ))
            .collect::<Vec<_>>();
        let mut stack = stack;
        alien_core::remove_off_platform_gated_resources(
            &mut stack,
            platform,
            effective_config.stack_settings.compute.as_mut(),
        );
        let config = &effective_config;
        info!(
            "Running deployment-time preflights for platform {:?}",
            platform
        );

        let mut all_results: Vec<CheckResult> = Vec::new();
        if !warnings.is_empty() {
            all_results.push(CheckResult::with_warnings(warnings).with_check_description(
                "Supplied gate inputs must apply to the deployment platform".into(),
            ));
        }

        // Run compile-time checks first (fast, no cloud API calls)
        let compile_summary = self.run_compile_time_checks(&stack, platform).await?;
        let compile_checks_succeeded = compile_summary.success;
        all_results.extend(compile_summary.results);

        // Apply mutations BEFORE compatibility checks
        // This ensures compatibility checks compare mutated stacks (old mutated vs new mutated)
        let chosen_config = self
            .choose_machines_with_room(&stack, stack_state, config, client_config, &mut all_results)
            .await?;
        let config = chosen_config.as_ref().unwrap_or(config);
        let mutated_stack = self.apply_mutations(stack, stack_state, config).await?;
        // Drop the comparison projection before later awaits so it does not enlarge
        // the deployment future and exhaust the runner's worker stack.
        let mut authorization_mismatch: Option<String> = None;
        let (direct_setup, imported_setup_authorized) = {
            let direct_setup =
                setup_authority == Some(alien_core::InitialSetupAuthority::DirectSetup);
            let automatic_azure_runtime = !config.setup_run
                && config.base_platform.unwrap_or(platform) == Platform::Azure
                && config.stack_settings.setup_management == alien_core::SetupManagement::Automatic;
            // A direct setup's runtime still needs setup's authorization for the changes
            // only setup applies, such as its own management grant.
            let hashed_target = match old_stack {
                Some(_) if !direct_setup || !config.setup_run => {
                    Some(without_declined_live_resources(
                        &mutated_stack,
                        &config.input_values,
                        platform,
                    )?)
                }
                _ => None,
            };
            let imported_setup_authorized = match (
                old_stack,
                hashed_target.as_ref(),
                setup_update_authorization,
            ) {
                (Some(installed), Some(target), Some(authorization)) if automatic_azure_runtime => {
                    crate::compatibility::azure_setup::authorization_matches(
                        installed,
                        target,
                        authorization,
                    )?
                }
                (_, Some(target), Some(authorization)) => {
                    let mismatches =
                        setup_update_authorization_mismatches(old_stack, target, authorization);
                    if !mismatches.is_empty() {
                        let description = mismatches.join("; ");
                        info!(
                            release_id = %authorization.release_id,
                            mismatches = %description,
                            "setup update authorization does not match this update"
                        );
                        if let Some(installed) = old_stack {
                            let differences = SetupOwnedDifferences::between(installed, target);
                            info!(
                                added = ?differences.added,
                                removed = ?differences.removed,
                                changed = ?differences.changed,
                                changed_profiles = ?differences.changed_profiles,
                                management_changed = differences.management_changed,
                                "setup-owned entries and permission profiles that differ between the installed and target stacks"
                            );
                        }
                        authorization_mismatch = Some(description);
                    }
                    authorization_mismatch.is_none()
                }
                _ => false,
            };
            if automatic_azure_runtime && !imported_setup_authorized {
                if let (Some(installed), Some(target)) = (old_stack, hashed_target.as_ref()) {
                    crate::compatibility::azure_setup::require_unchanged_setup(installed, target)?;
                }
            }
            (direct_setup, imported_setup_authorized)
        };
        let setup_update_authorized = direct_setup || imported_setup_authorized;

        // Match the deployment values used by the IAM policy writers. Physical
        // resource names use shared create naming or saved outputs where available.
        let mut permission_context = alien_permissions::PermissionContext::new();
        if let Some(aws) = client_config.aws_config() {
            permission_context = permission_context
                .with_aws_account_id(aws.account_id.clone())
                .with_aws_region(aws.region.clone())
                .with_stack_prefix(stack_state.resource_prefix.clone());
        }

        if let Some(alien_core::ManagementConfig::Aws(management)) = &config.management_config {
            permission_context =
                permission_context.with_managing_role_arn(management.managing_role_arn.clone());
            if let Some(account) =
                alien_permissions::PermissionContext::extract_account_id_from_role_arn(
                    &management.managing_role_arn,
                )
            {
                permission_context = permission_context.with_managing_account_id(account);
            }
        }

        all_results.push(crate::compile_time::iam_workload_aggregate::check(
            &mutated_stack,
            platform,
            setup_authority,
            &permission_context,
            Some(stack_state),
        )?);

        let prerequisite_summary = self
            .run_deployment_prerequisite_checks(&mutated_stack, stack_state, config)
            .await?;
        all_results.extend(prerequisite_summary.results);

        // Run compatibility checks on mutated stack if old stack is provided
        // This detects if mutations added frozen resources during updates
        // Frozen changes are setup-owned. A matching setup authorization proves
        // that the setup workflow already applied and imported them; a runtime
        // configuration flag must never authorize their mutation.
        if let Some(old_stack) = old_stack {
            if !setup_update_authorized {
                let compatibility_summary = self
                    .run_compatibility_checks(old_stack, &mutated_stack, config, platform)
                    .await?;
                // These checks compare the prepared target with installed resources,
                // including runtime-owned capacity changes. Do not duplicate that
                // decision using a hash of the unprepared release.
                // Setup is where missing target prerequisites (such as a new
                // external binding) can be supplied. They must not hide the
                // independently required Frozen handoff behind a generic error.
                // Intrinsically invalid releases still fail ordinary validation.
                if !compatibility_summary.success && compile_checks_succeeded {
                    let mut message = compatibility_summary
                        .results
                        .iter()
                        .chain(all_results.iter())
                        .flat_map(|result| result.errors.iter().cloned())
                        .collect::<Vec<_>>()
                        .join("; ");
                    if let Some(mismatch) = &authorization_mismatch {
                        message.push_str(&format!(
                            "; the recorded setup update authorization does not cover this update ({mismatch})"
                        ));
                    }
                    return Err(AlienError::new(ErrorData::SetupRequired { message }));
                }
                all_results.extend(compatibility_summary.results);
            } else if compile_checks_succeeded
                && direct_setup
                && !config.setup_run
                && !imported_setup_authorized
                && matches!(platform, Platform::Aws | Platform::Gcp | Platform::Azure)
                && crate::compatibility::PermissionProfilesUnchangedCheck.management_needs_setup(
                    old_stack,
                    &mutated_stack,
                    config,
                )
            {
                // A direct setup's runtime applies the other changes itself, but never
                // rewrites its own management identity: only a setup run does. Only the
                // clouds grant automatic setup every resource type, so on other platforms
                // adding a resource changes the grant and must keep running. Without
                // this, the update would run on the old grants, and the setup run that
                // applies the new ones would find nothing pending.
                return Err(AlienError::new(ErrorData::SetupRequired {
                    message:
                        "this release changes the management permissions, which only setup applies"
                            .to_string(),
                }));
            } else {
                info!("Applying explicit authority for frozen resource changes");
            }
        }

        // Run runtime checks on the mutated stack
        let runtime_summary = self
            .run_runtime_checks(&mutated_stack, stack_state, config, client_config, platform)
            .await?;
        all_results.extend(runtime_summary.results);

        let summary = PreflightSummary::from_results(all_results);

        // Return error if any checks failed
        if !summary.success {
            error!(
                error_count = summary.failed_checks,
                warning_count = summary.warning_count,
                "Deployment-time preflight checks failed"
            );
            for result in &summary.results {
                if !result.success || !result.warnings.is_empty() {
                    let check_name = result.check_description.as_deref().unwrap_or("unknown");
                    for msg in &result.errors {
                        error!(check = %check_name, "Preflight error: {}", msg);
                    }
                    for msg in &result.warnings {
                        warn!(check = %check_name, "Preflight warning: {}", msg);
                    }
                }
            }
            return Err(AlienError::new(ErrorData::ValidationFailed {
                error_count: summary.failed_checks,
                warning_count: summary.warning_count,
                results: summary.results,
            }));
        }

        for result in &summary.results {
            let check_name = result.check_description.as_deref().unwrap_or("unknown");
            for message in &result.warnings {
                warn!(check = %check_name, "Preflight warning: {}", message);
            }
        }

        Ok((mutated_stack, summary, setup_update_authorized))
    }
}

/// The target as the setup re-import hashed it: without the declined Live resources an update
/// strips only after these preflights. The compatibility checks keep the unstripped target, whose
/// gated resources carry the exemptions those checks read.
#[cfg(feature = "runtime-checks")]
fn without_declined_live_resources(
    stack: &Stack,
    input_values: &std::collections::HashMap<String, serde_json::Value>,
    platform: Platform,
) -> Result<Stack> {
    let (answers, still_frozen_gating) = alien_core::surviving_frozen_gate_answers(stack);
    let declined = alien_core::declined_live_resources(
        stack,
        input_values,
        &answers,
        &still_frozen_gating,
        platform,
    )
    .map_err(|message| {
        AlienError::new(ErrorData::DeploymentPrerequisiteCheckFailed {
            check_name: "Every runtime gate resolves to a boolean".to_string(),
            message,
            platform: Some(platform.to_string()),
        })
    })?;
    let mut projected = stack.clone();
    alien_core::remove_declined_live_resources(&mut projected, &declined);
    Ok(projected)
}

/// The compares of a recorded setup update authorization that fail for this update, each naming
/// the compare and its computed and recorded digests. Empty when the authorization matches.
fn setup_update_authorization_mismatches(
    old_stack: Option<&Stack>,
    target_stack: &Stack,
    authorization: &alien_core::SetupUpdateAuthorization,
) -> Vec<String> {
    let mut mismatches = Vec::new();
    match old_stack.map(Stack::setup_update_digest) {
        None => mismatches.push("baseline: no installed stack to compare".to_string()),
        Some(computed) if computed != authorization.baseline_frozen_digest => {
            mismatches.push(format!(
                "baseline: computed {computed}, recorded {}",
                authorization.baseline_frozen_digest
            ))
        }
        Some(_) => {}
    }
    let computed = target_stack.setup_update_digest();
    if computed != authorization.target_frozen_digest {
        mismatches.push(format!(
            "target: computed {computed}, recorded {}",
            authorization.target_frozen_digest
        ));
    }
    mismatches
}

/// What differs between the installed stack and the target in the inputs of their setup update
/// digests, by ID and profile name only. The recorded authorization holds only digests, so this
/// is the closest view of what changed without logging configuration values.
#[derive(Debug, PartialEq, Eq)]
struct SetupOwnedDifferences {
    added: Vec<String>,
    removed: Vec<String>,
    changed: Vec<String>,
    changed_profiles: Vec<String>,
    management_changed: bool,
}

impl SetupOwnedDifferences {
    fn between(old_stack: &Stack, target_stack: &Stack) -> Self {
        let installed = old_stack.setup_owned_entry_digests();
        let target = target_stack.setup_owned_entry_digests();
        let added = target
            .keys()
            .filter(|id| !installed.contains_key(*id))
            .cloned()
            .collect();
        let removed = installed
            .keys()
            .filter(|id| !target.contains_key(*id))
            .cloned()
            .collect();
        let changed = installed
            .iter()
            .filter(|(id, digest)| target.get(*id).is_some_and(|other| other != *digest))
            .map(|(id, _)| id.clone())
            .collect();
        // Compare as JSON, as the digest does: key order is ignored, list order is not.
        let json = |profile: Option<&alien_core::permissions::PermissionProfile>| {
            profile.map(|profile| {
                serde_json::to_value(profile).expect("permission profiles always serialize to JSON")
            })
        };
        let (installed_profiles, target_profiles) = (
            &old_stack.permissions.profiles,
            &target_stack.permissions.profiles,
        );
        let changed_profiles = installed_profiles
            .keys()
            .chain(
                target_profiles
                    .keys()
                    .filter(|name| !installed_profiles.contains_key(*name)),
            )
            .filter(|name| json(installed_profiles.get(*name)) != json(target_profiles.get(*name)))
            .cloned()
            .collect();
        let management_changed = serde_json::to_value(&old_stack.permissions.management)
            .expect("management permissions always serialize to JSON")
            != serde_json::to_value(&target_stack.permissions.management)
                .expect("management permissions always serialize to JSON");
        Self {
            added,
            removed,
            changed,
            changed_profiles,
            management_changed,
        }
    }
}

impl Default for PreflightRunner {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod setup_update_authorization_tests {
    use super::*;
    use alien_core::permissions::{
        ManagementPermissions, PermissionProfile, PermissionSetReference,
    };
    use alien_core::{
        PermissionsConfig, Queue, QueueBatchConfig, ResourceLifecycle, SetupUpdateAuthorization,
        Worker, WorkerCode, WorkerTrigger,
    };
    use indexmap::IndexMap;

    #[cfg(feature = "runtime-checks")]
    #[tokio::test]
    async fn automatic_machine_selection_says_why_when_it_cannot_read_quota() {
        let runner = PreflightRunner::with_registry(PreflightRegistry::new());
        let config = deployment_config();
        let state = StackState::new(Platform::Azure);
        let client = ClientConfig::Local {
            state_directory: "/unused".into(),
        };
        let stack = Stack::new("azure-pool".into())
            .add(
                alien_core::ComputeCluster::new("compute".into()).build(),
                ResourceLifecycle::Live,
            )
            .build();
        let mut results = Vec::new();
        let chosen = runner
            .choose_machines_with_room(&stack, &state, &config, &client, &mut results)
            .await
            .unwrap();
        assert!(chosen.is_none());
        assert_eq!(results.len(), 1, "{results:?}");
        let text = format!("{results:?}");
        assert!(
            text.contains("no Azure credentials for reading quota"),
            "{text}"
        );
    }

    #[cfg(feature = "runtime-checks")]
    #[tokio::test]
    async fn azure_automatic_updates_require_setup_for_identity_and_role_changes() {
        let runner = PreflightRunner::with_registry(PreflightRegistry::new());
        let mut config = deployment_config();
        config.stack_settings.setup_management = alien_core::SetupManagement::Automatic;
        let state = StackState::new(Platform::Azure);
        let client = ClientConfig::Local {
            state_directory: "/unused".into(),
        };
        let installed = Stack::new("azure-update".into())
            .permission(
                "execution",
                PermissionProfile::new().resource("secrets", ["vault/data-read"]),
            )
            .add(
                alien_core::ServiceAccount::new("execution-sa".into()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                alien_core::Vault::new("secrets".into()).build(),
                ResourceLifecycle::Live,
            )
            .build();
        let mut identity_added = installed.clone();
        identity_added.resources.extend(
            Stack::new("extra".into())
                .add(
                    alien_core::ServiceAccount::new("second-sa".into()).build(),
                    ResourceLifecycle::Frozen,
                )
                .build()
                .resources,
        );
        let mut identity_removed = installed.clone();
        identity_removed.resources.shift_remove("execution-sa");
        let mut identity_changed = installed.clone();
        identity_changed
            .resources
            .get_mut("execution-sa")
            .unwrap()
            .config = alien_core::Resource::new(
            alien_core::ServiceAccount::new("execution-sa".into())
                .stack_permission_set(
                    alien_permissions::get_permission_set("vault/data-write")
                        .unwrap()
                        .clone(),
                )
                .build(),
        );
        let mut role_changed = installed.clone();
        role_changed.permissions.profiles.insert(
            "execution".into(),
            PermissionProfile::new().resource("secrets", ["vault/data-write"]),
        );

        let mut worker_added = installed.clone();
        worker_added.resources.extend(
            Stack::new("extra".into())
                .add(
                    Worker::new("api".into())
                        .code(WorkerCode::Image {
                            image: "app:v1".into(),
                        })
                        .permissions("execution".into())
                        .build(),
                    ResourceLifecycle::Live,
                )
                .build()
                .resources,
        );
        assert_eq!(
            installed.setup_update_digest(),
            worker_added.setup_update_digest()
        );
        let mut declined_worker = worker_added.clone();
        declined_worker
            .inputs
            .push(alien_core::StackInputDefinition::deployer_boolean(
                "apiEnabled",
                "API",
                "Enable the API worker",
                Some(false),
            ));
        declined_worker
            .resources
            .get_mut("api")
            .unwrap()
            .enabled_when = Some("apiEnabled".into());
        let installed_without_worker = without_declined_live_resources(
            &declined_worker,
            &config.input_values,
            Platform::Azure,
        )
        .unwrap();
        assert!(!installed_without_worker.resources.contains_key("api"));
        runner
            .run_deployment_time_preflights(
                declined_worker.clone(),
                &state,
                &config,
                &client,
                Some(&installed_without_worker),
                None,
                Some(alien_core::InitialSetupAuthority::DirectSetup),
            )
            .await
            .expect("an unchanged declined worker must not require setup");
        let mut enabled_config = config.clone();
        enabled_config
            .input_values
            .insert("apiEnabled".into(), serde_json::json!(true));
        assert_eq!(
            runner
                .run_deployment_time_preflights(
                    declined_worker,
                    &state,
                    &enabled_config,
                    &client,
                    Some(&installed_without_worker),
                    None,
                    Some(alien_core::InitialSetupAuthority::DirectSetup),
                )
                .await
                .expect_err("enabling the worker must still check its roles")
                .code,
            "DEPLOYMENT_SETUP_REQUIRED"
        );
        for target in [
            identity_added,
            identity_removed,
            identity_changed,
            role_changed,
            worker_added,
        ] {
            let error = runner
                .run_deployment_time_preflights(
                    target.clone(),
                    &state,
                    &config,
                    &client,
                    Some(&installed),
                    None,
                    Some(alien_core::InitialSetupAuthority::DirectSetup),
                )
                .await
                .expect_err(
                    "automatic runtime cannot change setup-owned Azure identities or roles",
                );
            assert_eq!(error.code, "DEPLOYMENT_SETUP_REQUIRED");

            let mut approved = authorization(&installed);
            approved.baseline_frozen_digest =
                alien_permissions::generators::azure_setup_owned::azure_setup_update_digest(
                    &installed,
                )
                .unwrap();
            approved.target_frozen_digest =
                alien_permissions::generators::azure_setup_owned::azure_setup_update_digest(
                    &target,
                )
                .unwrap();
            runner
                .run_deployment_time_preflights(
                    target.clone(),
                    &state,
                    &config,
                    &client,
                    Some(&installed),
                    Some(&approved),
                    Some(alien_core::InitialSetupAuthority::DirectSetup),
                )
                .await
                .expect("the registered setup authorizes its exact target");
            approved.target_frozen_digest = "another-target".into();
            assert_eq!(
                runner
                    .run_deployment_time_preflights(
                        target.clone(),
                        &state,
                        &config,
                        &client,
                        Some(&installed),
                        Some(&approved),
                        Some(alien_core::InitialSetupAuthority::DirectSetup),
                    )
                    .await
                    .unwrap_err()
                    .code,
                "DEPLOYMENT_SETUP_REQUIRED"
            );

            let mut setup_config = config.clone();
            setup_config.setup_run = true;
            runner
                .run_deployment_time_preflights(
                    target,
                    &state,
                    &setup_config,
                    &client,
                    Some(&installed),
                    None,
                    Some(alien_core::InitialSetupAuthority::DirectSetup),
                )
                .await
                .expect("privileged setup can apply the change");
        }

        let mut target = installed.clone();
        target.resources.extend(
            Stack::new("extra".into())
                .add(
                    alien_core::Storage::new("archive".into()).build(),
                    ResourceLifecycle::Frozen,
                )
                .build()
                .resources,
        );
        runner
            .run_deployment_time_preflights(
                target.clone(),
                &state,
                &config,
                &client,
                Some(&installed),
                None,
                Some(alien_core::InitialSetupAuthority::DirectSetup),
            )
            .await
            .expect("automatic application infrastructure still runs without setup");
        let mut reordered = installed.clone();
        reordered.resources.reverse();
        runner
            .run_deployment_time_preflights(
                reordered,
                &state,
                &config,
                &client,
                Some(&installed),
                None,
                Some(alien_core::InitialSetupAuthority::DirectSetup),
            )
            .await
            .expect("resource order does not change installed roles");
    }

    fn empty_stack() -> Stack {
        Stack {
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
            operations: None,
            id: "stack".to_string(),
            resources: IndexMap::new(),
            inputs: vec![],
            permissions: PermissionsConfig::default(),
            supported_platforms: None,
        }
    }

    #[cfg(feature = "runtime-checks")]
    #[tokio::test]
    async fn batch_metadata_read_requires_setup_without_heartbeat_grants() {
        let source = |batch, management| {
            let queue = Queue::new("jobs".into()).build();
            let trigger = if batch {
                WorkerTrigger::queue_batch(
                    &queue,
                    QueueBatchConfig {
                        max_messages: 10,
                        max_window_seconds: 0,
                    },
                )
                .unwrap()
            } else {
                WorkerTrigger::queue(&queue)
            };
            Stack::new("stack".into())
                .management(management)
                .add(queue, ResourceLifecycle::Frozen)
                .add(
                    Worker::new("consumer".into())
                        .code(WorkerCode::Image {
                            image: "app:v1".into(),
                        })
                        .permissions("execution".into())
                        .commands_enabled(false)
                        .trigger(trigger)
                        .build(),
                    ResourceLifecycle::Live,
                )
                .build()
        };
        let runner = PreflightRunner::with_registry({
            let mut registry = PreflightRegistry::new();
            registry.add_mutation(Box::new(
                crate::mutations::ManagementPermissionProfileMutation,
            ));
            registry.add_compatibility_check(Box::new(
                crate::compatibility::PermissionProfilesUnchangedCheck,
            ));
            registry
        });
        let mut config = deployment_config();
        config.stack_settings.heartbeats =
            serde_json::from_value(serde_json::json!("off")).unwrap();
        let state = StackState::new(Platform::Aws);
        let client = ClientConfig::Local {
            state_directory: "/unused".into(),
        };
        for management in [
            ManagementPermissions::Auto,
            ManagementPermissions::Extend(PermissionProfile::new()),
            ManagementPermissions::Override(PermissionProfile::new().global(["worker/provision"])),
            ManagementPermissions::Override(
                PermissionProfile::new()
                    .global(["worker/provision"])
                    .resource(
                        "jobs",
                        [
                            "queue/heartbeat",
                            "queue/management",
                            "queue/provision",
                            "queue/data-read",
                        ],
                    ),
            ),
        ] {
            let old = runner
                .apply_mutations(source(false, management.clone()), &state, &config)
                .await
                .unwrap();
            let target_source = source(true, management.clone());
            let target = runner
                .apply_mutations(target_source.clone(), &state, &config)
                .await
                .unwrap();
            let profile = match target.management() {
                ManagementPermissions::Extend(profile)
                | ManagementPermissions::Override(profile) => profile,
                _ => panic!("prepared management profile required"),
            };
            assert!(profile.0["jobs"]
                .contains(&PermissionSetReference::from_name("queue/batch-metadata")));
            assert_eq!(
                runner
                    .apply_mutations(target.clone(), &state, &config)
                    .await
                    .unwrap()
                    .permissions,
                target.permissions
            );
            let error = runner
                .run_deployment_time_preflights(
                    target_source.clone(),
                    &state,
                    &config,
                    &client,
                    Some(&old),
                    None,
                    None,
                )
                .await
                .expect_err("metadata grant must require setup before cloud access");
            assert_eq!(error.code, "DEPLOYMENT_SETUP_REQUIRED");
            let mut approval = authorization(&old);
            approval.target_frozen_digest = target.setup_update_digest();
            let (_, _, authorized) = runner
                .run_deployment_time_preflights(
                    target_source,
                    &state,
                    &config,
                    &client,
                    Some(&old),
                    Some(&approval),
                    None,
                )
                .await
                .unwrap();
            assert!(authorized);
            runner
                .run_deployment_time_preflights(
                    source(false, management),
                    &state,
                    &config,
                    &client,
                    Some(&old),
                    None,
                    None,
                )
                .await
                .expect("nonoptin remains unchanged");
        }
        // Existing canonical grants cover the prerequisite without changing IAM.
        for grant in [
            "queue/heartbeat",
            "queue/management",
            "queue/provision",
            "queue/data-read",
            "queue/data-write",
            "queue/publish",
        ] {
            let management = ManagementPermissions::Override(
                PermissionProfile::new().global([grant, "worker/provision"]),
            );
            let old = runner
                .apply_mutations(source(false, management.clone()), &state, &config)
                .await
                .unwrap();
            let new = runner
                .apply_mutations(source(true, management), &state, &config)
                .await
                .unwrap();
            assert_eq!(old.permissions, new.permissions);
        }
        for platform in [Platform::Aws, Platform::Gcp, Platform::Azure] {
            let state = StackState::new(platform);
            let config = deployment_config();
            let old = runner
                .apply_mutations(source(false, ManagementPermissions::Auto), &state, &config)
                .await
                .unwrap();
            let new = runner
                .apply_mutations(source(true, ManagementPermissions::Auto), &state, &config)
                .await
                .unwrap();
            assert_eq!(
                old.permissions, new.permissions,
                "default profile on {platform}"
            );
        }
    }

    #[cfg(feature = "runtime-checks")]
    #[tokio::test]
    async fn queue_batch_visibility_requires_matching_setup_handoff() {
        let stack = |window: Option<u32>, image: &str, lifecycle| {
            let queue = Queue::new("jobs".into()).build();
            let trigger = match window {
                Some(window) => WorkerTrigger::queue_batch(
                    &queue,
                    QueueBatchConfig {
                        max_messages: 10,
                        max_window_seconds: window,
                    },
                )
                .unwrap(),
                None => WorkerTrigger::queue(&queue),
            };
            Stack::new("stack".into())
                .add(queue, lifecycle)
                .add(
                    Worker::new("consumer".into())
                        .code(WorkerCode::Image {
                            image: image.into(),
                        })
                        .permissions("execution".into())
                        .trigger(trigger)
                        .build(),
                    ResourceLifecycle::Live,
                )
                .build()
        };
        // Reload a legacy stack with the omitted batch field, as stored before batching.
        let old: Stack = serde_json::from_value(
            serde_json::to_value(stack(None, "app:v1", ResourceLifecycle::Frozen)).unwrap(),
        )
        .unwrap();
        let target = stack(Some(5), "app:v2", ResourceLifecycle::Frozen);
        let runner = PreflightRunner::with_registry(PreflightRegistry::new());
        let config = deployment_config();
        let state = StackState::new(Platform::Aws);
        // No cloud client is available: the setup gate must run before provider writes.
        let client = ClientConfig::Local {
            state_directory: "/unused".into(),
        };
        let error = runner
            .run_deployment_time_preflights(
                target.clone(),
                &state,
                &config,
                &client,
                Some(&old),
                None,
                None,
            )
            .await
            .expect_err("derived Frozen queue visibility needs setup");
        assert_eq!(error.code, "DEPLOYMENT_SETUP_REQUIRED");
        assert!(!error.retryable);
        assert!(error.message.contains("Frozen queue 'jobs'"));
        assert_ne!(old.setup_update_digest(), target.setup_update_digest());

        let mut approval = authorization(&old);
        approval.target_frozen_digest = target.setup_update_digest();
        let (_, _, authorized) = runner
            .run_deployment_time_preflights(
                target.clone(),
                &state,
                &config,
                &client,
                Some(&old),
                Some(&approval),
                None,
            )
            .await
            .expect("matching setup handoff permits the prepared target");
        assert!(authorized);
        let error = runner
            .run_deployment_time_preflights(
                stack(Some(10), "app:v2", ResourceLifecycle::Frozen),
                &state,
                &config,
                &client,
                Some(&old),
                Some(&approval),
                None,
            )
            .await
            .expect_err("approval for a smaller window cannot authorize a larger one");
        assert_eq!(error.code, "DEPLOYMENT_SETUP_REQUIRED");

        for (baseline, desired) in [
            (old.clone(), old.clone()),
            (
                old.clone(),
                stack(None, "app:v2", ResourceLifecycle::Frozen),
            ),
            (
                target.clone(),
                stack(None, "app:v3", ResourceLifecycle::Frozen),
            ),
            (
                target.clone(),
                stack(Some(1), "app:v3", ResourceLifecycle::Frozen),
            ),
            (
                stack(None, "app:v1", ResourceLifecycle::Live),
                stack(Some(5), "app:v2", ResourceLifecycle::Live),
            ),
        ] {
            let (_, _, authorized) = runner
                .run_deployment_time_preflights(
                    desired,
                    &state,
                    &config,
                    &client,
                    Some(&baseline),
                    None,
                    None,
                )
                .await
                .expect("unchanged, legacy, decreasing, or Live queue needs no setup");
            assert!(!authorized);
        }
        for platform in [Platform::Gcp, Platform::Azure] {
            runner
                .run_deployment_time_preflights(
                    target.clone(),
                    &StackState::new(platform),
                    &config,
                    &client,
                    Some(&old),
                    None,
                    None,
                )
                .await
                .expect("SQS visibility gate is AWS-only");
        }
    }

    #[cfg(feature = "runtime-checks")]
    #[tokio::test]
    async fn removed_container_allocations_do_not_block_updates_but_unknown_ids_do() {
        use alien_core::{
            ComputeSettings, Container, ContainerResourceSelection, ResourceLifecycle,
        };
        let previous = Stack::new("stack".into())
            .add(
                Container::new("removed".into())
                    .code(alien_core::ContainerCode::Image {
                        image: "nginx:alpine".into(),
                    })
                    .cpu(alien_core::ResourceSpec {
                        min: "1".into(),
                        desired: "1".into(),
                    })
                    .memory(alien_core::ResourceSpec {
                        min: "1Gi".into(),
                        desired: "1Gi".into(),
                    })
                    .permissions("app".into())
                    .build(),
                ResourceLifecycle::Live,
            )
            .build();
        let target = empty_stack();
        let mut config = deployment_config();
        config.stack_settings.compute = Some(ComputeSettings {
            pools: Default::default(),
            containers: [(
                "removed".into(),
                ContainerResourceSelection {
                    cpu: Some(4.into()),
                    volume_size_gib: None,
                    memory: Some("8Gi".into()),
                },
            )]
            .into(),
        });
        let mut registry = crate::PreflightRegistry::new();
        registry.add_mutation(Box::new(crate::mutations::ContainerResourcesMutation));
        let runner = PreflightRunner::with_registry(registry);
        let state = StackState::new(Platform::Local);
        let client = ClientConfig::Local {
            state_directory: "/unused".into(),
        };
        runner
            .run_deployment_time_preflights(
                target.clone(),
                &state,
                &config,
                &client,
                Some(&previous),
                None,
                None,
            )
            .await
            .expect("removed allocation is obsolete");
        runner
            .run_deployment_time_preflights(
                target.clone(),
                &state,
                &config,
                &client,
                Some(&empty_stack()),
                None,
                None,
            )
            .await
            .expect("a later release still ignores obsolete saved allocations");
        runner
            .run_deployment_time_preflights(target, &state, &config, &client, None, None, None)
            .await
            .expect_err("unknown allocation on a fresh install still fails");
    }

    #[tokio::test]
    async fn generated_container_cannot_bypass_network_capability_checks() {
        struct AddEndpoint;
        #[async_trait::async_trait]
        impl crate::StackMutation for AddEndpoint {
            fn description(&self) -> &'static str {
                "test generated endpoint"
            }
            fn should_run(&self, _: &Stack, _: &StackState, _: &DeploymentConfig) -> bool {
                true
            }
            async fn mutate(
                &self,
                stack: Stack,
                _: &StackState,
                _: &DeploymentConfig,
            ) -> Result<Stack> {
                let container = alien_core::Container::new("generated".into())
                    .code(alien_core::ContainerCode::Image {
                        image: "example:latest".into(),
                    })
                    .cpu(alien_core::ResourceSpec {
                        min: "1".into(),
                        desired: "1".into(),
                    })
                    .memory(alien_core::ResourceSpec {
                        min: "128Mi".into(),
                        desired: "128Mi".into(),
                    })
                    .permissions("app".into())
                    .egress(alien_core::ContainerEgress::Internet)
                    .security(alien_core::ContainerSecurity {
                        profile: alien_core::ContainerSecurityProfile::Restricted,
                        run_as_user: Some(1000),
                        run_as_group: Some(1000),
                        read_only_root_filesystem: Some(false),
                        writable_paths: Vec::new(),
                    })
                    .build();
                let mut result = stack;
                result.resources.insert(
                    "generated".into(),
                    alien_core::ResourceEntry {
                        config: alien_core::Resource::new(container),
                        lifecycle: alien_core::ResourceLifecycle::Live,
                        dependencies: vec![],
                        enabled_when: None,
                        display_name: None,
                        description: None,
                        remote_access: false,
                    },
                );
                Ok(result)
            }
        }
        let mut registry = PreflightRegistry::new();
        registry.add_mutation(Box::new(AddEndpoint));
        let runner = PreflightRunner::with_registry(registry);
        let error = runner
            .apply_mutations(
                empty_stack(),
                &StackState::new(Platform::Local),
                &deployment_config(),
            )
            .await
            .unwrap_err();
        let Some(ErrorData::ValidationFailed { results, .. }) = error.error else {
            panic!("expected capability validation failure")
        };
        assert!(results[0]
            .errors
            .iter()
            .any(|message| message.contains("CONTAINER_NETWORK_SETTINGS_UNSUPPORTED")));
    }

    fn authorization(stack: &Stack) -> SetupUpdateAuthorization {
        SetupUpdateAuthorization {
            nonce: "revision".to_string(),
            baseline_frozen_digest: stack.setup_update_digest(),
            target_frozen_digest: stack.setup_update_digest(),
            release_id: "release".to_string(),
            setup_target: "target".to_string(),
            setup_fingerprint: "fingerprint".to_string(),
            setup_fingerprint_version: 1,
        }
    }

    #[cfg(feature = "runtime-checks")]
    #[tokio::test]
    async fn invalid_quota_override_rejected_even_without_registered_checks() {
        let config = DeploymentConfig::builder()
            .stack_settings(alien_core::StackSettings {
                quota_warning_overrides: vec!["L-TYPO".into()],
                ..Default::default()
            })
            .environment_variables(alien_core::EnvironmentVariablesSnapshot {
                variables: vec![],
                hash: String::new(),
                created_at: String::new(),
            })
            .allow_frozen_changes(false)
            .external_bindings(alien_core::ExternalBindings::default())
            .build();
        let runner = PreflightRunner::with_registry(crate::PreflightRegistry::new());
        let error = runner
            .run_runtime_checks(
                &empty_stack(),
                &StackState::new(Platform::Aws),
                &config,
                &ClientConfig::Local {
                    state_directory: "/unused".into(),
                },
                Platform::Aws,
            )
            .await
            .expect_err(
                "invalid settings cannot hide behind an empty stack or missing credentials",
            );
        assert!(error.to_string().contains("L-TYPO"));
        assert!(error.to_string().contains("Unknown quota override"));
    }

    #[cfg(feature = "runtime-checks")]
    #[tokio::test]
    async fn prepared_changes_require_setup_but_unchanged_targets_do_not() {
        let old = empty_stack();
        let target = Stack::new("stack".to_string())
            .add(
                alien_core::Storage::new("evidence".to_string()).build(),
                alien_core::ResourceLifecycle::Frozen,
            )
            .build();
        let config = DeploymentConfig::builder()
            .stack_settings(alien_core::StackSettings::default())
            .environment_variables(alien_core::EnvironmentVariablesSnapshot {
                variables: vec![],
                hash: String::new(),
                created_at: String::new(),
            })
            .allow_frozen_changes(false)
            .external_bindings(alien_core::ExternalBindings::default())
            .build();
        let runner = PreflightRunner::with_registry(crate::PreflightRegistry::new());
        let state = StackState::new(Platform::Local);
        let client = ClientConfig::Local {
            state_directory: "/unused".to_string(),
        };
        let error = runner
            .run_deployment_time_preflights(
                target.clone(),
                &state,
                &config,
                &client,
                Some(&old),
                None,
                None,
            )
            .await
            .expect_err("new frozen storage needs setup");
        assert_eq!(error.code, "DEPLOYMENT_SETUP_REQUIRED");
        assert!(error.message.contains("evidence"));
        assert!(!error.retryable);
        assert!(old.resources.is_empty());
        runner
            .run_deployment_time_preflights(
                old.clone(),
                &state,
                &config,
                &client,
                Some(&old),
                None,
                None,
            )
            .await
            .expect("same release/configuration needs no setup");
        let (_, _, authorized) = runner
            .run_deployment_time_preflights(
                target,
                &state,
                &config,
                &client,
                Some(&old),
                None,
                Some(alien_core::InitialSetupAuthority::DirectSetup),
            )
            .await
            .expect("explicit setup authority may prepare the new storage");
        assert!(authorized);
    }

    #[cfg(feature = "runtime-checks")]
    #[tokio::test]
    async fn unmatched_setup_authorization_names_the_failed_compare() {
        let old = empty_stack();
        let target = Stack::new("stack".to_string())
            .add(
                alien_core::Storage::new("evidence".to_string()).build(),
                alien_core::ResourceLifecycle::Frozen,
            )
            .build();
        let approved_elsewhere = Stack::new("stack".to_string())
            .add(
                alien_core::Storage::new("other".to_string()).build(),
                alien_core::ResourceLifecycle::Frozen,
            )
            .build();
        let runner = PreflightRunner::with_registry(PreflightRegistry::new());
        let state = StackState::new(Platform::Local);
        let client = ClientConfig::Local {
            state_directory: "/unused".to_string(),
        };
        let config = deployment_config();
        let run = |authorization: SetupUpdateAuthorization| {
            let (runner, target, old, state, config, client) =
                (&runner, &target, &old, &state, &config, &client);
            async move {
                runner
                    .run_deployment_time_preflights(
                        target.clone(),
                        state,
                        config,
                        client,
                        Some(old),
                        Some(&authorization),
                        None,
                    )
                    .await
                    .expect_err("an authorization for other changes does not cover these")
            }
        };

        // The setup that recorded this authorization installed a different target.
        let mut other_target = authorization(&old);
        other_target.target_frozen_digest = approved_elsewhere.setup_update_digest();
        let error = run(other_target).await;
        assert_eq!(error.code, "DEPLOYMENT_SETUP_REQUIRED");
        assert!(error.message.contains("evidence"), "{}", error.message);
        assert!(
            error.message.contains(&format!(
                "target: computed {}, recorded {}",
                target.setup_update_digest(),
                approved_elsewhere.setup_update_digest()
            )),
            "{}",
            error.message
        );
        assert!(!error.message.contains("baseline:"), "{}", error.message);

        // The setup started from a different installed stack.
        let mut other_baseline = authorization(&target);
        other_baseline.baseline_frozen_digest = approved_elsewhere.setup_update_digest();
        let error = run(other_baseline).await;
        assert_eq!(error.code, "DEPLOYMENT_SETUP_REQUIRED");
        assert!(
            error.message.contains(&format!(
                "baseline: computed {}, recorded {}",
                old.setup_update_digest(),
                approved_elsewhere.setup_update_digest()
            )),
            "{}",
            error.message
        );
        assert!(!error.message.contains("target:"), "{}", error.message);
    }

    #[cfg(feature = "runtime-checks")]
    #[tokio::test]
    async fn permission_updates_require_exact_setup_authority() {
        let baseline = empty_stack();
        let mut target = baseline.clone();
        target.permissions.management = ManagementPermissions::Extend(
            PermissionProfile::new().resource("database", ["postgres/key-recovery"]),
        );
        let mut authority = authorization(&baseline);
        let runner = PreflightRunner::with_registry({
            let mut registry = crate::PreflightRegistry::new();
            registry.add_compatibility_check(Box::new(
                crate::compatibility::PermissionProfilesUnchangedCheck,
            ));
            registry
        });
        let config = deployment_config();
        let state = StackState::new(Platform::Local);
        let client = ClientConfig::Local {
            state_directory: "/unused".to_string(),
        };
        for stale_authority in [None, Some(&authority)] {
            let error = runner
                .run_deployment_time_preflights(
                    target.clone(),
                    &state,
                    &config,
                    &client,
                    Some(&baseline),
                    stale_authority,
                    None,
                )
                .await
                .expect_err("an uninstalled grant requires setup");
            assert_eq!(error.code, "DEPLOYMENT_SETUP_REQUIRED");
        }
        authority.target_frozen_digest = target.setup_update_digest();
        let (prepared, _, authorized) = runner
            .run_deployment_time_preflights(
                target.clone(),
                &state,
                &config,
                &client,
                Some(&baseline),
                Some(&authority),
                None,
            )
            .await
            .expect("setup installed this exact grant");
        assert!(authorized);
        assert_eq!(prepared.permissions, target.permissions);

        target.permissions.management = ManagementPermissions::Extend(
            PermissionProfile::new().resource("database", ["postgres/provision"]),
        );
        let error = runner
            .run_deployment_time_preflights(
                target,
                &state,
                &config,
                &client,
                Some(&baseline),
                Some(&authority),
                None,
            )
            .await
            .expect_err("recovery authority cannot authorize provisioning");
        assert_eq!(error.code, "DEPLOYMENT_SETUP_REQUIRED");
    }

    /// Runs the deployment preflights for a direct setup on a local client.
    #[cfg(feature = "runtime-checks")]
    async fn direct_setup_preflight(
        runner: &PreflightRunner,
        target: &Stack,
        installed: Option<&Stack>,
        state: &StackState,
        config: &DeploymentConfig,
    ) -> Result<(Stack, PreflightSummary, bool)> {
        authorized_direct_setup_preflight(runner, target, installed, state, config, None).await
    }

    /// Runs the deployment preflights for a direct setup that a setup rerun authorized.
    #[cfg(feature = "runtime-checks")]
    async fn authorized_direct_setup_preflight(
        runner: &PreflightRunner,
        target: &Stack,
        installed: Option<&Stack>,
        state: &StackState,
        config: &DeploymentConfig,
        authorization: Option<&SetupUpdateAuthorization>,
    ) -> Result<(Stack, PreflightSummary, bool)> {
        runner
            .run_deployment_time_preflights(
                target.clone(),
                state,
                config,
                &ClientConfig::Local {
                    state_directory: "/unused".to_string(),
                },
                installed,
                authorization,
                Some(alien_core::InitialSetupAuthority::DirectSetup),
            )
            .await
    }

    /// A direct setup's runtime never rewrites its own management identity, so an update
    /// that needs a wider management grant must stop for setup instead of running on the
    /// installed grants. A setup run applies it; narrowing keeps running on the wider
    /// installed grants until the next setup run. Off the clouds, such as Local, it runs.
    #[cfg(feature = "runtime-checks")]
    #[tokio::test]
    async fn direct_setup_runtime_requires_setup_for_a_wider_management_grant() {
        let with_management = |grants: &[&str]| {
            let mut stack = empty_stack();
            stack.permissions.management = ManagementPermissions::Extend(
                PermissionProfile::new().global(grants.iter().copied()),
            );
            stack
        };
        let installed = with_management(&["storage/provision"]);
        let widened = with_management(&["storage/provision", "storage/data-read"]);
        let changed = with_management(&["storage/data-read"]);
        let runner = PreflightRunner::with_registry(PreflightRegistry::new());
        let runtime = deployment_config();
        let mut setup = deployment_config();
        setup.setup_run = true;

        for platform in [Platform::Aws, Platform::Gcp] {
            let state = StackState::new(platform);
            for target in [&widened, &changed] {
                let error =
                    direct_setup_preflight(&runner, target, Some(&installed), &state, &runtime)
                        .await
                        .expect_err("the runtime cannot grant itself new management permissions");
                assert_eq!(error.code, "DEPLOYMENT_SETUP_REQUIRED", "{platform:?}");

                let (prepared, _, authorized) =
                    direct_setup_preflight(&runner, target, Some(&installed), &state, &setup)
                        .await
                        .expect("a setup run applies the new management permissions");
                assert!(authorized, "{platform:?}");
                assert_eq!(prepared.permissions, target.permissions, "{platform:?}");
            }

            let (prepared, _, authorized) =
                direct_setup_preflight(&runner, &installed, Some(&widened), &state, &runtime)
                    .await
                    .expect("narrowing runs on the wider installed grants");
            assert!(authorized, "{platform:?}");
            assert_eq!(prepared.permissions, installed.permissions, "{platform:?}");

            direct_setup_preflight(&runner, &installed, Some(&installed), &state, &runtime)
                .await
                .expect("unchanged management permissions need no setup");
        }

        // Off the clouds the management grant follows the resources, so a Local update runs.
        let (prepared, _, _) = direct_setup_preflight(
            &runner,
            &widened,
            Some(&installed),
            &StackState::new(Platform::Local),
            &runtime,
        )
        .await
        .expect("a local update has no management grant to wait for");
        assert_eq!(prepared.permissions, widened.permissions);
    }

    /// Automatic AWS setup installs provision grants for every resource type, so a release
    /// that adds a resource keeps running without setup; only a wider declared grant stops.
    #[cfg(feature = "runtime-checks")]
    #[tokio::test]
    async fn automatic_aws_runtime_adds_resources_but_halts_for_a_wider_management_grant() {
        let runner = PreflightRunner::with_registry({
            let mut registry = PreflightRegistry::new();
            registry.add_mutation(Box::new(
                crate::mutations::ManagementPermissionProfileMutation,
            ));
            registry
        });
        let state = StackState::new(Platform::Aws);
        let mut runtime = deployment_config();
        runtime.stack_settings.setup_management = alien_core::SetupManagement::Automatic;
        let mut setup = runtime.clone();
        setup.setup_run = true;
        let release = |management: ManagementPermissions, with_queue: bool| {
            let stack = Stack::new("stack".into()).management(management).add(
                alien_core::Storage::new("data".into()).build(),
                ResourceLifecycle::Live,
            );
            if with_queue {
                stack
                    .add(Queue::new("jobs".into()).build(), ResourceLifecycle::Live)
                    .build()
            } else {
                stack.build()
            }
        };

        let (installed, _, _) = direct_setup_preflight(
            &runner,
            &release(ManagementPermissions::Auto, false),
            None,
            &state,
            &setup,
        )
        .await
        .expect("setup installs the first release");

        let (with_queue, _, _) = direct_setup_preflight(
            &runner,
            &release(ManagementPermissions::Auto, true),
            Some(&installed),
            &state,
            &runtime,
        )
        .await
        .expect("setup already granted provisioning for every resource type");
        assert!(with_queue.resources.contains_key("jobs"));

        let wider = release(
            ManagementPermissions::Extend(PermissionProfile::new().global(["storage/data-read"])),
            false,
        );
        let error = direct_setup_preflight(&runner, &wider, Some(&installed), &state, &runtime)
            .await
            .expect_err("only setup can add a data grant to the management role");
        assert_eq!(error.code, "DEPLOYMENT_SETUP_REQUIRED");
        direct_setup_preflight(&runner, &wider, Some(&installed), &state, &setup)
            .await
            .expect("a setup run applies the data grant");
    }

    /// An automatic CloudFormation or Terraform rerun installs the wider management grant
    /// itself and records an exact authorization. The direct-setup runtime must accept it,
    /// or the update would wait for a setup run the installer never makes.
    #[cfg(feature = "runtime-checks")]
    #[tokio::test]
    async fn direct_setup_runtime_accepts_a_rerun_authorization_for_a_wider_management_grant() {
        let with_management = |grants: &[&str]| {
            let mut stack = empty_stack();
            stack.permissions.management = ManagementPermissions::Extend(
                PermissionProfile::new().global(grants.iter().copied()),
            );
            stack
        };
        let installed = with_management(&["storage/provision"]);
        let widened = with_management(&["storage/provision", "storage/data-read"]);
        let other = with_management(&["storage/provision", "storage/data-write"]);
        let authorization = |target: &Stack| SetupUpdateAuthorization {
            nonce: "nonce".to_string(),
            baseline_frozen_digest: installed.setup_update_digest(),
            target_frozen_digest: target.setup_update_digest(),
            release_id: "release".to_string(),
            setup_target: "target".to_string(),
            setup_fingerprint: "fingerprint".to_string(),
            setup_fingerprint_version: 1,
        };
        let runner = PreflightRunner::with_registry(PreflightRegistry::new());
        let runtime = deployment_config();

        for platform in [Platform::Aws, Platform::Gcp] {
            let state = StackState::new(platform);
            let (prepared, _, authorized) = authorized_direct_setup_preflight(
                &runner,
                &widened,
                Some(&installed),
                &state,
                &runtime,
                Some(&authorization(&widened)),
            )
            .await
            .expect("the rerun already installed this management grant");
            assert!(authorized, "{platform:?}");
            assert_eq!(prepared.permissions, widened.permissions, "{platform:?}");

            let error = authorized_direct_setup_preflight(
                &runner,
                &widened,
                Some(&installed),
                &state,
                &runtime,
                Some(&authorization(&other)),
            )
            .await
            .expect_err("an authorization for another grant does not cover this one");
            assert_eq!(error.code, "DEPLOYMENT_SETUP_REQUIRED", "{platform:?}");
        }
    }

    #[test]
    fn setup_owned_differences_name_ids_and_profiles_only() {
        let installed = Stack::new("stack".to_string())
            .permission(
                "kept",
                PermissionProfile::new().resource("kept-store", ["storage/data-read"]),
            )
            .permission(
                "widened",
                PermissionProfile::new().resource("kept-store", ["storage/data-read"]),
            )
            .permission(
                "dropped",
                PermissionProfile::new().resource("kept-store", ["storage/data-read"]),
            )
            .add(
                alien_core::Storage::new("kept-store".into()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                alien_core::Storage::new("changed-store".into()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                alien_core::Storage::new("removed-store".into()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                alien_core::Storage::new("live-store".into()).build(),
                ResourceLifecycle::Live,
            )
            .build();
        let target = Stack::new("stack".to_string())
            .permission(
                "kept",
                PermissionProfile::new().resource("kept-store", ["storage/data-read"]),
            )
            .permission(
                "widened",
                PermissionProfile::new().resource("kept-store", ["storage/data-write"]),
            )
            .permission(
                "added",
                PermissionProfile::new().resource("kept-store", ["storage/data-read"]),
            )
            .add(
                alien_core::Storage::new("kept-store".into()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                alien_core::Storage::new("changed-store".into())
                    .versioning(true)
                    .build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                alien_core::Storage::new("added-store".into()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                alien_core::Storage::new("live-store".into())
                    .versioning(true)
                    .build(),
                ResourceLifecycle::Live,
            )
            .build();

        assert_eq!(
            SetupOwnedDifferences::between(&installed, &target),
            SetupOwnedDifferences {
                added: vec!["added-store".to_string()],
                removed: vec!["removed-store".to_string()],
                changed: vec!["changed-store".to_string()],
                changed_profiles: vec![
                    "widened".to_string(),
                    "dropped".to_string(),
                    "added".to_string(),
                ],
                management_changed: false,
            }
        );

        let mut managed = installed.clone();
        managed.permissions.management = ManagementPermissions::Extend(
            PermissionProfile::new().resource("kept-store", ["storage/data-read"]),
        );
        assert_eq!(
            SetupOwnedDifferences::between(&installed, &managed),
            SetupOwnedDifferences {
                added: vec![],
                removed: vec![],
                changed: vec![],
                changed_profiles: vec![],
                management_changed: true,
            }
        );
        assert_eq!(
            SetupOwnedDifferences::between(&installed, &installed),
            SetupOwnedDifferences {
                added: vec![],
                removed: vec![],
                changed: vec![],
                changed_profiles: vec![],
                management_changed: false,
            }
        );
    }

    #[test]
    fn setup_authority_requires_exact_baseline_and_target_revisions() {
        let stack = empty_stack();
        let digest = stack.setup_update_digest();
        let mut authority = authorization(&stack);
        assert_eq!(
            setup_update_authorization_mismatches(Some(&stack), &stack, &authority),
            Vec::<String>::new()
        );

        authority.baseline_frozen_digest = "different".to_string();
        assert_eq!(
            setup_update_authorization_mismatches(Some(&stack), &stack, &authority),
            vec![format!("baseline: computed {digest}, recorded different")]
        );

        authority = authorization(&stack);
        authority.target_frozen_digest = "different".to_string();
        assert_eq!(
            setup_update_authorization_mismatches(Some(&stack), &stack, &authority),
            vec![format!("target: computed {digest}, recorded different")]
        );
        assert_eq!(
            setup_update_authorization_mismatches(None, &stack, &authority),
            vec![
                "baseline: no installed stack to compare".to_string(),
                format!("target: computed {digest}, recorded different"),
            ]
        );
    }

    #[cfg(feature = "runtime-checks")]
    fn sandbox_stack(
        lifecycle: alien_core::ResourceLifecycle,
        image: &str,
        private_base_image: Option<&str>,
    ) -> Stack {
        let sandbox = alien_core::Sandbox::new("agents".to_string())
            .code(alien_core::SandboxCode::Image {
                image: image.to_string(),
                bundled_from: None,
            })
            .maybe_private_base_image(private_base_image.map(str::to_string))
            .egress(alien_core::SandboxEgress::Internet)
            .lifecycle(alien_core::SandboxLifecyclePolicy {
                max_lifetime_seconds: None,
                idle_pause_seconds: None,
            })
            .build();
        Stack::new("stack".to_string())
            .add(sandbox, lifecycle)
            .build()
    }

    /// A Live sandbox beside the Frozen network a deny connector attaches to.
    #[cfg(feature = "runtime-checks")]
    fn live_sandbox_stack(
        image: &str,
        private_base_image: Option<&str>,
        egress: alien_core::SandboxEgress,
    ) -> Stack {
        let mut stack = sandbox_stack(
            alien_core::ResourceLifecycle::Live,
            image,
            private_base_image,
        );
        stack.resources.shift_insert(
            0,
            "net".to_string(),
            alien_core::ResourceEntry {
                config: alien_core::Resource::new(
                    alien_core::Network::new("net".to_string())
                        .settings(alien_core::NetworkSettings::Create {
                            cidr: Some("10.0.0.0/16".to_string()),
                            availability_zones: 2,
                        })
                        .build(),
                ),
                lifecycle: alien_core::ResourceLifecycle::Frozen,
                dependencies: vec![],
                display_name: None,
                description: None,
                remote_access: false,
                enabled_when: None,
            },
        );
        let sandbox = stack.resources.get_mut("agents").expect("sandbox");
        let mut config = sandbox
            .config
            .downcast_ref::<alien_core::Sandbox>()
            .expect("sandbox")
            .clone();
        config.egress = egress;
        sandbox.config = alien_core::Resource::new(config);
        stack
    }

    fn deployment_config() -> DeploymentConfig {
        DeploymentConfig::builder()
            .stack_settings(alien_core::StackSettings::default())
            .environment_variables(alien_core::EnvironmentVariablesSnapshot {
                variables: vec![],
                hash: String::new(),
                created_at: String::new(),
            })
            .allow_frozen_changes(false)
            .external_bindings(alien_core::ExternalBindings::default())
            .build()
    }

    /// A blocked setup input change must clear on the setup rerun: the rerun's authorization is
    /// minted from the digests of the installed and target stacks, so those must differ.
    #[cfg(feature = "runtime-checks")]
    #[tokio::test]
    async fn a_live_sandbox_setup_input_change_blocks_until_setup_reruns() {
        use alien_core::SandboxEgress::{Internet, None as NoEgress};
        const BASE_A: &str = "123456789012.dkr.ecr.us-east-1.amazonaws.com/team/base-a:1";
        const V1: &str = "s3://bucket/sandbox-bundle/v1/bundle.zip";
        const V2: &str = "s3://bucket/sandbox-bundle/v2/bundle.zip";
        let old = live_sandbox_stack(V1, Some(BASE_A), Internet);
        let runner = PreflightRunner::with_registry({
            let mut registry = crate::PreflightRegistry::new();
            registry.add_compatibility_check(Box::new(
                crate::compatibility::SandboxSetupInputsUnchangedCheck,
            ));
            registry
        });
        let config = deployment_config();
        let state = StackState::new(Platform::Aws);
        let client = ClientConfig::Local {
            state_directory: "/unused".to_string(),
        };

        let tag_only = live_sandbox_stack(
            V2,
            Some("123456789012.dkr.ecr.us-east-1.amazonaws.com/team/base-a:2"),
            Internet,
        );
        runner
            .run_deployment_time_preflights(
                tag_only,
                &state,
                &config,
                &client,
                Some(&old),
                None,
                None,
            )
            .await
            .expect("a new tag and bundle roll without setup");

        for (target, input) in [
            (
                live_sandbox_stack(
                    "s3://bucket/other-prefix/v2/bundle.zip",
                    Some(BASE_A),
                    Internet,
                ),
                "build role policy",
            ),
            (live_sandbox_stack(V2, Some(BASE_A), NoEgress), "egress"),
        ] {
            let error = runner
                .run_deployment_time_preflights(
                    target.clone(),
                    &state,
                    &config,
                    &client,
                    Some(&old),
                    None,
                    None,
                )
                .await
                .expect_err("a setup input change needs setup");
            assert_eq!(error.code, "DEPLOYMENT_SETUP_REQUIRED");
            assert!(
                error.message.contains("agents") && error.message.contains(input),
                "{input}: {}",
                error.message
            );

            let rerun = SetupUpdateAuthorization {
                nonce: "revision".to_string(),
                baseline_frozen_digest: old.setup_update_digest(),
                target_frozen_digest: target.setup_update_digest(),
                release_id: "release".to_string(),
                setup_target: "target".to_string(),
                setup_fingerprint: "fingerprint".to_string(),
                setup_fingerprint_version: 1,
            };
            assert_ne!(rerun.baseline_frozen_digest, rerun.target_frozen_digest);
            let (_, _, authorized) = runner
                .run_deployment_time_preflights(
                    target,
                    &state,
                    &config,
                    &client,
                    Some(&old),
                    Some(&rerun),
                    None,
                )
                .await
                .expect("the setup rerun applies the blocked update");
            assert!(authorized, "{input}");
        }
    }

    /// The AWS sandbox setup inputs (build role, network, grant) do not exist on Kubernetes: a
    /// Live sandbox there rolls its image, limits and lifecycle without setup, and only its
    /// egress mode (the chart's NetworkPolicy) waits for it.
    #[cfg(feature = "runtime-checks")]
    #[tokio::test]
    async fn a_live_kubernetes_sandbox_rolls_image_limits_and_lifecycle_without_setup() {
        use alien_core::{SandboxCode, SandboxLifecyclePolicy, SandboxLimits};
        let stack = |image: &str, memory: &str, max_lifetime: Option<u32>, egress| {
            let sandbox = alien_core::Sandbox::new("agents".to_string())
                .code(SandboxCode::Image {
                    image: image.to_string(),
                    bundled_from: None,
                })
                .limits(SandboxLimits {
                    cpu: "1".to_string(),
                    memory: memory.to_string(),
                    disk: "10Gi".to_string(),
                    max_processes: None,
                })
                .egress(egress)
                .lifecycle(SandboxLifecyclePolicy {
                    max_lifetime_seconds: max_lifetime,
                    idle_pause_seconds: None,
                })
                .build();
            Stack::new("stack".to_string())
                .add(sandbox, alien_core::ResourceLifecycle::Live)
                .build()
        };
        let runner = PreflightRunner::with_registry({
            let mut registry = crate::PreflightRegistry::new();
            registry.add_compatibility_check(Box::new(
                crate::compatibility::SandboxSetupInputsUnchangedCheck,
            ));
            registry
        });
        let config = deployment_config();
        let state = StackState::new(Platform::Kubernetes);
        let client = ClientConfig::Local {
            state_directory: "/unused".to_string(),
        };
        let old = stack("app:1", "2Gi", None, alien_core::SandboxEgress::None);

        runner
            .run_deployment_time_preflights(
                stack("app:2", "4Gi", Some(3600), alien_core::SandboxEgress::None),
                &state,
                &config,
                &client,
                Some(&old),
                None,
                None,
            )
            .await
            .expect("image, limits and lifetime roll at runtime");

        let error = runner
            .run_deployment_time_preflights(
                stack("app:1", "2Gi", None, alien_core::SandboxEgress::Internet),
                &state,
                &config,
                &client,
                Some(&old),
                None,
                None,
            )
            .await
            .expect_err("the egress mode is the chart's NetworkPolicy");
        assert_eq!(error.code, "DEPLOYMENT_SETUP_REQUIRED");
    }

    /// The Frozen check is built from the installed stack's platform, so only an Azure or GCP
    /// install may roll a Frozen sandbox's image without setup.
    #[cfg(feature = "runtime-checks")]
    #[tokio::test]
    async fn azure_and_gcp_frozen_sandboxes_roll_their_image_without_setup() {
        let old = sandbox_stack(alien_core::ResourceLifecycle::Frozen, "ubuntu", None);
        let target = sandbox_stack(alien_core::ResourceLifecycle::Frozen, "debian", None);
        let runner = PreflightRunner::with_registry(crate::PreflightRegistry::new());
        let config = deployment_config();
        let client = ClientConfig::Local {
            state_directory: "/unused".to_string(),
        };

        for platform in [Platform::Azure, Platform::Gcp] {
            runner
                .run_compatibility_checks(&old, &target, &config, platform)
                .await
                .map(|summary| assert!(summary.success, "{platform}: {:?}", summary.results))
                .expect("checks run");
        }
        let on_aws = runner
            .run_compatibility_checks(&old, &target, &config, Platform::Aws)
            .await
            .expect("checks run");
        assert!(!on_aws.success, "an AWS Frozen image change needs setup");

        for platform in [Platform::Azure, Platform::Gcp] {
            runner
                .run_deployment_time_preflights(
                    target.clone(),
                    &StackState::new(platform),
                    &config,
                    &client,
                    Some(&old),
                    None,
                    None,
                )
                .await
                .unwrap_or_else(|error| {
                    panic!("a {platform} deployment rolls the image: {error:?}")
                });
        }
        let error = runner
            .run_deployment_time_preflights(
                target,
                &StackState::new(Platform::Local),
                &config,
                &client,
                Some(&old),
                None,
                None,
            )
            .await
            .expect_err("the deployment's own platform is neither Azure nor GCP");
        assert_eq!(error.code, "DEPLOYMENT_SETUP_REQUIRED");
    }
}

/// Checks that need the stack prepared as its setup packages prepare it, so they run only once
/// the stack passed every other check. Like build-time checks, they apply to new releases and
/// never to rendering a setup package for a stack released earlier.
async fn new_release_checks(stack: &Stack, platform: Platform) -> Result<()> {
    if platform == Platform::Aws {
        crate::compile_time::aws_permissions_boundary_size::check_aws_permissions_boundary_size(
            stack,
        )
        .await?;
    }
    Ok(())
}

/// Run stack checks that need no cloud access, logging each failure and warning.
async fn run_stack_checks(
    checks: Vec<&dyn CompileTimeCheck>,
    stack: &Stack,
    platform: Platform,
) -> Result<Vec<CheckResult>> {
    let mut platform_stack = stack.clone();
    alien_core::remove_off_platform_gated_resources(&mut platform_stack, platform, None);
    let stack = &platform_stack;
    let mut results = Vec::new();

    for check in checks {
        debug!("Running check: {}", check.description());

        let mut result =
            check
                .check(stack, platform)
                .await
                .context(ErrorData::CompileTimeCheckFailed {
                    check_name: check.description().to_string(),
                    message: "Check execution failed".to_string(),
                    resource_id: None,
                })?;

        result = result.with_check_metadata(check.code(), check.description());

        if !result.success {
            error!(check = %check.description(), "Compile-time check failed");
            for msg in &result.errors {
                error!(check = %check.description(), "  {}", msg);
            }
        }

        for warning in &result.warnings {
            warn!(check = %check.description(), "  Warning: {}", warning);
        }

        results.push(result);
    }

    Ok(results)
}

/// Reject scope narrowing before setup authorization can allow an installed resource's removal.
pub fn validate_gate_platform_changes(old: &Stack, new: &Stack, platform: Platform) -> Result<()> {
    validate_installed_gate_platform_changes(old.resources.keys(), new, platform)
}

fn validate_installed_gate_platform_changes<'a>(
    installed_ids: impl Iterator<Item = &'a String>,
    new: &Stack,
    platform: Platform,
) -> Result<()> {
    for id in installed_ids {
        let Some(input_id) = new
            .resources
            .get(id)
            .and_then(|entry| entry.enabled_when.as_deref())
        else {
            continue;
        };
        if alien_core::gate_is_off_platform(&new.inputs, input_id, platform) {
            return Err(AlienError::new(ErrorData::StackCompatibilityCheckFailed {
                check_name: "GATE_PLATFORM_SCOPE_NARROWED".into(),
                message: format!(
                    "Cannot exclude platform '{platform}' from input '{input_id}': gated resource \
                     '{id}' is already installed. Keep this platform in the input's platforms; \
                     remove the resource explicitly in a separate release if deletion is intended."
                ),
                old_resource_id: Some(id.clone()),
                new_resource_id: Some(id.clone()),
            }));
        }
    }
    Ok(())
}
