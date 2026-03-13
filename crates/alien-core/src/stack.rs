use crate::permissions::{ManagementPermissions, PermissionProfile, PermissionsConfig};
use crate::{Resource, ResourceLifecycle, ResourceRef};
use bon::Builder;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
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
#[serde(rename_all = "camelCase", deny_unknown_fields)]
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
    ///         PermissionProfile::new().global(["storage/management", "function/management"])
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
    use crate::{Function, PermissionSetReference, Storage};
    use insta::assert_json_snapshot;

    #[test]
    fn test_stack_serialization() {
        use crate::FunctionCode;

        let storage = Storage::new("my-bucket".to_string())
            .public_read(true)
            .build();

        let function = Function::new("my-function".to_string())
            .code(FunctionCode::Image {
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
            .add(function.clone(), ResourceLifecycle::Live);

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

        // Verify snapshot
        assert_json_snapshot!("stack_serialization_account_managed", stack);
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

        // Verify snapshot
        assert_json_snapshot!("empty_stack_serialization_account", stack);
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
}
