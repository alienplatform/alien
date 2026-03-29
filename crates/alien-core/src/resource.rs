use crate::error::Result;
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::borrow::Cow;
use std::fmt::Debug;
#[cfg(feature = "openapi")]
use utoipa::openapi::schema::AdditionalProperties;
#[cfg(feature = "openapi")]
use utoipa::openapi::{ObjectBuilder, Ref, RefOr, Schema, Type};
#[cfg(feature = "openapi")]
use utoipa::{PartialSchema, ToSchema};

/// Type alias for resource type identifiers
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ResourceType(pub Cow<'static, str>);

impl ResourceType {
    /// Create a new ResourceType from a static string (const-friendly)
    pub const fn from_static(s: &'static str) -> Self {
        Self(Cow::Borrowed(s))
    }
}

impl From<String> for ResourceType {
    fn from(s: String) -> Self {
        Self(Cow::Owned(s))
    }
}

impl From<&str> for ResourceType {
    fn from(s: &str) -> Self {
        Self(Cow::Owned(s.to_string()))
    }
}

impl std::fmt::Display for ResourceType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<ResourceType> for String {
    fn from(val: ResourceType) -> Self {
        val.0.into_owned()
    }
}

impl AsRef<str> for ResourceType {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

#[cfg(feature = "openapi")]
impl PartialSchema for ResourceType {
    fn schema() -> RefOr<Schema> {
        RefOr::T(Schema::Object(
            ObjectBuilder::new()
                .schema_type(Type::String)
                .description(Some("Resource type identifier that determines the specific kind of resource. This field is used for polymorphic deserialization and resource-specific behavior."))
                .examples([
                    "function",
                    "storage",
                    "queue",
                    "redis",
                    "postgres"
                ])
                .build()
        ))
    }
}

#[cfg(feature = "openapi")]
impl ToSchema for ResourceType {
    fn name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("ResourceType")
    }
}

/// Trait that defines the interface for all resource types in the Alien system.
/// This trait enables extensibility by allowing new resource types to be registered
/// and managed alongside built-in resources.
pub trait ResourceDefinition: Debug + Send + Sync + 'static {
    /// Returns the resource type for this instance
    fn get_resource_type(&self) -> ResourceType;

    /// Returns the unique identifier for this specific resource instance
    fn id(&self) -> &str;

    /// Returns the list of other resources this resource depends on
    fn get_dependencies(&self) -> Vec<ResourceRef>;

    /// Returns the permission profile name for this resource, if it has one.
    ///
    /// Used by `ServiceAccountDependenciesMutation` to wire the corresponding
    /// `{profile}-sa` service account as a declared dependency so the executor
    /// enforces ordering and propagates SA changes automatically.
    ///
    /// Override in concrete types that carry a `permissions` field (Container, Function).
    fn get_permissions(&self) -> Option<&str> {
        None
    }

    /// Validates if an update from the current configuration to a new configuration is allowed
    fn validate_update(&self, new_config: &dyn ResourceDefinition) -> Result<()>;

    /// Provides access to the underlying concrete type for downcasting
    fn as_any(&self) -> &dyn Any;

    /// Provides mutable access to the underlying concrete type for downcasting
    fn as_any_mut(&mut self) -> &mut dyn Any;

    /// Creates a boxed clone of this resource definition
    fn box_clone(&self) -> Box<dyn ResourceDefinition>;

    /// For equality comparison between resource definitions
    fn resource_eq(&self, other: &dyn ResourceDefinition) -> bool;

    /// Serialize this resource to a JSON value (without the "type" tag - that's added by Resource)
    fn to_json_value(&self) -> serde_json::Result<serde_json::Value>;
}

/// Clone implementation for boxed ResourceDefinition trait objects
impl Clone for Box<dyn ResourceDefinition> {
    fn clone(&self) -> Self {
        self.box_clone()
    }
}

#[derive(Debug, Clone)]
pub struct Resource {
    inner: Box<dyn ResourceDefinition>,
}

