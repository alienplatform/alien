use crate::permissions::{ManagementPermissions, PermissionProfile, PermissionsConfig};
use crate::{Platform, Resource, ResourceLifecycle, ResourceRef, StackInputDefinition};
use bon::Builder;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ResourceEntry {
    /// Resource configuration (can be any type of resource)
    pub config: Resource,
    /// Lifecycle management configuration for this resource
    pub lifecycle: ResourceLifecycle,
    /// Additional dependencies for this resource beyond those defined in the resource itself.
    /// The total dependencies are: resource.get_dependencies() + this list
    pub dependencies: Vec<ResourceRef>,
    /// Enable remote bindings for this resource (BYOB use case).
    /// When true, binding params are synced to StackState's `remote_binding_params`.
    /// Default: false (prevents sensitive data in synced state).
    #[serde(default)]
    pub remote_access: bool,
}

/// A bag of resources, unaware of any cloud.
#[derive(Builder, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
#[builder(start_fn = new)]
pub struct Stack {
    /// Unique identifier for the stack
    #[builder(start_fn)]
    pub id: String,
    /// Map of resource IDs to their configurations and lifecycle settings
    #[builder(field)]
    pub resources: IndexMap<String, ResourceEntry>,
    /// Combined permissions configuration containing both profiles and management
    #[builder(field)]
    #[serde(default)]
    pub permissions: PermissionsConfig,
    /// Which platforms this stack supports. When None, all platforms are supported.
    #[builder(field)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supported_platforms: Option<Vec<Platform>>,
    /// Input definitions required before setup or deployment can proceed.
    #[builder(field)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inputs: Vec<StackInputDefinition>,
}

impl Stack {
    /// Returns an iterator over the resources in the stack, including their lifecycle state.
    pub fn resources(&self) -> impl Iterator<Item = (&String, &ResourceEntry)> {
        self.resources.iter()
    }

