use crate::resource::{ResourceDefinition, ResourceOutputsDefinition, ResourceRef, ResourceType};
use alien_error::AlienError;
use bon::Builder;
use serde::{Deserialize, Serialize};
use std::any::Any;

/// Setup-owned identity used to issue short-lived credentials for resources
/// explicitly published through Remote Bindings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Builder)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[builder(start_fn = new)]
pub struct RemoteBindings {
    #[builder(start_fn)]
    pub id: String,

    /// Resource-neutral desired grants. Setup generators and direct controllers compile this
    /// from the Remote Bindings registry; users do not author it directly.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[builder(default)]
    pub grants: Vec<RemoteBindingGrant>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemoteBindingGrant {
    pub resource_id: String,
    pub permission_set: String,
    pub revision: u32,
}

impl RemoteBindings {
    pub const RESOURCE_TYPE: ResourceType = ResourceType::from_static("remote-bindings");
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemoteBindingsOutputs {
    /// Role ARN, service-account email, or managed-identity resource ID.
    pub resource_id: String,
    /// Provider-specific impersonation configuration consumed by the manager.
    pub access_configuration: String,
    /// AWS STS ExternalId required by this role's trust policy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_id: Option<String>,
}

impl ResourceDefinition for RemoteBindings {
    fn get_resource_type(&self) -> ResourceType {
        Self::RESOURCE_TYPE
    }
    fn id(&self) -> &str {
        &self.id
    }
    fn get_dependencies(&self) -> Vec<ResourceRef> {
        Vec::new()
    }

    fn validate_update(&self, new_config: &dyn ResourceDefinition) -> crate::error::Result<()> {
        let Some(new) = new_config.as_any().downcast_ref::<Self>() else {
            return Err(AlienError::new(
                crate::error::ErrorData::UnexpectedResourceType {
                    resource_id: self.id.clone(),
                    expected: Self::RESOURCE_TYPE,
                    actual: new_config.get_resource_type(),
                },
            ));
        };
        if self.id != new.id {
            return Err(AlienError::new(
                crate::error::ErrorData::InvalidResourceUpdate {
                    resource_id: self.id.clone(),
                    reason: "the 'id' field is immutable".to_string(),
                },
            ));
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
        other.as_any().downcast_ref::<Self>() == Some(self)
    }
    fn to_json_value(&self) -> serde_json::Result<serde_json::Value> {
        serde_json::to_value(self)
    }
}

impl ResourceOutputsDefinition for RemoteBindingsOutputs {
    fn get_resource_type(&self) -> ResourceType {
        RemoteBindings::RESOURCE_TYPE
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn box_clone(&self) -> Box<dyn ResourceOutputsDefinition> {
        Box::new(self.clone())
    }
    fn outputs_eq(&self, other: &dyn ResourceOutputsDefinition) -> bool {
        other.as_any().downcast_ref::<Self>() == Some(self)
    }
    fn to_json_value(&self) -> serde_json::Result<serde_json::Value> {
        serde_json::to_value(self)
    }
}
