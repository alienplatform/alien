pub mod compatibility;
pub mod compile_time;
pub mod error;
pub mod mutations;
pub mod runner;
pub mod runtime;

use crate::error::Result;
use alien_core::{ClientConfig, DeploymentConfig, Platform, Stack, StackState};
use serde::{Deserialize, Serialize};

#[cfg(feature = "openapi")]
use utoipa::ToSchema;

/// Result of a preflight check
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct CheckResult {
    /// Description of the check
    pub check_description: Option<String>,
    /// Whether the check passed
    pub success: bool,
    /// Error messages (if any)
    pub errors: Vec<String>,
    /// Warning messages (if any)
    pub warnings: Vec<String>,
}

impl CheckResult {
    /// Create a successful check result
    pub fn success() -> Self {
        Self {
            success: true,
            errors: Vec::new(),
            warnings: Vec::new(),
            check_description: None,
        }
    }

    /// Create a check result with warnings
    pub fn with_warnings(warnings: Vec<String>) -> Self {
        Self {
            success: true,
            errors: Vec::new(),
            warnings,
            check_description: None,
        }
    }

    /// Create a failed check result
    pub fn failed(errors: Vec<String>) -> Self {
        Self {
            success: false,
            errors,
            warnings: Vec::new(),
            check_description: None,
        }
    }

    /// Create a failed check result with warnings
    pub fn failed_with_warnings(errors: Vec<String>, warnings: Vec<String>) -> Self {
        Self {
            success: false,
            errors,
            warnings,
            check_description: None,
        }
    }

    /// Add an error to the result
    pub fn add_error(&mut self, error: String) {
        self.errors.push(error);
        self.success = false;
    }

    /// Add a warning to the result
    pub fn add_warning(&mut self, warning: String) {
        self.warnings.push(warning);
    }

    /// Merge another check result into this one
    pub fn merge(&mut self, other: CheckResult) {
        self.errors.extend(other.errors);
        self.warnings.extend(other.warnings);
        if !other.success {
            self.success = false;
        }
    }

    /// Set the check description for this result
    pub fn with_check_description(mut self, check_description: String) -> Self {
        self.check_description = Some(check_description);
        self
    }
}

/// Validates stack configuration without requiring cloud access.
/// Can run during build time for early error detection.
#[async_trait::async_trait]
pub trait CompileTimeCheck: Send + Sync {
    /// User-facing description of what this check validates
    fn description(&self) -> &'static str;

    /// Condition to determine if this check should run
    fn should_run(&self, stack: &Stack, platform: Platform) -> bool;

    /// Run the check without cloud access
    async fn check(&self, stack: &Stack, platform: Platform) -> Result<CheckResult>;
}

/// Validates cloud environment readiness with cloud client access.
/// Only runs during deployment time.
#[cfg(feature = "runtime-checks")]
#[async_trait::async_trait]
pub trait RuntimeCheck: Send + Sync {
    /// User-facing description of what this check validates
    fn description(&self) -> &'static str;

    /// Condition to determine if this check should run
    fn should_run(&self, stack: &Stack, platform: Platform) -> bool;

    /// Run the check with cloud client access
    async fn check(
        &self,
        stack: &Stack,
        stack_state: &StackState,
        config: &DeploymentConfig,
        client_config: &ClientConfig,
    ) -> Result<CheckResult>;
}

/// Validates compatibility between old and new stack configurations.
/// Runs during stack updates to prevent breaking changes.
#[async_trait::async_trait]
pub trait StackCompatibilityCheck: Send + Sync {
    /// User-facing description of what this check validates
    fn description(&self) -> &'static str;

    /// Compare old and new stacks for compatibility
    async fn check(&self, old_stack: &Stack, new_stack: &Stack) -> Result<CheckResult>;
}

/// Modifies the stack to ensure successful deployment.
/// Always runs at deployment time but does NOT query cloud state.
/// Mutations should only modify the stack based on stack configuration and stack state.
#[async_trait::async_trait]
pub trait StackMutation: Send + Sync {
    /// User-facing description of what this mutation does
    fn description(&self) -> &'static str;

    /// Condition to determine if this mutation should run
    fn should_run(
        &self,
        stack: &Stack,
        stack_state: &StackState,
        config: &DeploymentConfig,
    ) -> bool;

    /// Apply the mutation to the stack
    async fn mutate(
        &self,
        stack: Stack,
        stack_state: &StackState,
        config: &DeploymentConfig,
    ) -> Result<Stack>;
}

/// Registry of all available checks and mutations
pub struct PreflightRegistry {
    compile_time_checks: Vec<Box<dyn CompileTimeCheck>>,
    #[cfg(feature = "runtime-checks")]
    runtime_checks: Vec<Box<dyn RuntimeCheck>>,
    compatibility_checks: Vec<Box<dyn StackCompatibilityCheck>>,
    mutations: Vec<Box<dyn StackMutation>>,
}

