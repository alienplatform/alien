//! Stack Executor – **alien-infra**'s reconciliation engine
//!
//! This module owns the logic that reconciles an Alien *stack* with
//! cloud resources. It is completely platform-agnostic and delegates all
//! provider-specific heavy-lifting to implementations of
//! [`ResourceController`].
//!
//! The public surface is intentionally small:
//! * [`StackExecutor::new`] – validate a stack & build a dependency graph.
//! * [`StackExecutor::plan`] – do a pure diff between desired ↔ current.
//! * [`StackExecutor::step`] – advance every **ready** resource by one step.
//! * [`StackExecutor::run_until_synced`] – test helper that runs until desired == current.

use alien_error::{AlienError, Context, IntoAlienError};
use petgraph::algo::tarjan_scc;
use petgraph::graph::{DiGraph, NodeIndex};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{debug, error, info, warn};

use crate::{
    core::{
        state_utils::StackResourceStateExt, DefaultPlatformServiceProvider,
        PlatformServiceProvider, ResourceControllerContext, ResourceControllerStepResult,
        ResourceRegistry,
    },
    error::{ErrorData, Result},
};
use alien_core::ClientConfig;
use alien_core::{
    alien_event, AlienEvent, Resource, ResourceLifecycle, ResourceRef, ResourceStatus, Stack,
    StackResourceState, StackState,
};

/// Represents the outcome of a planning phase, identifying necessary changes.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlanResult {
    /// Resource IDs to be created.
    pub creates: Vec<String>,
    /// Map of Resource IDs to their new configuration for updates.
    pub updates: HashMap<String, Resource>,
    /// Resource IDs to be deleted.
    pub deletes: Vec<String>,
}

/// Represents the outcome of a single step in the stack execution process.
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StepResult {
    /// The resulting state of the stack after the step.
    pub next_state: StackState,
    /// An optional suggested duration to wait before executing the next step.
    /// This is the minimum delay suggested by any resource processed in this step,
    /// so we poll again as soon as the fastest resource is ready.
    pub suggested_delay_ms: Option<u64>,
}

/// Result of run_until_synced that includes the final state even on failure
#[derive(Debug)]
pub struct CompletionResult {
    /// The final stack state, available even if execution failed
    pub final_state: StackState,
    /// Whether the execution completed successfully (reached terminal state)
    pub success: bool,
    /// Error details if execution failed
    pub error: Option<AlienError<ErrorData>>,
}

impl CompletionResult {
    /// Convert the CompletionResult into a Result<StackState, AlienError<ErrorData>>
    /// This provides a convenient way to handle completion results in tests and other contexts
    /// where you want to propagate errors using the `?` operator.
    pub fn into_result(self) -> Result<StackState> {
        if self.success {
            Ok(self.final_state)
        } else {
            Err(self.error.unwrap_or_else(|| {
                AlienError::new(ErrorData::ExecutionStepFailed {
                    message: "Execution completed unsuccessfully with no error details".to_string(),
                    resource_id: None,
                })
            }))
        }
    }
}

/// Represents the configuration needed for a resource within the executor.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ResourceConfig {
    /// The full, canonical configuration of the resource.
    resource: Resource,
    /// All dependencies for this resource - combines the resource's intrinsic dependencies
    /// with any additional dependencies specified in the stack entry.
    dependencies: Vec<ResourceRef>,
    /// Cached identifier so that we do not need to allocate/clamp from the
    /// `Resource` over and over during graph operations.
    id: String,
    /// The lifecycle of the resource in the stack.
    lifecycle: ResourceLifecycle,
}

/// Drives the state machines inside a `Stack` for a particular platform.
pub struct StackExecutor {
    // --- Derived during construction ---
    resources: HashMap<String, ResourceConfig>,
    _dependency_graph: DiGraph<String, ()>,
    id_to_node_index: HashMap<String, NodeIndex>,
    node_index_to_id: HashMap<NodeIndex, String>,
    lifecycle_filter: Option<HashSet<ResourceLifecycle>>,
    /// Resource IDs that were excluded by the lifecycle filter but ARE in the
    /// desired stack. These must not be removed from state — other in-scope
    /// resources may depend on them.
    filtered_out_ids: HashSet<String>,
    desired_stack: Stack,

    // --- Stored from config for use during step() ---
    client_config: ClientConfig,
    resource_registry: Arc<ResourceRegistry>,
    service_provider: Arc<dyn PlatformServiceProvider>,
    deployment_config: alien_core::DeploymentConfig,
}

const MAX_RETRIES: u32 = 10;

/// Configuration for creating a [`StackExecutor`] via the builder pattern.
///
/// # Example
/// ```ignore
/// let executor = StackExecutor::builder(&stack, client_config)
///     .deployment_config(&config)
///     .lifecycle_filter(vec![ResourceLifecycle::Live])
///     .build()?;
/// ```
#[derive(bon::Builder)]
#[builder(start_fn = builder, finish_fn(vis = "", name = __build))]
pub struct StackExecutorConfig<'a> {
    /// The target stack to deploy
    #[builder(start_fn)]
    stack: &'a Stack,

    /// Cloud credentials for deployment execution
    #[builder(start_fn)]
    client_config: ClientConfig,

    /// Deployment configuration containing stack settings, management config, and deployment-time settings
    deployment_config: &'a alien_core::DeploymentConfig,

    /// Lifecycle filter - only resources with matching lifecycle are processed
    lifecycle_filter: Option<Vec<ResourceLifecycle>>,

    /// Custom resource registry (defaults to built-in registry)
    #[builder(default = Arc::new(ResourceRegistry::with_built_ins()))]
    resource_registry: Arc<ResourceRegistry>,

    /// Custom service provider for testing (defaults to real cloud clients)
    #[builder(default = Arc::new(DefaultPlatformServiceProvider::default()))]
    service_provider: Arc<dyn PlatformServiceProvider>,
}

/// Extension trait to add `build()` method to the builder that returns `Result<StackExecutor>`.
impl<S: stack_executor_config_builder::IsComplete> StackExecutorConfigBuilder<'_, S> {
    /// Build the StackExecutor from this configuration.
    ///
    /// This performs eager validation so that malformed stacks are
    /// rejected upfront before any cloud-side mutations occur.
    pub fn build(self) -> Result<StackExecutor> {
        let config = self.__build();
        StackExecutor::from_config(config)
    }
}

