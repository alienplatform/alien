use std::{collections::HashSet, time::Duration};
use tracing::{info, warn};

use crate::core::{ResourceControllerContext, ResourcePermissionsHelper};
use crate::error::{ErrorData, Result};
use alien_aws_clients::iam::{CreateRoleRequest, CreateRoleTag, IamApi};
use alien_core::{
    standard_resource_tags, AwsRemoteStackManagementHeartbeatData, HeartbeatBackend,
    KubernetesCluster, ObservedHealth, Platform, ProviderLifecycleState, RemoteStackManagement,
    RemoteStackManagementHeartbeatData, RemoteStackManagementHeartbeatStatus,
    RemoteStackManagementOutputs, ResourceHeartbeat, ResourceHeartbeatData, ResourceLifecycle,
    ResourceOutputs, ResourceStatus, Worker,
};
use alien_error::{AlienError, Context, ContextError, IntoAlienError};
use alien_macros::controller;
use alien_permissions::{
    generators::{AwsIamPolicy, AwsIamStatement, AwsRuntimePermissionsGenerator},
    get_permission_set, BindingTarget, PermissionContext,
};
use chrono::Utc;

/// Generates the AWS IAM role name for RemoteStackManagement.
fn get_aws_management_role_name(prefix: &str) -> String {
    format!("{}-management", prefix)
}