impl PreflightRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        Self {
            compile_time_checks: Vec::new(),
            #[cfg(feature = "runtime-checks")]
            runtime_checks: Vec::new(),
            compatibility_checks: Vec::new(),
            mutations: Vec::new(),
        }
    }

    /// Create a registry with built-in checks and mutations
    pub fn with_built_ins() -> Self {
        let mut registry = Self::new();

        // Add compile-time checks
        registry.add_compile_time_check(Box::new(compile_time::AllowedUserResourcesCheck));
        registry.add_compile_time_check(Box::new(compile_time::UniqueResourcesCheck));
        registry.add_compile_time_check(Box::new(compile_time::FrozenResourceLifecycleCheck));
        registry.add_compile_time_check(Box::new(compile_time::ContainerLifecycleCheck));
        registry.add_compile_time_check(Box::new(compile_time::PublicFunctionLifecycleCheck));
        registry.add_compile_time_check(Box::new(compile_time::ValidResourceDependenciesCheck));
        registry.add_compile_time_check(Box::new(compile_time::ResourceReferencesExistCheck));
        registry.add_compile_time_check(Box::new(compile_time::SingleQueueTriggerCheck));
        registry.add_compile_time_check(Box::new(
            compile_time::ServiceAccountImpersonateValidationCheck,
        ));
        registry.add_compile_time_check(Box::new(compile_time::NetworkSettingsPlatformCheck));
        registry.add_compile_time_check(Box::new(compile_time::PublicSubnetsRequiredCheck));
        registry.add_compile_time_check(Box::new(compile_time::PermissionProfilesExistCheck));
        registry.add_compile_time_check(Box::new(compile_time::SingleExposedPortCheck));
        registry.add_compile_time_check(Box::new(compile_time::ResourceNameLengthCheck));
        registry.add_compile_time_check(Box::new(compile_time::ResourceIdPatternCheck));
        registry.add_compile_time_check(Box::new(compile_time::CapacityGroupProfileCheck));
        registry.add_compile_time_check(Box::new(compile_time::SaasRequiredCheck));

        // Add compatibility checks
        registry.add_compatibility_check(Box::new(compatibility::PermissionProfilesUnchangedCheck));
        registry.add_compatibility_check(Box::new(compatibility::FrozenResourcesUnchangedCheck));

        // Add runtime checks
        #[cfg(feature = "runtime-checks")]
        {
            // TODO: Add runtime checks
            // registry.add_runtime_check(Box::new(runtime::CloudConnectivityCheck));
            // registry.add_runtime_check(Box::new(runtime::CloudQuotasCheck));
            // registry.add_runtime_check(Box::new(runtime::CloudApisEnabledCheck));
            // registry.add_runtime_check(Box::new(runtime::SufficientPermissionsCheck));
        }

        // Add mutations in order of execution.
        //
        // Mutations are evaluated incrementally: each mutation's should_run() is checked
        // against the current (already-mutated) stack, not the original. This is critical
        // because some mutations create resources (e.g., SecretsVaultMutation adds a vault)
        // that later mutations need to see.
        //
        // The ordering follows four phases:
        //
        //   Phase 1 — Global infrastructure (always needed, no resource-type checks)
        //   Phase 2 — Resource creation (adds new resources based on user config)
        //   Phase 3 — Platform infrastructure & service activations (react to resource types)
        //   Phase 4 — Dependency wiring (must run last, after all resources exist)
        //
        // Phase 3 MUST run after Phase 2 so it can see all resource types, including
        // mutation-created ones. For example, GcpServiceActivationMutation needs to see the
        // vault created by SecretsVaultMutation to add enable-secret-manager.
        //
        // Within each phase, order doesn't affect correctness — provisioning order is
        // determined by the dependency graph, not mutation order.

        // Phase 1: Global infrastructure
        registry.add_mutation(Box::new(mutations::NetworkMutation));
        registry.add_mutation(Box::new(mutations::AzureResourceGroupMutation));

        // Phase 2: Resource creation
        registry.add_mutation(Box::new(mutations::ContainerClusterMutation));
        registry.add_mutation(Box::new(mutations::RemoteStackManagementMutation));
        registry.add_mutation(Box::new(mutations::ManagementPermissionProfileMutation));
        registry.add_mutation(Box::new(mutations::ServiceAccountMutation));
        registry.add_mutation(Box::new(mutations::SecretsVaultMutation));

        // Phase 3: Service activations and platform infrastructure
        // These scan resource types to decide what to create, so they must see all
        // resources from Phase 2 (vault, etc.)
        registry.add_mutation(Box::new(mutations::AzureServiceActivationMutation));
        registry.add_mutation(Box::new(mutations::GcpServiceActivationMutation));
        registry.add_mutation(Box::new(mutations::AzureContainerAppsEnvironmentMutation));
        registry.add_mutation(Box::new(mutations::AzureServiceBusNamespaceMutation));
        registry.add_mutation(Box::new(mutations::AzureStorageAccountMutation));

        // Phase 4: Dependency wiring (must be last, after all resources exist)
        registry.add_mutation(Box::new(mutations::ServiceAccountDependenciesMutation));
        registry.add_mutation(Box::new(mutations::InfrastructureDependenciesMutation));

        registry
    }

    /// Add a compile-time check
    pub fn add_compile_time_check(&mut self, check: Box<dyn CompileTimeCheck>) {
        self.compile_time_checks.push(check);
    }

    /// Add a runtime check
    #[cfg(feature = "runtime-checks")]
    pub fn add_runtime_check(&mut self, check: Box<dyn RuntimeCheck>) {
        self.runtime_checks.push(check);
    }

    /// Add a compatibility check
    pub fn add_compatibility_check(&mut self, check: Box<dyn StackCompatibilityCheck>) {
        self.compatibility_checks.push(check);
    }

    /// Add a mutation
    pub fn add_mutation(&mut self, mutation: Box<dyn StackMutation>) {
        self.mutations.push(mutation);
    }

    /// Get compile-time checks that should run for the given stack and platform
    pub fn get_compile_time_checks(
        &self,
        stack: &Stack,
        platform: Platform,
    ) -> Vec<&dyn CompileTimeCheck> {
        self.compile_time_checks
            .iter()
            .filter(|check| check.should_run(stack, platform))
            .map(|check| check.as_ref())
            .collect()
    }

    /// Get runtime checks that should run for the given stack and platform
    #[cfg(feature = "runtime-checks")]
    pub fn get_runtime_checks(&self, stack: &Stack, platform: Platform) -> Vec<&dyn RuntimeCheck> {
        self.runtime_checks
            .iter()
            .filter(|check| check.should_run(stack, platform))
            .map(|check| check.as_ref())
            .collect()
    }

    /// Get all compatibility checks
    pub fn get_compatibility_checks(&self) -> Vec<&dyn StackCompatibilityCheck> {
        self.compatibility_checks
            .iter()
            .map(|check| check.as_ref())
            .collect()
    }

    /// Get mutations that should run for the given stack, stack state, and config.
    ///
    /// NOTE: This filters against a single snapshot of the stack. For incremental
    /// evaluation (where each mutation sees the stack modified by prior mutations),
    /// use `get_all_mutations()` and check `should_run()` in the execution loop.
    pub fn get_mutations(
        &self,
        stack: &Stack,
        stack_state: &StackState,
        config: &DeploymentConfig,
    ) -> Vec<&dyn StackMutation> {
        self.mutations
            .iter()
            .filter(|mutation| mutation.should_run(stack, stack_state, config))
            .map(|mutation| mutation.as_ref())
            .collect()
    }

    /// Get all registered mutations in execution order, without filtering.
    ///
    /// Used by the runner to evaluate `should_run()` incrementally against the
    /// current stack state after each mutation has been applied.
    pub fn get_all_mutations(&self) -> Vec<&dyn StackMutation> {
        self.mutations
            .iter()
            .map(|mutation| mutation.as_ref())
            .collect()
    }
}

