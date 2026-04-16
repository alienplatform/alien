use std::time::Duration;
use tracing::{info, warn};

use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use alien_aws_clients::iam::CreateRoleRequest;
use alien_core::permissions::PermissionSet;
use alien_core::{
    RemoteStackManagement, RemoteStackManagementOutputs, ResourceOutputs, ResourceStatus,
};
use alien_error::{AlienError, Context, ContextError, IntoAlienError};
use alien_macros::{controller, flow_entry, handler, terminal_state};
use alien_permissions::{
    generators::AwsRuntimePermissionsGenerator, get_permission_set, BindingTarget,
    PermissionContext,
};

/// Generates the AWS IAM role name for RemoteStackManagement.
fn get_aws_management_role_name(prefix: &str) -> String {
    format!("{}-management", prefix)
}

// Define the inline policy name we will manage
const MANAGED_POLICY_NAME: &str = "alien-management-policy";

#[controller]
pub struct AwsRemoteStackManagementController {
    /// The ARN of the created IAM role.
    pub(crate) role_arn: Option<String>,
    /// The name of the created IAM role.
    pub(crate) role_name: Option<String>,
    /// Whether management permissions have been applied
    pub(crate) management_permissions_applied: bool,
}

#[controller]
impl AwsRemoteStackManagementController {
    // ─────────────── CREATE FLOW ──────────────────────────────

