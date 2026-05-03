use crate::error::{ErrorData, Result};
use crate::{CheckResult, PreflightRegistry, PreflightSummary};
use alien_core::{ClientConfig, DeploymentConfig, Platform, Stack, StackState};
use alien_error::{AlienError, Context};
use tracing::{debug, error, info, warn};

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

            // Set the check description in the result
            result.check_description = Some(check.description().to_string());

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

        Ok(PreflightSummary::from_results(results))
    }

    /// Run stack compatibility checks between two stacks
    pub async fn run_compatibility_checks(
        &self,
        old_stack: &Stack,
        new_stack: &Stack,
    ) -> Result<PreflightSummary> {
        info!("Running stack compatibility checks");

        let checks = self.registry.get_compatibility_checks();
        let mut results = Vec::new();

        for check in checks {
            debug!("Running compatibility check: {}", check.description());

            let mut result = check.check(old_stack, new_stack).await.context(
                ErrorData::StackCompatibilityCheckFailed {
                    check_name: check.description().to_string(),
                    message: "Compatibility check execution failed".to_string(),
                    old_resource_id: None,
                    new_resource_id: None,
                },
            )?;

            // Set the check description in the result
            result.check_description = Some(check.description().to_string());

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

            // Set the check description in the result
            result.check_description = Some(check.description().to_string());

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

        Ok(current_stack)
    }

    /// Run template-generation preflights (skips deployment prerequisite checks).
    ///
    /// Used by CloudFormation template generation where deployment-environment checks
    /// (Horizon required, DNS/TLS required) are irrelevant — the platform provisions
    /// that infrastructure separately.
    pub async fn run_template_preflights(
        &self,
        stack: &Stack,
        platform: Platform,
    ) -> Result<PreflightSummary> {
        info!(
            "Running template-generation preflights for platform {:?}",
            platform
        );

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

            result.check_description = Some(check.description().to_string());

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

    /// Run the complete preflight pipeline for build-time (compile-time checks only)
    pub async fn run_build_time_preflights(
        &self,
        stack: &Stack,
        platform: Platform,
    ) -> Result<PreflightSummary> {
        info!("Running build-time preflights for platform {:?}", platform);

        // Run compile-time checks only - mutations are now deployment-time only
        let check_summary = self.run_compile_time_checks(stack, platform).await?;

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

        Ok(check_summary)
    }

    /// Run deployment-time preflights (compile-time checks + mutations + compatibility checks + runtime checks)
    ///
    /// The order is critical:
    /// 1. Compile-time checks on user-provided stack (fast validation)
    /// 2. Apply mutations to add infrastructure resources
    /// 3. Compatibility checks on mutated stacks (detects frozen resource changes)
    /// 4. Runtime checks on mutated stack (cloud API validation)
    #[cfg(feature = "runtime-checks")]
    pub async fn run_deployment_time_preflights(
        &self,
        stack: Stack,
        stack_state: &StackState,
        config: &DeploymentConfig,
        client_config: &ClientConfig,
        old_stack: Option<&Stack>,
        skip_frozen_check: bool,
    ) -> Result<(Stack, PreflightSummary)> {
        let platform = stack_state.platform;
        info!(
            "Running deployment-time preflights for platform {:?}",
            platform
        );

        let mut all_results: Vec<CheckResult> = Vec::new();

        // Run compile-time checks first (fast, no cloud API calls)
        let compile_summary = self.run_compile_time_checks(&stack, platform).await?;
        all_results.extend(compile_summary.results);

        // Apply mutations BEFORE compatibility checks
        // This ensures compatibility checks compare mutated stacks (old mutated vs new mutated)
        let mutated_stack = self.apply_mutations(stack, stack_state, config).await?;

        // Run compatibility checks on mutated stack if old stack is provided
        // This detects if mutations added frozen resources during updates
        // Skip the check if allow_frozen_changes flag is set
        if let Some(old_stack) = old_stack {
            if !skip_frozen_check {
                let compatibility_summary = self
                    .run_compatibility_checks(old_stack, &mutated_stack)
                    .await?;
                all_results.extend(compatibility_summary.results);
            } else {
                info!("Skipping frozen resource compatibility checks (allow_frozen_changes=true)");
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

        Ok((mutated_stack, summary))
    }
}

impl Default for PreflightRunner {
    fn default() -> Self {
        Self::new()
    }
}
