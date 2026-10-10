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

impl ResourceEntry {
    /// Returns intrinsic and stack-authored dependencies in planner order.
    pub fn combined_dependencies(&self) -> Vec<ResourceRef> {
        let mut dependencies = self.config.get_dependencies();
        dependencies.extend(self.dependencies.clone());
        dependencies
    }

    /// Returns whether this resource is published through Remote Bindings.
    ///
    /// Provider emitters use this generic signal to create the stack-level
    /// Remote Bindings identity. Each resource emitter remains responsible for
    /// granting that identity only the resource's declared data-plane access.
    pub fn has_remote_bindings(&self) -> bool {
        crate::remote_bindings::remote_binding_for_entry(self).is_some()
    }

    /// Whether the controller's non-secret binding locator must be synchronized
    /// into stack state. The built-in secrets vault is consumed by the manager,
    /// but is not exposed through the external Remote Bindings API.
    pub fn publishes_binding_params(&self) -> bool {
        self.remote_access
            || self
                .config
                .downcast_ref::<crate::Vault>()
                .is_some_and(|vault| vault.id == "secrets")
    }
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
    /// Exact image repositories approved for containers created after installation.
    /// The runtime API also requires an immutable SHA-256 digest.
    #[builder(field)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dynamic_container_repositories: Vec<String>,
    /// Released Container resources whose image repositories are approved for
    /// containers created after installation.
    #[builder(field)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dynamic_container_image_resources: Vec<String>,
    /// Operations this stack's deployments run: the plugins, their settings, and
    /// which operations run without approval. Changing them takes a new release.
    #[builder(field)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operations: Option<crate::OperationsConfig>,
}

