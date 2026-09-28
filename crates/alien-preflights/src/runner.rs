use crate::error::{ErrorData, Result};
use crate::{PreflightRegistry, PreflightSummary};
use alien_core::{DeploymentConfig, Platform, Stack, StackState};
use alien_error::{AlienError, Context};
use tracing::{debug, error, info, warn};

#[cfg(feature = "runtime-checks")]
use crate::CheckResult;
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
                .check_with_config(old_stack, new_stack, config)
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
        let direct_setup = setup_authority == Some(alien_core::InitialSetupAuthority::DirectSetup);
        let hashed_target = match old_stack {
            Some(_) if !direct_setup => Some(without_declined_live_resources(
                &mutated_stack,
                &config.input_values,
                platform,
            )?),
            _ => None,
        };
        let setup_update_authorized = direct_setup
            || hashed_target.as_ref().is_some_and(|target| {
                setup_update_authorization.is_some_and(|authorization| {
                    setup_update_authorization_matches(old_stack, target, authorization)
                })
            });

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
                if !compatibility_summary.success && all_results.iter().all(|result| result.success)
                {
                    return Err(AlienError::new(ErrorData::SetupRequired {
                        message: compatibility_summary
                            .results
                            .iter()
                            .flat_map(|result| result.errors.iter().cloned())
                            .collect::<Vec<_>>()
                            .join("; "),
                    }));
                }
                all_results.extend(compatibility_summary.results);
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
    let declined =
        alien_core::declined_live_resources(stack, input_values, &answers, &still_frozen_gating)
            .map_err(|message| {
                AlienError::new(ErrorData::DeploymentPrerequisiteCheckFailed {
                    check_name: "Every runtime gate resolves to a boolean".to_string(),
                    message,
                    platform: Some(platform.to_string()),
                })
            })?;
    let mut projected = stack.clone();
    alien_core::remove_declined_resources(&mut projected, &declined);
    Ok(projected)
}

fn setup_update_authorization_matches(
    old_stack: Option<&Stack>,
    target_stack: &Stack,
    authorization: &alien_core::SetupUpdateAuthorization,
) -> bool {
    old_stack.is_some_and(|old_stack| {
        old_stack.setup_owned_digest() == authorization.baseline_frozen_digest
    }) && target_stack.setup_owned_digest() == authorization.target_frozen_digest
}

impl Default for PreflightRunner {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod setup_update_authorization_tests {
    use super::*;
    use alien_core::{PermissionsConfig, SetupUpdateAuthorization};
    use indexmap::IndexMap;

    fn empty_stack() -> Stack {
        Stack {
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
            id: "stack".to_string(),
            resources: IndexMap::new(),
            inputs: vec![],
            permissions: PermissionsConfig::default(),
            supported_platforms: None,
        }
    }

    fn authorization(stack: &Stack) -> SetupUpdateAuthorization {
        SetupUpdateAuthorization {
            nonce: "revision".to_string(),
            baseline_frozen_digest: stack.setup_owned_digest(),
            target_frozen_digest: stack.setup_owned_digest(),
            release_id: "release".to_string(),
            setup_target: "target".to_string(),
            setup_fingerprint: "fingerprint".to_string(),
            setup_fingerprint_version: 1,
        }
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

    #[test]
    fn setup_authority_requires_exact_baseline_and_target_revisions() {
        let stack = empty_stack();
        let mut authority = authorization(&stack);
        assert!(setup_update_authorization_matches(
            Some(&stack),
            &stack,
            &authority
        ));

        authority.baseline_frozen_digest = "different".to_string();
        assert!(!setup_update_authorization_matches(
            Some(&stack),
            &stack,
            &authority
        ));

        authority = authorization(&stack);
        authority.target_frozen_digest = "different".to_string();
        assert!(!setup_update_authorization_matches(
            Some(&stack),
            &stack,
            &authority
        ));
        assert!(!setup_update_authorization_matches(
            None, &stack, &authority
        ));
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
            })
            .maybe_private_base_image(private_base_image.map(str::to_string))
            .egress(alien_core::SandboxEgress::Allow)
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

    #[cfg(feature = "runtime-checks")]
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

    /// A blocked repository change must clear on the setup rerun: the rerun's authorization is
    /// minted from the digests of the installed and target stacks, so those must differ.
    #[cfg(feature = "runtime-checks")]
    #[tokio::test]
    async fn a_live_sandbox_setup_input_change_blocks_until_setup_reruns() {
        use alien_core::SandboxEgress::{Allow, Deny};
        const BASE_A: &str = "123456789012.dkr.ecr.us-east-1.amazonaws.com/team/base-a:1";
        const V1: &str = "s3://bucket/sandbox-bundle/v1/bundle.zip";
        const V2: &str = "s3://bucket/sandbox-bundle/v2/bundle.zip";
        let old = live_sandbox_stack(V1, Some(BASE_A), Allow);
        let runner = PreflightRunner::with_registry({
            let mut registry = crate::PreflightRegistry::new();
            registry.add_compatibility_check(Box::new(
                crate::compatibility::SandboxSetupInputsUnchangedCheck,
            ));
            registry
        });
        let config = deployment_config();
        let state = StackState::new(Platform::Local);
        let client = ClientConfig::Local {
            state_directory: "/unused".to_string(),
        };

        let tag_only = live_sandbox_stack(
            V2,
            Some("123456789012.dkr.ecr.us-east-1.amazonaws.com/team/base-a:2"),
            Allow,
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
                    V2,
                    Some("123456789012.dkr.ecr.us-east-1.amazonaws.com/team/base-b:1"),
                    Allow,
                ),
                "build role policy",
            ),
            (live_sandbox_stack(V2, Some(BASE_A), Deny), "egress"),
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
                baseline_frozen_digest: old.setup_owned_digest(),
                target_frozen_digest: target.setup_owned_digest(),
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

    /// The Frozen check is built from the installed stack's platform, so only an Azure install
    /// may roll a Frozen sandbox's image without setup.
    #[cfg(feature = "runtime-checks")]
    #[tokio::test]
    async fn only_an_azure_frozen_sandbox_rolls_its_image_without_setup() {
        let old = sandbox_stack(alien_core::ResourceLifecycle::Frozen, "ubuntu", None);
        let target = sandbox_stack(alien_core::ResourceLifecycle::Frozen, "debian", None);
        let runner = PreflightRunner::with_registry(crate::PreflightRegistry::new());
        let config = deployment_config();
        let client = ClientConfig::Local {
            state_directory: "/unused".to_string(),
        };

        runner
            .run_compatibility_checks(&old, &target, &config, Platform::Azure)
            .await
            .map(|summary| assert!(summary.success, "{:?}", summary.results))
            .expect("checks run");
        let on_aws = runner
            .run_compatibility_checks(&old, &target, &config, Platform::Aws)
            .await
            .expect("checks run");
        assert!(!on_aws.success, "an AWS Frozen image change needs setup");

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
            .expect_err("the deployment's own platform is not Azure");
        assert_eq!(error.code, "DEPLOYMENT_SETUP_REQUIRED");
    }
}
