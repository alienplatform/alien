use alien_error::{AlienError, Context, ContextError, IntoAlienError, IntoAlienErrorDirect};
use alien_macros::controller;
use std::fmt::Debug;
use std::time::Duration;
use tracing::{debug, info};

use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use alien_aws_clients::iam::CreateRoleRequest;
use alien_core::{ArtifactRegistry, ArtifactRegistryOutputs, ResourceOutputs, ResourceStatus};

/// AWS Artifact Registry controller.
///
/// AWS ECR implicitly exists in every AWS account and region, but this controller
/// creates two IAM roles to manage access: one for pull permissions and one for push+pull permissions.
#[controller]
pub struct AwsArtifactRegistryController {
    /// AWS account ID for generating the ECR registry URL
    pub(crate) account_id: Option<String>,
    /// The AWS region for this registry
    pub(crate) region: Option<String>,
    /// The ARN of the pull role
    pub(crate) pull_role_arn: Option<String>,
    /// The ARN of the push+pull role
    pub(crate) push_role_arn: Option<String>,
    /// The repository prefix (resource id)
    pub(crate) repository_prefix: Option<String>,
}

#[controller]
impl AwsArtifactRegistryController {
    // ─────────────── CREATE FLOW ──────────────────────────────
    #[flow_entry(Create)]
    #[handler(
        state = CreateStart,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let config = ctx.desired_resource_config::<ArtifactRegistry>()?;

        info!(
            registry_id = %config.id,
            region = %aws_cfg.region,
            "Setting up AWS ECR registry with roles"
        );

        let account_id = aws_cfg.account_id.to_string();

        // Store the repository prefix using resource_prefix-config.id pattern
        self.repository_prefix = Some(format!("{}-{}", ctx.resource_prefix, config.id));

        info!(
            registry_id = %config.id,
            account_id = %account_id,
            region = %aws_cfg.region,
            repository_prefix = %self.repository_prefix.as_deref().unwrap_or("unknown"),
            "AWS ECR registry is ready (implicitly exists)"
        );

        self.account_id = Some(account_id);
        self.region = Some(aws_cfg.region.clone());