impl Serialize for Resource {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        let mut v = self.inner.to_json_value().map_err(serde::ser::Error::custom)?;
        v.as_object_mut()
            .ok_or_else(|| serde::ser::Error::custom("resource must serialize as object"))?
            .insert("type".into(), serde_json::Value::String(self.inner.get_resource_type().0.into_owned()));
        v.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Resource {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        let mut value = serde_json::Value::deserialize(deserializer)?;
        let type_tag = value.get("type").and_then(|v| v.as_str())
            .ok_or_else(|| serde::de::Error::missing_field("type"))?
            .to_string();

        // Remove the "type" tag before passing to concrete deserializer
        // (structs with deny_unknown_fields would reject it)
        if let Some(obj) = value.as_object_mut() {
            obj.remove("type");
        }

        let inner: Box<dyn ResourceDefinition> = match type_tag.as_str() {
            "vault" => Box::new(serde_json::from_value::<crate::resources::Vault>(value).map_err(serde::de::Error::custom)?),
            "function" => Box::new(serde_json::from_value::<crate::resources::Function>(value).map_err(serde::de::Error::custom)?),
            "container" => Box::new(serde_json::from_value::<crate::resources::Container>(value).map_err(serde::de::Error::custom)?),
            "container-cluster" => Box::new(serde_json::from_value::<crate::resources::ContainerCluster>(value).map_err(serde::de::Error::custom)?),
            "storage" => Box::new(serde_json::from_value::<crate::resources::Storage>(value).map_err(serde::de::Error::custom)?),
            "queue" => Box::new(serde_json::from_value::<crate::resources::Queue>(value).map_err(serde::de::Error::custom)?),
            "kv" => Box::new(serde_json::from_value::<crate::resources::Kv>(value).map_err(serde::de::Error::custom)?),
            "network" => Box::new(serde_json::from_value::<crate::resources::Network>(value).map_err(serde::de::Error::custom)?),
            "build" => Box::new(serde_json::from_value::<crate::resources::Build>(value).map_err(serde::de::Error::custom)?),
            "service-account" => Box::new(serde_json::from_value::<crate::resources::ServiceAccount>(value).map_err(serde::de::Error::custom)?),
            "artifact-registry" => Box::new(serde_json::from_value::<crate::resources::ArtifactRegistry>(value).map_err(serde::de::Error::custom)?),
            "service_activation" => Box::new(serde_json::from_value::<crate::resources::ServiceActivation>(value).map_err(serde::de::Error::custom)?),
            "remote-stack-management" => Box::new(serde_json::from_value::<crate::resources::RemoteStackManagement>(value).map_err(serde::de::Error::custom)?),
            "azure_resource_group" => Box::new(serde_json::from_value::<crate::resources::AzureResourceGroup>(value).map_err(serde::de::Error::custom)?),
            "azure_storage_account" => Box::new(serde_json::from_value::<crate::resources::AzureStorageAccount>(value).map_err(serde::de::Error::custom)?),
            "azure_container_apps_environment" => Box::new(serde_json::from_value::<crate::resources::AzureContainerAppsEnvironment>(value).map_err(serde::de::Error::custom)?),
            "azure_service_bus_namespace" => Box::new(serde_json::from_value::<crate::resources::AzureServiceBusNamespace>(value).map_err(serde::de::Error::custom)?),
            other => return Err(serde::de::Error::unknown_variant(other, &[
                "vault", "function", "container", "container-cluster", "storage", "queue", "kv",
                "network", "build", "service-account", "artifact-registry", "service_activation",
                "remote-stack-management", "azure_resource_group", "azure_storage_account",
                "azure_container_apps_environment", "azure_service_bus_namespace",
            ])),
        };

        Ok(Resource { inner })
    }
}

impl Resource {
    /// Creates a new Resource from any type that implements ResourceDefinition
    pub fn new<T: ResourceDefinition>(resource: T) -> Self {
        Self {
            inner: Box::new(resource),
        }
    }

    /// Creates a new Resource from a boxed ResourceDefinition
    pub fn from_boxed(boxed_resource: Box<dyn ResourceDefinition>) -> Self {
        Self {
            inner: boxed_resource,
        }
    }

    /// Returns the resource type identifier
    pub fn resource_type(&self) -> ResourceType {
        self.inner.get_resource_type()
    }

    /// Returns the unique identifier for this resource instance
    pub fn id(&self) -> &str {
        self.inner.id()
    }

    /// Returns the list of other resources this resource depends on
    pub fn get_dependencies(&self) -> Vec<ResourceRef> {
        self.inner.get_dependencies()
    }

    /// Returns the permission profile name for this resource, if it has one.
    pub fn get_permissions(&self) -> Option<&str> {
        self.inner.get_permissions()
    }

