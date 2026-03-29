# Preflights

## What are Preflights?

Preflights run before deployment to validate the stack and prepare it for provisioning.

They do two things:
1. **Checks** - validate the stack configuration
2. **Mutations** - add required infrastructure resources

Why? Terraform and CloudFormation give errors mid-deployment. You find out something is wrong after resources are half-created. For Alien deployments into remote environments you don't control, this is unacceptable. Preflights catch problems early and give a complete picture of everything that needs fixing.

## Checks vs Mutations

**Checks** validate. They don't modify the stack. If a check fails, deployment stops.

```rust
pub trait CompileTimeCheck: Send + Sync {
    fn description(&self) -> &'static str;
    fn should_run(&self, stack: &Stack, platform: Platform) -> bool;
    async fn check(&self, stack: &Stack, platform: Platform) -> Result<CheckResult>;
}
```

**Mutations** modify the stack. They add infrastructure resources that the user shouldn't define manually.

```rust
pub trait StackMutation: Send + Sync {
    fn description(&self) -> &'static str;
    fn should_run(&self, stack: &Stack, stack_state: &StackState, config: &DeploymentConfig) -> bool;
    async fn mutate(&self, stack: Stack, stack_state: &StackState, config: &DeploymentConfig) -> Result<Stack>;
}
```

Example: The user defines a `Function`. On Azure, that Function needs a Container Apps Environment. The `AzureContainerAppsEnvironmentMutation` automatically adds it.

## Types of Checks

### Compile-Time Checks

Validate stack structure. No external API calls - pure validation. Can run during build or deployment.

Running these at build time enables **shift-left error detection** - catching problems before deployment even starts.

```rust
pub trait CompileTimeCheck: Send + Sync {
    fn description(&self) -> &'static str;
    fn should_run(&self, stack: &Stack, platform: Platform) -> bool;
    async fn check(&self, stack: &Stack, platform: Platform) -> Result<CheckResult>;
}
```

Examples:

- **HorizonRequiredCheck** - Containers on cloud platforms (AWS/GCP/Azure) require a Horizon cluster for orchestration. This is available through the alien.dev platform (alien-hosted or private managers). Local and Kubernetes platforms support containers natively without Horizon.
- **DnsTlsRequiredCheck** - External URLs on cloud platforms require DNS and TLS configuration (domain assignment, certificate issuance). This is provided automatically by the alien.dev platform. Standalone managers do not currently support external URLs on cloud platforms.
- **UniqueResourcesCheck** - All resource IDs in the stack are unique
- **ResourceReferencesExistCheck** - All resource references point to existing resources
- **SingleQueueTriggerCheck** - Each queue has at most one trigger function

### Runtime Checks

Runtime checks validate the actual target environment — they require API calls. They run during deployment only.

```rust
pub trait RuntimeCheck: Send + Sync {
    fn description(&self) -> &'static str;
    fn should_run(&self, stack: &Stack, platform: Platform) -> bool;
    async fn check(
        &self,
        stack: &Stack,
        stack_state: &StackState,
        config: &DeploymentConfig,
        client_config: &ClientConfig
    ) -> Result<CheckResult>;
}
```

Examples (planned):

- **Connectivity and authentication**
  - AWS: `sts:GetCallerIdentity`
  - GCP: `oauth2.tokeninfo`
  - Azure: `GET /me` (Microsoft Graph)
  - Kubernetes: `GET /namespaces`

- **Quota checks** - sufficient quotas for resources being deployed

- **API enablement** - required APIs are enabled (GCP/Azure only, AWS APIs are always enabled)

- **Permission checks** - sufficient permissions to deploy all resources

### Compatibility Checks

Validate changes between stack versions. Run during updates.

Examples:
- **PermissionProfilesUnchangedCheck** - Permission profiles haven't changed
- **FrozenResourcesUnchangedCheck** - Frozen resources haven't been modified

## Mutations

Mutations add infrastructure resources based on platform and stack composition.

### Execution Order

Mutations run in a specific order. Dependencies matter.

