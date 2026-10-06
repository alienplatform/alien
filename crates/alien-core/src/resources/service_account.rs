use crate::error::{ErrorData, Result};
use crate::permissions::{PermissionProfile, PermissionSet};
use crate::resource::{ResourceDefinition, ResourceOutputsDefinition, ResourceRef, ResourceType};
use alien_error::AlienError;
use bon::Builder;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::borrow::Cow;
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

    /// Resolved grants for concrete resource IDs, captured for setup comparison.
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    #[builder(default)]
    pub resource_permission_sets: IndexMap<String, Vec<PermissionSet>>,
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
    pub fn from_permission_profile(
        id: String,
        profile: &PermissionProfile,
        permission_set_resolver: impl Fn(&str) -> Option<PermissionSet>,
    ) -> Result<Self> {
        let mut stack_permission_sets = Vec::new();
        let mut resource_permission_sets = IndexMap::new();
        for (resource_id, references) in &profile.0 {
            let sets = references
                .iter()
                .map(|reference| {
                    reference.resolve(&permission_set_resolver).ok_or_else(|| {
                        AlienError::new(ErrorData::GenericError {
                            message: format!(
                                "Permission set '{}' not found for service account '{}'",
                                reference.id(),
                                id
                            ),
                        })
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            if resource_id == "*" {
                stack_permission_sets = sets;
            } else {
                resource_permission_sets.insert(resource_id.clone(), sets);
            }
        }
        Ok(Self {
            id,
            stack_permission_sets,
            resource_permission_sets,
        })
    }

    /// Uses captured grants; legacy templates may supply their existing explicit profile.
    /// This compatibility path does not create a profile or add default grants.
    pub fn concrete_permission_sets(
        &self,
        legacy_profile: Option<&PermissionProfile>,
        resolver: impl Fn(&str) -> Option<PermissionSet>,
    ) -> Result<Cow<'_, IndexMap<String, Vec<PermissionSet>>>> {
        if !self.resource_permission_sets.is_empty() {
            return Ok(Cow::Borrowed(&self.resource_permission_sets));
        }
        match legacy_profile {
            Some(profile) => Ok(Cow::Owned(
                Self::from_permission_profile(self.id.clone(), profile, resolver)?
                    .resource_permission_sets,
            )),
            None => Ok(Cow::Borrowed(&self.resource_permission_sets)),
        }
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
impl ResourceDefinition for ServiceAccount {
    fn get_resource_type(&self) -> ResourceType {
        Self::RESOURCE_TYPE
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

    fn to_json_value(&self) -> serde_json::Result<serde_json::Value> {
        serde_json::to_value(self)
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

impl ResourceOutputsDefinition for ServiceAccountOutputs {
    fn get_resource_type(&self) -> ResourceType {
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

    fn to_json_value(&self) -> serde_json::Result<serde_json::Value> {
        serde_json::to_value(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn signing(scope: &str) -> PermissionSet {
        serde_json::from_value(json!({
            "id": "storage/sign", "description": "Sign object URLs",
            "platforms": {"gcp": [{"label": "sign", "description": "Sign URLs",
                "grant": {"permissions": ["iam.serviceAccounts.signBlob"]},
                "binding": {"resource": {"scope": scope}, "stack": {"scope": scope}}
            }]}
        }))
        .unwrap()
    }

    #[test]
    fn concrete_definitions_are_captured_separately_and_override_legacy_lookup() {
        let old = signing("projects/${projectName}");
        let new = signing("projects/${projectName}/serviceAccounts/${serviceAccountName}@${projectName}.iam.gserviceaccount.com");
        let profile = PermissionProfile::new()
            .resource("*", ["storage/sign"])
            .resource("objects", ["storage/sign"]);
        let before =
            ServiceAccount::from_permission_profile("reader-sa".to_string(), &profile, |_| {
                Some(old.clone())
            })
            .unwrap();
        let after =
            ServiceAccount::from_permission_profile("reader-sa".to_string(), &profile, |_| {
                Some(new.clone())
            })
            .unwrap();
        assert_eq!(before.stack_permission_sets, vec![old.clone()]);
        assert_eq!(before.resource_permission_sets["objects"], vec![old]);
        assert_eq!(after.resource_permission_sets["objects"], vec![new.clone()]);
        assert!(!before.resource_eq(&after));
        assert_eq!(
            after
                .concrete_permission_sets(Some(&profile), |_| panic!(
                    "captured definitions must be used"
                ))
                .unwrap()["objects"],
            vec![new]
        );
        assert!(
            ServiceAccount::from_permission_profile("reader-sa".to_string(), &profile, |_| None)
                .is_err()
        );
    }

    #[test]
    fn legacy_account_deserializes_without_implicit_grants_and_rejects_malformed_map() {
        let account: ServiceAccount =
            serde_json::from_value(json!({"id": "reader-sa", "stackPermissionSets": []})).unwrap();
        assert!(account.resource_permission_sets.is_empty());
        assert!(account
            .concrete_permission_sets(None, |_| None)
            .unwrap()
            .is_empty());
        assert!(serde_json::to_value(&account)
            .unwrap()
            .get("resourcePermissionSets")
            .is_none());
        let set = signing("projects/${projectName}");
        let profile = PermissionProfile::new().resource("objects", ["storage/sign"]);
        assert_eq!(
            account
                .concrete_permission_sets(Some(&profile), |_| Some(set.clone()))
                .unwrap()["objects"],
            vec![set]
        );
        assert!(account
            .concrete_permission_sets(Some(&profile), |_| None)
            .is_err());
        assert!(serde_json::from_value::<ServiceAccount>(json!({"id": "reader-sa", "stackPermissionSets": [], "resourcePermissionSets": {"objects": ["storage/sign"]}})).is_err());
    }
}
