use crate::resource::{ResourceDefinition, ResourceOutputsDefinition, ResourceRef, ResourceType};
use alien_error::AlienError;
use bon::Builder;
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::fmt::Debug;

/// A protected install's verifier, `<prefix>-verifier`: the only principal that changes what
/// runs. Worker controllers hand it the cloud writes they build, and machine rolls go through
/// it; it checks them against the signed release before anything is written.
///
/// Setup creates it, and only setup changes it, so it is Frozen only. A preflight adds it to a
/// stack with signing settings that has a function or a machine cluster; a stack can't declare
/// it, and its id is reserved.
///
/// Maps to:
/// - AWS: a Lambda function and its role
/// - GCP: a Cloud Run service and its service account
/// - Azure: a Container App in a managed environment of its own
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Builder)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[builder(start_fn = new)]
pub struct SigningVerifier {
    /// Identifier of the verifier, always [`SigningVerifier::ID`].
    #[builder(start_fn)]
    pub id: String,
}

impl SigningVerifier {
    /// The resource type identifier for the verifier.
    pub const RESOURCE_TYPE: ResourceType = ResourceType::from_static("verifier");

    /// The verifier's resource id, reserved: no stack resource may take it.
    pub const ID: &'static str = "verifier";

    /// Returns the verifier's identifier.
    pub fn id(&self) -> &str {
        &self.id
    }
}

/// Whether `stack` is protected and runs machines: its verifier launches every machine, and only
/// into the subnets setup fixes for it.
pub fn verifier_launches_machines(stack: &crate::Stack) -> bool {
    let has = |resource_type: ResourceType| {
        stack
            .resources()
            .any(|(_, entry)| entry.config.resource_type() == resource_type)
    };
    has(SigningVerifier::RESOURCE_TYPE) && has(crate::ComputeCluster::RESOURCE_TYPE)
}

/// Resource outputs for the verifier.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct SigningVerifierOutputs {
    /// The verifier's name in the cloud: `<prefix>-verifier`.
    pub name: String,
}

impl ResourceDefinition for SigningVerifier {
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
        match new_config.as_any().downcast_ref::<SigningVerifier>() {
            Some(new) if new.id == self.id => Ok(()),
            Some(_) => Err(AlienError::new(
                crate::error::ErrorData::InvalidResourceUpdate {
                    resource_id: self.id.clone(),
                    reason: "the 'id' field is immutable".to_string(),
                },
            )),
            None => Err(AlienError::new(
                crate::error::ErrorData::UnexpectedResourceType {
                    resource_id: self.id.clone(),
                    expected: Self::RESOURCE_TYPE,
                    actual: new_config.get_resource_type(),
                },
            )),
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
        other.as_any().downcast_ref::<SigningVerifier>() == Some(self)
    }

    fn to_json_value(&self) -> serde_json::Result<serde_json::Value> {
        serde_json::to_value(self)
    }
}

impl ResourceOutputsDefinition for SigningVerifierOutputs {
    fn get_resource_type(&self) -> ResourceType {
        SigningVerifier::RESOURCE_TYPE.clone()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn box_clone(&self) -> Box<dyn ResourceOutputsDefinition> {
        Box::new(self.clone())
    }

    fn outputs_eq(&self, other: &dyn ResourceOutputsDefinition) -> bool {
        other.as_any().downcast_ref::<SigningVerifierOutputs>() == Some(self)
    }

    fn to_json_value(&self) -> serde_json::Result<serde_json::Value> {
        serde_json::to_value(self)
    }
}
