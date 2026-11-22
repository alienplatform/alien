use alien_client_core::{ErrorData, Result};
use alien_core::{setup_resource_tags, standard_resource_tags};
use alien_error::AlienError;

use super::{CreateRoleRequest, CreateRoleResponse, CreateRoleTag, IamApi, Role};

/// Prepares management-written roles for the deployment's permissions boundary.
/// Call `prepare_existing` before changing policies, trust, or removing policies
/// from an existing role.
///
/// A role only setup writes, and that must stay without the deployment boundary, uses
/// [`IamRoleWriter::setup_owned`]: it is created unbounded and tagged `managed-by=setup`, as the
/// CloudFormation and Terraform setups create it, and management can neither adopt nor change it.
pub struct IamRoleWriter<'a> {
    iam: &'a dyn IamApi,
    permissions_boundary: Option<&'a str>,
    /// Set for a setup-owned role: its resource type, for the setup tags.
    setup_owned: Option<&'a str>,
}

impl<'a> IamRoleWriter<'a> {
    pub fn new(iam: &'a dyn IamApi, permissions_boundary: Option<&'a str>) -> Self {
        Self {
            iam,
            permissions_boundary,
            setup_owned: None,
        }
    }

    /// A writer for a role only setup credentials write and that carries no permissions
    /// boundary. `deployment_boundary` is the boundary an earlier release attached to the role,
    /// which `prepare_existing` and `prepare_for_removal` remove.
    pub fn setup_owned(
        iam: &'a dyn IamApi,
        deployment_boundary: Option<&'a str>,
        resource_type: &'a str,
    ) -> Self {
        Self {
            iam,
            permissions_boundary: deployment_boundary,
            setup_owned: Some(resource_type),
        }
    }

    /// The tags a new role of `resource_id` is created with: `managed-by=runtime` for a role
    /// management writes, the template setups' tags for a setup-owned role.
    pub fn creation_tags(&self, deployment_prefix: &str, resource_id: &str) -> Vec<CreateRoleTag> {
        let tags: Vec<(String, String)> = match self.setup_owned {
            Some(resource_type) => {
                setup_resource_tags(deployment_prefix, resource_id, resource_type)
                    .into_iter()
                    .collect()
            }
            None => standard_resource_tags(deployment_prefix, resource_id)
                .into_iter()
                .collect(),
        };
        tags.into_iter()
            .map(|(key, value)| CreateRoleTag { key, value })
            .collect()
    }