    /// Returns a mutable iterator over the resources in the stack, including their lifecycle state.
    pub fn resources_mut(&mut self) -> impl Iterator<Item = (&String, &mut ResourceEntry)> {
        self.resources.iter_mut()
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    /// Create a reference to the current stack
    pub fn current() -> StackRef {
        StackRef::Current
    }

    /// Returns the permissions configuration for the stack.
    pub fn permissions(&self) -> &PermissionsConfig {
        &self.permissions
    }

    /// Returns the permission profiles for the stack.
    pub fn permission_profiles(&self) -> &IndexMap<String, PermissionProfile> {
        &self.permissions.profiles
    }

    /// Returns the management permissions configuration for the stack.
    pub fn management(&self) -> &ManagementPermissions {
        &self.permissions.management
    }

    /// Returns the supported platforms, or None if all platforms are supported.
    pub fn supported_platforms(&self) -> Option<&[Platform]> {
        self.supported_platforms.as_deref()
    }

    /// Returns stack input definitions.
    pub fn inputs(&self) -> &[StackInputDefinition] {
        &self.inputs
    }

    /// Returns true if the given platform is supported by this stack.
    /// When supported_platforms is None, all platforms are supported.
    pub fn supports_platform(&self, platform: &Platform) -> bool {
        match &self.supported_platforms {
            Some(platforms) => platforms.contains(platform),
            None => true,
        }
    }

    /// Returns the ordered list of command-capable targets in this stack:
    /// Worker, Container, and Daemon resources with `commands_enabled` set,
    /// in stack declaration order.
    ///
    /// Declaration order is the order resources were added to the stack
    /// (via `StackBuilder::add`/`add_with_dependencies`/`add_with_remote_access`).
    /// `resources` is an `IndexMap`, which preserves insertion order, so
    /// iterating it directly yields declaration order without any extra
    /// bookkeeping.
    pub fn command_targets(&self) -> Vec<crate::commands_types::CommandTarget> {
        use crate::commands_types::{CommandTarget, CommandTargetType};

        self.resources
            .iter()
            .filter_map(|(id, entry)| {
                if let Some(worker) = entry.config.downcast_ref::<crate::Worker>() {
                    worker
                        .commands_enabled
                        .then(|| CommandTarget::new(id.clone(), CommandTargetType::Worker))
                } else if let Some(container) = entry.config.downcast_ref::<crate::Container>() {
                    container
                        .commands_enabled
                        .then(|| CommandTarget::new(id.clone(), CommandTargetType::Container))
                } else if let Some(daemon) = entry.config.downcast_ref::<crate::Daemon>() {
                    daemon
                        .commands_enabled
                        .then(|| CommandTarget::new(id.clone(), CommandTargetType::Daemon))
                } else {
                    None
                }
            })
            .collect()
    }

    /// Returns the commands polling environment variables
    /// (`ALIEN_COMMANDS_POLLING_ENABLED`, `ALIEN_COMMANDS_POLLING_URL`,
    /// `ALIEN_COMMANDS_TOKEN`, `ALIEN_COMMANDS_TARGET_RESOURCE_ID`) for each
    /// command-enabled Worker in this stack, scoped via `target_resources` so
    /// every var reaches only its own Worker.
    ///
    /// Scoping the whole quartet (not just the target id) matters twice over:
    /// a commands-disabled Worker must never see `POLLING_ENABLED=true` — the
    /// runtime fail-fast-requires the target id once polling is on, so a
    /// deployment-wide flag would crash it at startup — and it also shouldn't
    /// run a pointless polling loop.
    ///
    /// Container/Daemon command targets are out of scope here: their commands
    /// receiver env injection is handled by [`Self::receiver_command_env_vars`]
    /// below, so this only covers polling Workers.
    pub fn worker_command_polling_env_vars(
        &self,
        polling_url: &str,
        polling_token: Option<&str>,
    ) -> Vec<crate::EnvironmentVariable> {
        self.command_targets()
            .into_iter()
            .filter(|target| {
                target.resource_type == crate::commands_types::CommandTargetType::Worker
            })
            .flat_map(|target| {
                let scope = Some(vec![target.resource_id.clone()]);
                let mut vars = vec![
                    crate::EnvironmentVariable {
                        name: crate::ENV_ALIEN_COMMANDS_POLLING_ENABLED.to_string(),
                        value: "true".to_string(),
                        var_type: crate::EnvironmentVariableType::Plain,
                        target_resources: scope.clone(),
                    },
                    crate::EnvironmentVariable {
                        name: crate::ENV_ALIEN_COMMANDS_POLLING_URL.to_string(),
                        value: polling_url.to_string(),
                        var_type: crate::EnvironmentVariableType::Plain,
                        target_resources: scope.clone(),
                    },
                ];
                if let Some(token) = polling_token {
                    vars.push(crate::EnvironmentVariable {
                        name: crate::ENV_ALIEN_COMMANDS_TOKEN.to_string(),
                        value: token.to_string(),
                        var_type: crate::EnvironmentVariableType::Secret,
                        target_resources: scope.clone(),
                    });
                }
                vars.push(crate::EnvironmentVariable {
                    name: crate::ENV_ALIEN_COMMANDS_TARGET_RESOURCE_ID.to_string(),
                    value: target.resource_id,
                    var_type: crate::EnvironmentVariableType::Plain,
                    target_resources: scope,
                });
                vars
            })
            .collect()
    }

    /// Returns the command *receiver* environment variables for each
    /// command-enabled Container and Daemon in this stack, scoped via
    /// `target_resources` so every var reaches only its own resource.
    ///
    /// This is the receiver-side sibling of [`Self::worker_command_polling_env_vars`].
    /// Workers *poll* (the quartet above); Containers and Daemons run the
    /// pull *receiver*, which reads a fixed contract of vars (PACKAGE_LAYOUT
    /// DECIDED(09), ALIEN-221/222):
    ///   - `ALIEN_COMMANDS_URL` (Plain) — base receiver URL
    ///   - `ALIEN_COMMANDS_TOKEN` (Secret, only if a token is present)
    ///   - `ALIEN_COMMANDS_TARGET_RESOURCE_ID` (Plain) — this resource's id
    ///   - `ALIEN_COMMANDS_TARGET_RESOURCE_TYPE` (Plain) — `container`/`daemon`
    ///
    /// `ALIEN_DEPLOYMENT_ID` is intentionally NOT emitted here: the manager and
    /// operator already inject it deployment-wide (`target_resources: None`), so
    /// it reaches every Container/Daemon via that path — re-scoping it per
    /// resource would be redundant. This mirrors the worker helper, which also
    /// relies on the deployment-wide `ALIEN_DEPLOYMENT_ID`.
    ///
    /// Workers are excluded here (they get the polling quartet instead), and a
    /// commands-disabled Container/Daemon is never a `command_targets()` entry,
    /// so it receives nothing — the receiver fail-fasts on a partial config, so
    /// a deployment-wide flag would crash it at startup.
    pub fn receiver_command_env_vars(
        &self,
        commands_url: &str,
        commands_token: Option<&str>,
    ) -> Vec<crate::EnvironmentVariable> {
        use crate::commands_types::CommandTargetType;

        self.command_targets()
            .into_iter()
            .filter(|target| {
                matches!(
                    target.resource_type,
                    CommandTargetType::Container | CommandTargetType::Daemon
                )
            })
            .flat_map(|target| {
                let scope = Some(vec![target.resource_id.clone()]);
                let mut vars = vec![crate::EnvironmentVariable {
                    name: crate::ENV_ALIEN_COMMANDS_URL.to_string(),
                    value: commands_url.to_string(),
                    var_type: crate::EnvironmentVariableType::Plain,
                    target_resources: scope.clone(),
                }];
                if let Some(token) = commands_token {
                    vars.push(crate::EnvironmentVariable {
                        name: crate::ENV_ALIEN_COMMANDS_TOKEN.to_string(),
                        value: token.to_string(),
                        var_type: crate::EnvironmentVariableType::Secret,
                        target_resources: scope.clone(),
                    });
                }
                vars.push(crate::EnvironmentVariable {
                    name: crate::ENV_ALIEN_COMMANDS_TARGET_RESOURCE_TYPE.to_string(),
                    value: target.resource_type.as_str().to_string(),
                    var_type: crate::EnvironmentVariableType::Plain,
                    target_resources: scope.clone(),
                });
                vars.push(crate::EnvironmentVariable {
                    name: crate::ENV_ALIEN_COMMANDS_TARGET_RESOURCE_ID.to_string(),
                    value: target.resource_id,
                    var_type: crate::EnvironmentVariableType::Plain,
                    target_resources: scope,
                });
                vars
            })
            .collect()
    }
}

impl StackBuilder {
    /// Adds a resource to the stack with its lifecycle state.
    /// The resource's intrinsic dependencies (from resource.get_dependencies()) are automatically included.
    /// Use add_with_dependencies() if you need to specify additional dependencies.
    pub fn add<T: crate::ResourceDefinition>(
        self,
        resource: T,
        lifecycle: ResourceLifecycle,
    ) -> Self {
        self.add_with_dependencies(resource, lifecycle, vec![])
    }

