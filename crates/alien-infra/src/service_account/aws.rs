use std::time::Duration;
use tracing::{info, warn};

use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use alien_aws_clients::iam::{
    CreateRoleRequest, TrustPolicyDocument, TrustPolicyPrincipal, TrustPolicyPrincipalValue,
    TrustPolicyStatement,
};
use alien_core::{
    Build, Container, ContainerCluster, Function, ResourceOutputs, ResourceStatus, ServiceAccount,
    ServiceAccountOutputs,
};
use alien_error::{AlienError, Context, ContextError, IntoAlienError};
use alien_macros::{controller, flow_entry, handler, terminal_state};
use alien_permissions::{
    generators::{AwsIamPolicy, AwsIamStatement, AwsRuntimePermissionsGenerator},
    BindingTarget, PermissionContext,
};

/// Generates the AWS IAM role name for a ServiceAccount.
fn get_aws_role_name(prefix: &str, name: &str) -> String {
    format!("{}-{}", prefix, name)
}

// Define the inline policy name we will manage
const MANAGED_POLICY_NAME: &str = "alien-managed-policy";

#[controller]
pub struct AwsServiceAccountController {
    /// The ARN of the created IAM role.
    pub role_arn: Option<String>,
    /// The name of the created IAM role.
    pub(crate) role_name: Option<String>,
    /// Whether stack-level permissions have been applied
    pub(crate) stack_permissions_applied: bool,
}

#[controller]
impl AwsServiceAccountController {
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
        let config = ctx.desired_resource_config::<ServiceAccount>()?;
        let aws_config = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_iam_client(aws_config).await?;

        let role_name = get_aws_role_name(ctx.resource_prefix, &config.id);
        let assume_role_policy =
            Self::generate_assume_role_policy_for_service_account(config, ctx)?;

        info!(
            role_name = %role_name,
            service_account_id = %config.id,
            "Creating IAM role for ServiceAccount"
        );

        let role_request = CreateRoleRequest::builder()
            .role_name(role_name.clone())
            .assume_role_policy_document(assume_role_policy)
            .description(format!(
                "Service account role for Alien resource {}",
                config.id
            ))
            .build();

