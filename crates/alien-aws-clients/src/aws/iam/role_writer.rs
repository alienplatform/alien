use alien_client_core::{ErrorData, Result};
use alien_core::standard_resource_tags;
use alien_error::AlienError;

use super::{CreateRoleRequest, CreateRoleResponse, IamApi};

/// Prepares management-written roles for the deployment's permissions boundary.
/// Call `prepare_existing` before changing policies, trust, or removing policies
/// from an existing role. Setup-only roles should use the IAM client directly.
pub struct IamRoleWriter<'a> {
    iam: &'a dyn IamApi,
    permissions_boundary: Option<&'a str>,
}

impl<'a> IamRoleWriter<'a> {
    pub fn new(iam: &'a dyn IamApi, permissions_boundary: Option<&'a str>) -> Self {
        Self {
            iam,
            permissions_boundary,
        }
    }

    /// Creates a role with the deployment's boundary, when configured.
    pub async fn create(&self, mut request: CreateRoleRequest) -> Result<CreateRoleResponse> {
        if let Some(boundary) = self.permissions_boundary {
            request.permissions_boundary = Some(boundary.to_owned());
        }
        self.iam.create_role(request).await
    }

    /// Verifies runtime ownership and adopts an unbounded role before a write.
    /// Setup-owned roles and roles tagged for another deployment or resource are refused.
    /// With no deployment boundary, this performs no additional IAM calls.
    pub async fn prepare_existing(
        &self,
        role_name: &str,
        deployment_prefix: &str,
        resource_id: &str,
    ) -> Result<()> {
        let Some(boundary) = self.permissions_boundary else {
            return Ok(());
        };
        let role = self.iam.get_role(role_name).await?.get_role_result.role;
        let expected_tags = standard_resource_tags(deployment_prefix, resource_id);
        let owned = role.tags.as_ref().is_some_and(|tags| {
            expected_tags.iter().all(|(key, value)| {
                tags.member
                    .iter()
                    .any(|tag| &tag.key == key && &tag.value == value)
            })
        });
        if !owned {
            return Err(AlienError::new(ErrorData::ExistingCloudResourceNotOwned {
                resource_id: resource_id.to_owned(),
                object: format!("IAM role '{role_name}'"),
            }));
        }
        if let Some(attached) = role.permissions_boundary {
            if attached.permissions_boundary_arn.as_deref() == Some(boundary) {
                return Ok(());
            }
            return Err(AlienError::new(ErrorData::InvalidInput {
                message: format!("Role '{role_name}' has a different permissions boundary; reconcile it in setup before management writes this role"),
                field_name: Some("permissionsBoundary".to_owned()),
            }));
        }

        let policies = self
            .iam
            .list_attached_role_policies(role_name)
            .await?
            .list_attached_role_policies_result;
        // The client currently returns one page. Never adopt on a partial audit.
        if policies.is_truncated == Some(true) {
            return Err(AlienError::new(ErrorData::InvalidInput {
                message: format!(
                    "Cannot adopt role '{role_name}': its attached policy list is truncated"
                ),
                field_name: Some("attachedPolicies".to_owned()),
            }));
        }
        if let Some(policies) = policies.attached_policies {
            for policy in policies.member {
                if policy.policy_arn.split(':').nth(4) == Some("aws") {
                    return Err(AlienError::new(ErrorData::InvalidInput {
                        message: format!("Cannot adopt role '{role_name}' with AWS-managed policy '{}': compatibility with the deployment permissions boundary has not been established; reconcile its permissions in setup", policy.policy_arn),
                        field_name: Some("attachedPolicies".to_owned()),
                    }));
                }
            }
        }
        self.iam
            .put_role_permissions_boundary(role_name, boundary)
            .await
    }
}
