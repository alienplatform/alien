//! Defines core permission types and structures used across Alien Infra.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
/// Grant permissions for a specific cloud platform
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PermissionGrant {
    /// AWS IAM actions (only for AWS)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actions: Option<Vec<String>>,
    /// GCP permissions (only for GCP)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permissions: Option<Vec<String>>,
    /// Azure actions (only for Azure)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_actions: Option<Vec<String>>,
}

/// AWS-specific binding specification
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AwsBindingSpec {
    /// Resource ARNs to bind to
    pub resources: Vec<String>,
    /// Optional condition for additional filtering (rare)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub condition: Option<IndexMap<String, IndexMap<String, String>>>,
}

/// GCP-specific binding specification
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GcpBindingSpec {
    /// Scope (project/resource level)
    pub scope: String,
    /// Optional condition for filtering resources
    #[serde(skip_serializing_if = "Option::is_none")]
    pub condition: Option<GcpCondition>,
}

/// Azure-specific binding specification
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AzureBindingSpec {
    /// Scope (subscription/resource group/resource level)
    pub scope: String,
}

/// Generic binding configuration for permissions
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BindingConfiguration<T> {
    /// Stack-level binding
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stack: Option<T>,
    /// Resource-level binding
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource: Option<T>,
}

impl<T> BindingConfiguration<T> {
    /// Check if the binding configuration is empty (no stack or resource bindings)
    pub fn is_empty(&self) -> bool {
        self.stack.is_none() && self.resource.is_none()
    }
}

/// GCP IAM condition
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GcpCondition {
    pub title: String,
    pub expression: String,
}

/// AWS-specific platform permission configuration
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AwsPlatformPermission {
    /// What permissions to grant
    pub grant: PermissionGrant,
    /// How to bind the permissions (stack vs resource scope)
    pub binding: BindingConfiguration<AwsBindingSpec>,
}

/// GCP-specific platform permission configuration
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GcpPlatformPermission {
    /// What permissions to grant
    pub grant: PermissionGrant,
    /// How to bind the permissions (stack vs resource scope)
    pub binding: BindingConfiguration<GcpBindingSpec>,
}

/// Azure-specific platform permission configuration
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AzurePlatformPermission {
    /// What permissions to grant
    pub grant: PermissionGrant,
    /// How to bind the permissions (stack vs resource scope)
    pub binding: BindingConfiguration<AzureBindingSpec>,
}

/// Platform-specific permission configurations
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlatformPermissions {
    /// AWS permission configurations
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aws: Option<Vec<AwsPlatformPermission>>,
    /// GCP permission configurations
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gcp: Option<Vec<GcpPlatformPermission>>,
    /// Azure permission configurations
    #[serde(skip_serializing_if = "Option::is_none")]
    pub azure: Option<Vec<AzurePlatformPermission>>,
}

/// A permission set that can be applied across different cloud platforms
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PermissionSet {
    /// Unique identifier for the permission set (e.g., "storage/data-read")
    pub id: String,
    /// Human-readable description of what this permission set allows
    pub description: String,
    /// Platform-specific permission configurations
    pub platforms: PlatformPermissions,
}

/// Reference to a permission set - either by name or inline definition
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(untagged)]
pub enum PermissionSetReference {
    /// Reference to a built-in permission set by name (e.g., "storage/data-read")
    Name(String),
    /// Inline permission set definition
    Inline(PermissionSet),
}

impl PermissionSetReference {
    /// Get the ID of the permission set, whether it's a reference or inline
    pub fn id(&self) -> &str {
        match self {
            PermissionSetReference::Name(name) => name,
            PermissionSetReference::Inline(permission_set) => &permission_set.id,
        }
    }

    /// Create a permission set reference from a name
    pub fn from_name(name: impl Into<String>) -> Self {
        PermissionSetReference::Name(name.into())
    }

    /// Create a permission set reference from an inline permission set
    pub fn from_inline(permission_set: PermissionSet) -> Self {
        PermissionSetReference::Inline(permission_set)
    }

    /// Resolve this reference to a concrete PermissionSet
    /// Takes a resolver function for built-in permission sets
    pub fn resolve(
        &self,
        resolver: impl Fn(&str) -> Option<PermissionSet>,
    ) -> Option<PermissionSet> {
        match self {
            PermissionSetReference::Name(name) => resolver(name),
            PermissionSetReference::Inline(permission_set) => Some(permission_set.clone()),
        }
    }
}