const LEGACY_INLINE_POLICY_NAME: &str = "alien-management-policy";
const MANAGED_POLICY_BASE_NAME: &str = "deployment-management";
const MAX_MANAGED_POLICY_BYTES: usize = 5_500;
const IAM_POLICY_NAME_MAX_LEN: usize = 128;

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
            .description(match ctx.deployment_name_for_metadata() {
                Some(deployment_name) => format!(
                    "Cross-account management IAM role for {deployment_name}. Resource prefix: {}.",
                    ctx.resource_prefix
                ),
                None => format!(
                    "Cross-account management IAM role. Resource prefix: {}.",
                    ctx.resource_prefix
                ),
            })
            .tags(
                standard_resource_tags(ctx.resource_prefix, &config.id)
                    .into_iter()
                    .map(|(key, value)| CreateRoleTag { key, value })
                    .collect(),
            )
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
        let aws_config = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_iam_client(aws_config).await?;
        let role_name = self.role_name.as_ref().unwrap();

        info!(
            role_name = %role_name,
            "Applying management permission sets to cross-account IAM role"
        );

        let policy_documents = self.generate_management_policy_documents(ctx)?;

        if !policy_documents.is_empty() {
            self.apply_management_policy_documents(
                ctx,
                client.as_ref(),
                role_name,
                &policy_documents,
            )
            .await?;
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

        emit_aws_remote_stack_management_heartbeat(ctx, self)?;

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
        let aws_config = ctx.get_aws_config()?;
        let client = ctx.service_provider.get_aws_iam_client(aws_config).await?;
        let role_name = self.role_name.as_ref().unwrap();

        info!(
            role_name = %role_name,
            "Updating cross-account management IAM role policies"
        );

        let policy_documents = self.generate_management_policy_documents(ctx)?;

        if !policy_documents.is_empty() {
            self.apply_management_policy_documents(
                ctx,
                client.as_ref(),
                role_name,
                &policy_documents,
            )
            .await?;

            info!(
                role_name = %role_name,
                "Cross-account management IAM role policies updated successfully"
            );
        } else {
            self.reconcile_owned_management_policies(ctx, client.as_ref(), role_name, &[])
                .await?;
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
                        let detach_result = client
                            .detach_role_policy(role_name, &policy.policy_arn)
                            .await;
                        match detach_result {
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

                        if self.is_owned_management_policy_name(&policy.policy_name) {
                            self.delete_owned_policy(client.as_ref(), &policy.policy_arn)
                                .await?;
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
        if let (Some(role_arn), Some(_role_name)) = (&self.role_arn, &self.role_name) {
            Some(ResourceOutputs::new(RemoteStackManagementOutputs {
                management_resource_id: role_arn.clone(),
                access_configuration: role_arn.clone(),
            }))
        } else {
            None
        }
    }
}

fn emit_aws_remote_stack_management_heartbeat(
    ctx: &ResourceControllerContext<'_>,
    controller: &AwsRemoteStackManagementController,
) -> Result<()> {
    let config = ctx.desired_resource_config::<RemoteStackManagement>()?;

    ctx.emit_heartbeat(ResourceHeartbeat {
        deployment_id: None,
        resource_id: config.id.clone(),
        resource_type: RemoteStackManagement::RESOURCE_TYPE,
        controller_platform: Platform::Aws,
        backend: HeartbeatBackend::Aws,
            source: Default::default(),
            alien_resource_id: None,
        observed_at: Utc::now(),
        data: ResourceHeartbeatData::RemoteStackManagement(
            RemoteStackManagementHeartbeatData::AwsIamRole(AwsRemoteStackManagementHeartbeatData {
                status: RemoteStackManagementHeartbeatStatus {
                    health: ObservedHealth::Healthy,
                    lifecycle: ProviderLifecycleState::Running,
                    message: controller.role_name.as_ref().map(|role_name| {
                        format!("AWS management role '{}' is reachable", role_name)
                    }),
                    stale: false,
                    partial: false,
                    collection_issues: vec![],
                },
                role_name: controller.role_name.clone(),
                role_arn: controller.role_arn.clone(),
                management_permissions_applied: controller.management_permissions_applied,
            }),
        ),
        raw: vec![],
    });

    Ok(())
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

    /// Generate managed IAM policy documents for management permissions from the stack's management profile.
    fn generate_management_policy_documents(
        &self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<Vec<String>> {
        // Get the management permission profile from the stack
        // The stack processor should have processed the management permissions
        let management_permissions = ctx.desired_stack.management();
        let management_profile = management_permissions.profile()
            .ok_or_else(|| AlienError::new(ErrorData::InfrastructureError {
                message: "Management permissions not configured or set to Auto. Management permissions must be explicitly configured for remote stack management.".to_string(),
                operation: Some("generate_management_policy_document".to_string()),
                resource_id: Some("management".to_string()),
            }))?;

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

        // Include all permission sets from the management profile in the stack-level
        // management policies. Generate each permission set independently so IAM
        // statement effects and conditions remain intact.
        let generator = AwsRuntimePermissionsGenerator::new();
        let mut all_statements = Vec::new();
        if let Some(global_permission_set_ids) = management_profile.0.get("*") {
            for permission_set_ref in global_permission_set_ids {
                let permission_set =
                    permission_set_ref.resolve(|name| get_permission_set(name).cloned());
                let Some(permission_set) = permission_set else {
                    continue;
                };
                if permission_set.platforms.aws.is_none() {
                    continue;
                }

                let policy = generator
                    .generate_policy(&permission_set, BindingTarget::Stack, &permission_context)
                    .context(ErrorData::InfrastructureError {
                        message: format!(
                            "Failed to generate IAM policy for management permission set '{}'",
                            permission_set.id
                        ),
                        operation: Some("generate_management_policy_document".to_string()),
                        resource_id: Some("management".to_string()),
                    })?;
                all_statements.extend(policy.statement);
            }
        }

        self.append_resource_scoped_management_statements(
            ctx,
            management_profile,
            &permission_context,
            &generator,
            &mut all_statements,
        )?;

        // The management role always needs to read itself during heartbeat.
        if let Some(role_name) = &self.role_name {
            all_statements.push(AwsIamStatement {
                sid: "ReadOwnManagementRole".to_string(),
                effect: "Allow".to_string(),
                action: vec!["iam:GetRole".to_string()],
                resource: vec![format!(
                    "arn:aws:iam::{}:role/{}",
                    aws_config.account_id, role_name
                )],
                condition: None,
            });
        }

        if all_statements.is_empty() {
            return Ok(Vec::new());
        }

        self.chunk_management_policy_documents(all_statements)
    }

    fn append_resource_scoped_management_statements(
        &self,
        ctx: &ResourceControllerContext<'_>,
        management_profile: &alien_core::permissions::PermissionProfile,
        base_permission_context: &PermissionContext,
        generator: &AwsRuntimePermissionsGenerator,
        all_statements: &mut Vec<AwsIamStatement>,
    ) -> Result<()> {
        let mut seen = HashSet::new();
        for (resource_id, permission_set_refs) in management_profile
            .0
            .iter()
            .filter(|(scope, _)| *scope != "*")
        {
            let Some(resource_entry) = ctx.desired_stack.resources.get(resource_id) else {
                continue;
            };
            if resource_entry.lifecycle != ResourceLifecycle::Live {
                continue;
            }
            let permission_context = Self::resource_scoped_management_permission_context(
                ctx,
                base_permission_context,
                resource_id,
                resource_entry,
            )?;

            for permission_set_ref in permission_set_refs {
                if !seen.insert((resource_id.clone(), permission_set_ref.id().to_string())) {
                    continue;
                }
                if permission_set_ref.id().ends_with("/provision") {
                    continue;
                }
                let Some(permission_set) =
                    permission_set_ref.resolve(|name| get_permission_set(name).cloned())
                else {
                    continue;
                };
                if permission_set.platforms.aws.is_none() {
                    continue;
                }

                let policy = generator
                    .generate_policy(&permission_set, BindingTarget::Resource, &permission_context)
                    .context(ErrorData::InfrastructureError {
                        message: format!(
                            "Failed to generate resource-scoped IAM policy for management permission set '{}'",
                            permission_set.id
                        ),
                        operation: Some("generate_management_policy_document".to_string()),
                        resource_id: Some(resource_id.clone()),
                    })?;
                all_statements.extend(policy.statement);
            }
        }

        Ok(())
    }

    fn resource_scoped_management_permission_context(
        ctx: &ResourceControllerContext<'_>,
        base_permission_context: &PermissionContext,
        resource_id: &str,
        resource_entry: &alien_core::ResourceEntry,
    ) -> Result<PermissionContext> {
        if let Some(cluster) = resource_entry.config.downcast_ref::<KubernetesCluster>() {
            return ResourcePermissionsHelper::aws_kubernetes_cluster_permission_context(
                ctx, cluster,
            )
            .map(|context| context.with_resource_id(resource_id.to_string()));
        }

        let mut context = base_permission_context
            .clone()
            .with_resource_id(resource_id.to_string());
        context.resource_name = None;

        if resource_entry.config.downcast_ref::<Worker>().is_some() {
            return Ok(
                context.with_resource_name(format!("{}-{}", ctx.resource_prefix, resource_id))
            );
        }

        Ok(context)
    }

    fn chunk_management_policy_documents(
        &self,
        statements: Vec<AwsIamStatement>,
    ) -> Result<Vec<String>> {
        let mut chunks = Vec::new();
        let mut current = Vec::new();

        for statement in statements {
            let mut candidate = current.clone();
            candidate.push(statement.clone());
            if self.management_policy_document_size(&candidate)? <= MAX_MANAGED_POLICY_BYTES {
                current = candidate;
                continue;
            }

            if current.is_empty() {
                return Err(AlienError::new(ErrorData::InfrastructureError {
                    message: format!(
                        "AWS IAM statement '{}' is too large for a managed policy",
                        statement.sid
                    ),
                    operation: Some("chunk_management_policy_documents".to_string()),
                    resource_id: Some("management".to_string()),
                }));
            }

            chunks.push(self.serialize_management_policy_document(current)?);
            current = vec![statement];
        }

        if !current.is_empty() {
            chunks.push(self.serialize_management_policy_document(current)?);
        }

        Ok(chunks)
    }

    fn management_policy_document_size(&self, statements: &[AwsIamStatement]) -> Result<usize> {
        self.serialize_management_policy_document(statements.to_vec())
            .map(|policy| policy.len())
    }

    fn serialize_management_policy_document(
        &self,
        statements: Vec<AwsIamStatement>,
    ) -> Result<String> {
        let policy = AwsIamPolicy {
            version: "2012-10-17".to_string(),
            statement: statements,
        };

        serde_json::to_string_pretty(&policy)
            .into_alien_error()
            .context(ErrorData::InfrastructureError {
                message: "Failed to serialize management IAM policy document".to_string(),
                operation: Some("generate_management_policy_document".to_string()),
                resource_id: Some("management".to_string()),
            })
    }

    async fn apply_management_policy_documents(
        &self,
        ctx: &ResourceControllerContext<'_>,
        client: &dyn IamApi,
        role_name: &str,
        policy_documents: &[String],
    ) -> Result<()> {
        let desired_policy_arns = self
            .ensure_desired_management_policies(ctx, client, policy_documents)
            .await?;
        for policy_arn in &desired_policy_arns {
            client
                .attach_role_policy(role_name, policy_arn)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to attach management policy '{}' to IAM role '{}'",
                        policy_arn, role_name
                    ),
                    resource_id: Some("remote-stack-management".to_string()),
                })?;
        }

        self.reconcile_owned_management_policies(ctx, client, role_name, &desired_policy_arns)
            .await?;
        self.delete_legacy_inline_policy(client, role_name).await?;

        Ok(())
    }

    async fn ensure_desired_management_policies(
        &self,
        ctx: &ResourceControllerContext<'_>,
        client: &dyn IamApi,
        policy_documents: &[String],
    ) -> Result<Vec<String>> {
        let aws_config = ctx.get_aws_config()?;
        let mut policy_arns = Vec::new();

        for (idx, policy_document) in policy_documents.iter().enumerate() {
            let policy_name = self.management_policy_name(ctx.resource_prefix, idx);
            let policy_arn = self.management_policy_arn(&aws_config.account_id, &policy_name);

            match client
                .create_policy(&policy_name, policy_document, None)
                .await
            {
                Ok(response) => {
                    policy_arns.push(response.create_policy_result.policy.arn);
                }
                Err(e) if is_remote_conflict(&e) => {
                    self.create_default_policy_version(client, &policy_arn, policy_document)
                        .await?;
                    policy_arns.push(policy_arn);
                }
                Err(e) => {
                    return Err(e
                        .context(ErrorData::CloudPlatformError {
                            message: format!(
                                "Failed to create management IAM policy '{}'",
                                policy_name
                            ),
                            resource_id: Some("remote-stack-management".to_string()),
                        })
                        .into());
                }
            }
        }

        Ok(policy_arns)
    }

    async fn create_default_policy_version(
        &self,
        client: &dyn IamApi,
        policy_arn: &str,
        policy_document: &str,
    ) -> Result<()> {
        self.prune_policy_versions_for_new_default(client, policy_arn)
            .await?;
        client
            .create_policy_version(policy_arn, policy_document, true)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to create new default version for '{}'", policy_arn),
                resource_id: Some("remote-stack-management".to_string()),
            })?;
        Ok(())
    }

    async fn prune_policy_versions_for_new_default(
        &self,
        client: &dyn IamApi,
        policy_arn: &str,
    ) -> Result<()> {
        let versions = client.list_policy_versions(policy_arn).await.context(
            ErrorData::CloudPlatformError {
                message: format!("Failed to list policy versions for '{}'", policy_arn),
                resource_id: Some("remote-stack-management".to_string()),
            },
        )?;
        let versions = versions
            .list_policy_versions_result
            .versions
            .map(|versions| versions.member)
            .unwrap_or_default();

        if versions.len() < 5 {
            return Ok(());
        }

        let Some(version_to_delete) = versions
            .iter()
            .filter(|version| !version.is_default_version)
            .min_by_key(|version| version.create_date.as_deref().unwrap_or(""))
        else {
            return Ok(());
        };

        client
            .delete_policy_version(policy_arn, &version_to_delete.version_id)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to delete old policy version '{}' for '{}'",
                    version_to_delete.version_id, policy_arn
                ),
                resource_id: Some("remote-stack-management".to_string()),
            })?;
        Ok(())
    }

    async fn reconcile_owned_management_policies(
        &self,
        _ctx: &ResourceControllerContext<'_>,
        client: &dyn IamApi,
        role_name: &str,
        desired_policy_arns: &[String],
    ) -> Result<()> {
        let desired: HashSet<&str> = desired_policy_arns.iter().map(String::as_str).collect();
        let response = client
            .list_attached_role_policies(role_name)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to list attached policies for management role '{}'",
                    role_name
                ),
                resource_id: Some("remote-stack-management".to_string()),
            })?;
        let attached = response
            .list_attached_role_policies_result
            .attached_policies
            .map(|attached| attached.member)
            .unwrap_or_default();

        for policy in attached {
            if !self.is_owned_management_policy_name(&policy.policy_name)
                || desired.contains(policy.policy_arn.as_str())
            {
                continue;
            }

            client
                .detach_role_policy(role_name, &policy.policy_arn)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to detach stale management policy '{}' from role '{}'",
                        policy.policy_arn, role_name
                    ),
                    resource_id: Some("remote-stack-management".to_string()),
                })?;
            self.delete_owned_policy(client, &policy.policy_arn).await?;
        }

        Ok(())
    }

    async fn delete_legacy_inline_policy(
        &self,
        client: &dyn IamApi,
        role_name: &str,
    ) -> Result<()> {
        match client
            .delete_role_policy(role_name, LEGACY_INLINE_POLICY_NAME)
            .await
        {
            Ok(_) => Ok(()),
            Err(e) if is_remote_not_found(&e) => Ok(()),
            Err(e) => Err(e
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to delete legacy inline management policy from role '{}'",
                        role_name
                    ),
                    resource_id: Some("remote-stack-management".to_string()),
                })
                .into()),
        }
    }

    async fn delete_owned_policy(&self, client: &dyn IamApi, policy_arn: &str) -> Result<()> {
        let versions = match client.list_policy_versions(policy_arn).await {
            Ok(response) => response
                .list_policy_versions_result
                .versions
                .map(|versions| versions.member)
                .unwrap_or_default(),
            Err(e) if is_remote_not_found(&e) => return Ok(()),
            Err(e) => {
                return Err(e
                    .context(ErrorData::CloudPlatformError {
                        message: format!("Failed to list policy versions for '{}'", policy_arn),
                        resource_id: Some("remote-stack-management".to_string()),
                    })
                    .into());
            }
        };

        for version in versions {
            if version.is_default_version {
                continue;
            }
            match client
                .delete_policy_version(policy_arn, &version.version_id)
                .await
            {
                Ok(_) => {}
                Err(e) if is_remote_not_found(&e) => {}
                Err(e) => {
                    return Err(e
                        .context(ErrorData::CloudPlatformError {
                            message: format!(
                                "Failed to delete policy version '{}' for '{}'",
                                version.version_id, policy_arn
                            ),
                            resource_id: Some("remote-stack-management".to_string()),
                        })
                        .into());
                }
            }
        }

        match client.delete_policy(policy_arn).await {
            Ok(_) => Ok(()),
            Err(e) if is_remote_not_found(&e) => Ok(()),
            Err(e) => Err(e
                .context(ErrorData::CloudPlatformError {
                    message: format!("Failed to delete management policy '{}'", policy_arn),
                    resource_id: Some("remote-stack-management".to_string()),
                })
                .into()),
        }
    }

    fn management_policy_name(&self, resource_prefix: &str, idx: usize) -> String {
        let suffix = format!("-{idx}");
        let base =
            sanitize_iam_policy_name(&format!("{resource_prefix}-{MANAGED_POLICY_BASE_NAME}"));
        if base.len() + suffix.len() <= IAM_POLICY_NAME_MAX_LEN {
            return format!("{base}{suffix}");
        }

        let max_base_len = IAM_POLICY_NAME_MAX_LEN - suffix.len();
        format!("{}{}", &base[..max_base_len], suffix)
    }

    fn management_policy_arn(&self, account_id: &str, policy_name: &str) -> String {
        format!("arn:aws:iam::{account_id}:policy/{policy_name}")
    }

    fn is_owned_management_policy_name(&self, policy_name: &str) -> bool {
        policy_name.contains(MANAGED_POLICY_BASE_NAME)
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

fn sanitize_iam_policy_name(input: &str) -> String {
    input
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '_' | '+' | '=' | ',' | '.' | '@' | '-') {
                c
            } else {
                '-'
            }
        })
        .collect()
}

fn is_remote_conflict(error: &alien_error::AlienError<alien_client_core::ErrorData>) -> bool {
    matches!(
        error.error,
        Some(alien_client_core::ErrorData::RemoteResourceConflict { .. })
    )
}

fn is_remote_not_found(error: &alien_error::AlienError<alien_client_core::ErrorData>) -> bool {
    matches!(
        error.error,
        Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. })
    )
}