        Ok(HandlerAction::Continue {
            state: CreatingPullRole,
            suggested_delay: None,
        })
    }

    #[handler(
        state = CreatingPullRole,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_pull_role(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let config = ctx.desired_resource_config::<ArtifactRegistry>()?;
        let iam_client = ctx.service_provider.get_aws_iam_client(aws_cfg).await?;

        let pull_role_name = format!("{}-{}-pull", ctx.resource_prefix, config.id);

        info!(
            role_name = %pull_role_name,
            "Creating pull role for artifact registry"
        );

        // Create assume role policy that allows service account roles to assume this role
        let assume_role_policy = self.generate_service_account_assume_role_policy(ctx)?;

        let request = CreateRoleRequest::builder()
            .role_name(pull_role_name.clone())
            .assume_role_policy_document(assume_role_policy)
            .description(format!("Alien ECR pull role for registry {}", config.id))
            .build();

        let response =
            iam_client
                .create_role(request)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!("Failed to create pull role '{}'", pull_role_name),
                    resource_id: Some(config.id.clone()),
                })?;

        self.pull_role_arn = Some(response.create_role_result.role.arn);

        info!(
            role_name = %pull_role_name,
            role_arn = %self.pull_role_arn.as_deref().unwrap_or("unknown"),
            "Pull role created successfully"
        );

        Ok(HandlerAction::Continue {
            state: CreatingPullRolePolicy,
            suggested_delay: None,
        })
    }

    #[handler(
        state = CreatingPullRolePolicy,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_pull_role_policy(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let config = ctx.desired_resource_config::<ArtifactRegistry>()?;
        let iam_client = ctx.service_provider.get_aws_iam_client(aws_cfg).await?;

        let pull_role_name = format!("{}-{}-pull", ctx.resource_prefix, config.id);
        let policy_name = "ECRPullPolicy";

        // Create policy document for ECR pull permissions
        let policy_document = self.generate_ecr_pull_policy(ctx, &config.id)?;

        iam_client
            .put_role_policy(&pull_role_name, policy_name, &policy_document)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to attach pull policy to role '{}'", pull_role_name),
                resource_id: Some(config.id.clone()),
            })?;

        info!(
            role_name = %pull_role_name,
            "Pull role policy attached successfully"
        );

        Ok(HandlerAction::Continue {
            state: CreatingPushRole,
            suggested_delay: None,
        })
    }

    #[handler(
        state = CreatingPushRole,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_push_role(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let config = ctx.desired_resource_config::<ArtifactRegistry>()?;
        let iam_client = ctx.service_provider.get_aws_iam_client(aws_cfg).await?;

        let push_role_name = format!("{}-{}-push", ctx.resource_prefix, config.id);

        info!(
            role_name = %push_role_name,
            "Creating push role for artifact registry"
        );

        // Create assume role policy that allows service account roles to assume this role
        let assume_role_policy = self.generate_service_account_assume_role_policy(ctx)?;

        let request = CreateRoleRequest::builder()
            .role_name(push_role_name.clone())
            .assume_role_policy_document(assume_role_policy)
            .description(format!("Alien ECR push role for registry {}", config.id))
            .build();

        let response =
            iam_client
                .create_role(request)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!("Failed to create push role '{}'", push_role_name),
                    resource_id: Some(config.id.clone()),
                })?;

        self.push_role_arn = Some(response.create_role_result.role.arn);

        info!(
            role_name = %push_role_name,
            role_arn = %self.push_role_arn.as_deref().unwrap_or("unknown"),
            "Push role created successfully"
        );

        Ok(HandlerAction::Continue {
            state: CreatingPushRolePolicy,
            suggested_delay: None,
        })
    }

    #[handler(
        state = CreatingPushRolePolicy,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_push_role_policy(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let config = ctx.desired_resource_config::<ArtifactRegistry>()?;
        let iam_client = ctx.service_provider.get_aws_iam_client(aws_cfg).await?;

        let push_role_name = format!("{}-{}-push", ctx.resource_prefix, config.id);
        let policy_name = "ECRPushPolicy";

        // Create policy document for ECR push+pull permissions
        let policy_document = self.generate_ecr_push_policy(ctx, &config.id)?;

        iam_client
            .put_role_policy(&push_role_name, policy_name, &policy_document)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to attach push policy to role '{}'", push_role_name),
                resource_id: Some(config.id.clone()),
            })?;

        info!(
            role_name = %push_role_name,
            "Push role policy attached successfully"
        );

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
        let config = ctx.desired_resource_config::<ArtifactRegistry>()?;

        info!(registry=%config.id, "Applying resource-scoped permissions for ECR repository");

        // Apply resource-scoped permissions from the stack using the centralized helper.
        // This handles wildcard ("*") permissions and management SA permissions.
        {
            use crate::core::ResourcePermissionsHelper;
            ResourcePermissionsHelper::apply_aws_resource_scoped_permissions(
                ctx,
                &config.id,
                &config.id,
                "artifact-registry",
            )
            .await?;
        }

        info!(registry=%config.id, "Successfully applied resource-scoped permissions");

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
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
        let config = ctx.desired_resource_config::<ArtifactRegistry>()?;

        info!(
            registry_id = %config.id,
            "Starting AWS ECR registry update (checking if policies need updating)"
        );

        // Always go through the policy update flow for consistency
        // Even if no updates are needed, we verify the policies are correct
        Ok(HandlerAction::Continue {
            state: UpdatingPullRoleTrustPolicy,
            suggested_delay: None,
        })
    }

    #[handler(
        state = UpdatingPullRoleTrustPolicy,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn updating_pull_role_trust_policy(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let config = ctx.desired_resource_config::<ArtifactRegistry>()?;
        let iam_client = ctx.service_provider.get_aws_iam_client(aws_cfg).await?;

        let pull_role_name = format!("{}-{}-pull", ctx.resource_prefix, config.id);

        // Generate updated trust policy (allow service account roles)
        let assume_role_policy = self.generate_service_account_assume_role_policy(ctx)?;

        iam_client
            .update_assume_role_policy(&pull_role_name, &assume_role_policy)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to update pull role trust policy for '{}'",
                    pull_role_name
                ),
                resource_id: Some(config.id.clone()),
            })?;

        info!(
            role_name = %pull_role_name,
            "Pull role trust policy updated successfully"
        );

        Ok(HandlerAction::Continue {
            state: UpdatingPullRolePolicy,
            suggested_delay: None,
        })
    }

    #[handler(
        state = UpdatingPullRolePolicy,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn updating_pull_role_policy(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let config = ctx.desired_resource_config::<ArtifactRegistry>()?;
        let iam_client = ctx.service_provider.get_aws_iam_client(aws_cfg).await?;

        let pull_role_name = format!("{}-{}-pull", ctx.resource_prefix, config.id);
        let policy_name = "ECRPullPolicy";

        // Always update the policy to ensure it's correct
        let policy_document = self.generate_ecr_pull_policy(ctx, &config.id)?;

        iam_client
            .put_role_policy(&pull_role_name, policy_name, &policy_document)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to update pull role policy for '{}'", pull_role_name),
                resource_id: Some(config.id.clone()),
            })?;

        info!(
            role_name = %pull_role_name,
            "Pull role policy updated successfully"
        );

        Ok(HandlerAction::Continue {
            state: UpdatingPushRoleTrustPolicy,
            suggested_delay: None,
        })
    }

    #[handler(
        state = UpdatingPushRoleTrustPolicy,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn updating_push_role_trust_policy(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let config = ctx.desired_resource_config::<ArtifactRegistry>()?;
        let iam_client = ctx.service_provider.get_aws_iam_client(aws_cfg).await?;

        let push_role_name = format!("{}-{}-push", ctx.resource_prefix, config.id);

        // Generate updated trust policy (allow service account roles)
        let assume_role_policy = self.generate_service_account_assume_role_policy(ctx)?;

        iam_client
            .update_assume_role_policy(&push_role_name, &assume_role_policy)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to update push role trust policy for '{}'",
                    push_role_name
                ),
                resource_id: Some(config.id.clone()),
            })?;

        info!(
            role_name = %push_role_name,
            "Push role trust policy updated successfully"
        );

        Ok(HandlerAction::Continue {
            state: UpdatingPushRolePolicy,
            suggested_delay: None,
        })
    }

    #[handler(
        state = UpdatingPushRolePolicy,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn updating_push_role_policy(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let config = ctx.desired_resource_config::<ArtifactRegistry>()?;
        let iam_client = ctx.service_provider.get_aws_iam_client(aws_cfg).await?;

        let push_role_name = format!("{}-{}-push", ctx.resource_prefix, config.id);
        let policy_name = "ECRPushPolicy";

        // Always update the policy to ensure it's correct
        let policy_document = self.generate_ecr_push_policy(ctx, &config.id)?;

        iam_client
            .put_role_policy(&push_role_name, policy_name, &policy_document)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to update push role policy for '{}'", push_role_name),
                resource_id: Some(config.id.clone()),
            })?;

        info!(
            role_name = %push_role_name,
            "Push role policy updated successfully"
        );

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
        let config = ctx.desired_resource_config::<ArtifactRegistry>()?;

        info!(
            registry_id = %config.id,
            "Deleting AWS ECR registry roles"
        );

        Ok(HandlerAction::Continue {
            state: DeletingPullRolePolicy,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeletingPullRolePolicy,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_pull_role_policy(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let config = ctx.desired_resource_config::<ArtifactRegistry>()?;
        let iam_client = ctx.service_provider.get_aws_iam_client(aws_cfg).await?;

        let pull_role_name = format!("{}-{}-pull", ctx.resource_prefix, config.id);
        let policy_name = "ECRPullPolicy";

        // Delete pull role policy - treat NotFound as success for idempotent deletion
        match iam_client
            .delete_role_policy(&pull_role_name, policy_name)
            .await
        {
            Ok(_) => {
                info!(role_name = %pull_role_name, "Pull role policy deleted successfully");
            }
            Err(e)
                if matches!(
                    e.error,
                    Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                info!(role_name = %pull_role_name, "Pull role policy already deleted");
            }
            Err(e) => {
                return Err(e.into_alien_error().context(ErrorData::CloudPlatformError {
                    message: format!("Failed to delete pull role policy '{}'", pull_role_name),
                    resource_id: Some(config.id.clone()),
                }));
            }
        }

        Ok(HandlerAction::Continue {
            state: DeletingPullRole,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeletingPullRole,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_pull_role(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let config = ctx.desired_resource_config::<ArtifactRegistry>()?;
        let iam_client = ctx.service_provider.get_aws_iam_client(aws_cfg).await?;

        let pull_role_name = format!("{}-{}-pull", ctx.resource_prefix, config.id);

        // Delete pull role - treat NotFound as success for idempotent deletion
        match iam_client.delete_role(&pull_role_name).await {
            Ok(_) => {
                info!(role_name = %pull_role_name, "Pull role deleted successfully");
            }
            Err(e)
                if matches!(
                    e.error,
                    Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                info!(role_name = %pull_role_name, "Pull role already deleted");
            }
            Err(e) => {
                return Err(e.into_alien_error().context(ErrorData::CloudPlatformError {
                    message: format!("Failed to delete pull role '{}'", pull_role_name),
                    resource_id: Some(config.id.clone()),
                }));
            }
        }

        self.pull_role_arn = None;

        Ok(HandlerAction::Continue {
            state: DeletingPushRolePolicy,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeletingPushRolePolicy,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_push_role_policy(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let config = ctx.desired_resource_config::<ArtifactRegistry>()?;
        let iam_client = ctx.service_provider.get_aws_iam_client(aws_cfg).await?;

        let push_role_name = format!("{}-{}-push", ctx.resource_prefix, config.id);
        let policy_name = "ECRPushPolicy";

        // Delete push role policy - treat NotFound as success for idempotent deletion
        match iam_client
            .delete_role_policy(&push_role_name, policy_name)
            .await
        {
            Ok(_) => {
                info!(role_name = %push_role_name, "Push role policy deleted successfully");
            }
            Err(e)
                if matches!(
                    e.error,
                    Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                info!(role_name = %push_role_name, "Push role policy already deleted");
            }
            Err(e) => {
                return Err(e.into_alien_error().context(ErrorData::CloudPlatformError {
                    message: format!("Failed to delete push role policy '{}'", push_role_name),
                    resource_id: Some(config.id.clone()),
                }));
            }
        }

        Ok(HandlerAction::Continue {
            state: DeletingPushRole,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeletingPushRole,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_push_role(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let config = ctx.desired_resource_config::<ArtifactRegistry>()?;
        let iam_client = ctx.service_provider.get_aws_iam_client(aws_cfg).await?;

        let push_role_name = format!("{}-{}-push", ctx.resource_prefix, config.id);

        // Delete push role - treat NotFound as success for idempotent deletion
        match iam_client.delete_role(&push_role_name).await {
            Ok(_) => {
                info!(role_name = %push_role_name, "Push role deleted successfully");
            }
            Err(e)
                if matches!(
                    e.error,
                    Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                info!(role_name = %push_role_name, "Push role already deleted");
            }
            Err(e) => {
                return Err(e.into_alien_error().context(ErrorData::CloudPlatformError {
                    message: format!("Failed to delete push role '{}'", push_role_name),
                    resource_id: Some(config.id.clone()),
                }));
            }
        }

        self.push_role_arn = None;

        Ok(HandlerAction::Continue {
            state: Deleted,
            suggested_delay: None,
        })
    }

    // ─────────────── READY STATE ──────────────────────────────
    #[handler(
        state = Ready,
        on_failure = RefreshFailed,
        status = ResourceStatus::Running,
    )]
    async fn ready(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let config = ctx.desired_resource_config::<ArtifactRegistry>()?;

        // Heartbeat check: verify stored account/region haven't drifted
        if let (Some(stored_account_id), Some(stored_region)) = (&self.account_id, &self.region) {
            // Check for configuration drift
            if stored_account_id != &aws_cfg.account_id.to_string() {
                return Err(AlienError::new(ErrorData::ResourceDrift {
                    resource_id: config.id.clone(),
                    message: format!(
                        "AWS account ID changed from {} to {}",
                        stored_account_id, aws_cfg.account_id
                    ),
                }));
            }

            if stored_region != &aws_cfg.region {
                return Err(AlienError::new(ErrorData::ResourceDrift {
                    resource_id: config.id.clone(),
                    message: format!(
                        "AWS region changed from {} to {}",
                        stored_region, aws_cfg.region
                    ),
                }));
            }

            debug!(account_id=%stored_account_id, region=%stored_region, "AWS ECR registry heartbeat check passed");
        }

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: Some(Duration::from_secs(30)),
        })
    }

    // ─────────────── TERMINAL STATES ──────────────────────────
    terminal_state!(
        state = CreateFailed,
        status = ResourceStatus::ProvisionFailed
    );
    terminal_state!(state = UpdateFailed, status = ResourceStatus::UpdateFailed);
    terminal_state!(state = DeleteFailed, status = ResourceStatus::DeleteFailed);
    terminal_state!(
        state = RefreshFailed,
        status = ResourceStatus::RefreshFailed
    );
    terminal_state!(state = Deleted, status = ResourceStatus::Deleted);

    fn build_outputs(&self) -> Option<ResourceOutputs> {
        if let (Some(account_id), Some(region)) = (&self.account_id, &self.region) {
            let registry_endpoint = format!("{}.dkr.ecr.{}.amazonaws.com", account_id, region);
            let registry_id = format!("{}:{}", account_id, region);
            Some(ResourceOutputs::new(ArtifactRegistryOutputs {
                registry_id,
                registry_endpoint,
                pull_role: self.pull_role_arn.clone(),
                push_role: self.push_role_arn.clone(),
            }))
        } else {
            None
        }
    }

    fn get_binding_params(&self) -> Result<Option<serde_json::Value>> {
        use alien_core::bindings::ArtifactRegistryBinding;

        if let Some(repository_prefix) = &self.repository_prefix {
            let binding = ArtifactRegistryBinding::ecr(
                repository_prefix.clone(),
                self.pull_role_arn.clone(),
                self.push_role_arn.clone(),
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

impl AwsArtifactRegistryController {
    /// Generates an assume role policy that allows service account roles in the stack to assume this role
    /// In the new permission system, service accounts are created from permission profiles,
    /// and the artifact registry roles can be assumed by any service account role in the stack
    fn generate_service_account_assume_role_policy(
        &self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<String> {
        let aws_cfg = ctx.get_aws_config()?;
        let mut service_account_role_arns = Vec::new();

        // Find all service account roles in the stack that could assume this role
        for (resource_id, resource_entry) in ctx.desired_stack.resources() {
            if let Some(_service_account) = resource_entry
                .config
                .downcast_ref::<alien_core::ServiceAccount>()
            {
                // Generate the role ARN for this service account
                let role_name = format!("{}-{}", ctx.resource_prefix, resource_id);
                let role_arn = format!("arn:aws:iam::{}:role/{}", aws_cfg.account_id, role_name);
                service_account_role_arns.push(role_arn);
            }
        }

        // If no service accounts found, create a minimal policy (shouldn't happen in practice)
        if service_account_role_arns.is_empty() {
            let policy = serde_json::json!({
                "Version": "2012-10-17",
                "Statement": [
                    {
                        "Effect": "Deny",
                        "Principal": "*",
                        "Action": "*"
                    }
                ]
            });
            return Ok(policy.to_string());
        }

        // Create the principal object with service account role ARNs
        let principal = if service_account_role_arns.len() == 1 {
            serde_json::json!({
                "AWS": service_account_role_arns[0]
            })
        } else {
            serde_json::json!({
                "AWS": service_account_role_arns
            })
        };

        let policy = serde_json::json!({
            "Version": "2012-10-17",
            "Statement": [
                {
                    "Effect": "Allow",
                    "Principal": principal,
                    "Action": "sts:AssumeRole"
                }
            ]
        });

        Ok(policy.to_string())
    }

    /// Generates ECR pull policy for the pull role
    fn generate_ecr_pull_policy(
        &self,
        ctx: &ResourceControllerContext<'_>,
        registry_id: &str,
    ) -> Result<String> {
        let aws_cfg = ctx.get_aws_config()?;

        let policy = serde_json::json!({
            "Version": "2012-10-17",
            "Statement": [
                {
                    "Effect": "Allow",
                    "Action": [
                        "ecr:GetAuthorizationToken"
                    ],
                    "Resource": "*"
                },
                {
                    "Effect": "Allow",
                    "Action": [
                        "ecr:BatchCheckLayerAvailability",
                        "ecr:GetDownloadUrlForLayer",
                        "ecr:BatchGetImage",
                        "ecr:DescribeRepositories",
                        "ecr:DescribeImages",
                        "ecr:ListImages"
                    ],
                    "Resource": format!("arn:aws:ecr:{}:{}:repository/{}-{}-*", aws_cfg.region, aws_cfg.account_id, ctx.resource_prefix, registry_id)
                }
            ]
        });

        Ok(policy.to_string())
    }

    /// Generates ECR push+pull policy for the push role
    fn generate_ecr_push_policy(
        &self,
        ctx: &ResourceControllerContext<'_>,
        registry_id: &str,
    ) -> Result<String> {
        let aws_cfg = ctx.get_aws_config()?;

        let policy = serde_json::json!({
            "Version": "2012-10-17",
            "Statement": [
                {
                    "Effect": "Allow",
                    "Action": [
                        "ecr:GetAuthorizationToken"
                    ],
                    "Resource": "*"
                },
                {
                    "Effect": "Allow",
                    "Action": [
                        "ecr:BatchCheckLayerAvailability",
                        "ecr:GetDownloadUrlForLayer",
                        "ecr:BatchGetImage",
                        "ecr:CompleteLayerUpload",
                        "ecr:UploadLayerPart",
                        "ecr:InitiateLayerUpload",
                        "ecr:PutImage",
                        "ecr:DescribeRepositories",
                        "ecr:DescribeImages",
                        "ecr:ListImages"
                    ],
                    "Resource": format!("arn:aws:ecr:{}:{}:repository/{}-{}-*", aws_cfg.region, aws_cfg.account_id, ctx.resource_prefix, registry_id)
                }
            ]
        });

        Ok(policy.to_string())
    }

    /// Create a mock controller for testing
    #[cfg(test)]
    pub fn mock_ready(account_id: &str, region: &str) -> Self {
        Self {
            state: AwsArtifactRegistryState::Ready,
            account_id: Some(account_id.to_string()),
            region: Some(region.to_string()),
            pull_role_arn: Some(format!("arn:aws:iam::{}:role/test-pull-role", account_id)),
            push_role_arn: Some(format!("arn:aws:iam::{}:role/test-push-role", account_id)),
            repository_prefix: Some("test-artifact-registry".to_string()),
            _internal_stay_count: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::controller_test::SingleControllerExecutor;
    use crate::MockPlatformServiceProvider;
    use alien_aws_clients::iam::{CreateRoleResponse, CreateRoleResult, MockIamApi, Role};
    use alien_core::Platform;
    use std::sync::Arc;

    fn basic_artifact_registry() -> ArtifactRegistry {
        ArtifactRegistry::new("my-registry".to_string()).build()
    }

    fn create_successful_role_response(role_name: &str) -> CreateRoleResponse {
        CreateRoleResponse {
            create_role_result: CreateRoleResult {
                role: Role {
                    path: "/".to_string(),
                    role_name: role_name.to_string(),
                    role_id: "AROAEXAMPLE123".to_string(),
                    arn: format!("arn:aws:iam::123456789012:role/{}", role_name),
                    create_date: "2023-01-01T00:00:00Z".to_string(),
                    assume_role_policy_document: None,
                    description: None,
                    max_session_duration: None,
                    permissions_boundary: None,
                    tags: None,
                    role_last_used: None,
                },
            },
        }
    }

    fn setup_mock_client_for_creation_and_deletion() -> Arc<MockIamApi> {
        let mut mock_iam = MockIamApi::new();

        // Mock successful pull role creation
        mock_iam
            .expect_create_role()
            .returning(|request| Ok(create_successful_role_response(&request.role_name)));

        // Mock successful policy attachment
        mock_iam
            .expect_put_role_policy()
            .returning(|_, _, _| Ok(()));

        // Mock successful policy deletion (for both roles)
        mock_iam
            .expect_delete_role_policy()
            .returning(|_, _| Ok(()));

        // Mock successful role deletion (for both roles)
        mock_iam.expect_delete_role().returning(|_| Ok(()));

        Arc::new(mock_iam)
    }

    fn setup_mock_client_for_creation_and_update() -> Arc<MockIamApi> {
        let mut mock_iam = MockIamApi::new();

        // Mock successful pull role creation
        mock_iam
            .expect_create_role()
            .returning(|request| Ok(create_successful_role_response(&request.role_name)));

        // Mock successful policy attachment (for both create and update)
        mock_iam
            .expect_put_role_policy()
            .returning(|_, _, _| Ok(()));

        // Mock successful trust policy update (for updates)
        mock_iam
            .expect_update_assume_role_policy()
            .returning(|_, _| Ok(()));

        Arc::new(mock_iam)
    }

    fn setup_mock_service_provider(mock_iam: Arc<MockIamApi>) -> Arc<MockPlatformServiceProvider> {
        let mut mock_provider = MockPlatformServiceProvider::new();

        mock_provider
            .expect_get_aws_iam_client()
            .returning(move |_| Ok(mock_iam.clone()));

        Arc::new(mock_provider)
    }

    #[tokio::test]
    async fn test_create_and_delete_flow_succeeds() {
        let registry = basic_artifact_registry();
        // Use the same values as AwsClientConfig::mock()
        let account_id = "123456789012";
        let region = "us-east-1";

        let mock_iam = setup_mock_client_for_creation_and_deletion();
        let mock_provider = setup_mock_service_provider(mock_iam);

        let mut executor = SingleControllerExecutor::builder()
            .resource(registry)
            .controller(AwsArtifactRegistryController::default())
            .platform(Platform::Aws)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        // Test create flow
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);

        // Verify outputs
        let outputs = executor.outputs().unwrap();
        let registry_outputs = outputs.downcast_ref::<ArtifactRegistryOutputs>().unwrap();

        assert_eq!(
            registry_outputs.registry_id,
            format!("{}:{}", account_id, region)
        );
        assert_eq!(
            registry_outputs.registry_endpoint,
            format!("{}.dkr.ecr.{}.amazonaws.com", account_id, region)
        );
        assert!(registry_outputs.pull_role.is_some());
        assert!(registry_outputs.push_role.is_some());

        // Test delete flow
        executor.delete().unwrap();
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Deleted);
    }

    #[tokio::test]
    async fn test_update_flow_succeeds() {
        let registry = basic_artifact_registry();
        // Use the same values as AwsClientConfig::mock()
        let _account_id = "123456789012";
        let _region = "us-east-1";

        let mock_iam = setup_mock_client_for_creation_and_update();
        let mock_provider = setup_mock_service_provider(mock_iam);

        let mut executor = SingleControllerExecutor::builder()
            .resource(registry.clone())
            .controller(AwsArtifactRegistryController::default())
            .platform(Platform::Aws)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        // Initial creation
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);

        // Test update flow (should be no-op)
        executor.update(registry).unwrap();
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);
    }
}