/// Permission profile that maps resources to permission sets
/// Key can be "*" for all resources or resource name for specific resource
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(transparent)]
pub struct PermissionProfile(pub IndexMap<String, Vec<PermissionSetReference>>);

impl PermissionProfile {
    /// Create a new permission profile
    pub fn new() -> Self {
        Self(IndexMap::new())
    }

    /// Add global permissions (applies to all resources)
    pub fn global<I>(mut self, permission_sets: I) -> Self
    where
        I: IntoIterator,
        I::Item: Into<PermissionSetReference>,
    {
        let permission_list: Vec<PermissionSetReference> =
            permission_sets.into_iter().map(|s| s.into()).collect();
        self.0.insert("*".to_string(), permission_list);
        self
    }

    /// Add resource-scoped permissions
    pub fn resource<I>(mut self, resource_name: impl Into<String>, permission_sets: I) -> Self
    where
        I: IntoIterator,
        I::Item: Into<PermissionSetReference>,
    {
        let permission_list: Vec<PermissionSetReference> =
            permission_sets.into_iter().map(|s| s.into()).collect();
        self.0.insert(resource_name.into(), permission_list);
        self
    }
}

impl Default for PermissionProfile {
    fn default() -> Self {
        Self::new()
    }
}

impl From<String> for PermissionSetReference {
    fn from(name: String) -> Self {
        PermissionSetReference::Name(name)
    }
}

impl From<&str> for PermissionSetReference {
    fn from(name: &str) -> Self {
        PermissionSetReference::Name(name.to_string())
    }
}

impl From<PermissionSet> for PermissionSetReference {
    fn from(permission_set: PermissionSet) -> Self {
        PermissionSetReference::Inline(permission_set)
    }
}

/// Management permissions configuration for stack management access
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub enum ManagementPermissions {
    /// Auto-derived permissions only (default)
    /// Uses resource lifecycles to determine management permissions:
    /// - Frozen resources: `<type>/management`
    /// - Live resources: `<type>/provision`
    Auto,

    /// Add permissions to auto-derived baseline
    Extend(PermissionProfile),

    /// Replace auto-derived permissions entirely
    Override(PermissionProfile),
}

impl Default for ManagementPermissions {
    fn default() -> Self {
        ManagementPermissions::Auto
    }
}

/// Combined permissions configuration that contains both profiles and management
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PermissionsConfig {
    /// Permission profiles that define access control for compute services
    /// Key is the profile name, value is the permission configuration
    pub profiles: IndexMap<String, PermissionProfile>,
    /// Management permissions configuration for stack management access
    #[serde(default)]
    pub management: ManagementPermissions,
}

impl PermissionsConfig {
    /// Create a new permissions config with auto management
    pub fn new() -> Self {
        Self {
            profiles: IndexMap::new(),
            management: ManagementPermissions::Auto,
        }
    }

    /// Add a permission profile
    pub fn with_profile(mut self, name: impl Into<String>, profile: PermissionProfile) -> Self {
        self.profiles.insert(name.into(), profile);
        self
    }

    /// Set management permissions
    pub fn with_management(mut self, management: ManagementPermissions) -> Self {
        self.management = management;
        self
    }
}

impl Default for PermissionsConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl ManagementPermissions {
    /// Create auto-derived management permissions
    pub fn auto() -> Self {
        ManagementPermissions::Auto
    }

    /// Create management permissions that extend auto-derived baseline
    pub fn extend(profile: PermissionProfile) -> Self {
        ManagementPermissions::Extend(profile)
    }

    /// Create management permissions that override auto-derived permissions
    pub fn override_(profile: PermissionProfile) -> Self {
        ManagementPermissions::Override(profile)
    }

    /// Get the permission profile if present (for Extend/Override variants)
    pub fn profile(&self) -> Option<&PermissionProfile> {
        match self {
            ManagementPermissions::Auto => None,
            ManagementPermissions::Extend(profile) => Some(profile),
            ManagementPermissions::Override(profile) => Some(profile),
        }
    }

    /// Check if this is the auto variant
    pub fn is_auto(&self) -> bool {
        matches!(self, ManagementPermissions::Auto)
    }

    /// Check if this extends auto-derived permissions
    pub fn is_extend(&self) -> bool {
        matches!(self, ManagementPermissions::Extend(_))
    }

    /// Check if this overrides auto-derived permissions
    pub fn is_override(&self) -> bool {
        matches!(self, ManagementPermissions::Override(_))
    }
}