    /// Validates if an update from the current configuration to a new configuration is allowed
    pub fn validate_update(&self, new_config: &Resource) -> Result<()> {
        self.inner.validate_update(new_config.inner.as_ref())
    }

    /// Provides access to the underlying ResourceDefinition trait object
    pub fn as_resource_definition(&self) -> &dyn ResourceDefinition {
        self.inner.as_ref()
    }

    /// Generic downcasting for any type
    pub fn downcast_ref<T: ResourceDefinition + 'static>(&self) -> Option<&T> {
        self.inner.as_any().downcast_ref::<T>()
    }

    /// Generic mutable downcasting for any type
    pub fn downcast_mut<T: ResourceDefinition + 'static>(&mut self) -> Option<&mut T> {
        self.inner.as_any_mut().downcast_mut::<T>()
    }
}

impl PartialEq for Resource {
    fn eq(&self, other: &Self) -> bool {
        self.inner.resource_eq(other.inner.as_ref())
    }
}

impl Eq for Resource {}

/// OpenAPI schema implementation for Resource.
///
/// The schema represents the flattened JSON structure of any resource type in the Alien system.
/// All resources have a common base structure with `type` and `id` fields, plus type-specific
/// additional properties that vary depending on the concrete resource implementation.
///
/// # Schema Structure
/// - `type` (required): The resource type identifier (e.g., "function", "storage", "queue")
/// - `id` (required): The unique identifier for this specific resource instance
/// - Additional properties: Type-specific fields that vary by resource type (e.g., Function has `code`, `memory_mb`, etc.)
///
/// # Example JSON
/// ```json
/// {
///   "type": "function",
///   "id": "my-function",
///   "code": { "type": "image", "image": "my-image:latest" },
///   "memoryMb": 512,
///   "timeoutSeconds": 30
/// }
/// ```
#[cfg(feature = "openapi")]
impl PartialSchema for Resource {
    fn schema() -> RefOr<Schema> {
        RefOr::T(Schema::Object(
            ObjectBuilder::new()
                .schema_type(Type::Object)
                .property("type", Ref::from_schema_name("ResourceType"))
                .property("id",
                    ObjectBuilder::new()
                        .schema_type(Type::String)
                        .description(Some("The unique identifier for this specific resource instance. Must contain only alphanumeric characters, hyphens, and underscores ([A-Za-z0-9-_]). Maximum 64 characters."))
                        .build()
                )
                .required("type")
                .required("id")
                .additional_properties(Some(AdditionalProperties::FreeForm(true)))
                .description(Some("Resource that can hold any resource type in the Alien system. All resources share common 'type' and 'id' fields with additional type-specific properties."))
                .build()
        ))
    }
}

#[cfg(feature = "openapi")]
impl ToSchema for Resource {
    fn name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("BaseResource")
    }
}

/// New ResourceRef that works with any resource type.
/// This can eventually replace the enum-based ResourceRef for full extensibility.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ResourceRef {
    #[serde(rename = "type")]
    pub resource_type: ResourceType,
    pub id: String,
}

impl ResourceRef {
    /// Creates a new ResourceRef
    pub fn new(resource_type: ResourceType, id: impl Into<String>) -> Self {
        Self {
            resource_type,
            id: id.into(),
        }
    }

    /// Returns the resource type
    pub fn resource_type(&self) -> &ResourceType {
        &self.resource_type
    }

    /// Returns the resource id
    pub fn id(&self) -> &str {
        &self.id
    }
}

impl<T: ResourceDefinition> From<&T> for ResourceRef {
    fn from(resource: &T) -> Self {
        Self::new(resource.get_resource_type(), resource.id())
    }
}

impl From<&Resource> for ResourceRef {
    fn from(resource: &Resource) -> Self {
        Self::new(resource.resource_type(), resource.id())
    }
}

/// Trait that defines the interface for all resource output types in the Alien system.
/// This trait enables extensibility by allowing new resource output types to be registered
/// and managed alongside built-in resource outputs.
pub trait ResourceOutputsDefinition: Debug + Send + Sync + 'static {
    /// Returns the resource type for this instance
    fn get_resource_type(&self) -> ResourceType;

    /// Provides access to the underlying concrete type for downcasting
    fn as_any(&self) -> &dyn Any;

    /// Creates a boxed clone of this resource outputs
    fn box_clone(&self) -> Box<dyn ResourceOutputsDefinition>;

    /// For equality comparison between resource outputs
    fn outputs_eq(&self, other: &dyn ResourceOutputsDefinition) -> bool;

    /// Serialize this resource outputs to a JSON value (without the "type" tag - that's added by ResourceOutputs)
    fn to_json_value(&self) -> serde_json::Result<serde_json::Value>;
}

