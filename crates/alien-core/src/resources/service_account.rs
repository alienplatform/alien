use crate::error::{ErrorData, Result};
use crate::permissions::{PermissionProfile, PermissionSet, PermissionSetReference};
use crate::resource::{ResourceDefinition, ResourceOutputsDefinition, ResourceRef, ResourceType};
use alien_error::AlienError;
use bon::Builder;
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::fmt::Debug;

/// Represents a non-human identity that can be assumed by compute services
/// such as Lambda, Cloud Run, ECS, Container Apps, etc.
///
/// Maps to:
/// - AWS: IAM Role
/// - GCP: Service Account
/// - Azure: User-assigned Managed Identity
///
/// The ServiceAccount is automatically created from permission profiles in the stack
/// and contains the resolved permission sets for both stack-level and resource-scoped access.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Builder)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[builder(start_fn = new)]
pub struct ServiceAccount {
    /// Identifier for the service account. Must contain only alphanumeric characters, hyphens, and underscores ([A-Za-z0-9-_]).
    /// Maximum 64 characters.
    #[builder(start_fn)]
    pub id: String,

    /// Stack-level permission sets that apply to all resources in the stack.
    /// These are derived from the "*" scope in the permission profile.
    /// Resource-scoped permissions are handled by individual resource controllers.
    #[builder(field)]
    pub stack_permission_sets: Vec<PermissionSet>,
}

impl ServiceAccount {
    /// The resource type identifier for ServiceAccount
    pub const RESOURCE_TYPE: ResourceType = ResourceType::from_static("service-account");

    /// Returns the service account's unique identifier.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Creates a ServiceAccount from a permission profile by resolving permission set references.
    /// This is used by the stack processor to convert profiles into concrete ServiceAccount resources.
    /// Only stack-level permissions ("*" scope) are processed - resource-scoped permissions are
    /// handled by individual resource controllers when they create their resources.
    pub fn from_permission_profile(
        id: String,
        profile: &PermissionProfile,
        permission_set_resolver: impl Fn(&str) -> Option<PermissionSet>,
    ) -> Result<Self> {
        let mut stack_permission_sets = Vec::new();

        // Only process stack-level permissions ("*" scope)
        if let Some(permission_set_refs) = profile.0.get("*") {
            for permission_set_ref in permission_set_refs {
                let permission_set = match permission_set_ref {
                    PermissionSetReference::Name(name) => {
                        // Look up built-in permission set by name
                        permission_set_resolver(&name).ok_or_else(|| {
                            AlienError::new(ErrorData::GenericError {
                                message: format!(
                                    "Permission set '{}' not found for service account '{}'",
                                    name, id
                                ),
                            })
                        })?
                    }
                    PermissionSetReference::Inline(inline_permission_set) => {
                        // Use the inline permission set directly
                        inline_permission_set.clone()
                    }
                };
                stack_permission_sets.push(permission_set);
            }
        }

        Ok(ServiceAccount {
            id,
            stack_permission_sets,
        })
    }
}

impl ServiceAccountBuilder {
    /// Adds a stack-level permission set to the service account.
    /// Stack-level permissions apply to all resources in the stack.
    pub fn stack_permission_set(mut self, permission_set: PermissionSet) -> Self {
        self.stack_permission_sets.push(permission_set);
        self
    }
}

// Implementation of ResourceDefinition trait for ServiceAccount
#[typetag::serde(name = "service-account")]
impl ResourceDefinition for ServiceAccount {
    fn resource_type() -> ResourceType {
        Self::RESOURCE_TYPE.clone()
    }

    fn get_resource_type(&self) -> ResourceType {
        Self::resource_type()
    }

    fn id(&self) -> &str {
        &self.id
    }

    fn get_dependencies(&self) -> Vec<ResourceRef> {
        // ServiceAccount doesn't depend on other resources directly
        // Dependencies will be managed through the stack processor
        Vec::new()
    }

    fn validate_update(&self, new_config: &dyn ResourceDefinition) -> Result<()> {
        let new_service_account = new_config
            .as_any()
            .downcast_ref::<ServiceAccount>()
            .ok_or_else(|| {
                AlienError::new(ErrorData::UnexpectedResourceType {
                    resource_id: self.id.clone(),
                    expected: Self::RESOURCE_TYPE,
                    actual: new_config.get_resource_type(),
                })
            })?;

        if self.id != new_service_account.id {
            return Err(AlienError::new(ErrorData::InvalidResourceUpdate {
                resource_id: self.id.clone(),
                reason: "the 'id' field is immutable".to_string(),
            }));
        }

        Ok(())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn box_clone(&self) -> Box<dyn ResourceDefinition> {
        Box::new(self.clone())
    }

    fn resource_eq(&self, other: &dyn ResourceDefinition) -> bool {
        other.as_any().downcast_ref::<ServiceAccount>() == Some(self)
    }
}

/// Outputs generated by a successfully provisioned ServiceAccount.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ServiceAccountOutputs {
    /// The platform-specific identifier of the service account
    /// - AWS: Role ARN
    /// - GCP: Service Account email
    /// - Azure: Managed Identity client ID
    pub identity: String,

    /// The platform-specific resource name/ID
    /// - AWS: Role name
    /// - GCP: Service Account unique ID
    /// - Azure: Managed Identity resource ID
    pub resource_id: String,
}

#[typetag::serde(name = "service-account")]
impl ResourceOutputsDefinition for ServiceAccountOutputs {
    fn resource_type() -> ResourceType {
        ServiceAccount::RESOURCE_TYPE.clone()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn box_clone(&self) -> Box<dyn ResourceOutputsDefinition> {
        Box::new(self.clone())
    }

    fn outputs_eq(&self, other: &dyn ResourceOutputsDefinition) -> bool {
        other.as_any().downcast_ref::<ServiceAccountOutputs>() == Some(self)
    }
}
