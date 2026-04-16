use crate::resource::{ResourceDefinition, ResourceOutputsDefinition, ResourceRef, ResourceType};
use alien_error::AlienError;
use bon::Builder;
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::fmt::Debug;

/// Represents cross-account management access configuration for a stack deployed
/// on AWS, GCP, or Azure platforms. This resource sets up the necessary IAM/RBAC
/// configuration to allow another cloud account to manage the stack.
///
/// Maps to:
/// - AWS: Cross-account IAM role with management permissions
/// - GCP: Service account with management permissions and impersonation rights
/// - Azure: User-assigned managed identity with federated credential and custom RBAC
///
/// This resource is automatically created for AWS, GCP, and Azure platforms
/// when the stack needs to be managed by another account. The management account
/// and identity information comes from the platform configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Builder)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[builder(start_fn = new)]
pub struct RemoteStackManagement {
    /// Identifier for the remote stack management. Must contain only alphanumeric characters, hyphens, and underscores ([A-Za-z0-9-_]).
    /// Maximum 64 characters.
    #[builder(start_fn)]
    pub id: String,
}

impl RemoteStackManagement {
    /// The resource type identifier for RemoteStackManagement
    pub const RESOURCE_TYPE: ResourceType = ResourceType::from_static("remote-stack-management");

    /// Returns the remote stack management's unique identifier.
    pub fn id(&self) -> &str {
        &self.id
    }
}

/// Resource outputs for RemoteStackManagement.
/// Different platforms will provide different outputs based on their implementation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemoteStackManagementOutputs {
    /// Platform-specific management resource identifier
    /// For AWS: The ARN of the created cross-account role
    /// For GCP: The email of the created service account
    /// For Azure: The resource ID of the target user-assigned managed identity
    pub management_resource_id: String,

    /// Platform-specific access configuration
    /// For AWS: The role ARN to assume
    /// For GCP: The service account email to impersonate
    /// For Azure: JSON containing the target managed identity client ID and tenant ID
    pub access_configuration: String,
}

// Implementation of ResourceDefinition trait for RemoteStackManagement
impl ResourceDefinition for RemoteStackManagement {
    fn get_resource_type(&self) -> ResourceType {
        Self::RESOURCE_TYPE
    }

    fn id(&self) -> &str {
        &self.id
    }

    fn get_dependencies(&self) -> Vec<ResourceRef> {
        // RemoteStackManagement typically doesn't depend on other resources,
        // but may depend on infrastructure requirements like resource groups
        Vec::new()
    }

    fn validate_update(&self, new_config: &dyn ResourceDefinition) -> crate::error::Result<()> {
        // Try to downcast to RemoteStackManagement for type-specific validation
        if let Some(new_remote_mgmt) = new_config.as_any().downcast_ref::<RemoteStackManagement>() {
            // Validate that the ID matches
            if self.id != new_remote_mgmt.id {
                return Err(AlienError::new(
                    crate::error::ErrorData::InvalidResourceUpdate {
                        resource_id: self.id.clone(),
                        reason: "the 'id' field is immutable".to_string(),
                    },
                ));
            }

            // RemoteStackManagement configuration can be updated
            Ok(())
        } else {
            Err(AlienError::new(
                crate::error::ErrorData::UnexpectedResourceType {
                    resource_id: self.id.clone(),
                    expected: Self::RESOURCE_TYPE,
                    actual: new_config.get_resource_type(),
                },
            ))
        }
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
        other
            .as_any()
            .downcast_ref::<RemoteStackManagement>()
            .map(|other_remote_mgmt| self == other_remote_mgmt)
            .unwrap_or(false)
    }

    fn to_json_value(&self) -> serde_json::Result<serde_json::Value> {
        serde_json::to_value(self)
    }
}

impl ResourceOutputsDefinition for RemoteStackManagementOutputs {
    fn get_resource_type(&self) -> ResourceType {
        RemoteStackManagement::RESOURCE_TYPE.clone()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn box_clone(&self) -> Box<dyn ResourceOutputsDefinition> {
        Box::new(self.clone())
    }

    fn outputs_eq(&self, other: &dyn ResourceOutputsDefinition) -> bool {
        other
            .as_any()
            .downcast_ref::<RemoteStackManagementOutputs>()
            == Some(self)
    }

    fn to_json_value(&self) -> serde_json::Result<serde_json::Value> {
        serde_json::to_value(self)
    }
}