    #[flow_entry(Create)]
    #[handler(
        state = CreatingRole,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_role(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<RemoteStackManagement>()?;
        let aws_config = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_iam_client(aws_config).await?;

        let role_name = get_aws_management_role_name(ctx.resource_prefix);

        // Get management role ARN from stack settings (required for cross-account management)
        let aws_management = ctx.get_aws_management_config()?
            .ok_or_else(|| AlienError::new(ErrorData::InfrastructureError {
                message: "AWS management configuration is required for RemoteStackManagement. Please configure management settings in your stack.".to_string(),
                operation: Some("create_cross_account_role".to_string()),
                resource_id: Some(config.id.clone()),
            }))?;
        let managing_role_arn = &aws_management.managing_role_arn;

        let assume_role_policy = Self::generate_cross_account_trust_policy(managing_role_arn);

        info!(
            role_name = %role_name,
            managing_role_arn = %managing_role_arn,
            "Creating cross-account management IAM role"
        );

        let role_request = CreateRoleRequest::builder()
            .role_name(role_name.clone())
            .assume_role_policy_document(assume_role_policy)
            .description(format!(
                "Cross-account management role for Alien stack {}",
                ctx.resource_prefix
            ))
            .build();

        let created_role =
            client
                .create_role(role_request)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to create cross-account management IAM role '{}'",
                        role_name
                    ),
                    resource_id: Some(config.id.clone()),
                })?;

        let role_arn = created_role.create_role_result.role.arn;

        info!(
            role_name = %role_name,
            role_arn = %role_arn,
            "Cross-account management IAM role created successfully"
        );

        self.role_name = Some(role_name);
        self.role_arn = Some(role_arn);

        Ok(HandlerAction::Continue {
            state: ApplyingManagementPermissions,
            suggested_delay: None,
        })
    }

    #[handler(
        state = ApplyingManagementPermissions,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn applying_management_permissions(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<RemoteStackManagement>()?;
        let aws_config = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_iam_client(aws_config).await?;
        let role_name = self.role_name.as_ref().unwrap();

        info!(
            role_name = %role_name,
            "Applying management permission sets to cross-account IAM role"
        );

        // Generate management policy document from the stack's management permission profile
        let policy_document = self.generate_management_policy_document(ctx)?;

        if !policy_document.is_empty() {
            client
                .put_role_policy(role_name, MANAGED_POLICY_NAME, &policy_document)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to apply management permissions to IAM role '{}'",
                        role_name
                    ),
                    resource_id: Some(config.id.clone()),
                })?;

            info!(
                role_name = %role_name,
                "Management permissions applied successfully"
            );
        } else {
            info!(
                role_name = %role_name,
                "No management permissions to apply"
            );
        }

        self.management_permissions_applied = true;

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    // ─────────────── READY STATE ──────────────────────────────

    #[handler(state = Ready, on_failure = RefreshFailed, status = ResourceStatus::Running)]
    async fn ready(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let aws_config = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_iam_client(aws_config).await?;
        let role_name = self.role_name.as_ref().unwrap();

        // Heartbeat check: verify role still exists
        let role = client
            .get_role(role_name)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to get cross-account management IAM role during heartbeat check"
                    .to_string(),
                resource_id: Some(
                    ctx.desired_resource_config::<RemoteStackManagement>()?
                        .id
                        .clone(),
                ),
            })?;

        // Check if role ARN matches what we expect
        if let Some(expected_arn) = &self.role_arn {
            if role.get_role_result.role.arn != *expected_arn {
                return Err(AlienError::new(ErrorData::ResourceDrift {
                    resource_id: ctx
                        .desired_resource_config::<RemoteStackManagement>()?
                        .id
                        .clone(),
                    message: format!(
                        "Role ARN changed from {} to {}",
                        expected_arn, role.get_role_result.role.arn
                    ),
                }));
            }
        }

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: Some(Duration::from_secs(30)), // Check again in 30 seconds
        })
    }

    // ─────────────── UPDATE FLOW ──────────────────────────────

    #[flow_entry(Update, from = [Ready, RefreshFailed])]
    #[handler(
        state = UpdateStart,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<RemoteStackManagement>()?;
        let aws_config = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_iam_client(aws_config).await?;
        let role_name = self.role_name.as_ref().unwrap();

        info!(
            role_name = %role_name,
            "Updating cross-account management IAM role policies"
        );

        // Re-generate and apply management permissions
        let policy_document = self.generate_management_policy_document(ctx)?;

        if !policy_document.is_empty() {
            client
                .put_role_policy(role_name, MANAGED_POLICY_NAME, &policy_document)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to update management permissions for IAM role '{}'",
                        role_name
                    ),
                    resource_id: Some(config.id.clone()),
                })?;

            info!(
                role_name = %role_name,
                "Cross-account management IAM role policies updated successfully"
            );
        } else {
            // Remove policy if no permissions are needed
            match client
                .delete_role_policy(role_name, MANAGED_POLICY_NAME)
                .await
            {
                Ok(_) => {
                    info!(role_name = %role_name, "Removed empty management policy from IAM role");
                }
                Err(e) => {
                    // Policy might not exist, which is fine
                    warn!(role_name = %role_name, error = %e, "Failed to delete management policy during update (policy might not exist)");
                }
            }
        }

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    // ─────────────── DELETE FLOW ──────────────────────────────

    #[flow_entry(Delete)]
    #[handler(
        state = DeleteStart,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn delete_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let role_name = match &self.role_name {
            Some(name) => name,
            None => {
                // No role was created, skip to deleted
                return Ok(HandlerAction::Continue {
                    state: Deleted,
                    suggested_delay: None,
                });
            }
        };

        let aws_config = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_iam_client(aws_config).await?;

        info!(
            role_name = %role_name,
            "Starting comprehensive policy cleanup before management role deletion"
        );

        // Step 1: List and detach all managed (attached) policies
        match client.list_attached_role_policies(role_name).await {
            Ok(response) => {
                if let Some(attached_policies) = response
                    .list_attached_role_policies_result
                    .attached_policies
                {
                    for policy in &attached_policies.member {
                        info!(
                            role_name = %role_name,
                            policy_arn = %policy.policy_arn,
                            "Detaching managed policy from management role"
                        );
                        match client
                            .detach_role_policy(role_name, &policy.policy_arn)
                            .await
                        {
                            Ok(_) => {}
                            Err(e) => {
                                if let Some(
                                    alien_client_core::ErrorData::RemoteResourceNotFound { .. },
                                ) = &e.error
                                {
                                    warn!(role_name = %role_name, policy_arn = %policy.policy_arn, "Managed policy already detached");
                                } else {
                                    return Err(e.context(ErrorData::CloudPlatformError {
                                        message: format!(
                                            "Failed to detach managed policy '{}' from management role",
                                            policy.policy_arn
                                        ),
                                        resource_id: Some("remote-stack-management".to_string()),
                                    }).into());
                                }
                            }
                        }
                    }
                }
            }
            Err(e) => {
                if let Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. }) = &e.error
                {
                    warn!(role_name = %role_name, "Management role not found when listing attached policies");
                } else {
                    return Err(e
                        .context(ErrorData::CloudPlatformError {
                            message: "Failed to list attached policies on management role"
                                .to_string(),
                            resource_id: Some("remote-stack-management".to_string()),
                        })
                        .into());
                }
            }
        }

        // Step 2: List and delete all inline policies
        match client.list_role_policies(role_name).await {
            Ok(response) => {
                if let Some(policy_names) = response.list_role_policies_result.policy_names {
                    for policy_name in &policy_names.member {
                        info!(
                            role_name = %role_name,
                            policy_name = %policy_name,
                            "Deleting inline policy from management role"
                        );
                        match client.delete_role_policy(role_name, policy_name).await {
                            Ok(_) => {}
                            Err(e) => {
                                if let Some(
                                    alien_client_core::ErrorData::RemoteResourceNotFound { .. },
                                ) = &e.error
                                {
                                    warn!(role_name = %role_name, policy_name = %policy_name, "Inline policy already deleted");
                                } else {
                                    return Err(e.context(ErrorData::CloudPlatformError {
                                        message: format!(
                                            "Failed to delete inline policy '{}' from management role",
                                            policy_name
                                        ),
                                        resource_id: Some("remote-stack-management".to_string()),
                                    }).into());
                                }
                            }
                        }
                    }
                }
            }
            Err(e) => {
                if let Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. }) = &e.error
                {
                    warn!(role_name = %role_name, "Management role not found when listing inline policies");
                } else {
                    return Err(e
                        .context(ErrorData::CloudPlatformError {
                            message: "Failed to list inline policies on management role"
                                .to_string(),
                            resource_id: Some("remote-stack-management".to_string()),
                        })
                        .into());
                }
            }
        }

        info!(
            role_name = %role_name,
            "Policy cleanup completed, proceeding to management role deletion"
        );

        Ok(HandlerAction::Continue {
            state: DeletingRole,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeletingRole,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_role(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let role_name = self.role_name.as_ref().unwrap();
        let aws_config = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_iam_client(aws_config).await?;

        info!(
            role_name = %role_name,
            "Deleting cross-account management IAM role"
        );

        match client.delete_role(role_name).await {
            Ok(_) => {
                info!(
                    role_name = %role_name,
                    "Cross-account management IAM role deleted successfully"
                );
            }
            Err(e) => {
                // Check if it's a resource not found error (role doesn't exist)
                if let Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. }) = &e.error
                {
                    warn!(role_name = %role_name, "Cross-account management IAM role already deleted");
                } else {
                    return Err(e
                        .context(ErrorData::CloudPlatformError {
                            message: "Failed to delete cross-account management IAM role"
                                .to_string(),
                            resource_id: Some(
                                ctx.desired_resource_config::<RemoteStackManagement>()?
                                    .id
                                    .clone(),
                            ),
                        })
                        .into());
                }
            }
        }

        self.role_name = None;
        self.role_arn = None;
        self.management_permissions_applied = false;

        Ok(HandlerAction::Continue {
            state: Deleted,
            suggested_delay: None,
        })
    }

    // ─────────────── TERMINAL STATES ──────────────────────────

    terminal_state!(
        state = CreateFailed,
        status = ResourceStatus::ProvisionFailed
    );
    terminal_state!(state = UpdateFailed, status = ResourceStatus::UpdateFailed);
    terminal_state!(state = DeleteFailed, status = ResourceStatus::DeleteFailed);
    terminal_state!(state = Deleted, status = ResourceStatus::Deleted);
    terminal_state!(
        state = RefreshFailed,
        status = ResourceStatus::RefreshFailed
    );

    fn build_outputs(&self) -> Option<ResourceOutputs> {
        if let (Some(role_arn), Some(role_name)) = (&self.role_arn, &self.role_name) {
            Some(ResourceOutputs::new(RemoteStackManagementOutputs {
                management_resource_id: role_arn.clone(),
                access_configuration: role_arn.clone(),
            }))
        } else {
            None
        }
    }
}