/// Clone implementation for boxed ResourceOutputsDefinition trait objects
impl Clone for Box<dyn ResourceOutputsDefinition> {
    fn clone(&self) -> Self {
        self.box_clone()
    }
}

/// New Resource outputs wrapper that can hold any ResourceOutputsDefinition.
/// This replaces the old ResourceOutputs enum to enable runtime extensibility.
#[derive(Debug, Clone)]
pub struct ResourceOutputs {
    inner: Box<dyn ResourceOutputsDefinition>,
}

impl Serialize for ResourceOutputs {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        let mut v = self.inner.to_json_value().map_err(serde::ser::Error::custom)?;
        v.as_object_mut()
            .ok_or_else(|| serde::ser::Error::custom("resource outputs must serialize as object"))?
            .insert("type".into(), serde_json::Value::String(self.inner.get_resource_type().0.into_owned()));
        v.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ResourceOutputs {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        let mut value = serde_json::Value::deserialize(deserializer)?;
        let type_tag = value.get("type").and_then(|v| v.as_str())
            .ok_or_else(|| serde::de::Error::missing_field("type"))?
            .to_string();

        // Remove the "type" tag before passing to concrete deserializer
        // (structs with deny_unknown_fields would reject it)
        if let Some(obj) = value.as_object_mut() {
            obj.remove("type");
        }

        let inner: Box<dyn ResourceOutputsDefinition> = match type_tag.as_str() {
            "vault" => Box::new(serde_json::from_value::<crate::resources::VaultOutputs>(value).map_err(serde::de::Error::custom)?),
            "function" => Box::new(serde_json::from_value::<crate::resources::FunctionOutputs>(value).map_err(serde::de::Error::custom)?),
            "container" => Box::new(serde_json::from_value::<crate::resources::ContainerOutputs>(value).map_err(serde::de::Error::custom)?),
            "container-cluster" => Box::new(serde_json::from_value::<crate::resources::ContainerClusterOutputs>(value).map_err(serde::de::Error::custom)?),
            "storage" => Box::new(serde_json::from_value::<crate::resources::StorageOutputs>(value).map_err(serde::de::Error::custom)?),
            "queue" => Box::new(serde_json::from_value::<crate::resources::QueueOutputs>(value).map_err(serde::de::Error::custom)?),
            "kv" => Box::new(serde_json::from_value::<crate::resources::KvOutputs>(value).map_err(serde::de::Error::custom)?),
            "network" => Box::new(serde_json::from_value::<crate::resources::NetworkOutputs>(value).map_err(serde::de::Error::custom)?),
            "build" => Box::new(serde_json::from_value::<crate::resources::BuildOutputs>(value).map_err(serde::de::Error::custom)?),
            "service-account" => Box::new(serde_json::from_value::<crate::resources::ServiceAccountOutputs>(value).map_err(serde::de::Error::custom)?),
            "artifact-registry" => Box::new(serde_json::from_value::<crate::resources::ArtifactRegistryOutputs>(value).map_err(serde::de::Error::custom)?),
            "service_activation" => Box::new(serde_json::from_value::<crate::resources::ServiceActivationOutputs>(value).map_err(serde::de::Error::custom)?),
            "remote-stack-management" => Box::new(serde_json::from_value::<crate::resources::RemoteStackManagementOutputs>(value).map_err(serde::de::Error::custom)?),
            "azure_resource_group" => Box::new(serde_json::from_value::<crate::resources::AzureResourceGroupOutputs>(value).map_err(serde::de::Error::custom)?),
            "azure_storage_account" => Box::new(serde_json::from_value::<crate::resources::AzureStorageAccountOutputs>(value).map_err(serde::de::Error::custom)?),
            "azure_container_apps_environment" => Box::new(serde_json::from_value::<crate::resources::AzureContainerAppsEnvironmentOutputs>(value).map_err(serde::de::Error::custom)?),
            "azure_service_bus_namespace" => Box::new(serde_json::from_value::<crate::resources::AzureServiceBusNamespaceOutputs>(value).map_err(serde::de::Error::custom)?),
            other => return Err(serde::de::Error::unknown_variant(other, &[
                "vault", "function", "container", "container-cluster", "storage", "queue", "kv",
                "network", "build", "service-account", "artifact-registry", "service_activation",
                "remote-stack-management", "azure_resource_group", "azure_storage_account",
                "azure_container_apps_environment", "azure_service_bus_namespace",
            ])),
        };

        Ok(ResourceOutputs { inner })
    }
}

