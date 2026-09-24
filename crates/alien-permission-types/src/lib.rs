//! Shared permission data types used by Alien's runtime and build-time registry validation.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum AwsPermissionEffect {
    #[default]
    Allow,
    Deny,
}

impl AwsPermissionEffect {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Allow => "Allow",
            Self::Deny => "Deny",
        }
    }

    pub fn is_allow(&self) -> bool {
        matches!(self, Self::Allow)
    }
}

/// Grant permissions for a specific cloud platform
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PermissionGrant {
    /// AWS IAM actions (only for AWS)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actions: Option<Vec<String>>,
    /// GCP permissions that require an exact residual custom role.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permissions: Option<Vec<String>>,
    /// Provider predefined roles to bind directly.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub predefined_roles: Option<Vec<String>>,
    /// GCP residual custom permissions to pair with predefined roles.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub residual_permissions: Option<Vec<String>>,
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
    /// ARN patterns rendered as IAM `NotResource`, in place of `resources`. Its one use is a
    /// tag-on-create grant whose implied check AWS authorizes against no resource; the build
    /// refuses it anywhere else.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub not_resources: Vec<String>,
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
    /// Stable admin-facing label for this permission entry.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Short admin-facing description of why this entry exists.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// IAM effect. Defaults to Allow.
    #[serde(default, skip_serializing_if = "AwsPermissionEffect::is_allow")]
    pub effect: AwsPermissionEffect,
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
    /// Stable admin-facing label for this permission entry.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Short admin-facing description of why this entry exists.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
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
    /// Stable admin-facing label for this permission entry.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Short admin-facing description of why this entry exists.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
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

impl PermissionSet {
    /// An `Allow` on `NotResource` grants everything outside the listed patterns, so it is held
    /// to the one grant that needs it: the tag a create applies, which AWS authorizes against no
    /// resource. Excluding every ARN of the tagging service leaves only that resourceless check.
    pub fn validate_not_resources(&self) -> Result<(), String> {
        let id = &self.id;
        for entry in self.platforms.aws.iter().flatten() {
            let specs = [
                entry.binding.stack.as_ref(),
                entry.binding.resource.as_ref(),
            ];
            for spec in specs.into_iter().flatten() {
                if spec.not_resources.is_empty() {
                    continue;
                }
                if !spec.resources.is_empty() {
                    return Err(format!(
                        "{id}: a binding names both resources and notResources"
                    ));
                }
                if !entry.effect.is_allow() {
                    return Err(format!("{id}: notResources is only for an Allow"));
                }
                let actions = entry.grant.actions.as_deref().unwrap_or_default();
                if actions.is_empty() || !actions.iter().all(|a| a.ends_with(":TagResource")) {
                    return Err(format!(
                        "{id}: notResources may only grant a TagResource action, got {actions:?}"
                    ));
                }
                let bounded_by_request_tags = spec
                    .condition
                    .as_ref()
                    .and_then(|condition| condition.get("StringEquals"))
                    .is_some_and(|keys| keys.keys().any(|k| k.starts_with("aws:RequestTag/")));
                if !bounded_by_request_tags {
                    return Err(format!(
                        "{id}: a notResources grant must be bounded by StringEquals on \
                         aws:RequestTag"
                    ));
                }
                let services: Vec<&str> =
                    actions.iter().filter_map(|a| a.split(':').next()).collect();
                for pattern in &spec.not_resources {
                    let service = pattern
                        .strip_prefix("arn:aws:")
                        .and_then(|rest| rest.split(':').next());
                    if !service.is_some_and(|service| services.contains(&service)) {
                        return Err(format!(
                            "{id}: notResources pattern '{pattern}' must name the action's own \
                             service"
                        ));
                    }
                }
            }
        }
        Ok(())
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn tag_on_create(
        actions: &[&str],
        not_resources: &[&str],
        resources: &[&str],
    ) -> PermissionSet {
        let condition = IndexMap::from([(
            "StringEquals".to_string(),
            IndexMap::from([("aws:RequestTag/deployment".to_string(), "acme".to_string())]),
        )]);
        PermissionSet {
            id: "test/set".to_string(),
            description: "test".to_string(),
            platforms: PlatformPermissions {
                aws: Some(vec![AwsPlatformPermission {
                    label: None,
                    description: None,
                    effect: AwsPermissionEffect::Allow,
                    grant: PermissionGrant {
                        actions: Some(actions.iter().map(|a| a.to_string()).collect()),
                        ..Default::default()
                    },
                    binding: BindingConfiguration {
                        stack: Some(AwsBindingSpec {
                            resources: resources.iter().map(|r| r.to_string()).collect(),
                            not_resources: not_resources.iter().map(|r| r.to_string()).collect(),
                            condition: Some(condition),
                        }),
                        resource: None,
                    },
                }]),
                gcp: None,
                azure: None,
            },
        }
    }

    #[test]
    fn a_request_tag_bound_tag_on_create_may_exclude_its_services_arns() {
        let set = tag_on_create(&["lambda:TagResource"], &["arn:aws:lambda:*:*:*"], &[]);
        assert_eq!(set.validate_not_resources(), Ok(()));
    }

    #[test]
    fn not_resources_is_refused_beyond_tag_on_create() {
        for (set, why) in [
            (
                tag_on_create(&["lambda:InvokeFunction"], &["arn:aws:lambda:*:*:*"], &[]),
                "an action other than TagResource",
            ),
            (
                tag_on_create(&["lambda:TagResource"], &["arn:aws:s3:::*"], &[]),
                "another service's ARNs, which leaves every Lambda ARN in",
            ),
            (
                tag_on_create(&["lambda:TagResource"], &["arn:*"], &[]),
                "a pattern IAM refuses",
            ),
            (
                tag_on_create(&["lambda:TagResource"], &["arn:aws:lambda:*:*:*"], &["*"]),
                "both Resource and NotResource",
            ),
        ] {
            assert!(set.validate_not_resources().is_err(), "must refuse {why}");
        }

        let mut unbounded = tag_on_create(&["lambda:TagResource"], &["arn:aws:lambda:*:*:*"], &[]);
        unbounded.platforms.aws.as_mut().unwrap()[0]
            .binding
            .stack
            .as_mut()
            .unwrap()
            .condition = None;
        assert!(
            unbounded.validate_not_resources().is_err(),
            "must refuse no request-tag bound"
        );
    }
}