// Separate impl block for helper methods
impl AwsRemoteStackManagementController {
    /// Generate trust policy for cross-account access with specific role restriction
    fn generate_cross_account_trust_policy(managing_role_arn: &str) -> String {
        format!(
            r#"{{
                "Version": "2012-10-17",
                "Statement": [
                    {{
                        "Sid": "AllowOnlySpecificSourceRole",
                        "Effect": "Allow",
                        "Principal": {{
                            "AWS": "{}"
                        }},
                        "Action": "sts:AssumeRole",
                        "Condition": {{
                            "StringEquals": {{
                                "aws:PrincipalArn": "{}"
                            }}
                        }}
                    }}
                ]
            }}"#,
            managing_role_arn, managing_role_arn
        )
    }

    /// Generate IAM policy document for management permissions from the stack's management profile
    fn generate_management_policy_document(
        &self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<String> {
        // Get the management permission profile from the stack
        // The stack processor should have processed the management permissions
        let management_permissions = ctx.desired_stack.management();
        let management_profile = management_permissions.profile()
            .ok_or_else(|| AlienError::new(ErrorData::InfrastructureError {
                message: "Management permissions not configured or set to Auto. Management permissions must be explicitly configured for remote stack management.".to_string(),
                operation: Some("generate_management_policy_document".to_string()),
                resource_id: Some("management".to_string()),
            }))?;

        // Get the global permissions for management (should be under "*")
        let global_permission_set_ids = management_profile.0.get("*").ok_or_else(|| {
            AlienError::new(ErrorData::InfrastructureError {
                message: "Management permission profile missing global permissions (*)".to_string(),
                operation: Some("generate_management_policy_document".to_string()),
                resource_id: Some("management".to_string()),
            })
        })?;

        // Include all permission sets from the management profile in the stack-level
        // management policy. The management profile is curated by
        // ManagementPermissionProfileMutation to include only what the management role
        // needs: /provision sets for lifecycle operations, /management sets for ongoing
        // management, /heartbeat and /telemetry for health checks, and /data-write
        // sets for manager-level operations like the vault API.
        //
        // Using stack-level wildcard bindings ensures the management role has all
        // necessary permissions from the moment it's created, without depending on
        // per-resource policy propagation timing.
        let mut combined_actions = Vec::new();
        let mut combined_resources = std::collections::HashSet::new();

        for permission_set_ref in global_permission_set_ids {
            let permission_set =
                permission_set_ref.resolve(|name| get_permission_set(name).cloned());
            if let Some(permission_set) = permission_set {
                if let Some(aws_platform) = &permission_set.platforms.aws {
                    for platform_permission in aws_platform {
                        if let Some(actions) = &platform_permission.grant.actions {
                            combined_actions.extend(actions.clone());
                        }

                        if let Some(stack_binding) = &platform_permission.binding.stack {
                            combined_resources.extend(stack_binding.resources.clone());
                        }
                    }
                }
            }
        }

        // Always include iam:GetRole for the management role itself. During the Running
        // phase the manager impersonates this role and runs heartbeat checks on all resources,
        // including the RSM resource. The RSM Ready handler calls iam:GetRole on the
        // management role to verify it still exists, so the role needs this self-permission.
        if !combined_actions.contains(&"iam:GetRole".to_string()) {
            combined_actions.push("iam:GetRole".to_string());
        }
        if let Some(role_name) = &self.role_name {
            let aws_config_for_arn = ctx.get_aws_config()?;
            let self_role_arn = format!(
                "arn:aws:iam::{}:role/{}",
                aws_config_for_arn.account_id, role_name
            );
            combined_resources.insert(self_role_arn);
        }

        // Remove duplicates from actions
        combined_actions.sort();
        combined_actions.dedup();

        // If no actions were found (shouldn't happen now since we always add iam:GetRole),
        // return an empty string so that callers skip the PutRolePolicy call.
        if combined_actions.is_empty() {
            return Ok(String::new());
        }

        // Create the combined permission set
        let management_permission_set = PermissionSet {
            id: "management".to_string(),
            description: "Auto-generated management permissions for stack".to_string(),
            platforms: alien_core::permissions::PlatformPermissions {
                aws: Some(vec![alien_core::permissions::AwsPlatformPermission {
                    grant: alien_core::permissions::PermissionGrant {
                        actions: Some(combined_actions),
                        permissions: None,
                        data_actions: None,
                    },
                    binding: alien_core::permissions::BindingConfiguration {
                        stack: Some(alien_core::permissions::AwsBindingSpec {
                            resources: combined_resources.into_iter().collect(),
                            condition: None,
                        }),
                        resource: None,
                    },
                }]),
                gcp: None,
                azure: None,
            },
        };

        let generator = AwsRuntimePermissionsGenerator::new();

        let aws_config = ctx.get_aws_config()?;
        let mut permission_context = PermissionContext::new()
            .with_stack_prefix(ctx.resource_prefix.to_string())
            .with_aws_region(aws_config.region.clone())
            .with_aws_account_id(aws_config.account_id.clone());

        // Add managing role ARN from stack settings for cross-account access (required)
        let aws_management = ctx.get_aws_management_config()?
            .ok_or_else(|| AlienError::new(ErrorData::InfrastructureError {
                message: "AWS management configuration is required for RemoteStackManagement permissions. Please configure management settings in your stack.".to_string(),
                operation: Some("apply_management_permissions".to_string()),
                resource_id: Some("management".to_string()),
            }))?;
        permission_context =
            permission_context.with_managing_role_arn(aws_management.managing_role_arn.clone());

        // Extract and set managing account ID from role ARN
        if let Some(managing_account_id) =
            alien_permissions::PermissionContext::extract_account_id_from_role_arn(
                &aws_management.managing_role_arn,
            )
        {
            permission_context = permission_context.with_managing_account_id(managing_account_id);
        }

        // Generate stack-level policy for the management permission set
        let policy = generator
            .generate_policy(
                &management_permission_set,
                BindingTarget::Stack,
                &permission_context,
            )
            .context(ErrorData::InfrastructureError {
                message: "Failed to generate IAM policy for management permission set".to_string(),
                operation: Some("generate_management_policy_document".to_string()),
                resource_id: Some("management".to_string()),
            })?;

        let policy_document = serde_json::to_string_pretty(&policy)
            .into_alien_error()
            .context(ErrorData::InfrastructureError {
                message: "Failed to serialize management IAM policy document".to_string(),
                operation: Some("generate_management_policy_document".to_string()),
                resource_id: Some("management".to_string()),
            })?;

        Ok(policy_document)
    }

    /// Creates a controller in a ready state with mock values for testing purposes.
    #[cfg(feature = "test-utils")]
    pub fn mock_ready(role_name: &str) -> Self {
        Self {
            state: AwsRemoteStackManagementState::Ready,
            role_arn: Some(format!("arn:aws:iam::123456789012:role/{}", role_name)),
            role_name: Some(role_name.to_string()),
            management_permissions_applied: true,
            _internal_stay_count: None,
        }
    }
}