```rust
// 1. Global infrastructure (network before other infra)
NetworkMutation
AzureResourceGroupMutation
KubernetesNamespaceMutation

// 2. Service activations (enable APIs)
AzureServiceActivationMutation
GcpServiceActivationMutation

// 3. Platform-specific infrastructure
AzureContainerAppsEnvironmentMutation
AzureServiceBusNamespaceMutation
AzureStorageAccountMutation

// 4. Cross-account management
RemoteStackManagementMutation

// 5. Container cluster capacity groups
ContainerClusterMutation  // runs on every deploy — adapts capacity groups when containers change

// 6. Permission and service account setup
ManagementPermissionProfileMutation
ServiceAccountMutation

// 7. Application-specific
SecretsVaultMutation
CommandRequestQueuesMutation

// 8. Dependencies (must be last)
ServiceAccountDependenciesMutation
InfrastructureDependenciesMutation
```

### Example: AzureResourceGroupMutation

All Azure resources need a resource group. This mutation adds one:

```rust
impl StackMutation for AzureResourceGroupMutation {
    fn description(&self) -> &'static str {
        "Add Azure Resource Group required by all Azure resources"
    }

    fn should_run(&self, stack: &Stack, stack_state: &StackState, _config: &DeploymentConfig) -> bool {
        // Only for Azure
        if stack_state.platform != Platform::Azure {
            return false;
        }
        // Only if we have user-defined resources
        let has_user_resources = stack.resources.iter().any(|(_, entry)| {
            matches!(entry.config.resource_type().as_ref(), 
                "function" | "storage" | "vault" | "kv")
        });
        // Only if not already added
        has_user_resources && !stack.resources.contains_key("default-resource-group")
    }

    async fn mutate(&self, mut stack: Stack, _stack_state: &StackState, _config: &DeploymentConfig) -> Result<Stack> {
        let resource_group = AzureResourceGroup::new("default-resource-group").build();
        stack.resources.insert("default-resource-group", ResourceEntry {
            config: Resource::new(resource_group),
            lifecycle: ResourceLifecycle::Frozen,
            dependencies: Vec::new(),
            remote_access: false,
        });
        Ok(stack)
    }
}
```

## Execution Pipeline

### Build Time

Runs compile-time checks only. Fast validation, no external API calls.

```rust
let runner = PreflightRunner::new();
let summary = runner.run_build_time_preflights(&stack, platform).await?;
```

### Deployment Time

Full pipeline:

1. **Compile-time checks** on user-provided stack (fast validation)
2. **Apply mutations** to add infrastructure resources
3. **Compatibility checks** on mutated stack (if updating)
4. **Runtime checks** on mutated stack (platform API validation)

```rust
let runner = PreflightRunner::new();
let (mutated_stack, summary) = runner.run_deployment_time_preflights(
    stack,
    &stack_state,
    &client_config,
    old_stack.as_ref(),
    skip_frozen_check,
).await?;
```

The mutated stack is what gets passed to `alien-infra` for provisioning.

## Key Principle: Run All Checks

Even if one check fails, run all checks. Give the user a complete picture:

```rust
for check in checks {
    let result = check.check(stack, platform).await?;
    results.push(result);  // Don't return early on failure
}

Ok(PreflightSummary::from_results(results))
```

This way, the user sees all 5 problems at once instead of fixing them one by one.

## Implementation

### Crate Structure

```
alien-preflights/
├── src/
│   ├── lib.rs           # Traits and registry
│   ├── runner.rs        # PreflightRunner
│   ├── compile_time/    # Compile-time checks
│   ├── runtime/         # Runtime checks
│   ├── compatibility/   # Compatibility checks
│   └── mutations/       # Stack mutations
```

### PreflightRegistry

All checks and mutations are registered in `PreflightRegistry`:

```rust
let registry = PreflightRegistry::with_built_ins();
let checks = registry.get_compile_time_checks(&stack, platform);
let mutations = registry.get_mutations(&stack, &stack_state);
```

The registry filters by `should_run()` - only applicable checks/mutations execute.