        let created_role =
            client
                .create_role(role_request)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!("Failed to create IAM role '{}'", role_name),
                    resource_id: Some(config.id.clone()),
                })?;

        let role_arn = created_role.create_role_result.role.arn;

        info!(
            role_name = %role_name,
            role_arn = %role_arn,
            "IAM role created successfully"
        );

        self.role_name = Some(role_name);
        self.role_arn = Some(role_arn);

        Ok(HandlerAction::Continue {
            state: ApplyingStackPermissions,
            suggested_delay: None,
        })
    }

    #[handler(
        state = ApplyingStackPermissions,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn applying_stack_permissions(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<ServiceAccount>()?;
        let aws_config = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_iam_client(aws_config).await?;
        let role_name = self.role_name.as_ref().unwrap();

        info!(
            role_name = %role_name,
            stack_permission_sets_count = config.stack_permission_sets.len(),
            "Applying stack-level permission sets to IAM role"
        );

        // Generate combined policy document for all stack-level permission sets
        let policy_document = self.generate_stack_policy_document(config, ctx)?;

        if !policy_document.is_empty() {
            client
                .put_role_policy(role_name, MANAGED_POLICY_NAME, &policy_document)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to apply stack permissions to IAM role '{}'",
                        role_name
                    ),
                    resource_id: Some(config.id.clone()),
                })?;

            info!(
                role_name = %role_name,
                "Stack-level permissions applied successfully"
            );
        } else {
            info!(
                role_name = %role_name,
                "No stack-level permissions to apply"
            );
        }

        self.stack_permissions_applied = true;

        Ok(HandlerAction::Continue {
            state: ApplyingResourcePermissions,
            suggested_delay: None,
        })
    }

    #[handler(
        state = ApplyingResourcePermissions,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn applying_resource_permissions(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<ServiceAccount>()?;

        info!(
            service_account_id = %config.id,
            "Applying resource-scoped permissions for service account"
        );

        // Apply resource-scoped permissions using the centralized helper.
        // This attaches management SA permissions (e.g., service-account/heartbeat)
        // as inline policies on the management role.
        {
            use crate::core::ResourcePermissionsHelper;
            ResourcePermissionsHelper::apply_aws_resource_scoped_permissions(
                ctx,
                &config.id,
                &config.id,
                "service-account",
            )
            .await?;
        }

        info!(
            service_account_id = %config.id,
            "Successfully applied resource-scoped permissions for service account"
        );

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
                message: "Failed to get IAM role during heartbeat check".to_string(),
                resource_id: Some(ctx.desired_resource_config::<ServiceAccount>()?.id.clone()),
            })?;

        // Check if role ARN matches what we expect
        if let Some(expected_arn) = &self.role_arn {
            if role.get_role_result.role.arn != *expected_arn {
                return Err(AlienError::new(ErrorData::ResourceDrift {
                    resource_id: ctx.desired_resource_config::<ServiceAccount>()?.id.clone(),
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
        let config = ctx.desired_resource_config::<ServiceAccount>()?;
        let aws_config = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_iam_client(aws_config).await?;
        let role_name = self.role_name.as_ref().unwrap();

        info!(
            role_name = %role_name,
            "Updating IAM role policies"
        );

        // Re-generate and apply stack-level permissions
        let policy_document = self.generate_stack_policy_document(config, ctx)?;

        if !policy_document.is_empty() {
            client
                .put_role_policy(role_name, MANAGED_POLICY_NAME, &policy_document)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to update stack permissions for IAM role '{}'",
                        role_name
                    ),
                    resource_id: Some(config.id.clone()),
                })?;

            info!(
                role_name = %role_name,
                "IAM role policies updated successfully"
            );
        } else {
            // Remove policy if no permissions are needed
            match client
                .delete_role_policy(role_name, MANAGED_POLICY_NAME)
                .await
            {
                Ok(_) => {
                    info!(role_name = %role_name, "Removed empty policy from IAM role");
                }
                Err(e) => {
                    // Policy might not exist, which is fine
                    warn!(role_name = %role_name, error = %e, "Failed to delete policy during update (policy might not exist)");
                }
            }
        }

        Ok(HandlerAction::Continue {
            state: ApplyingResourcePermissions,
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
            "Starting comprehensive policy cleanup before role deletion"
        );

        // Step 1: List and detach all managed policies
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
                            policy_name = %policy.policy_name,
                            "Detaching managed policy"
                        );

                        match client
                            .detach_role_policy(role_name, &policy.policy_arn)
                            .await
                        {
                            Ok(_) => {
                                info!(
                                    role_name = %role_name,
                                    policy_arn = %policy.policy_arn,
                                    "Managed policy detached successfully"
                                );
                            }
                            Err(e) => {
                                // Check if it's a resource not found error
                                if let Some(
                                    alien_client_core::ErrorData::RemoteResourceNotFound { .. },
                                ) = &e.error
                                {
                                    warn!(
                                        role_name = %role_name,
                                        policy_arn = %policy.policy_arn,
                                        "Managed policy already detached or doesn't exist"
                                    );
                                } else {
                                    return Err(e
                                        .context(ErrorData::CloudPlatformError {
                                            message: format!(
                                                "Failed to detach managed policy '{}'",
                                                policy.policy_arn
                                            ),
                                            resource_id: Some(
                                                ctx.desired_resource_config::<ServiceAccount>()?
                                                    .id
                                                    .clone(),
                                            ),
                                        })
                                        .into());
                                }
                            }
                        }
                    }
                } else {
                    info!(role_name = %role_name, "No managed policies attached to role");
                }
            }
            Err(e) => {
                // Check if it's a resource not found error (role doesn't exist)
                if let Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. }) = &e.error
                {
                    warn!(role_name = %role_name, "Role not found during managed policy cleanup");
                } else {
                    return Err(e
                        .context(ErrorData::CloudPlatformError {
                            message: "Failed to list attached managed policies".to_string(),
                            resource_id: Some(
                                ctx.desired_resource_config::<ServiceAccount>()?.id.clone(),
                            ),
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
                            "Deleting inline policy"
                        );

                        match client.delete_role_policy(role_name, policy_name).await {
                            Ok(_) => {
                                info!(
                                    role_name = %role_name,
                                    policy_name = %policy_name,
                                    "Inline policy deleted successfully"
                                );
                            }
                            Err(e) => {
                                // Check if it's a resource not found error
                                if let Some(
                                    alien_client_core::ErrorData::RemoteResourceNotFound { .. },
                                ) = &e.error
                                {
                                    warn!(
                                        role_name = %role_name,
                                        policy_name = %policy_name,
                                        "Inline policy already deleted or doesn't exist"
                                    );
                                } else {
                                    return Err(e
                                        .context(ErrorData::CloudPlatformError {
                                            message: format!(
                                                "Failed to delete inline policy '{}'",
                                                policy_name
                                            ),
                                            resource_id: Some(
                                                ctx.desired_resource_config::<ServiceAccount>()?
                                                    .id
                                                    .clone(),
                                            ),
                                        })
                                        .into());
                                }
                            }
                        }
                    }
                } else {
                    info!(role_name = %role_name, "No inline policies attached to role");
                }
            }
            Err(e) => {
                // Check if it's a resource not found error (role doesn't exist)
                if let Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. }) = &e.error
                {
                    warn!(role_name = %role_name, "Role not found during inline policy cleanup");
                } else {
                    return Err(e
                        .context(ErrorData::CloudPlatformError {
                            message: "Failed to list inline policies".to_string(),
                            resource_id: Some(
                                ctx.desired_resource_config::<ServiceAccount>()?.id.clone(),
                            ),
                        })
                        .into());
                }
            }
        }

        info!(role_name = %role_name, "Policy cleanup completed, proceeding to role deletion");

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
            "Deleting IAM role"
        );

        match client.delete_role(role_name).await {
            Ok(_) => {
                info!(
                    role_name = %role_name,
                    "IAM role deleted successfully"
                );
            }
            Err(e) => {
                // Check if it's a resource not found error (role doesn't exist)
                if let Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. }) = &e.error
                {
                    warn!(role_name = %role_name, "IAM role already deleted");
                } else {
                    return Err(e
                        .context(ErrorData::CloudPlatformError {
                            message: "Failed to delete IAM role".to_string(),
                            resource_id: Some(
                                ctx.desired_resource_config::<ServiceAccount>()?.id.clone(),
                            ),
                        })
                        .into());
                }
            }
        }

        self.role_name = None;
        self.role_arn = None;
        self.stack_permissions_applied = false;

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
            Some(ResourceOutputs::new(ServiceAccountOutputs {
                identity: role_arn.clone(),
                resource_id: role_name.clone(),
            }))
        } else {
            None
        }
    }

    fn get_binding_params(&self) -> Result<Option<serde_json::Value>> {
        use alien_core::bindings::{BindingValue, ServiceAccountBinding};

        if let (Some(role_arn), Some(role_name)) = (&self.role_arn, &self.role_name) {
            let binding = ServiceAccountBinding::aws_iam(
                BindingValue::Value(role_name.clone()),
                BindingValue::Value(role_arn.clone()),
            );
            Ok(Some(
                serde_json::to_value(binding).into_alien_error().context(
                    ErrorData::ResourceStateSerializationFailed {
                        resource_id: "binding".to_string(),
                        message: "Failed to serialize binding parameters".to_string(),
                    },
                )?,
            ))
        } else {
            Ok(None)
        }
    }
}