    /// Creates a role with the deployment's boundary, when configured. A setup-owned role is
    /// created without one.
    pub async fn create(&self, mut request: CreateRoleRequest) -> Result<CreateRoleResponse> {
        if self.setup_owned.is_some() {
            request.permissions_boundary = None;
        } else if let Some(boundary) = self.permissions_boundary {
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
        self.prepare(role_name, deployment_prefix, resource_id, true)
            .await
    }

    /// Like [`Self::prepare_existing`], before removing the role's policies and the role.
    /// The boundary is attached so the bounded cleanup grants apply, but AWS-managed
    /// policies are not refused: the role is being deleted, not kept running under it.
    pub async fn prepare_for_removal(
        &self,
        role_name: &str,
        deployment_prefix: &str,
        resource_id: &str,
    ) -> Result<()> {
        self.prepare(role_name, deployment_prefix, resource_id, false)
            .await
    }

    /// Verifies runtime ownership without adopting the role, before the calls that are granted
    /// on runtime-owned roles without the boundary: detaching the AWS-managed policies that
    /// earlier releases attached. With no deployment boundary, this performs no IAM calls.
    pub async fn verify_owned(
        &self,
        role_name: &str,
        deployment_prefix: &str,
        resource_id: &str,
    ) -> Result<()> {
        if let Some(resource_type) = self.setup_owned {
            return self
                .prepare_setup_owned(role_name, deployment_prefix, resource_id, resource_type)
                .await;
        }
        if self.permissions_boundary.is_none() {
            return Ok(());
        }
        self.owned_role(role_name, deployment_prefix, resource_id)
            .await
            .map(|_| ())
    }

    async fn prepare(
        &self,
        role_name: &str,
        deployment_prefix: &str,
        resource_id: &str,
        audit_attached_policies: bool,
    ) -> Result<()> {
        if let Some(resource_type) = self.setup_owned {
            return self
                .prepare_setup_owned(role_name, deployment_prefix, resource_id, resource_type)
                .await;
        }
        let Some(boundary) = self.permissions_boundary else {
            return Ok(());
        };
        let role = self
            .owned_role(role_name, deployment_prefix, resource_id)
            .await?;
        if let Some(attached) = role.permissions_boundary {
            if attached.permissions_boundary_arn.as_deref() == Some(boundary) {
                return Ok(());
            }
            return Err(AlienError::new(ErrorData::InvalidInput {
                message: format!("Role '{role_name}' has a different permissions boundary; reconcile it in setup before management writes this role"),
                field_name: Some("permissionsBoundary".to_owned()),
            }));
        }

        if audit_attached_policies {
            self.audit_attached_policies(role_name).await?;
        }
        self.iam
            .put_role_permissions_boundary(role_name, boundary)
            .await
    }

    /// A setup-owned role must belong to this deployment's resource, tagged by setup or, from an
    /// earlier direct setup, by the runtime. Such an earlier role is retagged `managed-by=setup`
    /// and loses the deployment boundary that release attached. Any other boundary is refused.
    async fn prepare_setup_owned(
        &self,
        role_name: &str,
        deployment_prefix: &str,
        resource_id: &str,
        resource_type: &str,
    ) -> Result<()> {
        let role = self.iam.get_role(role_name).await?.get_role_result.role;
        let has = |tags: &[(String, String)]| {
            role.tags.as_ref().is_some_and(|role_tags| {
                tags.iter().all(|(key, value)| {
                    role_tags
                        .member
                        .iter()
                        .any(|tag| &tag.key == key && &tag.value == value)
                })
            })
        };
        let setup_tags: Vec<_> = setup_resource_tags(deployment_prefix, resource_id, resource_type)
            .into_iter()
            .collect();
        let runtime_tags: Vec<_> = standard_resource_tags(deployment_prefix, resource_id)
            .into_iter()
            .collect();
        let tagged_for_setup = has(&setup_tags);
        if !tagged_for_setup && !has(&runtime_tags) {
            return Err(AlienError::new(ErrorData::ExistingCloudResourceNotOwned {
                resource_id: resource_id.to_owned(),
                object: format!("IAM role '{role_name}'"),
            }));
        }
        let attached = role
            .permissions_boundary
            .as_ref()
            .and_then(|boundary| boundary.permissions_boundary_arn.as_deref());
        if attached.is_some() && attached != self.permissions_boundary {
            return Err(AlienError::new(ErrorData::InvalidInput {
                message: format!("Role '{role_name}' has a permissions boundary setup did not attach; reconcile it before setup writes this role"),
                field_name: Some("permissionsBoundary".to_owned()),
            }));
        }
        if !tagged_for_setup {
            self.iam
                .tag_role(
                    role_name,
                    setup_tags
                        .into_iter()
                        .map(|(key, value)| CreateRoleTag { key, value })
                        .collect(),
                )
                .await?;
        }
        if attached.is_some() {
            self.iam.delete_role_permissions_boundary(role_name).await?;
        }
        Ok(())
    }

    async fn owned_role(
        &self,
        role_name: &str,
        deployment_prefix: &str,
        resource_id: &str,
    ) -> Result<Role> {
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
        Ok(role)
    }

    async fn audit_attached_policies(&self, role_name: &str) -> Result<()> {
        let policies = self
            .iam
            .list_attached_role_policies(role_name)
            .await?
            .list_attached_role_policies_result;
        // Never adopt on a partial audit, including responses from other IamApi implementations.
        if policies.is_truncated != Some(false) {
            return Err(AlienError::new(ErrorData::InvalidInput {
                message: format!(
                    "Cannot adopt role '{role_name}': its attached policy list is truncated or not confirmed complete"
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
        Ok(())
    }
}