    /// Adds a resource to the stack with its lifecycle state and additional dependencies.
    /// The total dependencies will be: resource.get_dependencies() + additional_dependencies
    pub fn add_with_dependencies<T: crate::ResourceDefinition>(
        mut self,
        resource: T,
        lifecycle: ResourceLifecycle,
        additional_dependencies: Vec<ResourceRef>,
    ) -> Self {
        let resource = Resource::new(resource);
        self.resources.insert(
            resource.id().to_string(),
            ResourceEntry {
                config: resource,
                lifecycle,
                dependencies: additional_dependencies,
                remote_access: false,
            },
        );
        self
    }

    /// Adds a resource with remote access enabled.
    /// When remote_access is true, binding params are synced to StackState for external access.
    pub fn add_with_remote_access<T: crate::ResourceDefinition>(
        mut self,
        resource: T,
        lifecycle: ResourceLifecycle,
    ) -> Self {
        let resource = Resource::new(resource);
        self.resources.insert(
            resource.id().to_string(),
            ResourceEntry {
                config: resource,
                lifecycle,
                dependencies: vec![],
                remote_access: true,
            },
        );
        self
    }

    /// Sets the permissions configuration for the stack.
    /// This defines access control for compute services in the stack.
    pub fn permissions(mut self, permissions: PermissionsConfig) -> Self {
        self.permissions = permissions;
        self
    }

    /// Add a single permission profile to the stack - allows fluent chaining
    ///
    /// # Example
    /// ```rust
    /// # use alien_core::{Stack, permissions::PermissionProfile};
    /// Stack::new("my-stack".to_string())
    ///     .permission("execution", PermissionProfile::new().global(["storage/data-read"]))
    ///     .permission("management", PermissionProfile::new().global(["storage/management"]))
    ///     .build()
    /// # ;
    /// ```
    pub fn permission(mut self, name: impl Into<String>, profile: PermissionProfile) -> Self {
        self.permissions.profiles.insert(name.into(), profile);
        self
    }

    /// Sets the supported platforms for this stack.
    pub fn platforms(mut self, platforms: Vec<Platform>) -> Self {
        self.supported_platforms = Some(platforms);
        self
    }

    /// Sets stack input definitions.
    pub fn inputs(mut self, inputs: Vec<StackInputDefinition>) -> Self {
        self.inputs = inputs;
        self
    }