// Separate impl block for helper methods
impl AwsServiceAccountController {
    /// Generate assume role policy for the service account based on stack analysis.
    ///
    /// This function determines which AWS services and IAM roles should be allowed to assume
    /// this service account's IAM role by analyzing:
    /// 1. Functions/Builds that use a permission profile matching this ServiceAccount
    /// 2. Other ServiceAccounts that have impersonation permissions for this ServiceAccount
    ///
    /// **Naming Convention Assumption**: ServiceAccounts created from permission profiles follow
    /// the pattern `{profile-name}-sa`. For example, the "execution" profile creates
    /// an "execution-sa" ServiceAccount resource. This function attempts to find the
    /// corresponding profile by stripping the "-sa" suffix. If the ServiceAccount
    /// was created directly (not from a profile), or uses a non-standard name, it falls back to
    /// using the full ServiceAccount ID as the profile name.
    fn generate_assume_role_policy_for_service_account(
        service_account: &ServiceAccount,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<String> {
        let mut services = Vec::new();
        let mut role_arns = Vec::new();

        // Attempt to determine the corresponding permission profile name.
        // Convention: ServiceAccount "xyz-sa" corresponds to profile "xyz"
        // If no suffix is found, use the ServiceAccount ID as-is (handles direct ServiceAccount resources)
        let profile_name = service_account
            .id
            .strip_suffix("-sa")
            .unwrap_or(&service_account.id);

        // Analyze the stack in a single pass to determine:
        // 1. Which AWS services need to assume this role (Functions using this profile -> Lambda, Builds -> CodeBuild)
        // 2. Which other ServiceAccounts can impersonate this one
        for (_, resource_entry) in ctx.desired_stack.resources() {
            let resource = &resource_entry.config;

            // Check if there are Functions that use THIS service account's profile
            if let Some(function) = resource.downcast_ref::<Function>() {
                if function.get_permissions() == profile_name {
                    if !services.contains(&"lambda.amazonaws.com".to_string()) {
                        services.push("lambda.amazonaws.com".to_string());
                    }
                }
            }

            // Check if there are Builds that use THIS service account's profile
            if let Some(build) = resource.downcast_ref::<Build>() {
                if build.get_permissions() == profile_name {
                    if !services.contains(&"codebuild.amazonaws.com".to_string()) {
                        services.push("codebuild.amazonaws.com".to_string());
                    }
                }
            }
        }

        // Check if any Container in the stack uses this profile — if so, the ContainerCluster VM
        // role needs to assume this SA role to vend per-container credentials via the IMDS proxy.
        // The VM role ARN is deterministic: {prefix}-{clusterId}-role (set in container_cluster/aws.rs).
        let has_container_using_profile = ctx.desired_stack.resources().any(|(_, entry)| {
            entry
                .config
                .downcast_ref::<Container>()
                .map(|c| c.get_permissions() == profile_name)
                .unwrap_or(false)
        });

        if has_container_using_profile {
            let account_id = ctx
                .get_aws_config()
                .ok()
                .map(|c| c.account_id.clone())
                .unwrap_or_default();

            for (cluster_id, entry) in ctx.desired_stack.resources() {
                if entry.config.downcast_ref::<ContainerCluster>().is_some() {
                    let vm_role_arn = format!(
                        "arn:aws:iam::{}:role/{}-{}-role",
                        account_id, ctx.resource_prefix, cluster_id,
                    );
                    if !role_arns.contains(&vm_role_arn) {
                        info!(
                            service_account = %service_account.id,
                            cluster_id = %cluster_id,
                            vm_role_arn = %vm_role_arn,
                            "Adding ContainerCluster VM role to SA trust policy for IMDS credential vending"
                        );
                        role_arns.push(vm_role_arn);
                    }
                }
            }
        }

        // Find ServiceAccounts that can impersonate this service account by checking permission profiles.
        //
        // Impersonation logic: A permission profile with resource-scoped "service-account/impersonate"
        // permission for this ServiceAccount allows the ServiceAccount created from that profile to
        // assume this ServiceAccount's role.
        //
        // Example: Profile "execution" with resource-scoped permission:
        //   "agent-management": ["service-account/impersonate"]
        // allows "execution-sa" to assume "agent-management-sa"'s role.
        //
        // Note: This only works for ServiceAccounts created from profiles (via the ServiceAccountMutation).
        // Manually-defined ServiceAccounts cannot be granted impersonation permissions this way.
        //
        // The permission can be scoped to either:
        // 1. The profile name (e.g., "agent-management") that creates the ServiceAccount
        // 2. The full ServiceAccount resource ID (e.g., "agent-management-sa")
        for (profile_name, permission_profile) in &ctx.desired_stack.permissions.profiles {
            // Determine the profile name for this service account (strip "-sa" suffix if present)
            let target_profile_name = service_account
                .id
                .strip_suffix("-sa")
                .unwrap_or(&service_account.id);

            // Check if this profile has impersonate permission scoped to either:
            // - The profile name (e.g., "agent-management")
            // - The full ServiceAccount ID (e.g., "agent-management-sa")
            let permission_set_refs = permission_profile
                .0
                .get(target_profile_name)
                .or_else(|| permission_profile.0.get(&service_account.id));

            if let Some(permission_set_refs) = permission_set_refs {
                let has_impersonate = permission_set_refs.iter().any(|perm_ref| match perm_ref {
                    alien_core::permissions::PermissionSetReference::Name(name) => {
                        name == "service-account/impersonate"
                    }
                    alien_core::permissions::PermissionSetReference::Inline(inline) => {
                        inline.id == "service-account/impersonate"
                    }
                });

                if has_impersonate {
                    // Find the ServiceAccount resource created from this profile
                    // Convention: profile "xyz" creates ServiceAccount "xyz-sa"
                    let impersonator_sa_id = format!("{}-sa", profile_name);

                    // Verify this ServiceAccount actually exists in the stack
                    let sa_exists = ctx.desired_stack.resources().any(|(resource_id, entry)| {
                        resource_id == &impersonator_sa_id
                            && entry.config.downcast_ref::<ServiceAccount>().is_some()
                    });

                    if sa_exists && impersonator_sa_id != service_account.id {
                        let impersonator_role_arn = format!(
                            "arn:aws:iam::{}:role/{}-{}",
                            ctx.get_aws_config()
                                .ok()
                                .map(|c| c.account_id.clone())
                                .unwrap_or_default(),
                            ctx.resource_prefix,
                            impersonator_sa_id
                        );

                        if !role_arns.contains(&impersonator_role_arn) {
                            info!(
                                service_account = %service_account.id,
                                impersonator_profile = %profile_name,
                                impersonator_sa = %impersonator_sa_id,
                                impersonator_role = %impersonator_role_arn,
                                "Adding impersonator service account to trust policy"
                            );
                            role_arns.push(impersonator_role_arn);
                        }
                    }
                }
            }
        }

        // Build trust policy statements
        let mut statements = Vec::new();

        // Ensure we have at least one principal (service or role)
        if services.is_empty() && role_arns.is_empty() {
            warn!(
                service_account = %service_account.id,
                "No principals found for service account trust policy - defaulting to Lambda service"
            );
            services.push("lambda.amazonaws.com".to_string());
        }

        // Statement for AWS services (only if there are services that need to assume this role)
        if !services.is_empty() {
            let principal_value = if services.len() == 1 {
                TrustPolicyPrincipalValue::Single(services[0].clone())
            } else {
                TrustPolicyPrincipalValue::Multiple(services)
            };

            statements.push(TrustPolicyStatement {
                effect: "Allow".to_string(),
                principal: TrustPolicyPrincipal::Service {
                    service: principal_value,
                },
                action: "sts:AssumeRole".to_string(),
            });
        }

        // Statement for other IAM roles (impersonators)
        if !role_arns.is_empty() {
            let principal_value = if role_arns.len() == 1 {
                TrustPolicyPrincipalValue::Single(role_arns[0].clone())
            } else {
                TrustPolicyPrincipalValue::Multiple(role_arns)
            };

            statements.push(TrustPolicyStatement {
                effect: "Allow".to_string(),
                principal: TrustPolicyPrincipal::Aws {
                    aws: principal_value,
                },
                action: "sts:AssumeRole".to_string(),
            });
        }

        // Create the complete trust policy document
        let trust_policy = TrustPolicyDocument {
            version: "2012-10-17".to_string(),
            statement: statements,
        };

        // Serialize to JSON
        serde_json::to_string(&trust_policy)
            .into_alien_error()
            .context(ErrorData::InfrastructureError {
                message: "Failed to serialize trust policy document".to_string(),
                operation: Some("generate_assume_role_policy".to_string()),
                resource_id: Some(service_account.id.clone()),
            })
    }

    /// Generate IAM policy document for stack-level permission sets
    fn generate_stack_policy_document(
        &self,
        service_account: &ServiceAccount,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<String> {
        if service_account.stack_permission_sets.is_empty() {
            return Ok(String::new());
        }

        let generator = AwsRuntimePermissionsGenerator::new();
        let mut all_statements = Vec::new();

        let aws_config = ctx.get_aws_config()?;

        let mut permission_context = PermissionContext::new()
            .with_stack_prefix(ctx.resource_prefix.to_string())
            .with_aws_region(aws_config.region.clone())
            .with_aws_account_id(aws_config.account_id.clone());

        // Add managing account ID from stack settings if available
        if let Some(aws_management) = ctx.get_aws_management_config()? {
            permission_context =
                permission_context.with_managing_role_arn(aws_management.managing_role_arn.clone());

            // Extract and set managing account ID from role ARN
            if let Some(managing_account_id) =
                alien_permissions::PermissionContext::extract_account_id_from_role_arn(
                    &aws_management.managing_role_arn,
                )
            {
                permission_context =
                    permission_context.with_managing_account_id(managing_account_id);
            }
        }

        for permission_set in &service_account.stack_permission_sets {
            // Generate stack-level policy for this permission set
            let policy = generator
                .generate_policy(permission_set, BindingTarget::Stack, &permission_context)
                .context(ErrorData::InfrastructureError {
                    message: format!(
                        "Failed to generate IAM policy for permission set '{}'",
                        permission_set.id
                    ),
                    operation: Some("generate_stack_policy_document".to_string()),
                    resource_id: Some(service_account.id.clone()),
                })?;

            all_statements.extend(policy.statement);
        }

        // Cross-account ECR pull: when a managing account is configured and this
        // service account is used by Lambda functions, add ECR permissions so Lambda
        // can pull container images from the management account's ECR registry.
        if let Some(aws_management) = ctx.get_aws_management_config()? {
            let managing_account_id =
                alien_permissions::PermissionContext::extract_account_id_from_role_arn(
                    &aws_management.managing_role_arn,
                );
            let profile_name = service_account
                .id
                .strip_suffix("-sa")
                .unwrap_or(&service_account.id);
            let is_lambda_role = ctx.desired_stack.resources().any(|(_, entry)| {
                entry
                    .config
                    .downcast_ref::<Function>()
                    .map(|f| f.get_permissions() == profile_name)
                    .unwrap_or(false)
            });

            if is_lambda_role {
                if let Some(ref mgmt_account) = managing_account_id {
                    all_statements.push(AwsIamStatement {
                        sid: "EcrCrossAccountAuth".to_string(),
                        effect: "Allow".to_string(),
                        action: vec!["ecr:GetAuthorizationToken".to_string()],
                        resource: vec!["*".to_string()],
                        condition: None,
                    });
                    all_statements.push(AwsIamStatement {
                        sid: "EcrCrossAccountPull".to_string(),
                        effect: "Allow".to_string(),
                        action: vec![
                            "ecr:BatchGetImage".to_string(),
                            "ecr:GetDownloadUrlForLayer".to_string(),
                        ],
                        resource: vec![format!("arn:aws:ecr:*:{}:repository/*", mgmt_account)],
                        condition: None,
                    });
                }
            }
        }

        if all_statements.is_empty() {
            return Ok(String::new());
        }

        let final_policy = AwsIamPolicy {
            version: "2012-10-17".to_string(),
            statement: all_statements,
        };

        let policy_document = serde_json::to_string_pretty(&final_policy)
            .into_alien_error()
            .context(ErrorData::InfrastructureError {
                message: "Failed to serialize IAM policy document".to_string(),
                operation: Some("generate_stack_policy_document".to_string()),
                resource_id: Some(service_account.id.clone()),
            })?;

        if policy_document.len() > 10_240 {
            warn!(
                sa_id = %service_account.id,
                policy_size_bytes = policy_document.len(),
                "IAM inline policy exceeds 10,240 byte limit — PutRolePolicy will fail"
            );
        }

        Ok(policy_document)
    }

    /// Creates a controller in a ready state with mock values for testing purposes.
    #[cfg(feature = "test-utils")]
    pub fn mock_ready(role_name: &str) -> Self {
        Self {
            state: AwsServiceAccountState::Ready,
            role_arn: Some(format!("arn:aws:iam::123456789012:role/{}", role_name)),
            role_name: Some(role_name.to_string()),
            stack_permissions_applied: true,
            _internal_stay_count: None,
        }
    }
}