impl StackExecutor {
    /// Creates a new builder for constructing a StackExecutor.
    ///
    /// This performs eager validation so that malformed stacks are
    /// rejected upfront before any cloud-side mutations occur:
    /// * Ensures every resource id is unique.
    /// * Builds a dependency graph and checks that it is *acyclic*.
    /// * Verifies that each resource is supported on the requested platform.
    ///
    /// # Example
    /// ```ignore
    /// let executor = StackExecutor::builder(&stack, client_config)
    ///     .lifecycle_filter(vec![ResourceLifecycle::Live])
    ///     .external_bindings(config.external_bindings.clone())
    ///     .build()?;
    /// ```
    pub fn builder(stack: &Stack, client_config: ClientConfig) -> StackExecutorConfigBuilder<'_> {
        StackExecutorConfig::builder(stack, client_config)
    }

    /// Simple constructor for basic use cases (tests, examples).
    /// For production use, prefer the builder pattern.
    pub fn new(
        stack: &Stack,
        client_config: ClientConfig,
        lifecycle_filter: Option<Vec<ResourceLifecycle>>,
    ) -> Result<Self> {
        // Create a minimal deployment config with defaults
        let deployment_config = alien_core::DeploymentConfig::builder()
            .stack_settings(alien_core::StackSettings::default())
            .environment_variables(alien_core::EnvironmentVariablesSnapshot {
                variables: vec![],
                hash: String::new(),
                created_at: String::new(),
            })
            .external_bindings(alien_core::ExternalBindings::default())
            .allow_frozen_changes(false)
            .build();

        Self::builder(stack, client_config)
            .deployment_config(&deployment_config)
            .maybe_lifecycle_filter(lifecycle_filter)
            .build()
    }

    /// Construct from a config struct (internal implementation)
    fn from_config(config: StackExecutorConfig<'_>) -> Result<Self> {
        let stack = config.stack;
        let client_config = config.client_config;
        let resource_registry = config.resource_registry;
        let service_provider = config.service_provider;
        let deployment_config = config.deployment_config.clone();
        let lifecycle_filter = config.lifecycle_filter;
        let platform = client_config.platform();

        let mut graph = DiGraph::<String, ()>::new();
        let mut resource_map = HashMap::new();
        let mut id_to_node = HashMap::new();
        let mut node_to_id = HashMap::new();

        // Create a filter set if provided
        let filter_set =
            lifecycle_filter.map(|filters| filters.into_iter().collect::<HashSet<_>>());

        // Track filtered out resources to check dependencies later
        let mut filtered_out_ids = HashSet::new();

        // First pass: iterate through each resource in the stack.
        // This loop populates the resource_map and adds nodes to the dependency graph.
        // It also performs initial validation like checking for duplicate resource IDs
        // and ensuring a controller exists for each resource on the target platform.
        for (id, resource_entry) in stack.resources() {
            let id = id.clone();

            // Apply lifecycle filter if provided
            if let Some(filter_set) = &filter_set {
                if !filter_set.contains(&resource_entry.lifecycle) {
                    filtered_out_ids.insert(id.clone());
                    continue; // Skip this resource if its lifecycle doesn't match the filter
                }
            }

            if resource_map.contains_key(&id) {
                return Err(AlienError::new(ErrorData::DuplicateResourceId {
                    resource_id: id,
                }));
            }
            let node_index = graph.add_node(id.clone());
            id_to_node.insert(id.clone(), node_index);
            node_to_id.insert(node_index, id.clone());
            // Combine intrinsic dependencies from the resource with additional dependencies from the stack entry
            let mut all_dependencies = resource_entry.config.get_dependencies();
            all_dependencies.extend(resource_entry.dependencies.clone());

            resource_map.insert(
                id.clone(),
                ResourceConfig {
                    id: id.clone(),
                    resource: resource_entry.config.clone(),
                    dependencies: all_dependencies,
                    lifecycle: resource_entry.lifecycle,
                },
            );
            // Ensure that a controller exists for the resource on the specified platform.
            let resource_type = resource_entry.config.resource_type();
            resource_registry
                .get_controller(resource_type.clone(), platform)
                .context(ErrorData::ControllerNotAvailable {
                    resource_type,
                    platform,
                })?;
        }

        // Second pass: build the dependency graph edges.
        // This ensures that all nodes are added before attempting to create edges.
        for config in resource_map.values() {
            let source_id_str = &config.id;
            let source_node_index = id_to_node[source_id_str];
            for dependency_ref in &config.dependencies {
                let target_id_str = dependency_ref.id();

                // If the dependency was filtered out, we skip adding a graph edge –
                // the executor will rely on the runtime state to ensure that
                // the dependency is already satisfied (e.g. the resource is
                // `Running`).  We do **not** fail validation here because
                // incremental deployments frequently operate on a subset of
                // lifecycles (e.g. only `Live` resources) while relying on
                // resources from other lifecycles that were provisioned in a
                // previous phase.
                if filtered_out_ids.contains(target_id_str) {
                    continue; // Skip edge creation, but do not error.
                }

                // Check if the target dependency node exists in the graph.
                if let Some(target_node_index) = id_to_node.get(target_id_str) {
                    graph.add_edge(source_node_index, *target_node_index, ());
                } else {
                    // If a dependency is not defined *anywhere* in the stack, return a validation error.
                    return Err(AlienError::new(ErrorData::DependencyNotFound {
                        resource_id: source_id_str.to_string(),
                        dependency_id: target_id_str.to_string(),
                    }));
                }
            }
        }

        // Detect cycles in the dependency graph using Tarjan's algorithm for SCCs.
        // An SCC with more than one node indicates a cycle.
        let sccs = tarjan_scc(&graph);
        for scc in sccs {
            if scc.len() > 1 {
                let cycle_nodes: Vec<String> = scc
                    .into_iter()
                    .map(|idx| node_to_id[&idx].clone())
                    .collect();
                return Err(AlienError::new(ErrorData::CircularDependencyDetected {
                    resource_ids: cycle_nodes,
                }));
            }
        }

        Ok(Self {
            resources: resource_map,
            _dependency_graph: graph,
            id_to_node_index: id_to_node,
            node_index_to_id: node_to_id,
            lifecycle_filter: filter_set,
            filtered_out_ids,
            desired_stack: stack.clone(),
            client_config,
            resource_registry,
            service_provider,
            deployment_config,
        })
    }

    /// Constructs a `StackExecutor` specifically configured to delete all resources.
    /// It uses an empty internal `Stack` definition.
    pub fn for_deletion(
        client_config: ClientConfig,
        deployment_config: &alien_core::DeploymentConfig,
        lifecycle_filter: Option<Vec<ResourceLifecycle>>,
    ) -> Result<Self> {
        let platform = client_config.platform();
        let empty_stack = Stack::new(format!("delete-stack-{:?}", platform)).build();

        Self::builder(&empty_stack, client_config)
            .deployment_config(deployment_config)
            .maybe_lifecycle_filter(lifecycle_filter)
            .build()
    }

    /// Constructs a `StackExecutor` specifically configured to delete all resources.
    /// Accepts a custom service provider (primarily for testing).
    pub fn for_deletion_with_service_provider(
        client_config: ClientConfig,
        deployment_config: &alien_core::DeploymentConfig,
        service_provider: Arc<dyn PlatformServiceProvider>,
        lifecycle_filter: Option<Vec<ResourceLifecycle>>,
    ) -> Result<Self> {
        let platform = client_config.platform();
        let empty_stack = Stack::new(format!("delete-stack-{:?}", platform)).build();

        Self::builder(&empty_stack, client_config)
            .deployment_config(deployment_config)
            .service_provider(service_provider)
            .maybe_lifecycle_filter(lifecycle_filter)
            .build()
    }

    /// Computes the **diff** between the *desired* stack configuration and the
    /// *current* [`StackState`].
    ///
    /// The result is a [`PlanResult`] containing three mutually-exclusive
    /// collections that describe which resources should be **created**, **updated**,
    /// or **deleted** during the *next* reconciliation step.
    ///
    /// IMPORTANT: `plan` is **pure** – it never mutates cloud resources. It only
    /// inspects data that is already present in memory.
    ///
    /// # Planning Logic
    /// 1. **Creates**: Resources in desired state but not in current state
    /// 2. **Updates**: Resources in both states but with different configurations
    /// 3. **Deletes**: Resources in current state but not in desired state (respecting lifecycle filters)
    pub fn plan(&self, state: &StackState) -> Result<PlanResult> {
        // 0. Validate that every dependency *outside* the filtered executor scope
        //    already exists in the state and is `Running` / `Deleted`.  This
        //    prevents us from provisioning resources that would immediately
        //    fail at runtime due to missing prerequisites.

        for (res_id, res_cfg) in &self.resources {
            for dep_ref in &res_cfg.dependencies {
                let dep_id = dep_ref.id();

                // If the dependency is also managed by this executor we will
                // handle ordering through the regular dependency logic – skip.
                if self.resources.contains_key(dep_id) {
                    continue;
                }

                // Otherwise the dependency must already be in the stack state
                // and terminal (Running / Deleted).
                match state.resources.get(dep_id) {
                    Some(view)
                        if matches!(
                            view.status,
                            ResourceStatus::Running | ResourceStatus::Deleted
                        ) => {}
                    _ => {
                        return Err(AlienError::new(ErrorData::DependencyNotReady {
                            resource_id: res_id.to_string(),
                            dependency_id: dep_id.to_string(),
                        }));
                    }
                }
            }
        }

        let mut plan_result = PlanResult::default();

        // 1. Identify Creates: Resources in desired state (self.resources) but not in current state (state.resources)
        // OR resources that exist in state but are Deleted (recreation case)
        for resource_id in self.resources.keys() {
            match state.resources.get(resource_id) {
                None => {
                    // Resource not in state at all - plan create
                    debug!("Planning CREATE for resource '{}'", resource_id);
                    plan_result.creates.push(resource_id.clone());
                }
                Some(current_resource_state) => {
                    // Resource exists in state - check if it's deleted and needs recreation
                    if current_resource_state.status == ResourceStatus::Deleted {
                        debug!(
                            "Planning CREATE for deleted resource '{}' (recreation)",
                            resource_id
                        );
                        plan_result.creates.push(resource_id.clone());
                    }
                    // Otherwise, it will be handled by the update/delete logic below
                }
            }
        }

        // Pre-check: When using lifecycle filters, build a dependency map to check if resources can be safely deleted
        let mut has_dependents: HashMap<String, Vec<String>> = HashMap::new();

        if self.lifecycle_filter.is_some() {
            // Build map of resource_id -> list of resources that depend on it
            for (res_id, resource_state) in &state.resources {
                // Skip resources that are already being deleted or deleted
                if resource_state.status == ResourceStatus::Deleting
                    || resource_state.status == ResourceStatus::Deleted
                {
                    continue;
                }

                // Use the dependencies from the state instead of getting them from the config
                for dependency in &resource_state.dependencies {
                    let dep_id = dependency.id().to_string();
                    has_dependents
                        .entry(dep_id)
                        .or_default()
                        .push(res_id.clone());
                }
            }
        }

        // 2. Identify Deletes & Updates
        for (resource_id, current_resource_state) in &state.resources {
            match self.resources.get(resource_id) {
                // Resource NOT in desired state -> Delete (unless externally provisioned or filtered by lifecycle)
                None => {
                    // Skip deletion if the resource is externally provisioned
                    if current_resource_state.is_externally_provisioned {
                        debug!(
                            "Skipping DELETE for externally provisioned resource '{}' (status: {:?})",
                            resource_id, current_resource_state.status
                        );
                        continue;
                    }

                    // Skip if the resource status is already Deleting, Deleted, or DeleteFailed
                    if current_resource_state.status == ResourceStatus::Deleting
                        || current_resource_state.status == ResourceStatus::Deleted
                        || current_resource_state.status == ResourceStatus::DeleteFailed
                    {
                        continue;
                    }

                    // If we have a lifecycle filter and the resource's lifecycle doesn't match, skip deletion
                    if let Some(ref filter_set) = self.lifecycle_filter {
                        // If there's a lifecycle filter and it's not empty, only delete resources that match the filter
                        if !filter_set.is_empty() {
                            // If we don't have lifecycle information for the resource, skip deletion
                            // This is a defensive approach - if we can't determine the lifecycle, don't delete
                            if current_resource_state.lifecycle.is_none() {
                                debug!(
                                    "Resource '{}' has no lifecycle information, skipping deletion",
                                    resource_id
                                );
                                continue;
                            }

                            // Get the resource lifecycle from the state
                            let resource_lifecycle = current_resource_state
                                .lifecycle
                                .unwrap_or(ResourceLifecycle::Live);

                            // Skip deletion if the resource lifecycle doesn't match any in the filter
                            if !filter_set.contains(&resource_lifecycle) {
                                debug!(
                                    "Resource '{}' with lifecycle {:?} not in deletion filter, skipping deletion",
                                    resource_id, resource_lifecycle
                                );
                                continue;
                            }

                            // Now check if this resource has any active dependents
                            if let Some(dependent_resources) = has_dependents.get(resource_id) {
                                // Check if any of the dependent resources are active and have a lifecycle not in our filter
                                let has_active_dependents_outside_filter =
                                    dependent_resources.iter().any(|dep_id| {
                                        if let Some(dependent_state) = state.resources.get(dep_id) {
                                            // Only consider active dependents (not deleting/deleted)
                                            if dependent_state.status != ResourceStatus::Deleting
                                                && dependent_state.status != ResourceStatus::Deleted
                                            {
                                                // Check if the dependent has a lifecycle outside our filter
                                                let dep_lifecycle = dependent_state
                                                    .lifecycle
                                                    .unwrap_or(ResourceLifecycle::Live);
                                                !filter_set.contains(&dep_lifecycle)
                                            } else {
                                                false
                                            }
                                        } else {
                                            false
                                        }
                                    });

                                if has_active_dependents_outside_filter {
                                    debug!(
                                        "Resource '{}' has active dependents with lifecycles outside filter, skipping deletion",
                                        resource_id
                                    );
                                    continue;
                                }
                            }
                        }
                    }

                    // If we get here, the resource should be deleted
                    debug!(
                        "Planning DELETE for resource '{}' (status: {:?})",
                        resource_id, current_resource_state.status
                    );
                    plan_result.deletes.push(resource_id.clone());
                }
                // Resource EXISTS in desired state -> Check for Update
                Some(desired_config) => {
                    // For config comparison, we need to consider:
                    // - If resource has internal state: use the config from state
                    // - If resource is Pending (no internal state): use the config stored in the state
                    // This prevents false config change detection for Pending resources
                    let current_resource_config_opt = Some(&current_resource_state.config);

                    // Compare desired resource config with the config in the *current* state
                    if Some(&desired_config.resource) != current_resource_config_opt {
                        match current_resource_state.status {
                            ResourceStatus::Running | ResourceStatus::UpdateFailed => {
                                // Check if all new dependencies are ready before planning the update
                                let new_dependencies_ready =
                                    desired_config.dependencies.iter().all(|dep_ref| {
                                        let dep_id = dep_ref.id();
                                        match state.resources.get(dep_id) {
                                            Some(dep_view) => matches!(
                                                dep_view.status,
                                                ResourceStatus::Running | ResourceStatus::Deleted
                                            ),
                                            None => false, // Dependency not present in state (yet)
                                        }
                                    });

                                if new_dependencies_ready {
                                    debug!("Scheduling UPDATE transition for '{}'", resource_id);

                                    // Validate the update before adding to plan
                                    if let Some(current_config) =
                                        current_resource_config_opt.as_ref()
                                    {
                                        current_config
                                            .validate_update(&desired_config.resource)
                                            .context(ErrorData::ResourceConfigInvalid {
                                                message: "Resource cannot be updated".to_string(),
                                                resource_id: Some(resource_id.clone()),
                                            })?;
                                    }

                                    plan_result.updates.insert(
                                        resource_id.clone(),
                                        desired_config.resource.clone(),
                                    );
                                } else {
                                    debug!("Deferring UPDATE for '{}' - new dependencies not ready yet", resource_id);
                                }
                            }
                            ResourceStatus::ProvisionFailed => {
                                info!("Restarting CREATE for '{}' due to config change during ProvisionFailed", resource_id);
                                plan_result.creates.push(resource_id.clone());
                                plan_result.updates.remove(resource_id); // Ensure no conflicting update plan
                                plan_result.deletes.retain(|id| id != resource_id);
                                // Ensure no conflicting delete plan
                            }
                            ResourceStatus::DeleteFailed => {
                                warn!("Config changed for '{}' while in DeleteFailed status, ignoring change", resource_id);
                            }
                            ResourceStatus::Deleted => {
                                // Resource is deleted but desired again (with different config) - plan for recreation
                                info!("Planning CREATE for '{}' due to config change while Deleted (recreation)", resource_id);
                                plan_result.creates.push(resource_id.clone());
                                plan_result.updates.remove(resource_id); // Ensure no conflicting update plan
                                plan_result.deletes.retain(|id| id != resource_id);
                                // Ensure no conflicting delete plan
                            }
                            ResourceStatus::Pending => {
                                // Config changed while Pending. Usually means the desired config was updated before initialization.
                                // No action needed here; the desired_config will be used when it initializes.
                                info!("Config changed for '{}' while Pending, will use new config on initialization", resource_id);
                            }
                            ResourceStatus::Provisioning => {
                                // Delete-then-recreate: the resource is stuck mid-provisioning
                                // with stale config. Since transition_to_delete_start() is now
                                // unconditional, we can safely interrupt it.
                                info!("Config changed for '{}' during Provisioning, planning delete-then-recreate", resource_id);
                                plan_result.deletes.push(resource_id.clone());
                                plan_result.creates.retain(|id| id != resource_id);
                                plan_result.updates.remove(resource_id);
                            }
                            _ => {
                                // Updating, Deleting -- wait for stable before acting
                                warn!("Config changed for '{}' while in status {:?}, ignoring change until stable", resource_id, current_resource_state.status);
                            }
                        }
                    } else {
                        // Configs match, no update action needed from diffing.
                        // The resource will proceed based on its current state later.
                    }
                }
            }
        }

        // 3. Propagate dependency changes: If a dependency is being updated or created, mark dependent resources for update
        let mut dependency_propagation_needed = true;
        while dependency_propagation_needed {
            dependency_propagation_needed = false;
            let current_updates = plan_result.updates.keys().cloned().collect::<HashSet<_>>();
            let current_creates: HashSet<String> = plan_result.creates.iter().cloned().collect();

            for (resource_id, desired_config) in &self.resources {
                // Skip if this resource is already marked for update, creation, or deletion
                if current_updates.contains(resource_id)
                    || plan_result.creates.contains(resource_id)
                    || plan_result.deletes.contains(resource_id)
                {
                    continue;
                }

                // Check if this resource has any dependencies that are being updated or created
                let has_dependency_change = desired_config.dependencies.iter().any(|dep_ref| {
                    current_updates.contains(dep_ref.id()) || current_creates.contains(dep_ref.id())
                });

                if has_dependency_change {
                    // Check if the resource exists in current state and is in a status that allows updates
                    if let Some(current_resource_state) = state.resources.get(resource_id) {
                        match current_resource_state.status {
                            ResourceStatus::Running | ResourceStatus::UpdateFailed => {
                                // Check if all dependencies are ready before planning the update from dependency propagation
                                let dependencies_ready =
                                    desired_config.dependencies.iter().all(|dep_ref| {
                                        let dep_id = dep_ref.id();
                                        match state.resources.get(dep_id) {
                                            Some(dep_view) => matches!(
                                                dep_view.status,
                                                ResourceStatus::Running | ResourceStatus::Deleted
                                            ),
                                            None => false, // Dependency not present in state (yet)
                                        }
                                    });

                                if dependencies_ready {
                                    debug!(
                                        "Planning UPDATE for resource '{}' due to dependency changes (create/update)",
                                        resource_id
                                    );

                                    // Validate the update before adding to plan (even though config didn't change)
                                    if current_resource_state.has_internal_state() {
                                        current_resource_state.config
                                            .validate_update(&desired_config.resource)
                                            .context(ErrorData::ResourceConfigInvalid {
                                                message: "Resource cannot be updated due to dependency changes (create/update)".to_string(),
                                                resource_id: Some(resource_id.clone()),
                                            })?;
                                    }

                                    plan_result.updates.insert(
                                        resource_id.clone(),
                                        desired_config.resource.clone(),
                                    );
                                    dependency_propagation_needed = true; // Continue iterating as this might trigger more updates
                                } else {
                                    debug!("Deferring UPDATE for '{}' due to dependency changes - dependencies not ready yet", resource_id);
                                }
                            }
                            _ => {
                                // Skip resources that are not in a state that allows updates
                                debug!(
                                    "Skipping dependency change update for resource '{}' in status {:?}",
                                    resource_id, current_resource_state.status
                                );
                            }
                        }
                    }
                }
            }
        }

        // Note: We don't need final validation of dependency readiness here.
        // The execution phase already handles dependency ordering through dependencies_met() and is_ready logic.

        Ok(plan_result)
    }

    /// Performs one *incremental* reconciliation iteration.
    ///
    /// 1. Runs [`plan`] to identify high-level transitions and inject them into
    ///    a working copy of the provided [`StackState`].
    /// 2. Determines which resources are *ready* based on dependency ordering
    ///    and their current status.
    /// 3. For every ready resource delegates the heavy-lifting to the
    ///    appropriate [`ResourceController`]. The controller returns the next
    ///    [`ResourceControllerState`](crate::core::ResourceControllerState) as well
    ///    as an optional back-off suggestion.
    /// 4. Aggregates the per-resource results and returns a new [`StackState`]
    ///    snapshot together with the **minimum** suggested delay – callers can
    ///    use it to implement efficient polling (poll as soon as the fastest
    ///    resource is ready).
    ///
    /// The method is entirely *stateless* from the executor's perspective: all
    /// data required for the next iteration is carried within the returned
    /// `StackState`.
    #[alien_event(AlienEvent::StackStep {
        next_state: state.clone(),
        suggested_delay_ms: None,
    })]
    pub async fn step(&self, state: StackState) -> Result<StepResult> {
        let mut next_state = state.clone(); // Clone the input state to modify

        // --- Planning Phase ---
        let plan_result = self.plan(&state)?;
        debug!(
            "Plan result: {} creates, {} updates, {} deletes",
            plan_result.creates.len(),
            plan_result.updates.len(),
            plan_result.deletes.len()
        );

        // Apply planned transitions directly or prepare initial state
        let mut initial_transitions: HashMap<String, StackResourceState> = HashMap::new();
        let mut removed_pending_ids: Vec<String> = Vec::new();
        let mut removed_external_ids: Vec<String> = Vec::new();

        // Handle Creates - prepare initial pending states or handle external bindings
        for resource_id in &plan_result.creates {
            let desired_config = self.resources.get(resource_id).ok_or_else(|| {
                AlienError::new(ErrorData::InfrastructureError {
                    message: format!(
                        "Planned create resource '{}' not found in desired config",
                        resource_id
                    ),
                    operation: Some("step".to_string()),
                    resource_id: Some(resource_id.clone()),
                })
            })?;

            // Get the lifecycle from the resource configuration
            let resource_lifecycle = Some(desired_config.lifecycle);

            // Check if this resource has an external binding
            if let Some(binding) = self.deployment_config.external_bindings.get(resource_id) {
                // Validate binding type matches resource type
                alien_core::validate_binding_type(&desired_config.resource, binding).context(
                    ErrorData::ResourceConfigInvalid {
                        message: format!(
                            "External binding type mismatch for resource '{}'",
                            resource_id
                        ),
                        resource_id: Some(resource_id.clone()),
                    },
                )?;

                info!(
                    "Using external binding for '{}' -> Running (externally provisioned)",
                    resource_id
                );

                // Get resource entry to check remote_access flag
                let resource_entry = self.desired_stack.resources.get(resource_id);
                let remote_access = resource_entry.map(|e| e.remote_access).unwrap_or(false);

                // Create resource state as Running with external binding
                let mut resource_state = StackResourceState::new_pending(
                    desired_config.resource.resource_type().to_string(),
                    desired_config.resource.clone(),
                    resource_lifecycle,
                    desired_config.dependencies.clone(),
                );
                resource_state.status = ResourceStatus::Running;
                resource_state.is_externally_provisioned = true;

                // Only sync binding params if remote_access is enabled
                if remote_access {
                    resource_state.remote_binding_params =
                        Some(serde_json::to_value(binding).into_alien_error().context(
                            ErrorData::ResourceStateSerializationFailed {
                                resource_id: resource_id.clone(),
                                message:
                                    "Failed to serialize external binding parameters".to_string(),
                            },
                        )?);
                }

                // Populate outputs from the binding so dependent resources can
                // call get_resource_outputs() (e.g., functions reading the
                // Container Apps Environment name/resource ID).
                if let Some(outputs) = binding.to_resource_outputs() {
                    resource_state.outputs = Some(outputs);
                }

                initial_transitions.insert(resource_id.clone(), resource_state);
                continue;
            }

            debug!("Preparing CREATE for '{}' -> Pending", resource_id);
            let pending_view = StackResourceState::new_pending(
                desired_config.resource.resource_type().to_string(),
                desired_config.resource.clone(),
                resource_lifecycle,
                desired_config.dependencies.clone(),
            );
            initial_transitions.insert(resource_id.clone(), pending_view);
        }

        // Handle Deletes - initiate deletion transitions only for deletion-ready resources
        for resource_id in &plan_result.deletes {
            if let Some(resource_state) = next_state.resources.get_mut(resource_id) {
                debug!(
                    "Processing planned DELETE for '{}' (status: {:?})",
                    resource_id, resource_state.status
                );
                match resource_state.status {
                    ResourceStatus::Pending => {
                        debug!("Removing Pending resource directly");
                        removed_pending_ids.push(resource_id.clone());
                    }
                    // Eligible for initiating delete transition.
                    // Provisioning is included: transition_to_delete_start() is now unconditional,
                    // so we can interrupt a mid-provisioning resource when its config changed.
                    ResourceStatus::Running
                    | ResourceStatus::Provisioning
                    | ResourceStatus::ProvisionFailed
                    | ResourceStatus::UpdateFailed
                    | ResourceStatus::DeleteFailed => {
                        // Check if this resource is ready for deletion (all dependents are deleted/deleting)
                        if !self.deletion_ready(resource_id, &state) {
                            debug!(
                                "Resource '{}' not ready for deletion - skipping until dependents are deleted",
                                resource_id
                            );
                            continue;
                        }

                        match resource_state.get_internal_controller() {
                            Ok(Some(mut resource_controller)) => {
                                match resource_controller.transition_to_delete_start() {
                                    Ok(()) => {
                                        debug!(
                                            "Transitioning to delete state ({:?})",
                                            resource_controller.get_status()
                                        );
                                        resource_state.status = resource_controller.get_status();
                                        resource_state.outputs = resource_controller.get_outputs();
                                        resource_state.retry_attempt = 0;
                                        resource_state.error = None; // Clear error when starting delete
                                                                     // Update internal state with the modified controller
                                        if let Err(e) = resource_state
                                            .set_internal_controller(Some(resource_controller))
                                        {
                                            error!("Failed to serialize controller state: {}", e);
                                        }
                                        // Keep existing config and previous_config during deletion
                                    }
                                    Err(e) => {
                                        error!(
                                            "Failed to start delete transition for resource '{}': {}",
                                            resource_id, e
                                        );
                                        let failed_state = resource_state.with_updates(|state| {
                                            state.status = ResourceStatus::DeleteFailed;
                                            state.error = Some(e.into_generic());
                                            state.retry_attempt = 0;
                                            
                                            // Preserve current controller state as last_failed_state for retry
                                            if let Ok(Some(current_controller)) = state.get_internal_controller() {
                                                if let Err(set_error) = state.set_last_failed_controller(Some(current_controller)) {
                                                    error!("Failed to preserve controller state for retry: {}", set_error);
                                                }
                                            }
                                        });
                                        *resource_state = failed_state;
                                    }
                                }
                            }
                            Ok(None) => {
                                warn!("Cannot start delete transition for status {:?} because it has no controller, executor will proceed", resource_state.status);
                            }
                            Err(e) => {
                                error!(
                                    "Failed to deserialize controller for delete transition: {}",
                                    e
                                );
                                // Note: Cannot preserve controller state since deserialization failed
                                // This indicates corrupted state - manual intervention may be needed
                                let failed_state = resource_state
                                    .with_failure(ResourceStatus::DeleteFailed, e.into_generic());
                                *resource_state = failed_state;
                            }
                        }
                    }
                    // Already Deleting or Deleted - no action needed here, subsequent steps handle it.
                    ResourceStatus::Deleting | ResourceStatus::Deleted => {
                        debug!(
                            "Already {:?}, no delete transition needed",
                            resource_state.status
                        );
                    }
                    // Updating - let the current operation finish or fail first.
                    _ => {
                        warn!(
                            "Planned delete for resource in {:?} status, waiting for stabilization",
                            resource_state.status
                        );
                    }
                }
            } else {
                // This case should ideally not happen if plan is correct, but handle defensively.
                warn!(
                    "Planned delete for resource '{}' not found in current state, skipping",
                    resource_id
                );
            }
        }

        // Remove externally provisioned resources that are no longer in the desired stack.
        // These were not created by us, so they don't need deletion — just remove from state
        // so they don't block stack status (e.g., staying Running during a full-stack delete).
        //
        // IMPORTANT: When a lifecycle filter is active, some resources are excluded from
        // self.resources even though they ARE in the desired stack (just filtered out).
        // Don't remove those — they're still needed by resources within the filtered scope
        // (e.g., a Live function depends on a Frozen container-apps-environment).
        for (resource_id, resource_state) in &next_state.resources {
            if resource_state.is_externally_provisioned
                && !self.resources.contains_key(resource_id)
                && !self.filtered_out_ids.contains(resource_id)
            {
                debug!(
                    "Removing externally provisioned resource '{}' from state (not in desired stack)",
                    resource_id
                );
                removed_external_ids.push(resource_id.clone());
            }
        }

        // Handle Updates - create initial update transition states
        for (resource_id, new_config) in &plan_result.updates {
            if let Some(current_state) = state.resources.get(resource_id) {
                // Use original state for comparison
                debug!(
                    "Preparing UPDATE for '{}' (status: {:?})",
                    resource_id, current_state.status
                );

                // Try to get the controller and transition to update
                match current_state.get_internal_controller() {
                    Ok(Some(mut controller)) => {
                        match controller.transition_to_update() {
                            Ok(()) => {
                                // Calculate updated dependencies based on new config
                                let desired_config = match self.resources.get(resource_id) {
                                    Some(config) => config,
                                    None => {
                                        error!("Resource '{}' not found in desired config during update", resource_id);
                                        let failed_update_state = current_state
                                            .with_failure(ResourceStatus::UpdateFailed, AlienError::new(ErrorData::InfrastructureError {
                                                message: format!("Resource '{}' not found in desired config during update", resource_id),
                                                operation: Some("update_dependencies".to_string()),
                                                resource_id: Some(resource_id.clone()),
                                            }).into_generic());
                                        initial_transitions
                                            .insert(resource_id.clone(), failed_update_state);
                                        continue;
                                    }
                                };

                                let updated_state = current_state.with_updates(|state| {
                                    state.config = new_config.clone(); // Set to new desired config
                                    state.previous_config = Some(current_state.config.clone()); // Store old config
                                    state.dependencies = desired_config.dependencies.clone(); // Update dependencies to match new config
                                    state.status = controller.get_status();
                                    state.outputs = controller.get_outputs();
                                    state.retry_attempt = 0;
                                    state.error = None; // Clear error when starting update
                                                        // Update internal state with the mutated controller
                                    if let Err(e) = state.set_internal_controller(Some(controller))
                                    {
                                        error!("Failed to serialize controller state: {}", e);
                                    }
                                });
                                initial_transitions.insert(resource_id.clone(), updated_state);
                            }
                            Err(e) => {
                                error!(
                                    "Failed to start update transition for resource '{}': {}",
                                    resource_id, e
                                );
                                // Create a failed update state and preserve current controller state for retry
                                let failed_update_state = current_state.with_updates(|state| {
                                    state.status = ResourceStatus::UpdateFailed;
                                    state.error = Some(e.into_generic());
                                    state.retry_attempt = 0;

                                    // Preserve current controller state as last_failed_state for retry
                                    if let Ok(Some(current_controller)) =
                                        state.get_internal_controller()
                                    {
                                        if let Err(set_error) = state
                                            .set_last_failed_controller(Some(current_controller))
                                        {
                                            error!(
                                                "Failed to preserve controller state for retry: {}",
                                                set_error
                                            );
                                        }
                                    }
                                });
                                initial_transitions
                                    .insert(resource_id.clone(), failed_update_state);
                            }
                        }
                    }
                    Ok(None) => {
                        warn!(
                            "Cannot start update transition from state {:?}, skipping planned update",
                            current_state.status
                        );
                    }
                    Err(e) => {
                        error!(
                            "Failed to deserialize controller for update transition: {}",
                            e
                        );
                        let failed_update_state = current_state
                            .with_failure(ResourceStatus::UpdateFailed, e.into_generic());
                        initial_transitions.insert(resource_id.clone(), failed_update_state);
                    }
                }
            } else {
                warn!(
                    "Planned update for non-existent resource '{}', skipping",
                    resource_id
                );
            }
        }

        // Remove pending resources marked for deletion
        for id in removed_pending_ids {
            next_state.resources.remove(&id);
        }

        // Remove externally provisioned resources that are no longer in the desired stack
        for id in removed_external_ids {
            next_state.resources.remove(&id);
        }

        // Apply the calculated initial transitions for Creates/Updates
        next_state.resources.extend(initial_transitions);
        // --- Planning Phase End ---

        // --- Execution Phase ---
        // Now, proceed with processing ready resources based on the potentially updated next_state
        let mut subsequent_state_updates = HashMap::new();
        let mut min_delay: Option<Duration> = None;

        // Determine ready resources by iterating through the *current state* after applying the plan
        let mut ready_resource_ids = Vec::new();
        // Use the keys from the state *after* applying the plan
        for resource_id in next_state.resources.keys() {
            // Check status and terminal state using next_state
            let current_resource_view = next_state.resources.get(resource_id).unwrap(); // Should always exist
                                                                                        // Check terminality based on *desired* state + current status
            let is_potentially_terminal = self.is_resource_synced(resource_id, &next_state);

            // If the planned transition put it into a terminal state (e.g. failed update plan), skip.
            // If the resource is not desired but is now Deleted (terminal), skip.
            if is_potentially_terminal {
                // Need to differentiate: is it terminal *because* it reached the desired state (Running)?
                // Or is it terminal because it's Deleted and not desired?
                // Or terminal due to a failure state?
                // The main `is_synced` check at the end handles the overall stack goal.
                // Here, we just want to avoid stepping resources that are *already* in a final (success, deleted, or failed) state *for their current operation*.
                if current_resource_view.status.is_terminal() {
                    continue; // Skip resources already in a terminal status (Running, Deleted, *Failed)
                }
            }

            // When a lifecycle filter is active, skip heartbeat checks for resources
            // outside the executor's scope. For example, during Provisioning (Live-only),
            // frozen resources are already Running from InitialSetup and should not be
            // stepped — they may use different credentials than what the current executor has.
            // However, resources actively being deleted (Deleting status) must still be
            // stepped to complete their deletion, even though they're not in the desired stack.
            if self.lifecycle_filter.is_some()
                && !self.resources.contains_key(resource_id)
                && current_resource_view.status != ResourceStatus::Deleting
            {
                continue;
            }

            let is_actively_modifying = matches!(
                current_resource_view.status,
                ResourceStatus::Updating | ResourceStatus::Deleting
            );

            // A resource is ready if:
            // 1. It's actively being updated or deleted.
            // 2. It's already Running - needs to run Ready handler for health checks.
            // 3. Or, it's not actively modifying (e.g., Pending) AND its dependencies were met before this step.
            let is_ready = if is_actively_modifying {
                true
            } else if current_resource_view.status == ResourceStatus::Running {
                // Running resources should always be stepped to run their Ready handler.
                // This is important for local platform where ephemeral state (ports, URLs)
                // needs to be refreshed after restart, even if config hasn't changed.
                true
            } else {
                // Check dependencies only if the resource is defined in the target graph
                if let Some(node_index) = self.id_to_node_index.get(resource_id) {
                    // Resource is in target graph - use creation dependency logic
                    self.dependencies_met(*node_index, &state) // Use original state for deps
                } else {
                    // Resource is in state but not in target graph (i.e., being deleted)
                    // For deletion, check if all dependents are already deleted
                    self.deletion_ready(resource_id, &state)
                }
            };

            if is_ready {
                ready_resource_ids.push(resource_id.clone());
            }
        }

        if !ready_resource_ids.is_empty() {
            debug!("Ready resources for step: {:?}", ready_resource_ids);
        }

        // Process each ready resource by stepping its state machine
        for resource_id in ready_resource_ids {
            // Get current resource state (may be updated during initialization)
            let mut current_resource_state = next_state
                .resources
                .get(&resource_id)
                .cloned()
                .ok_or_else(|| {
                    AlienError::new(ErrorData::ExecutionStepFailed {
                        message: format!(
                            "Ready resource '{}' not found in next_state",
                            resource_id
                        ),
                        resource_id: Some(resource_id.to_string()),
                    })
                })?;

            let context_resource: Resource;

            // Use current desired config from the stack if it exists, otherwise use stored config for deletion
            context_resource = if let Some(resource_config) = self.resources.get(&resource_id) {
                // Resource is still in desired stack (create/update case)
                resource_config.resource.clone()
            } else {
                // Resource is not in desired stack (deletion case) - use stored config from stack state
                current_resource_state.config.clone()
            };

            // Handle initialization for Pending resources without controller
            if current_resource_state.status == ResourceStatus::Pending
                && !current_resource_state.has_internal_state()
            {
                info!(
                    "Initializing controller for Pending resource '{}'",
                    resource_id
                );

                // Get the controller for this resource type
                let resource_type = context_resource.resource_type();
                let controller = self
                    .resource_registry
                    .get_controller(resource_type, next_state.platform)?;

                // Controllers now implement Default, so we get a fresh instance with the default state
                let initial_status = controller.get_status();
                let initial_outputs = controller.get_outputs();

                // Update the resource state with the initialized controller
                current_resource_state.set_internal_controller(Some(controller))?;
                current_resource_state.status = initial_status;
                current_resource_state.outputs = initial_outputs;
                current_resource_state.error = None;
                current_resource_state.retry_attempt = 0;

                info!(
                    "Initialized controller for '{}' with status {:?}",
                    resource_id, initial_status
                );
            }

            // We don't need to get a separate controller since the controller is stored
            // in the internal_state and handles its own stepping
            if !current_resource_state.has_internal_state() {
                warn!(
                    "Resource '{}' has no controller state. Skipping step.",
                    resource_id
                );
                continue;
            }

            let context = ResourceControllerContext {
                desired_config: &context_resource,
                platform: next_state.platform,
                client_config: self.client_config.clone(),
                state: &next_state,
                resource_prefix: &next_state.resource_prefix,
                registry: &self.resource_registry,
                desired_stack: &self.desired_stack,
                service_provider: &self.service_provider,
                deployment_config: &self.deployment_config,
            };

            // Use the *original* current_resource_state (before potential initialization changes below)
            // for retry count and existing internal state.
            let original_retry_attempt = current_resource_state.retry_attempt;

            let (step_outcome_result, updated_controller) = {
                let mut resource_controller =
                    current_resource_state.get_internal_controller()?.unwrap();

                let step_result = if !resource_controller.get_status().is_terminal() {
                    // The controller now returns a step result with suggested delay
                    resource_controller.step(&context).await
                } else {
                    // If already terminal, no step needed
                    Ok(ResourceControllerStepResult {
                        suggested_delay: None,
                    })
                };

                (step_result, resource_controller.box_clone())
            };

            // Handle the step result
            let (next_retry_attempt, next_error, step_suggested_delay, force_failure) =
                match step_outcome_result {
                    Ok(step_result) => {
                        // Step succeeded, clear retry count and error
                        (0, None, step_result.suggested_delay, false)
                    }
                    Err(err) => {
                        warn!("Step failed for '{}': {}", resource_id, err);

                        let error = Some(err.clone().into_generic());

                        // Check if the error is retryable
                        if !err.retryable {
                            error!(
                            "Non-retryable error for '{}', transitioning to failure immediately",
                            resource_id
                        );
                            // For non-retryable errors, immediately transition to failure (no retries)
                            (0, error, None, true) // force_failure = true bypasses retry logic
                        } else {
                            let new_retry_attempt = original_retry_attempt + 1;
                            if new_retry_attempt >= MAX_RETRIES {
                                error!(
                                    "Max retries reached for '{}', transitioning to failure",
                                    resource_id
                                );
                                // Transition to failure state on the updated controller
                                (new_retry_attempt, error, None, true) // force_failure = true
                            } else {
                                // Calculate exponential backoff delay: 2^(retry_attempt) seconds
                                let delay_secs = 2u64.pow(original_retry_attempt);
                                let delay = Duration::from_secs(delay_secs);
                                info!(
                                    "Scheduling retry for '{}' (attempt {}/{}) after {:?}",
                                    resource_id, new_retry_attempt, MAX_RETRIES, delay
                                );
                                // Update min_delay if this retry suggests a delay
                                min_delay = Some(min_delay.map_or(delay, |d| d.min(delay)));
                                (new_retry_attempt, error, None, false) // No step delay on error, use retry delay instead
                            }
                        }
                    }
                };

            // Update min_delay with step-suggested delay.
            // None means "proceed immediately" — the handler just did work and its next
            // state should run ASAP (e.g., CreateStart → CreateWaitForActive). We treat
            // it as Duration::ZERO so it wins over longer heartbeat delays from Running
            // resources, preventing transitions from being blocked behind 30s+ heartbeats.
            //
            // IMPORTANT: Ready/heartbeat handlers MUST return an explicit Some(duration)
            // to avoid collapsing polling delays from other resources to zero. A Ready
            // handler returning None is a bug — it means "call me again immediately"
            // which is never correct for a heartbeat check.
            let effective_delay = step_suggested_delay.unwrap_or(Duration::ZERO);
            min_delay = Some(min_delay.map_or(effective_delay, |d| d.min(effective_delay)));

            // Use the appropriate controller based on whether we failed
            let final_controller =
                if force_failure || (next_error.is_some() && next_retry_attempt >= MAX_RETRIES) {
                    let mut failure_controller = updated_controller.clone();
                    failure_controller.transition_to_failure();
                    failure_controller
                } else {
                    updated_controller.clone()
                };

            // Get the updated status, outputs, and binding params from the final controller
            let next_status = final_controller.get_status();
            let next_outputs = final_controller.get_outputs();
            let next_binding_params = final_controller.get_binding_params()?;

            // Automatically update config to match desired state (except during deletion)
            let next_config = if current_resource_state.status == ResourceStatus::Deleting {
                // During deletion, preserve the current config
                current_resource_state.config.clone()
            } else {
                // For create/update operations, ensure config matches desired state
                self.resources
                    .get(&resource_id)
                    .map(|rc| rc.resource.clone())
                    .unwrap_or_else(|| current_resource_state.config.clone())
            };

            // Set the internal controller outside the closure to handle errors properly
            let mut next_state = current_resource_state.clone();
            next_state
                .set_internal_controller(Some(final_controller))
                .context(ErrorData::ResourceStateSerializationFailed {
                    resource_id: resource_id.clone(),
                    message: "Failed to serialize final controller state".to_string(),
                })?;

            // Handle last failed controller serialization outside closure
            if force_failure || (next_error.is_some() && next_retry_attempt >= MAX_RETRIES) {
                next_state
                    .set_last_failed_controller(Some(updated_controller.box_clone()))
                    .context(ErrorData::ResourceStateSerializationFailed {
                        resource_id: resource_id.clone(),
                        message: "Failed to serialize last failed controller state".to_string(),
                    })?;
            }

            let next_state = next_state.with_updates(|state| {
                state.status = next_status;
                state.outputs = next_outputs;
                state.remote_binding_params = next_binding_params;
                state.config = next_config;
                state.retry_attempt = next_retry_attempt;
                state.error = next_error;
            });

            // Always record the resulting state from the step
            subsequent_state_updates.insert(resource_id.clone(), next_state);
        }
        // --- Execution Phase End ---

        // Apply all subsequent updates gathered in this step to the next_state
        debug!(
            "Applying {} resource state updates",
            subsequent_state_updates.len()
        );
        next_state.resources.extend(subsequent_state_updates);

        let step_result = StepResult {
            next_state: next_state.clone(),
            suggested_delay_ms: min_delay.map(|d| d.as_millis() as u64),
        };

        // Update the event with final results
        _event_handle
            .update(AlienEvent::StackStep {
                next_state: step_result.next_state.clone(),
                suggested_delay_ms: step_result.suggested_delay_ms,
            })
            .await
            .context(ErrorData::InfrastructureError {
                message: "Failed to update StackStep event".to_string(),
                operation: Some("event_update".to_string()),
                resource_id: None,
            })?;

        Ok(step_result)
    }

    /// Runs the reconciliation loop until the stack is "synced" (desired state matches current state).
    ///
    /// This helper is primarily for tests - it repeatedly calls [`step`] until:
    /// - All desired resources are `Running` with matching configs
    /// - All resources to be deleted are `Deleted`
    ///
    /// In production, the executor is driven by an external orchestrator (e.g. Temporal, Lambda)
    /// that calls `step()` periodically. There is no "completion" in production - controllers
    /// continuously run heartbeat checks.
    ///
    /// Safety guards:
    /// 1. Back-pressure sleep between iterations (suggested delay or 50ms minimum)
    /// 2. Max steps limit (proportional to resource count) to prevent infinite loops
    ///
    /// If the limit is hit, returns [`CompletionResult`] with `success=false`.
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn run_until_synced(&self, state: StackState) -> CompletionResult {
        info!(
            platform = ?state.platform,
            "Running state steps until synced"
        );
        let mut current_state = state.clone();
        let mut step_counter = 0;
        // Increased max_steps slightly to accommodate polling delays
        let max_steps = self.resources.len() * 20 + 10;

        while !self.is_synced(&current_state) {
            let current_state_for_step = current_state.clone();
            let step_result = match self.step(current_state_for_step).await {
                Ok(result) => result,
                Err(e) => {
                    error!("Step failed during execution: {}", e);
                    return CompletionResult {
                        final_state: current_state,
                        success: false,
                        error: Some(e),
                    };
                }
            };

            // Update the mutable current_state with the result from the step
            current_state = step_result.next_state;
            let suggested_delay_ms = step_result.suggested_delay_ms;

            // The loop condition `!self.is_synced(&current_state)` is the primary check.
            // We no longer break early if a single step didn't change the status/retry/error.
            // This allows polling (which might only return a delay) to complete.

            if let Some(delay_ms) = suggested_delay_ms {
                debug!(delay_ms = delay_ms, "Waiting before next step");
                sleep(Duration::from_millis(delay_ms)).await;
            } else {
                // If there's no delay, add a small pause to prevent tight looping
                // in case of rapid state changes without delays.
                sleep(Duration::from_millis(50)).await;
            }

            step_counter += 1;
            if step_counter > max_steps {
                let pending_resource_ids: Vec<String> = self
                    .resources
                    .keys()
                    .filter(|id| !self.is_resource_synced(id, &current_state))
                    .cloned()
                    .collect();

                let error = AlienError::new(ErrorData::ExecutionMaxStepsReached {
                    max_steps: max_steps as u64,
                    pending_resources: pending_resource_ids,
                });

                return CompletionResult {
                    final_state: current_state,
                    success: false,
                    error: Some(error),
                };
            }
        }
        info!(
            platform = ?current_state.platform,
            step_counter,
            "Run until synced finished"
        );
        CompletionResult {
            final_state: current_state,
            success: true,
            error: None,
        }
    }

    fn dependencies_met(&self, node_index: NodeIndex, state: &StackState) -> bool {
        // Resolve the source resource ID from the graph index.
        let source_id = match self.node_index_to_id.get(&node_index) {
            Some(id) => id,
            None => return false, // Defensive: should never happen.
        };

        // Look up the full ResourceConfig to access the *declared* dependencies – this list
        // includes references to resources that might have been filtered out of
        // this executor run but are still required to be `Running` (or
        // `Deleted`) in the *current* stack state.
        let resource_cfg = match self.resources.get(source_id) {
            Some(cfg) => cfg,
            None => return false, // Defensive.
        };

        resource_cfg.dependencies.iter().all(|dep_ref| {
            let dep_id = dep_ref.id();
            match state.resources.get(dep_id) {
                Some(dep_view) => matches!(
                    dep_view.status,
                    ResourceStatus::Running | ResourceStatus::Deleted
                ),
                None => false, // Dependency not present in state (yet)
            }
        })
    }

    /// Checks if a resource is ready for deletion by verifying all its dependents are deleted.
    /// For deletion, the dependency logic is reversed: a resource can only be deleted when
    /// all resources that depend on it are already deleted or being deleted.
    fn deletion_ready(&self, resource_id: &str, state: &StackState) -> bool {
        // Find all resources in the state that depend on this resource
        for (dependent_id, dependent_state) in &state.resources {
            // Skip if the dependent is already deleted or being deleted
            if matches!(
                dependent_state.status,
                ResourceStatus::Deleted | ResourceStatus::Deleting
            ) {
                continue;
            }

            // Check if this dependent depends on our resource in current dependencies
            for dep_ref in &dependent_state.dependencies {
                if dep_ref.id() == resource_id {
                    // Found an active dependent - not ready for deletion
                    debug!(
                        "Resource '{}' not ready for deletion: active dependent '{}' (status: {:?})",
                        resource_id, dependent_id, dependent_state.status
                    );
                    return false;
                }
            }

            // For resources that are updating or provisioning, also check previous dependencies
            // since they might still be using the old dependencies during the transition process
            if matches!(
                dependent_state.status,
                ResourceStatus::Updating | ResourceStatus::Provisioning
            ) {
                if let Some(prev_config) = &dependent_state.previous_config {
                    for dep_ref in prev_config.get_dependencies() {
                        if dep_ref.id() == resource_id {
                            // Found a resource that is transitioning and previously depended on this resource
                            debug!(
                                "Resource '{}' not ready for deletion: resource '{}' is {:?} and previously depended on it",
                                resource_id, dependent_id, dependent_state.status
                            );
                            return false;
                        }
                    }
                }
            }
        }

        // No active dependents found - ready for deletion
        debug!(
            "Resource '{}' is ready for deletion - no active dependents",
            resource_id
        );
        true
    }

    pub(crate) fn is_resource_synced(&self, resource_id: &str, state: &StackState) -> bool {
        match state.resources.get(resource_id) {
            Some(view) => {
                // If controller exists, check its status. Otherwise (Pending), it's not terminal.
                match view.get_internal_controller() {
                    Ok(Some(controller)) => controller.get_status().is_terminal(),
                    Ok(None) => false, // No controller means not terminal (Pending)
                    Err(_) => false,   // Failed to deserialize means not terminal
                }
            }
            None => {
                // If the resource isn't even in the state map, it's effectively Pending and not terminal.
                // This also handles the case where a resource desired for deletion is already gone.
                // Check if it *should* exist according to the desired state (self.resources)
                if self.resources.contains_key(resource_id) {
                    false // Desired resource not yet in state -> not terminal
                } else {
                    true // Resource not desired and not in state -> terminal (effectively Deleted)
                }
            }
        }
    }

    /// Checks if the stack state is "synced" - meaning the desired state matches the current state.
    ///
    /// This returns true when:
    /// - All desired resources are `Running` with matching configs
    /// - All resources to be deleted are `Deleted` or `DeleteFailed`
    ///
    /// Note: This does NOT mean the stack is "done" - controllers continuously run heartbeat
    /// checks even when synced. This method is primarily used by tests and `run_until_synced`.
    pub(crate) fn is_synced(&self, state: &StackState) -> bool {
        // Check 1: All resources defined in the target stack (self.resources) must be synced AND match config.
        let desired_resources_are_synced = self
            .resources
            .iter() // Iterate over desired (id, config) pairs
            .all(|(id, desired_resource_config)| {
                match state.resources.get(id) {
                    Some(current_view) => {
                        // Resource exists in state. Check if it's Running AND config matches.
                        // DeleteFailed is NOT a valid terminal state for a *desired* resource.
                        let is_running = current_view.status == ResourceStatus::Running;

                        let config_matches =
                            current_view
                                .internal_state
                                .as_ref()
                                .is_some_and(|_controller| {
                                    current_view.config == desired_resource_config.resource
                                });

                        is_running && config_matches
                    }
                    None => {
                        false // Desired resource not even in state yet -> not terminal
                    }
                }
            });

        if !desired_resources_are_synced {
            return false;
        }

        // Check 2: All resources *existing in the state* but *not* in the target stack must be
        // synced (deleted) with respect to the current lifecycle filter. A resource is considered
        // synced when **one** of the following holds:
        //   1. It is `Deleted` or `DeleteFailed`.
        //   2. It matches the lifecycle filter *and* still has active dependents whose lifecycle
        //      is **outside** the filter. In that case we purposefully keep it `Running` and
        //      treat that as terminal for the scope of this executor.
        //   3. It does **not** match the lifecycle filter (the executor is not responsible for it).
        let filter_set_opt = &self.lifecycle_filter;

        let deleting_resources_are_synced = state.resources.iter().all(|(res_id, view)| {
            // Skip resources that are part of the desired stack – they were already handled above.
            if self.resources.contains_key(res_id) || view.is_externally_provisioned {
                return true;
            }

            let lifecycle = view.lifecycle.unwrap_or(ResourceLifecycle::Live);

            // Helper to check if this resource still has active dependents **outside** the filter.
            let has_active_dependents_outside_filter = || {
                state.resources.iter().any(|(_dep_id, dep_view)| {
                    if dep_view.status == ResourceStatus::Deleting
                        || dep_view.status == ResourceStatus::Deleted
                    {
                        return false; // Not active
                    }

                    // If dep_view has no controller (Pending) we conservatively ignore – not active yet.
                    let _dep_controller = match &dep_view.internal_state {
                        Some(controller) => controller,
                        None => return false,
                    };

                    // Check lifecycle condition – we only care about dependents that are **outside** the filter.
                    let dep_lifecycle = dep_view.lifecycle.unwrap_or(ResourceLifecycle::Live);
                    if let Some(filter_set) = filter_set_opt {
                        if filter_set.contains(&dep_lifecycle) {
                            return false; // Inside filter, ignore for this logic
                        }
                    }

                    // Does the dependent reference the current resource?
                    dep_view
                        .dependencies
                        .iter()
                        .any(|d| d.id() == res_id.as_str())
                })
            };

            match filter_set_opt {
                None => {
                    // No lifecycle filter – we expect the resource to be Deleted/DeleteFailed.
                    view.status == ResourceStatus::Deleted
                        || view.status == ResourceStatus::DeleteFailed
                }
                Some(filter_set) if filter_set.is_empty() => {
                    // Empty filter treated same as None.
                    view.status == ResourceStatus::Deleted
                        || view.status == ResourceStatus::DeleteFailed
                }
                Some(filter_set) => {
                    if !filter_set.contains(&lifecycle) {
                        // Resource outside filter – executor not responsible.
                        true
                    } else if view.status == ResourceStatus::Deleted
                        || view.status == ResourceStatus::DeleteFailed
                    {
                        true
                    } else {
                        // Matches filter and not deleted – check dependents.
                        has_active_dependents_outside_filter()
                    }
                }
            }
        });

        desired_resources_are_synced && deleting_resources_are_synced
    }
}