    /// Sets the management permissions configuration for the stack.
    /// This defines how management permissions are derived and configured.
    ///
    /// # Examples
    /// ```rust
    /// # use alien_core::{Stack, permissions::{ManagementPermissions, PermissionProfile}};
    /// // Auto-derived management permissions (default)
    /// Stack::new("my-stack".to_string())
    ///     .management(ManagementPermissions::auto())
    ///     .build();
    ///
    /// // Extend auto-derived permissions
    /// Stack::new("my-stack".to_string())
    ///     .management(ManagementPermissions::extend(
    ///         PermissionProfile::new().global(["vault/data-write"])
    ///     ))
    ///     .build();
    ///
    /// // Override auto-derived permissions entirely
    /// Stack::new("my-stack".to_string())
    ///     .management(ManagementPermissions::override_(
    ///         PermissionProfile::new().global(["storage/heartbeat", "worker/provision"])
    ///     ))
    ///     .build();
    /// ```
    pub fn management(mut self, management: ManagementPermissions) -> Self {
        self.permissions.management = management;
        self
    }
}

/// Reference to a stack for management permissions
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub enum StackRef {
    /// Reference to the current stack being built
    Current,
    /// Reference to another stack by ID
    External(String),
}

impl StackRef {
    /// Create a StackRef from a stack reference
    pub fn from_stack(stack: &Stack) -> Self {
        StackRef::External(stack.id().to_string())
    }
}

impl From<&Stack> for StackRef {
    fn from(stack: &Stack) -> Self {
        StackRef::External(stack.id().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resource::ResourceLifecycle;
    use crate::{
        Container, ContainerCode, Daemon, DaemonCode, PermissionSetReference, ResourceSpec,
        Storage, Worker, WorkerCode,
    };
    use insta::assert_json_snapshot;

    #[test]
    fn test_stack_serialization() {
        use crate::WorkerCode;

        let storage = Storage::new("my-bucket".to_string())
            .public_read(true)
            .build();

        let worker = Worker::new("my-worker".to_string())
            .code(WorkerCode::Image {
                image: "rust:latest".to_string(),
            })
            .permissions("execution".to_string())
            .link(&storage)
            .build();

        // Create permission profiles for the new system
        let mut permissions = IndexMap::new();
        let mut execution_profile = PermissionProfile::new();
        execution_profile.0.insert(
            "*".to_string(),
            vec![
                PermissionSetReference::from_name("storage/data-read"),
                PermissionSetReference::from_name("storage/data-write"),
            ],
        );
        permissions.insert("execution".to_string(), execution_profile);

        let stack_builder = Stack::new("test-stack".to_string())
            .add(storage, ResourceLifecycle::Frozen)
            .add(worker.clone(), ResourceLifecycle::Live);

        let stack = stack_builder
            .permissions(PermissionsConfig {
                profiles: permissions,
                management: ManagementPermissions::Auto,
            })
            .build();

        // Serialize and Deserialize
        let serialized_stack =
            serde_json::to_string_pretty(&stack).expect("Failed to serialize stack");
        let deserialized_stack: Stack =
            serde_json::from_str(&serialized_stack).expect("Failed to deserialize stack");

        // Assert equality
        assert_eq!(
            stack, deserialized_stack,
            "Original and deserialized stacks do not match."
        );

        // Verify snapshot (sort maps to be deterministic across Rust versions)
        let mut settings = insta::Settings::clone_current();
        settings.set_sort_maps(true);
        settings.bind(|| {
            assert_json_snapshot!("stack_serialization_account_managed", stack);
        });
    }

    #[test]
    fn test_empty_stack_serialization() {
        let stack_builder = Stack::new("empty-test-stack".to_string());

        let stack = stack_builder
            .permissions(PermissionsConfig::new()) // Empty permissions for existing tests
            .build();

        // Serialize and Deserialize
        let serialized_stack =
            serde_json::to_string_pretty(&stack).expect("Failed to serialize empty stack");
        let deserialized_stack: Stack =
            serde_json::from_str(&serialized_stack).expect("Failed to deserialize empty stack");

        // Assert equality
        assert_eq!(
            stack, deserialized_stack,
            "Original and deserialized empty stacks do not match."
        );

        // Verify snapshot (sort maps to be deterministic across Rust versions)
        let mut settings = insta::Settings::clone_current();
        settings.set_sort_maps(true);
        settings.bind(|| {
            assert_json_snapshot!("empty_stack_serialization_account", stack);
        });
    }

    #[test]
    fn stack_deserializes_resources_without_public_endpoints() {
        let container = Container::new("api".to_string())
            .code(ContainerCode::Image {
                image: "example.com/api:latest".to_string(),
            })
            .cpu(ResourceSpec {
                min: "0.5".to_string(),
                desired: "1".to_string(),
            })
            .memory(ResourceSpec {
                min: "512Mi".to_string(),
                desired: "1Gi".to_string(),
            })
            .port(8080)
            .permissions("container-execution".to_string())
            .build();
        let daemon = Daemon::new("agent".to_string())
            .code(DaemonCode::Image {
                image: "example.com/agent:latest".to_string(),
            })
            .permissions("daemon-execution".to_string())
            .build();
        let worker = Worker::new("worker".to_string())
            .code(WorkerCode::Image {
                image: "example.com/worker:latest".to_string(),
            })
            .permissions("worker-execution".to_string())
            .build();
        let stack = Stack::new("legacy-stack".to_string())
            .add(container, ResourceLifecycle::Live)
            .add(daemon, ResourceLifecycle::Live)
            .add(worker, ResourceLifecycle::Live)
            .build();

        let mut legacy_json = serde_json::to_value(stack).expect("stack should serialize");
        for resource_id in ["api", "agent", "worker"] {
            legacy_json
                .pointer_mut(&format!("/resources/{resource_id}/config"))
                .and_then(serde_json::Value::as_object_mut)
                .expect("resource config should be an object")
                .remove("publicEndpoints");
        }

        let stack: Stack =
            serde_json::from_value(legacy_json).expect("legacy stack should deserialize");

        let container = stack
            .resources
            .get("api")
            .and_then(|entry| entry.config.downcast_ref::<Container>())
            .expect("api should be a container");
        assert!(container.public_endpoints.is_empty());

        let daemon = stack
            .resources
            .get("agent")
            .and_then(|entry| entry.config.downcast_ref::<Daemon>())
            .expect("agent should be a daemon");
        assert!(daemon.public_endpoints.is_empty());

        let worker = stack
            .resources
            .get("worker")
            .and_then(|entry| entry.config.downcast_ref::<Worker>())
            .expect("worker should be a worker");
        assert!(worker.public_endpoints.is_empty());
    }

    #[test]
    fn test_stack_with_permissions() {
        use crate::permissions::PermissionProfile;
        use indexmap::IndexMap;

        // Create a simple stack with permissions
        let storage = Storage::new("test-storage".to_string()).build();

        // Create a permission profile
        let mut permission_profile = PermissionProfile::new();
        permission_profile.0.insert(
            "*".to_string(),
            vec![PermissionSetReference::from_name("storage/data-read")],
        );

        let mut permissions = IndexMap::new();
        permissions.insert("reader".to_string(), permission_profile);

        let stack = Stack::new("test-permissions-stack".to_string())
            .add(storage, ResourceLifecycle::Frozen)
            .permissions(PermissionsConfig {
                profiles: permissions,
                management: ManagementPermissions::Auto,
            })
            .build();

        // Verify permissions are accessible
        assert_eq!(stack.permission_profiles().len(), 1);
        assert!(stack.permission_profiles().contains_key("reader"));

        let reader_profile = stack.permission_profiles().get("reader").unwrap();
        assert_eq!(reader_profile.0.len(), 1);
        assert!(reader_profile.0.contains_key("*"));

        let global_permissions = reader_profile.0.get("*").unwrap();
        assert_eq!(
            global_permissions,
            &vec![PermissionSetReference::from_name("storage/data-read")]
        );

        // Test serialization/deserialization
        let serialized = serde_json::to_string_pretty(&stack).expect("Failed to serialize");
        let deserialized: Stack = serde_json::from_str(&serialized).expect("Failed to deserialize");
        assert_eq!(stack, deserialized);
    }

    #[test]
    fn test_stack_with_management_permissions() {
        use crate::permissions::{ManagementPermissions, PermissionProfile};

        // Create a simple stack with management permissions
        let storage = Storage::new("test-storage".to_string()).build();

        // Create a permission profile for management
        let mut management_profile = PermissionProfile::new();
        management_profile.0.insert(
            "*".to_string(),
            vec![PermissionSetReference::from_name("vault/data-write")],
        );

        // Test auto management permissions (default)
        let stack_auto = Stack::new("test-auto-management-stack".to_string())
            .add(storage.clone(), ResourceLifecycle::Frozen)
            .management(ManagementPermissions::auto())
            .build();

        assert!(stack_auto.management().is_auto());
        assert!(stack_auto.management().profile().is_none());

        // Test extend management permissions
        let stack_extend = Stack::new("test-extend-management-stack".to_string())
            .add(storage.clone(), ResourceLifecycle::Frozen)
            .management(ManagementPermissions::extend(management_profile.clone()))
            .build();

        assert!(stack_extend.management().is_extend());
        assert_eq!(
            stack_extend.management().profile().unwrap(),
            &management_profile
        );

        // Test override management permissions
        let stack_override = Stack::new("test-override-management-stack".to_string())
            .add(storage.clone(), ResourceLifecycle::Frozen)
            .management(ManagementPermissions::override_(management_profile.clone()))
            .build();

        assert!(stack_override.management().is_override());
        assert_eq!(
            stack_override.management().profile().unwrap(),
            &management_profile
        );

        // Test default management permissions
        let stack_default = Stack::new("test-default-management-stack".to_string())
            .add(storage, ResourceLifecycle::Frozen)
            .build();

        assert!(stack_default.management().is_auto());

        // Test serialization/deserialization with management
        let serialized = serde_json::to_string_pretty(&stack_extend).expect("Failed to serialize");
        let deserialized: Stack = serde_json::from_str(&serialized).expect("Failed to deserialize");
        assert_eq!(stack_extend, deserialized);
    }

    #[test]
    fn command_targets_returns_only_commands_enabled_resources_in_declaration_order() {
        let worker_enabled = Worker::new("worker-a".to_string())
            .code(WorkerCode::Image {
                image: "worker:latest".to_string(),
            })
            .permissions("execution".to_string())
            .commands_enabled(true)
            .build();

        let container_disabled = Container::new("container-b".to_string())
            .code(ContainerCode::Image {
                image: "container:latest".to_string(),
            })
            .cpu(ResourceSpec {
                min: "0.5".to_string(),
                desired: "1".to_string(),
            })
            .memory(ResourceSpec {
                min: "512Mi".to_string(),
                desired: "1Gi".to_string(),
            })
            .port(8080)
            .permissions("container-execution".to_string())
            .build();

        let daemon_enabled = Daemon::new("daemon-c".to_string())
            .code(DaemonCode::Image {
                image: "daemon:latest".to_string(),
            })
            .permissions("daemon-execution".to_string())
            .commands_enabled(true)
            .build();

        let container_enabled = Container::new("container-d".to_string())
            .code(ContainerCode::Image {
                image: "container:latest".to_string(),
            })
            .cpu(ResourceSpec {
                min: "0.5".to_string(),
                desired: "1".to_string(),
            })
            .memory(ResourceSpec {
                min: "512Mi".to_string(),
                desired: "1Gi".to_string(),
            })
            .port(8080)
            .permissions("container-execution".to_string())
            .commands_enabled(true)
            .build();

        let worker_disabled = Worker::new("worker-e".to_string())
            .code(WorkerCode::Image {
                image: "worker:latest".to_string(),
            })
            .permissions("execution".to_string())
            .build();

        let storage = Storage::new("bucket-f".to_string()).build();

        // Declaration order: worker-a (enabled), container-b (disabled),
        // daemon-c (enabled), container-d (enabled), worker-e (disabled),
        // bucket-f (not a command-capable resource type at all).
        let stack = Stack::new("command-targets-stack".to_string())
            .add(worker_enabled, ResourceLifecycle::Live)
            .add(container_disabled, ResourceLifecycle::Live)
            .add(daemon_enabled, ResourceLifecycle::Live)
            .add(container_enabled, ResourceLifecycle::Live)
            .add(worker_disabled, ResourceLifecycle::Live)
            .add(storage, ResourceLifecycle::Frozen)
            .build();

        let targets = stack.command_targets();

        assert_eq!(
            targets,
            vec![
                crate::commands_types::CommandTarget::new(
                    "worker-a",
                    crate::commands_types::CommandTargetType::Worker
                ),
                crate::commands_types::CommandTarget::new(
                    "daemon-c",
                    crate::commands_types::CommandTargetType::Daemon
                ),
                crate::commands_types::CommandTarget::new(
                    "container-d",
                    crate::commands_types::CommandTargetType::Container
                ),
            ]
        );
    }

    #[test]
    fn command_targets_empty_when_no_commands_enabled_resources() {
        let worker = Worker::new("worker-only".to_string())
            .code(WorkerCode::Image {
                image: "worker:latest".to_string(),
            })
            .permissions("execution".to_string())
            .build();

        let stack = Stack::new("no-targets-stack".to_string())
            .add(worker, ResourceLifecycle::Live)
            .build();

        assert!(stack.command_targets().is_empty());
    }

    #[test]
    fn worker_command_polling_env_vars_scopes_quartet_per_worker() {
        let worker_a = Worker::new("worker-a".to_string())
            .code(WorkerCode::Image {
                image: "worker:latest".to_string(),
            })
            .permissions("execution".to_string())
            .commands_enabled(true)
            .build();

        let worker_b = Worker::new("worker-b".to_string())
            .code(WorkerCode::Image {
                image: "worker:latest".to_string(),
            })
            .permissions("execution".to_string())
            .commands_enabled(true)
            .build();

        // Commands-DISABLED Worker: must receive NONE of the polling vars.
        // A deployment-wide POLLING_ENABLED=true would crash it at startup
        // (the runtime fail-fast-requires the target id once polling is on).
        let worker_disabled = Worker::new("worker-off".to_string())
            .code(WorkerCode::Image {
                image: "worker:latest".to_string(),
            })
            .permissions("execution".to_string())
            .build();

        let daemon_enabled = Daemon::new("daemon-c".to_string())
            .code(DaemonCode::Image {
                image: "daemon:latest".to_string(),
            })
            .permissions("daemon-execution".to_string())
            .commands_enabled(true)
            .build();

        let stack = Stack::new("worker-polling-env-stack".to_string())
            .add(worker_a, ResourceLifecycle::Live)
            .add(worker_b, ResourceLifecycle::Live)
            .add(worker_disabled, ResourceLifecycle::Live)
            .add(daemon_enabled, ResourceLifecycle::Live)
            .build();

        let vars =
            stack.worker_command_polling_env_vars("https://cmd.example.test/v1", Some("tok"));

        // Every var is scoped to exactly one command-enabled Worker — nothing
        // is deployment-wide, and neither the disabled Worker nor the Daemon
        // is ever a scope target.
        assert!(vars.iter().all(|v| {
            v.target_resources == Some(vec!["worker-a".to_string()])
                || v.target_resources == Some(vec!["worker-b".to_string()])
        }));

        // Each command-enabled Worker gets the full quartet.
        for worker_id in ["worker-a", "worker-b"] {
            let scoped: Vec<_> = vars
                .iter()
                .filter(|v| v.target_resources == Some(vec![worker_id.to_string()]))
                .collect();
            assert_eq!(scoped.len(), 4, "expected quartet for {worker_id}");
            assert!(scoped.iter().any(|v| {
                v.name == crate::ENV_ALIEN_COMMANDS_POLLING_ENABLED && v.value == "true"
            }));
            assert!(scoped.iter().any(|v| {
                v.name == crate::ENV_ALIEN_COMMANDS_POLLING_URL
                    && v.value == "https://cmd.example.test/v1"
            }));
            assert!(scoped.iter().any(|v| {
                v.name == crate::ENV_ALIEN_COMMANDS_TOKEN
                    && v.value == "tok"
                    && v.var_type == crate::EnvironmentVariableType::Secret
            }));
            assert!(scoped.iter().any(|v| {
                v.name == crate::ENV_ALIEN_COMMANDS_TARGET_RESOURCE_ID && v.value == worker_id
            }));
        }
    }

    #[test]
    fn worker_command_polling_env_vars_omits_token_when_absent() {
        let worker = Worker::new("worker-a".to_string())
            .code(WorkerCode::Image {
                image: "worker:latest".to_string(),
            })
            .permissions("execution".to_string())
            .commands_enabled(true)
            .build();

        let stack = Stack::new("worker-no-token-stack".to_string())
            .add(worker, ResourceLifecycle::Live)
            .build();

        let vars = stack.worker_command_polling_env_vars("https://cmd.example.test/v1", None);

        assert_eq!(vars.len(), 3);
        assert!(!vars
            .iter()
            .any(|v| v.name == crate::ENV_ALIEN_COMMANDS_TOKEN));
    }

    #[test]
    fn receiver_command_env_vars_scopes_contract_per_container_and_daemon() {
        let container_a = Container::new("container-a".to_string())
            .code(ContainerCode::Image {
                image: "container:latest".to_string(),
            })
            .cpu(ResourceSpec {
                min: "0.5".to_string(),
                desired: "1".to_string(),
            })
            .memory(ResourceSpec {
                min: "512Mi".to_string(),
                desired: "1Gi".to_string(),
            })
            .port(8080)
            .permissions("container-execution".to_string())
            .commands_enabled(true)
            .build();

        let daemon_b = Daemon::new("daemon-b".to_string())
            .code(DaemonCode::Image {
                image: "daemon:latest".to_string(),
            })
            .permissions("daemon-execution".to_string())
            .commands_enabled(true)
            .build();

        // Commands-DISABLED container: must receive NONE of the receiver vars.
        let container_off = Container::new("container-off".to_string())
            .code(ContainerCode::Image {
                image: "container:latest".to_string(),
            })
            .cpu(ResourceSpec {
                min: "0.5".to_string(),
                desired: "1".to_string(),
            })
            .memory(ResourceSpec {
                min: "512Mi".to_string(),
                desired: "1Gi".to_string(),
            })
            .port(8080)
            .permissions("container-execution".to_string())
            .build();

        // Commands-enabled Worker: gets the polling quartet, NOT the receiver
        // contract — it must never be a receiver scope target.
        let worker_enabled = Worker::new("worker-c".to_string())
            .code(WorkerCode::Image {
                image: "worker:latest".to_string(),
            })
            .permissions("execution".to_string())
            .commands_enabled(true)
            .build();

        let stack = Stack::new("receiver-env-stack".to_string())
            .add(container_a, ResourceLifecycle::Live)
            .add(daemon_b, ResourceLifecycle::Live)
            .add(container_off, ResourceLifecycle::Live)
            .add(worker_enabled, ResourceLifecycle::Live)
            .build();

        let vars = stack.receiver_command_env_vars("https://cmd.example.test/v1", Some("tok"));

        // Every var is scoped to exactly one command-enabled Container/Daemon —
        // nothing is deployment-wide, and neither the disabled container nor the
        // Worker is ever a scope target.
        assert!(vars.iter().all(|v| {
            v.target_resources == Some(vec!["container-a".to_string()])
                || v.target_resources == Some(vec!["daemon-b".to_string()])
        }));

        // No polling vars and no ALIEN_DEPLOYMENT_ID leak in (deployment-wide).
        assert!(!vars.iter().any(|v| {
            v.name == crate::ENV_ALIEN_COMMANDS_POLLING_ENABLED
                || v.name == crate::ENV_ALIEN_COMMANDS_POLLING_URL
                || v.name == crate::ENV_ALIEN_DEPLOYMENT_ID
        }));

        for (resource_id, expected_type) in [("container-a", "container"), ("daemon-b", "daemon")] {
            let scoped: Vec<_> = vars
                .iter()
                .filter(|v| v.target_resources == Some(vec![resource_id.to_string()]))
                .collect();
            assert_eq!(
                scoped.len(),
                4,
                "expected 4 receiver vars for {resource_id}"
            );
            assert!(scoped.iter().any(|v| {
                v.name == crate::ENV_ALIEN_COMMANDS_URL
                    && v.value == "https://cmd.example.test/v1"
                    && v.var_type == crate::EnvironmentVariableType::Plain
            }));
            assert!(scoped.iter().any(|v| {
                v.name == crate::ENV_ALIEN_COMMANDS_TOKEN
                    && v.value == "tok"
                    && v.var_type == crate::EnvironmentVariableType::Secret
            }));
            assert!(scoped.iter().any(|v| {
                v.name == crate::ENV_ALIEN_COMMANDS_TARGET_RESOURCE_ID && v.value == resource_id
            }));
            assert!(scoped.iter().any(|v| {
                v.name == crate::ENV_ALIEN_COMMANDS_TARGET_RESOURCE_TYPE
                    && v.value == expected_type
                    && v.var_type == crate::EnvironmentVariableType::Plain
            }));
        }
    }

    #[test]
    fn receiver_command_env_vars_omits_token_when_absent() {
        let container = Container::new("container-a".to_string())
            .code(ContainerCode::Image {
                image: "container:latest".to_string(),
            })
            .cpu(ResourceSpec {
                min: "0.5".to_string(),
                desired: "1".to_string(),
            })
            .memory(ResourceSpec {
                min: "512Mi".to_string(),
                desired: "1Gi".to_string(),
            })
            .port(8080)
            .permissions("container-execution".to_string())
            .commands_enabled(true)
            .build();

        let stack = Stack::new("receiver-no-token-stack".to_string())
            .add(container, ResourceLifecycle::Live)
            .build();

        let vars = stack.receiver_command_env_vars("https://cmd.example.test/v1", None);

        assert_eq!(vars.len(), 3);
        assert!(!vars
            .iter()
            .any(|v| v.name == crate::ENV_ALIEN_COMMANDS_TOKEN));
        assert!(vars
            .iter()
            .any(|v| v.name == crate::ENV_ALIEN_COMMANDS_TARGET_RESOURCE_TYPE
                && v.value == "container"));
    }

    #[test]
    fn receiver_command_env_vars_empty_without_command_targets() {
        let worker = Worker::new("worker-only".to_string())
            .code(WorkerCode::Image {
                image: "worker:latest".to_string(),
            })
            .permissions("execution".to_string())
            .commands_enabled(true)
            .build();

        let stack = Stack::new("receiver-worker-only-stack".to_string())
            .add(worker, ResourceLifecycle::Live)
            .build();

        // Only a Worker target exists → receiver helper yields nothing.
        assert!(stack
            .receiver_command_env_vars("https://cmd.example.test/v1", Some("tok"))
            .is_empty());
    }
}