impl ResourceOutputs {
    /// Creates a new ResourceOutputs from any type that implements ResourceOutputsDefinition
    pub fn new<T: ResourceOutputsDefinition>(outputs: T) -> Self {
        Self {
            inner: Box::new(outputs),
        }
    }

    /// Provides access to the underlying ResourceOutputsDefinition trait object
    pub fn as_resource_outputs(&self) -> &dyn ResourceOutputsDefinition {
        self.inner.as_ref()
    }

    /// Generic downcasting for any type
    pub fn downcast_ref<T: ResourceOutputsDefinition + 'static>(&self) -> Option<&T> {
        self.inner.as_any().downcast_ref::<T>()
    }
}

impl PartialEq for ResourceOutputs {
    fn eq(&self, other: &Self) -> bool {
        self.inner.outputs_eq(other.inner.as_ref())
    }
}

impl Eq for ResourceOutputs {}

/// OpenAPI schema implementation for ResourceOutputs.
///
/// The schema represents the flattened JSON structure of any resource outputs in the Alien system.
/// All resource outputs have a common base structure with a `type` field, plus type-specific
/// additional properties that vary depending on the concrete resource implementation.
///
/// # Schema Structure
/// - `type` (required): The resource type identifier (e.g., "function", "storage", "queue")
/// - Additional properties: Type-specific output fields that vary by resource type
///
/// # Example JSON
/// ```json
/// {
///   "type": "function",
///   "functionArn": "arn:aws:lambda:us-east-1:123456789012:function:my-function",
///   "functionUrl": "https://abc123.lambda-url.us-east-1.on.aws/"
/// }
/// ```
#[cfg(feature = "openapi")]
impl PartialSchema for ResourceOutputs {
    fn schema() -> RefOr<Schema> {
        RefOr::T(Schema::Object(
            ObjectBuilder::new()
                .schema_type(Type::Object)
                .property("type", Ref::from_schema_name("ResourceType"))
                .required("type")
                .additional_properties(Some(AdditionalProperties::FreeForm(true)))
                .description(Some("Resource outputs that can hold output data for any resource type in the Alien system. All resource outputs share a common 'type' field with additional type-specific output properties."))
                .build()
        ))
    }
}

#[cfg(feature = "openapi")]
impl ToSchema for ResourceOutputs {
    fn name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("BaseResourceOutputs")
    }
}

/// Represents the high-level status of a resource during its lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "kebab-case")]
pub enum ResourceStatus {
    Pending,      // Initial state before any action starts
    Provisioning, // Resource is being created or updated
    ProvisionFailed,
    Running, // Resource is active and configured as desired
    Updating,
    UpdateFailed,
    Deleting, // Resource is being removed
    DeleteFailed,
    Deleted,       // Resource has been successfully removed (terminal state)
    RefreshFailed, // Resource heartbeat/health check failed
}

impl ResourceStatus {
    pub fn is_terminal(&self) -> bool {
        match self {
            ResourceStatus::Deleted => true,
            ResourceStatus::ProvisionFailed => true,
            ResourceStatus::UpdateFailed => true,
            ResourceStatus::DeleteFailed => true,
            ResourceStatus::RefreshFailed => true,
            _ => false, // Pending, Provisioning, Updating, Deleting are not terminal
        }
    }
}

/// Describes the lifecycle of a resource within a stack, determining how it's managed and deployed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Hash, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "kebab-case")]
pub enum ResourceLifecycle {
    /// Frozen resources are set up once and not modified after creation. They receive
    /// heartbeat-only permissions for ongoing health checks but no management permissions.
    /// Example: S3 buckets for logs, VPCs, IAM roles.
    Frozen,

    /// Live resources are updated on every deploy and require management permissions
    /// for ongoing updates. All resources (Frozen and Live) are created during initial setup.
    /// Example: Lambda functions, Cloud Run services.
    Live,
}