impl Default for PreflightRegistry {
    fn default() -> Self {
        Self::with_built_ins()
    }
}

/// Summary of all preflight results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreflightSummary {
    /// Number of checks that passed
    pub passed_checks: usize,
    /// Number of checks that failed
    pub failed_checks: usize,
    /// Total number of warnings
    pub warning_count: usize,
    /// All check results
    pub results: Vec<CheckResult>,
    /// Whether all checks passed
    pub success: bool,
}

impl PreflightSummary {
    /// Create a new preflight summary from results
    pub fn from_results(results: Vec<CheckResult>) -> Self {
        let passed_checks = results.iter().filter(|r| r.success).count();
        let failed_checks = results.iter().filter(|r| !r.success).count();
        let warning_count = results.iter().map(|r| r.warnings.len()).sum();
        let success = failed_checks == 0;

        Self {
            passed_checks,
            failed_checks,
            warning_count,
            results,
            success,
        }
    }

    /// Get all error messages
    pub fn get_errors(&self) -> Vec<String> {
        self.results
            .iter()
            .flat_map(|result| {
                let name = result
                    .check_description
                    .as_deref()
                    .unwrap_or("Unknown check");
                result
                    .errors
                    .iter()
                    .map(move |err| format!("{}: {}", name, err))
            })
            .collect()
    }

    /// Get all warning messages
    pub fn get_warnings(&self) -> Vec<String> {
        self.results
            .iter()
            .flat_map(|result| {
                let name = result
                    .check_description
                    .as_deref()
                    .unwrap_or("Unknown check");
                result
                    .warnings
                    .iter()
                    .map(move |warn| format!("{}: {}", name, warn))
            })
            .collect()
    }
}