impl Stack {
    /// Digest of what setup owns: every Frozen entry plus each Live sandbox's setup inputs. Without
    /// a Live sandbox the bytes equal the Frozen-only projection. Declined gated entries must be
    /// stripped by the caller.
    pub fn setup_owned_digest(&self) -> String {
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

        let mut live_sandbox_inputs = self
            .resources
            .iter()
            .filter(|(_, entry)| entry.lifecycle == ResourceLifecycle::Live)
            .filter_map(|(id, entry)| {
                let sandbox = entry.config.downcast_ref::<crate::Sandbox>()?;
                let mut inputs = crate::sandbox_setup_inputs::comparable_aws_sandbox_setup_inputs(
                    self,
                    sandbox,
                    entry.lifecycle,
                );
                for (_, value) in &mut inputs {
                    canonicalize_json(value);
                }
                Some((id, inputs))
            })
            .collect::<Vec<_>>();
        live_sandbox_inputs.sort_unstable_by_key(|(id, _)| *id);

        let encoded = if live_sandbox_inputs.is_empty() {
            serde_json::to_vec(&resources)
        } else {
            serde_json::to_vec(&(&resources, &live_sandbox_inputs))
        }
        .expect("canonical setup-owned projection always serializes");
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

    /// Declares the operations this stack's deployments run.
    pub fn operations(mut self, operations: crate::OperationsConfig) -> Self {
        self.operations = Some(operations);
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

    fn resource_entry<T: crate::ResourceDefinition>(
        resource: T,
        lifecycle: ResourceLifecycle,
        remote_access: bool,
    ) -> ResourceEntry {
        ResourceEntry {
            config: Resource::new(resource),
            lifecycle,
            dependencies: Vec::new(),
            remote_access,
            enabled_when: None,
        }
    }

    /// The grant is rendered by setup, so a resource setup renders nothing for — a Live bucket —
    /// cannot be published, while a Live sandbox can: setup still installs its scaffolding.
    #[test]
    fn remote_bindings_require_a_setup_rendered_resource_and_opt_in() {
        assert!(resource_entry(
            Storage::new("archive".to_string()).build(),
            ResourceLifecycle::Frozen,
            true,
        )
        .has_remote_bindings());
        assert!(!resource_entry(
            Storage::new("archive".to_string()).build(),
            ResourceLifecycle::Frozen,
            false,
        )
        .has_remote_bindings());
        assert!(!resource_entry(
            Storage::new("archive".to_string()).build(),
            ResourceLifecycle::Live,
            true,
        )
        .has_remote_bindings());
        let sandbox = crate::Sandbox::new("agents".to_string())
            .code(crate::SandboxCode::Image {
                image: "s3://alien-bundles/sandbox/bundle.zip".to_string(),
            })
            .egress(crate::SandboxEgress::Allow)
            .lifecycle(crate::SandboxLifecyclePolicy {
                max_lifetime_seconds: None,
                idle_pause_seconds: None,
            })
            .build();
        assert!(resource_entry(sandbox, ResourceLifecycle::Live, true).has_remote_bindings());
        assert!(!resource_entry(
            Worker::new("worker".to_string())
                .code(WorkerCode::Image {
                    image: "example.com/worker:latest".to_string(),
                })
                .permissions("worker-execution".to_string())
                .build(),
            ResourceLifecycle::Frozen,
            true,
        )
        .has_remote_bindings());
    }

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
            // Snapshot the JSON wire representation. The generic snapshot serializer
            // exposes serde_json's private arbitrary-precision Number wrapper.
            let mut json = serde_json::to_value(&stack).expect("serialize stack as JSON");
            json.sort_all_objects();
            insta::assert_snapshot!(
                "stack_serialization_account_managed",
                serde_json::to_string_pretty(&json).expect("format stack JSON")
            );
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

    fn digest_sandbox(id: &str, image: &str, private_base_image: Option<&str>) -> crate::Sandbox {
        crate::Sandbox::new(id.to_string())
            .code(crate::SandboxCode::Image {
                image: image.to_string(),
            })
            .maybe_private_base_image(private_base_image.map(str::to_string))
            .egress(crate::SandboxEgress::Deny)
            .lifecycle(crate::SandboxLifecyclePolicy {
                max_lifetime_seconds: None,
                idle_pause_seconds: None,
            })
            .build()
    }

    /// Stored setup authorizations carry this digest, so its bytes must not move for any stack
    /// without a Live sandbox.
    #[test]
    fn setup_owned_digest_is_stable_without_a_live_sandbox() {
        let stack = Stack::new("golden".to_string())
            .add(
                Storage::new("ledger".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                digest_sandbox(
                    "frozen-agents",
                    "s3://bucket/frozen.zip",
                    Some("123456789012.dkr.ecr.us-east-1.amazonaws.com/team/base:1"),
                ),
                ResourceLifecycle::Frozen,
            )
            .add(
                Storage::new("scratch".to_string()).build(),
                ResourceLifecycle::Live,
            )
            .build();

        assert_eq!(
            stack.setup_owned_digest(),
            "aacf34583a0bb9e3d48dd08be0bdd1da0f1d1e3666f93b844ecb0e1506ca537e"
        );
    }

    struct LiveSandbox {
        image: &'static str,
        private_base_image: Option<&'static str>,
        egress: crate::SandboxEgress,
        network_id: &'static str,
        remote_access: bool,
    }

    const BASE_A: &str = "123456789012.dkr.ecr.us-east-1.amazonaws.com/team/base-a:1";

    impl LiveSandbox {
        fn installed() -> Self {
            Self {
                image: "s3://bucket/sandbox-bundle/v1/bundle.zip",
                private_base_image: Some(BASE_A),
                egress: crate::SandboxEgress::Allow,
                network_id: "net-a",
                remote_access: false,
            }
        }

        fn digest(self) -> String {
            let mut sandbox = digest_sandbox("agents", self.image, self.private_base_image);
            sandbox.egress = self.egress;
            let mut stack = Stack::new("stack".to_string())
                .add(
                    crate::Network::new(self.network_id.to_string())
                        .settings(crate::NetworkSettings::Create {
                            cidr: Some("10.0.0.0/16".to_string()),
                            availability_zones: 2,
                        })
                        .build(),
                    ResourceLifecycle::Frozen,
                )
                .add(sandbox, ResourceLifecycle::Live)
                .build();
            stack
                .resources
                .get_mut("agents")
                .expect("sandbox")
                .remote_access = self.remote_access;
            let entry = &stack.resources["agents"];
            crate::sandbox_setup_inputs::aws_sandbox_setup_inputs(
                &stack,
                entry.config.downcast_ref().expect("sandbox"),
                entry.lifecycle,
                crate::sandbox_setup_inputs::SETUP_INPUTS_COMPARISON_ACCOUNT,
            )
            .expect("the inputs resolve, so no case is compared by its whole configuration");
            stack.setup_owned_digest()
        }
    }

    #[test]
    fn setup_owned_digest_follows_a_live_sandboxs_setup_inputs_but_not_its_image() {
        let installed = LiveSandbox::installed().digest();

        for (unchanged, why) in [
            (
                LiveSandbox {
                    image: "s3://bucket/sandbox-bundle/v2/bundle.zip",
                    ..LiveSandbox::installed()
                },
                "a new bundle under the same prefix",
            ),
            (
                LiveSandbox {
                    private_base_image: Some(
                        "123456789012.dkr.ecr.us-east-1.amazonaws.com/team/base-a:2",
                    ),
                    ..LiveSandbox::installed()
                },
                "a new tag",
            ),
            (
                LiveSandbox {
                    private_base_image: Some(
                        "123456789012.dkr.ecr.us-east-1.amazonaws.com/team/base-a@sha256:abc",
                    ),
                    ..LiveSandbox::installed()
                },
                "a digest",
            ),
        ] {
            assert_eq!(
                installed,
                unchanged.digest(),
                "{why} is the runtime's to roll"
            );
        }

        for (changed, what) in [
            (
                LiveSandbox {
                    private_base_image: None,
                    ..LiveSandbox::installed()
                },
                "a dropped private base",
            ),
            (
                LiveSandbox {
                    private_base_image: Some(
                        "123456789012.dkr.ecr.us-east-1.amazonaws.com/team/base-b:1",
                    ),
                    ..LiveSandbox::installed()
                },
                "another repository",
            ),
            (
                LiveSandbox {
                    private_base_image: Some(
                        "210987654321.dkr.ecr.us-east-1.amazonaws.com/team/base-a:1",
                    ),
                    ..LiveSandbox::installed()
                },
                "another account",
            ),
            (
                LiveSandbox {
                    private_base_image: Some(
                        "123456789012.dkr.ecr.{region}.amazonaws.com/team/base-a:1",
                    ),
                    ..LiveSandbox::installed()
                },
                "the deployment's region",
            ),
            (
                LiveSandbox {
                    egress: crate::SandboxEgress::Deny,
                    ..LiveSandbox::installed()
                },
                "egress allow to deny",
            ),
            (
                LiveSandbox {
                    image: "s3://other-bucket/sandbox-bundle/v1/bundle.zip",
                    ..LiveSandbox::installed()
                },
                "another bundle bucket",
            ),
            (
                LiveSandbox {
                    image: "s3://bucket/other-prefix/v1/bundle.zip",
                    ..LiveSandbox::installed()
                },
                "another bundle prefix",
            ),
            (
                LiveSandbox {
                    remote_access: true,
                    ..LiveSandbox::installed()
                },
                "a remote grant",
            ),
        ] {
            assert_ne!(installed, changed.digest(), "{what} is setup-owned");
        }

        let deny = |network_id| {
            LiveSandbox {
                egress: crate::SandboxEgress::Deny,
                network_id,
                ..LiveSandbox::installed()
            }
            .digest()
        };
        assert_ne!(
            deny("net-a"),
            deny("net-b"),
            "the connector's network is setup-owned"
        );
    }

    #[test]
    fn setup_owned_digest_is_order_independent() {
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

        assert_eq!(first.setup_owned_digest(), second.setup_owned_digest());
    }
}
