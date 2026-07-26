use crate::permissions::{ManagementPermissions, PermissionProfile, PermissionsConfig};
use crate::{Platform, Resource, ResourceLifecycle, ResourceRef, StackInputDefinition};
use bon::Builder;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

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
    /// Id of the boolean stack input that decides whether this resource is
    /// created at all. `None` means always create it.
    ///
    /// Set by `.enabled(input)` in the SDK. Setup emitters render the resource
    /// conditionally on the matching template variable, so a deployer who says no
    /// never gets the resource, its outputs, or anything derived from it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled_when: Option<String>,
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
    /// Returns a deterministic digest of the complete Frozen resource set.
    /// Resource and object-key ordering do not affect the digest.
    pub fn frozen_resources_digest(&self) -> String {
        let mut resources = self
            .resources
            .iter()
            .filter(|(_, entry)| entry.lifecycle == ResourceLifecycle::Frozen)
            .map(|(id, entry)| {
                let mut value =
                    serde_json::to_value(entry).expect("resource entries always serialize to JSON");
                canonicalize_json(&mut value);
                (id, value)
            })
            .collect::<Vec<_>>();
        resources.sort_unstable_by(|(left, _), (right, _)| left.cmp(right));

        let encoded = serde_json::to_vec(&resources)
            .expect("canonical Frozen resource projection always serializes");
        format!("{:x}", Sha256::digest(encoded))
    }
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
}

fn canonicalize_json(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Array(values) => {
            for value in values {
                canonicalize_json(value);
            }
        }
        serde_json::Value::Object(object) => {
            let mut entries = std::mem::take(object).into_iter().collect::<Vec<_>>();
            entries.sort_unstable_by(|(left, _), (right, _)| left.cmp(right));
            for (key, mut value) in entries {
                canonicalize_json(&mut value);
                object.insert(key, value);
            }
        }
        _ => {}
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
        self,
        resource: T,
        lifecycle: ResourceLifecycle,
        additional_dependencies: Vec<ResourceRef>,
    ) -> Self {
        let mut entry = Self::entry(resource, lifecycle);
        entry.dependencies = additional_dependencies;
        self.insert(entry)
    }

    /// Adds a resource whose creation follows a boolean stack input.
    /// The deployer's answer decides whether it is provisioned at all.
    ///
    /// Stacks are authored through the TypeScript SDK's `.enabled(input)`, which
    /// sets the field on the resource; this is the Rust-side seam the generator
    /// and preflight tests build gated stacks with.
    #[doc(hidden)]
    pub fn add_enabled_when<T: crate::ResourceDefinition>(
        self,
        resource: T,
        lifecycle: ResourceLifecycle,
        input_id: impl Into<String>,
    ) -> Self {
        let mut entry = Self::entry(resource, lifecycle);
        entry.enabled_when = Some(input_id.into());
        self.insert(entry)
    }

    /// Adds a resource with remote access enabled.
    /// When remote_access is true, binding params are synced to StackState for external access.
    pub fn add_with_remote_access<T: crate::ResourceDefinition>(
        self,
        resource: T,
        lifecycle: ResourceLifecycle,
    ) -> Self {
        let mut entry = Self::entry(resource, lifecycle);
        entry.remote_access = true;
        self.insert(entry)
    }

    /// The only place a `ResourceEntry` is spelled out. Each public `add_*`
    /// varies one field of it, so a new per-entry field costs one edit here
    /// instead of one per method.
    fn entry<T: crate::ResourceDefinition>(
        resource: T,
        lifecycle: ResourceLifecycle,
    ) -> ResourceEntry {
        ResourceEntry {
            config: Resource::new(resource),
            lifecycle,
            dependencies: Vec::new(),
            remote_access: false,
            enabled_when: None,
        }
    }

    fn insert(mut self, entry: ResourceEntry) -> Self {
        self.resources.insert(entry.config.id().to_string(), entry);
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
    fn frozen_resource_digest_is_order_independent_and_ignores_live_resources() {
        let first = Stack::new("first".to_string())
            .add(
                Storage::new("alpha".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                Storage::new("beta".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                Storage::new("live-one".to_string()).build(),
                ResourceLifecycle::Live,
            )
            .build();
        let second = Stack::new("second".to_string())
            .add(
                Storage::new("beta".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                Storage::new("alpha".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                Storage::new("live-two".to_string()).build(),
                ResourceLifecycle::Live,
            )
            .build();

        assert_eq!(
            first.frozen_resources_digest(),
            second.frozen_resources_digest()
        );
    }
}
